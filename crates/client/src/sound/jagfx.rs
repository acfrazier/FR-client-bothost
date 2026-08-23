//! Port of `~/experiments/Server/webclient/src/sound/JagFX.ts`. The TS
//! `JagFX.synth`/`delays` statics and the shared `waveBytes`/`waveBuffer`/
//! `Tone.buf` scratch live on the `Client`-owned table (design: mutable
//! statics on the client). Writes past the fixed-size TS buffers are silently
//! dropped there, so the byte writes here are bounds-guarded the same way.

use crate::io::Packet;
use crate::sound::Tone;

const SYNTH_CAPACITY: usize = 1000;
/// 22050 Hz * 20 s, the size of the TS `JagFX.waveBytes` static.
const WAVE_BYTES: usize = 22050 * 20;
/// Shared `Tone.buf` from TS: 22050 Hz * 10 s of i32 scratch.
const TONE_BUF: usize = 22050 * 10;

/// One synthesized sound (a `JagFX` instance in TS). The fields are public
/// like the TS `tones`/`loopBegin`/`loopEnd`.
#[derive(Default)]
pub struct Sound {
    pub tones: [Option<Tone>; 10],
    pub loop_begin: i32,
    pub loop_end: i32,
}

/// The `JagFX` table: every sound plus the shared generation scratch.
pub struct JagFX {
    pub synth: Vec<Option<Sound>>,
    pub delays: Vec<i32>,
    wave_buffer: Packet,
    tone_buf: Vec<i32>,
}

impl Default for JagFX {
    fn default() -> Self {
        JagFX {
            synth: (0..SYNTH_CAPACITY).map(|_| None).collect(),
            delays: vec![0; SYNTH_CAPACITY],
            // `JagFX.waveBuffer` from TS wraps the shared `waveBytes`; the
            // packet owns that storage here. Scratch is lazy: `Default` does
            // not allocate the ~1.3 MiB wave/tone buffers until `generate`.
            wave_buffer: Packet::new(Vec::new()),
            tone_buf: Vec::new(),
        }
    }
}

impl JagFX {
    /// Grow the shared generation scratch to the TS static sizes on first
    /// use (Task 4: `Default` no longer owns them until `generate`).
    fn ensure_scratch(&mut self) {
        if self.wave_buffer.length() < WAVE_BYTES {
            self.wave_buffer = Packet::new(vec![0u8; WAVE_BYTES]);
        }
        if self.tone_buf.len() < TONE_BUF {
            self.tone_buf.resize(TONE_BUF, 0);
        }
    }

    /// `JagFX.init(buf)` from TS: read `sounds.dat` into the synth table.
    pub fn init(&mut self, buf: &mut Packet) {
        loop {
            let id = buf.g2();
            if id == 65535 {
                break;
            }
            let mut sound = Sound::default();
            sound.load(buf);
            self.delays[id as usize] = sound.optimise_start();
            self.synth[id as usize] = Some(sound);
        }
    }

    /// `JagFX.generate(id, loopCount)` from TS: synth the sound as a WAV
    /// packet, or `None` when the id is not in the table.
    pub fn generate(&mut self, id: i32, loop_count: i32) -> Option<&Packet> {
        self.ensure_scratch();
        let sound = self.synth.get_mut(id as usize)?.as_mut()?;
        let wave = self.wave_buffer.data_mut();
        let length = Self::make_sound(sound, &mut self.tone_buf, wave, loop_count);

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

    #[cfg(test)]
    pub fn wave_scratch_len(&self) -> usize {
        self.wave_buffer.data().len()
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
mod tests {
    use super::JagFX;

    /// Task 4: `Default` must not allocate the 441000-byte wave scratch (or
    /// the tone buffer) until `generate` first synthesises a sound.
    #[test]
    fn jagfx_default_has_no_wave_scratch() {
        let j = JagFX::default();
        assert_eq!(j.wave_scratch_len(), 0);
    }
}
