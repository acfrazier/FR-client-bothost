//! Port of `~/experiments/Server/webclient/src/io/OnDemand.ts` plus the Java
//! `OnDemand.java` worker thread. The TS Worker boundary becomes one OS
//! thread (the spec's "OnDemand thread"): the client-side handle owns the
//! version tables and the TS `requests`/`completed` bookkeeping; the worker
//! owns the engine ondemand socket pump — a second connection to the game
//! port, Java `Client.portOff + 43594` — and posts completed files back.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;

use crate::dash3d::model::ModelProvider;
use crate::datastruct::{Arena, LinkList, LinkList2, LinkableTrait, Links};
use crate::io::client_stream::ClientStream;
use crate::io::jagfile::JagFile;
use crate::io::packet::Packet;

/// Reconnect gate in Java `OnDemand.send`: the socket is not reopened within
/// 4 s of the last open. Spawn starts past the gate (first send is not
/// gated) and `DropSocket` resets it so a relogin reconnects immediately.
const SOCKET_OPEN_GATE: Duration = Duration::from_millis(4000);

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
        let _ = self.tx.send(WorkerCommand::Request { archive: 0, file: id });
    }
}

/// Client → worker commands (TS `postMessage` inbound messages).
enum WorkerCommand {
    Request { archive: i32, file: i32 },
    PrefetchPriority { archive: i32, file: i32, priority: i32 },
    Prefetch { archive: i32, file: i32 },
    ClearPrefetches,
    /// Logout: the engine dropped the update connection, so drop the
    /// worker's stream and reset the reconnect state. Unlike `Stop`, the
    /// worker and the version tables stay alive for the next login.
    DropSocket,
    Stop,
}

/// Worker → client messages (TS `onmessage` outbound messages).
enum WorkerMessage {
    Completed {
        archive: i32,
        file: i32,
        urgent: bool,
        data: Option<Vec<u8>>,
    },
    Message(String),
    FailCount(i32),
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
    /// Some when the `main_file_cache` file store is present (Java
    /// `app.fileStreams[0] != null`).
    cache_dir: Option<String>,
    tx: mpsc::Sender<WorkerMessage>,
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
        }
    }

    /// Parse the version/crc/index tables from the versionlist jag and spawn
    /// the worker thread (TS `new OnDemand(versionlist, app)` + Java `init`).
    /// Returns `None` when the versionlist lacks one of the four version or
    /// crc tables (TS throws on those).
    pub fn new(
        versionlist: &JagFile,
        host: &str,
        port: u16,
        cache_dir: &str,
        ingame: Arc<AtomicBool>,
    ) -> Option<Self> {
        let versions = read_table(
            versionlist,
            &["model_version", "anim_version", "midi_version", "map_version"],
            2,
            |buf| buf.g2(),
        )?;
        let crcs = read_table(
            versionlist,
            &["model_crc", "anim_crc", "midi_crc", "map_crc"],
            4,
            |buf| buf.g4(),
        )?;

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

        let (command_tx, command_rx) = mpsc::channel();
        let (message_tx, message_rx) = mpsc::channel();
        let worker_running = Arc::new(AtomicBool::new(true));
        let cache_present =
            std::path::Path::new(&format!("{cache_dir}/main_file_cache.dat")).exists();
        let worker = Worker {
            commands: command_rx,
            versions: versions.clone(),
            crcs: crcs.clone(),
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
            cache_dir: cache_present.then(|| cache_dir.to_string()),
            tx: message_tx,
            running: worker_running.clone(),
            ingame: ingame.clone(),
        };
        let handle = thread::spawn(move || worker_main(worker));

        let mut arena = Arena::new();
        let requests = LinkList2::new(&mut arena);
        Some(OnDemand {
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
            tx: Some(command_tx),
            rx: message_rx,
            worker: Some(handle),
            ingame,
            worker_running,
        })
    }

    /// TS `OnDemand.stop()`: stop the worker and close its socket.
    pub fn stop(&mut self) {
        self.running = false;
        self.worker_running.store(false, Ordering::Relaxed);
        if let Some(tx) = &self.tx {
            let _ = tx.send(WorkerCommand::Stop);
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }

    /// `logout` path: the engine dropped the update connection, so the
    /// worker drops its stream and resets the reopen gate. The worker and
    /// the version tables stay alive (Java `unload` stops OnDemand, not
    /// `logout`); a no-op when there is no worker (`new_unconnected`).
    pub fn drop_socket(&self) {
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
                return if ty == 0 { self.map_land[i] } else { self.map_loc[i] };
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
        if let Some(tx) = &self.tx {
            let _ = tx.send(WorkerCommand::PrefetchPriority { archive, file, priority });
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
        let mut req = self.completed.pop_front()?;
        if let Some(id) = self.find_request_id(req.archive, req.file) {
            self.arena.unlink2(id);
            self.arena.take(id);
        }
        if let Some(data) = req.data.take() {
            let body = if data.len() >= 2 { &data[..data.len() - 2] } else { &data };
            req.data = Some(gunzip(body));
        }
        Some(req)
    }

    /// `OnDemand.run()` heartbeat: sync the shared ingame flag, bump `cycle`,
    /// and pull worker messages into `completed`/`message`/`fail_count`.
    pub fn run(&mut self, ingame: bool) {
        if !self.running {
            return;
        }
        self.ingame.store(ingame, Ordering::Relaxed);
        self.cycle += 1;
        self.drain_worker();
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
                WorkerMessage::Completed { archive, file, urgent, data } => {
                    // TS reuses the tracked request node when found; the
                    // completed list here holds an owned copy so the payload
                    // is independent of the requests arena.
                    let mut req = OnDemandRequest::new(archive, file);
                    req.urgent = urgent;
                    req.data = data;
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

/// Worker thread body: Java `OnDemand.run` with the command channel drained
/// each pass (TS message handling). Sleeps 20 ms (50 ms when only prefetches
/// remain and a local cache exists), then one pump cycle.
fn worker_main(mut worker: Worker) {
    while worker.running.load(Ordering::Relaxed) {
        worker.drain_commands();
        let delay = if worker.top_priority == 0 && worker.cache_dir.is_some() {
            50
        } else {
            20
        };
        thread::sleep(Duration::from_millis(delay));
        worker.tick();
    }
}

impl Worker {
    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.try_recv() {
            match cmd {
                WorkerCommand::Request { archive, file } => {
                    if self.valid_file(archive, file) {
                        self.queue.push_back(OnDemandRequest::new(archive, file));
                    }
                }
                WorkerCommand::PrefetchPriority { archive, file, priority } => {
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
                    self.socket_open_time = Instant::now() - SOCKET_OPEN_GATE;
                }
                WorkerCommand::Stop => return,
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

    /// Java `prefetchPriority`: only meaningful when a local cache exists;
    /// the file is dropped from the prefetch set when the cache copy already
    /// validates against its crc/version.
    fn prefetch_priority(&mut self, archive: i32, file: i32, priority: i32) {
        let Some(dir) = &self.cache_dir else { return };
        if !self.valid_file(archive, file) {
            return;
        }
        let data = cache_read(dir, archive + 1, file);
        if validate(
            self.crcs[archive as usize][file as usize],
            self.versions[archive as usize][file as usize],
            data.as_deref(),
        ) {
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
        if self.cache_dir.is_none() || !self.valid_file(archive, file) {
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
            let mut cached = self.cache_dir.as_ref().and_then(|dir| {
                cache_read(dir, req.archive + 1, req.file)
            });
            if !validate(
                self.crcs[req.archive as usize][req.file as usize],
                self.versions[req.archive as usize][req.file as usize],
                cached.as_deref(),
            ) {
                cached = None;
            }
            if cached.is_none() {
                self.missing.push_back(req);
            } else {
                req.data = cached;
                self.complete(req);
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
            let Some(req) = self.missing.pop_front() else { break };
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
            if let Some(mut stream) = self.stream.take() {
                stream.close();
            }
            self.part_available = 0;
        }
    }

    fn try_read(&mut self) -> Result<(), ()> {
        let available = {
            let Some(stream) = &mut self.stream else { return Ok(()) };
            stream.available().map_err(|_| ())?
        };

        if self.part_available == 0 && available >= 6 {
            self.active = true;
            let mut hdr = [0u8; 6];
            {
                let Some(stream) = &mut self.stream else { return Ok(()) };
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
                        let _ = self.tx.send(WorkerMessage::Completed {
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
                Some(ci) => (
                    self.pending[ci].data.as_ref().map_or(0, |d| d.len()),
                    false,
                ),
                None => (self.buf.len(), true),
            };
            {
                let Some(stream) = &mut self.stream else { return Ok(()) };
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
                        .read_bytes(data, self.part_offset as usize, self.part_available as usize)
                        .map_err(|_| ())?;
                }
            }
            if self.part_available + self.part_offset >= data_len as i32 && self.current.is_some() {
                let ci = self.current.take().expect("current matched above");
                let req = self.pending.remove(ci);
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
        } else {
            if let Some(mut stream) = self.stream.take() {
                stream.close();
            }
            self.part_available = 0;
            self.set_fail_count(self.fail_count + 1);
        }
    }

    /// Java `openSocket(portOff + 43594)`: handshake is byte 15, then the
    /// engine replies with 8 bytes.
    fn open_socket(&mut self) -> io::Result<()> {
        let mut stream = ClientStream::connect(&self.host, self.port)?;
        stream.write(&[15], 1)?;
        for _ in 0..8 {
            stream.read()?;
        }
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
            let _ = self.tx.send(WorkerMessage::Completed {
                archive: req.archive,
                file: req.file,
                urgent: req.urgent,
                data: req.data,
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
                if let Some(mut stream) = self.stream.take() {
                    stream.close();
                }
                self.part_available = 0;
            }
        } else {
            self.packet_cycle = 0;
            self.set_message(String::new());
        }

        if self.ingame.load(Ordering::Relaxed)
            && self.stream.is_some()
            && (self.top_priority > 0 || self.cache_dir.is_none())
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
                    self.packet_cycle = 5000;
                }
            }
        }
    }

    fn set_message(&mut self, message: String) {
        if self.message == message {
            return;
        }
        self.message = message;
        let _ = self.tx.send(WorkerMessage::Message(self.message.clone()));
    }

    fn set_fail_count(&mut self, fail_count: i32) {
        if self.fail_count == fail_count {
            return;
        }
        self.fail_count = fail_count;
        let _ = self.tx.send(WorkerMessage::FailCount(fail_count));
    }
}

/// One read from the `main_file_cache` file store, the read path of the
/// engine `FileStream.read` / Java `FileStream.readFromFile` (the client
/// never writes its cache, so the write path is not ported).
fn cache_read(cache_dir: &str, archive: i32, file: i32) -> Option<Vec<u8>> {
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

/// Java `OnDemand.validate`: the 2-byte version trailer must match and the
/// CRC32 of the payload must match the versionlist table.
fn validate(crc: i32, version: i32, src: Option<&[u8]>) -> bool {
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
