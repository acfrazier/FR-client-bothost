#![allow(dead_code)] // Shared by separate integration-test executables.

use client::io::{JagFile, Packet};
use client::unpack::{prepare_runtime_cache, PreparedRuntimeCache, RuntimeCacheRequest};
use client::Transport;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const PACK_NAMES: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];

const SLOTS: [usize; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const PADDING_BYTES: usize = 128 * 1024;

#[derive(Clone)]
pub struct Entry {
    pub archive: u8,
    pub file: u16,
    pub payload: Vec<u8>,
    compressed: Vec<u8>,
}

#[derive(Clone)]
pub struct PackSet {
    pub packs: Vec<Vec<u8>>,
    pub entries: Vec<Entry>,
    versionlist_members: Vec<(String, Vec<u8>)>,
    seed: u8,
}

impl PackSet {
    pub fn new(seed: u8) -> Self {
        let payloads = [
            vec![seed; 18],
            (0..24).map(|n| seed.wrapping_add(n)).collect(),
            (0..32)
                .map(|n| seed.wrapping_mul(3).wrapping_add(n))
                .collect(),
            (0..40)
                .map(|n| seed.wrapping_mul(7).wrapping_add(n))
                .collect(),
        ];
        let entries: Vec<Entry> = payloads
            .into_iter()
            .enumerate()
            .map(|(archive, payload)| Entry {
                archive: archive as u8,
                file: 0,
                compressed: gzip(&payload),
                payload,
            })
            .collect();

        let mut versionlist_members = Vec::new();
        for (archive, name) in ["model", "anim", "midi", "map"].iter().enumerate() {
            let entry = &entries[archive];
            let crc = Packet::getcrc(&entry.compressed, 0, entry.compressed.len());
            versionlist_members.push((format!("{name}_version"), vec![0, 1]));
            versionlist_members.push((format!("{name}_crc"), crc.to_be_bytes().to_vec()));
            versionlist_members.push((format!("{name}_index"), vec![0]));
        }
        versionlist_members.push(("fixture_padding".to_string(), padding()));

        let versionlist = jag(&versionlist_members, 9);
        let mut packs = Vec::with_capacity(PACK_NAMES.len());
        for name in PACK_NAMES {
            if name == "versionlist" {
                packs.push(versionlist.clone());
            } else {
                let bytes = format!("fixture-{seed}-{name}").into_bytes();
                let mut members = vec![("data".to_string(), bytes)];
                if name == "config" {
                    members.push(("fixture_padding".to_string(), padding()));
                }
                packs.push(jag(&members, 9));
            }
        }
        Self {
            seed,
            packs,
            entries,
            versionlist_members,
        }
    }

    /// Same decoded JAG members and entry tables, but a different packed
    /// versionlist stream. The large deterministic member makes the codec
    /// change observable even for this synthetic fixture.
    pub fn recompressed_versionlist(&self) -> Self {
        let mut replacement = self.clone();
        let index = PACK_NAMES
            .iter()
            .position(|name| *name == "versionlist")
            .unwrap();
        replacement.packs[index] = jag(&self.versionlist_members, 1);
        assert_ne!(
            replacement.packs[index], self.packs[index],
            "fixture codecs must produce distinct packed versionlists"
        );
        replacement
    }

    /// Re-encodes config without changing its decoded members.
    pub fn recompressed_config(&self) -> Self {
        let mut replacement = self.clone();
        let index = PACK_NAMES
            .iter()
            .position(|name| *name == "config")
            .unwrap();
        let members = vec![
            (
                "data".to_string(),
                format!("fixture-{}-config", self.seed).into_bytes(),
            ),
            ("fixture_padding".to_string(), padding()),
        ];
        replacement.packs[index] = jag(&members, 1);
        assert_ne!(
            replacement.packs[index], self.packs[index],
            "fixture codecs must produce distinct packed config JAGs"
        );
        replacement
    }

    pub fn write_to(&self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        for (name, bytes) in PACK_NAMES.iter().zip(&self.packs) {
            std::fs::write(dir.join(name), bytes)?;
        }
        Ok(())
    }
}

fn padding() -> Vec<u8> {
    let mut out = Vec::with_capacity(PADDING_BYTES);
    let mut state = 0x9e37_79b9u32;
    for _ in 0..PADDING_BYTES {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        out.push(state as u8);
    }
    out
}

fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}

fn jag(files: &[(String, Vec<u8>)], level: u32) -> Vec<u8> {
    let mut raw = (files.len() as u16).to_be_bytes().to_vec();
    for (name, bytes) in files {
        raw.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        put_g3(&mut raw, bytes.len());
        put_g3(&mut raw, bytes.len());
    }
    for (_, bytes) in files {
        raw.extend_from_slice(bytes);
    }
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::new(level));
    encoder.write_all(&raw).unwrap();
    let compressed = encoder.finish().unwrap();
    let compressed = &compressed[4..]; // JagFile supplies the BZh header.
    assert_ne!(
        raw.len(),
        compressed.len(),
        "fixture jag must use bzip2 packing"
    );
    let mut out = Vec::with_capacity(6 + compressed.len());
    put_g3(&mut out, raw.len());
    put_g3(&mut out, compressed.len());
    out.extend_from_slice(compressed);
    out
}

fn put_g3(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&(value as u32).to_be_bytes()[1..]);
}

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "startup-snapshot-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct HttpCounts {
    crcs: usize,
    jags: HashMap<String, usize>,
}

struct HttpState {
    packs: PackSet,
    counts: Mutex<HttpCounts>,
    changed: Condvar,
    stop: AtomicBool,
}

pub struct HttpServer {
    port: u16,
    state: Arc<HttpState>,
    thread: Option<JoinHandle<()>>,
}

impl HttpServer {
    pub fn start(packs: PackSet) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let crc_body = checksum_body(&packs);
        let state = Arc::new(HttpState {
            packs,
            counts: Mutex::new(HttpCounts::default()),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let server_state = Arc::clone(&state);
        let thread = thread::spawn(move || loop {
            let (mut socket, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("synthetic HTTP accept failed: {error}"),
            };
            if server_state.stop.load(Ordering::Acquire) {
                break;
            }
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let path = read_http_path(&mut socket);
            let body = if path == "/crc" {
                {
                    let mut counts = server_state.counts.lock().unwrap();
                    counts.crcs += 1;
                    server_state.changed.notify_all();
                }
                crc_body.as_slice()
            } else if let Some((name, bytes)) = PACK_NAMES
                .iter()
                .zip(&server_state.packs.packs)
                .find(|(name, _)| path.starts_with(&format!("/{name}")))
            {
                {
                    let mut counts = server_state.counts.lock().unwrap();
                    *counts.jags.entry((*name).to_string()).or_default() += 1;
                    server_state.changed.notify_all();
                }
                bytes.as_slice()
            } else {
                panic!("unexpected synthetic update-server request {path}");
            };
            respond_http(&mut socket, body).unwrap();
        });
        Self {
            port,
            state,
            thread: Some(thread),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn crc_count(&self) -> usize {
        self.state.counts.lock().unwrap().crcs
    }

    pub fn jag_count(&self, name: &str) -> usize {
        *self
            .state
            .counts
            .lock()
            .unwrap()
            .jags
            .get(name)
            .unwrap_or(&0)
    }

    pub fn wait_for_crcs(&self, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut counts = self.state.counts.lock().unwrap();
        while counts.crcs < count {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, result) = self.state.changed.wait_timeout(counts, remaining).unwrap();
            counts = next;
            if result.timed_out() && counts.crcs < count {
                return false;
            }
        }
        true
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_http_path(socket: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while !request.ends_with(b"\r\n\r\n") {
        socket.read_exact(&mut byte).unwrap();
        request.push(byte[0]);
        assert!(request.len() < 16 * 1024, "oversized HTTP request headers");
    }
    String::from_utf8(request)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string()
}

fn respond_http(socket: &mut TcpStream, body: &[u8]) -> io::Result<()> {
    write!(
        socket,
        "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )?;
    socket.write_all(body)
}

fn checksum_body(packs: &PackSet) -> Vec<u8> {
    let mut checksums = [0i32; 9];
    for (bytes, slot) in packs.packs.iter().zip(SLOTS) {
        checksums[slot] = Packet::getcrc(bytes, 0, bytes.len());
    }
    let mut body = Vec::with_capacity(40);
    for checksum in checksums {
        body.extend_from_slice(&checksum.to_be_bytes());
    }
    let folded = checksums.iter().fold(1234i32, |value, checksum| {
        value.wrapping_shl(1).wrapping_add(*checksum)
    });
    body.extend_from_slice(&folded.to_be_bytes());
    body
}

#[derive(Default)]
struct GateState {
    arrivals: usize,
    released: bool,
}

pub struct EntryGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl EntryGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(GateState::default()),
            changed: Condvar::new(),
        })
    }

    pub fn arrivals(&self) -> usize {
        self.state.lock().unwrap().arrivals
    }

    pub fn wait_for_arrivals(&self, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        while state.arrivals < count {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, result) = self.changed.wait_timeout(state, remaining).unwrap();
            state = next;
            if result.timed_out() && state.arrivals < count {
                return false;
            }
        }
        true
    }

    pub fn release(&self) {
        let mut state = self.state.lock().unwrap();
        state.released = true;
        self.changed.notify_all();
    }

    fn pause_connection(&self) {
        let mut state = self.state.lock().unwrap();
        state.arrivals += 1;
        self.changed.notify_all();
        while !state.released {
            state = self.changed.wait(state).unwrap();
        }
    }
}

struct EntryState {
    entries: Vec<Entry>,
    requests: Mutex<Vec<(u8, u16)>>,
    changed: Condvar,
    active: Mutex<Vec<TcpStream>>,
    gate: Option<Arc<EntryGate>>,
    stop: AtomicBool,
}

pub struct EntryServer {
    port: u16,
    state: Arc<EntryState>,
    thread: Option<JoinHandle<()>>,
}

impl EntryServer {
    pub fn start(entries: Vec<Entry>) -> Self {
        Self::start_inner(entries, None)
    }

    pub fn gated(entries: Vec<Entry>, gate: Arc<EntryGate>) -> Self {
        Self::start_inner(entries, Some(gate))
    }

    fn start_inner(entries: Vec<Entry>, gate: Option<Arc<EntryGate>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(EntryState {
            entries,
            requests: Mutex::new(Vec::new()),
            changed: Condvar::new(),
            active: Mutex::new(Vec::new()),
            gate,
            stop: AtomicBool::new(false),
        });
        let server_state = Arc::clone(&state);
        let thread = thread::spawn(move || {
            let mut handlers = Vec::new();
            loop {
                let (socket, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => panic!("synthetic entry accept failed: {error}"),
                };
                if server_state.stop.load(Ordering::Acquire) {
                    break;
                }
                server_state
                    .active
                    .lock()
                    .unwrap()
                    .push(socket.try_clone().unwrap());
                let client_state = Arc::clone(&server_state);
                handlers.push(thread::spawn(move || {
                    serve_entry_connection(socket, client_state)
                }));
            }
            for handler in handlers {
                let _ = handler.join();
            }
        });
        Self {
            port,
            state,
            thread: Some(thread),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn request_count(&self) -> usize {
        self.state.requests.lock().unwrap().len()
    }

    pub fn wait_for_requests(&self, count: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut requests = self.state.requests.lock().unwrap();
        while requests.len() < count {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, result) = self
                .state
                .changed
                .wait_timeout(requests, remaining)
                .unwrap();
            requests = next;
            if result.timed_out() && requests.len() < count {
                return false;
            }
        }
        true
    }
}

impl Drop for EntryServer {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Release);
        if let Some(gate) = &self.state.gate {
            gate.release();
        }
        for socket in self.state.active.lock().unwrap().iter() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_entry_connection(mut socket: TcpStream, state: Arc<EntryState>) {
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut handshake = [0u8; 1];
    if socket.read_exact(&mut handshake).is_err() || handshake[0] != 15 {
        return;
    }
    if socket.write_all(&[0; 8]).is_err() {
        return;
    }

    let mut gate_pending = state.gate.is_some();
    loop {
        if state.stop.load(Ordering::Acquire) {
            return;
        }
        let mut request = [0u8; 4];
        if socket.read_exact(&mut request).is_err() {
            return;
        }
        let archive = request[0];
        let file = u16::from_be_bytes([request[1], request[2]]);
        {
            let mut requests = state.requests.lock().unwrap();
            requests.push((archive, file));
            state.changed.notify_all();
        }
        if gate_pending {
            state.gate.as_ref().unwrap().pause_connection();
            gate_pending = false;
        }
        let Some(entry) = state
            .entries
            .iter()
            .find(|entry| entry.archive == archive && entry.file == file)
        else {
            return;
        };
        let mut body = entry.compressed.clone();
        body.extend_from_slice(&[0, 1]);
        let Ok(length) = u16::try_from(body.len()) else {
            return;
        };
        let mut header = [0u8; 6];
        header[0] = archive;
        header[1..3].copy_from_slice(&file.to_be_bytes());
        header[3..5].copy_from_slice(&length.to_be_bytes());
        header[5] = 0;
        if socket.write_all(&header).is_err() || socket.write_all(&body).is_err() {
            return;
        }
    }
}

pub fn prepare(
    source: &Path,
    root: &Path,
    asset_port: u16,
    game_port: u16,
    revision: client::io::ClientRevision,
) -> Result<Arc<PreparedRuntimeCache>, String> {
    prepare_runtime_cache(&RuntimeCacheRequest {
        revision,
        transport: Transport::Tcp,
        jag_source: source,
        snapshot_root: root,
        asset_host: "127.0.0.1",
        asset_port,
        game_host: "127.0.0.1",
        game_port,
    })
    .map_err(|error| error.to_string())
}

pub fn flip_same_size_bytes(path: &Path) -> io::Result<()> {
    let mut bytes = std::fs::read(path)?;
    if bytes.len() <= 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fixture too short to mutate byte 8",
        ));
    }
    bytes[8] ^= 1;
    std::fs::write(path, bytes)
}

pub fn forge_integrity_digest(retained_dir: &Path) -> io::Result<()> {
    forge_digest(retained_dir, "payload.models.bin.sha256")
}

pub fn forge_manifest_digest(retained_dir: &Path) -> io::Result<()> {
    forge_digest(retained_dir, "manifest.sha256")
}

fn forge_digest(retained_dir: &Path, field: &str) -> io::Result<()> {
    let path = retained_dir.join("integrity");
    let original = std::fs::read_to_string(&path)?;
    let mut changed = false;
    let mut output = String::new();
    for line in original.lines() {
        let Some((key, value)) = line.split_once('=') else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        if !changed && key == field {
            let replacement = if value == "0".repeat(value.len()) {
                "1".repeat(value.len())
            } else {
                "0".repeat(value.len())
            };
            output.push_str(key);
            output.push('=');
            output.push_str(&replacement);
            output.push('\n');
            changed = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if !changed {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("integrity sidecar has no {field} field"),
        ));
    }
    std::fs::write(path, output)
}
