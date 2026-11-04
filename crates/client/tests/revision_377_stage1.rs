//! Standalone 377 contract tests. Existing shared module lists stay unchanged.
//!
//! The included files are the actual product modules, not copies. These tests
//! prove metadata, shared JAG decoding and CRC admission, not network dispatch,
//! config-record decoding, interface decoding, login or an in-game session.
//! Set CLIENT_377_CACHE_DIR to the eight extracted real 377 JAGs used to
//! generate fixtures/revision_377/cache.json. Only hosted CI may skip that asset.

use client::io::{jagfile, packet, ClientProt};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[path = "../src/io/cache_377.rs"]
mod cache_377;
#[path = "../src/io/client_prot_377.rs"]
mod client_prot_377;
#[path = "../src/io/server_prot_sizes_377.inc.rs"]
mod server_prot_sizes_377;

use cache_377::{load_archive, CacheArchive377, CacheLoadError377, CACHE_JAG_NAMES_377};
use client_prot_377::ClientProt377;
use packet::Packet;
use server_prot_sizes_377::SERVER_PROT_SIZES_377;

#[test]
fn complete_tables_match_the_java_contract() {
    let contract: Value =
        serde_json::from_str(include_str!("../../../docs/revision-377/protocol-377.json")).unwrap();
    assert_eq!(contract["revision"], 377);
    let inbound = contract["inbound"].as_array().unwrap();
    assert_eq!(inbound.len(), 256);
    for (id, row) in inbound.iter().enumerate() {
        assert_eq!(row["id"].as_u64().unwrap(), id as u64);
        assert_eq!(
            row["length"].as_i64().unwrap(),
            SERVER_PROT_SIZES_377[id] as i64
        );
    }
    assert_eq!(contract["unreachable_java_sizes"], serde_json::json!([0]));
    let outbound = contract["outbound"].as_array().unwrap();
    assert_eq!(outbound.len(), 80);
    assert_eq!(ClientProt377::ALL.len(), outbound.len());
    for (row, packet) in outbound.iter().zip(ClientProt377::ALL) {
        assert_eq!(row["id"].as_i64().unwrap(), packet.id as i64);
        assert_eq!(row["length"].as_i64().unwrap(), packet.length as i64);
    }
    assert!(ClientProt377::ALL
        .windows(2)
        .all(|pair| pair[0].id < pair[1].id));
}

#[test]
fn changed_fixed_widths_and_variable_headers_do_not_inherit_289() {
    assert_eq!(ClientProt377::ANTICHEAT_OPLOGIC2.length, 1);
    assert_eq!(ClientProt377::ANTICHEAT_OPLOGIC4.length, 3);
    assert_eq!(ClientProt377::ANTICHEAT_CYCLELOGIC3.length, 4);
    assert_eq!(ClientProt377::IDK_SAVEDESIGN.length, 13);
    assert_eq!(ClientProt377::TUT_CLICKSIDE.length, 1);
    assert_eq!(ClientProt377::RESUME_P_NAMEDIALOG.length, 8);
    assert_eq!(ClientProt377::MOVE_GAMECLICK.length, -1);
    assert_eq!(ClientProt377::MOVE_MINIMAPCLICK.length, -1);
    assert_eq!(ClientProt377::MOVE_OPCLICK.length, -1);
    assert_eq!(ClientProt377::CLIENT_CHEAT.length, -1);
    assert_ne!(
        ClientProt377::NO_TIMEOUT,
        client::io::ClientProt289::NO_TIMEOUT
    );
    assert_eq!(SERVER_PROT_SIZES_377[5], 0); // Java LOGOUT
    assert_eq!(SERVER_PROT_SIZES_377[222], 4); // Java REBUILD_NORMAL
    assert_eq!(SERVER_PROT_SIZES_377[53], -2); // Java REBUILD_REGION
}

#[test]
fn tables_describe_all_three_java_frame_header_forms() {
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/revision_377/manifest.json")).unwrap();
    let mut forms_seen = [false; 3];
    for case in manifest["cases"].as_array().unwrap() {
        let frame = decode_hex(case["frame_hex"].as_str().unwrap());
        let mut packet = Packet::new(frame.clone());
        let opcode = packet.g1() as usize;
        assert_eq!(opcode as u64, case["opcode"].as_u64().unwrap());
        let size = match SERVER_PROT_SIZES_377[opcode] {
            -1 => packet.g1(),
            -2 => packet.g2(),
            fixed => fixed,
        } as usize;
        let header = packet.pos;
        forms_seen[header - 1] = true;
        assert_eq!(header as u64, case["header_length"].as_u64().unwrap());
        if case["payload_hex"].is_null() {
            assert!(
                frame.len() < header + size,
                "declared payload is still pending"
            );
        } else {
            let payload = decode_hex(case["payload_hex"].as_str().unwrap());
            assert_eq!(size, payload.len());
            assert_eq!(&frame[header..], payload);
        }
    }
    assert!(forms_seen.into_iter().all(|seen| seen));
}

#[test]
fn jag_names_preserve_the_nine_slot_login_contract() {
    assert_eq!(
        CACHE_JAG_NAMES_377,
        client::io::cache_289::CACHE_JAG_NAMES_289
    );
    for (index, archive) in CacheArchive377::ALL.into_iter().enumerate() {
        assert_eq!(archive.crc_slot(), index + 1);
        assert_eq!(archive.jag_name(), CACHE_JAG_NAMES_377[index]);
        assert_eq!(
            CacheArchive377::from_jag_name(archive.jag_name()),
            Some(archive)
        );
    }
    assert_eq!(CacheArchive377::from_jag_name("client.jar"), None);
    assert_eq!(CacheArchive377::from_jag_name("model_version"), None);
}

#[test]
fn missing_jags_are_errors_not_synthetic_cache_tables() {
    let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/revision-377-deliberately-absent-cache");
    assert!(!missing.exists());
    let error = load_archive(&missing, CacheArchive377::Config, &[0; 9])
        .err()
        .unwrap();
    assert!(matches!(
        error,
        CacheLoadError377::Read {
            archive: CacheArchive377::Config,
            ..
        }
    ));
    assert!(std::error::Error::source(&error).is_some());
    assert!(error.to_string().starts_with("config: "));
}

#[test]
fn real_377_jags_and_every_member_match_independent_oracles() {
    let Some(dir) = real_cache_dir() else { return };
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/revision_377/cache.json")).unwrap();
    let mut checksums = [0; 9];
    for row in fixture["archives"].as_array().unwrap() {
        checksums[row["crc_slot"].as_u64().unwrap() as usize] =
            row["crc32"].as_u64().unwrap() as i32;
    }
    let mut member_count = 0;
    for (archive, oracle) in CacheArchive377::ALL
        .into_iter()
        .zip(fixture["archives"].as_array().unwrap())
    {
        assert_eq!(archive.jag_name(), oracle["name"].as_str().unwrap());
        let packed = std::fs::read(dir.join(archive.jag_name())).unwrap();
        assert_eq!(packed.len() as u64, oracle["length"].as_u64().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(&packed)),
            oracle["sha256"].as_str().unwrap()
        );
        let jag = load_archive(&dir, archive, &checksums).unwrap();
        let members = oracle["members"].as_array().unwrap();
        assert_eq!(jag.file_count as usize, members.len());
        for member in members {
            let index = member["index"].as_i64().unwrap() as i32;
            let data = jag.read_index(index).unwrap();
            assert_eq!(data.len() as u64, member["length"].as_u64().unwrap());
            assert_eq!(
                format!("{:x}", Sha256::digest(&data)),
                member["sha256"].as_str().unwrap(),
                "{} member {index}",
                archive.jag_name()
            );
            member_count += 1;
        }
        match archive {
            CacheArchive377::Config => {
                // Java's nine config unpackers read these g2 table counts.
                // This checks headers and member lookup, not config TLVs.
                for name in [
                    "flo.dat",
                    "idk.dat",
                    "loc.dat",
                    "npc.dat",
                    "obj.dat",
                    "seq.dat",
                    "spotanim.dat",
                    "varp.dat",
                    "varbit.dat",
                ] {
                    let mut buf = Packet::new(jag.read(name).unwrap());
                    let count = buf.g2();
                    assert!(count > 0, "{name}");
                    println!("377 config {name}: {count} records (header only)");
                }
                for name in ["loc.idx", "npc.idx", "obj.idx"] {
                    assert!(jag.read(name).is_some(), "{name}");
                }
            }
            CacheArchive377::Interface => {
                let mut buf = Packet::new(jag.read("data").unwrap());
                let count = buf.g2();
                assert!(count > 0);
                println!("377 interface data: {count} slots (header only)");
            }
            CacheArchive377::Versionlist => {
                for family in ["model", "anim", "midi", "map"] {
                    let version = jag.read(&format!("{family}_version")).unwrap();
                    let crc = jag.read(&format!("{family}_crc")).unwrap();
                    assert_eq!(version.len() % 2, 0);
                    assert_eq!(crc.len() % 4, 0);
                    assert_eq!(version.len() / 2, crc.len() / 4);
                    println!("377 {family} version/CRC entries: {}", version.len() / 2);
                }
                assert_eq!(jag.read("map_index").unwrap().len() % 7, 0);
                assert_eq!(jag.read("anim_index").unwrap().len() % 2, 0);
                assert!(jag.read("model_index").is_some());
                assert!(jag.read("midi_index").is_some());
            }
            _ => {}
        }
    }
    assert_eq!(member_count, 173);
    let mut wrong = checksums;
    wrong[1] ^= 1;
    let error = load_archive(&dir, CacheArchive377::Title, &wrong)
        .err()
        .unwrap();
    assert!(
        matches!(error, CacheLoadError377::Checksum { archive: CacheArchive377::Title, expected, actual } if expected == wrong[1] && actual == checksums[1])
    );
    assert!(std::error::Error::source(&error).is_none());
    assert!(error.to_string().starts_with("title: CRC32 "));
    println!("377 cache: 8 real JAGs and {member_count} independently hashed members matched");
}

fn real_cache_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLIENT_377_CACHE_DIR") {
        return Some(PathBuf::from(path));
    }
    if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        eprintln!("SKIP real 377 cache: CLIENT_377_CACHE_DIR is not set on hosted CI");
        return None;
    }
    panic!("CLIENT_377_CACHE_DIR must name the real eight-JAG revision-377 cache fixture");
}

fn decode_hex(hex: &str) -> Vec<u8> {
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    assert!(remainder.is_empty());
    pairs
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
