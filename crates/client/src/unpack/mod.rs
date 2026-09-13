//! Cache snapshot lifecycle: ordered preparation, complete publication and
//! process-wide reuse for the local `main_file_cache`. The host runs this once
//! per cache version: every non-empty file from the idx archives is stripped
//! of its gzip + 2-byte version trailer and written as a length-prefixed
//! record into a versioned snapshot under `~/.274bot/unpack/<version>/`, so
//! later boots inject the unpacked data without re-reading the live
//! (mutating) cache.
//!
//! Ordered preparation (see [`prepare_snapshot`] / [`boot_snapshot`]):
//!
//! 1. read the selected cache's versionlist and hash it into the version;
//! 2. validate the snapshot already present for that version ([`SnapshotState`]);
//! 3. fetch only genuinely missing jag packs through the client's real
//!    update-server fetch (`/crc` + `getJagFile`), CRC-checked and persisted;
//! 4. unpack into a staging directory and publish it with the manifest
//!    renamed last, so a reader never observes a partial snapshot as ready;
//! 5. inject the verified snapshot into the process-wide model/anim stores.
//!
//! A snapshot is [`SnapshotState::Ready`] only when its completion manifest
//! proves it was published whole *and* matches the selected cache version; a
//! merely nonempty directory is never valid. Concurrent clients share one
//! attempt per cache identity, including the outcome of a failed attempt, so
//! there is no per-client fetch/unpack/injection loop. Successes are recorded
//! for the whole process; a failed attempt is shared with concurrent waiters
//! but a later request retries instead of inheriting a stale failure.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use crate::client::Client;
use crate::io::jagfile::JagFile;
use crate::io::ondemand::cache_read;
use crate::io::packet::Packet;
use crate::BotTarget;

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

/// Unpacked archive files written by [`unpack_archive`].
const BINS: [&str; 4] = ["models.bin", "anims.bin", "midi.bin", "maps.bin"];

/// One archive's tables and destinations:
/// `(version table, crc table, store idx, snapshot file)`. The store idx is
/// the `main_file_cache.idxN` / `cache_read` archive (1..4); the OnDemand
/// archive index is `store_idx - 1`.
const ARCHIVES: [(&str, &str, i32, &str); 4] = [
    ("model_version", "model_crc", MODELS, "models.bin"),
    ("anim_version", "anim_crc", ANIMS, "anims.bin"),
    ("midi_version", "midi_crc", MIDI, "midi.bin"),
    ("map_version", "map_crc", MAPS, "maps.bin"),
];

/// Source of cache *entries* for a genuinely cold cache (no usable local
/// `main_file_cache`): the client's update-protocol worker. Snapshot
/// preparation drives this trait, so the snapshot writer never owns a socket
/// and the worker never owns the snapshot format. Implementations must return
/// only payloads that validate against the versionlist crc/version tables
/// (Java `OnDemand.validate`), gunzipped, in `(file, payload)` pairs.
pub trait EntrySource {
    /// `archive` is the OnDemand archive index (0..3); a required entry the
    /// engine or the version/CRC check cannot produce is an `Err`.
    fn fetch_entries(&mut self, archive: i32, files: &[i32])
        -> Result<Vec<(i32, Vec<u8>)>, String>;
}

/// Update-server checksum slot for each jag pack (`JAG_FILES` order in the
/// client: title=1 .. sounds=8). Used by the cache fetch, which reuses
/// `Client::get_jag_checksums_for` / `Client::get_jag_file_for`.
const JAG_INDEX: [(&str, usize); 8] = [
    ("title", 1),
    ("config", 2),
    ("interface", 3),
    ("media", 4),
    ("versionlist", 5),
    ("textures", 6),
    ("wordenc", 7),
    ("sounds", 8),
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
    /// Bytes of the written record stream (`*.bin` file size).
    pub bytes: u64,
}

/// The versioned snapshot result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Short stable hash of the versionlist jag content.
    pub version: String,
    /// The snapshot directory (`out_dir + "/" + version`).
    pub dir: String,
    /// Byte size of each copied jag pack, in [`JAGS`] order.
    pub jags: Vec<(String, u64)>,
    /// Where the record payloads came from: `local-store` or `update-server`.
    pub source: String,
    /// Set only when the manifest was written after every payload file was
    /// published. The readiness gate requires it: a snapshot without it is
    /// interrupted or predates the completion marker.
    pub complete: bool,
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

    /// Rebuild an error from a message shared through a once-per-key table.
    fn from_message(message: String) -> Self {
        UnpackError { message }
    }
}

impl fmt::Display for UnpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UnpackError {}

/// Readiness of the snapshot for one selected cache version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotState {
    /// `{out_dir}/{version}` does not exist.
    Missing,
    /// Present but not proven complete for this version; the reason names the
    /// file or check that failed.
    Incomplete(String),
    /// Complete, version-bound and usable.
    Ready(Manifest),
}

impl SnapshotState {
    /// Short description for status lines and error messages.
    pub fn describe(&self) -> String {
        match self {
            SnapshotState::Missing => "snapshot missing".to_string(),
            SnapshotState::Incomplete(reason) => format!("snapshot incomplete: {reason}"),
            SnapshotState::Ready(manifest) => {
                format!("snapshot {} complete", manifest.version)
            }
        }
    }
}

/// Unpack the local cache into `{out_dir}/{version}/` and return its
/// manifest. `cache_dir` is the jag pack directory (the engine's
/// `data/pack/client`); the `main_file_cache.*` store lives one level up and
/// is located by `cache_read`'s parent fallback.
///
/// The snapshot for the selected version is written into a staging directory
/// and published file by file with `manifest` renamed last, so an interrupted
/// run never leaves a readable snapshot behind. An already complete snapshot
/// for this version is returned untouched (warm reuse, no rewrite); an
/// incomplete or legacy one is republished.
pub fn unpack_cache(cache_dir: &str, out_dir: &str) -> Result<Manifest, UnpackError> {
    publish_snapshot(cache_dir, out_dir, None)
}

/// Cold start without a usable local file store: fetch every required entry
/// through `source` (the client's OnDemand worker) and publish the same
/// snapshot. The data comes from the real update-server fetch, each payload
/// validated against the selected versionlist; nothing is admitted as ready
/// until every required entry is present.
pub fn fetch_snapshot(
    cache_dir: &str,
    out_dir: &str,
    source: &mut dyn EntrySource,
) -> Result<Manifest, UnpackError> {
    publish_snapshot(cache_dir, out_dir, Some(source))
}

/// Shared staged publication for both sources: [`unpack_cache`] reads the
/// local `main_file_cache` store, [`fetch_snapshot`] fills from the update
/// server. Returns the manifest of a complete, verified snapshot.
fn publish_snapshot(
    cache_dir: &str,
    out_dir: &str,
    mut source: Option<&mut dyn EntrySource>,
) -> Result<Manifest, UnpackError> {
    let versionlist = read_versionlist(cache_dir)?;
    let version = version_hash(&versionlist);
    if let SnapshotState::Ready(existing) =
        snapshot_state_for_version(out_dir, &version, &versionlist)
    {
        return Ok(existing);
    }

    let dir_path = Path::new(out_dir).join(&version);
    let dir = dir_path.to_string_lossy().into_owned();
    let staging = staging_dir(out_dir, &version);
    let _ = std::fs::remove_dir_all(&staging);
    // The snapshot root is created for the staging directory; when this
    // attempt fails before anything is published it is removed again, so a
    // rejected profile leaves no half-made snapshot root behind.
    let root_created = !Path::new(out_dir).exists();
    std::fs::create_dir_all(out_dir).map_err(|e| UnpackError::io("create snapshot root", e))?;
    std::fs::create_dir_all(&staging)
        .map_err(|e| UnpackError::io("create snapshot staging dir", e))?;

    let staged = stage_snapshot(
        cache_dir,
        &staging,
        &dir,
        &version,
        &versionlist,
        &mut source,
    );
    if let Err(e) = staged {
        let _ = std::fs::remove_dir_all(&staging);
        cleanup_empty_root(out_dir, root_created);
        return Err(e);
    }

    // The selected version is the cache's identity: if the cache changed
    // while it was being read, the staged payload is a mix and must not be
    // published.
    let source = match read_versionlist(cache_dir) {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            cleanup_empty_root(out_dir, root_created);
            return Err(e);
        }
    };
    if source != versionlist {
        let _ = std::fs::remove_dir_all(&staging);
        cleanup_empty_root(out_dir, root_created);
        return Err(UnpackError::new(
            "cache versionlist changed during preparation; retry",
        ));
    }

    let published = publish(&staging, &dir_path);
    let _ = std::fs::remove_dir_all(&staging);
    published?;

    match snapshot_state_for_version(out_dir, &version, &versionlist) {
        SnapshotState::Ready(manifest) => Ok(manifest),
        other => Err(UnpackError::new(format!(
            "published snapshot failed verification: {}",
            other.describe()
        ))),
    }
}

/// Copy the jag packs and unpack every idx archive into `staging`, returning
/// the manifest that will be published with them. `source` selects where the
/// record payloads come from; the completeness rules and the manifest are the
/// same for both sources.
fn stage_snapshot(
    cache_dir: &str,
    staging: &Path,
    dir: &str,
    version: &str,
    versionlist: &[u8],
    source: &mut Option<&mut dyn EntrySource>,
) -> Result<Manifest, UnpackError> {
    let origin = if source.is_some() {
        "update-server"
    } else {
        "local-store"
    };
    let mut jags = Vec::with_capacity(JAGS.len());
    for name in JAGS {
        let src = Path::new(cache_dir).join(name);
        let bytes =
            std::fs::read(&src).map_err(|e| UnpackError::io(&format!("read jag {name}"), e))?;
        std::fs::write(staging.join(name), &bytes)
            .map_err(|e| UnpackError::io(&format!("write jag {name}"), e))?;
        jags.push((name.to_string(), bytes.len() as u64));
    }

    let jag = JagFile::new(versionlist.to_vec());
    let mut archives: [ArchiveStats; ARCHIVES.len()] = Default::default();
    for (index, (version_table, crc_table, store_idx, file_name)) in
        ARCHIVES.into_iter().enumerate()
    {
        let tables = ArchiveTables::read(&jag, version_table, crc_table)?;
        archives[index] = match source.as_deref_mut() {
            Some(source) => {
                let mut payload = NetworkPayload { source };
                unpack_archive(staging, &tables, store_idx, file_name, &mut payload)?
            }
            None => {
                let mut payload = StorePayload::open(cache_dir, store_idx, &tables)?;
                unpack_archive(staging, &tables, store_idx, file_name, &mut payload)?
            }
        };
    }
    let (models, anims, midi, maps) = (
        archives[0].clone(),
        archives[1].clone(),
        archives[2].clone(),
        archives[3].clone(),
    );

    let manifest = Manifest {
        version: version.to_string(),
        dir: dir.to_string(),
        jags,
        source: origin.to_string(),
        complete: true,
        models,
        anims,
        midi,
        maps,
    };
    let text = manifest_text(&manifest);
    validate_records(&manifest, &text)?;
    std::fs::write(staging.join("manifest"), text)
        .map_err(|e| UnpackError::io("write manifest", e))?;
    Ok(manifest)
}

/// Publish a staged snapshot into `dir`: every payload file first, the
/// `manifest` completion marker last. Readers therefore never observe a
/// manifest for a payload that is not entirely in place.
fn publish(staging: &Path, dir: &Path) -> Result<(), UnpackError> {
    std::fs::create_dir_all(dir).map_err(|e| UnpackError::io("create snapshot dir", e))?;
    for name in payload_names() {
        publish_file(&staging.join(name), &dir.join(name))?;
    }
    // The manifest is the completion marker: it must land after the payload.
    publish_file(&staging.join("manifest"), &dir.join("manifest"))
}

/// Move one staged file into the published snapshot. On Unix the rename
/// replaces an existing file atomically; on Windows a rename onto an existing
/// file fails, so the destination is removed first (the manifest-last publish
/// keeps that window from ever being visible as ready).
fn publish_file(staged: &Path, target: &Path) -> Result<(), UnpackError> {
    match std::fs::rename(staged, target) {
        Ok(()) => Ok(()),
        Err(_) if target.exists() => {
            std::fs::remove_file(target)
                .map_err(|e| UnpackError::io(&format!("replace {}", target.display()), e))?;
            std::fs::rename(staged, target)
                .map_err(|e| UnpackError::io(&format!("publish {}", target.display()), e))
        }
        Err(e) => Err(UnpackError::io(&format!("publish {}", target.display()), e)),
    }
}

/// Every file a complete snapshot must contain, in publish order.
fn payload_names() -> impl Iterator<Item = &'static str> {
    JAGS.into_iter().chain(BINS)
}

/// Unique staging directory for this process/version inside the snapshot root.
fn staging_dir(out_dir: &str, version: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    Path::new(out_dir).join(format!(
        ".{version}.staging-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Remove the snapshot root again when this attempt created it and nothing
/// was published: a preparation that fails (no store, no source, a cache that
/// changed mid-read) leaves no snapshot root behind. `remove_dir` only
/// succeeds on an empty directory, so a concurrently published snapshot is
/// never disturbed.
fn cleanup_empty_root(out_dir: &str, created: bool) {
    if created {
        let _ = std::fs::remove_dir(out_dir);
    }
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

/// The selected cache's version: the versionlist content hash every snapshot
/// path, manifest and later validation is bound to.
pub fn selected_version(cache_dir: &str) -> Result<String, UnpackError> {
    Ok(version_hash(&read_versionlist(cache_dir)?))
}

fn read_versionlist(cache_dir: &str) -> Result<Vec<u8>, UnpackError> {
    let path = Path::new(cache_dir).join("versionlist");
    std::fs::read(&path).map_err(|e| UnpackError::io(&format!("read {}", path.display()), e))
}

/// Readiness of the snapshot for the selected cache version under `out_dir`.
pub fn snapshot_state(cache_dir: &str, out_dir: &str) -> Result<SnapshotState, UnpackError> {
    let versionlist = read_versionlist(cache_dir)?;
    Ok(snapshot_state_for_version(
        out_dir,
        &version_hash(&versionlist),
        &versionlist,
    ))
}

/// Readiness of the snapshot for an explicit version. `cache_versionlist` is
/// the selected cache's versionlist content; the snapshot's own copy must
/// hash to the same version, which binds the payload to this cache content.
///
/// A snapshot is `Ready` only when its completion manifest parses, matches the
/// version, is marked complete, and every payload file is present with the
/// recorded size and consistent record counts. Anything else — including a
/// nonempty directory with no manifest — is `Incomplete` with the failing
/// check named.
pub fn snapshot_state_for_version(
    out_dir: &str,
    version: &str,
    cache_versionlist: &[u8],
) -> SnapshotState {
    let dir = Path::new(out_dir).join(version);
    if !dir.is_dir() {
        return SnapshotState::Missing;
    }
    let manifest_path = dir.join("manifest");
    let text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(e) => {
            return SnapshotState::Incomplete(format!("{}: {e}", manifest_path.display()));
        }
    };
    let Some(manifest) = manifest_from_text(&text) else {
        return SnapshotState::Incomplete(format!(
            "{}: unreadable manifest",
            manifest_path.display()
        ));
    };
    if manifest.version != version {
        return SnapshotState::Incomplete(format!(
            "manifest version {} does not match selected version {version}",
            manifest.version
        ));
    }
    if !manifest.complete {
        return SnapshotState::Incomplete(
            "completion marker missing (interrupted or pre-marker snapshot)".to_string(),
        );
    }
    match std::fs::read(dir.join("versionlist")) {
        Ok(bytes) if version_hash(&bytes) == version_hash(cache_versionlist) => {}
        Ok(_) => {
            return SnapshotState::Incomplete(
                "versionlist copy does not match the selected cache version".to_string(),
            );
        }
        Err(e) => return SnapshotState::Incomplete(format!("versionlist copy: {e}")),
    }
    for (index, name) in JAGS.iter().enumerate() {
        match manifest.jags.get(index) {
            Some((declared, size)) if declared == name => match file_size(&dir.join(name)) {
                Ok(actual) if actual == *size => {}
                Ok(actual) => {
                    return SnapshotState::Incomplete(format!(
                        "{name}: {actual} bytes on disk, manifest records {size}"
                    ));
                }
                Err(e) => return SnapshotState::Incomplete(format!("{name}: {e}")),
            },
            _ => {
                return SnapshotState::Incomplete(format!("manifest has no size for jag {name}"));
            }
        }
    }
    for (name, stats) in [
        ("models.bin", &manifest.models),
        ("anims.bin", &manifest.anims),
        ("midi.bin", &manifest.midi),
        ("maps.bin", &manifest.maps),
    ] {
        match file_size(&dir.join(name)) {
            Ok(actual) if actual == stats.bytes => {}
            Ok(actual) => {
                return SnapshotState::Incomplete(format!(
                    "{name}: {actual} bytes on disk, manifest records {}",
                    stats.bytes
                ));
            }
            Err(e) => return SnapshotState::Incomplete(format!("{name}: {e}")),
        }
        if stats.total != stats.unpacked + stats.skipped {
            return SnapshotState::Incomplete(format!(
                "{name}: {} total != {} unpacked + {} skipped",
                stats.total, stats.unpacked, stats.skipped
            ));
        }
        if stats.unpacked == 0 || stats.bytes == 0 {
            return SnapshotState::Incomplete(format!("{name}: no records in the snapshot"));
        }
    }
    SnapshotState::Ready(manifest)
}

fn file_size(path: &Path) -> io::Result<u64> {
    Ok(std::fs::metadata(path)?.len())
}

/// Cross-check a manifest against its own record stream before it is
/// published: each archive's declared counts must describe the bytes written.
/// This is the writer-side half of "success implies completeness".
fn validate_records(manifest: &Manifest, _text: &str) -> Result<(), UnpackError> {
    for (name, stats) in [
        ("models.bin", &manifest.models),
        ("anims.bin", &manifest.anims),
        ("midi.bin", &manifest.midi),
        ("maps.bin", &manifest.maps),
    ] {
        if stats.total != stats.unpacked + stats.skipped {
            return Err(UnpackError::new(format!(
                "{name}: {} total != {} unpacked + {} skipped",
                stats.total, stats.unpacked, stats.skipped
            )));
        }
    }
    if manifest.jags.len() != JAGS.len() {
        return Err(UnpackError::new("manifest jag list is incomplete"));
    }
    Ok(())
}

/// Version and CRC tables for one archive, read from the versionlist jag.
/// `version == 0` means the entry is intentionally absent from this cache
/// revision (Java `OnDemand.validFile` / `versions[archive][file] != 0`);
/// any entry with a nonzero version is *required* and must be readable and
/// validate against its CRC.
struct ArchiveTables {
    version_table: &'static str,
    crc_table: &'static str,
    versions: Vec<i32>,
    crcs: Vec<i32>,
}

impl ArchiveTables {
    fn read(
        jag: &JagFile,
        version_table: &'static str,
        crc_table: &'static str,
    ) -> Result<Self, UnpackError> {
        let versions_raw = jag.read(version_table).ok_or_else(|| {
            UnpackError::new(format!("versionlist missing `{version_table}` table"))
        })?;
        let crcs_raw = jag
            .read(crc_table)
            .ok_or_else(|| UnpackError::new(format!("versionlist missing `{crc_table}` table")))?;
        let mut versions = Vec::with_capacity(versions_raw.len() / 2);
        let mut packet = Packet::new(versions_raw);
        while packet.pos + 2 <= packet.data().len() {
            versions.push(packet.g2());
        }
        let mut crcs = Vec::with_capacity(crcs_raw.len() / 4);
        let mut packet = Packet::new(crcs_raw);
        while packet.pos + 4 <= packet.data().len() {
            crcs.push(packet.g4());
        }
        if crcs.len() < versions.len() {
            return Err(UnpackError::new(format!(
                "versionlist `{crc_table}` covers {} of {} `{version_table}` entries",
                crcs.len(),
                versions.len()
            )));
        }
        Ok(Self {
            version_table,
            crc_table,
            versions,
            crcs,
        })
    }

    fn total(&self) -> u32 {
        self.versions.len() as u32
    }

    /// Selected version of one entry; 0 (absent) when the table does not
    /// cover the index.
    fn version(&self, file: i32) -> i32 {
        self.versions.get(file as usize).copied().unwrap_or(0)
    }

    fn crc(&self, file: i32) -> i32 {
        self.crcs.get(file as usize).copied().unwrap_or(0)
    }

    /// `(file, version)` for every required entry, in index order.
    fn required(&self) -> impl Iterator<Item = i32> + '_ {
        (0..self.versions.len() as i32).filter(move |file| self.version(*file) != 0)
    }
}

/// Payload source for one archive's required entries. Implementations return
/// **validated, gunzipped** payloads — the same bytes `unpack_archive` would
/// write as a record — and fail when a required entry cannot be produced.
trait PayloadSource {
    fn payloads(&mut self, archive: i32, files: &[i32]) -> Result<Vec<Vec<u8>>, UnpackError>;
}

/// Local `main_file_cache` store reader: the normal source when the cache was
/// unpacked by the engine (or a previous run) and is complete.
struct StorePayload<'a> {
    cache_dir: &'a str,
    idx: i32,
    tables: &'a ArchiveTables,
    idx_file: File,
    idx_entries: i32,
}

impl<'a> StorePayload<'a> {
    fn open(cache_dir: &'a str, idx: i32, tables: &'a ArchiveTables) -> Result<Self, UnpackError> {
        let store_dir = file_store_dir(cache_dir)
            .ok_or_else(|| UnpackError::new("main_file_cache.dat not found"))?;
        let path = format!("{store_dir}/main_file_cache.idx{idx}");
        let idx_file = File::open(&path).map_err(|e| UnpackError::io("open idx", e))?;
        let idx_entries = (idx_file
            .metadata()
            .map_err(|e| UnpackError::io("stat idx", e))?
            .len()
            / 6) as i32;
        Ok(Self {
            cache_dir,
            idx,
            tables,
            idx_file,
            idx_entries,
        })
    }
}

impl PayloadSource for StorePayload<'_> {
    fn payloads(&mut self, archive: i32, files: &[i32]) -> Result<Vec<Vec<u8>>, UnpackError> {
        let mut out = Vec::with_capacity(files.len());
        for &file in files {
            let version = self.tables.version(file);
            if file >= self.idx_entries {
                return Err(UnpackError::new(format!(
                    "archive {archive} file {file}: index {} requires version {version} but has no store record",
                    self.tables.version_table
                )));
            }
            let size =
                idx_size(&mut self.idx_file, file).map_err(|e| UnpackError::io("read idx", e))?;
            if size <= 0 {
                return Err(UnpackError::new(format!(
                    "archive {archive} file {file}: index {} requires version {version} but the store entry is empty",
                    self.tables.version_table
                )));
            }
            let data = cache_read(self.cache_dir, self.idx, file).ok_or_else(|| {
                UnpackError::new(format!(
                    "archive {archive} file {file}: idx size {size} but read failed (corrupt sector chain)"
                ))
            })?;
            if !crate::io::ondemand::validate(self.tables.crc(file), version, Some(&data)) {
                return Err(UnpackError::new(format!(
                    "archive {archive} file {file}: index {} version {version} but the store payload failed the versionlist version/CRC check ({})",
                    self.tables.version_table, self.tables.crc_table
                )));
            }
            let body = if data.len() >= 2 {
                &data[..data.len() - 2]
            } else {
                &data
            };
            let raw = gunzip(body).ok_or_else(|| {
                UnpackError::new(format!(
                    "archive {archive} file {file}: corrupt gzip stream"
                ))
            })?;
            out.push(raw);
        }
        Ok(out)
    }
}

/// Update-server source for a genuinely cold cache: entries come from the
/// client's OnDemand worker, which validates each download against the
/// versionlist before handing it over.
struct NetworkPayload<'a> {
    source: &'a mut dyn EntrySource,
}

impl PayloadSource for NetworkPayload<'_> {
    fn payloads(&mut self, store_idx: i32, files: &[i32]) -> Result<Vec<Vec<u8>>, UnpackError> {
        // OnDemand archive indices are 0..3; the store index is archive + 1.
        let archive = store_idx - 1;
        let fetched = self
            .source
            .fetch_entries(archive, files)
            .map_err(|e| UnpackError::new(format!("update server: {e}")))?;
        let mut by_file: HashMap<i32, Vec<u8>> = fetched.into_iter().collect();
        let mut out = Vec::with_capacity(files.len());
        for &file in files {
            match by_file.remove(&file) {
                Some(raw) => out.push(raw),
                None => {
                    return Err(UnpackError::new(format!(
                        "archive {store_idx} file {file}: update server returned no payload"
                    )));
                }
            }
        }
        Ok(out)
    }
}

/// Batch size for one payload request: the OnDemand worker keeps at most ten
/// urgent requests in flight, so a small batch keeps memory bounded and still
/// pipelines the fetch.
const PAYLOAD_BATCH: usize = 16;

/// Write one archive's record stream from `source`: every required entry is
/// fetched and written in index order, `version == 0` entries are counted as
/// intentionally absent, and any required entry the source cannot produce is
/// a hard error — an incomplete store or download never yields a snapshot.
fn unpack_archive(
    out_dir: &Path,
    tables: &ArchiveTables,
    idx: i32,
    file_name: &str,
    source: &mut dyn PayloadSource,
) -> Result<ArchiveStats, UnpackError> {
    let out_path = out_dir.join(file_name);
    let mut out = File::create(&out_path).map_err(|e| UnpackError::io("create archive file", e))?;
    let mut stats = ArchiveStats {
        total: tables.total(),
        unpacked: 0,
        skipped: 0,
        bytes: 0,
    };
    // Count the intentionally absent entries first so `total == unpacked +
    // skipped` holds even when a required entry fails.
    stats.skipped = (0..tables.versions.len() as i32)
        .filter(|file| tables.version(*file) == 0)
        .count() as u32;

    let mut pending: Vec<i32> = Vec::with_capacity(PAYLOAD_BATCH);
    for file in tables.required() {
        pending.push(file);
        if pending.len() >= PAYLOAD_BATCH {
            write_records(&mut out, &mut stats, &mut pending, &mut *source, idx)?;
        }
    }
    if !pending.is_empty() {
        write_records(&mut out, &mut stats, &mut pending, &mut *source, idx)?;
    }

    out.flush()
        .map_err(|e| UnpackError::io("flush archive file", e))?;
    drop(out);
    stats.bytes = file_size(&out_path).map_err(|e| UnpackError::io("stat archive file", e))?;
    Ok(stats)
}

fn write_records(
    out: &mut File,
    stats: &mut ArchiveStats,
    pending: &mut Vec<i32>,
    source: &mut dyn PayloadSource,
    idx: i32,
) -> Result<(), UnpackError> {
    let payloads = source.payloads(idx, pending)?;
    debug_assert_eq!(payloads.len(), pending.len());
    for (file, raw) in pending.iter().zip(payloads.iter()) {
        encode_record(out, *file as u32, raw).map_err(|e| UnpackError::io("write record", e))?;
        stats.unpacked += 1;
    }
    pending.clear();
    Ok(())
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
/// a snapshot that is not complete for the selected version, or a truncated
/// record, is `Err`.
pub fn load_snapshot(cache_dir: &str, out_dir: &str) -> Result<Loaded, UnpackError> {
    let versionlist = read_versionlist(cache_dir)?;
    let version = version_hash(&versionlist);
    match snapshot_state_for_version(out_dir, &version, &versionlist) {
        SnapshotState::Ready(_) => {}
        other => {
            return Err(UnpackError::new(format!(
                "{}/{}: {}",
                out_dir,
                version,
                other.describe()
            )));
        }
    }
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
/// (50-head wall) must not re-read `models.bin` / wipe the stores. Concurrent
/// callers for the same cache identity share one attempt, including a failed
/// attempt's outcome. Returns `(loaded, first)` so `maininit` prints once.
pub fn load_snapshot_once(cache_dir: &str, out_dir: &str) -> Result<(Loaded, bool), UnpackError> {
    let key = snapshot_key(cache_dir, out_dir);
    let (outcome, ran) = once_per_key(&SNAPSHOT_INJECT, &key, || {
        PREPARE_INJECTS.fetch_add(1, Ordering::Relaxed);
        match load_snapshot(cache_dir, out_dir) {
            Ok(loaded) => Outcome::Success(Ok(loaded)),
            Err(e) => Outcome::Failure(Err(e.to_string())),
        }
    });
    match outcome {
        Ok(loaded) => Ok((loaded, ran)),
        Err(message) => Err(UnpackError::from_message(message)),
    }
}

/// Where the update server (asset origin) lives for a cache fetch. The fetch
/// reuses the client's real `/crc` + `getJagFile` machinery, so a fetched jag
/// is CRC-checked and persisted exactly like `maininit`'s own fetch.
#[derive(Debug, Clone, Copy)]
pub struct FetchEndpoint<'a> {
    pub target: BotTarget,
    pub host: &'a str,
    pub port: u16,
}

/// What ordered preparation achieved for one cache identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotPreparation {
    /// The selected version's snapshot is complete and usable.
    Ready {
        version: String,
        dir: String,
        models: u32,
        anim_records: u32,
        /// Where the record payloads came from: `local-store` or
        /// `update-server`.
        source: &'static str,
        /// Jag packs fetched from the update server (empty on a warm start).
        fetched: Vec<String>,
        /// True when this attempt published the snapshot (cold start or
        /// repair); false when it reused an already complete snapshot.
        published: bool,
    },
    /// No usable snapshot after every available source was tried: clients
    /// keep the OnDemand fallback.
    Unavailable { reason: String },
}

impl SnapshotPreparation {
    pub fn is_ready(&self) -> bool {
        matches!(self, SnapshotPreparation::Ready { .. })
    }
}

/// Process-wide one-time-work evidence for cache preparation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrepareCounters {
    /// Preparation attempts actually executed (one per cold/repair request;
    /// concurrent callers share an attempt).
    pub attempts: u64,
    /// Jag packs fetched from the update server.
    pub jags_fetched: u64,
    /// Attempts that published a snapshot (cold start or repair).
    pub snapshots_published: u64,
    /// Attempts that found the snapshot already complete (warm start).
    pub warm_reuses: u64,
    /// Attempts whose snapshot was filled from the update-server entry
    /// protocol instead of the local file store.
    pub network_fills: u64,
    /// Snapshot injects actually run: the process-wide boot inject loads the
    /// stores once per cache identity, so two clients share one injection.
    pub injections: u64,
}

/// Ordered preparation for the selected cache version: validate the snapshot,
/// fetch only genuinely missing jag packs through the update-server fetch,
/// unpack from the local store — or, when that store is absent or incomplete,
/// fill the snapshot from `source` (the client's update-protocol worker) —
/// and report the outcome. At most one attempt runs per cache identity at a
/// time; concurrent callers wait for it and share its outcome. A completed
/// success is reused for the whole process; a failed attempt is not cached, so
/// a later request retries.
pub fn prepare_snapshot(
    cache_dir: &str,
    out_dir: &str,
    endpoint: Option<FetchEndpoint<'_>>,
    source: Option<&mut dyn EntrySource>,
) -> Arc<SnapshotPreparation> {
    let key = snapshot_key(cache_dir, out_dir);
    let (outcome, _ran) = once_per_key(&SNAPSHOT_PREPARE, &key, || {
        let prepared = Arc::new(attempt_prepare(cache_dir, out_dir, endpoint, source));
        match &*prepared {
            SnapshotPreparation::Ready { .. } => Outcome::Success(prepared),
            SnapshotPreparation::Unavailable { .. } => Outcome::Failure(prepared),
        }
    });
    outcome
}

/// Process-wide preparation counters (one-time-work evidence).
pub fn prepare_counters() -> PrepareCounters {
    PrepareCounters {
        attempts: PREPARE_ATTEMPTS.load(Ordering::Relaxed),
        jags_fetched: PREPARE_FETCHED.load(Ordering::Relaxed),
        snapshots_published: PREPARE_PUBLISHED.load(Ordering::Relaxed),
        warm_reuses: PREPARE_REUSED.load(Ordering::Relaxed),
        network_fills: PREPARE_NETWORK.load(Ordering::Relaxed),
        injections: PREPARE_INJECTS.load(Ordering::Relaxed),
    }
}

fn attempt_prepare(
    cache_dir: &str,
    out_dir: &str,
    endpoint: Option<FetchEndpoint<'_>>,
    source: Option<&mut dyn EntrySource>,
) -> SnapshotPreparation {
    PREPARE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);

    // Fetch genuinely missing pack files first, through the update server's
    // real `/crc` + `getJagFile` machinery: `versionlist` is one of the pack
    // files, so a genuinely empty cache directory has no version to read at
    // all until this fetch has run. A present file is validated by the
    // profile's cache identity check, not re-downloaded.
    let missing: Vec<&str> = JAGS
        .into_iter()
        .filter(|name| !Path::new(cache_dir).join(name).is_file())
        .collect();
    let fetched = if missing.is_empty() {
        Vec::new()
    } else {
        let Some(endpoint) = endpoint else {
            return SnapshotPreparation::Unavailable {
                reason: format!(
                    "cache pack files missing ({}); no update-server endpoint for the fetch",
                    missing.join(", ")
                ),
            };
        };
        match fetch_jags(cache_dir, endpoint, &missing) {
            Ok(fetched) => fetched,
            Err(e) => {
                return SnapshotPreparation::Unavailable {
                    reason: format!("cache fetch failed: {e}"),
                };
            }
        }
    };

    let versionlist = match read_versionlist(cache_dir) {
        Ok(bytes) => bytes,
        Err(e) => {
            return SnapshotPreparation::Unavailable {
                reason: format!("selected cache versionlist unreadable after fetch: {e}"),
            };
        }
    };
    let version = version_hash(&versionlist);

    if let SnapshotState::Ready(manifest) =
        snapshot_state_for_version(out_dir, &version, &versionlist)
    {
        PREPARE_REUSED.fetch_add(1, Ordering::Relaxed);
        return SnapshotPreparation::Ready {
            version,
            dir: manifest.dir,
            models: manifest.models.unpacked,
            anim_records: manifest.anims.unpacked,
            source: if manifest.source == "update-server" {
                "update-server"
            } else {
                "local-store"
            },
            fetched: Vec::new(),
            published: false,
        };
    }

    // Source order: the local file store first (no network, engine-authored
    // data), then the update-server entry protocol for a cache whose store is
    // absent or incomplete. A failed attempt reports both reasons.
    let mut reasons = Vec::new();
    match unpack_cache(cache_dir, out_dir) {
        Ok(manifest) => {
            PREPARE_PUBLISHED.fetch_add(1, Ordering::Relaxed);
            return SnapshotPreparation::Ready {
                version: manifest.version,
                dir: manifest.dir,
                models: manifest.models.unpacked,
                anim_records: manifest.anims.unpacked,
                source: "local-store",
                fetched,
                published: true,
            };
        }
        Err(e) => reasons.push(format!("local store: {e}")),
    }

    match source {
        Some(source) => match fetch_snapshot(cache_dir, out_dir, source) {
            Ok(manifest) => {
                PREPARE_PUBLISHED.fetch_add(1, Ordering::Relaxed);
                PREPARE_NETWORK.fetch_add(1, Ordering::Relaxed);
                SnapshotPreparation::Ready {
                    version: manifest.version,
                    dir: manifest.dir,
                    models: manifest.models.unpacked,
                    anim_records: manifest.anims.unpacked,
                    source: "update-server",
                    fetched,
                    published: true,
                }
            }
            Err(e) => {
                reasons.push(format!("update-server fill: {e}"));
                SnapshotPreparation::Unavailable {
                    reason: reasons.join("; "),
                }
            }
        },
        None => {
            reasons.push("no update-server entry source is available for a cold fill".to_string());
            SnapshotPreparation::Unavailable {
                reason: reasons.join("; "),
            }
        }
    }
}

/// Fetch the named jag packs through the client's update-server fetch: one
/// `/crc` read for the checksum slots, then one CRC-checked GET per file that
/// persists into `cache_dir`.
fn fetch_jags(
    cache_dir: &str,
    endpoint: FetchEndpoint<'_>,
    missing: &[&str],
) -> Result<Vec<String>, UnpackError> {
    let checksums = Client::get_jag_checksums_for(endpoint.target, endpoint.host, endpoint.port)
        .map_err(|e| UnpackError::new(format!("update server /crc: {e}")))?;
    let mut fetched = Vec::with_capacity(missing.len());
    for name in missing {
        let index = JAG_INDEX
            .iter()
            .find(|(jag, _)| jag == name)
            .map(|(_, index)| *index)
            .ok_or_else(|| UnpackError::new(format!("{name}: not a jag pack file")))?;
        Client::get_jag_file_for(
            endpoint.target,
            cache_dir,
            endpoint.host,
            endpoint.port,
            name,
            index,
            &checksums,
        )
        .ok_or_else(|| UnpackError::new(format!("{name}: fetch or CRC check failed")))?;
        fetched.push((*name).to_string());
    }
    PREPARE_FETCHED.fetch_add(fetched.len() as u64, Ordering::Relaxed);
    Ok(fetched)
}

/// The client boot path: ordered preparation followed by the process-wide
/// inject. `first` is true for the process' first client on this cache
/// identity, so status lines print once instead of once per slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotBoot {
    Ready {
        loaded: Loaded,
        first: bool,
        /// True when this boot prepared the snapshot (cold start or repair).
        published: bool,
    },
    /// No usable snapshot; the caller keeps the OnDemand fallback.
    Degraded { reason: String, first: bool },
}

/// Check, prepare (fetch/unpack as needed) and inject the selected version's
/// snapshot. Never fatal: an unusable snapshot returns [`SnapshotBoot::Degraded`]
/// with the reason, and the caller falls back to the OnDemand path. `source`
/// is the caller's OnDemand worker, used only when the local store cannot
/// supply a complete snapshot.
pub fn boot_snapshot(
    cache_dir: &str,
    out_dir: &str,
    endpoint: Option<FetchEndpoint<'_>>,
    source: Option<&mut dyn EntrySource>,
) -> SnapshotBoot {
    match &*prepare_snapshot(cache_dir, out_dir, endpoint, source) {
        SnapshotPreparation::Ready { published, .. } => {
            match load_snapshot_once(cache_dir, out_dir) {
                Ok((loaded, first)) => SnapshotBoot::Ready {
                    loaded,
                    first,
                    published: *published,
                },
                Err(e) => SnapshotBoot::Degraded {
                    reason: format!("snapshot inject failed: {e}"),
                    first: claim_boot_report(cache_dir, out_dir),
                },
            }
        }
        SnapshotPreparation::Unavailable { reason } => SnapshotBoot::Degraded {
            reason: reason.clone(),
            first: claim_boot_report(cache_dir, out_dir),
        },
    }
}

/// True the first time this process reports the boot outcome for a cache
/// identity: per-slot warning noise is what this prevents.
fn claim_boot_report(cache_dir: &str, out_dir: &str) -> bool {
    static REPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let reported = REPORTED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut reported = reported.lock().unwrap_or_else(|p| p.into_inner());
    reported.insert(snapshot_key(cache_dir, out_dir))
}

/// Identity of one prepared snapshot: the cache directory it was unpacked
/// from and the snapshot root it was published to.
fn snapshot_key(cache_dir: &str, out_dir: &str) -> String {
    format!("{cache_dir}\u{1}{out_dir}")
}

/// The outcome of one once-per-key attempt. A `Failure` is shared with the
/// callers that waited for the attempt, but is not reused by later callers:
/// they run a fresh attempt.
#[derive(Debug, Clone)]
enum Outcome<T> {
    Success(T),
    Failure(T),
}

#[derive(Debug)]
struct OnceTable<T> {
    /// Recorded outcome per key (the newest attempt's).
    record: HashMap<String, Outcome<T>>,
    /// Keys whose attempt is currently running.
    in_flight: HashSet<String>,
}

impl<T> Default for OnceTable<T> {
    fn default() -> Self {
        OnceTable {
            record: HashMap::new(),
            in_flight: HashSet::new(),
        }
    }
}

type OnceCell<T> = OnceLock<(Mutex<OnceTable<T>>, Condvar)>;

/// Run `work` at most once at a time per `key` in this process. Concurrent
/// callers wait for the running attempt and share its outcome, success or
/// failure. A recorded success is returned to every later caller; a recorded
/// failure is only shared with callers that actually waited for it, so a
/// later request retries instead of inheriting a stale failure. The returned
/// bool is true for the caller that ran the work.
fn once_per_key<T: Clone>(
    cell: &'static OnceCell<T>,
    key: &str,
    work: impl FnOnce() -> Outcome<T>,
) -> (T, bool) {
    let (lock, cv) = cell.get_or_init(|| (Mutex::new(OnceTable::default()), Condvar::new()));
    let mut waited = false;
    let mut table = lock.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        match table.record.entry(key.to_string()) {
            Entry::Occupied(entry) => match entry.get() {
                Outcome::Success(outcome) => return (outcome.clone(), false),
                Outcome::Failure(outcome) if waited => return (outcome.clone(), false),
                Outcome::Failure(_) => {}
            },
            Entry::Vacant(_) => {}
        }
        if table.in_flight.contains(key) {
            waited = true;
            table = cv.wait(table).unwrap_or_else(|p| p.into_inner());
            continue;
        }
        table.in_flight.insert(key.to_string());
        break;
    }
    drop(table);

    let outcome = work();

    let mut table = lock.lock().unwrap_or_else(|p| p.into_inner());
    table.in_flight.remove(key);
    table.record.insert(key.to_string(), outcome.clone());
    cv.notify_all();
    drop(table);
    (outcome_value(outcome), true)
}

fn outcome_value<T>(outcome: Outcome<T>) -> T {
    match outcome {
        Outcome::Success(value) | Outcome::Failure(value) => value,
    }
}

static SNAPSHOT_INJECT: OnceCell<Result<Loaded, String>> = OnceLock::new();
static SNAPSHOT_PREPARE: OnceCell<Arc<SnapshotPreparation>> = OnceLock::new();

static PREPARE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static PREPARE_FETCHED: AtomicU64 = AtomicU64::new(0);
static PREPARE_PUBLISHED: AtomicU64 = AtomicU64::new(0);
static PREPARE_REUSED: AtomicU64 = AtomicU64::new(0);
static PREPARE_NETWORK: AtomicU64 = AtomicU64::new(0);
static PREPARE_INJECTS: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
pub fn reset_snapshot_inject_for_tests() {
    let (lock, _) =
        SNAPSHOT_INJECT.get_or_init(|| (Mutex::new(OnceTable::default()), Condvar::new()));
    lock.lock()
        .unwrap_or_else(|p| p.into_inner())
        .record
        .clear();
}

#[cfg(test)]
pub fn reset_preparation_for_tests() {
    let (lock, _) =
        SNAPSHOT_PREPARE.get_or_init(|| (Mutex::new(OnceTable::default()), Condvar::new()));
    lock.lock()
        .unwrap_or_else(|p| p.into_inner())
        .record
        .clear();
}

/// Read every `[id][len][raw]` record from `path` and hand each to `apply`.
/// Returns the record count; a truncated header or body, or an empty file, is
/// a hard error (the writer never publishes partial or empty archives).
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
    s.push_str(&format!("source={}\n", m.source));
    s.push_str(&format!("complete={}\n", u8::from(m.complete)));
    for (name, size) in &m.jags {
        s.push_str(&format!("jag.{name}.bytes={size}\n"));
    }
    for (name, a) in [
        ("models", &m.models),
        ("anims", &m.anims),
        ("midi", &m.midi),
        ("maps", &m.maps),
    ] {
        s.push_str(&format!("{name}.total={}\n", a.total));
        s.push_str(&format!("{name}.unpacked={}\n", a.unpacked));
        s.push_str(&format!("{name}.skipped={}\n", a.skipped));
        s.push_str(&format!("{name}.bytes={}\n", a.bytes));
    }
    s
}

/// Inverse of [`manifest_text`]. `None` when a required key is missing, a
/// value does not parse, or the jag list is not exactly the [`JAGS`] set.
fn manifest_from_text(text: &str) -> Option<Manifest> {
    let mut version = None;
    let mut dir = None;
    let mut source = None;
    let mut complete = false;
    let mut jags: HashMap<String, u64> = HashMap::new();
    let mut stats: HashMap<String, ArchiveStats> = HashMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "version" => version = Some(value.to_string()),
            "dir" => dir = Some(value.to_string()),
            "source" => source = Some(value.to_string()),
            "complete" => complete = value == "1",
            _ => {
                if let Some(name) = key
                    .strip_prefix("jag.")
                    .and_then(|k| k.strip_suffix(".bytes"))
                {
                    jags.insert(name.to_string(), value.parse().ok()?);
                } else if let Some((archive, field)) = key.split_once('.') {
                    let entry = stats.entry(archive.to_string()).or_default();
                    let value: u64 = value.parse().ok()?;
                    match field {
                        "total" => entry.total = value as u32,
                        "unpacked" => entry.unpacked = value as u32,
                        "skipped" => entry.skipped = value as u32,
                        "bytes" => entry.bytes = value,
                        _ => return None,
                    }
                } else {
                    return None;
                }
            }
        }
    }
    let mut ordered = Vec::with_capacity(JAGS.len());
    for name in JAGS {
        ordered.push((name.to_string(), *jags.get(name)?));
    }
    let take = |name: &str| -> Option<ArchiveStats> {
        let entry = stats.get(name).cloned()?;
        Some(entry)
    };
    Some(Manifest {
        version: version?,
        dir: dir?,
        jags: ordered,
        source: source?,
        complete,
        models: take("models")?,
        anims: take("anims")?,
        midi: take("midi")?,
        maps: take("maps")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::io::jagfile::JagFile;

    /// Write every file a complete snapshot must contain for `versionlist`,
    /// with `models`/`anims` as the record streams the loader reads.
    fn plant_snapshot(out_dir: &Path, versionlist: &[u8], models: &[u8], anims: &[u8]) -> Manifest {
        let version = version_hash(versionlist);
        let dir = out_dir.join(&version);
        std::fs::create_dir_all(&dir).unwrap();
        let mut jags = Vec::new();
        for name in JAGS {
            let bytes: &[u8] = if name == "versionlist" {
                versionlist
            } else {
                b"\0\0\x06\0\0\x06\0\0"
            };
            std::fs::write(dir.join(name), bytes).unwrap();
            jags.push((name.to_string(), bytes.len() as u64));
        }
        std::fs::write(dir.join("models.bin"), models).unwrap();
        std::fs::write(dir.join("anims.bin"), anims).unwrap();
        let manifest = Manifest {
            version,
            dir: dir.to_string_lossy().into_owned(),
            jags,
            source: "test-fixture".to_string(),
            complete: true,
            models: ArchiveStats {
                total: 1,
                unpacked: 1,
                skipped: 0,
                bytes: models.len() as u64,
            },
            anims: ArchiveStats {
                total: 1,
                unpacked: 1,
                skipped: 0,
                bytes: anims.len() as u64,
            },
            midi: ArchiveStats {
                total: 1,
                unpacked: 1,
                skipped: 0,
                bytes: 1,
            },
            maps: ArchiveStats {
                total: 1,
                unpacked: 1,
                skipped: 0,
                bytes: 1,
            },
        };
        std::fs::write(dir.join("midi.bin"), [0u8]).unwrap();
        std::fs::write(dir.join("maps.bin"), [0u8]).unwrap();
        std::fs::write(dir.join("manifest"), manifest_text(&manifest)).unwrap();
        manifest
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("274bot-unpack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One gzipped `main_file_cache` record per archive plus the versionlist
    /// jag the unpacker reads its archive tables from: version 1 for every
    /// entry (required) and the CRC of the stored gzip payload, exactly the
    /// pairs `OnDemand.validate` compares.
    fn synthetic_cache(dir: &Path) -> Vec<u8> {
        let entries: [(i32, &str, &[u8]); 4] = [
            (MODELS, "model", &[0u8; 18]),
            (
                ANIMS,
                "anim",
                &[
                    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
                ],
            ),
            (MIDI, "midi", &[1u8, 2, 3]),
            (MAPS, "map", &[4u8, 5, 6, 7]),
        ];
        let mut jag_files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut dat = vec![0u8; 520 * (entries.len() + 1)];
        for (index, (archive, name, raw)) in entries.iter().enumerate() {
            let sector = index + 1;
            let mut payload = gzip(raw);
            let crc = Packet::getcrc(&payload, 0, payload.len());
            payload.extend_from_slice(&[0x00, 0x01]);
            let block = sector * 520;
            dat[block..block + 2].copy_from_slice(&0u16.to_be_bytes());
            dat[block + 2..block + 4].copy_from_slice(&0u16.to_be_bytes());
            dat[block + 4..block + 7].copy_from_slice(&[0, 0, 0]);
            dat[block + 7] = (archive + 1) as u8;
            dat[block + 8..block + 8 + payload.len()].copy_from_slice(&payload);
            let size = payload.len() as u32;
            let mut idx = [0u8; 6];
            idx[0] = ((size >> 16) & 0xff) as u8;
            idx[1] = ((size >> 8) & 0xff) as u8;
            idx[2] = (size & 0xff) as u8;
            idx[3] = ((sector as u32 >> 16) & 0xff) as u8;
            idx[4] = ((sector as u32 >> 8) & 0xff) as u8;
            idx[5] = (sector as u32 & 0xff) as u8;
            std::fs::write(dir.join(format!("main_file_cache.idx{archive}")), idx).unwrap();
            jag_files.push((format!("{name}_version"), vec![0x00, 0x01]));
            jag_files.push((format!("{name}_crc"), crc.to_be_bytes().to_vec()));
        }
        std::fs::write(dir.join("main_file_cache.dat"), dat).unwrap();
        let refs: Vec<(&str, &[u8])> = jag_files
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        let versionlist = jag(&refs);
        std::fs::write(dir.join("versionlist"), &versionlist).unwrap();
        for name in JAGS {
            if name != "versionlist" {
                std::fs::write(dir.join(name), b"\0\0\x06\0\0\x06\0\0").unwrap();
            }
        }
        versionlist
    }

    fn gzip(raw: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::best());
        enc.write_all(raw).unwrap();
        enc.finish().unwrap()
    }

    /// One `[id][len][raw]` record as a standalone byte stream.
    fn record(id: u32, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        encode_record(&mut out, id, data).unwrap();
        out
    }

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
        let mut models = Vec::new();
        encode_record(&mut models, 1, &[0u8; 18]).unwrap();
        // One empty-ish anim record is still a truncated error; write a
        // minimal valid frame stream (same layout as tests/inject.rs).
        let anim_data: [u8; 15] = [
            0x00, 0x01, 0x75, 0x31, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x01,
        ];
        let mut anims = Vec::new();
        encode_record(&mut anims, 0, &anim_data).unwrap();
        plant_snapshot(&out, versionlist, &models, &anims);
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
    fn snapshot_state_requires_a_complete_version_bound_snapshot() {
        let cache = tmp("state-cache");
        let out = tmp("state-out");
        let versionlist = synthetic_cache(&cache);
        let version = version_hash(&versionlist);

        assert_eq!(
            snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
            SnapshotState::Missing
        );

        let models = record(1, &[0u8; 18]);
        let anims = record(0, &[0x00, 0x01, 0x75, 0x31]);
        let manifest = plant_snapshot(&out, &versionlist, &models, &anims);
        assert!(matches!(
            snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
            SnapshotState::Ready(_)
        ));

        // An interrupted publication (payload present, marker gone) is never
        // ready, even though the directory is nonempty.
        let dir = out.join(&version);
        std::fs::remove_file(dir.join("manifest")).unwrap();
        let state = snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
        assert!(
            matches!(&state, SnapshotState::Incomplete(reason) if reason.contains("manifest")),
            "missing marker must be incomplete, got {state:?}"
        );

        // A pre-marker (legacy) manifest is not complete either.
        let mut legacy = manifest.clone();
        legacy.complete = false;
        std::fs::write(dir.join("manifest"), manifest_text(&legacy)).unwrap();
        let state = snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
        assert!(
            matches!(&state, SnapshotState::Incomplete(reason) if reason.contains("completion marker")),
            "legacy manifest must be incomplete, got {state:?}"
        );

        std::fs::write(dir.join("manifest"), manifest_text(&manifest)).unwrap();
        std::fs::write(dir.join("models.bin"), &models[..models.len() - 1]).unwrap();
        let state = snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
        assert!(
            matches!(&state, SnapshotState::Incomplete(reason) if reason.contains("models.bin")),
            "a truncated archive must be incomplete, got {state:?}"
        );
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn unpack_cache_publishes_completely_then_reuses_without_rewriting() {
        let cache = tmp("publish-cache");
        let out = tmp("publish-out");
        synthetic_cache(&cache);

        let first = unpack_cache(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
        assert!(first.complete);
        assert_eq!(first.models.unpacked, 1);
        assert_eq!(first.models.bytes, 26); // [id u32][len u32][18 bytes]
        assert_eq!(first.source, "local-store");
        assert!(matches!(
            snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
            SnapshotState::Ready(_)
        ));
        let models = Path::new(&first.dir).join("models.bin");
        let published = std::fs::metadata(&models).unwrap().modified().unwrap();

        // Warm: an already verified snapshot is returned as-is, not rewritten.
        let second = unpack_cache(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            std::fs::metadata(&models).unwrap().modified().unwrap(),
            published,
            "a complete snapshot must not be rewritten"
        );
        assert!(
            !std::fs::read_dir(&out)
                .unwrap()
                .filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().contains(".staging-")),
            "staging directories must be cleaned up"
        );
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn prepare_snapshot_repairs_an_interrupted_snapshot_and_shares_one_attempt() {
        reset_preparation_for_tests();
        let cache = tmp("prepare-cache");
        let out = tmp("prepare-out");
        let versionlist = synthetic_cache(&cache);
        let version = version_hash(&versionlist);

        // A published snapshot whose completion marker was lost (interrupted
        // publish) must be repaired, not accepted.
        let models = record(1, &[0u8; 18]);
        let anims = record(0, &[0x00, 0x01, 0x75, 0x31]);
        plant_snapshot(&out, &versionlist, &models, &anims);
        std::fs::remove_file(out.join(&version).join("manifest")).unwrap();

        let prepared = prepare_snapshot(cache.to_str().unwrap(), out.to_str().unwrap(), None, None);
        match &*prepared {
            SnapshotPreparation::Ready {
                version: prepared_version,
                published,
                models: unpacked_models,
                ..
            } => {
                assert_eq!(prepared_version, &version);
                assert!(published, "an interrupted snapshot must be republished");
                assert_eq!(*unpacked_models, 1);
            }
            other => panic!("expected a repaired snapshot, got {other:?}"),
        }
        assert!(matches!(
            snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
            SnapshotState::Ready(_)
        ));

        // Concurrent callers share one attempt: every thread observes the same
        // outcome, and only the first reports having published it.
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let cache = cache.clone();
                let out = out.clone();
                std::thread::spawn(move || {
                    prepare_snapshot(cache.to_str().unwrap(), out.to_str().unwrap(), None, None)
                })
            })
            .collect();
        let outcomes: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        let first = outcomes[0].clone();
        assert!(
            outcomes.iter().all(|outcome| Arc::ptr_eq(outcome, &first)),
            "concurrent preparations must share one attempt"
        );
        match &*first {
            SnapshotPreparation::Ready { published, .. } => {
                assert!(published, "the shared attempt is the repair that ran");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn failed_preparation_is_shared_but_not_reused() {
        reset_preparation_for_tests();
        let cache = tmp("failed-cache");
        let out = tmp("failed-out");
        let versionlist = synthetic_cache(&cache);
        // Remove the local file store: nothing can be unpacked from here, and
        // no HTTP fetch can supply it.
        std::fs::remove_file(cache.join("main_file_cache.dat")).unwrap();

        let prepared = prepare_snapshot(cache.to_str().unwrap(), out.to_str().unwrap(), None, None);
        match &*prepared {
            SnapshotPreparation::Unavailable { reason } => {
                assert!(
                    reason.contains("main_file_cache.dat"),
                    "the reason must name the missing store, got {reason}"
                );
            }
            other => panic!("unpacking without a file store must be unavailable, got {other:?}"),
        }
        assert!(
            !out.join(&version_hash(&versionlist)).exists(),
            "a failed preparation must not publish anything"
        );

        // A later request retries instead of inheriting the stale failure.
        synthetic_cache(&cache);
        let retried = prepare_snapshot(cache.to_str().unwrap(), out.to_str().unwrap(), None, None);
        assert!(
            retried.is_ready(),
            "a repaired cache must prepare on a later request: {retried:?}"
        );
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
    }

    /// The real local caches must satisfy the same rule the writer enforces:
    /// every entry with a nonzero versionlist version is present in the store
    /// and validates against its CRC/version pairing, while version-0 entries
    /// are the only ones skipped. This is the no-mirror check on real data:
    /// it fails the moment a store and its versionlist disagree.
    #[test]
    fn real_store_required_entries_validate() {
        for (label, cache) in [
            (
                "274",
                std::env::var("HOME")
                    .ok()
                    .map(|h| format!("{h}/experiments/Server/engine/data/pack/client")),
            ),
            (
                "289",
                std::env::var("HOME")
                    .ok()
                    .map(|h| format!("{h}/experiments/lostcity-289/engine/data/pack/client")),
            ),
        ] {
            let Some(cache) = cache else { continue };
            let Ok(versionlist) = std::fs::read(format!("{cache}/versionlist")) else {
                continue;
            };
            let jag = JagFile::new(versionlist);
            for (table, crc_table, idx) in [
                ("model_version", "model_crc", MODELS),
                ("anim_version", "anim_crc", ANIMS),
                ("midi_version", "midi_crc", MIDI),
                ("map_version", "map_crc", MAPS),
            ] {
                let versions_raw = jag.read(table).unwrap_or_default();
                let crcs_raw = jag.read(crc_table).unwrap_or_default();
                let total = versions_raw.len() / 2;
                let store = match file_store_dir(&cache) {
                    Some(store) => store,
                    None => continue,
                };
                let mut idx_file = File::open(format!("{store}/main_file_cache.idx{idx}")).unwrap();
                let idx_entries = (idx_file.metadata().unwrap().len() / 6) as i32;
                let (mut zero, mut nonzero, mut missing_idx, mut zero_size, mut read_fail) =
                    (0, 0, 0, 0, 0);
                let mut bad_validate = 0;
                let mut bad_examples: Vec<(i32, i32, i32, usize)> = Vec::new();
                for file in 0..total as i32 {
                    let version = ((versions_raw[file as usize * 2] as i32) << 8)
                        | versions_raw[file as usize * 2 + 1] as i32;
                    let crc = if (file as usize) * 4 + 4 <= crcs_raw.len() {
                        i32::from_be_bytes([
                            crcs_raw[file as usize * 4],
                            crcs_raw[file as usize * 4 + 1],
                            crcs_raw[file as usize * 4 + 2],
                            crcs_raw[file as usize * 4 + 3],
                        ])
                    } else {
                        0
                    };
                    if version == 0 {
                        zero += 1;
                        continue;
                    }
                    nonzero += 1;
                    if file >= idx_entries {
                        missing_idx += 1;
                        continue;
                    }
                    let size = idx_size(&mut idx_file, file).unwrap();
                    if size <= 0 {
                        zero_size += 1;
                        continue;
                    }
                    let Some(data) = cache_read(&cache, idx, file) else {
                        read_fail += 1;
                        continue;
                    };
                    if !crate::io::ondemand::validate(crc, version, Some(&data)) {
                        bad_validate += 1;
                        if bad_examples.len() < 3 {
                            bad_examples.push((file, version, crc, data.len()));
                        }
                    }
                }
                assert_eq!(
                    (missing_idx, zero_size, read_fail, bad_validate),
                    (0, 0, 0, 0),
                    "{label} {table}: required entries must all be present and validate \
                     (zero={zero} nonzero={nonzero} examples={bad_examples:?})"
                );
                assert_eq!(
                    zero + nonzero,
                    total,
                    "{label} {table}: every versionlist entry is either absent (version 0) or required"
                );
            }
        }
    }

    /// A fake update-server entry source: serves payloads per
    /// `(on-demand archive, file)` and can report entries the engine rejects.
    struct FakeSource {
        payloads: HashMap<(i32, i32), Vec<u8>>,
        absent: Vec<(i32, i32)>,
        calls: usize,
    }

    impl EntrySource for FakeSource {
        fn fetch_entries(
            &mut self,
            archive: i32,
            files: &[i32],
        ) -> Result<Vec<(i32, Vec<u8>)>, String> {
            self.calls += 1;
            let mut out = Vec::new();
            for &file in files {
                if self.absent.contains(&(archive, file)) {
                    return Err(format!(
                        "archive {archive} file {file}: the engine reports the entry absent"
                    ));
                }
                let payload = self
                    .payloads
                    .get(&(archive, file))
                    .cloned()
                    .ok_or_else(|| {
                        format!("archive {archive} file {file}: the engine returned no payload")
                    })?;
                out.push((file, payload));
            }
            Ok(out)
        }
    }

    fn cold_fill_source() -> FakeSource {
        let mut source = FakeSource {
            payloads: HashMap::new(),
            absent: Vec::new(),
            calls: 0,
        };
        source.payloads.insert((0, 0), vec![0u8; 18]);
        source.payloads.insert(
            (1, 0),
            vec![
                0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
        );
        source.payloads.insert((2, 0), vec![1, 2, 3]);
        source.payloads.insert((3, 0), vec![4, 5, 6, 7]);
        source
    }

    #[test]
    fn cold_fill_uses_the_update_server_source_and_requires_every_entry() {
        let cache = tmp("cold-fill-cache");
        let out = tmp("cold-fill-out");
        synthetic_cache(&cache);
        // No local file store at all: the only source left is the update
        // server, which is exactly the genuinely cold case.
        for idx in 1..=4 {
            let _ = std::fs::remove_file(cache.join(format!("main_file_cache.idx{idx}")));
        }
        std::fs::remove_file(cache.join("main_file_cache.dat")).unwrap();

        let mut source = cold_fill_source();
        let manifest =
            fetch_snapshot(cache.to_str().unwrap(), out.to_str().unwrap(), &mut source).unwrap();
        assert_eq!(manifest.source, "update-server");
        assert!(manifest.complete);
        assert_eq!(manifest.models.unpacked, 1);
        assert_eq!(manifest.anims.unpacked, 1);
        assert!(source.calls >= 1, "the fill must go through the source");
        assert!(matches!(
            snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
            SnapshotState::Ready(_)
        ));

        // A required entry the engine cannot produce fails the fill and
        // publishes nothing, even after the earlier archives were staged.
        let out = tmp("cold-fill-failed-out");
        let mut source = cold_fill_source();
        source.absent.push((3, 0));
        let err = fetch_snapshot(cache.to_str().unwrap(), out.to_str().unwrap(), &mut source)
            .unwrap_err();
        assert!(
            err.to_string().contains("absent"),
            "an absent required entry must fail the fill, got: {err}"
        );
        assert!(
            matches!(
                snapshot_state(cache.to_str().unwrap(), out.to_str().unwrap()).unwrap(),
                SnapshotState::Missing
            ),
            "a failed cold fill must not leave a snapshot behind"
        );
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
    fn manifest_text_round_trips() {
        let cache = tmp("manifest-cache");
        let out = tmp("manifest-out");
        let versionlist = synthetic_cache(&cache);
        let models = record(1, &[0u8; 18]);
        let anims = record(0, &[0x00, 0x01, 0x75, 0x31]);
        let manifest = plant_snapshot(&out, &versionlist, &models, &anims);
        let parsed = manifest_from_text(&manifest_text(&manifest)).unwrap();
        assert_eq!(parsed, manifest);
        assert!(manifest_from_text("version=x\n").is_none());
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn empty_record_stream_is_an_error() {
        let path =
            std::env::temp_dir().join(format!("274bot-empty-records-{}", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        let err = load_records(&path, |_, _| {}).unwrap_err();
        assert!(err.to_string().contains("empty"), "got: {err}");
        let _ = std::fs::remove_file(&path);
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
        let version = [0x00u8, 0x01];
        let crc = [0x00u8, 0x00, 0x00, 0x00];
        let versionlist = JagFile::new(jag(&[
            ("model_version", &version[..]),
            ("model_crc", &crc[..]),
        ]));

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

        let tables = ArchiveTables::read(&versionlist, "model_version", "model_crc").unwrap();
        let mut source = StorePayload::open(tmp.to_str().unwrap(), MODELS, &tables).unwrap();
        let err = unpack_archive(&tmp, &tables, MODELS, "models.bin", &mut source).unwrap_err();
        assert!(
            err.to_string().contains("gzip") || err.to_string().contains("version/CRC check"),
            "a payload whose trailer/CRC do not match must be a hard error, got: {err}"
        );
    }

    /// A required (`version != 0`) entry the store cannot produce is an error,
    /// while `version == 0` entries are counted as intentionally absent. This
    /// is the gate that separates "engine never had it" from "preparation is
    /// incomplete".
    #[test]
    fn required_entries_must_be_present_and_validate() {
        let cache = tmp("required-cache");
        let out = tmp("required-out");
        let versionlist = synthetic_cache(&cache);
        let jagfile = JagFile::new(versionlist);

        // One required model entry, present and valid: unpacks.
        let tables = ArchiveTables::read(&jagfile, "model_version", "model_crc").unwrap();
        assert_eq!(tables.total(), 1);
        assert_eq!(tables.required().count(), 1);
        let mut source = StorePayload::open(cache.to_str().unwrap(), MODELS, &tables).unwrap();
        let stats = unpack_archive(&out, &tables, MODELS, "models.bin", &mut source).unwrap();
        assert_eq!(stats.unpacked, 1);
        assert_eq!(stats.skipped, 0);

        // A version-0 entry is intentionally absent, not an error.
        let absent_version = [0x00u8, 0x00];
        let absent_crc = [0x00u8, 0x00, 0x00, 0x00];
        let absent = JagFile::new(jag(&[
            ("model_version", &absent_version[..]),
            ("model_crc", &absent_crc[..]),
        ]));
        let tables = ArchiveTables::read(&absent, "model_version", "model_crc").unwrap();
        assert_eq!(tables.required().count(), 0);
        let mut source = StorePayload::open(cache.to_str().unwrap(), MODELS, &tables).unwrap();
        let stats = unpack_archive(&out, &tables, MODELS, "models.bin", &mut source).unwrap();
        assert_eq!(stats.unpacked, 0);
        assert_eq!(stats.skipped, 1);

        // The same required entry with no store record is a hard error.
        std::fs::write(cache.join("main_file_cache.idx1"), [0, 0, 0, 0, 0, 0]).unwrap();
        let tables = ArchiveTables::read(&jagfile, "model_version", "model_crc").unwrap();
        let mut source = StorePayload::open(cache.to_str().unwrap(), MODELS, &tables).unwrap();
        let err = unpack_archive(&out, &tables, MODELS, "models.bin", &mut source).unwrap_err();
        assert!(
            err.to_string().contains("store entry is empty"),
            "a required entry with no store record must fail, got: {err}"
        );

        // And a required entry whose payload does not validate is an error.
        synthetic_cache(&cache);
        let mut dat = std::fs::read(cache.join("main_file_cache.dat")).unwrap();
        dat[520 + 8] ^= 0xff; // corrupt the gzip payload, leaving the trailer
        std::fs::write(cache.join("main_file_cache.dat"), dat).unwrap();
        let tables = ArchiveTables::read(&jagfile, "model_version", "model_crc").unwrap();
        let mut source = StorePayload::open(cache.to_str().unwrap(), MODELS, &tables).unwrap();
        let err = unpack_archive(&out, &tables, MODELS, "models.bin", &mut source).unwrap_err();
        assert!(
            err.to_string().contains("version/CRC check"),
            "a required entry that does not validate must fail, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&cache);
        let _ = std::fs::remove_dir_all(&out);
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
