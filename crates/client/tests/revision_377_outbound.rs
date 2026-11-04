//! Integration gates for 377-only shared-writer routing and batch rejection.
use client::client::{
    Client, ClientConfig, ClientNpc, ClientPlayer, ClientRevision, MiniMenuAction,
};
use client::io::{
    map_client_prot, ClientProt, ClientProt377, ClientStream, Packet, PacketObserver,
};
use std::io::{ErrorKind, Read};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
fn unsupported_action_discards_whole_batch_and_closes_without_wire_bytes() {
    let mut c = client();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    c.stream = Some(ClientStream::connect("127.0.0.1", address.port()).unwrap());
    let (mut server, _) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    c.out.set_observer(Some(Arc::new(StrictObserver)));
    c.local_player = Some(ClientPlayer::at(5, 5));
    c.npc[7] = Some(Box::new(ClientNpc::at(5, 5)));
    c.menu_action[0] = MiniMenuAction::OP_NPC2;
    c.menu_param_a[0] = 7;
    c.doAction(0);
    c.game_loop();
    assert!(!c.ingame);
    assert_eq!(c.out.pos, 0);
    let reason = c.take_session_exit_reason().unwrap().to_string();
    assert!(reason.contains("unsupported_outbound_377"), "{reason}");
    let mut bytes = [0; 1];
    match server.read(&mut bytes) {
        Ok(0) => {}
        Err(e) if e.kind() == ErrorKind::ConnectionReset => {}
        result => panic!("rejected batch must never reach socket: {result:?} {bytes:?}"),
    }
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
