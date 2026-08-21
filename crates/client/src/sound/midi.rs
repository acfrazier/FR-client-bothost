//! Midi backend: the 274 `signlink` control plane condensed into one trait.
//! `saveMidi`/`stopMidi`/`setMidiVolume` become `play`/`stop`/`set_volume`;
//! volumes are the 274 `midivol` ladder 0 / -400 / -800 / -1200 (1/100 dB),
//! applied by the output `Fade` — the backend plays raw.

pub trait Midi: Send {
    /// `saveMidi(fading, data)`: start the on-demand archive-2 bytes. The
    /// 274 `midivol` ladder is applied by the output `Fade`, never here.
    fn play(&mut self, data: &[u8], volume: i32, fading: bool);
    /// `stopMidi()`: silence the current song.
    fn stop(&mut self);
    /// True while the backend is still rendering. Headless backends report
    /// true (the control plane owns the fade); the audio backend reports
    /// the sequencer state, so `saveMidi(fading=true)` can take Java's
    /// immediate-midisave arm once the current song reached EOF
    /// (`Client.java` 6266: midisave replaces the file now).
    fn is_playing(&self) -> bool {
        true
    }
    /// `setMidiVolume(active, volume)`: the "voladjust" poke. The backend
    /// has no gain stage (volume lives on the output `Fade`), so the trait
    /// keeps this as a documented no-op.
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
    use std::path::Path;
    use std::sync::Arc;

    fn sound_font_candidates(cache_dir: &str) -> Vec<String> {
        let mut out = vec![
            format!("{cache_dir}/soundfont.sf2"),
            format!("{cache_dir}/Florestan.sf2"),
            format!("{cache_dir}/SCC1_Florestan.sf2"),
            "Florestan.sf2".into(),
            "SCC1_Florestan.sf2".into(),
        ];
        // pack/client → engine/public/client (three parents up).
        if let Some(engine) = Path::new(cache_dir).parent().and_then(|p| p.parent()).and_then(|p| p.parent()) {
            out.push(engine.join("public/client/SCC1_Florestan.sf2").display().to_string());
            out.push(engine.join("public/bot/SCC1_Florestan.sf2").display().to_string());
        }
        if let Ok(home) = std::env::var("HOME") {
            out.push(format!("{home}/experiments/Server/engine/public/client/SCC1_Florestan.sf2"));
            out.push(format!("{home}/experiments/Server/engine/public/bot/SCC1_Florestan.sf2"));
        }
        out
    }

    use rustysynth::{MidiFile, MidiFileLoopType, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};

    use super::Midi;

    const SAMPLE_RATE: i32 = 22050;

    /// The `audio` backend: one rustysynth sequencer and one SF2. The SF2 is
    /// picked up from `cache_dir` (`soundfont.sf2` / `Florestan.sf2`) or the
    /// current directory; without one the backend stays silent. The fade and
    /// the `midivol` ladder live entirely on the output `Fade`, so the
    /// backend plays raw. The synthesizer keeps its own `Arc<SoundFont>` and
    /// the sequencer keeps its own `Arc<MidiFile>`, so neither is stored
    /// here after construction.
    pub struct RustyMidi {
        sequencer: Option<MidiFileSequencer>,
    }

    impl RustyMidi {
        pub fn new(cache_dir: &str) -> Self {
            for candidate in sound_font_candidates(cache_dir) {
                if let Some(midi) = Self::with_sound_font(&candidate) {
                    return midi;
                }
            }
            eprintln!(
                "audio: no soundfont (looked for Florestan.sf2 / SCC1_Florestan.sf2 under {cache_dir} and engine/public); midi silent"
            );
            Self { sequencer: None }
        }

        /// True when an SF2 loaded and rustysynth can render.
        pub fn has_sound_font(&self) -> bool {
            self.sequencer.is_some()
        }

        /// Load an explicit SF2; a missing/unreadable font yields a silent
        /// backend (spec: "Midi missing, audio on → warn + silence").
        pub fn with_sound_font(path: &str) -> Option<Self> {
            let file = File::open(path).ok()?;
            let mut file = file;
            let sound_font = Arc::new(SoundFont::new(&mut file).ok()?);
            let mut settings = SynthesizerSettings::new(SAMPLE_RATE);
            // 274 has no chorus/reverb engine (Java plays midi dry), and
            // rustysynth defaults both on; the reverb tail pushes dense
            // songs like scape_main toward the i16 rail. Keep master_volume
            // at rustysynth's default 0.5.
            settings.enable_reverb_and_chorus = false;
            let synthesizer = Synthesizer::new(&sound_font, &settings).ok()?;
            let sequencer = MidiFileSequencer::new(synthesizer);
            Some(Self { sequencer: Some(sequencer) })
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
        fn play(&mut self, data: &[u8], _volume: i32, _fading: bool) {
            let Some(sequencer) = &mut self.sequencer else { return };
            let Ok(midi_file) = MidiFile::new_with_loop_type(
                &mut Cursor::new(data),
                MidiFileLoopType::RpgMaker,
            ) else {
                return;
            };
            let midi_file = Arc::new(midi_file);
            // One-shot, like the native player behind Java `midisave`: a
            // finished song reports `is_playing()` false, so the next
            // `saveMidi(fading=true)` replaces the file immediately.
            sequencer.play(&midi_file, false);
        }

        fn is_playing(&self) -> bool {
            self.sequencer.as_ref().is_some_and(|s| !s.end_of_sequence())
        }

        fn stop(&mut self) {
            if let Some(sequencer) = &mut self.sequencer {
                sequencer.stop();
            }
        }

        /// `setMidiVolume`: the 274 `midivol` ladder is applied by the
        /// output `Fade`; the backend has no gain stage, so this is a
        /// documented no-op.
        fn set_volume(&mut self, _volume: i32) {}

        fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
            RustyMidi::render(self, left, right);
        }
    }
}

#[cfg(feature = "audio")]
pub use rusty::RustyMidi;
