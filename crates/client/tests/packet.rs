use client::io::{Isaac, Packet, PacketObserver};
use num_bigint::BigUint;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

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
