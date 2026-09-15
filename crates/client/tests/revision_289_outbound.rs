//! H fixtures independently chosen from public primary 289 client.java.
use client::client::{Client, ClientConfig, ClientRevision};
use std::sync::atomic::{AtomicU64, Ordering};

include!("fixtures/revision_289/outbound_lengths.rs");

fn client(revision: ClientRevision) -> Client {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: std::env::temp_dir()
                .join(format!(
                    "289-h-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ))
                .to_string_lossy()
                .into_owned(),
            members: true,
            lowmem: false,
        },
        revision,
    );
    c.ingame = true;
    c
}

#[test]
fn all_action_families_ordered_bytes_and_declared_lengths() {
    use client::client::{ClientNpc, ClientPlayer, MiniMenuAction as A};
    use client::io::ClientProt289 as P;
    // (UI action, declared protocol entry, independent opcode, target kind, extras).
    let rows = [
        (A::OP_NPC1, P::OPNPC1, 252, 0, 0),
        (A::OP_NPC2, P::OPNPC2, 21, 0, 0),
        (A::OP_NPC3, P::OPNPC3, 178, 0, 0),
        (A::OP_NPC4, P::OPNPC4, 30, 0, 0),
        (A::OP_NPC5, P::OPNPC5, 247, 0, 0),
        (A::TGT_NPC, P::OPNPCT, 108, 0, 1),
        (A::USEHELD_ONNPC, P::OPNPCU, 160, 0, 2),
        (A::OP_PLAYER1, P::OPPLAYER1, 220, 0, 0),
        (A::OP_PLAYER2, P::OPPLAYER2, 51, 0, 0),
        (A::OP_PLAYER3, P::OPPLAYER3, 13, 0, 0),
        (A::OP_PLAYER4, P::OPPLAYER4, 189, 0, 0),
        (A::OP_PLAYER5, P::OPPLAYER5, 69, 0, 0),
        (A::TGT_PLAYER, P::OPPLAYERT, 138, 0, 1),
        (A::USEHELD_ONPLAYER, P::OPPLAYERU, 16, 0, 2),
        (A::OP_LOC1, P::OPLOC1, 10, 1, 0),
        (A::OP_LOC2, P::OPLOC2, 45, 1, 0),
        (A::OP_LOC3, P::OPLOC3, 196, 1, 0),
        (A::OP_LOC4, P::OPLOC4, 53, 1, 0),
        (A::OP_LOC5, P::OPLOC5, 126, 1, 0),
        (A::TGT_LOC, P::OPLOCT, 218, 1, 1),
        (A::USEHELD_ONLOC, P::OPLOCU, 184, 1, 2),
        (A::OP_OBJ1, P::OPOBJ1, 97, 2, 0),
        (A::OP_OBJ2, P::OPOBJ2, 4, 2, 0),
        (A::OP_OBJ3, P::OPOBJ3, 110, 2, 0),
        (A::OP_OBJ4, P::OPOBJ4, 147, 2, 0),
        (A::OP_OBJ5, P::OPOBJ5, 22, 2, 0),
        (A::TGT_OBJ, P::OPOBJT, 241, 2, 1),
        (A::USEHELD_ONOBJ, P::OPOBJU, 55, 2, 2),
        (A::OP_HELD1, P::OPHELD1, 76, 3, 0),
        (A::OP_HELD2, P::OPHELD2, 177, 3, 0),
        (A::OP_HELD3, P::OPHELD3, 40, 3, 0),
        (A::OP_HELD4, P::OPHELD4, 191, 3, 0),
        (A::OP_HELD5, P::OPHELD5, 79, 3, 0),
        (A::TGT_HELD, P::OPHELDT, 112, 3, 1),
        (A::USEHELD_ONHELD, P::OPHELDU, 200, 3, 2),
        (A::INV_BUTTON1, P::INV_BUTTON1, 44, 3, 0),
        (A::INV_BUTTON2, P::INV_BUTTON2, 111, 3, 0),
        (A::INV_BUTTON3, P::INV_BUTTON3, 124, 3, 0),
        (A::INV_BUTTON4, P::INV_BUTTON4, 248, 3, 0),
        (A::INV_BUTTON5, P::INV_BUTTON5, 227, 3, 0),
    ];
    assert_eq!(rows.len(), 40);
    for (action, prot, opcode, kind, extra) in rows {
        let mut c = client(ClientRevision::R289);
        c.local_player = Some(ClientPlayer::at(5, 5));
        c.npc[7] = Some(Box::new(ClientNpc::at(5, 5)));
        c.players[7] = Some(Box::new(ClientPlayer::at(5, 5)));
        c.target_com_id = 0x1234;
        c.obj_com_id = 0x2345;
        c.obj_selected_slot = 0x3456;
        c.obj_selected_com_id = 0x4567;
        c.menu_action[0] = action;
        c.menu_param_a[0] = 7;
        c.menu_param_b[0] = 10;
        c.menu_param_c[0] = 12;
        if kind == 1 {
            let typecode = (2 << 29) | (7 << 14) | (12 << 7) | 10;
            c.world
                .add_scenery(0, 10, 12, 0, typecode, 0, 1, 1, 0, 0, 0, 0, 0);
            c.menu_param_a[0] = typecode;
        }
        c.doAction(0);
        let mut expected = vec![opcode];
        expected.extend_from_slice(match kind {
            0 => &[0, 7][..],
            1 | 2 => &[0, 10, 0, 12, 0, 7][..],
            _ => &[0, 7, 0, 10, 0, 12][..],
        });
        if extra == 1 {
            expected.extend_from_slice(&[0x12, 0x34]);
        }
        if extra == 2 {
            expected.extend_from_slice(&[0x23, 0x45, 0x34, 0x56, 0x45, 0x67]);
        }
        assert_eq!(prot.id, opcode as i32);
        assert_eq!(prot.length as usize, expected.len() - 1);
        assert!(
            c.out.pos >= expected.len(),
            "opcode {opcode}: no production emission"
        );
        assert_eq!(
            &c.out.data()[c.out.pos - expected.len()..c.out.pos],
            expected,
            "opcode {opcode}"
        );
    }
}

#[test]
fn widget_dialog_and_snapshot_ordered_frames() {
    use client::client::MiniMenuAction as A;
    let mut c = client(ClientRevision::R289);
    c.set_iface(1, client::config::if_type::IfType::default());
    for action in [A::IF_BUTTON, A::TOGGLE_BUTTON, A::SELECT_BUTTON] {
        c.out.pos = 0;
        c.menu_action[0] = action;
        c.menu_param_c[0] = 1;
        c.doAction(0);
        assert_eq!(&c.out.data()[..c.out.pos], &[86, 0, 1]);
    }
    c.out.pos = 0;
    c.menu_action[0] = A::PAUSE_BUTTON;
    c.menu_param_c[0] = 0x1234;
    c.doAction(0);
    c.doAction(0);
    assert_eq!(&c.out.data()[..c.out.pos], &[166, 0x12, 0x34]);
    c.out.pos = 0;
    c.menu_action[0] = A::CLOSE_BUTTON;
    c.doAction(0);
    assert_eq!(&c.out.data()[..c.out.pos], &[93]);
    c.out.pos = 0;
    c.dialog_input_open = true;
    c.dialog_input = "16909060".into();
    c.shell.apply_key(true, 0, 13);
    c.handle_chat_input();
    assert_eq!(&c.out.data()[..c.out.pos], &[180, 1, 2, 3, 4]);
    c.out.pos = 0;
    c.report_abuse_input = "a".into();
    c.report_abuse_mute_option = true;
    c.set_iface(
        2,
        client::config::if_type::IfType {
            client_code: 601,
            ..Default::default()
        },
    );
    c.client_button(2);
    assert_eq!(
        &c.out.data()[..c.out.pos],
        &[93, 94, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1]
    );
}

#[test]
fn report_controls_preserve_274_stubs_and_289_ordered_frames() {
    for revision in [ClientRevision::R274, ClientRevision::R289] {
        let mut c = client(revision);
        c.report_abuse_input = "a".into();
        c.set_iface(
            2,
            client::config::IfType {
                client_code: 613,
                ..Default::default()
            },
        );
        assert!(!c.client_button(2));
        assert_eq!(c.report_abuse_mute_option, revision == ClientRevision::R289);
        assert_eq!(c.out.pos, 0, "mute toggle sends no packet");
        for code in 601..=612 {
            c.out.pos = 0;
            c.main_modal_id = 7;
            c.report_abuse_mute_option = true;
            c.set_iface(
                2,
                client::config::IfType {
                    client_code: code,
                    ..Default::default()
                },
            );
            assert!(!c.client_button(2));
            if revision == ClientRevision::R274 {
                assert_eq!(c.out.pos, 0, "published 274 report reasons are idle");
                assert_eq!(c.main_modal_id, 7, "274 stub must not close the modal");
            } else {
                assert_eq!(
                    &c.out.data()[..c.out.pos],
                    &[93, 94, 0, 0, 0, 0, 0, 0, 0, 1, (code - 601) as u8, 1]
                );
                assert_eq!(c.main_modal_id, -1);
            }
        }
    }
}

#[test]
fn inventory_drag_real_release_ordered_bytes() {
    use client::config::if_type::{ComponentType, IfType, IfTypeMut};
    let _renderer = client::render::Renderer::new(false);
    let mut c = client(ClientRevision::R289);
    c.side_modal_id = 1;
    c.set_iface(
        1,
        IfType {
            id: 1,
            r#type: ComponentType::TYPE_LAYER,
            width: 190,
            height: 261,
            children: Some(vec![2]),
            child_x: Some(vec![0]),
            child_y: Some(vec![0]),
            ..Default::default()
        },
    );
    c.set_iface(
        2,
        IfType {
            id: 2,
            layer_id: 1,
            r#type: ComponentType::TYPE_INV,
            obj_swap: true,
            width: 2,
            height: 1,
            ..Default::default()
        },
    );
    c.set_iface_mut(
        2,
        IfTypeMut {
            link_obj_type: Some(vec![1, 0]),
            link_obj_number: Some(vec![1, 0]),
            ..Default::default()
        },
    );
    std::sync::Arc::get_mut(&mut c.cache).unwrap().objs = vec![client::config::ObjType::default()];
    c.shell.apply_mouse_move(569, 221);
    c.build_minimenu();
    c.shell.apply_mouse_down(1, 569, 221);
    c.shell.latch_click();
    c.mouse_loop();
    assert_eq!(c.obj_drag_area, 2);
    c.shell.apply_mouse_move(609, 221);
    for _ in 0..5 {
        c.handle_obj_drag();
    }
    c.shell.apply_mouse_move(601, 221);
    c.shell.apply_mouse_up();
    c.handle_obj_drag();
    assert_eq!(&c.out.data()[..c.out.pos], &[253, 0, 2, 0, 0, 0, 1, 0]);
}

#[test]
fn design_chat_modes_keepalive_and_map_completion() {
    let mut c = client(ClientRevision::R289);
    c.set_iface(
        1,
        client::config::if_type::IfType {
            client_code: 326,
            ..Default::default()
        },
    );
    c.idk_design_gender = false;
    c.idk_design_part = [1, 2, 3, 4, 5, 6, 7];
    c.idk_design_colour = [1, 2, 3, 4, 5];
    c.client_button(1);
    assert_eq!(
        &c.out.data()[..c.out.pos],
        &[27, 1, 1, 2, 3, 4, 5, 6, 7, 1, 2, 3, 4, 5]
    );
    c.out.pos = 0;
    c.chat_public_mode = 0;
    c.chat_private_mode = 2;
    c.chat_trade_mode = 1;
    c.shell.apply_mouse_down(1, 20, 480);
    c.shell.latch_click();
    c.chat_mode_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[161, 1, 2, 1]);
    let mut c = client(ClientRevision::R289);
    c.no_timeout_timer = 49;
    c.game_loop();
    assert_eq!(c.out.pos, 0);
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[181]);
    let mut c = client(ClientRevision::R289);
    c.awaiting_player_info = false;
    c.scene_state = 1;
    c.map_build_index = vec![0];
    c.map_build_ground_file = vec![-1];
    c.map_build_location_file = vec![-1];
    c.map_build_ground_data = vec![None];
    c.map_build_location_data = vec![None];
    c.game_loop();
    assert_eq!(c.scene_state, 2);
    // Map build's four source keepalives precede completion (J:10505-10536).
    assert_eq!(&c.out.data()[..c.out.pos], &[181, 181, 181, 181, 214]);
}

#[test]
fn draw_counters_and_tutorial_ordered_payloads() {
    let mut c = client(ClientRevision::R289);
    let mut r = client::render::Renderer::new(false);
    c.tut_flash_icon = 3;
    c.active_icon = 3;
    c.redraw_icons = true;
    r.game_draw(&mut c);
    assert_eq!(&c.out.data()[..c.out.pos], &[146, 3]);
    c.out.pos = 0;
    c.scene_state = 2;
    c.cyclelogic6 = 122;
    c.minimap_flag_x = 10;
    c.minimap_flag_z = 10;
    let mut p = client::client::ClientPlayer::at(10, 10);
    p.x = 1344;
    p.z = 1344;
    c.local_player = Some(p);
    r.game_draw(&mut c);
    assert_eq!(&c.out.data()[..c.out.pos], &[255, 62]);
    c.out.pos = 0;
    r.cyclelogic3 = 112;
    r.minimap_build_buffer(&mut c, 0);
    assert_eq!(&c.out.data()[..c.out.pos], &[125, 50]);
    c.out.pos = 0;
    r.cyclelogic1 = 1174;
    r.game_draw(&mut c);
    let bytes = &c.out.data()[..c.out.pos];
    assert_eq!(bytes[0], 130);
    assert_eq!(bytes[1] as usize, bytes.len() - 2);
    // Independent primary ordered grammar: optional branches and one random byte.
    // Enumerate all16 legal optional combinations, not a distribution claim.
    let payload = &bytes[2..];
    let mut accepted = false;
    for mask in 0..16 {
        let mut expected = Vec::new();
        if mask & 1 != 0 {
            expected.extend_from_slice(&11499u16.to_be_bytes());
        }
        expected.extend_from_slice(&10548u16.to_be_bytes());
        if mask & 2 != 0 {
            expected.push(139);
        }
        if mask & 4 != 0 {
            expected.push(94);
        }
        expected.extend_from_slice(&51693u16.to_be_bytes());
        expected.push(16);
        expected.extend_from_slice(&15036u16.to_be_bytes());
        if mask & 8 != 0 {
            expected.push(65);
        }
        if payload.len() == expected.len() + 3
            && payload.starts_with(&expected)
            && payload.ends_with(&22990u16.to_be_bytes())
        {
            accepted = true;
        }
    }
    assert!(accepted, "source grammar: {payload:?}");
    assert_eq!(r.cyclelogic1, 0);
}

#[test]
fn deterministic_cycle2_is_a_legal_choice_sequence() {
    let mut c = client(ClientRevision::R289);
    c.local_player = Some(client::client::ClientPlayer::at(5, 5));
    let typecode = (2 << 29) | (1 << 14) | (10 << 7) | 10;
    c.world
        .add_scenery(0, 10, 10, 0, typecode, 0, 1, 1, 0, 0, 0, 0, 0);
    c.menu_action[0] = client::client::MiniMenuAction::OP_LOC1;
    c.menu_param_a[0] = typecode;
    c.menu_param_b[0] = 10;
    c.menu_param_c[0] = 10;
    c.cyclelogic2 = 1085;
    c.doAction(0);
    assert_eq!(c.cyclelogic2, 1086);
    c.out.pos = 0;
    c.doAction(0);
    let mut expected = vec![154, 18];
    expected.extend_from_slice(&16791u16.to_be_bytes());
    expected.push(254);
    for word in [0u16, 16128, 52610, 0, 55420, 35025, 46628] {
        expected.extend_from_slice(&word.to_be_bytes());
    }
    expected.push(0);
    assert_eq!(&c.out.data()[..expected.len()], expected);
    assert_eq!(c.cyclelogic2, 0);
}

#[test]
fn operation_counter_boundaries_preserve_primary_no_reset() {
    use client::client::{ClientPlayer, MiniMenuAction as A};
    // J exact threshold/payload rows: unlike cycle counters these do not reset.
    let cases: &[(i32, i32, i32, &[u8])] = &[
        (1, A::OP_LOC2, 139, &[195, 0, 0, 0, 0]),
        (2, A::OP_LOC3, 124, &[81, 0x94, 0x42]),
        (3, A::OP_OBJ5, 118, &[122, 0, 0, 0, 0]),
        (4, A::OP_PLAYER1, 52, &[49, 131]),
        (5, A::OP_PLAYER4, 66, &[46, 154]),
        (6, A::INV_BUTTON1, 133, &[73, 0x17, 0xe6]),
        (7, A::OP_OBJ1, 123, &[133, 0, 0, 0, 0]),
        (8, A::OP_OBJ4, 75, &[168, 19]),
        (9, A::OP_HELD4, 116, &[88, 0xc6, 0xa4, 0x39]),
    ];
    for &(which, action, threshold, golden) in cases {
        let mut c = client(ClientRevision::R289);
        c.local_player = Some(ClientPlayer::at(5, 5));
        c.players[1] = Some(Box::new(ClientPlayer::at(5, 5)));
        let typecode = (2 << 29) | (1 << 14) | (1 << 7) | 1;
        c.world
            .add_scenery(0, 1, 1, 0, typecode, 0, 1, 1, 0, 0, 0, 0, 0);
        c.menu_action[0] = action;
        c.menu_param_a[0] = if which <= 2 {
            typecode
        } else if which == 6 {
            4
        } else {
            1
        };
        c.menu_param_b[0] = 1;
        c.menu_param_c[0] = 1;
        if which == 7 {
            c.menu_param_b[0] = 4;
        }
        c.map_build_base_z = 1;
        match which {
            1 => c.oplogic1 = threshold - 2,
            2 => c.oplogic2 = threshold - 2,
            3 => c.oplogic3 = threshold - 2,
            4 => c.oplogic4 = threshold - 2,
            5 => c.oplogic5 = threshold - 2,
            6 => c.oplogic6 = threshold - 2,
            7 => c.oplogic7 = threshold - 2,
            8 => c.oplogic8 = threshold - 2,
            _ => c.oplogic9 = threshold - 2,
        }
        // Match only the prefix immediately before the known final action frame.
        for iteration in 0..3 {
            c.out.pos = 0;
            c.doAction(0);
            let action_len = if which == 4 || which == 5 { 3 } else { 7 };
            let mut prefix = &c.out.data()[..c.out.pos - action_len];
            let mut found = false;
            while !prefix.is_empty() {
                if prefix[0] == 67 {
                    let size = prefix[1] as usize;
                    prefix = &prefix[2 + size..];
                } else {
                    assert_eq!(&prefix[..golden.len()], golden, "counter {which}");
                    prefix = &prefix[golden.len()..];
                    assert!(!found);
                    found = true;
                }
            }
            assert_eq!(
                found,
                iteration > 0,
                "counter {which} iteration {iteration}"
            );
        }
    }
}

#[test]
fn social_keyboard_and_chat_ordered_frames() {
    let mut c = client(ClientRevision::R289);
    let mut player = client::client::ClientPlayer::at(5, 5);
    player.name = Some("Fixture".into());
    c.local_player = Some(player);
    for (kind, opcode) in [(1, 235), (2, 203), (4, 192), (5, 251)] {
        c.social_input_type = kind;
        c.social_input_open = true;
        c.social_input.clear();
        c.shell.apply_key(true, 0, 97);
        c.shell.apply_key(true, 0, 13);
        c.out.pos = 0;
        c.handle_chat_input();
        assert_eq!(
            &c.out.data()[..c.out.pos],
            &[opcode, 0, 0, 0, 0, 0, 0, 0, 1]
        );
    }
    c.social_input_type = 3;
    c.social_input_open = true;
    c.social_input.clear();
    c.social_userhash = 1;
    c.shell.apply_key(true, 0, 101);
    c.shell.apply_key(true, 0, 13);
    c.out.pos = 0;
    c.handle_chat_input();
    assert_eq!(
        &c.out.data()[..c.out.pos],
        &[107, 9, 0, 0, 0, 0, 0, 0, 0, 1, 0x10]
    );
    c.out.pos = 0;
    c.chat_input = "red:wave2:e".into();
    c.shell.apply_key(true, 0, 13);
    c.handle_chat_input();
    assert_eq!(&c.out.data()[..c.out.pos], &[156, 3, 1, 2, 0x10]);
    c.out.pos = 0;
    c.chat_input = "::a".into();
    c.shell.apply_key(true, 0, 13);
    c.handle_chat_input();
    assert_eq!(&c.out.data()[..c.out.pos], &[34, 2, 97, 10]);
}

#[test]
fn movement_minimap_tail_and_isaac_concatenation() {
    use client::client::ClientPlayer;
    use client::io::Isaac;
    let mut c = client(ClientRevision::R289);
    let mut player = ClientPlayer::at(10, 10);
    player.x = 1344;
    player.z = 1344;
    c.local_player = Some(player);
    c.shell.apply_mouse_down(1, 648, 83);
    c.shell.latch_click();
    c.minimap_loop();
    assert_eq!(
        &c.out.data()[..c.out.pos],
        &[236, 19, 0, 0, 10, 0, 10, 0, 0, 0, 0, 57, 0, 0, 89, 5, 64, 5, 64, 0, 63]
    );
    c.out.pos = 0;
    c.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    for kind in [0, 2] {
        assert!(c.tryMove(5, 5, 10, 10, true, 0, 0, 0, 0, 0, kind));
    }
    let mut decoder = Isaac::new(&[1, 2, 3, 4]);
    let bytes = &c.out.data()[..c.out.pos];
    assert_eq!(bytes.len(), 14);
    for (frame, opcode) in bytes.as_chunks::<7>().0.iter().zip([234, 67]) {
        assert_eq!(frame[0].wrapping_sub(decoder.next_int() as u8), opcode);
        assert_eq!(&frame[1..], &[5, 0, 0, 10, 0, 10]);
    }
}

#[test]
fn cold_login_resets_focus_click_and_pending_samples() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut header = [0; 2];
        s.read_exact(&mut header).unwrap();
        s.write_all(&[0; 17]).unwrap();
        s.read_exact(&mut header).unwrap();
        let mut login = vec![0; header[1] as usize];
        s.read_exact(&mut login).unwrap();
        s.write_all(&[2, 0, 1]).unwrap();
    });
    let mut c = client(ClientRevision::R289);
    c.config.port = addr.port();
    c.mouse_tracked = true;
    c.shell.apply_focus(false);
    c.game_loop();
    c.shell.apply_mouse_move(50, 50);
    c.shell.sample_mouse();
    c.login("fixture", "fixture", false).unwrap();
    server.join().unwrap();
    assert!(c.shell.focused);
    // Detach the completed synthetic handshake, inspect plaintext UI emission.
    c.stream = None;
    c.out = client::io::Packet::alloc(1);
    c.shell.apply_mouse_down(1, 0, 0);
    c.shell.latch_click();
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[229, 0, 224, 255, 240, 0, 0]);
}

#[test]
fn mouse_three_encodings_and_duplicate_carry() {
    let mut c = client(ClientRevision::R289);
    c.mouse_tracked = true;
    // Eight unchanged samples force the long interval form on the next change.
    for _ in 0..8 {
        c.shell.apply_mouse_move(0, 0);
        c.shell.sample_mouse();
    }
    c.shell.apply_mouse_move(1, 0);
    c.shell.sample_mouse();
    c.shell.apply_mouse_move(764, 502);
    c.shell.sample_mouse();
    c.shell.apply_mouse_move(-1, -1);
    c.shell.sample_mouse();
    c.shell.apply_mouse_down(1, 0, 0);
    c.shell.latch_click();
    c.game_loop();
    assert_eq!(
        &c.out.data()[..12],
        &[229, 10, 0xc0, 0x40, 0, 1, 0x85, 0xdf, 0x1a, 0x87, 0xff, 0xff]
    );
}

#[test]
fn mouse_budget_239_admits_whole_four_byte_sample_then_stops() {
    let mut c = client(ClientRevision::R289);
    c.mouse_tracked = true;
    let mut expected = vec![229, 243];
    for n in 0..79 {
        let (x, y) = if n % 2 == 0 { (100, 100) } else { (700, 400) };
        c.shell.apply_mouse_move(x, y);
        c.shell.sample_mouse();
        let absolute = (8388608 + y * 765 + x) as u32;
        expected.extend_from_slice(&absolute.to_be_bytes()[1..]);
    }
    c.shell.apply_mouse_move(101, 100);
    c.shell.sample_mouse();
    expected.extend_from_slice(&[0x08, 0x60]);
    for _ in 0..8 {
        c.shell.sample_mouse();
    }
    c.shell.apply_mouse_move(500, 500);
    c.shell.sample_mouse();
    expected.extend_from_slice(&(0xc0400000u32 + 500 * 765 + 500).to_be_bytes());
    c.shell.apply_mouse_move(501, 500);
    c.shell.sample_mouse();
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], expected);
    c.out.pos = 0;
    c.game_loop();
    assert_eq!(c.out.pos, 0, "one retained sample waits for admission");
    c.shell.apply_mouse_down(1, 501, 500);
    c.shell.latch_click();
    c.game_loop();
    assert_eq!(&c.out.data()[..4], &[229, 2, 0x08, 0x60]);
    assert_eq!(c.out.data()[4], 224);
}

#[test]
fn mouse_recorder_cap_and_whole_sample_budget_retain_order() {
    let mut c = client(ClientRevision::R289);
    c.mouse_tracked = true;
    for i in 0..501 {
        c.shell.apply_mouse_move(
            if i % 2 == 0 { 764 } else { 0 },
            if i % 2 == 0 { 502 } else { 0 },
        );
        c.shell.sample_mouse();
    }
    let mut payload = Vec::new();
    for _ in 0..6 {
        c.out.pos = 0;
        c.game_loop();
        assert_eq!(&c.out.data()[..2], &[229, 240]);
        payload.extend_from_slice(&c.out.data()[2..242]);
    }
    c.out.pos = 0;
    c.game_loop();
    assert_eq!(c.out.pos, 0); // remaining20 is below admission40
    c.shell.apply_mouse_down(1, 0, 0);
    c.shell.latch_click();
    c.game_loop();
    assert_eq!(&c.out.data()[..2], &[229, 60]);
    payload.extend_from_slice(&c.out.data()[2..62]);
    let expected: Vec<u8> = [0x85, 0xdf, 0x1a, 0x80, 0, 0].repeat(250);
    assert_eq!(payload, expected); // sample501 alone is dropped by recorder-full rule
}

#[test]
fn sampled_mouse_encodes_before_click() {
    let mut c = client(ClientRevision::R289);
    c.mouse_tracked = true;
    c.shell.apply_mouse_move(1, 2);
    c.shell.sample_mouse();
    c.shell.apply_mouse_down(1, 1, 2);
    c.shell.latch_click();
    c.game_loop();
    assert_eq!(
        &c.out.data()[..c.out.pos],
        &[229, 2, 8, 98, 224, 255, 240, 5, 251]
    );
}

#[test]
fn cycle5_counts_only_drawn_mode2_crosshairs() {
    let mut c = client(ClientRevision::R289);
    let mut r = client::render::Renderer::new(false);
    c.scene_state = 2;
    c.cross_mode = 2;
    for _ in 0..57 {
        r.game_draw(&mut c);
    }
    assert_eq!(c.out.pos, 0);
    c.scene_state = 1;
    r.game_draw(&mut c);
    assert_eq!(c.out.pos, 0);
    c.scene_state = 2;
    r.game_draw(&mut c);
    assert_eq!(&c.out.data()[..c.out.pos], &[85]);
}

#[test]
fn legacy_focus_keeps_existing_held_behavior() {
    let mut c = client(ClientRevision::R274);
    c.shell.apply_key(true, 37, 1);
    c.shell.apply_focus(false);
    assert_eq!(c.shell.key_held[1], 1);
    c.game_loop();
    assert_eq!(c.out.pos, 0);
}

#[test]
fn cycle4_input_poll_counts_calls_even_without_keys() {
    let mut c = client(ClientRevision::R289);
    for _ in 0..192 {
        c.handle_chat_input();
    }
    assert_eq!(c.out.pos, 0);
    c.handle_chat_input();
    assert_eq!(&c.out.data()[..c.out.pos], &[137, 232]);
    c.out.pos = 0;
    c.handle_chat_input();
    assert_eq!(c.out.pos, 0);
}

#[test]
fn click_is_packed_before_ui_consumes_it() {
    let mut c = client(ClientRevision::R289);
    c.shell.apply_mouse_down(2, -20, 900);
    c.shell.latch_click();
    // First real event uses epoch millis, so J:5883-5887 saturates4095.
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[224, 0xff, 0xfd, 0xdc, 0x1e]);
    c.out.pos = 0;
    c.shell.latch_click();
    c.game_loop();
    assert_eq!(c.out.pos, 0);
}

#[test]
fn idle_real_loop_threshold_and_repeat_subtract_500() {
    let mut c = client(ClientRevision::R289);
    for tick in 1..=5001 {
        c.no_timeout_timer = 0;
        c.out.pos = 0;
        c.game_loop();
        let bytes = &c.out.data()[..c.out.pos];
        assert_eq!(
            bytes.last() == Some(&145),
            tick == 4501 || tick == 5001,
            "tick {tick}"
        );
        if tick == 4501 || tick == 5001 {
            assert_eq!(c.logout_timer, 250);
        }
    }
}

#[test]
fn focus_edges_clear_held_keys_without_idle_reset() {
    let mut c = client(ClientRevision::R289);
    c.shell.apply_key(true, 37, 1);
    c.shell.apply_focus(false);
    assert_eq!(c.shell.key_held[1], 0);
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[149, 0]);
    c.out.pos = 0;
    c.shell.apply_focus(false);
    c.game_loop();
    assert_eq!(c.out.pos, 0);
    c.shell.apply_focus(true);
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[149, 1]);
}

#[test]
fn camera_arrow_latch_and_twenty_loop_gate() {
    let mut c = client(ClientRevision::R289);
    c.orbit_camera_pitch = 0x123;
    c.orbit_camera_yaw = 0x456;
    c.game_loop();
    assert_eq!(c.out.pos, 0);
    c.shell.apply_key(true, 37, 1);
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[193, 1, 35, 4, 86]);
    c.out.pos = 0;
    c.game_loop(); // held input sets pending during cooldown
    c.shell.apply_key(false, 37, 1);
    for _ in 0..18 {
        c.game_loop();
    }
    assert_eq!(c.out.pos, 0);
    c.game_loop();
    assert_eq!(&c.out.data()[..c.out.pos], &[193, 1, 35, 4, 86]);
}

#[test]
fn cycle7_real_loop_boundary_and_legacy_default() {
    // J:6023-6027: increment before >62; opcode232 has no payload.
    for revision in [ClientRevision::R289, ClientRevision::R274] {
        let mut c = client(revision);
        for _ in 0..62 {
            c.no_timeout_timer = 0;
            c.game_loop();
            assert_eq!(c.out.pos, 0);
        }
        c.no_timeout_timer = 0;
        c.game_loop();
        assert_eq!(
            &c.out.data()[..c.out.pos],
            if revision.is_289() {
                &[232][..]
            } else {
                &[][..]
            }
        );
    }
}

#[test]
fn mouse_delta_interval_edges_and_tracking_disabled() {
    // J:5854: inclusive -32/+31; outside those limits uses absolute p3.
    for (dx, dy, golden) in [
        (-32, -32, vec![0, 0]),
        (31, 31, vec![15, 255]),
        (-33, 0, vec![0x81, 0x2b, 0x17]),
        (32, 0, vec![0x81, 0x2b, 0x58]),
    ] {
        let mut c = client(ClientRevision::R289);
        c.mouse_tracked = true;
        c.shell.apply_mouse_move(100, 100);
        c.shell.sample_mouse();
        c.shell.apply_mouse_down(1, 100, 100);
        c.shell.latch_click();
        c.game_loop();
        c.out.pos = 0;
        c.shell.apply_mouse_move(100 + dx, 100 + dy);
        c.shell.sample_mouse();
        c.game_loop();
        assert_eq!(&c.out.data()[2..2 + golden.len()], golden);
    }
    for duplicates in [7, 8, 2047, 2100] {
        let mut c = client(ClientRevision::R289);
        c.mouse_tracked = true;
        for n in 0..duplicates {
            c.shell.apply_mouse_move(0, 0);
            c.shell.sample_mouse();
            if n % 40 == 39 {
                c.game_loop();
                c.out.pos = 0;
            }
        }
        c.shell.apply_mouse_move(1, 0);
        c.shell.sample_mouse();
        c.shell.apply_mouse_down(1, 0, 0);
        c.shell.latch_click();
        c.game_loop();
        let expected = match duplicates {
            7 => vec![229, 2, 0x78, 0x60],
            8 => vec![229, 4, 0xc0, 0x40, 0, 1],
            _ => vec![229, 4, 0xff, 0xf8, 0, 1],
        };
        assert_eq!(&c.out.data()[..expected.len()], expected);
    }
    let mut c = client(ClientRevision::R289);
    c.shell.apply_mouse_move(500, 500);
    for _ in 0..40 {
        c.shell.sample_mouse();
    }
    c.game_loop();
    assert_eq!(c.out.pos, 0);
    c.mouse_tracked = true;
    c.game_loop();
    assert_eq!(c.out.pos, 0, "disabled tracking cleared samples");
}

#[test]
fn click_elapsed_units_and_button_clamps() {
    let mut c = client(ClientRevision::R289);
    for (time, button, x, y, payload) in [
        (49, 1, -2, -3, [0, 0, 0, 0]),
        (99, 2, 900, 900, [0, 0x1d, 0xdf, 0x1a]),
        (148, 1, 0, 0, [0, 0, 0, 0]),
        (204898, 1, 0, 0, [0xff, 0xf0, 0, 0]),
    ] {
        c.out.pos = 0;
        c.shell.apply_mouse_down(button, x, y);
        c.shell.latch_click();
        // Deterministic timestamp at the public latched-event seam, not live proof.
        c.shell.mouse_click_time = time;
        c.game_loop();
        assert_eq!(&c.out.data()[..c.out.pos], [&[224][..], &payload].concat());
    }
}

#[test]
fn movement_signed_turns_through_ground_pick() {
    for (x, z, golden) in [
        (13, 11, vec![234, 7, 1, 0, 12, 0, 10, 1, 1]),
        (7, 9, vec![234, 7, 1, 0, 8, 0, 10, 255, 255]),
    ] {
        let mut c = client(ClientRevision::R289);
        c.local_player = Some(client::client::ClientPlayer::at(10, 10));
        c.shell.apply_key(true, 17, 5);
        c.world.ground_x = x;
        c.world.ground_z = z;
        c.game_loop();
        assert_eq!(&c.out.data()[..c.out.pos], golden);
        assert_eq!(c.world.ground_x, -1);
    }
}

#[test]
fn real_driver_owns_recorder_and_click_pointer_is_separate() {
    let mut c = client(ClientRevision::R289);
    assert_eq!((c.shell.mouse_x, c.shell.mouse_y), (0, 0));
    c.shell.apply_mouse_move(1, 2);
    c.shell.apply_mouse_down(1, 90, 100);
    assert_eq!((c.shell.mouse_x, c.shell.mouse_y), (1, 2));
    c.shell.latch_click(); // clear the pre-driver click
    c.already_started = true;
    c.mouse_tracked = true;
    let mut renderer = client::render::Renderer::new(false);
    let start = std::time::Instant::now();
    let mut observed = false;
    c.run(&mut renderer, |c| {
        // Admission by a real latched click after the timer has sampled; no
        // direct sample_mouse call in this driver test.
        if start.elapsed() >= std::time::Duration::from_millis(100) {
            if c.out.data()[..c.out.pos]
                .windows(4)
                .any(|b| b == [229, 2, 8, 98])
            {
                observed = true;
                c.shell.state = -1;
            } else {
                c.shell.apply_mouse_down(1, 1, 2);
            }
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(3),
            "driver did not sample"
        );
    });
    assert!(observed);
}

#[test]
fn consume_chat_key_does_not_count_a_revision289_input_poll() {
    let mut c = client(ClientRevision::R289);
    c.ingame = true;
    c.dialog_input_open = true;
    for _ in 0..192 {
        c.handle_chat_input();
    }
    assert_eq!(c.out.pos, 0);
    for _ in 0..50 {
        c.consume_chat_key(b'1' as i32);
    }
    assert_eq!(c.dialog_input, "1111111111");
    assert_eq!(
        c.out.pos, 0,
        "character consumption must not emit ANTICHEAT_CYCLELOGIC4"
    );
    c.handle_chat_input();
    assert_eq!(&c.out.data()[..c.out.pos], &[137, 232]);
}

#[test]
fn handle_chat_input_counts_one_frame_for_many_queued_keys() {
    let mut c = client(ClientRevision::R289);
    c.ingame = true;
    for _ in 0..192 {
        c.handle_chat_input();
    }
    c.shell.apply_key(true, 0, b'a' as i32);
    c.shell.apply_key(true, 0, b'b' as i32);
    c.shell.apply_key(true, 0, b'c' as i32);
    c.handle_chat_input();
    assert_eq!(c.chat_input, "abc");
    assert_eq!(
        &c.out.data()[..c.out.pos],
        &[137, 232],
        "one poll with three keys is still one cyclelogic4 frame"
    );
}
