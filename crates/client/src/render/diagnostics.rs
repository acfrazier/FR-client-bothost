//! Optional LIVE draw-level render trace.
//!
//! Compiled only with `--features render-diagnostics`. Ordinary release
//! builds must not mention this module. There is no public start/take API,
//! no TLS recorder, and no hot-path `env::var` once the OnceLock config is
//! parsed. Enable at runtime with `BOT_RENDER_TRACE=1` plus a tile region
//! and/or loc IDs; output stops after `BOT_RENDER_TRACE_FRAMES` (max 300).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::client::client::{APPLET_H, APPLET_W};
use crate::dash3d::{BuildArea, Model};

/// Cap unique shade identities so a wide region cannot grow a dump set.
const MAX_SHADE_KEYS: usize = 4096;
/// Cap histogram buckets so one loc cannot flood the log.
const MAX_HIST_BUCKETS: usize = 24;
const SENTINEL: i32 = 12_345_678;

/// Betty north-wall default when TRACE is on but X/Z are omitted.
const DEFAULT_X: i32 = 3012;
const DEFAULT_Z: i32 = 3261;
const DEFAULT_R: i32 = 4;
const MAX_FRAMES: u32 = 300;
/// Orbit-tile Chebyshev slack around the filter origin. `max(R, 8)` so a
/// wide mainland build that merely contains the origin does not arm.
const STATION_MIN: i32 = 8;

const PREFIX: &str = "[render-trace]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceFilter {
    pub origin_x: i32,
    pub origin_z: i32,
    pub radius: i32,
    pub ids: Vec<i32>,
    pub max_frames: u32,
}

impl TraceFilter {
    pub fn from_env_values(
        x: Option<i32>,
        z: Option<i32>,
        radius: Option<i32>,
        ids: &[i32],
        frames: Option<u32>,
    ) -> Self {
        let max_frames = frames.unwrap_or(MAX_FRAMES).clamp(1, MAX_FRAMES);
        Self {
            origin_x: x.unwrap_or(DEFAULT_X),
            origin_z: z.unwrap_or(DEFAULT_Z),
            radius: radius.unwrap_or(DEFAULT_R).clamp(0, 16),
            ids: ids.to_vec(),
            max_frames,
        }
    }

    pub fn tile_in_region(&self, world_x: i32, world_z: i32) -> bool {
        (world_x - self.origin_x).abs() <= self.radius
            && (world_z - self.origin_z).abs() <= self.radius
    }

    pub fn id_listed(&self, id: i32) -> bool {
        self.ids.is_empty() || self.ids.contains(&id)
    }

    /// Locs: region AND optional ID list. Entity sprites (kind != loc) in
    /// the region always match so covering NPCs/players are visible.
    pub fn matches(&self, world_x: i32, world_z: i32, id: i32, kind: u8) -> bool {
        if !self.tile_in_region(world_x, world_z) {
            return false;
        }
        if kind != 2 {
            return true;
        }
        self.id_listed(id)
    }

    pub fn station_radius(&self) -> i32 {
        self.radius.max(STATION_MIN)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameCam {
    pub cycle: i32,
    pub eye_x: i32,
    pub eye_y: i32,
    pub eye_z: i32,
    pub yaw: i32,
    pub pitch: i32,
    pub pre_jitter_x: i32,
    pub pre_jitter_y: i32,
    pub pre_jitter_z: i32,
    pub orbit_x: i32,
    pub orbit_z: i32,
    pub orbit_yaw: i32,
    pub orbit_pitch: i32,
    pub macro_x: i32,
    pub macro_z: i32,
    pub macro_angle: i32,
    pub base_x: i32,
    pub base_z: i32,
}

impl FrameCam {
    pub fn orbit_world_tile(&self) -> (i32, i32) {
        (
            self.base_x + (self.orbit_x >> 7),
            self.base_z + (self.orbit_z >> 7),
        )
    }
}

/// True only at the intended station: map is built, filter origin is in
/// this 104×104 build, and the orbit (player follow) world tile is within
/// `max(R, 8)` of the origin. Login/loading/Lumbridge/far-mainland paints
/// do not arm. A build that merely contains the origin is not enough.
pub fn station_near(cam: &FrameCam, filter: &TraceFilter) -> bool {
    if cam.base_x == 0 && cam.base_z == 0 {
        return false;
    }
    let local_x = filter.origin_x - cam.base_x;
    let local_z = filter.origin_z - cam.base_z;
    if !(0..BuildArea::SIZE).contains(&local_x) || !(0..BuildArea::SIZE).contains(&local_z) {
        return false;
    }
    let (owx, owz) = cam.orbit_world_tile();
    let r = filter.station_radius();
    (owx - filter.origin_x).abs() <= r && (owz - filter.origin_z).abs() <= r
}

/// Fill-level action: `submitted` means a model was handed to `world_render`.
/// That is not a pixel promise; `paint reason=accepted` is the raster gate.
pub fn handoff_action(occluded: bool, submitted: bool, model: &str) -> &'static str {
    if occluded {
        "occluded"
    } else if submitted {
        "submitted"
    } else if model == "n/a" {
        "skipped"
    } else {
        "missing"
    }
}

struct Live {
    cam: FrameCam,
}

static CONFIG: OnceLock<Option<TraceFilter>> = OnceLock::new();
static LIVE: Mutex<Option<Live>> = Mutex::new(None);
static BACKEND: OnceLock<&'static str> = OnceLock::new();
static FRAME: AtomicU32 = AtomicU32::new(0);
static SEQ: AtomicU32 = AtomicU32::new(0);
static BACKEND_LOGGED: AtomicBool = AtomicBool::new(false);
static STOPPED: AtomicBool = AtomicBool::new(false);
static ARMED: AtomicBool = AtomicBool::new(false);
static SHADE_DUMPED: OnceLock<Mutex<HashSet<ShadeKey>>> = OnceLock::new();
static GAME_IMAGE_FB: Mutex<Option<GameImageFb>> = Mutex::new(None);

/// Inclusive scene-space slot on `area_game` (512×334). Verified from
/// `CpuBackend::scene` (`set_clipping(game.width, game.height)` after
/// `cls`) and the hop-4 still census 617–746 × 160–240 via
/// `x=40+2*sx`, `y=2*sy`. Not a product viewport.
pub const CAVE_SLOT_SX0: i32 = 288;
pub const CAVE_SLOT_SX1: i32 = 353;
pub const CAVE_SLOT_SY0: i32 = 80;
pub const CAVE_SLOT_SY1: i32 = 120;
/// `area_game.blit_into(draw_area, 4, 4)` in `CpuBackend::composite_scene`.
pub const AREA_GAME_BLIT_X: i32 = 4;
pub const AREA_GAME_BLIT_Y: i32 = 4;
pub const PACKED_ZERO: u32 = 0x0000_0000;
/// `colour_table[2]` at brightness 0.8 (Hermes-corrected gouraud
/// `(((S<<15)>>7)>>8)=S` → table[2], not table[256]).
pub const PACKED_PALETTE2: u32 = 0x0009_0707;
pub const PACKED_RGB2: u32 = 0x0002_0202;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedHist {
    pub n: u32,
    pub zero: u32,
    pub palette2: u32,
    pub rgb2: u32,
    pub other: u32,
}

impl PackedHist {
    pub const EMPTY: Self = Self {
        n: 0,
        zero: 0,
        palette2: 0,
        rgb2: 0,
        other: 0,
    };
}

/// Camera copied at pixel production. Never reread from `LIVE` to label
/// a later upload or PNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRoiCam {
    pub cycle: i32,
    pub eye_x: i32,
    pub eye_y: i32,
    pub eye_z: i32,
    pub yaw: i32,
    pub pitch: i32,
    pub origin_x: i32,
    pub origin_z: i32,
    pub trace_frame: u32,
}

/// ImGui `HiDpiMode::Default`: item rects are logical; PNG is physical
/// (`logical * display_framebuffer_scale`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelRoiCoordDomain {
    LogicalImGui,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelRoiImage {
    pub logical_min: [f32; 2],
    pub logical_size: [f32; 2],
    pub fb_scale: [f32; 2],
    pub domain: PixelRoiCoordDomain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelRoiUpload {
    pub frame_id: u64,
    pub packed: PackedHist,
    pub rgba: PackedHist,
    pub packed_fp: u64,
    pub slot: String,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelRoiMeta {
    pub frame_id: u64,
    pub cam: PixelRoiCam,
    pub area_game: PackedHist,
    pub draw_area: PackedHist,
    pub area_game_fp: u64,
    pub draw_area_fp: u64,
    pub upload: Option<PixelRoiUpload>,
}

impl PixelRoiMeta {
    pub fn produced(
        frame_id: u64,
        cam: PixelRoiCam,
        area_game: PackedHist,
        draw_area: PackedHist,
    ) -> Self {
        Self {
            frame_id,
            cam,
            area_game,
            draw_area,
            area_game_fp: 0,
            draw_area_fp: 0,
            upload: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PixelRoiShotBind {
    pub meta: PixelRoiMeta,
    pub image: PixelRoiImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PixelRoiProof {
    Accept { frame_id: u64 },
    RejectUnavailable { reason: &'static str },
    RejectMismatch { reason: &'static str },
}

thread_local! {
    static THREAD_PIXEL_ROI: RefCell<Option<PixelRoiMeta>> = const { RefCell::new(None) };
}

static UI_TAKEN_ROI: Mutex<Option<(PixelRoiMeta, String, u64)>> = Mutex::new(None);
static PRESENTED_ROI: Mutex<Option<(PixelRoiMeta, String)>> = Mutex::new(None);
static READBACK_CTX: Mutex<Option<String>> = Mutex::new(None);
static GAME_IMAGE_PRESENT: AtomicU32 = AtomicU32::new(0);
const GAME_IMAGE_NONE: u32 = 0;
const GAME_IMAGE_CPU: u32 = 1;
const GAME_IMAGE_GPU: u32 = 2;

static PIXEL_ROI_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_pixel_roi_id() -> u64 {
    PIXEL_ROI_ID.fetch_add(1, Ordering::Relaxed)
}

pub fn stamp_thread_pixel_roi(meta: PixelRoiMeta) {
    THREAD_PIXEL_ROI.with(|slot| *slot.borrow_mut() = Some(meta));
}

pub fn take_thread_pixel_roi() -> Option<PixelRoiMeta> {
    THREAD_PIXEL_ROI.with(|slot| slot.borrow_mut().take())
}

pub fn clear_thread_pixel_roi() {
    THREAD_PIXEL_ROI.with(|slot| *slot.borrow_mut() = None);
}

pub fn attach_upload(
    meta: &mut PixelRoiMeta,
    packed: PackedHist,
    rgba: PackedHist,
    packed_fp: u64,
    slot: &str,
    generation: u64,
) {
    meta.upload = Some(PixelRoiUpload {
        frame_id: meta.frame_id,
        packed,
        rgba,
        packed_fp,
        slot: slot.to_string(),
        generation,
    });
}

pub fn bind_shot(meta: PixelRoiMeta, image: PixelRoiImage) -> PixelRoiShotBind {
    PixelRoiShotBind { meta, image }
}

pub fn label_cam<'a>(
    bind: &'a PixelRoiShotBind,
    _live_now: Option<&PixelRoiCam>,
) -> &'a PixelRoiCam {
    &bind.meta.cam
}

pub fn pixel_roi_proof(
    screenshot_requested: bool,
    bind: Option<&PixelRoiShotBind>,
) -> PixelRoiProof {
    if !screenshot_requested {
        return PixelRoiProof::RejectUnavailable {
            reason: "no-screenshot-request",
        };
    }
    let Some(bind) = bind else {
        return PixelRoiProof::RejectUnavailable {
            reason: "no-attached-bind",
        };
    };
    if bind.meta.frame_id == 0 {
        return PixelRoiProof::RejectUnavailable {
            reason: "no-frame-id",
        };
    }
    PixelRoiProof::Accept {
        frame_id: bind.meta.frame_id,
    }
}

pub fn pixel_roi_ids_agree(cpu_id: u64, other_id: Option<u64>) -> PixelRoiProof {
    match other_id {
        Some(id) if id == cpu_id && cpu_id != 0 => PixelRoiProof::Accept { frame_id: cpu_id },
        Some(_) => PixelRoiProof::RejectMismatch {
            reason: "frame-id-mismatch",
        },
        None => PixelRoiProof::RejectUnavailable {
            reason: "missing-stage-id",
        },
    }
}

pub fn cam_from_live(cam: FrameCam) -> PixelRoiCam {
    PixelRoiCam {
        cycle: cam.cycle,
        eye_x: cam.eye_x,
        eye_y: cam.eye_y,
        eye_z: cam.eye_z,
        yaw: cam.yaw,
        pitch: cam.pitch,
        origin_x: cam.base_x,
        origin_z: cam.base_z,
        trace_frame: frame_no(),
    }
}

#[derive(Clone, Copy, Debug)]
struct GameImageFb {
    min_x: f32,
    min_y: f32,
    size_x: f32,
    size_y: f32,
    fb_x: f32,
    fb_y: f32,
}

/// Compact identity: layer 0=ground, 1=wall, 2=wall2, 3=decor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ShadeKey {
    layer: u8,
    world_x: i32,
    world_z: i32,
    id: i32,
}

fn parse_ids(raw: &str) -> Vec<i32> {
    raw.split(',')
        .filter_map(|part| part.trim().parse().ok())
        .collect()
}

fn parse_i32(key: &str) -> Option<i32> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn parse_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

fn load_filter() -> Option<TraceFilter> {
    let on = std::env::var("BOT_RENDER_TRACE").is_ok_and(|v| v == "1");
    if !on {
        return None;
    }
    let ids = std::env::var("BOT_RENDER_TRACE_IDS")
        .ok()
        .map(|v| parse_ids(&v))
        .unwrap_or_default();
    Some(TraceFilter::from_env_values(
        parse_i32("BOT_RENDER_TRACE_X"),
        parse_i32("BOT_RENDER_TRACE_Z"),
        parse_i32("BOT_RENDER_TRACE_R"),
        &ids,
        parse_u32("BOT_RENDER_TRACE_FRAMES"),
    ))
}

fn config() -> Option<&'static TraceFilter> {
    CONFIG.get_or_init(load_filter).as_ref()
}

fn tracing() -> bool {
    !STOPPED.load(Ordering::Relaxed) && config().is_some()
}

fn armed() -> bool {
    ARMED.load(Ordering::Relaxed)
}

fn backend_name() -> &'static str {
    BACKEND.get().copied().unwrap_or("?")
}

fn store_cam(cam: FrameCam) {
    if let Ok(mut g) = LIVE.lock() {
        *g = Some(Live { cam });
    }
}

fn emit_frame_line(n: u32, cam: FrameCam) {
    SEQ.store(0, Ordering::Relaxed);
    eprintln!(
        "{PREFIX} frame={n} cycle={} eye={},{},{} yaw={} pitch={} prejitter={},{},{} orbit={},{} ory={} orp={} macro={},{},{} base={},{} backend={}",
        cam.cycle,
        cam.eye_x,
        cam.eye_y,
        cam.eye_z,
        cam.yaw,
        cam.pitch,
        cam.pre_jitter_x,
        cam.pre_jitter_y,
        cam.pre_jitter_z,
        cam.orbit_x,
        cam.orbit_z,
        cam.orbit_yaw,
        cam.orbit_pitch,
        cam.macro_x,
        cam.macro_z,
        cam.macro_angle,
        cam.base_x,
        cam.base_z,
        backend_name()
    );
}

fn try_count_armed_frame(cam: FrameCam, filter: &TraceFilter) -> bool {
    let n = FRAME.fetch_add(1, Ordering::Relaxed) + 1;
    if n > filter.max_frames {
        FRAME.fetch_sub(1, Ordering::Relaxed);
        if !STOPPED.swap(true, Ordering::Relaxed) {
            eprintln!(
                "{PREFIX} stop frames={} region={},{} r={} ids={:?}",
                filter.max_frames, filter.origin_x, filter.origin_z, filter.radius, filter.ids
            );
        }
        ARMED.store(false, Ordering::Relaxed);
        return false;
    }
    ARMED.store(true, Ordering::Relaxed);
    emit_frame_line(n, cam);
    true
}

/// Matching visit/sprite can arm only when the stored camera is already
/// at the station. A same-build fill that merely walks origin±R (Lumbridge
/// or far Port-Sarim in a 104×104 that contains Betty) must not start the
/// 300-cap or emit a bare frame line.
fn late_arm() -> bool {
    if !tracing() || armed() {
        return armed() && tracing();
    }
    let Some(filter) = filter_ref() else {
        return false;
    };
    let Some(cam) = LIVE.lock().ok().and_then(|g| g.as_ref().map(|l| l.cam)) else {
        return false;
    };
    if !station_near(&cam, &filter) {
        return false;
    }
    try_count_armed_frame(cam, &filter)
}

fn loc_id(typecode: i32) -> i32 {
    (typecode >> 14) & 0x7fff
}

fn loc_kind(typecode: i32) -> u8 {
    ((typecode >> 29) & 0x3) as u8
}

/// Both wall slots share `Wall.typecode`. `typecode2` is the signed
/// shape/angle info byte (`((angle << 6) + shape)`), not a loc typecode.
pub fn wall_shade_typecode(typecode: i32, typecode2: i32) -> i32 {
    let _ = typecode2;
    typecode
}

fn kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "player",
        1 => "npc",
        2 => "loc",
        3 => "obj",
        _ => "unknown",
    }
}

fn next_seq() -> u32 {
    SEQ.fetch_add(1, Ordering::Relaxed) + 1
}

fn world_tile(local_x: i32, local_z: i32) -> (i32, i32) {
    LIVE.lock()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .map(|l| (l.cam.base_x + local_x, l.cam.base_z + local_z))
        })
        .unwrap_or((local_x, local_z))
}

fn filter_ref() -> Option<TraceFilter> {
    config().cloned()
}

fn frame_no() -> u32 {
    FRAME.load(Ordering::Relaxed)
}

/// Persist the selected raster backend before any frame line. Logs once.
pub fn backend(kind: &'static str) {
    let _ = BACKEND.set(kind);
    if config().is_none() {
        return;
    }
    if BACKEND_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!("{PREFIX} backend={kind}");
}

/// Start a traced frame. The 300 cap counts **armed** frames only: orbit
/// world tile near the filter origin inside this build. Unarmed paints
/// store the camera for world-tile conversion and emit nothing.
pub fn begin_frame(cam: FrameCam) {
    let Some(filter) = config().cloned() else {
        return;
    };
    if STOPPED.load(Ordering::Relaxed) {
        return;
    }
    store_cam(cam);
    ARMED.store(false, Ordering::Relaxed);
    SEQ.store(0, Ordering::Relaxed);
    if !station_near(&cam, &filter) {
        return;
    }
    try_count_armed_frame(cam, &filter);
}

fn accept_tile_id(local_x: i32, local_z: i32, typecode: i32) -> Option<(i32, i32, i32, u8)> {
    if !tracing() {
        return None;
    }
    let filter = filter_ref()?;
    let (wx, wz) = world_tile(local_x, local_z);
    let id = loc_id(typecode);
    let kind = loc_kind(typecode);
    if !filter.matches(wx, wz, id, kind) {
        return None;
    }
    if !armed() && !late_arm() {
        return None;
    }
    Some((wx, wz, id, kind))
}

fn accept_tile(local_x: i32, local_z: i32) -> Option<(i32, i32)> {
    if !tracing() {
        return None;
    }
    let filter = filter_ref()?;
    let (wx, wz) = world_tile(local_x, local_z);
    if !filter.tile_in_region(wx, wz) {
        return None;
    }
    if !armed() && !late_arm() {
        return None;
    }
    Some((wx, wz))
}

/// Tile entered the fill queue / RING walk.
pub fn visit(
    local_x: i32,
    local_z: i32,
    level: i32,
    draw_front: bool,
    draw_back: bool,
    corner_sides: i32,
    sides_before: i32,
    sides_after: i32,
    check_adjacent: bool,
) {
    let Some((wx, wz)) = accept_tile(local_x, local_z) else {
        return;
    };
    let seq = next_seq();
    eprintln!(
        "{PREFIX} f={} seq={seq} visit tile={wx},{wz} local={local_x},{local_z} lvl={level} drawFront={} drawBack={} corner={corner_sides} sidesBefore={sides_before} sidesAfter={sides_after} adjacent={}",
        frame_no(),
        u8::from(draw_front),
        u8::from(draw_back),
        u8::from(check_adjacent)
    );
}

/// Wall front/back decision.
pub fn wall(
    local_x: i32,
    local_z: i32,
    typecode: i32,
    pass: &'static str,
    occluded: bool,
    submitted: bool,
) {
    let Some((wx, wz, id, kind)) = accept_tile_id(local_x, local_z, typecode) else {
        return;
    };
    let seq = next_seq();
    let action = handoff_action(occluded, submitted, "");
    eprintln!(
        "{PREFIX} f={} seq={seq} wall tile={wx},{wz} id={id} kind={} tc={typecode} pass={pass} occluded={} action={action}",
        frame_no(),
        kind_name(kind),
        u8::from(occluded)
    );
}

/// Wall-decoration front/back / offset-corner decision.
pub fn decor(
    local_x: i32,
    local_z: i32,
    typecode: i32,
    wshape: i32,
    angle: i32,
    pass: &'static str,
    occluded: bool,
    branch: &'static str,
    model: &'static str,
    submitted: bool,
) {
    let Some((wx, wz, id, kind)) = accept_tile_id(local_x, local_z, typecode) else {
        return;
    };
    let seq = next_seq();
    let action = handoff_action(occluded, submitted, model);
    eprintln!(
        "{PREFIX} f={} seq={seq} decor tile={wx},{wz} id={id} kind={} tc={typecode} wshape={wshape} angle={angle} pass={pass} branch={branch} model={model} occluded={} action={action}",
        frame_no(),
        kind_name(kind),
        u8::from(occluded)
    );
}

/// Sprite buffer / occlusion / paint (locs, NPCs, players).
pub fn sprite(
    min_x: i32,
    min_z: i32,
    max_x: i32,
    max_z: i32,
    typecode: i32,
    distance: i32,
    action: &'static str,
    occluded: bool,
) {
    if !tracing() {
        return;
    }
    let Some(filter) = filter_ref() else {
        return;
    };
    let (wmin_x, wmin_z) = world_tile(min_x, min_z);
    let (wmax_x, wmax_z) = world_tile(max_x, max_z);
    let id = loc_id(typecode);
    let kind = loc_kind(typecode);
    let overlaps =
        (wmin_x..=wmax_x).any(|x| (wmin_z..=wmax_z).any(|z| filter.tile_in_region(x, z)));
    if !overlaps {
        return;
    }
    if kind == 2 && !filter.id_listed(id) {
        return;
    }
    if !armed() && !late_arm() {
        return;
    }
    let seq = next_seq();
    eprintln!(
        "{PREFIX} f={} seq={seq} sprite tiles={wmin_x},{wmin_z}..{wmax_x},{wmax_z} id={id} kind={} tc={typecode} dist={distance} occluded={} action={action}",
        frame_no(),
        kind_name(kind),
        u8::from(occluded)
    );
}

/// GPU mesh emit filtered by loc ID only (no tile on the emitter).
pub fn mesh_id(typecode: i32, action: &'static str) {
    if !tracing() || !armed() {
        return;
    }
    let Some(filter) = filter_ref() else {
        return;
    };
    let id = loc_id(typecode);
    let kind = loc_kind(typecode);
    if filter.ids.is_empty() || !filter.id_listed(id) {
        return;
    }
    let seq = next_seq();
    eprintln!(
        "{PREFIX} f={} seq={seq} mesh id={id} kind={} tc={typecode} action={action}",
        frame_no(),
        kind_name(kind)
    );
}

/// GPU mesh emit (or skip) for a matching loc.
pub fn mesh(local_x: i32, local_z: i32, typecode: i32, action: &'static str) {
    let Some((wx, wz, id, kind)) = accept_tile_id(local_x, local_z, typecode) else {
        return;
    };
    let seq = next_seq();
    eprintln!(
        "{PREFIX} f={} seq={seq} mesh tile={wx},{wz} id={id} kind={} tc={typecode} action={action}",
        frame_no(),
        kind_name(kind)
    );
}

/// `Model.world_render` near-plane / frustum reject or accepted paint.
pub fn paint(typecode: i32, reason: &'static str, mid_z: i32, max_z: i32) {
    if !tracing() || !armed() {
        return;
    }
    let Some(filter) = filter_ref() else {
        return;
    };
    let id = loc_id(typecode);
    let kind = loc_kind(typecode);
    // world_render has no tile; require an explicit ID list so this is
    // not a global all-model dump.
    if filter.ids.is_empty() || !filter.id_listed(id) {
        return;
    }
    let seq = next_seq();
    eprintln!(
        "{PREFIX} f={} seq={seq} paint id={id} kind={} tc={typecode} reason={reason} midz={mid_z} maxz={max_z}",
        frame_no(),
        kind_name(kind)
    );
}

fn shade_once(key: ShadeKey) -> bool {
    let dumped = SHADE_DUMPED.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut dumped) = dumped.lock() else {
        return false;
    };
    if dumped.contains(&key) {
        return false;
    }
    if dumped.len() >= MAX_SHADE_KEYS {
        return false;
    }
    dumped.insert(key);
    true
}

fn format_hist(values: &[i32]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    let mut counts: BTreeMap<i32, u32> = BTreeMap::new();
    for &v in values {
        *counts.entry(v).or_insert(0) += 1;
    }
    let unique = counts.len();
    let mut parts: Vec<String> = counts
        .iter()
        .take(MAX_HIST_BUCKETS)
        .map(|(v, n)| format!("{v}:{n}"))
        .collect();
    if unique > MAX_HIST_BUCKETS {
        parts.push(format!("+{}", unique - MAX_HIST_BUCKETS));
    }
    parts.join(",")
}

fn shade_stats(values: &[i32]) -> (i32, i32, u32, u32, u32) {
    let mut min = i32::MAX;
    let mut max = i32::MIN;
    let mut unlit = 0u32;
    let mut shade2 = 0u32;
    let mut sentinel = 0u32;
    for &v in values {
        min = min.min(v);
        max = max.max(v);
        if v == 0 {
            unlit += 1;
        }
        if v == SENTINEL {
            sentinel += 1;
        }
        // HSL lightness is the low 7 bits after get_colour / get_ocol.
        // Exact 2 is the untextured clamp; DEBUG lit counts treat 2 as lit.
        if v != SENTINEL && (v & 0x7f) == 2 {
            shade2 += 1;
        }
    }
    if values.is_empty() {
        min = 0;
        max = 0;
    }
    (min, max, unlit, shade2, sentinel)
}

fn rtype_hist(render_type: Option<&[i32]>, n: usize) -> (u32, String) {
    let Some(rt) = render_type else {
        return (0, "none".to_string());
    };
    let slice = &rt[..n.min(rt.len())];
    let hidden = slice.iter().filter(|&&t| t == -1).count() as u32;
    (hidden, format_hist(slice))
}

fn tex_summary(model: &Model, n: usize) -> String {
    let textured_rt = model
        .face_render_type
        .as_ref()
        .map(|rt| {
            rt.iter()
                .take(n)
                .filter(|&&t| t != -1 && t & 0x2 == 0x2)
                .count()
        })
        .unwrap_or(0);
    format!("faces={textured_rt} num_t={}", model.num_t)
}

/// Wall/decor face shades, once per loc+tile+layer for this armed run.
/// `layer` is `wall`, `wall2`, or `decor`. Identity is tile + loc id.
pub fn loc_shades(local_x: i32, local_z: i32, typecode: i32, layer: &'static str, model: &Model) {
    let Some((wx, wz, id, kind)) = accept_tile_id(local_x, local_z, typecode) else {
        return;
    };
    let layer_code = match layer {
        "wall" => 1,
        "wall2" => 2,
        "decor" => 3,
        _ => 4,
    };
    if !shade_once(ShadeKey {
        layer: layer_code,
        world_x: wx,
        world_z: wz,
        id,
    }) {
        return;
    }
    let n =
        (model.num_faces as usize).min(model.face_colour_a.as_ref().map(|v| v.len()).unwrap_or(0));
    let fca = model.face_colour_a.as_ref().map(|v| &v[..n]).unwrap_or(&[]);
    let (fca_min, fca_max, unlit, shade2, sentinel) = shade_stats(fca);
    let (hidden, rtypes) = rtype_hist(model.face_render_type.as_deref(), model.num_faces as usize);
    let hsl = model
        .face_colour
        .as_ref()
        .map(|v| format_hist(&v[..n.min(v.len())]))
        .unwrap_or_else(|| "none".to_string());
    let seq = next_seq();
    eprintln!(
        "{PREFIX} f={} seq={seq} shade loc tile={wx},{wz} local={local_x},{local_z} id={id} kind={} tc={typecode} layer={layer} faces={} hidden={hidden} unlit={unlit} shade2={shade2} sentinel={sentinel} fca_min={fca_min} fca_max={fca_max} hist={} rtype={rtypes} tex={} hsl={hsl}",
        frame_no(),
        kind_name(kind),
        model.num_faces,
        format_hist(fca),
        tex_summary(model, model.num_faces as usize)
    );
}

/// Ground overlay / quick / stamp shades, once per tile for this armed run.
pub fn ground_shades(
    local_x: i32,
    local_z: i32,
    src: &'static str,
    shape: i32,
    rotation: i32,
    texture: i32,
    overlay: i32,
    underlay: i32,
    face_colour_a: &[i32],
    colours: [i32; 4],
    colour2: [i32; 4],
) {
    let Some((wx, wz)) = accept_tile(local_x, local_z) else {
        return;
    };
    if !shade_once(ShadeKey {
        layer: 0,
        world_x: wx,
        world_z: wz,
        id: 0,
    }) {
        return;
    }
    let (fca_min, fca_max, unlit, shade2, sentinel) = shade_stats(face_colour_a);
    let seq = next_seq();
    eprintln!(
        "{PREFIX} f={} seq={seq} shade ground tile={wx},{wz} local={local_x},{local_z} src={src} shape={shape} rot={rotation} tex={texture} overlay={overlay} underlay={underlay} faces={} hidden=0 unlit={unlit} shade2={shade2} sentinel={sentinel} fca_min={fca_min} fca_max={fca_max} hist={} colours={},{},{},{} colour2={},{},{},{}",
        frame_no(),
        face_colour_a.len(),
        format_hist(face_colour_a),
        colours[0],
        colours[1],
        colours[2],
        colours[3],
        colour2[0],
        colour2[1],
        colour2[2],
        colour2[3]
    );
}

/// Inclusive applet rectangle of the cave slot after the (4,4) blit.
pub fn cave_slot_applet() -> (i32, i32, i32, i32) {
    (
        AREA_GAME_BLIT_X + CAVE_SLOT_SX0,
        AREA_GAME_BLIT_Y + CAVE_SLOT_SY0,
        AREA_GAME_BLIT_X + CAVE_SLOT_SX1,
        AREA_GAME_BLIT_Y + CAVE_SLOT_SY1,
    )
}

/// Sample only an armed `scene_state==2` frame, once. `scene_state==1`
/// is the last-FBO freeze and must not touch pixels.
pub fn pixel_roi_should_sample(scene_state: i32, armed: bool, already: bool) -> bool {
    scene_state == 2 && armed && !already
}

pub fn tracing_enabled() -> bool {
    tracing()
}

/// Layout-only stash of this UI frame's Game Image rect (no pixel copy).
pub fn note_game_image_fb(min_x: f32, min_y: f32, size_x: f32, size_y: f32, fb_x: f32, fb_y: f32) {
    if let Ok(mut g) = GAME_IMAGE_FB.lock() {
        *g = Some(GameImageFb {
            min_x,
            min_y,
            size_x,
            size_y,
            fb_x,
            fb_y,
        });
    }
}

pub fn this_frame_image() -> Option<PixelRoiImage> {
    GAME_IMAGE_FB.lock().ok().and_then(|g| {
        g.map(|fb| PixelRoiImage {
            logical_min: [fb.min_x, fb.min_y],
            logical_size: [fb.size_x, fb.size_y],
            fb_scale: [fb.fb_x, fb.fb_y],
            domain: PixelRoiCoordDomain::LogicalImGui,
        })
    })
}

pub fn set_ui_taken_roi(meta: PixelRoiMeta, slot: &str, generation: u64) {
    if let Ok(mut g) = UI_TAKEN_ROI.lock() {
        *g = Some((meta, slot.to_string(), generation));
    }
}

pub fn take_ui_taken_roi() -> Option<(PixelRoiMeta, String, u64)> {
    UI_TAKEN_ROI.lock().ok().and_then(|mut g| g.take())
}

fn clear_ui_taken_roi() {
    if let Ok(mut g) = UI_TAKEN_ROI.lock() {
        *g = None;
    }
}

fn set_presented_roi(meta: PixelRoiMeta, slot: &str) {
    if let Ok(mut g) = PRESENTED_ROI.lock() {
        *g = Some((meta, slot.to_string()));
    }
}

fn clear_presented_roi() {
    if let Ok(mut g) = PRESENTED_ROI.lock() {
        *g = None;
    }
}

/// Focused Game Image is about to upload a CPU pixmap. `None` clears any
/// leftover taken stamp so an untracked present cannot keep a prior ROI.
pub fn prepare_present_roi(roi: Option<(PixelRoiMeta, &str, u64)>) {
    match roi {
        Some((meta, slot, generation)) => set_ui_taken_roi(meta, slot, generation),
        None => clear_ui_taken_roi(),
    }
}

/// Arm the next focused Game Image CPU upload so a missing stamp clears
/// presented instead of leaving the previous bot frame attached.
pub fn arm_game_image_cpu_upload() {
    GAME_IMAGE_PRESENT.store(GAME_IMAGE_CPU, Ordering::Relaxed);
}

/// Arm the next Game Image GPU bind so a texture change clears presented
/// and a same-texture last-FBO hold does not.
pub fn arm_game_image_gpu_present() {
    GAME_IMAGE_PRESENT.store(GAME_IMAGE_GPU, Ordering::Relaxed);
}

fn take_game_image_arm(kind: u32) -> bool {
    GAME_IMAGE_PRESENT
        .compare_exchange(kind, GAME_IMAGE_NONE, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

/// Finish a CPU upload: attach when a stamp travelled with this present,
/// otherwise drop presented if this upload is the focused Game Image.
pub fn complete_cpu_upload(packed: &[u32], rgba: &[u8], width: i32, height: i32) {
    let armed = take_game_image_arm(GAME_IMAGE_CPU);
    match take_ui_taken_roi() {
        Some((mut meta, slot, generation)) => {
            attach_panel_upload(&mut meta, packed, rgba, width, height, &slot, generation);
            set_presented_roi(meta, &slot);
        }
        None if armed => clear_presented_roi(),
        None => {}
    }
}

/// New GPU texture on the focused Game Image: prior CPU ROI is not that image.
/// Rail/tile rebinds are not armed and must not drop the Game Image stamp.
pub fn note_gpu_texture_changed() {
    if take_game_image_arm(GAME_IMAGE_GPU) {
        clear_ui_taken_roi();
        clear_presented_roi();
    }
}

/// Same actual GPU texture as last present (last-FBO / bind no-op).
pub fn note_gpu_texture_held() {
    let _ = take_game_image_arm(GAME_IMAGE_GPU);
}

/// Slot identity of the texture being copied at readback enqueue.
pub fn note_readback_context(slot: &str) {
    if let Ok(mut g) = READBACK_CTX.lock() {
        *g = Some(slot.to_string());
    }
}

pub fn reset_pixel_roi_ui() {
    clear_ui_taken_roi();
    clear_presented_roi();
    if let Ok(mut g) = READBACK_CTX.lock() {
        *g = None;
    }
    if let Ok(mut g) = GAME_IMAGE_FB.lock() {
        *g = None;
    }
    GAME_IMAGE_PRESENT.store(GAME_IMAGE_NONE, Ordering::Relaxed);
}

/// Snapshot presented CPU meta + this-frame logical image at readback enqueue.
/// Bound to the held-texture slot: another slot's context cannot wear this ROI.
pub fn snapshot_shot_bind() -> Option<PixelRoiShotBind> {
    let (meta, slot) = PRESENTED_ROI.lock().ok().and_then(|g| g.clone())?;
    if let Some(ctx) = READBACK_CTX.lock().ok().and_then(|g| g.clone()) {
        if ctx != slot {
            return None;
        }
    }
    let image = this_frame_image()?;
    Some(bind_shot(meta, image))
}

fn clamp_roi(
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> Option<(i32, i32, i32, i32)> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let x0 = x0.max(0);
    let y0 = y0.max(0);
    let x1 = x1.min(width - 1);
    let y1 = y1.min(height - 1);
    if x0 > x1 || y0 > y1 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

fn bucket(packed: u32, h: &mut PackedHist) {
    h.n += 1;
    match packed & 0x00ff_ffff {
        PACKED_ZERO => h.zero += 1,
        PACKED_PALETTE2 => h.palette2 += 1,
        PACKED_RGB2 => h.rgb2 += 1,
        _ => h.other += 1,
    }
}

pub fn packed_roi_hist(
    pixels: &[i32],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> PackedHist {
    let mut h = PackedHist::EMPTY;
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return h;
    };
    for y in y0..=y1 {
        let row = (y * width) as usize;
        for x in x0..=x1 {
            let i = row + x as usize;
            if let Some(p) = pixels.get(i) {
                bucket(*p as u32, &mut h);
            }
        }
    }
    h
}

pub fn packed_u32_roi_hist(
    pixels: &[u32],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> PackedHist {
    let mut h = PackedHist::EMPTY;
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return h;
    };
    for y in y0..=y1 {
        let row = (y * width) as usize;
        for x in x0..=x1 {
            let i = row + x as usize;
            if let Some(p) = pixels.get(i) {
                bucket(*p, &mut h);
            }
        }
    }
    h
}

pub fn rgba_roi_hist(
    rgba: &[u8],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> PackedHist {
    let mut h = PackedHist::EMPTY;
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return h;
    };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let i = ((y * width + x) as usize) * 4;
            if i + 2 < rgba.len() {
                let packed =
                    ((rgba[i] as u32) << 16) | ((rgba[i + 1] as u32) << 8) | (rgba[i + 2] as u32);
                bucket(packed, &mut h);
            }
        }
    }
    h
}

pub fn packed_roi_fp(
    pixels: &[i32],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> u64 {
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return 0;
    };
    let mut fp = 0u64;
    let mut i = 1u64;
    for y in y0..=y1 {
        let row = (y * width) as usize;
        for x in x0..=x1 {
            if let Some(p) = pixels.get(row + x as usize) {
                fp ^= (*p as u32 as u64).wrapping_mul(i);
                i = i.wrapping_add(1);
            }
        }
    }
    fp
}

fn packed_u32_roi_fp(
    pixels: &[u32],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> u64 {
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return 0;
    };
    let mut fp = 0u64;
    let mut i = 1u64;
    for y in y0..=y1 {
        let row = (y * width) as usize;
        for x in x0..=x1 {
            if let Some(p) = pixels.get(row + x as usize) {
                fp ^= (*p as u64).wrapping_mul(i);
                i = i.wrapping_add(1);
            }
        }
    }
    fp
}

#[allow(dead_code)]
fn roi_samples(
    pixels: &[i32],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> String {
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return String::new();
    };
    let mut out = Vec::new();
    'walk: for y in y0..=y1 {
        let row = (y * width) as usize;
        for x in x0..=x1 {
            if let Some(p) = pixels.get(row + x as usize) {
                out.push(format!("{:06x}", *p as u32 & 0x00ff_ffff));
                if out.len() == 8 {
                    break 'walk;
                }
            }
        }
    }
    out.join(",")
}

#[allow(dead_code)]
fn roi_samples_u32(
    pixels: &[u32],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> String {
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return String::new();
    };
    let mut out = Vec::new();
    'walk: for y in y0..=y1 {
        let row = (y * width) as usize;
        for x in x0..=x1 {
            if let Some(p) = pixels.get(row + x as usize) {
                out.push(format!("{:06x}", *p & 0x00ff_ffff));
                if out.len() == 8 {
                    break 'walk;
                }
            }
        }
    }
    out.join(",")
}

fn roi_samples_rgba(
    rgba: &[u8],
    width: i32,
    height: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> String {
    let Some((x0, y0, x1, y1)) = clamp_roi(width, height, x0, y0, x1, y1) else {
        return String::new();
    };
    let mut out = Vec::new();
    'walk: for y in y0..=y1 {
        for x in x0..=x1 {
            let i = ((y * width + x) as usize) * 4;
            if i + 2 < rgba.len() {
                let packed =
                    ((rgba[i] as u32) << 16) | ((rgba[i + 1] as u32) << 8) | (rgba[i + 2] as u32);
                out.push(format!("{packed:06x}"));
                if out.len() == 8 {
                    break 'walk;
                }
            }
        }
    }
    out.join(",")
}

/// Map the inclusive applet slot through the Game Image rect into PNG
/// framebuffer pixels. `size` is the ImGui Image size; `fb_*` is the
/// window framebuffer scale (HiDPI).
pub fn cave_slot_png_roi(
    min_x: f32,
    min_y: f32,
    size_x: f32,
    size_y: f32,
    fb_x: f32,
    fb_y: f32,
    applet_w: f32,
    applet_h: f32,
) -> (i32, i32, i32, i32) {
    let (ax0, ay0, ax1, ay1) = cave_slot_applet();
    let sx = if applet_w > 0.0 {
        size_x / applet_w
    } else {
        1.0
    };
    let sy = if applet_h > 0.0 {
        size_y / applet_h
    } else {
        1.0
    };
    let x0 = ((min_x + ax0 as f32 * sx) * fb_x).floor() as i32;
    let y0 = ((min_y + ay0 as f32 * sy) * fb_y).floor() as i32;
    let x1 = ((min_x + ax1 as f32 * sx) * fb_x).floor() as i32;
    let y1 = ((min_y + ay1 as f32 * sy) * fb_y).floor() as i32;
    (x0, y0, x1, y1)
}

fn emit_hist_line(
    stage: &str,
    cam: PixelRoiCam,
    frame_id: u64,
    dim_w: i32,
    dim_h: i32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    h: PackedHist,
    fp: u64,
    sample: &str,
    extra: &str,
) {
    eprintln!(
        "{PREFIX} pixel-roi stage={stage} frame_id={frame_id} f={} cycle={} eye={},{},{} yaw={} pitch={} origin={},{} dim={dim_w}x{dim_h} roi={x0}..{x1},{y0}..{y1} n={} zero={} palette2={} rgb2={} other={} fp={fp:016x} sample={sample}{extra}",
        cam.trace_frame,
        cam.cycle,
        cam.eye_x,
        cam.eye_y,
        cam.eye_z,
        cam.yaw,
        cam.pitch,
        cam.origin_x,
        cam.origin_z,
        h.n,
        h.zero,
        h.palette2,
        h.rgb2,
        h.other,
    );
}

/// Hist + TLS stamp for this production pixmap. Does not emit and does
/// not consume a later screenshot proof.
pub fn stamp_cpu_slot_pixels(
    area_game: Option<(&[i32], i32, i32)>,
    draw_area: Option<(&[i32], i32, i32)>,
    cam: PixelRoiCam,
) -> Option<PixelRoiMeta> {
    let mut meta = PixelRoiMeta::produced(
        next_pixel_roi_id(),
        cam,
        PackedHist::EMPTY,
        PackedHist::EMPTY,
    );
    if let Some((px, w, h)) = area_game {
        meta.area_game = packed_roi_hist(
            px,
            w,
            h,
            CAVE_SLOT_SX0,
            CAVE_SLOT_SY0,
            CAVE_SLOT_SX1,
            CAVE_SLOT_SY1,
        );
        meta.area_game_fp = packed_roi_fp(
            px,
            w,
            h,
            CAVE_SLOT_SX0,
            CAVE_SLOT_SY0,
            CAVE_SLOT_SX1,
            CAVE_SLOT_SY1,
        );
    }
    if let Some((px, w, h)) = draw_area {
        let (x0, y0, x1, y1) = cave_slot_applet();
        meta.draw_area = packed_roi_hist(px, w, h, x0, y0, x1, y1);
        meta.draw_area_fp = packed_roi_fp(px, w, h, x0, y0, x1, y1);
    }
    stamp_thread_pixel_roi(meta.clone());
    Some(meta)
}

/// Stamp this armed `scene_state==2` frame onto the slot-thread TLS.
/// Does not emit and does not consume a later screenshot proof.
pub fn dump_cpu_slot_pixels(
    area_game: Option<(&[i32], i32, i32)>,
    draw_area: Option<(&[i32], i32, i32)>,
    scene_state: i32,
) {
    if !tracing() {
        clear_thread_pixel_roi();
        return;
    }
    if !pixel_roi_should_sample(scene_state, armed(), false) {
        clear_thread_pixel_roi();
        return;
    }
    let cam = LIVE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|l| cam_from_live(l.cam)))
        .unwrap_or(PixelRoiCam {
            cycle: 0,
            eye_x: 0,
            eye_y: 0,
            eye_z: 0,
            yaw: 0,
            pitch: 0,
            origin_x: 0,
            origin_z: 0,
            trace_frame: frame_no(),
        });
    let _ = stamp_cpu_slot_pixels(area_game, draw_area, cam);
}

/// Attach upload hists to the mailbox meta that travelled with this pixmap.
pub fn attach_panel_upload(
    meta: &mut PixelRoiMeta,
    packed: &[u32],
    rgba: &[u8],
    width: i32,
    height: i32,
    slot: &str,
    generation: u64,
) {
    if !tracing() {
        return;
    }
    let (x0, y0, x1, y1) = cave_slot_applet();
    let packed_h = packed_u32_roi_hist(packed, width, height, x0, y0, x1, y1);
    let fp = packed_u32_roi_fp(packed, width, height, x0, y0, x1, y1);
    let rgba_h = rgba_roi_hist(rgba, width, height, x0, y0, x1, y1);
    let _ = (width, height, y1);
    attach_upload(meta, packed_h, rgba_h, fp, slot, generation);
}

/// Emit the triad from the bind attached at readback enqueue, or reject.
/// Never reads current `LIVE` / last Game Image rect.
pub fn dump_png_slot(
    rgba: &[u8],
    width: i32,
    height: i32,
    label: &str,
    screenshot_requested: bool,
    bind: Option<&PixelRoiShotBind>,
) {
    if !tracing() {
        return;
    }
    match pixel_roi_proof(screenshot_requested, bind) {
        PixelRoiProof::RejectUnavailable { reason } => {
            eprintln!("{PREFIX} pixel-roi rejected=unavailable reason={reason} label={label} dim={width}x{height}");
            return;
        }
        PixelRoiProof::RejectMismatch { reason } => {
            eprintln!("{PREFIX} pixel-roi rejected=mismatch reason={reason} label={label}");
            return;
        }
        PixelRoiProof::Accept { frame_id } => {
            let Some(bind) = bind else { return };
            if let Some(upload) = bind.meta.upload.as_ref() {
                if let PixelRoiProof::RejectMismatch { reason } =
                    pixel_roi_ids_agree(frame_id, Some(upload.frame_id))
                {
                    eprintln!("{PREFIX} pixel-roi rejected=mismatch reason={reason} label={label} frame_id={frame_id}");
                    return;
                }
            }
            let cam = bind.meta.cam;
            let (sx0, sy0, sx1, sy1) = (CAVE_SLOT_SX0, CAVE_SLOT_SY0, CAVE_SLOT_SX1, CAVE_SLOT_SY1);
            let (ax0, ay0, ax1, ay1) = cave_slot_applet();
            emit_hist_line(
                "area_game",
                cam,
                frame_id,
                512,
                334,
                sx0,
                sy0,
                sx1,
                sy1,
                bind.meta.area_game,
                bind.meta.area_game_fp,
                "",
                "",
            );
            emit_hist_line(
                "draw_area",
                cam,
                frame_id,
                765,
                503,
                ax0,
                ay0,
                ax1,
                ay1,
                bind.meta.draw_area,
                bind.meta.draw_area_fp,
                "",
                "",
            );
            if let Some(upload) = bind.meta.upload.as_ref() {
                let extra = format!(" slot={} gen={}", upload.slot, upload.generation);
                emit_hist_line(
                    "upload-packed",
                    cam,
                    frame_id,
                    765,
                    503,
                    ax0,
                    ay0,
                    ax1,
                    ay1,
                    upload.packed,
                    upload.packed_fp,
                    "",
                    &extra,
                );
                emit_hist_line(
                    "upload-rgba",
                    cam,
                    frame_id,
                    765,
                    503,
                    ax0,
                    ay0,
                    ax1,
                    ay1,
                    upload.rgba,
                    upload.packed_fp,
                    "",
                    &extra,
                );
            }
            let img = bind.image;
            let (x0, y0, x1, y1) = cave_slot_png_roi(
                img.logical_min[0],
                img.logical_min[1],
                img.logical_size[0],
                img.logical_size[1],
                img.fb_scale[0],
                img.fb_scale[1],
                APPLET_W as f32,
                APPLET_H as f32,
            );
            let hist = rgba_roi_hist(rgba, width, height, x0, y0, x1, y1);
            let sample = roi_samples_rgba(rgba, width, height, x0, y0, x1, y1);
            let extra = format!(
                " label={label} domain=logical-imgui image={:.1},{:.1},{:.1}x{:.1} fb={:.2},{:.2}",
                img.logical_min[0],
                img.logical_min[1],
                img.logical_size[0],
                img.logical_size[1],
                img.fb_scale[0],
                img.fb_scale[1]
            );
            emit_hist_line(
                "png", cam, frame_id, width, height, x0, y0, x1, y1, hist, 0, &sample, &extra,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_chebyshev_includes_betty_north_wall() {
        let f = TraceFilter::from_env_values(Some(3012), Some(3261), Some(4), &[], None);
        assert!(f.tile_in_region(3011, 3261));
        assert!(f.tile_in_region(3013, 3261));
        assert!(f.tile_in_region(3012, 3262));
        assert!(f.tile_in_region(3010, 3257));
        assert!(!f.tile_in_region(3020, 3270));
    }

    #[test]
    fn ids_filter_locs_but_entities_in_region_pass() {
        let f = TraceFilter::from_env_values(Some(3012), Some(3261), Some(4), &[908, 1838], None);
        assert!(f.matches(3013, 3261, 908, 2));
        assert!(f.matches(3012, 3261, 1838, 2));
        assert!(!f.matches(3012, 3261, 724, 2));
        assert!(f.matches(3012, 3258, 1, 1));
        assert!(!f.matches(3100, 3300, 908, 2));
    }

    #[test]
    fn empty_ids_match_any_loc_in_region() {
        let f = TraceFilter::from_env_values(Some(3012), Some(3261), Some(4), &[], None);
        assert!(f.matches(3011, 3261, 724, 2));
        assert!(f.matches(3012, 3262, 1902, 2));
        assert!(!f.matches(2900, 3200, 908, 2));
    }

    #[test]
    fn frames_clamp_to_300() {
        let f = TraceFilter::from_env_values(None, None, None, &[], Some(9999));
        assert_eq!(f.max_frames, 300);
        assert_eq!(f.origin_x, 3012);
        assert_eq!(f.origin_z, 3261);
        let zero = TraceFilter::from_env_values(None, None, None, &[], Some(0));
        assert_eq!(zero.max_frames, 1);
    }

    fn betty_cam() -> FrameCam {
        FrameCam {
            cycle: 1,
            eye_x: 6720,
            eye_y: -1097,
            eye_z: 5512,
            yaw: 0,
            pitch: 256,
            pre_jitter_x: 6720,
            pre_jitter_y: -1097,
            pre_jitter_z: 5512,
            orbit_x: (3012 - 2960) << 7,
            orbit_z: (3258 - 3208) << 7,
            orbit_yaw: 0,
            orbit_pitch: 256,
            macro_x: 0,
            macro_z: 0,
            macro_angle: 0,
            base_x: 2960,
            base_z: 3208,
        }
    }

    #[test]
    fn unarmed_paints_do_not_consume_cap() {
        let f = TraceFilter::from_env_values(Some(3012), Some(3261), Some(4), &[908], Some(300));
        let mut counted = 0u32;
        let unready = FrameCam {
            base_x: 0,
            base_z: 0,
            orbit_x: 0,
            orbit_z: 0,
            ..betty_cam()
        };
        let lumbridge = FrameCam {
            base_x: 3200,
            base_z: 3200,
            orbit_x: (3222 - 3200) << 7,
            orbit_z: (3218 - 3200) << 7,
            ..betty_cam()
        };
        // Same Port-Sarim-sized build as Betty, orbit still far south.
        let same_build_far = FrameCam {
            orbit_x: (3010 - 2960) << 7,
            orbit_z: (3228 - 3208) << 7,
            ..betty_cam()
        };
        for cam in [unready, lumbridge, same_build_far] {
            assert!(!station_near(&cam, &f), "{cam:?}");
            if station_near(&cam, &f) {
                counted += 1;
            }
        }
        assert_eq!(counted, 0);
        assert!(station_near(&betty_cam(), &f));
        if station_near(&betty_cam(), &f) {
            counted += 1;
        }
        assert_eq!(counted, 1);
    }

    #[test]
    fn handoff_labels_missing_not_drawn() {
        assert_eq!(handoff_action(true, false, "n/a"), "occluded");
        assert_eq!(handoff_action(false, true, "resolved"), "submitted");
        assert_eq!(handoff_action(false, false, "missing"), "missing");
        assert_eq!(handoff_action(false, false, "n/a"), "skipped");
        assert_eq!(handoff_action(false, false, ""), "missing");
    }

    #[test]
    fn shade_hist_orders_values_and_caps_buckets() {
        assert_eq!(format_hist(&[]), "none");
        assert_eq!(format_hist(&[2, 2, 3778, 2]), "2:3,3778:1");
        let many: Vec<i32> = (0..30).collect();
        let hist = format_hist(&many);
        assert!(hist.starts_with("0:1,1:1"));
        assert!(hist.ends_with("+6"), "{hist}");
        assert_eq!(hist.matches(',').count(), MAX_HIST_BUCKETS);
    }

    #[test]
    fn shade_stats_count_lightness_2_not_as_unlit() {
        let (min, max, unlit, shade2, sentinel) = shade_stats(&[0, 2, 3778, SENTINEL, 130]);
        assert_eq!(min, 0);
        assert_eq!(max, SENTINEL);
        assert_eq!(unlit, 1);
        // 2 and 130 (0x82) both have lightness 2; sentinel is excluded.
        assert_eq!(shade2, 2);
        assert_eq!(sentinel, 1);
    }

    #[test]
    fn shade_identity_connects_primitive_and_tile() {
        let wall = ShadeKey {
            layer: 1,
            world_x: 3011,
            world_z: 9816,
            id: 1417,
        };
        let other_tile = ShadeKey {
            layer: 1,
            world_x: 3010,
            world_z: 9816,
            id: 1417,
        };
        let ground = ShadeKey {
            layer: 0,
            world_x: 3011,
            world_z: 9816,
            id: 0,
        };
        assert_ne!(wall, other_tile);
        assert_ne!(wall, ground);
        assert_eq!(wall.id, 1417);
    }

    #[test]
    fn packed_roi_hist_counts_zero_palette2_rgb2_other_and_clamps() {
        // 4×3 surface; slot request extends past the right edge.
        let mut px = vec![0i32; 12];
        px[0] = 0x0000_0000;
        px[1] = 0x0009_0707;
        px[2] = 0x0002_0202;
        px[3] = 0x00ff_0000;
        px[4] = 0x0009_0707;
        px[5] = 0x0002_0202;
        px[6] = 0x00ff_0000;
        px[7] = 0x00ff_0000;
        let h = packed_roi_hist(&px, 4, 3, 0, 0, 5, 1);
        assert_eq!(
            h,
            PackedHist {
                n: 8,
                zero: 1,
                palette2: 2,
                rgb2: 2,
                other: 3,
            }
        );
        let empty = packed_roi_hist(&px, 4, 3, 9, 9, 10, 10);
        assert_eq!(empty.n, 0);
        assert_eq!(empty.zero + empty.palette2 + empty.rgb2 + empty.other, 0);
    }

    #[test]
    fn rgba_roi_hist_maps_expand_channels_to_the_same_buckets() {
        // expand_rgba: 0x00RRGGBB → [R,G,B,255]
        let rgba = [
            0x00, 0x00, 0x00, 255, // zero
            0x09, 0x07, 0x07, 255, // colour_table[2] @ 0.8
            0x02, 0x02, 0x02, 255, // measured PNG hole
            0x74, 0x59, 0x2a, 255, // gold-ish other
        ];
        let h = rgba_roi_hist(&rgba, 4, 1, 0, 0, 3, 0);
        assert_eq!(
            h,
            PackedHist {
                n: 4,
                zero: 1,
                palette2: 1,
                rgb2: 1,
                other: 1,
            }
        );
    }

    #[test]
    fn cave_slot_applet_is_area_game_blit_plus_scene_roi() {
        let (x0, y0, x1, y1) = cave_slot_applet();
        assert_eq!(AREA_GAME_BLIT_X, 4);
        assert_eq!(AREA_GAME_BLIT_Y, 4);
        assert_eq!(x0, AREA_GAME_BLIT_X + CAVE_SLOT_SX0);
        assert_eq!(y0, AREA_GAME_BLIT_Y + CAVE_SLOT_SY0);
        assert_eq!(x1, AREA_GAME_BLIT_X + CAVE_SLOT_SX1);
        assert_eq!(y1, AREA_GAME_BLIT_Y + CAVE_SLOT_SY1);
        assert_eq!(
            (CAVE_SLOT_SX0, CAVE_SLOT_SY0, CAVE_SLOT_SX1, CAVE_SLOT_SY1),
            (288, 80, 353, 120)
        );
        let n = packed_roi_hist(
            &vec![0i32; 512 * 334],
            512,
            334,
            CAVE_SLOT_SX0,
            CAVE_SLOT_SY0,
            CAVE_SLOT_SX1,
            CAVE_SLOT_SY1,
        );
        assert_eq!(n.n, 66 * 41);
    }

    #[test]
    fn pixel_roi_sample_requires_scene2_and_armed_and_is_once() {
        assert!(
            !pixel_roi_should_sample(1, true, false),
            "last-FBO freeze must not sample"
        );
        assert!(
            !pixel_roi_should_sample(2, false, false),
            "unarmed TRACE must not sample"
        );
        assert!(!pixel_roi_should_sample(2, true, true), "already dumped");
        assert!(pixel_roi_should_sample(2, true, false));
    }

    fn cam(cycle: i32, eye_x: i32) -> PixelRoiCam {
        PixelRoiCam {
            cycle,
            eye_x,
            eye_y: -664,
            eye_z: 5924,
            yaw: 0,
            pitch: 128,
            origin_x: 2960,
            origin_z: 9760,
            trace_frame: 1,
        }
    }

    fn image(min_x: f32) -> PixelRoiImage {
        PixelRoiImage {
            logical_min: [min_x, 0.0],
            logical_size: [765.0, 503.0],
            fb_scale: [2.0, 2.0],
            domain: PixelRoiCoordDomain::LogicalImGui,
        }
    }

    #[test]
    fn delayed_readback_keeps_production_cam_when_live_changes() {
        let produced =
            PixelRoiMeta::produced(7, cam(100, 6607), PackedHist::EMPTY, PackedHist::EMPTY);
        let mut live = cam(200, 1);
        attach_upload(
            &mut produced.clone(),
            PackedHist::EMPTY,
            PackedHist::EMPTY,
            0,
            "alice",
            3,
        );
        let bind = bind_shot(produced, image(16.0));
        live.cycle = 999;
        live.eye_x = 0;
        assert_eq!(label_cam(&bind, Some(&live)).cycle, 100);
        assert_eq!(label_cam(&bind, Some(&live)).eye_x, 6607);
        assert_eq!(bind.image.logical_min[0], 16.0);
        assert_eq!(bind.image.domain, PixelRoiCoordDomain::LogicalImGui);
        assert_eq!(
            pixel_roi_proof(true, Some(&bind)),
            PixelRoiProof::Accept { frame_id: 7 }
        );
    }

    #[test]
    fn selection_change_does_not_relabel_enqueued_bind() {
        let mut meta = PixelRoiMeta::produced(
            4,
            cam(10, 1),
            PackedHist {
                n: 1,
                ..PackedHist::EMPTY
            },
            PackedHist::EMPTY,
        );
        attach_upload(
            &mut meta,
            PackedHist {
                n: 1,
                ..PackedHist::EMPTY
            },
            PackedHist::EMPTY,
            0,
            "alice",
            1,
        );
        let bind = bind_shot(meta, image(8.0));
        let later = PixelRoiMeta::produced(9, cam(11, 2), PackedHist::EMPTY, PackedHist::EMPTY);
        assert_eq!(bind.meta.frame_id, 4);
        assert_eq!(
            bind.meta.upload.as_ref().map(|u| u.slot.as_str()),
            Some("alice")
        );
        assert_eq!(bind.meta.upload.as_ref().map(|u| u.generation), Some(1));
        assert_ne!(later.frame_id, bind.meta.frame_id);
        assert_eq!(
            pixel_roi_ids_agree(
                bind.meta.frame_id,
                bind.meta.upload.as_ref().map(|u| u.frame_id)
            ),
            PixelRoiProof::Accept { frame_id: 4 }
        );
        assert_eq!(
            pixel_roi_ids_agree(bind.meta.frame_id, Some(later.frame_id)),
            PixelRoiProof::RejectMismatch {
                reason: "frame-id-mismatch"
            }
        );
    }

    fn slot_pixels(tag: i32) -> (Vec<i32>, Vec<i32>) {
        let game = vec![tag; 512 * 334];
        let mut draw = vec![0i32; 765 * 503];
        for y in 0..334 {
            for x in 0..512 {
                draw[((y + 4) * 765 + (x + 4)) as usize] = tag;
            }
        }
        (game, draw)
    }

    #[test]
    fn later_armed_stamp_is_the_screenshot_proof_not_the_first() {
        let (g1, d1) = slot_pixels(0x020202);
        let (g2, d2) = slot_pixels(0x090707);
        let first = stamp_cpu_slot_pixels(Some((&g1, 512, 334)), Some((&d1, 765, 503)), cam(1, 1))
            .expect("first stamp");
        let later = stamp_cpu_slot_pixels(Some((&g2, 512, 334)), Some((&d2, 765, 503)), cam(2, 2))
            .expect("later stamp");
        assert_ne!(first.frame_id, later.frame_id);
        assert_eq!(first.area_game.rgb2, 66 * 41);
        assert_eq!(later.area_game.palette2, 66 * 41);
        assert_eq!(
            pixel_roi_proof(false, None),
            PixelRoiProof::RejectUnavailable {
                reason: "no-screenshot-request"
            }
        );
        let bind = bind_shot(later.clone(), image(16.0));
        assert_eq!(
            pixel_roi_proof(true, Some(&bind)),
            PixelRoiProof::Accept {
                frame_id: later.frame_id
            }
        );
        assert_ne!(
            pixel_roi_proof(true, Some(&bind)),
            PixelRoiProof::Accept {
                frame_id: first.frame_id
            }
        );
        assert_eq!(
            pixel_roi_proof(true, None),
            PixelRoiProof::RejectUnavailable {
                reason: "no-attached-bind"
            }
        );
    }

    #[test]
    fn tracked_then_untracked_cpu_or_gpu_or_other_slot_rejects_stale_presented() {
        reset_pixel_roi_ui();
        let alice = PixelRoiMeta::produced(
            4,
            cam(10, 1),
            PackedHist {
                n: 8,
                ..PackedHist::EMPTY
            },
            PackedHist::EMPTY,
        );
        prepare_present_roi(Some((alice.clone(), "alice", 1)));
        arm_game_image_cpu_upload();
        complete_cpu_upload(&[0u32; 4], &[0u8; 16], 2, 2);
        note_game_image_fb(16.0, 0.0, 765.0, 503.0, 2.0, 2.0);
        note_readback_context("alice");
        assert_eq!(
            snapshot_shot_bind().expect("tracked alice").meta.frame_id,
            4
        );

        prepare_present_roi(None);
        arm_game_image_cpu_upload();
        complete_cpu_upload(&[1u32; 4], &[1u8; 16], 2, 2);
        note_readback_context("alice");
        assert!(
            snapshot_shot_bind().is_none(),
            "untracked CPU present must drop alice ROI"
        );

        prepare_present_roi(Some((alice.clone(), "alice", 2)));
        arm_game_image_cpu_upload();
        complete_cpu_upload(&[0u32; 4], &[0u8; 16], 2, 2);
        note_readback_context("alice");
        assert!(snapshot_shot_bind().is_some());
        arm_game_image_gpu_present();
        note_gpu_texture_changed();
        note_readback_context("alice");
        assert!(
            snapshot_shot_bind().is_none(),
            "new GPU texture must drop prior CPU ROI"
        );

        prepare_present_roi(Some((alice, "alice", 3)));
        arm_game_image_cpu_upload();
        complete_cpu_upload(&[0u32; 4], &[0u8; 16], 2, 2);
        note_gpu_texture_held();
        note_readback_context("alice");
        assert_eq!(
            snapshot_shot_bind()
                .expect("held GPU/CPU image")
                .meta
                .frame_id,
            4
        );
        note_readback_context("bob");
        assert!(
            snapshot_shot_bind().is_none(),
            "readback context for another slot must not accept alice ROI"
        );
    }

    #[test]
    fn cave_slot_png_roi_is_logical_imgui_times_framebuffer_scale() {
        let (x0, y0, x1, y1) = cave_slot_png_roi(16.0, 0.0, 765.0, 503.0, 2.0, 2.0, 765.0, 503.0);
        let (ax0, ay0, ax1, ay1) = cave_slot_applet();
        assert_eq!(x0, ((16.0 + ax0 as f32) * 2.0) as i32);
        assert_eq!(y0, (ay0 as f32 * 2.0) as i32);
        assert_eq!(x1, ((16.0 + ax1 as f32) * 2.0) as i32);
        assert_eq!(y1, (ay1 as f32 * 2.0) as i32);
        assert_eq!((ax0, ay0, ax1, ay1), (292, 84, 357, 124));
    }

    #[test]
    fn wall2_shade_identity_uses_loc_typecode_not_info_byte() {
        // addLoc: loc id in bits 14.., kind loc via 0x40000000; typecode2 is
        // the signed ((angle << 6) + shape) info byte (cave wall angle 3).
        let typecode = (1417i32 << 14).wrapping_add(0x4000_0000);
        let typecode2 = ((3i32 << 6) + 0).wrapping_shl(24) >> 24;
        assert_eq!(loc_id(typecode), 1417);
        assert_eq!(loc_kind(typecode), 2);
        assert_ne!(loc_id(typecode2), 1417);
        assert_ne!(loc_kind(typecode2), 2);
        let wall2 = wall_shade_typecode(typecode, typecode2);
        assert_eq!(wall2, typecode);
        assert_eq!(loc_id(wall2), 1417);
        assert_eq!(loc_kind(wall2), 2);
        assert_eq!(
            wall_shade_typecode(typecode, typecode2),
            wall_shade_typecode(typecode, 0)
        );
    }
}
