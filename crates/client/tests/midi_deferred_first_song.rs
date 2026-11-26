//! With the host hook (`preload_sound_font_async`) the first song never reads
//! the SoundFont on the slot thread, even when it arrives while the
//! background load is still running.
//!
//! The SoundFont state is process-wide and one-way, so this lives in its own
//! test binary. The loader thread is gated: it does not touch the disk until
//! the test releases it, so "the load is still running" is a state the test
//! holds, not a race it hopes to win. Three clients share that one loading
//! window: one plays, one is stopped, one is superseded by a newer song and
//! one gets garbage bytes after a valid song (the garbage is rejected).
#![cfg(all(feature = "audio", feature = "test-support"))]

#[path = "common/isolation.rs"]
mod isolation;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use client::client::ClientConfig;
use client::sound::midi::preload_sound_font_async_gated;
use client::sound::{Midi, RustyMidi};

/// A valid one-track MIDI whose only note sounds 8192 ticks (about 8.5 s)
/// after the start: it plays, but the first second is silent.
const LATE_NOTE_MIDI: &[u8] = &[
    b'M', b'T', b'h', b'd', 0, 0, 0, 6, 0, 0, 0, 1, 0x01, 0xE0, // format 0, 480 ticks
    b'M', b'T', b'r', b'k', 0, 0, 0, 16, //
    0x00, 0xC0, 0x00, // program change
    0xC0, 0x00, 0x90, 0x3C, 0x40, // +8192 ticks: note on
    0x00, 0x80, 0x3C, 0x40, // note off
    0x00, 0xFF, 0x2F, 0x00, // end of track
];

struct Slot {
    client: isolation::IsolatedClient,
    midi: Arc<Mutex<RustyMidi>>,
}

fn slot() -> Slot {
    let mut client = isolation::IsolatedClient::with_default_config(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: String::new(),
        members: true,
        lowmem: false,
    });
    let midi = Arc::new(Mutex::new(RustyMidi::new("/tmp")));
    client.midi = midi.clone();
    Slot { client, midi }
}

impl Slot {
    fn peak_of_one_second(&self) -> f32 {
        let mut left = vec![0f32; 22050];
        let mut right = vec![0f32; 22050];
        self.midi.lock().unwrap().render(&mut left, &mut right);
        left.iter()
            .chain(right.iter())
            .fold(0f32, |p, s| p.max(s.abs()))
    }
}

#[test]
fn songs_arriving_during_the_background_font_load_are_held_not_loaded() {
    let engine = client::engine_dir();
    let font = engine.join("public/client/SCC1_Florestan.sf2");
    let song = client::content_dir().join("songs/scape main.mid");
    let (Ok(_), Ok(song)) = (std::fs::metadata(&font), std::fs::read(&song)) else {
        eprintln!("skipping: engine SoundFont/songs absent");
        return;
    };

    // A save_midi that blocked on the loader would hang this binary.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        eprintln!("save_midi blocked on the gated SoundFont loader");
        std::process::abort();
    });

    let (release, gate) = mpsc::channel();
    let loader = preload_sound_font_async_gated("/tmp", gate)
        .expect("the first host preload starts a loader");

    let mut played = slot();
    let mut stopped = slot();
    let mut superseded = slot();
    let mut kept = slot();

    // The loader is parked before its first file read. A slot-thread load
    // would leave a SoundFont behind; a blocking wait would never return.
    played.client.save_midi(&song, true);
    stopped.client.save_midi(&song, true);
    stopped.client.stop_midi();
    superseded.client.save_midi(&song, true);
    superseded.client.save_midi(LATE_NOTE_MIDI, true);
    kept.client.save_midi(&song, true);
    kept.client.save_midi(b"not a midi file", true);
    for s in [&mut played, &mut stopped, &mut superseded, &mut kept] {
        s.client.music_tick();
        assert!(
            !s.midi.lock().unwrap().has_sound_font(),
            "no SoundFont may be loaded on the slot thread while the host load runs"
        );
        assert_eq!(
            s.peak_of_one_second(),
            0.0,
            "nothing sounds before the font"
        );
    }
    assert!(played.client.midi_playing, "the song is accepted at once");
    assert!(played.client.midi_deferred, "and held for the font");
    assert!(!played.midi.lock().unwrap().is_playing());
    assert!(
        !stopped.client.midi_deferred,
        "stop_midi drops the held song"
    );

    release.send(()).unwrap();
    loader.join().unwrap();

    for s in [&mut played, &mut stopped, &mut superseded, &mut kept] {
        s.client.music_tick();
    }

    // The held first song starts once the font is there, and is audible.
    assert!(played.client.midi_playing && !played.client.midi_deferred);
    assert!(played.midi.lock().unwrap().is_playing());
    assert!(
        played.peak_of_one_second() > 0.01,
        "the first song must render audibly"
    );

    // A stopped song never comes back.
    assert!(!stopped.client.midi_playing);
    assert!(!stopped.midi.lock().unwrap().is_playing());
    assert!(!stopped.midi.lock().unwrap().has_sound_font());
    assert_eq!(stopped.peak_of_one_second(), 0.0);

    // The newer song replaced the held first one: the late-note song plays
    // (silent for its first second) instead of the audible first song.
    assert!(superseded.client.midi_playing && !superseded.client.midi_deferred);
    assert!(superseded.midi.lock().unwrap().is_playing());
    assert!(
        superseded.peak_of_one_second() < 1e-4,
        "the superseded song must not start"
    );

    // Garbage after a valid held song is rejected and does not replace it:
    // the valid first song still starts once the font is there.
    assert!(kept.client.midi_playing && !kept.client.midi_deferred);
    assert!(kept.midi.lock().unwrap().is_playing());
    assert!(
        kept.peak_of_one_second() > 0.01,
        "rejected bytes must not replace the held song"
    );
}
