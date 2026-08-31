// Port of `~/experiments/Server/webclient/src/graphics/Pix2D.ts`. The TS
// statics (pixels / width / height / clipping) become a struct borrowing the
// framebuffer, so a `Pix2D` is the transient draw target TS binds with
// `setPixels`. Blend expressions keep TS's int32 wrap semantics: intermediate
// products exceed i32, and `& 0xff00ff00` + `>> 8` only observe the low bits,
// so wrapping ops reproduce the TS double arithmetic exactly.

use std::cell::Cell;

// The active coverage target (the GPU overlay pass): a raw pointer into a
// per-pixel `u8` mark buffer plus its dimensions, set by `coverage_guard`.
// `Pix2D::with_pixels` attaches it to a matching-size surface so the pixel
// writes mark coverage. Zero when the CPU path draws (the `coverage` field
// then stays `None` and the CPU oracle is byte-identical).
thread_local! {
    static COVERAGE: Cell<(usize, u32, u32)> = const { Cell::new((0, 0, 0)) };
}

/// A guard that makes every `Pix2D::with_pixels` on this thread attach
/// `buffer` as its coverage target until the guard drops — the same shape
/// as the deleted `GpuChromeGuard` (a raw pointer in a thread-local, owned
/// by the guard, cleared on drop including unwind). Single-threaded render
/// loop.
pub struct CoverageGuard;

/// Activate `buffer` (one byte per pixel, `width*height` bytes) as the
/// coverage target for matching `Pix2D` surfaces on this thread.
pub fn coverage_guard(buffer: &mut [u8], width: u32, height: u32) -> CoverageGuard {
    COVERAGE.set((buffer.as_mut_ptr() as usize, width, height));
    CoverageGuard
}

impl Drop for CoverageGuard {
    fn drop(&mut self) {
        COVERAGE.set((0, 0, 0));
    }
}

pub struct Pix2D<'a> {
    pub pixels: &'a mut [i32],
    pub width: i32,
    pub height: i32,
    pub clip_min_x: i32,
    pub clip_min_y: i32,
    pub clip_max_x: i32,
    pub clip_max_y: i32,
    pub size_x: i32,
    pub max_x: i32,
    pub max_y: i32,
    /// Coverage marks (the GPU overlay pass only): a byte per pixel, set
    /// when a draw writes that pixel. `None` on the CPU path.
    pub(crate) coverage: Option<&'a mut [u8]>,
}

impl<'a> Pix2D<'a> {
    /// TS `setPixels(pixels, width, height)` followed by the default full
    /// clipping: bind a framebuffer as the active draw target. A surface
    /// sized like the active coverage target (the GPU overlay pass binds
    /// `area_game`) attaches it, so its pixel writes mark coverage.
    pub fn with_pixels(pixels: &'a mut [i32], width: i32, height: i32) -> Self {
        let mut s = Pix2D {
            pixels,
            width,
            height,
            clip_min_x: 0,
            clip_min_y: 0,
            clip_max_x: 0,
            clip_max_y: 0,
            size_x: 0,
            max_x: 0,
            max_y: 0,
            coverage: None,
        };
        let (ptr, cw, ch) = COVERAGE.get();
        if ptr != 0 && cw == width as u32 && ch == height as u32 {
            // SAFETY: `CoverageGuard` owns the scope; the pointer is
            // cleared on drop (normal or unwinding). Single-threaded
            // render loop, and the buffer outlives this surface.
            s.coverage =
                Some(unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, (cw * ch) as usize) });
        }
        s.set_clipping(0, 0, width, height);
        s
    }

    pub fn reset_clipping(&mut self) {
        self.clip_min_x = 0;
        self.clip_min_y = 0;
        self.clip_max_x = self.width;
        self.clip_max_y = self.height;
        self.size_x = self.clip_max_x - 1;
        self.max_x = self.clip_max_x / 2;
    }

    pub fn set_clipping(&mut self, mut x1: i32, mut y1: i32, mut x2: i32, mut y2: i32) {
        if x1 < 0 {
            x1 = 0;
        }
        if y1 < 0 {
            y1 = 0;
        }
        if x2 > self.width {
            x2 = self.width;
        }
        if y2 > self.height {
            y2 = self.height;
        }
        self.clip_min_y = y1;
        self.clip_max_y = y2;
        self.clip_min_x = x1;
        self.clip_max_x = x2;
        self.size_x = self.clip_max_x - 1;
        self.max_x = self.clip_max_x / 2;
        self.max_y = self.clip_max_y / 2;
    }

    pub fn cls(&mut self) {
        self.pixels.fill(0);
    }

    pub fn fill_rect_trans(
        &mut self,
        mut x: i32,
        mut y: i32,
        mut width: i32,
        mut height: i32,
        rgb: i32,
        alpha: i32,
    ) {
        if x < self.clip_min_x {
            width -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if y < self.clip_min_y {
            height -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if x + width > self.clip_max_x {
            width = self.clip_max_x - x;
        }
        if y + height > self.clip_max_y {
            height = self.clip_max_y - y;
        }
        let inv_alpha = 256 - alpha;
        let r0 = ((rgb >> 16) & 0xff) * alpha;
        let g0 = ((rgb >> 8) & 0xff) * alpha;
        let b0 = (rgb & 0xff) * alpha;
        let step = self.width - width;
        let mut offset = x + y * self.width;
        for _ in 0..height {
            for _ in 0..width {
                let r1 = ((self.pixels[offset as usize] >> 16) & 0xff) * inv_alpha;
                let g1 = ((self.pixels[offset as usize] >> 8) & 0xff) * inv_alpha;
                let b1 = (self.pixels[offset as usize] & 0xff) * inv_alpha;
                self.pixels[offset as usize] =
                    (((r0 + r1) >> 8) << 16) + (((g0 + g1) >> 8) << 8) + ((b0 + b1) >> 8);
                offset += 1;
            }
            offset += step;
        }
        self.mark_rect(x, y, width, height);
    }

    pub fn fill_rect(&mut self, mut x: i32, mut y: i32, mut width: i32, mut height: i32, rgb: i32) {
        if x < self.clip_min_x {
            width -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if y < self.clip_min_y {
            height -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if x + width > self.clip_max_x {
            width = self.clip_max_x - x;
        }
        if y + height > self.clip_max_y {
            height = self.clip_max_y - y;
        }
        let step = self.width - width;
        let mut offset = x + y * self.width;
        for _ in 0..height {
            for _ in 0..width {
                self.pixels[offset as usize] = rgb;
                offset += 1;
            }
            offset += step;
        }
        self.mark_rect(x, y, width, height);
    }

    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: i32) {
        self.hline(x, y, w, rgb);
        self.hline(x, y + h - 1, w, rgb);
        self.vline(x, y, h, rgb);
        self.vline(x + w - 1, y, h, rgb);
    }

    pub fn draw_rect_trans(&mut self, x: i32, y: i32, w: i32, h: i32, rgb: i32, alpha: i32) {
        self.hline_trans(x, y, w, rgb, alpha);
        self.hline_trans(x, y + h - 1, w, rgb, alpha);
        if h >= 3 {
            self.vline_trans(x, y, h, rgb, alpha);
            self.vline_trans(x + w - 1, y, h, rgb, alpha);
        }
    }

    pub fn hline(&mut self, mut x: i32, y: i32, mut width: i32, rgb: i32) {
        if y < self.clip_min_y || y >= self.clip_max_y {
            return;
        }
        if x < self.clip_min_x {
            width -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if x + width > self.clip_max_x {
            width = self.clip_max_x - x;
        }
        let off = x + y * self.width;
        for i in 0..width {
            self.pixels[(off + i) as usize] = rgb;
        }
        self.mark_row(off, width);
    }

    pub fn hline_trans(&mut self, mut x: i32, y: i32, mut width: i32, rgb: i32, alpha: i32) {
        if y < self.clip_min_y || y >= self.clip_max_y {
            return;
        }
        if x < self.clip_min_x {
            width -= self.clip_min_x - x;
            x = self.clip_min_x;
        }
        if x + width > self.clip_max_x {
            width = self.clip_max_x - x;
        }
        let inv_alpha = 256 - alpha;
        let r0 = ((rgb >> 16) & 0xff) * alpha;
        let g0 = ((rgb >> 8) & 0xff) * alpha;
        let b0 = (rgb & 0xff) * alpha;
        let start = x + y * self.width;
        for offset in start..start + width {
            let r1 = ((self.pixels[offset as usize] >> 16) & 0xff) * inv_alpha;
            let g1 = ((self.pixels[offset as usize] >> 8) & 0xff) * inv_alpha;
            let b1 = (self.pixels[offset as usize] & 0xff) * inv_alpha;
            self.pixels[offset as usize] =
                (((r0 + r1) >> 8) << 16) + (((g0 + g1) >> 8) << 8) + ((b0 + b1) >> 8);
        }
        self.mark_row(x + y * self.width, width);
    }

    pub fn vline(&mut self, x: i32, mut y: i32, mut height: i32, rgb: i32) {
        if x < self.clip_min_x || x >= self.clip_max_x {
            return;
        }
        if y < self.clip_min_y {
            height -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if y + height > self.clip_max_y {
            height = self.clip_max_y - y;
        }
        let off = x + y * self.width;
        for i in 0..height {
            self.pixels[(off + i * self.width) as usize] = rgb;
        }
        self.mark_col(off, height);
    }

    pub fn vline_trans(&mut self, x: i32, mut y: i32, mut height: i32, rgb: i32, alpha: i32) {
        if x < self.clip_min_x || x >= self.clip_max_x {
            return;
        }
        if y < self.clip_min_y {
            height -= self.clip_min_y - y;
            y = self.clip_min_y;
        }
        if y + height > self.clip_max_y {
            height = self.clip_max_y - y;
        }
        let inv_alpha = 256 - alpha;
        let r0 = ((rgb >> 16) & 0xff) * alpha;
        let g0 = ((rgb >> 8) & 0xff) * alpha;
        let b0 = (rgb & 0xff) * alpha;
        let mut offset = x + y * self.width;
        for _ in 0..height {
            let r1 = ((self.pixels[offset as usize] >> 16) & 0xff) * inv_alpha;
            let g1 = ((self.pixels[offset as usize] >> 8) & 0xff) * inv_alpha;
            let b1 = (self.pixels[offset as usize] & 0xff) * inv_alpha;
            self.pixels[offset as usize] =
                (((r0 + r1) >> 8) << 16) + (((g0 + g1) >> 8) << 8) + ((b0 + b1) >> 8);
            offset += self.width;
        }
        self.mark_col(x + y * self.width, height);
    }

    // mapview applet:

    pub fn fill_circle(
        &mut self,
        x_center: i32,
        y_center: i32,
        y_radius: i32,
        rgb: i32,
        alpha: i32,
    ) {
        let inv_alpha = 256 - alpha;
        let r0 = ((rgb >> 16) & 0xff) * alpha;
        let g0 = ((rgb >> 8) & 0xff) * alpha;
        let b0 = (rgb & 0xff) * alpha;

        let mut y_start = y_center - y_radius;
        if y_start < 0 {
            y_start = 0;
        }
        let mut y_end = y_center + y_radius;
        if y_end >= self.height {
            y_end = self.height - 1;
        }

        for y in y_start..=y_end {
            let midpoint = y - y_center;
            let x_radius = ((y_radius as i64 * y_radius as i64 - midpoint as i64 * midpoint as i64)
                as f64)
                .sqrt() as i32;

            let mut x_start = x_center - x_radius;
            if x_start < 0 {
                x_start = 0;
            }
            let mut x_end = x_center + x_radius;
            if x_end >= self.width {
                x_end = self.width - 1;
            }

            let row_start = x_start + y * self.width;
            for offset in row_start..=x_end + y * self.width {
                let r1 = ((self.pixels[offset as usize] >> 16) & 0xff) * inv_alpha;
                let g1 = ((self.pixels[offset as usize] >> 8) & 0xff) * inv_alpha;
                let b1 = (self.pixels[offset as usize] & 0xff) * inv_alpha;
                self.pixels[offset as usize] =
                    (((r0 + r1) >> 8) << 16) + (((g0 + g1) >> 8) << 8) + ((b0 + b1) >> 8);
            }
            self.mark_row(row_start, x_end - x_start + 1);
        }
    }

    /// Mark one written pixel fully opaque (chat, minimenu, glyphs). The
    /// GPU chrome composite uses this byte as the overlay alpha: 255 is
    /// solid over the scene. A no-op when no coverage buffer is attached.
    pub(crate) fn mark_pixel(&mut self, off: i32) {
        self.mark_pixel_alpha(off, 255);
    }

    /// Mark one written pixel with overlay alpha `a` (0 = scene hole).
    /// Nav debug fills use this so translucent path tiles composite over
    /// the 3D world instead of stamping opaque dark quads.
    pub(crate) fn mark_pixel_alpha(&mut self, off: i32, a: u8) {
        if let Some(cov) = &mut self.coverage {
            cov[off as usize] = a;
        }
    }

    /// Mark the exact written region covered (the GPU overlay pass only;
    /// a no-op when no coverage buffer is attached).
    fn mark_rect(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if let Some(cov) = &mut self.coverage {
            for cy in 0..h {
                let row = (y + cy) * self.width;
                for cx in 0..w {
                    cov[(row + x + cx) as usize] = 255;
                }
            }
        }
    }

    /// Mark one written row segment covered.
    fn mark_row(&mut self, off: i32, len: i32) {
        if let Some(cov) = &mut self.coverage {
            for i in 0..len {
                cov[(off + i) as usize] = 255;
            }
        }
    }

    /// Mark one written column segment covered.
    fn mark_col(&mut self, off: i32, len: i32) {
        if let Some(cov) = &mut self.coverage {
            for i in 0..len {
                cov[(off + i * self.width) as usize] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{coverage_guard, Pix2D};

    #[test]
    fn mark_pixel_is_opaque_and_alpha_is_the_coverage_byte() {
        let mut pix = vec![0i32; 4];
        let mut cov = vec![0u8; 4];
        let _g = coverage_guard(&mut cov, 2, 2);
        {
            let mut s = Pix2D::with_pixels(&mut pix, 2, 2);
            s.mark_pixel(0);
            s.mark_pixel_alpha(1, 82);
        }
        assert_eq!(cov[0], 255, "chat/minimenu coverage is opaque");
        assert_eq!(cov[1], 82, "nav fill coverage is the overlay alpha");
        assert_eq!(cov[2], 0);
    }
}
