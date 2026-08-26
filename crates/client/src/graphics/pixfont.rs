// Port of `~/experiments/Server/webclient/src/graphics/PixFont.ts`. The TS
// `extends Linkable2` link chains are not needed until the client queues fonts,
// so the font is a plain struct; drawing targets a `&mut Pix2D`. `depack`
// returns `Result`. Char codes index the 256-slot arrays through `.get()`
// (TS out-of-range `charCodeAt` results read `undefined`).

use super::colour::Colour;
use super::pix2d::Pix2D;
use crate::io::{JagFile, Packet};
use crate::util::JavaRandom;

pub struct PixFont {
    pub char_mask: Vec<Vec<i8>>,
    pub char_mask_width: [i32; 256],
    pub char_mask_height: [i32; 256],
    pub char_offset_x: [i32; 256],
    pub char_offset_y: [i32; 256],
    pub char_advance: [i32; 256],

    pub rand: JavaRandom,
    pub strikeout: bool,
    pub height: i32,
}

impl PixFont {
    pub fn new() -> Self {
        PixFont {
            char_mask: vec![Vec::new(); 256],
            char_mask_width: [0; 256],
            char_mask_height: [0; 256],
            char_offset_x: [0; 256],
            char_offset_y: [0; 256],
            char_advance: [0; 256],
            rand: JavaRandom::now(),
            strikeout: false,
            height: 0,
        }
    }

    pub fn depack(archive: &JagFile, name: &str, quill: bool) -> Result<Self, ()> {
        let mut dat = Packet::new(archive.read(&format!("{name}.dat")).ok_or(())?);
        let mut idx = Packet::new(archive.read("index.dat").ok_or(())?);

        if dat.available() < 2 {
            return Err(());
        }
        idx.pos = (dat.g2() + 4) as usize;

        if idx.available() < 1 {
            return Err(());
        }
        let pal_count = idx.g1();
        if pal_count > 0 {
            idx.pos = idx.pos.saturating_add((pal_count as usize - 1) * 3);
        }

        if idx.available() < 7 * 256 {
            return Err(());
        }

        let mut font = PixFont::new();
        for c in 0..256 {
            font.char_offset_x[c] = idx.g1();
            font.char_offset_y[c] = idx.g1();
            let wi = idx.g2();
            let hi = idx.g2();
            font.char_mask_width[c] = wi;
            font.char_mask_height[c] = hi;
            let pixel_order = idx.g1();

            let len = (wi as i64 * hi as i64) as usize;
            if dat.available() < len {
                return Err(());
            }
            font.char_mask[c] = vec![0i8; len];

            if pixel_order == 0 {
                for j in 0..len {
                    font.char_mask[c][j] = dat.g1b() as i8;
                }
            } else if pixel_order == 1 {
                for x in 0..wi {
                    for y in 0..hi {
                        font.char_mask[c][(x + y * wi) as usize] = dat.g1b() as i8;
                    }
                }
            }

            if hi > font.height && c < 128 {
                font.height = hi;
            }

            font.char_offset_x[c] = 1;
            font.char_advance[c] = wi + 2;

            // strip trailing blank rows/columns by comparing the last row
            // against an all-blank row
            let space = Self::glyph_row_sum(&font.char_mask[c], wi, hi, false);
            if space <= (hi / 7) as i64 {
                font.char_advance[c] -= 1;
                font.char_offset_x[c] = 0;
            }
            let space = Self::glyph_row_sum(&font.char_mask[c], wi, hi, true);
            if space <= (hi / 7) as i64 {
                font.char_advance[c] -= 1;
            }
        }

        if quill {
            // ' ' = 'I'
            font.char_advance[32] = font.char_advance[73];
        } else {
            // ' ' = 'i'
            font.char_advance[32] = font.char_advance[105];
        }

        Ok(font)
    }

    /// Sum of one row of a glyph mask; out-of-range reads stand in for the
    /// TS `undefined` (NaN), which never satisfies `<= hi / 7`.
    fn glyph_row_sum(mask: &[i8], wi: i32, hi: i32, last: bool) -> i64 {
        let mut space = 0i64;
        for y in (hi / 7)..hi {
            let idx = if last { wi + y * wi - 1 } else { y * wi };
            space += mask.get(idx as usize).copied().map(i64::from).unwrap_or(i64::MIN);
        }
        space
    }

    fn glyph(&self, code: u32) -> (&[i8], i32, i32, i32, i32) {
        (
            self.char_mask.get(code as usize).map(|m| m.as_slice()).unwrap_or(&[]),
            self.char_offset_x.get(code as usize).copied().unwrap_or(0),
            self.char_offset_y.get(code as usize).copied().unwrap_or(0),
            self.char_mask_width.get(code as usize).copied().unwrap_or(0),
            self.char_mask_height.get(code as usize).copied().unwrap_or(0),
        )
    }

    pub fn centre_string(&self, surface: &mut Pix2D, str: Option<&str>, x: i32, y: i32, rgb: i32) {
        let str = match str {
            Some(s) => s,
            None => return,
        };
        self.draw_string(surface, Some(str), x - self.string_wid(Some(str)) / 2, y, rgb);
    }

    pub fn centre_string_tag(&mut self, surface: &mut Pix2D, str: &str, x: i32, y: i32, rgb: i32, shadowed: bool) {
        self.draw_string_tag(surface, str, x - self.string_wid(Some(str)) / 2, y, rgb, shadowed);
    }

    pub fn string_wid(&self, str: Option<&str>) -> i32 {
        let str = match str {
            Some(s) => s,
            None => return 0,
        };
        let chars: Vec<char> = str.chars().collect();
        let length = chars.len();
        let mut w = 0;
        let mut c = 0;
        while c < length {
            if chars[c] == '@' && c + 4 < length && chars[c + 4] == '@' {
                c += 4;
            } else {
                w += self.char_advance.get(chars[c] as u32 as usize).copied().unwrap_or(0);
            }
            c += 1;
        }
        w
    }

    pub fn draw_string(&self, surface: &mut Pix2D, str: Option<&str>, mut x: i32, mut y: i32, rgb: i32) {
        let str = match str {
            Some(s) => s,
            None => return,
        };
        y -= self.height;
        for c in str.chars() {
            let code = c as u32;
            if code != 32 {
                let (mask, ox, oy, w, h) = self.glyph(code);
                self.plot_letter(surface, mask, x + ox, y + oy, w, h, rgb);
            }
            x += self.char_advance.get(code as usize).copied().unwrap_or(0);
        }
    }

    pub fn centre_string_wave(&self, surface: &mut Pix2D, str: Option<&str>, mut x: i32, y: i32, rgb: i32, phase: i32) {
        let str = match str {
            Some(s) => s,
            None => return,
        };
        x -= self.string_wid(Some(str)) / 2;
        let off_y = y - self.height;
        for (i, c) in str.chars().enumerate() {
            let code = c as u32;
            if code != 32 {
                let (mask, ox, oy, w, h) = self.glyph(code);
                let wave = (f64::sin(i as f64 / 2.0 + phase as f64 / 5.0) * 5.0) as i32;
                self.plot_letter(surface, mask, x + ox, off_y + oy + wave, w, h, rgb);
            }
            x += self.char_advance.get(code as usize).copied().unwrap_or(0);
        }
    }

    pub fn draw_string_tag(&mut self, surface: &mut Pix2D, str: &str, mut x: i32, mut y: i32, mut rgb: i32, shadowed: bool) {
        self.strikeout = false;
        let start_x = x;
        let chars: Vec<char> = str.chars().collect();
        let length = chars.len();
        y -= self.height;
        let mut i = 0;
        while i < length {
            if chars[i] == '@' && i + 4 < length && chars[i + 4] == '@' {
                let tag: String = chars[i + 1..i + 4].iter().collect();
                let tag = self.update_state(&tag);
                if tag != -1 {
                    rgb = tag;
                }
                i += 4;
            } else {
                let code = chars[i] as u32;
                if code != 32 {
                    let (mask, ox, oy, w, h) = self.glyph(code);
                    if shadowed {
                        self.plot_letter(surface, mask, x + ox + 1, y + oy + 1, w, h, Colour::BLACK);
                    }
                    self.plot_letter(surface, mask, x + ox, y + oy, w, h, rgb);
                }
                x += self.char_advance.get(code as usize).copied().unwrap_or(0);
            }
            i += 1;
        }
        if self.strikeout {
            surface.hline(start_x, y + (self.height as f64 * 0.7) as i32, x - start_x, Colour::DARKRED);
        }
    }

    pub fn draw_string_anti_macro(&mut self, surface: &mut Pix2D, str: &str, mut x: i32, y: i32, mut rgb: i32, shadowed: bool, seed: i32) {
        self.rand.set_seed(seed as i64);
        let rand = (self.rand.next_int() & 0x1f) + 192;
        let off_y = y - self.height;
        let chars: Vec<char> = str.chars().collect();
        let length = chars.len();
        let mut i = 0;
        while i < length {
            if chars[i] == '@' && i + 4 < length && chars[i + 4] == '@' {
                let tag: String = chars[i + 1..i + 4].iter().collect();
                let tag = self.update_state(&tag);
                if tag != -1 {
                    rgb = tag;
                }
                i += 4;
            } else {
                let code = chars[i] as u32;
                if code != 32 {
                    let (mask, ox, oy, w, h) = self.glyph(code);
                    if shadowed {
                        self.plot_letter_trans(surface, mask, x + ox + 1, off_y + oy + 1, w, h, Colour::BLACK, 192);
                    }
                    self.plot_letter_trans(surface, mask, x + ox, off_y + oy, w, h, rgb, rand);
                }
                x += self.char_advance.get(code as usize).copied().unwrap_or(0);
                if (self.rand.next_int() & 0x3) == 0 {
                    x += 1;
                }
            }
            i += 1;
        }
    }

    pub fn update_state(&mut self, tag: &str) -> i32 {
        match tag {
            "red" => Colour::RED,
            "gre" => Colour::GREEN,
            "blu" => Colour::BLUE,
            "yel" => Colour::YELLOW,
            "cya" => Colour::CYAN,
            "mag" => Colour::MAGENTA,
            "whi" => Colour::WHITE,
            "bla" => Colour::BLACK,
            "lre" => Colour::LIGHTRED,
            "dre" => Colour::DARKRED,
            "dbl" => Colour::DARKBLUE,
            "or1" => Colour::ORANGE1,
            "or2" => Colour::ORANGE2,
            "or3" => Colour::ORANGE3,
            "gr1" => Colour::GREEN1,
            "gr2" => Colour::GREEN2,
            "gr3" => Colour::GREEN3,
            "str" => {
                self.strikeout = true;
                -1
            }
            _ => -1,
        }
    }

    pub fn draw_string_right(&self, surface: &mut Pix2D, str: &str, x: i32, y: i32, rgb: i32, shadowed: bool) {
        if shadowed {
            self.draw_string(surface, Some(str), x - self.string_wid(Some(str)) + 1, y + 1, Colour::BLACK);
        }
        self.draw_string(surface, Some(str), x - self.string_wid(Some(str)), y, rgb);
    }

    fn plot_letter(&self, surface: &mut Pix2D, data: &[i8], x: i32, y: i32, w: i32, h: i32, rgb: i32) {
        let mut x = x;
        let mut y = y;
        let mut w = w;
        let mut h = h;
        let mut dst_off = x + y * surface.width;
        let mut dst_step = surface.width - w;
        let mut src_step = 0;
        let mut src_off = 0;

        if y < surface.clip_min_y {
            let cutoff = surface.clip_min_y - y;
            h -= cutoff;
            y = surface.clip_min_y;
            src_off += cutoff * w;
            dst_off += cutoff * surface.width;
        }
        if y + h >= surface.clip_max_y {
            h -= y + h + 1 - surface.clip_max_y;
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
        if x + w >= surface.clip_max_x {
            let cutoff = x + w + 1 - surface.clip_max_x;
            w -= cutoff;
            src_step += cutoff;
            dst_step += cutoff;
        }

        if w > 0 && h > 0 {
            self.plot(surface, data, rgb, src_off, dst_off, w, h, dst_step, src_step);
        }
    }

    fn plot(&self, surface: &mut Pix2D, src: &[i8], rgb: i32, mut src_off: i32, mut dst_off: i32, w: i32, h: i32, dst_step: i32, src_step: i32) {
        let hw = w >> 2;
        let rem = w & 0x3;

        for _ in 0..h {
            for _ in 0..hw {
                for _ in 0..4 {
                    if src.get(src_off as usize).copied().unwrap_or(0) == 0 {
                        dst_off += 1;
                    } else {
                        if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                            *p = rgb;
                        }
                        dst_off += 1;
                    }
                    src_off += 1;
                }
            }
            for _ in 0..rem {
                if src.get(src_off as usize).copied().unwrap_or(0) == 0 {
                    dst_off += 1;
                } else {
                    if let Some(p) = surface.pixels.get_mut(dst_off as usize) {
                        *p = rgb;
                    }
                    dst_off += 1;
                }
                src_off += 1;
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }

    fn plot_letter_trans(&self, surface: &mut Pix2D, data: &[i8], x: i32, y: i32, w: i32, h: i32, rgb: i32, alpha: i32) {
        let mut x = x;
        let mut y = y;
        let mut w = w;
        let mut h = h;
        let mut dst_off = x + y * surface.width;
        let mut dst_step = surface.width - w;
        let mut src_step = 0;
        let mut src_off = 0;

        if y < surface.clip_min_y {
            let cutoff = surface.clip_min_y - y;
            h -= cutoff;
            y = surface.clip_min_y;
            src_off += cutoff * w;
            dst_off += cutoff * surface.width;
        }
        if y + h >= surface.clip_max_y {
            h -= y + h + 1 - surface.clip_max_y;
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
        if x + w >= surface.clip_max_x {
            let cutoff = x + w + 1 - surface.clip_max_x;
            w -= cutoff;
            src_step += cutoff;
            dst_step += cutoff;
        }

        if w > 0 && h > 0 {
            self.plot_trans(surface, data, rgb, src_off, dst_off, w, h, dst_step, src_step, alpha);
        }
    }

    fn plot_trans(&self, surface: &mut Pix2D, src: &[i8], rgb: i32, mut src_off: i32, mut dst_off: i32, w: i32, h: i32, dst_step: i32, src_step: i32, alpha: i32) {
        let mixed = ((((rgb & 0xff00ff).wrapping_mul(alpha)) & 0xff00_ff00u32 as i32)
            + (((rgb & 0xff00).wrapping_mul(alpha)) & 0xff0000))
            >> 8;
        let inv_alpha = 256 - alpha;

        for _ in 0..h {
            for _ in 0..w {
                if src.get(src_off as usize).copied().unwrap_or(0) == 0 {
                    dst_off += 1;
                } else {
                    let dst_rgb = surface.pixels[dst_off as usize];
                    surface.pixels[dst_off as usize] = (((((dst_rgb & 0xff00ff).wrapping_mul(inv_alpha)) & 0xff00_ff00u32 as i32)
                        + (((dst_rgb & 0xff00).wrapping_mul(inv_alpha)) & 0xff0000))
                        >> 8)
                        .wrapping_add(mixed);
                    dst_off += 1;
                }
                src_off += 1;
            }
            dst_off += dst_step;
            src_off += src_step;
        }
    }
}
