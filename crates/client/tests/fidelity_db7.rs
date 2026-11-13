//! DB7: Java 289 client.java:3288-3291 clears the model type, not its id.
use client::client::{Client, ClientConfig, ClientRevision};
use client::config::{IfType, IfTypeMut};
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
fn b7_setobject_none_keeps_model1_id() {
    let mut c = client();
    c.set_iface(7, IfType::default());
    c.set_iface_mut(
        7,
        IfTypeMut {
            model1_type: 4,
            model1_id: 77,
            model_xan: 100,
            model_yan: 200,
            model_zoom: 300,
            ..Default::default()
        },
    );
    send(&mut c, ServerProt289::IF_SETOBJECT, &[0, 7, 255, 255, 0, 1]);
    let com = c.ifaces_mut[7].as_ref().unwrap();
    assert_eq!((com.model1_type, com.model1_id), (0, 77));
    assert_eq!(
        (com.model_xan, com.model_yan, com.model_zoom),
        (100, 200, 300)
    );
}
