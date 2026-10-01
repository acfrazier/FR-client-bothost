//! Runtime cache identity, transfer refresh and immutable-owner regressions.
#[path = "support/startup_snapshot.rs"]
mod support;

use client::io::{ClientRevision, JagFile, OnDemand};
use client::{ClientSessionConfig, ClientSessionProfile, Transport};
use std::path::Path;
use std::sync::Arc;

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

#[test]
fn prepared_maps_reject_a_different_identity_revision_or_resource_path() {
    let temp = support::TempDir::new("runtime-map-binding");
    let source = temp.path().join("cache");
    let root = temp.path().join("snapshots");
    let fixture = support::PackSet::new(41);
    fixture.write_to(&source).unwrap();
    let http = support::HttpServer::start(fixture.clone());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let prepared = support::prepare(
        &source,
        &root,
        http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    let identity = prepared.identity.content_id_hex();
    let jag = JagFile::new(std::fs::read(prepared.jag_dir.join("versionlist")).unwrap());
    for (revision, id, cache) in [
        (
            ClientRevision::R274,
            identity.as_str(),
            prepared.jag_dir.as_path(),
        ),
        (
            ClientRevision::R289,
            "forged-identity",
            prepared.jag_dir.as_path(),
        ),
        (ClientRevision::R289, identity.as_str(), source.as_path()),
    ] {
        assert!(
            OnDemand::new_bound(
                &jag,
                Transport::Tcp,
                revision,
                "127.0.0.1",
                entries.port(),
                cache.to_str().unwrap(),
                id,
                None,
                None,
                Some(Arc::clone(&prepared.map_archive)),
            )
            .is_err(),
            "a verified map capability cannot be rebound"
        );
    }
    let valid = ClientSessionConfig {
        revision: ClientRevision::R289,
        transport: Transport::Tcp,
        game_host: "127.0.0.1".into(),
        game_port: entries.port(),
        asset_host: "127.0.0.1".into(),
        asset_port: http.port(),
        cache_dir: prepared.jag_dir.clone(),
        unpack_dir: prepared.unpack_root().to_owned(),
        rsa_modulus: "123456789".into(),
        rsa_exponent: "65537".into(),
        expected_crc: Some(prepared.expected_crc),
        content_id: identity,
        file_store_dir: None,
        ondemand_persist_dir: None,
        map_archive: Some(Arc::clone(&prepared.map_archive)),
    };
    for mutation in 0..4 {
        let mut changed = valid.clone();
        match mutation {
            0 => changed.revision = ClientRevision::R274,
            1 => changed.content_id = "forged-identity".into(),
            2 => changed.cache_dir = source.clone(),
            3 => changed.unpack_dir = root.clone(),
            _ => unreachable!(),
        }
        assert!(
            ClientSessionProfile::new(changed).is_err(),
            "a profile cannot redirect verified maps to different resource inputs"
        );
    }
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

#[test]
fn alternating_servers_reuse_both_retained_snapshots() {
    let temp = support::TempDir::new("runtime-alternating-servers");
    let source = temp.path().join("cache");
    let root = temp.path().join("snapshots");
    let fixture = support::PackSet::new(49);
    let a = support::HttpServer::start(fixture.clone());
    let b = support::HttpServer::start(fixture.recompressed_config());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let mut fills = Vec::new();
    for port in [a.port(), b.port(), a.port(), b.port()] {
        let before = entries.request_count();
        let prepared =
            support::prepare(&source, &root, port, entries.port(), ClientRevision::R289).unwrap();
        fills.push((entries.request_count() - before) / fixture.entries.len());
        // Separate launches release every owner before switching servers.
        drop(prepared);
    }
    assert_eq!(fills, [1, 1, 0, 0]);
    assert_eq!(entries.request_count(), fixture.entries.len() * 2);
    assert_eq!(a.jag_count("config"), 1);
    assert_eq!(b.jag_count("config"), 1);
}

#[test]
fn server_update_prunes_old_retention_only_after_its_last_owner_releases() {
    let temp = support::TempDir::new("runtime-pruning");
    let source = temp.path().join("cache");
    let root = temp.path().join("snapshots");
    let fixture = support::PackSet::new(51);
    let update = fixture.recompressed_config();
    fixture.write_to(&source).unwrap();
    let old_http = support::HttpServer::start(fixture.clone());
    let new_http = support::HttpServer::start(update.clone());
    let entries = support::EntryServer::start(fixture.entries.clone());
    let old = support::prepare(
        &source,
        &root,
        old_http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    let old_retained = old.persist_dir.parent().unwrap().to_owned();
    let old_namespace = old_retained.parent().unwrap().parent().unwrap().to_owned();
    let old_maps = std::fs::read(old.snapshot_dir.join("maps.bin")).unwrap();
    let updated = support::prepare(
        &source,
        &root,
        new_http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    let newest = updated.persist_dir.parent().unwrap().to_owned();
    assert_ne!(old_retained, newest);
    assert!(
        old_retained.join("manifest").exists(),
        "an active owner's overlay must survive"
    );
    assert_eq!(
        std::fs::read(old.snapshot_dir.join("maps.bin")).unwrap(),
        old_maps
    );
    let held_map = Arc::clone(&old.map_archive);
    drop(old);
    // Push the old namespace outside the three-copy reuse window while its
    // prepared map capability still owns it.
    for packs in [
        fixture.recompressed_versionlist(),
        update.recompressed_versionlist(),
    ] {
        let http = support::HttpServer::start(packs);
        drop(
            support::prepare(
                &source,
                &root,
                http.port(),
                entries.port(),
                ClientRevision::R289,
            )
            .unwrap(),
        );
    }
    let warm = support::prepare(
        &source,
        &root,
        new_http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    assert!(
        old_retained.exists(),
        "map capability retains the owner lease"
    );
    drop(held_map);
    let next = support::prepare(
        &source,
        &root,
        new_http.port(),
        entries.port(),
        ClientRevision::R289,
    )
    .unwrap();
    assert!(
        !old_namespace.exists(),
        "released old namespace is pruned on preparation"
    );
    assert!(newest.join("manifest").exists());
    assert_eq!(next.identity, warm.identity);
    assert_eq!(
        std::fs::read(next.snapshot_dir.join("maps.bin")).unwrap(),
        old_maps
    );
    assert_eq!(
        entries.request_count(),
        fixture.entries.len() * 4,
        "both warm preparations reuse"
    );
}
