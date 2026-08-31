// Port of `~/experiments/Server/webclient/src/graphics/Pix32.ts`. The TS
// `extends Pix2D` inheritance collapses: plotting targets a `&mut Pix2D`.
// `fromJpeg` decodes a jag JPEG (title.dat); a 0x00 first byte is patched
// to 0xFF as client-ts `decodeJpeg`. Rotated plots read the source through
// `.get()` because out-of-bounds samples are normal there (TS typed arrays
// return `undefined` → 0); destination writes outside the framebuffer are
// skipped (TS silently ignores them).

use super::pix2d::Pix2D;
use super::pix8::Pix8;
use crate::datastruct::{LinkableTrait, Links};
use crate::io::{JagFile, Packet};

#[derive(Clone)]
pub struct Pix32 {
    pub data: Vec<i32>,
    pub wi: i32,  // width
    pub hi: i32,  // height
    pub xof: i32, // x offset
    pub yof: i32, // y offset
    pub owi: i32, // original width
    pub ohi: i32, // original height
    /// Cache-link state for the `LruCache` the `ObjType.spriteCache`
    /// renders into (Java `Linkable2` superclass).
    pub links: Links,
}

impl LinkableTrait for Pix32 {
    fn links(&self) -> &Links {
        &self.links
    }

    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }

    fn sentinel() -> Self {
        Pix32::new(0, 0)
    }
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
            links: Links::new(0),
        }
    }

    /// `Pix32.fromJpeg(archive, name)`: decode a JPEG stored in the jag.
    /// Client-ts `decodeJpeg` patches a missing SOI (`data[0] !== 0xff`).
    pub fn from_jpeg(jag: &JagFile, name: &str) -> Option<Self> {
        let mut bytes = jag.read(name)?;
        if bytes.first().copied() != Some(0xff) {
            if bytes.is_empty() {
                return None;
            }
            bytes[0] = 0xff;
        }
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
            .ok()?
            .to_rgb8();
        let wi = decoded.width() as i32;
        let hi = decoded.height() as i32;
        let mut data = Vec::with_capacity((wi as usize) * (hi as usize));
        for pixel in decoded.pixels() {
            let [r, g, b] = pixel.0;
            data.push(((r as i32) << 16) | ((g as i32) << 8) | (b as i32));
        }
        Some(Pix32 {
            data,
            wi,
            hi,
            owi: wi,
            ohi: hi,
            xof: 0,
            yof: 0,
            links: Links::new(0),
        })
    }

    pub fn depack(jag: &JagFile, name: &str, sprite: i32) -> Option<Self> {
        let mut dat = Packet::new(jag.read(&format!("{name}.dat"))?);
        let mut index = Packet::new(jag.read("index.dat")?);

        if dat.available() < 2 {
            return None;
        }
        index.pos = dat.g2() as usize;

        if index.available() < 5 {
            return None;
        }
        let owi = index.g2();
        let ohi = index.g2();
        let bpal_count = index.g1();
        if index.available() < 3 * bpal_count.saturating_sub(1) as usize {
            return None;
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
                return None;
            }
            index.pos += 2;
            let a = index.g2();
            let b = index.g2();
            dat.pos = dat.pos.saturating_add((a as i64 * b as i64) as usize);
            index.pos += 1;
        }

        if dat.pos > dat.length() || index.pos > index.length() {
            return None;
        }

        if index.available() < 1 + 1 + 2 + 2 + 1 {
            return None;
        }
        let xof = index.g1();
        let yof = index.g1();
        let wi = index.g2();
        let hi = index.g2();
        let encoding = index.g1();

        let pixel_len = (wi as i64 * hi as i64) as usize;
        if dat.available() < pixel_len {
            return None;
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
                    image.data[(x + y * wi) as usize] =
                        bpal.get(dat.g1() as usize).copied().unwrap_or(0);
                }
            }
        }

        Some(image)
    }

    pub fn rgb_adjust(&mut self, r: i32, g: i32, b: i32) {
        for i in 0..self.data.len() {
            let rgb = self.data[i];
            if rgb != 0 {
                let mut red = (rgb >> 16) & 0xff;
                red += r;
                red = red.clamp(1, 255);

                let mut green = (rgb >> 8) & 0xff;
                green += g;
                green = green.clamp(1, 255);

                let mut blue = rgb & 0xff;
                blue += b;
                blue = blue.clamp(1, 255);

                self.data[i] = (red << 16) + (green << 8) + blue;
            }
        }
    }

    pub fn trim(&mut self) {
        let mut pixels = vec![0; (self.owi as usize) * (self.ohi as usize)];
        for y in 0..self.hi {
            for x in 0..self.wi {
                pixels[((self.yof + y) * self.owi + self.xof + x) as usize] =
                    self.data[(self.wi * y + x) as usize];
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
                self.data.swap(off1 as usize, off2 as usize);
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
                self.data.swap(off1 as usize, off2 as usize);
            }
        }
    }

    pub fn quick_plot_sprite(&self, surface: &mut Pix2D, x: i32, y: i32) {
        let mut x = x;
        let mut y = y;
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

    #[allow(clippy::too_many_arguments)]
    fn plot_quick(
        &self,
        surface: &mut Pix2D,
        w: i32,
        h: i32,
        mut src_off: i32,
        src_step: i32,
        mut dst_off: i32,
        dst_step: i32,
    ) {
        let qw = w >> 2;
        let rem = w & 0x3;

        for _ in 0..h {
            for _ in 0..qw {
                for _ in 0..4 {
                    if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                        *p = self.data.get(src_off as usize).copied().unwrap_or(0);
                        surface.mark_pixel(dst_off);
                    }
                    src_off += 1;
                    dst_off += 1;
                }
            }
            for _ in 0..rem {
                if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                    *p = self.data.get(src_off as usize).copied().unwrap_or(0);
                    surface.mark_pixel(dst_off);
                }
                src_off += 1;
                dst_off += 1;
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    pub fn plot_sprite(&self, surface: &mut Pix2D, x: i32, y: i32) {
        let mut x = x;
        let mut y = y;
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

    #[allow(clippy::too_many_arguments)]
    fn plot(
        &self,
        surface: &mut Pix2D,
        w: i32,
        h: i32,
        mut src_off: i32,
        src_step: i32,
        mut dst_off: i32,
        dst_step: i32,
    ) {
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
                            surface.mark_pixel(dst_off);
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
                        surface.mark_pixel(dst_off);
                    }
                    dst_off += 1;
                }
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    pub fn trans_plot_sprite(&self, surface: &mut Pix2D, x: i32, y: i32, alpha: i32) {
        let mut x = x;
        let mut y = y;
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

    #[allow(clippy::too_many_arguments)]
    fn tran_sprite(
        &self,
        surface: &mut Pix2D,
        mut src_off: i32,
        mut dst_off: i32,
        w: i32,
        h: i32,
        dst_step: i32,
        src_step: i32,
        alpha: i32,
    ) {
        let inv_alpha = 256 - alpha;

        for _ in 0..h {
            for _ in 0..w {
                let rgb = self.data.get(src_off as usize).copied().unwrap_or(0);
                src_off += 1;
                if rgb == 0 {
                    dst_off += 1;
                } else {
                    let dst_rgb = surface.pixels[dst_off as usize];
                    surface.pixels[dst_off as usize] = ((((rgb & 0xff00ff)
                        .wrapping_mul(alpha)
                        .wrapping_add((dst_rgb & 0xff00ff).wrapping_mul(inv_alpha)))
                        & 0xff00_ff00u32 as i32)
                        + (((rgb & 0xff00).wrapping_mul(alpha)
                            + (dst_rgb & 0xff00).wrapping_mul(inv_alpha))
                            & 0xff0000))
                        >> 8;
                    surface.mark_pixel(dst_off);
                    dst_off += 1;
                }
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    #[allow(clippy::too_many_arguments)]
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

        let mut left_x = anchor_x.wrapping_shl(16).wrapping_add(
            center_y
                .wrapping_mul(sin_zoom)
                .wrapping_add(center_x.wrapping_mul(cos_zoom)),
        );
        let mut left_y = anchor_y.wrapping_shl(16).wrapping_add(
            center_y
                .wrapping_mul(cos_zoom)
                .wrapping_sub(center_x.wrapping_mul(sin_zoom)),
        );
        let mut left_off = x + y * surface.width;

        for i in 0..h {
            let dst_off = line_start[i as usize];
            let dst_start = left_off + dst_off;
            let width = line_width[i as usize];

            let mut src_x = left_x.wrapping_add(cos_zoom.wrapping_mul(dst_off));
            let mut src_y = left_y.wrapping_sub(sin_zoom.wrapping_mul(dst_off));

            for dst_x in dst_start..dst_start + width {
                let idx = (src_x >> 16) as i64 + (src_y >> 16) as i64 * self.wi as i64;
                if let Some(p) = surface.pixels.get_mut(dst_x as usize) {
                    *p = self.data.get(idx as usize).copied().unwrap_or(0);
                    surface.mark_pixel(dst_x);
                }
                src_x = src_x.wrapping_add(cos_zoom);
                src_y = src_y.wrapping_sub(sin_zoom);
            }

            left_x = left_x.wrapping_add(sin_zoom);
            left_y = left_y.wrapping_add(cos_zoom);
            left_off += surface.width;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rotate_plot_sprite(
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
    ) {
        let center_x = -w / 2;
        let center_y = -h / 2;

        let sin = (f64::sin(theta) * 65536.0) as i32;
        let cos = (f64::cos(theta) * 65536.0) as i32;
        let sin_zoom = sin.wrapping_mul(zoom) >> 8;
        let cos_zoom = cos.wrapping_mul(zoom) >> 8;

        let mut left_x = anchor_x.wrapping_shl(16).wrapping_add(
            center_y
                .wrapping_mul(sin_zoom)
                .wrapping_add(center_x.wrapping_mul(cos_zoom)),
        );
        let mut left_y = anchor_y.wrapping_shl(16).wrapping_add(
            center_y
                .wrapping_mul(cos_zoom)
                .wrapping_sub(center_x.wrapping_mul(sin_zoom)),
        );
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
                            surface.mark_pixel(dst_x);
                        }
                        dst_x += 1;
                    }
                    Some(&0) => {
                        dst_x += 1;
                    }
                    Some(&rgb) => {
                        if let Some(p) = surface.pixels.get_mut(dst_x as usize) {
                            *p = rgb;
                            surface.mark_pixel(dst_x);
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
            self.plot_scanline(
                surface, src_step, dst_step, w, h, dst_off, src_off, &mask.data,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn plot_scanline(
        &self,
        surface: &mut Pix2D,
        mut src_off: i32,
        mut dst_off: i32,
        w: i32,
        h: i32,
        dst_step: i32,
        src_step: i32,
        mask: &[i8],
    ) {
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
                            surface.mark_pixel(dst_off);
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
                        surface.mark_pixel(dst_off);
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
