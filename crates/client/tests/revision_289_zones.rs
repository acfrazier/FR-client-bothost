//! Minimal independent fixtures from RuneWiki/openrs2-nonfree 289 pin
//! 0c00ef249546fada67b1f6eb8bbe01ea7c250c95, client.java method149
//! (6794-7050), full-follows2755-2773 and rebuild2999-3125. No captures.
use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision};
use client::io::Packet;
use std::time::{SystemTime, UNIX_EPOCH};

fn empty_cache_dir() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "r289-zones-empty-cache-{}-{stamp}",
            std::process::id()
        ))
        .display()
        .to_string()
}

fn zone(c: &mut Client, enclosed: bool, id: u8, bytes: &[u8]) {
    let p = if enclosed {
        let mut frame = vec![c.zone_update_x as u8, c.zone_update_z as u8, id];
        frame.extend(bytes);
        dispatch(c, 112, &frame)
    } else {
        dispatch(c, id as i32, bytes)
    };
    assert!(c.ingame, "operation {id}, enclosed={enclosed}");
    assert_eq!(p.available(), 0);
}

#[test]
fn loc_changes_direct_and_enclosed_and_full_zone_expiry() {
    for enclosed in [false, true] {
        let mut c = client();
        let before = c.gens;
        dispatch(&mut c, 155, &[8, 16]);
        assert_eq!((c.zone_update_x, c.zone_update_z), (8, 16));
        zone(&mut c, enclosed, 90, &[0x12, 0x29, 0, 5]);
        let loc = c.loc_changes.head().unwrap();
        assert_eq!(
            (loc.x, loc.z, loc.new_type, loc.new_shape, loc.new_angle),
            (9, 18, 5, 10, 1)
        );
        assert_eq!((loc.start_time, loc.end_time), (0, -1));
        zone(&mut c, enclosed, 194, &[0x12, 0x29]);
        assert_eq!(c.loc_changes.head().unwrap().new_type, -1);
        c.loc_changes.head().unwrap().end_time = 90;
        c.loc_changes.push(client::dash3d::LocChange {
            x: 9,
            z: 18,
            level: 1,
            end_time: 91,
            ..Default::default()
        });
        c.loc_changes.push(client::dash3d::LocChange {
            x: 16,
            z: 18,
            level: 0,
            end_time: 92,
            ..Default::default()
        });
        c.ground_obj[0][9][18] = Some(Box::new(client::datastruct::LinkList::new()));
        c.ground_obj[1][9][18] = Some(Box::new(client::datastruct::LinkList::new()));
        dispatch(&mut c, 144, &[8, 16]);
        assert!(c.ingame);
        assert!(c.ground_obj[0][9][18].is_none());
        assert!(c.ground_obj[1][9][18].is_some());
        let mut node = c.loc_changes.head();
        while let Some(loc) = node {
            assert_eq!(
                loc.end_time,
                if loc.level == 1 {
                    91
                } else if loc.x == 16 {
                    92
                } else {
                    0
                }
            );
            node = c.loc_changes.next_node();
        }
        assert_eq!(c.gens.scene, before.scene + 4);
        assert_eq!(c.gens.player, before.player);
    }
}

#[test]
fn objects_direct_and_enclosed_receiver_count_mask_and_pile() {
    for enclosed in [false, true] {
        let mut c = client();
        std::sync::Arc::get_mut(&mut c.cache).unwrap().objs =
            vec![client::config::ObjType::default()];
        c.self_slot = 3;
        c.zone_update_x = 8;
        c.zone_update_z = 16;
        let before = c.gens;
        zone(&mut c, enclosed, 60, &[0x12, 0, 0, 0, 9]);
        assert_eq!(
            c.ground_obj[0][9][18]
                .as_mut()
                .unwrap()
                .head()
                .unwrap()
                .count,
            9
        );
        assert!(c.world.ground_object_at(0, 9, 18).is_some());
        zone(&mut c, enclosed, 117, &[0x12, 0x80, 0, 0, 8, 0, 12]);
        assert_eq!(
            c.ground_obj[0][9][18]
                .as_mut()
                .unwrap()
                .head()
                .unwrap()
                .count,
            9
        );
        zone(&mut c, enclosed, 117, &[0x12, 0x80, 0, 0, 9, 0, 12]);
        assert_eq!(
            c.ground_obj[0][9][18]
                .as_mut()
                .unwrap()
                .head()
                .unwrap()
                .count,
            12
        );
        zone(&mut c, enclosed, 71, &[0x12, 0x80, 0]);
        assert!(c.ground_obj[0][9][18].is_none());
        assert!(c.world.ground_object_at(0, 9, 18).is_none());
        zone(&mut c, enclosed, 176, &[0x12, 0, 0, 0, 7, 0, 3]);
        assert!(c.ground_obj[0][9][18].is_none());
        zone(&mut c, enclosed, 176, &[0x12, 0, 0, 0, 7, 0, 4]);
        assert_eq!(
            c.ground_obj[0][9][18]
                .as_mut()
                .unwrap()
                .head()
                .unwrap()
                .count,
            7
        );
        assert_eq!(c.gens.scene, before.scene + 6);
        assert_eq!(c.gens.player, before.player);
        std::sync::Arc::get_mut(&mut c.cache).unwrap().objs[0].stackable = true;
        std::sync::Arc::get_mut(&mut c.cache).unwrap().objs[0].cost = i32::MAX;
        // Java int pile value wraps; a valid large count must not T2 after apply.
        zone(&mut c, enclosed, 117, &[0x12, 0, 0, 0, 7, 0, 1]);
    }
}

fn client() -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: empty_cache_dir(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    let cache = std::sync::Arc::get_mut(&mut c.cache).expect("isolated fixture cache");
    cache.objs.resize_with(1, Default::default);
    cache.locs.resize_with(6, Default::default);
    cache.seqs.resize_with(8, Default::default);
    cache.spots.resize_with(4, Default::default);
    c.ingame = true;
    c.loop_cycle = 50;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c
}

#[test]
fn enclosed_rejects_unknown_short_and_invalid_tail_before_sound_prefix() {
    for tail in [
        vec![255],
        vec![60, 0x11, 0, 0, 0],
        vec![90, 0x11, 255, 0, 0],
        vec![60, 0x11, 255, 255, 0, 1],
        vec![90, 0x11, 0, 255, 255],
        vec![233, 0x11, 255, 255, 0, 0, 1],
        vec![106, 0x11, 0, 255, 255],
    ] {
        let mut c = client();
        let before = c.gens;
        let mut frame = vec![8, 8, 91, 0x22, 0, 3, 0x17];
        frame.extend(tail);
        dispatch(&mut c, 112, &frame);
        assert!(!c.ingame);
        // logout resets queue count but not backing arrays: catches hidden apply.
        assert_eq!(c.wave_ids[0], 0);
        assert_eq!(c.wave_loops[0], 0);
        assert_eq!((c.zone_update_x, c.zone_update_z), (0, 0));
        assert_eq!(c.gens.scene, before.scene + 1, "reset only");
        assert_eq!(c.gens.player, before.player + 1);
    }
}

#[test]
fn rebuild_translates_local_and_pending_state_and_keeps_world_until_load() {
    let mut c = client();
    c.scene_state = 2;
    c.map_build_prev_base_x = 0;
    c.map_build_prev_base_z = 0;
    c.minimap_flag_x = 20;
    c.minimap_flag_z = 30;
    c.world
        .set_decor(0, 10, 10, 0, 0, 0, 0x40000000, 0, 0, 0, 0, 0, 0, 0);
    c.loc_changes.push(client::dash3d::LocChange {
        x: 20,
        z: 30,
        ..Default::default()
    });
    let cache = std::sync::Arc::clone(&c.cache);
    let before = c.gens;
    dispatch(&mut c, 219, &[0, 7, 0, 8]);
    assert!(c.ingame);
    assert_eq!(c.scene_state, 1);
    assert!(c.awaiting_player_info);
    assert_eq!((c.map_build_base_x, c.map_build_base_z), (8, 16));
    assert_eq!(
        (
            c.local_player.as_ref().unwrap().route_x[0],
            c.local_player.as_ref().unwrap().route_z[0]
        ),
        (2, -6)
    );
    assert_eq!(
        (
            c.loc_changes.head().unwrap().x,
            c.loc_changes.head().unwrap().z
        ),
        (12, 14)
    );
    assert_eq!((c.minimap_flag_x, c.minimap_flag_z), (12, 14));
    assert!(c.world.get_decor(0, 10, 10).is_some());
    assert!(std::sync::Arc::ptr_eq(&cache, &c.cache));
    assert_eq!(c.gens.scene, before.scene + 1);
    assert_eq!(c.gens.player, before.player + 1);
    assert_eq!(c.gens.world, before.world + 1);
    let before = c.gens;
    dispatch(&mut c, 219, &[0, 9, 0]);
    assert!(!c.ingame);
    assert_eq!(
        (c.map_build_centre_zone_x, c.map_build_centre_zone_z),
        (7, 8)
    );
    assert_eq!(c.gens.scene, before.scene + 1);
}

#[test]
fn merge_direct_and_enclosed_attaches_local_and_remote_and_schedules() {
    // Independent empty model: 18-byte all-zero footer = zero vertices/faces.
    client::dash3d::Model::unpack(4095, Some(&[0; 18]));
    assert!(client::dash3d::Model::load(4095).is_some());
    for enclosed in [false, true] {
        for local in [false, true] {
            let mut c = client();
            let cache = std::sync::Arc::get_mut(&mut c.cache).unwrap();
            cache.locs.clear();
            cache.locs.push(client::config::LocType {
                id: 0,
                model: Some(vec![4095]),
                width: 2,
                length: 3,
                ..Default::default()
            });
            c.self_slot = 5;
            assert!(
                c.cache
                    .loc(0)
                    .get_model(&c.cache, 10, 1, 0, 0, 0, 0, -1)
                    .is_some(),
                "fixture model"
            );
            c.players[6] = Some(Box::new(ClientPlayer::at(10, 10)));
            c.zone_update_x = 8;
            c.zone_update_z = 8;
            let before = c.gens;
            zone(
                &mut c,
                enclosed,
                83,
                &[
                    0x23,
                    0x29,
                    0,
                    0,
                    0,
                    2,
                    0,
                    8,
                    0,
                    if local { 5 } else { 6 },
                    3,
                    2,
                    255,
                    254,
                ],
            );
            let p = if local {
                c.local_player.as_ref().unwrap()
            } else {
                c.players[6].as_ref().unwrap()
            };
            assert!(p.loc_model.is_some());
            assert_eq!((p.loc_start_cycle, p.loc_stop_cycle), (52, 58));
            assert_eq!((p.loc_offset_x, p.loc_offset_z), (1472, 1536));
            assert_eq!(
                (p.min_tile_x, p.max_tile_x, p.min_tile_z, p.max_tile_z),
                (9, 13, 9, 13)
            );
            let change = c.loc_changes.head().unwrap();
            assert_eq!((change.start_time, change.end_time), (3, 9));
            assert_eq!(c.gens.scene, before.scene + 1);
            assert_eq!(c.gens.player, before.player + 1);
            if enclosed {
                let before = c.gens;
                let frame = [
                    8, 8, 91, 0x22, 0, 3, 0x17, 60, 0x22, 0, 0, 0, 1, 83, 0x23, 0x29, 0, 0, 0, 2,
                    0, 8, 0, 5, 3, 2, 255, 254, 83, 0x23, 0x29, 0, 0, 0, 2, 0, 8, 0, 6, 3, 2, 255,
                    254,
                ];
                dispatch(&mut c, 112, &frame);
                assert!(c.ingame);
                assert_eq!(c.wave_ids[0], 3);
                assert_eq!(c.wave_count, 1);
                assert!(c.ground_obj[0][10][10].is_some());
                assert!(c.local_player.as_ref().unwrap().loc_model.is_some());
                assert!(c.players[6].as_ref().unwrap().loc_model.is_some());
                assert_eq!(c.gens.scene, before.scene + 1);
                assert_eq!(c.gens.player, before.player + 1);
                assert_eq!(c.gens.inv, before.inv);
                assert_eq!(c.gens.world, before.world);
            }
        }
    }
}

#[test]
fn loc_anim_direct_and_enclosed_uses_primary_edge_and_southeast() {
    for enclosed in [false, true] {
        let mut c = client();
        c.minusedlevel = 4;
        let before = c.gens;
        if enclosed {
            dispatch(&mut c, 112, &[8, 8, 91, 0x22, 0, 3, 7]);
        } else {
            dispatch(&mut c, 91, &[0x22, 0, 3, 7]);
        }
        assert!(!c.ingame);
        assert_eq!(c.wave_ids[0], 0);
        assert_eq!(c.gens.scene, before.scene + 1);
        let mut c = client();
        c.zone_update_x = 104;
        c.zone_update_z = 104;
        zone(&mut c, enclosed, 233, &[0, 0, 0, 0, 0, 1]);
        assert!(c.spotanims.head().is_none());
    }
    for enclosed in [false, true] {
        for edge in [102, 103] {
            let mut c = client();
            c.minusedlevel = 3;
            c.zone_update_x = 96;
            c.zone_update_z = 96;
            c.groundh[3][edge][edge] = 10;
            c.groundh[3][edge + 1][edge] = 20;
            c.groundh[3][edge + 1][edge + 1] = 30;
            c.groundh[3][edge][edge + 1] = 40;
            c.world.set_decor(
                3,
                edge as i32,
                edge as i32,
                0,
                0,
                0,
                0x40000000,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            );
            let pos = if edge == 102 { 0x66 } else { 0x77 };
            zone(&mut c, enclosed, 106, &[pos, 0x13, 0, 7]);
            let d = c.world.get_decor(3, edge as i32, edge as i32).unwrap();
            if edge == 102 {
                assert_eq!((d.anim_seq, d.anim_shape, d.anim_angle), (7, 4, 0));
                assert_eq!((d.h_sw, d.h_se, d.h_ne, d.h_nw), (10, 20, 30, 40));
            } else {
                assert_eq!(d.anim_seq, -1);
            }
        }
    }
}

#[test]
fn projectile_and_map_anim_direct_and_enclosed_signed_fields() {
    for enclosed in [false, true] {
        let mut c = client();
        c.zone_update_x = 8;
        c.zone_update_z = 8;
        let before = c.gens;
        // (10,11) -> (9,9), target -6, heights 4/8, delays 2/8.
        zone(
            &mut c,
            enclosed,
            87,
            &[0x23, 255, 254, 255, 250, 0, 0, 1, 2, 0, 2, 0, 8, 0, 0],
        );
        let p = c.projectiles.head().unwrap();
        assert_eq!(
            (p.src_x, p.src_z, p.target, p.h1, p.h2, p.t1, p.t2),
            (1344, 1472, -6, -4, 8, 52, 58)
        );
        assert!(p.velocity_x < 0.0 && p.velocity_z < p.velocity_x);
        zone(&mut c, enclosed, 233, &[0x23, 0, 3, 9, 0, 4]);
        let a = c.spotanims.head().unwrap();
        assert_eq!((a.x, a.z, a.y, a.start_cycle), (1344, 1472, -9, 54));
        assert_eq!(c.gens.scene, before.scene + 2);
        assert_eq!(c.gens.player, before.player);
    }
}
fn dispatch(c: &mut Client, id: i32, bytes: &[u8]) -> Packet {
    c.psize = bytes.len() as i32;
    let mut p = Packet::new(bytes.to_vec());
    p.set_frame_end(bytes.len());
    c.handle_packet(id, &mut p);
    p
}

#[test]
fn area_sound_direct_and_enclosed_consumes_and_obeys_primary_gates() {
    for enclosed in [false, true] {
        for gate in 0..6 {
            let mut c = client();
            c.zone_update_x = 8;
            c.zone_update_z = 8;
            std::sync::Arc::make_mut(&mut c.jagfx.delays)[3] = 7;
            // Sound at (11,11), radius 1: local (10,10) is on inclusive edge.
            match gate {
                1 => c.wave_enabled = false,
                2 => c.config.lowmem = true,
                3 => c.wave_count = 50,
                4 => c.local_player.as_mut().unwrap().route_x[0] = 9,
                _ => {}
            }
            let loops = if gate == 5 { 0 } else { 7 };
            let payload = [0x33, 0, 3, 0x18 | loops]; // bit3 ignored, loops may be 0
            let before = c.gens;
            let p = if enclosed {
                let mut frame = vec![8, 8, 91];
                frame.extend(payload);
                dispatch(&mut c, 112, &frame)
            } else {
                dispatch(&mut c, 91, &payload)
            };
            assert!(c.ingame, "gate={gate}, enclosed={enclosed}");
            assert_eq!(p.available(), 0);
            let accepted = gate == 0 || gate == 5;
            assert_eq!(
                c.wave_count,
                if gate == 3 { 50 } else { i32::from(accepted) }
            );
            if accepted {
                assert_eq!(
                    (c.wave_ids[0], c.wave_loops[0], c.wave_delay[0]),
                    (3, loops as i32, 7)
                );
            }
            assert_eq!(c.gens.scene, before.scene + u64::from(enclosed));
            assert_eq!(c.gens.player, before.player);
        }
    }
}
