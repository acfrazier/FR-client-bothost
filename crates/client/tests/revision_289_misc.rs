//! Independent public-source G fixtures: RuneWiki/openrs2-nonfree
//! 0c00ef249546fada67b1f6eb8bbe01ea7c250c95, client.java (J).
use client::client::{Client, ClientConfig, ClientGens, ClientRevision};
use client::io::Packet;

fn client() -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    c.ingame = true;
    c
}

fn dispatch(c: &mut Client, id: i32, bytes: &[u8]) -> Packet {
    let before = gens(c.gens);
    c.psize = bytes.len() as i32;
    let mut p = Packet::new(bytes.to_vec());
    p.set_frame_end(bytes.len());
    c.handle_packet(id, &mut p);
    if c.ingame && c.revision().is_289() {
        assert_eq!(p.pos, bytes.len(), "exact consumption for {id}");
        let mut expected = before;
        match id {
            73 | 82 | 133 | 208 => expected[8] += 1,
            164 => expected[9] += 1,
            247 => expected[10] += 1,
            _ => {}
        }
        assert_eq!(gens(c.gens), expected, "publication for {id}");
    }
    p
}

#[test]
fn hud_flags_timer_and_generations() {
    // J:2734-2738,2912-2916,3154-3158,3466-3470.
    let mut c = client();
    let mut expected = gens(c.gens);
    c.minimap_flag_x = 12;
    c.minimap_flag_z = 34;
    dispatch(&mut c, 164, &[]);
    assert!(c.ingame);
    assert_eq!((c.minimap_flag_x, c.minimap_flag_z), (0, 34));
    expected[9] += 1;
    assert_eq!(gens(c.gens), expected);
    for value in [0, 1, 2, 255] {
        dispatch(&mut c, 136, &[value]);
        assert_eq!(c.minimap_state, value as i32);
        assert_eq!(gens(c.gens), expected);
        dispatch(&mut c, 247, &[value]);
        assert_eq!(c.in_multizone, value as i32);
        expected[10] += 1;
        assert_eq!(gens(c.gens), expected);
    }
    dispatch(&mut c, 204, &[0, 60]);
    assert_eq!(c.reboot_timer, 1800);
    assert_eq!(gens(c.gens), expected);
}

#[test]
fn every_fixed_frame_rejects_short_long_before_mutation() {
    // Class17 lengths, ordered fields from the G ledger's primary branches.
    let frames: &[(i32, &[u8])] = &[
        (29, &[0, 3, 0, 8]),
        (73, &[4, 5, 0, 20, 10, 100]),
        (82, &[4, 5, 0, 20, 10, 100]),
        (115, &[1, 0, 7, 4, 5, 6]),
        (133, &[]),
        (136, &[2]),
        (164, &[]),
        (177, &[0, 3, 2, 0, 9]),
        (187, &[0, 7]),
        (204, &[0, 60]),
        (208, &[4, 3, 2, 1]),
        (247, &[1]),
    ];
    for &(id, frame) in frames {
        for len in 0..=frame.len() + 1 {
            if len == frame.len() {
                continue;
            }
            let mut c = client();
            c.hint_type = 42;
            c.cam_x = 99;
            c.cam_move_to_lx = 88;
            c.cam_look_at_lx = 77;
            c.minimap_state = 9;
            c.minimap_flag_x = 8;
            c.in_multizone = 7;
            c.reboot_timer = 123;
            c.wave_ids[0] = 555;
            let before = gens(c.gens);
            // Retain valid bytes in backing storage: declared size is authoritative.
            let mut bytes = frame.to_vec();
            bytes.push(99);
            let mut p = Packet::new(bytes);
            c.psize = len as i32;
            p.set_frame_end(len);
            c.handle_packet(id, &mut p);
            assert!(!c.ingame, "id {id} len {len}");
            assert_eq!(c.hint_type, 42);
            assert_eq!((c.cam_x, c.cam_move_to_lx, c.cam_look_at_lx), (99, 88, 77));
            assert_eq!(
                (
                    c.minimap_state,
                    c.minimap_flag_x,
                    c.in_multizone,
                    c.reboot_timer
                ),
                (9, 8, 7, 123)
            );
            assert_eq!((c.wave_count, c.wave_ids[0]), (0, 555));
            assert_eq!(gens(c.gens), before.map(|g| g + 1));
        }
    }
}

#[test]
fn semantic_indices_fail_before_effects() {
    for (id, bytes) in [
        (208, vec![5, 1, 2, 3]),
        (208, vec![255, 1, 2, 3]),
        (115, vec![1, 255, 255, 1, 2, 3]),
        (115, vec![10, 8, 0, 1, 2, 3]),
        (177, vec![255, 255, 2, 0, 9]),
    ] {
        let mut c = client();
        c.hint_type = 42;
        c.wave_ids[0] = 555;
        let before = gens(c.gens);
        dispatch(&mut c, id, &bytes);
        assert!(!c.ingame, "id {id}");
        assert_eq!(c.hint_type, 42);
        assert_eq!((c.wave_count, c.wave_ids[0]), (0, 555));
        assert!(c.cam_shake.iter().all(|&v| !v));
        assert_eq!(gens(c.gens), before.map(|g| g + 1));
    }
    for case in 0..4 {
        let mut c = client();
        match case {
            0 => c.wave_count = -1,
            1 => c.wave_loops.clear(),
            2 => std::sync::Arc::make_mut(&mut c.jagfx.delays).clear(),
            _ => std::sync::Arc::make_mut(&mut c.jagfx.delays)[3] = i32::MAX,
        }
        let count = c.wave_count;
        let ids = c.wave_ids.clone();
        dispatch(&mut c, 177, &[0, 3, 2, 0, 9]);
        assert!(!c.ingame);
        assert_eq!(c.wave_count, count);
        assert_eq!(c.wave_ids, ids);
    }
}

#[test]
fn minimap_click_and_multiway_paint_follow_dispatch() {
    use client::client::ClientPlayer;
    use client::graphics::{Pix32, Pix8};
    use client::render::Renderer;
    let mut c = client();
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.local_player.as_mut().unwrap().x = 1344;
    c.local_player.as_mut().unwrap().z = 1344;
    c.shell.apply_mouse_down(1, 648, 83);
    c.shell.latch_click();
    for state in [1, 2, 255] {
        dispatch(&mut c, 136, &[state]);
        c.minimap_loop();
        assert_eq!(c.out.pos, 0);
    }
    dispatch(&mut c, 136, &[0]);
    c.minimap_loop();
    assert_eq!(c.out.pos, 21);

    let mut r = Renderer::new(false);
    c.set_draw(true);
    c.scene_state = 2;
    r.game_draw(&mut c);
    let mut media = client::render::Media::empty();
    media.mapback = Some(Pix8::new(172, 156, vec![0]));
    let mut icon = Pix32::new(1, 1);
    icon.data[0] = 0x123456;
    media.headicons[1] = Some(icon);
    r.media = std::sync::Arc::new(media);
    // J:1799-1800 + sprite method388: headicons1 at x472,y296.
    for state in [1, 0, 2] {
        dispatch(&mut c, 247, &[state]);
        r.game_draw(&mut c);
        let game = r.area_game.as_ref().unwrap();
        assert_eq!(
            game.pixels[(296 * game.width + 472) as usize] == 0x123456,
            state == 1
        );
    }
    dispatch(&mut c, 136, &[0]);
    r.game_draw(&mut c);
    assert_eq!(r.area_map.as_ref().unwrap().pixels[79 * 172 + 98], 0xffffff);
    dispatch(&mut c, 136, &[2]);
    r.game_draw(&mut c);
    assert_eq!(r.area_map.as_ref().unwrap().pixels[79 * 172 + 98], 0);
}

#[test]
fn terrain_height_bridge_and_bad_plane_are_staged() {
    let mut c = client();
    c.groundh[1][4][5] = 100;
    c.groundh[1][5][5] = 200;
    c.groundh[1][4][6] = 300;
    c.groundh[1][5][6] = 400;
    c.mapl[1][4][5] = 2; // LINK_BELOW, J:method133
    dispatch(&mut c, 73, &[4, 5, 0, 20, 1, 100]);
    assert_eq!(c.cam_y, 230); // tile-centre bilinear 250 minus20
    c.minusedlevel = 4;
    let before = gens(c.gens);
    dispatch(&mut c, 73, &[6, 7, 0, 40, 1, 100]);
    assert!(!c.ingame);
    assert_eq!((c.cam_move_to_lx, c.cam_move_to_lz, c.cam_y), (4, 5, 230));
    assert_eq!(gens(c.gens), before.map(|g| g + 1));
}

#[test]
fn music_requests_and_jingle_countdown_use_existing_owner() {
    use client::io::OnDemand;
    let mut c = client();
    c.midi_active = true;
    c.on_demand = Some(OnDemand::new_unconnected());
    dispatch(&mut c, 187, &[0, 7]);
    assert_eq!(c.on_demand.as_ref().unwrap().remaining(), 1);
    dispatch(&mut c, 29, &[0, 8, 0, 30]);
    assert_eq!(c.on_demand.as_ref().unwrap().remaining(), 2);
    dispatch(&mut c, 187, &[0, 9]);
    assert_eq!(c.next_midi_song, 9);
    assert_eq!(c.on_demand.as_ref().unwrap().remaining(), 2);
    c.sounds_do_queue();
    assert_eq!(c.next_music_delay, 10);
    c.sounds_do_queue();
    assert_eq!((c.next_music_delay, c.midi_song), (0, 9));
    assert!(c.midi_fading);
    assert_eq!(c.on_demand.as_ref().unwrap().remaining(), 3);
    // Truncated delay cannot issue even a request before logout resets music.
    dispatch(&mut c, 29, &[0, 10, 1]);
    assert!(!c.ingame);
    assert_eq!(c.on_demand.as_ref().unwrap().remaining(), 3);
}

#[test]
fn r274_misc_dispatch_keeps_legacy_ids_and_padding_behavior() {
    use client::io::ServerProt;
    let mut c = Client::new(client().config);
    c.ingame = true;
    assert_eq!(c.revision(), ClientRevision::R274);
    dispatch(&mut c, ServerProt::CAM_MOVETO, &[4, 5, 0, 20, 1, 100]);
    assert_eq!((c.cam_x, c.cam_y, c.cam_z), (576, -20, 704));
    let p = dispatch(&mut c, ServerProt::HINT_ARROW, &[10, 0, 7, 1, 2, 3]);
    assert_eq!((c.hint_type, c.hint_player, p.pos), (10, 7, 3));
    dispatch(&mut c, ServerProt::SET_MULTIWAY, &[1]);
    assert_eq!(c.in_multizone, 1);
    dispatch(&mut c, ServerProt::MIDI_SONG, &[255, 255]);
    assert_eq!(c.next_midi_song, -1);
    dispatch(&mut c, ServerProt::UPDATE_REBOOT_TIMER, &[0, 60]);
    assert_eq!(c.reboot_timer, 1800);
    assert!(c.ingame);
}

fn gens(g: ClientGens) -> [u64; 11] {
    [
        g.npc, g.player, g.inv, g.varp, g.stat, g.chat, g.scene, g.iface, g.camera, g.map_flag,
        g.world,
    ]
}

#[test]
fn hints_consume_padding_and_preserve_unsigned_types() {
    // J:2684-2720; six-byte framing Class17:11, padding is not a field.
    let mut c = client();
    for kind in 0..=255u8 {
        let before = c.gens;
        let p = dispatch(&mut c, 115, &[kind, 0, 7, 0x12, 0x34, 0x56]);
        assert!(c.ingame, "type {kind}");
        assert_eq!(p.pos, 6);
        assert_eq!(
            c.hint_type,
            if (2..=6).contains(&kind) {
                2
            } else {
                kind as i32
            }
        );
        match kind {
            1 => assert_eq!(c.hint_npc, 7),
            10 => assert_eq!(c.hint_player, 7),
            2..=6 => {
                assert_eq!(
                    (c.hint_tile_x, c.hint_tile_z, c.hint_height),
                    (7, 0x1234, 0x56)
                );
                let offsets = [(64, 64), (0, 64), (128, 64), (64, 0), (64, 128)];
                assert_eq!(
                    (c.hint_offset_x, c.hint_offset_z),
                    offsets[(kind - 2) as usize]
                );
            }
            _ => {}
        }
        assert_eq!(gens(c.gens), gens(before));
    }
    // Both disabling encodings clear an already-active actor hint.
    for kind in [0, 255] {
        dispatch(&mut c, 115, &[10, 0, 7, 1, 2, 3]);
        dispatch(&mut c, 115, &[kind, 1, 2, 3, 4, 5]);
        assert_eq!(c.hint_type, kind as i32);
    }
    for kind in [0, 1, 2, 3, 4, 5, 6, 10, 255] {
        for len in [0, 1, 2, 3, 4, 5, 7] {
            let mut c = client();
            c.hint_type = 42;
            let frame = [kind, 0, 7, 1, 2, 3, 4];
            dispatch(&mut c, 115, &frame[..len]);
            assert!(!c.ingame);
            assert_eq!(c.hint_type, 42);
        }
    }
}

#[test]
fn camera_targets_snap_ease_shake_and_publish_once() {
    // J:2804-2818, 3417-3443, 2959-2971, 3518-3525.
    let mut c = client();
    let before = c.gens;
    dispatch(&mut c, 73, &[4, 5, 0, 20, 10, 99]);
    assert!(c.ingame && c.cinema_cam);
    assert_eq!((c.cam_x, c.cam_y, c.cam_z), (0, 0, 0));
    c.cinema_camera();
    assert!(c.cam_x > 0 && c.cam_x < 576);
    dispatch(&mut c, 73, &[4, 5, 0, 20, 10, 100]);
    assert_eq!((c.cam_x, c.cam_y, c.cam_z), (576, -20, 704));
    c.cam_pitch = 250;
    dispatch(&mut c, 82, &[4, 6, 0, 20, 10, 99]);
    assert_eq!(c.cam_pitch, 250);
    c.cinema_camera();
    assert_eq!(c.cam_pitch, 228); // 250 - (10 + (250 - 128) * 99 / 1000)
    dispatch(&mut c, 82, &[4, 6, 0, 20, 10, 100]);
    assert_eq!((c.cam_pitch, c.cam_yaw), (128, 0));
    dispatch(&mut c, 82, &[4, 5, 0xff, 0xff, 10, 100]);
    assert_eq!(c.cam_pitch, 383);
    for axis in 0..5 {
        c.cam_shake_cycle[axis] = 17;
        dispatch(&mut c, 208, &[axis as u8, 3, 4, 5]);
        assert!(c.cam_shake[axis]);
        assert_eq!(
            (
                c.cam_shake_axis[axis],
                c.cam_shake_ran[axis],
                c.cam_shake_amp[axis],
                c.cam_shake_cycle[axis]
            ),
            (3, 4, 5, 0)
        );
    }
    dispatch(&mut c, 133, &[]);
    assert!(!c.cinema_cam && c.cam_shake.iter().all(|&s| !s));
    let mut expected = gens(before);
    expected[8] += 11;
    assert_eq!(gens(c.gens), expected);
}

#[test]
fn music_and_synth_preserve_primary_gates() {
    // J:3302-3340. Global sound, not the area91 operation.
    for gate in 0..5 {
        let mut c = client();
        c.midi_active = gate != 1;
        c.config.lowmem = gate == 2;
        c.next_music_delay = if gate == 3 { 40 } else { 0 };
        c.next_midi_song = if gate == 4 { 7 } else { 3 };
        c.midi_song = 2;
        c.midi_fading = false;
        let before = gens(c.gens);
        dispatch(&mut c, 187, &[0, 7]);
        assert!(c.ingame);
        assert_eq!(c.next_midi_song, 7);
        assert_eq!(c.midi_song, if gate == 0 { 7 } else { 2 });
        assert_eq!(c.midi_fading, gate == 0);
        dispatch(&mut c, 187, &[255, 255]);
        assert_eq!(c.next_midi_song, -1);
        assert_eq!(c.midi_song, if gate == 0 || gate == 4 { -1 } else { 2 });
        dispatch(&mut c, 29, &[255, 255, 0x12, 0x34]);
        if gate != 1 && gate != 2 {
            assert_eq!((c.midi_song, c.next_music_delay), (65535, 0x1234));
            assert!(!c.midi_fading);
        } else {
            assert_eq!((c.midi_song, c.next_music_delay), (2, 0));
        }
        assert_eq!(c.next_midi_song, -1);
        assert_eq!(gens(c.gens), before);
    }
    for gate in 0..4 {
        let mut c = client();
        c.wave_enabled = gate != 1;
        c.config.lowmem = gate == 2;
        c.wave_count = if gate == 3 { 50 } else { 49 };
        std::sync::Arc::make_mut(&mut c.jagfx.delays)[3] = 7;
        let before = gens(c.gens);
        dispatch(&mut c, 177, &[0, 3, 2, 0, 9]);
        assert!(c.ingame);
        assert_eq!(c.wave_count, if gate == 0 || gate == 3 { 50 } else { 49 });
        if gate == 0 {
            assert_eq!(
                (c.wave_ids[49], c.wave_loops[49], c.wave_delay[49]),
                (3, 2, 16)
            );
        }
        assert_eq!(gens(c.gens), before);
    }
}
