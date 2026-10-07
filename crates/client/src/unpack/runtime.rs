//! Pre-freeze preparation. Source files are read-only; an Arc owns the staged
//! packs and snapshot until the last profile releases them. No persisted
//! identity sidecar is trusted, and clients share this result rather than
//! scanning or copying a snapshot per bot.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use sha2::{Digest, Sha256};

use super::{
    fetch_snapshot, refresh_jags_with_checksums, unpack_cache_from_store, FetchEndpoint,
    SnapshotState,
};
use crate::client::Client;
use crate::content_identity::{compute_decoded_content_identity, DecodedContentIdentity};
use crate::io::{ClientRevision, JagFile, OnDemand};
use crate::Transport;

pub struct RuntimeCacheRequest<'a> {
    pub revision: ClientRevision,
    pub transport: Transport,
    /// Read-only source packs and optional main_file_cache store.
    pub jag_source: &'a Path,
    /// Retained snapshots and parent of process-owned runtime directories.
    pub snapshot_root: &'a Path,
    pub asset_host: &'a str,
    pub asset_port: u16,
    pub game_host: &'a str,
    pub game_port: u16,
}

#[derive(Debug)]
pub struct PreparedRuntimeCache {
    owned: Arc<OwnedDirectory>,
    pub jag_dir: PathBuf,
    pub snapshot_dir: PathBuf,
    pub version: String,
    pub expected_crc: [i32; 9],
    pub identity: DecodedContentIdentity,
    pub transfer_sha256: BTreeMap<String, String>,
    pub source: String,
    pub asset_host: String,
    pub asset_port: u16,
    pub fetched: Vec<String>,
    pub reused: Vec<String>,
    /// Read-only original `main_file_cache` when identity-checked maps exist.
    pub store_dir: Option<PathBuf>,
    /// Content-identity overlay for completed ondemand payloads.
    pub persist_dir: PathBuf,
    /// Indexed, read-only maps from the actual-byte-verified runtime copy.
    pub map_archive: Arc<PreparedMapArchive>,
}

impl PreparedRuntimeCache {
    pub fn unpack_root(&self) -> &Path {
        &self.owned.path
    }
}

/// A capability for the verified decoded maps, not a path or digest sidecar.
/// One file/index is shared by the session's OnDemand hub. Holding it also
/// keeps the private runtime directory alive; no decoded world is cloned per bot.
#[derive(Debug)]
pub struct PreparedMapArchive {
    file: Mutex<File>,
    records: HashMap<i32, (u64, u32)>,
    revision: ClientRevision,
    content_id: String,
    jag_dir: PathBuf,
    owned: Arc<OwnedDirectory>,
}

impl PartialEq for PreparedMapArchive {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.content_id == other.content_id
            && self.jag_dir == other.jag_dir
            && self.owned.path == other.owned.path
    }
}

impl Eq for PreparedMapArchive {}

impl PreparedMapArchive {
    fn open(
        snapshot_dir: &Path,
        revision: ClientRevision,
        identity: &DecodedContentIdentity,
        jag_dir: &Path,
        owned: Arc<OwnedDirectory>,
    ) -> Result<Self, String> {
        let mut file = File::open(snapshot_dir.join("maps.bin")).map_err(|e| e.to_string())?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        let mut records = HashMap::new();
        let mut position = 0u64;
        while position < size {
            let mut header = [0u8; 8];
            file.read_exact(&mut header).map_err(|e| e.to_string())?;
            let id = i32::try_from(u32::from_le_bytes(header[..4].try_into().unwrap()))
                .map_err(|e| e.to_string())?;
            let len = u32::from_le_bytes(header[4..].try_into().unwrap());
            let payload = position.checked_add(8).ok_or("map offset overflow")?;
            position = payload
                .checked_add(u64::from(len))
                .ok_or("map length overflow")?;
            if position > size || records.insert(id, (payload, len)).is_some() {
                return Err("invalid prepared map record".into());
            }
            file.seek(SeekFrom::Start(position))
                .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            file: Mutex::new(file),
            records,
            revision,
            content_id: identity.content_id_hex(),
            jag_dir: jag_dir.to_owned(),
            owned,
        })
    }

    pub(crate) fn matches_binding(
        &self,
        revision: ClientRevision,
        content_id: &str,
        jag_dir: &Path,
    ) -> bool {
        self.revision == revision && self.content_id == content_id && self.jag_dir == jag_dir
    }

    pub(crate) fn unpack_root(&self) -> &Path {
        &self.owned.path
    }

    pub(crate) fn contains(&self, id: i32) -> bool {
        self.records.contains_key(&id)
    }

    pub(crate) fn read(&self, id: i32) -> io::Result<Option<Vec<u8>>> {
        let Some(&(offset, len)) = self.records.get(&id) else {
            return Ok(None);
        };
        let mut file = self.file.lock().unwrap_or_else(|p| p.into_inner());
        file.seek(SeekFrom::Start(offset))?;
        let mut data = vec![0; len as usize];
        file.read_exact(&mut data)?;
        Ok(Some(data))
    }
}

#[derive(Debug)]
struct OwnedDirectory {
    path: PathBuf,
    _retention: RetentionLease,
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Registered outside the retained directory, under the short revision lock.
/// The map capability also holds this lease, protecting its persistence overlay.
#[derive(Debug)]
struct RetentionLease(PathBuf);

impl Drop for RetentionLease {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Debug)]
pub enum RuntimeCacheError {
    Asset {
        kind: crate::client::client::AssetFetchError,
        message: String,
    },
    Other(String),
}

impl RuntimeCacheError {
    pub fn is_connection(&self) -> bool {
        matches!(
            self,
            Self::Asset {
                kind: crate::client::client::AssetFetchError::Connection,
                ..
            }
        )
    }
}

impl std::fmt::Display for RuntimeCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Asset { message, .. } | Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for RuntimeCacheError {}

impl From<String> for RuntimeCacheError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for RuntimeCacheError {
    fn from(message: &str) -> Self {
        Self::Other(message.into())
    }
}

impl From<super::UnpackError> for RuntimeCacheError {
    fn from(error: super::UnpackError) -> Self {
        match error.asset_failure {
            Some(kind) => Self::Asset {
                kind,
                message: error.message,
            },
            None => Self::Other(error.message),
        }
    }
}

/// Parse `.runtime-<pid>-<n>` where both sides are exact non-empty decimal runs.
/// Pid must be a positive value representable as a platform process id (never 0).
fn parse_runtime_staging_name(name: &str) -> Option<(u32, u64)> {
    let rest = name.strip_prefix(".runtime-")?;
    let (pid_str, n_str) = rest.split_once('-')?;
    if pid_str.is_empty()
        || n_str.is_empty()
        || !pid_str.bytes().all(|b| b.is_ascii_digit())
        || !n_str.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let pid: u32 = pid_str.parse().ok()?;
    let n: u64 = n_str.parse().ok()?;
    if !is_valid_foreign_pid(pid) {
        return None;
    }
    Some((pid, n))
}

/// Pid values safe to probe and to treat as foreign staging owners.
pub(super) fn is_valid_foreign_pid(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        // Positive pid_t only; never hand 0/negative to kill(2).
        libc::pid_t::try_from(pid).is_ok_and(|p| p > 0)
    }
    #[cfg(not(unix))]
    {
        // Windows process ids are DWORD: every nonzero u32 is probeable.
        true
    }
}

/// True when `pid` still appears to own a live process. Unknown results count as
/// alive so a staging directory that might still be in use is never deleted.
pub(super) fn process_is_alive(pid: u32) -> bool {
    if !is_valid_foreign_pid(pid) {
        // Invalid/special pids are treated as live so their dirs are kept.
        return true;
    }
    #[cfg(unix)]
    {
        let pid_t = pid as libc::pid_t;
        // SAFETY: signal 0 performs no delivery; it only probes existence/permissions.
        // `pid_t` is a positive value checked by `is_valid_foreign_pid`.
        let result = unsafe { libc::kill(pid_t, 0) };
        if result == 0 {
            return true;
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EPERM) => true,
            Some(libc::ESRCH) => false,
            _ => true,
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, STILL_ACTIVE,
        };
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: query-only open; handle is closed before return on every path.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            // SAFETY: OpenProcess failed and set the thread last-error we read here.
            let err = unsafe { GetLastError() };
            // Only the documented no-such-process failure means dead. ACCESS_DENIED
            // and every other error are treated as alive (keep the directory).
            return err != ERROR_INVALID_PARAMETER;
        }
        let mut exit_code = 0u32;
        // SAFETY: `handle` is a process handle from OpenProcess above.
        let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
        // SAFETY: closes the handle opened above.
        unsafe {
            let _ = CloseHandle(handle);
        };
        if ok == 0 {
            return true;
        }
        exit_code == STILL_ACTIVE as u32
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

/// Walk `root` and remove `.runtime-<pid>-<n>` left by dead foreign processes.
/// `is_alive` is injected so tests can supply a deterministic probe.
fn sweep_runtime_staging(root: &Path, self_pid: u32, is_alive: impl Fn(u32) -> bool) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some((pid, _)) = parse_runtime_staging_name(name) else {
            continue;
        };
        if pid == self_pid || is_alive(pid) {
            continue;
        }
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// Once per process per snapshot root, drop `.runtime-<pid>-<n>` directories left
/// by dead foreign processes. Errors and non-matching names are ignored.
fn sweep_leaked_runtime_staging(snapshot_root: &Path) {
    static SWEPT_ROOTS: LazyLock<Mutex<HashSet<PathBuf>>> =
        LazyLock::new(|| Mutex::new(HashSet::new()));
    {
        let mut guard = match SWEPT_ROOTS.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if guard.contains(snapshot_root) {
            return;
        }
        guard.insert(snapshot_root.to_path_buf());
    }

    sweep_runtime_staging(snapshot_root, std::process::id(), process_is_alive);
}

pub fn prepare_runtime_cache(
    request: &RuntimeCacheRequest<'_>,
) -> Result<Arc<PreparedRuntimeCache>, RuntimeCacheError> {
    let checksums = Client::get_jag_checksums_checked(
        request.transport,
        request.asset_host,
        request.asset_port,
    )
    .map_err(|kind| RuntimeCacheError::Asset {
        kind,
        message: format!("update server /crc: {}", kind.message()),
    })?;
    // Negotiate before locking: different revision/transfer negotiations do not
    // serialize. The lock inode is stable and explicitly unlocked on release.
    let revision_root = request
        .snapshot_root
        .join(format!("revision-{}", request.revision.as_i32()));
    std::fs::create_dir_all(&revision_root).map_err(|e| e.to_string())?;
    let negotiation_key = negotiation_key(&checksums);
    let _preparation_lock =
        super::PreparationLock::acquire(&revision_root.join(format!(".{negotiation_key}.lock")))?;
    let negotiation_root = revision_root.join(&negotiation_key);
    let lease = register_retention_owner(&revision_root, &negotiation_key)?;
    // Readers hold the same lock as publication. Packed retained inputs are
    // still individually checked against the freshly negotiated server CRCs.
    let retained_sources = retained_jag_sources(&negotiation_root);
    let mut jag_sources: Vec<&Path> = retained_sources.iter().map(PathBuf::as_path).collect();
    jag_sources.push(request.jag_source);
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(request.snapshot_root).map_err(|e| e.to_string())?;
    sweep_leaked_runtime_staging(request.snapshot_root);
    let owned = loop {
        let path = request.snapshot_root.join(format!(
            ".runtime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&path) {
            Ok(()) => {
                break Arc::new(OwnedDirectory {
                    path,
                    _retention: lease,
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("runtime staging: {e}").into()),
        }
    };
    let jag_dir = owned.path.join("jags");
    let refreshed = refresh_jags_with_checksums(
        &jag_sources,
        &jag_dir,
        FetchEndpoint {
            transport: request.transport,
            host: request.asset_host,
            port: request.asset_port,
        },
        checksums,
    )?;
    let transfer_sha256 = hashes(&jag_dir, &super::JAGS)?;
    let versionlist = std::fs::read(jag_dir.join("versionlist")).map_err(|e| e.to_string())?;
    let version = super::version_hash(&versionlist);
    let snapshot_dir = owned.path.join(&version);
    let transfer_key = format!("{:x}", Sha256::digest(&versionlist));
    let retained_root = negotiation_root.join(&transfer_key);
    let retained = retained_root.join(&version);
    let cache = jag_dir.to_str().ok_or("runtime cache path is not UTF-8")?;
    let root = owned
        .path
        .to_str()
        .ok_or("runtime snapshot path is not UTF-8")?;
    let source;
    let needs_retention;
    if let SnapshotState::Ready(manifest) =
        super::checked_snapshot_state(&retained_root, &version, &versionlist)
    {
        // Recheck after locking, then publish private copies through the same
        // verified staged path. Replacement of retained files cannot mutate an
        // active prepared profile, even after this lock has been released.
        super::retain_completed_snapshot(&retained, &owned.path, &versionlist)?;
        source = manifest.source;
        needs_retention = false;
    } else {
        needs_retention = true;
        // Legacy JagFile parsing can panic. Convert that boundary to a useful
        // preparation error rather than crashing the profile worker.
        let jag = std::panic::catch_unwind(|| JagFile::new(versionlist.clone()))
            .map_err(|_| "invalid runtime versionlist".to_string())?;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            unpack_cache_from_store(cache, root, &request.jag_source.to_string_lossy())
        })) {
            Ok(Ok(manifest)) => source = manifest.source,
            local => {
                let local_error = match local {
                    Ok(Err(e)) => e.to_string(),
                    _ => "malformed local snapshot input".into(),
                };
                let transfer_id = format!("{:x}", Sha256::digest(&versionlist));
                let mut worker = OnDemand::new_bound(
                    &jag,
                    request.transport,
                    request.revision,
                    request.game_host,
                    request.game_port,
                    cache,
                    &transfer_id,
                    super::file_store_dir(&request.jag_source.to_string_lossy()).as_deref(),
                    None,
                    None,
                )
                .map_err(RuntimeCacheError::Other)?;
                let result = fetch_snapshot(cache, root, &mut worker);
                worker.stop();
                source = result
                    .map_err(|e| format!("local store: {local_error}; update-server fill: {e}"))?
                    .source;
            }
        }
    }
    let identity =
        compute_decoded_content_identity(request.revision.as_i32() as u16, &jag_dir, &snapshot_dir)
            .map_err(|e| format!("decoded content identity: {e}"))?;
    // Persist the already checked/decode-complete files, never drive the entry
    // source a second time. Canonical identity above always hashes actual owned
    // bytes; retained metadata is a corruption check, not facts authority.
    let persist_dir = retained.join("ondemand");
    let retention = (|| -> Result<(), RuntimeCacheError> {
        if needs_retention {
            super::retain_completed_snapshot(&snapshot_dir, &retained_root, &versionlist)?;
        }
        std::fs::create_dir_all(&persist_dir).map_err(|e| e.to_string())?;
        Ok(())
    })();
    let persist_dir = match retention {
        Ok(()) => {
            // Registration and pruning share a short revision lock; independent
            // negotiation keys still perform their network fills concurrently.
            prune_retained_copies(&revision_root, &negotiation_key);
            persist_dir
        }
        Err(error) => {
            eprintln!(
                "{}bot: warning: could not save game assets for reuse: {error}; \
                 using verified runtime assets; the next launch will download them again",
                request.revision.as_i32()
            );
            let fallback = snapshot_dir.join("ondemand");
            std::fs::create_dir_all(&fallback).map_err(|e| e.to_string())?;
            fallback
        }
    };
    let store_dir = super::file_store_dir(&request.jag_source.to_string_lossy()).map(PathBuf::from);
    // Index headers only after canonical identity has verified the payloads.
    // The capability retains this immutable private copy, never shared retained files.
    let map_archive = Arc::new(PreparedMapArchive::open(
        &snapshot_dir,
        request.revision,
        &identity,
        &jag_dir,
        Arc::clone(&owned),
    )?);
    Ok(Arc::new(PreparedRuntimeCache {
        owned,
        jag_dir,
        snapshot_dir,
        version,
        expected_crc: checksums,
        identity,
        transfer_sha256,
        source,
        asset_host: request.asset_host.into(),
        asset_port: request.asset_port,
        fetched: refreshed.fetched,
        reused: refreshed.reused,
        store_dir,
        persist_dir,
        map_archive,
    }))
}

fn is_negotiation_key(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

fn register_retention_owner(
    revision_root: &Path,
    key: &str,
) -> Result<RetentionLease, RuntimeCacheError> {
    let _lock = super::PreparationLock::acquire(&revision_root.join(".retention.lock"))?;
    static NEXT_OWNER: AtomicU64 = AtomicU64::new(0);
    loop {
        let path = revision_root.join(format!(
            ".{key}.owner-{}-{}",
            std::process::id(),
            NEXT_OWNER.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(RetentionLease(path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("register retained snapshot owner: {error}").into()),
        }
    }
}

// Three recent namespaces cover switching servers plus one server update without
// allowing abandoned snapshots to accumulate indefinitely. Owner leases may
// temporarily exceed this bound.
const RETAINED_COPY_LIMIT: usize = 3;

/// Keep the most recently used namespaces and any namespace that a live or
/// unknown process may still read/write. Persistent preparation-lock inodes are
/// separate and never removed. The old unnamespaced layout is not visited.
fn prune_retained_copies(revision_root: &Path, newest: &str) {
    let Ok(_lock) = super::PreparationLock::acquire(&revision_root.join(".retention.lock")) else {
        return;
    };
    prune_retained_copies_with_liveness(
        revision_root,
        newest,
        std::process::id(),
        process_is_alive,
    );
}

fn prune_retained_copies_with_liveness(
    revision_root: &Path,
    newest: &str,
    self_pid: u32,
    is_alive: impl Fn(u32) -> bool,
) {
    // Updating a dedicated stamp, rather than the namespace directory's mtime,
    // records verified reuse as well as publication. The time is set outright:
    // rewriting an empty stamp writes no data, and Windows then leaves its
    // modification time alone. A failed stamp must not cause another server's
    // reusable assets to be evicted.
    let stamped = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(revision_root.join(newest).join(".last-used"))
        .and_then(|stamp| stamp.set_modified(std::time::SystemTime::now()));
    if stamped.is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(revision_root) else {
        return;
    };
    let mut protected = HashSet::new();
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if is_negotiation_key(name) && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let path = entry.path();
            let last_used = match std::fs::metadata(path.join(".last-used")) {
                Ok(metadata) => match metadata.modified() {
                    Ok(time) => time,
                    Err(_) => return,
                },
                // Namespaces retained before recency tracking are oldest.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    std::time::SystemTime::UNIX_EPOCH
                }
                Err(_) => return,
            };
            candidates.push((name.to_owned(), path, last_used));
            continue;
        }
        let Some((key, owner)) = name.strip_prefix('.').and_then(|n| n.split_once(".owner-"))
        else {
            continue;
        };
        if !is_negotiation_key(key) {
            continue;
        }
        // Reuse the strict decimal parsing and conservative PID probe of the
        // dead-runtime sweep. Malformed owner records fail safe.
        match parse_runtime_staging_name(&format!(".runtime-{owner}")) {
            Some((pid, _)) if pid != self_pid && !is_alive(pid) => {
                let _ = std::fs::remove_file(entry.path());
            }
            _ => {
                protected.insert(key.to_owned());
            }
        }
    }
    candidates.sort_unstable_by(|(a, _, a_time), (b, _, b_time)| {
        // The just-verified key stays first even if the wall clock moved back.
        (b == newest, b_time, b).cmp(&(a == newest, a_time, a))
    });
    for (key, path, _) in candidates.into_iter().skip(RETAINED_COPY_LIMIT) {
        if !protected.contains(&key) {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn negotiation_key(checksums: &[i32; 9]) -> String {
    let mut hash = Sha256::new();
    for crc in checksums {
        hash.update(crc.to_be_bytes());
    }
    format!("{:x}", hash.finalize())
}

/// Only candidate paths are enumerated here: readiness/digests and server CRCs
/// are checked by the existing publisher and refresh core under the lock.
fn retained_jag_sources(root: &Path) -> Vec<PathBuf> {
    let Ok(transfers) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut sources = Vec::new();
    for transfer in transfers.flatten() {
        if !transfer.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(versions) = std::fs::read_dir(transfer.path()) else {
            continue;
        };
        for version in versions.flatten() {
            if version.file_type().is_ok_and(|kind| kind.is_dir()) {
                sources.push(version.path());
            }
        }
    }
    sources.sort();
    sources
}

fn hashes(dir: &Path, names: &[&str]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut buffer = vec![0; 1024 * 1024];
    for name in names {
        let path = dir.join(name);
        let mut file =
            std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut hash = Sha256::new();
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        out.insert(name.to_string(), format!("{:x}", hash.finalize()));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
