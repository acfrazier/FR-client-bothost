//! Boot inject: load a prepared snapshot into the process-wide model and
//! anim-frame stores. Uses the exact `[id][len][raw]` record format and the
//! completeness contract the writer publishes: a snapshot is only loadable
//! when its completion manifest matches the selected cache version and every
//! payload file is present with the recorded size.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use client::dash3d::model::ModelProvider;
use client::dash3d::{AnimFrame, Model};
use client::unpack::{load_snapshot, snapshot_state, unpack_cache, version_hash, SnapshotState};

/// The jag packs a complete snapshot carries, in the writer's order.
const JAGS: [&str; 8] = [
    "config",
    "interface",
    "textures",
    "media",
    "title",
    "sounds",
    "wordenc",
    "versionlist",
];

/// Serialise the store-touching tests in this binary: the model/anim stores
/// are process-wide, so concurrent loads would interleave their assertions.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// `[id: u32 LE][len: u32 LE][len bytes]` — the exact record format.
fn encode_record(id: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// A minimal valid model: 18-byte trailer, all counts zero.
fn model_record(id: u32) -> Vec<u8> {
    encode_record(id, &[0u8; 18])
}

/// A minimal anim frame stream holding one frame id 30001 (the record id is
/// unused by `AnimFrame::unpack`, which reads the frame id from the data).
/// The in-record `Packet` integers are big-endian (`g2`), unlike the
/// little-endian `[id][len]` framing.
fn anim_record() -> Vec<u8> {
    let data: [u8; 15] = [
        0x00, 0x01, // total = 1
        0x75, 0x31, // frame id = 30001
        0x00, // group count = 0
        0x00, // delay = 0
        0x00, // base size = 0
        0x00, 0x03, // head length = 3
        0x00, 0x00, // tran1 length = 0
        0x00, 0x00, // tran2 length = 0
        0x00, 0x01, // del length = 1
    ];
    encode_record(0, &data)
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("274bot-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cache_dir() -> Option<String> {
    let cache = client::cache_dir();
    cache
        .join("versionlist")
        .is_file()
        .then(|| cache.display().to_string())
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Lay out the snapshot a successful preparation would publish for this
/// versionlist: every jag copy, both record streams, and the completion
/// manifest written last. Returns the version directory.
fn plant_complete_snapshot(out: &Path, versionlist: &[u8], models: &[u8], anims: &[u8]) -> PathBuf {
    let version = version_hash(versionlist);
    let dir = out.join(&version);
    std::fs::create_dir_all(&dir).unwrap();
    let mut manifest = String::new();
    manifest.push_str(&format!("version={version}\n"));
    manifest.push_str(&format!("dir={}\n", dir.display()));
    manifest.push_str("source=test-fixture\n");
    manifest.push_str("complete=1\n");
    for name in JAGS {
        let bytes: &[u8] = if name == "versionlist" {
            versionlist
        } else {
            &[0u8, 0, 6, 0, 0, 6, 0, 0]
        };
        std::fs::write(dir.join(name), bytes).unwrap();
        manifest.push_str(&format!("jag.{name}.bytes={}\n", bytes.len()));
    }
    std::fs::write(dir.join("models.bin"), models).unwrap();
    std::fs::write(dir.join("anims.bin"), anims).unwrap();
    std::fs::write(dir.join("midi.bin"), [0u8]).unwrap();
    std::fs::write(dir.join("maps.bin"), [0u8]).unwrap();
    for (name, bytes) in [("models", models), ("anims", anims)] {
        manifest.push_str(&format!("{name}.total=1\n"));
        manifest.push_str(&format!("{name}.unpacked=1\n"));
        manifest.push_str(&format!("{name}.skipped=0\n"));
        manifest.push_str(&format!("{name}.bytes={}\n", bytes.len()));
    }
    for name in ["midi", "maps"] {
        manifest.push_str(&format!("{name}.total=1\n"));
        manifest.push_str(&format!("{name}.unpacked=1\n"));
        manifest.push_str(&format!("{name}.skipped=0\n"));
        manifest.push_str(&format!("{name}.bytes=1\n"));
    }
    std::fs::write(dir.join("manifest"), manifest).unwrap();
    dir
}

#[test]
fn loads_fake_snapshot_into_stores() {
    let _guard = lock();

    let cache = tmp_dir("inject-fake-cache");
    let out = tmp_dir("inject-fake-out");
    let versionlist = b"fake versionlist content";
    std::fs::write(cache.join("versionlist"), versionlist).unwrap();
    plant_complete_snapshot(
        &out,
        versionlist,
        &[model_record(30002), model_record(30003)].concat(),
        &anim_record(),
    );

    // Pre-size the anim store so the high (collision-free) frame id fits.
    AnimFrame::init(40000);

    let loaded = load_snapshot(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
    assert_eq!(loaded.models, 2);
    assert_eq!(loaded.anim_records, 1);

    assert!(Model::load(30002).is_some(), "model 30002 loadable");
    assert!(Model::load(30003).is_some(), "model 30003 loadable");
    assert!(AnimFrame::get(30001).is_some(), "anim frame 30001 loadable");
}

/// `AnimFrame.unpack` indexes `list[frame_id]`. The in-record `total` is
/// the number of frames in that archive entry, not the global id space.
/// Cube seq 1133 uses frame 8483; a table sized to `total` (e.g. 16) panics.
#[test]
fn unpack_grows_the_frame_table_to_fit_the_frame_id() {
    let _guard = lock();
    AnimFrame::init(16);
    let rec = anim_record();
    let len = u32::from_le_bytes(rec[4..8].try_into().unwrap()) as usize;
    AnimFrame::unpack(&rec[8..8 + len]);
    assert!(
        AnimFrame::get(30001).is_some(),
        "frame id 30001 must land even when init() was only 16 slots"
    );
}

#[test]
fn missing_snapshot_dir_is_an_error() {
    let _guard = lock();

    let cache = tmp_dir("inject-missing-cache");
    let out = tmp_dir("inject-missing-out");
    let versionlist = b"missing-snapshot";
    std::fs::write(cache.join("versionlist"), versionlist).unwrap();

    let version = version_hash(versionlist);
    assert_eq!(
        snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
        SnapshotState::Missing
    );
    let err = load_snapshot(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap_err();
    assert!(
        err.to_string().contains(&version) && err.to_string().contains("snapshot missing"),
        "an absent snapshot must be an Err naming the selected version, got: {err}"
    );
}

/// A payload without its completion manifest is an interrupted (or pre-marker)
/// publication: it must never be admitted as ready, even though the directory
/// is nonempty and both record streams look loadable.
#[test]
fn snapshot_without_a_manifest_is_not_ready() {
    let _guard = lock();

    let cache = tmp_dir("inject-interrupted-cache");
    let out = tmp_dir("inject-interrupted-out");
    let versionlist = b"interrupted snapshot versionlist";
    std::fs::write(cache.join("versionlist"), versionlist).unwrap();
    let dir = plant_complete_snapshot(&out, versionlist, &model_record(30004), &anim_record());
    std::fs::remove_file(dir.join("manifest")).unwrap();

    let state = snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
    assert!(
        matches!(&state, SnapshotState::Incomplete(reason) if reason.contains("manifest")),
        "a snapshot with no completion manifest must be incomplete, got {state:?}"
    );
    let err = load_snapshot(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("incomplete"),
        "an interrupted snapshot must not load, got: {err}"
    );
}

/// A snapshot whose archive file is empty is incomplete: the readiness gate
/// counts records, not just the presence of a file.
#[test]
fn empty_snapshot_archive_is_not_ready() {
    let _guard = lock();

    let cache = tmp_dir("inject-empty-cache");
    let out = tmp_dir("inject-empty-out");
    let versionlist = b"empty snapshot versionlist";
    std::fs::write(cache.join("versionlist"), versionlist).unwrap();
    let dir = plant_complete_snapshot(&out, versionlist, &[], &anim_record());

    let state = snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
    assert!(
        matches!(&state, SnapshotState::Incomplete(reason) if reason.contains("models.bin")),
        "an empty models.bin must be incomplete, got {state:?}"
    );
    assert!(
        dir.join("manifest").is_file(),
        "the fixture's manifest itself is complete; the archive is what fails"
    );
    let err = load_snapshot(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("models.bin"),
        "an empty snapshot archive must be an Err naming it, got: {err}"
    );
}

#[test]
fn real_cache_round_trip() {
    let _guard = lock();

    let Some(cache) = cache_dir() else {
        return;
    };

    let out = tmp_dir("inject-roundtrip-out");
    let manifest = unpack_cache(&cache, out.to_str().unwrap()).unwrap();

    let loaded = load_snapshot(&cache, out.to_str().unwrap()).unwrap();
    assert_eq!(loaded.models, manifest.models.unpacked as usize);

    let bytes = std::fs::read(Path::new(&manifest.dir).join("models.bin")).unwrap();
    let first_id = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as i32;
    assert!(Model::load(first_id).is_some(), "model {first_id} loadable");
}

struct NoopProvider;
impl ModelProvider for NoopProvider {
    fn request_model(&mut self, _id: i32) {}
}

#[test]
fn model_init_does_not_drop_unpacked_meta() {
    let _guard = lock();
    Model::init(16, Box::new(NoopProvider));
    Model::unpack(3, Some(&[0u8; 18]));
    assert!(Model::load(3).is_some(), "unpacked before second init");
    Model::init(16, Box::new(NoopProvider));
    assert!(
        Model::load(3).is_some(),
        "a later client's Model::init must not wipe the process-wide snapshot"
    );
}

#[test]
fn anim_init_does_not_drop_unpacked_frames() {
    let _guard = lock();
    AnimFrame::init(16);
    let rec = anim_record();
    let len = u32::from_le_bytes(rec[4..8].try_into().unwrap()) as usize;
    AnimFrame::unpack(&rec[8..8 + len]);
    assert!(AnimFrame::get(30001).is_some());
    AnimFrame::init(16);
    assert!(
        AnimFrame::get(30001).is_some(),
        "a later client's AnimFrame::init must not wipe unpacked frames"
    );
}
