//! Task 1: one-shot cache unpacker. Snapshot the local 274 cache once into
//! an immutable versioned dir, then verify the record stream is real
//! gunzipped data (not the gzip + 2-byte trailer read off disk).

use std::path::Path;

use client::io::JagFile;
use client::unpack::unpack_cache;

fn cache_dir() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let cache = format!("{home}/experiments/Server/engine/data/pack/client");
    Path::new(&cache)
        .join("versionlist")
        .is_file()
        .then_some(cache)
}

#[test]
fn unpacks_versioned_snapshot() {
    let Some(cache) = cache_dir() else {
        return;
    };

    // Versionlist model count and the idx1 size==0 (never-preserved) count.
    let versionlist = std::fs::read(format!("{cache}/versionlist")).unwrap();
    let jag = JagFile::new(versionlist);
    let model_version = jag.read("model_version").expect("model_version table");
    let model_total = model_version.len() / 2;
    let idx1 = std::fs::read(
        Path::new(&cache)
            .parent()
            .unwrap()
            .join("main_file_cache.idx1"),
    )
    .unwrap();
    let size_zero = idx1
        .chunks(6)
        .filter(|r| r.len() == 6 && ((r[0] as u32) << 16) + ((r[1] as u32) << 8) + r[2] as u32 == 0)
        .count();

    let tmp = std::env::temp_dir().join(format!("274bot-unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let manifest = unpack_cache(&cache, tmp.to_str().unwrap()).unwrap();

    let dir = Path::new(&manifest.dir);
    assert!(dir.join("models.bin").is_file(), "models.bin exists");
    assert!(dir.join("anims.bin").is_file(), "anims.bin exists");
    assert!(dir.join("midi.bin").is_file(), "midi.bin exists");
    assert!(dir.join("maps.bin").is_file(), "maps.bin exists");
    assert!(dir.join("manifest").is_file(), "manifest exists");

    assert_eq!(manifest.models.total as usize, model_total);
    assert_eq!(manifest.models.unpacked as usize, model_total - size_zero);
    assert!(manifest.models.unpacked > 0, "models must unpack");
    assert_eq!(manifest.models.skipped as usize, size_zero);

    // Read one record back: gunzipped bytes are non-empty and no longer
    // start with the gzip magic (proving the strip + gunzip happened).
    let bytes = std::fs::read(dir.join("models.bin")).unwrap();
    assert!(bytes.len() >= 8, "record header present");
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    assert!(bytes.len() >= 8 + len, "record body present");
    let data = &bytes[8..8 + len];
    assert!(!data.is_empty(), "gunzipped model bytes non-empty");
    assert!(
        !data.starts_with(&[0x1f, 0x8b]),
        "record must be gunzipped, not gzip + trailer"
    );
}
