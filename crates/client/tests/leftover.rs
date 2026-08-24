// CAM_* / HINT_ARROW / UPDATE_RUNWEIGHT / UPDATE_REBOOT_TIMER /
// P_COUNTDIALOG / SET_MULTIWAY / MINIMAP_TOGGLE applies, the cinema camera
// and the shake jitter, plus the dialog-amount keys, the click-crosshair
// tick / walk consume / plot. The /tmp cache has no packs, so `Client::new`
// falls back to `Cache::default()` and never touches the network (the /crc
// fetch on 127.0.0.1 is refused instantly).
use client::client::{Client, ClientConfig, ClientPlayer};
use client::config::idk_type::IdkType;
use client::config::if_type::IfType;
use client::graphics::{Pix3D, Pix32};
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

/// `game_loop` advances `cross_cycle` by 20 per loop and clears
/// `cross_mode` once it passes 400 (TS 2206-2211).
#[test]
fn cross_tick_clears_after_400() {
    let mut c = client();
    c.ingame = true;
    c.cross_mode = 1;
    c.cross_cycle = 380;
    c.game_loop();
    assert_eq!(c.cross_cycle, 400);
    assert_eq!(c.cross_mode, 0);
}

/// A successful walk consume re-arms the crosshair at the clicked point
/// (mode 1, cycle 0) as TS 2317-2322.
#[test]
fn walk_consume_sets_cross_mode_1() {
    let mut c = client();
    c.ingame = true;
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.world.ground_x = 6;
    c.world.ground_z = 5;
    c.shell.mouse_click_x = 100;
    c.shell.mouse_click_y = 80;
    c.game_loop();
    assert_eq!(c.cross_mode, 1);
    assert_eq!(c.cross_cycle, 0);
    assert_eq!(c.cross_x, 100);
    assert_eq!(c.cross_y, 80);
}

/// Java `Client.addPlayers` (7576-7584) / TS 4265-4275: when the local
/// player's tile equals the dest flag, clear it. Without this the flag
/// stays after arrival and, with a stuck `World.click`, hops every frame.
#[test]
fn add_players_clears_minimap_flag_on_arrival() {
    let mut c = client();
    c.ingame = true;
    c.scene_state = 2;
    let mut p = ClientPlayer::at(10, 10);
    p.x = 10 * 128 + 64;
    p.z = 10 * 128 + 64;
    c.local_player = Some(p);
    c.minimap_flag_x = 10;
    c.minimap_flag_z = 10;
    c.game_draw();
    assert_eq!(c.minimap_flag_x, 0);
    assert_eq!(c.minimap_flag_z, 10); // only X is zeroed, matching Java
}

/// Neighbouring tile must not clear the dest flag.
#[test]
fn add_players_keeps_minimap_flag_when_not_arrived() {
    let mut c = client();
    c.ingame = true;
    c.scene_state = 2;
    let mut p = ClientPlayer::at(10, 10);
    p.x = 10 * 128 + 64;
    p.z = 10 * 128 + 64;
    c.local_player = Some(p);
    c.minimap_flag_x = 11;
    c.minimap_flag_z = 10;
    c.game_draw();
    assert_eq!(c.minimap_flag_x, 11);
    assert_eq!(c.minimap_flag_z, 10);
}

/// `prepare_game` depacks `media/cross` into all 8 frames. A missing pack
/// is a skip; an empty frame is the live "X doesn't draw" failure.
#[test]
fn prepare_game_loads_cross_sprites() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.scene_state = 2;
    c.game_draw();
    for i in 0..8 {
        let s = c.cross[i]
            .as_ref()
            .unwrap_or_else(|| panic!("cross[{i}] missing after prepare_game"));
        assert!(s.wi > 0 && s.hi > 0, "cross[{i}] empty size");
        assert!(
            s.data.iter().any(|&p| p != 0),
            "cross[{i}] has no opaque pixels"
        );
    }
}

/// The click crosshair plots into `area_game` at `(cross_x - 12,
/// cross_y - 12)` (TS 4840-4843). Real pack only: a missing `media` pack
/// leaves the sprites `None` and the plot is a no-op.
#[test]
fn cross_plot_mode1_into_area_game() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.scene_state = 2; // game_draw_main (and its overlays) run only in-scene
    c.game_draw(); // prepare_game loads the media sprites
    let mut s = Pix32::new(16, 16);
    s.data[0] = 0x123456; // known top-left pixel: verifies the plot origin
    c.cross[0] = Some(s);
    c.cross_x = 100;
    c.cross_y = 80;
    c.cross_mode = 1;
    c.cross_cycle = 0;
    c.game_draw();
    let game = c.area_game.as_ref().unwrap();
    assert_eq!(game.pixels[68 * 512 + 88], 0x123456);
}

/// Mode 2 ops plot `cross[cycle/100 + 4]`; the sprite frame is the
/// second half of the array (TS 4842).
#[test]
fn cross_plot_mode2_uses_second_half() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.scene_state = 2;
    c.game_draw();
    let mut s = Pix32::new(16, 16);
    s.data[0] = 0x654321;
    c.cross[4] = Some(s); // mode 2, cycle 0 -> index 4
    c.cross_x = 100;
    c.cross_y = 80;
    c.cross_mode = 2;
    c.cross_cycle = 0;
    c.game_draw();
    let game = c.area_game.as_ref().unwrap();
    assert_eq!(game.pixels[68 * 512 + 88], 0x654321);
}

// Task 3: `clientButton` player-design arms (TS 10998-11065) and the
// `clientComponent` design-preview/switch arms (TS 10773-10842).

#[test]
fn cc_logout_still_returns_true() {
    let mut c = client();
    let com = IfType {
        client_code: 205,
        ..IfType::default()
    };
    assert!(c.client_button(&com));
}

#[test]
fn design_switch_male_female_toggles() {
    let mut c = client();
    c.idk_design_gender = true;
    let com = IfType {
        client_code: 325,
        ..IfType::default()
    }; // CC_SWITCH_TO_FEMALE
    assert!(!c.client_button(&com));
    assert!(!c.idk_design_gender);
    // already female: a second switch is a no-op
    assert!(!c.client_button(&com));
    assert!(!c.idk_design_gender);
    // switch back to male
    let male = IfType {
        client_code: 324,
        ..IfType::default()
    };
    assert!(!c.client_button(&male));
    assert!(c.idk_design_gender);
    // already male: no-op
    assert!(!c.client_button(&male));
    assert!(c.idk_design_gender);
}

#[test]
fn design_kit_cycle_wraps_and_respects_gender() {
    let mut c = client();
    // idk table: [0] male head, [1] disabled, [2]+[3] female heads.
    c.cache.idks = vec![
        IdkType {
            part: 0,
            ..IdkType::default()
        },
        IdkType {
            part: 0,
            disable: true,
            ..IdkType::default()
        },
        IdkType {
            part: 7,
            ..IdkType::default()
        },
        IdkType {
            part: 7,
            ..IdkType::default()
        },
    ];
    c.idk_design_gender = true;
    c.idk_design_part[0] = 0;
    let left = IfType {
        client_code: 300,
        ..IfType::default()
    }; // CC_CHANGE_HEAD_L
    assert!(!c.client_button(&left));
    assert!(c.idk_design_redraw);
    // male needs part 0: down from 0 wraps 3 -> 2 (female, skip) ->
    // 1 (disabled) -> 0, the only match.
    assert_eq!(c.idk_design_part[0], 0);

    // switch to female: validate picks the first female head (part 7).
    let female = IfType {
        client_code: 325,
        ..IfType::default()
    };
    assert!(!c.client_button(&female));
    assert!(!c.idk_design_gender);
    assert_eq!(c.idk_design_part[0], 2);

    // down from 2: 1 disabled, 0 male, wraps to 3 (female).
    assert!(!c.client_button(&left));
    assert_eq!(c.idk_design_part[0], 3);

    // switch back to male: validate picks the first male head.
    let male = IfType {
        client_code: 324,
        ..IfType::default()
    };
    assert!(!c.client_button(&male));
    assert!(c.idk_design_gender);
    assert_eq!(c.idk_design_part[0], 0);
}

#[test]
fn design_colour_cycle_wraps_hair_table() {
    let mut c = client();
    c.idk_design_colour[0] = 0;
    let com = IfType {
        client_code: 314,
        ..IfType::default()
    }; // CC_RECOLOUR_HAIR_L
    assert!(!c.client_button(&com));
    // the hair recol table has 12 entries: 0 - 1 wraps to 11.
    assert_eq!(c.idk_design_colour[0], 11);
    assert!(c.idk_design_redraw);
    // right arrow from 11 wraps to 0.
    let right = IfType {
        client_code: 315,
        ..IfType::default()
    };
    assert!(!c.client_button(&right));
    assert_eq!(c.idk_design_colour[0], 0);
}

#[test]
fn design_accept_encodes_idk_savedesign() {
    let mut c = client();
    c.idk_design_gender = false;
    c.idk_design_part = [0, 1, 2, 3, 4, 5, 6];
    c.idk_design_colour = [7, 8, 9, 10, 11];
    let com = IfType {
        client_code: 326,
        ..IfType::default()
    }; // CC_ACCEPT_DESIGN
    assert!(c.client_button(&com));
    assert_eq!(c.out.pos, 14); // p1_enc + 1 gender + 7 parts + 5 colours
    assert_eq!(c.out.data()[0], ClientProt::IDK_SAVEDESIGN.id as u8);
    assert_eq!(c.out.data()[1], 1); // female
    assert_eq!(&c.out.data()[2..9], &[0, 1, 2, 3, 4, 5, 6]);
    assert_eq!(&c.out.data()[9..14], &[7, 8, 9, 10, 11]);
}

#[test]
fn design_preview_sets_rotation_and_caches_temp_model() {
    let mut c = client();
    c.cache.ifaces.resize(328, None);
    c.cache.ifaces[327] = Some(IfType {
        id: 327,
        client_code: 327, // CC_DESIGN_PREVIEW
        ..IfType::default()
    });
    c.loop_cycle = 40;
    c.idk_design_redraw = true; // empty idk table + parts -1 -> empty model
    c.client_component(327);
    let com = c.cache.ifaces[327].as_ref().unwrap();
    assert_eq!(com.model_xan, 150);
    assert_eq!(com.model_yan, 215); // sin(40/40)*256 | 0 = 215, & 0x7ff
    assert_eq!(com.model1_type, 5);
    assert_eq!(com.model1_id, 0);
    assert!(!c.idk_design_redraw);
    // the temp model is reachable through getModel(5, 0).
    assert!(IfType::get_model(&c.cache, None, 5, 0).is_some());
}

#[test]
fn design_switch_buttons_swap_graphic_names() {
    let mut c = client();
    c.cache.ifaces.resize(2, None);
    c.cache.ifaces[1] = Some(IfType {
        id: 1,
        client_code: 324, // CC_SWITCH_TO_MALE
        graphic_name: "male".into(),
        graphic2_name: "female".into(),
        ..IfType::default()
    });
    c.idk_design_gender = true;
    c.client_component(1);
    // male + already male: shows the female snapshot (the button to press).
    assert_eq!(c.cache.ifaces[1].as_ref().unwrap().graphic_name, "female");
    c.idk_design_gender = false;
    c.client_component(1);
    assert_eq!(c.cache.ifaces[1].as_ref().unwrap().graphic_name, "male");
}

/// Java `Client.java` 3682-3686 / Client.ts 1883-1887: the cold-login
/// reset (`reset_idk_design`) flips back to male, revalidates the kits for
/// it and zeroes the colours.
#[test]
fn cold_login_reset_revalidates_design() {
    let mut c = client();
    c.idk_design_gender = false;
    c.idk_design_colour = [3; 5];
    c.cache.idks = vec![
        IdkType {
            part: 0,
            ..IdkType::default()
        },
        IdkType {
            part: 7,
            ..IdkType::default()
        },
    ];
    c.reset_idk_design(); // the cold-login reset path
    assert!(c.idk_design_gender);
    assert!(c.idk_design_redraw);
    assert_eq!(c.idk_design_colour, [0; 5]);
    assert_eq!(c.idk_design_part[0], 0); // first male head
    assert_eq!(c.idk_design_part[1], -1); // no male torso in the table
}

#[test]
fn cold_login_reset_empty_table_leaves_parts_minus_one() {
    // Isolate from an ambient /tmp idk pack: a fresh empty cache dir keeps
    // the "no idks" premise literal.
    let dir = std::env::temp_dir().join(format!("274-noidk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_string_lossy().into_owned(),
        members: true,
        lowmem: false,
    });
    c.idk_design_part = [2; 7];
    c.reset_idk_design(); // empty cache: no idks
    assert!(c.idk_design_gender);
    assert!(c.idk_design_redraw);
    assert_eq!(c.idk_design_part, [-1; 7]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Java `REBUILD_NORMAL` binds `areaGame` without cls, then plots
/// "Loading - please wait." on the frozen last 3D frame. Filling black
/// is the live "viewport goes completely black between chunks" bug.
#[test]
fn scene_loading_splash_keeps_last_frame() {
    let mut c = client();
    c.ingame = true;
    c.draw = true;
    c.scene_state = 1;
    c.game_draw();
    let game = c.area_game.as_mut().expect("prepare_game allocates area_game");
    game.pixels[100] = 0x00ff00;
    c.game_loop();
    assert_eq!(
        c.area_game.as_ref().unwrap().pixels[100],
        0x00ff00,
        "splash must not cls the frozen viewport"
    );
}

/// Options-panel varp clientcodes (TS 10608-10684). Brightness rebuilds
/// the HSL table (OnceLock-first-wins was why the slider did nothing).
#[test]
fn clientcode_1_rebuilds_colour_table() {
    Pix3D::init_colour_table(0.8);
    let mid = Pix3D::colour_table()[200 * 128 + 64];
    Pix3D::init_colour_table(0.6);
    let dark = Pix3D::colour_table()[200 * 128 + 64];
    assert_ne!(mid, dark, "darker brightness must change the HSL table");
}

#[test]
fn clientcode_options_panel_fields() {
    let mut c = client();
    c.apply_clientcode(5, 1);
    assert_eq!(c.one_mouse_button, 1);
    c.apply_clientcode(6, 1);
    assert_eq!(c.chat_effects, 1);
    c.apply_clientcode(9, 1);
    assert_eq!(c.bank_arrange_mode, 1);
    c.apply_clientcode(4, 4);
    assert!(!c.wave_enabled);
    c.apply_clientcode(4, 0);
    assert!(c.wave_enabled);
    assert_eq!(c.wave_volume, 0);
}
