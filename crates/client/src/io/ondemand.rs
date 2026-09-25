//! Port of `~/experiments/Server/webclient/src/io/OnDemand.ts` plus the Java
//! `OnDemand.java` worker thread. The TS Worker boundary becomes one OS
//! thread (the spec's "OnDemand thread"): the client-side handle owns the
//! version tables and the TS `requests`/`completed` bookkeeping; the worker
//! owns the engine ondemand socket pump — a second connection to the game
//! port, Java `Client.portOff + 43594` — and posts completed files back.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use crate::dash3d::model::ModelProvider;
use crate::datastruct::{Arena, LinkList, LinkList2, LinkableTrait, Links};
use crate::io::client_stream::ClientStream;
use crate::io::jagfile::JagFile;
use crate::io::packet::Packet;
use crate::io::ClientRevision;
use crate::BotTarget;

/// Reconnect gate in Java `OnDemand.send`: the socket is not reopened within
/// 4 s of the last open. Spawn starts past the gate (first send is not
/// gated) and `DropSocket` resets it so a relogin reconnects immediately.
/// After a dead socket the 4 s gate is skipped only for the first reconnect
/// following a network completion; repeated recoveries without progress back
/// off exponentially up to this bound.
const SOCKET_OPEN_GATE: Duration = Duration::from_millis(4000);
const RECONNECT_BACKOFF_START: Duration = Duration::from_millis(20);

/// One requested file, Java `OnDemandRequest` / TS `OnDemandRequest`. Implements
/// `LinkableTrait` so requests sit on the TS `LinkList2` and completed files on
/// the `LinkList`, using the two independent link chains as in TS.
pub struct OnDemandRequest {
    pub archive: i32,
    pub file: i32,
    pub data: Option<Vec<u8>>,
    pub cycle: i32,
    pub urgent: bool,
    links: Links,
}

impl OnDemandRequest {
    fn new(archive: i32, file: i32) -> Self {
        OnDemandRequest {
            archive,
            file,
            data: None,
            cycle: 0,
            urgent: true,
            links: Links::new(0),
        }
    }
}

impl LinkableTrait for OnDemandRequest {
    fn links(&self) -> &Links {
        &self.links
    }

    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }

    fn sentinel() -> Self {
        OnDemandRequest::new(0, 0)
    }
}

/// TS `OnDemandProvider` (`requestModel`).
pub trait OnDemandProvider {
    fn request_model(&mut self, id: i32);
}

/// `'static` bridge from the process-wide `Model` store to this OnDemand's
/// worker (`Model.init`'s provider hook). It owns a clone of the command
/// sender, so it can outlive the `Client` that created it; the worker is
/// shared state, not owned here. Archive-0 requests mirror
/// `OnDemand::request(0, id)` (without the request-list dedupe — the
/// engine tolerates repeats, and `request_download` retries until the
/// model unpacks).
pub(crate) struct ModelProviderHandle {
    tx: mpsc::Sender<WorkerCommand>,
}

impl ModelProvider for ModelProviderHandle {
    fn request_model(&mut self, id: i32) {
        let _ = self.tx.send(WorkerCommand::Request {
            archive: 0,
            file: id,
        });
    }
}

/// Client → worker commands (TS `postMessage` inbound messages).
enum WorkerCommand {
    Request {
        archive: i32,
        file: i32,
    },
    PrefetchPriority {
        archive: i32,
        file: i32,
        priority: i32,
    },
    Prefetch {
        archive: i32,
        file: i32,
    },
    ClearPrefetches,
    /// Logout: the engine dropped the update connection, so drop the
    /// worker's stream and reset the reconnect state. Unlike `Stop`, the
    /// worker and the version tables stay alive for the next login.
    DropSocket,
    Stop,
}

/// Worker → client messages (TS `onmessage` outbound messages).
#[derive(Clone)]
enum WorkerMessage {
    Completed {
        archive: i32,
        file: i32,
        urgent: bool,
        data: Option<Arc<Vec<u8>>>,
    },
    Message(String),
    FailCount(i32),
}

/// One OnDemand OS thread + byte-15 socket per `(host, port)`. Fifty
/// `Client`s subscribe; they do not each open an update connection.
struct OnDemandHub {
    cmd: mpsc::Sender<WorkerCommand>,
    subs: Arc<Mutex<HashMap<u64, mpsc::Sender<WorkerMessage>>>>,
    clients: AtomicUsize,
    ingame_n: AtomicUsize,
    running: Arc<AtomicBool>,
    ingame: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
    identity: Option<HubIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HubIdentity {
    target: BotTarget,
    revision: ClientRevision,
    cache_dir: String,
    content_id: String,
    file_store_dir: Option<String>,
    persist_dir: Option<String>,
    tables_sha256: [u8; 32],
}

struct BoundHubIdentity<'a> {
    target: BotTarget,
    revision: ClientRevision,
    cache_dir: &'a str,
    content_id: &'a str,
    file_store_dir: Option<&'a str>,
    persist_dir: Option<&'a str>,
}

static NEXT_SLOT: AtomicUsize = AtomicUsize::new(1);

type HubMap = HashMap<(String, u16), Arc<OnDemandHub>>;
type HubGuard = std::sync::MutexGuard<'static, Option<HubMap>>;
type WorkerEnds = (
    mpsc::Sender<WorkerCommand>,
    mpsc::Receiver<WorkerMessage>,
    Arc<AtomicBool>,
    Arc<AtomicBool>,
    u64,
);

static HUBS: Mutex<Option<HubMap>> = Mutex::new(None);

fn hubs() -> HubGuard {
    HUBS.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct OnDemand {
    /// `versions[archive][file]` from the versionlist jag (Java `versions`).
    versions: Vec<Vec<i32>>,
    /// `crcs[archive][file]` (Java `crcs`); mirrored for the TS struct shape,
    /// the worker owns the authoritative copy it validates against.
    #[allow(dead_code)]
    crcs: Vec<Vec<i32>>,
    /// `modelUse` — per-model priority flags.
    model_use: Vec<i32>,
    /// `mapIndex`/`mapLand`/`mapLoc`/`mapFree` — per-square map files.
    map_index: Vec<i32>,
    map_land: Vec<i32>,
    map_loc: Vec<i32>,
    map_free: Vec<i32>,
    /// `animFrameIndex`.
    anim_frame_index: Vec<i32>,
    /// `midiJingle` — per-midi jingle flag.
    midi_jingle: Vec<i32>,
    /// False for `new_unconnected`: without a versionlist there is nothing to
    /// validate against, so `request` accepts any file (test constructor only).
    has_tables: bool,

    /// TS `OnDemand.message`, updated from worker posts.
    pub message: String,
    /// TS `OnDemand.failCount`.
    pub fail_count: i32,
    /// TS `OnDemand.cycle`; bumped once per `run()`.
    pub cycle: i32,
    running: bool,

    /// TS `requests` LinkList2 over an arena this OnDemand owns.
    arena: Arena<OnDemandRequest>,
    requests: LinkList2<OnDemandRequest>,
    /// TS `completed` LinkList: finished files awaiting `loop_request`.
    completed: LinkList<OnDemandRequest>,

    /// Worker command channel; `None` for `new_unconnected` (no worker).
    tx: Option<mpsc::Sender<WorkerCommand>>,
    rx: mpsc::Receiver<WorkerMessage>,
    worker: Option<JoinHandle<()>>,
    /// Shared `app.ingame` snapshot for the worker's priority byte and the
    /// no-timeout keepalive (TS `setIngame`).
    ingame: Arc<AtomicBool>,
    worker_running: Arc<AtomicBool>,
    /// Hub this handle is subscribed to (`None` for `new_unconnected`).
    hub_key: Option<(String, u16)>,
    slot_id: Option<u64>,
    /// Files this handle `request`ed or prefetched; drain ignores Completeds
    /// for files a sibling asked for (archive-93 map prefetches included).
    want: HashSet<(i32, i32)>,
    reported_ingame: bool,
}

/// The Java `OnDemand.run` loop + socket pump, one OS thread per OnDemand.
/// Owns the `queue`/`missing`/`pending`/`prefetches` lists and the
/// `ClientStream` to the engine ondemand socket.
struct Worker {
    commands: mpsc::Receiver<WorkerCommand>,
    versions: Vec<Vec<i32>>,
    crcs: Vec<Vec<i32>>,
    priorities: Vec<Vec<i32>>,
    top_priority: i32,
    queue: VecDeque<OnDemandRequest>,
    missing: VecDeque<OnDemandRequest>,
    pending: Vec<OnDemandRequest>,
    prefetches: VecDeque<OnDemandRequest>,
    message: String,
    fail_count: i32,
    urgent_count: i32,
    request_count: i32,
    loaded_prefetch_files: i32,
    total_prefetch_files: i32,
    buf: [u8; 500],
    part_offset: i32,
    part_available: i32,
    packet_cycle: i32,
    no_timeout_cycle: i32,
    active: bool,
    socket_open_time: Instant,
    current: Option<usize>,
    stream: Option<ClientStream>,
    host: String,
    port: u16,
    target: Option<BotTarget>,
    revision: Option<ClientRevision>,
    /// Some when the `main_file_cache` file store is present (Java
    /// `app.fileStreams[0] != null`).
    cache_dir: Option<String>,
    /// Content-identity overlay for completed ondemand files (gzip + trailer).
    persist_dir: Option<PathBuf>,
    recovering: bool,
    reconnect_backoff: Duration,
    saw_network_progress: bool,
    subs: Arc<Mutex<HashMap<u64, mpsc::Sender<WorkerMessage>>>>,
    running: Arc<AtomicBool>,
    ingame: Arc<AtomicBool>,
}

impl OnDemand {
    /// Test constructor: no versionlist, no worker, no socket. `request`
    /// still queues into `requests` so `remaining()` behaves like the live
    /// object (the brief's unit-test path).
    pub fn new_unconnected() -> Self {
        let mut arena = Arena::new();
        let requests = LinkList2::new(&mut arena);
        let (_tx, rx) = mpsc::channel();
        OnDemand {
            versions: Vec::new(),
            crcs: Vec::new(),
            model_use: Vec::new(),
            map_index: Vec::new(),
            map_land: Vec::new(),
            map_loc: Vec::new(),
            map_free: Vec::new(),
            anim_frame_index: Vec::new(),
            midi_jingle: Vec::new(),
            has_tables: false,
            message: String::new(),
            fail_count: 0,
            cycle: 0,
            running: true,
            arena,
            requests,
            completed: LinkList::new(),
            tx: None,
            rx,
            worker: None,
            ingame: Arc::new(AtomicBool::new(false)),
            worker_running: Arc::new(AtomicBool::new(false)),
            hub_key: None,
            slot_id: None,
            want: HashSet::new(),
            reported_ingame: false,
        }
    }

    /// How many live hub workers exist for `(host, port)` (0 or 1). Tests
    /// prove fifty slots share one update socket.
    pub fn live_workers_for(host: &str, port: u16) -> usize {
        let map = hubs();
        match map.as_ref() {
            Some(m) if m.contains_key(&(host.to_string(), port)) => 1,
            _ => 0,
        }
    }

    /// Worker keepalive flag: true if any subscribed handle last `run` with
    /// `ingame == true`.
    pub fn hub_ingame_for(host: &str, port: u16) -> bool {
        let map = hubs();
        map.as_ref()
            .and_then(|m| m.get(&(host.to_string(), port)))
            .is_some_and(|h| h.ingame.load(Ordering::Relaxed))
    }

    /// Parse the version/crc/index tables from the versionlist jag and spawn
    /// the worker thread (TS `new OnDemand(versionlist, app)` + Java `init`).
    /// Returns `None` when the versionlist lacks one of the four version or
    /// crc tables (TS throws on those). Unbound handles default to revision
    /// 274 so the Java keepalive still fires.
    pub fn new(
        versionlist: &JagFile,
        host: &str,
        port: u16,
        cache_dir: &str,
        ingame: Arc<AtomicBool>,
    ) -> Option<Self> {
        Self::new_with_revision(
            versionlist,
            host,
            port,
            cache_dir,
            ingame,
            ClientRevision::R274,
        )
    }

    /// Unbound constructor with an explicit protocol revision. Revision 289
    /// must not send the Java keepalive (`00 00 00 0a`); 274 still does.
    pub fn new_with_revision(
        versionlist: &JagFile,
        host: &str,
        port: u16,
        cache_dir: &str,
        _ingame: Arc<AtomicBool>,
        revision: ClientRevision,
    ) -> Option<Self> {
        Self::new_inner(versionlist, host, port, cache_dir, None, Some(revision))
            .ok()
            .flatten()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_bound(
        versionlist: &JagFile,
        target: BotTarget,
        revision: ClientRevision,
        host: &str,
        port: u16,
        cache_dir: &str,
        content_id: &str,
        file_store_dir: Option<&str>,
        persist_dir: Option<&str>,
    ) -> Result<Self, String> {
        Self::new_inner(
            versionlist,
            host,
            port,
            cache_dir,
            Some(BoundHubIdentity {
                target,
                revision,
                cache_dir,
                content_id,
                file_store_dir,
                persist_dir,
            }),
            None,
        )?
        .ok_or_else(|| "bound OnDemand versionlist is missing required tables".to_string())
    }

    fn new_inner(
        versionlist: &JagFile,
        host: &str,
        port: u16,
        cache_dir: &str,
        bound: Option<BoundHubIdentity<'_>>,
        unbound_revision: Option<ClientRevision>,
    ) -> Result<Option<Self>, String> {
        let Some(versions) = read_table(
            versionlist,
            &[
                "model_version",
                "anim_version",
                "midi_version",
                "map_version",
            ],
            2,
            |buf| buf.g2(),
        ) else {
            return Ok(None);
        };
        let Some(crcs) = read_table(
            versionlist,
            &["model_crc", "anim_crc", "midi_crc", "map_crc"],
            4,
            |buf| buf.g4(),
        ) else {
            return Ok(None);
        };

        // `modelUse` is sized by the model version count, padded with 0
        // (TS fills `versions[0].length` entries from the raw bytes).
        let model_use = match versionlist.read("model_index") {
            Some(data) => (0..versions[0].len())
                .map(|i| data.get(i).copied().unwrap_or(0) as i32)
                .collect(),
            None => Vec::new(),
        };

        // TS reads map/anim/midi indexes as g2/g2/g2/g1, g2, and g1 streams;
        // a missing entry leaves the arrays empty rather than throwing.
        let (map_index, map_land, map_loc, map_free) = match versionlist.read("map_index") {
            Some(data) => {
                let count = data.len() / 7;
                let mut buf = Packet::new(data);
                let mut index = Vec::with_capacity(count);
                let mut land = Vec::with_capacity(count);
                let mut loc = Vec::with_capacity(count);
                let mut free = Vec::with_capacity(count);
                for _ in 0..count {
                    index.push(buf.g2());
                    land.push(buf.g2());
                    loc.push(buf.g2());
                    free.push(buf.g1());
                }
                (index, land, loc, free)
            }
            None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        };
        let anim_frame_index = read_raw_table(versionlist, "anim_index", 2, |buf| buf.g2());
        let midi_jingle = read_raw_table(versionlist, "midi_index", 1, |buf| buf.g1());

        let identity = bound.as_ref().map(|identity| HubIdentity {
            target: identity.target,
            revision: identity.revision,
            cache_dir: identity.cache_dir.to_string(),
            content_id: identity.content_id.to_string(),
            file_store_dir: identity.file_store_dir.map(str::to_string),
            persist_dir: identity.persist_dir.map(str::to_string),
            tables_sha256: tables_sha256(&versions, &crcs),
        });
        let store_hint = bound
            .as_ref()
            .and_then(|identity| identity.file_store_dir)
            .unwrap_or(cache_dir);
        let revision = bound
            .as_ref()
            .map(|identity| identity.revision)
            .or(unbound_revision);
        let (cmd, message_rx, worker_running, hub_ingame, slot_id) =
            subscribe_hub(host, port, store_hint, &versions, &crcs, identity, revision)?;

        let mut arena = Arena::new();
        let requests = LinkList2::new(&mut arena);
        Ok(Some(OnDemand {
            versions,
            crcs,
            model_use,
            map_index,
            map_land,
            map_loc,
            map_free,
            anim_frame_index,
            midi_jingle,
            has_tables: true,
            message: String::new(),
            fail_count: 0,
            cycle: 0,
            running: true,
            arena,
            requests,
            completed: LinkList::new(),
            tx: Some(cmd),
            rx: message_rx,
            worker: None,
            ingame: hub_ingame,
            worker_running,
            hub_key: Some((host.to_string(), port)),
            slot_id: Some(slot_id),
            want: HashSet::new(),
            reported_ingame: false,
        }))
    }

    /// TS `OnDemand.stop()`: detach this handle. The hub worker stays up
    /// while other slots are subscribed; the last detach Stops it.
    pub fn stop(&mut self) {
        self.running = false;
        self.detach_hub();
        if self.worker.is_some() {
            self.worker_running.store(false, Ordering::Relaxed);
            if let Some(tx) = &self.tx {
                let _ = tx.send(WorkerCommand::Stop);
            }
            if let Some(handle) = self.worker.take() {
                let _ = handle.join();
            }
        }
    }

    fn detach_hub(&mut self) {
        let Some(key) = self.hub_key.take() else {
            return;
        };
        let slot_id = self.slot_id.take();
        let clear_ingame = self.reported_ingame;
        self.reported_ingame = false;
        let join_handle;
        {
            let mut guard = hubs();
            let map = guard.get_or_insert_with(HashMap::new);
            let Some(hub) = map.get(&key).cloned() else {
                return;
            };
            if let Some(id) = slot_id {
                hub.subs
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&id);
            }
            if clear_ingame {
                let prev = hub.ingame_n.fetch_sub(1, Ordering::Relaxed);
                if prev == 1 {
                    hub.ingame.store(false, Ordering::Relaxed);
                }
            }
            let prev = hub.clients.fetch_sub(1, Ordering::Relaxed);
            if prev == 1 {
                hub.running.store(false, Ordering::Relaxed);
                let _ = hub.cmd.send(WorkerCommand::Stop);
                join_handle = hub.handle.lock().unwrap_or_else(|p| p.into_inner()).take();
                map.remove(&key);
            } else {
                join_handle = None;
            }
        }
        if let Some(handle) = join_handle {
            let _ = handle.join();
        }
    }

    /// `logout` path: only the **last** slot may drop the process update
    /// socket. A sibling still in game must keep the byte-15 connection.
    pub fn drop_socket(&self) {
        if let Some(key) = &self.hub_key {
            let guard = hubs();
            if let Some(hub) = guard.as_ref().and_then(|m| m.get(key)) {
                if hub.clients.load(Ordering::Relaxed) > 1 {
                    return;
                }
            }
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(WorkerCommand::DropSocket);
        }
    }

    /// `getFileCount(archive)`.
    pub fn get_file_count(&self, archive: i32) -> i32 {
        self.versions[archive as usize].len() as i32
    }

    /// A `'static` handle for `Model.init`'s provider hook (`None` for
    /// `new_unconnected`, which has no worker to send to).
    pub fn model_provider(&self) -> Option<Box<dyn ModelProvider + Send>> {
        self.tx.as_ref().map(|tx| {
            Box::new(ModelProviderHandle { tx: tx.clone() }) as Box<dyn ModelProvider + Send>
        })
    }

    /// `getAnimFrameCount()`.
    pub fn get_anim_frame_count(&self) -> i32 {
        self.anim_frame_index.len() as i32
    }

    /// `getMapFile(x, z, type)`: the land (type 0) or location (type 1) file
    /// id for the square, or -1 when the map index has no entry.
    pub fn get_map_file(&self, x: i32, z: i32, ty: i32) -> i32 {
        let map = (x << 8) + z;
        for i in 0..self.map_index.len() {
            if self.map_index[i] == map {
                return if ty == 0 {
                    self.map_land[i]
                } else {
                    self.map_loc[i]
                };
            }
        }
        -1
    }

    /// `prefetchMaps(members)`.
    pub fn prefetch_maps(&mut self, members: bool) {
        for i in 0..self.map_index.len() {
            if members || self.map_free[i] != 0 {
                self.prefetch_priority(3, self.map_loc[i], 2);
                self.prefetch_priority(3, self.map_land[i], 2);
            }
        }
    }

    /// `hasMapLocFile(file)`.
    pub fn has_map_loc_file(&self, file: i32) -> bool {
        self.map_loc.contains(&file)
    }

    /// `getModelUse(id)`.
    pub fn get_model_use(&self, id: i32) -> i32 {
        self.model_use[id as usize] & 0xFF
    }

    /// Java `Client.maininit` (5251-5277) `getModelUse` bits to a prefetch
    /// priority: the first matching bit in the 8/0x20/0x10/0x40/0x80/2/4
    /// ladder wins, then `& 1` overrides everything with 3.
    pub fn model_use_priority(use_bits: i32) -> i32 {
        let priority = if use_bits & 0x8 != 0 {
            10
        } else if use_bits & 0x20 != 0 {
            9
        } else if use_bits & 0x10 != 0 {
            8
        } else if use_bits & 0x40 != 0 {
            7
        } else if use_bits & 0x80 != 0 {
            6
        } else if use_bits & 0x2 != 0 {
            5
        } else if use_bits & 0x4 != 0 {
            4
        } else {
            0
        };
        if use_bits & 0x1 != 0 {
            3
        } else {
            priority
        }
    }

    /// `isMidiJingle(id)`.
    pub fn is_midi_jingle(&self, id: i32) -> bool {
        self.midi_jingle.get(id as usize).copied() == Some(1)
    }

    /// Java `Client.maininit` 5206-5210: urgent `request` of models whose
    /// `getModelUse & 1` bit is set. The red loading bar waits
    /// `remaining()==0` on this set only.
    pub fn request_in_use_models(&mut self) {
        let n = self.get_file_count(0);
        for i in 0..n {
            if self.get_model_use(i) & 1 != 0 {
                self.request(0, i);
            }
        }
    }

    /// Request **every** model (archive 0), not just the `model_use & 1`
    /// "in-use" subset. The bot host unpacks the whole 2004 cache on boot,
    /// so any model a random event (the Maze), an on-demand map, or a
    /// script later needs is already fetched — `Model::load` never misses
    /// because a loc was placed before its model arrived.
    pub fn request_all_models(&mut self) {
        let n = self.get_file_count(0);
        for i in 0..n {
            self.request(0, i);
        }
    }

    /// Unpack every model present in the local file store (idx1) directly —
    /// no OnDemand/server round-trip. The bot host's cache is complete, so a
    /// random-event model (the Maze walls) is available at boot and
    /// `Model::load` never misses because a loc was placed before its model
    /// arrived. Returns the number of models unpacked.
    pub fn unpack_models_from_cache(&self, cache_dir: &str) -> usize {
        let n = self.get_file_count(0);
        let mut count = 0;
        for i in 0..n {
            if let Some(data) = cache_read(cache_dir, 1, i) {
                // The store keeps gzip + a 2-byte version trailer, exactly
                // what `loop_request` strips before handing to `Model::unpack`.
                let body = if data.len() >= 2 {
                    &data[..data.len() - 2]
                } else {
                    &data
                };
                let raw = gunzip(body);
                crate::dash3d::Model::unpack(i, Some(raw.as_slice()));
                count += 1;
            }
        }
        count
    }

    /// Java `Client.maininit` 5251-5285: `prefetchPriority` the rest of the
    /// models, then maps, then midi jingles. These are not in `remaining()`
    /// — OnDemand downloads them after title, and `onDemand.message`
    /// becomes `"Loading extra files - x%"` under the login buttons.
    pub fn prefetch_extra_files(&mut self, members: bool, lowmem: bool) {
        let n = self.get_file_count(0);
        for i in 0..n {
            let priority = Self::model_use_priority(self.get_model_use(i));
            if priority != 0 {
                self.prefetch_priority(0, i, priority);
            }
        }
        self.prefetch_maps(members);
        if !lowmem {
            let midi = self.get_file_count(2);
            for i in 1..midi {
                if self.is_midi_jingle(i) {
                    self.prefetch_priority(2, i, 1);
                }
            }
        }
    }

    /// `prefetchPriority(archive, file, priority)` — forwarded to the worker,
    /// which validates against the local cache before raising the priority
    /// (Java does the same read on its thread).
    pub fn prefetch_priority(&mut self, archive: i32, file: i32, priority: i32) {
        self.want_file(archive, file);
        if let Some(tx) = &self.tx {
            let _ = tx.send(WorkerCommand::PrefetchPriority {
                archive,
                file,
                priority,
            });
        }
    }

    /// `clearPrefetches()`.
    pub fn clear_prefetches(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(WorkerCommand::ClearPrefetches);
        }
    }

    /// `prefetch(archive, file)`.
    pub fn prefetch(&mut self, archive: i32, file: i32) {
        self.want_file(archive, file);
        if let Some(tx) = &self.tx {
            let _ = tx.send(WorkerCommand::Prefetch { archive, file });
        }
    }

    /// `request(archive, file)`: guard against invalid files, dedupe against
    /// the `requests` list, queue the request, and tell the worker. Without a
    /// versionlist (`new_unconnected`) the guard is skipped.
    pub fn request(&mut self, archive: i32, file: i32) {
        if self.has_tables && !self.valid_file(archive, file) {
            return;
        }
        if self.find_request_id(archive, file).is_some() {
            return;
        }
        self.want_file(archive, file);
        let id = self.arena.alloc(OnDemandRequest::new(archive, file));
        self.requests.push(&mut self.arena, id);
        if let Some(tx) = &self.tx {
            let _ = tx.send(WorkerCommand::Request { archive, file });
        }
    }

    /// `remaining()`: outstanding requests, counted on the `requests` list as
    /// Java/TS do (includes in-flight requests until `loop_request` pops).
    pub fn remaining(&self) -> usize {
        self.requests.size(&self.arena)
    }

    /// `loop()`: pop the next completed request, gunzip its payload (the
    /// engine sends gzip + a 2-byte version trailer; TS strips the trailer
    /// before gunzipping), and unlink the request from `requests`.
    pub fn loop_request(&mut self) -> Option<OnDemandRequest> {
        let mut req = self.pop_completed_raw()?;
        if let Some(data) = req.data.take() {
            let body = if data.len() >= 2 {
                &data[..data.len() - 2]
            } else {
                &data
            };
            req.data = Some(gunzip(body));
        }
        Some(req)
    }

    /// Pop one completed request with its **raw** payload (gzip + the 2-byte
    /// version trailer, exactly as the wire/store carry it) and unlink it from
    /// `requests`. Cold cache preparation needs the raw bytes to run
    /// `validate` against the versionlist crc/version tables before gunzipping.
    pub(crate) fn pop_completed_raw(&mut self) -> Option<OnDemandRequest> {
        let req = self.completed.pop_front()?;
        if let Some(id) = self.find_request_id(req.archive, req.file) {
            self.arena.unlink2(id);
            self.arena.take(id);
        }
        Some(req)
    }

    /// Version/CRC table entries for `(archive, file)`, or `None` when the
    /// tables do not cover the file. `OnDemand.validate` arguments.
    pub(crate) fn version_crc(&self, archive: i32, file: i32) -> Option<(i32, i32)> {
        if archive < 0 || file < 0 {
            return None;
        }
        let version = *self.versions.get(archive as usize)?.get(file as usize)?;
        let crc = *self.crcs.get(archive as usize)?.get(file as usize)?;
        Some((version, crc))
    }

    /// `OnDemand.run()` heartbeat: sync the shared ingame flag, bump `cycle`,
    /// and pull worker messages into `completed`/`message`/`fail_count`.
    pub fn run(&mut self, ingame: bool) {
        if !self.running {
            return;
        }
        self.set_reported_ingame(ingame);
        self.cycle += 1;
        self.drain_worker();
    }

    fn want_file(&mut self, archive: i32, file: i32) {
        self.want.insert((archive, file));
        // Worker remaps non-urgent archive-3 completions to 93.
        if archive == 3 {
            self.want.insert((93, file));
        }
    }

    fn set_reported_ingame(&mut self, ingame: bool) {
        if self.hub_key.is_none() {
            self.ingame.store(ingame, Ordering::Relaxed);
            return;
        }
        if ingame == self.reported_ingame {
            return;
        }
        self.reported_ingame = ingame;
        let Some(key) = &self.hub_key else { return };
        let guard = hubs();
        let Some(hub) = guard.as_ref().and_then(|m| m.get(key)) else {
            return;
        };
        if ingame {
            hub.ingame_n.fetch_add(1, Ordering::Relaxed);
            hub.ingame.store(true, Ordering::Relaxed);
        } else {
            let prev = hub.ingame_n.fetch_sub(1, Ordering::Relaxed);
            if prev == 1 {
                hub.ingame.store(false, Ordering::Relaxed);
            }
        }
    }

    fn valid_file(&self, archive: i32, file: i32) -> bool {
        archive >= 0
            && (archive as usize) < self.versions.len()
            && file >= 0
            && (file as usize) < self.versions[archive as usize].len()
            && self.versions[archive as usize][file as usize] != 0
    }

    fn find_request_id(&mut self, archive: i32, file: i32) -> Option<usize> {
        let mut id = self.requests.head(&self.arena);
        while let Some(node_id) = id {
            let node = self.arena.get(node_id);
            if node.archive == archive && node.file == file {
                return Some(node_id);
            }
            id = self.requests.next(&self.arena);
        }
        None
    }

    fn drain_worker(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                WorkerMessage::Completed {
                    archive,
                    file,
                    urgent,
                    data,
                } => {
                    // `want` is the per-handle request/prefetch set. Archive 0/1
                    // Completeds also arrive from `Model::request_download` via
                    // the process provider, which never touches `want` — drop
                    // those and `check_scene` stays on scene 1 (-3, loc models).
                    let keep = archive == 0 || archive == 1 || self.want.remove(&(archive, file));
                    if !keep {
                        continue;
                    }
                    let mut req = OnDemandRequest::new(archive, file);
                    req.urgent = urgent;
                    req.data = data.map(|d| (*d).clone());
                    self.completed.push(req);
                }
                WorkerMessage::Message(m) => self.message = m,
                WorkerMessage::FailCount(n) => self.fail_count = n,
            }
        }
    }
}

impl OnDemandProvider for OnDemand {
    /// `requestModel(id)`.
    fn request_model(&mut self, id: i32) {
        self.request(0, id);
    }
}

impl Drop for OnDemand {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn subscribe_hub(
    host: &str,
    port: u16,
    cache_dir: &str,
    versions: &[Vec<i32>],
    crcs: &[Vec<i32>],
    identity: Option<HubIdentity>,
    revision: Option<ClientRevision>,
) -> Result<WorkerEnds, String> {
    let key = (host.to_string(), port);
    let slot_id = NEXT_SLOT.fetch_add(1, Ordering::Relaxed) as u64;
    let (msg_tx, msg_rx) = mpsc::channel();
    let mut guard = hubs();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(hub) = map.get(&key) {
        if hub.identity != identity {
            return Err("OnDemand identity mismatch for occupied endpoint".into());
        }
        hub.clients.fetch_add(1, Ordering::Relaxed);
        hub.subs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(slot_id, msg_tx);
        return Ok((
            hub.cmd.clone(),
            msg_rx,
            hub.running.clone(),
            hub.ingame.clone(),
            slot_id,
        ));
    }
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let mut first = HashMap::new();
    first.insert(slot_id, msg_tx);
    let subs = Arc::new(Mutex::new(first));
    let running = Arc::new(AtomicBool::new(true));
    let ingame = Arc::new(AtomicBool::new(false));
    let worker = Worker {
        commands: cmd_rx,
        versions: versions.to_vec(),
        crcs: crcs.to_vec(),
        priorities: versions.iter().map(|v| vec![0; v.len()]).collect(),
        top_priority: 0,
        queue: VecDeque::new(),
        missing: VecDeque::new(),
        pending: Vec::new(),
        prefetches: VecDeque::new(),
        message: String::new(),
        fail_count: 0,
        urgent_count: 0,
        request_count: 0,
        loaded_prefetch_files: 0,
        total_prefetch_files: 0,
        buf: [0; 500],
        part_offset: 0,
        part_available: 0,
        packet_cycle: 0,
        no_timeout_cycle: 0,
        active: false,
        socket_open_time: Instant::now() - SOCKET_OPEN_GATE,
        current: None,
        stream: None,
        host: host.to_string(),
        port,
        target: identity.as_ref().map(|identity| identity.target),
        revision,
        cache_dir: resolve_file_store(cache_dir),
        persist_dir: identity
            .as_ref()
            .and_then(|identity| identity.persist_dir.as_ref().map(PathBuf::from)),
        recovering: false,
        reconnect_backoff: Duration::ZERO,
        saw_network_progress: false,
        subs: Arc::clone(&subs),
        running: Arc::clone(&running),
        ingame: Arc::clone(&ingame),
    };
    let handle = thread::Builder::new()
        .name("ondemand".into())
        .stack_size(1024 * 1024)
        .spawn(move || worker_main(worker))
        .map_err(|error| format!("failed to spawn OnDemand worker: {error}"))?;
    let hub = Arc::new(OnDemandHub {
        cmd: cmd_tx.clone(),
        subs,
        clients: AtomicUsize::new(1),
        ingame_n: AtomicUsize::new(0),
        running: Arc::clone(&running),
        ingame: Arc::clone(&ingame),
        handle: Mutex::new(Some(handle)),
        identity,
    });
    map.insert(key, hub);
    Ok((cmd_tx, msg_rx, running, ingame, slot_id))
}

fn tables_sha256(versions: &[Vec<i32>], crcs: &[Vec<i32>]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for tables in [versions, crcs] {
        digest.update((tables.len() as u64).to_be_bytes());
        for table in tables {
            digest.update((table.len() as u64).to_be_bytes());
            for value in table {
                digest.update(value.to_be_bytes());
            }
        }
    }
    digest.finalize().into()
}

/// Worker thread body: Java `OnDemand.run` with the command channel drained
/// each pass (TS message handling). Sleeps 20 ms (50 ms when only prefetches
/// remain and a local cache exists), then one pump cycle. Queued demand is
/// pumped immediately so a matching local map is not delayed by the idle
/// prefetch interval.
fn worker_main(mut worker: Worker) {
    while worker.running.load(Ordering::Relaxed) {
        worker.drain_commands();
        let delay = if !worker.queue.is_empty() {
            0
        } else if worker.top_priority == 0 && worker.cache_dir.is_some() {
            50
        } else {
            20
        };
        if delay > 0 {
            thread::sleep(Duration::from_millis(delay));
        }
        worker.tick();
    }
}

impl Worker {
    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.try_recv() {
            match cmd {
                WorkerCommand::Request { archive, file } => {
                    if !self.valid_file(archive, file) {
                        continue;
                    }
                    if self.promote_urgent(archive, file) {
                        continue;
                    }
                    self.queue.push_back(OnDemandRequest::new(archive, file));
                }
                WorkerCommand::PrefetchPriority {
                    archive,
                    file,
                    priority,
                } => {
                    self.prefetch_priority(archive, file, priority);
                }
                WorkerCommand::Prefetch { archive, file } => {
                    self.prefetch(archive, file);
                }
                WorkerCommand::ClearPrefetches => self.prefetches.clear(),
                WorkerCommand::DropSocket => {
                    if let Some(mut stream) = self.stream.take() {
                        stream.close();
                    }
                    self.packet_cycle = 0;
                    self.part_available = 0;
                    self.current = None;
                    self.reconnect_backoff = Duration::ZERO;
                    self.saw_network_progress = false;
                    self.socket_open_time = Instant::now() - SOCKET_OPEN_GATE;
                }
                WorkerCommand::Stop => {
                    self.running.store(false, Ordering::Relaxed);
                }
            }
        }
    }

    /// A later urgent `request` must not be dropped just because a
    /// prefetch of the same file is already queued: `complete` remaps
    /// non-urgent archive-3 to 93, and `check_scene` would wait forever
    /// for the map square.
    fn promote_urgent(&mut self, archive: i32, file: i32) -> bool {
        let bump = |r: &mut OnDemandRequest| {
            if r.archive == archive && r.file == file {
                r.urgent = true;
                true
            } else {
                false
            }
        };
        self.queue.iter_mut().any(bump)
            || self.missing.iter_mut().any(bump)
            || self.pending.iter_mut().any(bump)
            || self.prefetches.iter_mut().any(bump)
    }

    fn emit(&self, msg: WorkerMessage) {
        let mut subs = self.subs.lock().unwrap_or_else(|p| p.into_inner());
        subs.retain(|_, s| s.send(msg.clone()).is_ok());
    }

    fn valid_file(&self, archive: i32, file: i32) -> bool {
        archive >= 0
            && (archive as usize) < self.versions.len()
            && file >= 0
            && (file as usize) < self.versions[archive as usize].len()
            && self.versions[archive as usize][file as usize] != 0
    }

    /// Java `prefetchPriority`: only meaningful when a local cache exists;
    /// the file is dropped from the prefetch set when the cache copy already
    /// validates against its crc/version.
    fn prefetch_priority(&mut self, archive: i32, file: i32, priority: i32) {
        if self.cache_dir.is_none() && self.persist_dir.is_none() {
            return;
        }
        if !self.valid_file(archive, file) {
            return;
        }
        if let Some(data) = self.cached_payload(archive, file) {
            // Java skips the extra-files download. This port never writes
            // idx, so Model::unpack only runs on Completed: post archive-0
            // cache hits the same way `complete` posts non-urgent models.
            if archive == 0 {
                let mut req = OnDemandRequest::new(archive, file);
                req.urgent = false;
                req.data = Some(data);
                self.complete(req);
            }
            return;
        }
        self.priorities[archive as usize][file as usize] = priority;
        if priority > self.top_priority {
            self.top_priority = priority;
        }
        self.total_prefetch_files += 1;
    }

    /// Java `prefetch`: only files already given a priority are pushed, and
    /// only when a prefetch pass is active (`topPriority != 0`). Unlike the
    /// Java subscript swap (a 317 bug), the guard indexes by archive/file.
    fn prefetch(&mut self, archive: i32, file: i32) {
        if (self.cache_dir.is_none() && self.persist_dir.is_none())
            || !self.valid_file(archive, file)
        {
            return;
        }
        if self.priorities[archive as usize][file as usize] == 0 || self.top_priority == 0 {
            return;
        }
        let mut req = OnDemandRequest::new(archive, file);
        req.urgent = false;
        self.prefetches.push_back(req);
    }

    /// Java `handleQueue`: serve requests from the local cache when the copy
    /// validates; anything else goes to `missing` and then the network.
    fn handle_queue(&mut self) {
        while let Some(mut req) = self.queue.pop_front() {
            self.active = true;
            if let Some(cached) = self.cached_payload(req.archive, req.file) {
                req.data = Some(cached);
                self.complete(req);
            } else {
                self.missing.push_back(req);
            }
        }
    }

    /// Java `handlePending`: count urgent/non-urgent pending, then promote
    /// `missing` requests into `pending` (up to 10 urgent) and send them.
    fn handle_pending(&mut self) {
        self.urgent_count = 0;
        self.request_count = 0;
        for req in &self.pending {
            if req.urgent {
                self.urgent_count += 1;
            } else {
                self.request_count += 1;
            }
        }
        while self.urgent_count < 10 {
            let Some(req) = self.missing.pop_front() else {
                break;
            };
            if self.priorities[req.archive as usize][req.file as usize] != 0 {
                self.loaded_prefetch_files += 1;
            }
            self.priorities[req.archive as usize][req.file as usize] = 0;
            let (archive, file, urgent) = (req.archive, req.file, req.urgent);
            self.pending.push(req);
            self.urgent_count += 1;
            self.send(archive, file, urgent);
            self.active = true;
        }
    }

    /// Java `handleExtra`: while nothing urgent is pending, serve prefetches
    /// by priority (lowest priority first, `topPriority` stepping down).
    fn handle_extra(&mut self) {
        while self.urgent_count == 0 {
            if self.request_count >= 10 || self.top_priority == 0 {
                return;
            }
            while let Some(extra) = self.prefetches.pop_front() {
                if self.priorities[extra.archive as usize][extra.file as usize] != 0 {
                    self.priorities[extra.archive as usize][extra.file as usize] = 0;
                    let (archive, file, urgent) = (extra.archive, extra.file, extra.urgent);
                    self.pending.push(extra);
                    self.send(archive, file, urgent);
                    self.active = true;
                    self.bump_prefetch_progress();
                    self.request_count += 1;
                    if self.request_count == 10 {
                        return;
                    }
                }
            }
            for archive in 0..4 {
                for file in 0..self.priorities[archive].len() {
                    if self.priorities[archive][file] == self.top_priority {
                        self.priorities[archive][file] = 0;
                        let mut req = OnDemandRequest::new(archive as i32, file as i32);
                        req.urgent = false;
                        self.pending.push(req);
                        self.send(archive as i32, file as i32, false);
                        self.active = true;
                        self.bump_prefetch_progress();
                        self.request_count += 1;
                        if self.request_count == 10 {
                            return;
                        }
                    }
                }
            }
            self.top_priority -= 1;
        }
    }

    fn bump_prefetch_progress(&mut self) {
        if self.loaded_prefetch_files < self.total_prefetch_files {
            self.loaded_prefetch_files += 1;
        }
        self.set_message(format!(
            "Loading extra files - {}%",
            self.loaded_prefetch_files * 100 / self.total_prefetch_files
        ));
    }

    /// Java `read`: consume the 6-byte chunk header and the part payload from
    /// the ondemand socket. Errors close the socket and reset the part state,
    /// exactly like the Java `catch (IOException)`.
    fn read(&mut self) {
        if self.try_read().is_err() {
            self.recover_and_resend();
        }
    }

    fn try_read(&mut self) -> Result<(), ()> {
        let available = {
            let Some(stream) = &mut self.stream else {
                return Ok(());
            };
            stream.available().map_err(|_| ())?
        };

        if self.part_available == 0 && available >= 6 {
            self.active = true;
            let mut hdr = [0u8; 6];
            {
                let Some(stream) = &mut self.stream else {
                    return Ok(());
                };
                stream.read_bytes(&mut hdr, 0, 6).map_err(|_| ())?;
            }
            let archive = hdr[0] as i32;
            let file = ((hdr[1] as i32) << 8) + hdr[2] as i32;
            let size = ((hdr[3] as i32) << 8) + hdr[4] as i32;
            let part = hdr[5] as i32;

            self.current = None;
            let mut i = 0;
            while i < self.pending.len() {
                if self.pending[i].archive == archive && self.pending[i].file == file {
                    self.current = Some(i);
                }
                if self.current.is_some() {
                    self.pending[i].cycle = 0;
                }
                i += 1;
            }

            if let Some(ci) = self.current {
                self.packet_cycle = 0;
                if size == 0 {
                    // "Rej": the engine has no such file; urgent requests are
                    // completed with null data, others are dropped (Java).
                    let req = self.pending.remove(ci);
                    self.current = None;
                    if req.urgent {
                        self.emit(WorkerMessage::Completed {
                            archive: req.archive,
                            file: req.file,
                            urgent: true,
                            data: None,
                        });
                    }
                } else {
                    if self.pending[ci].data.is_none() && part == 0 {
                        self.pending[ci].data = Some(vec![0u8; size as usize]);
                    }
                    if self.pending[ci].data.is_none() && part != 0 {
                        // Java throws "missing start of file" here; the catch
                        // in read() resets the socket.
                        return Err(());
                    }
                }
            }

            self.part_offset = part * 500;
            self.part_available = 500;
            if self.part_available > size - part * 500 {
                self.part_available = size - part * 500;
            }
        }

        if self.part_available > 0 && available >= self.part_available {
            self.active = true;
            let (data_len, into_buf) = match self.current {
                Some(ci) => (self.pending[ci].data.as_ref().map_or(0, |d| d.len()), false),
                None => (self.buf.len(), true),
            };
            {
                let Some(stream) = &mut self.stream else {
                    return Ok(());
                };
                if into_buf {
                    stream
                        .read_bytes(&mut self.buf, 0, self.part_available as usize)
                        .map_err(|_| ())?;
                } else {
                    let ci = self.current.expect("current matched above");
                    let data = self.pending[ci]
                        .data
                        .as_mut()
                        .expect("part data allocated with the header");
                    stream
                        .read_bytes(
                            data,
                            self.part_offset as usize,
                            self.part_available as usize,
                        )
                        .map_err(|_| ())?;
                }
            }
            if self.part_available + self.part_offset >= data_len as i32 && self.current.is_some() {
                let ci = self.current.take().expect("current matched above");
                let req = self.pending.remove(ci);
                self.note_network_complete(&req);
                self.complete(req);
            }
            self.part_available = 0;
        }
        Ok(())
    }

    /// Java `send`: open the ondemand socket on first use (4 s reconnect
    /// gate), write the 4-byte request, and reset the timeout bookkeeping.
    fn send(&mut self, archive: i32, file: i32, urgent: bool) {
        if self.stream.is_none() {
            let now = Instant::now();
            if now.duration_since(self.socket_open_time) < SOCKET_OPEN_GATE {
                return;
            }
            if self.open_socket().is_err() {
                self.part_available = 0;
                self.apply_reconnect_gate();
                self.set_fail_count(self.fail_count + 1);
                return;
            }
            self.socket_open_time = now;
            self.packet_cycle = 0;
        }
        self.buf[0] = archive as u8;
        self.buf[1] = (file >> 8) as u8;
        self.buf[2] = file as u8;
        self.buf[3] = if urgent {
            2
        } else if self.ingame.load(Ordering::Relaxed) {
            0
        } else {
            1
        };
        let ok = self
            .stream
            .as_mut()
            .is_some_and(|s| s.write(&self.buf, 4).is_ok());
        if ok {
            self.no_timeout_cycle = 0;
            self.set_fail_count(-10000);
        } else if self.recovering {
            self.note_dead_stream();
            self.set_fail_count(self.fail_count + 1);
        } else {
            self.recover_and_resend();
            self.set_fail_count(self.fail_count + 1);
        }
    }

    /// Java `openSocket(portOff + 43594)`: handshake is byte 15, then the
    /// engine replies with 8 bytes.
    fn open_socket(&mut self) -> io::Result<()> {
        let mut stream = match self.target {
            Some(target) => ClientStream::connect_for(target, &self.host, self.port)?,
            None => ClientStream::connect(&self.host, self.port)?,
        };
        stream.write(&[15], 1)?;
        for _ in 0..8 {
            stream.read()?;
        }
        self.saw_network_progress = false;
        self.stream = Some(stream);
        Ok(())
    }

    /// `complete`: remove the request from `pending`, convert finished
    /// non-urgent map prefetches to urgent archive 93 (TS `complete`), and
    /// post completions to the client. Java persists every completed file to
    /// its local cache, so only urgent files hit `completed` there; this port
    /// never writes the cache, so archive-0 files are posted even when not
    /// urgent — that is what warms the process-wide `Model` store for
    /// first-login `get_temp_model` (`on_demand_loop` -> `Model::unpack`).
    fn complete(&mut self, mut req: OnDemandRequest) {
        if let Some(i) = self
            .pending
            .iter()
            .position(|p| p.archive == req.archive && p.file == req.file)
        {
            self.pending.remove(i);
        }
        if !req.urgent && req.archive == 3 {
            req.urgent = true;
            req.archive = 93;
        }
        if req.urgent || req.archive == 0 {
            self.emit(WorkerMessage::Completed {
                archive: req.archive,
                file: req.file,
                urgent: req.urgent,
                data: req.data.map(Arc::new),
            });
        }
    }

    /// One `OnDemand.run` cycle: up to 100 queue/pending/extra/read passes,
    /// then the pending resend/timeout bookkeeping from Java `run`.
    fn tick(&mut self) {
        self.active = true;
        for i in 0..100 {
            if !self.active {
                break;
            }
            self.active = false;
            self.handle_queue();
            self.handle_pending();
            if self.urgent_count == 0 && i >= 5 {
                break;
            }
            self.handle_extra();
            if self.stream.is_some() {
                self.read();
            }
        }

        // Pending files already left `missing`, so `handle_pending` will not
        // send them again. After a dead socket the reconnect gate may have
        // deferred `recover_and_resend`; retry here each tick so we do not
        // wait a 50-cycle resend for the gate to lift.
        if self.stream.is_none() && !self.pending.is_empty() {
            let jobs: Vec<(i32, i32, bool)> = self
                .pending
                .iter()
                .map(|req| (req.archive, req.file, req.urgent))
                .collect();
            for (archive, file, urgent) in jobs {
                self.send(archive, file, urgent);
                if self.stream.is_none() {
                    break;
                }
            }
        }

        let mut loading = false;
        let mut resend: Vec<(i32, i32, bool)> = Vec::new();
        for req in self.pending.iter_mut() {
            if req.urgent {
                loading = true;
                req.cycle += 1;
                if req.cycle > 50 {
                    req.cycle = 0;
                    resend.push((req.archive, req.file, req.urgent));
                }
            }
        }
        if !loading {
            for req in self.pending.iter_mut() {
                loading = true;
                req.cycle += 1;
                if req.cycle > 50 {
                    req.cycle = 0;
                    resend.push((req.archive, req.file, req.urgent));
                }
            }
        }
        for (archive, file, urgent) in resend {
            self.send(archive, file, urgent);
        }

        if loading {
            self.packet_cycle += 1;
            if self.packet_cycle > 750 {
                self.recover_and_resend();
            }
        } else {
            self.packet_cycle = 0;
            self.set_message(String::new());
        }

        if self.ingame.load(Ordering::Relaxed)
            && self.stream.is_some()
            && (self.top_priority > 0 || self.cache_dir.is_none())
            && self.wants_java_keepalive()
        {
            self.no_timeout_cycle += 1;
            if self.no_timeout_cycle > 500 {
                self.no_timeout_cycle = 0;
                self.buf[0] = 0;
                self.buf[1] = 0;
                self.buf[2] = 0;
                self.buf[3] = 10;
                let ok = self
                    .stream
                    .as_mut()
                    .is_some_and(|s| s.write(&self.buf, 4).is_ok());
                if !ok {
                    self.recover_and_resend();
                }
            }
        }
    }

    fn wants_java_keepalive(&self) -> bool {
        matches!(self.revision, Some(ClientRevision::R274))
    }

    fn cached_payload(&self, archive: i32, file: i32) -> Option<Vec<u8>> {
        if archive < 0 || file < 0 {
            return None;
        }
        let crc = *self.crcs.get(archive as usize)?.get(file as usize)?;
        let version = *self.versions.get(archive as usize)?.get(file as usize)?;
        if let Some(data) = self.read_persisted(archive, file) {
            if validate(crc, version, Some(&data)) {
                return Some(data);
            }
        }
        let data = self
            .cache_dir
            .as_ref()
            .and_then(|dir| cache_read(dir, archive + 1, file))?;
        validate(crc, version, Some(&data)).then_some(data)
    }

    fn read_persisted(&self, archive: i32, file: i32) -> Option<Vec<u8>> {
        let path = self
            .persist_dir
            .as_ref()?
            .join(archive.to_string())
            .join(file.to_string());
        std::fs::read(path).ok()
    }

    fn persist_completed(&self, archive: i32, file: i32, data: &[u8]) {
        if !(0..=3).contains(&archive) || file < 0 {
            return;
        }
        let Some(root) = &self.persist_dir else {
            return;
        };
        let Some(crc) = self
            .crcs
            .get(archive as usize)
            .and_then(|t| t.get(file as usize))
        else {
            return;
        };
        let Some(version) = self
            .versions
            .get(archive as usize)
            .and_then(|t| t.get(file as usize))
        else {
            return;
        };
        if !validate(*crc, *version, Some(data)) {
            return;
        }
        let dir = root.join(archive.to_string());
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(file.to_string());
        static TMP: AtomicU64 = AtomicU64::new(1);
        let tmp = dir.join(format!(
            ".{file}.{}.{}.tmp",
            std::process::id(),
            TMP.fetch_add(1, Ordering::Relaxed)
        ));
        if std::fs::write(&tmp, data).is_err() {
            return;
        }
        let _ = std::fs::rename(tmp, path);
    }

    fn note_network_complete(&mut self, req: &OnDemandRequest) {
        if let Some(data) = req.data.as_ref() {
            self.persist_completed(req.archive, req.file, data);
        }
        self.saw_network_progress = true;
        self.reconnect_backoff = Duration::ZERO;
    }

    fn apply_reconnect_gate(&mut self) {
        if self.saw_network_progress {
            // Consume progress so only this recovery skips the gate.
            self.saw_network_progress = false;
            self.socket_open_time = Instant::now() - SOCKET_OPEN_GATE;
            self.reconnect_backoff = Duration::ZERO;
            return;
        }
        if self.reconnect_backoff.is_zero() {
            self.reconnect_backoff = RECONNECT_BACKOFF_START;
        } else {
            self.reconnect_backoff = self
                .reconnect_backoff
                .saturating_mul(2)
                .min(SOCKET_OPEN_GATE);
        }
        self.socket_open_time = Instant::now() + self.reconnect_backoff - SOCKET_OPEN_GATE;
    }

    fn note_dead_stream(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.close();
        }
        self.part_available = 0;
        self.current = None;
        self.apply_reconnect_gate();
    }

    fn recover_and_resend(&mut self) {
        if self.recovering {
            self.note_dead_stream();
            return;
        }
        self.recovering = true;
        self.note_dead_stream();
        let jobs: Vec<(i32, i32, bool)> = self
            .pending
            .iter()
            .map(|req| (req.archive, req.file, req.urgent))
            .collect();
        for req in &mut self.pending {
            req.cycle = 0;
        }
        for (archive, file, urgent) in jobs {
            self.send(archive, file, urgent);
            if self.stream.is_none() {
                break;
            }
        }
        self.recovering = false;
    }

    fn set_message(&mut self, message: String) {
        if self.message == message {
            return;
        }
        self.message = message;
        self.emit(WorkerMessage::Message(self.message.clone()));
    }

    fn set_fail_count(&mut self, fail_count: i32) {
        if self.fail_count == fail_count {
            return;
        }
        self.fail_count = fail_count;
        self.emit(WorkerMessage::FailCount(fail_count));
    }
}

/// Directory that actually holds `main_file_cache.dat`. `--cache` is the
/// jag pack (`pack/client`); the idx store lives in `pack/` one level up,
/// matching `cache_read`'s parent fallback. Java `fileStreams[0] != null`.
fn resolve_file_store(cache_dir: &str) -> Option<String> {
    let here = std::path::Path::new(cache_dir);
    if here.join("main_file_cache.dat").is_file() {
        return Some(cache_dir.to_string());
    }
    let parent = here.parent()?;
    if parent.join("main_file_cache.dat").is_file() {
        return Some(parent.to_str()?.to_string());
    }
    None
}

/// One read from the `main_file_cache` file store, the read path of the
/// engine `FileStream.read` / Java `FileStream.readFromFile` (the client
/// never writes its cache, so the write path is not ported). `pub(crate)`
/// so the client's `maininit` can unpack the whole store eagerly without a
/// server round-trip.
pub(crate) fn cache_read(cache_dir: &str, archive: i32, file: i32) -> Option<Vec<u8>> {
    cache_read_in(cache_dir, archive, file).or_else(|| {
        // Engine layout: jag packs live in pack/client, the main_file_cache
        // lives in pack/ (one directory up).
        let parent = std::path::Path::new(cache_dir).parent()?;
        cache_read_in(parent.to_str()?, archive, file)
    })
}

fn cache_read_in(cache_dir: &str, archive: i32, file: i32) -> Option<Vec<u8>> {
    if !(1..=4).contains(&archive) || file < 0 {
        return None;
    }
    use std::io::{Read, Seek, SeekFrom};
    let mut idx = std::fs::File::open(format!("{cache_dir}/main_file_cache.idx{archive}")).ok()?;
    let mut dat = std::fs::File::open(format!("{cache_dir}/main_file_cache.dat")).ok()?;
    idx.seek(SeekFrom::Start(file as u64 * 6)).ok()?;
    let mut rec = [0u8; 6];
    idx.read_exact(&mut rec).ok()?;
    let size = ((rec[0] as i32) << 16) + ((rec[1] as i32) << 8) + rec[2] as i32;
    let mut sector = ((rec[3] as i32) << 16) + ((rec[4] as i32) << 8) + rec[5] as i32;
    if size <= 0 || size > 2_000_000 {
        return None;
    }
    let mut out = Vec::with_capacity(size as usize);
    let mut part = 0i32;
    while (out.len() as i32) < size {
        if sector == 0 {
            return None;
        }
        dat.seek(SeekFrom::Start(sector as u64 * 520)).ok()?;
        let mut block = [0u8; 520];
        dat.read_exact(&mut block).ok()?;
        let file_id = ((block[0] as i32) << 8) + block[1] as i32;
        let part_id = ((block[2] as i32) << 8) + block[3] as i32;
        let next = ((block[4] as i32) << 16) + ((block[5] as i32) << 8) + block[6] as i32;
        let archive_id = block[7] as i32;
        if file_id != file || part_id != part || archive_id != archive + 1 {
            return None;
        }
        let take = ((size as usize) - out.len()).min(512);
        out.extend_from_slice(&block[8..8 + take]);
        sector = next;
        part += 1;
    }
    Some(out)
}

/// Cold-cache entry source: the client's update-protocol worker. The `unpack`
/// module drives snapshot preparation through this trait, so the snapshot
/// writer never owns a socket and the worker never owns the snapshot format.
/// Implementations must return only payloads that validate against the
/// versionlist crc/version tables (Java `OnDemand.validate`).
impl crate::unpack::EntrySource for OnDemand {
    fn fetch_entries(
        &mut self,
        archive: i32,
        files: &[i32],
    ) -> Result<Vec<(i32, Vec<u8>)>, String> {
        files.iter().try_for_each(|file| {
            if self.has_tables && !self.valid_file(archive, *file) {
                return Err(format!(
                    "archive {archive} file {file}: not a required versionlist entry"
                ));
            }
            Ok(())
        })?;
        for file in files {
            self.request(archive, *file);
        }
        let mut out = Vec::with_capacity(files.len());
        let mut last_progress = Instant::now();
        while out.len() < files.len() {
            self.run(false);
            while let Some(mut req) = self.pop_completed_raw() {
                if req.archive != archive || !files.contains(&req.file) {
                    continue;
                }
                let Some((version, crc)) = self.version_crc(req.archive, req.file) else {
                    return Err(format!(
                        "archive {archive} file {}: no versionlist entry",
                        req.file
                    ));
                };
                let Some(data) = req.data.take() else {
                    return Err(format!(
                        "archive {archive} file {}: the engine reports the entry absent",
                        req.file
                    ));
                };
                if !validate(crc, version, Some(&data)) {
                    return Err(format!(
                        "archive {archive} file {}: downloaded payload failed the versionlist \
                         version/CRC check",
                        req.file
                    ));
                }
                let body = if data.len() >= 2 {
                    &data[..data.len() - 2]
                } else {
                    &data
                };
                let raw = {
                    let mut raw = Vec::new();
                    GzDecoder::new(body).read_to_end(&mut raw).map_err(|_| {
                        format!(
                            "archive {archive} file {}: downloaded payload is not a gzip stream",
                            req.file
                        )
                    })?;
                    raw
                };
                out.push((req.file, raw));
                last_progress = Instant::now();
            }
            if out.len() < files.len() {
                if last_progress.elapsed() > FETCH_IDLE_TIMEOUT {
                    return Err(format!(
                        "archive {archive}: update server completed {}/{} files; no progress for \
                         {FETCH_IDLE_TIMEOUT:?}",
                        out.len(),
                        files.len()
                    ));
                }
                thread::sleep(FETCH_POLL);
            }
        }
        Ok(out)
    }
}

/// Idle bound for one cold-cache fetch batch: no completed file for this long
/// fails the batch (the caller reports the degraded fallback honestly).
const FETCH_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Poll interval while draining a cold-cache fetch batch.
const FETCH_POLL: Duration = Duration::from_millis(5);

/// Java `OnDemand.validate`: the 2-byte version trailer must match and the
/// CRC32 of the payload must match the versionlist table.
pub(crate) fn validate(crc: i32, version: i32, src: Option<&[u8]>) -> bool {
    let Some(src) = src else { return false };
    if src.len() < 2 {
        return false;
    }
    let trailer = src.len() - 2;
    let got_version = ((src[trailer] as i32) << 8) + src[trailer + 1] as i32;
    got_version == version && Packet::getcrc(src, 0, trailer) == crc
}

/// `gunzipSync(subarray(0, length - 2))` from TS `loop`. A corrupt stream
/// keeps the raw bytes so the failure surfaces downstream (the spec treats
/// bad ondemand data as a hard load error rather than an empty world).
fn gunzip(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if GzDecoder::new(src).read_to_end(&mut out).is_err() {
        return src.to_vec();
    }
    out
}

/// One g2/g4 table (`model_version`, `map_crc`, ...) from the versionlist.
fn read_table<F: Fn(&mut Packet) -> i32>(
    jag: &JagFile,
    names: &[&str],
    width: usize,
    read: F,
) -> Option<Vec<Vec<i32>>> {
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let data = jag.read(name)?;
        let mut buf = Packet::new(data);
        let count = buf.length() / width;
        let mut table = Vec::with_capacity(count);
        for _ in 0..count {
            table.push(read(&mut buf));
        }
        tables.push(table);
    }
    Some(tables)
}

/// One raw index table (`anim_index` as g2s, `midi_index` as g1s).
fn read_raw_table<F: Fn(&mut Packet) -> i32>(
    jag: &JagFile,
    name: &str,
    width: usize,
    read: F,
) -> Vec<i32> {
    match jag.read(name) {
        Some(data) => {
            let mut buf = Packet::new(data);
            let count = buf.length() / width;
            (0..count).map(|_| read(&mut buf)).collect()
        }
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::OnDemand;

    /// Java `Client.maininit` (5251-5277) maps `getModelUse` bits to a
    /// prefetch priority: the first matching bit in the 8/0x20/0x10/0x40/
    /// 0x80/2/4 ladder wins, then `& 1` overrides everything with 3.
    #[test]
    fn model_use_priority_matches_java() {
        assert_eq!(OnDemand::model_use_priority(0), 0);
        assert_eq!(OnDemand::model_use_priority(1), 3); // &1 last, wins
        assert_eq!(OnDemand::model_use_priority(2), 5);
        assert_eq!(OnDemand::model_use_priority(4), 4);
        assert_eq!(OnDemand::model_use_priority(8), 10);
        assert_eq!(OnDemand::model_use_priority(0x10), 8);
        assert_eq!(OnDemand::model_use_priority(0x20), 9);
        assert_eq!(OnDemand::model_use_priority(0x40), 7);
        assert_eq!(OnDemand::model_use_priority(0x80), 6);
        assert_eq!(OnDemand::model_use_priority(0x09), 3); // 8 then &1 -> 3
    }
}
