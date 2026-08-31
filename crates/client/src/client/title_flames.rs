//! Title-screen torch flames, 1:1 of client-ts `TitleFlames.ts`.
//! The TS 35 ms `setInterval` becomes `render_flames` once per title frame.

use crate::graphics::{Colour, Pix32, Pix8, PixMap};

const FLAME_WIDTH: i32 = 128;
const FLAME_HEIGHT: i32 = 256;
const TITLE_FLAME_PIXELS: usize = 33920;

pub struct TitleFlames {
    runes: Vec<Pix8>,
    pub active: bool,
    flame_left: Option<Pix32>,
    flame_right: Option<Pix32>,
    flame_buffer1: Vec<i32>,
    flame_buffer0: Vec<i32>,
    flame_buffer3: Vec<i32>,
    flame_buffer2: Vec<i32>,
    flame_gradient: Vec<i32>,
    flame_gradient0: Vec<i32>,
    flame_gradient1: Vec<i32>,
    flame_gradient2: Vec<i32>,
    flame_line_offset: Vec<i32>,
    pub cycle: i32,
    pub cooling_cycle: i32,
    flame_gradient_cycle0: i32,
    flame_gradient_cycle1: i32,
    rng: u32,
}

impl TitleFlames {
    pub fn new(runes: Vec<Pix8>) -> Self {
        TitleFlames {
            runes,
            active: false,
            flame_left: None,
            flame_right: None,
            flame_buffer1: Vec::new(),
            flame_buffer0: Vec::new(),
            flame_buffer3: Vec::new(),
            flame_buffer2: Vec::new(),
            flame_gradient: Vec::new(),
            flame_gradient0: Vec::new(),
            flame_gradient1: Vec::new(),
            flame_gradient2: Vec::new(),
            flame_line_offset: vec![0; FLAME_HEIGHT as usize],
            cycle: 0,
            cooling_cycle: 0,
            flame_gradient_cycle0: 0,
            flame_gradient_cycle1: 0,
            rng: 0xC0FFEE,
        }
    }

    pub fn setup_fire(&mut self, title_left: &PixMap, title_right: &PixMap) {
        let mut flame_left = Pix32::new(FLAME_WIDTH, 265);
        let mut flame_right = Pix32::new(FLAME_WIDTH, 265);
        let n = TITLE_FLAME_PIXELS
            .min(title_left.pixels.len())
            .min(flame_left.data.len());
        flame_left.data[..n].copy_from_slice(&title_left.pixels[..n]);
        let n = TITLE_FLAME_PIXELS
            .min(title_right.pixels.len())
            .min(flame_right.data.len());
        flame_right.data[..n].copy_from_slice(&title_right.pixels[..n]);
        self.flame_left = Some(flame_left);
        self.flame_right = Some(flame_right);

        let mut g0 = vec![0i32; 256];
        for (index, slot) in g0.iter_mut().enumerate().take(64) {
            *slot = index as i32 * 262144;
        }
        for index in 0..64 {
            g0[index + 64] = index as i32 * 1024 + Colour::RED;
        }
        for index in 0..64 {
            g0[index + 128] = index as i32 * 4 + Colour::YELLOW;
        }
        for index in 0..64 {
            g0[index + 192] = Colour::WHITE;
        }

        let mut g1 = vec![0i32; 256];
        for (index, slot) in g1.iter_mut().enumerate().take(64) {
            *slot = index as i32 * 1024;
        }
        for index in 0..64 {
            g1[index + 64] = index as i32 * 4 + Colour::GREEN;
        }
        for index in 0..64 {
            g1[index + 128] = index as i32 * 262144 + Colour::CYAN;
        }
        for index in 0..64 {
            g1[index + 192] = Colour::WHITE;
        }

        let mut g2 = vec![0i32; 256];
        for (index, slot) in g2.iter_mut().enumerate().take(64) {
            *slot = index as i32 * 4;
        }
        for index in 0..64 {
            g2[index + 64] = index as i32 * 262144 + Colour::BLUE;
        }
        for index in 0..64 {
            g2[index + 128] = index as i32 * 1024 + Colour::MAGENTA;
        }
        for index in 0..64 {
            g2[index + 192] = Colour::WHITE;
        }

        self.flame_gradient0 = g0;
        self.flame_gradient1 = g1;
        self.flame_gradient2 = g2;
        self.flame_gradient = vec![0; 256];
        self.flame_buffer0 = vec![0; 32768];
        self.flame_buffer1 = vec![0; 32768];
        self.generate_flame_cooling_map(None);
        self.flame_buffer3 = vec![0; 32768];
        self.flame_buffer2 = vec![0; 32768];
    }

    pub fn start(&mut self) {
        self.active = true;
    }

    pub fn close(&mut self) {
        self.active = false;
        self.flame_left = None;
        self.flame_right = None;
        self.flame_gradient.clear();
        self.flame_gradient0.clear();
        self.flame_gradient1.clear();
        self.flame_gradient2.clear();
        self.flame_buffer0.clear();
        self.flame_buffer1.clear();
        self.flame_buffer3.clear();
        self.flame_buffer2.clear();
    }

    pub fn render_flames(
        &mut self,
        title_left: &mut PixMap,
        title_right: &mut PixMap,
        loop_cycle: i32,
    ) {
        if !self.active {
            return;
        }
        self.cycle += 1;
        self.update_flames(loop_cycle);
        self.update_flames(loop_cycle);
        self.draw_flames(title_left, title_right);
    }

    fn rand(&mut self, max: i32) -> i32 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.rng >> 16) as i32).rem_euclid(max)
    }

    fn update_flames(&mut self, loop_cycle: i32) {
        if self.flame_buffer3.is_empty()
            || self.flame_buffer2.is_empty()
            || self.flame_buffer0.is_empty()
        {
            return;
        }

        for x in 10..117 {
            let rand = self.rand(100);
            if rand < 50 {
                let i = (x + ((FLAME_HEIGHT - 2) << 7)) as usize;
                if i < self.flame_buffer3.len() {
                    self.flame_buffer3[i] = 255;
                }
            }
        }

        for _ in 0..100 {
            let x = self.rand(124) + 2;
            let y = self.rand(128) + 128;
            let index = (x + (y << 7)) as usize;
            if index < self.flame_buffer3.len() {
                self.flame_buffer3[index] = 192;
            }
        }

        for y in 1..FLAME_HEIGHT - 1 {
            for x in 1..127 {
                let index = (x + (y << 7)) as usize;
                self.flame_buffer2[index] = (self.flame_buffer3[index - 1]
                    + self.flame_buffer3[index + 1]
                    + self.flame_buffer3[index - 128]
                    + self.flame_buffer3[index + 128])
                    / 4;
            }
        }

        self.cooling_cycle += 128;
        if self.cooling_cycle > self.flame_buffer0.len() as i32 {
            self.cooling_cycle -= self.flame_buffer0.len() as i32;
            let rune = if self.runes.is_empty() {
                None
            } else {
                let i = self.rand(self.runes.len() as i32) as usize;
                Some(self.runes[i].clone())
            };
            self.generate_flame_cooling_map(rune.as_ref());
        }

        let mask = (self.flame_buffer0.len() - 1) as i32;
        for y in 1..FLAME_HEIGHT - 1 {
            for x in 1..127 {
                let index = (x + (y << 7)) as usize;
                let mut intensity = self.flame_buffer2[index + 128]
                    - (self.flame_buffer0[((index as i32 + self.cooling_cycle) & mask) as usize]
                        / 5);
                if intensity < 0 {
                    intensity = 0;
                }
                self.flame_buffer3[index] = intensity;
            }
        }

        self.flame_line_offset.copy_within(1.., 0);
        let last = FLAME_HEIGHT as usize - 1;
        self.flame_line_offset[last] = ((loop_cycle as f64 / 14.0).sin() * 16.0
            + (loop_cycle as f64 / 15.0).sin() * 14.0
            + (loop_cycle as f64 / 16.0).sin() * 12.0)
            as i32;

        if self.flame_gradient_cycle0 > 0 {
            self.flame_gradient_cycle0 -= 4;
        }
        if self.flame_gradient_cycle1 > 0 {
            self.flame_gradient_cycle1 -= 4;
        }
        if self.flame_gradient_cycle0 == 0 && self.flame_gradient_cycle1 == 0 {
            let rand = self.rand(2000);
            if rand == 0 {
                self.flame_gradient_cycle0 = 1024;
            } else if rand == 1 {
                self.flame_gradient_cycle1 = 1024;
            }
        }
    }

    fn generate_flame_cooling_map(&mut self, image: Option<&Pix8>) {
        if self.flame_buffer0.is_empty() || self.flame_buffer1.is_empty() {
            return;
        }
        self.flame_buffer0.fill(0);
        for _ in 0..5000 {
            let rand = self.rand(FLAME_WIDTH * FLAME_HEIGHT) as usize;
            if rand < self.flame_buffer0.len() {
                self.flame_buffer0[rand] = self.rand(256);
            }
        }
        for _ in 0..20 {
            for y in 1..FLAME_HEIGHT - 1 {
                for x in 1..127 {
                    let index = (x + (y << 7)) as usize;
                    self.flame_buffer1[index] = (self.flame_buffer0[index - 1]
                        + self.flame_buffer0[index + 1]
                        + self.flame_buffer0[index - 128]
                        + self.flame_buffer0[index + 128])
                        / 4;
                }
            }
            std::mem::swap(&mut self.flame_buffer0, &mut self.flame_buffer1);
        }

        if let Some(image) = image {
            let mut off = 0usize;
            for y in 0..image.hi {
                for x in 0..image.wi {
                    let pix = image.data.get(off).copied().unwrap_or(0);
                    off += 1;
                    if pix != 0 {
                        let x0 = x + image.xof + 16;
                        let y0 = y + image.yof + 16;
                        let index = (x0 + (y0 << 7)) as usize;
                        if index < self.flame_buffer0.len() {
                            self.flame_buffer0[index] = 0;
                        }
                    }
                }
            }
        }
    }

    fn draw_flames(&mut self, title_left: &mut PixMap, title_right: &mut PixMap) {
        if self.flame_gradient.is_empty()
            || self.flame_gradient0.is_empty()
            || self.flame_left.is_none()
            || self.flame_right.is_none()
            || self.flame_buffer3.is_empty()
        {
            return;
        }
        if self.flame_gradient_cycle0 > 0 {
            let cycle = self.flame_gradient_cycle0;
            let target = self.flame_gradient1.clone();
            self.do_blend(cycle, &target);
        } else if self.flame_gradient_cycle1 > 0 {
            let cycle = self.flame_gradient_cycle1;
            let target = self.flame_gradient2.clone();
            self.do_blend(cycle, &target);
        } else {
            self.flame_gradient.copy_from_slice(&self.flame_gradient0);
        }

        if let Some(base) = self.flame_left.as_ref() {
            Self::draw_single_flame(
                title_left,
                base,
                0,
                &self.flame_gradient,
                &self.flame_buffer3,
                &self.flame_line_offset,
            );
        }
        if let Some(base) = self.flame_right.as_ref() {
            Self::draw_single_flame(
                title_right,
                base,
                1,
                &self.flame_gradient,
                &self.flame_buffer3,
                &self.flame_line_offset,
            );
        }
    }

    fn draw_single_flame(
        title: &mut PixMap,
        base: &Pix32,
        side: i32,
        flame_gradient: &[i32],
        flame_buffer3: &[i32],
        flame_line_offset: &[i32],
    ) {
        let n = TITLE_FLAME_PIXELS
            .min(base.data.len())
            .min(title.pixels.len());
        title.pixels[..n].copy_from_slice(&base.data[..n]);

        let mut src_offset = 0i32;
        let mut dst_offset = if side == 0 { 1152 } else { 1176 };

        for y in 1..FLAME_HEIGHT - 1 {
            let offset = (flame_line_offset[y as usize] * (FLAME_HEIGHT - y)) / FLAME_HEIGHT;
            if side == 0 {
                let mut step = offset + 22;
                if step < 0 {
                    step = 0;
                }
                src_offset += step;
                for _x in step..FLAME_WIDTH {
                    dst_offset = Self::blend_pixel(
                        title,
                        src_offset,
                        dst_offset,
                        flame_gradient,
                        flame_buffer3,
                    );
                    src_offset += 1;
                }
                dst_offset += step;
            } else {
                let step = 103 - offset;
                dst_offset += offset;
                for _x in 0..step {
                    dst_offset = Self::blend_pixel(
                        title,
                        src_offset,
                        dst_offset,
                        flame_gradient,
                        flame_buffer3,
                    );
                    src_offset += 1;
                }
                src_offset += FLAME_WIDTH - step;
                dst_offset += FLAME_WIDTH - step - offset;
            }
        }
    }

    fn do_blend(&mut self, cycle: i32, target: &[i32]) {
        for (slot, (&g0, &t)) in self
            .flame_gradient
            .iter_mut()
            .zip(self.flame_gradient0.iter().zip(target.iter()))
            .take(256)
        {
            *slot = if cycle > 768 {
                Self::merge(g0, t, 1024 - cycle)
            } else if cycle > 256 {
                t
            } else {
                Self::merge(t, g0, 256 - cycle)
            };
        }
    }

    fn merge(src: i32, dst: i32, alpha: i32) -> i32 {
        let inv_alpha = 256 - alpha;
        ((((src & 0xff00ff)
            .wrapping_mul(inv_alpha)
            .wrapping_add((dst & 0xff00ff).wrapping_mul(alpha)))
            & 0xff00_ff00u32 as i32)
            + (((src & 0xff00).wrapping_mul(inv_alpha) + (dst & 0xff00).wrapping_mul(alpha))
                & 0xff0000))
            >> 8
    }

    fn blend_pixel(
        title: &mut PixMap,
        src_offset: i32,
        dst_offset: i32,
        flame_gradient: &[i32],
        flame_buffer3: &[i32],
    ) -> i32 {
        let src = src_offset as usize;
        let dst = dst_offset as usize;
        if src >= flame_buffer3.len() || dst >= title.pixels.len() {
            return dst_offset + 1;
        }
        let mut value = flame_buffer3[src];
        if value == 0 {
            return dst_offset + 1;
        }
        let alpha = value;
        let inv_alpha = 256 - value;
        value = flame_gradient.get(value as usize).copied().unwrap_or(0);
        let background = title.pixels[dst];
        title.pixels[dst] = ((((value & 0xff00ff)
            .wrapping_mul(alpha)
            .wrapping_add((background & 0xff00ff).wrapping_mul(inv_alpha)))
            & 0xff00_ff00u32 as i32)
            + (((value & 0xff00).wrapping_mul(alpha)
                + (background & 0xff00).wrapping_mul(inv_alpha))
                & 0xff0000))
            >> 8;
        dst_offset + 1
    }
}
