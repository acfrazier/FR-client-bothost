//! Midi backend: the 274 `signlink` control plane condensed into one trait.
//! `saveMidi`/`stopMidi`/`setMidiVolume` become `play`/`stop`/`set_volume`;
//! volumes are the 274 `midivol` ladder 0 / -400 / -800 / -1200 (1/100 dB),
//! applied by the output `Fade` — the backend plays raw.

use std::any::Any;

/// A song made ready off the shared `midi` lock by a [`SongPreparer`]:
/// parsed, and (first play) with its synthesizer built. Opaque to the
/// client; only the backend that issued the preparer can start it.
pub struct PreparedSong(pub Box<dyn Any + Send>);

/// The slow half of [`Midi::play`], detached from the backend so the caller
/// can run it without holding the lock the audio callback renders under.
pub type SongPreparer = Box<dyn FnOnce(&[u8]) -> PreparedSong + Send>;

/// The slow half of starting a song the backend held for the SoundFont load
/// (synthesizer build only; the song is already parsed), detached like a
/// [`SongPreparer`] so it runs without the shared `midi` lock.
pub type DeferredStart = Box<dyn FnOnce() -> PreparedSong + Send>;

pub trait Midi: Send {
    /// `saveMidi(fading, data)`: start the on-demand archive-2 bytes. The
    /// 274 `midivol` ladder is applied by the output `Fade`, never here.
    /// Returns true when the backend accepted the bytes and started them;
    /// false when the file cannot be played (parse failure), in which case
    /// the caller must not restore the fade gain.
    fn play(&mut self, data: &[u8], volume: i32, fading: bool) -> bool;
    /// The slow half of `play` (MIDI parse; on first play the SoundFont load
    /// and synthesizer build) as a closure that needs no backend borrow.
    /// `Client::save_midi` takes it under a short lock, runs it with the lock
    /// released so the audio callback keeps rendering, then swaps the result
    /// in with [`Midi::play_prepared`]. `None` (the default) means the
    /// backend's `play` is cheap and is simply called under the lock.
    fn preparer(&self) -> Option<SongPreparer> {
        None
    }
    /// The quick half of `play`: start a song a [`SongPreparer`] from this
    /// backend produced. Same return contract as `play`. Only backends that
    /// return a preparer are ever handed a prepared song.
    fn play_prepared(&mut self, _song: PreparedSong, _volume: i32, _fading: bool) -> bool {
        false
    }
    /// True while the backend holds a song it could not start yet because the
    /// host's background SoundFont load (`preload_sound_font_async`) is still
    /// running. Only the `audio` backend ever defers; the song is released by
    /// [`Midi::take_deferred_start`].
    fn awaiting_sound_font(&self) -> bool {
        false
    }
    /// Release the held song once the SoundFont is ready: the returned start
    /// builds its synthesizer off the lock (no file read left to do) and its
    /// result goes to [`Midi::play_prepared`]. `None` while the load is still
    /// running or when nothing is held.
    fn take_deferred_start(&mut self) -> Option<DeferredStart> {
        None
    }
    /// `stopMidi()`: silence the current song. A song still held for the
    /// SoundFont load is dropped too.
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, OnceLock};

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

    use super::{DeferredStart, Midi, PreparedSong, SongPreparer};

    const SAMPLE_RATE: i32 = 22050;

    /// The process-wide SoundFont: `None` inside means "looked, none found".
    static FONT: OnceLock<Option<Arc<SoundFont>>> = OnceLock::new();
    /// Set by [`preload_sound_font_async`]: the host loads the SoundFont on a
    /// background thread, so a play must never touch the disk itself.
    static HOST_PRELOAD: AtomicBool = AtomicBool::new(false);

    /// The SoundFont search: first readable candidate wins; logs once when
    /// none is found.
    fn load_sound_font(cache_dir: &str) -> Option<Arc<SoundFont>> {
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
    }

    /// Lazy synchronous load (the standalone client path): the first caller
    /// reads the file, everyone else waits for that one load.
    fn shared_sound_font(cache_dir: &str) -> Option<Arc<SoundFont>> {
        FONT.get_or_init(|| load_sound_font(cache_dir)).clone()
    }

    /// What a play sees of the SoundFont.
    enum FontLookup {
        /// Loaded (or known missing: `None`).
        Ready(Option<Arc<SoundFont>>),
        /// The host's background load has not finished. Never blocks, never
        /// reads a file.
        Loading,
    }

    fn font_for_play(cache_dir: &str) -> FontLookup {
        if HOST_PRELOAD.load(Ordering::Acquire) {
            FONT.get()
                .map_or(FontLookup::Loading, |font| FontLookup::Ready(font.clone()))
        } else {
            FontLookup::Ready(shared_sound_font(cache_dir))
        }
    }

    /// Host opt-in: load the SoundFont once on a background thread so no song
    /// ever reads the file on the caller's (slot) thread. After this call a
    /// play that arrives before the load finishes is held by the backend and
    /// started by `Client::music_tick` once the font is ready. A missing or
    /// unreadable font ends as "silent", logged once, never probed again.
    /// Idempotent: the first call's `cache_dir` is the one searched. Without
    /// this call the backend keeps its lazy synchronous load.
    pub fn preload_sound_font_async(cache_dir: &str) {
        spawn_preload(cache_dir, || {});
    }

    fn spawn_preload(
        cache_dir: &str,
        before_load: impl FnOnce() + Send + 'static,
    ) -> Option<std::thread::JoinHandle<()>> {
        if HOST_PRELOAD.swap(true, Ordering::AcqRel) {
            return None;
        }
        let cache_dir = cache_dir.to_string();
        let spawned = std::thread::Builder::new()
            .name("soundfont-preload".into())
            .spawn(move || {
                before_load();
                shared_sound_font(&cache_dir);
            });
        match spawned {
            Ok(handle) => Some(handle),
            Err(_) => {
                // No thread, no background load: fall back to the lazy load.
                HOST_PRELOAD.store(false, Ordering::Release);
                None
            }
        }
    }

    /// Test seam: like [`preload_sound_font_async`], but the loader thread
    /// waits for `gate` (a message or the sender's drop) before it touches
    /// the disk, so a test controls exactly when the load finishes. Returns
    /// the loader's handle; joining it means the font state is final.
    #[cfg(feature = "test-support")]
    pub fn preload_sound_font_async_gated(
        cache_dir: &str,
        gate: std::sync::mpsc::Receiver<()>,
    ) -> Option<std::thread::JoinHandle<()>> {
        spawn_preload(cache_dir, move || {
            let _ = gate.recv();
        })
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
        /// The newest song that arrived while the host's background
        /// SoundFont load was still running, already parsed; see
        /// `take_deferred_start`.
        deferred: Option<Arc<MidiFile>>,
    }

    impl RustyMidi {
        pub fn new(cache_dir: &str) -> Self {
            Self {
                cache_dir: cache_dir.to_string(),
                sequencer: None,
                deferred: None,
            }
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
                deferred: None,
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
        /// Test-support entry point for the RPG Maker loop path. It uses the same
        /// parser, prepared-song handling, sequencer, and render path as normal
        /// playback; only the sequencer's loop flag is selectable here.
        #[cfg(feature = "test-support")]
        pub fn play_rpgmaker_with_loop_for_test(&mut self, data: &[u8], enabled: bool) -> bool {
            let prepared = prepare(&self.cache_dir, self.sequencer.is_none(), data);
            self.play_prepared_with_loop(PreparedSong(Box::new(prepared)), enabled)
        }

        fn play_prepared_with_loop(&mut self, song: PreparedSong, play_loop: bool) -> bool {
            let Ok(prepared) = song.0.downcast::<Prepared>() else {
                // Not from this backend's preparer: nothing was started.
                return false;
            };
            let Prepared { sequencer, outcome } = *prepared;
            if self.sequencer.is_none() {
                self.sequencer = sequencer;
            }
            let midi_file = match outcome {
                Outcome::AwaitingFont(song) => {
                    // The newest valid song wins; the slot thread did no disk
                    // work.
                    self.deferred = Some(song);
                    return true;
                }
                Outcome::Silent => {
                    self.deferred = None;
                    return true;
                }
                // A rejected song keeps the old state, a held song included.
                Outcome::Rejected => return false,
                Outcome::Song(midi_file) => midi_file,
            };
            self.deferred = None;
            let Some(sequencer) = &mut self.sequencer else {
                return true;
            };
            sequencer.play(&midi_file, play_loop);
            true
        }
    }

    /// What a [`RustyMidi`] preparer hands back: the parsed song, and the
    /// sequencer when this was the first play and it had to be built.
    struct Prepared {
        /// A freshly built sequencer for a backend that had none.
        sequencer: Option<MidiFileSequencer>,
        outcome: Outcome,
    }

    enum Outcome {
        /// No SoundFont/synthesizer: the backend accepts and stays silent,
        /// like `NullMidi`; there is nothing to leave playing.
        Silent,
        /// The host's background SoundFont load is still running: the parsed
        /// song is held until it finishes. Nothing was read from disk.
        AwaitingFont(Arc<MidiFile>),
        /// The bytes are not a MIDI file the backend can play.
        Rejected,
        Song(Arc<MidiFile>),
    }

    /// The slow half of a play: SoundFont load + synthesizer build (only when
    /// the backend has no sequencer yet) and the MIDI parse. Touches no
    /// backend state, so it runs without the shared lock.
    fn prepare(cache_dir: &str, needs_sequencer: bool, data: &[u8]) -> Prepared {
        let sequencer = if needs_sequencer {
            match font_for_play(cache_dir) {
                FontLookup::Loading => {
                    // Parsing needs no SoundFont and no disk: validate now, so
                    // garbage never replaces a valid held song.
                    return Prepared {
                        sequencer: None,
                        outcome: match parse_song(data) {
                            Some(song) => Outcome::AwaitingFont(song),
                            None => Outcome::Rejected,
                        },
                    };
                }
                FontLookup::Ready(font) => font.and_then(|font| sequencer_from_font(&font)),
            }
        } else {
            None
        };
        if needs_sequencer && sequencer.is_none() {
            return Prepared {
                sequencer,
                outcome: Outcome::Silent,
            };
        }
        // A rejected swap-in must not silently keep the old song: the caller
        // leaves the fade at the floor, so the previous song cannot come back
        // at full volume.
        let outcome = match parse_song(data) {
            Some(midi_file) => Outcome::Song(midi_file),
            None => Outcome::Rejected,
        };
        Prepared { sequencer, outcome }
    }

    fn parse_song(data: &[u8]) -> Option<Arc<MidiFile>> {
        MidiFile::new_with_loop_type(&mut Cursor::new(data), MidiFileLoopType::RpgMaker)
            .ok()
            .map(Arc::new)
    }

    impl Midi for RustyMidi {
        fn play(&mut self, data: &[u8], volume: i32, fading: bool) -> bool {
            let prepared = prepare(&self.cache_dir, self.sequencer.is_none(), data);
            self.play_prepared(PreparedSong(Box::new(prepared)), volume, fading)
        }

        fn preparer(&self) -> Option<SongPreparer> {
            let cache_dir = self.cache_dir.clone();
            let needs_sequencer = self.sequencer.is_none();
            Some(Box::new(move |data| {
                PreparedSong(Box::new(prepare(&cache_dir, needs_sequencer, data)))
            }))
        }

        fn play_prepared(&mut self, song: PreparedSong, _volume: i32, _fading: bool) -> bool {
            // One-shot, like the native player behind Java `midisave`: a
            // finished song reports `is_playing()` false, so the next
            // `saveMidi(fading=true)` replaces the file immediately. Loop
            // playback stays behind the `test-support` entry point.
            self.play_prepared_with_loop(song, false)
        }

        fn is_playing(&self) -> bool {
            self.sequencer
                .as_ref()
                .is_some_and(|s| !s.end_of_sequence())
        }

        fn awaiting_sound_font(&self) -> bool {
            self.deferred.is_some()
        }

        fn take_deferred_start(&mut self) -> Option<DeferredStart> {
            // `FONT.get()` never blocks: ready means loaded or known missing.
            let font = FONT.get()?.clone();
            let song = self.deferred.take()?;
            Some(Box::new(move || {
                let sequencer = font.and_then(|font| sequencer_from_font(&font));
                let outcome = if sequencer.is_some() {
                    Outcome::Song(song)
                } else {
                    Outcome::Silent
                };
                PreparedSong(Box::new(Prepared { sequencer, outcome }))
            }))
        }

        fn stop(&mut self) {
            self.deferred = None;
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

#[cfg(all(feature = "audio", feature = "test-support"))]
pub use rusty::preload_sound_font_async_gated;
#[cfg(feature = "audio")]
pub use rusty::{preload_sound_font_async, RustyMidi};
