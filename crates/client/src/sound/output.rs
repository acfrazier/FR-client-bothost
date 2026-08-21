//! Speaker output and the mandatory period fade.
//!
//! `Fade` is the output-gain envelope every build needs (the control plane
//! arms it from `saveMidi`/`stopMidi`/`setMidiVolume`), so it is always
//! compiled. `AudioOut` needs a real audio device and lives behind
//! `feature = "audio"`.

/// One fade tick moves the output 0.25 dB (tinymidipcm.js `fadeStepDb`).
/// The 274 deob exposes only the `midifade` boolean and the `midivol`
/// ladder; the native signlink player's curve is not in the source, so this
/// is a documented TS-proxy for it.
const FADE_STEP_DB: f32 = 0.25;
/// One fade tick per 50 ms (tinymidipcm.js `fadeInterval`).
const FADE_TICK_MS: u64 = 50;
/// The fade-out floor (-36 dB; tinymidipcm.js `fadeEndStep * fadeStepDb`).
const FADE_FLOOR_DB: f32 = -36.0;

/// 274 `midivol` (1/100 dB) → dB, the fade swap-in target.
fn midivol_to_db(midivol: i32) -> f32 {
    midivol as f32 / 100.0
}

/// dB → linear gain (equivalently `10^(midivol/2000)`).
fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The output-gain envelope: a zone-song change (`saveMidi(fading=true)`)
/// ramps the current song out to the floor; `Client::music_tick` then swaps
/// the pending song in at the `midivol` target (`finish_fade`). Jingles and
/// `stopMidi` hard-cut. There is no fade-in leg — Java's `midifade` is a
/// boolean "fade the outgoing song out", nothing more. `gain()` is the
/// linear multiplier the mixer applies; `step_ms` advances the 0.25 dB /
/// 50 ms ramp from the audio callback clock.
pub struct Fade {
    /// Current output gain in dB; `-inf` after `stop_hard` (gain 0.0).
    db: f32,
    /// Ramp endpoint from `midivol` (`finish_fade`/`set_target_vol`).
    target_db: f32,
    /// A zone-song fade-out is running.
    fading: bool,
    /// Ramping down to the floor (the only direction a fade runs).
    fading_out: bool,
    /// Latched once when the ramp hits the floor; `swap_due` consumes it.
    swap_due: bool,
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
            fading_out: false,
            swap_due: false,
            tick_ms: 0,
        }
    }

    /// `saveMidi(fading=true)`: arm a fade-out of the current song. The new
    /// song is held (`Client::midi_pending`) until the ramp hits the floor.
    pub fn fade_out(&mut self) {
        self.fading = true;
        self.fading_out = true;
        self.swap_due = false;
        self.tick_ms = 0;
    }

    /// `saveMidi(fading=false)` / the swap-in: jump the gain to the `midivol`
    /// target and end any fade (Java `midisave` starts the new song at the
    /// current `midivol`, with no fade-in).
    pub fn finish_fade(&mut self, target_midivol: i32) {
        self.target_db = midivol_to_db(target_midivol);
        self.db = self.target_db;
        self.fading = false;
        self.fading_out = false;
        self.swap_due = false;
        self.tick_ms = 0;
    }

    /// `Client::music_tick`: `true` once when the fade-out ramp reached the
    /// floor (-36 dB), clearing the latch.
    pub fn swap_due(&mut self) -> bool {
        std::mem::take(&mut self.swap_due)
    }

    /// `stopMidi()`: hard-cut to silence.
    pub fn stop_hard(&mut self) {
        self.fading = false;
        self.fading_out = false;
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
                self.fading = false;
                self.swap_due = true;
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
        pub sample_rate: u32,
    }

    /// Why the speaker could not be opened (Task 6 logs and continues).
    #[derive(Debug)]
    pub enum AudioError {
        NoDevice,
        Config(String),
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
            // Prefer the 274 rate (rustysynth/JagFX). Many macOS devices
            // reject 22050; fall back to the device default and resample.
            let (config, src_rate, dst_rate) = Self::pick_config(&device)?;
            let stream = device
                .build_output_stream::<i16, _, _>(
                    config,
                    {
                        // Scratch lives on the stream callback so the
                        // steady-state path never allocates (it only grows
                        // if the device changes its buffer size).
                        let mut scratch: Vec<f32> = Vec::new();
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            fill_buffer(
                                data,
                                &mut scratch,
                                &midi,
                                &waves,
                                &fade,
                                src_rate,
                                dst_rate,
                            );
                        }
                    },
                    |err| eprintln!("audio: output stream error: {err}"),
                    None,
                )
                .map_err(AudioError::Build)?;
            stream.play().map_err(AudioError::Play)?;
            Ok(AudioOut {
                _stream: stream,
                sample_rate: dst_rate,
            })
        }

        fn pick_config(
            device: &cpal::Device,
        ) -> Result<(cpal::StreamConfig, u32, u32), AudioError> {
            if let Ok(cfgs) = device.supported_output_configs() {
                for range in cfgs {
                    if range.channels() >= 2
                        && range.min_sample_rate() <= SAMPLE_RATE
                        && range.max_sample_rate() >= SAMPLE_RATE
                    {
                        let config = cpal::StreamConfig {
                            channels: 2,
                            sample_rate: SAMPLE_RATE,
                            buffer_size: cpal::BufferSize::Default,
                        };
                        return Ok((config, SAMPLE_RATE, SAMPLE_RATE));
                    }
                }
            }
            let supported = device
                .default_output_config()
                .map_err(|e| AudioError::Config(e.to_string()))?;
            let dst = supported.sample_rate();
            let config = cpal::StreamConfig {
                channels: 2,
                sample_rate: dst,
                buffer_size: cpal::BufferSize::Default,
            };
            Ok((config, SAMPLE_RATE, dst))
        }
    }

    impl std::fmt::Display for AudioError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                AudioError::NoDevice => write!(f, "no default output device"),
                AudioError::Config(e) => write!(f, "output config: {e}"),
                AudioError::Build(e) => write!(f, "build output stream: {e}"),
                AudioError::Play(e) => write!(f, "start output stream: {e}"),
            }
        }
    }

    impl std::error::Error for AudioError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match self {
                AudioError::Build(e) => Some(e),
                AudioError::Play(e) => Some(e),
                AudioError::NoDevice | AudioError::Config(_) => None,
            }
        }
    }

    /// Fill one output buffer from the shared client state, locking each
    /// piece only for the work that needs it: the fade clock (step + gain),
    /// the synth render (the one potentially slow call), and the wave-queue
    /// drain/mix. `scratch` is preallocated on the caller so the steady-state
    /// callback never allocates; it only grows when the device changes its
    /// buffer size.
    fn fill_buffer(
        data: &mut [i16],
        scratch: &mut Vec<f32>,
        midi: &Mutex<dyn Midi>,
        waves: &Mutex<Vec<i16>>,
        fade: &Mutex<Fade>,
        src_rate: u32,
        dst_rate: u32,
    ) {
        let out_frames = data.len() / 2;
        let in_frames = if src_rate == dst_rate {
            out_frames
        } else {
            // +1 neighbor for linear interpolation at the last output frame.
            ((out_frames as u64 * src_rate as u64) / dst_rate as u64) as usize + 2
        };
        if scratch.len() < in_frames * 2 {
            scratch.resize(in_frames * 2, 0.0);
        }
        // Fade clock follows wall-clock of the output buffer.
        let gain = {
            let mut fade = fade.lock().unwrap();
            fade.step_ms((out_frames as u64 * 1000) / dst_rate.max(1) as u64);
            fade.gain()
        };
        let (left, rest) = scratch.split_at_mut(in_frames);
        let right = &mut rest[..in_frames];
        {
            let mut midi = midi.lock().unwrap();
            midi.render(left, right);
        }
        {
            let mut waves = waves.lock().unwrap();
            let n = waves.len().min(in_frames);
            let drained: Vec<i16> = waves.drain(..n).collect();
            if src_rate == dst_rate {
                let mut wave = drained.into_iter();
                for (frame, out) in data.chunks_exact_mut(2).enumerate() {
                    let w = wave.next().unwrap_or(0) as f32;
                    out[0] = (left[frame] * gain * 32767.0 + w).clamp(-32768.0, 32767.0) as i16;
                    out[1] = (right[frame] * gain * 32767.0 + w).clamp(-32768.0, 32767.0) as i16;
                }
            } else {
                for (frame, out) in data.chunks_exact_mut(2).enumerate() {
                    let src_pos = frame as f32 * src_rate as f32 / dst_rate as f32;
                    let i = src_pos as usize;
                    let frac = src_pos - i as f32;
                    let i1 = (i + 1).min(in_frames.saturating_sub(1));
                    let i = i.min(in_frames.saturating_sub(1));
                    let l = left[i] * (1.0 - frac) + left[i1] * frac;
                    let r = right[i] * (1.0 - frac) + right[i1] * frac;
                    let w = *drained.get(i).unwrap_or(&0) as f32;
                    out[0] = (l * gain * 32767.0 + w).clamp(-32768.0, 32767.0) as i16;
                    out[1] = (r * gain * 32767.0 + w).clamp(-32768.0, 32767.0) as i16;
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct Tone {
            level: f32,
        }

        impl Midi for Tone {
            fn play(&mut self, _data: &[u8], _volume: i32, _fading: bool) -> bool {
                true
            }
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
            let fade = Mutex::new(Fade::new());
            fade.lock().unwrap().finish_fade(0); // gain 1.0
            let midi = Mutex::new(Tone { level: 0.5 });
            let waves = Mutex::new(Vec::new());
            let mut data = vec![0i16; 4];
            let mut scratch = Vec::new();
            fill_buffer(&mut data, &mut scratch, &midi, &waves, &fade, SAMPLE_RATE, SAMPLE_RATE);
            // Fade gain 1.0 (midivol 0) × 0.5 × 32767 — no extra mix scale.
            assert_eq!(data, vec![16383, 16383, 16383, 16383]);
        }

        #[test]
        fn mix_adds_waves_and_clips() {
            let fade = Mutex::new(Fade::new());
            fade.lock().unwrap().finish_fade(0);
            let midi = Mutex::new(Tone { level: 1.0 });
            let waves = Mutex::new(vec![500i16, -32768i16]);
            let mut data = vec![0i16; 4];
            let mut scratch = Vec::new();
            fill_buffer(&mut data, &mut scratch, &midi, &waves, &fade, SAMPLE_RATE, SAMPLE_RATE);
            // 32767 + 500 clips to 32767; 32767 + (-32768) = -1
            assert_eq!(data, vec![32767, 32767, -1, -1]);
        }

        #[test]
        fn stop_hard_zeroes_midi_output() {
            let fade = Mutex::new(Fade::new());
            fade.lock().unwrap().finish_fade(0);
            fade.lock().unwrap().stop_hard();
            let midi = Mutex::new(Tone { level: 1.0 });
            let waves = Mutex::new(vec![1000i16; 2]);
            let mut data = vec![0i16; 4];
            let mut scratch = Vec::new();
            fill_buffer(&mut data, &mut scratch, &midi, &waves, &fade, SAMPLE_RATE, SAMPLE_RATE);
            // waves still mix; the synth path is muted
            assert_eq!(data, vec![1000, 1000, 1000, 1000]);
        }
    }
}
