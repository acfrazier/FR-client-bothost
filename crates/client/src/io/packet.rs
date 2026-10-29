// Port of `~/experiments/Server/webclient/src/io/Packet.ts` lines 1–275
// (p1Enc and rsaenc are ported separately). CRC is bit-identical to the TS
// table; all value math uses i32 wrapping like JS `| 0`.
use std::cell::RefCell;
use std::num::NonZeroUsize;
use std::sync::Arc;

use num_bigint::BigUint;

use super::isaac::Isaac;

/// Observes completed encrypted output packets without changing their bytes.
///
/// Packet bodies use their revision-specific descriptor: a nonnegative value
/// is a fixed body length, `-1` is a one-byte variable-length prefix, and `-2`
/// is a two-byte variable-length prefix.
pub trait PacketObserver: Send + Sync {
    /// Returns the revision-specific body-length descriptor for this opcode.
    fn packet_body_length(&self, opcode: u8) -> i16;

    /// Reports one completed encoded packet, including its opcode and length
    /// prefix.
    fn packet_committed(&self, opcode: u8, encoded_length: usize);

    /// Reports the observed player-info tick and current session boundary.
    fn observed_tick(&self, _tick: u64, _session: u64) {}

    /// Reports an accepted local edit that may encode no packet.
    fn foreign_local_input(&self) {}

    /// Returns the observer's monotonic count of all committed packet types.
    fn committed_packet_count(&self) -> Option<u64> {
        None
    }
}

#[derive(Clone, Copy)]
struct PacketObservation {
    opcode: u8,
    body_start: NonZeroUsize,
    body_length: i16,
}
struct ObserverState {
    observer: Arc<dyn PacketObserver>,
    observation: Option<PacketObservation>,
}

enum PacketRole {
    General,
    Inbound,
    InboundFrame(usize),
    Outbound,
    OutboundObserved(Box<ObserverState>),
}

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
    role: PacketRole,
}

impl Packet {
    pub fn new(data: Vec<u8>) -> Self {
        Packet {
            data,
            pos: 0,
            bit_pos: 0,
            random: None,
            role: PacketRole::General,
        }
    }

    /// Sets the optional observer for completed encrypted output packets.
    ///
    /// # Panics
    ///
    /// Panics when attaching an observer to an inbound packet.
    pub fn set_observer(&mut self, observer: Option<Arc<dyn PacketObserver>>) {
        assert!(
            !matches!(
                &self.role,
                PacketRole::Inbound | PacketRole::InboundFrame(_)
            ) || observer.is_none(),
            "cannot observe encrypted output on an inbound packet"
        );
        let role = std::mem::replace(&mut self.role, PacketRole::General);
        self.role = match (role, observer) {
            (PacketRole::General, Some(observer)) | (PacketRole::Outbound, Some(observer)) => {
                PacketRole::OutboundObserved(Box::new(ObserverState {
                    observer,
                    observation: None,
                }))
            }
            (PacketRole::OutboundObserved(mut state), Some(observer)) => {
                state.observer = observer;
                state.observation = None;
                PacketRole::OutboundObserved(state)
            }
            (PacketRole::OutboundObserved(_), None) => PacketRole::Outbound,
            (role, None) => role,
            (role, Some(_)) => {
                self.role = role;
                unreachable!("inbound observer attachment was rejected above")
            }
        };
    }

    /// Keep the receiving slot's observation identity across socket adoption.
    pub(crate) fn preserve_observer_from(&mut self, previous: &mut Self) {
        assert!(
            !matches!(
                &self.role,
                PacketRole::Inbound | PacketRole::InboundFrame(_)
            ),
            "cannot preserve an output observer into an inbound packet"
        );
        assert!(
            matches!(
                &previous.role,
                PacketRole::Outbound | PacketRole::OutboundObserved(_)
            ),
            "observer identity must come from an outbound packet"
        );
        let previous_role = std::mem::replace(&mut previous.role, PacketRole::Outbound);
        let observer_state = match previous_role {
            PacketRole::OutboundObserved(mut state) => {
                state.observation = None;
                Some(state)
            }
            PacketRole::Outbound => None,
            _ => unreachable!("outbound role was checked above"),
        };
        self.role = observer_state
            .map(PacketRole::OutboundObserved)
            .unwrap_or(PacketRole::Outbound);
    }

    /// Returns whether an encrypted-output observer is installed.
    pub fn has_observer(&self) -> bool {
        matches!(&self.role, PacketRole::OutboundObserved(_))
    }

    /// Whether this packet still carries a client outbound role.
    pub(crate) fn is_outbound(&self) -> bool {
        matches!(
            &self.role,
            PacketRole::Outbound | PacketRole::OutboundObserved(_)
        )
    }

    fn observer(&self) -> Option<&Arc<dyn PacketObserver>> {
        match &self.role {
            PacketRole::OutboundObserved(state) => Some(&state.observer),
            _ => None,
        }
    }

    /// Finalizes a variable-length packet after its caller has finished
    /// encoding it. Incomplete or mismatched packets are not reported.
    pub fn finish_packet(&mut self) {
        self.finish_observation(true);
    }

    /// Reports an input observation boundary to the optional host observer.
    pub fn observe_tick(&self, tick: u64, session: u64) {
        if let Some(observer) = self.observer() {
            observer.observed_tick(tick, session);
        }
    }

    /// Reads the installed observer's count without copying packet or cipher data.
    pub fn committed_packet_count(&self) -> Option<u64> {
        self.observer()?.committed_packet_count()
    }

    /// Reports a local prompt edit without treating it as a committed packet.
    pub fn observe_foreign_local_input(&self) {
        if let Some(observer) = self.observer() {
            observer.foreign_local_input();
        }
    }

    fn observe_progress(&mut self) {
        if self.has_observer() {
            self.finish_observation(false);
        }
    }

    fn finish_observation(&mut self, variable_is_final: bool) {
        let observation = match &self.role {
            PacketRole::OutboundObserved(state) => state.observation,
            _ => None,
        };
        let Some(observation) = observation else {
            return;
        };
        let Some(body_length) = self.pos.checked_sub(observation.body_start.get()) else {
            return;
        };
        let encoded_length = body_length + 1;
        let complete = match observation.body_length {
            body_length if body_length >= 0 => encoded_length == 1 + body_length as usize,
            -1 if variable_is_final && encoded_length >= 2 => {
                self.data[observation.body_start.get()] as usize == encoded_length - 2
            }
            -2 if variable_is_final && encoded_length >= 3 => {
                u16::from_be_bytes([
                    self.data[observation.body_start.get()],
                    self.data[observation.body_start.get() + 1],
                ]) as usize
                    == encoded_length - 3
            }
            _ => false,
        };
        if complete {
            if let PacketRole::OutboundObserved(state) = &mut self.role {
                state.observation = None;
                state
                    .observer
                    .packet_committed(observation.opcode, encoded_length);
            }
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
        self.frame_end().unwrap_or(self.data.len()) - self.pos
    }

    /// Restrict reads to the exact bytes of the current wire frame. The
    /// backing allocation is intentionally reusable, so handlers must not see
    /// bytes left by a previous, longer packet.
    ///
    /// # Panics
    ///
    /// Panics if called on an outbound packet or if `end` exceeds the buffer.
    pub fn set_frame_end(&mut self, end: usize) {
        assert!(
            !matches!(
                &self.role,
                PacketRole::Outbound | PacketRole::OutboundObserved(_)
            ),
            "cannot apply an inbound frame bound to an outbound packet"
        );
        assert!(end <= self.data.len());
        self.role = PacketRole::InboundFrame(end);
    }

    pub fn clear_frame_end(&mut self) {
        if matches!(&self.role, PacketRole::InboundFrame(_)) {
            self.role = PacketRole::Inbound;
        }
    }

    pub fn frame_end(&self) -> Option<usize> {
        match &self.role {
            PacketRole::InboundFrame(end) => Some(*end),
            _ => None,
        }
    }

    fn assert_can_read(&self, length: usize) {
        assert!(self
            .pos
            .checked_add(length)
            .is_some_and(|end| end <= self.frame_end().unwrap_or(self.data.len())));
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

    fn allocation_capacity(kind: i32) -> usize {
        match kind {
            0 => 100,
            1 => 5000,
            _ => 30000,
        }
    }

    #[cfg(feature = "render-test-hooks")]
    pub fn render_test_capacities() -> [usize; 3] {
        [
            Self::allocation_capacity(0),
            Self::allocation_capacity(1),
            Self::allocation_capacity(2),
        ]
    }

    pub fn alloc(kind: i32) -> Packet {
        let cached = match kind {
            0 => CACHE_MIN.with(|c| c.borrow_mut().pop()),
            1 => CACHE_MID.with(|c| c.borrow_mut().pop()),
            _ => CACHE_MAX.with(|c| c.borrow_mut().pop()),
        };
        if let Some(mut p) = cached {
            p.pos = 0;
            p.role = PacketRole::General;
            return p;
        }
        Packet::new(vec![0; Self::allocation_capacity(kind)])
    }

    pub(crate) fn alloc_inbound(kind: i32) -> Packet {
        let mut packet = Self::alloc(kind);
        packet.role = PacketRole::Inbound;
        packet
    }

    pub(crate) fn alloc_outbound(kind: i32) -> Packet {
        let mut packet = Self::alloc(kind);
        packet.role = PacketRole::Outbound;
        packet
    }

    pub fn release(mut self) {
        self.role = PacketRole::General;
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
        self.assert_can_read(1);
        let v = self.data[self.pos] as i32;
        self.pos += 1;
        v
    }

    // signed
    pub fn g1b(&mut self) -> i32 {
        self.assert_can_read(1);
        let v = self.data[self.pos] as i8 as i32;
        self.pos += 1;
        v
    }

    pub fn g2(&mut self) -> i32 {
        self.assert_can_read(2);
        let v = u16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]) as i32;
        self.pos += 2;
        v
    }

    // signed
    pub fn g2b(&mut self) -> i32 {
        self.assert_can_read(2);
        let v = i16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]) as i32;
        self.pos += 2;
        v
    }

    pub fn g3(&mut self) -> i32 {
        self.assert_can_read(3);
        let v = (self.data[self.pos] as i32) << 16
            | u16::from_be_bytes([self.data[self.pos + 1], self.data[self.pos + 2]]) as i32;
        self.pos += 3;
        v
    }

    pub fn g4(&mut self) -> i32 {
        self.assert_can_read(4);
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
        self.assert_can_read(8);
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
        while self.pos < self.frame_end().unwrap_or(self.data.len()) {
            let b = self.data[self.pos];
            self.pos += 1;
            if b == 10 {
                return s;
            }
            s.push(b as char);
        }
        assert!(
            self.frame_end().is_none(),
            "unterminated string in bounded frame"
        );
        s
    }

    pub fn gdata(&mut self, length: usize, offset: usize, dest: &mut [u8]) {
        self.assert_can_read(length);
        let end = self.pos + length;
        dest[offset..offset + length].copy_from_slice(&self.data[self.pos..end]);
        self.pos = end;
    }

    pub fn p1_enc(&mut self, opcode: i32) {
        if self.has_observer() {
            self.finish_observation(true);
        }
        let r = self.random.as_mut().map(|r| r.next_int()).unwrap_or(0);
        self.data[self.pos] = opcode.wrapping_add(r) as u8;
        self.pos += 1;
        if let PacketRole::OutboundObserved(state) = &mut self.role {
            let opcode = opcode as u8;
            state.observation = Some(PacketObservation {
                opcode,
                body_start: NonZeroUsize::new(self.pos)
                    .expect("an opcode has a nonzero end cursor"),
                body_length: state.observer.packet_body_length(opcode),
            });
        }
        if self.has_observer() {
            self.finish_observation(false);
        }
    }

    pub fn p1(&mut self, value: i32) {
        self.data[self.pos] = value as u8;
        self.pos += 1;
        self.observe_progress();
    }

    pub fn p2(&mut self, value: i32) {
        self.data[self.pos] = (value >> 8) as u8;
        self.data[self.pos + 1] = value as u8;
        self.pos += 2;
        self.observe_progress();
    }

    pub fn ip2(&mut self, value: i32) {
        self.data[self.pos] = value as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.pos += 2;
        self.observe_progress();
    }

    pub fn p3(&mut self, value: i32) {
        self.data[self.pos] = (value >> 16) as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.data[self.pos + 2] = value as u8;
        self.pos += 3;
        self.observe_progress();
    }

    pub fn p4(&mut self, value: i32) {
        self.data[self.pos] = (value >> 24) as u8;
        self.data[self.pos + 1] = (value >> 16) as u8;
        self.data[self.pos + 2] = (value >> 8) as u8;
        self.data[self.pos + 3] = value as u8;
        self.pos += 4;
        self.observe_progress();
    }

    pub fn ip4(&mut self, value: i32) {
        self.data[self.pos] = value as u8;
        self.data[self.pos + 1] = (value >> 8) as u8;
        self.data[self.pos + 2] = (value >> 16) as u8;
        self.data[self.pos + 3] = (value >> 24) as u8;
        self.pos += 4;
        self.observe_progress();
    }

    pub fn p8(&mut self, value: i64) {
        self.data[self.pos..self.pos + 8].copy_from_slice(&value.to_be_bytes());
        self.pos += 8;
        self.observe_progress();
    }

    pub fn pjstr(&mut self, s: &str) {
        for c in s.chars() {
            self.data[self.pos] = c as u8;
            self.pos += 1;
        }
        self.data[self.pos] = 10;
        self.pos += 1;
        self.observe_progress();
    }

    pub fn pdata(&mut self, src: &[u8], offset: usize, length: usize) {
        let end = self.pos + length;
        self.data[self.pos..end].copy_from_slice(&src[offset..offset + length]);
        self.pos = end;
        self.observe_progress();
    }

    pub fn psize1(&mut self, size: i32) {
        self.data[self.pos - size as usize - 1] = size as u8;
        self.finish_observation(true);
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

#[cfg(test)]
mod observer_memory_tests {
    use super::*;

    #[test]
    fn packet_observer_layout_preserves_base_packet_and_client_sizes() {
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(std::mem::size_of::<PacketObservation>(), 16);
            assert_eq!(std::mem::size_of::<ObserverState>(), 32);
            assert_eq!(std::mem::size_of::<PacketRole>(), 16);
            assert_eq!(std::mem::size_of::<Packet>(), 2128);
        }
        let packet_size = std::mem::size_of::<Packet>();
        let client_size = std::mem::size_of::<crate::client::Client>();
        let nav_debug_fields = std::mem::size_of::<Option<crate::render::budget::BudgetCharge>>()
            + std::mem::size_of::<Option<crate::render::budget::RenderLimit>>();
        println!(
            "S3 client memory: packet={packet_size} client={client_size} nav_debug_fields={nav_debug_fields}"
        );
        #[cfg(target_os = "macos")]
        assert_eq!(client_size, 28872 + nav_debug_fields);
    }
}

#[cfg(test)]
mod rsa_encoding_tests {
    use super::*;

    #[test]
    fn rsaenc_omits_zero_top_byte_like_java_big_integer() {
        let mut raw = vec![0u8; 64];
        raw[1] = 0x7f;
        raw[63] = 0x2a;

        let mut packet = Packet::new(raw.clone());
        packet.pos = raw.len();
        let modulus = BigUint::from(1u8) << 512usize;
        let exponent = BigUint::from(1u8);
        packet.rsaenc(&modulus, &exponent);

        assert_eq!(packet.data[0], 63);
        assert_eq!(&packet.data[1..], &raw[1..]);
    }
}
