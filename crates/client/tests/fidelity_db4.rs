//! DB4: Java 289 client.java:3405-3413,3529-3537 uses zero-initialized varps.
use client::client::{Client, ClientConfig, ClientRevision};
use client::io::{Packet, ServerProt289};
fn client() -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: std::env::temp_dir()
                .join(format!("fidelity-dispatch-{}", std::process::id()))
                .display()
                .to_string(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    c.ingame = true;
    c.ptype = -1;
    c
}

fn send(c: &mut Client, opcode: i32, bytes: &[u8]) {
    c.ptype = opcode;
    c.psize = bytes.len() as i32;
    c.handle_packet(opcode, &mut Packet::new(bytes.to_vec()));
}

#[test]
fn b4_varp_zero_for_an_unwritten_id_is_not_a_change() {
    let mut c = client();
    c.tut_com_id = 5;
    c.redraw_side = false;
    c.redraw_chat = false;
    send(&mut c, ServerProt289::VARP_SMALL, &[0, 3, 0]);
    assert_eq!(c.var_serv.get(3).copied(), Some(0));
    assert_eq!((c.redraw_side, c.redraw_chat), (false, false));
}

#[test]
fn large_and_sync_zero_values_do_not_change_unwritten_varps() {
    let mut c = client();
    c.tut_com_id = 5;
    c.redraw_side = false;
    c.redraw_chat = false;
    send(&mut c, ServerProt289::VARP_LARGE, &[0, 3, 0, 0, 0, 0]);
    assert_eq!(c.var_serv.get(3).copied(), Some(0));
    assert_eq!((c.redraw_side, c.redraw_chat), (false, false));
    send(&mut c, ServerProt289::VARP_SYNC, &[]);
    assert_eq!((c.redraw_side, c.redraw_chat), (false, false));
    assert!(c.var.is_empty(), "zero state stays implicit");
}

#[test]
fn sync_zero_for_an_unwritten_id_is_not_a_change() {
    let mut c = client();
    c.var_serv = vec![0; 4];
    c.redraw_side = false;
    send(&mut c, ServerProt289::VARP_SYNC, &[]);
    assert!(!c.redraw_side);
    assert!(c.var.is_empty(), "zero state stays implicit");
}

#[test]
fn control_varp_change_redraws_and_repeat_does_not() {
    let mut c = client();
    c.tut_com_id = 5;
    for (value, redraw) in [(9, true), (9, false), (0, true), (0, false)] {
        c.redraw_side = false;
        c.redraw_chat = false;
        send(&mut c, ServerProt289::VARP_SMALL, &[0, 3, value]);
        assert_eq!(
            (c.var[3], c.var_serv[3]),
            (i32::from(value), i32::from(value))
        );
        assert_eq!((c.redraw_side, c.redraw_chat), (redraw, redraw));
    }
}
