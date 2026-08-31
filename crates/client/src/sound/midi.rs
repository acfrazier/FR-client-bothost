//! Midi backend: the 274 `signlink` control plane condensed into one trait.
//! `saveMidi`/`stopMidi`/`setMidiVolume` become `play`/`stop`/`set_volume`;
//! volumes are the 274 `midivol` ladder 0 / -400 / -800 / -1200 (1/100 dB),
//! applied by the output `Fade` — the backend plays raw.

pub trait Midi: Send {
    /// `saveMidi(fading, data)`: start the on-demand archive-2 bytes. The
    /// 274 `midivol` ladder is applied by the output `Fade`, never here.
    /// Returns true when the backend accepted the bytes and started them;
    /// false when the file cannot be played (parse failure), in which case
    /// the caller must not restore the fade gain.
    fn play(&mut self, data: &[u8], volume: i32, fading: bool) -> bool;
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
    fn play(&mut self, _data: &[u8], _volume: i32, _fading: bool) -> bool {
        true
    }
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
        if let Some(engine) = Path::new(cache_dir)
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            out.push(
                engine
                    .join("public/client/SCC1_Florestan.sf2")
                    .display()
                    .to_string(),
            );
            out.push(
                engine
                    .join("public/bot/SCC1_Florestan.sf2")
                    .display()
                    .to_string(),
            );
        }
        let engine = crate::engine_dir();
        out.push(
            engine
                .join("public/client/SCC1_Florestan.sf2")
                .display()
                .to_string(),
        );
        out.push(
            engine
                .join("public/bot/SCC1_Florestan.sf2")
                .display()
                .to_string(),
        );
        out
    }

    use rustysynth::{
        MidiFile, MidiFileLoopType, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings,
    };

    use super::Midi;

    const SAMPLE_RATE: i32 = 22050;

    fn shared_sound_font(cache_dir: &str) -> Option<Arc<SoundFont>> {
        static FONT: std::sync::OnceLock<Option<Arc<SoundFont>>> = std::sync::OnceLock::new();
        FONT.get_or_init(|| {
            for candidate in sound_font_candidates(cache_dir) {
                if let Ok(mut file) = File::open(&candidate) {
                    if let Ok(font) = SoundFont::new(&mut file) {
                        return Some(Arc::new(font));
                    }
                }
            }
            eprintln!(
                "audio: no soundfont (looked for Florestan.sf2 / SCC1_Florestan.sf2 under {cache_dir} and engine/public); midi silent"
            );
            None
        })
        .clone()
    }

    fn sequencer_from_font(sound_font: &Arc<SoundFont>) -> Option<MidiFileSequencer> {
        let mut settings = SynthesizerSettings::new(SAMPLE_RATE);
        // 274 has no chorus/reverb engine (Java plays midi dry), and
        // rustysynth defaults both on; the reverb tail pushes dense
        // songs like scape_main toward the i16 rail. Keep master_volume
        // at rustysynth's default 0.5.
        settings.enable_reverb_and_chorus = false;
        let synthesizer = Synthesizer::new(sound_font, &settings).ok()?;
        Some(MidiFileSequencer::new(synthesizer))
    }

    /// The `audio` backend: one rustysynth sequencer per slot that actually
    /// plays, sharing one process `Arc<SoundFont>`. `new` does not load the
    /// SF2 or build a synthesizer — fifty unheaded Clients were each paying
    /// tens of MB at spawn.
    pub struct RustyMidi {
        cache_dir: String,
        sequencer: Option<MidiFileSequencer>,
    }

    impl RustyMidi {
        pub fn new(cache_dir: &str) -> Self {
            Self {
                cache_dir: cache_dir.to_string(),
                sequencer: None,
            }
        }

        fn ensure_sequencer(&mut self) {
            if self.sequencer.is_some() {
                return;
            }
            let Some(font) = shared_sound_font(&self.cache_dir) else {
                return;
            };
            self.sequencer = sequencer_from_font(&font);
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
            let sequencer = sequencer_from_font(&sound_font)?;
            Some(Self {
                cache_dir: String::new(),
                sequencer: Some(sequencer),
            })
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
        fn play(&mut self, data: &[u8], _volume: i32, _fading: bool) -> bool {
            self.ensure_sequencer();
            let Some(sequencer) = &mut self.sequencer else {
                // No soundfont: the backend accepts (and stays silent), like
                // `NullMidi`; there is nothing to leave playing.
                return true;
            };
            let Ok(midi_file) =
                MidiFile::new_with_loop_type(&mut Cursor::new(data), MidiFileLoopType::RpgMaker)
            else {
                // A rejected swap-in must not silently keep the old song: the
                // caller leaves the fade at the floor, so the previous song
                // cannot come back at full volume.
                return false;
            };
            let midi_file = Arc::new(midi_file);
            // One-shot, like the native player behind Java `midisave`: a
            // finished song reports `is_playing()` false, so the next
            // `saveMidi(fading=true)` replaces the file immediately.
            sequencer.play(&midi_file, false);
            true
        }

        fn is_playing(&self) -> bool {
            self.sequencer
                .as_ref()
                .is_some_and(|s| !s.end_of_sequence())
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
