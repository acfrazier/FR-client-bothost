//! DB3: Java 289 client.java:2593-2611,2665-2683,2838-2858,3444-3458.
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

fn clear_redraw(c: &mut Client) {
    c.redraw_side = false;
    c.redraw_icons = false;
    c.redraw_chat = false;
    c.redraw_frame = false;
}

#[test]
fn b3_openmain_sets_no_redraw_flag_when_nothing_closes() {
    let mut c = client();
    c.set_iface(10, IfType::default());
    clear_redraw(&mut c);
    send(&mut c, ServerProt289::IF_OPENMAIN, &[0, 10]);
    assert_eq!(c.main_modal_id, 10);
    assert_eq!(
        (c.redraw_side, c.redraw_icons, c.redraw_chat),
        (false, false, false)
    );
}

#[test]
fn b3_openmain_side_redraws_chat_only_when_it_closes_chat() {
    let mut c = client();
    clear_redraw(&mut c);
    send(&mut c, ServerProt289::IF_OPENMAIN_SIDE, &[0, 10, 0, 11]);
    assert_eq!((c.main_modal_id, c.side_modal_id), (10, 11));
    assert!(c.redraw_side && c.redraw_icons);
    assert!(!c.redraw_chat);
}

#[test]
fn b3_openside_redraws_chat_only_when_it_closes_chat() {
    let mut c = client();
    c.set_iface(10, IfType::default());
    clear_redraw(&mut c);
    send(&mut c, ServerProt289::IF_OPENSIDE, &[0, 10]);
    assert_eq!(c.side_modal_id, 10);
    assert!(c.redraw_side && c.redraw_icons);
    assert!(!c.redraw_chat);
}

#[test]
fn b3_openchat_redraws_side_only_when_it_closes_side() {
    let mut c = client();
    c.set_iface(10, IfType::default());
    clear_redraw(&mut c);
    send(&mut c, ServerProt289::IF_OPENCHAT, &[0, 10]);
    assert_eq!(c.chat_modal_id, 10);
    assert!(c.redraw_chat);
    assert_eq!((c.redraw_side, c.redraw_icons), (false, false));
}

#[test]
fn control_openmain_closes_side_and_chat_with_their_flags() {
    let mut c = client();
    c.set_iface(10, IfType::default());
    c.side_modal_id = 3;
    c.chat_modal_id = 4;
    clear_redraw(&mut c);
    send(&mut c, ServerProt289::IF_OPENMAIN, &[0, 10]);
    assert_eq!(
        (c.main_modal_id, c.side_modal_id, c.chat_modal_id),
        (10, -1, -1)
    );
    assert!(c.redraw_side && c.redraw_icons && c.redraw_chat);
}

#[test]
fn redraw_flags_follow_each_closed_layer_or_input_dialog() {
    for opcode in [
        ServerProt289::IF_OPENMAIN,
        ServerProt289::IF_OPENMAIN_SIDE,
        ServerProt289::IF_OPENSIDE,
    ] {
        for (chat, input) in [(-1, false), (4, false), (-1, true), (4, true)] {
            let mut c = client();
            c.set_iface(10, IfType::default());
            c.chat_modal_id = chat;
            c.dialog_input_open = input;
            c.resumed_pause_button = true;
            clear_redraw(&mut c);
            let bytes: &[u8] = if opcode == ServerProt289::IF_OPENMAIN_SIDE {
                &[0, 10, 0, 11]
            } else {
                &[0, 10]
            };
            send(&mut c, opcode, bytes);
            assert_eq!(
                c.redraw_chat,
                chat != -1 || input,
                "opcode {opcode}, chat {chat}, input {input}"
            );
            assert_eq!(c.chat_modal_id, -1);
            assert!(!c.dialog_input_open);
            assert!(!c.resumed_pause_button);
            assert!(!c.redraw_frame);
        }
    }
    let mut c = client();
    c.set_iface(10, IfType::default());
    c.side_modal_id = 3;
    clear_redraw(&mut c);
    send(&mut c, ServerProt289::IF_OPENCHAT, &[0, 10]);
    assert!(c.redraw_side && c.redraw_icons && c.redraw_chat);
    assert_eq!(c.side_modal_id, -1);
}
