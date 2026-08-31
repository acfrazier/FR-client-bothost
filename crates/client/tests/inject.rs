//! Task 2 boot inject: load a Task 1 snapshot into the process-wide model
//! and anim-frame stores. Uses the exact `[id][len][raw]` record format and
//! checks the stores end up populated (or a missing snapshot errors cleanly).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use client::dash3d::model::ModelProvider;
use client::dash3d::{AnimFrame, Model};
use client::unpack::{load_snapshot, unpack_cache, version_hash};

/// Serialise the store-touching tests in this binary: the model/anim stores
/// are process-wide, so concurrent loads would interleave their assertions.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// `[id: u32 LE][len: u32 LE][len bytes]` — the exact Task 1 record format.
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

#[test]
fn loads_fake_snapshot_into_stores() {
    let _guard = lock();

    let cache = tmp_dir("inject-fake-cache");
    let out = tmp_dir("inject-fake-out");
    let versionlist = b"fake versionlist content";
    std::fs::write(cache.join("versionlist"), versionlist).unwrap();

    let version = version_hash(versionlist);
    let dir = out.join(&version);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("models.bin"),
        [model_record(30002), model_record(30003)].concat(),
    )
    .unwrap();
    std::fs::write(dir.join("anims.bin"), anim_record()).unwrap();

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
    std::fs::write(cache.join("versionlist"), b"missing-snapshot").unwrap();

    let err = load_snapshot(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("models.bin"),
        "missing snapshot dir must be an Err naming the read, got: {err}"
    );
}

#[test]
fn empty_snapshot_file_is_an_error() {
    let _guard = lock();

    let cache = tmp_dir("inject-empty-cache");
    let out = tmp_dir("inject-empty-out");
    let versionlist = b"empty snapshot versionlist";
    std::fs::write(cache.join("versionlist"), versionlist).unwrap();

    let version = version_hash(versionlist);
    let dir = out.join(&version);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("models.bin"), b"").unwrap();
    std::fs::write(dir.join("anims.bin"), b"").unwrap();

    let err = load_snapshot(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap_err();
    assert!(
        err.to_string().contains("empty"),
        "empty snapshot file must be an Err, got: {err}"
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
