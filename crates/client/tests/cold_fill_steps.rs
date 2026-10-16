//! Stepped cold cache fill against synthetic local servers: byte identity
//! with the one-call fill, the per-step bound, cancellation, the request
//! window, and the in-game worker's unchanged Java cycle.
#[path = "support/startup_snapshot.rs"]
mod support;

use client::io::{ClientRevision, JagFile, OnDemand};
use client::unpack::{
    prepare_runtime_cache, RuntimeCacheFill, RuntimeCacheRequest, RuntimeCacheStep, FILL_WINDOW,
};
use client::Transport;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use support::{HttpServer, PacedEntryServer, PackSet, TempDir};

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
