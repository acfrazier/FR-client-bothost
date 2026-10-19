//! Pre-freeze preparation. Source files are read-only; an Arc owns the staged
//! packs and snapshot until the last profile releases them. No persisted
//! identity sidecar is trusted, and clients share this result rather than
//! scanning or copying a snapshot per bot.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::net::SocketAddr;
#[cfg(feature = "standalone-native-transport")]
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::fill::{FillProgress, NetworkSnapshot, SnapshotStart};
use super::http::HttpFetch;
use super::{unpack_cache_from_store, FetchEndpoint, JagRefresh, SnapshotState};
use crate::client::client::AssetFetchError;
use crate::client::Client;
use crate::content_identity::{compute_decoded_content_identity, DecodedContentIdentity};
use crate::io::host_network::{
    installed_host_network, HostLookupAddresses, HostLookupFailure, HostLookupTicket, HostNetwork,
};
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
    /// The resolver could not look up an update server's name (`host`).
    /// This is a connection failure.
    Lookup {
        host: String,
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
            } | Self::Lookup { .. }
        )
    }

    /// The update server name that the resolver could not look up.
    pub fn failed_lookup(&self) -> Option<&str> {
        match self {
            Self::Lookup { host, .. } => Some(host),
            _ => None,
        }
    }
}

impl std::fmt::Display for RuntimeCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Asset { message, .. } | Self::Lookup { message, .. } | Self::Other(message) => {
                f.write_str(message)
            }
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

/// Windows `ERROR_INVALID_PARAMETER`. Only this `OpenProcess` failure means dead.
#[cfg(any(windows, test))]
const OPEN_PROCESS_DEAD: u32 = 87;

/// Windows `ERROR_ACCESS_DENIED`. This failure means the process may still be live.
#[cfg(any(windows, test))]
const OPEN_PROCESS_ACCESS_DENIED: u32 = 5;

#[cfg(windows)]
const _: () = {
    assert!(windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER == OPEN_PROCESS_DEAD);
    assert!(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED == OPEN_PROCESS_ACCESS_DENIED);
};

/// Classify an `OpenProcess` failure from an injected code.
///
/// Only `ERROR_INVALID_PARAMETER` means the process is gone. Access denied and
/// every other error mean it may still be live.
#[cfg(any(windows, test))]
fn open_process_failure_means_alive(error: u32) -> bool {
    error != OPEN_PROCESS_DEAD
}

/// True when `pid` still appears to own a live process. Unknown results count as
/// alive so a staging directory that might still be in use is never deleted.
pub fn process_is_alive(pid: u32) -> bool {
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
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, STILL_ACTIVE};
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
            return open_process_failure_means_alive(err);
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

/// Prepare a runtime cache in one call: [`RuntimeCacheFill`] stepped with no
/// deadline until it is ready.
pub fn prepare_runtime_cache(
    request: &RuntimeCacheRequest<'_>,
) -> Result<Arc<PreparedRuntimeCache>, RuntimeCacheError> {
    let mut fill = RuntimeCacheFill::begin(request);
    loop {
        if let RuntimeCacheStep::Ready(prepared) = fill.step(None)? {
            return Ok(prepared);
        }
    }
}

/// Outcome of one [`RuntimeCacheFill::step`].
#[derive(Debug)]
pub enum RuntimeCacheStep {
    /// More steps follow. During an update-server fill `total` is the number
    /// of required entries and `written` the records written so far.
    Continue {
        written: u32,
        total: Option<u32>,
    },
    Ready(Arc<PreparedRuntimeCache>),
}

/// [`prepare_runtime_cache`] in bounded steps for a cooperative scheduler.
/// One step does one of: asset-host lookup; the `/crc` request; the preparation
/// and retention locks; one JAG pack; the retained or local-store snapshot;
/// game-host lookup; the update-server fill; publication; decoded identity;
/// retention, pruning and the map index. A deadline bounds network and lock
/// waits. An unfinished request or a held lock continues in the next step.
/// Hosted DNS runs on the process Resolver. The fill preserves typed lookup
/// failures and reports backend lookups that finish after their first step
/// deadline to the observer from [`Self::observe_slow_lookups`]. Standalone
/// fallback lookups use the same observer. Calls keep the one-call order.
///
/// Between steps the fill holds the preparation lock, the retention lease,
/// the runtime and staging directories, an HTTP request or resolver ticket in
/// flight, and the fill's OnDemand worker. Dropping it before it is ready, or
/// after an error, stops the worker, removes the staging and runtime
/// directories, and releases the lease and lock. Nothing is published.
/// A later fill starts again cleanly.
pub struct RuntimeCacheFill {
    request: OwnedRuntimeCacheRequest,
    #[cfg(feature = "standalone-native-transport")]
    lookups: Lookups,
    hosted_lookups: HostedLookups,
    slow_lookups: Option<fn(&SlowLookup<'_>)>,
    stage: Stage,
}

#[derive(Default)]
struct HostedLookups {
    asset: Option<HostLookupAddresses>,
    asset_pending: Option<PendingHostLookup>,
    game: Option<HostLookupAddresses>,
    game_pending: Option<PendingHostLookup>,
}

struct PendingHostLookup {
    ticket: Box<dyn HostLookupTicket>,
    step_deadline: Option<Instant>,
}

/// A name lookup of a [`RuntimeCacheFill`] that ended after its step deadline.
/// Hosted DNS reports backend timing from the process Resolver. The standalone
/// fallback reports the timing of its system resolver call.
#[derive(Debug, Clone, Copy)]
pub struct SlowLookup<'a> {
    /// `"asset host"` or `"game host"`.
    pub role: &'static str,
    /// The name that the fill looked up.
    pub host: &'a str,
    /// The time that the resolver backend took.
    pub took: Duration,
    /// The time from the step deadline to the end of the lookup.
    pub past_deadline: Duration,
}

/// The standalone-native address results, each host looked up at most once,
/// kept until the fill ends. Hosted mode leaves these empty.
#[cfg(feature = "standalone-native-transport")]
#[derive(Default)]
struct Lookups {
    /// Standalone asset-host addresses for `/crc` and stepped JAG requests.
    asset: Option<Vec<SocketAddr>>,
    /// Standalone game-host addresses for the OnDemand worker.
    game: Option<Vec<SocketAddr>>,
}

/// Resolve a host only for the standalone-native fallback, which has no
/// non-blocking resolver. Hosted paths use the process Resolver through
/// HostNetwork instead.
#[cfg(feature = "standalone-native-transport")]
fn looked_up<'a>(
    slot: &'a mut Option<Vec<SocketAddr>>,
    observer: Option<fn(&SlowLookup<'_>)>,
    role: &'static str,
    host: &str,
    port: u16,
    until: Option<Instant>,
) -> io::Result<Option<&'a [SocketAddr]>> {
    if slot.is_some() {
        return Ok(slot.as_deref());
    }
    let started = Instant::now();
    let addrs = (host, port).to_socket_addrs();
    let ended = Instant::now();
    if let (Some(observer), Some(until)) = (observer, until.filter(|until| ended > *until)) {
        observer(&SlowLookup {
            role,
            host,
            took: ended - started,
            past_deadline: ended - until,
        });
    }
    *slot = Some(addrs?.collect());
    Ok(None)
}

fn poll_hosted_lookup<'a>(
    network: &dyn HostNetwork,
    pending: &'a mut Option<PendingHostLookup>,
    resolved: &'a mut Option<HostLookupAddresses>,
    observer: Option<fn(&SlowLookup<'_>)>,
    role: &'static str,
    host: &str,
    port: u16,
    until: Option<Instant>,
) -> Result<Option<&'a [SocketAddr]>, HostLookupFailure> {
    if let Some(addresses) = resolved.as_ref() {
        return Ok(Some(addresses.as_slice()));
    }
    if pending.is_none() {
        *pending = Some(PendingHostLookup {
            ticket: network.start_cold_fill_lookup(host, port)?,
            step_deadline: until,
        });
    }
    let poll_until = until.unwrap_or_else(|| Instant::now() + super::http::MAINTENANCE_INTERVAL);
    match pending
        .as_mut()
        .expect("started hosted lookup")
        .ticket
        .poll(poll_until)
    {
        Ok(Some(addresses)) => {
            let completed = pending.take().expect("completed hosted lookup");
            report_slow_hosted_lookup(
                observer,
                role,
                host,
                completed.step_deadline,
                completed.ticket.backend_timing(),
            );
            *resolved = Some(addresses);
            Ok(resolved.as_ref().map(HostLookupAddresses::as_slice))
        }
        Ok(None) => Ok(None),
        Err(error) => {
            let completed = pending.take().expect("failed hosted lookup");
            report_slow_hosted_lookup(
                observer,
                role,
                host,
                completed.step_deadline,
                completed.ticket.backend_timing(),
            );
            Err(error)
        }
    }
}

fn report_slow_hosted_lookup(
    observer: Option<fn(&SlowLookup<'_>)>,
    role: &'static str,
    host: &str,
    step_deadline: Option<Instant>,
    timing: Option<(Instant, Instant)>,
) {
    let (Some(observer), Some(step_deadline), Some((started, ended))) =
        (observer, step_deadline, timing)
    else {
        return;
    };
    if ended > step_deadline {
        observer(&SlowLookup {
            role,
            host,
            took: ended.saturating_duration_since(started),
            past_deadline: ended.saturating_duration_since(step_deadline),
        });
    }
}

fn addresses_with_port(addresses: &[SocketAddr], port: u16) -> HostLookupAddresses {
    let mut output = [SocketAddr::from(([0, 0, 0, 0], 0)); 8];
    let len = addresses.len();
    for (destination, address) in output[..len].iter_mut().zip(addresses.iter().copied()) {
        *destination = address;
        destination.set_port(port);
    }
    HostLookupAddresses::from_slice(&output[..len])
}

fn hosted_lookup_error(
    host: &str,
    failure: HostLookupFailure,
    message: String,
) -> RuntimeCacheError {
    match failure {
        HostLookupFailure::Lookup => RuntimeCacheError::Lookup {
            host: host.to_owned(),
            message: format!("{message}: name lookup failed"),
        },
        failure => RuntimeCacheError::Other(format!("{message}: hosted lookup {failure:?}")),
    }
}

struct OwnedRuntimeCacheRequest {
    revision: ClientRevision,
    transport: Transport,
    jag_source: PathBuf,
    snapshot_root: PathBuf,
    asset_host: String,
    asset_port: u16,
    game_host: String,
    game_port: u16,
}

impl OwnedRuntimeCacheRequest {
    fn endpoint(&self) -> FetchEndpoint<'_> {
        FetchEndpoint {
            transport: self.transport,
            host: &self.asset_host,
            port: self.asset_port,
        }
    }
}

enum Stage {
    /// The `/crc` request, once a stepped one has started.
    Negotiate(Option<Box<HttpFetch>>),
    /// The namespace lock, once taken, waits here for the retention lock.
    Lock(Negotiation, Option<super::PreparationLock>),
    /// The pack slot, and its download once a stepped one has started.
    Jag(Box<Held>, usize, Option<Box<HttpFetch>>),
    Snapshot(Box<Held>),
    LocalStore(Box<Held>),
    /// The local store failed: find a reusable snapshot before resolving the
    /// game host and starting the update-server fill.
    Worker(Box<Held>, Box<JagFile>),
    Network(Box<Held>),
    Publish(Box<Held>),
    Identity(Box<Held>),
    Finish(Box<Held>, DecodedContentIdentity),
    /// Retention is done; pruning waits for the retention lock.
    Prune(Box<Held>, DecodedContentIdentity, Retained),
    /// Ready, failed or dropped: nothing is held.
    Done,
}

/// Where the finish step left the retained copy.
struct Retained {
    persist_dir: PathBuf,
    /// Retention succeeded, so the revision's older copies are pruned.
    prune: bool,
}

struct Negotiation {
    checksums: [i32; 9],
    revision_root: PathBuf,
    negotiation_key: String,
}

/// State that lives across steps. Fields drop in declaration order: the
/// worker stops before the staging directory goes, and the lock is last.
struct Held {
    worker: Option<OnDemand>,
    snapshot: Option<NetworkSnapshot>,
    owned: Arc<OwnedDirectory>,
    _lock: super::PreparationLock,
    checksums: [i32; 9],
    revision_root: PathBuf,
    negotiation_key: String,
    negotiation_root: PathBuf,
    jag_sources: Vec<PathBuf>,
    jag_dir: PathBuf,
    refresh: JagRefresh,
    files: Option<SnapshotFiles>,
    local_error: String,
    entries: Option<u32>,
}

/// Paths and digests fixed once the JAG packs are in place.
struct SnapshotFiles {
    transfer_sha256: BTreeMap<String, String>,
    versionlist: Vec<u8>,
    version: String,
    snapshot_dir: PathBuf,
    retained_root: PathBuf,
    retained: PathBuf,
    source: String,
    needs_retention: bool,
}

impl Held {
    fn files(&self) -> &SnapshotFiles {
        self.files
            .as_ref()
            .expect("snapshot files follow the JAG stage")
    }

    fn files_mut(&mut self) -> &mut SnapshotFiles {
        self.files
            .as_mut()
            .expect("snapshot files follow the JAG stage")
    }

    fn cache(&self) -> Result<&str, RuntimeCacheError> {
        Ok(self
            .jag_dir
            .to_str()
            .ok_or("runtime cache path is not UTF-8")?)
    }

    fn root(&self) -> Result<&str, RuntimeCacheError> {
        Ok(self
            .owned
            .path
            .to_str()
            .ok_or("runtime snapshot path is not UTF-8")?)
    }

    fn progress(&self) -> RuntimeCacheStep {
        RuntimeCacheStep::Continue {
            written: self.entries.unwrap_or(0),
            total: self.entries,
        }
    }
}

impl RuntimeCacheFill {
    /// Capture the request. No I/O happens until the first [`Self::step`].
    pub fn begin(request: &RuntimeCacheRequest<'_>) -> Self {
        Self {
            request: OwnedRuntimeCacheRequest {
                revision: request.revision,
                transport: request.transport,
                jag_source: request.jag_source.to_path_buf(),
                snapshot_root: request.snapshot_root.to_path_buf(),
                asset_host: request.asset_host.to_string(),
                asset_port: request.asset_port,
                game_host: request.game_host.to_string(),
                game_port: request.game_port,
            },
            #[cfg(feature = "standalone-native-transport")]
            lookups: Lookups::default(),
            hosted_lookups: HostedLookups::default(),
            slow_lookups: None,
            stage: Stage::Negotiate(None),
        }
    }

    /// Give each name lookup that completes after its step deadline to
    /// `observer` ([`SlowLookup`]).
    pub fn observe_slow_lookups(&mut self, observer: fn(&SlowLookup<'_>)) {
        self.slow_lookups = Some(observer);
    }

    /// Do the next bounded unit of preparation. Waits for the update server,
    /// the locks or update-server entries end at `until`; `None` waits as
    /// the one-call preparation does. After an error the fill holds nothing
    /// and every later step fails.
    pub fn step(&mut self, until: Option<Instant>) -> Result<RuntimeCacheStep, RuntimeCacheError> {
        let stage = std::mem::replace(&mut self.stage, Stage::Done);
        let (next, outcome) = self.advance(stage, until)?;
        self.stage = next;
        Ok(outcome)
    }

    fn advance(
        &mut self,
        stage: Stage,
        until: Option<Instant>,
    ) -> Result<(Stage, RuntimeCacheStep), RuntimeCacheError> {
        let request = &self.request;
        let waiting = RuntimeCacheStep::Continue {
            written: 0,
            total: None,
        };
        match stage {
            Stage::Negotiate(fetch) => {
                let crc_error = |kind: AssetFetchError| RuntimeCacheError::Asset {
                    kind,
                    message: format!("update server /crc: {}", kind.message()),
                };
                #[cfg(feature = "standalone-native-transport")]
                let lookup_error = |_: io::Error| RuntimeCacheError::Lookup {
                    host: request.asset_host.clone(),
                    message: crc_error(AssetFetchError::Connection).to_string(),
                };
                let mut fetch = fetch;
                if fetch.is_none() {
                    let addresses = if let Some(network) = installed_host_network() {
                        let Some(addresses) = poll_hosted_lookup(
                            network.as_ref(),
                            &mut self.hosted_lookups.asset_pending,
                            &mut self.hosted_lookups.asset,
                            self.slow_lookups,
                            "asset host",
                            &request.asset_host,
                            request.asset_port,
                            until,
                        )
                        .map_err(|failure| {
                            hosted_lookup_error(
                                &request.asset_host,
                                failure,
                                crc_error(AssetFetchError::Connection).to_string(),
                            )
                        })?
                        else {
                            return Ok((Stage::Negotiate(None), waiting));
                        };
                        Some(addresses)
                    } else {
                        #[cfg(feature = "standalone-native-transport")]
                        {
                            let Some(addresses) = looked_up(
                                &mut self.lookups.asset,
                                self.slow_lookups,
                                "asset host",
                                &request.asset_host,
                                request.asset_port,
                                until,
                            )
                            .map_err(lookup_error)?
                            else {
                                return Ok((Stage::Negotiate(None), waiting));
                            };
                            Some(addresses)
                        }
                        #[cfg(not(feature = "standalone-native-transport"))]
                        {
                            None
                        }
                    };
                    let fetch_started = match addresses {
                        Some(addresses) => HttpFetch::start_resolved(
                            request.transport,
                            &request.asset_host,
                            addresses,
                            "/crc",
                        ),
                        None => HttpFetch::start(
                            request.transport,
                            &request.asset_host,
                            request.asset_port,
                            "/crc",
                        ),
                    }
                    .map_err(crc_error)?;
                    fetch = Some(Box::new(fetch_started));
                }
                let mut fetch = fetch.expect("the /crc fetch was started");
                let body = match until {
                    Some(until) => fetch.poll(until).map_err(crc_error)?,
                    None => Some(
                        fetch
                            .blocking_with_maintenance(&mut |_| {})
                            .map_err(crc_error)?,
                    ),
                };
                let Some(body) = body else {
                    return Ok((Stage::Negotiate(Some(fetch)), waiting));
                };
                let checksums = Client::parse_jag_checksums(body)
                    .map_err(|_| crc_error(AssetFetchError::Checksum))?;
                // Negotiate before locking: different revision/transfer
                // negotiations do not serialize. The lock inode is stable and
                // explicitly unlocked on release.
                let revision_root = request
                    .snapshot_root
                    .join(format!("revision-{}", request.revision.as_i32()));
                std::fs::create_dir_all(&revision_root).map_err(|e| e.to_string())?;
                let negotiation_key = negotiation_key(&checksums);
                Ok((
                    Stage::Lock(
                        Negotiation {
                            checksums,
                            revision_root,
                            negotiation_key,
                        },
                        None,
                    ),
                    waiting,
                ))
            }
            Stage::Lock(negotiation, lock) => {
                let lock = match lock {
                    Some(lock) => lock,
                    None => {
                        let lock_path = negotiation
                            .revision_root
                            .join(format!(".{}.lock", negotiation.negotiation_key));
                        match until {
                            None => super::PreparationLock::acquire(&lock_path)?,
                            Some(until) => {
                                match super::PreparationLock::acquire_until(&lock_path, until)? {
                                    Some(lock) => lock,
                                    // Another process is preparing the same namespace.
                                    None => return Ok((Stage::Lock(negotiation, None), waiting)),
                                }
                            }
                        }
                    }
                };
                let Some(lease) = register_retention_owner(
                    &negotiation.revision_root,
                    &negotiation.negotiation_key,
                    until,
                )?
                else {
                    // Another preparation is registering or pruning.
                    return Ok((Stage::Lock(negotiation, Some(lock)), waiting));
                };
                let held = self.lock_acquired(negotiation, lock, lease)?;
                Ok((Stage::Jag(held, 0, None), waiting))
            }
            Stage::Jag(mut held, slot, fetch) => {
                let in_place = fetch.is_none() && {
                    let sources: Vec<&Path> =
                        held.jag_sources.iter().map(PathBuf::as_path).collect();
                    super::reuse_jag(&sources, &held.jag_dir, slot, &mut held.refresh)?
                };
                if !in_place {
                    let mut fetch = match fetch {
                        Some(fetch) => fetch,
                        None => {
                            let addresses = if installed_host_network().is_some() {
                                Some(
                                    self.hosted_lookups
                                        .asset
                                        .as_ref()
                                        .ok_or_else(|| {
                                            RuntimeCacheError::Other(
                                                "hosted asset addresses missing after /crc".into(),
                                            )
                                        })?
                                        .as_slice(),
                                )
                            } else {
                                #[cfg(feature = "standalone-native-transport")]
                                {
                                    self.lookups.asset.as_deref()
                                }
                                #[cfg(not(feature = "standalone-native-transport"))]
                                {
                                    None
                                }
                            };
                            Box::new(super::start_jag_download(
                                request.endpoint(),
                                addresses,
                                slot,
                                &held.refresh,
                            )?)
                        }
                    };
                    let body = match until {
                        Some(until) => fetch
                            .poll(until)
                            .map_err(|kind| super::jag_fetch_error(slot, kind))?,
                        None => Some(
                            fetch
                                .blocking_with_maintenance(&mut |_| {})
                                .map_err(|kind| super::jag_fetch_error(slot, kind))?,
                        ),
                    };
                    let Some(body) = body else {
                        return Ok((Stage::Jag(held, slot, Some(fetch)), waiting));
                    };
                    super::accept_jag_download(
                        &held.jag_dir,
                        slot,
                        &mut held.refresh,
                        body,
                    )?;
                }
                if slot + 1 < super::JAGS.len() {
                    return Ok((Stage::Jag(held, slot + 1, None), waiting));
                }
                super::PREPARE_FETCHED
                    .fetch_add(held.refresh.fetched.len() as u64, Ordering::Relaxed);
                Ok((Stage::Snapshot(held), waiting))
            }
            Stage::Snapshot(mut held) => {
                let transfer_sha256 = hashes(&held.jag_dir, &super::JAGS)?;
                let versionlist =
                    std::fs::read(held.jag_dir.join("versionlist")).map_err(|e| e.to_string())?;
                let version = super::version_hash(&versionlist);
                let snapshot_dir = held.owned.path.join(&version);
                let transfer_key = format!("{:x}", Sha256::digest(&versionlist));
                let retained_root = held.negotiation_root.join(&transfer_key);
                let retained = retained_root.join(&version);
                held.cache()?;
                held.root()?;
                let ready = super::checked_snapshot_state(&retained_root, &version, &versionlist);
                held.files = Some(SnapshotFiles {
                    transfer_sha256,
                    versionlist,
                    version,
                    snapshot_dir,
                    retained_root,
                    retained,
                    source: String::new(),
                    needs_retention: true,
                });
                let SnapshotState::Ready(manifest) = ready else {
                    return Ok((Stage::LocalStore(held), waiting));
                };
                // Recheck after locking, then publish private copies through
                // the same verified staged path. Replacement of retained files
                // cannot mutate an active prepared profile, even after this
                // lock has been released.
                let files = held.files();
                super::retain_completed_snapshot(
                    &files.retained,
                    &held.owned.path,
                    &files.versionlist,
                )?;
                let files = held.files_mut();
                files.source = manifest.source;
                files.needs_retention = false;
                Ok((Stage::Identity(held), waiting))
            }
            Stage::LocalStore(mut held) => {
                // Legacy JagFile parsing can panic. Convert that boundary to a
                // useful preparation error rather than crashing the worker.
                let versionlist = held.files().versionlist.clone();
                let jag = std::panic::catch_unwind(|| JagFile::new(versionlist))
                    .map_err(|_| "invalid runtime versionlist".to_string())?;
                let (cache, root) = (held.cache()?.to_string(), held.root()?.to_string());
                let store = request.jag_source.to_string_lossy().into_owned();
                let local = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    unpack_cache_from_store(&cache, &root, &store)
                }));
                let local_error = match local {
                    Ok(Ok(manifest)) => {
                        held.files_mut().source = manifest.source;
                        return Ok((Stage::Identity(held), waiting));
                    }
                    Ok(Err(e)) => e.to_string(),
                    Err(_) => "malformed local snapshot input".into(),
                };
                held.local_error = local_error;
                Ok((Stage::Worker(held, Box::new(jag)), waiting))
            }
            Stage::Worker(mut held, jag) => {
                let (cache, root) = (held.cache()?.to_string(), held.root()?.to_string());
                if held.snapshot.is_none() {
                    match NetworkSnapshot::begin(&cache, &root).map_err(|e| held.fill_error(e))? {
                        SnapshotStart::Ready(manifest) => {
                            held.files_mut().source = manifest.source;
                            return Ok((Stage::Identity(held), waiting));
                        }
                        SnapshotStart::Fill(snapshot) => held.snapshot = Some(*snapshot),
                    }
                }
                #[cfg(feature = "standalone-native-transport")]
                if installed_host_network().is_none()
                    && self.lookups.game.is_none()
                    && request.game_host == request.asset_host
                {
                    // The same host: its standalone asset lookup serves the
                    // game port.
                    self.lookups.game = self.lookups.asset.as_ref().map(|addrs| {
                        addrs
                            .iter()
                            .map(|addr| {
                                let mut addr = *addr;
                                addr.set_port(request.game_port);
                                addr
                            })
                            .collect()
                    });
                }
                if installed_host_network().is_some()
                    && self.hosted_lookups.game.is_none()
                    && request.game_host == request.asset_host
                {
                    self.hosted_lookups.game = self.hosted_lookups.asset.as_ref().map(|addresses| {
                        if request.game_port == request.asset_port {
                            *addresses
                        } else {
                            addresses_with_port(addresses.as_slice(), request.game_port)
                        }
                    });
                }
                let addrs = if let Some(network) = installed_host_network() {
                    let resolved = poll_hosted_lookup(
                        network.as_ref(),
                        &mut self.hosted_lookups.game_pending,
                        &mut self.hosted_lookups.game,
                        self.slow_lookups,
                        "game host",
                        &request.game_host,
                        request.game_port,
                        until,
                    )
                    .map_err(|failure| {
                        hosted_lookup_error(
                            &request.game_host,
                            failure,
                            format!(
                                "local store: {}; update-server fill: update server {}",
                                held.local_error, request.game_host
                            ),
                        )
                    })?;
                    let Some(addrs) = resolved else {
                        return Ok((Stage::Worker(held, jag), waiting));
                    };
                    Some(addrs)
                } else {
                    #[cfg(feature = "standalone-native-transport")]
                    {
                        let addrs = looked_up(
                            &mut self.lookups.game,
                            self.slow_lookups,
                            "game host",
                            &request.game_host,
                            request.game_port,
                            until,
                        )
                        .map_err(|error| RuntimeCacheError::Lookup {
                            host: request.game_host.clone(),
                            message: format!(
                                "local store: {}; update-server fill: update server {}: \
                                 name lookup failed: {error}",
                                held.local_error, request.game_host
                            ),
                        })?;
                        let Some(addrs) = addrs else {
                            return Ok((Stage::Worker(held, jag), waiting));
                        };
                        Some(addrs)
                    }
                    #[cfg(not(feature = "standalone-native-transport"))]
                    {
                        None
                    }
                };
                let store = request.jag_source.to_string_lossy().into_owned();
                let transfer_id = format!("{:x}", Sha256::digest(&held.files().versionlist));
                let worker = OnDemand::new_cold_fill(
                    &jag,
                    request.transport,
                    request.revision,
                    &request.game_host,
                    request.game_port,
                    addrs,
                    &cache,
                    &transfer_id,
                    super::file_store_dir(&store).as_deref(),
                )
                .map_err(RuntimeCacheError::Other)?;
                held.worker = Some(worker);
                let total = held
                    .snapshot
                    .as_ref()
                    .expect("the worker stage retains its network snapshot")
                    .total();
                Ok((
                    Stage::Network(held),
                    RuntimeCacheStep::Continue {
                        written: 0,
                        total: Some(total),
                    },
                ))
            }
            Stage::Network(mut held) => {
                let Held {
                    worker, snapshot, ..
                } = &mut *held;
                let (Some(worker), Some(snapshot)) = (worker.as_mut(), snapshot.as_mut()) else {
                    unreachable!("the network stage owns its worker and snapshot");
                };
                let total = snapshot.total();
                let progress = snapshot.step(worker, until);
                match progress.map_err(|e| held.fill_error(e))? {
                    FillProgress::Continue { written, total } => Ok((
                        Stage::Network(held),
                        RuntimeCacheStep::Continue {
                            written,
                            total: Some(total),
                        },
                    )),
                    FillProgress::Staged => {
                        held.entries = Some(total);
                        let outcome = held.progress();
                        Ok((Stage::Publish(held), outcome))
                    }
                }
            }
            Stage::Publish(mut held) => {
                let snapshot = held
                    .snapshot
                    .take()
                    .expect("the publish stage owns its staged snapshot");
                let published = snapshot.publish();
                held.stop_worker();
                let manifest = published.map_err(|e| held.fill_error(e))?;
                held.files_mut().source = manifest.source;
                let outcome = held.progress();
                Ok((Stage::Identity(held), outcome))
            }
            Stage::Identity(held) => {
                let identity = compute_decoded_content_identity(
                    request.revision.as_i32() as u16,
                    &held.jag_dir,
                    &held.files().snapshot_dir,
                )
                .map_err(|e| format!("decoded content identity: {e}"))?;
                let outcome = held.progress();
                Ok((Stage::Finish(held, identity), outcome))
            }
            Stage::Finish(held, identity) => {
                let retained = self.retain(&held)?;
                self.prune_and_index(held, identity, retained, until)
            }
            Stage::Prune(held, identity, retained) => {
                self.prune_and_index(held, identity, retained, until)
            }
            Stage::Done => Err("runtime cache preparation already finished".into()),
        }
    }

    /// Create the private runtime directory for a registered lease.
    fn lock_acquired(
        &self,
        negotiation: Negotiation,
        lock: super::PreparationLock,
        lease: RetentionLease,
    ) -> Result<Box<Held>, RuntimeCacheError> {
        let request = &self.request;
        let Negotiation {
            checksums,
            revision_root,
            negotiation_key,
        } = negotiation;
        let negotiation_root = revision_root.join(&negotiation_key);
        // Readers hold the same lock as publication. Packed retained inputs
        // are still individually checked against the freshly negotiated CRCs.
        let mut jag_sources = retained_jag_sources(&negotiation_root);
        jag_sources.push(request.jag_source.clone());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(&request.snapshot_root).map_err(|e| e.to_string())?;
        sweep_leaked_runtime_staging(&request.snapshot_root);
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
        let sources: Vec<&Path> = jag_sources.iter().map(PathBuf::as_path).collect();
        super::prepare_jag_dest(&sources, &jag_dir)?;
        Ok(Box::new(Held {
            worker: None,
            snapshot: None,
            owned,
            _lock: lock,
            checksums,
            revision_root,
            negotiation_key,
            negotiation_root,
            jag_sources,
            jag_dir,
            refresh: JagRefresh {
                checksums,
                fetched: Vec::new(),
                reused: Vec::new(),
            },
            files: None,
            local_error: String::new(),
            entries: None,
        }))
    }

    /// Save the checked runtime files for reuse. A failure only costs the
    /// next launch a download, so it falls back to the runtime copy.
    fn retain(&self, held: &Held) -> Result<Retained, RuntimeCacheError> {
        let request = &self.request;
        let files = held.files();
        // Persist the already checked/decode-complete files, never drive the
        // entry source a second time. Canonical identity always hashes actual
        // owned bytes; retained metadata is a corruption check, not facts
        // authority.
        let persist_dir = files.retained.join("ondemand");
        let retention = (|| -> Result<(), RuntimeCacheError> {
            if files.needs_retention {
                super::retain_completed_snapshot(
                    &files.snapshot_dir,
                    &files.retained_root,
                    &files.versionlist,
                )?;
            }
            std::fs::create_dir_all(&persist_dir).map_err(|e| e.to_string())?;
            Ok(())
        })();
        match retention {
            Ok(()) => Ok(Retained {
                persist_dir,
                prune: true,
            }),
            Err(error) => {
                eprintln!(
                    "{}bot: warning: could not save game assets for reuse: {error}; \
                     using verified runtime assets; the next launch will download them again",
                    request.revision.as_i32()
                );
                let fallback = files.snapshot_dir.join("ondemand");
                std::fs::create_dir_all(&fallback).map_err(|e| e.to_string())?;
                Ok(Retained {
                    persist_dir: fallback,
                    prune: false,
                })
            }
        }
    }

    /// Prune older retained copies, then index the map headers. With a
    /// deadline, a retention lock that another holder keeps past it leaves
    /// the pruning to the next step.
    fn prune_and_index(
        &self,
        held: Box<Held>,
        identity: DecodedContentIdentity,
        retained: Retained,
        until: Option<Instant>,
    ) -> Result<(Stage, RuntimeCacheStep), RuntimeCacheError> {
        // Registration and pruning share a short revision lock; independent
        // negotiation keys still fill concurrently.
        if retained.prune
            && !prune_retained_copies(&held.revision_root, &held.negotiation_key, until)
        {
            let outcome = held.progress();
            return Ok((Stage::Prune(held, identity, retained), outcome));
        }
        let request = &self.request;
        let files = held.files();
        let store_dir =
            super::file_store_dir(&request.jag_source.to_string_lossy()).map(PathBuf::from);
        // Index headers only after canonical identity has verified the
        // payloads. The capability retains this immutable private copy, never
        // shared retained files.
        let map_archive = Arc::new(PreparedMapArchive::open(
            &files.snapshot_dir,
            request.revision,
            &identity,
            &held.jag_dir,
            Arc::clone(&held.owned),
        )?);
        let prepared = Arc::new(PreparedRuntimeCache {
            owned: Arc::clone(&held.owned),
            jag_dir: held.jag_dir.clone(),
            snapshot_dir: files.snapshot_dir.clone(),
            version: files.version.clone(),
            expected_crc: held.checksums,
            identity,
            transfer_sha256: files.transfer_sha256.clone(),
            source: files.source.clone(),
            asset_host: request.asset_host.clone(),
            asset_port: request.asset_port,
            fetched: held.refresh.fetched.clone(),
            reused: held.refresh.reused.clone(),
            store_dir,
            persist_dir: retained.persist_dir,
            map_archive,
        });
        Ok((Stage::Done, RuntimeCacheStep::Ready(prepared)))
    }
}

impl Held {
    fn stop_worker(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker.stop();
        }
    }

    /// The one-call preparation's combined local-store and fill message.
    fn fill_error(&self, error: super::UnpackError) -> RuntimeCacheError {
        RuntimeCacheError::Other(format!(
            "local store: {}; update-server fill: {error}",
            self.local_error
        ))
    }
}

fn is_negotiation_key(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Register this preparation as an owner of the namespace `key`. With a
/// deadline, `None` means another preparation kept the retention lock
/// past it.
fn register_retention_owner(
    revision_root: &Path,
    key: &str,
    until: Option<Instant>,
) -> Result<Option<RetentionLease>, RuntimeCacheError> {
    let Some(_lock) = retention_lock(revision_root, until)? else {
        return Ok(None);
    };
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
            Ok(_) => return Ok(Some(RetentionLease(path))),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("register retained snapshot owner: {error}").into()),
        }
    }
}

// Three recent namespaces cover switching servers plus one server update without
// allowing abandoned snapshots to accumulate indefinitely. Owner leases may
// temporarily exceed this bound.
const RETAINED_COPY_LIMIT: usize = 3;

/// The revision's `.retention.lock`, waited for until `until` (`None`:
/// as long as it takes). `Ok(None)`: another holder kept it past `until`.
fn retention_lock(
    revision_root: &Path,
    until: Option<Instant>,
) -> Result<Option<super::PreparationLock>, super::UnpackError> {
    let path = revision_root.join(".retention.lock");
    match until {
        None => super::PreparationLock::acquire(&path).map(Some),
        Some(until) => super::PreparationLock::acquire_until(&path, until),
    }
}

/// Keep the most recently used namespaces and any namespace that a live or
/// unknown process may still read/write. Persistent preparation-lock inodes are
/// separate and never removed. The old unnamespaced layout is not visited.
/// False when another holder kept the retention lock past `until`; an
/// unusable lock skips pruning.
fn prune_retained_copies(revision_root: &Path, newest: &str, until: Option<Instant>) -> bool {
    let lock = match retention_lock(revision_root, until) {
        Ok(Some(lock)) => lock,
        Ok(None) => return false,
        Err(_) => return true,
    };
    prune_retained_copies_with_liveness(
        revision_root,
        newest,
        std::process::id(),
        process_is_alive,
    );
    drop(lock);
    true
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
