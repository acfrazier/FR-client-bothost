//! One-shot unpacker for the local 274 `main_file_cache`. The bot host runs
//! this once per cache version: every non-empty file from the idx archives
//! is stripped of its gzip + 2-byte version trailer and written as a
//! length-prefixed record into an immutable snapshot under
//! `~/.274bot/unpack/<version>/`, so later boots inject the unpacked data
//! without re-reading the live (mutating) cache.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use crate::io::jagfile::JagFile;
use crate::io::ondemand::cache_read;

/// `main_file_cache` archive index per OnDemand archive (archive + 1):
/// models=1, anims=2, midi=3, maps=4. idx0 (the title index) is skipped.
const MODELS: i32 = 1;
const ANIMS: i32 = 2;
const MIDI: i32 = 3;
const MAPS: i32 = 4;

/// Jag pack files copied verbatim into the snapshot.
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

/// Per-archive totals for the manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveStats {
    /// File count from the versionlist (`{archive}_version` entries).
    pub total: u32,
    /// Records written (files whose `cache_read` returned data).
    pub unpacked: u32,
    /// `size == 0` idx entries: never preserved, skipped (not an error).
    pub skipped: u32,
}

/// The versioned snapshot result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Short stable hash of the versionlist jag content.
    pub version: String,
    /// The snapshot directory (`out_dir + "/" + version`).
    pub dir: String,
    pub models: ArchiveStats,
    pub anims: ArchiveStats,
    pub midi: ArchiveStats,
    pub maps: ArchiveStats,
}

/// A real read/write failure: missing inputs, an IO error, or a corrupt
/// (size>0 but unreadable) sector chain. `size==0` entries are skipped and
/// counted, never an error.
#[derive(Debug)]
pub struct UnpackError {
    message: String,
}

impl UnpackError {
    fn new(message: impl Into<String>) -> Self {
        UnpackError {
            message: message.into(),
        }
    }

    fn io(context: &str, e: io::Error) -> Self {
        UnpackError::new(format!("{context}: {e}"))
    }
}

impl fmt::Display for UnpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UnpackError {}

/// Unpack the local cache into `{out_dir}/{version}/` and return its
/// manifest. `cache_dir` is the jag pack directory (the engine's
/// `data/pack/client`); the `main_file_cache.*` store lives one level up and
/// is located by `cache_read`'s parent fallback.
pub fn unpack_cache(cache_dir: &str, out_dir: &str) -> Result<Manifest, UnpackError> {
    let versionlist_path = Path::new(cache_dir).join("versionlist");
    let versionlist =
        std::fs::read(&versionlist_path).map_err(|e| UnpackError::io("read versionlist", e))?;

    let version = version_hash(&versionlist);
    let dir_path = Path::new(out_dir).join(&version);
    std::fs::create_dir_all(&dir_path).map_err(|e| UnpackError::io("create snapshot dir", e))?;
    let dir = dir_path.to_string_lossy().into_owned();

    for name in JAGS {
        let src = Path::new(cache_dir).join(name);
        let bytes =
            std::fs::read(&src).map_err(|e| UnpackError::io(&format!("read jag {name}"), e))?;
        std::fs::write(Path::new(&dir).join(name), bytes)
            .map_err(|e| UnpackError::io(&format!("write jag {name}"), e))?;
    }

    let jag = JagFile::new(versionlist);
    let models = unpack_archive(cache_dir, &dir, &jag, "model_version", MODELS, "models.bin")?;
    let anims = unpack_archive(cache_dir, &dir, &jag, "anim_version", ANIMS, "anims.bin")?;
    let midi = unpack_archive(cache_dir, &dir, &jag, "midi_version", MIDI, "midi.bin")?;
    let maps = unpack_archive(cache_dir, &dir, &jag, "map_version", MAPS, "maps.bin")?;

    let manifest = Manifest {
        version,
        dir: dir.clone(),
        models,
        anims,
        midi,
        maps,
    };
    std::fs::write(Path::new(&dir).join("manifest"), manifest_text(&manifest))
        .map_err(|e| UnpackError::io("write manifest", e))?;
    Ok(manifest)
}

/// First 8 bytes of the SHA-256 of the versionlist content, hex-encoded.
/// Re-running is idempotent; any cache change yields a new version. Public
/// so tests can lay out a fake snapshot at the same path the loader reads.
pub fn version_hash(versionlist: &[u8]) -> String {
    let digest = Sha256::digest(versionlist);
    hex(&digest[..8])
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unpack_archive(
    cache_dir: &str,
    out_dir: &str,
    jag: &JagFile,
    table: &str,
    idx: i32,
    file_name: &str,
) -> Result<ArchiveStats, UnpackError> {
    let table_data = jag
        .read(table)
        .ok_or_else(|| UnpackError::new(format!("versionlist missing `{table}` table")))?;
    let total = (table_data.len() / 2) as u32;

    let store_dir = file_store_dir(cache_dir)
        .ok_or_else(|| UnpackError::new("main_file_cache.dat not found"))?;
    let idx_path = format!("{store_dir}/main_file_cache.idx{idx}");
    let mut idx_file = File::open(&idx_path).map_err(|e| UnpackError::io("open idx", e))?;
    let idx_entries = (idx_file
        .metadata()
        .map_err(|e| UnpackError::io("stat idx", e))?
        .len()
        / 6) as i32;

    let out_path = Path::new(out_dir).join(file_name);
    let mut out = File::create(&out_path).map_err(|e| UnpackError::io("create archive file", e))?;

    let mut stats = ArchiveStats {
        total,
        unpacked: 0,
        skipped: 0,
    };
    for file in 0..total as i32 {
        // A versionlist entry with no idx record is never preserved (the
        // midi table has one trailing version-0 entry past idx3).
        if file >= idx_entries {
            stats.skipped += 1;
            continue;
        }
        let size = idx_size(&mut idx_file, file).map_err(|e| UnpackError::io("read idx", e))?;
        if size <= 0 {
            stats.skipped += 1;
            continue;
        }
        let data = cache_read(cache_dir, idx, file).ok_or_else(|| {
            UnpackError::new(format!(
                "archive {idx} file {file}: idx size {size} but read failed (corrupt sector chain)"
            ))
        })?;
        let body = if data.len() >= 2 {
            &data[..data.len() - 2]
        } else {
            &data[..]
        };
        let raw = gunzip(body).ok_or_else(|| {
            UnpackError::new(format!("archive {idx} file {file}: corrupt gzip stream"))
        })?;
        encode_record(&mut out, file as u32, &raw)
            .map_err(|e| UnpackError::io("write record", e))?;
        stats.unpacked += 1;
    }
    Ok(stats)
}

/// Directory that actually holds `main_file_cache.dat`: `cache_dir` itself,
/// or (the engine layout) its parent — one level above the jag pack.
fn file_store_dir(cache_dir: &str) -> Option<String> {
    let here = Path::new(cache_dir);
    if here.join("main_file_cache.dat").is_file() {
        return Some(cache_dir.to_string());
    }
    let parent = here.parent()?;
    if parent.join("main_file_cache.dat").is_file() {
        return Some(parent.to_str()?.to_string());
    }
    None
}

fn idx_size(idx_file: &mut File, file: i32) -> io::Result<i32> {
    idx_file.seek(SeekFrom::Start(file as u64 * 6))?;
    let mut rec = [0u8; 6];
    idx_file.read_exact(&mut rec)?;
    Ok(((rec[0] as i32) << 16) + ((rec[1] as i32) << 8) + rec[2] as i32)
}

/// `gunzipSync(subarray(0, length - 2))` from TS `loop`/`OnDemand::loop_request`.
/// Unlike the network path (which keeps the raw bytes so a failure surfaces
/// downstream), the one-shot tool has no downstream: a corrupt stream must be
/// a hard error, so failure is signalled as `None`.
fn gunzip(src: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    GzDecoder::new(src).read_to_end(&mut out).ok()?;
    Some(out)
}

/// Length-prefixed record: `[id: u32 LE][len: u32 LE][len bytes]`.
fn encode_record<W: Write>(out: &mut W, id: u32, data: &[u8]) -> io::Result<()> {
    out.write_all(&id.to_le_bytes())?;
    out.write_all(&(data.len() as u32).to_le_bytes())?;
    out.write_all(data)
}

/// Inverse of `encode_record`; `None` on a truncated header or body.
fn decode_record<'a>(data: &'a [u8], pos: &mut usize) -> Option<(u32, &'a [u8])> {
    if *pos + 8 > data.len() {
        return None;
    }
    let id = u32::from_le_bytes(data[*pos..*pos + 4].try_into().ok()?);
    let len = u32::from_le_bytes(data[*pos + 4..*pos + 8].try_into().ok()?) as usize;
    *pos += 8;
    if *pos + len > data.len() {
        return None;
    }
    let bytes = &data[*pos..*pos + len];
    *pos += len;
    Some((id, bytes))
}

/// What the boot inject pulled from a snapshot: model + anim record counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Loaded {
    /// `models.bin` records unpacked into the process-wide model store.
    pub models: usize,
    /// `anims.bin` records unpacked into the process-wide anim store (each
    /// record may hold one or more frames).
    pub anim_records: usize,
}

/// Load the snapshot written by [`unpack_cache`] into the process-wide
/// model/animation stores so every model/anim is available before the scene
/// places its locs. `cache_dir` supplies the versionlist content used to
/// recompute the version (the same file `unpack_cache` hashed); `out_dir` is
/// the snapshot root (`~/.274bot/unpack`). A missing or empty snapshot dir,
/// or a truncated record, is `Err`.
pub fn load_snapshot(cache_dir: &str, out_dir: &str) -> Result<Loaded, UnpackError> {
    let versionlist_path = Path::new(cache_dir).join("versionlist");
    let versionlist =
        std::fs::read(&versionlist_path).map_err(|e| UnpackError::io("read versionlist", e))?;
    let version = version_hash(&versionlist);
    let dir_path = Path::new(out_dir).join(&version);

    let models = load_records(&dir_path.join("models.bin"), |id, raw| {
        crate::dash3d::Model::unpack(id as i32, Some(raw));
    })?;
    let anim_records = load_records(&dir_path.join("anims.bin"), |_id, raw| {
        crate::dash3d::AnimFrame::unpack(raw);
    })?;

    Ok(Loaded {
        models,
        anim_records,
    })
}

/// Process-wide snapshot inject: the first caller unpacks; later clients
/// (50-head wall) must not re-read `models.bin` / wipe the stores.
/// Returns `(loaded, first)` so `maininit` prints once.
pub fn load_snapshot_once(cache_dir: &str, out_dir: &str) -> Result<(Loaded, bool), UnpackError> {
    {
        let g = SNAPSHOT_INJECT.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(loaded) = *g {
            return Ok((loaded, false));
        }
    }
    let loaded = load_snapshot(cache_dir, out_dir)?;
    let mut g = SNAPSHOT_INJECT.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(existing) = *g {
        return Ok((existing, false));
    }
    *g = Some(loaded);
    Ok((loaded, true))
}

static SNAPSHOT_INJECT: Mutex<Option<Loaded>> = Mutex::new(None);

#[cfg(test)]
pub fn reset_snapshot_inject_for_tests() {
    *SNAPSHOT_INJECT.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

/// Read every `[id][len][raw]` record from `path` and hand each to `apply`.
/// Returns the record count; a truncated header or body, or an empty file, is
/// a hard error (Task 1 never writes partial or empty archives).
fn load_records(path: &Path, mut apply: impl FnMut(u32, &[u8])) -> Result<usize, UnpackError> {
    let bytes =
        std::fs::read(path).map_err(|e| UnpackError::io(&format!("read {}", path.display()), e))?;
    let mut count = 0usize;
    let mut pos = 0usize;
    while pos < bytes.len() {
        let Some((id, raw)) = decode_record(&bytes, &mut pos) else {
            return Err(UnpackError::new(format!(
                "{}: truncated record at byte {pos}",
                path.display()
            )));
        };
        apply(id, raw);
        count += 1;
    }
    if count == 0 {
        return Err(UnpackError::new(format!(
            "{}: no records (empty snapshot file)",
            path.display()
        )));
    }
    Ok(count)
}

fn manifest_text(m: &Manifest) -> String {
    let mut s = String::new();
    s.push_str(&format!("version={}\n", m.version));
    s.push_str(&format!("dir={}\n", m.dir));
    for (name, a) in [
        ("models", &m.models),
        ("anims", &m.anims),
        ("midi", &m.midi),
        ("maps", &m.maps),
    ] {
        s.push_str(&format!("{name}.total={}\n", a.total));
        s.push_str(&format!("{name}.unpacked={}\n", a.unpacked));
        s.push_str(&format!("{name}.skipped={}\n", a.skipped));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::io::jagfile::JagFile;

    #[test]
    fn load_snapshot_once_injects_only_the_first_call() {
        reset_snapshot_inject_for_tests();
        let pid = std::process::id();
        let cache = std::env::temp_dir().join(format!("274bot-once-cache-{pid}"));
        let out = std::env::temp_dir().join(format!("274bot-once-out-{pid}"));
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&cache).unwrap();
        let versionlist = b"once-inject versionlist";
        std::fs::write(cache.join("versionlist"), versionlist).unwrap();
        let version = version_hash(versionlist);
        let dir = out.join(&version);
        std::fs::create_dir_all(&dir).unwrap();
        let mut models = Vec::new();
        encode_record(&mut models, 1, &[0u8; 18]).unwrap();
        std::fs::write(dir.join("models.bin"), &models).unwrap();
        // One empty-ish anim record is still a truncated error; write a
        // minimal valid frame stream (same layout as tests/inject.rs).
        let anim_data: [u8; 15] = [
            0x00, 0x01, 0x75, 0x31, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01,
        ];
        let mut anims = Vec::new();
        encode_record(&mut anims, 0, &anim_data).unwrap();
        std::fs::write(dir.join("anims.bin"), &anims).unwrap();
        crate::dash3d::AnimFrame::init(16);

        let (a, first) =
            load_snapshot_once(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
        assert!(first);
        assert_eq!(a.models, 1);
        let (b, first) =
            load_snapshot_once(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
        assert!(!first);
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn record_round_trip() {
        let mut buf = Vec::new();
        let payload = b"gunzipped model bytes";
        encode_record(&mut buf, 7, payload).unwrap();
        encode_record(&mut buf, 42, b"").unwrap();

        let mut pos = 0;
        let (id0, data0) = decode_record(&buf, &mut pos).unwrap();
        assert_eq!(id0, 7);
        assert_eq!(data0, payload);

        let (id1, data1) = decode_record(&buf, &mut pos).unwrap();
        assert_eq!(id1, 42);
        assert!(data1.is_empty());
        assert_eq!(pos, buf.len());
    }

    #[test]
    fn decode_rejects_truncated_header() {
        let mut pos = 0;
        assert!(decode_record(&[1, 2, 3], &mut pos).is_none());
    }

    #[test]
    fn decode_rejects_truncated_body() {
        let mut buf = Vec::new();
        encode_record(&mut buf, 1, b"abcdef").unwrap();
        // Header (8 bytes) declares 6 bytes, but the slice ends after 9.
        let mut pos = 0;
        assert!(decode_record(&buf[..9], &mut pos).is_none());
    }

    #[test]
    fn version_hash_is_stable_and_short() {
        assert_eq!(version_hash(b"same"), version_hash(b"same"));
        assert_ne!(version_hash(b"same"), version_hash(b"diff"));
        assert_eq!(version_hash(b"x").len(), 16);
    }

    #[test]
    fn gunzip_rejects_truncated_stream() {
        // Valid gzip magic but a truncated header/stream must not decode.
        let bytes = [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(gunzip(&bytes).is_none());
    }

    #[test]
    fn corrupt_gzip_payload_is_an_error() {
        let versionlist = JagFile::new(jag(&[("model_version", &[0x00, 0x01])]));

        let tmp =
            std::env::temp_dir().join(format!("274bot-unpack-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // idx1: one entry, size=10, sector=1.
        std::fs::write(tmp.join("main_file_cache.idx1"), [0, 0, 10, 0, 0, 1]).unwrap();

        // dat: one 520-byte block at sector 1 (block offset 520).
        let mut dat = vec![0u8; 520 * 2];
        let b = 520;
        dat[b + 7] = 2; // archive_id = idx + 1
        dat[b + 8..b + 16].copy_from_slice(&[0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00]);
        dat[b + 16] = 0x00; // 2-byte version trailer
        dat[b + 17] = 0x01;
        std::fs::write(tmp.join("main_file_cache.dat"), dat).unwrap();

        let err = unpack_archive(
            tmp.to_str().unwrap(),
            tmp.to_str().unwrap(),
            &versionlist,
            "model_version",
            MODELS,
            "models.bin",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("gzip"),
            "corrupt gzip must be a hard error, got: {err}"
        );
    }

    fn jag(files: &[(&str, &[u8])]) -> Vec<u8> {
        let packed: Vec<Vec<u8>> = files.iter().map(|(_, d)| bz2(d)).collect();
        let data_len: usize = packed.iter().map(|d| d.len()).sum();
        let total = (8 + 10 * files.len() + data_len) as i32;
        let mut out = Vec::new();
        g3(&mut out, total);
        g3(&mut out, total);
        out.push((files.len() >> 8) as u8);
        out.push(files.len() as u8);
        for ((name, data), packed_data) in files.iter().zip(packed.iter()) {
            out.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
            g3(&mut out, data.len() as i32);
            g3(&mut out, packed_data.len() as i32);
        }
        for d in &packed {
            out.extend_from_slice(d);
        }
        out
    }

    fn bz2(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        enc.write_all(data).unwrap();
        let out = enc.finish().unwrap();
        assert!(out.starts_with(b"BZh"));
        out[4..].to_vec()
    }

    fn g3(out: &mut Vec<u8>, value: i32) {
        out.push((value >> 16) as u8);
        out.push((value >> 8) as u8);
        out.push(value as u8);
    }
}
