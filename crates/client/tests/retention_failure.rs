//! Retention is best effort: a verified cold fill must survive an unwritable
//! retained snapshot root (the stand-in for a full disk from the
//! REVIEW-STARTUP-SNAPSHOT-1 probe). Adapted from that scratch probe as a
//! permanent Unix regression: the negotiation directory is made read-only, so
//! each preparation keeps its verified runtime copy and warns instead of
//! failing, and nothing completed is retained.
//!
//! Unix only: the failure is induced with a read-only directory mode.
#![cfg(unix)]
#[path = "support/startup_snapshot.rs"]
mod support;

use client::content_identity::DecodedContentIdentity;
use client::io::{ClientRevision, JagFile, OnDemand};
use client::unpack::PreparedRuntimeCache;
use client::Transport;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Restores writability of the read-only negotiation directory even when an
/// assertion fails, so the temp dir can always be removed.
struct WritableOnDrop {
    path: PathBuf,
}

impl Drop for WritableOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o755));
    }
}

#[test]
fn retention_failure_keeps_verified_runtime_without_retaining() {
    // Capture the real launch warnings without installing a process-global
    // logger or mutating the parent's stderr while other suites run.
    if std::env::var_os("STARTUP_RETENTION_WARNING_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "retention_failure_keeps_verified_runtime_without_retaining",
                "--nocapture",
            ])
            .env("STARTUP_RETENTION_WARNING_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "read-only preparation failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            stderr
                .matches("warning: could not save game assets for reuse:")
                .count(),
            2,
            "exactly one retention warning per preparation: {stderr}"
        );
        return;
    }
    let temp = support::TempDir::new("retention-failure");
    let source = temp.path().join("cache");
    let fixture = support::PackSet::new(97);
    fixture.write_to(&source).unwrap();
    let http = support::HttpServer::start(fixture.clone());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let per_fill = fixture.entries.len();

    // Learn the retained layout from an ordinary successful preparation.
    let learn_root = temp.path().join("learn");
    let learned = support::prepare(
        &source,
        &learn_root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    let learned_identity = learned.identity.clone();
    let learned_maps = std::fs::read(learned.snapshot_dir.join("maps.bin")).unwrap();
    let retained = learned.persist_dir.parent().unwrap().to_owned();
    let rel = retained.strip_prefix(&learn_root).unwrap().to_owned();
    drop(learned);
    assert_eq!(entries.request_count(), per_fill);

    // Same layout in a fresh root, but the negotiation directory is read-only,
    // so publishing the retained copy (and its overlay dir) fails.
    let root = temp.path().join("home");
    let negotiation = root.join(rel.parent().unwrap().parent().unwrap());
    std::fs::create_dir_all(&negotiation).unwrap();
    std::fs::set_permissions(&negotiation, std::fs::Permissions::from_mode(0o555)).unwrap();
    let _restore = WritableOnDrop {
        path: negotiation.clone(),
    };

    let before_first = entries.request_count();
    let first = support::prepare(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .expect("a verified runtime copy must survive retention failure");
    assert_verified_runtime(&first, &learned_identity, &learned_maps);
    assert_serves_fixture_map(&first, entries.port(), &fixture);
    assert_eq!(
        first.persist_dir,
        first.snapshot_dir.join("ondemand"),
        "persistence falls back to the private runtime copy"
    );
    assert_eq!(
        entries.request_count() - before_first,
        per_fill,
        "one full entry fill per attempt"
    );
    assert_no_completed_snapshot(&root);
    drop(first);

    let before_second = entries.request_count();
    let second = support::prepare(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .expect("the next launch retries the fill instead of reusing a retained copy");
    assert_verified_runtime(&second, &learned_identity, &learned_maps);
    assert_serves_fixture_map(&second, entries.port(), &fixture);
    assert_eq!(
        second.persist_dir,
        second.snapshot_dir.join("ondemand"),
        "persistence falls back to the private runtime copy"
    );
    assert_eq!(
        entries.request_count() - before_second,
        per_fill,
        "one full entry fill per attempt"
    );
    assert_no_completed_snapshot(&root);
    drop(second);
}

fn assert_verified_runtime(
    prepared: &Arc<PreparedRuntimeCache>,
    identity: &DecodedContentIdentity,
    maps: &[u8],
) {
    assert_eq!(
        prepared.identity, *identity,
        "canonical identity comes from the verified runtime bytes"
    );
    assert_eq!(
        std::fs::read(prepared.snapshot_dir.join("maps.bin")).unwrap(),
        maps,
        "the private runtime copy keeps the decoded maps"
    );
}

fn assert_serves_fixture_map(
    prepared: &Arc<PreparedRuntimeCache>,
    game_port: u16,
    fixture: &support::PackSet,
) {
    let jag = JagFile::new(std::fs::read(prepared.jag_dir.join("versionlist")).unwrap());
    let mut maps = OnDemand::new_bound(
        &jag,
        Transport::Tcp,
        ClientRevision::R289,
        "127.0.0.1",
        game_port,
        prepared.jag_dir.to_str().unwrap(),
        &prepared.identity.content_id_hex(),
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
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(data, fixture.entries[3].payload);
}

/// No completed retained namespace may exist under the fresh root: completed
/// candidates carry a `manifest` with `complete=1` plus an `integrity` file,
/// and private runtime / staging trees are dot-prefixed and never candidates.
fn assert_no_completed_snapshot(root: &Path) {
    fn visit(root: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.join("integrity").is_file()
                && std::fs::read_to_string(path.join("manifest"))
                    .is_ok_and(|text| text.lines().any(|line| line == "complete=1"))
            {
                found.push(path.clone());
            }
            visit(&path, found);
        }
    }

    let mut found = Vec::new();
    visit(root, &mut found);
    assert!(
        found.is_empty(),
        "retention failure must not leave a completed namespace: {found:?}"
    );
}
