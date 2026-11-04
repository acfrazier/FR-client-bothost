//! Client-Java 377 `config/FloType.java`: defaults 9-43, opcodes 64-96,
//! HSL conversion and random jitter 100-180, packed HSL 184-197.
//! Opcode 7 intentionally restores chroma from the saved underlay hue.

use super::{DecodeError377, Reader377};

#[derive(Debug, PartialEq, Eq)]
pub struct FloType377 {
    pub colour: i32,
    pub texture: i32,
    pub overlay: bool,
    pub occlude: bool,
    pub debugname: Option<String>,
    pub hue: i32,
    pub saturation: i32,
    pub lightness: i32,
    pub underlay_hue: i32,
    pub chroma: i32,
    pub overlay_hsl: i32,
}

impl Default for FloType377 {
    fn default() -> Self {
        Self {
            colour: 0,
            texture: -1,
            overlay: false,
            occlude: true,
            debugname: None,
            hue: 0,
            saturation: 0,
            lightness: 0,
            underlay_hue: 0,
            chroma: 0,
            overlay_hsl: 0,
        }
    }
}

impl FloType377 {
    /// Supply Java-compatible [0,1) draws. Cache377 uses the existing JavaRandom;
    /// conformance tests can supply identical draws to the Java oracle.
    pub fn decode(
        dat: &mut Reader377<'_>,
        random: &mut impl FnMut() -> f64,
    ) -> Result<Self, DecodeError377> {
        let mut flo = Self::default();
        loop {
            let code = dat.g1()?;
            match code {
                0 => return Ok(flo),
                1 => {
                    flo.colour = dat.g3()?;
                    flo.get_hsl(flo.colour, random);
                }
                2 => flo.texture = dat.g1()?,
                3 => flo.overlay = true,
                5 => flo.occlude = false,
                6 => flo.debugname = Some(dat.gjstr()?),
                7 => {
                    let saved = (flo.hue, flo.saturation, flo.lightness, flo.underlay_hue);
                    flo.get_hsl(dat.g3()?, random);
                    (flo.hue, flo.saturation, flo.lightness, flo.underlay_hue) = saved;
                    flo.chroma = saved.3;
                }
                _ => return Err(dat.unknown(code)),
            }
        }
    }

    fn get_hsl(&mut self, rgb: i32, random: &mut impl FnMut() -> f64) {
        let rgb = if rgb == 0xff00ff { 0 } else { rgb };
        let red = f64::from((rgb >> 16) & 0xff) / 256.0;
        let green = f64::from((rgb >> 8) & 0xff) / 256.0;
        let blue = f64::from(rgb & 0xff) / 256.0;
        let min = red.min(green).min(blue);
        let max = red.max(green).max(blue);
        let mut h = 0.0;
        let mut s = 0.0;
        let l = (min + max) / 2.0;
        if min != max {
            s = if l < 0.5 {
                (max - min) / (min + max)
            } else {
                (max - min) / (2.0 - max - min)
            };
            if red == max {
                h = (green - blue) / (max - min);
            } else if green == max {
                h = (blue - red) / (max - min) + 2.0;
            } else if blue == max {
                h = (red - green) / (max - min) + 4.0;
            }
        }
        h /= 6.0;
        self.hue = (h * 256.0) as i32;
        self.saturation = ((s * 256.0) as i32).clamp(0, 255);
        self.lightness = ((l * 256.0) as i32).clamp(0, 255);
        self.chroma = (if l > 0.5 {
            (1.0 - l) * s * 512.0
        } else {
            s * l * 512.0
        }) as i32;
        self.chroma = self.chroma.max(1);
        self.underlay_hue = (f64::from(self.chroma) * h) as i32;
        let hue = (self.hue + (random() * 16.0) as i32 - 8).clamp(0, 255);
        let saturation = (self.saturation + (random() * 48.0) as i32 - 24).clamp(0, 255);
        let lightness = (self.lightness + (random() * 48.0) as i32 - 24).clamp(0, 255);
        self.overlay_hsl = Self::get_table(hue, saturation, lightness);
    }

    pub fn get_table(hue: i32, mut saturation: i32, lightness: i32) -> i32 {
        if lightness > 179 {
            saturation /= 2;
        }
        if lightness > 192 {
            saturation /= 2;
        }
        if lightness > 217 {
            saturation /= 2;
        }
        if lightness > 243 {
            saturation /= 2;
        }
        lightness / 2 + ((hue / 4) << 10) + ((saturation / 32) << 7)
    }
}
