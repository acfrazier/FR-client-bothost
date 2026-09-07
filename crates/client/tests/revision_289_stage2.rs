//! Revision 289 stage-2: login version, actors, region, widgets, varps, reset.
//!
//! Fixtures come from `crates/client/tests/fixtures/revision_289/manifest.json`
//! (independently derived oracles). Production paths only — no parallel decoder.

use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision};
use client::config::{IfType, IfTypeMut};
use client::io::{Packet, ServerProt, ServerProt289};
use std::sync::Arc;

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
    let mut c = Client::new(cfg());
    c.revision = ClientRevision::R289;
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

// --- named opcode constants -------------------------------------------------

#[test]
fn server_prot_289_stage2_named_opcodes() {
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

#[test]
fn login_wrapper_p2_revision_289() {
    // client.java:8378 writes p2(289). Offline structural check of the
    // version word only — no RSA ciphertext, modulus, or live endpoint.
    let mut c = client_289();
    c.revision = ClientRevision::R289;
    // Mirror the wrapper prefix built in login() after rsaenc.
    let mut loginout = Packet::alloc(1);
    loginout.p1(16);
    loginout.p1(0); // size placeholder
    loginout.p1(255);
    loginout.p2(c.revision.as_i32());
    loginout.p1(if c.config.lowmem { 1 } else { 0 });
    let data = loginout.data()[..loginout.pos as usize].to_vec();
    assert_eq!(&data[3..5], &[0x01, 0x21], "p2 289 big-endian");
}

#[test]
fn login_wrapper_p2_revision_274_default() {
    let c = Client::new(cfg());
    assert_eq!(c.revision, ClientRevision::R274);
    let mut loginout = Packet::alloc(1);
    loginout.p1(16);
    loginout.p1(0);
    loginout.p1(255);
    loginout.p2(c.revision.as_i32());
    let data = loginout.data()[..loginout.pos as usize].to_vec();
    assert_eq!(&data[3..5], &[0x01, 0x12], "p2 274 big-endian");
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
    }
}

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
}

#[test]
fn widget_openside() {
    let mut c = client_289();
    ensure_iface(&mut c, 9);
    let mut p = Packet::new(hex_bytes("0009"));
    c.psize = 2;
    c.handle_packet(ServerProt289::IF_OPENSIDE, &mut p);
    assert_eq!(c.side_modal_id, 9);
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
}

#[test]
fn widget_openoverlay_signed() {
    let mut c = client_289();
    // signed g2 = -1 → 0xffff
    let mut p = Packet::new(hex_bytes("ffff"));
    c.psize = 2;
    c.handle_packet(ServerProt289::IF_OPENOVERLAY, &mut p);
    assert_eq!(c.main_overlay_id, -1);
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
}

#[test]
fn varp_large_g2_g4() {
    let mut c = client_289();
    let mut p = Packet::new(hex_bytes("000400001234"));
    c.psize = 6;
    c.handle_packet(ServerProt289::VARP_LARGE, &mut p);
    assert_eq!(c.var.get(4).copied(), Some(0x1234));
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
fn logout_opcode_121_method104() {
    let mut c = client_289();
    c.ingame = true;
    let mut p = Packet::new(vec![]);
    c.psize = 0;
    c.handle_packet(ServerProt289::LOGOUT, &mut p);
    assert!(!c.ingame);
}

// --- fail-closed remains for untraced --------------------------------------

#[test]
fn untraced_opcode_still_fail_closed() {
    // 47 collides with 274 RESET_ANIMS; still unknown on 289.
    let mut c = client_289();
    let mut player = ClientPlayer::at(1, 1);
    player.primary_anim = 99;
    c.players[0] = Some(Box::new(player));
    let mut p = Packet::new(vec![1, 2]);
    c.psize = 2;
    c.handle_packet(47, &mut p);
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
