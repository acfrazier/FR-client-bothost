//! Synthetic bytes from the original Java Packet/Isaac, not a Rust encoder.
#[path = "common/isolation.rs"]
mod isolation;

use client::client::{Client, ClientConfig, ClientRevision};
use client::config::Cache;
use client::io::{ClientStream, Isaac, Packet};
use client::{ClientSessionConfig, ClientSessionProfile, Transport};
use num_bigint::BigUint;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

fn oracle() -> Value {
    serde_json::from_str(include_str!("fixtures/revision_377/wire.json")).unwrap()
}

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn client(revision: ClientRevision, port: u16) -> isolation::IsolatedClient {
    isolation::IsolatedClient::with_config(
        ClientConfig {
            host: String::new(),
            port: 0,
            cache_dir: String::new(),
            members: true,
            lowmem: false,
        },
        revision,
        (port != 0).then(|| (Ipv4Addr::LOCALHOST, port).into()),
    )
}

fn socket_pair() -> (ClientStream, TcpStream) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let client = ClientStream::connect(&address.ip().to_string(), address.port()).unwrap();
    let (server, _) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    server.set_nodelay(true).unwrap();
    (client, server)
}

fn buffered(c: &mut Client, count: i32) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while c.stream.as_mut().unwrap().available().unwrap() < count {
        assert!(Instant::now() < deadline, "socket fragment did not arrive");
        thread::yield_now();
    }
}

fn refusal(code: u8, revision: ClientRevision) -> client::client::LoginError {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut hello = [0; 2];
        socket.read_exact(&mut hello).unwrap();
        assert_eq!(hello, [14, 6]); // Original Java JString.toBase37("alice") selector.
        socket.write_all(&[0; 8]).unwrap();
        socket.write_all(&[code]).unwrap();
        if code == 21 {
            socket.write_all(&[7]).unwrap();
        }
    });
    let mut c = client(revision, port);
    c.set_external_reconnect_owner(true);
    c.set_login_deadline(Instant::now() + Duration::from_secs(3));
    let error = c.login("alice", "test", false).unwrap_err();
    assert_eq!(error.code, i32::from(code));
    assert_eq!(c.login_mes1, error.mes1);
    assert_eq!(c.login_mes2, error.mes2);
    server.join().unwrap();
    error
}

fn java_message(code: u8) {
    let expected = oracle();
    let error = refusal(code, ClientRevision::R377);
    assert_eq!(
        error.mes1,
        expected["messages"][code.to_string()]["text"][0]
    );
    assert_eq!(
        error.mes2,
        expected["messages"][code.to_string()]["text"][1]
    );
    assert!(error.retry_after.is_none());
    assert!(error.transport.is_none());
}

#[test]
fn response_6_matches_java() {
    java_message(6);
}
#[test]
fn response_18_matches_java() {
    java_message(18);
}
#[test]
fn response_22_matches_java() {
    java_message(22);
}
#[test]
fn response_23_matches_java() {
    java_message(23);
}
#[test]
fn response_24_matches_java() {
    java_message(24);
}
#[test]
fn response_25_matches_java() {
    java_message(25);
}
#[test]
fn response_26_matches_java() {
    java_message(26);
}

#[test]
fn legacy_messages_and_host_owned_transfer_are_unchanged() {
    for revision in [ClientRevision::R274, ClientRevision::R289] {
        assert_eq!(
            refusal(6, revision).mes2,
            "Wrong RSA key; check this world's login key."
        );
        for code in [18, 22, 23, 24, 25, 26] {
            assert_eq!(refusal(code, revision).mes1, "Unexpected server response");
        }
    }
    let transfer = refusal(21, ClientRevision::R377);
    assert_eq!(transfer.retry_after, Some(Duration::from_secs(7)));
    assert_eq!(
        transfer.mes2,
        "Your profile will be transferred in: 7 seconds"
    );
    for code in [1, 3, 4, 5, 7, 8, 9, 10, 12, 13, 14, 16, 17, 20, 254] {
        assert!(refusal(code, ClientRevision::R377).transport.is_none());
    }
}

#[test]
fn original_java_isaac_and_rsa_sign_padding_match() {
    let expected = oracle();
    for direction in 0..2 {
        let seed = [1, 2, 0x1234_5678, -1].map(|word: i32| word.wrapping_add(direction * 50));
        let mut cipher = Isaac::new(&seed);
        for word in expected["isaac"][direction as usize].as_array().unwrap() {
            assert_eq!(cipher.next_int(), word.as_i64().unwrap() as i32);
        }
    }
    for case in expected["rsa"].as_array().unwrap() {
        let mut packet = Packet::alloc(1);
        let raw = hex(case["plaintext"].as_str().unwrap());
        packet.pdata(&raw, 0, raw.len());
        packet.rsaenc(
            &BigUint::from(case["modulus"].as_u64().unwrap()),
            &BigUint::from(case["exponent"].as_u64().unwrap()),
        );
        assert_eq!(
            &packet.data()[..packet.pos],
            hex(case["frame"].as_str().unwrap())
        );
    }
    let mut c = client(ClientRevision::R377, 0);
    c.login_uid = 0x7654_3210;
    c.write_login_block([1, 2, 0x1234_5678, -1], "alice", "test");
    assert_eq!(
        &c.out.data()[..c.out.pos],
        hex(expected["plaintext"].as_str().unwrap())
    );
}

#[test]
fn every_fixed_byte_and_short_fragment_boundary_advances_isaac_once() {
    // Java Protocol lengths:125 fixed1,63 g1,226 g2. Runtime effects are also checked.
    let frames = [
        vec![125, 73],
        vec![63, 3, b'h', b'i', 10],
        vec![226, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
    ];
    for frame in frames {
        for split in 1..frame.len() {
            let (stream, mut server) = socket_pair();
            let mut c = client(ClientRevision::R377, 0);
            c.stream = Some(stream);
            c.ingame = true;
            c.ptype = -1;
            let mut cipher = Isaac::new(&[51, 52, 53, 54]);
            let mut wire = frame.clone();
            wire[0] = wire[0].wrapping_add(cipher.next_int() as u8);
            c.random_in = Some(Isaac::new(&[51, 52, 53, 54]));
            server.write_all(&wire[..split]).unwrap();
            buffered(&mut c, split as i32);
            assert!(!c.tcp_in(), "premature dispatch at split {split}");
            assert_eq!(c.ptype, i32::from(frame[0]));
            server.write_all(&wire[split..]).unwrap();
            buffered(&mut c, (wire.len() - split) as i32);
            let deadline = Instant::now() + Duration::from_secs(3);
            while !c.tcp_in() {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            assert!(c.ingame);
            assert_eq!(c.ptype, -1);
            assert_eq!(c.random_in.as_mut().unwrap().next_int(), cipher.next_int());
            if frame[0] == 125 {
                assert_eq!(c.runenergy, 73);
            }
            if frame[0] == 63 {
                assert_eq!(c.chat_text[0], "hi");
            }
            if frame[0] == 226 {
                assert_eq!(c.ignore_count, 1);
                assert_eq!(c.ignore_userhash[0], 1);
            }
        }
    }
}

fn profiled_client(port: u16) -> Client {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let host = Ipv4Addr::LOCALHOST.to_string();
    let profile = Arc::new(
        ClientSessionProfile::new(ClientSessionConfig {
            revision: ClientRevision::R377,
            transport: Transport::Tcp,
            game_host: host.clone(),
            game_port: port,
            asset_host: host,
            asset_port: port,
            cache_dir: root.clone(),
            unpack_dir: root,
            rsa_modulus: ((BigUint::from(1u32) << 512usize) - BigUint::from(1u32)).to_string(),
            rsa_exponent: "1".into(),
            expected_crc: None,
            content_id: "synthetic-377-wire".into(),
            file_store_dir: None,
            ondemand_persist_dir: None,
            map_archive: None,
        })
        .unwrap(),
    );
    Client::from_shared_with_profile(
        profile.client_config(true, false),
        Arc::new(Cache::default()),
        Arc::new(Vec::new()),
        Arc::new(Vec::new()),
        profile,
    )
    .unwrap()
}

#[test]
fn real_login_checks_all_crcs_seed_directions_and_partial_frame_reconnect() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let server = thread::spawn(move || {
        for reconnect in [false, true] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut hello = [0; 2];
            socket.read_exact(&mut hello).unwrap();
            assert_eq!(hello, [14, 6]);
            for byte in [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0x12, 0x34, 0x56, 0x78, 0xff, 0xff, 0xff, 0xff,
            ] {
                socket.write_all(&[byte]).unwrap();
            }
            let mut head = [0; 2];
            socket.read_exact(&mut head).unwrap();
            assert_eq!(head[0], if reconnect { 18 } else { 16 });
            let mut body = vec![0; usize::from(head[1])];
            socket.read_exact(&mut body).unwrap();
            assert_eq!(&body[..4], &[255, 1, 121, 0]);
            for index in 0..9 {
                assert_eq!(
                    &body[4 + 4 * index..8 + 4 * index],
                    &(0x1020_3040i32 + index as i32).to_be_bytes()
                );
            }
            let rsa_len = usize::from(body[40]);
            assert_eq!(body.len(), 41 + rsa_len);
            let plain = &body[41..];
            assert_eq!(plain[0], 10);
            assert_eq!(
                &plain[9..17],
                &[0x12, 0x34, 0x56, 0x78, 0xff, 0xff, 0xff, 0xff]
            );
            assert_eq!(&plain[17..], b"vT2\x10alice\ntest\n");
            let seed: [i32; 4] = std::array::from_fn(|i| {
                i32::from_be_bytes(plain[1 + i * 4..5 + i * 4].try_into().unwrap())
            });
            tx.send(seed).unwrap();
            socket
                .write_all(if reconnect { &[15][..] } else { &[2, 2, 1][..] })
                .unwrap();
            let mut cipher = Isaac::new(&seed.map(|word| word.wrapping_add(50)));
            if reconnect {
                socket
                    .write_all(&[125u8.wrapping_add(cipher.next_int() as u8), 91])
                    .unwrap();
            } else {
                // Only the opcode and first short-length byte of an old frame arrive.
                socket
                    .write_all(&[226u8.wrapping_add(cipher.next_int() as u8), 0])
                    .unwrap();
            }
            resume.recv_timeout(Duration::from_secs(3)).unwrap();
        }
    });
    let mut c = profiled_client(port);
    c.login_uid = 0x7654_3210;
    c.jag_checksum = std::array::from_fn(|i| 0x1020_3040 + i as i32);
    for reconnect in [false, true] {
        c.set_login_deadline(Instant::now() + Duration::from_secs(3));
        c.login("alice", "test", reconnect).unwrap();
        let seed = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            c.out.random.as_mut().unwrap().next_int(),
            Isaac::new(&seed).next_int()
        );
        assert_eq!(c.ptype, -1);
        assert_eq!(c.psize, 0);
        buffered(&mut c, 2);
        if reconnect {
            assert!(c.tcp_in());
            assert_eq!(c.runenergy, 91);
        } else {
            assert_eq!(c.staffmodlevel, 2);
            assert!(c.mouse_tracked);
            assert!(!c.tcp_in());
            assert_eq!(c.ptype, 226);
            assert_eq!(c.psize, -2);
        }
        release.send(()).unwrap();
    }
    server.join().unwrap();
}

#[test]
fn eof_before_hello_and_after_seed_stays_a_transport_failure() {
    for after_seed in [false, true] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut hello = [0; 2];
            socket.read_exact(&mut hello).unwrap();
            if after_seed {
                socket.write_all(&[0; 17]).unwrap();
                let mut head = [0; 2];
                socket.read_exact(&mut head).unwrap();
                let mut body = vec![0; usize::from(head[1])];
                socket.read_exact(&mut body).unwrap();
            }
        });
        let mut c = client(ClientRevision::R377, port);
        c.set_external_reconnect_owner(true);
        c.set_login_deadline(Instant::now() + Duration::from_secs(3));
        let error = c.login("alice", "test", false).unwrap_err();
        assert_eq!(error.code, -1);
        assert!(error.transport.is_some());
        assert!(error.retry_after.is_none());
        assert!(!c.ingame);
        server.join().unwrap();
    }
}
