use client::io::{ClientProt289, Isaac, Packet, PacketObserver, ServerProt289};
use num_bigint::BigUint;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
enum ObserverEvent {
    Tick(u64, u64),
    Packet(u8, usize),
}

#[derive(Default)]
struct RecordingObserver(Mutex<Vec<ObserverEvent>>);

impl PacketObserver for RecordingObserver {
    fn packet_body_length(&self, opcode: u8) -> i16 {
        match opcode {
            120 => 3,
            121 => 0,
            130 => -1,
            131 => -2,
            opcode if opcode == client::io::ClientProt289::MOVE_GAMECLICK.id as u8 => -1,
            opcode if opcode == ClientProt289::EVENT_MOUSE_CLICK.id as u8 => 4,
            opcode if opcode == ClientProt289::CLIENT_CHEAT.id as u8 => -1,
            opcode if opcode == ClientProt289::MOVE_MINIMAPCLICK.id as u8 => -1,
            _ => 0,
        }
    }

    fn packet_committed(&self, opcode: u8, encoded_length: usize) {
        self.0
            .lock()
            .unwrap()
            .push(ObserverEvent::Packet(opcode, encoded_length));
    }

    fn observed_tick(&self, tick: u64, session: u64) {
        self.0
            .lock()
            .unwrap()
            .push(ObserverEvent::Tick(tick, session));
    }
}
fn packet_role_test_client() -> client::client::Client {
    client::client::Client::new_with_revision(
        client::client::ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: String::new(),
            members: true,
            lowmem: true,
        },
        client::client::ClientRevision::R289,
    )
}

fn flush_test_client() -> client::client::Client {
    let mut client = packet_role_test_client();
    client.ingame = true;
    client.ptype = -1;
    client.out.random = Some(Isaac::new(&[1, 2, 3, 4]));
    client.last_outbound = Instant::now();
    client.local_player = Some(client::client::ClientPlayer::at(10, 10));
    client
}

fn attach_game_loop_sink(client: &mut client::client::Client) -> TcpStream {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    client.stream =
        Some(client::io::ClientStream::connect(&address.ip().to_string(), address.port()).unwrap());
    let (server, _) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    server
}

fn send_empty_289_player_info(server: &mut TcpStream) {
    server
        .write_all(&[ServerProt289::PLAYER_INFO as u8, 0, 2, 0, 0])
        .unwrap();
}

fn read_game_loop_flush(server: &mut TcpStream, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    server.read_exact(&mut bytes).unwrap();
    bytes
}

fn decode_first_opcode(mut packet: Vec<u8>) -> (u8, Vec<u8>) {
    let mut isaac = Isaac::new(&[1, 2, 3, 4]);
    let opcode = packet[0].wrapping_sub(isaac.next_int() as u8);
    packet[0] = opcode;
    (opcode, packet)
}

#[test]
fn observer_preserves_ciphertext_and_reports_completed_packets_across_resets() {
    fn write_packets(packet: &mut Packet) -> Vec<Vec<u8>> {
        let mut outputs = Vec::new();
        for body in [0x0203, 0x0405] {
            packet.p1_enc(120);
            packet.p1(1);
            packet.p2(body);
            packet.finish_packet();
            outputs.push(packet.data()[..packet.pos].to_vec());
            packet.pos = 0;
        }
        packet.p1_enc(121);
        packet.finish_packet();
        outputs.push(packet.data()[..packet.pos].to_vec());
        outputs
    }

    let mut plain = Packet::new(vec![0; 100]);
    plain.random = Some(Isaac::new(&[1, 2, 3, 4]));
    let plain_outputs = write_packets(&mut plain);

    let observer = Arc::new(RecordingObserver::default());
    let mut observed = Packet::new(vec![0; 100]);
    observed.random = Some(Isaac::new(&[1, 2, 3, 4]));
    observed.set_observer(Some(observer.clone()));
    let observed_outputs = write_packets(&mut observed);

    assert_eq!(observed_outputs, plain_outputs);
    assert_eq!(
        *observer.0.lock().unwrap(),
        [
            ObserverEvent::Packet(120, 4),
            ObserverEvent::Packet(120, 4),
            ObserverEvent::Packet(121, 1),
        ]
    );
}

#[test]
fn observer_ignores_payload_and_waits_for_variable_length_backpatch() {
    let observer = Arc::new(RecordingObserver::default());
    let mut packet = Packet::new(vec![0; 100]);
    packet.set_observer(Some(observer.clone()));
    packet.p1(1);
    packet.p2(2);
    assert!(observer.0.lock().unwrap().is_empty());

    packet.p1_enc(130);
    packet.p1(0);
    packet.p2(0x1234);
    assert!(observer.0.lock().unwrap().is_empty());
    packet.psize1(2);
    assert_eq!(&packet.data()[..packet.pos], &[1, 0, 2, 130, 2, 0x12, 0x34]);
    assert_eq!(*observer.0.lock().unwrap(), [ObserverEvent::Packet(130, 4)]);
}

#[test]
fn observer_finalizes_two_byte_variable_length_packet() {
    let observer = Arc::new(RecordingObserver::default());
    let mut packet = Packet::new(vec![0; 100]);
    packet.set_observer(Some(observer.clone()));
    packet.p1_enc(131);
    packet.p2(1);
    packet.p1(0xab);
    packet.finish_packet();

    assert_eq!(&packet.data()[..packet.pos], &[131, 0, 1, 0xab]);
    assert_eq!(*observer.0.lock().unwrap(), [ObserverEvent::Packet(131, 4)]);
}
#[test]
fn tick_boundary_precedes_manual_packet_observation() {
    let observer = Arc::new(RecordingObserver::default());
    let mut packet = Packet::new(vec![0; 100]);
    packet.set_observer(Some(observer.clone()));
    packet.observe_tick(17, 4);
    packet.p1_enc(130);
    packet.p1(0);
    packet.p1(0xab);
    packet.psize1(1);

    assert_eq!(
        *observer.0.lock().unwrap(),
        [ObserverEvent::Tick(17, 4), ObserverEvent::Packet(130, 3),]
    );
}
#[test]
fn client_packet_roles_reject_cross_direction_mutations_without_state_loss() {
    let mut client = packet_role_test_client();
    let inbound_observer = Arc::new(RecordingObserver::default());

    client.r#in.set_frame_end(0);
    let framed_attach = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.r#in.set_observer(Some(inbound_observer.clone()));
    }));
    assert!(framed_attach.is_err());
    assert_eq!(client.r#in.frame_end(), Some(0));
    assert!(!client.r#in.has_observer());

    client.r#in.clear_frame_end();
    let unframed_attach = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.r#in.set_observer(Some(inbound_observer.clone()));
    }));
    assert!(unframed_attach.is_err());
    assert_eq!(client.r#in.frame_end(), None);
    assert!(!client.r#in.has_observer());

    let first_observer = Arc::new(RecordingObserver::default());
    client.out.set_observer(Some(first_observer.clone()));
    let bound_output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.out.set_frame_end(0);
    }));
    assert!(bound_output.is_err());
    assert!(client.out.has_observer());
    assert_eq!(client.out.frame_end(), None);
    client.out.clear_frame_end();
    assert!(client.out.has_observer());
    assert_eq!(client.out.frame_end(), None);
    client.out.p1_enc(121);
    assert_eq!(
        *first_observer.0.lock().unwrap(),
        [ObserverEvent::Packet(121, 1)]
    );

    client.out.set_observer(None);
    client.out.clear_frame_end();
    assert!(!client.out.has_observer());
    assert_eq!(client.out.frame_end(), None);
    let unobserved_output = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.out.set_frame_end(0);
    }));
    assert!(unobserved_output.is_err());
    assert!(!client.out.has_observer());
    assert_eq!(client.out.frame_end(), None);

    let replacement_observer = Arc::new(RecordingObserver::default());
    client.out.set_observer(Some(replacement_observer.clone()));
    client.out.p1_enc(121);
    assert_eq!(
        *first_observer.0.lock().unwrap(),
        [ObserverEvent::Packet(121, 1)]
    );
    assert_eq!(
        *replacement_observer.0.lock().unwrap(),
        [ObserverEvent::Packet(121, 1)]
    );
}

#[test]
fn released_packet_does_not_retain_observer() {
    let observer = Arc::new(RecordingObserver::default());
    let mut packet = Packet::alloc(0);
    packet.set_observer(Some(observer));
    assert!(packet.has_observer());
    packet.release();

    let packet = Packet::alloc(0);
    assert!(!packet.has_observer());
}

#[test]
fn legacy_movement_reports_completion_before_return_without_changing_ciphertext() {
    fn movement_client() -> client::client::Client {
        let mut client = client::client::Client::new_with_revision(
            client::client::ClientConfig {
                host: "127.0.0.1".into(),
                port: 43594,
                cache_dir: String::new(),
                members: true,
                lowmem: true,
            },
            client::client::ClientRevision::R289,
        );
        for row in &mut client.collision[0].flags {
            row.fill(0);
        }
        client.out.random = Some(Isaac::new(&[17, 29, 41, 53]));
        client
    }
    let mut plain = movement_client();
    assert!(plain.tryMove(5, 5, 6, 5, false, 0, 0, 0, 0, 0, 0));
    let observer = Arc::new(RecordingObserver::default());
    let mut observed = movement_client();
    observed.out.set_observer(Some(observer.clone()));
    assert!(observed.tryMove(5, 5, 6, 5, false, 0, 0, 0, 0, 0, 0));
    assert_eq!(
        &observed.out.data()[..observed.out.pos],
        &plain.out.data()[..plain.out.pos]
    );
    assert_eq!(
        *observer.0.lock().unwrap(),
        [ObserverEvent::Packet(
            client::io::ClientProt289::MOVE_GAMECLICK.id as u8,
            observed.out.pos,
        )]
    );
}

#[test]
fn game_loop_flush_commits_last_cheat_after_tick_without_changing_bytes() {
    let command = "where";
    let mut plain = flush_test_client();
    let mut observed = flush_test_client();
    plain.set_cheat_admission(client::client::CheatAdmission::Granted);
    observed.set_cheat_admission(client::client::CheatAdmission::Granted);
    let observer = Arc::new(RecordingObserver::default());
    observed.out.set_observer(Some(observer.clone()));
    let mut plain_server = attach_game_loop_sink(&mut plain);
    let mut observed_server = attach_game_loop_sink(&mut observed);
    send_empty_289_player_info(&mut plain_server);
    send_empty_289_player_info(&mut observed_server);

    assert!(plain.send_cheat(command).is_sent());
    assert!(observed.send_cheat(command).is_sent());
    plain.last_outbound = Instant::now();
    observed.last_outbound = Instant::now();
    plain.game_loop();
    observed.game_loop();

    let encoded_length = command.len() + 3;
    let plain_bytes = read_game_loop_flush(&mut plain_server, encoded_length);
    let observed_bytes = read_game_loop_flush(&mut observed_server, encoded_length);
    assert_eq!(
        observed_bytes, plain_bytes,
        "installing the observer must not change client wire bytes"
    );
    let (_, decoded) = decode_first_opcode(observed_bytes);
    assert_eq!(decoded[0], ClientProt289::CLIENT_CHEAT.id as u8);
    assert_eq!(decoded[1] as usize, command.len() + 1);
    assert_eq!(&decoded[2..], b"where\n");
    assert_eq!(observed.gens.player_info, 1);
    assert_eq!(
        *observer.0.lock().unwrap(),
        [
            ObserverEvent::Tick(1, 0),
            ObserverEvent::Packet(ClientProt289::CLIENT_CHEAT.id as u8, encoded_length),
        ]
    );
}

#[test]
fn game_loop_flush_commits_last_minimap_walk_with_all_fourteen_trailer_bytes() {
    fn minimap_client() -> client::client::Client {
        let mut client = flush_test_client();
        let mut player = client::client::ClientPlayer::at(10, 10);
        player.x = 1344;
        player.z = 1344;
        client.local_player = Some(player);
        client.shell.apply_mouse_down(1, 648, 83);
        client.shell.latch_click();
        client
    }

    let mut plain = minimap_client();
    let mut observed = minimap_client();
    let observer = Arc::new(RecordingObserver::default());
    observed.out.set_observer(Some(observer.clone()));
    let mut plain_server = attach_game_loop_sink(&mut plain);
    let mut observed_server = attach_game_loop_sink(&mut observed);
    send_empty_289_player_info(&mut plain_server);
    send_empty_289_player_info(&mut observed_server);

    plain.last_outbound = Instant::now();
    observed.last_outbound = Instant::now();
    plain.game_loop();
    observed.game_loop();

    let plain_bytes = read_game_loop_flush(&mut plain_server, 26);
    let observed_bytes = read_game_loop_flush(&mut observed_server, 26);
    assert_eq!(
        observed_bytes, plain_bytes,
        "installing the observer must not change client wire bytes"
    );
    let (_, decoded) = decode_first_opcode(observed_bytes.clone());
    assert_eq!(decoded[0], ClientProt289::EVENT_MOUSE_CLICK.id as u8);
    let mut opcode_cipher = Isaac::new(&[1, 2, 3, 4]);
    let _ = opcode_cipher.next_int();
    assert_eq!(
        observed_bytes[5].wrapping_sub(opcode_cipher.next_int() as u8),
        ClientProt289::MOVE_MINIMAPCLICK.id as u8
    );
    assert_eq!(
        decoded[6], 19,
        "the body includes all 14 minimap trailer bytes"
    );
    assert_eq!(&decoded[7..12], &[0, 0, 10, 0, 10]);
    assert_eq!(
        &decoded[12..],
        &[0, 0, 0, 0, 57, 0, 0, 89, 5, 64, 5, 64, 0, 63]
    );
    assert_eq!(observed.gens.player_info, 1);
    assert_eq!(
        *observer.0.lock().unwrap(),
        [
            ObserverEvent::Tick(1, 0),
            ObserverEvent::Packet(ClientProt289::EVENT_MOUSE_CLICK.id as u8, 5),
            ObserverEvent::Packet(ClientProt289::MOVE_MINIMAPCLICK.id as u8, 21),
        ]
    );
}

#[test]
fn p1_enc_xors_isaac() {
    let mut p = Packet::alloc(0);
    p.random = Some(Isaac::new(&[1, 2, 3, 4]));
    p.p1_enc(120);
    assert_eq!(p.data()[0], 182); // (120 + -621246914) & 0xff
}

#[test]
fn rsaenc_java_key_hi_1337() {
    let mut p = Packet::alloc(1);
    p.pjstr("hi");
    p.p4(1337);
    let n = BigUint::from_str(client::JAVA_LOGIN_RSAN).unwrap();
    let e = BigUint::from_str(client::JAVA_LOGIN_RSAE).unwrap();
    p.rsaenc(&n, &e);
    assert!(p.pos >= 2);
    assert_eq!(p.data()[0] as usize, p.pos - 1);
    assert_eq!(
        &p.data()[..p.pos],
        &[
            64, 86, 83, 87, 172, 26, 221, 24, 175, 132, 67, 99, 115, 197, 146, 98, 155, 170, 193,
            109, 58, 8, 193, 175, 2, 236, 115, 37, 114, 49, 206, 94, 10, 55, 29, 205, 170, 223,
            128, 245, 135, 72, 178, 150, 234, 153, 197, 241, 204, 145, 159, 190, 42, 207, 8, 85,
            247, 113, 125, 158, 157, 214, 15, 100, 176
        ]
    );
}

#[test]
fn p1_p2_p4_big_endian_layout() {
    let mut p = Packet::alloc(0);
    p.p1(1);
    p.p2(0x0203);
    p.p4(0x04050607);
    assert_eq!(&p.data()[..p.pos], &[1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn g_roundtrip_signed_and_smart() {
    let mut p = Packet::alloc(0);
    p.p1(0xff);
    p.p2(0xfffe);
    p.pos = 0;
    assert_eq!(p.g1(), 255);
    p.pos = 0;
    assert_eq!(p.g1b(), -1);
    // p2 wrote data[1..3] = [0xff, 0xfe]; rewind to 1, not 0
    p.pos = 1;
    assert_eq!(p.g2(), 0xfffe);
    p.pos = 1;
    assert_eq!(p.g2b(), -2);
}

#[test]
fn crc32_matches_client_ts() {
    let src: Vec<u8> = (0..10).collect();
    assert_eq!(Packet::getcrc(&src, 0, 10), 1164760902);
}

#[test]
fn pjstr_newline_terminated() {
    let mut p = Packet::alloc(0);
    p.pjstr("hi");
    assert_eq!(&p.data()[..p.pos], b"hi\n");
    p.pos = 0;
    assert_eq!(p.gjstr(), "hi");
}

#[test]
fn bounded_frame_hides_reusable_tail_from_strings_and_reads() {
    let mut p = Packet::new(b"ok\nSTALE".to_vec());
    p.set_frame_end(3);
    assert_eq!(p.gjstr(), "ok");
    p.pos = 3;
    let short = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.g1()));
    assert!(short.is_err());
}
