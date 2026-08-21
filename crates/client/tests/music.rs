use client::sound::Fade;

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
