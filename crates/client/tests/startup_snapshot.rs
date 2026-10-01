//! Cross-process retained-snapshot behavior against synthetic local servers.
//! No public endpoint, client login, or real user cache is used.
#[path = "support/startup_snapshot.rs"]
mod support;

use client::content_identity::compute_decoded_content_identity;
use client::io::{ClientRevision, JagFile, OnDemand};
use client::Transport;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const WORKER: &str = "CLIENT_STARTUP_SNAPSHOT_WORKER";
const SOURCE: &str = "CLIENT_STARTUP_SNAPSHOT_SOURCE";
const ROOT: &str = "CLIENT_STARTUP_SNAPSHOT_ROOT";
const ASSET_PORT: &str = "CLIENT_STARTUP_SNAPSHOT_ASSET_PORT";
const GAME_PORT: &str = "CLIENT_STARTUP_SNAPSHOT_GAME_PORT";
const REVISION: &str = "CLIENT_STARTUP_SNAPSHOT_REVISION";
const RESULT_PREFIX: &str = "SS_RESULT\t";

#[derive(Debug)]
struct WorkerResult {
    identity: String,
    version: String,
    retained: PathBuf,
    runtime: PathBuf,
    map_digest: String,
}

struct WorkerChild(Option<Child>);

struct WorkerExit {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl WorkerExit {
    fn success(self) -> WorkerResult {
        assert!(
            self.status.success(),
            "worker failed: status={}\nstdout:\n{}\nstderr:\n{}",
            self.status,
            self.stdout,
            self.stderr
        );
        let line = self
            .stdout
            .lines()
            .find(|line| line.starts_with(RESULT_PREFIX))
            .unwrap_or_else(|| panic!("worker omitted result line:\n{}", self.stdout));
        let mut fields = line.split('\t');
        assert_eq!(fields.next(), Some("SS_RESULT"));
        WorkerResult {
            identity: fields.next().expect("identity").to_string(),
            version: fields.next().expect("version").to_string(),
            retained: PathBuf::from(fields.next().expect("retained path")),
            runtime: PathBuf::from(fields.next().expect("runtime path")),
            map_digest: fields.next().expect("decoded map digest").to_owned(),
        }
    }

    #[cfg(feature = "snapshot-test-hooks")]
    fn exit_code(self, expected: i32) {
        assert_eq!(
            self.status.code(),
            Some(expected),
            "worker exit mismatch: status={}\nstdout:\n{}\nstderr:\n{}",
            self.status,
            self.stdout,
            self.stderr
        );
    }
}

impl WorkerChild {
    fn spawn(
        source: &Path,
        root: &Path,
        asset_port: u16,
        game_port: u16,
        revision: ClientRevision,
        failpoint: Option<&str>,
    ) -> Self {
        let executable = std::env::current_exe().expect("integration test executable");
        let mut command = Command::new(executable);
        command
            .arg("--exact")
            .arg("process_worker")
            .arg("--ignored")
            .arg("--nocapture")
            .env(WORKER, "1")
            .env(SOURCE, source)
            .env(ROOT, root)
            .env(ASSET_PORT, asset_port.to_string())
            .env(GAME_PORT, game_port.to_string())
            .env(
                REVISION,
                match revision {
                    ClientRevision::R274 => "274",
                    ClientRevision::R289 => "289",
                },
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(failpoint) = failpoint {
            command.env("CLIENT_TEST_SNAPSHOT_FAILPOINT", failpoint);
        } else {
            command.env_remove("CLIENT_TEST_SNAPSHOT_FAILPOINT");
        }
        Self(Some(
            command.spawn().expect("spawn independent test process"),
        ))
    }

    fn finish(mut self, timeout: Duration) -> WorkerExit {
        let deadline = Instant::now() + timeout;
        let mut child = self.0.take().unwrap();
        let status = loop {
            match child.try_wait().expect("poll worker process") {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let pid = child.id();
                    let _ = child.kill(); // this exact child PID was recorded above
                    let _ = child.wait();
                    panic!("snapshot worker pid {pid} exceeded {timeout:?}");
                }
                None => thread::sleep(Duration::from_millis(10)),
            }
        };
        let mut stdout = String::new();
        let mut stderr = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        WorkerExit {
            status,
            stdout,
            stderr,
        }
    }
}

impl Drop for WorkerChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill(); // only the child owned by this guard
                let _ = child.wait();
            }
        }
    }
}

/// Invoked only by this test executable's child processes.
#[test]
#[ignore = "subprocess entrypoint; exercised by the process behavior tests"]
fn process_worker() {
    assert!(
        std::env::var_os(WORKER).is_some(),
        "child worker environment"
    );
    let source = PathBuf::from(std::env::var_os(SOURCE).expect("worker source"));
    let root = PathBuf::from(std::env::var_os(ROOT).expect("worker root"));
    let asset_port = std::env::var(ASSET_PORT)
        .expect("worker asset port")
        .parse()
        .unwrap();
    let game_port = std::env::var(GAME_PORT)
        .expect("worker game port")
        .parse()
        .unwrap();
    let revision = match std::env::var(REVISION).as_deref() {
        Ok("274") => ClientRevision::R274,
        Ok("289") => ClientRevision::R289,
        other => panic!("unsupported worker revision {other:?}"),
    };
    let prepared = support::prepare(&source, &root, asset_port, game_port, revision)
        .unwrap_or_else(|error| panic!("runtime preparation failed: {error}"));
    let identity = prepared.identity.content_id_hex();
    let version = prepared.version.clone();
    let retained = prepared
        .persist_dir
        .parent()
        .expect("retained snapshot parent")
        .to_owned();
    let runtime = prepared.unpack_root().to_owned();
    let jag = JagFile::new(std::fs::read(prepared.jag_dir.join("versionlist")).unwrap());
    let mut maps = OnDemand::new_bound(
        &jag,
        Transport::Tcp,
        revision,
        "127.0.0.1",
        game_port,
        prepared.jag_dir.to_str().unwrap(),
        &identity,
        None,
        None,
        Some(Arc::clone(&prepared.map_archive)),
    )
    .unwrap();
    maps.request(3, 0);
    let deadline = Instant::now() + Duration::from_secs(3);
    let data = loop {
        maps.run(false);
        if let Some(request) = maps.loop_request() {
            assert_eq!((request.archive, request.file), (3, 0));
            break request.data.expect("decoded map payload");
        }
        assert!(
            Instant::now() < deadline,
            "prepared map completion timed out"
        );
        thread::sleep(Duration::from_millis(5));
    };
    let map_digest = format!("{:x}", Sha256::digest(data));
    drop(prepared);
    assert!(runtime.exists(), "map capability owns the private runtime");
    drop(maps);
    assert!(
        !runtime.exists(),
        "last map capability releases the runtime"
    );
    println!(
        "\nSS_RESULT\t{identity}\t{version}\t{}\t{}\t{map_digest}",
        retained.display(),
        runtime.display()
    );
}

#[test]
fn ordinary_launch_persists_and_fresh_process_reuses_without_entry_fill() {
    let temp = support::TempDir::new("ordinary-launch");
    let source = temp.path().join("home/cache");
    let root = temp.path().join("home/snapshots");
    let fixture = support::PackSet::new(31);
    fixture.write_to(&source).unwrap();
    assert!(!source.join("main_file_cache.dat").exists());
    assert!(!temp.path().join("home/main_file_cache.dat").exists());

    let http = support::HttpServer::start(fixture.clone());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let first = WorkerChild::spawn(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();

    assert_eq!(entries.request_count(), fixture.entries.len());
    let expected_map = format!("{:x}", Sha256::digest(&fixture.entries[3].payload));
    assert_eq!(
        first.map_digest, expected_map,
        "cold scene receives the decoded map bytes"
    );
    assert_eq!(
        http.crc_count(),
        1,
        "cold preparation still negotiates /crc"
    );
    assert_published(&root, &first.retained, &fixture);
    assert!(
        !first.runtime.exists(),
        "only the process-owned runtime copy drops"
    );
    assert!(!contains_runtime_directory(&root));

    let second = WorkerChild::spawn(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();
    assert_eq!(second.identity, first.identity);
    assert_eq!(second.version, first.version);
    assert_eq!(second.retained, first.retained);
    assert_eq!(
        second.map_digest, expected_map,
        "fresh warm scene receives the same decoded map bytes"
    );
    assert_eq!(
        entries.request_count(),
        fixture.entries.len(),
        "warm process did not refill"
    );
    assert_eq!(http.crc_count(), 2, "each launch still negotiates /crc");
    assert!(!second.runtime.exists());
}

#[test]
fn deep_home_publishes_reuses_and_repairs_in_fresh_processes() {
    let temp = support::TempDir::new("deep-home");
    let source = temp.path().join("cache");
    let mut root = temp.path().to_owned();
    for _ in 0..12 {
        root = root.join("ordinary-home-path-component");
    }
    let fixture = support::PackSet::new(37);
    fixture.write_to(&source).unwrap();
    let http = support::HttpServer::start(fixture.clone());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let launch = || {
        WorkerChild::spawn(
            &source,
            &root,
            http.port(),
            entries.port(),
            ClientRevision::R289,
            None,
        )
        .finish(Duration::from_secs(20))
        .success()
    };

    let first = launch();
    assert_published(&root, &first.retained, &fixture);
    assert_eq!(entries.request_count(), fixture.entries.len());
    std::fs::remove_dir_all(&source).unwrap();

    let second = launch();
    assert_eq!(second.identity, first.identity);
    assert_eq!(second.retained, first.retained);
    assert_eq!(entries.request_count(), fixture.entries.len());
    for name in support::PACK_NAMES {
        assert_eq!(http.jag_count(name), 0, "retained JAG {name} reused");
    }

    support::flip_same_size_bytes(&first.retained.join("maps.bin")).unwrap();
    let repaired = launch();
    assert_eq!(repaired.identity, first.identity);
    assert_eq!(entries.request_count(), fixture.entries.len() * 2);
    assert_eq!(http.crc_count(), 3);
    assert_published(&root, &repaired.retained, &fixture);
    assert!(!first.runtime.exists());
    assert!(!second.runtime.exists());
    assert!(!repaired.runtime.exists());
    assert!(!contains_runtime_directory(&root));
}

#[test]
fn retained_corruption_never_authorizes_warm_reuse_or_identity() {
    #[derive(Clone, Copy, Debug)]
    enum Damage {
        MissingMarker,
        TruncatedBin,
        SameSizeBin,
        AlteredJag,
        ForgedDigest,
        ForgedManifestDigest,
    }
    const CASES: [Damage; 6] = [
        Damage::MissingMarker,
        Damage::TruncatedBin,
        Damage::SameSizeBin,
        Damage::AlteredJag,
        Damage::ForgedDigest,
        Damage::ForgedManifestDigest,
    ];

    for damage in CASES {
        let temp = support::TempDir::new(&format!("corruption-{damage:?}"));
        let source = temp.path().join("home/cache");
        let root = temp.path().join("home/snapshots");
        let fixture = support::PackSet::new(41);
        fixture.write_to(&source).unwrap();
        let http = support::HttpServer::start(fixture.clone());
        let entries = support::EntryServer::start(fixture.entries.clone());

        let first = WorkerChild::spawn(
            &source,
            &root,
            http.port(),
            entries.port(),
            ClientRevision::R289,
            None,
        )
        .finish(Duration::from_secs(20))
        .success();
        let victim = match damage {
            Damage::MissingMarker => first.retained.join("manifest"),
            Damage::TruncatedBin | Damage::SameSizeBin => first.retained.join("maps.bin"),
            Damage::AlteredJag => first.retained.join("config"),
            Damage::ForgedDigest | Damage::ForgedManifestDigest => first.retained.join("integrity"),
        };
        match damage {
            Damage::MissingMarker => std::fs::remove_file(&victim).unwrap(),
            Damage::TruncatedBin => {
                let bytes = std::fs::read(&victim).unwrap();
                assert!(bytes.len() > 9);
                std::fs::write(&victim, &bytes[..bytes.len() - 1]).unwrap();
            }
            Damage::SameSizeBin | Damage::AlteredJag => {
                support::flip_same_size_bytes(&victim)
                    .unwrap_or_else(|error| panic!("{damage:?} fixture damage: {error}"));
            }
            Damage::ForgedDigest => support::forge_integrity_digest(&first.retained).unwrap(),
            Damage::ForgedManifestDigest => {
                support::forge_manifest_digest(&first.retained).unwrap()
            }
        }

        let repaired = WorkerChild::spawn(
            &source,
            &root,
            http.port(),
            entries.port(),
            ClientRevision::R289,
            None,
        )
        .finish(Duration::from_secs(20))
        .success();
        assert_eq!(
            repaired.identity, first.identity,
            "{damage:?} retained bytes must not become content identity"
        );
        assert_eq!(repaired.retained, first.retained);
        assert_eq!(
            entries.request_count(),
            fixture.entries.len() * 2,
            "{damage:?} must be repaired from validated entry payloads"
        );
        assert_published(&root, &repaired.retained, &fixture);
        assert!(!repaired.runtime.exists());
    }
}

#[test]
fn packed_recompression_preserves_semantics_but_invalidates_transfer_selection() {
    let temp = support::TempDir::new("recompression");
    let source = temp.path().join("home/cache");
    let root = temp.path().join("home/snapshots");
    let original = support::PackSet::new(51);
    let recompressed = original.recompressed_versionlist();
    original.write_to(&source).unwrap();

    let original_http = support::HttpServer::start(original.clone());
    let recompressed_http = support::HttpServer::start(recompressed.clone());
    let entries = support::EntryServer::start(original.entries.clone());
    let first = WorkerChild::spawn(
        &source,
        &root,
        original_http.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();
    let second = WorkerChild::spawn(
        &source,
        &root,
        recompressed_http.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();

    assert_eq!(
        first.identity, second.identity,
        "decoded semantics are unchanged"
    );
    assert_ne!(
        first.version, second.version,
        "packed versionlist transfer changed"
    );
    assert_ne!(
        first.retained, second.retained,
        "new transfer selects a new snapshot"
    );
    assert_eq!(recompressed_http.crc_count(), 1);
    assert_eq!(
        recompressed_http.jag_count("versionlist"),
        1,
        "the changed packed versionlist is fetched and CRC-checked"
    );
    assert_eq!(
        entries.request_count(),
        original.entries.len() * 2,
        "a new transfer selection cannot reuse stale prepared records"
    );
    assert_published(&root, &second.retained, &recompressed);
}

#[test]
fn changed_revision_or_version_does_not_reuse_another_selection() {
    let temp = support::TempDir::new("revision-version");
    let source = temp.path().join("home/cache");
    let root = temp.path().join("home/snapshots");
    let original = support::PackSet::new(61);
    original.write_to(&source).unwrap();
    let http = support::HttpServer::start(original.clone());
    let entries = support::EntryServer::start(original.entries.clone());

    let first = WorkerChild::spawn(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();
    let other_revision = WorkerChild::spawn(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R274,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();
    assert_ne!(first.identity, other_revision.identity);
    assert_ne!(first.retained, other_revision.retained);
    assert!(first.retained.to_string_lossy().contains("revision-289"));
    assert!(other_revision
        .retained
        .to_string_lossy()
        .contains("revision-274"));
    assert_eq!(entries.request_count(), original.entries.len() * 2);

    let changed = support::PackSet::new(62);
    changed.write_to(&source).unwrap();
    let changed_http = support::HttpServer::start(changed.clone());
    let changed_entries = support::EntryServer::start(changed.entries.clone());
    let changed_version = WorkerChild::spawn(
        &source,
        &root,
        changed_http.port(),
        changed_entries.port(),
        ClientRevision::R289,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();
    assert_ne!(first.version, changed_version.version);
    assert_ne!(first.identity, changed_version.identity);
    assert_ne!(first.retained, changed_version.retained);
    assert_eq!(changed_entries.request_count(), changed.entries.len());
    assert_published(&root, &changed_version.retained, &changed);
    let identity_from_bytes =
        compute_decoded_content_identity(289, &changed_version.retained, &changed_version.retained)
            .unwrap()
            .content_id_hex();
    assert_eq!(changed_version.identity, identity_from_bytes);
    assert_ne!(identity_from_bytes, first.identity);

    support::forge_integrity_digest(&changed_version.retained).unwrap();
    let repaired_change = WorkerChild::spawn(
        &source,
        &root,
        changed_http.port(),
        changed_entries.port(),
        ClientRevision::R289,
        None,
    )
    .finish(Duration::from_secs(20))
    .success();
    assert_eq!(repaired_change.identity, identity_from_bytes);
    assert_ne!(repaired_change.identity, first.identity);
    assert_eq!(
        changed_entries.request_count(),
        changed.entries.len() * 2,
        "a forged recorded digest triggers repair, and identity still comes from actual bytes"
    );
    assert_published(&root, &repaired_change.retained, &changed);
}

#[test]
fn same_negotiation_race_publishes_once_and_returns_one_complete_snapshot() {
    let temp = support::TempDir::new("same-key-race");
    let source = temp.path().join("home/cache");
    let root = temp.path().join("home/snapshots");
    let fixture = support::PackSet::new(71);
    fixture.write_to(&source).unwrap();
    let http = support::HttpServer::start(fixture.clone());
    let gate = support::EntryGate::new();
    let entries = support::EntryServer::gated(fixture.entries.clone(), gate.clone());

    let first = WorkerChild::spawn(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    );
    let second = WorkerChild::spawn(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    );
    assert!(
        http.wait_for_crcs(2, Duration::from_secs(10)),
        "both workers negotiated"
    );
    assert!(
        gate.wait_for_arrivals(1, Duration::from_secs(10)),
        "one worker began fill"
    );
    let duplicate_fill_started = gate.wait_for_arrivals(2, Duration::from_millis(300));
    gate.release();

    let one = first.finish(Duration::from_secs(20)).success();
    let two = second.finish(Duration::from_secs(20)).success();
    assert!(
        !duplicate_fill_started,
        "only the lock owner may run EntrySource fill"
    );
    assert_eq!(one.identity, two.identity);
    assert_eq!(one.retained, two.retained);
    assert_eq!(entries.request_count(), fixture.entries.len());
    assert_published(&root, &one.retained, &fixture);
}

#[test]
fn different_negotiation_keys_do_not_serialize_entry_fills() {
    let temp = support::TempDir::new("independent-keys");
    let source_a = temp.path().join("home/cache-a");
    let source_b = temp.path().join("home/cache-b");
    let root = temp.path().join("home/snapshots");
    let fixture_a = support::PackSet::new(81);
    let fixture_b = fixture_a.recompressed_config();
    fixture_a.write_to(&source_a).unwrap();
    fixture_b.write_to(&source_b).unwrap();
    let http_a = support::HttpServer::start(fixture_a.clone());
    let http_b = support::HttpServer::start(fixture_b.clone());
    let gate = support::EntryGate::new();
    let entries = support::EntryServer::gated(fixture_a.entries.clone(), gate.clone());

    let first = WorkerChild::spawn(
        &source_a,
        &root,
        http_a.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    );
    let second = WorkerChild::spawn(
        &source_b,
        &root,
        http_b.port(),
        entries.port(),
        ClientRevision::R289,
        None,
    );
    assert!(http_a.wait_for_crcs(1, Duration::from_secs(10)));
    assert!(http_b.wait_for_crcs(1, Duration::from_secs(10)));
    let both_entered_fill = gate.wait_for_arrivals(2, Duration::from_secs(5));
    gate.release();

    let one = first.finish(Duration::from_secs(20)).success();
    let two = second.finish(Duration::from_secs(20)).success();
    assert!(
        both_entered_fill,
        "distinct negotiated CRC keys must acquire independent preparation locks"
    );
    assert_eq!(one.identity, two.identity, "only packed encoding differs");
    assert_ne!(
        one.retained.parent().and_then(Path::parent),
        two.retained.parent().and_then(Path::parent),
        "retained paths are namespaced by distinct negotiated keys"
    );
    assert_eq!(entries.request_count(), fixture_a.entries.len() * 2);
    assert_published(&root, &one.retained, &fixture_a);
    assert_published(&root, &two.retained, &fixture_b);
}

#[cfg(feature = "snapshot-test-hooks")]
#[test]
fn interrupted_replacement_at_each_publication_stage_is_repaired_by_fresh_process() {
    for failpoint in [
        "before-payload-completion",
        "during-replacement",
        "before-marker",
    ] {
        let temp = support::TempDir::new(&format!("crash-{failpoint}"));
        let source = temp.path().join("home/cache");
        let root = temp.path().join("home/snapshots");
        let fixture = support::PackSet::new(91);
        fixture.write_to(&source).unwrap();
        let http = support::HttpServer::start(fixture.clone());
        let entries = support::EntryServer::start(fixture.entries.clone());

        let active = support::prepare(
            &source,
            &root,
            http.port(),
            entries.port(),
            ClientRevision::R289,
        )
        .unwrap();
        let retained = active.persist_dir.parent().unwrap().to_owned();
        let active_maps = std::fs::read(active.snapshot_dir.join("maps.bin")).unwrap();
        let maps_path = retained.join("maps.bin");
        support::flip_same_size_bytes(&maps_path).unwrap();
        let damaged_maps = std::fs::read(&maps_path).unwrap();

        WorkerChild::spawn(
            &source,
            &root,
            http.port(),
            entries.port(),
            ClientRevision::R289,
            Some(failpoint),
        )
        .finish(Duration::from_secs(20))
        .exit_code(86);
        assert!(
            !retained_staging_directories(retained.parent().unwrap(), &active.version).is_empty(),
            "{failpoint} must leave its crashed staging directory"
        );

        if failpoint == "before-payload-completion" {
            assert_eq!(
                std::fs::read(&maps_path).unwrap(),
                damaged_maps,
                "pre-completion crash must leave target payloads untouched"
            );
        } else {
            assert!(
                !retained.join("manifest").exists(),
                "{failpoint} must leave the completion marker unavailable"
            );
            assert!(!has_complete_marker(&root));
        }
        assert_eq!(
            std::fs::read(active.snapshot_dir.join("maps.bin")).unwrap(),
            active_maps,
            "{failpoint} must not mutate an active Arc-owned snapshot"
        );

        let repaired = WorkerChild::spawn(
            &source,
            &root,
            http.port(),
            entries.port(),
            ClientRevision::R289,
            None,
        )
        .finish(Duration::from_secs(20))
        .success();
        assert_eq!(
            repaired.identity,
            active.identity.content_id_hex(),
            "{failpoint}"
        );
        assert_published(&root, &repaired.retained, &fixture);
        assert!(
            retained_staging_directories(retained.parent().unwrap(), &active.version).is_empty(),
            "{failpoint} recovery must sweep the dead owner's staging directory"
        );
        assert_eq!(
            entries.request_count(),
            fixture.entries.len() * 3,
            "{failpoint} must force a fresh validated fill after recovery"
        );
        assert_eq!(
            std::fs::read(active.snapshot_dir.join("maps.bin")).unwrap(),
            active_maps,
            "fresh-process recovery leaves the original Arc immutable"
        );
        drop(active);
        assert!(!contains_runtime_directory(&root));
    }
}
#[cfg(feature = "snapshot-test-hooks")]
fn retained_staging_directories(root: &Path, version: &str) -> Vec<PathBuf> {
    let prefix = format!(".{version}.staging-");
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            (file_type.is_dir() && entry.file_name().to_str()?.starts_with(&prefix))
                .then(|| entry.path())
        })
        .collect()
}

fn assert_published(root: &Path, retained: &Path, fixture: &support::PackSet) {
    assert!(
        retained.join("manifest").is_file(),
        "completion marker exists"
    );
    assert!(
        retained.join("integrity").is_file(),
        "integrity sidecar exists"
    );
    let ready = discover_ready_snapshots(root);
    assert!(
        ready.iter().any(|path| path == retained),
        "ready retained marker must be discovered recursively under {root:?}: {ready:?}"
    );
    for (name, expected) in support::PACK_NAMES.iter().zip(&fixture.packs) {
        let actual = std::fs::read(retained.join(name)).unwrap();
        assert_eq!(actual.as_slice(), expected.as_slice(), "JAG {name}");
    }
    for (bin, entry) in ["models.bin", "anims.bin", "midi.bin", "maps.bin"]
        .iter()
        .zip(&fixture.entries)
    {
        let bytes = std::fs::read(retained.join(bin)).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            u32::from(entry.file),
            "{bin} record ID"
        );
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            entry.payload.len(),
            "{bin} record length"
        );
        assert_eq!(&bytes[8..], entry.payload.as_slice(), "{bin} payload");
    }
}

#[cfg(feature = "snapshot-test-hooks")]
fn has_complete_marker(root: &Path) -> bool {
    !discover_ready_snapshots(root).is_empty()
}

fn discover_ready_snapshots(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue; // Private runtime and uncommitted staging trees are not shared candidates.
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest = path.join("manifest");
            if path.join("integrity").is_file()
                && std::fs::read_to_string(&manifest)
                    .is_ok_and(|text| text.lines().any(|line| line == "complete=1"))
            {
                found.push(path.clone());
            }
            visit(&path, found);
        }
    }

    let mut found = Vec::new();
    visit(root, &mut found);
    found
}

fn contains_runtime_directory(root: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(".runtime-") {
            return true;
        }
        if path.is_dir() && contains_runtime_directory(&path) {
            return true;
        }
    }
    false
}
