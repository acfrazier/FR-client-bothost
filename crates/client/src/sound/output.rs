//! Speaker output and the mandatory period fade.
//!
//! `Fade` is the output-gain envelope every build needs (the control plane
//! arms it from `saveMidi`/`stopMidi`/`setMidiVolume`), so it is always
//! compiled. `AudioOut` needs a real audio device and lives behind
//! `feature = "audio"`.

/// tinymidipcm.js `fadeStepDb`: one fade tick moves the output 0.25 dB.
const FADE_STEP_DB: f32 = 0.25;
/// tinymidipcm.js `fadeInterval`: one fade tick per 50 ms.
const FADE_TICK_MS: u64 = 50;
/// tinymidipcm.js `fadeEndStep * fadeStepDb`: the fade-out floor (-36 dB).
const FADE_FLOOR_DB: f32 = -36.0;

/// 274 `midivol` (1/100 dB) → dB, the fade ramp target; `RustyMidi::gain`
/// maps the same ladder to linear gain.
fn midivol_to_db(midivol: i32) -> f32 {
    midivol as f32 / 100.0
}

/// dB → linear gain (equivalently `10^(midivol/2000)`).
fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The output-gain envelope: `saveMidi(fading=true)` fades the period music
/// out then in toward the `midivol` target; jingles/`stopMidi` hard-cut.
/// `gain()` is the linear multiplier the mixer applies; `step_ms` advances
/// the 0.25 dB / 50 ms ramp from the audio callback clock.
pub struct Fade {
    /// Current output gain in dB; `-inf` after `stop_hard` (gain 0.0).
    db: f32,
    /// Ramp endpoint from `midivol` (`begin_song`/`set_target_vol`).
    target_db: f32,
    /// A zone-song ramp is running (`begin_song(true, ...)`).
    fading: bool,
    /// Ramping down to the floor before ramping up to the target.
    fading_out: bool,
    /// Milliseconds accumulated toward the next 50 ms fade tick.
    tick_ms: u64,
}

impl Fade {
    /// A fresh fade, open at 0 dB until the control plane arms it.
    pub fn new() -> Self {
        Fade {
            db: 0.0,
            target_db: 0.0,
            fading: false,
            fading_out: true,
            tick_ms: 0,
        }
    }

    /// `saveMidi(fading, midivol)`: with `fading` the output ramps down to
    /// the floor and back up to the `midivol` target; without it the gain
    /// jumps straight to the target (jingles).
    pub fn begin_song(&mut self, fading: bool, target_midivol: i32) {
        self.target_db = midivol_to_db(target_midivol);
        self.fading = fading;
        self.fading_out = true;
        self.tick_ms = 0;
        if !fading {
            self.db = self.target_db;
        }
    }

    /// `stopMidi()`: hard-cut to silence.
    pub fn stop_hard(&mut self) {
        self.fading = false;
        self.db = f32::NEG_INFINITY;
        self.tick_ms = 0;
    }

    /// `setMidiVolume(midivol)`: retarget the ramp; when nothing is ramping
    /// the gain jumps to the new ladder value ("voladjust").
    pub fn set_target_vol(&mut self, midivol: i32) {
        self.target_db = midivol_to_db(midivol);
        if !self.fading {
            self.db = self.target_db;
        }
    }

    /// Linear output multiplier for the mixer (1.0 = unity).
    pub fn gain(&self) -> f32 {
        db_to_gain(self.db)
    }

    /// Advance the fade clock by `ms`; every 50 ms tick moves 0.25 dB.
    pub fn step_ms(&mut self, ms: u64) {
        self.tick_ms += ms;
        while self.tick_ms >= FADE_TICK_MS {
            self.tick_ms -= FADE_TICK_MS;
            if self.fading {
                self.step();
            }
        }
    }

    fn step(&mut self) {
        if self.fading_out {
            self.db = (self.db - FADE_STEP_DB).max(FADE_FLOOR_DB);
            if self.db <= FADE_FLOOR_DB {
                self.fading_out = false;
            }
        } else {
            self.db = (self.db + FADE_STEP_DB).min(self.target_db);
            if self.db >= self.target_db {
                self.fading = false;
            }
        }
    }
}

impl Default for Fade {
    fn default() -> Self {
        Self::new()
    }
}

/// The cpal speaker: one 22050 Hz stereo stream mixing the rustysynth
/// render (scaled by the shared `Fade`) with the queued JagFX samples.
/// Opening fails with `AudioError` and the caller keeps running headless
/// (spec: audio device failure is not fatal).
#[cfg(feature = "audio")]
pub use device::{AudioError, AudioOut};

#[cfg(feature = "audio")]
mod device {
    use std::sync::{Arc, Mutex};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use crate::sound::Midi;

    use super::Fade;

    /// 22050 Hz, the rustysynth/JagFX rate (tinymidipcm.js `sampleRate`).
    const SAMPLE_RATE: u32 = 22050;

    /// The open speaker. Dropping the `Stream` stops the callback.
    pub struct AudioOut {
        _stream: cpal::Stream,
    }

    /// Why the speaker could not be opened (Task 6 logs and continues).
    #[derive(Debug)]
    pub enum AudioError {
        NoDevice,
        Build(cpal::Error),
        Play(cpal::Error),
    }

    impl AudioOut {
        /// Open the default output at 22050 Hz stereo. `midi`/`waves`/`fade`
        /// are the shared client state: the callback renders the synth,
        /// steps the fade clock, and drains the wave queue into the buffer.
        pub fn try_open(
            midi: Arc<Mutex<dyn Midi>>,
            waves: Arc<Mutex<Vec<i16>>>,
            fade: Arc<Mutex<Fade>>,
        ) -> Result<Self, AudioError> {
            let host = cpal::default_host();
            let device = host.default_output_device().ok_or(AudioError::NoDevice)?;
            let config = cpal::StreamConfig {
                channels: 2,
                sample_rate: SAMPLE_RATE,
                buffer_size: cpal::BufferSize::Default,
            };
            let stream = device
                .build_output_stream::<i16, _, _>(
                    config,
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                        let mut fade = fade.lock().unwrap();
                        let mut midi = midi.lock().unwrap();
                        let mut waves = waves.lock().unwrap();
                        render_block(data, &mut *midi, &mut waves, &mut fade);
                    },
                    |err| eprintln!("audio: output stream error: {err}"),
                    None,
                )
                .map_err(AudioError::Build)?;
            stream.play().map_err(AudioError::Play)?;
            Ok(AudioOut { _stream: stream })
        }
    }

    impl std::fmt::Display for AudioError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                AudioError::NoDevice => write!(f, "no default output device"),
                AudioError::Build(e) => write!(f, "build output stream: {e}"),
                AudioError::Play(e) => write!(f, "start output stream: {e}"),
            }
        }
    }

    impl std::error::Error for AudioError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                AudioError::Build(e) | AudioError::Play(e) => Some(e),
                AudioError::NoDevice => None,
            }
        }
    }

    /// One callback block: step the fade clock, render the synth through the
    /// fade gain, and mix the JagFX wave queue (mono i16) into both channels.
    fn render_block(data: &mut [i16], midi: &mut dyn Midi, waves: &mut Vec<i16>, fade: &mut Fade) {
        let frames = data.len() / 2;
        fade.step_ms((frames as u64 * 1000) / SAMPLE_RATE as u64);
        let gain = fade.gain();
        let mut left = vec![0f32; frames];
        let mut right = vec![0f32; frames];
        midi.render(&mut left, &mut right);
        let mut wave = waves.drain(..waves.len().min(frames));
        for (frame, out) in data.chunks_exact_mut(2).enumerate() {
            let w = wave.next().unwrap_or(0) as f32;
            out[0] = (left[frame] * gain * 32767.0 + w).clamp(-32768.0, 32767.0) as i16;
            out[1] = (right[frame] * gain * 32767.0 + w).clamp(-32768.0, 32767.0) as i16;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct Tone {
            level: f32,
        }

        impl Midi for Tone {
            fn play(&mut self, _data: &[u8], _volume: i32, _fading: bool) {}
            fn stop(&mut self) {}
            fn set_volume(&mut self, _volume: i32) {}
            fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    *l = self.level;
                    *r = self.level;
                }
            }
        }

        #[test]
        fn mix_scales_render_by_fade_gain() {
            let mut fade = Fade::new();
            fade.begin_song(false, 0); // gain 1.0
            let mut midi = Tone { level: 0.5 };
            let mut waves = Vec::new();
            let mut data = vec![0i16; 4];
            render_block(&mut data, &mut midi, &mut waves, &mut fade);
            assert_eq!(data, vec![16383, 16383, 16383, 16383]);
        }

        #[test]
        fn mix_adds_waves_and_clips() {
            let mut fade = Fade::new();
            fade.begin_song(false, 0);
            let mut midi = Tone { level: 1.0 };
            let mut waves = vec![500i16, -32768i16];
            let mut data = vec![0i16; 4];
            render_block(&mut data, &mut midi, &mut waves, &mut fade);
            // 32767 + 500 clips to 32767; 32767 + (-32768) = -1
            assert_eq!(data, vec![32767, 32767, -1, -1]);
        }

        #[test]
        fn stop_hard_zeroes_midi_output() {
            let mut fade = Fade::new();
            fade.begin_song(false, 0);
            fade.stop_hard();
            let mut midi = Tone { level: 1.0 };
            let mut waves = vec![1000i16; 2];
            let mut data = vec![0i16; 4];
            render_block(&mut data, &mut midi, &mut waves, &mut fade);
            // waves still mix; the synth path is muted
            assert_eq!(data, vec![1000, 1000, 1000, 1000]);
        }
    }
}
