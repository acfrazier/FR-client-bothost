//! DB5: Java 289 client.java:4733-4818 supplies welcome and mute-option text.
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

fn update(c: &mut Client, code: i32) -> (String, i32) {
    c.set_iface(
        99,
        IfType {
            id: 99,
            layer_id: 42,
            client_code: code,
            ..Default::default()
        },
    );
    c.set_iface_mut(
        99,
        IfTypeMut {
            text: "decoded".into(),
            colour: 0x123456,
            ..Default::default()
        },
    );
    c.client_component(99);
    let com = c.ifaces_mut[99].as_ref().unwrap();
    (com.text.clone(), com.colour)
}

#[test]
fn b5_welcome_screen_shows_the_unread_message_count() {
    let mut c = client();
    c.set_iface(
        99,
        IfType {
            id: 99,
            layer_id: 42,
            client_code: 651,
            ..Default::default()
        },
    );
    send(
        &mut c,
        ServerProt289::LAST_LOGIN_INFO,
        &[1, 2, 3, 4, 0, 9, 201, 0, 10, 0],
    );
    assert_eq!(c.last_login_message_count, 10);
    c.client_component(99);
    let com = c.ifaces_mut[99].as_ref().unwrap();
    assert_eq!(
        (com.text.as_str(), com.colour),
        ("10 unread messages", 65280)
    );
}

#[test]
fn login_text_uses_java_days_and_host_dns_or_dotted_address_for_both_codes() {
    let mut c = client();
    c.last_login_ip = 0x01020304;
    for code in [650, 655] {
        for (days, when) in [(0, "earlier today"), (1, "yesterday"), (9, "9 days ago")] {
            c.days_since_login = days;
            c.last_login_dns_display = None;
            assert_eq!(
                update(&mut c, code),
                (format!("You last logged in {when} from: 1.2.3.4"), 0x123456)
            );
            c.last_login_dns_display = Some("host.example".into());
            assert_eq!(
                update(&mut c, code),
                (
                    format!("You last logged in {when} from: host.example"),
                    0x123456
                )
            );
        }
    }
    c.last_login_ip = 0;
    for code in [650, 655] {
        assert_eq!(update(&mut c, code), (String::new(), 0x123456));
    }
    c.last_login_ip = 0xff020304u32 as i32;
    c.last_login_dns_display = None;
    c.days_since_login = 0;
    assert_eq!(
        update(&mut c, 655).0,
        "You last logged in earlier today from: 255.2.3.4"
    );
}

#[test]
fn unread_message_text_and_colours_follow_java() {
    let mut c = client();
    for (count, text, colour) in [
        (-1, "decoded", 0x123456),
        (0, "0 unread messages", 16776960),
        (1, "1 unread message", 65280),
        (12, "12 unread messages", 65280),
    ] {
        c.last_login_message_count = count;
        assert_eq!(update(&mut c, 651), (text.into(), colour));
    }
}

#[test]
fn recovery_lines_follow_java_days_and_member_warning() {
    let mut c = client();
    for (days, warning, lines) in [
        (201, 0, ["", "", ""]),
        (
            201,
            1,
            [
                "@yel@This is a non-members world: @whi@Since you are a member we",
                "@whi@recommend you use a members world instead. You may use",
                "@whi@this world but member benefits are unavailable whilst here.",
            ],
        ),
        (
            200,
            0,
            [
                "You have not yet set any password recovery questions.",
                "We strongly recommend you do so now to secure your account.",
                "Do this from the 'account management' area on our front webpage",
            ],
        ),
        (
            0,
            0,
            [
                "Earlier today you changed your recovery questions",
                "If you do not remember making this change then cancel it immediately",
                "Do this from the 'account management' area on our front webpage",
            ],
        ),
        (
            1,
            0,
            [
                "Yesterday you changed your recovery questions",
                "If you do not remember making this change then cancel it immediately",
                "Do this from the 'account management' area on our front webpage",
            ],
        ),
        (
            9,
            0,
            [
                "9 days ago you changed your recovery questions",
                "If you do not remember making this change then cancel it immediately",
                "Do this from the 'account management' area on our front webpage",
            ],
        ),
    ] {
        c.days_since_recovery_change = days;
        c.members_warning = warning;
        for (code, text) in (652..=654).zip(lines) {
            assert_eq!(
                update(&mut c, code),
                (text.into(), 0x123456),
                "days {days}, warning {warning}, code {code}"
            );
        }
    }
}

#[test]
fn mute_option_uses_java_privilege_text_and_colour() {
    let mut c = client();
    for level in [-1, 0, 1, 2] {
        c.staffmodlevel = level;
        for mute in [false, true] {
            c.report_abuse_mute_option = mute;
            let expected = if level < 1 {
                (String::new(), 0x123456)
            } else if mute {
                (
                    "Moderator option: Mute player for 48 hours: <ON>".into(),
                    16711680,
                )
            } else {
                (
                    "Moderator option: Mute player for 48 hours: <OFF>".into(),
                    16777215,
                )
            };
            assert_eq!(update(&mut c, 613), expected);
        }
    }
}

#[test]
fn other_revisions_keep_their_existing_component_behaviour() {
    for revision in [ClientRevision::R274, ClientRevision::R377] {
        let mut c = client_with_revision(revision);
        c.last_login_ip = 0x01020304;
        c.last_login_message_count = 10;
        c.staffmodlevel = 1;
        for code in [613, 650, 651, 652, 653, 654, 655] {
            assert_eq!(
                update(&mut c, code),
                ("decoded".into(), 0x123456),
                "{revision:?} code {code}"
            );
        }
    }
}
