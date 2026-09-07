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

fn client_274() -> Client {
    let mut c = Client::new(cfg());
    c.ingame = true;
    c.ptype = -1;
    c
}

fn client_289() -> Client {
    let mut c = client_274();
    c.revision = ClientRevision::R289;
    c
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
    let chunks = chunks.to_vec();
    let n_chunks = chunks.len();
    // phase: client waits for server to finish writing chunk i before polling.
    let written = Arc::new(Mutex::new(0usize));
    let written_s = Arc::clone(&written);
    let done = Arc::new(Barrier::new(2));
    let done_s = Arc::clone(&done);
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        for (i, chunk) in chunks.into_iter().enumerate() {
            sock.write_all(&chunk).unwrap();
            let _ = sock.flush();
            *written_s.lock().unwrap() = i + 1;
            // Let the client poll this chunk before the next write.
            thread::sleep(Duration::from_millis(30));
        }
        done_s.wait();
        let _ = sock.shutdown(std::net::Shutdown::Both);
    });

    c.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    c.random_in = None;
    c.ptype = -1;

    let mut accepted = 0usize;
    // Wait until first chunk is on the wire.
    for _ in 0..50 {
        if *written.lock().unwrap() >= 1 {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    for chunk_i in 0..n_chunks {
        // Wait until this chunk has been written.
        for _ in 0..50 {
            if *written.lock().unwrap() > chunk_i {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        for _ in 0..polls_per_chunk {
            if c.tcp_in() {
                accepted += 1;
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
    done.wait();
    handle.join().unwrap();
    accepted
}

#[test]
fn default_client_stays_on_274_public_tables() {
    let c = Client::new(cfg());
    assert_eq!(c.revision, ClientRevision::R274);
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
    // manifest: variable_g2_frame_complete — opcode 47, g2 len 2, payload 0102.
    // Numeric 47 collides with 274 RESET_ANIMS; stage-1 must T1 fail-closed
    // (no anim clear), not run the 274 handler.
    let mut c = client_289();
    c.ingame = true;
    let mut player = ClientPlayer::at(1, 1);
    player.primary_anim = 42;
    c.players[0] = Some(Box::new(player));
    let frame = hex_bytes("2f00020102");
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
    // manifest: fixed_logout_opcode_121 — length 0 fixed; stage-1 proves framing
    // selects size 0 and delivers an empty payload to dispatch. Full method104
    // lifecycle is stage-2; here we only require the frame is accepted once.
    let mut c = client_289();
    c.ingame = true;
    let frame = hex_bytes("79");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 1);
    // Opcode 121 is not yet a handled 289 logout branch → T1 logout.
    assert!(!c.ingame);
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
fn r289_opcode_219_fail_closed_not_obj_reveal_or_rebuild() {
    // 219 = 274 OBJ_REVEAL and 289 REBUILD_NORMAL. Stage-1 must not run either.
    let mut c = client_289();
    c.ingame = true;
    c.scene_state = 2;
    c.map_build_centre_zone_x = 50;
    c.map_build_centre_zone_z = 50;
    let mut p = Packet::new(vec![0, 1, 0, 2]); // would-be rebuild coords
    c.psize = 4;
    c.handle_packet(219, &mut p);
    assert!(!c.ingame, "colliding 219 must T1 logout");
    assert_eq!(c.scene_state, 2, "must not enter REBUILD_NORMAL path");
    assert_eq!(c.map_build_centre_zone_x, 50);
    assert_eq!(c.map_build_centre_zone_z, 50);
}

#[test]
fn r289_opcode_47_fail_closed_not_reset_anims_direct() {
    let mut c = client_289();
    c.ingame = true;
    let mut player = ClientPlayer::at(1, 1);
    player.primary_anim = 99;
    c.players[0] = Some(Box::new(player));
    let mut p = Packet::new(vec![1, 2]);
    c.psize = 2;
    c.handle_packet(ServerProt::RESET_ANIMS, &mut p);
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
    assert_eq!(sim.revision, ClientRevision::R274);
    assert!(sim.adopt_from(&mut head).is_some());
    assert_eq!(
        sim.revision,
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
