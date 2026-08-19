// Port of `~/experiments/Server/webclient/src/dash3d/Pix3D.ts`, tables only.
// The trig/divide tables and the 65536-entry colour table are immutable after
// init, so they are process-wide `OnceLock`s (design: "Immutable tables ...
// process-wide OnceLock"). Per-frame draw state that was static on Pix3D
// (`scanline`, `originX/Y`, `trans`, texture pool) is out of this task's scope
// and will live on `Client`.

use std::sync::OnceLock;

pub struct Pix3D;

struct TrigTables {
    div_table: [i32; 512],
    div_table2: [i32; 2048],
    sin_table: [i32; 2048],
    cos_table: [i32; 2048],
}

static TRIG: OnceLock<TrigTables> = OnceLock::new();
static COLOUR_TABLE: OnceLock<[i32; 65536]> = OnceLock::new();

fn trig() -> &'static TrigTables {
    TRIG.get_or_init(|| {
        let mut div_table = [0i32; 512];
        for i in 1..512 {
            div_table[i] = 32768 / i as i32;
        }
        let mut div_table2 = [0i32; 2048];
        for i in 1..2048 {
            div_table2[i] = 65536 / i as i32;
        }
        let mut sin_table = [0i32; 2048];
        let mut cos_table = [0i32; 2048];
        for i in 0..2048 {
            // angular frequency: 2 * pi / 2048 = 0.0030679615757712823
            // * 65536 = maximum amplitude
            sin_table[i] = (f64::sin(i as f64 * 0.0030679615757712823) * 65536.0) as i32;
            cos_table[i] = (f64::cos(i as f64 * 0.0030679615757712823) * 65536.0) as i32;
        }
        TrigTables {
            div_table,
            div_table2,
            sin_table,
            cos_table,
        }
    })
}

impl Pix3D {
    pub fn div_table() -> &'static [i32; 512] {
        &trig().div_table
    }

    pub fn div_table2() -> &'static [i32; 2048] {
        &trig().div_table2
    }

    pub fn sin_table() -> &'static [i32; 2048] {
        &trig().sin_table
    }

    pub fn cos_table() -> &'static [i32; 2048] {
        &trig().cos_table
    }

    /// Builds the 65536-entry HSL→RGB colour table once. The TS adds
    /// `Math.random() * 0.03 - 0.015` jitter to `brightness`; this port keeps
    /// the table deterministic (deviation documented).
    pub fn init_colour_table(brightness: f64) {
        let _ = COLOUR_TABLE.get_or_init(|| build_colour_table(brightness));
    }

    pub fn colour_table() -> &'static [i32; 65536] {
        COLOUR_TABLE
            .get()
            .expect("Pix3D::init_colour_table must be called before colour_table()")
    }

    fn gamma_correct(rgb: i32, gamma: f64) -> i32 {
        let r = (rgb >> 16) as f64 / 256.0;
        let g = ((rgb >> 8) & 0xff) as f64 / 256.0;
        let b = (rgb & 0xff) as f64 / 256.0;

        let pow_r = r.powf(gamma);
        let pow_g = g.powf(gamma);
        let pow_b = b.powf(gamma);

        let int_r = (pow_r * 256.0) as i32;
        let int_g = (pow_g * 256.0) as i32;
        let int_b = (pow_b * 256.0) as i32;
        (int_r << 16) + (int_g << 8) + int_b
    }
}

fn build_colour_table(brightness: f64) -> [i32; 65536] {
    let mut table = [0i32; 65536];
    let mut offset = 0;
    for y in 0..512 {
        let hue = ((y / 8) as f64) / 64.0 + 0.0078125;
        let saturation = ((y & 0x7) as f64) / 8.0 + 0.0625;
        for x in 0..128 {
            let lightness = x as f64 / 128.0;
            let mut r = lightness;
            let mut g = lightness;
            let mut b = lightness;

            if saturation != 0.0 {
                let q = if lightness < 0.5 {
                    lightness * (saturation + 1.0)
                } else {
                    lightness + saturation - lightness * saturation
                };
                let p = lightness * 2.0 - q;
                let mut t = hue + 0.3333333333333333;
                if t > 1.0 {
                    t -= 1.0;
                }
                let mut d11 = hue - 0.3333333333333333;
                if d11 < 0.0 {
                    d11 += 1.0;
                }

                if t * 6.0 < 1.0 {
                    r = p + (q - p) * 6.0 * t;
                } else if t * 2.0 < 1.0 {
                    r = q;
                } else if t * 3.0 < 2.0 {
                    r = p + (q - p) * (0.6666666666666666 - t) * 6.0;
                } else {
                    r = p;
                }

                if hue * 6.0 < 1.0 {
                    g = p + (q - p) * 6.0 * hue;
                } else if hue * 2.0 < 1.0 {
                    g = q;
                } else if hue * 3.0 < 2.0 {
                    g = p + (q - p) * (0.6666666666666666 - hue) * 6.0;
                } else {
                    g = p;
                }

                if d11 * 6.0 < 1.0 {
                    b = p + (q - p) * 6.0 * d11;
                } else if d11 * 2.0 < 1.0 {
                    b = q;
                } else if d11 * 3.0 < 2.0 {
                    b = p + (q - p) * (0.6666666666666666 - d11) * 6.0;
                } else {
                    b = p;
                }
            }

            let int_r = (r * 256.0) as i32;
            let int_g = (g * 256.0) as i32;
            let int_b = (b * 256.0) as i32;
            let rgb = (int_r << 16) + (int_g << 8) + int_b;
            table[offset] = Pix3D::gamma_correct(rgb, brightness);
            offset += 1;
        }
    }
    table
}
