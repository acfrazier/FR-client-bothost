//! Region-load OnDemand contract: a matching local map store must not touch
//! the update socket; a mismatched identity falls back to the wire; a 289
//! engine that closes on the legacy keepalive must not leave outstanding
//! requests waiting for the resend timer.
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use client::client::{Client, ClientConfig};
use client::config::Cache;
use client::io::{ClientRevision, JagFile, OnDemand, Packet};
use client::render::Renderer;
use client::BotTarget;

const MAP_RAW: &[u8] = b"map-bytes";
const KEEPALIVE_IDLE: Duration = Duration::from_secs(18);
static NEXT_TMP: AtomicU64 = AtomicU64::new(1);

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "274bot-od-scene-{name}-{}-{}",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gz(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

fn g3(out: &mut Vec<u8>, value: i32) {
    out.push((value >> 16) as u8);
    out.push((value >> 8) as u8);
    out.push(value as u8);
}

fn bz2(data: &[u8]) -> Vec<u8> {
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    enc.write_all(data).unwrap();
    let out = enc.finish().unwrap();
    assert!(out.starts_with(b"BZh"));
    out[4..].to_vec()
}

fn jag(files: &[(&str, &[u8])]) -> Vec<u8> {
    let packed: Vec<Vec<u8>> = files.iter().map(|(_, d)| bz2(d)).collect();
    let data_len: usize = packed.iter().map(|d| d.len()).sum();
    let total = (8 + 10 * files.len() + data_len) as i32;
    let mut out = Vec::new();
    g3(&mut out, total);
    g3(&mut out, total);
    out.push((files.len() >> 8) as u8);
    out.push(files.len() as u8);
    for ((name, data), packed_data) in files.iter().zip(packed.iter()) {
        out.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        g3(&mut out, data.len() as i32);
        g3(&mut out, packed_data.len() as i32);
    }
    for d in &packed {
        out.extend_from_slice(d);
    }
    out
}

fn map_payload() -> (Vec<u8>, i32) {
    let mut payload = gz(MAP_RAW);
    let crc = Packet::getcrc(&payload, 0, payload.len());
    payload.extend_from_slice(&[0, 1]);
    (payload, crc)
}

fn versionlist_bytes(crc: i32) -> Vec<u8> {
    jag(&[
        ("model_version", &[0, 1]),
        ("anim_version", &[0, 1]),
        ("midi_version", &[0, 1]),
        ("map_version", &[0, 1]),
        ("model_crc", &[0, 0, 0, 0]),
        ("anim_crc", &[0, 0, 0, 0]),
        ("midi_crc", &[0, 0, 0, 0]),
        ("map_crc", &crc.to_be_bytes()),
    ])
}

fn map_versionlist(crc: i32) -> JagFile {
    JagFile::new(versionlist_bytes(crc))
}

/// `main_file_cache` record for OnDemand archive 3 / idx 4, file 0.
fn write_map_store(dir: &Path, payload: &[u8]) {
    std::fs::create_dir_all(dir).unwrap();
    let sector = 1i32;
    let mut dat = vec![0u8; 520 * 2];
    let block = 520usize;
    dat[block..block + 2].copy_from_slice(&0u16.to_be_bytes());
    dat[block + 2..block + 4].copy_from_slice(&0u16.to_be_bytes());
    dat[block + 4..block + 7].copy_from_slice(&[0, 0, 0]);
    dat[block + 7] = 5; // idx 4 + 1
    dat[block + 8..block + 8 + payload.len()].copy_from_slice(payload);
    let size = payload.len() as u32;
    let mut idx = [0u8; 6];
    idx[0] = ((size >> 16) & 0xff) as u8;
    idx[1] = ((size >> 8) & 0xff) as u8;
    idx[2] = (size & 0xff) as u8;
    idx[3] = ((sector as u32 >> 16) & 0xff) as u8;
    idx[4] = ((sector as u32 >> 8) & 0xff) as u8;
    idx[5] = (sector as u32 & 0xff) as u8;
    std::fs::write(dir.join("main_file_cache.idx4"), idx).unwrap();
    std::fs::write(dir.join("main_file_cache.dat"), dat).unwrap();
}

fn handshake(sock: &mut TcpStream) {
    sock.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
    sock.set_write_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    let mut b = [0u8; 1];
    sock.read_exact(&mut b).unwrap();
    assert_eq!(b[0], 15, "ondemand handshake byte");
    sock.write_all(&[0; 8]).unwrap();
}

fn serve_map(sock: &mut TcpStream, body: &[u8]) {
    let mut req = [0u8; 4];
    sock.read_exact(&mut req).unwrap();
    assert_eq!(req[0], 3, "map archive");
    assert_eq!(u16::from_be_bytes([req[1], req[2]]), 0, "map file");
    let len = body.len() as u16;
    let mut chunk = vec![3, 0, 0, (len >> 8) as u8, len as u8, 0];
    chunk.extend_from_slice(body);
    sock.write_all(&chunk).unwrap();
}

fn wait_map(od: &mut OnDemand, deadline: Duration) -> Option<Vec<u8>> {
    let end = Instant::now() + deadline;
    loop {
        od.run(true);
        if let Some(req) = od.loop_request() {
            if req.archive == 3 && req.file == 0 {
                return req.data;
            }
        }
        if Instant::now() > end {
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// A CRC-valid local `main_file_cache` map must complete a region-style
/// archive-3 request without opening the ondemand socket.
#[test]
fn matching_local_map_store_issues_zero_ondemand_requests() {
    let _r = Renderer::new(false);
    let (payload, crc) = map_payload();
    let store = tmp("hit-store");
    write_map_store(&store, &payload);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_thread = Arc::clone(&accepts);
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(800);
        while Instant::now() < deadline {
            if listener.accept().is_ok() {
                accepts_thread.fetch_add(1, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    let versionlist = map_versionlist(crc);
    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        port,
        store.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.request(3, 0);
    let got = wait_map(&mut od, Duration::from_millis(800));
    drop(od);
    server.join().unwrap();
    assert_eq!(
        got.as_deref(),
        Some(MAP_RAW),
        "matching store must complete locally"
    );
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        0,
        "matching local map must not open the ondemand socket"
    );
}

/// Runtime staging's JAG directory has no `main_file_cache`. The separately
/// bound source store must still be used when its CRC matches.
#[test]
fn bound_file_store_used_when_jag_dir_has_no_store() {
    let _r = Renderer::new(false);
    let (payload, crc) = map_payload();
    let root = tmp("bound");
    let jag_dir = root.join("jags");
    let store = root.join("store");
    std::fs::create_dir_all(&jag_dir).unwrap();
    std::fs::write(jag_dir.join("versionlist"), b"unused").unwrap();
    write_map_store(&store, &payload);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_thread = Arc::clone(&accepts);
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(800);
        while Instant::now() < deadline {
            if listener.accept().is_ok() {
                accepts_thread.fetch_add(1, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    let versionlist = map_versionlist(crc);
    let persist = tmp("bound-persist");
    let mut od = OnDemand::new_bound(
        &versionlist,
        BotTarget::Local,
        ClientRevision::R289,
        "127.0.0.1",
        port,
        jag_dir.to_str().unwrap(),
        "scene-load-bound",
        Some(store.to_str().unwrap()),
        Some(persist.to_str().unwrap()),
    )
    .unwrap();
    od.request(3, 0);
    let got = wait_map(&mut od, Duration::from_millis(800));
    drop(od);
    server.join().unwrap();
    assert_eq!(
        got.as_deref(),
        Some(MAP_RAW),
        "bound source store must complete locally"
    );
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        0,
        "bound file store must not open the ondemand socket"
    );
    assert!(
        !persist.join("3").join("0").exists(),
        "local cache hits must not copy the store into persist"
    );
}

/// A map fetched over ondemand is retained under the content-identity overlay
/// so a later region load does not reopen the update socket.
#[test]
fn completed_ondemand_maps_are_retained() {
    let _r = Renderer::new(false);
    let (payload, crc) = map_payload();
    let persist = tmp("retain");
    let cache = tmp("retain-cache");
    let versionlist = map_versionlist(crc);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        handshake(&mut sock);
        serve_map(&mut sock, &payload);
    });

    let mut od = OnDemand::new_bound(
        &versionlist,
        BotTarget::Local,
        ClientRevision::R289,
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        "scene-load-retain",
        None,
        Some(persist.to_str().unwrap()),
    )
    .unwrap();
    od.request(3, 0);
    let first = wait_map(&mut od, Duration::from_secs(3));
    drop(od);
    server.join().unwrap();
    assert_eq!(first.as_deref(), Some(MAP_RAW));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_thread = Arc::clone(&accepts);
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(800);
        while Instant::now() < deadline {
            if listener.accept().is_ok() {
                accepts_thread.fetch_add(1, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    let mut od = OnDemand::new_bound(
        &versionlist,
        BotTarget::Local,
        ClientRevision::R289,
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        "scene-load-retain-2",
        None,
        Some(persist.to_str().unwrap()),
    )
    .unwrap();
    od.request(3, 0);
    let second = wait_map(&mut od, Duration::from_millis(800));
    drop(od);
    server.join().unwrap();
    assert_eq!(second.as_deref(), Some(MAP_RAW));
    assert_eq!(
        accepts.load(Ordering::Relaxed),
        0,
        "retained map must not reopen ondemand"
    );
}

/// CRC/version identity is authoritative: a present but foreign map still
/// goes over ondemand.
#[test]
fn mismatched_map_identity_falls_back_to_ondemand() {
    let _r = Renderer::new(false);
    let (good, good_crc) = map_payload();
    let mut foreign = gz(b"other-map");
    foreign.extend_from_slice(&[0, 1]);
    let root = tmp("miss");
    write_map_store(&root, &foreign);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        handshake(&mut sock);
        serve_map(&mut sock, &good);
    });

    let versionlist = map_versionlist(good_crc);
    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        port,
        root.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.request(3, 0);
    let got = wait_map(&mut od, Duration::from_secs(3));
    server.join().unwrap();
    assert_eq!(got.as_deref(), Some(MAP_RAW));
}

/// OnDemand.ts closes the client when `priority > 2`. Outstanding map
/// requests must reconnect and complete without waiting a 50-cycle resend.
#[test]
fn closed_ondemand_socket_reconnects_and_resends_immediately() {
    let _r = Renderer::new(false);
    let (payload, crc) = map_payload();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_s = Arc::clone(&accepts);
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        accepts_s.fetch_add(1, Ordering::Relaxed);
        handshake(&mut sock);
        let mut req = [0u8; 4];
        sock.read_exact(&mut req).unwrap();
        sock.shutdown(Shutdown::Both).ok();
        drop(sock);
        let (mut sock, _) = listener.accept().unwrap();
        accepts_s.fetch_add(1, Ordering::Relaxed);
        handshake(&mut sock);
        serve_map(&mut sock, &payload);
    });

    let cache = tmp("reconnect");
    let versionlist = map_versionlist(crc);
    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.request(3, 0);
    let started = Instant::now();
    let got = wait_map(&mut od, Duration::from_millis(800));
    let elapsed = started.elapsed();
    server.join().unwrap();
    assert_eq!(got.as_deref(), Some(MAP_RAW));
    assert!(
        elapsed < Duration::from_millis(800),
        "closed socket waited {elapsed:?}; must not sit out a ~1.2s resend"
    );
    assert_eq!(accepts.load(Ordering::Relaxed), 2);
}

fn bound_ondemand(
    revision: ClientRevision,
    port: u16,
    cache: &Path,
    persist: Option<&Path>,
    content_id: &str,
) -> OnDemand {
    let (_payload, crc) = map_payload();
    OnDemand::new_bound(
        &map_versionlist(crc),
        BotTarget::Local,
        revision,
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        content_id,
        None,
        persist.and_then(|p| p.to_str()),
    )
    .unwrap()
}

fn idle_run(od: &mut OnDemand, until: Instant, stop: Option<&AtomicBool>) {
    while Instant::now() < until && !stop.is_some_and(|s| s.load(Ordering::Relaxed)) {
        od.run(true);
        thread::sleep(Duration::from_millis(20));
    }
}

/// Engine OnDemand.ts: `archive > 3 || priority > 2` closes the socket.
/// The legacy Java keepalive is `00 00 00 0a`. A bound 289 worker must not
/// send it across a long idle; a later map request must complete on the
/// still-open connection without a resend wait.
#[test]
fn bound_revision_289_does_not_send_legacy_keepalive() {
    let _r = Renderer::new(false);
    let (payload, _crc) = map_payload();
    let body = payload.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let saw_legacy = Arc::new(AtomicBool::new(false));
    let saw_legacy_s = Arc::clone(&saw_legacy);
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        handshake(&mut sock);
        serve_map(&mut sock, &body);
        sock.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let idle_until = Instant::now() + KEEPALIVE_IDLE;
        while Instant::now() < idle_until {
            let mut extra = [0u8; 4];
            match sock.read_exact(&mut extra) {
                Ok(()) if extra == [0, 0, 0, 10] => {
                    saw_legacy_s.store(true, Ordering::Relaxed);
                    sock.shutdown(Shutdown::Both).ok();
                    return;
                }
                Ok(()) if extra[0] == 3 => {
                    let len = body.len() as u16;
                    let mut chunk = vec![3, 0, 0, (len >> 8) as u8, len as u8, 0];
                    chunk.extend_from_slice(&body);
                    sock.write_all(&chunk).unwrap();
                    return;
                }
                Ok(()) => {
                    sock.shutdown(Shutdown::Both).ok();
                    return;
                }
                Err(_) => {}
            }
        }
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        serve_map(&mut sock, &body);
    });

    let cache = tmp("ka-289");
    let mut od = bound_ondemand(
        ClientRevision::R289,
        port,
        &cache,
        None,
        "scene-load-ka-289",
    );
    od.request(3, 0);
    let first = wait_map(&mut od, Duration::from_secs(3));
    assert_eq!(first.as_deref(), Some(MAP_RAW));
    idle_run(&mut od, Instant::now() + KEEPALIVE_IDLE, Some(&saw_legacy));
    assert!(
        !saw_legacy.load(Ordering::Relaxed),
        "bound 289 must not send 00 00 00 0a; OnDemand.ts closes on priority 10"
    );
    od.request(3, 0);
    let started = Instant::now();
    let second = wait_map(&mut od, Duration::from_secs(3));
    server.join().unwrap();
    assert_eq!(second.as_deref(), Some(MAP_RAW));
    assert!(
        started.elapsed() < Duration::from_millis(800),
        "second map waited {:?}; keepalive must not have closed the socket",
        started.elapsed()
    );
}

/// Bound 274 still speaks the Java keepalive that 289 engines reject.
#[test]
fn bound_revision_274_sends_java_keepalive() {
    let _r = Renderer::new(false);
    let (payload, _crc) = map_payload();
    let body = payload.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let saw_legacy = Arc::new(AtomicBool::new(false));
    let saw_legacy_s = Arc::clone(&saw_legacy);
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        handshake(&mut sock);
        serve_map(&mut sock, &body);
        sock.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let idle_until = Instant::now() + KEEPALIVE_IDLE;
        while Instant::now() < idle_until {
            let mut extra = [0u8; 4];
            match sock.read_exact(&mut extra) {
                Ok(()) if extra == [0, 0, 0, 10] => {
                    saw_legacy_s.store(true, Ordering::Relaxed);
                    return;
                }
                Ok(()) => return,
                Err(_) => {}
            }
        }
    });

    let cache = tmp("ka-274");
    let mut od = bound_ondemand(
        ClientRevision::R274,
        port,
        &cache,
        None,
        "scene-load-ka-274",
    );
    od.request(3, 0);
    let first = wait_map(&mut od, Duration::from_secs(3));
    assert_eq!(first.as_deref(), Some(MAP_RAW));
    idle_run(&mut od, Instant::now() + KEEPALIVE_IDLE, Some(&saw_legacy));
    drop(od);
    server.join().unwrap();
    assert!(
        saw_legacy.load(Ordering::Relaxed),
        "bound 274 must still send 00 00 00 0a"
    );
}

/// `load_on_demand` without a session profile still passes the Client
/// revision: unbound 274 keeps the Java keepalive.
#[test]
fn unbound_load_on_demand_274_sends_java_keepalive() {
    let _r = Renderer::new(false);
    let (payload, crc) = map_payload();
    let body = payload.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let saw_legacy = Arc::new(AtomicBool::new(false));
    let saw_legacy_s = Arc::clone(&saw_legacy);
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        handshake(&mut sock);
        serve_map(&mut sock, &body);
        sock.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let idle_until = Instant::now() + KEEPALIVE_IDLE;
        while Instant::now() < idle_until {
            let mut extra = [0u8; 4];
            match sock.read_exact(&mut extra) {
                Ok(()) if extra == [0, 0, 0, 10] => {
                    saw_legacy_s.store(true, Ordering::Relaxed);
                    return;
                }
                Ok(()) => return,
                Err(_) => {}
            }
        }
    });

    let cache = tmp("ka-unbound-274");
    std::fs::write(cache.join("versionlist"), versionlist_bytes(crc)).unwrap();
    let mut c = Client::from_shared_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port,
            cache_dir: cache.to_str().unwrap().into(),
            members: true,
            lowmem: false,
        },
        Arc::new(Cache::default()),
        Arc::new(vec![]),
        vec![],
        ClientRevision::R274,
    );
    let od = c.on_demand.as_mut().expect("versionlist starts OnDemand");
    od.request(3, 0);
    let first = wait_map(od, Duration::from_secs(3));
    assert_eq!(first.as_deref(), Some(MAP_RAW));
    idle_run(od, Instant::now() + KEEPALIVE_IDLE, Some(&saw_legacy));
    drop(c);
    server.join().unwrap();
    assert!(
        saw_legacy.load(Ordering::Relaxed),
        "unbound 274 load_on_demand must still send 00 00 00 0a"
    );
}

/// A server that accepts then closes without serving must not tight-loop
/// reconnects. Backoff grows to the 4 s Java gate.
#[test]
fn no_progress_reconnect_is_rate_limited() {
    let _r = Renderer::new(false);
    let (_payload, crc) = map_payload();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_s = Arc::clone(&accepts);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_s = Arc::clone(&stop);
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(1200);
        while Instant::now() < deadline && !stop_s.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut sock, _)) => {
                    accepts_s.fetch_add(1, Ordering::Relaxed);
                    let _ = sock.set_nonblocking(false);
                    handshake(&mut sock);
                    sock.shutdown(Shutdown::Both).ok();
                }
                Err(_) => thread::sleep(Duration::from_millis(2)),
            }
        }
    });
    let cache = tmp("storm");
    let mut od = OnDemand::new(
        &map_versionlist(crc),
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.request(3, 0);
    let end = Instant::now() + Duration::from_millis(1000);
    while Instant::now() < end {
        od.run(true);
        thread::sleep(Duration::from_millis(10));
    }
    stop.store(true, Ordering::Relaxed);
    drop(od);
    server.join().unwrap();
    let n = accepts.load(Ordering::Relaxed);
    assert!(n >= 1, "must attempt the update socket");
    assert!(
        n <= 12,
        "no-progress reconnect storm: {n} accepts in ~1s (20ms doubling caps well below tick rate)"
    );
}

/// A refused connect must keep Java's 4 s open gate so `fail_count` cannot
/// cross 3 in a 1 s outage (`maininit` errors at `fail_count > 3`).
#[test]
fn refused_connect_keeps_java_open_gate() {
    let _r = Renderer::new(false);
    let (payload, crc) = map_payload();
    let body = payload.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let cache = tmp("refused");
    let mut od = OnDemand::new(
        &map_versionlist(crc),
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.request(3, 0);
    let down_until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < down_until {
        od.run(true);
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        od.fail_count <= 3,
        "refused connect used fast backoff: fail_count={} after 1s",
        od.fail_count
    );

    let server = thread::spawn(move || {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        let (mut sock, _) = listener.accept().unwrap();
        handshake(&mut sock);
        serve_map(&mut sock, &body);
    });
    let got = wait_map(&mut od, Duration::from_secs(6));
    server.join().unwrap();
    assert_eq!(got.as_deref(), Some(MAP_RAW));
}

/// After a file completed over the network, a later idle close still
/// reconnects immediately rather than sitting out the 4 s gate.
#[test]
fn idle_close_after_network_complete_reconnects_immediately() {
    let _r = Renderer::new(false);
    let (payload, crc) = map_payload();
    let body = payload.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepts = Arc::new(AtomicUsize::new(0));
    let accepts_s = Arc::clone(&accepts);
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        accepts_s.fetch_add(1, Ordering::Relaxed);
        handshake(&mut sock);
        serve_map(&mut sock, &body);
        sock.shutdown(Shutdown::Both).ok();
        drop(sock);
        let (mut sock, _) = listener.accept().unwrap();
        accepts_s.fetch_add(1, Ordering::Relaxed);
        handshake(&mut sock);
        serve_map(&mut sock, &body);
    });
    let cache = tmp("idle-close");
    let mut od = OnDemand::new(
        &map_versionlist(crc),
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.request(3, 0);
    let first = wait_map(&mut od, Duration::from_secs(3));
    assert_eq!(first.as_deref(), Some(MAP_RAW));
    let started = Instant::now();
    od.request(3, 0);
    let second = wait_map(&mut od, Duration::from_millis(800));
    server.join().unwrap();
    assert_eq!(second.as_deref(), Some(MAP_RAW));
    assert!(
        started.elapsed() < Duration::from_millis(800),
        "idle-close after serve waited {:?}; progress must skip the 4s gate",
        started.elapsed()
    );
    assert_eq!(accepts.load(Ordering::Relaxed), 2);
}
