//! Cleanup D fixtures are independently bit-packed from pinned 289 Java
//! client methods 212/185/172/153/128 and 226/124/222. No captured payloads.
use client::client::{Client, ClientConfig, ClientNpc, ClientPlayer, ClientRevision};
use client::config::{Cache, NpcType, SeqType};
use client::io::ServerProt;
use client::io::{Packet, ServerProt289};
use client::util::JString;
use std::sync::Arc;

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
    c.loop_cycle = 50;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.local_player.as_mut().unwrap().name = Some("Bob".into());
    c.local_player.as_mut().unwrap().ready = true;
    c.players[2047] = Some(Box::new(ClientPlayer::at(3, 4)));
    c
}

fn bits(fields: &[(usize, u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut position = 0;
    for &(width, value) in fields {
        assert!(value < (1 << width));
        for shift in (0..width).rev() {
            if position % 8 == 0 {
                bytes.push(0);
            }
            let last = bytes.last_mut().unwrap();
            *last |= (((value >> shift) & 1) as u8) << (7 - position % 8);
            position += 1;
        }
    }
    bytes
}

fn dispatch(c: &mut Client, id: i32, bytes: Vec<u8>) -> Packet {
    c.psize = bytes.len() as i32;
    let mut p = Packet::new(bytes);
    p.set_frame_end(c.psize as usize);
    c.handle_packet(id, &mut p);
    p
}

#[test]
fn truncated_combined_player_mask_does_not_apply_movement_or_prefix() {
    let mut c = client();
    c.local_player.as_mut().unwrap().face_entity = 77;
    c.player_count = 1;
    c.player_ids[0] = 3;
    c.players[3] = Some(Box::new(ClientPlayer::at(8, 9)));
    let before = c.gens;
    // Local walk east + mask, old player removed by count=0, sentinel.
    let mut frame = bits(&[(1, 1), (2, 1), (3, 4), (1, 1), (8, 0), (11, 2047)]);
    // FACEENTITY valid prefix, HITMARK truncated after damage/type.
    frame.extend([0x14, 0, 12, 5, 1]);
    dispatch(&mut c, ServerProt289::PLAYER_INFO, frame);
    assert!(!c.ingame);
    let local = c.local_player.as_ref().unwrap();
    assert_eq!(local.route_x[0], 10, "movement must remain staged on T2");
    assert_eq!(
        local.face_entity, 77,
        "valid mask prefix must remain staged"
    );
    assert_eq!(c.player_count, 1);
    assert!(c.players[3].is_some());
    assert_eq!(c.gens.player, before.player + 1, "only lifecycle reset");
    assert_eq!(c.gens.chat, before.chat + 1);
}

#[test]
fn local_walk_run_and_teleport_with_no_new_record_endings() {
    for kind in [1, 2, 3] {
        let mut c = client();
        let mut fields = vec![(1, 1), (2, kind)];
        match kind {
            1 => fields.extend([(3, 4), (1, 0)]),
            2 => fields.extend([(3, 4), (3, 1), (1, 0)]),
            3 => fields.extend([(2, 2), (7, 80), (7, 81), (1, 1), (1, 0)]),
            _ => unreachable!(),
        }
        fields.push((8, 0));
        let packet = dispatch(&mut c, ServerProt289::PLAYER_INFO, bits(&fields));
        assert!(c.ingame);
        assert_eq!(packet.pos, packet.data().len());
        let p = c.local_player.as_ref().unwrap();
        match kind {
            1 => assert_eq!(
                (p.route_x[0], p.route_z[0], p.route_length, p.route_run[0]),
                (11, 10, 1, false)
            ),
            2 => assert_eq!(
                (
                    p.route_x[0],
                    p.route_z[0],
                    p.route_length,
                    p.route_run[0],
                    p.route_run[1]
                ),
                (11, 11, 2, true, true)
            ),
            3 => {
                assert_eq!((p.route_x[0], p.route_z[0], p.route_length), (80, 81, 0));
                assert_eq!(
                    (p.x, p.z, c.minusedlevel),
                    (80 * 128 + 64, 81 * 128 + 64, 2)
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(c.players[2047].as_ref().unwrap().route_x[0], 3);
    }
}

#[test]
fn old_actor_retention_mask_walk_run_remove_and_count_shrink() {
    for npc in [false, true] {
        for kind in 0..6 {
            let mut c = client();
            if npc {
                seed_npc(&mut c);
            } else {
                seed_remote(&mut c);
            }
            let mut fields = Vec::new();
            if !npc {
                fields.push((1, 0));
            }
            fields.push((8, if kind == 5 { 0 } else { 1 }));
            match kind {
                0 => fields.push((1, 0)),
                1 => fields.extend([(1, 1), (2, 0)]),
                2 => fields.extend([(1, 1), (2, 1), (3, 4), (1, 0)]),
                3 => fields.extend([(1, 1), (2, 2), (3, 4), (3, 1), (1, 0)]),
                4 => fields.extend([(1, 1), (2, 3)]),
                _ => {}
            }
            // Player's remaining padding + mask exceeds method153's
            // 10-bit threshold, so it needs the new-player sentinel.
            if kind == 1 && !npc {
                fields.push((11, 2047));
            }
            let mut frame = bits(&fields);
            if kind == 1 {
                frame.push(0);
            }
            let id = if npc {
                ServerProt289::NPC_INFO
            } else {
                ServerProt289::PLAYER_INFO
            };
            let p = dispatch(&mut c, id, frame);
            assert!(c.ingame, "npc={npc} kind={kind}");
            assert_eq!(p.pos, p.data().len());
            let entity = if npc {
                c.npc[3].as_ref().map(|n| &n.entity)
            } else {
                c.players[3].as_ref().map(|p| &p.entity)
            };
            if kind >= 4 {
                assert!(entity.is_none());
                assert_eq!(c.entity_removal_count, 1);
            } else {
                let entity = entity.unwrap();
                assert_eq!(entity.cycle, 50);
                assert_eq!(
                    (entity.route_x[0], entity.route_z[0]),
                    match kind {
                        2 => (9, 9),
                        3 => (9, 10),
                        _ => (8, 9),
                    }
                );
                assert_eq!(c.entity_update_count, i32::from(kind == 1));
            }
            assert_eq!(
                if npc { c.npc_count } else { c.player_count },
                i32::from(kind < 4)
            );
        }
    }
}

#[test]
fn new_actor_offsets_metadata_and_readdition_keep_identity() {
    for npc in [false, true] {
        for sentinel in [false, true] {
            let mut c = client();
            cache(&mut c);
            if npc {
                seed_npc(&mut c);
            } else {
                seed_remote(&mut c);
            }
            let pointer = if npc {
                c.npc[3].as_deref().unwrap() as *const ClientNpc as usize
            } else {
                c.players[3].as_deref().unwrap() as *const ClientPlayer as usize
            };
            let mut fields = Vec::new();
            if !npc {
                fields.push((1, 0));
            }
            // Remove the old visibility entry, then re-add the same actor.
            fields.extend([(8, 0), (if npc { 14 } else { 11 }, 3)]);
            if npc {
                fields.push((11, 0));
            }
            fields.extend([(5, 31), (5, 15), (1, 1), (1, u32::from(sentinel))]);
            if sentinel {
                fields.push((if npc { 14 } else { 11 }, if npc { 16383 } else { 2047 }));
            }
            let mut frame = bits(&fields);
            if sentinel {
                frame.extend([4, 0, 12]);
            }
            let id = if npc {
                ServerProt289::NPC_INFO
            } else {
                ServerProt289::PLAYER_INFO
            };
            let p = dispatch(&mut c, id, frame);
            assert!(c.ingame);
            assert_eq!(p.pos, p.data().len());
            assert_eq!(c.entity_removal_count, 1);
            let entity = if npc {
                &c.npc[3].as_ref().unwrap().entity
            } else {
                &c.players[3].as_ref().unwrap().entity
            };
            assert_eq!((entity.route_x[0], entity.route_z[0]), (9, 25));
            assert_eq!(entity.cycle, 50);
            if sentinel {
                assert_eq!(entity.face_entity, 12);
            }
            let after = if npc {
                c.npc[3].as_deref().unwrap() as *const ClientNpc as usize
            } else {
                c.players[3].as_deref().unwrap() as *const ClientPlayer as usize
            };
            assert_eq!(pointer, after);
            if npc {
                assert_eq!(
                    (
                        entity.size,
                        entity.turnspeed,
                        entity.walkanim_l,
                        entity.walkanim_r
                    ),
                    (3, 17, 14, 13)
                );
            }
        }
    }
}

#[test]
fn new_player_appearance_cache_and_transformed_local_values() {
    let mut c = client();
    cache(&mut c);
    c.player_appearance_buffer[3] = Some(Packet::new(appearance(false)));
    // Local run east/north first; new player's relative origin is post-move.
    let fields = [
        (1, 1),
        (2, 2),
        (3, 4),
        (3, 1),
        (1, 0),
        (8, 0),
        (11, 3),
        (5, 16),
        (5, 1),
        (1, 1),
        (1, 0),
    ];
    dispatch(&mut c, ServerProt289::PLAYER_INFO, bits(&fields));
    assert!(c.ingame);
    let p = c.players[3].as_ref().unwrap();
    assert_eq!((p.route_x[0], p.route_z[0]), (-5, 12));
    assert_eq!(p.name.as_deref(), Some("A"));
    assert!(p.ready);
    c.local_player.as_mut().unwrap().appearance[5] = 300;
    let a = appearance(true);
    let mut body = vec![a.len() as u8];
    body.extend(a);
    // Preserve remote visibility while applying local appearance.
    let mut frame = bits(&[(1, 1), (2, 0), (8, 1), (1, 0), (11, 2047)]);
    frame.push(1);
    frame.extend(body);
    dispatch(&mut c, ServerProt289::PLAYER_INFO, frame);
    assert!(c.ingame);
    let p = c.local_player.as_ref().unwrap();
    assert_eq!(p.transmog, Some(0));
    assert_eq!(p.appearance[0], 65535);
    assert_eq!(p.appearance[5], 300, "transform stops part writes");
    assert_eq!((p.name.as_deref(), p.skill_level), (Some("A"), 321));
    assert_eq!(p.walkanim_l, -1);
}

#[test]
fn actor_animation_priority_duplicate_reset_and_spot_sentinel() {
    for npc in [false, true] {
        for (old, new, expected, reset) in [
            (0, 1, 0, false),
            (0, 2, 2, true),
            (0, 0, 0, true),
            (1, 1, 1, false),
            (0, 65535, -1, true),
        ] {
            let mut c = client();
            cache(&mut c);
            if npc {
                seed_npc(&mut c);
            }
            let e = if npc {
                &mut c.npc[3].as_mut().unwrap().entity
            } else {
                &mut c.local_player.as_mut().unwrap().entity
            };
            e.primary_anim = old;
            e.primary_anim_frame = 7;
            e.primary_anim_cycle = 8;
            e.primary_anim_delay = 9;
            e.primary_anim_loop = 4;
            let body = [(new >> 8) as u8, new as u8, 3];
            let frame = if npc {
                npc_mask(2, &body)
            } else {
                player_mask(true, 2, &body)
            };
            let id = if npc {
                ServerProt289::NPC_INFO
            } else {
                ServerProt289::PLAYER_INFO
            };
            dispatch(&mut c, id, frame);
            assert!(c.ingame);
            let e = if npc {
                &c.npc[3].as_ref().unwrap().entity
            } else {
                &c.local_player.as_ref().unwrap().entity
            };
            assert_eq!(e.primary_anim, expected);
            assert_eq!(
                (
                    e.primary_anim_frame,
                    e.primary_anim_cycle,
                    e.primary_anim_delay
                ),
                if reset { (0, 0, 3) } else { (7, 8, 9) }
            );
            assert_eq!(e.primary_anim_loop, if reset || old == new { 0 } else { 4 });
            let body = [255, 255, 0, 0, 0, 0];
            let frame = if npc {
                npc_mask(64, &body)
            } else {
                player_mask(true, 256, &body)
            };
            dispatch(&mut c, id, frame);
            assert!(c.ingame);
            let e = if npc {
                &c.npc[3].as_ref().unwrap().entity
            } else {
                &c.local_player.as_ref().unwrap().entity
            };
            assert_eq!(
                (e.spotanim_id, e.spotanim_frame, e.spotanim_last_cycle),
                (-1, 0, 50)
            );
        }
    }
}

#[test]
fn npc_truncated_combination_and_declared_bit_bounds_are_atomic() {
    let mut c = client();
    seed_npc(&mut c);
    c.npc[3].as_mut().unwrap().health = 77;
    let before = c.gens;
    // Valid HIT1 then truncated ANIM. Nothing may apply from the prefix.
    dispatch(
        &mut c,
        ServerProt289::NPC_INFO,
        npc_mask(3, &[5, 1, 20, 40, 0]),
    );
    assert!(!c.ingame);
    assert_eq!(c.npc[3].as_ref().unwrap().health, 77);
    assert_eq!(c.npc[3].as_ref().unwrap().cycle, 0);
    assert_eq!(c.gens.npc, before.npc + 1);
    for npc in [false, true] {
        for end in [0, 1] {
            let mut c = client();
            let mut p = Packet::new(vec![0x80; 64]);
            c.psize = end;
            p.set_frame_end(end as usize);
            c.handle_packet(
                if npc {
                    ServerProt289::NPC_INFO
                } else {
                    ServerProt289::PLAYER_INFO
                },
                &mut p,
            );
            assert!(!c.ingame);
            assert_eq!(c.local_player.as_ref().unwrap().route_x[0], 10);
        }
    }
}

#[test]
fn truncated_later_appearance_does_not_apply_earlier_actor_or_chat() {
    let mut c = client();
    seed_remote(&mut c);
    // Local SAY, remote APPEARANCE, complete outer block but invalid inner data.
    let mut frame = bits(&[(1, 1), (2, 0), (8, 1), (1, 1), (2, 0), (11, 2047)]);
    frame.extend([8, b'h', b'i', 10, 1, 1, 0]);
    dispatch(&mut c, ServerProt289::PLAYER_INFO, frame);
    assert!(!c.ingame);
    assert_eq!(c.chat_seq, 0);
    assert!(c.local_player.as_ref().unwrap().chat_message.is_none());
    assert_eq!(c.players[3].as_ref().unwrap().name.as_deref(), Some("Bob"));
    assert!(c.player_appearance_buffer[3].is_none());
}

#[test]
fn default_274_hit_timers_and_say_behavior_are_preserved() {
    let config = client().config;
    let mut c = Client::new(config);
    assert_eq!(c.revision(), ClientRevision::R274);
    c.ingame = true;
    c.loop_cycle = 50;
    c.local_player = Some(ClientPlayer::at(10, 10));
    c.players[2047] = Some(Box::new(ClientPlayer::default()));
    seed_npc(&mut c);
    for (id, frame) in [
        (
            ServerProt::PLAYER_INFO,
            player_mask(true, 16, &[5, 1, 20, 40]),
        ),
        (
            ServerProt::PLAYER_INFO,
            player_mask(true, 1024, &[5, 1, 20, 40]),
        ),
        (ServerProt::NPC_INFO, npc_mask(1, &[5, 1, 20, 40])),
        (ServerProt::NPC_INFO, npc_mask(16, &[5, 1, 20, 40])),
    ] {
        dispatch(&mut c, id, frame);
        assert!(c.ingame);
        let timer = if id == ServerProt::PLAYER_INFO {
            c.local_player.as_ref().unwrap().combat_cycle
        } else {
            c.npc[3].as_ref().unwrap().combat_cycle
        };
        assert_eq!(timer, 450);
    }
    dispatch(
        &mut c,
        ServerProt::PLAYER_INFO,
        player_mask(true, 8, b"~hello\n"),
    );
    assert!(c.ingame);
    assert_eq!(
        c.local_player.as_ref().unwrap().chat_message.as_deref(),
        Some("~hello")
    );
    assert_eq!(c.chat_seq, 0);
}

fn seed_npc(c: &mut Client) {
    c.npc_count = 1;
    c.npc_ids[0] = 3;
    c.npc[3] = Some(Box::new(ClientNpc::default()));
    c.npc[3].as_mut().unwrap().route_x[0] = 8;
    c.npc[3].as_mut().unwrap().route_z[0] = 9;
}

fn seed_remote(c: &mut Client) {
    c.player_count = 1;
    c.player_ids[0] = 3;
    let mut p = ClientPlayer::at(8, 9);
    p.name = Some("Bob".into());
    p.ready = true;
    c.players[3] = Some(Box::new(p));
}

fn cache(c: &mut Client) {
    c.cache = Arc::new(Cache {
        npcs: vec![NpcType {
            id: 0,
            size: 3,
            turnspeed: 17,
            readyanim: 10,
            walkanim: 11,
            walkanim_b: 12,
            walkanim_l: 13,
            walkanim_r: 14,
            ..Default::default()
        }],
        seqs: vec![
            SeqType {
                priority: 5,
                duplicatebehaviour: 1,
                ..Default::default()
            },
            SeqType {
                priority: 2,
                duplicatebehaviour: 2,
                ..Default::default()
            },
            SeqType {
                priority: 8,
                ..Default::default()
            },
        ],
        ..Default::default()
    });
}

fn player_mask(local: bool, mask: u16, body: &[u8]) -> Vec<u8> {
    let mut frame = if local {
        bits(&[(1, 1), (2, 0), (8, 0), (11, 2047)])
    } else {
        bits(&[(1, 0), (8, 1), (1, 1), (2, 0), (11, 2047)])
    };
    frame.push(mask as u8 | if mask > 255 { 128 } else { 0 });
    if mask > 255 {
        frame.push((mask >> 8) as u8);
    }
    frame.extend_from_slice(body);
    frame
}

fn npc_mask(mask: u8, body: &[u8]) -> Vec<u8> {
    let mut frame = bits(&[(8, 1), (1, 1), (2, 0), (14, 16383)]);
    frame.push(mask);
    frame.extend_from_slice(body);
    frame
}

// method39: gender/icons, variable parts, five colours, seven sequences,
// u64 namehash (1 => A), combat, skill. The transform branch ends parts early.
fn appearance(transform: bool) -> Vec<u8> {
    let mut a = vec![1, 2];
    if transform {
        a.extend([255, 255, 0, 0]);
    } else {
        a.extend([0; 12]);
    }
    a.extend([255, 0, 0, 0, 0]); // invalid colour clamps to zero
    a.extend([255; 14]);
    a.extend(1u64.to_be_bytes());
    a.push(42);
    a.extend(321u16.to_be_bytes());
    a
}

#[test]
fn every_player_single_mask_and_source_order_combination() {
    let a = appearance(false);
    let mut ap = vec![a.len() as u8];
    ap.extend(a);
    let cases = vec![
        (1, ap),
        (2, vec![0, 0, 3]),
        (4, vec![255, 255]),
        (8, b"~hello\n".to_vec()),
        (16, vec![5, 1, 20, 40]),
        (32, vec![0, 12, 0, 13]),
        (64, vec![2, 3, 2, 1, 0x65]),
        (256, vec![0, 7, 255, 254, 0, 5]),
        (512, vec![1, 2, 3, 4, 0, 10, 0, 20, 5]),
        (1024, vec![9, 2, 30, 50]),
    ];
    for (mask, body) in cases.iter().cloned().chain(std::iter::once((
        0x77f,
        cases.iter().flat_map(|(_, b)| b.iter().copied()).collect(),
    ))) {
        let mut c = client();
        cache(&mut c);
        c.local_player.as_mut().unwrap().route_length = 3;
        c.local_player.as_mut().unwrap().face_entity = 12;
        let before = c.gens;
        let frame = player_mask(true, mask, &body);
        let size = frame.len();
        let packet = dispatch(&mut c, ServerProt289::PLAYER_INFO, frame);
        assert!(c.ingame, "mask {mask:x}");
        assert_eq!(packet.pos, size);
        assert_eq!(c.gens.player, before.player + 1);
        assert_eq!(c.gens.npc, before.npc);
        assert_eq!(c.gens.chat, before.chat + u64::from(mask & (8 | 64) != 0));
        let p = c.local_player.as_ref().unwrap();
        if mask & 1 != 0 {
            assert_eq!(p.name.as_deref(), Some("A"));
            assert_eq!(
                (p.gender, p.headicons, p.combat_level, p.skill_level),
                (1, 2, 42, 321)
            );
            assert_eq!(p.colour, [0; 5]);
            assert_eq!(p.readyanim, -1);
            assert_eq!(p.base_id, 1);
            assert!(c.player_appearance_buffer[2047].is_some());
        }
        if mask & 2 != 0 {
            assert_eq!(
                (p.primary_anim, p.primary_anim_delay, p.preanim_route_length),
                (0, 3, if mask & 512 != 0 { 0 } else { 3 })
            );
        }
        if mask & 4 != 0 {
            assert_eq!(p.face_entity, -1);
        }
        if mask & 8 != 0 && mask & 64 == 0 {
            assert_eq!(p.chat_message.as_deref(), Some("hello"));
        }
        if mask & 16 != 0 {
            assert_eq!(
                (p.damage_values[0], p.damage_types[0], p.damage_cycles[0]),
                (5, 1, 120)
            );
        }
        if mask & 32 != 0 {
            assert_eq!((p.face_square_x, p.face_square_z), (12, 13));
        }
        if mask & 64 != 0 {
            assert_eq!((p.chat_colour, p.chat_effect, p.chat_timer), (2, 3, 150));
            assert_eq!(p.chat_message.as_deref(), Some("Hi"));
            assert_eq!(c.chat_type[0], 1);
            assert_eq!(c.chat_seq, if mask & 8 != 0 { 2 } else { 1 });
        }
        if mask & 256 != 0 {
            assert_eq!(
                (
                    p.spotanim_id,
                    p.spotanim_height,
                    p.spotanim_last_cycle,
                    p.spotanim_frame,
                    p.spotanim_cycle
                ),
                (7, -2, 55, -1, 0)
            );
        }
        if mask & 512 != 0 {
            assert_eq!(
                (
                    p.exact_start_x,
                    p.exact_start_z,
                    p.exact_end_x,
                    p.exact_end_z
                ),
                (1, 2, 3, 4)
            );
            assert_eq!(
                (
                    p.exact_move_end,
                    p.exact_move_start,
                    p.exact_move_facing,
                    p.route_length
                ),
                (60, 70, 5, 0)
            );
        }
        if mask & 1024 != 0 {
            let slot = usize::from(mask & 16 != 0);
            assert_eq!((p.damage_values[slot], p.damage_types[slot]), (9, 2));
            assert_eq!((p.health, p.total_health), (30, 50));
        } else if mask & 16 != 0 {
            assert_eq!((p.health, p.total_health), (20, 40));
        }
        if mask & (16 | 1024) != 0 {
            assert_eq!(p.combat_cycle, 350);
        }
        // Local mask writes never replace or move the stale array-owned slot.
        assert_eq!(c.players[2047].as_ref().unwrap().route_x[0], 3);
    }
}

#[test]
fn every_npc_single_mask_and_source_order_combination() {
    let cases = [
        (1, vec![5, 1, 20, 40]),
        (2, vec![0, 0, 3]),
        (4, vec![255, 255]),
        (8, b"~hi\n".to_vec()),
        (16, vec![9, 2, 30, 50]),
        (32, vec![0, 0]),
        (64, vec![0, 7, 255, 254, 0, 5]),
        (128, vec![0, 12, 0, 13]),
    ];
    for (mask, body) in cases.iter().cloned().chain(std::iter::once((
        255,
        cases.iter().flat_map(|(_, b)| b.iter().copied()).collect(),
    ))) {
        let mut c = client();
        cache(&mut c);
        seed_npc(&mut c);
        c.npc[3].as_mut().unwrap().face_entity = 12;
        let pointer = c.npc[3].as_deref().unwrap() as *const ClientNpc;
        let before = c.gens;
        let frame = npc_mask(mask, &body);
        let size = frame.len();
        let p = dispatch(&mut c, ServerProt289::NPC_INFO, frame);
        assert!(c.ingame, "mask {mask:x}");
        assert_eq!(p.pos, size);
        assert_eq!(c.gens.npc, before.npc + 1);
        assert_eq!(c.gens.player, before.player);
        assert_eq!(c.gens.chat, before.chat);
        let n = c.npc[3].as_deref().unwrap();
        assert_eq!(n as *const ClientNpc, pointer, "actor ownership preserved");
        if mask & 1 != 0 {
            assert_eq!(
                (n.damage_values[0], n.damage_types[0], n.damage_cycles[0]),
                (5, 1, 120)
            );
        }
        if mask & 2 != 0 {
            assert_eq!((n.primary_anim, n.primary_anim_delay), (0, 3));
        }
        if mask & 4 != 0 {
            assert_eq!(n.face_entity, -1);
        }
        if mask & 8 != 0 {
            assert_eq!(n.chat_message.as_deref(), Some("~hi"));
            assert_eq!(n.chat_timer, 100);
        }
        if mask & 16 != 0 {
            assert_eq!(
                (
                    n.damage_values[usize::from(mask & 1 != 0)],
                    n.health,
                    n.total_health
                ),
                (9, 30, 50)
            );
        } else if mask & 1 != 0 {
            assert_eq!((n.health, n.total_health), (20, 40));
        }
        if mask & 17 != 0 {
            assert_eq!(n.combat_cycle, 350);
        }
        if mask & 32 != 0 {
            assert_eq!(n.r#type, Some(0));
            assert_eq!(
                (
                    n.size,
                    n.turnspeed,
                    n.readyanim,
                    n.walkanim,
                    n.walkanim_b,
                    n.walkanim_l,
                    n.walkanim_r
                ),
                (3, 17, 10, 11, 12, 14, 13)
            );
        }
        if mask & 64 != 0 {
            assert_eq!(
                (
                    n.spotanim_id,
                    n.spotanim_height,
                    n.spotanim_last_cycle,
                    n.spotanim_frame,
                    n.spotanim_cycle
                ),
                (7, -2, 55, -1, 0)
            );
        }
        if mask & 128 != 0 {
            assert_eq!((n.face_square_x, n.face_square_z), (12, 13));
        }
    }
}

#[test]
fn say_local_remote_and_tilde_publish_only_actual_log() {
    for local in [false, true] {
        for tilde in [false, true] {
            let mut c = client();
            if !local {
                seed_remote(&mut c);
            }
            let before = c.gens;
            let body: &[u8] = if tilde { b"~hello\n" } else { b"hello\n" };
            dispatch(
                &mut c,
                ServerProt289::PLAYER_INFO,
                player_mask(local, 8, body),
            );
            assert!(c.ingame);
            let p = if local {
                c.local_player.as_ref().unwrap()
            } else {
                c.players[3].as_deref().unwrap()
            };
            assert_eq!(
                (
                    p.chat_message.as_deref(),
                    p.chat_timer,
                    p.chat_colour,
                    p.chat_effect
                ),
                (Some("hello"), 150, 0, 0)
            );
            assert_eq!(c.chat_seq, u64::from(local || tilde));
            assert_eq!(c.gens.chat, before.chat + u64::from(local || tilde));
            if local || tilde {
                assert_eq!(
                    (&c.chat_text[0], &c.chat_username[0], c.chat_type[0]),
                    (&"hello".to_owned(), &"Bob".to_owned(), 2)
                );
            }
        }
    }
}

#[test]
fn public_chat_staff_ignore_disabled_ready_and_name_gates() {
    for staff in [0u8, 1, 2, 3, 4] {
        for gate in 0..5 {
            let mut c = client();
            seed_remote(&mut c);
            match gate {
                1 => {
                    c.ignore_count = 1;
                    c.ignore_userhash[0] = JString::to_userhash("Bob") as i64;
                }
                2 => c.chat_disabled = 1,
                3 => c.players[3].as_mut().unwrap().ready = false,
                4 => c.players[3].as_mut().unwrap().name = None,
                _ => {}
            }
            let before = c.gens;
            let frame = player_mask(false, 64, &[2, 3, staff, 1, 0x65]);
            let size = frame.len();
            let p = dispatch(&mut c, ServerProt289::PLAYER_INFO, frame);
            assert!(c.ingame);
            assert_eq!(p.pos, size);
            let log = gate == 0 || (gate == 1 && staff > 1);
            assert_eq!(c.gens.chat, before.chat + u64::from(log));
            assert_eq!(c.chat_seq, u64::from(log));
            if log {
                assert_eq!(c.chat_text[0], "Hi");
                assert_eq!(c.chat_type[0], if (1..=3).contains(&staff) { 1 } else { 2 });
                assert_eq!(
                    c.chat_username[0],
                    match staff {
                        1 => "@cr1@Bob",
                        2 | 3 => "@cr2@Bob",
                        _ => "Bob",
                    }
                );
            } else {
                assert!(c.players[3].as_ref().unwrap().chat_message.is_none());
            }
        }
    }
}
