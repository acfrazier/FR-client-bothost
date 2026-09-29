//! The first song must not stall the audio callback.
//!
//! `Client::save_midi` used to hold the shared `midi` lock through
//! `RustyMidi::play`, which on the first song parses the MIDI, loads the
//! SoundFont and builds the synthesizer. The cpal output callback takes the
//! same lock to render, so it blocked for the whole build. This lives in its
//! own test binary because the SoundFont is a process-wide `OnceLock`: any
//! sibling test that already played a song would hide the cold load.
#![cfg(feature = "audio")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use client::client::{Client, ClientConfig};
use client::sound::RustyMidi;

/// The longest a callback may wait for the `midi` lock while the first song
/// is handed over. The cold parse + SoundFont load + synthesizer build held
/// the lock for ~15-25 ms on the dev machine (already longer than a typical
/// 10-23 ms device buffer); the swap under the lock is microseconds.
const MAX_CALLBACK_STALL: Duration = Duration::from_millis(5);

#[test]
fn first_song_does_not_stall_the_output_callback() {
    let engine = client::engine_dir();
    let font = engine.join("public/client/SCC1_Florestan.sf2");
    let song = client::content_dir().join("songs/scape main.mid");
    let (Ok(_), Ok(song)) = (std::fs::metadata(&font), std::fs::read(&song)) else {
        eprintln!("skipping: engine SoundFont/songs absent");
        return;
    };

    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.midi = Arc::new(Mutex::new(RustyMidi::new("/tmp")));

    // Stand-in for the cpal callback: lock, render one block, release.
    let midi = c.midi.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let max_wait_us = Arc::new(AtomicU64::new(0));
    let calls = Arc::new(AtomicU64::new(0));
    let callback = {
        let (stop, max_wait_us, calls) = (stop.clone(), max_wait_us.clone(), calls.clone());
        thread::spawn(move || {
            let mut left = vec![0f32; 512];
            let mut right = vec![0f32; 512];
            while !stop.load(Ordering::Relaxed) {
                let asked = Instant::now();
                let mut backend = midi.lock().unwrap();
                let waited = asked.elapsed().as_micros() as u64;
                backend.render(&mut left, &mut right);
                drop(backend);
                max_wait_us.fetch_max(waited, Ordering::Relaxed);
                calls.fetch_add(1, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    while calls.load(Ordering::Relaxed) < 5 {
        thread::sleep(Duration::from_millis(1));
    }

    let started = Instant::now();
    c.save_midi(&song, true);
    let save_took = started.elapsed();
    // Let the callback observe the freshly swapped-in song.
    let seen = calls.load(Ordering::Relaxed);
    while calls.load(Ordering::Relaxed) < seen + 5 {
        thread::sleep(Duration::from_millis(1));
    }
    stop.store(true, Ordering::Relaxed);
    callback.join().unwrap();

    let stall = Duration::from_micros(max_wait_us.load(Ordering::Relaxed));
    eprintln!("first song: save_midi took {save_took:?}, worst callback lock wait {stall:?}");

    assert!(
        c.midi_playing,
        "the first song must still start immediately"
    );
    assert!(
        c.midi.lock().unwrap().is_playing(),
        "the first song must be playing after save_midi"
    );
    let mut left = vec![0f32; 22050];
    let mut right = vec![0f32; 22050];
    c.midi.lock().unwrap().render(&mut left, &mut right);
    let peak = left
        .iter()
        .chain(right.iter())
        .fold(0f32, |p, s| p.max(s.abs()));
    assert!(peak > 0.01, "the first song must render audibly");
    assert!(
        stall < MAX_CALLBACK_STALL,
        "audio callback stalled {stall:?} on the first song (save_midi took {save_took:?})"
    );
}
