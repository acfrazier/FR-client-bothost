//! Revision 289 stage-1: production framing + inventory through Client paths.
//!
//! Fixtures come from `crates/client/tests/fixtures/revision_289/manifest.json`
//! (independently derived oracles). Cases outside stage-1 (actors, login RSA,
//! region/widgets) are not asserted here.

use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision};
use client::config::{IfType, IfTypeMut};
use client::io::{ClientStream, Packet, ServerProt, ServerProt289, SERVER_PROT_SIZES};
use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Barrier};
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

fn client_274() -> Client {
    let mut c = Client::new(cfg());
    c.ingame = true;
    c.ptype = -1;
    c
}

fn client_289() -> Client {
    let mut c = Client::new_with_revision(cfg(), ClientRevision::R289);
    c.ingame = true;
    c.ptype = -1;
    c
}

#[test]
fn last_login_info_decodes_fields_and_selects_welcome_interface() {
    let mut c = client_289();
    let mut welcome = IfType::default();
    welcome.id = 99;
    welcome.layer_id = 42;
    welcome.client_code = 650;
    c.ifaces = Arc::new(vec![Some(Box::new(welcome))]);
    c.side_modal_id = 7;
    c.report_abuse_input = "private".into();
    c.report_abuse_mute_option = true;

    let mut p = Packet::new(vec![1, 2, 3, 4, 0, 9, 201, 0, 10, 0]);
    c.handle_packet(ServerProt289::LAST_LOGIN_INFO, &mut p);

    assert_eq!(c.last_login_ip, 0x01020304);
    assert_eq!(c.days_since_login, 9);
    assert_eq!(c.days_since_recovery_change, 201);
    assert_eq!(c.last_login_message_count, 10);
    assert_eq!(c.members_warning, 0);
    assert_eq!(c.welcome_interface_id, 42);
    assert_eq!(c.main_modal_id, 42);
    assert_eq!(c.side_modal_id, -1);
    assert!(c.report_abuse_input.is_empty());
    assert!(!c.report_abuse_mute_option);
}

#[test]
fn last_login_info_selects_members_warning_welcome_655_and_layer() {
    let mut c = client_289();
    let mut welcome = IfType::default();
    welcome.id = 99;
    welcome.layer_id = 142;
    welcome.client_code = 655;
    c.ifaces = Arc::new(vec![Some(Box::new(welcome))]);
    let mut p = Packet::new(vec![1, 0, 0, 1, 0, 0, 201, 0, 0, 1]);
    let before = c.gens.iface;
    c.handle_packet(ServerProt289::LAST_LOGIN_INFO, &mut p);
    assert_eq!(c.welcome_interface_id, 142);
    assert_eq!(c.main_modal_id, 142);
    assert_eq!(c.gens.iface, before + 1);
}

#[test]
fn last_login_info_does_not_clear_report_state_when_welcome_gate_is_closed() {
    let mut c = client_289();
    c.main_modal_id = 77;
    c.report_abuse_input = "private".into();
    c.report_abuse_mute_option = true;
    let mut p = Packet::new(vec![1, 2, 3, 4, 0, 0, 201, 0, 0, 0]);
    c.handle_packet(ServerProt289::LAST_LOGIN_INFO, &mut p);
    assert_eq!(c.main_modal_id, 77);
    assert_eq!(c.report_abuse_input, "private");
    assert!(c.report_abuse_mute_option);
}

#[test]
fn welcome_close_without_component_publishes_iface_once() {
    let mut c = client_289();
    c.ifaces = Arc::new(vec![]);
    c.side_modal_id = 7;
    c.chat_modal_id = 8;
    c.report_abuse_input = "retain-on-failure".into();
    let before = c.gens.iface;
    let frame = hex_bytes("fd010203040009c9000a00");
    assert_eq!(feed_frames(&mut c, &frame, 1), 1);
    assert!(c.ingame);
    assert_eq!(
        (c.side_modal_id, c.chat_modal_id, c.main_modal_id),
        (-1, -1, -1)
    );
    assert_eq!(c.welcome_interface_id, -1);
    assert_eq!(c.gens.iface, before + 1);
}

#[test]
fn welcome_method110_emits_close_preserves_count_dialog_and_pause_without_modals() {
    // Primary J:2282-2301 differs from inbound IF_CLOSE: emits 93, does not
    // close the count dialog, and only clears resumed pause with a side/chat modal.
    let mut c = client_289();
    c.ifaces = Arc::new(vec![]);
    c.dialog_input_open = true;
    c.resumed_pause_button = true;
    c.out.pos = 0;
    assert_eq!(
        feed_frames(&mut c, &hex_bytes("fd010203040009c9000a00"), 1),
        1
    );
    assert_eq!(&c.out.data()[..c.out.pos], &[93]);
    assert!(c.dialog_input_open);
    assert!(c.resumed_pause_button);
}

#[test]
fn overlong_last_login_rejected_before_report_mutation() {
    let mut c = client_289();
    c.report_abuse_input = "retain-on-failure".into();
    let mut p = Packet::new(hex_bytes("010203040009c9000a0000"));
    p.set_frame_end(11);
    c.psize = 11;
    c.handle_packet(ServerProt289::LAST_LOGIN_INFO, &mut p);
    assert!(!c.ingame);
    assert_eq!(p.pos, 0, "fixed admission precedes all field reads");
    assert_eq!(c.report_abuse_input, "retain-on-failure");
}

fn ensure_inv_slots(c: &mut Client, com_id: usize, n: usize) {
    // Grow ifaces_mut and install inv arrays so inventory writes land.
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        if slots.len() <= com_id {
            slots.resize_with(com_id + 1, || None);
        }
        let mut m = IfTypeMut::default();
        m.link_obj_type = Some(vec![0; n]);
        m.link_obj_number = Some(vec![0; n]);
        slots[com_id] = Some(Arc::new(m));
    }
    // Shared decode table entry so iface_mut lookups stay consistent.
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

fn inv_slot(c: &Client, com_id: usize, slot: usize) -> (i32, i32) {
    let m = c.ifaces_mut[com_id].as_ref().unwrap();
    (
        m.link_obj_type.as_ref().unwrap()[slot],
        m.link_obj_number.as_ref().unwrap()[slot],
    )
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    let h = hex.trim();
    assert!(h.len() % 2 == 0, "odd hex length");
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Feed raw (no-ISAAC) game bytes into the production `ClientStream` + `tcp_in`
/// path. Returns how many complete frames `tcp_in` accepted.
fn feed_frames(c: &mut Client, frame: &[u8], max_polls: usize) -> usize {
    feed_chunks(c, &[frame.to_vec()], max_polls)
}

/// Write socket bytes in successive chunks with a client poll between each
/// chunk so incomplete headers/payloads exercise production `read_packet`
/// pending paths (not only one-shot full-frame delivery).
fn feed_chunks(c: &mut Client, chunks: &[Vec<u8>], polls_per_chunk: usize) -> usize {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    c.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    let (mut sock, _) = listener.accept().unwrap();
    sock.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
    c.random_in = None;
    c.ptype = -1;
    let mut accepted = 0usize;
    let mut written = 0;
    for chunk in chunks {
        sock.write_all(chunk).unwrap();
        written += chunk.len();
        // The next chunk cannot be written until the prescribed parser polls
        // finish. Count unread prior bytes, not a complete logical frame.
        wait_received(c.stream.as_mut().expect("stream before chunk"), written);
        for _ in 0..polls_per_chunk {
            if c.tcp_in() {
                accepted += 1;
            }
        }
    }
    accepted
}

fn wait_received(stream: &mut ClientStream, written: usize) {
    let unread = written - stream.bytes_in() as usize;
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while (stream.available().unwrap() as usize) < unread {
        assert!(std::time::Instant::now() < deadline, "chunk not received");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn transport_readiness_counts_unread_bytes_before_fragment_poll() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut stream = ClientStream::connect("127.0.0.1", addr.port()).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    server.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
    server.write_all(&[1, 2]).unwrap();
    wait_received(&mut stream, 2);
    assert_eq!(stream.read().unwrap(), 1);
    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        server.write_all(&[3]).unwrap();
    });
    wait_received(&mut stream, 3);
    assert_eq!(stream.available().unwrap(), 2);
    assert_eq!(stream.read().unwrap(), 2);
    assert_eq!(stream.read().unwrap(), 3);
    writer.join().unwrap();
    let mut c = client_289();
    assert_eq!(feed_chunks(&mut c, &[vec![13], vec![1], vec![2, 1]], 1), 1);
    assert_eq!((c.chat_public_mode, c.chat_private_mode, c.chat_trade_mode), (1, 2, 1));
}

#[test]
fn default_client_stays_on_274_public_tables() {
    let c = Client::new(cfg());
    assert_eq!(c.revision(), ClientRevision::R274);
    assert_eq!(ServerProt::UPDATE_INV_FULL, 106);
    assert_eq!(ServerProt::UPDATE_INV_PARTIAL, 172);
    assert_eq!(ServerProt::LOGOUT, 88);
    assert_eq!(SERVER_PROT_SIZES[106], -2);
    assert_eq!(SERVER_PROT_SIZES[172], -2);
    assert_eq!(SERVER_PROT_SIZES[88], 0);
    // 289 named IDs must not overwrite 274 constants.
    assert_ne!(ServerProt::UPDATE_INV_FULL, ServerProt289::UPDATE_INV_FULL);
    assert_ne!(
        ServerProt::UPDATE_INV_PARTIAL,
        ServerProt289::UPDATE_INV_PARTIAL
    );
}

#[test]
fn inventory_full_g2_count_289_production_dispatch() {
    // manifest: inventory_full_g2_count
    // payload 00030002000102000303 → component 3, count 2, (1,2), (3,3)
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    let payload = hex_bytes("00030002000102000303");
    let mut p = Packet::new(payload);
    c.psize = 10;
    let before = c.gens.inv;
    c.handle_packet(ServerProt289::UPDATE_INV_FULL, &mut p);
    assert_eq!(p.pos, 10, "exact payload cursor consumption");
    assert_eq!(c.gens.inv, before + 1);
    assert_eq!(inv_slot(&c, 3, 0), (1, 2));
    assert_eq!(inv_slot(&c, 3, 1), (3, 3));
    assert_eq!(inv_slot(&c, 3, 2), (0, 0), "zero-fill remaining entries");
    assert_eq!(inv_slot(&c, 3, 3), (0, 0));
    assert_eq!(c.ptype, -1);
}

#[test]
fn inventory_partial_gsmart_slot_289_production_dispatch() {
    // manifest: inventory_partial_gsmart_slot
    // payload 000301000100 — component 3, gsmart slot 1, item 1, count 0
    // declared length 6; trailing frame byte outside payload is not part of this vector
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    // Seed slot 1 so the clear is observable.
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[1] = 99;
        m.link_obj_number.as_mut().unwrap()[1] = 5;
    }
    let payload = hex_bytes("000301000100");
    let mut p = Packet::new(payload);
    c.psize = 6;
    c.handle_packet(ServerProt289::UPDATE_INV_PARTIAL, &mut p);
    assert_eq!(p.pos, 6, "exact payload cursor consumption");
    assert_eq!(inv_slot(&c, 3, 1), (1, 0));
    assert_eq!(inv_slot(&c, 3, 0), (0, 0));
}

#[test]
fn inventory_full_274_unchanged_g1_count() {
    // 274 path still uses g1 entry count (not g2).
    let mut c = client_274();
    ensure_inv_slots(&mut c, 3, 4);
    let mut p = Packet::alloc(0);
    p.p2(3); // component
    p.p1(2); // g1 count
    p.p2(1);
    p.p1(2);
    p.p2(3);
    p.p1(3);
    let end = p.pos;
    p.pos = 0;
    c.psize = end as i32;
    c.handle_packet(ServerProt::UPDATE_INV_FULL, &mut p);
    assert_eq!(p.pos, end);
    assert_eq!(inv_slot(&c, 3, 0), (1, 2));
    assert_eq!(inv_slot(&c, 3, 1), (3, 3));
}

#[test]
fn inventory_partial_274_unchanged_g1_slot() {
    let mut c = client_274();
    ensure_inv_slots(&mut c, 3, 4);
    let mut p = Packet::alloc(0);
    p.p2(3);
    p.p1(1); // g1 slot
    p.p2(1);
    p.p1(0);
    let end = p.pos;
    p.pos = 0;
    c.psize = end as i32;
    c.handle_packet(ServerProt::UPDATE_INV_PARTIAL, &mut p);
    assert_eq!(p.pos, end);
    assert_eq!(inv_slot(&c, 3, 1), (1, 0));
}

#[test]
fn framing_variable_g2_pending_no_dispatch() {
    // manifest: variable_g2_frame_pending — opcode 47, g2 len 5, no payload yet
    let mut c = client_289();
    let frame = hex_bytes("2f0005");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 0, "incomplete payload must not dispatch");
    // Header consumed into partial-frame state: ptype set, psize declared.
    assert_eq!(c.ptype, 47);
    assert_eq!(c.psize, 5);
}

#[test]
fn framing_variable_g2_complete_fail_closed_not_reset_anims() {
    // manifest: variable_g2_frame_complete — opcode 236, g2 len 2, payload 0102.
    // Untraced R289 ids must T1 fail-closed (no anim clear).
    let mut c = client_289();
    c.ingame = true;
    let mut player = ClientPlayer::at(1, 1);
    player.primary_anim = 42;
    c.players[0] = Some(Box::new(player));
    let frame = hex_bytes("ec00020102");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 1, "complete frame must pass read_packet");
    assert!(!c.ingame, "unknown/colliding R289 id must T1 logout");
    assert_eq!(
        c.players[0].as_ref().map(|p| p.primary_anim),
        Some(42),
        "must not run 274 RESET_ANIMS on R289 opcode 47"
    );
}

#[test]
fn framing_fixed_logout_opcode_121_header() {
    // manifest: fixed_logout_opcode_121 — length 0 fixed. Stage-2 wires method104
    // logout; framing still delivers one empty payload frame.
    let mut c = client_289();
    c.ingame = true;
    let frame = hex_bytes("79");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 1);
    assert!(!c.ingame, "LOGOUT 121 must call logout");
}

#[test]
fn framing_inventory_full_through_tcp_in() {
    // Full production path: frame header + payload → inventory state.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    let frame = hex_bytes("6b000a00030002000102000303");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 1);
    assert_eq!(inv_slot(&c, 3, 0), (1, 2));
    assert_eq!(inv_slot(&c, 3, 1), (3, 3));
    assert_eq!(c.ptype, -1);
}

#[test]
fn framing_inventory_partial_through_tcp_in_ignores_trailing_outside_declared() {
    // frame_hex includes a trailing 00 outside the declared 6-byte payload.
    // Inventory must consume only the declared frame; the trailer is a separate
    // socket byte (next opcode), not part of the inv decode.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[1] = 99;
        m.link_obj_number.as_mut().unwrap()[1] = 5;
    }
    let frame = hex_bytes("4c000600030100010000");
    // Stop after the first successful frame so the trailing 00 is not required
    // to form a second complete packet for this assertion.
    let accepted = feed_frames(&mut c, &frame, 1);
    assert_eq!(accepted, 1);
    assert_eq!(inv_slot(&c, 3, 1), (1, 0));
    assert_eq!(inv_slot(&c, 3, 0), (0, 0));
    assert_eq!(c.ptype, -1);
}

#[test]
fn map_projanim_107_still_zone_on_274_not_inventory() {
    // Regression: 274 opcode 107 remains MAP_PROJANIM zone path, not inv.
    let mut c = client_274();
    ensure_inv_slots(&mut c, 0, 4);
    // Minimal zone MAP_PROJANIM body is large; just ensure dispatch does not
    // write inventory arrays when handed empty-ish buffer via zone_packet panic
    // path is caught — instead call bump/match by ensuring handle_packet with
    // a tiny buffer logs T2 rather than filling inv.
    let before = inv_slot(&c, 0, 0);
    let mut p = Packet::new(vec![0u8; 15]);
    c.psize = 15;
    // Direct zone path: top-level MAP_PROJANIM on 274.
    c.handle_packet(ServerProt::MAP_PROJANIM, &mut p);
    assert_eq!(
        inv_slot(&c, 0, 0),
        before,
        "274 MAP_PROJANIM must not touch inv"
    );
}

#[test]
fn truncated_inventory_full_does_not_partially_publish() {
    // Declared g2 count=2 but only one slot present → OOB → T2 logout, no half write.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[0] = 42;
        m.link_obj_number.as_mut().unwrap()[0] = 7;
    }
    // component 3, count 2, only one item pair
    let mut p = Packet::new(hex_bytes("00030002000102"));
    c.psize = 7;
    c.ingame = true;
    c.handle_packet(ServerProt289::UPDATE_INV_FULL, &mut p);
    // catch_unwind → T2 logout; inventory must stay pre-update values.
    assert!(!c.ingame);
    assert_eq!(
        inv_slot(&c, 3, 0),
        (42, 7),
        "no partial publication on truncated frame"
    );
}

#[test]
fn production_buffer_truncated_inv_full_ignores_stale_bytes_past_psize() {
    // Production r#in is Packet::alloc(1) → 5000 bytes. Declared psize bounds
    // the frame; stale bytes past psize must not be read as extra slots.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[0] = 42;
        m.link_obj_number.as_mut().unwrap()[0] = 7;
        m.link_obj_type.as_mut().unwrap()[1] = 11;
        m.link_obj_number.as_mut().unwrap()[1] = 3;
    }
    let mut p = Packet::alloc(1);
    assert_eq!(p.length(), 5000);
    // Valid header for count=2 but only first slot inside psize=7; rest of
    // buffer filled with bytes that would decode as a second slot if read.
    let head = hex_bytes("00030002000102");
    p.data_mut()[..head.len()].copy_from_slice(&head);
    for b in p.data_mut()[head.len()..].iter_mut() {
        *b = 0x01; // would look like id/count material past psize
    }
    p.pos = 0;
    c.psize = 7;
    c.ingame = true;
    c.handle_packet(ServerProt289::UPDATE_INV_FULL, &mut p);
    assert!(!c.ingame, "truncated declared frame → T2 logout");
    assert_eq!(inv_slot(&c, 3, 0), (42, 7), "no publish from stale tail");
    assert_eq!(inv_slot(&c, 3, 1), (11, 3), "second slot unchanged");
}

#[test]
fn production_buffer_truncated_inv_partial_no_partial_publication() {
    // Two partial entries; second entry truncated inside psize on a 5000-byte
    // production buffer. First entry must not land before whole-frame validate.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[0] = 5;
        m.link_obj_number.as_mut().unwrap()[0] = 9;
        m.link_obj_type.as_mut().unwrap()[1] = 6;
        m.link_obj_number.as_mut().unwrap()[1] = 8;
    }
    let mut p = Packet::alloc(1);
    // com=3, entry0: slot1 id1 count0 (complete), entry1: slot0 then truncated
    // bytes: 00 03 | 01 00 01 00 | 00 00 02  (missing count byte for 2nd)
    let head = hex_bytes("000301000100000002");
    p.data_mut()[..head.len()].copy_from_slice(&head);
    for b in p.data_mut()[head.len()..].iter_mut() {
        *b = 0xff;
    }
    p.pos = 0;
    c.psize = head.len() as i32; // ends mid second entry
    c.ingame = true;
    c.handle_packet(ServerProt289::UPDATE_INV_PARTIAL, &mut p);
    assert!(!c.ingame);
    assert_eq!(
        inv_slot(&c, 3, 0),
        (5, 9),
        "no partial commit of earlier entry"
    );
    assert_eq!(inv_slot(&c, 3, 1), (6, 8), "slot1 stays pre-update");
}

#[test]
fn r289_opcode_219_rebuild_not_obj_reveal() {
    // 219 = 274 OBJ_REVEAL and 289 REBUILD_NORMAL. Stage-2 runs rebuild only.
    let mut c = client_289();
    c.ingame = true;
    c.scene_state = 2;
    c.map_build_centre_zone_x = 50;
    c.map_build_centre_zone_z = 50;
    let mut p = Packet::new(vec![0, 1, 0, 2]); // zone 1 / 2
    c.psize = 4;
    c.handle_packet(219, &mut p);
    assert!(c.ingame, "REBUILD must not T1 logout");
    assert_eq!(c.scene_state, 1, "must enter REBUILD_NORMAL path");
    assert_eq!(c.map_build_centre_zone_x, 1);
    assert_eq!(c.map_build_centre_zone_z, 2);
}

#[test]
fn r289_untraced_opcode_fail_closed_not_reset_anims_direct() {
    let mut c = client_289();
    c.ingame = true;
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
fn fragmented_stream_header_then_payload_completes_once() {
    // Opcode + g2 length header first, payload later across tcp_in polls.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    // 107 / 0x6b, g2 len 10, payload inventory_full_g2_count
    let header = hex_bytes("6b000a");
    let payload = hex_bytes("00030002000102000303");
    let accepted = feed_chunks(&mut c, &[header, payload], 6);
    assert_eq!(accepted, 1, "fragmented complete frame dispatches once");
    assert_eq!(inv_slot(&c, 3, 0), (1, 2));
    assert_eq!(inv_slot(&c, 3, 1), (3, 3));
    assert_eq!(c.ptype, -1);
}

#[test]
fn fragmented_stream_partial_payload_stays_pending() {
    let mut c = client_289();
    // header wants 5 payload bytes; only 2 arrive
    let header = hex_bytes("2f0005");
    let partial = hex_bytes("0102");
    let accepted = feed_chunks(&mut c, &[header, partial], 6);
    assert_eq!(accepted, 0, "incomplete payload must not dispatch");
    assert_eq!(c.ptype, 47);
    assert_eq!(c.psize, 5);
    assert!(c.ingame, "pending incomplete frame must not logout");
}

#[test]
fn adopt_from_carries_revision_with_stream_baton() {
    let mut head = client_289();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let b2 = Arc::clone(&barrier);
    let t = thread::spawn(move || {
        let (_sock, _) = listener.accept().unwrap();
        b2.wait();
    });
    head.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    head.ptype = 47;
    head.psize = 5;

    let mut sim = client_274();
    assert_eq!(sim.revision(), ClientRevision::R274);
    assert!(sim.adopt_from(&mut head).is_some());
    assert_eq!(
        sim.revision(),
        ClientRevision::R289,
        "adopt_from must carry session revision with the socket"
    );
    assert_eq!(sim.ptype, 47);
    assert_eq!(sim.psize, 5);
    assert!(sim.stream.is_some());
    assert!(head.stream.is_none());
    barrier.wait();
    t.join().unwrap();
}

#[test]
fn overlong_289_inv_full_exact_end_required_zero_publication() {
    // Declared psize includes two pad bytes after a complete count=1 body.
    // R289 must reject before publishing (exact consumed-length contract).
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[0] = 42;
        m.link_obj_number.as_mut().unwrap()[0] = 7;
    }
    // com=3, count=1, (id=1,count=2), pad 00 00 → 9 declared bytes
    let mut p = Packet::new(hex_bytes("000300010001020000"));
    c.psize = 9;
    c.ingame = true;
    c.handle_packet(ServerProt289::UPDATE_INV_FULL, &mut p);
    assert!(!c.ingame, "overlong R289 full inv → T2 logout");
    assert_eq!(inv_slot(&c, 3, 0), (42, 7), "zero inventory publication");
}

#[test]
fn incomplete_289_inv_full_zero_publication_production_stream() {
    // Production tcp_in path: outer g2 length matches delivered bytes, but
    // g2 inventory count claims 2 slots while only one is present → T2, no publish.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[0] = 42;
        m.link_obj_number.as_mut().unwrap()[0] = 7;
        m.link_obj_type.as_mut().unwrap()[1] = 11;
        m.link_obj_number.as_mut().unwrap()[1] = 3;
    }
    // opcode 107, g2 len 7, body: com=3 count=2 + only first slot
    let frame = hex_bytes("6b000700030002000102");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 1, "complete socket frame still dispatches");
    assert!(!c.ingame, "truncated inv full → T2 logout");
    assert_eq!(inv_slot(&c, 3, 0), (42, 7));
    assert_eq!(inv_slot(&c, 3, 1), (11, 3));
}

#[test]
fn overlong_289_inv_full_zero_publication_production_stream() {
    // opcode 107, g2 len 9, exact one-slot body + 2 pad bytes inside psize.
    let mut c = client_289();
    ensure_inv_slots(&mut c, 3, 4);
    {
        let slots = Arc::make_mut(&mut c.ifaces_mut);
        let m = Arc::make_mut(slots[3].as_mut().unwrap());
        m.link_obj_type.as_mut().unwrap()[0] = 42;
        m.link_obj_number.as_mut().unwrap()[0] = 7;
    }
    let frame = hex_bytes("6b0009000300010001020000");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 1);
    assert!(!c.ingame);
    assert_eq!(
        inv_slot(&c, 3, 0),
        (42, 7),
        "pad inside psize must not publish"
    );
}

#[test]
fn inv_full_274_still_allows_trailing_pad_inside_psize() {
    // Intentional 274 behavior: leftover pad inside declared frame still publishes.
    let mut c = client_274();
    ensure_inv_slots(&mut c, 3, 4);
    let mut p = Packet::alloc(0);
    p.p2(3);
    p.p1(1); // g1 count
    p.p2(1);
    p.p1(2);
    p.p1(0xff); // pad
    let end = p.pos;
    p.pos = 0;
    c.psize = end as i32;
    c.handle_packet(ServerProt::UPDATE_INV_FULL, &mut p);
    assert!(c.ingame);
    assert_eq!(inv_slot(&c, 3, 0), (1, 2));
}

#[test]
fn adopt_from_failed_leaves_target_entirely_unchanged() {
    // Target is live R289 with partial-frame state; source has no stream and
    // a different revision. Failed adopt must not touch target at all.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let b2 = Arc::clone(&barrier);
    let t = thread::spawn(move || {
        let (_sock, _) = listener.accept().unwrap();
        b2.wait();
    });

    let mut target = client_289();
    target.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    target.ptype = 47;
    target.psize = 5;
    target.ptype0 = 1;
    target.ptype1 = 2;
    target.ptype2 = 3;
    target.baton = false;
    target.out.p1(0xaa);
    let out_pos = target.out.pos;
    let in_len = target.r#in.length();

    let mut source = client_274();
    assert!(source.stream.is_none());
    assert_eq!(source.revision(), ClientRevision::R274);

    assert!(target.adopt_from(&mut source).is_none());
    assert_eq!(target.revision(), ClientRevision::R289);
    assert!(target.stream.is_some(), "target live stream kept");
    assert_eq!(target.ptype, 47);
    assert_eq!(target.psize, 5);
    assert_eq!(target.ptype0, 1);
    assert_eq!(target.ptype1, 2);
    assert_eq!(target.ptype2, 3);
    assert!(!target.baton);
    assert_eq!(target.out.pos, out_pos);
    assert_eq!(target.r#in.length(), in_len);
    assert_eq!(source.revision(), ClientRevision::R274);
    assert!(source.stream.is_none());

    barrier.wait();
    t.join().unwrap();
}

#[test]
fn adopt_from_failed_across_revisions_does_not_assign_source_revision() {
    // Target R274 (default), source R289 without stream — pre-fix bug assigned
    // revision before take() and left target mutated on None.
    let mut target = client_274();
    target.ptype = 88;
    target.psize = 0;
    let mut source = client_289();
    assert!(source.stream.is_none());
    assert!(target.adopt_from(&mut source).is_none());
    assert_eq!(
        target.revision(),
        ClientRevision::R274,
        "failed adopt must not copy source revision"
    );
    assert_eq!(target.ptype, 88);
    assert_eq!(target.psize, 0);
    assert!(!target.baton);
}

#[test]
fn revision_bound_at_construction_read_only_getter() {
    // Callsite API: revision is construction-bound + getter; no public field set.
    let c = Client::new(cfg());
    assert_eq!(c.revision(), ClientRevision::R274);
    assert!(c.revision().is_274());

    let c289 = Client::new_with_revision(cfg(), ClientRevision::R289);
    assert_eq!(c289.revision(), ClientRevision::R289);
    assert!(c289.revision().is_289());
    assert_eq!(c289.revision().as_i32(), 289);

    let shared = Client::from_shared(
        cfg(),
        Arc::new(client::config::Cache::default()),
        Arc::new(vec![]),
        vec![],
    );
    assert_eq!(shared.revision(), ClientRevision::R274);

    let shared289 = Client::from_shared_with_revision(
        cfg(),
        Arc::new(client::config::Cache::default()),
        Arc::new(vec![]),
        vec![],
        ClientRevision::R289,
    );
    assert_eq!(shared289.revision(), ClientRevision::R289);
}

#[test]
fn declared_frame_larger_than_storage_is_not_shortened_to_storage() {
    let mut c = client_289();
    c.psize = 4;
    let mut p = Packet::new(vec![0, 1, 1]);
    c.self_slot = 321;
    c.handle_packet(ServerProt289::UPDATE_PID, &mut p);
    assert!(!c.ingame);
    assert_eq!(p.pos, 0);
    assert_eq!(c.self_slot, 321);
}

#[test]
fn unterminated_option_does_not_consume_previous_frames_newline() {
    let mut c = client_289();
    let chunks = [hex_bytes("1509020041747461636b0a"), hex_bytes("1503020058")];
    let before = generations(&c);
    assert_eq!(feed_chunks(&mut c, &chunks, 6), 2);
    assert!(!c.ingame);
    assert_eq!(c.player_op[1].as_deref(), Some("Attack"));
    assert_eq!(c.r#in.frame_end(), Some(3));
    assert_eq!(c.r#in.pos, 3);
    assert_eq!(generations(&c), before.map(|g| g + 1));
}

#[test]
fn fixed_empty_sync_after_welcome_and_unknown_frame_reset_once() {
    let mut c = client_289();
    welcome_table(&mut c);
    c.var = vec![0];
    c.var_serv = vec![7];
    let before = generations(&c);
    let frame = hex_bytes("fd010203040009c9000a00ac");
    assert_eq!(feed_frames(&mut c, &frame, 6), 2);
    assert!(c.ingame);
    assert_eq!(c.r#in.frame_end(), Some(0));
    assert_eq!(c.var[0], 7);
    let mut expected = before;
    expected[3] += 1;
    expected[7] += 1;
    assert_eq!(generations(&c), expected);
    // ID122 has a fixed table size but no primary operation; never route it
    // through a colliding 274 handler. T1 is a reset outcome.
    assert_eq!(feed_frames(&mut c, &hex_bytes("7a000000000000"), 1), 1);
    assert!(!c.ingame);
    assert_eq!(generations(&c), expected.map(|g| g + 1));
}

fn generations(c: &Client) -> [u64; 11] {
    let g = &c.gens;
    [
        g.npc, g.player, g.inv, g.varp, g.stat, g.chat, g.scene, g.iface, g.camera, g.map_flag,
        g.world,
    ]
}

fn welcome_table(c: &mut Client) {
    let mut normal = IfType::default();
    normal.id = 99;
    normal.layer_id = 42;
    normal.client_code = 650;
    let mut warning = IfType::default();
    warning.id = 199;
    warning.layer_id = 142;
    warning.client_code = 655;
    c.ifaces = Arc::new(vec![Some(Box::new(normal)), Some(Box::new(warning))]);
}

#[test]
fn welcome_truth_table_through_socket() {
    // Independently specified from J:3165-3177, not the Rust branch logic.
    for (recovery, members, layer) in [
        (200, 0, 142),
        (200, 1, 142),
        (200, 2, 142),
        (201, 0, 42),
        (201, 1, 142),
        (201, 2, 42),
    ] {
        let mut c = client_289();
        welcome_table(&mut c);
        c.side_modal_id = 7;
        c.chat_modal_id = 8;
        c.resumed_pause_button = true;
        c.redraw_side = false;
        c.redraw_chat = false;
        c.redraw_icons = false;
        let before = generations(&c);
        let frame = [253, 1, 2, 3, 4, 0, 9, recovery, 0, 10, members];
        assert_eq!(feed_frames(&mut c, &frame, 1), 1);
        assert!(c.ingame);
        assert_eq!(c.r#in.pos, 10);
        assert_eq!(
            (
                c.last_login_ip,
                c.days_since_login,
                c.last_login_message_count
            ),
            (0x01020304, 9, 10)
        );
        assert_eq!(
            (c.days_since_recovery_change, c.members_warning),
            (recovery as i32, members as i32)
        );
        assert_eq!((c.welcome_interface_id, c.main_modal_id), (layer, layer));
        assert_eq!((c.side_modal_id, c.chat_modal_id), (-1, -1));
        assert!(!c.resumed_pause_button);
        assert!(c.redraw_side && c.redraw_icons && c.redraw_chat);
        let mut expected = before;
        expected[7] += 1;
        assert_eq!(generations(&c), expected);
        assert_eq!(&c.out.data()[..c.out.pos], &[93]);
    }
}

#[test]
fn welcome_closed_gates_have_no_ui_publication_or_emit_through_socket() {
    for (ip, modal) in [(0u8, -1), (1, 77)] {
        let mut c = client_289();
        welcome_table(&mut c);
        c.main_modal_id = modal;
        c.side_modal_id = 7;
        c.chat_modal_id = 8;
        c.dialog_input_open = true;
        c.report_abuse_input = "keep".into();
        c.report_abuse_mute_option = true;
        let before = generations(&c);
        let frame = [253, 0, 0, 0, ip, 0, 9, 200, 0, 10, 1];
        assert_eq!(feed_frames(&mut c, &frame, 1), 1);
        assert!(c.ingame);
        assert_eq!(
            (c.main_modal_id, c.side_modal_id, c.chat_modal_id),
            (modal, 7, 8)
        );
        assert_eq!(c.welcome_interface_id, -1);
        assert_eq!(c.report_abuse_input, "keep");
        assert!(c.report_abuse_mute_option && c.dialog_input_open);
        assert_eq!(generations(&c), before);
        assert_eq!(c.out.pos, 0);
    }
}

#[test]
fn all_fixed_sizes_reject_short_and_long_before_decode_on_production_dispatch() {
    // A socket fixed packet has no length prefix: a short payload is pending
    // and any extra byte is the next opcode. Inject declared bounds at the
    // production handle_packet boundary to exercise impossible-size admission.
    let mut c = client_289();
    let sizes = *c.revision().server_prot_sizes();
    for (id, size) in sizes.into_iter().enumerate().filter(|(_, size)| *size >= 0) {
        let lengths = if size == 0 {
            vec![1]
        } else {
            vec![size as usize - 1, size as usize + 1]
        };
        for end in lengths {
            c.ingame = true;
            c.self_slot = 321;
            c.members_account = 2;
            c.report_abuse_input = "not-applied".into();
            c.report_abuse_mute_option = true;
            let before = generations(&c);
            let mut p = Packet::alloc(1);
            p.data_mut().fill(0xff);
            p.set_frame_end(end);
            c.psize = end as i32;
            c.handle_packet(id as i32, &mut p);
            assert!(!c.ingame, "id={id}, end={end}");
            assert_eq!(p.pos, 0, "id={id}, admission before any decode");
            assert_eq!((c.self_slot, c.members_account), (321, 2));
            assert_eq!(c.report_abuse_input, "not-applied");
            assert!(c.report_abuse_mute_option);
            assert_eq!(generations(&c), before.map(|g| g + 1));
        }
    }
}

#[test]
fn last_login_every_short_bound_and_long_never_applies_before_reset() {
    let mut c = client_289();
    welcome_table(&mut c);
    for end in (0..10).chain([11]) {
        c.ingame = true;
        c.last_login_ip = 123;
        c.days_since_login = 8;
        c.last_login_dns_display = Some("synthetic.example".into());
        c.report_abuse_input = "not-applied".into();
        c.report_abuse_mute_option = true;
        let before = generations(&c);
        let out = c.out.pos;
        let mut p = Packet::alloc(1);
        p.data_mut()[..10].copy_from_slice(&hex_bytes("010203040009c9000a00"));
        p.set_frame_end(end);
        c.psize = end as i32;
        c.handle_packet(253, &mut p);
        assert!(!c.ingame);
        assert_eq!(p.pos, 0);
        assert_eq!(c.report_abuse_input, "not-applied");
        assert!(c.report_abuse_mute_option);
        assert_eq!(c.out.pos, out, "no welcome close emit on failure");
        assert_eq!(
            (
                c.last_login_ip,
                c.days_since_login,
                c.days_since_recovery_change,
                c.last_login_message_count,
                c.members_warning
            ),
            (0, 0, 0, 0, 0)
        );
        assert_eq!(c.welcome_interface_id, -1);
        assert!(c.last_login_dns_display.is_none());
        assert_eq!(generations(&c), before.map(|g| g + 1));
    }
}

#[test]
fn fragmented_last_login_and_adjacent_pid_publish_each_once() {
    let mut c = client_289();
    welcome_table(&mut c);
    let before = generations(&c);
    let chunks = [
        hex_bytes("fd010203"),
        hex_bytes("040009c9"),
        hex_bytes("000a0078012301"),
    ];
    assert_eq!(feed_chunks(&mut c, &chunks, 6), 2);
    assert!(c.ingame);
    assert_eq!(c.welcome_interface_id, 42);
    assert_eq!((c.self_slot, c.members_account), (0x0123, 1));
    let mut expected = before;
    expected[7] += 1;
    assert_eq!(generations(&c), expected);
    assert_eq!(&c.out.data()[..c.out.pos], &[93]);
    assert_eq!(c.r#in.frame_end(), Some(3));
}

#[test]
fn short_fixed_socket_payload_stays_pending_without_mutation() {
    let mut c = client_289();
    c.report_abuse_input = "pending".into();
    c.self_slot = 321;
    let before = generations(&c);
    assert_eq!(
        feed_chunks(&mut c, &[vec![253], hex_bytes("010203040009c9000a")], 6),
        0
    );
    assert!(c.ingame);
    assert_eq!((c.ptype, c.psize), (253, 10));
    assert_eq!(c.last_login_ip, 0);
    assert_eq!(c.report_abuse_input, "pending");
    assert_eq!(c.self_slot, 321);
    assert_eq!(generations(&c), before);
}

#[test]
fn variable_zero_after_valid_frame_cannot_read_reusable_tail() {
    for frame in ["6b0000", "c400"] {
        let mut c = client_289();
        welcome_table(&mut c);
        let before = generations(&c);
        let chunks = [hex_bytes("fd010203040009c9000a00"), hex_bytes(frame)];
        assert_eq!(feed_chunks(&mut c, &chunks, 6), 2);
        assert!(!c.ingame, "zero inventory/game-string frame is malformed");
        assert_eq!(c.r#in.frame_end(), Some(0));
        assert_eq!(c.r#in.pos, 0);
        let mut expected = before.map(|g| g + 1);
        expected[7] += 1; // previous successful welcome, not failed packet
        assert_eq!(generations(&c), expected);
    }
}

#[test]
fn variable_zero_ignorelist_is_valid_and_preserves_adjacent_pid() {
    let mut c = client_289();
    c.ignore_count = 1;
    let before = generations(&c);
    assert_eq!(
        feed_chunks(&mut c, &[hex_bytes("2f00"), hex_bytes("0078012301")], 6),
        2
    );
    assert!(c.ingame);
    assert_eq!(c.ignore_count, 0);
    assert_eq!((c.self_slot, c.members_account), (0x0123, 1));
    assert_eq!(generations(&c), before);
}

#[test]
fn welcome_then_logout_resets_fields_and_all_families_exactly_once() {
    let mut c = client_289();
    welcome_table(&mut c);
    assert_eq!(
        feed_frames(&mut c, &hex_bytes("fd010203040009c9000a00"), 1),
        1
    );
    c.last_login_dns_display = Some("synthetic.example".into());
    let before = generations(&c);
    assert_eq!(feed_frames(&mut c, &[121], 1), 1);
    assert!(!c.ingame);
    assert_eq!(c.revision(), ClientRevision::R289);
    assert_eq!(
        (
            c.last_login_ip,
            c.days_since_login,
            c.days_since_recovery_change,
            c.last_login_message_count,
            c.members_warning
        ),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(
        (
            c.welcome_interface_id,
            c.main_modal_id,
            c.side_modal_id,
            c.chat_modal_id
        ),
        (-1, -1, -1, -1)
    );
    assert!(c.last_login_dns_display.is_none());
    assert_eq!(generations(&c), before.map(|g| g + 1));
}

#[test]
fn in_band_actor_failure_has_reset_not_success_publication() {
    let mut c = client_289();
    // Valid empty actor prefix plus trailing byte triggers get_player_pos's
    // in-band size mismatch/logout, not a panic caught by handle_packet.
    let before = generations(&c);
    assert_eq!(feed_frames(&mut c, &hex_bytes("bc0003000000"), 1), 1);
    assert!(!c.ingame);
    assert_eq!(generations(&c), before.map(|g| g + 1));
}

#[test]
fn default_274_last_login_is_noop_and_adjacent_pid_keeps_legacy_meaning() {
    let mut c = client_274();
    c.side_modal_id = 7;
    c.report_abuse_input = "keep".into();
    c.report_abuse_mute_option = true;
    let before = generations(&c);
    let frame = [
        ServerProt::LAST_LOGIN_INFO as u8,
        1,
        2,
        3,
        4,
        0,
        9,
        200,
        0,
        10,
        1,
        ServerProt::UPDATE_PID as u8,
        1,
        35,
        1,
    ];
    assert_eq!(feed_frames(&mut c, &frame, 6), 2);
    assert!(c.ingame);
    assert_eq!(c.revision(), ClientRevision::R274);
    assert_eq!(c.last_login_ip, 0);
    assert_eq!(c.side_modal_id, 7);
    assert_eq!(c.report_abuse_input, "keep");
    assert!(c.report_abuse_mute_option);
    assert_eq!(c.out.pos, 0);
    assert_eq!((c.self_slot, c.members_account), (291, 1));
    assert_eq!(generations(&c), before);
}
