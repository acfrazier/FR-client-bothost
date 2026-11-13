//! DB2: Java 274:2017, Java 289:8824, Java 377:11544 recurse only into type 1.
use client::client::{Client, ClientConfig, ClientRevision};
use client::config::{IfType, IfTypeMut};
use client::io::{Packet, ServerProt289};

fn client() -> Client {
    client_with_revision(ClientRevision::R289)
}

fn client_with_revision(revision: ClientRevision) -> Client {
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
        revision,
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

fn layer(id: i32, parent: i32, kind: i32, children: Option<Vec<i32>>) -> IfType {
    IfType {
        id,
        layer_id: parent,
        r#type: kind,
        children,
        ..Default::default()
    }
}

fn nested(c: &mut Client, kind: i32) {
    c.set_iface(10, layer(10, 10, 0, Some(vec![11])));
    c.set_iface(11, layer(11, 10, kind, Some(vec![12])));
    c.set_iface(12, layer(12, 11, 6, None));
    c.set_iface_mut(
        11,
        IfTypeMut {
            anim_frame: 7,
            anim_cycle: 3,
            ..Default::default()
        },
    );
    c.set_iface_mut(
        12,
        IfTypeMut {
            anim_frame: 5,
            anim_cycle: 9,
            ..Default::default()
        },
    );
}

#[test]
fn b2_if_anim_reset_does_not_enter_type_0_layers() {
    let mut c = client();
    nested(&mut c, 0);
    send(&mut c, ServerProt289::IF_OPENMAIN, &[0, 10]);
    assert_eq!(c.main_modal_id, 10);
    assert_eq!(
        (c.if_(11).unwrap().anim_frame, c.if_(11).unwrap().anim_cycle),
        (0, 0)
    );
    assert_eq!(
        (c.if_(12).unwrap().anim_frame, c.if_(12).unwrap().anim_cycle),
        (5, 9)
    );
}

#[test]
fn all_revisions_recurse_only_into_type_1() {
    for revision in [
        ClientRevision::R274,
        ClientRevision::R289,
        ClientRevision::R377,
    ] {
        let mut c = client_with_revision(revision);
        for kind in [0, 1, 6] {
            nested(&mut c, kind);
            c.if_anim_reset(10);
            assert_eq!(
                (c.if_(11).unwrap().anim_frame, c.if_(11).unwrap().anim_cycle),
                (0, 0),
                "{revision:?} type {kind}"
            );
            let expected = if kind == 1 { (0, 0) } else { (5, 9) };
            assert_eq!(
                (c.if_(12).unwrap().anim_frame, c.if_(12).unwrap().anim_cycle),
                expected,
                "{revision:?} type {kind}"
            );
        }
    }
}
