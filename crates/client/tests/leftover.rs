// CAM_* / HINT_ARROW / UPDATE_RUNWEIGHT / UPDATE_REBOOT_TIMER /
// P_COUNTDIALOG / SET_MULTIWAY / MINIMAP_TOGGLE applies, the cinema camera
// and the shake jitter, plus the dialog-amount keys. The /tmp cache has no
// packs, so `Client::new` falls back to `Cache::default()` and never touches
// the network (the /crc fetch on 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig};
use client::config::if_type::IfType;
use client::io::{ClientProt, Packet, ServerProt};
use client::util::JavaRandom;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn cam_reset_clears_cinema() {
    let mut c = client();
    c.cinema_cam = true;
    c.cam_shake[0] = true;
    let mut p = Packet::new(vec![]);
    c.apply_cam_reset(&mut p);
    assert!(!c.cinema_cam);
    assert!(!c.cam_shake[0]);
}

#[test]
fn p_countdialog_opens_amount() {
    let mut c = client();
    c.apply_p_countdialog();
    assert!(c.dialog_input_open);
    assert!(!c.social_input_open);
    assert!(c.dialog_input.is_empty());
    assert!(c.redraw_chat);
}

#[test]
fn dialog_enter_sends_resume_p_countdialog() {
    let mut c = client();
    c.dialog_input_open = true;
    c.dialog_input = "42".into();
    c.shell.apply_key(true, 0, 13);
    c.handle_chat_input();
    assert_eq!(c.out.data()[0], ClientProt::RESUME_P_COUNTDIALOG.id as u8);
    assert_eq!(&c.out.data()[1..5], &[0, 0, 0, 42]); // p4(value)
    assert!(!c.dialog_input_open);
}

#[test]
fn dialog_keys_append_digits_and_backspace() {
    let mut c = client();
    c.dialog_input_open = true;
    c.shell.apply_key(true, 0, '4' as i32);
    c.shell.apply_key(true, 0, '2' as i32);
    c.shell.apply_key(true, 0, 8);
    c.handle_chat_input();
    assert_eq!(c.dialog_input, "4");
    assert!(c.dialog_input_open);
}

#[test]
fn dialog_digits_capped_at_10() {
    let mut c = client();
    c.dialog_input_open = true;
    c.dialog_input = "1234567890".into();
    c.shell.apply_key(true, 0, '9' as i32);
    c.handle_chat_input();
    assert_eq!(c.dialog_input, "1234567890");
}

#[test]
fn dialog_letters_are_ignored() {
    let mut c = client();
    c.dialog_input_open = true;
    c.shell.apply_key(true, 0, 'x' as i32);
    c.handle_chat_input();
    assert!(c.dialog_input.is_empty());
}

#[test]
fn cam_lookat_rate2_over_100_aims_immediately() {
    let mut c = client();
    // payload: lx, lz, hei(g2), rate, rate2
    let mut p = Packet::new(vec![10, 10, 0, 50, 7, 100]);
    c.apply_cam_lookat(&mut p);
    assert!(c.cinema_cam);
    assert_eq!(c.cam_look_at_lx, 10);
    assert_eq!(c.cam_look_at_lz, 10);
    assert_eq!(c.cam_look_at_hei, 50);
    assert_eq!(c.cam_look_at_rate, 7);
    assert_eq!(c.cam_look_at_rate2, 100);
    // the aim looks from the default origin camera at (10, 10): the ground
    // height is 0, so deltaY is negative and the wrapped pitch
    // (atan2 * 325.949 & 0x7ff) lands > 383 and clamps down to it; yaw is
    // atan2(1344, 1344) * -325.949 & 0x7ff = -255.9997... | 0 → 1793.
    assert_eq!(c.cam_pitch, 383);
    assert_eq!(c.cam_yaw, 1793);
}

#[test]
fn cam_lookat_rate2_under_100_does_not_aim() {
    let mut c = client();
    let mut p = Packet::new(vec![10, 20, 0, 50, 7, 99]);
    c.apply_cam_lookat(&mut p);
    assert!(c.cinema_cam);
    assert_eq!(c.cam_pitch, 0);
    assert_eq!(c.cam_yaw, 0);
}

#[test]
fn cam_shake_marks_axis_and_fields() {
    let mut c = client();
    // axis 2, ran 3, amp 4, rate 5
    let mut p = Packet::new(vec![2, 3, 4, 5]);
    c.apply_cam_shake(&mut p);
    assert!(c.cam_shake[2]);
    assert_eq!(c.cam_shake_axis[2], 3); // TS assigns ran to camShakeAxis
    assert_eq!(c.cam_shake_ran[2], 4); // amp → camShakeRan
    assert_eq!(c.cam_shake_amp[2], 5); // rate → camShakeAmp
    assert_eq!(c.cam_shake_cycle[2], 0);
}

#[test]
fn cam_moveto_rate2_over_100_jumps_camera() {
    let mut c = client();
    let mut p = Packet::new(vec![3, 4, 0, 60, 9, 100]);
    c.apply_cam_moveto(&mut p);
    assert!(c.cinema_cam);
    assert_eq!(c.cam_move_to_lx, 3);
    assert_eq!(c.cam_move_to_lz, 4);
    assert_eq!(c.cam_move_to_hei, 60);
    // immediate jump: cam_x = 3*128+64, cam_z = 4*128+64, cam_y = 0 - hei
    assert_eq!(c.cam_x, 3 * 128 + 64);
    assert_eq!(c.cam_z, 4 * 128 + 64);
    assert_eq!(c.cam_y, -60);
}

#[test]
fn hint_arrow_type_1_reads_npc() {
    let mut c = client();
    let mut p = Packet::new(vec![1, 0, 7]);
    c.apply_hint_arrow(&mut p);
    assert_eq!(c.hint_type, 1);
    assert_eq!(c.hint_npc, 7);
    assert_eq!(p.available(), 0); // frame fully consumed
}

#[test]
fn hint_arrow_type_2_to_6_reads_tile() {
    let mut c = client();
    let mut p = Packet::new(vec![4, 0, 10, 0, 20, 30]);
    c.apply_hint_arrow(&mut p);
    assert_eq!(c.hint_type, 2); // rewritten from 4
    assert_eq!(c.hint_offset_x, 128);
    assert_eq!(c.hint_offset_z, 64);
    assert_eq!(c.hint_tile_x, 10);
    assert_eq!(c.hint_tile_z, 20);
    assert_eq!(c.hint_height, 30);
    assert_eq!(p.available(), 0);
}

#[test]
fn hint_arrow_type_10_reads_player() {
    let mut c = client();
    let mut p = Packet::new(vec![10, 0, 9]);
    c.apply_hint_arrow(&mut p);
    assert_eq!(c.hint_type, 10);
    assert_eq!(c.hint_player, 9);
    assert_eq!(p.available(), 0);
}

#[test]
fn update_runweight_sets_value_and_redraws_stats_tab() {
    let mut c = client();
    c.active_icon = 12;
    let mut p = Packet::new(vec![0xff, 0xfe]); // g2b signed -2
    c.apply_update_runweight(&mut p);
    assert_eq!(c.runweight, -2);
    assert!(c.redraw_side);
}

#[test]
fn update_runweight_other_tab_does_not_redraw() {
    let mut c = client();
    c.active_icon = 3;
    let mut p = Packet::new(vec![0, 5]);
    c.apply_update_runweight(&mut p);
    assert_eq!(c.runweight, 5);
    assert!(!c.redraw_side);
}

#[test]
fn reboot_timer_scales_by_30() {
    let mut c = client();
    let mut p = Packet::new(vec![0, 60]);
    c.apply_update_reboot_timer(&mut p);
    assert_eq!(c.reboot_timer, 1800);
}

#[test]
fn set_multiway_reads_zone() {
    let mut c = client();
    let mut p = Packet::new(vec![1]);
    c.apply_set_multiway(&mut p);
    assert_eq!(c.in_multizone, 1);
}

#[test]
fn minimap_toggle_reads_state() {
    let mut c = client();
    let mut p = Packet::new(vec![2]);
    c.apply_minimap_toggle(&mut p);
    assert_eq!(c.minimap_state, 2);
}

#[test]
fn dispatch_cam_reset_resets_ptype() {
    let mut c = client();
    c.cinema_cam = true;
    c.cam_shake[3] = true;
    let mut p = Packet::new(vec![]);
    c.handle_packet(ServerProt::CAM_RESET, &mut p);
    assert!(!c.cinema_cam);
    assert_eq!(c.ptype, -1);
}

#[test]
fn cinema_camera_eases_toward_move_target() {
    let mut c = client();
    c.cinema_cam = true;
    c.cam_move_to_lx = 4; // x target 4*128+64 = 576
    c.cam_move_to_lz = 5; // z target 5*128+64 = 704
    c.cam_move_to_hei = 0;
    c.cam_move_to_rate = 10;
    c.cam_move_to_rate2 = 0;
    c.cam_look_at_lx = 0;
    c.cam_look_at_lz = 0;
    c.cam_look_at_hei = 0;
    c.cam_look_at_rate = 0;
    c.cam_look_at_rate2 = 0;
    c.cinema_camera();
    // rate 10, rate2 0: cam_x/z step by exactly 10 toward the target
    assert_eq!(c.cam_x, 10);
    assert_eq!(c.cam_z, 10);
    assert_eq!(c.cam_y, 0);
}

#[test]
fn game_loop_ticks_shake_cycles_and_runs_cinema() {
    let mut c = client();
    c.ingame = true;
    c.scene_state = 2;
    c.cinema_cam = true;
    c.cam_shake_cycle[0] = 0;
    c.cam_move_to_lx = 4;
    c.cam_move_to_lz = 5;
    c.cam_move_to_hei = 0;
    c.cam_move_to_rate = 10;
    c.cam_move_to_rate2 = 0;
    c.cam_look_at_lx = 0;
    c.cam_look_at_lz = 0;
    c.cam_look_at_hei = 0;
    c.cam_look_at_rate = 0;
    c.cam_look_at_rate2 = 0;
    c.game_loop();
    assert_eq!(c.cam_shake_cycle[0], 1);
    assert_eq!(c.cam_x, 10); // cinema_camera eased toward the move target
}

#[test]
fn game_loop_without_cinema_skips_cinema_camera() {
    let mut c = client();
    c.ingame = true;
    c.scene_state = 2;
    c.cinema_cam = false;
    c.cam_move_to_lx = 4;
    c.cam_move_to_lz = 5;
    c.cam_move_to_hei = 0;
    c.cam_move_to_rate = 10;
    c.cam_move_to_rate2 = 0;
    c.cam_look_at_lx = 0;
    c.cam_look_at_lz = 0;
    c.cam_look_at_hei = 0;
    c.cam_look_at_rate = 0;
    c.cam_look_at_rate2 = 0;
    c.game_loop();
    assert_eq!(c.cam_x, 0); // no local_player, so no orbit follow either
    assert_eq!(c.cam_shake_cycle[0], 1); // cycles still tick
}

#[test]
fn shake_jitter_follows_seeded_random() {
    let mut c = client();
    c.cam_shake[2] = true;
    c.cam_shake_axis[2] = 1; // ran byte: random range
    c.cam_shake_ran[2] = 0; // amp byte: sin amplitude
    c.cam_shake_amp[2] = 0; // rate byte: sin frequency
    let mut rng = JavaRandom::new(42);
    c.rand = JavaRandom::new(42);
    let expected = (rng.next_double() * 3.0 - 1.0) as i32;
    let (_, _, jz, _, _) = c.cam_shake_jitter(0, 0, 100, 128, 0);
    assert_eq!(jz, 100 + expected);
}

#[test]
fn shake_zero_jitter_leaves_eye_unchanged() {
    let mut c = client();
    c.cam_shake[0] = true;
    c.cam_shake_axis[0] = 0; // random * 1 - 0 truncates to 0 in [0, 1)
    c.cam_shake_ran[0] = 0;
    c.cam_shake_amp[0] = 100;
    c.cam_shake_cycle[0] = 0; // sin(0) = 0
    let (jx, jy, jz, jp, jw) = c.cam_shake_jitter(10, 20, 30, 128, 40);
    assert_eq!((jx, jy, jz, jp, jw), (10, 20, 30, 128, 40));
}

#[test]
fn shake_pitch_clamps_to_128_and_383() {
    let mut c = client();
    c.cam_shake[4] = true;
    c.cam_shake_axis[4] = 0;
    c.cam_shake_ran[4] = 0;
    c.cam_shake_amp[4] = 0;
    let (_, _, _, p1, _) = c.cam_shake_jitter(0, 0, 0, 127, 0);
    assert_eq!(p1, 128);
    let (_, _, _, p2, _) = c.cam_shake_jitter(0, 0, 0, 400, 0);
    assert_eq!(p2, 383);
}

#[test]
fn shake_yaw_wraps_11_bit() {
    let mut c = client();
    c.cam_shake[3] = true;
    c.cam_shake_axis[3] = 200; // random in [-200, 201): yaw stays wrapped
    c.cam_shake_ran[3] = 0;
    c.cam_shake_amp[3] = 0;
    let (_, _, _, _, yaw) = c.cam_shake_jitter(0, 0, 0, 128, 2040);
    assert!((0..=2047).contains(&yaw));
}

#[test]
fn runweight_reaches_stats_tab_script() {
    let mut c = client();
    c.runweight = 42;
    let com = IfType {
        scripts: Some(vec![vec![12, 0]]), // opcode 12 runweight, halt
        ..IfType::default()
    };
    assert_eq!(c.get_if_var(&com, 0), Some(42));
}

#[test]
fn game_loop_ticks_reboot_timer_down() {
    let mut c = client();
    c.ingame = true;
    c.reboot_timer = 100;
    c.game_loop();
    assert_eq!(c.reboot_timer, 99);
}

#[test]
fn game_loop_holds_reboot_timer_at_one() {
    let mut c = client();
    c.ingame = true;
    c.reboot_timer = 1;
    c.game_loop();
    assert_eq!(c.reboot_timer, 1);
}

#[test]
fn pm_options_shift_below_reboot_line() {
    let mut c = client();
    c.split_private_chat = 1;
    c.chat_text[0] = "hello".into();
    c.chat_type[0] = 3;
    c.chat_username[0] = "eve".into();
    c.reboot_timer = 100;
    // the first PM row draws at y 316 when the reboot line reserves 329
    // (TS 2607-2609), so its hover band is mouse_y 311..=323: 320 hits.
    // With the unshifted line = 0 the band was 324..=336 and 320 missed.
    c.shell.mouse_x = 100;
    c.shell.mouse_y = 320;
    c.build_minimenu();
    assert!(c.menu_num_entries > 1, "the PM row must offer menu options");
    assert_eq!(c.menu_option[1], "Add ignore @whi@eve");
}
