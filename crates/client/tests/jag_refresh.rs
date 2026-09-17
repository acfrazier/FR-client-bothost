//! Pre-freeze jag refresh: missing or CRC-mismatched packs are materialized
//! into an explicit dest directory. Source bytes are never written.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use client::io::Packet;
use client::unpack::{refresh_jags, FetchEndpoint};
use client::BotTarget;

const JAG_SLOTS: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn tmp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "274bot-jag-refresh-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write_pack(dir: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(dir.join(name), bytes).unwrap();
}

fn crc_body(packs: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut checksums = [0i32; 9];
    for (name, bytes) in packs {
        let slot = JAG_SLOTS
            .iter()
            .position(|candidate| candidate == name)
            .unwrap()
            + 1;
        checksums[slot] = Packet::getcrc(bytes, 0, bytes.len());
    }
    let mut body = Packet::alloc(0);
    for &checksum in &checksums {
        body.p4(checksum);
    }
    let mut hash = 1234i32;
    for &checksum in &checksums {
        hash = hash.wrapping_shl(1).wrapping_add(checksum);
    }
    body.p4(hash);
    body.data()[..body.pos].to_vec()
}

fn read_http_request(sock: &mut std::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match sock.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&request).to_string()
}

fn respond(sock: &mut std::net::TcpStream, body: &[u8]) {
    let response = [
        b"HTTP/1.0 200 OK\r\nContent-Length: ".as_slice(),
        body.len().to_string().as_bytes(),
        b"\r\n\r\n",
        body,
    ]
    .concat();
    let _ = sock.write_all(&response);
}

/// Serve `/crc` forever and each pack file once. Returns fetched names.
fn serve_packs(
    packs: Arc<Vec<(String, Vec<u8>)>>,
    max_file_serves: usize,
) -> (u16, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut served = Vec::new();
        let mut got_crc = false;
        while Instant::now() < deadline && (served.len() < max_file_serves || !got_crc) {
            let (mut sock, _) = match listener.accept() {
                Ok(conn) => conn,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("accept: {e}"),
            };
            sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let request = read_http_request(&mut sock);
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            if path == "/crc" {
                respond(&mut sock, &crc_body(&packs));
                got_crc = true;
                continue;
            }
            let name = packs
                .iter()
                .map(|(name, _)| name.clone())
                .find(|name| path.starts_with(&format!("/{name}")))
                .unwrap_or_else(|| panic!("unexpected path {path}"));
            let bytes = packs
                .iter()
                .find(|(pack, _)| *pack == name)
                .map(|(_, bytes)| bytes.clone())
                .unwrap();
            respond(&mut sock, &bytes);
            served.push(name);
        }
        served
    });
    (port, handle)
}

fn fixture_packs(versionlist: &[u8]) -> Vec<(String, Vec<u8>)> {
    JAG_SLOTS
        .into_iter()
        .map(|name| {
            let bytes = if name == "versionlist" {
                versionlist.to_vec()
            } else {
                format!("{name}-bytes").into_bytes()
            };
            (name.to_string(), bytes)
        })
        .collect()
}

fn write_source(dir: &Path, packs: &[(String, Vec<u8>)]) {
    for (name, bytes) in packs {
        write_pack(dir, name, bytes);
    }
}

#[test]
fn mismatched_versionlist_is_refreshed_into_dest_without_writing_source() {
    let source = tmp("source");
    let dest = tmp("dest");
    let local = fixture_packs(b"local-versionlist");
    let server = fixture_packs(b"server-versionlist");
    write_source(&source, &local);
    let source_before: Vec<u8> = std::fs::read(source.join("versionlist")).unwrap();

    let packs = Arc::new(server.clone());
    let (port, handle) = serve_packs(Arc::clone(&packs), 1);
    let refreshed = refresh_jags(
        &[&source],
        &dest,
        FetchEndpoint {
            target: BotTarget::Local,
            host: "127.0.0.1",
            port,
        },
    )
    .expect("refresh");
    let served = handle.join().unwrap();

    assert_eq!(served, vec!["versionlist".to_string()]);
    assert_eq!(refreshed.fetched, vec!["versionlist".to_string()]);
    assert!(refreshed.reused.iter().all(|name| name != "versionlist"));
    assert_eq!(refreshed.reused.len(), 7);

    assert_eq!(
        std::fs::read(source.join("versionlist")).unwrap(),
        source_before,
        "operator/source versionlist must be byte-identical"
    );
    for (name, bytes) in &local {
        if name == "versionlist" {
            continue;
        }
        assert_eq!(
            std::fs::read(source.join(name)).unwrap(),
            *bytes,
            "source {name} unchanged"
        );
    }
    assert_eq!(
        std::fs::read(dest.join("versionlist")).unwrap(),
        b"server-versionlist"
    );
    for name in [
        "title",
        "config",
        "interface",
        "media",
        "textures",
        "wordenc",
        "sounds",
    ] {
        assert_eq!(
            std::fs::read(dest.join(name)).unwrap(),
            std::fs::read(source.join(name)).unwrap()
        );
    }

    let _ = std::fs::remove_dir_all(&source);
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn matching_files_are_reused_and_not_fetched() {
    let source = tmp("match-source");
    let dest = tmp("match-dest");
    let packs = fixture_packs(b"same-versionlist");
    write_source(&source, &packs);
    let served_packs = Arc::new(packs);
    let (port, handle) = serve_packs(Arc::clone(&served_packs), 0);
    let refreshed = refresh_jags(
        &[&source],
        &dest,
        FetchEndpoint {
            target: BotTarget::Local,
            host: "127.0.0.1",
            port,
        },
    )
    .expect("refresh");
    assert!(handle.join().unwrap().is_empty());

    assert!(refreshed.fetched.is_empty());
    assert_eq!(refreshed.reused.len(), 8);
    for name in JAG_SLOTS {
        assert_eq!(
            std::fs::read(dest.join(name)).unwrap(),
            std::fs::read(source.join(name)).unwrap()
        );
    }
    let _ = std::fs::remove_dir_all(&source);
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn source_directory_cannot_be_the_destination() {
    let source = tmp("same-directory");
    let local = fixture_packs(b"local-versionlist");
    write_source(&source, &local);
    let err = refresh_jags(
        &[&source],
        &source,
        FetchEndpoint {
            target: BotTarget::Local,
            host: "127.0.0.1",
            port: 1,
        },
    )
    .expect_err("must refuse before networking");
    assert!(err.to_string().contains("source"), "{err}");
    for (name, bytes) in local {
        assert_eq!(std::fs::read(source.join(name)).unwrap(), bytes);
    }
    std::fs::remove_dir_all(source).unwrap();
}

#[test]
fn download_must_be_persisted_before_refresh_succeeds() {
    let dest = tmp("persist-failure");
    std::fs::create_dir(dest.join("title")).unwrap();
    let packs = Arc::new(fixture_packs(b"wanted"));
    let (port, server) = serve_packs(packs, 1);
    let result = refresh_jags(
        &[],
        &dest,
        FetchEndpoint {
            target: BotTarget::Local,
            host: "127.0.0.1",
            port,
        },
    );
    assert!(result.is_err(), "an unwritable title must fail");
    assert!(result.unwrap_err().to_string().contains("title"));
    assert_eq!(server.join().unwrap(), vec!["title"]);
    std::fs::remove_dir_all(dest).unwrap();
}

#[test]
fn corrupt_download_fails_closed() {
    let dest = tmp("corrupt-dest");
    let packs = Arc::new(fixture_packs(b"wanted"));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if Instant::now() > deadline {
                break;
            }
            let (mut sock, _) = match listener.accept() {
                Ok(conn) => conn,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };
            let request = read_http_request(&mut sock);
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            if path == "/crc" {
                respond(&mut sock, &crc_body(&packs));
            } else {
                respond(&mut sock, b"not-the-crc-payload");
            }
        }
    });

    let err = refresh_jags(
        &[],
        &dest,
        FetchEndpoint {
            target: BotTarget::Local,
            host: "127.0.0.1",
            port,
        },
    )
    .expect_err("corrupt body must fail");
    assert!(
        err.to_string().contains("CRC") || err.to_string().contains("fetch"),
        "got: {err}"
    );
    drop(server);
    let _ = std::fs::remove_dir_all(&dest);
}
