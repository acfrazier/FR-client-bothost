// Port of `~/experiments/Server/webclient/src/graphics/PixMap.ts`. The canvas
// backing (`ImageData`/`putImageData`, `Canvas.ts`) is a `window`-feature
// concern; here the map is the CPU framebuffer only. `fill` routes through a
// `Pix2D` surface bound to `pixels`, mirroring the TS constructor's
// `Pix2D.setPixels(this.data, ...)`.

use super::pix2d::Pix2D;

#[derive(Clone)]
pub struct PixMap {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<i32>,
}

impl PixMap {
    pub fn new(width: i32, height: i32) -> Self {
        PixMap {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize)],
        }
    }

    pub fn fill(&mut self, rgb: i32) {
        let mut surface = Pix2D::with_pixels(&mut self.pixels, self.width, self.height);
        surface.fill_rect(0, 0, self.width, self.height, rgb);
    }

    /// TS `PixMap.draw(x, y)`: canvas `putImageData` of this map into `dest`
    /// at (x, y), clipped to both maps. Pixels are `0x00RRGGBB` and copy
    /// verbatim (the TS paint step converts RGB → RGBA for the canvas only).
    pub fn blit_into(&self, dest: &mut PixMap, x: i32, y: i32) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + self.width).min(dest.width);
        let y1 = (y + self.height).min(dest.height);
        for dy in y0..y1 {
            for dx in x0..x1 {
                dest.pixels[(dy * dest.width + dx) as usize] =
                    self.pixels[((dy - y) * self.width + (dx - x)) as usize];
            }
        }
    }
}
