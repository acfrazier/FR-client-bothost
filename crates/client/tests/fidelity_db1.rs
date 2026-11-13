//! DB1: Java 289 client.java:3184-3194, including overlapping appearance bits.
use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision};
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

fn head_key(c: &mut Client, hair: u16, beard: u16) -> i32 {
    let mut local = ClientPlayer::default();
    local.appearance[8] = hair;
    local.appearance[11] = beard;
    c.local_player = Some(local);
    send(c, ServerProt289::IF_SETPLAYERHEAD, &[0, 7]);
    c.ifaces_mut[7].as_ref().unwrap().model1_id
}

#[test]
fn b1_playerhead_key_is_the_java_sum() {
    let mut c = client();
    c.set_iface(7, IfType::default());
    assert_eq!(head_key(&mut c, 260, 266), 16906);
    assert_eq!(c.ifaces_mut[7].as_ref().unwrap().model1_type, 3);
}

#[test]
fn b1_playerhead_keys_differ_for_different_hair() {
    let mut c = client();
    c.set_iface(7, IfType::default());
    let kit4 = head_key(&mut c, 260, 266);
    let kit0 = head_key(&mut c, 256, 266);
    assert_ne!(kit4, kit0);
}

#[test]
fn b1_playerhead_transmog_key_has_the_java_offset() {
    let mut c = client();
    c.set_iface(7, IfType::default());
    c.local_player = Some(ClientPlayer {
        transmog: Some(5),
        ..Default::default()
    });
    send(&mut c, ServerProt289::IF_SETPLAYERHEAD, &[0, 7]);
    assert_eq!(c.ifaces_mut[7].as_ref().unwrap().model1_id, 305419901);
}

#[test]
fn b1_playerhead_keys_differ_for_hats_1040_and_1044() {
    let mut c = client();
    c.set_iface(7, IfType::default());
    let mut keys = Vec::new();
    for hat in [1040u16, 1044] {
        let mut local = ClientPlayer::default();
        local.appearance[0] = 512 + hat;
        local.appearance[8] = 256;
        c.local_player = Some(local);
        send(&mut c, ServerProt289::IF_SETPLAYERHEAD, &[0, 7]);
        keys.push(c.ifaces_mut[7].as_ref().unwrap().model1_id);
    }
    assert_eq!(keys, [0x614000, 0x618000]);
}

#[test]
fn control_playerhead_key_without_shared_bits() {
    let mut c = client();
    c.set_iface(7, IfType::default());
    assert_eq!(head_key(&mut c, 259, 266), (259 << 6) + 266);
}
