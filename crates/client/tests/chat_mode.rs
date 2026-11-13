//! Task 2: `chatModeLoop` (Java `Client.java` 2755-2800). Clicking the
//! Public/Private/Trade/duel strip buttons on the chatback cycles the mode
//! and emits `CHAT_SETMODE` (154) with the three modes; the Report abuse
//! button resets the report form and arms typed name input (Java 274
//! Client.java:2789-2797; Java 289 Client.java:4217-4224).
use client::client::{Client, ClientConfig, ClientRevision};
use client::config::if_type::IfType;

fn client() -> Client {
    client_with_revision(ClientRevision::R274)
}

fn client_with_revision(revision: ClientRevision) -> Client {
    Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        revision,
    )
}

/// Latch a left click at applet coords and run the chat mode handler.
fn click_chat(c: &mut Client, x: i32, y: i32) {
    c.shell.apply_mouse_down(1, x, y);
    c.shell.latch_click();
    c.chat_mode_loop();
}

#[test]
fn public_chat_button_cycles_mode_and_sends_chat_setmode() {
    let mut c = client();
    c.shell.apply_mouse_down(1, 55, 480);
    c.shell.latch_click();
    c.chat_mode_loop();
    assert_eq!(c.chat_public_mode, 1);
    // CHAT_SETMODE(154) + p1(public) p1(private) p1(trade) in Java order;
    // isaac is None at new, so p1_enc writes the raw opcode 154.
    assert_eq!(&c.out.data()[..c.out.pos], &[154, 1, 0, 0]);
}

#[test]
fn trade_button_cycles_trade_mode() {
    let mut c = client();
    c.chat_trade_mode = 2;
    c.shell.apply_mouse_down(1, 300, 480);
    c.shell.latch_click();
    c.chat_mode_loop();
    assert_eq!(c.chat_trade_mode, 0);
    // CHAT_SETMODE(154) + p1(public=0) p1(private=0) p1(trade=0); the
    // cycled value lands in the third (trade) slot, Java field order.
    assert_eq!(&c.out.data()[..c.out.pos], &[154, 0, 0, 0]);
}

#[test]
fn report_abuse_sends_close_modal_and_records_main_modal_id() {
    let mut c = client();
    c.set_iface(
        600,
        IfType {
            client_code: 600,
            layer_id: 7,
            ..IfType::default()
        },
    );
    click_chat(&mut c, 462, 480);
    // CLOSE_MODAL(51) was sent by close_modal
    assert!(c.out.pos > 0);
    assert_eq!(c.main_modal_id, 7);
    // close_modal also cleared the side/chat modals (nothing open here).
    assert_eq!(c.side_modal_id, -1);
    assert_eq!(c.chat_modal_id, -1);
}

#[test]
fn report_button_then_typing_fills_the_report_name() {
    for revision in [
        ClientRevision::R274,
        ClientRevision::R289,
        ClientRevision::R377,
    ] {
        let mut c = client_with_revision(revision);
        c.set_iface(
            2,
            IfType {
                id: 2,
                layer_id: 2,
                client_code: 600,
                ..Default::default()
            },
        );
        c.report_abuse_input = "stale".into();
        c.report_abuse_mute_option = true;
        click_chat(&mut c, 450, 480);
        assert_eq!(c.main_modal_id, 2, "{revision:?}");
        assert_eq!(c.report_abuse_com_id, 2, "{revision:?}");
        assert!(c.report_abuse_input.is_empty(), "{revision:?}");
        assert!(!c.report_abuse_mute_option, "{revision:?}");
        for key in b"bob" {
            c.shell.apply_key(true, 0, *key as i32);
        }
        c.handle_chat_input();
        assert_eq!(c.report_abuse_input, "bob", "{revision:?}");
        assert!(c.chat_input.is_empty(), "{revision:?}");
    }
}

#[test]
fn non_strip_clicks_are_ignored() {
    let mut c = client();
    click_chat(&mut c, 300, 400);
    assert_eq!(c.chat_public_mode, 0);
    assert_eq!(c.chat_trade_mode, 0);
    assert_eq!(c.out.pos, 0);
    assert_eq!(c.main_modal_id, -1);
}
