// Port of `~/experiments/Server/webclient/src/config/FloType.ts`. `getHsl`
// uses JS `Math.random()` jitter for `overlayHsl`; a small deterministic
// xorshift64 stands in so repeated unpacks agree.
use crate::io::{JagFile, Packet};

/// Stand-in for JS `Math.random()` (returns `[0, 1)`).
struct Random(u64);

impl Random {
    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub struct FloType {
    pub colour: i32,
    pub texture: i32,
    pub overlay: bool,
    pub occlude: bool,
    pub debugname: String,

    pub hue: i32,
    pub saturation: i32,
    pub lightness: i32,

    pub chroma: i32,
    pub underlay_hue: i32,
    pub overlay_hsl: i32,
}

impl Default for FloType {
    fn default() -> Self {
        FloType {
            colour: 0,
            texture: -1,
            overlay: false,
            occlude: true,
            debugname: String::new(),
            hue: 0,
            saturation: 0,
            lightness: 0,
            chroma: 0,
            underlay_hue: 0,
            overlay_hsl: 0,
        }
    }
}

impl FloType {
    pub fn unpack(jag: &JagFile) -> Vec<FloType> {
        let Some(data) = jag.read("flo.dat") else {
            return Vec::new();
        };
        let mut dat = Packet::new(data);
        let num = dat.g2();
        let mut rng = Random(0x9e37_79b9_7f4a_7c15);
        let mut list = Vec::with_capacity(num as usize);
        for _ in 0..num {
            let mut flo = FloType::default();
            flo.decode(&mut dat, &mut rng);
            list.push(flo);
        }
        list
    }

    fn decode(&mut self, dat: &mut Packet, rng: &mut Random) {
        loop {
            let code = dat.g1();
            if code == 0 {
                break;
            }
            match code {
                1 => {
                    self.colour = dat.g3();
                    self.get_hsl(self.colour, rng);
                }
                2 => self.texture = dat.g1(),
                3 => self.overlay = true,
                5 => self.occlude = false,
                6 => self.debugname = dat.gjstr(),
                _ => eprintln!("Error unrecognised flo config code: {code}"),
            }
        }
    }

    fn get_hsl(&mut self, rgb: i32, rng: &mut Random) {
        let red = ((rgb >> 16) & 0xff) as f64 / 256.0;
        let green = ((rgb >> 8) & 0xff) as f64 / 256.0;
        let blue = (rgb & 0xff) as f64 / 256.0;

        let mut min = red;
        if green < red {
            min = green;
        }
        if blue < min {
            min = blue;
        }

        let mut max = red;
        if green > red {
            max = green;
        }
        if blue > max {
            max = blue;
        }

        let mut h = 0.0;
        let mut s = 0.0;
        let l = (min + max) / 2.0;

        if min != max {
            if l < 0.5 {
                s = (max - min) / (max + min);
            }
            if l >= 0.5 {
                s = (max - min) / (2.0 - max - min);
            }

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
        self.saturation = (s * 256.0) as i32;
        self.lightness = (l * 256.0) as i32;

        self.saturation = self.saturation.clamp(0, 255);
        self.lightness = self.lightness.clamp(0, 255);

        if l > 0.5 {
            self.chroma = ((1.0 - l) * s * 512.0) as i32;
        } else {
            self.chroma = (l * s * 512.0) as i32;
        }

        if self.chroma < 1 {
            self.chroma = 1;
        }

        self.underlay_hue = (h * self.chroma as f64) as i32;

        let hue = (self.hue + (rng.next() * 16.0) as i32 - 8).clamp(0, 255);
        let saturation = (self.saturation + (rng.next() * 48.0) as i32 - 24).clamp(0, 255);
        let lightness = (self.lightness + (rng.next() * 48.0) as i32 - 24).clamp(0, 255);

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
        ((hue / 4) << 10) + ((saturation / 32) << 7) + (lightness / 2)
    }
}
