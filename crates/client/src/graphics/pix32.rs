// Port of `~/experiments/Server/webclient/src/graphics/Pix32.ts`. The TS
// `extends Pix2D` inheritance collapses: plotting targets a `&mut Pix2D`.
// `fromJpeg` (browser image decode) is out of scope; `depack` returns
// `Result`. Rotated plots read the source through `.get()` because out-of-bounds
// samples are normal there (TS typed arrays return `undefined` → 0); destination
// writes outside the framebuffer are skipped (TS silently ignores them).

use super::pix2d::Pix2D;
use super::pix8::Pix8;
use crate::io::{JagFile, Packet};

pub struct Pix32 {
    pub data: Vec<i32>,
    pub wi: i32, // width
    pub hi: i32, // height
    pub xof: i32, // x offset
    pub yof: i32, // y offset
    pub owi: i32, // original width
    pub ohi: i32, // original height
}

impl Pix32 {
    pub fn new(width: i32, height: i32) -> Self {
        Pix32 {
            data: vec![0; (width as usize) * (height as usize)],
            wi: width,
            hi: height,
            owi: width,
            ohi: height,
            xof: 0,
            yof: 0,
        }
    }

    pub fn depack(jag: &JagFile, name: &str, sprite: i32) -> Result<Self, ()> {
        let mut dat = Packet::new(jag.read(&format!("{name}.dat")).ok_or(())?);
        let mut index = Packet::new(jag.read("index.dat").ok_or(())?);

        if dat.available() < 2 {
            return Err(());
        }
        index.pos = dat.g2() as usize;

        if index.available() < 5 {
            return Err(());
        }
        let owi = index.g2();
        let ohi = index.g2();
        let bpal_count = index.g1();
        if index.available() < 3 * bpal_count.saturating_sub(1) as usize {
            return Err(());
        }
        let mut bpal = vec![0i32; bpal_count as usize];
        for i in 0..bpal_count - 1 {
            bpal[i as usize + 1] = index.g3();
            if bpal[i as usize + 1] == 0 {
                bpal[i as usize + 1] = 1;
            }
        }

        for _ in 0..sprite {
            if index.available() < 6 {
                return Err(());
            }
            index.pos += 2;
            let a = index.g2();
            let b = index.g2();
            dat.pos = dat.pos.saturating_add((a as i64 * b as i64) as usize);
            index.pos += 1;
        }

        if dat.pos > dat.length() || index.pos > index.length() {
            return Err(());
        }

        if index.available() < 1 + 1 + 2 + 2 + 1 {
            return Err(());
        }
        let xof = index.g1();
        let yof = index.g1();
        let wi = index.g2();
        let hi = index.g2();
        let encoding = index.g1();

        let pixel_len = (wi as i64 * hi as i64) as usize;
        if dat.available() < pixel_len {
            return Err(());
        }

        let mut image = Pix32::new(wi, hi);
        image.xof = xof;
        image.yof = yof;
        image.owi = owi;
        image.ohi = ohi;

        if encoding == 0 {
            for i in 0..pixel_len {
                image.data[i] = bpal.get(dat.g1() as usize).copied().unwrap_or(0);
            }
        } else if encoding == 1 {
            for x in 0..wi {
                for y in 0..hi {
                    image.data[(x + y * wi) as usize] = bpal.get(dat.g1() as usize).copied().unwrap_or(0);
                }
            }
        }

        Ok(image)
    }

    pub fn rgb_adjust(&mut self, r: i32, g: i32, b: i32) {
        for i in 0..self.data.len() {
            let rgb = self.data[i];
            if rgb != 0 {
                let mut red = (rgb >> 16) & 0xff;
                red += r;
                if red < 1 {
                    red = 1;
                } else if red > 255 {
                    red = 255;
                }

                let mut green = (rgb >> 8) & 0xff;
                green += g;
                if green < 1 {
                    green = 1;
                } else if green > 255 {
                    green = 255;
                }

                let mut blue = rgb & 0xff;
                blue += b;
                if blue < 1 {
                    blue = 1;
                } else if blue > 255 {
                    blue = 255;
                }

                self.data[i] = (red << 16) + (green << 8) + blue;
            }
        }
    }

    pub fn trim(&mut self) {
        let mut pixels = vec![0; (self.owi as usize) * (self.ohi as usize)];
        for y in 0..self.hi {
            for x in 0..self.wi {
                pixels[((self.yof + y) * self.owi + self.xof + x) as usize] = self.data[(self.wi * y + x) as usize];
            }
        }
        self.data = pixels;
        self.wi = self.owi;
        self.hi = self.ohi;
        self.xof = 0;
        self.yof = 0;
    }

    pub fn hflip(&mut self) {
        let width = self.wi;
        let height = self.hi;
        for y in 0..height {
            let div = width / 2;
            for x in 0..div {
                let off1 = x + y * width;
                let off2 = width - x - 1 + y * width;
                let tmp = self.data[off1 as usize];
                self.data[off1 as usize] = self.data[off2 as usize];
                self.data[off2 as usize] = tmp;
            }
        }
    }

    pub fn vflip(&mut self) {
        let width = self.wi;
        let height = self.hi;
        for y in 0..height / 2 {
            for x in 0..width {
                let off1 = x + y * width;
                let off2 = x + (height - y - 1) * width;
                let tmp = self.data[off1 as usize];
                self.data[off1 as usize] = self.data[off2 as usize];
                self.data[off2 as usize] = tmp;
            }
        }
    }

    pub fn quick_plot_sprite(&self, surface: &mut Pix2D, mut x: i32, mut y: i32) {
        x += self.xof;
        y += self.yof;

        let mut dst_off = x + y * surface.width;
        let mut src_off = 0;

        let mut h = self.hi;
        let mut w = self.wi;

        let mut dst_step = surface.width - w;
        let mut src_step = 0;

        if y < surface.clip_min_y {
            let cutoff = surface.clip_min_y - y;
            h -= cutoff;
            y = surface.clip_min_y;
            src_off += cutoff * w;
            dst_off += cutoff * surface.width;
        }
        if y + h > surface.clip_max_y {
            h -= y + h - surface.clip_max_y;
        }
        if x < surface.clip_min_x {
            let cutoff = surface.clip_min_x - x;
            w -= cutoff;
            x = surface.clip_min_x;
            src_off += cutoff;
            dst_off += cutoff;
            src_step += cutoff;
            dst_step += cutoff;
        }
        if x + w > surface.clip_max_x {
            let cutoff = x + w - surface.clip_max_x;
            w -= cutoff;
            src_step += cutoff;
            dst_step += cutoff;
        }

        if w > 0 && h > 0 {
            self.plot_quick(surface, w, h, src_off, src_step, dst_off, dst_step);
        }
    }

    fn plot_quick(&self, surface: &mut Pix2D, w: i32, h: i32, mut src_off: i32, src_step: i32, mut dst_off: i32, dst_step: i32) {
        let qw = w >> 2;
        let rem = w & 0x3;

        for _ in 0..h {
            for _ in 0..qw {
                for _ in 0..4 {
                    if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                        *p = self.data.get(src_off as usize).copied().unwrap_or(0);
                    }
                    src_off += 1;
                    dst_off += 1;
                }
            }
            for _ in 0..rem {
                if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                    *p = self.data.get(src_off as usize).copied().unwrap_or(0);
                }
                src_off += 1;
                dst_off += 1;
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    pub fn plot_sprite(&self, surface: &mut Pix2D, mut x: i32, mut y: i32) {
        x += self.xof;
        y += self.yof;

        let mut dst_off = x + y * surface.width;
        let mut src_off = 0;

        let mut h = self.hi;
        let mut w = self.wi;

        let mut dst_step = surface.width - w;
        let mut src_step = 0;

        if y < surface.clip_min_y {
            let cutoff = surface.clip_min_y - y;
            h -= cutoff;
            y = surface.clip_min_y;
            src_off += cutoff * w;
            dst_off += cutoff * surface.width;
        }
        if y + h > surface.clip_max_y {
            h -= y + h - surface.clip_max_y;
        }
        if x < surface.clip_min_x {
            let cutoff = surface.clip_min_x - x;
            w -= cutoff;
            x = surface.clip_min_x;
            src_off += cutoff;
            dst_off += cutoff;
            src_step += cutoff;
            dst_step += cutoff;
        }
        if x + w > surface.clip_max_x {
            let cutoff = x + w - surface.clip_max_x;
            w -= cutoff;
            src_step += cutoff;
            dst_step += cutoff;
        }

        if w > 0 && h > 0 {
            self.plot(surface, w, h, src_off, src_step, dst_off, dst_step);
        }
    }

    fn plot(&self, surface: &mut Pix2D, w: i32, h: i32, mut src_off: i32, src_step: i32, mut dst_off: i32, dst_step: i32) {
        let qw = w >> 2;
        let rem = w & 0x3;

        for _ in 0..h {
            for _ in 0..qw {
                for _ in 0..4 {
                    let rgb = self.data.get(src_off as usize).copied().unwrap_or(0);
                    src_off += 1;
                    if rgb == 0 {
                        dst_off += 1;
                    } else {
                        if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                            *p = rgb;
                        }
                        dst_off += 1;
                    }
                }
            }
            for _ in 0..rem {
                let rgb = self.data.get(src_off as usize).copied().unwrap_or(0);
                src_off += 1;
                if rgb == 0 {
                    dst_off += 1;
                } else {
                    if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                        *p = rgb;
                    }
                    dst_off += 1;
                }
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    pub fn trans_plot_sprite(&self, surface: &mut Pix2D, mut x: i32, mut y: i32, alpha: i32) {
        x += self.xof;
        y += self.yof;

        let mut dst_step = x + y * surface.width;
        let mut src_step = 0;
        let mut h = self.hi;
        let mut w = self.wi;
        let mut dst_off = surface.width - w;
        let mut src_off = 0;

        if y < surface.clip_min_y {
            let cutoff = surface.clip_min_y - y;
            h -= cutoff;
            y = surface.clip_min_y;
            src_step += cutoff * w;
            dst_step += cutoff * surface.width;
        }
        if y + h > surface.clip_max_y {
            h -= y + h - surface.clip_max_y;
        }
        if x < surface.clip_min_x {
            let cutoff = surface.clip_min_x - x;
            w -= cutoff;
            x = surface.clip_min_x;
            src_step += cutoff;
            dst_step += cutoff;
            src_off += cutoff;
            dst_off += cutoff;
        }
        if x + w > surface.clip_max_x {
            let cutoff = x + w - surface.clip_max_x;
            w -= cutoff;
            src_off += cutoff;
            dst_off += cutoff;
        }

        if w > 0 && h > 0 {
            self.tran_sprite(surface, src_step, dst_step, w, h, dst_off, src_off, alpha);
        }
    }

    fn tran_sprite(&self, surface: &mut Pix2D, mut src_off: i32, mut dst_off: i32, w: i32, h: i32, dst_step: i32, src_step: i32, alpha: i32) {
        let inv_alpha = 256 - alpha;

        for _ in 0..h {
            for _ in 0..w {
                let rgb = self.data.get(src_off as usize).copied().unwrap_or(0);
                src_off += 1;
                if rgb == 0 {
                    dst_off += 1;
                } else {
                    let dst_rgb = surface.pixels[dst_off as usize];
                    surface.pixels[dst_off as usize] = ((((rgb & 0xff00ff).wrapping_mul(alpha)
                        .wrapping_add((dst_rgb & 0xff00ff).wrapping_mul(inv_alpha)))
                        & 0xff00_ff00u32 as i32)
                        + (((rgb & 0xff00).wrapping_mul(alpha) + (dst_rgb & 0xff00).wrapping_mul(inv_alpha)) & 0xff0000))
                        >> 8;
                    dst_off += 1;
                }
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    pub fn scanline_rotate_plot_sprite(
        &self,
        surface: &mut Pix2D,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        anchor_x: i32,
        anchor_y: i32,
        theta: f64,
        zoom: i32,
        line_start: &[i32],
        line_width: &[i32],
    ) {
        let center_x = -w / 2;
        let center_y = -h / 2;

        let sin = (f64::sin(theta / 326.11) * 65536.0) as i32;
        let cos = (f64::cos(theta / 326.11) * 65536.0) as i32;
        let sin_zoom = sin.wrapping_mul(zoom) >> 8;
        let cos_zoom = cos.wrapping_mul(zoom) >> 8;

        let mut left_x = anchor_x.wrapping_shl(16).wrapping_add(center_y.wrapping_mul(sin_zoom).wrapping_add(center_x.wrapping_mul(cos_zoom)));
        let mut left_y = anchor_y.wrapping_shl(16).wrapping_add(center_y.wrapping_mul(cos_zoom).wrapping_sub(center_x.wrapping_mul(sin_zoom)));
        let mut left_off = x + y * surface.width;

        for i in 0..h {
            let dst_off = line_start[i as usize];
            let mut dst_x = left_off + dst_off;

            let mut src_x = left_x.wrapping_add(cos_zoom.wrapping_mul(dst_off));
            let mut src_y = left_y.wrapping_sub(sin_zoom.wrapping_mul(dst_off));

            for _ in 0..line_width[i as usize] {
                let idx = (src_x >> 16) as i64 + (src_y >> 16) as i64 * self.wi as i64;
                if let Some(p) = surface.pixels.get_mut(dst_x as usize) {
                    *p = self.data.get(idx as usize).copied().unwrap_or(0);
                }
                dst_x += 1;
                src_x = src_x.wrapping_add(cos_zoom);
                src_y = src_y.wrapping_sub(sin_zoom);
            }

            left_x = left_x.wrapping_add(sin_zoom);
            left_y = left_y.wrapping_add(cos_zoom);
            left_off += surface.width;
        }
    }

    pub fn rotate_plot_sprite(&self, surface: &mut Pix2D, x: i32, y: i32, w: i32, h: i32, anchor_x: i32, anchor_y: i32, theta: f64, zoom: i32) {
        let center_x = -w / 2;
        let center_y = -h / 2;

        let sin = (f64::sin(theta) * 65536.0) as i32;
        let cos = (f64::cos(theta) * 65536.0) as i32;
        let sin_zoom = sin.wrapping_mul(zoom) >> 8;
        let cos_zoom = cos.wrapping_mul(zoom) >> 8;

        let mut left_x = anchor_x.wrapping_shl(16).wrapping_add(center_y.wrapping_mul(sin_zoom).wrapping_add(center_x.wrapping_mul(cos_zoom)));
        let mut left_y = anchor_y.wrapping_shl(16).wrapping_add(center_y.wrapping_mul(cos_zoom).wrapping_sub(center_x.wrapping_mul(sin_zoom)));
        let mut left_off = x + y * surface.width;

        for _ in 0..h {
            let mut dst_x = left_off;
            let mut src_x = left_x;
            let mut src_y = left_y;

            for _ in 0..w {
                let idx = (src_x >> 16) as i64 + (src_y >> 16) as i64 * self.owi as i64;
                // TS: out-of-bounds samples read as `undefined`, which is not
                // `== 0` and writes 0 into the destination.
                match self.data.get(idx as usize) {
                    None => {
                        if let Some(p) = surface.pixels.get_mut(dst_x as usize) {
                            *p = 0;
                        }
                        dst_x += 1;
                    }
                    Some(&0) => {
                        dst_x += 1;
                    }
                    Some(&rgb) => {
                        if let Some(p) = surface.pixels.get_mut(dst_x as usize) {
                            *p = rgb;
                        }
                        dst_x += 1;
                    }
                }

                src_x = src_x.wrapping_add(cos_zoom);
                src_y = src_y.wrapping_sub(sin_zoom);
            }

            left_x = left_x.wrapping_add(sin_zoom);
            left_y = left_y.wrapping_add(cos_zoom);
            left_off += surface.width;
        }
    }

    pub fn scanline_plot_sprite(&self, surface: &mut Pix2D, mask: &Pix8, mut x: i32, mut y: i32) {
        x += self.xof;
        y += self.yof;

        let mut dst_step = x + y * surface.width;
        let mut src_step = 0;
        let mut h = self.hi;
        let mut w = self.wi;
        let mut dst_off = surface.width - w;
        let mut src_off = 0;

        if y < surface.clip_min_y {
            let cutoff = surface.clip_min_y - y;
            h -= cutoff;
            y = surface.clip_min_y;
            src_step += cutoff * w;
            dst_step += cutoff * surface.width;
        }
        if y + h > surface.clip_max_y {
            h -= y + h - surface.clip_max_y;
        }
        if x < surface.clip_min_x {
            let cutoff = surface.clip_min_x - x;
            w -= cutoff;
            x = surface.clip_min_x;
            src_step += cutoff;
            dst_step += cutoff;
            src_off += cutoff;
            dst_off += cutoff;
        }
        if x + w > surface.clip_max_x {
            let cutoff = x + w - surface.clip_max_x;
            w -= cutoff;
            src_off += cutoff;
            dst_off += cutoff;
        }

        if w > 0 && h > 0 {
            self.plot_scanline(surface, src_step, dst_step, w, h, dst_off, src_off, &mask.data);
        }
    }

    fn plot_scanline(&self, surface: &mut Pix2D, mut src_off: i32, mut dst_off: i32, w: i32, h: i32, dst_step: i32, src_step: i32, mask: &[i8]) {
        let qw = w >> 2;
        let rem = w & 0x3;

        for _ in 0..h {
            for _ in 0..qw {
                for _ in 0..4 {
                    let rgb = self.data.get(src_off as usize).copied().unwrap_or(0);
                    src_off += 1;
                    if rgb != 0 && mask.get(dst_off as usize).copied().unwrap_or(1) == 0 {
                        if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                            *p = rgb;
                        }
                        dst_off += 1;
                    } else {
                        dst_off += 1;
                    }
                }
            }
            for _ in 0..rem {
                let rgb = self.data.get(src_off as usize).copied().unwrap_or(0);
                src_off += 1;
                if rgb != 0 && mask.get(dst_off as usize).copied().unwrap_or(1) == 0 {
                    if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                        *p = rgb;
                    }
                    dst_off += 1;
                } else {
                    dst_off += 1;
                }
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }
}
