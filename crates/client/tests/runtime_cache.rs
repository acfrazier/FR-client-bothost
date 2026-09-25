//! Controlled retained snapshots and HTTP negotiation; no production endpoint.
use client::content_identity::compute_decoded_content_identity;
use client::io::{ClientRevision, JagFile, Packet};
use client::unpack::{prepare_runtime_cache, version_hash, RuntimeCacheRequest};
use client::BotTarget;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const JAGS: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "runtime-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn g3(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u32).to_be_bytes()[1..]);
}
fn jag(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut raw = (files.len() as u16).to_be_bytes().to_vec();
    for (name, bytes) in files {
        raw.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        g3(&mut raw, bytes.len());
        g3(&mut raw, bytes.len());
    }
    for (_, bytes) in files {
        raw.extend_from_slice(bytes);
    }
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    enc.write_all(&raw).unwrap();
    let compressed = enc.finish().unwrap();
    let compressed = &compressed[4..];
    assert_ne!(raw.len(), compressed.len());
    let mut out = Vec::new();
    g3(&mut out, raw.len());
    g3(&mut out, compressed.len());
    out.extend_from_slice(compressed);
    out
}
fn packs(crc: u32) -> Vec<Vec<u8>> {
    let mut members = Vec::new();
    for prefix in ["model", "anim", "midi", "map"] {
        members.push((format!("{prefix}_version"), vec![0, 1]));
        members.push((format!("{prefix}_crc"), crc.to_be_bytes().to_vec()));
        members.push((format!("{prefix}_index"), vec![0]));
    }
    let members: Vec<_> = members
        .iter()
        .map(|(n, b)| (n.as_str(), b.clone()))
        .collect();
    JAGS.iter()
        .map(|name| {
            if *name == "versionlist" {
                jag(&members)
            } else {
                jag(&[("data", b"content".to_vec())])
            }
        })
        .collect()
}
fn write_packs(dir: &Path, packs: &[Vec<u8>]) {
    std::fs::create_dir_all(dir).unwrap();
    for (name, bytes) in JAGS.iter().zip(packs) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
}
fn snapshot(root: &Path, packs: &[Vec<u8>]) -> PathBuf {
    let version = version_hash(&packs[4]);
    let dir = root.join(&version);
    write_packs(&dir, packs);
    let mut manifest = format!(
        "version={version}\ndir={}\nsource=update-server\ncomplete=1\n",
        dir.display()
    );
    for (name, bytes) in JAGS.iter().zip(packs) {
        manifest += &format!("jag.{name}.bytes={}\n", bytes.len());
    }
    for name in ["models", "anims", "midi", "maps"] {
        let mut bin = 0u32.to_le_bytes().to_vec();
        bin.extend_from_slice(&4u32.to_le_bytes());
        bin.extend_from_slice(b"body");
        std::fs::write(dir.join(format!("{name}.bin")), &bin).unwrap();
        manifest += &format!(
            "{name}.total=1\n{name}.unpacked=1\n{name}.skipped=0\n{name}.bytes={}\n",
            bin.len()
        );
    }
    std::fs::write(dir.join("manifest"), manifest).unwrap();
    dir
}
fn server(packs: Vec<Vec<u8>>, downloads: usize) -> (u16, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let thread = std::thread::spawn(move || {
        let mut crcs = [0i32; 9];
        for (i, p) in packs.iter().enumerate() {
            crcs[i + 1] = Packet::getcrc(p, 0, p.len());
        }
        let mut crc_body: Vec<u8> = crcs.iter().flat_map(|c| c.to_be_bytes()).collect();
        let folded = crcs
            .iter()
            .fold(1234i32, |n, c| n.wrapping_shl(1).wrapping_add(*c));
        crc_body.extend_from_slice(&folded.to_be_bytes());
        for _ in 0..=downloads {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                sock.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            let path = request.split_whitespace().nth(1).unwrap();
            let body = if path == "/crc" {
                &crc_body
            } else {
                &packs[JAGS
                    .iter()
                    .position(|n| path.starts_with(&format!("/{n}")))
                    .unwrap()]
            };
            write!(
                sock,
                "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            sock.write_all(body).unwrap();
        }
    });
    (port, thread)
}
fn prepare(
    source: &Path,
    root: &Path,
    packs: Vec<Vec<u8>>,
    downloads: usize,
) -> Result<std::sync::Arc<client::unpack::PreparedRuntimeCache>, String> {
    let (port, thread) = server(packs, downloads);
    let result = prepare_runtime_cache(&RuntimeCacheRequest {
        revision: ClientRevision::R289,
        target: BotTarget::Local,
        jag_source: source,
        snapshot_root: root,
        asset_host: "127.0.0.1",
        asset_port: port,
        game_host: "127.0.0.1",
        game_port: 1,
    });
    thread.join().unwrap();
    result.map_err(|error| error.to_string())
}
#[test]
fn equivalent_transfer_refresh_is_owned_and_cleanup_follows_last_arc() {
    let tmp = Temp::new();
    let source = tmp.0.join("source");
    let root = tmp.0.join("snapshots");
    let local = packs(1);
    let public = packs(2);
    write_packs(&source, &local);
    let local_snap = snapshot(&root, &local);
    snapshot(&root, &public);
    let expected = compute_decoded_content_identity(289, &source, local_snap).unwrap();
    let prepared = prepare(&source, &root, public.clone(), 1).unwrap();
    assert_eq!(prepared.identity, expected);
    assert_eq!(prepared.fetched, vec!["versionlist"]);
    for (i, name) in JAGS.iter().enumerate() {
        assert_eq!(std::fs::read(source.join(name)).unwrap(), local[i]);
        assert_eq!(
            std::fs::read(prepared.jag_dir.join(name)).unwrap(),
            public[i]
        );
        assert_eq!(
            prepared.expected_crc[i + 1],
            Packet::getcrc(&public[i], 0, public[i].len())
        );
    }
    let owned = prepared.unpack_root().to_owned();
    let shared = prepared.clone();
    drop(prepared);
    assert!(owned.exists());
    drop(shared);
    assert!(!owned.exists());
}
#[test]
fn same_size_bin_replacement_cannot_reuse_previous_compatibility() {
    let tmp = Temp::new();
    let source = tmp.0.join("source");
    let root = tmp.0.join("snapshots");
    let p = packs(1);
    write_packs(&source, &p);
    let snap = snapshot(&root, &p);
    let first = prepare(&source, &root, p.clone(), 0).unwrap();
    let path = snap.join("maps.bin");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[8] ^= 1;
    std::fs::write(path, bytes).unwrap();
    let second = prepare(&source, &root, p, 0).unwrap();
    assert_ne!(first.identity.content_id, second.identity.content_id);
    assert_eq!(
        std::fs::read(first.snapshot_dir.join("maps.bin")).unwrap()[8],
        b'b'
    );
}
#[test]
fn malformed_required_record_is_not_a_ready_identity() {
    let tmp = Temp::new();
    let source = tmp.0.join("source");
    let root = tmp.0.join("snapshots");
    let p = packs(1);
    write_packs(&source, &p);
    let snap = snapshot(&root, &p);
    let path = snap.join("maps.bin");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] = 1;
    std::fs::write(path, bytes).unwrap();
    let err = prepare(&source, &root, p, 0).unwrap_err();
    assert!(
        err.contains("maps")
            && (err.contains("unexpected")
                || err.contains("required")
                || err.contains("range")
                || err.contains("unknown extra")),
        "{err}"
    );
    assert!(!std::fs::read_dir(&root).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".runtime")));
}

fn spawn_short_child() -> std::process::Child {
    #[cfg(unix)]
    {
        std::process::Command::new("true")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn true")
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "exit", "0"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cmd exit")
    }
}

fn spawn_live_child() -> std::process::Child {
    #[cfg(unix)]
    {
        std::process::Command::new("sleep")
            .arg("30")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn sleep")
    }
    #[cfg(windows)]
    {
        std::process::Command::new("timeout")
            .args(["/T", "30", "/NOBREAK"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn timeout")
    }
}

#[test]
fn prepare_sweeps_leaked_runtime_staging_from_dead_pids_only() {
    let tmp = Temp::new();
    let source = tmp.0.join("source");
    let root = tmp.0.join("snapshots");
    let p = packs(1);
    write_packs(&source, &p);
    snapshot(&root, &p);

    let mut dead = spawn_short_child();
    let dead_pid = dead.id();
    dead.wait().expect("wait short child");

    let mut live = spawn_live_child();
    let live_pid = live.id();

    let dead_dir = root.join(format!(".runtime-{dead_pid}-0"));
    let self_dir = root.join(format!(".runtime-{}-999", std::process::id()));
    let live_dir = root.join(format!(".runtime-{live_pid}-0"));
    let other_dir = root.join("not-runtime-staging");
    let bad_name = root.join(".runtime-abc-1");
    for dir in [&dead_dir, &self_dir, &live_dir, &other_dir, &bad_name] {
        std::fs::create_dir(dir).unwrap();
        std::fs::write(dir.join("marker"), b"keep").unwrap();
    }

    let prepared = prepare(&source, &root, p, 0).unwrap();

    assert!(
        !dead_dir.exists(),
        "dead pid staging should be swept: {}",
        dead_dir.display()
    );
    assert!(
        self_dir.exists(),
        "current pid staging must stay: {}",
        self_dir.display()
    );
    assert!(
        live_dir.exists(),
        "live pid staging must stay: {}",
        live_dir.display()
    );
    assert!(
        other_dir.exists(),
        "non-matching name must stay: {}",
        other_dir.display()
    );
    assert!(
        bad_name.exists(),
        "non-decimal runtime name must stay: {}",
        bad_name.display()
    );
    assert!(prepared.unpack_root().exists());

    let _ = live.kill();
    let _ = live.wait();
    drop(prepared);
}
