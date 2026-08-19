//! Midi backend: the 274 `signlink` control plane condensed into one trait.
//! `saveMidi`/`stopMidi`/`setMidiVolume` become `play`/`stop`/`set_volume`;
//! volumes are the 274 `midivol` ladder 0 / -400 / -800 / -1200 (1/100 dB).

pub trait Midi: Send {
    /// `saveMidi(fading, data)`: start the on-demand archive-2 bytes.
    fn play(&mut self, data: &[u8], volume: i32, fading: bool);
    /// `stopMidi()`: silence the current song.
    fn stop(&mut self);
    /// `setMidiVolume(active, volume)`: the "voladjust" poke.
    fn set_volume(&mut self, volume: i32);
    /// Render one stereo f32 block at 22050 Hz into the output buffers; the
    /// mixer multiplies it by the shared fade gain. Headless backends have
    /// no output, so the default renders silence.
    fn render(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}

/// Default backend (feature `audio` off): requests still complete, but there
/// is no device and nothing is synthesised.
pub struct NullMidi;

impl Midi for NullMidi {
    fn play(&mut self, _data: &[u8], _volume: i32, _fading: bool) {}
    fn stop(&mut self) {}
    fn set_volume(&mut self, _volume: i32) {}
}

#[cfg(feature = "audio")]
mod rusty {
    use std::fs::File;
    use std::io::Cursor;
    use std::sync::Arc;

    use rustysynth::{MidiFile, MidiFileLoopType, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};

    use super::Midi;

    const SAMPLE_RATE: i32 = 22050;

    /// The `audio` backend: one rustysynth sequencer and one SF2. The SF2 is
    /// picked up from `cache_dir` (`soundfont.sf2` / `Florestan.sf2`) or the
    /// current directory; without one the backend stays silent but tracks the
    /// volume plane. `fading` is a fade-in hint from the control plane; the
    /// headless synth has no render clock to fade against, so it is ignored.
    /// The synthesizer keeps its own `Arc<SoundFont>` and the sequencer keeps
    /// its own `Arc<MidiFile>`, so neither is stored here after construction.
    pub struct RustyMidi {
        sequencer: Option<MidiFileSequencer>,
        volume: i32,
    }

    impl RustyMidi {
        pub fn new(cache_dir: &str) -> Self {
            for candidate in [
                format!("{cache_dir}/soundfont.sf2"),
                format!("{cache_dir}/Florestan.sf2"),
                "Florestan.sf2".to_string(),
            ] {
                if let Some(midi) = Self::with_sound_font(&candidate) {
                    return midi;
                }
            }
            Self { sequencer: None, volume: 0 }
        }

        /// Load an explicit SF2; a missing/unreadable font yields a silent
        /// backend (spec: "Midi missing, audio on → warn + silence").
        pub fn with_sound_font(path: &str) -> Option<Self> {
            let file = File::open(path).ok()?;
            let mut file = file;
            let sound_font = Arc::new(SoundFont::new(&mut file).ok()?);
            let settings = SynthesizerSettings::new(SAMPLE_RATE);
            let synthesizer = Synthesizer::new(&sound_font, &settings).ok()?;
            let sequencer = MidiFileSequencer::new(synthesizer);
            Some(Self { sequencer: Some(sequencer), volume: 0 })
        }

        pub fn volume(&self) -> i32 {
            self.volume
        }

        /// 274 `midivol` (1/100 dB) → linear gain, as the client-ts
        /// tinymidipcm `decibelsToGain`: 0 → 1.0, -400 → -4 dB, ... The
        /// mixer's `Fade` maps the same ladder.
        pub fn gain(volume: i32) -> f32 {
            10f32.powf(volume as f32 / 2000.0)
        }

        /// Render one block of stereo f32. The output is raw here: the 274
        /// `midivol` gain lives on the mixer's `Fade`, which the cpal
        /// callback applies after rendering.
        pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
            if let Some(sequencer) = &mut self.sequencer {
                sequencer.render(left, right);
            }
        }
    }

    impl Midi for RustyMidi {
        fn play(&mut self, data: &[u8], volume: i32, _fading: bool) {
            self.volume = volume;
            let Some(sequencer) = &mut self.sequencer else { return };
            let Ok(midi_file) = MidiFile::new_with_loop_type(
                &mut Cursor::new(data),
                MidiFileLoopType::RpgMaker,
            ) else {
                return;
            };
            let midi_file = Arc::new(midi_file);
            sequencer.play(&midi_file, true);
        }

        fn stop(&mut self) {
            if let Some(sequencer) = &mut self.sequencer {
                sequencer.stop();
            }
        }

        fn set_volume(&mut self, volume: i32) {
            self.volume = volume;
        }

        fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
            RustyMidi::render(self, left, right);
        }
    }
}

#[cfg(feature = "audio")]
pub use rusty::RustyMidi;
