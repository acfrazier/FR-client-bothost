//! `Client::from_shared` injects a process-wide `Arc<Cache>` + iface
//! template without a second unpack or `/crc` probe, and `maininit`
//! short-circuits its `load_cache` re-unpack on the injected cache
//! (`cache_from_shared`). `Client::new` (marker false) still re-unpacks
//! after its fresh jag fetch — that unpack is intentional and must not be
//! skipped. The temp caches have no real packs, so the HTTP listener
//! planted here is the only network these tests touch.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use client::client::{Client, ClientConfig};
use client::config::{Cache, IfType, IfTypeMut, ObjType};
use client::io::Packet;

fn cfg(dir: &std::path::Path) -> ClientConfig {
    ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_str().unwrap().into(),
        members: true,
        lowmem: false,
    }
}

/// Read the full HTTP request (headers) before responding: closing with
/// unread data in the receive buffer sends RST (not FIN) on macOS,
/// discarding the response the client is waiting for.
fn drain_request(s: &mut std::net::TcpStream) -> String {
    let mut req = Vec::new();
    let mut buf = [0u8; 1024];
    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => req.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&req).to_string()
}

fn respond(s: &mut std::net::TcpStream, body: &[u8]) {
    let resp = [
        b"HTTP/1.0 200 OK\r\nContent-Length: ".as_slice(),
        body.len().to_string().as_bytes(),
        b"\r\n\r\n",
        body,
    ]
    .concat();
    let _ = s.write_all(&resp);
}

/// One-shot HTTP/1.0 server: replies to the single `/crc` request and
/// closes. All seeded jags are CRC hits, so nothing else is fetched.
fn serve_once(body: Vec<u8>) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        drain_request(&mut s);
        respond(&mut s, &body);
    });
    (port, h)
}

/// The 9×g4 + hash body `get_jag_checksums` expects from `/crc`.
fn crc_body(checksums: &[i32; 9]) -> Vec<u8> {
    let mut body = Packet::alloc(0);
    for &c in checksums {
        body.p4(c);
    }
    let mut h = 1234i32;
    for &c in checksums {
        h = h.wrapping_shl(1).wrapping_add(c);
    }
    body.p4(h);
    body.data()[..body.pos].to_vec()
}

/// Seed all 8 jags so `maininit`'s jag fetch is a full CRC hit (no HTTP
/// past `/crc`). `config_bytes` is the on-disk `config` (a valid empty jag
/// for `Client::new`, garbage for the injected-cache test, where a
/// re-unpack must not run at all).
fn seed_jags(dir: &std::path::Path, config_bytes: &[u8], checksums: &mut [i32; 9]) {
    let names = [
        "title",
        "config",
        "interface",
        "media",
        "versionlist",
        "textures",
        "wordenc",
        "sounds",
    ];
    for (i, name) in names.iter().enumerate() {
        let bytes: Vec<u8> = if *name == "config" {
            config_bytes.to_vec()
        } else {
            format!("{name}-seed").into_bytes()
        };
        std::fs::write(dir.join(name), &bytes).unwrap();
        checksums[i + 1] = Packet::getcrc(&bytes, 0, bytes.len());
    }
}

/// The injected cache must survive `maininit`: `from_shared` unpacks
/// nothing, and the guard keys on `cache_from_shared` so the fresh jag
/// fetch never throws the shared `Arc<Cache>` away. The on-disk `config`
/// is garbage — a re-unpack would `Err` and set `errorLoading`, so a clean
/// run proves the `load_cache` step was skipped.
#[test]
fn from_shared_maininit_keeps_injected_cache_and_skips_reunpack() {
    let dir = std::env::temp_dir().join("274-from-shared");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut checksums = [0i32; 9];
    seed_jags(&dir, b"garbage-config", &mut checksums);

    let mut tables = Cache::default();
    tables.objs.resize(1, ObjType::default());
    tables.objs[0].name = "Coins".into();
    let injected = Arc::new(tables);

    let (port, th) = serve_once(crc_body(&checksums));
    let mut c = Client::from_shared(
        cfg(&dir),
        Arc::clone(&injected),
        Arc::new(vec![Some(Box::new(IfType {
            id: 42,
            ..IfType::default()
        }))]),
        vec![None, Some(Arc::new(IfTypeMut::default()))],
    );
    c.http_port = port;
    c.fetch_retry_wait = Duration::from_millis(1);
    assert!(c.cache_from_shared, "from_shared must arm the marker");
    assert!(Arc::ptr_eq(&c.cache, &injected));

    c.maininit();
    th.join().ok();

    assert_eq!(c.last_progress_percent, 100);
    assert!(
        !c.error_loading,
        "a re-unpack of the garbage on-disk config would have set errorLoading"
    );
    assert!(c.cache_from_shared, "the marker must survive maininit");
    assert!(
        Arc::ptr_eq(&c.cache, &injected),
        "maininit must not replace the injected Arc<Cache>"
    );
    assert_eq!(c.cache.obj(0).name, "Coins", "injected tables stay live");
    assert_eq!(
        c.if_(0).unwrap().id,
        42,
        "injected iface template stays per-client"
    );
}

/// `Client::new` keeps the marker false, so `maininit` still re-unpacks
/// after its fresh jag fetch: the constructor's cache (possibly stale
/// files, unpacked for immediate display) is replaced by a fresh
/// `Arc<Cache>`, not preserved.
#[test]
fn new_maininit_still_reunpacks_fresh_cache() {
    let dir = std::env::temp_dir().join("274-new-reunpack");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A valid (empty) config jag: both `new`'s unpack and `maininit`'s
    // re-unpack succeed, so the only observable difference is the fresh Arc.
    let mut checksums = [0i32; 9];
    seed_jags(&dir, &[0u8, 0, 6, 0, 0, 6, 0, 0], &mut checksums);

    let (port, th) = serve_once(crc_body(&checksums));
    let mut c = Client::new(cfg(&dir));
    c.http_port = port;
    c.fetch_retry_wait = Duration::from_millis(1);
    assert!(
        !c.cache_from_shared,
        "Client::new must leave the marker false"
    );
    let unpacked = Arc::clone(&c.cache);

    c.maininit();
    th.join().ok();

    assert_eq!(c.last_progress_percent, 100);
    assert!(!c.cache_from_shared);
    assert!(
        !Arc::ptr_eq(&c.cache, &unpacked),
        "Client::new + maininit must re-unpack a fresh Arc<Cache>"
    );
}
