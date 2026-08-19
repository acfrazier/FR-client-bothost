// Port of `~/experiments/Server/webclient/src/util/JavaRandom.ts`: a faithful
// reimplementation of `java.util.Random` using 16-bit limbs (53-bit safe, same
// output sequence as Java for the same seed).

const C0: i64 = 0xe66d; // low limb multiplier (0x5DEECE66D = 0x0005_deec_e66d)
const C1: i64 = 0xdeec;
const C2: i64 = 0x0005;

pub struct JavaRandom {
    s0: i64,
    s1: i64,
    s2: i64,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        let mut rng = JavaRandom {
            s0: 0,
            s1: 0,
            s2: 0,
        };
        rng.set_seed(seed);
        rng
    }

    /// Time-seeded instance (TS `new JavaRandom(Date.now())`).
    pub fn now() -> Self {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self::new(millis)
    }

    pub fn set_seed(&mut self, seed: i64) {
        // seed ^ 0x5DEECE66D, split across three 16-bit limbs
        self.s0 = (seed & 0xffff) ^ C0;
        self.s1 = ((seed >> 16) & 0xffff) ^ C1;
        self.s2 = ((seed >> 32) & 0xffff) ^ C2;
    }

    /// Advances the 48-bit LCG and returns the next 32 bits (unsigned).
    fn next(&mut self) -> i64 {
        let mut carry: i64 = 0xb;
        let mut r0 = self.s0 * C0 + carry;
        carry = r0 >> 16;
        r0 &= 0xffff;
        let mut r1 = self.s1 * C0 + self.s0 * C1 + carry;
        carry = r1 >> 16;
        r1 &= 0xffff;
        let r2 = (self.s2 * C0 + self.s1 * C1 + self.s0 * C2 + carry) & 0xffff;
        self.s2 = r2;
        self.s1 = r1;
        self.s0 = r0;
        self.s2 * 65536 + self.s1
    }

    /// TS `nextInt()` without a bound: signed 32-bit draw.
    pub fn next_int(&mut self) -> i32 {
        self.next() as i32
    }

    /// TS `nextInt(bound)`. The rejection loop in the TS never fires (doubles
    /// cannot overflow like Java ints), so this is a straight modulo.
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        if (bound & bound.wrapping_neg()) == bound {
            let r = self.next() >> 1;
            return ((r * bound as i64) >> 31) as i32;
        }
        let bits = self.next() >> 1;
        (bits % bound as i64) as i32
    }
}
