//! Pre-freeze preparation. Source files are read-only; an Arc owns the staged
//! packs and snapshot until the last profile releases them. No persisted
//! identity sidecar is trusted, and clients share this result rather than
//! scanning or copying a snapshot per bot.
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
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
use crate::BotTarget;

pub struct RuntimeCacheRequest<'a> {
    pub revision: ClientRevision,
    pub target: BotTarget,
    /// Read-only source packs and optional main_file_cache store.
    pub jag_source: &'a Path,
    /// Read-only retained snapshots; also parent of owned staging directories.
    pub snapshot_root: &'a Path,
    pub asset_host: &'a str,
    pub asset_port: u16,
    pub game_host: &'a str,
    pub game_port: u16,
}

#[derive(Debug)]
pub struct PreparedRuntimeCache {
    owned: OwnedDirectory,
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
}

impl PreparedRuntimeCache {
    pub fn unpack_root(&self) -> &Path {
        &self.owned.0
    }
}

#[derive(Debug)]
struct OwnedDirectory(PathBuf);
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
fn is_valid_foreign_pid(pid: u32) -> bool {
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
fn process_is_alive(pid: u32) -> bool {
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
    let checksums =
        Client::get_jag_checksums_checked(request.target, request.asset_host, request.asset_port)
            .map_err(|kind| RuntimeCacheError::Asset {
            kind,
            message: format!("update server /crc: {}", kind.message()),
        })?;
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
            Ok(()) => break OwnedDirectory(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("runtime staging: {e}").into()),
        }
    };
    let jag_dir = owned.0.join("jags");
    let refreshed = refresh_jags_with_checksums(
        &[request.jag_source],
        &jag_dir,
        FetchEndpoint {
            target: request.target,
            host: request.asset_host,
            port: request.asset_port,
        },
        checksums,
    )?;
    let transfer_sha256 = hashes(&jag_dir, &super::JAGS)?;
    let versionlist = std::fs::read(jag_dir.join("versionlist")).map_err(|e| e.to_string())?;
    let version = super::version_hash(&versionlist);
    let snapshot_dir = owned.0.join(&version);
    let retained = request.snapshot_root.join(&version);
    let cache = jag_dir.to_str().ok_or("runtime cache path is not UTF-8")?;
    let root = owned
        .0
        .to_str()
        .ok_or("runtime snapshot path is not UTF-8")?;
    let source;
    if let SnapshotState::Ready(manifest) = super::snapshot_state_for_version(
        &request.snapshot_root.to_string_lossy(),
        &version,
        &versionlist,
    ) {
        // Copy retained snapshot bins into this bind's owned staging directory.
        // The owned copies are then decoded; subsequent source replacement cannot
        // change this prepared profile's identity or assets. Publication still
        // writes the manifest last.
        std::fs::create_dir(&snapshot_dir).map_err(|e| e.to_string())?;
        for name in super::BINS {
            std::fs::copy(retained.join(name), snapshot_dir.join(name))
                .map_err(|e| e.to_string())?;
        }
        for name in super::JAGS {
            std::fs::copy(jag_dir.join(name), snapshot_dir.join(name))
                .map_err(|e| e.to_string())?;
        }
        let mut manifest = manifest;
        manifest.dir = snapshot_dir.to_string_lossy().into_owned();
        manifest.jags = super::JAGS
            .iter()
            .map(|name| {
                std::fs::metadata(snapshot_dir.join(name)).map(|m| (name.to_string(), m.len()))
            })
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        std::fs::write(
            snapshot_dir.join("manifest"),
            super::manifest_text(&manifest),
        )
        .map_err(|e| e.to_string())?;
        source = manifest.source;
    } else {
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
                    request.target,
                    request.revision,
                    request.game_host,
                    request.game_port,
                    cache,
                    &transfer_id,
                    super::file_store_dir(&request.jag_source.to_string_lossy()).as_deref(),
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
    let store_dir = super::file_store_dir(&request.jag_source.to_string_lossy()).map(PathBuf::from);
    let persist_dir = request.snapshot_root.join(&version).join("ondemand");
    std::fs::create_dir_all(&persist_dir).map_err(|e| e.to_string())?;
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
    }))
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
