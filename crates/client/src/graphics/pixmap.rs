// Port of `~/experiments/Server/webclient/src/graphics/PixMap.ts`. The canvas
// backing (`ImageData`/`putImageData`, `Canvas.ts`) is a `window`-feature
// concern; here the map is the CPU framebuffer only. `fill` routes through a
// `Pix2D` surface bound to `pixels`, mirroring the TS constructor's
// `Pix2D.setPixels(this.data, ...)`.

use super::pix2d::Pix2D;

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
}
