//! The mandatory period fade: `saveMidi(fading=true)` ramps the output gain
//! out then in toward the `midivol` target (0.25 dB per 50 ms tick,
//! tinymidipcm.js `fadeStepDb`/`fadeInterval`); jingles/`stopMidi` hard-cut.

use client::sound::Fade;

#[test]
fn fade_true_does_not_jump() {
    let mut f = Fade::new();
    f.begin_song(true, 0); // target 0 dB
    let g0 = f.gain();
    f.step_ms(50);
    let g1 = f.gain();
    assert_ne!(g0, g1); // moving
}

#[test]
fn fade_false_jumps_to_target() {
    let mut f = Fade::new();
    f.begin_song(false, 0);
    assert!((f.gain() - 1.0).abs() < 1e-4);
}

#[test]
fn stop_hard_is_silence() {
    let mut f = Fade::new();
    f.begin_song(false, 0);
    f.stop_hard();
    assert_eq!(f.gain(), 0.0);
}

/// The full out-then-in ramp lands exactly on the ladder value and holds
/// (144 fade-out steps to -36 dB, then back up to -4 dB).
#[test]
fn fade_true_ramp_lands_on_target_and_holds() {
    let mut f = Fade::new();
    f.begin_song(true, -400);
    for _ in 0..272 {
        f.step_ms(50);
    }
    let expected = 10f32.powf(-400.0 / 2000.0); // -4 dB
    assert!((f.gain() - expected).abs() < 1e-3);
    let held = f.gain();
    f.step_ms(1000);
    assert_eq!(f.gain(), held);
}

#[test]
fn fade_false_jumps_to_ladder_volumes() {
    for (midivol, expect) in [
        (0, 1.0),
        (-400, 10f32.powf(-400.0 / 2000.0)),
        (-800, 10f32.powf(-800.0 / 2000.0)),
        (-1200, 10f32.powf(-1200.0 / 2000.0)),
    ] {
        let mut f = Fade::new();
        f.begin_song(false, midivol);
        assert!((f.gain() - expect).abs() < 1e-4, "midivol {midivol}");
    }
}

#[test]
fn set_target_vol_retargets_mid_ramp() {
    let mut f = Fade::new();
    f.begin_song(true, 0);
    f.set_target_vol(-400);
    for _ in 0..300 {
        f.step_ms(50);
    }
    let expected = 10f32.powf(-400.0 / 2000.0);
    assert!((f.gain() - expected).abs() < 1e-3);
}

#[test]
fn step_ms_accumulates_to_ticks() {
    let mut f = Fade::new();
    f.begin_song(true, 0);
    f.step_ms(25);
    let g1 = f.gain();
    f.step_ms(25);
    let g2 = f.gain();
    assert_ne!(g1, g2); // two half ticks = one 0.25 dB step
}

#[test]
fn stop_hard_then_begin_song_ramps_back_in() {
    let mut f = Fade::new();
    f.begin_song(true, 0);
    f.step_ms(50);
    f.stop_hard();
    assert_eq!(f.gain(), 0.0);
    f.begin_song(true, 0);
    f.step_ms(50); // out phase snaps to the floor
    let g1 = f.gain();
    f.step_ms(50);
    let g2 = f.gain();
    assert!(g2 > g1);
}
