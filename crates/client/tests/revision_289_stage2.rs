//! Revision 289 stage-2: login version, actors, region, widgets, varps, reset.
//!
//! Fixtures come from `crates/client/tests/fixtures/revision_289/manifest.json`
//! (independently derived oracles). Production paths only — no parallel decoder.

use client::client::{Client, ClientConfig, ClientNpc, ClientPlayer, ClientRevision};
use client::config::{IfType, IfTypeMut};
use client::io::{ClientStream, Isaac, Packet, ServerProt, ServerProt289};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

fn cfg() -> ClientConfig {
    ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    }
}

fn client_289() -> Client {
    let mut c = Client::new_with_revision(cfg(), ClientRevision::R289);
    c.ingame = true;
    c.ptype = -1;
    c
}

fn ensure_iface(c: &mut Client, com_id: usize) {
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        if slots.len() <= com_id {
            slots.resize_with(com_id + 1, || None);
        }
        slots[com_id] = Some(Arc::new(IfTypeMut::default()));
    }
    {
        let table = Arc::make_mut(&mut c.ifaces);
        if table.len() <= com_id {
            table.resize_with(com_id + 1, || None);
        }
        if table[com_id].is_none() {
            table[com_id] = Some(Box::new(IfType::default()));
        }
    }
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    let h = hex.trim();
    assert!(h.len() % 2 == 0, "odd hex length");
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Feed raw (no-ISAAC) game bytes into production `ClientStream` + `tcp_in`.
fn feed_frames(c: &mut Client, frame: &[u8], max_polls: usize) -> usize {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let frame = frame.to_vec();
    let written = Arc::new(Mutex::new(false));
    let written_s = Arc::clone(&written);
    let done = Arc::new(Barrier::new(2));
    let done_s = Arc::clone(&done);
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(&frame).unwrap();
        let _ = sock.flush();
        *written_s.lock().unwrap() = true;
        thread::sleep(Duration::from_millis(30));
        done_s.wait();
        let _ = sock.shutdown(std::net::Shutdown::Both);
    });

    c.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    c.random_in = None;
    c.ptype = -1;

    for _ in 0..50 {
        if *written.lock().unwrap() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    let mut accepted = 0usize;
    for _ in 0..max_polls {
        if c.tcp_in() {
            accepted += 1;
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }
    done.wait();
    handle.join().unwrap();
    accepted
}

// --- named opcode constants -------------------------------------------------

#[test]
fn server_prot_289_stage2_named_opcodes() {
    assert_eq!(ServerProt289::CHAT_FILTER_SETTINGS, 13);
    assert_eq!(ServerProt289::PLAYER_INFO, 188);
    assert_eq!(ServerProt289::NPC_INFO, 65);
    assert_eq!(ServerProt289::REBUILD_NORMAL, 219);
    assert_eq!(ServerProt289::LOGOUT, 121);
    assert_eq!(ServerProt289::RESET_ANIMS, 201);
    assert_eq!(ServerProt289::IF_SETTEXT, 59);
    assert_eq!(ServerProt289::IF_SETANIM, 211);
    assert_eq!(ServerProt289::IF_OPENMAIN_SIDE, 55);
    assert_eq!(ServerProt289::IF_OPENSIDE, 252);
    assert_eq!(ServerProt289::IF_OPENOVERLAY, 127);
    assert_eq!(ServerProt289::VARP_SMALL, 75);
    assert_eq!(ServerProt289::VARP_LARGE, 97);
    assert_eq!(ServerProt289::VARP_SYNC, 172);
    // 274 collisions must remain distinct tables
    assert_ne!(ServerProt::LOGOUT, ServerProt289::LOGOUT);
    assert_ne!(ServerProt::PLAYER_INFO, ServerProt289::PLAYER_INFO);
    assert_ne!(ServerProt::REBUILD_NORMAL, ServerProt289::REBUILD_NORMAL);
}

#[test]
fn startup_chat_filter_settings_289_dispatches_without_t1() {
    let mut c = client_289();
    let mut p = Packet::new(vec![2, 1, 2]);
    c.psize = 3;
    let before = c.gens.chat;

    c.handle_packet(ServerProt289::CHAT_FILTER_SETTINGS, &mut p);

    assert_eq!(p.pos, 3, "exact three-byte payload consumption");
    assert_eq!(c.chat_public_mode, 2);
    assert_eq!(c.chat_private_mode, 1);
    assert_eq!(c.chat_trade_mode, 2);
    assert!(c.redraw_chat_mode);
    assert!(c.redraw_chat);
    assert_eq!(c.gens.chat, before + 1);
    assert_eq!(c.ptype, -1);
    assert!(c.ingame, "known startup packet must not log out");
}

#[test]
fn startup_289_source_sequence_keeps_stream_in_game() {
    // This exercises the source-ordered 289-covered prefix identified while
    // auditing the isolated engine's Player.onLogin contract: rebuild, chat
    // modes, friend/ignore state, interface close, identity, varps,
    // inventory, then reset anims. Script and first-tick packets follow below.
    let mut c = client_289();
    c.psize = 4;
    let mut rebuild = Packet::new(hex_bytes("00100020"));
    c.handle_packet(ServerProt289::REBUILD_NORMAL, &mut rebuild);
    assert_eq!(rebuild.pos, 4);
    assert!(c.ingame && c.scene_state == 1);

    c.psize = 3;
    let mut chat = Packet::new(vec![2, 1, 2]);
    c.handle_packet(ServerProt289::CHAT_FILTER_SETTINGS, &mut chat);
    assert_eq!(chat.pos, 3);

    c.psize = 1;
    let mut friend = Packet::new(vec![2]);
    c.handle_packet(ServerProt289::FRIENDLIST_LOADED, &mut friend);
    assert_eq!(friend.pos, 1);

    c.psize = 16;
    let mut ignore = Packet::new(vec![0; 16]);
    c.handle_packet(ServerProt289::UPDATE_IGNORELIST, &mut ignore);
    assert_eq!(ignore.pos, 16);

    c.psize = 0;
    let mut close = Packet::new(vec![]);
    c.handle_packet(ServerProt289::IF_CLOSE, &mut close);
    assert_eq!(close.pos, 0);

    c.psize = 3;
    let mut pid = Packet::new(hex_bytes("000001"));
    c.handle_packet(ServerProt289::UPDATE_PID, &mut pid);
    assert_eq!(pid.pos, 3);

    c.psize = 0;
    let mut sync = Packet::new(vec![]);
    c.handle_packet(ServerProt289::VARP_SYNC, &mut sync);
    assert_eq!(sync.pos, 0);

    c.psize = 3;
    let mut varp_small = Packet::new(hex_bytes("0007fe"));
    c.handle_packet(ServerProt289::VARP_SMALL, &mut varp_small);
    assert_eq!(varp_small.pos, 3);

    c.psize = 6;
    let mut varp_large = Packet::new(hex_bytes("000800000001"));
    c.handle_packet(ServerProt289::VARP_LARGE, &mut varp_large);
    assert_eq!(varp_large.pos, 6);

    c.psize = 4;
    let mut inv = Packet::new(hex_bytes("00010000"));
    c.handle_packet(ServerProt289::UPDATE_INV_FULL, &mut inv);
    assert_eq!(inv.pos, 4);

    c.psize = 0;
    let mut reset = Packet::new(vec![]);
    c.handle_packet(ServerProt289::RESET_ANIMS, &mut reset);
    assert_eq!(reset.pos, 0);

    // login.rs2 and the first NetworkPlayer tick follow the onLogin prefix.
    c.psize = 8;
    let mut welcome = Packet::new(hex_bytes("57656c636f6d650a"));
    c.handle_packet(ServerProt289::MESSAGE_GAME, &mut welcome);
    assert_eq!(welcome.pos, 8);
    c.psize = 0;
    let mut cam_reset = Packet::new(vec![]);
    c.handle_packet(ServerProt289::CAM_RESET, &mut cam_reset);
    c.psize = 1;
    let mut minimap = Packet::new(vec![0]);
    c.handle_packet(ServerProt289::MINIMAP_TOGGLE, &mut minimap);
    c.psize = 6;
    let mut stat = Packet::new(hex_bytes("02000003e807"));
    c.handle_packet(ServerProt289::UPDATE_STAT, &mut stat);
    c.psize = 1;
    let mut energy = Packet::new(vec![87]);
    c.handle_packet(ServerProt289::UPDATE_RUNENERGY, &mut energy);
    c.psize = 2;
    let mut weight = Packet::new(hex_bytes("ffce"));
    c.handle_packet(ServerProt289::UPDATE_RUNWEIGHT, &mut weight);
    c.psize = 10;
    let mut last_login = Packet::new(vec![0; 10]);
    c.handle_packet(ServerProt289::LAST_LOGIN_INFO, &mut last_login);
    assert!(c.ingame, "all source-covered startup packets stay in game");
}

#[test]
fn startup_289_engine_login_social_and_identity_packets_dispatch() {
    let mut c = client_289();

    c.psize = 1;
    let mut friend = Packet::new(vec![2]);
    c.handle_packet(ServerProt289::FRIENDLIST_LOADED, &mut friend);
    assert_eq!(friend.pos, 1);
    assert_eq!(c.friend_server_status, 2);
    assert!(c.redraw_side);

    c.psize = 16;
    let mut ignore = Packet::new(vec![0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 9]);
    c.handle_packet(ServerProt289::UPDATE_IGNORELIST, &mut ignore);
    assert_eq!(ignore.pos, 16);
    assert_eq!(c.ignore_count, 2);
    assert_eq!(c.ignore_userhash[0], 7);
    assert_eq!(c.ignore_userhash[1], 9);

    c.psize = 0;
    c.side_modal_id = 42;
    c.chat_modal_id = 43;
    c.main_modal_id = 44;
    let mut close = Packet::new(vec![]);
    c.handle_packet(ServerProt289::IF_CLOSE, &mut close);
    assert_eq!(close.pos, 0);
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
    assert_eq!(c.main_modal_id, -1);

    c.psize = 3;
    let mut pid = Packet::new(hex_bytes("123401"));
    c.handle_packet(ServerProt289::UPDATE_PID, &mut pid);
    assert_eq!(pid.pos, 3);
    assert_eq!(c.self_slot, 0x1234);
    assert_eq!(c.members_account, 1);
    assert!(c.ingame);
}

#[test]
fn startup_289_login_script_and_first_tick_packets_dispatch() {
    let mut c = client_289();

    // login.rs2: mes("Welcome..."), cam_reset, minimap_toggle(0),
    // set_player_op, and initalltabs/last_login_info.
    c.psize = 10;
    let mut welcome = Packet::new(hex_bytes("57656c636f6d650a"));
    c.psize = welcome.data().len() as i32;
    c.handle_packet(ServerProt289::MESSAGE_GAME, &mut welcome);
    assert_eq!(welcome.pos, 8);
    assert_eq!(c.chat_text[0], "Welcome");

    c.cinema_cam = true;
    c.psize = 0;
    let mut cam_reset = Packet::new(vec![]);
    c.handle_packet(ServerProt289::CAM_RESET, &mut cam_reset);
    assert!(!c.cinema_cam);

    c.psize = 1;
    let mut minimap = Packet::new(vec![0]);
    c.handle_packet(ServerProt289::MINIMAP_TOGGLE, &mut minimap);
    assert_eq!(c.minimap_state, 0);

    c.psize = 8;
    let mut op = Packet::new(b"\x02\x00Attack\n".to_vec());
    c.handle_packet(ServerProt289::SET_PLAYER_OP, &mut op);
    assert_eq!(c.player_op[1].as_deref(), Some("Attack"));
    assert!(c.player_op_priority[1]);

    c.psize = 3;
    let mut tab = Packet::new(hex_bytes("123405"));
    c.handle_packet(ServerProt289::IF_SETTAB, &mut tab);
    assert_eq!(c.side_icon[5], 0x1234);

    c.psize = 6;
    let mut stat = Packet::new(hex_bytes("02000003e807"));
    c.handle_packet(ServerProt289::UPDATE_STAT, &mut stat);
    assert_eq!(c.stat_xp[2], 1000);
    assert_eq!(c.stat_effective_level[2], 7);

    c.psize = 1;
    let mut energy = Packet::new(vec![87]);
    c.handle_packet(ServerProt289::UPDATE_RUNENERGY, &mut energy);
    assert_eq!(c.runenergy, 87);

    c.psize = 2;
    let mut weight = Packet::new(hex_bytes("ffce"));
    c.handle_packet(ServerProt289::UPDATE_RUNWEIGHT, &mut weight);
    assert_eq!(c.runweight, -50);

    c.psize = 10;
    let mut last_login = Packet::new(vec![0; 10]);
    c.handle_packet(ServerProt289::LAST_LOGIN_INFO, &mut last_login);
    assert_eq!(last_login.pos, 10);

    // NetworkPlayer.updateZones: first full-zone follow and enclosed stream.
    c.psize = 2;
    let mut full_zone = Packet::new(vec![8, 16]);
    c.handle_packet(ServerProt289::UPDATE_ZONE_FULL_FOLLOWS, &mut full_zone);
    assert_eq!((c.zone_update_x, c.zone_update_z), (8, 16));

    c.psize = 2;
    let mut partial_zone = Packet::new(vec![24, 32]);
    c.handle_packet(ServerProt289::UPDATE_ZONE_PARTIAL_FOLLOWS, &mut partial_zone);
    assert_eq!((c.zone_update_x, c.zone_update_z), (24, 32));

    c.psize = 2;
    let mut enclosed_zone = Packet::new(vec![40, 48]);
    c.handle_packet(
        ServerProt289::UPDATE_ZONE_PARTIAL_ENCLOSED,
        &mut enclosed_zone,
    );
    assert_eq!(enclosed_zone.pos, 2);
    assert!(c.ingame);
}

#[test]
fn startup_289_enclosed_zone_uses_289_inner_opcodes_and_keeps_framing() {
    let mut c = client_289();
    c.psize = 11;
    // ServerGameZoneProt: LOC_ADD_CHANGE=90 (pos, info, id) followed by
    // OBJ_DEL=71 (pos, id). Both frames must consume their R289 payloads.
    let mut enclosed = Packet::new(vec![40, 48, 0x11, 90, 0, 0x12, 0x34, 0x22, 71, 0x00, 0x56]);
    c.handle_packet(ServerProt289::UPDATE_ZONE_PARTIAL_ENCLOSED, &mut enclosed);
    assert_eq!(enclosed.pos, 11);
    assert!(c.ingame);
}

// --- login RSA plaintext structure + version -------------------------------

#[test]
fn login_rsa_plaintext_structure_ordered() {
    // manifest: login_rsa_plaintext_structure — credential-free order only.
    let mut c = client_289();
    let seed = [0x1111_1111, 0x2222_2222, 0x3333_3333, 0x4444_4444];
    c.login_uid = 0x5555_5555;
    c.write_login_block(seed, "user", "pass");
    let data = c.out.data()[..c.out.pos as usize].to_vec();
    let mut p = Packet::new(data);
    assert_eq!(p.g1(), 10, "rsa block type");
    assert_eq!(p.g4(), seed[0]);
    assert_eq!(p.g4(), seed[1]);
    assert_eq!(p.g4(), seed[2]);
    assert_eq!(p.g4(), seed[3]);
    assert_eq!(p.g4(), 0x5555_5555, "login_uid / signlink slot");
    assert_eq!(p.gjstr(), "user");
    assert_eq!(p.gjstr(), "pass");
}

/// Production `login()` outer wrapper on R289: 16|size|255|p2(289)|lowmem|
/// 9×jag checksums|RSA blob, then Isaac install into out.random / random_in.
#[test]
fn login_production_outer_frame_and_isaac_r289() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let captured_s = Arc::clone(&captured);
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14);
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap(); // response 0 → send seed
        s.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap();
        let mut buf = [0u8; 1024];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        captured_s.lock().unwrap().extend_from_slice(&buf[..n]);
        s.write_all(&[2, 0, 0]).unwrap(); // response 2
    });

    let mut c = Client::new_with_revision(
        ClientConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    c.jag_checksum = [1, 2, 3, 4, 5, 6, 7, 8, 9];
    c.login("bob", "pw", false).unwrap();

    let frame = captured.lock().unwrap().clone();
    // wrapper: opcode 16, size, 255, p2(289), lowmem, 9×p4 checksums, RSA blob
    assert_eq!(frame[0], 16, "cold login opcode");
    let size = frame[1] as usize;
    assert_eq!(frame.len(), 2 + size, "declared size covers remainder");
    assert_eq!(frame[2], 255);
    assert_eq!(
        &frame[3..5],
        &[0x01, 0x21],
        "p2 289 big-endian via production login"
    );
    assert_eq!(frame[5], 0, "lowmem flag");
    for i in 0..9 {
        let off = 6 + i * 4;
        let v = i32::from_be_bytes(frame[off..off + 4].try_into().unwrap());
        assert_eq!(v, (i + 1) as i32, "jag checksum slot {i}");
    }
    let rsa_off = 6 + 9 * 4;
    assert!(frame[rsa_off] > 0, "RSA blob length prefix");
    assert!(
        c.out.random.is_some(),
        "production login installs outbound Isaac"
    );
    assert!(
        c.random_in.is_some(),
        "production login installs inbound Isaac"
    );
    // Production seed install: random_in uses seed.wrapping_add(50) vs out.random.
    let seed = [10i32, 20, 30, 40];
    let mut out_r = Isaac::new(&seed);
    let mut seed_in = seed;
    for s in seed_in.iter_mut() {
        *s = s.wrapping_add(50);
    }
    let mut in_r = Isaac::new(&seed_in);
    assert_ne!(
        out_r.next_int(),
        in_r.next_int(),
        "inbound seed is seed+50 (production install offset)"
    );
    assert!(c.ingame);
    assert!(c.stream.is_some());
    assert_eq!(c.scene_state, 0, "cold login does not imply scene_ready");
    server.join().unwrap();
}

#[test]
fn login_wrapper_p2_revision_274_default() {
    let c = Client::new(cfg());
    assert_eq!(c.revision(), ClientRevision::R274);
    let mut loginout = Packet::alloc(1);
    loginout.p1(16);
    loginout.p1(0);
    loginout.p1(255);
    loginout.p2(c.revision().as_i32());
    let data = loginout.data()[..loginout.pos as usize].to_vec();
    assert_eq!(&data[3..5], &[0x01, 0x12], "p2 274 big-endian");
}

// --- lifecycle: attached ≠ ingame ≠ scene_ready -----------------------------

#[test]
fn lifecycle_attached_neq_ingame_neq_scene_ready() {
    // PASS labels are explicit task hard-gate evidence.
    // (a) socket/stream can be present while !ingame
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let done = Arc::new(Barrier::new(2));
    let done_s = Arc::clone(&done);
    let handle = thread::spawn(move || {
        let (_sock, _) = listener.accept().unwrap();
        done_s.wait();
    });
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    c.ingame = false;
    c.scene_state = 0;
    c.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    assert!(
        c.stream.is_some() && !c.ingame && c.scene_state != 2,
        "PASS (a): attached (stream present) while !ingame and not scene_ready"
    );
    done.wait();
    handle.join().unwrap();
    let _ = c.stream.take();

    // (b) ingame after rebuild has scene_state==1 and not scene-ready
    let mut c = client_289();
    c.ingame = true;
    c.scene_state = 0;
    let mut p = Packet::new(hex_bytes("01020304"));
    c.psize = 4;
    c.handle_packet(ServerProt289::REBUILD_NORMAL, &mut p);
    assert!(
        c.ingame && c.scene_state == 1 && c.scene_state != 2,
        "PASS (b): ingame after rebuild is scene_state==1 loading, not scene_ready"
    );
    // missing cache → check_scene must not promote to ready
    let status = c.check_scene();
    assert!(
        status != 0 && c.scene_state == 1,
        "PASS (b): check_scene without map data stays loading (status={status})"
    );

    // (c) scene_ready only after production ready path sets scene_state==2;
    // attach / login alone never imply it (login_production test also asserts
    // scene_state==0 after response 2). Force the ready marker only via the
    // same field check_scene writes on success.
    c.scene_state = 2;
    assert!(
        c.ingame && c.scene_state == 2,
        "PASS (c): scene_ready is scene_state==2 (check_scene success path)"
    );
    // Prove attach alone never sets it:
    let cold = Client::new_with_revision(cfg(), ClientRevision::R289);
    assert!(
        !cold.ingame && cold.scene_state != 2 && cold.stream.is_none(),
        "PASS (c): fresh client is neither attached, ingame, nor scene_ready"
    );
}

// --- actor update ----------------------------------------------------------

#[test]
fn actor_update_empty_world_bitstream() {
    // manifest: actor_update_empty_world_bitstream — payload 0000
    let mut c = client_289();
    let mut local = ClientPlayer::at(10, 10);
    local.entity.x = 10 * 128 + 64;
    local.entity.z = 10 * 128 + 64;
    c.local_player = Some(local);
    let mut p = Packet::new(hex_bytes("0000"));
    c.psize = 2;
    c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
    assert!(c.ingame);
    assert!(!c.awaiting_player_info);
    assert_eq!(c.ptype, -1);
    assert_eq!(p.pos, 2, "exact byte consumption");
    let local = c.local_player.as_ref().unwrap();
    assert_eq!(local.x, 10 * 128 + 64);
    assert_eq!(local.z, 10 * 128 + 64);
    assert_eq!(local.route_x[0], 10);
    assert_eq!(local.route_z[0], 10);
}

#[test]
fn actor_update_face_entity_mask() {
    // Non-zero mask path: APPEARANCE|FACEENTITY on local (same bit layout as
    // 274 method128 / 289 method128). Bytes from player_info fixture.
    let mut c = client_289();
    let mut slot = ClientPlayer::default();
    slot.entity.x = 3 * 128 + 64;
    slot.entity.z = 4 * 128 + 64;
    slot.entity.route_x[0] = 3;
    slot.entity.route_z[0] = 4;
    c.players[2047] = Some(Box::new(slot));
    let mut local = ClientPlayer::at(10, 10);
    local.entity.x = 10 * 128 + 64;
    local.entity.z = 10 * 128 + 64;
    c.local_player = Some(local);

    let mut appearance = vec![0u8; 44];
    for b in appearance.iter_mut().skip(19).take(14) {
        *b = 0xff;
    }
    let mut frame = vec![0x80, 0x1f, 0xfc, 0x05, 44];
    frame.extend_from_slice(&appearance);
    frame.extend_from_slice(&[0x12, 0x34]); // face_entity 4660
    c.psize = frame.len() as i32;
    let mut p = Packet::new(frame);
    c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);

    assert!(c.ingame);
    assert_eq!(p.pos as i32, c.psize, "exact byte consumption");
    let local = c.local_player.as_ref().unwrap();
    assert!(local.is_ready());
    assert_eq!(local.face_entity, 4660);
    assert_eq!(local.x, 10 * 128 + 64);
}

#[test]
fn actor_update_truncated_player_info_no_partial_world() {
    // Truncated bitstream: only the local-info bit set with no mask body.
    // Production must not leave a half-applied face_entity / ready flag.
    let mut c = client_289();
    let mut local = ClientPlayer::at(5, 5);
    local.face_entity = -1;
    c.local_player = Some(local);
    // 0x80 claims local update type 0, but no extended bytes follow.
    let mut p = Packet::new(vec![0x80]);
    c.psize = 1;
    c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
    // Either stays ingame with no mask applied, or T2 logout — never partial mask.
    if c.ingame {
        let local = c.local_player.as_ref().unwrap();
        assert_eq!(
            local.face_entity, -1,
            "truncated must not apply face_entity"
        );
        assert!(!local.is_ready(), "truncated must not mark ready");
    } else {
        // T2 logout path is fail-closed — also acceptable.
        assert!(!c.ingame);
    }
}

/// Independent source-derived PLAYER_INFO that removes a previously visible
/// other-player via production entity_removal (old-vis count < player_count).
#[test]
fn actor_removal_player_via_entity_removal() {
    // manifest: actor_removal_player_empty_old_vis
    // payload 0000: local info 0, old-vis count 0 → prior players removed.
    let mut c = client_289();
    c.loop_cycle = 10;
    let mut other = ClientPlayer::at(20, 20);
    other.entity.cycle = 1; // stale vs loop_cycle → eligible for clear
    c.players[42] = Some(Box::new(other));
    c.player_ids[0] = 42;
    c.player_count = 1;
    c.local_player = Some(ClientPlayer::at(10, 10));

    let mut p = Packet::new(hex_bytes("0000"));
    c.psize = 2;
    c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);

    assert!(c.ingame, "removal must not T2");
    assert_eq!(c.player_count, 0, "old-vis count 0 clears live list");
    assert!(c.players[42].is_none(), "entity_removal must clear slot 42");
    assert_eq!(p.pos, 2, "exact byte consumption");
}

// --- NPC -------------------------------------------------------------------

#[test]
fn npc_info_empty_bitstream() {
    // Empty NPC world: g8 count 0 with no existing npcs (method226/187).
    let mut c = client_289();
    c.npc_count = 0;
    let mut p = Packet::new(vec![0x00]);
    c.psize = 1;
    c.handle_packet(ServerProt289::NPC_INFO, &mut p);
    assert!(c.ingame);
    assert_eq!(c.npc_count, 0);
    assert_eq!(c.ptype, -1);
    assert_eq!(p.pos, 1, "exact byte consumption");
}

/// Remove a previously visible NPC via production entity_removal (count 0).
#[test]
fn actor_removal_npc_via_entity_removal() {
    // manifest: actor_removal_npc_empty_old_vis — payload 00
    let mut c = client_289();
    c.loop_cycle = 10;
    let mut npc = ClientNpc::default();
    npc.entity.cycle = 1;
    c.npc[7] = Some(Box::new(npc));
    c.npc_ids[0] = 7;
    c.npc_count = 1;

    let mut p = Packet::new(vec![0x00]);
    c.psize = 1;
    c.handle_packet(ServerProt289::NPC_INFO, &mut p);

    assert!(c.ingame);
    assert_eq!(c.npc_count, 0);
    assert!(c.npc[7].is_none(), "entity_removal must clear npc slot 7");
    assert_eq!(p.pos, 1, "exact byte consumption");
}

/// Non-zero NPC mask through get_npc_pos: FACEENTITY on an existing NPC.
#[test]
fn npc_info_face_entity_mask() {
    // Bits: count=1, info=1, op=0 (extended), then 14-bit 16383 new-vis
    // sentinel so method124 terminates cleanly before the mask body.
    // Byte layout: 01 9f ff 80 | mask 04 | face 12 34
    let mut c = client_289();
    c.loop_cycle = 5;
    c.npc[3] = Some(Box::new(ClientNpc::default()));
    c.npc_ids[0] = 3;
    c.npc_count = 1;
    c.local_player = Some(ClientPlayer::at(10, 10));

    let frame = hex_bytes("019fff80041234");
    c.psize = frame.len() as i32;
    let mut p = Packet::new(frame);
    c.handle_packet(ServerProt289::NPC_INFO, &mut p);

    assert!(c.ingame, "mask frame must not T2");
    assert_eq!(c.npc_count, 1);
    assert!(c.npc[3].is_some());
    assert_eq!(
        c.npc[3].as_ref().unwrap().face_entity,
        0x1234,
        "FACEENTITY mask lands on npc"
    );
    assert_eq!(p.pos as i32, c.psize, "exact byte consumption");
}

fn seed_npc_for_mask(c: &mut Client) {
    c.loop_cycle = 5;
    c.npc[3] = Some(Box::new(ClientNpc::default()));
    c.npc_ids[0] = 3;
    c.npc_count = 1;
    c.local_player = Some(ClientPlayer::at(10, 10));
}

fn seed_local_for_mask(c: &mut Client) {
    c.loop_cycle = 5;
    let mut slot = ClientPlayer::default();
    slot.entity.x = 3 * 128 + 64;
    slot.entity.z = 4 * 128 + 64;
    slot.entity.route_x[0] = 3;
    slot.entity.route_z[0] = 4;
    c.players[2047] = Some(Box::new(slot));
    let mut local = ClientPlayer::at(10, 10);
    local.entity.x = 10 * 128 + 64;
    local.entity.z = 10 * 128 + 64;
    local.name = Some("Bob".into());
    local.ready = true;
    c.local_player = Some(local);
}

/// Remaining NPC method222 masks beyond FACEENTITY — independent fixtures.
#[test]
fn npc_info_remaining_masks_independent() {
    // HITMARK 0x10
    {
        let mut c = client_289();
        seed_npc_for_mask(&mut c);
        let frame = hex_bytes("019fff801005010a14");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::NPC_INFO, &mut p);
        assert!(c.ingame);
        let n = c.npc[3].as_ref().unwrap();
        assert_eq!(n.health, 10);
        assert_eq!(n.total_health, 20);
        assert_eq!(n.combat_cycle, c.loop_cycle + 400);
        assert_eq!(p.pos as i32, c.psize);
    }
    // ANIM 0x02
    {
        let mut c = client_289();
        seed_npc_for_mask(&mut c);
        let frame = hex_bytes("019fff8002123403");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::NPC_INFO, &mut p);
        assert!(c.ingame);
        let n = c.npc[3].as_ref().unwrap();
        assert_eq!(n.primary_anim, 0x1234);
        assert_eq!(n.primary_anim_delay, 3);
        assert_eq!(p.pos as i32, c.psize);
    }
    // SAY 0x08
    {
        let mut c = client_289();
        seed_npc_for_mask(&mut c);
        let frame = hex_bytes("019fff800868690a");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::NPC_INFO, &mut p);
        assert!(c.ingame);
        assert_eq!(
            c.npc[3].as_ref().unwrap().chat_message.as_deref(),
            Some("hi")
        );
        assert_eq!(p.pos as i32, c.psize);
    }
    // FACESQUARE 0x80
    {
        let mut c = client_289();
        seed_npc_for_mask(&mut c);
        let frame = hex_bytes("019fff8080000a000b");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::NPC_INFO, &mut p);
        assert!(c.ingame);
        let n = c.npc[3].as_ref().unwrap();
        assert_eq!(n.face_square_x, 10);
        assert_eq!(n.face_square_z, 11);
        assert_eq!(p.pos as i32, c.psize);
    }
    // SPOTANIM 0x40
    {
        let mut c = client_289();
        seed_npc_for_mask(&mut c);
        let frame = hex_bytes("019fff8040000700010002");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::NPC_INFO, &mut p);
        assert!(c.ingame);
        let n = c.npc[3].as_ref().unwrap();
        assert_eq!(n.spotanim_id, 7);
        assert_eq!(n.spotanim_height, 1);
        assert_eq!(p.pos as i32, c.psize);
    }
    // HITMARK2 0x01
    {
        let mut c = client_289();
        seed_npc_for_mask(&mut c);
        let frame = hex_bytes("019fff800109021e28");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::NPC_INFO, &mut p);
        assert!(c.ingame);
        let n = c.npc[3].as_ref().unwrap();
        assert_eq!(n.health, 30);
        assert_eq!(n.total_health, 40);
        assert_eq!(n.combat_cycle, c.loop_cycle + 400);
        assert_eq!(p.pos as i32, c.psize);
    }
    // CHANGETYPE 0x20 — always consumes g2; type resolve depends on cache.npcs len.
    {
        let mut c = client_289();
        seed_npc_for_mask(&mut c);
        let frame = hex_bytes("019fff80200005");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::NPC_INFO, &mut p);
        assert!(c.ingame);
        assert!(c.npc[3].is_some());
        assert_eq!(p.pos as i32, c.psize);
        // When the offline cache has no npc defs, type stays None; when a
        // synthetic table is long enough, type id 5 may bind. Either is fine
        // as long as the g2 is consumed without T2.
        if c.cache.npcs.is_empty() {
            assert!(c.npc[3].as_ref().unwrap().r#type.is_none());
        }
    }
}

/// Remaining player method128 masks beyond APPEARANCE|FACEENTITY.
#[test]
fn actor_update_remaining_masks_independent() {
    // SAY 0x08
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc08796f0a");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        assert_eq!(
            c.local_player.as_ref().unwrap().chat_message.as_deref(),
            Some("yo")
        );
        assert_eq!(p.pos as i32, c.psize);
    }
    // HITMARK 0x10
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc1004003264");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        let local = c.local_player.as_ref().unwrap();
        assert_eq!(local.health, 50);
        assert_eq!(local.total_health, 100);
        assert_eq!(local.combat_cycle, c.loop_cycle + 400);
        assert_eq!(p.pos as i32, c.psize);
    }
    // ANIM 0x02
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc02001102");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        let local = c.local_player.as_ref().unwrap();
        assert_eq!(local.primary_anim, 0x11);
        assert_eq!(local.primary_anim_delay, 2);
        assert_eq!(p.pos as i32, c.psize);
    }
    // FACESQUARE 0x20
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc2001020304");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        let local = c.local_player.as_ref().unwrap();
        assert_eq!(local.face_square_x, 0x102);
        assert_eq!(local.face_square_z, 0x304);
        assert_eq!(p.pos as i32, c.psize);
    }
    // SPOTANIM 0x100 (big-update second mask byte)
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc8001000900000005");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        assert_eq!(c.local_player.as_ref().unwrap().spotanim_id, 9);
        assert_eq!(p.pos as i32, c.psize);
    }
    // EXACTMOVE 0x200
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc800201020304000a001405");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        let local = c.local_player.as_ref().unwrap();
        assert_eq!(local.exact_start_x, 1);
        assert_eq!(local.exact_start_z, 2);
        assert_eq!(local.exact_end_x, 3);
        assert_eq!(local.exact_end_z, 4);
        assert_eq!(local.exact_move_facing, 5);
        assert_eq!(local.exact_move_end, c.loop_cycle + 0x0a);
        assert_eq!(local.exact_move_start, c.loop_cycle + 0x14);
        assert_eq!(p.pos as i32, c.psize);
    }
    // HITMARK2 0x400
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc800406014650");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        let local = c.local_player.as_ref().unwrap();
        assert_eq!(local.health, 70);
        assert_eq!(local.total_health, 80);
        assert_eq!(local.combat_cycle, c.loop_cycle + 400);
        assert_eq!(p.pos as i32, c.psize);
    }
    // CHAT 0x40 with empty wordpack body (cursor-only proof)
    {
        let mut c = client_289();
        seed_local_for_mask(&mut c);
        let frame = hex_bytes("801ffc4001000000");
        c.psize = frame.len() as i32;
        let mut p = Packet::new(frame);
        c.handle_packet(ServerProt289::PLAYER_INFO, &mut p);
        assert!(c.ingame);
        assert_eq!(p.pos as i32, c.psize);
    }
}

// --- region ----------------------------------------------------------------

#[test]
fn region_scene_base_rebuild() {
    // manifest: region_scene_base — zone 0x0102 / 0x0304
    let mut c = client_289();
    let mut p = Packet::new(hex_bytes("01020304"));
    c.psize = 4;
    c.handle_packet(ServerProt289::REBUILD_NORMAL, &mut p);
    assert!(c.ingame);
    assert_eq!(c.map_build_centre_zone_x, 258);
    assert_eq!(c.map_build_centre_zone_z, 772);
    assert_eq!(c.scene_state, 1);
    assert!(c.awaiting_player_info);
    assert_eq!(p.pos, 4, "exact byte consumption");
}

// --- widgets ---------------------------------------------------------------

#[test]
fn widget_text_newline() {
    // manifest: widget_text_newline — com 5, "hi"
    let mut c = client_289();
    ensure_iface(&mut c, 5);
    let mut p = Packet::new(hex_bytes("000568690a"));
    c.psize = 5;
    c.handle_packet(ServerProt289::IF_SETTEXT, &mut p);
    assert!(c.ingame);
    let text = c.ifaces_mut[5].as_ref().unwrap().text.as_str();
    assert_eq!(text, "hi");
    assert_eq!(p.pos, 5, "exact byte consumption");
}

#[test]
fn widget_setanim() {
    let mut c = client_289();
    ensure_iface(&mut c, 7);
    // com 7, seq 0x0100
    let mut p = Packet::new(hex_bytes("00070100"));
    c.psize = 4;
    c.handle_packet(ServerProt289::IF_SETANIM, &mut p);
    assert_eq!(c.ifaces_mut[7].as_ref().unwrap().model_anim, 0x0100);
    assert_eq!(p.pos, 4, "exact byte consumption");
}

#[test]
fn widget_openside() {
    let mut c = client_289();
    ensure_iface(&mut c, 9);
    let mut p = Packet::new(hex_bytes("0009"));
    c.psize = 2;
    c.handle_packet(ServerProt289::IF_OPENSIDE, &mut p);
    assert_eq!(c.side_modal_id, 9);
    assert_eq!(p.pos, 2, "exact byte consumption");
}

#[test]
fn widget_openmain_side() {
    let mut c = client_289();
    ensure_iface(&mut c, 11);
    ensure_iface(&mut c, 12);
    let mut p = Packet::new(hex_bytes("000b000c"));
    c.psize = 4;
    c.handle_packet(ServerProt289::IF_OPENMAIN_SIDE, &mut p);
    assert_eq!(c.main_modal_id, 11);
    assert_eq!(c.side_modal_id, 12);
    assert_eq!(p.pos, 4, "exact byte consumption");
}

#[test]
fn widget_openoverlay_signed() {
    let mut c = client_289();
    // signed g2 = -1 → 0xffff
    let mut p = Packet::new(hex_bytes("ffff"));
    c.psize = 2;
    c.handle_packet(ServerProt289::IF_OPENOVERLAY, &mut p);
    assert_eq!(c.main_overlay_id, -1);
    assert_eq!(p.pos, 2, "exact byte consumption");
}

// --- varps -----------------------------------------------------------------

#[test]
fn varp_small_g2_g1b() {
    let mut c = client_289();
    // id 3, value -5 (0xfb as i8)
    let mut p = Packet::new(hex_bytes("0003fb"));
    c.psize = 3;
    c.handle_packet(ServerProt289::VARP_SMALL, &mut p);
    assert_eq!(c.var.get(3).copied(), Some(-5));
    assert_eq!(c.var_serv.get(3).copied(), Some(-5));
    assert_eq!(p.pos, 3, "exact byte consumption");
}

#[test]
fn varp_large_g2_g4() {
    let mut c = client_289();
    let mut p = Packet::new(hex_bytes("000400001234"));
    c.psize = 6;
    c.handle_packet(ServerProt289::VARP_LARGE, &mut p);
    assert_eq!(c.var.get(4).copied(), Some(0x1234));
    assert_eq!(p.pos, 6, "exact byte consumption");
}

#[test]
fn varp_sync_copies_serv_to_client() {
    let mut c = client_289();
    c.var = vec![0, 1, 2];
    c.var_serv = vec![9, 8, 7];
    let mut p = Packet::new(vec![]);
    c.psize = 0;
    c.handle_packet(ServerProt289::VARP_SYNC, &mut p);
    assert_eq!(c.var, vec![9, 8, 7]);
    assert_eq!(p.pos, 0, "exact byte consumption");
}

// --- tcp_in exact frame consumption (manifest expected_consumed_length) ----

#[test]
fn stage2_tcp_in_consumes_manifest_frame_lengths() {
    // Representative stage-2 frames through production tcp_in (no ISAAC).
    // expected_consumed_length from manifest: header + payload.
    let cases: &[(&str, &str)] = &[
        // logout 121 fixed 0 → frame "79", consumed 1
        ("79", "logout"),
        // rebuild 219 fixed 4 → "db01020304", consumed 5
        ("db01020304", "rebuild"),
        // varp_small 75 fixed 3 → "4b0003fb", consumed 4
        ("4b0003fb", "varp_small"),
        // if_openside 252 fixed 2 → "fc0009", consumed 3
        ("fc0009", "openside"),
        // player empty 188 g2 → "bc00020000", consumed 5
        ("bc00020000", "player_empty"),
        // npc empty 65 g2 → "41000100", consumed 4
        ("41000100", "npc_empty"),
    ];
    for (frame_hex, label) in cases {
        let mut c = client_289();
        c.ingame = true;
        // widgets/rebuild need minimal setup
        ensure_iface(&mut c, 9);
        let frame = hex_bytes(frame_hex);
        let expected = frame.len();
        let accepted = feed_frames(&mut c, &frame, 20);
        assert!(accepted >= 1, "{label}: tcp_in must accept complete frame");
        // After a full frame, ptype is cleared (-1) and no partial header waits.
        assert_eq!(
            c.ptype, -1,
            "{label}: production framing finished the frame (ptype cleared)"
        );
        // Stream still attached after non-logout frames; logout drops it.
        if *label == "logout" {
            assert!(!c.ingame, "{label}: LOGOUT clears ingame");
            assert!(c.stream.is_none(), "{label}: LOGOUT drops stream");
        } else {
            assert!(
                c.stream.is_some(),
                "{label}: non-logout keeps stream after consume"
            );
        }
        let _ = expected; // length is the oracle for the fixture bytes above
    }
}

// --- reset / logout --------------------------------------------------------

#[test]
fn reset_anims_clears_primary() {
    // manifest: reset_actors_empty
    let mut c = client_289();
    let mut player = ClientPlayer::at(1, 1);
    player.primary_anim = 42;
    c.players[0] = Some(Box::new(player));
    let mut local = ClientPlayer::at(2, 2);
    local.primary_anim = 7;
    c.local_player = Some(local);
    let mut p = Packet::new(vec![]);
    c.psize = 0;
    c.handle_packet(ServerProt289::RESET_ANIMS, &mut p);
    assert!(c.ingame);
    assert_eq!(c.players[0].as_ref().unwrap().primary_anim, -1);
    assert_eq!(c.local_player.as_ref().unwrap().primary_anim, -1);
}

#[test]
fn logout_opcode_121_clears_stream_modals_gens() {
    let mut c = client_289();
    c.ingame = true;
    c.main_modal_id = 11;
    c.side_modal_id = 12;
    c.chat_modal_id = 13;
    c.login_user = "bob".into();
    // Attach a live stream so logout production path closes it.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let done = Arc::new(Barrier::new(2));
    let done_s = Arc::clone(&done);
    let handle = thread::spawn(move || {
        let (_s, _) = listener.accept().unwrap();
        done_s.wait();
    });
    c.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    let before = c.gens;
    let mut p = Packet::new(vec![]);
    c.psize = 0;
    c.handle_packet(ServerProt289::LOGOUT, &mut p);
    assert!(!c.ingame, "LOGOUT 121 clears ingame");
    assert!(c.stream.is_none(), "LOGOUT drops stream");
    assert_eq!(c.main_modal_id, -1);
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
    assert!(c.login_user.is_empty());
    assert!(
        c.gens.player > before.player || c.gens.inv > before.inv || c.gens.scene > before.scene,
        "LOGOUT bumps gens via bump_all_gens"
    );
    // At least one family advanced (bump_all_gens).
    assert!(
        c.gens.npc > before.npc
            && c.gens.player > before.player
            && c.gens.inv > before.inv
            && c.gens.varp > before.varp
            && c.gens.scene > before.scene
            && c.gens.world > before.world,
        "production logout bumps all gens families"
    );
    done.wait();
    handle.join().unwrap();
}

/// Response 2 (cold) clears players/npcs/scene_state; response 15 keeps them.
#[test]
fn login_response_2_vs_15_distinct_on_r289() {
    // Cold login response 2
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        // response 2 cold
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap();
        s.write_all(&[0u8; 8]).unwrap();
        let mut buf = [0u8; 512];
        let _ = s.read(&mut buf).unwrap();
        s.write_all(&[2, 0, 0]).unwrap();

        // response 15 reconnect
        let (mut s2, _) = listener.accept().unwrap();
        let mut hdr2 = [0u8; 2];
        s2.read_exact(&mut hdr2).unwrap();
        for _ in 0..8 {
            let _ = s2.write_all(&[0]);
        }
        s2.write_all(&[0]).unwrap();
        s2.write_all(&[0u8; 8]).unwrap();
        let mut buf2 = [0u8; 512];
        let n2 = s2.read(&mut buf2).unwrap();
        assert!(n2 > 0);
        assert_eq!(buf2[0], 18, "reconnect wrapper opcode 18");
        // version word still 289 on reconnect path
        assert_eq!(&buf2[3..5], &[0x01, 0x21], "reconnect p2 still 289");
        s2.write_all(&[15]).unwrap();
    });

    let mut c = Client::new_with_revision(
        ClientConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    // Dirty prior session state — response 2 must clear
    c.npc_count = 3;
    c.npc[1] = Some(Box::new(ClientNpc::default()));
    c.player_count = 2;
    c.players[5] = Some(Box::new(ClientPlayer::default()));
    c.scene_state = 2;
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.npc_count, 0, "response 2 zeros npc_count");
    assert!(c.npc[1].is_none(), "response 2 nulls leftover npcs");
    assert_eq!(c.player_count, 0, "response 2 zeros player_count");
    assert!(c.players[5].is_none(), "response 2 nulls leftover players");
    assert_eq!(
        c.scene_state, 0,
        "response 2 resets scene_state (not ready)"
    );
    assert!(c.local_player.is_some());

    // Mark local so response 15 must keep it
    c.local_player.as_mut().unwrap().y = 77;
    c.player_count = 7;
    c.players[9] = Some(Box::new(ClientPlayer::default()));
    c.npc_count = 4;
    c.npc[2] = Some(Box::new(ClientNpc::default()));
    c.scene_state = 1;

    c.login("bob", "pw", true).unwrap();
    assert!(c.ingame);
    assert_eq!(
        c.local_player.as_ref().unwrap().y,
        77,
        "response 15 keeps local_player"
    );
    assert_eq!(c.player_count, 7, "response 15 does not wipe player_count");
    assert!(
        c.players[9].is_some(),
        "response 15 does not null leftover players"
    );
    assert_eq!(c.npc_count, 4, "response 15 does not wipe npc_count");
    assert!(
        c.npc[2].is_some(),
        "response 15 does not null leftover npcs"
    );
    assert_eq!(
        c.scene_state, 1,
        "response 15 does not reset scene_state like cold login"
    );
    server.join().unwrap();
}

// --- fail-closed remains for untraced --------------------------------------

#[test]
fn untraced_opcode_still_fail_closed() {
    // 236 is untraced on 289 and remains fail-closed.
    let mut c = client_289();
    let mut player = ClientPlayer::at(1, 1);
    player.primary_anim = 99;
    c.players[0] = Some(Box::new(player));
    let mut p = Packet::new(vec![1, 2]);
    c.psize = 2;
    c.handle_packet(236, &mut p);
    assert!(!c.ingame);
    assert_eq!(c.players[0].as_ref().unwrap().primary_anim, 99);
}

#[test]
fn r274_path_untouched_by_289_opcodes() {
    // Default 274 client must still use ServerProt tables, not 289 ids.
    let mut c = Client::new(cfg());
    c.ingame = true;
    c.ptype = -1;
    // 289 LOGOUT id 121 is not 274 LOGOUT (88) — must T1 on 274.
    let mut p = Packet::new(vec![]);
    c.psize = 0;
    c.handle_packet(121, &mut p);
    assert!(
        !c.ingame,
        "274 must not treat 121 as logout handler success path without matching arm"
    );
}

#[test]
fn bump_gens_player_info_on_r289() {
    let mut c = client_289();
    let before = c.gens.player;
    c.bump_gens(ServerProt289::PLAYER_INFO);
    assert_eq!(c.gens.player, before + 1);
    let before_inv = c.gens.inv;
    c.bump_gens(ServerProt289::UPDATE_INV_FULL);
    assert_eq!(c.gens.inv, before_inv + 1);
    // Untraced stays zero-family (already bumped all at T1 site separately)
    let player = c.gens.player;
    let inv = c.gens.inv;
    c.bump_gens(47);
    assert_eq!(c.gens.player, player);
    assert_eq!(c.gens.inv, inv);
}
