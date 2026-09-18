//! Optional LIVE draw-level render trace.
//!
//! Compiled only with `--features render-diagnostics`. Ordinary release
//! builds must not mention this module. There is no public start/take API,
//! no TLS recorder, and no hot-path `env::var` once the OnceLock config is
//! parsed. Enable at runtime with `BOT_RENDER_TRACE=1` plus a tile region
//! and/or loc IDs; output stops after `BOT_RENDER_TRACE_FRAMES` (max 300).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::dash3d::BuildArea;

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
        (self.base_x + (self.orbit_x >> 7), self.base_z + (self.orbit_z >> 7))
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
        .and_then(|g| g.as_ref().map(|l| (l.cam.base_x + local_x, l.cam.base_z + local_z)))
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
    let overlaps = (wmin_x..=wmax_x).any(|x| (wmin_z..=wmax_z).any(|z| filter.tile_in_region(x, z)));
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
}
