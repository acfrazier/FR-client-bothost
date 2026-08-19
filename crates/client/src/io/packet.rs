// Port of `~/experiments/Server/webclient/src/io/Packet.ts` lines 1–275
// (p1Enc and rsaenc are ported separately). CRC is bit-identical to the TS
// table; all value math uses i32 wrapping like JS `| 0`.
use std::cell::RefCell;

use num_bigint::BigUint;

use super::isaac::Isaac;

const CRC32_POLYNOMIAL: u32 = 0xedb8_8320;

const BITMASK: [i32; 33] = {
    let mut m = [0i32; 33];
    let mut i = 0;
    while i < 32 {
        m[i] = (1i32 << i).wrapping_sub(1);
        i += 1;
    }
    m[32] = -1;
    m
};

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut remainder = i as u32;
        let mut bit = 0;
        while bit < 8 {
            if remainder & 1 == 1 {
                remainder = (remainder >> 1) ^ CRC32_POLYNOMIAL;
            } else {
                remainder >>= 1;
            }
            bit += 1;
        }
        table[i] = remainder;
        i += 1;
    }
    table
};

// Packet pool: thread-local lists for now (moved onto Client in Task 10).
thread_local! {
    static CACHE_MIN: RefCell<Vec<Packet>> = const { RefCell::new(Vec::new()) };
    static CACHE_MID: RefCell<Vec<Packet>> = const { RefCell::new(Vec::new()) };
    static CACHE_MAX: RefCell<Vec<Packet>> = const { RefCell::new(Vec::new()) };
}

pub struct Packet {
    data: Vec<u8>,
    pub pos: usize,
    pub bit_pos: usize,
    pub random: Option<Isaac>,
}

impl Packet {
    pub fn new(data: Vec<u8>) -> Self {
        Packet {
            data,
            pos: 0,
            bit_pos: 0,
            random: None,
        }
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn length(&self) -> usize {
        self.data.len()
    }

    pub fn available(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn getcrc(src: &[u8], offset: usize, length: usize) -> i32 {
        let mut crc = 0xffff_ffffu32;
        for i in offset..length {
            crc = (crc >> 8) ^ CRC_TABLE[((crc ^ src[i] as u32) & 0xff) as usize];
        }
        (!crc) as i32
    }

    pub fn checkcrc(src: &[u8], offset: usize, length: usize, expected: i32) -> bool {
        Self::getcrc(src, offset, length) == expected
    }

    pub fn alloc(kind: i32) -> Packet {
        let cached = match kind {
            0 => CACHE_MIN.with(|c| c.borrow_mut().pop()),
            1 => CACHE_MID.with(|c| c.borrow_mut().pop()),
            _ => CACHE_MAX.with(|c| c.borrow_mut().pop()),
        };
        if let Some(mut p) = cached {
            p.pos = 0;
            return p;
        }
        match kind {
            0 => Packet::new(vec![0; 100]),
            1 => Packet::new(vec![0; 5000]),
            _ => Packet::new(vec![0; 30000]),
        }
    }

    pub fn release(self) {
        match self.data.len() {
            100 => CACHE_MIN.with(|c| {
                let mut cache = c.borrow_mut();
                if cache.len() < 1000 {
                    cache.push(self);
                }
            }),
            5000 => CACHE_MID.with(|c| {
                let mut cache = c.borrow_mut();
                if cache.len() < 250 {
                    cache.push(self);
                }
            }),
            30000 => CACHE_MAX.with(|c| {
                let mut cache = c.borrow_mut();
                if cache.len() < 50 {
                    cache.push(self);
                }
            }),
            _ => {}
        }
    }

    pub fn g1(&mut self) -> i32 {
        let v = self.data[self.pos] as i32;
        self.pos += 1;
        v
    }

    // signed
    pub fn g1b(&mut self) -> i32 {
        let v = self.data[self.pos] as i8 as i32;
        self.pos += 1;
        v
    }

    pub fn g2(&mut self) -> i32 {
        let v = u16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]) as i32;
        self.pos += 2;
        v
    }

    // signed
    pub fn g2b(&mut self) -> i32 {
        let v = i16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]) as i32;
        self.pos += 2;
        v
    }

    pub fn g3(&mut self) -> i32 {
        let v = (self.data[self.pos] as i32) << 16
            | u16::from_be_bytes([self.data[self.pos + 1], self.data[self.pos + 2]]) as i32;
        self.pos += 3;
        v
    }

    pub fn g4(&mut self) -> i32 {
        let v = i32::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        v
    }

    pub fn g8(&mut self) -> i64 {
        let v = i64::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ]);
        self.pos += 8;
        v
    }

    pub fn gsmarts(&mut self) -> i32 {
        if self.data[self.pos] < 0x80 {
            self.g1() - 0x40
        } else {
            self.g2() - 0xc000
        }
    }

    pub fn gsmart(&mut self) -> i32 {
        if self.data[self.pos] < 0x80 {
            self.g1()
        } else {
            self.g2() - 0x8000
        }
    }

    pub fn gjstr(&mut self) -> String {
        let mut s = String::new();
        while self.pos < self.data.len() {
            let b = self.data[self.pos];
            self.pos += 1;
            if b == 10 {
                break;
            }
            s.push(b as char);
        }
        s
    }

    pub fn gdata(&mut self, length: usize, offset: usize, dest: &mut [u8]) {
        let end = self.pos + length;
        dest[offset..offset + length].copy_from_slice(&self.data[self.pos..end]);
        self.pos = end;
    }

    pub fn p1_enc(&mut self, opcode: i32) {
        let r = self.random.as_mut().map(|r| r.next_int()).unwrap_or(0);
        self.data[self.pos] = opcode.wrapping_add(r) as u8;
        self.pos += 1;
    }

    pub fn p1(&mut self, value: i32) {
        self.data[self.pos] = value as u8;
        self.pos += 1;
    }

    pub fn p2(&mut self, value: i32) {
        self.data[self.pos] = (value >> 8) as u8;
        self.data[self.pos + 1] = value as u8;
        self.pos += 2;
    }

    pub fn ip2(&mut self, value: i32) {
        self.data[self.pos] = value as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.pos += 2;
    }

    pub fn p3(&mut self, value: i32) {
        self.data[self.pos] = (value >> 16) as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.data[self.pos + 2] = value as u8;
        self.pos += 3;
    }

    pub fn p4(&mut self, value: i32) {
        self.data[self.pos] = (value >> 24) as u8;
        self.data[self.pos + 1] = (value >> 16) as u8;
        self.data[self.pos + 2] = (value >> 8) as u8;
        self.data[self.pos + 3] = value as u8;
        self.pos += 4;
    }

    pub fn ip4(&mut self, value: i32) {
        self.data[self.pos] = value as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.data[self.pos + 2] = (value >> 16) as u8;
        self.data[self.pos + 3] = (value >> 24) as u8;
        self.pos += 4;
    }

    pub fn p8(&mut self, value: i64) {
        self.data[self.pos..self.pos + 8].copy_from_slice(&value.to_be_bytes());
        self.pos += 8;
    }

    pub fn pjstr(&mut self, s: &str) {
        for c in s.chars() {
            self.data[self.pos] = c as u8;
            self.pos += 1;
        }
        self.data[self.pos] = 10;
        self.pos += 1;
    }

    pub fn pdata(&mut self, src: &[u8], offset: usize, length: usize) {
        let end = self.pos + length;
        self.data[self.pos..end].copy_from_slice(&src[offset..offset + length]);
        self.pos = end;
    }

    pub fn psize1(&mut self, size: i32) {
        self.data[self.pos - size as usize - 1] = size as u8;
    }

    pub fn gbit_start(&mut self) {
        self.bit_pos = self.pos << 3;
    }

    pub fn gbit_end(&mut self) {
        self.pos = (self.bit_pos + 7) >> 3;
    }

    pub fn gbit(&mut self, n: usize) -> i32 {
        let mut byte_pos = self.bit_pos >> 3;
        let mut remaining = 8 - (self.bit_pos & 7);
        let mut value: i32 = 0;
        self.bit_pos += n;
        let mut n = n;

        while n > remaining {
            value = value.wrapping_add(
                ((self.data[byte_pos] as i32) & BITMASK[remaining])
                    .wrapping_shl((n - remaining) as u32),
            );
            byte_pos += 1;
            n -= remaining;
            remaining = 8;
        }

        if n == remaining {
            value = value.wrapping_add((self.data[byte_pos] as i32) & BITMASK[remaining]);
        } else {
            value = value.wrapping_add(
                (((self.data[byte_pos] as u32) >> (remaining - n)) as i32) & BITMASK[n],
            );
        }

        value
    }

    // Packet.ts `rsaenc` (lines 277-291): treat the written bytes as a big-endian
    // integer, modpow it, then write p1(len) + big-endian ciphertext.
    pub fn rsaenc(&mut self, modulus: &BigUint, exponent: &BigUint) {
        let length = self.pos;
        self.pos = 0;
        let mut temp = vec![0u8; length];
        self.gdata(length, 0, &mut temp);

        let big_raw = BigUint::from_bytes_be(&temp);
        let big_enc = big_raw.modpow(exponent, modulus);
        let mut raw_enc = big_enc.to_bytes_be();
        if raw_enc.first().is_some_and(|b| b & 0x80 != 0) {
            raw_enc.insert(0, 0);
        }

        self.pos = 0;
        self.p1(raw_enc.len() as i32);
        self.pdata(&raw_enc, 0, raw_enc.len());
    }
}
