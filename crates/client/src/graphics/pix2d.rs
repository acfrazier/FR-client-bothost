// Port of `~/experiments/Server/webclient/src/graphics/Pix2D.ts`. The TS
// statics (pixels / width / height / clipping) become a struct borrowing the
// framebuffer, so a `Pix2D` is the transient draw target TS binds with
// `setPixels`. Blend expressions keep TS's int32 wrap semantics: intermediate
// products exceed i32, and `& 0xff00ff00` + `>> 8` only observe the low bits,
// so wrapping ops reproduce the TS double arithmetic exactly.

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
}

impl<'a> Pix2D<'a> {
    /// TS `setPixels(pixels, width, height)` followed by the default full
    /// clipping: bind a framebuffer as the active draw target. On the GPU
    /// frame path this opens a recorded surface (the chrome quad layer).
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
        };
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.surface_open(s.pixels.as_ptr() as usize);
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
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.surface_clip(self.clip_min_x, self.clip_min_y, self.clip_max_x, self.clip_max_y);
        }
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
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.surface_clip(self.clip_min_x, self.clip_min_y, self.clip_max_x, self.clip_max_y);
        }
    }

    pub fn cls(&mut self) {
        self.pixels.fill(0);
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.surface_cls();
        }
    }

    pub fn fill_rect_trans(&mut self, mut x: i32, mut y: i32, mut width: i32, mut height: i32, rgb: i32, alpha: i32) {
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.fill_rect(x, y, width, height, rgb, alpha);
        }
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
    }

    pub fn fill_rect(&mut self, mut x: i32, mut y: i32, mut width: i32, mut height: i32, rgb: i32) {
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.fill_rect(x, y, width, height, rgb, 256);
        }
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
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.fill_rect(x, y, width, 1, rgb, 256);
        }
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
    }

    pub fn hline_trans(&mut self, mut x: i32, y: i32, mut width: i32, rgb: i32, alpha: i32) {
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.fill_rect(x, y, width, 1, rgb, alpha);
        }
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
        let mut offset = x + y * self.width;
        for _ in 0..width {
            let r1 = ((self.pixels[offset as usize] >> 16) & 0xff) * inv_alpha;
            let g1 = ((self.pixels[offset as usize] >> 8) & 0xff) * inv_alpha;
            let b1 = (self.pixels[offset as usize] & 0xff) * inv_alpha;
            self.pixels[offset as usize] =
                (((r0 + r1) >> 8) << 16) + (((g0 + g1) >> 8) << 8) + ((b0 + b1) >> 8);
            offset += 1;
        }
    }

    pub fn vline(&mut self, x: i32, mut y: i32, mut height: i32, rgb: i32) {
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.fill_rect(x, y, 1, height, rgb, 256);
        }
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
    }

    pub fn vline_trans(&mut self, x: i32, mut y: i32, mut height: i32, rgb: i32, alpha: i32) {
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.fill_rect(x, y, 1, height, rgb, alpha);
        }
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
    }

    // mapview applet:

    pub fn fill_circle(&mut self, x_center: i32, y_center: i32, y_radius: i32, rgb: i32, alpha: i32) {
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
            let x_radius = ((y_radius as i64 * y_radius as i64 - midpoint as i64 * midpoint as i64) as f64).sqrt() as i32;

            let mut x_start = x_center - x_radius;
            if x_start < 0 {
                x_start = 0;
            }
            let mut x_end = x_center + x_radius;
            if x_end >= self.width {
                x_end = self.width - 1;
            }

            let mut offset = x_start + y * self.width;
            for _x in x_start..=x_end {
                let r1 = ((self.pixels[offset as usize] >> 16) & 0xff) * inv_alpha;
                let g1 = ((self.pixels[offset as usize] >> 8) & 0xff) * inv_alpha;
                let b1 = (self.pixels[offset as usize] & 0xff) * inv_alpha;
                self.pixels[offset as usize] =
                    (((r0 + r1) >> 8) << 16) + (((g0 + g1) >> 8) << 8) + ((b0 + b1) >> 8);
                offset += 1;
            }
        }
    }
}
