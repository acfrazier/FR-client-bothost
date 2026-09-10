use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;

use client::client::{Client, ClientRevision};
use client::config::{Cache, IfType, IfTypeMut};
use client::io::{ClientStream, JagFile, OnDemand, Packet};
use client::{BotTarget, ClientSessionConfig, ClientSessionProfile};

const EMPTY_JAG: &[u8] = &[0, 0, 6, 0, 0, 6, 0, 0];

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "274-session-profile-{name}-{}-{:?}",
        std::process::id(),
        thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn profile_config(cache_dir: PathBuf, game_port: u16, asset_port: u16) -> ClientSessionConfig {
    ClientSessionConfig {
        revision: ClientRevision::R289,
        target: BotTarget::Local,
        game_host: "127.0.0.1".into(),
        game_port,
        asset_host: "127.0.0.1".into(),
        asset_port,
        cache_dir,
        unpack_dir: temp_dir("unpack"),
        // Exponent 1 and an intentionally oversized modulus make the test
        // login RSA block observable without weakening production parsing.
        rsa_modulus: format!("1{}", "0".repeat(300)),
        rsa_exponent: "1".into(),
        expected_crc: None,
        content_id: "fixture-289-a".into(),
    }
}

fn shared_client(profile: Arc<ClientSessionProfile>) -> Result<Client, String> {
    let config = profile.client_config(true, false);
    Client::from_shared_with_profile(
        config,
        Arc::new(Cache::default()),
        Arc::new(Vec::<Option<Box<IfType>>>::new()),
        Arc::new(Vec::<Option<Arc<IfTypeMut>>>::new()),
        profile,
    )
}

fn drain_request(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buf).unwrap();
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buf[..read]);
    }
    String::from_utf8(request).unwrap()
}

fn crc_body(checksums: &[i32; 9]) -> Vec<u8> {
    let mut body = Packet::alloc(1);
    for checksum in checksums {
        body.p4(*checksum);
    }
    let mut hash = 1234i32;
    for checksum in checksums {
        hash = hash.wrapping_shl(1).wrapping_add(*checksum);
    }
    body.p4(hash);
    body.data()[..body.pos].to_vec()
}

fn serve_http_once(body: Vec<u8>) -> (u16, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = drain_request(&mut stream);
        write!(
            stream,
            "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        request
    });
    (port, handle)
}

fn read_login_packet(stream: &mut TcpStream, expected_wrapper: u8, grant: &[u8]) -> Vec<u8> {
    let mut probe = [0u8; 2];
    stream.read_exact(&mut probe).unwrap();
    assert_eq!(probe[0], 14);
    stream.write_all(&[0; 8]).unwrap();
    stream.write_all(&[0]).unwrap();
    stream.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap();
    let mut prefix = [0u8; 2];
    stream.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix[0], expected_wrapper);
    let mut rest = vec![0u8; prefix[1] as usize];
    stream.read_exact(&mut rest).unwrap();
    let mut packet = prefix.to_vec();
    packet.extend(rest);
    stream.write_all(grant).unwrap();
    packet
}

fn write_empty_cache(dir: &Path) -> [i32; 9] {
    let mut checksums = [0i32; 9];
    for (index, name) in [
        "title",
        "config",
        "interface",
        "media",
        "versionlist",
        "textures",
        "wordenc",
        "sounds",
    ]
    .iter()
    .enumerate()
    {
        let bytes = if *name == "versionlist" {
            tiny_versionlist_bytes(0)
        } else {
            EMPTY_JAG.to_vec()
        };
        std::fs::write(dir.join(name), &bytes).unwrap();
        checksums[index + 1] = Packet::getcrc(&bytes, 0, bytes.len());
    }
    checksums
}

#[test]
fn profile_validates_and_exposes_frozen_owned_inputs() {
    let cache = temp_dir("getters");
    let config = profile_config(cache.clone(), 44594, 1080);
    let profile = ClientSessionProfile::new(config).unwrap();

    assert_eq!(profile.revision(), ClientRevision::R289);
    assert_eq!(profile.target(), BotTarget::Local);
    assert_eq!(profile.game_host(), "127.0.0.1");
    assert_eq!(profile.game_port(), 44594);
    assert_eq!(profile.asset_host(), "127.0.0.1");
    assert_eq!(profile.asset_port(), 1080);
    assert_eq!(profile.cache_dir(), cache.as_path());
    assert!(profile
        .unpack_dir()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("274-session-profile-unpack-"));
    assert_eq!(profile.rsa_exponent(), "1");
    assert_eq!(profile.expected_crc(), None);
    assert_eq!(profile.content_id(), "fixture-289-a");

    let client_config = profile.client_config(false, true);
    assert_eq!(client_config.host, "127.0.0.1");
    assert_eq!(client_config.port, 44594);
    assert_eq!(client_config.cache_dir, cache.to_str().unwrap());
    assert!(!client_config.members);
    assert!(client_config.lowmem);
}

#[test]
fn profile_rejects_bad_explicit_rsa_and_empty_identity_fields() {
    let mut bad_rsa = profile_config(temp_dir("bad-rsa"), 44594, 1080);
    bad_rsa.rsa_modulus = "not-decimal".into();
    assert!(ClientSessionProfile::new(bad_rsa)
        .unwrap_err()
        .contains("RSA modulus"));

    let mut empty_content = profile_config(temp_dir("empty-content"), 44594, 1080);
    empty_content.content_id.clear();
    assert!(ClientSessionProfile::new(empty_content)
        .unwrap_err()
        .contains("content_id"));
}

#[test]
fn bound_constructor_rejects_redundant_config_before_starting_ondemand() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let cache = temp_dir("pre-effect");
    std::fs::write(cache.join("versionlist"), tiny_versionlist_bytes(1)).unwrap();
    let profile = Arc::new(ClientSessionProfile::new(profile_config(cache, port, 1080)).unwrap());
    let mut config = profile.client_config(true, false);
    config.port = port.wrapping_add(1);

    let result = Client::from_shared_with_profile(
        config,
        Arc::new(Cache::default()),
        Arc::new(Vec::new()),
        Arc::new(Vec::new()),
        profile,
    );
    assert!(result.is_err());
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", port), 0);
    drop(listener);
}

#[test]
fn bound_login_and_reconnect_ignore_mutable_config_and_use_frozen_revision_rsa() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        tx.send(read_login_packet(&mut first, 16, &[2, 0, 0]))
            .unwrap();
        let (mut second, _) = listener.accept().unwrap();
        tx.send(read_login_packet(&mut second, 18, &[15])).unwrap();
    });

    let profile =
        Arc::new(ClientSessionProfile::new(profile_config(temp_dir("login"), port, 1080)).unwrap());
    let mut client = shared_client(Arc::clone(&profile)).unwrap();
    assert!(Arc::ptr_eq(client.session_profile().unwrap(), &profile));
    client.config.host = "192.0.2.1".into();
    client.config.port = 1;
    client.config.cache_dir = "/definitely/not/the/profile/cache".into();
    client.http_port = 1;

    client.login("bound-user", "pw", false).unwrap();
    client.login("bound-user", "pw", true).unwrap();
    server.join().unwrap();

    for (packet, wrapper) in [rx.recv().unwrap(), rx.recv().unwrap()]
        .into_iter()
        .zip([16, 18])
    {
        assert_eq!(packet[0], wrapper);
        assert_eq!(u16::from_be_bytes([packet[3], packet[4]]), 289);
        let rsa_len = packet[42] as usize;
        let rsa = &packet[43..43 + rsa_len];
        assert_eq!(
            rsa[0], 10,
            "exponent-one profile RSA must reveal login opcode"
        );
        assert!(
            rsa.windows("bound-user".len())
                .any(|window| window == b"bound-user"),
            "login must use the frozen explicit RSA pair"
        );
    }
}

#[test]
fn bound_maininit_uses_asset_endpoint_cache_and_expected_crc() {
    let cache = temp_dir("asset");
    let checksums = write_empty_cache(&cache);
    let (asset_port, server) = serve_http_once(crc_body(&checksums));
    let game_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let game_port = game_listener.local_addr().unwrap().port();
    let mut config = profile_config(cache.clone(), game_port, asset_port);
    config.expected_crc = Some(checksums);
    let profile = Arc::new(ClientSessionProfile::new(config).unwrap());
    let mut client = shared_client(profile).unwrap();

    client.config.host = "192.0.2.1".into();
    client.config.cache_dir = "/definitely/not/the/profile/cache".into();
    client.http_port = 1;
    client.fetch_retry_wait = std::time::Duration::from_millis(1);
    client.maininit();

    let request = server.join().unwrap();
    assert!(request.starts_with("GET /crc HTTP/1.0"));
    assert!(!client.error_loading);
    assert_eq!(client.last_progress_percent, 100);
    drop(game_listener);
}

#[test]
fn bound_crc_mismatch_fails_without_replacing_frozen_identity() {
    let cache = temp_dir("crc-mismatch");
    let expected = write_empty_cache(&cache);
    let mut changed = expected;
    changed[1] = changed[1].wrapping_add(1);
    let (asset_port, server) = serve_http_once(crc_body(&changed));
    let game_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let game_port = game_listener.local_addr().unwrap().port();
    let mut config = profile_config(cache, game_port, asset_port);
    config.expected_crc = Some(expected);
    let profile = Arc::new(ClientSessionProfile::new(config).unwrap());
    let mut client = shared_client(profile).unwrap();
    client.fetch_retry_wait = std::time::Duration::from_millis(1);

    client.maininit();
    server.join().unwrap();
    assert!(client.error_loading);
    assert_eq!(client.jag_checksum, expected);
    assert_eq!(client.last_progress_message, "Cache identity mismatch");
    drop(game_listener);
}

#[test]
fn incompatible_bound_adoption_leaves_both_streams_intact() {
    let first_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let first_port = first_listener.local_addr().unwrap().port();
    let second_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let second_port = second_listener.local_addr().unwrap().port();
    let first_accept = thread::spawn(move || first_listener.accept().unwrap().0);
    let second_accept = thread::spawn(move || second_listener.accept().unwrap().0);

    let first_profile = Arc::new(
        ClientSessionProfile::new(profile_config(temp_dir("adopt-a"), first_port, 1080)).unwrap(),
    );
    let mut second_config = profile_config(temp_dir("adopt-b"), second_port, 1080);
    second_config.content_id = "fixture-289-b".into();
    let second_profile = Arc::new(ClientSessionProfile::new(second_config).unwrap());
    let mut first = shared_client(first_profile).unwrap();
    let mut second = shared_client(second_profile).unwrap();
    first.stream =
        Some(ClientStream::connect_for(BotTarget::Local, "127.0.0.1", first_port).unwrap());
    second.stream =
        Some(ClientStream::connect_for(BotTarget::Local, "127.0.0.1", second_port).unwrap());
    let first_server = first_accept.join().unwrap();
    let second_server = second_accept.join().unwrap();

    assert!(second.adopt_from(&mut first).is_none());
    assert!(first.stream.is_some());
    assert!(second.stream.is_some());
    drop(first_server);
    drop(second_server);
}

#[test]
fn matching_bound_ondemand_shares_and_last_subscriber_stops_worker() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let cache = temp_dir("ondemand-share");
    std::fs::write(cache.join("versionlist"), tiny_versionlist_bytes(1)).unwrap();
    let profile = Arc::new(ClientSessionProfile::new(profile_config(cache, port, 1080)).unwrap());

    let first = shared_client(Arc::clone(&profile)).unwrap();
    let second = shared_client(Arc::clone(&profile)).unwrap();
    assert!(Arc::ptr_eq(first.session_profile().unwrap(), &profile));
    assert!(Arc::ptr_eq(second.session_profile().unwrap(), &profile));
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", port), 1);
    drop(first);
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", port), 1);
    drop(second);
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", port), 0);
    drop(listener);
}

#[test]
fn bound_ondemand_rejects_declared_and_actual_identity_mismatches() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let first_cache = temp_dir("ondemand-first");
    std::fs::write(first_cache.join("versionlist"), tiny_versionlist_bytes(1)).unwrap();
    let first_profile =
        Arc::new(ClientSessionProfile::new(profile_config(first_cache, port, 1080)).unwrap());
    let first = shared_client(first_profile).unwrap();

    let declared_cache = temp_dir("ondemand-declared");
    std::fs::write(
        declared_cache.join("versionlist"),
        tiny_versionlist_bytes(1),
    )
    .unwrap();
    let mut declared_config = profile_config(declared_cache, port, 1080);
    declared_config.content_id = "different-content".into();
    let declared_profile = Arc::new(ClientSessionProfile::new(declared_config).unwrap());
    assert!(shared_client(declared_profile)
        .err()
        .expect("declared mismatch must fail")
        .contains("OnDemand identity mismatch"));

    let actual_cache = temp_dir("ondemand-actual");
    std::fs::write(actual_cache.join("versionlist"), tiny_versionlist_bytes(2)).unwrap();
    let actual_profile =
        Arc::new(ClientSessionProfile::new(profile_config(actual_cache, port, 1080)).unwrap());
    assert!(shared_client(actual_profile)
        .err()
        .expect("actual mismatch must fail")
        .contains("OnDemand identity mismatch"));
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", port), 1);
    drop(first);
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", port), 0);
    drop(listener);
}

fn tiny_versionlist_bytes(version: u16) -> Vec<u8> {
    let version = version.to_be_bytes();
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("model_version", version.to_vec()),
        ("anim_version", version.to_vec()),
        ("midi_version", version.to_vec()),
        ("map_version", version.to_vec()),
        ("model_crc", vec![0, 0, 0, 0]),
        ("anim_crc", vec![0, 0, 0, 0]),
        ("midi_crc", vec![0, 0, 0, 0]),
        ("map_crc", vec![0, 0, 0, 0]),
        ("model_index", vec![0]),
    ];
    let entries: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, data)| (*name, data.as_slice()))
        .collect();
    jag(&entries)
}

fn jag(files: &[(&str, &[u8])]) -> Vec<u8> {
    let packed: Vec<Vec<u8>> = files.iter().map(|(_, data)| bz2(data)).collect();
    let data_len: usize = packed.iter().map(Vec::len).sum();
    let total = (8 + 10 * files.len() + data_len) as i32;
    let mut output = Vec::new();
    g3(&mut output, total);
    g3(&mut output, total);
    output.extend_from_slice(&(files.len() as u16).to_be_bytes());
    for ((name, data), packed_data) in files.iter().zip(&packed) {
        output.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        g3(&mut output, data.len() as i32);
        g3(&mut output, packed_data.len() as i32);
    }
    for data in packed {
        output.extend_from_slice(&data);
    }
    output
}

fn bz2(data: &[u8]) -> Vec<u8> {
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    encoder.write_all(data).unwrap();
    let output = encoder.finish().unwrap();
    output[4..].to_vec()
}

fn g3(output: &mut Vec<u8>, value: i32) {
    output.push((value >> 16) as u8);
    output.push((value >> 8) as u8);
    output.push(value as u8);
}
