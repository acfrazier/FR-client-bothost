//! Runtime cache identity, transfer refresh and immutable-owner regressions.
#[path = "support/startup_snapshot.rs"]
mod support;

use client::io::ClientRevision;
use std::path::Path;

#[test]
fn equivalent_transfer_refresh_is_owned_and_cleanup_follows_last_arc() {
    let temp = support::TempDir::new("runtime-transfer");
    let source = temp.path().join("home/cache");
    let root = temp.path().join("home/snapshots");
    let local = support::PackSet::new(11);
    let public = local.recompressed_versionlist();
    local.write_to(&source).unwrap();

    let local_http = support::HttpServer::start(local.clone());
    let public_http = support::HttpServer::start(public.clone());
    let entries = support::EntryServer::start(local.entries.clone());
    let prepared = support::prepare(
        &source,
        &root,
        local_http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    let expected = prepared.identity.clone();
    let original_runtime = prepared.unpack_root().to_owned();
    let original_bins = std::fs::read(prepared.snapshot_dir.join("maps.bin")).unwrap();
    let original_versionlist = std::fs::read(prepared.jag_dir.join("versionlist")).unwrap();

    let refreshed = support::prepare(
        &source,
        &root,
        public_http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    assert_eq!(refreshed.identity, expected);
    assert_eq!(refreshed.fetched, vec!["versionlist"]);
    for (index, name) in support::PACK_NAMES.iter().enumerate() {
        let original = &local.packs[index];
        assert_eq!(std::fs::read(source.join(name)).unwrap(), *original);
        assert_eq!(
            std::fs::read(refreshed.jag_dir.join(name)).unwrap(),
            public.packs[index]
        );
        assert_eq!(
            refreshed.expected_crc[index + 1],
            client::io::Packet::getcrc(&public.packs[index], 0, public.packs[index].len())
        );
    }
    assert_ne!(
        original_versionlist,
        std::fs::read(refreshed.jag_dir.join("versionlist")).unwrap()
    );
    assert_eq!(
        std::fs::read(prepared.snapshot_dir.join("maps.bin")).unwrap(),
        original_bins,
        "replacing a retained candidate cannot mutate an active Arc-owned runtime copy"
    );
    assert_eq!(entries.request_count(), local.entries.len() * 2);
    assert_eq!(public_http.jag_count("versionlist"), 1);

    let shared = prepared.clone();
    drop(prepared);
    assert!(original_runtime.exists());
    drop(shared);
    assert!(!original_runtime.exists());
}

#[test]
fn same_size_bin_replacement_is_repaired_without_changing_active_identity() {
    let temp = support::TempDir::new("runtime-same-size");
    let source = temp.path().join("home/cache");
    let root = temp.path().join("home/snapshots");
    let fixture = support::PackSet::new(21);
    fixture.write_to(&source).unwrap();
    let http = support::HttpServer::start(fixture.clone());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let first = support::prepare(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    let retained = first.persist_dir.parent().unwrap().to_owned();
    let active_identity = first.identity.clone();
    let active_maps = std::fs::read(first.snapshot_dir.join("maps.bin")).unwrap();
    support::flip_same_size_bytes(&retained.join("maps.bin")).unwrap();

    let second = support::prepare(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    assert_eq!(second.identity, active_identity);
    assert_eq!(
        std::fs::read(first.snapshot_dir.join("maps.bin")).unwrap(),
        active_maps,
        "retained repair must not mutate a previously returned Arc"
    );
    assert_eq!(
        std::fs::read(second.snapshot_dir.join("maps.bin")).unwrap(),
        active_maps
    );
    assert_eq!(entries.request_count(), fixture.entries.len() * 2);
    assert!(retained.join("manifest").is_file());
    assert!(retained.join("integrity").is_file());
}

#[test]
fn malformed_required_record_is_rejected_and_rebuilt_from_validated_entries() {
    let temp = support::TempDir::new("runtime-malformed-record");
    let source = temp.path().join("home/cache");
    let root = temp.path().join("home/snapshots");
    let fixture = support::PackSet::new(29);
    fixture.write_to(&source).unwrap();
    let http = support::HttpServer::start(fixture.clone());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let first = support::prepare(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    let retained = first.persist_dir.parent().unwrap().to_owned();
    let first_identity = first.identity.clone();
    let mut maps = std::fs::read(retained.join("maps.bin")).unwrap();
    maps[0] = 1; // Unknown record ID with the same declared size.
    std::fs::write(retained.join("maps.bin"), maps).unwrap();

    let repaired = support::prepare(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    assert_eq!(repaired.identity, first_identity);
    assert_eq!(entries.request_count(), fixture.entries.len() * 2);
    assert!(retained.join("manifest").is_file());
    assert!(retained.join("integrity").is_file());
    assert_valid_records(&repaired.snapshot_dir.join("maps.bin"));
}

fn assert_valid_records(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() >= 8);
    assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 0);
    assert_eq!(
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
        bytes.len() - 8
    );
}
