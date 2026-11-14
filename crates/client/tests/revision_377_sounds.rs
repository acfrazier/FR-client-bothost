//! Java-377 sound table boundaries and real-cache decode/constructor regression.
use client::client::{Client, ClientConfig, ClientRevision};
use client::config::{Cache, IfType, IfTypeMut};
use client::io::{JagFile, Packet};
use client::sound::JagFX;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

fn client(cache_dir: &Path, revision: ClientRevision, lowmem: bool) -> Client {
    Client::from_shared_with_revision(
        ClientConfig {
            cache_dir: cache_dir.to_str().unwrap().into(),
            lowmem,
            host: "127.0.0.1".into(),
            port: 43595,
            members: true,
        },
        Arc::new(Cache::default()),
        Arc::new(Vec::<Option<Box<IfType>>>::new()),
        Arc::new(Vec::<Option<Arc<IfTypeMut>>>::new()),
        revision,
    )
}

fn empty_sounds(ids: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for id in ids {
        bytes.extend_from_slice(&id.to_be_bytes());
        bytes.extend_from_slice(&[0; 14]); // ten absent tones, two loop bounds
    }
    bytes.extend_from_slice(&65535u16.to_be_bytes());
    bytes
}

#[test]
fn java_377_accepts_the_last_slot_and_keeps_empty_tables_shared() {
    let empty = JagFX::new_377();
    let mut loaded = JagFX::new_377();
    assert_eq!(empty.synth.len(), 5000);
    assert_eq!(empty.delays.len(), 5000);
    assert!(Arc::ptr_eq(&empty.synth, &loaded.synth));
    assert!(Arc::ptr_eq(&empty.delays, &loaded.delays));
    let mut packet = Packet::new(empty_sounds(&[1278, 4999]));
    loaded.init(&mut packet);
    assert_eq!(packet.pos, packet.length());
    assert!(loaded.synth[1278].is_some());
    assert!(loaded.synth[4999].is_some());
    assert!(empty.synth.iter().all(Option::is_none));
    assert_eq!(loaded.delays[4999], 0);
}

#[test]
fn constructor_and_memory_reconfiguration_select_revision_local_tables() {
    let dir = std::env::temp_dir().join(format!("377-sound-selection-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // A whole-archive-compressed single-member JAG, valid for every revision.
    let dat = empty_sounds(&[0]);
    let mut payload = Vec::new();
    payload.extend_from_slice(&1u16.to_be_bytes());
    payload.extend_from_slice(&JagFile::gen_hash("sounds.dat").to_be_bytes());
    for _ in 0..2 {
        payload.extend_from_slice(&(dat.len() as u32).to_be_bytes()[1..]);
    }
    payload.extend_from_slice(&dat);
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    encoder.write_all(&payload).unwrap();
    let compressed = encoder.finish().unwrap();
    let compressed = &compressed[4..];
    let mut archive = Vec::new();
    archive.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
    archive.extend_from_slice(&(compressed.len() as u32).to_be_bytes()[1..]);
    archive.extend_from_slice(compressed);
    std::fs::write(dir.join("sounds"), archive).unwrap();
    let legacy = client(&dir, ClientRevision::R274, false);
    let legacy_289 = client(&dir, ClientRevision::R289, false);
    let mut modern = client(&dir, ClientRevision::R377, true);
    assert_eq!(legacy.jagfx.synth.len(), 1000);
    assert_eq!(legacy_289.jagfx.synth.len(), 1000);
    assert!(Arc::ptr_eq(&legacy.jagfx.synth, &legacy_289.jagfx.synth));
    assert!(legacy.jagfx.synth[0].is_some());
    assert_eq!(modern.jagfx.synth.len(), 5000);
    assert!(modern.jagfx.synth.iter().all(Option::is_none));
    assert!(modern.configure_login_memory(false));
    assert!(modern.jagfx.synth[0].is_some());
    assert!(!Arc::ptr_eq(&legacy.jagfx.synth, &modern.jagfx.synth));
    let second = client(&dir, ClientRevision::R377, false);
    assert!(Arc::ptr_eq(&modern.jagfx.synth, &second.jagfx.synth));
    assert!(Arc::ptr_eq(&modern.jagfx.delays, &second.jagfx.delays));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn real_377_sounds_match_java_timing_and_construct_with_sound_enabled() {
    let dir = std::env::var_os("CLIENT_377_RUNTIME_CACHE_DIR")
        .or_else(|| std::env::var_os("CLIENT_377_CACHE_DIR"));
    let Some(dir) = dir else {
        eprintln!(
            "SKIP real 377 sounds: CLIENT_377_RUNTIME_CACHE_DIR and CLIENT_377_CACHE_DIR are not set"
        );
        return;
    };
    let dir = Path::new(&dir);
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/revision_377/sounds.json")).unwrap();
    let packed = std::fs::read(dir.join("sounds")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&packed)),
        fixture["sounds_sha256"]
    );
    let dat = JagFile::new(packed).read("sounds.dat").unwrap();
    let mut packet = Packet::new(dat);
    let mut decoded = JagFX::new_377();
    decoded.init(&mut packet);
    assert_eq!(packet.pos, packet.length());
    assert_eq!(json!(packet.pos), fixture["decoded_bytes"]);
    assert_eq!(json!(decoded.synth.len()), fixture["capacity"]);
    let rows: Vec<Value> = decoded
        .synth
        .iter()
        .enumerate()
        .map(|(id, sound)| {
            sound.as_ref().map_or(Value::Null, |sound| {
                let tones: Vec<Value> = sound
                    .tones
                    .iter()
                    .map(|tone| {
                        tone.as_ref()
                            .map_or(Value::Null, |tone| json!([tone.length, tone.start]))
                    })
                    .collect();
                json!([decoded.delays[id], sound.loop_begin, sound.loop_end, tones])
            })
        })
        .collect();
    assert_eq!(
        json!(rows.iter().filter(|r| !r.is_null()).count()),
        fixture["present"]
    );
    assert_eq!(
        json!(rows.iter().skip(1000).filter(|r| !r.is_null()).count()),
        fixture["high_ids"]
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(serde_json::to_vec(&rows).unwrap())),
        fixture["rows_sha256"]
    );
    for (id, expected) in fixture["samples"].as_object().unwrap() {
        let id = id.parse::<usize>().unwrap();
        assert_eq!(&rows[id], expected, "Java sound id {id}");
    }
    let first = client(dir, ClientRevision::R377, false);
    let second = client(dir, ClientRevision::R377, false);
    assert_eq!(first.jagfx.synth.len(), 5000);
    assert!(Arc::ptr_eq(&first.jagfx.synth, &second.jagfx.synth));
    for id in [1000, 1278, 1863, 2726] {
        assert!(first.jagfx.synth[id].is_some(), "constructor sound {id}");
    }
    eprintln!("377 real sounds: {} bytes fully consumed, {} sounds / {} above 999; Java timing digest matched; highmem shared constructor passed",
        packet.pos, fixture["present"], fixture["high_ids"]);
}
