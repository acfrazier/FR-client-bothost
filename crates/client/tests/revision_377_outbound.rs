//! Integration gates for revision-377 shared writers and chat packet routing.
use client::client::{CheatAdmission, Client, ClientConfig, ClientRevision};
use client::config::if_type::IfType;
use client::io::{map_client_prot, ClientProt, ClientProt377, Packet, PacketObserver};
use std::sync::Arc;
use std::time::Instant;

fn client() -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43595,
            cache_dir: std::env::temp_dir()
                .join(format!("377-outbound-absent-{}", std::process::id()))
                .to_string_lossy()
                .into_owned(),
            members: true,
            lowmem: true,
        },
        ClientRevision::R377,
    );
    c.ingame = true;
    c
}

#[test]
fn compatible_shared_writers_select_literal_java_ids() {
    for (shared, java) in [
        (ClientProt::NO_TIMEOUT, ClientProt377::NO_TIMEOUT),
        (
            ClientProt::MAP_BUILD_COMPLETE,
            ClientProt377::MAP_BUILD_COMPLETE,
        ),
        (ClientProt::IF_BUTTON, ClientProt377::IF_BUTTON),
        (ClientProt::IDLE_TIMER, ClientProt377::IDLE_TIMER),
        (
            ClientProt::RESUME_PAUSEBUTTON,
            ClientProt377::RESUME_PAUSEBUTTON,
        ),
        (ClientProt::CLOSE_MODAL, ClientProt377::CLOSE_MODAL),
        (ClientProt::TUT_CLICKSIDE, ClientProt377::TUT_CLICKSIDE),
        (ClientProt::IDK_SAVEDESIGN, ClientProt377::IDK_SAVEDESIGN),
    ] {
        assert_eq!(map_client_prot(ClientRevision::R377, shared), java);
    }
    // A coincidentally equal body length does not authorize a legacy encoder.
    assert_eq!(
        map_client_prot(ClientRevision::R377, ClientProt::OPNPC2).id,
        -1
    );
    assert_eq!(
        map_client_prot(ClientRevision::R377, ClientProt { id: 999, length: 0 }).id,
        -1
    );
    // Chat uses dedicated 377 writers, not the legacy shared descriptors.
    for shared in [ClientProt::MESSAGE_PRIVATE, ClientProt::MESSAGE_PUBLIC] {
        assert_eq!(map_client_prot(ClientRevision::R377, shared).id, -1);
    }
}

struct StrictObserver;
impl PacketObserver for StrictObserver {
    fn packet_body_length(&self, opcode: u8) -> i16 {
        ClientProt377::ALL
            .iter()
            .find(|p| p.id == i32::from(opcode))
            .expect("observer must never see a rejected opcode")
            .length as i16
    }
    fn packet_committed(&self, _: u8, _: usize) {}
}

#[test]
fn rejected_header_never_reaches_observer_or_cipher() {
    let mut p = Packet::new(vec![0; 100]);
    p.random = Some(client::io::Isaac::new(&[1, 2, 3, 4]));
    p.set_observer(Some(Arc::new(StrictObserver)));
    p.p1_enc(-1);
    assert_eq!(p.pos, 0);
    p.p1_enc(40);
    assert_eq!(&p.data()[..p.pos], &[102]); // First seeded word adds 62 mod 256.
}

#[test]
fn private_chat_uses_revision_377_packet_writer() {
    for recipient in [0i64, 1] {
        let mut c = client();
        c.social_input_open = true;
        c.social_input_type = 3;
        c.social_input = "Hi".into();
        c.social_userhash = recipient;
        c.consume_chat_key(13);

        assert_eq!(
            &c.out.data()[..c.out.pos],
            &[227, 9, 0, 0, 0, 0, 0, 0, 0, recipient as u8, 0x65]
        );
        assert!(c.ingame);
    }
}

#[test]
fn public_chat_uses_revision_377_transforms_and_prefixes() {
    let mut c = client();
    c.chat_input = "red:shake:Hi".into();
    c.consume_chat_key(13);

    assert_eq!(&c.out.data()[..c.out.pos], &[49, 3, 0xff, 131, 0x65]);
    assert!(c.ingame);
}

#[test]
fn cheat_377_uses_java_utf16_length_and_low_bytes() {
    let mut c = client();
    c.set_cheat_admission(CheatAdmission::Granted);
    assert!(c.send_cheat("a\u{1F642}").is_sent());

    assert_eq!(&c.out.data()[..c.out.pos], &[56, 4, b'a', 0x3d, 0x42, 10]);
}

#[test]
fn report_abuse_rules_and_mute_use_java_377_action_codes() {
    let mut c = client();
    c.report_abuse_input = "Bob".into();
    c.report_abuse_mute_option = true;
    c.set_iface(
        9,
        IfType {
            client_code: 613,
            ..IfType::default()
        },
    );
    assert!(!c.client_button(9));
    assert_eq!(
        &c.out.data()[..c.out.pos],
        &[110, 184, 0, 0, 0, 0, 0, 0, 0x0c, 0xdf, 12, 1]
    );
    assert_eq!(ClientProt377::SEND_SNAPSHOT.length, 10);
    assert_eq!(c.main_modal_id, -1);

    c.out.pos = 0;
    c.set_iface(
        9,
        IfType {
            client_code: 620,
            ..IfType::default()
        },
    );
    assert!(!c.client_button(9));
    assert!(!c.report_abuse_mute_option);
    assert_eq!(c.out.pos, 0);
}

#[test]
fn idle_and_unattended_session_do_not_panic_or_use_legacy_opcodes() {
    let mut c = client();
    for tick in 1..=5001 {
        c.last_outbound = Instant::now();
        c.out.pos = 0;
        c.game_loop();
        assert!(c.ingame, "ordinary idle tick {tick}");
        let bytes = &c.out.data()[..c.out.pos];
        assert_eq!(
            bytes.last() == Some(&202),
            tick == 4501 || tick == 5001,
            "tick {tick}: {bytes:?}"
        );
        assert!(!bytes.contains(&209), "274 idle opcode cannot leak");
    }
}
