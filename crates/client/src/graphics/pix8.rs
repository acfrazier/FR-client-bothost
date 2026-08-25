// Port of `~/experiments/Server/webclient/src/graphics/Pix8.ts`. The TS
// `extends Pix2D` inheritance collapses: plotting targets a `&mut Pix2D`
// instead of the global statics. `depack` throws in TS; here it returns
// `Result` and callers skip missing archives. Out-of-bounds reads follow the
// TS typed-array semantics (undefined → palette index 0 → black/transparent).

use super::pix2d::Pix2D;
use crate::io::{JagFile, Packet};

#[derive(Clone)]
pub struct Pix8 {
    pub data: Vec<i8>,
    pub bpal: Vec<i32>, // base palette
    pub wi: i32,        // width
    pub hi: i32,        // height
    pub xof: i32,       // x offset
    pub yof: i32,       // y offset
    pub owi: i32,       // original width
    pub ohi: i32,       // original height
}

impl Pix8 {
    pub fn new(width: i32, height: i32, palette: Vec<i32>) -> Self {
        Pix8 {
            data: vec![0; (width as usize) * (height as usize)],
            wi: width,
            hi: height,
            owi: width,
            ohi: height,
            xof: 0,
            yof: 0,
            bpal: palette,
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

        let mut image = Pix8::new(wi, hi, bpal);
        image.xof = xof;
        image.yof = yof;
        image.owi = owi;
        image.ohi = ohi;

        if encoding == 0 {
            for i in 0..pixel_len {
                image.data[i] = dat.g1b() as i8;
            }
        } else if encoding == 1 {
            for x in 0..wi {
                for y in 0..hi {
                    image.data[(x + y * wi) as usize] = dat.g1b() as i8;
                }
            }
        }

        Ok(image)
    }

    pub fn halve_size(&mut self) {
        self.owi /= 2;
        self.ohi /= 2;

        let mut pixels = vec![0i8; (self.owi as usize) * (self.ohi as usize)];
        let mut off = 0;
        for y in 0..self.hi {
            for x in 0..self.wi {
                let dst = ((x + self.xof) >> 1) + ((y + self.yof) >> 1) * self.owi;
                pixels[dst as usize] = self.data[off];
                off += 1;
            }
        }

        self.data = pixels;
        self.wi = self.owi;
        self.hi = self.ohi;
        self.xof = 0;
        self.yof = 0;
    }

    pub fn trim(&mut self) {
        if self.wi == self.owi && self.hi == self.ohi {
            return;
        }

        let mut pixels = vec![0i8; (self.owi as usize) * (self.ohi as usize)];
        let mut off = 0;
        for y in 0..self.hi {
            for x in 0..self.wi {
                pixels[(x + self.xof + (y + self.yof) * self.owi) as usize] = self.data[off];
                off += 1;
            }
        }

        self.data = pixels;
        self.wi = self.owi;
        self.hi = self.ohi;
        self.xof = 0;
        self.yof = 0;
    }

    pub fn rgb_adjust(&mut self, r: i32, g: i32, b: i32) {
        for i in 0..self.bpal.len() {
            let mut red = (self.bpal[i] >> 16) & 0xff;
            red += r;
            if red < 0 {
                red = 0;
            } else if red > 255 {
                red = 255;
            }

            let mut green = (self.bpal[i] >> 8) & 0xff;
            green += g;
            if green < 0 {
                green = 0;
            } else if green > 255 {
                green = 255;
            }

            let mut blue = self.bpal[i] & 0xff;
            blue += b;
            if blue < 0 {
                blue = 0;
            } else if blue > 255 {
                blue = 255;
            }

            self.bpal[i] = (red << 16) + (green << 8) + blue;
        }
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

    pub fn plot_sprite(&self, surface: &mut Pix2D, x: i32, y: i32) {
        if let Some(chrome) = crate::render::backend::gpu_chrome::GpuChrome::active() {
            chrome.sprite_pix8(self, x, y, 256);
        }
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

    fn plot(&self, surface: &mut Pix2D, w: i32, h: i32, mut src_off: i32, src_step: i32, mut dst_off: i32, dst_step: i32) {
        let qw = w >> 2;
        let rem = w & 0x3;

        for _ in 0..h {
            for _ in 0..qw {
                for _ in 0..4 {
                    let pal_index = self.data.get(src_off as usize).copied().unwrap_or(0);
                    src_off += 1;
                    if pal_index == 0 {
                        dst_off += 1;
                    } else {
                        if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                            *p = self.bpal.get((pal_index as u8) as usize).copied().unwrap_or(0);
                        }
                        dst_off += 1;
                    }
                }
            }
            for _ in 0..rem {
                let pal_index = self.data.get(src_off as usize).copied().unwrap_or(0);
                src_off += 1;
                if pal_index == 0 {
                    dst_off += 1;
                } else {
                    if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                        *p = self.bpal.get((pal_index as u8) as usize).copied().unwrap_or(0);
                    }
                    dst_off += 1;
                }
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    // mapview applet:

    pub fn scale_plot_sprite(&self, surface: &mut Pix2D, mut arg0: i32, mut arg1: i32, mut arg2: i32, mut arg3: i32) {
        let local2 = self.wi;
        let local5 = self.hi;
        let mut local7: i32 = 0;
        let mut local9: i32 = 0;
        let _local15 = local2.wrapping_shl(16).checked_div(arg2).unwrap_or(0);
        let _local21 = local5.wrapping_shl(16).checked_div(arg3).unwrap_or(0);
        let local24 = self.owi;
        let local27 = self.ohi;
        let local33 = local24.wrapping_shl(16).checked_div(arg2).unwrap_or(0);
        let local39 = local27.wrapping_shl(16).checked_div(arg3).unwrap_or(0);
        arg0 = (arg0 + (self.xof.wrapping_mul(arg2) + local24 - 1) / local24) | 0;
        arg1 = (arg1 + (self.yof.wrapping_mul(arg3) + local27 - 1) / local27) | 0;
        if self.xof.wrapping_mul(arg2) % local24 != 0 {
            local7 = (local24.wrapping_sub(self.xof.wrapping_mul(arg2) % local24))
                .wrapping_shl(16)
                .checked_div(arg2)
                .unwrap_or(0);
        }
        if self.yof.wrapping_mul(arg3) % local27 != 0 {
            local9 = (local27.wrapping_sub(self.yof.wrapping_mul(arg3) % local27))
                .wrapping_shl(16)
                .checked_div(arg3)
                .unwrap_or(0);
        }
        arg2 = arg2.wrapping_mul(self.wi - (local7 >> 16)) / local24;
        arg3 = arg3.wrapping_mul(self.hi - (local9 >> 16)) / local27;
        let mut local133 = arg0 + arg1 * surface.width;
        let mut local137 = surface.width - arg2;
        let mut local144: i32;
        if arg1 < surface.clip_min_y {
            local144 = surface.clip_min_y - arg1;
            arg3 -= local144;
            arg1 = 0;
            local133 = local133.wrapping_add(local144.wrapping_mul(surface.width));
            local9 = local9.wrapping_add(local39.wrapping_mul(local144));
        }
        if arg1 + arg3 > surface.clip_max_y {
            arg3 -= arg1 + arg3 - surface.clip_max_y;
        }
        if arg0 < surface.clip_min_x {
            local144 = surface.clip_min_x - arg0;
            arg2 -= local144;
            arg0 = 0;
            local133 += local144;
            local7 = local7.wrapping_add(local33.wrapping_mul(local144));
            local137 += local144;
        }
        if arg0 + arg2 > surface.clip_max_x {
            local144 = arg0 + arg2 - surface.clip_max_x;
            arg2 -= local144;
            local137 += local144;
        }
        self.plot_scale(surface, local7, local9, local133, local137, arg2, arg3, local33, local39, local2);
    }

    fn plot_scale(&self, surface: &mut Pix2D, mut off_w: i32, mut off_h: i32, mut dst_off: i32, dst_step: i32, w: i32, h: i32, scale_crop_width: i32, scale_crop_height: i32, arg11: i32) {
        let last_off_w = off_w;
        for _ in 0..h {
            let off_y = (off_h >> 16) * arg11;
            for _ in 0..w {
                let index = (off_w >> 16) + off_y;
                let rgb = self.data.get(index as usize).copied();
                match rgb {
                    // TS: out-of-bounds src reads as `undefined`, which is not
                    // `== 0` and writes `bpal[0]` (= 0) into the destination.
                    None => {
                        if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                            *p = self.bpal.first().copied().unwrap_or(0);
                        }
                        dst_off += 1;
                    }
                    Some(0) => {
                        dst_off += 1;
                    }
                    Some(rgb) => {
                        if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                            *p = self.bpal.get((rgb as u8) as usize).copied().unwrap_or(0);
                        }
                        dst_off += 1;
                    }
                }
                off_w = off_w.wrapping_add(scale_crop_width);
            }
            off_h = off_h.wrapping_add(scale_crop_height);
            off_w = last_off_w;
            dst_off += dst_step;
        }
    }
}
