//! Frozen cold-cache preparation: with no local `main_file_cache` store, the
//! snapshot is filled from the update-server entry protocol through the real
//! `OnDemand` worker, every downloaded payload is validated against the
//! selected versionlist, and the snapshot is published only when each
//! required entry arrived.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use client::io::jagfile::JagFile;
use client::io::ondemand::OnDemand;
use client::io::Packet;
use client::unpack::{
    fetch_snapshot, prepare_snapshot, snapshot_state, version_hash, FetchEndpoint,
    SnapshotPreparation, SnapshotState,
};
use client::BotTarget;

const JAGS: [&str; 8] = [
    "config",
    "interface",
    "textures",
    "media",
    "title",
    "sounds",
    "wordenc",
    "versionlist",
];

fn tmp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("274bot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gz(raw: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(raw).unwrap();
    enc.finish().unwrap()
}

fn bz2(data: &[u8]) -> Vec<u8> {
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    enc.write_all(data).unwrap();
    let out = enc.finish().unwrap();
    out[4..].to_vec()
}

/// One jag container from named entries (same layout the engine writes).
fn jag(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let packed: Vec<Vec<u8>> = files.iter().map(|(_, d)| bz2(d)).collect();
    let data_len: usize = packed.iter().map(|d| d.len()).sum();
    let total = (8 + 10 * files.len() + data_len) as i32;
    let mut out = Vec::new();
    for value in [total, total] {
        out.push((value >> 16) as u8);
        out.push((value >> 8) as u8);
        out.push(value as u8);
    }
    out.push((files.len() >> 8) as u8);
    out.push(files.len() as u8);
    for ((name, data), packed_data) in files.iter().zip(packed.iter()) {
        out.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        for value in [data.len() as i32, packed_data.len() as i32] {
            out.push((value >> 16) as u8);
            out.push((value >> 8) as u8);
            out.push(value as u8);
        }
    }
    for d in &packed {
        out.extend_from_slice(d);
    }
    out
}

/// The versionlist jag for `entries`: one required entry per archive (version 1)
/// with the CRC of its gzipped payload — tables OnDemand `validate` checks.
fn versionlist_jag(entries: &[(i32, i32, Vec<u8>)]) -> Vec<u8> {
    let names = ["model", "anim", "midi", "map"];
    let mut tables: Vec<(String, Vec<u8>)> = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let (_, _, raw) = entries
            .iter()
            .find(|(archive, _, _)| *archive == index as i32)
            .expect("fixture covers every archive");
        let payload = gz(raw);
        let crc = Packet::getcrc(&payload, 0, payload.len());
        tables.push((format!("{name}_version"), vec![0x00, 0x01]));
        tables.push((format!("{name}_crc"), crc.to_be_bytes().to_vec()));
    }
    jag(&tables)
}

/// A cache directory with the jags but *no* `main_file_cache` store: the only
/// source of entries left is the update server.
fn cold_cache(dir: &std::path::Path, entries: &[(i32, i32, Vec<u8>)]) -> Vec<u8> {
    let versionlist = versionlist_jag(entries);
    std::fs::write(dir.join("versionlist"), &versionlist).unwrap();
    for name in JAGS {
        if name != "versionlist" {
            std::fs::write(dir.join(name), b"\0\0\x06\0\0\x06\0\0").unwrap();
        }
    }
    versionlist
}

/// Mock engine ondemand socket: byte-15 handshake, then one chunk per
/// requested `(archive, file)` (6-byte header + gzip payload + version
/// trailer). Serves until every expected entry has been answered.
fn serve_entries(
    entries: Arc<Vec<(i32, i32, Vec<u8>)>>,
) -> (u16, thread::JoinHandle<Vec<(i32, i32)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut handshake = [0u8; 1];
        sock.read_exact(&mut handshake).unwrap();
        assert_eq!(handshake[0], 15, "ondemand handshake byte");
        sock.write_all(&[0; 8]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut served = Vec::new();
        while Instant::now() < deadline && served.len() < entries.len() {
            let mut req = [0u8; 4];
            if sock.read_exact(&mut req).is_err() {
                break;
            }
            let archive = req[0] as i32;
            let file = ((req[1] as i32) << 8) + req[2] as i32;
            if served.contains(&(archive, file)) {
                continue; // a duplicate/resend: answer again
            }
            let (_, _, raw) = entries
                .iter()
                .find(|(a, f, _)| *a == archive && *f == file)
                .expect("client requested a file the fixture serves");
            let mut body = gz(raw);
            body.extend_from_slice(&[0, 1]);
            let len = body.len() as u16;
            let mut chunk = vec![archive as u8, (file >> 8) as u8, file as u8];
            chunk.extend_from_slice(&[(len >> 8) as u8, len as u8, 0]);
            chunk.extend_from_slice(&body);
            sock.write_all(&chunk).unwrap();
            served.push((archive, file));
        }
        served
    });
    (port, handle)
}

/// The genuinely cold path: no store, so the snapshot is filled from the
/// update-server protocol through the real worker and published complete.
#[test]
fn cold_snapshot_is_filled_from_the_update_server() {
    let entries: Arc<Vec<(i32, i32, Vec<u8>)>> = Arc::new(vec![
        (0, 0, vec![0u8; 18]),
        (
            1,
            0,
            vec![
                0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        ),
        (2, 0, vec![1, 2, 3]),
        (3, 0, vec![4, 5, 6, 7]),
    ]);
    let cache = tmp_dir("cold-fetch-cache");
    let out = tmp_dir("cold-fetch-out");
    let versionlist = cold_cache(&cache, &entries);
    assert!(
        !cache.join("main_file_cache.dat").exists(),
        "fixture must have no local store"
    );

    let (port, server) = serve_entries(Arc::clone(&entries));
    let mut on_demand = OnDemand::new(
        &JagFile::new(versionlist),
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();

    let manifest = fetch_snapshot(
        cache.to_str().unwrap(),
        out.to_str().unwrap(),
        &mut on_demand,
    )
    .unwrap();
    let served = server.join().unwrap();

    assert_eq!(manifest.source, "update-server");
    assert!(manifest.complete);
    assert_eq!(manifest.models.unpacked, 1);
    assert_eq!(manifest.anims.unpacked, 1);
    assert_eq!(manifest.midi.unpacked, 1);
    assert_eq!(manifest.maps.unpacked, 1);
    assert_eq!(
        served.len(),
        entries.len(),
        "every required entry was fetched"
    );
    assert!(matches!(
        snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
        SnapshotState::Ready(_)
    ));

    // The record stream holds the downloaded, validated payloads.
    let models = std::fs::read(std::path::Path::new(&manifest.dir).join("models.bin")).unwrap();
    assert_eq!(models.len(), 8 + 18);
    assert_eq!(&models[8..], &[0u8; 18]);

    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_dir_all(&out);
}

/// A payload that does not match the versionlist CRC must fail the fill and
/// publish nothing: a corrupted download is never admitted as ready.
#[test]
fn cold_fill_rejects_a_payload_that_does_not_validate() {
    let entries: Arc<Vec<(i32, i32, Vec<u8>)>> = Arc::new(vec![
        (0, 0, vec![0u8; 18]),
        (
            1,
            0,
            vec![
                0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        ),
        (2, 0, vec![1, 2, 3]),
        (3, 0, vec![4, 5, 6, 7]),
    ]);
    let cache = tmp_dir("cold-fetch-bad-cache");
    let out = tmp_dir("cold-fetch-bad-out");
    let versionlist = cold_cache(&cache, &entries);

    // Serve a tampered model payload (same trailer, different bytes).
    let served: Arc<Vec<(i32, i32, Vec<u8>)>> = Arc::new(
        entries
            .iter()
            .map(|(a, f, raw)| {
                if *a == 0 {
                    (*a, *f, vec![7u8; raw.len()])
                } else {
                    (*a, *f, raw.clone())
                }
            })
            .collect(),
    );
    let (port, server) = serve_entries(Arc::clone(&served));
    let mut on_demand = OnDemand::new(
        &JagFile::new(versionlist.clone()),
        "127.0.0.1",
        port,
        cache.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();

    let err = fetch_snapshot(
        cache.to_str().unwrap(),
        out.to_str().unwrap(),
        &mut on_demand,
    )
    .unwrap_err();
    server.join().ok();
    assert!(
        err.to_string().contains("version/CRC check"),
        "a tampered payload must fail the fill, got: {err}"
    );
    let version = client::unpack::version_hash(&versionlist);
    assert!(
        !out.join(&version).exists(),
        "a failed cold fill must not publish a snapshot"
    );
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_dir_all(&out);
}

/// The 8 update-server pack files for an empty cache: an empty jag for every
/// pack except `versionlist`, which carries the entries' tables, in checksum
/// slot order (`title`=1 .. `sounds`=8).
fn packs(versionlist: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut packs: Vec<(String, Vec<u8>)> = JAGS
        .iter()
        .map(|name| {
            let bytes = if *name == "versionlist" {
                versionlist.to_vec()
            } else {
                b"\0\0\x06\0\0\x06\0\0".to_vec()
            };
            ((*name).to_string(), bytes)
        })
        .collect();
    packs.sort_by_key(|(name, _)| crc_slot(name));
    packs
}

fn crc_slot(name: &str) -> usize {
    match name {
        "title" => 1,
        "config" => 2,
        "interface" => 3,
        "media" => 4,
        "versionlist" => 5,
        "textures" => 6,
        "wordenc" => 7,
        "sounds" => 8,
        other => panic!("{other}: not a jag pack file"),
    }
}

/// The 9×g4 + hash body the client's `/crc` read expects (slot 0 empty).
fn crc_body(packs: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut checksums = [0i32; 9];
    for (name, bytes) in packs {
        checksums[crc_slot(name)] = Packet::getcrc(bytes, 0, bytes.len());
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

/// Mock update server: `/crc` plus one `GET /{name}{crc}` response per pack
/// file, each on its own connection (the client's HTTP helper opens one per
/// request). Serves until every pack has been fetched.
fn serve_jags(packs: Arc<Vec<(String, Vec<u8>)>>) -> (u16, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut served = Vec::new();
        while Instant::now() < deadline && served.len() < packs.len() {
            let (mut sock, _) = match listener.accept() {
                Ok(conn) => conn,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("mock update server accept: {e}"),
            };
            sock.set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let request = read_http_request(&mut sock);
            let path = request
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            if path == "/crc" {
                respond(&mut sock, &crc_body(&packs));
                continue;
            }
            let name = packs
                .iter()
                .map(|(name, _)| name.clone())
                .find(|name| path.starts_with(&format!("/{name}")))
                .unwrap_or_else(|| panic!("unexpected update-server path {path}"));
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

/// The genuinely empty cache directory: no pack file at all, so the
/// versionlist itself has to be fetched before any version exists to read.
/// The bind-time preparation (no entry source yet) fetches every pack through
/// the real `/crc` + `getJagFile` path and reports the missing local store as
/// the remaining failure; the boot-time preparation for the same cache
/// identity then fills the snapshot from the update-server entry protocol and
/// publishes it, and a later caller reuses that one fill.
#[test]
fn empty_cache_is_fetched_then_filled_from_the_update_server() {
    let entries: Arc<Vec<(i32, i32, Vec<u8>)>> = Arc::new(vec![
        (0, 0, vec![0u8; 18]),
        (
            1,
            0,
            vec![
                0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        ),
        (2, 0, vec![1, 2, 3]),
        (3, 0, vec![4, 5, 6, 7]),
    ]);
    let cache = tmp_dir("empty-fetch-cache");
    let out = tmp_dir("empty-fetch-out");
    assert!(
        std::fs::read_dir(&cache).unwrap().next().is_none(),
        "the cache directory must start genuinely empty"
    );
    let versionlist = versionlist_jag(&entries);
    let packs = Arc::new(packs(&versionlist));
    let (port, http) = serve_jags(Arc::clone(&packs));
    let endpoint = FetchEndpoint {
        target: BotTarget::Local,
        host: "127.0.0.1",
        port,
    };

    // 1. Bind-time preparation: no worker yet, so the packs (versionlist
    //    included) are fetched and the missing local store is what remains.
    let prepared = prepare_snapshot(
        cache.to_str().unwrap(),
        out.to_str().unwrap(),
        Some(endpoint),
        None,
    );
    let served = http.join().unwrap();
    match &*prepared {
        SnapshotPreparation::Unavailable { reason } => {
            assert!(
                reason.contains("local store"),
                "the missing local store is the remaining failure, got: {reason}"
            );
            assert!(
                !reason.contains("versionlist unreadable"),
                "the versionlist must be fetched before it is read: {reason}"
            );
        }
        other => panic!("no store and no worker cannot be ready: {other:?}"),
    }
    let mut seen = served.clone();
    seen.sort();
    let mut want: Vec<String> = packs.iter().map(|(name, _)| name.clone()).collect();
    want.sort();
    assert_eq!(seen, want, "every pack file was fetched");
    for (name, bytes) in packs.iter() {
        let on_disk = std::fs::read(cache.join(name))
            .unwrap_or_else(|e| panic!("{name} was not fetched into the cache: {e}"));
        assert_eq!(&on_disk, bytes, "{name} on disk");
    }
    assert!(
        !out.join(version_hash(&versionlist)).exists(),
        "no snapshot may be published while the store is missing"
    );
    assert!(
        !matches!(
            snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
            SnapshotState::Ready(_)
        ),
        "a store-less cache must not read as ready"
    );

    // 2. Boot-time preparation for the same cache identity, now with the
    //    client's update-protocol worker: the failed bind-time attempt is not
    //    inherited, and this one fill publishes the complete snapshot.
    let (entries_port, engine) = serve_entries(Arc::clone(&entries));
    let mut on_demand = OnDemand::new(
        &JagFile::new(versionlist.clone()),
        "127.0.0.1",
        entries_port,
        cache.to_str().unwrap(),
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let filled = prepare_snapshot(
        cache.to_str().unwrap(),
        out.to_str().unwrap(),
        Some(endpoint),
        Some(&mut on_demand),
    );
    let served_entries = engine.join().unwrap();
    match &*filled {
        SnapshotPreparation::Ready {
            published,
            source,
            fetched,
            ..
        } => {
            assert!(*published, "the boot-time fill publishes the snapshot");
            assert_eq!(*source, "update-server");
            assert!(fetched.is_empty(), "no pack file was missing by then");
        }
        other => panic!("expected a published snapshot, got {other:?}"),
    }
    assert_eq!(
        served_entries.len(),
        entries.len(),
        "one fetch per required entry"
    );
    assert!(matches!(
        snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
        SnapshotState::Ready(_)
    ));

    // 3. Later callers (the remaining slots) reuse the published snapshot
    //    instead of re-preparing — one fill for the process, not one per
    //    client — and need no endpoint at all to do it.
    let again = prepare_snapshot(cache.to_str().unwrap(), out.to_str().unwrap(), None, None);
    assert!(
        Arc::ptr_eq(&filled, &again),
        "a later preparation shares the published snapshot"
    );

    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_dir_all(&out);
}
