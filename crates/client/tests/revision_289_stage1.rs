//! Revision 289 stage-1: production framing + inventory through Client paths.
//!
//! Fixtures come from `crates/client/tests/fixtures/revision_289/manifest.json`
//! (independently derived oracles). Cases outside stage-1 (actors, login RSA,
//! region/widgets) are not asserted here.

use client::client::{Client, ClientConfig, ClientRevision};
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
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let data = frame.to_vec();
    let barrier = Arc::new(Barrier::new(2));
    let b2 = Arc::clone(&barrier);
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(&data).unwrap();
        // Hold the socket open until the client finishes polling so
        // available()/read_bytes see the full write.
        b2.wait();
        let _ = sock.shutdown(std::net::Shutdown::Both);
    });

    c.stream = Some(ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    c.random_in = None;
    c.ptype = -1;

    // Give the writer a moment to land the bytes before the first poll.
    thread::sleep(Duration::from_millis(20));

    let mut accepted = 0usize;
    for _ in 0..max_polls {
        if c.tcp_in() {
            accepted += 1;
        } else {
            // Pending incomplete frame or drained.
            thread::sleep(Duration::from_millis(5));
        }
    }
    barrier.wait();
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
fn framing_variable_g2_complete_reads_declared_payload() {
    // manifest: variable_g2_frame_complete — opcode 47, g2 len 2, payload 0102.
    // Numeric 47 collides with 274 RESET_ANIMS; stage-1 only requires the
    // frame to be fully read and handed to production dispatch (ptype cleared).
    let mut c = client_289();
    c.ingame = true;
    let frame = hex_bytes("2f00020102");
    let accepted = feed_frames(&mut c, &frame, 8);
    assert_eq!(accepted, 1, "complete frame must pass read_packet");
    assert_eq!(
        c.ptype, -1,
        "dispatch must clear ptype after a complete frame"
    );
    assert_eq!(c.psize, 2, "declared g2 length retained from header");
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
