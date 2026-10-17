//! Stepped cold cache fill against synthetic local servers: byte identity
//! with the one-call fill, the per-step bound (also against a slow update
//! server and a held retention lock), cancellation (also during the update
//! server handshake), the request window, the in-game worker's unchanged
//! Java cycle and the cold-fill worker's Java resend pace.
#[path = "support/startup_snapshot.rs"]
mod support;

use client::io::{ClientRevision, JagFile, OnDemand};
use client::unpack::{
    prepare_runtime_cache, RuntimeCacheFill, RuntimeCacheRequest, RuntimeCacheStep, FILL_WINDOW,
};
use client::Transport;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use support::{HttpServer, PacedEntryServer, PackSet, TempDir, PACK_NAMES};

/// Entries per archive for the multi-entry fixture: 418 required in total.
const COUNTS: [u16; 4] = [260, 90, 13, 90];

fn request<'a>(
    source: &'a Path,
    root: &'a Path,
    asset_port: u16,
    game_port: u16,
) -> RuntimeCacheRequest<'a> {
    RuntimeCacheRequest {
        revision: ClientRevision::R289,
        transport: Transport::Tcp,
        jag_source: source,
        snapshot_root: root,
        asset_host: "127.0.0.1",
        asset_port,
        game_host: "127.0.0.1",
        game_port,
    }
}

struct Fixture {
    packs: PackSet,
    http: HttpServer,
    source: TempDir,
}

fn fixture(label: &str) -> Fixture {
    let packs = PackSet::with_entries(37, COUNTS);
    let source = TempDir::new(&format!("{label}-source"));
    packs.write_to(source.path()).unwrap();
    Fixture {
        http: HttpServer::start(packs.clone()),
        packs,
        source,
    }
}

/// SHA-256 of every file in the one retained snapshot, by file name.
fn retained_digests(root: &Path) -> BTreeMap<String, String> {
    let dir = retained_dir(root).expect("a retained snapshot");
    let mut digests = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        if entry.path().is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            let name = entry.file_name().to_string_lossy().into_owned();
            // The manifest and its integrity digest name the snapshot's own
            // directory; compare them with that path removed.
            let bytes = if name == "manifest" || name == "integrity" {
                let text = String::from_utf8(bytes).unwrap();
                text.lines()
                    .filter(|line| !line.starts_with("dir=") && !line.starts_with("manifest."))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes()
            } else {
                bytes
            };
            digests.insert(name, format!("{:x}", Sha256::digest(&bytes)));
        }
    }
    digests
}

fn retained_dir(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if path.is_dir() && !hidden {
                stack.push(path);
            } else if entry.file_name() == "manifest" {
                return Some(dir);
            }
        }
    }
    None
}

fn hidden_entries(root: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with(prefix) {
                found.push(path.clone());
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    found
}

/// Step a fill to completion with `deadline` per step; the longest step.
fn step_to_ready(fill: &mut RuntimeCacheFill, deadline: Duration) -> (Duration, usize) {
    let mut longest = Duration::ZERO;
    let mut steps = 0;
    loop {
        let started = Instant::now();
        let step = fill.step(Some(started + deadline)).unwrap();
        longest = longest.max(started.elapsed());
        steps += 1;
        if let RuntimeCacheStep::Ready(_) = step {
            return (longest, steps);
        }
    }
}

/// The retained snapshot a stepped fill publishes is byte-identical to the
/// one-call fill's, and to the digests the batch-barrier fill produced for
/// this fixture before the change (recorded from the base client `1a1c765`).
#[test]
fn stepped_fill_is_byte_identical_to_the_one_call_fill() {
    let one_call = {
        let fixture = fixture("identity-one-call");
        let entries = PacedEntryServer::start(
            fixture.packs.entries.clone(),
            Duration::ZERO,
            Duration::ZERO,
        );
        let root = TempDir::new("identity-one-call-root");
        prepare_runtime_cache(&request(
            fixture.source.path(),
            root.path(),
            fixture.http.port(),
            entries.port(),
        ))
        .unwrap();
        retained_digests(root.path())
    };
    let stepped = {
        let fixture = fixture("identity-stepped");
        let entries = PacedEntryServer::start(
            fixture.packs.entries.clone(),
            Duration::ZERO,
            Duration::ZERO,
        );
        let root = TempDir::new("identity-stepped-root");
        let mut fill = RuntimeCacheFill::begin(&request(
            fixture.source.path(),
            root.path(),
            fixture.http.port(),
            entries.port(),
        ));
        step_to_ready(&mut fill, Duration::from_millis(2));
        retained_digests(root.path())
    };
    assert_eq!(stepped, one_call);
    let recorded: BTreeMap<String, String> = BEFORE_CHANGE_DIGESTS
        .iter()
        .map(|(name, digest)| (name.to_string(), digest.to_string()))
        .collect();
    assert_eq!(stepped, recorded);
}

/// Digests of the retained snapshot from the base client's batch-barrier
/// `prepare_runtime_cache` on this fixture (`COLD-FILL-STEPS` evidence,
/// `before-tests.log`).
const BEFORE_CHANGE_DIGESTS: &[(&str, &str)] = &[
    (
        "anims.bin",
        "8a924f006670fbf1a2ac7753c9ca06795604413ebf93fcdabc35fdc9576f301f",
    ),
    (
        "config",
        "3ba8a3a6a8ff5cecfe799350cddd3d00660035831d7d39cf8d5e9fea85e56754",
    ),
    (
        "integrity",
        "e523a25743ed801074853431b910e3bf1ed9ab3c1e40974dd56088d5a01cdf01",
    ),
    (
        "interface",
        "e51bc6dcf6af683cfc1cf0548afa4c07c792789c45fc1b92837f1f83470ce581",
    ),
    (
        "manifest",
        "da590067fd0819a9c08d4027733bc366c683f837aa2f3ee2c263a0ad1e4e037f",
    ),
    (
        "maps.bin",
        "bb072f6263bc7c36c37f01a9aacbaecfe7f1e0782e0f895589ab543427dc9376",
    ),
    (
        "media",
        "a06db8332e744c842f03487503fc4e66ac24cec4987b18acae9aebaaaa6df61d",
    ),
    (
        "midi.bin",
        "498b93f9e1be5af8979b915371af0c2b8db161f609266ff8adbf2e409595d05b",
    ),
    (
        "models.bin",
        "4eb3901412ead499af4be2159e35656527f877323cb55117a68b180b6a7b03dd",
    ),
    (
        "sounds",
        "dc771a7dfda5dc070722ef212cca17df54ca8f62068aa792c8339291d5e04fbf",
    ),
    (
        "textures",
        "70bd563fa1deb1fb40e7775fb8a63f8792b45a7230c8583d4d9b1bc53ccca8cd",
    ),
    (
        "title",
        "61c8a878d078524c6884de2e4e370c5bb22cfde33efecac4cca323370a719eec",
    ),
    (
        "versionlist",
        "6a6ea5285c077b55b8c8e172dfaca90aa88b7fd3210e9238d275748e41054ef5",
    ),
    (
        "wordenc",
        "9b4e55893d92ee2fa486f245cc6540146c859acf5835e14531993efe92dd5741",
    ),
];

/// Against a server that answers each request 30 ms late, no step runs past
/// its 10 ms deadline by more than the stated 250 ms bound, although the
/// whole fill takes well over a second. The root is on tmpfs: the bound is
/// the step's own work, not the disk's fsync latency.
#[test]
fn no_fill_step_outlasts_the_step_bound() {
    let fixture = fixture("bound");
    let entries = PacedEntryServer::start(
        fixture.packs.entries.clone(),
        Duration::from_millis(30),
        Duration::ZERO,
    );
    let root = TempDir::in_memory("bound-root");
    let mut fill = RuntimeCacheFill::begin(&request(
        fixture.source.path(),
        root.path(),
        fixture.http.port(),
        entries.port(),
    ));
    let started = Instant::now();
    let (longest, steps) = step_to_ready(&mut fill, Duration::from_millis(10));
    let total = started.elapsed();
    eprintln!("stepped fill: {steps} steps, longest {longest:?}, total {total:?}");
    assert!(total > Duration::from_secs(1), "fill took {total:?}");
    assert!(steps > 40, "{steps} steps");
    assert!(
        longest < Duration::from_millis(250),
        "longest step {longest:?} of {steps} (fill {total:?})"
    );
}

/// Dropping a fill mid-download releases the preparation lock, the
/// retention lease, the staging and runtime directories and the worker, and
/// publishes nothing; the next fill starts over and succeeds.
#[test]
fn cancelled_fill_releases_everything_and_the_next_fill_succeeds() {
    let fixture = fixture("cancel");
    let entries = PacedEntryServer::start(
        fixture.packs.entries.clone(),
        Duration::from_millis(2),
        Duration::ZERO,
    );
    let root = TempDir::new("cancel-root");
    let req = request(
        fixture.source.path(),
        root.path(),
        fixture.http.port(),
        entries.port(),
    );
    let mut fill = RuntimeCacheFill::begin(&req);
    let total = loop {
        match fill
            .step(Some(Instant::now() + Duration::from_millis(5)))
            .unwrap()
        {
            RuntimeCacheStep::Continue {
                written,
                total: Some(total),
            } if written > 0 => {
                assert!(written < total, "cancel before the fill finishes");
                break total;
            }
            RuntimeCacheStep::Continue { .. } => {}
            RuntimeCacheStep::Ready(_) => panic!("the fill finished before it was cancelled"),
        }
    };
    assert!(!hidden_entries(root.path(), ".runtime-").is_empty());
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", entries.port()), 1);
    drop(fill);

    assert_eq!(OnDemand::live_workers_for("127.0.0.1", entries.port()), 0);
    assert!(hidden_entries(root.path(), ".runtime-").is_empty());
    assert!(hidden_entries(root.path(), ".").iter().all(|path| {
        let name = path.file_name().unwrap().to_string_lossy();
        name.ends_with(".lock")
    }));
    assert_eq!(retained_dir(root.path()), None, "nothing is published");
    for lock in hidden_entries(root.path(), ".") {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock)
            .unwrap();
        file.try_lock()
            .unwrap_or_else(|error| panic!("{} still locked: {error}", lock.display()));
        file.unlock().unwrap();
    }
    let requested_before = entries.stats().requests;

    let prepared = prepare_runtime_cache(&req).unwrap();
    assert_eq!(prepared.source, "update-server");
    assert!(retained_dir(root.path()).is_some());
    assert!(
        entries.stats().requests >= requested_before + total as usize,
        "the next fill downloads every entry again"
    );
}

/// The fill keeps the worker's ten-request window full to the last request:
/// the server never holds more than ten requests, holds ten at some point,
/// and never goes idle after the first ten requests. (The worker's first
/// cycle can catch only part of the opening burst.) A batch barrier let the
/// connection go idle after every batch.
#[test]
fn fill_keeps_ten_requests_in_flight_and_never_idles() {
    let fixture = fixture("window");
    let entries = PacedEntryServer::start(
        fixture.packs.entries.clone(),
        Duration::ZERO,
        Duration::from_millis(3),
    );
    let root = TempDir::new("window-root");
    prepare_runtime_cache(&request(
        fixture.source.path(),
        root.path(),
        fixture.http.port(),
        entries.port(),
    ))
    .unwrap();
    let stats = entries.stats();
    eprintln!("window: {stats:?}");
    assert_eq!(stats.requests, fixture.packs.entries.len());
    const { assert!(FILL_WINDOW > 10) };
    assert_eq!(stats.max_outstanding, 10, "{stats:?}");
    assert!(
        stats.last_idle_restart.is_none_or(|request| request < 10),
        "{stats:?}"
    );
}

fn versionlist(packs: &PackSet) -> JagFile {
    JagFile::new(packs.packs[4].clone())
}

/// Request every entry at once and wait for all of them; the elapsed time.
fn drain_all(mut od: OnDemand, packs: &PackSet) -> Duration {
    let started = Instant::now();
    for entry in &packs.entries {
        od.request(i32::from(entry.archive), i32::from(entry.file));
    }
    let mut left = packs.entries.len();
    while left > 0 {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{left} missing"
        );
        od.run(false);
        while od.loop_request().is_some() {
            left -= 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let elapsed = started.elapsed();
    od.stop();
    elapsed
}

/// An in-game OnDemand keeps Java's cycle: a response that arrives 2 ms
/// after its request still waits for the worker's next 20 ms cycle, so the
/// 111 entries at ten per cycle take at least ten cycles. Only the
/// cold-fill worker wakes on the response.
#[test]
fn in_game_requests_keep_the_java_twenty_ms_cycle() {
    let packs = PackSet::with_entries(41, [120, 0, 0, 0]);
    let jag = versionlist(&packs);
    assert_eq!(packs.entries.len(), 111);
    let cache = TempDir::new("cycle-cache");

    let in_game = {
        let server = PacedEntryServer::start(
            packs.entries.clone(),
            Duration::from_millis(2),
            Duration::ZERO,
        );
        let od = OnDemand::new_bound(
            &jag,
            Transport::Tcp,
            ClientRevision::R289,
            "127.0.0.1",
            server.port(),
            cache.path().to_str().unwrap(),
            "cycle-in-game",
            None,
            None,
            None,
        )
        .unwrap();
        let elapsed = drain_all(od, &packs);
        assert!(server.stats().max_outstanding <= 10);
        elapsed
    };
    let cold_fill = {
        let server = PacedEntryServer::start(
            packs.entries.clone(),
            Duration::from_millis(2),
            Duration::ZERO,
        );
        let od = OnDemand::new_cold_fill(
            &jag,
            Transport::Tcp,
            ClientRevision::R289,
            "127.0.0.1",
            server.port(),
            cache.path().to_str().unwrap(),
            "cycle-cold-fill",
            None,
        )
        .unwrap();
        let elapsed = drain_all(od, &packs);
        assert!(server.stats().max_outstanding <= 10);
        elapsed
    };
    eprintln!("111-entry drain: in-game {in_game:?}, cold fill {cold_fill:?}");
    assert!(
        in_game >= Duration::from_millis(10 * 20),
        "in-game {in_game:?} (cold fill {cold_fill:?})"
    );
    assert!(
        cold_fill * 2 < in_game,
        "cold fill {cold_fill:?}, in-game {in_game:?}"
    );
}

/// The cold-fill worker wakes on every response, but it still counts Java's
/// 50-cycle resend in 20 ms cycles: an entry the server never answers is
/// sent once while the 184 other entries arrive, and again only about one
/// second later, as an in-game worker would.
#[test]
fn cold_fill_worker_resends_a_stuck_request_at_the_java_pace() {
    let packs = PackSet::with_entries(43, [200, 0, 0, 0]);
    let jag = versionlist(&packs);
    let cache = TempDir::new("resend-cache");
    let server = PacedEntryServer::start_holding(
        packs.entries.clone(),
        Duration::ZERO,
        Duration::from_millis(1),
        &[(0, 0)],
    );
    let mut od = OnDemand::new_cold_fill(
        &jag,
        Transport::Tcp,
        ClientRevision::R289,
        "127.0.0.1",
        server.port(),
        cache.path().to_str().unwrap(),
        "resend-cold-fill",
        None,
    )
    .unwrap();
    let started = Instant::now();
    for entry in &packs.entries {
        od.request(i32::from(entry.archive), i32::from(entry.file));
    }
    let mut left = packs.entries.len() - 1;
    while left > 0 {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{left} missing"
        );
        od.run(false);
        while od.loop_request().is_some() {
            left -= 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let others = started.elapsed();
    std::thread::sleep(Duration::from_millis(700).saturating_sub(started.elapsed()));
    let early = server.requests_for(0, 0);
    std::thread::sleep(Duration::from_millis(1600).saturating_sub(started.elapsed()));
    let late = server.requests_for(0, 0);
    od.stop();
    eprintln!("stuck entry: others done {others:?}, sent {early} by 0.7 s, {late} by 1.6 s");
    assert!(
        others < Duration::from_millis(700),
        "others took {others:?}"
    );
    assert_eq!(early, 1, "resent before Java's 50 cycles of 20 ms");
    assert!(late >= 2, "the Java resend still happens");
}

/// Step a fill to completion with an 8 ms deadline per step, as the host
/// does; the prepared cache, the longest step and the step count.
fn step_to_prepared(
    fill: &mut RuntimeCacheFill,
) -> (
    std::sync::Arc<client::unpack::PreparedRuntimeCache>,
    Duration,
    usize,
) {
    let mut longest = Duration::ZERO;
    let mut steps = 0;
    loop {
        let started = Instant::now();
        let step = fill.step(Some(started + HOST_STEP)).unwrap();
        longest = longest.max(started.elapsed());
        steps += 1;
        if let RuntimeCacheStep::Ready(prepared) = step {
            return (prepared, longest, steps);
        }
    }
}

/// The host's step deadline (`host-play` `TEMPLATE_STEP_WAIT`).
const HOST_STEP: Duration = Duration::from_millis(8);
/// The stated per-step bound of the stepped fill.
const STEP_BOUND: Duration = Duration::from_millis(250);

/// An update server that answers each HTTP request 350 ms late holds no
/// step past the bound: the `/crc` request and every JAG download continue
/// across steps, and the fill still completes with every pack downloaded.
#[test]
fn a_slow_update_server_never_holds_a_step_past_the_bound() {
    let packs = PackSet::with_entries(37, COUNTS);
    let delay = Duration::from_millis(350);
    let http = HttpServer::start_slow(packs.clone(), delay);
    let entries = PacedEntryServer::start(packs.entries.clone(), Duration::ZERO, Duration::ZERO);
    // An empty source: every pack is downloaded.
    let source = TempDir::in_memory("slow-http-source");
    let root = TempDir::in_memory("slow-http-root");
    let mut fill = RuntimeCacheFill::begin(&request(
        source.path(),
        root.path(),
        http.port(),
        entries.port(),
    ));
    let started = Instant::now();
    let (prepared, longest, steps) = step_to_prepared(&mut fill);
    let total = started.elapsed();
    eprintln!("slow update server: {steps} steps, longest {longest:?}, total {total:?}");
    assert_eq!(http.crc_count(), 1);
    for name in PACK_NAMES {
        assert_eq!(http.jag_count(name), 1, "{name} downloaded once");
    }
    assert_eq!(prepared.fetched.len(), PACK_NAMES.len());
    assert!(
        total >= delay * (PACK_NAMES.len() as u32 + 1),
        "every request waited for the server: {total:?}"
    );
    assert!(
        longest < STEP_BOUND,
        "longest step {longest:?} of {steps} (fill {total:?})"
    );
}

/// Hold `file`'s lock now and let it go after `hold` on another thread,
/// which hands the file back.
fn hold_lock(file: std::fs::File, hold: Duration) -> thread::JoinHandle<std::fs::File> {
    file.lock().unwrap();
    thread::spawn(move || {
        thread::sleep(hold);
        file.unlock().unwrap();
        file
    })
}

/// Another preparation that holds the revision's retention lock, first
/// while this fill registers its lease and again while it prunes, holds no
/// step past the bound. The fill waits in steps and completes once the
/// lock is free.
#[test]
fn a_held_retention_lock_never_holds_a_step_past_the_bound() {
    let packs = PackSet::with_entries(37, COUNTS);
    let http = HttpServer::start(packs.clone());
    let entries = PacedEntryServer::start(packs.entries.clone(), Duration::ZERO, Duration::ZERO);
    let source = TempDir::in_memory("retention-lock-source");
    packs.write_to(source.path()).unwrap();
    let root = TempDir::in_memory("retention-lock-root");
    let revision = root.path().join("revision-289");
    std::fs::create_dir_all(&revision).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(revision.join(".retention.lock"))
        .unwrap();
    let hold = Duration::from_millis(350);
    let mut fill = RuntimeCacheFill::begin(&request(
        source.path(),
        root.path(),
        http.port(),
        entries.port(),
    ));

    // Registration: the lock is held from before the first step.
    let started = Instant::now();
    let release = hold_lock(lock, hold);
    let mut register_longest = Duration::ZERO;
    let mut register_steps = 0;
    loop {
        let step_started = Instant::now();
        let step = fill.step(Some(step_started + HOST_STEP)).unwrap();
        register_longest = register_longest.max(step_started.elapsed());
        register_steps += 1;
        match step {
            // Staged: the next steps publish, hash and retain.
            RuntimeCacheStep::Continue {
                written,
                total: Some(total),
            } if written == total => break,
            RuntimeCacheStep::Continue { .. } => {}
            RuntimeCacheStep::Ready(_) => panic!("ready before the pruning hold"),
        }
    }
    let registered = started.elapsed();
    let lock = release.join().unwrap();

    // Pruning: the lock is held again before the publication step.
    let started = Instant::now();
    let release = hold_lock(lock, hold);
    let (prepared, prune_longest, prune_steps) = step_to_prepared(&mut fill);
    let pruned = started.elapsed();
    release.join().unwrap();
    eprintln!(
        "held retention lock: registration {register_steps} steps, longest \
         {register_longest:?}, staged at {registered:?}; pruning {prune_steps} steps, \
         longest {prune_longest:?}, ready at {pruned:?}"
    );
    assert_eq!(prepared.source, "update-server");
    assert!(registered >= hold, "registration waited for the lock");
    assert!(pruned >= hold, "pruning waited for the lock");
    assert!(
        register_longest < STEP_BOUND && prune_longest < STEP_BOUND,
        "longest step {register_longest:?} (registration), {prune_longest:?} (pruning)"
    );
    assert!(
        register_steps > 10 && prune_steps > 10,
        "the waits ran in steps: {register_steps}, {prune_steps}"
    );
}

/// An update server that takes the cold-fill worker's handshake byte but
/// never answers does not delay a cancel: dropping the fill returns at once
/// and closes the connection, and no worker is left.
#[test]
fn cancelling_a_fill_does_not_wait_for_the_update_server_handshake() {
    let packs = PackSet::with_entries(37, [3, 0, 0, 0]);
    let http = HttpServer::start(packs.clone());
    let source = TempDir::in_memory("handshake-source");
    packs.write_to(source.path()).unwrap();
    let root = TempDir::in_memory("handshake-root");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let game_port = listener.local_addr().unwrap().port();
    let (handshake_tx, handshake_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut byte = [0u8; 1];
        socket.read_exact(&mut byte).unwrap();
        handshake_tx.send(byte[0]).unwrap();
        // Never answer; report when the client closes (or the 3 s timeout).
        let closed = matches!(socket.read(&mut byte), Ok(0));
        closed_tx.send((closed, Instant::now())).unwrap();
    });
    let mut fill =
        RuntimeCacheFill::begin(&request(source.path(), root.path(), http.port(), game_port));
    // Up to the step that starts the worker, then one fill step.
    while !matches!(
        fill.step(Some(Instant::now() + HOST_STEP)).unwrap(),
        RuntimeCacheStep::Continue { total: Some(_), .. }
    ) {}
    fill.step(Some(Instant::now() + HOST_STEP)).unwrap();
    let handshake = handshake_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(handshake, 15, "the worker sent Java's handshake byte");
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", game_port), 1);

    let started = Instant::now();
    drop(fill);
    let cancel = started.elapsed();
    let (closed, closed_at) = closed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    server.join().unwrap();
    let closed_after = closed_at.saturating_duration_since(started);
    eprintln!(
        "cancel during the handshake: drop {cancel:?}, peer closed={closed} after {closed_after:?}"
    );
    assert!(
        cancel < STEP_BOUND,
        "cancel waited for the server: {cancel:?}"
    );
    assert!(
        closed && closed_after < Duration::from_secs(1),
        "the connection closed with the cancel (closed={closed}, after {closed_after:?})"
    );
    assert_eq!(OnDemand::live_workers_for("127.0.0.1", game_port), 0);
}

/// Responses that arrive every 19 ms, just inside Java's 20 ms cycle, do
/// not stretch the cold-fill worker's resend: an entry that the server
/// never answers is sent again after Java's 51 cycles of 20 ms (1.02 s),
/// not after about twice that.
#[test]
fn paced_responses_keep_the_java_resend_interval() {
    let packs = PackSet::with_entries(43, [200, 0, 0, 0]);
    let jag = versionlist(&packs);
    let cache = TempDir::in_memory("resend-cadence-cache");
    let server = PacedEntryServer::start_holding(
        packs.entries.clone(),
        Duration::ZERO,
        Duration::from_millis(19),
        &[(0, 0)],
    );
    let mut od = OnDemand::new_cold_fill(
        &jag,
        Transport::Tcp,
        ClientRevision::R289,
        "127.0.0.1",
        server.port(),
        cache.path().to_str().unwrap(),
        "resend-cadence",
        None,
    )
    .unwrap();
    let started = Instant::now();
    for entry in &packs.entries {
        od.request(i32::from(entry.archive), i32::from(entry.file));
    }
    let mut resent = None;
    while started.elapsed() < Duration::from_millis(2500) {
        od.run(false);
        while od.loop_request().is_some() {}
        if server.requests_for(0, 0) >= 2 {
            resent = Some(started.elapsed());
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    let answered = server.stats().requests;
    od.stop();
    eprintln!("19 ms responses: first resend at {resent:?}, {answered} requests read");
    let resent = resent.expect("the held entry is sent again");
    assert!(
        resent >= Duration::from_millis(950) && resent < Duration::from_millis(1300),
        "first resend at {resent:?}, Java's 51 cycles of 20 ms are 1.02 s"
    );
}
