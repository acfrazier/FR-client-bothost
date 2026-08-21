use std::sync::{Arc, Mutex};

use client::sound::{Fade, Midi};

#[test]
fn fade_out_reaches_floor_then_signals_swap() {
    let mut f = Fade::new();
    f.fade_out();
    let mut due = false;
    for _ in 0..200 {
        f.step_ms(50);
        if f.swap_due() {
            due = true;
            break;
        }
    }
    assert!(due); // 144 × 50 ms = 7.2 s → −36 dB
    f.finish_fade(0);
    assert!((f.gain() - 1.0).abs() < 1e-6);
}

#[test]
fn set_target_vol_jumps_when_not_fading() {
    let mut f = Fade::new();
    f.set_target_vol(-400); // −4 dB
    assert!((f.gain() - 0.6310).abs() < 1e-3);
}

fn client() -> client::client::Client {
    client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// The first song plays now even with `fading=true` (nothing to fade out):
/// Java's first-song short-circuit.
#[test]
fn first_song_plays_immediately() {
    let mut c = client();
    c.save_midi(&[1, 2, 3], true);
    assert!(c.midi_playing);
    assert!(c.midi_pending.is_none());
}

/// A zone-song change while something plays holds the bytes pending and arms
/// the fade-out. Headless the audio callback never steps the fade, so no
/// swap fires — acceptable (no device → no output).
#[test]
fn zone_song_change_holds_pending_until_swap() {
    let mut c = client();
    c.save_midi(&[1, 2, 3], true); // first song: plays now
    c.save_midi(&[4, 5, 6], true); // zone change: held pending
    assert!(c.midi_pending.is_some());
    c.music_tick();
    assert!(c.midi_pending.is_some());
}

/// A jingle interrupts any song immediately at the current volume.
#[test]
fn jingle_plays_immediately_even_while_song_plays() {
    let mut c = client();
    c.save_midi(&[1, 2, 3], true);
    c.save_midi(&[4, 5, 6], false); // jingle
    assert!(c.midi_pending.is_none());
    assert!(c.midi_playing);
}

/// Driving the shared fade to the floor (as the audio callback would)
/// latches `swap_due`, and `music_tick` swaps the pending song in.
#[test]
fn music_tick_swaps_pending_after_fade_out() {
    let mut c = client();
    c.save_midi(&[1, 2, 3], true);
    c.midi_volume = -800;
    c.save_midi(&[4, 5, 6], true);
    {
        let mut fade = c.fade.lock().unwrap();
        fade.step_ms(200 * 50); // well past the 144 ticks to -36 dB
    }
    c.music_tick();
    assert!(c.midi_pending.is_none());
    assert!(c.midi_playing);
    assert!((c.fade.lock().unwrap().gain() - 10f32.powf(-800.0 / 2000.0)).abs() < 1e-3);
}

/// `stopMidi` drops any held pending swap and marks nothing playing.
#[test]
fn stop_midi_drops_pending() {
    let mut c = client();
    c.save_midi(&[1, 2, 3], true);
    c.save_midi(&[4, 5, 6], true);
    c.stop_midi();
    assert!(c.midi_pending.is_none());
    assert!(!c.midi_playing);
}

/// A fake backend whose sequencer has already reached EOF (`is_playing`
/// false) — the state of the title player once `scape_main` has finished.
struct EndedMidi;

impl Midi for EndedMidi {
    fn play(&mut self, _data: &[u8], _volume: i32, _fading: bool) {}
    fn stop(&mut self) {}
    fn set_volume(&mut self, _volume: i32) {}
    fn is_playing(&self) -> bool {
        false
    }
}

/// After the title song's sequencer reached EOF, a zone-song change plays
/// immediately: Java `midisave` replaces the file now (nothing left to
/// fade out), so the bytes must not sit pending waiting for the ramp.
#[test]
fn zone_song_plays_immediately_after_backend_eof() {
    let mut c = client();
    c.midi = Arc::new(Mutex::new(EndedMidi));
    c.save_midi(&[1, 2, 3], true); // title scape_main: first song plays now
    assert!(c.midi_playing);
    c.save_midi(&[4, 5, 6], true); // zone change: backend already at EOF
    assert!(c.midi_pending.is_none());
    assert!(c.midi_playing);
}

/// The `nextMusicDelay` countdown from Java `soundsDoQueue` (`Client.java`
/// 1997-2008): each pass subtracts 20; at zero the next zone song is
/// re-requested with `midi_fading` set (the jingle → zone restore path).
#[test]
fn music_delay_counts_down_and_requests_next_song() {
    let mut c = client();
    c.next_midi_song = 42;
    c.next_music_delay = 40;
    c.sounds_do_queue();
    assert_eq!(c.next_music_delay, 20);
    assert_eq!(c.midi_song, -1); // still counting down
    c.sounds_do_queue();
    assert_eq!(c.next_music_delay, 0);
    assert_eq!(c.midi_song, 42);
    assert!(c.midi_fading);
}

/// Muted midi skips the re-request even once the countdown hits zero
/// (Java gates on `midiActive && !lowMem`).
#[test]
fn music_delay_zero_does_not_requeue_when_midi_inactive() {
    let mut c = client();
    c.midi_active = false;
    c.next_midi_song = 7;
    c.next_music_delay = 20;
    c.sounds_do_queue();
    assert_eq!(c.next_music_delay, 0);
    assert_eq!(c.midi_song, -1);
}
