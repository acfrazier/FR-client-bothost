//! Port of `~/experiments/Server/webclient/src/sound/JagFX.ts`. The TS
//! `JagFX.synth`/`delays` statics are one process table (Arc, keyed by
//! cache_dir); `waveBytes`/`waveBuffer`/`Tone.buf` scratch stay per-client.
//! Writes past the fixed-size TS buffers are silently dropped there, so the
//! byte writes here are bounds-guarded the same way.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::io::Packet;
use crate::sound::Tone;

const SYNTH_CAPACITY: usize = 1000;
/// 22050 Hz * 20 s, the size of the TS `JagFX.waveBytes` static.
const WAVE_BYTES: usize = 22050 * 20;
/// Shared `Tone.buf` from TS: 22050 Hz * 10 s of i32 scratch.
const TONE_BUF: usize = 22050 * 10;

/// One synthesized sound (a `JagFX` instance in TS). The fields are public
/// like the TS `tones`/`loopBegin`/`loopEnd`.
#[derive(Clone, Default)]
pub struct Sound {
    pub tones: [Option<Tone>; 10],
    pub loop_begin: i32,
    pub loop_end: i32,
}

/// The `JagFX` table: every sound plus the shared generation scratch.
pub struct JagFX {
    pub synth: Arc<Vec<Option<Sound>>>,
    pub delays: Arc<Vec<i32>>,
    wave_buffer: Packet,
    tone_buf: Vec<i32>,
}

fn empty_table() -> (Arc<Vec<Option<Sound>>>, Arc<Vec<i32>>) {
    static EMPTY: OnceLock<(Arc<Vec<Option<Sound>>>, Arc<Vec<i32>>)> = OnceLock::new();
    EMPTY
        .get_or_init(|| {
            (
                Arc::new((0..SYNTH_CAPACITY).map(|_| None).collect()),
                Arc::new(vec![0; SYNTH_CAPACITY]),
            )
        })
        .clone()
}

fn unpacked_tables() -> &'static Mutex<HashMap<String, (Arc<Vec<Option<Sound>>>, Arc<Vec<i32>>)>> {
    static TABLES: OnceLock<Mutex<HashMap<String, (Arc<Vec<Option<Sound>>>, Arc<Vec<i32>>)>>> =
        OnceLock::new();
    TABLES.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Default for JagFX {
    fn default() -> Self {
        let (synth, delays) = empty_table();
        JagFX {
            synth,
            delays,
            // Scratch is allocated on first `generate` — 50 unheaded
            // clients must not pay 441 KB + 882 KB of zeroed wave/tone
            // buffers each (~65 MB) before any synth runs.
            wave_buffer: Packet::new(Vec::new()),
            tone_buf: Vec::new(),
        }
    }
}

impl JagFX {
    fn ensure_scratch(&mut self) {
        if self.wave_buffer.length() < WAVE_BYTES {
            self.wave_buffer = Packet::new(vec![0u8; WAVE_BYTES]);
        }
        if self.tone_buf.len() < TONE_BUF {
            self.tone_buf.resize(TONE_BUF, 0);
        }
    }

    /// `JagFX.init(buf)` from TS: read `sounds.dat` into the synth table.
    /// COWs the process empty table so a test fixture does not fill it.
    pub fn init(&mut self, buf: &mut Packet) {
        let delays = Arc::make_mut(&mut self.delays);
        let synth = Arc::make_mut(&mut self.synth);
        loop {
            let id = buf.g2();
            if id == 65535 {
                break;
            }
            let mut sound = Sound::default();
            sound.load(buf);
            delays[id as usize] = sound.optimise_start();
            synth[id as usize] = Some(sound);
        }
    }

    /// Process-wide synth table for `cache_dir` (one unpack of `sounds.dat`).
    /// Lowmem / missing file stays on the shared empty table.
    pub fn load_shared(cache_dir: &str, lowmem: bool) -> Self {
        if lowmem {
            return Self::default();
        }
        {
            let tables = unpacked_tables().lock().expect("jagfx tables");
            if let Some((synth, delays)) = tables.get(cache_dir) {
                return JagFX {
                    synth: Arc::clone(synth),
                    delays: Arc::clone(delays),
                    wave_buffer: Packet::new(Vec::new()),
                    tone_buf: Vec::new(),
                };
            }
        }
        let mut jagfx = Self::default();
        let Ok(bytes) = std::fs::read(format!("{cache_dir}/sounds")) else {
            return jagfx;
        };
        let Some(dat) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::io::JagFile::new(bytes)
        }))
        .ok()
        .and_then(|jag| jag.read("sounds.dat")) else {
            return jagfx;
        };
        jagfx.init(&mut Packet::new(dat));
        {
            let mut tables = unpacked_tables().lock().expect("jagfx tables");
            if let Some((synth, delays)) = tables.get(cache_dir) {
                jagfx.synth = Arc::clone(synth);
                jagfx.delays = Arc::clone(delays);
            } else {
                tables.insert(
                    cache_dir.to_string(),
                    (Arc::clone(&jagfx.synth), Arc::clone(&jagfx.delays)),
                );
            }
        }
        jagfx
    }

    /// `JagFX.generate(id, loopCount)` from TS: synth the sound as a WAV
    /// packet, or `None` when the id is not in the table. Clones one `Sound`
    /// so Tone generate can mutate without COWing the process table.
    pub fn generate(&mut self, id: i32, loop_count: i32) -> Option<&Packet> {
        self.ensure_scratch();
        let mut sound = self.synth.get(id as usize)?.clone()?;
        let wave = self.wave_buffer.data_mut();
        let length = Self::make_sound(&mut sound, &mut self.tone_buf, wave, loop_count);

        self.wave_buffer.pos = 0;
        self.wave_buffer.p4(0x5249_4646); // "RIFF" ChunkID
        self.wave_buffer.ip4(length + 36); // ChunkSize
        self.wave_buffer.p4(0x5741_5645); // "WAVE" format
        self.wave_buffer.p4(0x666d_7420); // "fmt " chunk id
        self.wave_buffer.ip4(16); // chunk size
        self.wave_buffer.ip2(1); // audio format
        self.wave_buffer.ip2(1); // num channels
        self.wave_buffer.ip4(22050); // sample rate
        self.wave_buffer.ip4(22050); // byte rate
        self.wave_buffer.ip2(1); // block align
        self.wave_buffer.ip2(8); // bits per sample
        self.wave_buffer.p4(0x6461_7461); // "data"
        self.wave_buffer.ip4(length);
        self.wave_buffer.pos += length as usize;
        Some(&self.wave_buffer)
    }

    fn make_sound(sound: &mut Sound, tone_buf: &mut [i32], wave: &mut [u8], mut loop_count: i32) -> i32 {
        let mut duration = 0;
        for tone in sound.tones.iter().flatten() {
            if tone.length + tone.start > duration {
                duration = tone.length + tone.start;
            }
        }

        if duration == 0 {
            return 0;
        }

        let mut sample_count = ((duration as f64 * 22050.0) / 1000.0) as i32;
        let mut loop_start = ((sound.loop_begin as f64 * 22050.0) / 1000.0) as i32;
        let mut loop_stop = ((sound.loop_end as f64 * 22050.0) / 1000.0) as i32;

        if loop_start < 0 || loop_stop < 0 || loop_stop > sample_count || loop_start >= loop_stop {
            loop_count = 0;
        }

        let mut total_sample_count = sample_count + (loop_stop - loop_start) * (loop_count - 1);
        for sample in 44..total_sample_count + 44 {
            if let Some(slot) = wave.get_mut(sample as usize) {
                *slot = 128; // -128 as u8: silent PCM
            }
        }

        for tone in sound.tones.iter_mut().flatten() {
            let tone_sample_count = ((tone.length as f64 * 22050.0) / 1000.0) as i32;
            let start = ((tone.start as f64 * 22050.0) / 1000.0) as i32;
            let samples = tone.generate(tone_sample_count, tone.length, tone_buf);

            // TS bounds this loop to `toneSampleCount`; the scratch beyond it
            // holds stale samples from earlier tones.
            for (sample, &s) in samples.iter().take(tone_sample_count as usize).enumerate() {
                // `((samples[sample] >> 8) << 24) >> 24`: the second byte of
                // the 16-bit sample, sign-extended
                let byte = ((s >> 8) & 0xff) as u8 as i8 as i32;
                if let Some(slot) = wave.get_mut(sample + start as usize + 44) {
                    *slot = slot.wrapping_add(byte as u8);
                }
            }
        }

        if loop_count > 1 {
            loop_start += 44;
            loop_stop += 44;
            sample_count += 44;
            total_sample_count += 44;

            let end_offset = total_sample_count - sample_count;
            let mut sample = sample_count - 1;
            while sample >= loop_stop {
                if let Some(src) = wave.get(sample as usize).copied() {
                    if let Some(slot) = wave.get_mut(sample as usize + end_offset as usize) {
                        *slot = src;
                    }
                }
                sample -= 1;
            }

            let mut loop_index = 1;
            while loop_index < loop_count {
                let offset = (loop_stop - loop_start) * loop_index;
                let mut sample = loop_start;
                while sample < loop_stop {
                    if let Some(src) = wave.get(sample as usize).copied() {
                        if let Some(slot) = wave.get_mut(sample as usize + offset as usize) {
                            *slot = src;
                        }
                    }
                    sample += 1;
                }
                loop_index += 1;
            }

            total_sample_count -= 44;
        }

        total_sample_count
    }
}

impl Sound {
    fn load(&mut self, dat: &mut Packet) {
        for tone in 0..10 {
            if dat.g1() != 0 {
                dat.pos -= 1;
                self.tones[tone] = Some(Tone::load(dat));
            }
        }

        self.loop_begin = dat.g2();
        self.loop_end = dat.g2();
    }

    /// `optimiseStart()` from TS: trim the shared silent lead-in and return
    /// it as the sound's start delay.
    fn optimise_start(&mut self) -> i32 {
        let mut start = 9999999;
        for tone in self.tones.iter().flatten() {
            let tone_start = (tone.start as f64 / 20.0) as i32;
            if tone_start < start {
                start = tone_start;
            }
        }

        if self.loop_begin < self.loop_end && ((self.loop_begin as f64 / 20.0) as i32) < start {
            start = (self.loop_begin as f64 / 20.0) as i32;
        }

        if start == 9999999 || start == 0 {
            return 0;
        }

        for tone in self.tones.iter_mut().flatten() {
            tone.start -= start * 20;
        }

        if self.loop_begin < self.loop_end {
            self.loop_begin -= start * 20;
            self.loop_end -= start * 20;
        }

        start
    }
}

#[cfg(test)]
mod jagfx_size_tests {
    use super::*;

    #[test]
    fn default_does_not_allocate_wave_scratch() {
        let j = JagFX::default();
        assert_eq!(j.wave_buffer.length(), 0);
        assert!(j.tone_buf.is_empty());
    }

    #[test]
    fn default_tables_are_one_process_arc() {
        let a = JagFX::default();
        let b = JagFX::default();
        assert!(
            std::sync::Arc::ptr_eq(&a.synth, &b.synth),
            "empty synth table must be one process Arc"
        );
        assert!(std::sync::Arc::ptr_eq(&a.delays, &b.delays));
    }
}
