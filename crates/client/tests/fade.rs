//! The mandatory period fade: `saveMidi(fading=true)` ramps the current song
//! out to the floor (0.25 dB per 50 ms tick — tinymidipcm.js
//! `fadeStepDb`/`fadeInterval`, a documented proxy for the native player),
//! then `music_tick` swaps the new song in via `finish_fade` at the `midivol`
//! target. There is no fade-in leg; jingles and `stopMidi` hard-cut.

use client::sound::Fade;

#[test]
fn fade_out_ramps_down_not_up() {
    let mut f = Fade::new();
    f.fade_out();
    let g0 = f.gain();
    f.step_ms(50);
    let g1 = f.gain();
    assert!(g1 < g0); // moving, and only downwards
}

#[test]
fn finish_fade_jumps_to_target() {
    let mut f = Fade::new();
    f.finish_fade(0);
    assert!((f.gain() - 1.0).abs() < 1e-4);
}

#[test]
fn stop_hard_is_silence() {
    let mut f = Fade::new();
    f.finish_fade(0);
    f.stop_hard();
    assert_eq!(f.gain(), 0.0);
}

/// The full fade-out lands on the floor, latches `swap_due` once, and the
/// swap-in jumps straight to the ladder value and holds (144 fade-out steps
/// to -36 dB, then `finish_fade` to -4 dB — no fade-in).
#[test]
fn fade_out_then_swap_lands_on_target_and_holds() {
    let mut f = Fade::new();
    f.fade_out();
    for _ in 0..200 {
        f.step_ms(50);
        if f.swap_due() {
            break;
        }
    }
    f.finish_fade(-400);
    let expected = 10f32.powf(-400.0 / 2000.0); // -4 dB
    assert!((f.gain() - expected).abs() < 1e-3);
    let held = f.gain();
    f.step_ms(1000);
    assert_eq!(f.gain(), held);
}

#[test]
fn finish_fade_jumps_to_ladder_volumes() {
    for (midivol, expect) in [
        (0, 1.0),
        (-400, 10f32.powf(-400.0 / 2000.0)),
        (-800, 10f32.powf(-800.0 / 2000.0)),
        (-1200, 10f32.powf(-1200.0 / 2000.0)),
    ] {
        let mut f = Fade::new();
        f.finish_fade(midivol);
        assert!((f.gain() - expect).abs() < 1e-4, "midivol {midivol}");
    }
}

#[test]
fn set_target_vol_jumps_when_not_fading() {
    let mut f = Fade::new();
    f.set_target_vol(-400); // −4 dB
    assert!((f.gain() - 0.6310).abs() < 1e-3);
}

#[test]
fn set_target_vol_retargets_mid_ramp() {
    let mut f = Fade::new();
    f.fade_out();
    f.set_target_vol(-400); // retargets only while fading
    for _ in 0..200 {
        f.step_ms(50);
        if f.swap_due() {
            break;
        }
    }
    f.finish_fade(-400);
    let expected = 10f32.powf(-400.0 / 2000.0);
    assert!((f.gain() - expected).abs() < 1e-3);
}

#[test]
fn step_ms_accumulates_to_ticks() {
    let mut f = Fade::new();
    f.fade_out();
    f.step_ms(25);
    let g1 = f.gain();
    f.step_ms(25);
    let g2 = f.gain();
    assert_ne!(g1, g2); // two half ticks = one 0.25 dB step
}

#[test]
fn stop_hard_then_finish_fade_recovers() {
    let mut f = Fade::new();
    f.fade_out();
    f.step_ms(50);
    f.stop_hard();
    assert_eq!(f.gain(), 0.0);
    f.finish_fade(0);
    assert!((f.gain() - 1.0).abs() < 1e-4);
}
