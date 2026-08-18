// Port of `~/experiments/Server/webclient/src/io/Isaac.ts` with i32 wrapping:
// JS `+`/`<<` become `wrapping_add`/`wrapping_shl`, `>>>` becomes a logical
// shift on the bit pattern (`(x as u32) >> n) as i32`).
pub struct Isaac {
    count: i32,
    rsl: [i32; 256],
    mem: [i32; 256],
    a: i32,
    b: i32,
    c: i32,
}

impl Isaac {
    pub fn new(seed: &[i32]) -> Self {
        let mut rng = Isaac {
            count: 0,
            rsl: [0; 256],
            mem: [0; 256],
            a: 0,
            b: 0,
            c: 0,
        };
        for (i, &s) in seed.iter().take(256).enumerate() {
            rng.rsl[i] = s;
        }
        rng.init();
        rng
    }

    pub fn next_int(&mut self) -> i32 {
        if self.count == 0 {
            self.isaac();
            self.count = 255;
        } else {
            self.count -= 1;
        }
        self.rsl[self.count as usize]
    }

    fn init(&mut self) {
        let mut a = -0x61c8_8647i32;
        let mut b = -0x61c8_8647i32;
        let mut c = -0x61c8_8647i32;
        let mut d = -0x61c8_8647i32;
        let mut e = -0x61c8_8647i32;
        let mut f = -0x61c8_8647i32;
        let mut g = -0x61c8_8647i32;
        let mut h = -0x61c8_8647i32;

        for _ in 0..4 {
            a ^= b.wrapping_shl(11);
            d = d.wrapping_add(a);
            b = b.wrapping_add(c);
            b ^= ((c as u32) >> 2) as i32;
            e = e.wrapping_add(b);
            c = c.wrapping_add(d);
            c ^= d.wrapping_shl(8);
            f = f.wrapping_add(c);
            d = d.wrapping_add(e);
            d ^= ((e as u32) >> 16) as i32;
            g = g.wrapping_add(d);
            e = e.wrapping_add(f);
            e ^= f.wrapping_shl(10);
            h = h.wrapping_add(e);
            f = f.wrapping_add(g);
            f ^= ((g as u32) >> 4) as i32;
            a = a.wrapping_add(f);
            g = g.wrapping_add(h);
            g ^= h.wrapping_shl(8);
            b = b.wrapping_add(g);
            h = h.wrapping_add(a);
            h ^= ((a as u32) >> 9) as i32;
            c = c.wrapping_add(h);
            a = a.wrapping_add(b);
        }

        for i in (0..256).step_by(8) {
            a = a.wrapping_add(self.rsl[i]);
            b = b.wrapping_add(self.rsl[i + 1]);
            c = c.wrapping_add(self.rsl[i + 2]);
            d = d.wrapping_add(self.rsl[i + 3]);
            e = e.wrapping_add(self.rsl[i + 4]);
            f = f.wrapping_add(self.rsl[i + 5]);
            g = g.wrapping_add(self.rsl[i + 6]);
            h = h.wrapping_add(self.rsl[i + 7]);

            a ^= b.wrapping_shl(11);
            d = d.wrapping_add(a);
            b = b.wrapping_add(c);
            b ^= ((c as u32) >> 2) as i32;
            e = e.wrapping_add(b);
            c = c.wrapping_add(d);
            c ^= d.wrapping_shl(8);
            f = f.wrapping_add(c);
            d = d.wrapping_add(e);
            d ^= ((e as u32) >> 16) as i32;
            g = g.wrapping_add(d);
            e = e.wrapping_add(f);
            e ^= f.wrapping_shl(10);
            h = h.wrapping_add(e);
            f = f.wrapping_add(g);
            f ^= ((g as u32) >> 4) as i32;
            a = a.wrapping_add(f);
            g = g.wrapping_add(h);
            g ^= h.wrapping_shl(8);
            b = b.wrapping_add(g);
            h = h.wrapping_add(a);
            h ^= ((a as u32) >> 9) as i32;
            c = c.wrapping_add(h);
            a = a.wrapping_add(b);

            self.mem[i] = a;
            self.mem[i + 1] = b;
            self.mem[i + 2] = c;
            self.mem[i + 3] = d;
            self.mem[i + 4] = e;
            self.mem[i + 5] = f;
            self.mem[i + 6] = g;
            self.mem[i + 7] = h;
        }

        for i in (0..256).step_by(8) {
            a = a.wrapping_add(self.mem[i]);
            b = b.wrapping_add(self.mem[i + 1]);
            c = c.wrapping_add(self.mem[i + 2]);
            d = d.wrapping_add(self.mem[i + 3]);
            e = e.wrapping_add(self.mem[i + 4]);
            f = f.wrapping_add(self.mem[i + 5]);
            g = g.wrapping_add(self.mem[i + 6]);
            h = h.wrapping_add(self.mem[i + 7]);

            a ^= b.wrapping_shl(11);
            d = d.wrapping_add(a);
            b = b.wrapping_add(c);
            b ^= ((c as u32) >> 2) as i32;
            e = e.wrapping_add(b);
            c = c.wrapping_add(d);
            c ^= d.wrapping_shl(8);
            f = f.wrapping_add(c);
            d = d.wrapping_add(e);
            d ^= ((e as u32) >> 16) as i32;
            g = g.wrapping_add(d);
            e = e.wrapping_add(f);
            e ^= f.wrapping_shl(10);
            h = h.wrapping_add(e);
            f = f.wrapping_add(g);
            f ^= ((g as u32) >> 4) as i32;
            a = a.wrapping_add(f);
            g = g.wrapping_add(h);
            g ^= h.wrapping_shl(8);
            b = b.wrapping_add(g);
            h = h.wrapping_add(a);
            h ^= ((a as u32) >> 9) as i32;
            c = c.wrapping_add(h);
            a = a.wrapping_add(b);

            self.mem[i] = a;
            self.mem[i + 1] = b;
            self.mem[i + 2] = c;
            self.mem[i + 3] = d;
            self.mem[i + 4] = e;
            self.mem[i + 5] = f;
            self.mem[i + 6] = g;
            self.mem[i + 7] = h;
        }

        self.isaac();
        self.count = 256;
    }

    fn isaac(&mut self) {
        self.c = self.c.wrapping_add(1);
        self.b = self.b.wrapping_add(self.c);

        for i in 0..256 {
            let x = self.mem[i];

            match i & 3 {
                0 => self.a ^= self.a.wrapping_shl(13),
                1 => self.a ^= ((self.a as u32) >> 6) as i32,
                2 => self.a ^= self.a.wrapping_shl(2),
                _ => self.a ^= ((self.a as u32) >> 16) as i32,
            }

            self.a = self.a.wrapping_add(self.mem[(i + 128) & 0xff]);

            let y = self.mem[((x as u32) >> 2) as usize & 0xff]
                .wrapping_add(self.a)
                .wrapping_add(self.b);
            self.mem[i] = y;
            self.b = self.mem[(((y as u32) >> 8) >> 2) as usize & 0xff].wrapping_add(x);
            self.rsl[i] = self.b;
        }
    }
}
