//! Task 18: OnDemand request worker.
//!
//! `new_unconnected` builds an OnDemand with no versionlist and no worker
//! thread; `request` queues into the client-side request list and
//! `remaining` counts it, exactly as Java/TS track `requests`. The socket
//! test drives the full worker pump against a mock engine ondemand socket.
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use client::io::{JagFile, OnDemand};

#[test]
fn request_increments_remaining() {
    let mut od = OnDemand::new_unconnected();
    assert_eq!(od.remaining(), 0);
    od.request(2, 0);
    assert_eq!(od.remaining(), 1);
}

#[test]
fn duplicate_request_is_deduplicated() {
    let mut od = OnDemand::new_unconnected();
    od.request(2, 0);
    od.request(2, 0);
    assert_eq!(od.remaining(), 1);
}

#[test]
fn different_files_both_queue() {
    let mut od = OnDemand::new_unconnected();
    od.request(2, 0);
    od.request(3, 100);
    assert_eq!(od.remaining(), 2);
}

/// End-to-end worker pump against a mock engine ondemand socket: byte-15
/// handshake, 4-byte request (archive 2 / file 0 / urgent), and one chunk of
/// gzip + 2-byte version trailer, which the client gunzips on `loop_request`.
#[test]
fn worker_downloads_and_gunzips_from_ondemand_socket() {
    use std::io::Read;
    use std::net::TcpListener;

    let files: Vec<(&str, Vec<u8>)> = vec![
        ("model_version", vec![0, 1]),
        ("anim_version", vec![0, 1]),
        ("midi_version", vec![0, 1]),
        ("map_version", vec![0, 1]),
        ("model_crc", vec![0, 0, 0, 0]),
        ("anim_crc", vec![0, 0, 0, 0]),
        ("midi_crc", vec![0, 0, 0, 0]),
        ("map_crc", vec![0, 0, 0, 0]),
    ];
    let entries: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
    let versionlist = JagFile::new(jag(&entries));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let payload: &'static [u8] = b"midi bytes";
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut b = [0u8; 1];
        sock.read_exact(&mut b).unwrap();
        assert_eq!(b[0], 15, "ondemand handshake byte");
        sock.write_all(&[0; 8]).unwrap();
        let mut req = [0u8; 4];
        sock.read_exact(&mut req).unwrap();
        assert_eq!(req[0], 2, "archive");
        assert_eq!(u16::from_be_bytes([req[1], req[2]]), 0, "file");
        assert_eq!(req[3], 2, "urgent priority");
        // gzip(payload) + 2-byte version trailer, served as one part
        let mut body = gz(payload);
        body.extend_from_slice(&[0, 1]);
        let len = body.len() as u16;
        let mut chunk = vec![2, 0, 0, (len >> 8) as u8, len as u8, 0];
        chunk.extend_from_slice(&body);
        sock.write_all(&chunk).unwrap();
    });

    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        port,
        "/tmp",
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.request(2, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    let got = loop {
        od.run(false);
        if let Some(req) = od.loop_request() {
            break req.data;
        }
        if Instant::now() > deadline {
            panic!("ondemand worker did not complete the request");
        }
        thread::sleep(Duration::from_millis(10));
    };
    server.join().unwrap();
    assert_eq!(got.as_deref(), Some(payload));
}

/// Task 10: after `drop_socket` the worker must reopen the ondemand socket
/// immediately — no 4 s `socket_open_time` gate and no 750-cycle dead-socket
/// wait. The server accepts two connections on the same listener: the first
/// is served and left to the worker's `drop_socket` to close; the second
/// (a reconnect) is served right after.
#[test]
fn worker_reopens_socket_immediately_after_drop_socket() {
    use std::io::Read;
    use std::net::TcpListener;

    let files: Vec<(&str, Vec<u8>)> = vec![
        ("model_version", vec![0, 1]),
        ("anim_version", vec![0, 1]),
        ("midi_version", vec![0, 1]),
        ("map_version", vec![0, 1]),
        ("model_crc", vec![0, 0, 0, 0]),
        ("anim_crc", vec![0, 0, 0, 0]),
        ("midi_crc", vec![0, 0, 0, 0]),
        ("map_crc", vec![0, 0, 0, 0]),
    ];
    let entries: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
    let versionlist = JagFile::new(jag(&entries));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let payload: &'static [u8] = b"midi bytes";
    let server = thread::spawn(move || {
        for round in 0..2 {
            let (mut sock, _) = listener.accept().unwrap();
            let mut b = [0u8; 1];
            sock.read_exact(&mut b).unwrap();
            assert_eq!(b[0], 15, "ondemand handshake byte (round {round})");
            sock.write_all(&[0; 8]).unwrap();
            let mut req = [0u8; 4];
            sock.read_exact(&mut req).unwrap();
            assert_eq!(req[0], 2, "archive");
            assert_eq!(u16::from_be_bytes([req[1], req[2]]), 0, "file");
            assert_eq!(req[3], 2, "urgent priority");
            let mut body = gz(payload);
            body.extend_from_slice(&[0, 1]);
            let len = body.len() as u16;
            let mut chunk = vec![2, 0, 0, (len >> 8) as u8, len as u8, 0];
            chunk.extend_from_slice(&body);
            sock.write_all(&chunk).unwrap();
        }
    });

    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        port,
        "/tmp",
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();

    // One full download cycle, polling the OnDemand pump like the client
    // main loop; `None` when the deadline passes without a completion.
    let fetch = |od: &mut OnDemand, deadline: Duration| -> Option<Vec<u8>> {
        od.request(2, 0);
        let deadline = Instant::now() + deadline;
        loop {
            od.run(false);
            if let Some(req) = od.loop_request() {
                return req.data;
            }
            if Instant::now() > deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    };

    let first = fetch(&mut od, Duration::from_secs(5));
    assert_eq!(first.as_deref(), Some(payload));

    // Logout drops the engine update connection; the worker must drop its
    // stream and reopen on the next request without the 4 s gate.
    od.drop_socket();
    let second = fetch(&mut od, Duration::from_millis(1500));
    assert_eq!(second.as_deref(), Some(payload));

    server.join().unwrap();
}

/// Task 12 fix: a prefetched (non-urgent) archive-0 model must still be
/// posted to `completed` so `Client::on_demand_loop` can `Model::unpack` it.
/// Java persists every completed file to its local cache and re-reads it on
/// the next `handleQueue`; this port never writes the cache, so dropping
/// non-urgent completions would discard every prefetched model and first-
/// login `get_temp_model` would still network-fetch.
#[test]
fn prefetched_archive0_model_posts_to_completed() {
    use std::io::Read;
    use std::net::TcpListener;

    let files: Vec<(&str, Vec<u8>)> = vec![
        // g2 table, two entries: file 0 version 0, file 1 version 1.
        ("model_version", vec![0, 0, 0, 1]),
        ("anim_version", vec![0, 1]),
        ("midi_version", vec![0, 1]),
        ("map_version", vec![0, 1]),
        // g4 table, two entries so `validate` can index file 1.
        ("model_crc", vec![0, 0, 0, 0, 0, 0, 0, 0]),
        ("anim_crc", vec![0, 0, 0, 0]),
        ("midi_crc", vec![0, 0, 0, 0]),
        ("map_crc", vec![0, 0, 0, 0]),
    ];
    let entries: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
    let versionlist = JagFile::new(jag(&entries));

    // `prefetch_priority` only engages when a local cache exists.
    let cache_dir = std::env::temp_dir().join("274bot-t12").join("cache");
    let _ = std::fs::remove_dir_all(cache_dir.parent().unwrap());
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("main_file_cache.dat"), b"").unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let payload: &'static [u8] = b"model bytes";
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut b = [0u8; 1];
        sock.read_exact(&mut b).unwrap();
        assert_eq!(b[0], 15, "ondemand handshake byte");
        sock.write_all(&[0; 8]).unwrap();
        let mut req = [0u8; 4];
        sock.read_exact(&mut req).unwrap();
        assert_eq!(req[0], 0, "archive 0");
        assert_eq!(u16::from_be_bytes([req[1], req[2]]), 1, "file 1");
        assert_eq!(req[3], 1, "prefetch, not urgent, not ingame");
        // gzip(payload) + 2-byte version trailer, served as one part
        let mut body = gz(payload);
        body.extend_from_slice(&[0, 1]);
        let len = body.len() as u16;
        let mut chunk = vec![0, 0, 1, (len >> 8) as u8, len as u8, 0];
        chunk.extend_from_slice(&body);
        sock.write_all(&chunk).unwrap();
    });

    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        port,
        cache_dir.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    od.prefetch_priority(0, 1, 5);
    let deadline = Instant::now() + Duration::from_secs(5);
    let got = loop {
        od.run(false);
        if let Some(req) = od.loop_request() {
            break req.data;
        }
        if Instant::now() > deadline {
            panic!("prefetched model never posted to completed");
        }
        thread::sleep(Duration::from_millis(10));
    };
    server.join().unwrap();
    assert_eq!(got.as_deref(), Some(payload));
}

/// Java `Client.maininit` 5206-5210 urgent-`request`s only `getModelUse & 1`.
/// Prefetchable models (`model_use_priority != 0`) outnumber that set, so the
/// red bar must not `request` the rest. Extra files go through
/// `prefetchPriority` and do not count in `remaining()`.
#[test]
fn bar_urgent_models_are_in_use_bit_only() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    let path = format!("{cache}/versionlist");
    if !std::path::Path::new(&path).is_file() {
        return;
    }
    let versionlist = JagFile::new(std::fs::read(&path).unwrap());
    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        1,
        &cache,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("engine versionlist");
    od.request_in_use_models();
    let bar = od.remaining();
    let prefetchable = (0..od.get_file_count(0))
        .filter(|&i| OnDemand::model_use_priority(od.get_model_use(i)) != 0)
        .count();
    assert!(
        bar < prefetchable,
        "Java waits remaining()==0 only for getModelUse&1 ({bar}); other use bits ({prefetchable}) prefetch after title"
    );
    od.prefetch_extra_files(true, false);
    assert_eq!(
        od.remaining(),
        bar,
        "prefetchPriority must not count in remaining(); extra files download on the title"
    );
}

/// Java `prefetchPriority` skips a CRC-valid FileStream copy (no extra-files
/// bar). This port never writes idx, so `Model::unpack` only happens on
/// `Completed`. A cache-hit extra model (not `getModelUse & 1`) must still
/// post archive-0 data so first login can `Model::load` it. The engine pack
/// keeps `main_file_cache.dat` in `pack/`, one directory above `--cache`
/// `pack/client`.
#[test]
fn prefetch_cache_hit_posts_extra_model_for_unpack() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    let path = format!("{cache}/versionlist");
    if !std::path::Path::new(&path).is_file() {
        return;
    }
    let versionlist = JagFile::new(std::fs::read(&path).unwrap());
    let mut od = OnDemand::new(
        &versionlist,
        "127.0.0.1",
        1,
        &cache,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("engine versionlist");
    let extra = (0..od.get_file_count(0)).find(|&i| {
        let bits = od.get_model_use(i);
        bits & 1 == 0 && OnDemand::model_use_priority(bits) != 0
    });
    let Some(extra) = extra else {
        return;
    };
    od.prefetch_extra_files(true, false);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut got = false;
    while Instant::now() < deadline {
        od.run(false);
        while let Some(req) = od.loop_request() {
            if req.archive == 0 && req.file == extra && req.data.as_ref().is_some_and(|d| !d.is_empty())
            {
                got = true;
            }
        }
        if got {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        got,
        "prefetch cache-hit of extra model {extra} must complete with bytes for Model::unpack"
    );
}

/// Versionlist jag in the engine pack layout (bzip2 per file, like the
/// `jag` helper in tests/graphics.rs).
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

fn bz2(data: &[u8]) -> Vec<u8> {
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    enc.write_all(data).unwrap();
    let out = enc.finish().unwrap();
    assert!(out.starts_with(b"BZh"));
    out[4..].to_vec()
}

fn g3(out: &mut Vec<u8>, value: i32) {
    out.push((value >> 16) as u8);
    out.push((value >> 8) as u8);
    out.push(value as u8);
}

fn gz(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}
