//! DB6: Java 289 client.java:3165-3180 opens the welcome layer without frame redraw.
use client::client::{Client, ClientConfig, ClientRevision};
use client::config::IfType;
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
fn b6_last_login_info_does_not_set_redraw_frame() {
    let mut c = client();
    c.set_iface(
        42,
        IfType {
            id: 99,
            layer_id: 42,
            client_code: 650,
            ..Default::default()
        },
    );
    c.redraw_frame = false;
    send(
        &mut c,
        ServerProt289::LAST_LOGIN_INFO,
        &[1, 2, 3, 4, 0, 9, 201, 0, 10, 0],
    );
    assert_eq!(c.main_modal_id, 42);
    assert!(!c.redraw_frame);
    c.redraw_frame = true;
    c.main_modal_id = -1;
    send(
        &mut c,
        ServerProt289::LAST_LOGIN_INFO,
        &[1, 2, 3, 4, 0, 9, 201, 0, 10, 0],
    );
    assert!(
        c.redraw_frame,
        "the handler does not clear an existing redraw request"
    );
}
