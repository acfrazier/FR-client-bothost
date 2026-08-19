use client::client::key_codes::{self, JavaKeyCode};
use client::client::GameShell;

#[test]
fn left_down_sets_click() {
    let mut sh = GameShell::new();
    sh.apply_mouse_down(1, 100, 40);
    sh.latch_click(); // copy next* → mouse_click* as GameShell.run does before mainloop
    assert_eq!(sh.mouse_click_button, 1);
    assert_eq!(sh.mouse_click_x, 100);
    assert_eq!(sh.mouse_click_y, 40);
    assert_eq!(sh.mouse_button, 1);
}

#[test]
fn right_down_uses_java_button_2() {
    let mut sh = GameShell::new();
    sh.apply_mouse_down(2, 50, 60);
    sh.latch_click();
    assert_eq!(sh.mouse_click_button, 2);
    assert_eq!(sh.mouse_click_x, 50);
    assert_eq!(sh.mouse_click_y, 60);
    assert_eq!(sh.mouse_button, 2);
}

#[test]
fn latch_click_clears_pending_button() {
    let mut sh = GameShell::new();
    sh.apply_mouse_down(1, 10, 20);
    sh.latch_click();
    assert_eq!(sh.mouse_click_button, 1);
    // no new click: the next frame's latch clears the click
    sh.latch_click();
    assert_eq!(sh.mouse_click_button, 0);
}

#[test]
fn mouse_up_releases_button() {
    let mut sh = GameShell::new();
    sh.apply_mouse_down(1, 10, 20);
    sh.apply_mouse_up();
    assert_eq!(sh.mouse_button, 0);
    assert_eq!(sh.mouse_x, 10);
    assert_eq!(sh.mouse_y, 20);
}

#[test]
fn mouse_move_updates_position_only() {
    let mut sh = GameShell::new();
    sh.apply_mouse_move(33, 44);
    assert_eq!(sh.mouse_x, 33);
    assert_eq!(sh.mouse_y, 44);
    assert_eq!(sh.mouse_button, 0);
    // click coordinates come from the down, not a later move
    sh.apply_mouse_down(1, 10, 20);
    sh.apply_mouse_move(33, 44);
    sh.latch_click();
    assert_eq!(sh.mouse_click_x, 10);
    assert_eq!(sh.mouse_click_y, 20);
}

#[test]
fn defaults_match_java_fields() {
    let sh = GameShell::new();
    assert_eq!(sh.mouse_button, 0);
    assert_eq!(sh.mouse_x, -1);
    assert_eq!(sh.mouse_y, -1);
    assert_eq!(sh.mouse_click_button, 0);
    assert_eq!(sh.mouse_click_x, -1);
    assert_eq!(sh.mouse_click_y, -1);
    assert_eq!(sh.key_queue_write, 0);
}

#[test]
fn key_down_sets_held_and_queues() {
    let mut sh = GameShell::new();
    sh.apply_key(true, 37, 1); // ArrowLeft: ch 1, held-only (not queued)
    assert_eq!(sh.key_held[1], 1);
    assert_eq!(sh.key_held[37], 0); // the raw keycode is not an index
    assert_eq!(sh.key_queue_write, 0); // ch <= 4 is not queued
    sh.apply_key(true, 65, 97); // 'a'
    assert_eq!(sh.key_held[97], 1);
    assert_eq!(sh.key_queue_write, 1);
    assert_eq!(sh.key_queue[0], 97);
}

#[test]
fn key_up_clears_held_without_queueing() {
    let mut sh = GameShell::new();
    sh.apply_key(true, 65, 97);
    sh.apply_key(false, 65, 97);
    assert_eq!(sh.key_held[97], 0);
    assert_eq!(sh.key_queue_write, 1);
}

#[test]
fn key_held_guards_ch_range() {
    let mut sh = GameShell::new();
    sh.apply_key(true, 65, 0); // ch 0 is outside 0 < ch < 128
    sh.apply_key(true, 65, 128);
    assert_eq!(sh.key_held[0], 0);
    assert_eq!(sh.key_held[127], 0);
    sh.apply_key(true, 65, 127);
    assert_eq!(sh.key_held[127], 1);
}

#[test]
fn key_queue_wraps_at_128() {
    let mut sh = GameShell::new();
    for i in 0..128 {
        sh.apply_key(true, 65, 100 + i);
    }
    assert_eq!(sh.key_queue_write, 0);
    assert_eq!(sh.key_queue[127], 227);
    assert_eq!(sh.key_queue[0], 100);
}

#[test]
fn key_codes_arrows_enter_backspace_space() {
    assert_eq!(
        key_codes::lookup("ArrowLeft"),
        Some(JavaKeyCode { code: 37, ch: 1 })
    );
    assert_eq!(
        key_codes::lookup("ArrowRight"),
        Some(JavaKeyCode { code: 39, ch: 2 })
    );
    assert_eq!(
        key_codes::lookup("ArrowUp"),
        Some(JavaKeyCode { code: 38, ch: 3 })
    );
    assert_eq!(
        key_codes::lookup("ArrowDown"),
        Some(JavaKeyCode { code: 40, ch: 4 })
    );
    assert_eq!(
        key_codes::lookup("Enter"),
        Some(JavaKeyCode { code: 10, ch: 10 })
    );
    assert_eq!(
        key_codes::lookup("Backspace"),
        Some(JavaKeyCode { code: 8, ch: 8 })
    );
    assert_eq!(
        key_codes::lookup(" "),
        Some(JavaKeyCode { code: 32, ch: 32 })
    );
}

#[test]
fn enter_lookup_from_cr_and_named() {
    assert_eq!(client::client::lookup("Enter").unwrap().ch, 10);
    assert_eq!(client::client::lookup("\r").unwrap().ch, 10);
    assert_eq!(client::client::lookup("\n").unwrap().ch, 10);
}

#[test]
fn key_codes_letters_and_digits() {
    assert_eq!(
        key_codes::lookup("a"),
        Some(JavaKeyCode { code: 65, ch: 97 })
    );
    assert_eq!(
        key_codes::lookup("z"),
        Some(JavaKeyCode { code: 90, ch: 122 })
    );
    assert_eq!(
        key_codes::lookup("A"),
        Some(JavaKeyCode { code: 65, ch: 65 })
    );
    assert_eq!(
        key_codes::lookup("Z"),
        Some(JavaKeyCode { code: 90, ch: 90 })
    );
    assert_eq!(
        key_codes::lookup("0"),
        Some(JavaKeyCode { code: 48, ch: 48 })
    );
    assert_eq!(
        key_codes::lookup("9"),
        Some(JavaKeyCode { code: 57, ch: 57 })
    );
    assert_eq!(key_codes::lookup("Escape"), None);
}
