//! Client machine: 1:1 skeleton of `webclient/src/client/Client.ts`.
//!
//! Java-public fields used by login and later `RawClient`-style reads start
//! here (`ingame`, `loop_cycle` as an instance field, `loginUser`, `loginPass`,
//! `out`, `in`, menu arrays). `login` runs the 274 handshake (wrapper opcode
//! 16 cold / 18 reconnect) over Java-style TCP `ClientStream`.
//! There is no snapshot/query API.
//!
//! Headless = `Client` only (task 8): the sim machine (`mainloop`, packet
//! apply, `tryMove`/`doAction`, scene build) never constructs a `Renderer`,
//! a `Present` target, a `RenderBackend`, or a wgpu device, and never
//! decodes a model. `tests/headless.rs` pins the construction counters at
//! zero through a full login + build + sim run.

use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::client::client_build::ClientBuild;
use crate::client::client_draw::get_av_h;
use crate::client::config::ClientConfig;
use crate::client::game_shell::GameShell;
use crate::client::login_error::LoginError;
use crate::client::mini_menu_action::MiniMenuAction;
use crate::client::skill::Skill;
use crate::config::if_type::{
    default_mut, ButtonType, ComponentType, IfType, IfTypeMut, IfTypeOwned, IfTypeView,
};
use crate::config::seq_type::{RESTART_RESET, RESTART_RESETLOOP};
use crate::config::{Cache, ObjType};
use crate::core::world::LevelHeightmaps;
use crate::core::World;
use crate::dash3d::client_player::{recol1d, recol2d};
use crate::dash3d::{
    AnimFrame, BuildArea, ClientEntity, ClientObj, ClientProj, CollisionFlag, CollisionMap,
    DirectionFlag, LocAngle, LocChange, LocLayer, LocShape, MapFlag, MapSpotAnim, Model,
    LOC_SHAPE_TO_LAYER,
};
pub use crate::dash3d::{ClientNpc, ClientPlayer};
use crate::datastruct::LinkList;
use crate::graphics::Pix3D;
use crate::io::{
    ClientProt, ClientStream, Isaac, JagFile, OnDemand, Packet, ServerProt, SERVER_PROT_SIZES,
};
use crate::login_rsa;
use crate::render::nav_debug::NavDebugPaint;
use crate::render::Renderer;
use crate::sound::{Fade, JagFX, Midi};
use crate::util::JString;
use crate::wordfilter::{WordFilter, WordPack};

const MAX_PLAYER_COUNT: usize = 2048;
const MAX_NPC_COUNT: usize = 16384;
const MENU_CAPACITY: usize = 500;
const CLIENT_VERSION: i32 = 274;

/// Client code of the red "Click here to logout" control; `clientButton`
/// arms `logoutTimer` (Java `Client.java` 8746).
const CC_LOGOUT: i32 = 205;

/// Client code of the bank inventory interface; the bank arrange-mode
/// toggle makes obj-drag insert instead of swap (TS `CC_BANKMODE`).
const CC_BANKMODE: i32 = 206;

// Friend/ignore client-code ranges, verbatim `ClientCode.ts`.
const CC_FRIENDS_START: i32 = 1;
const CC_FRIENDS_END: i32 = 100;
const CC_FRIENDS_UPDATE_START: i32 = 101;
const CC_FRIENDS_UPDATE_END: i32 = 200;
const CC_ADD_FRIEND: i32 = 201;
const CC_DEL_FRIEND: i32 = 202;
const CC_FRIENDS_SIZE: i32 = 203;
const CC_FRIENDS2_START: i32 = 701;
const CC_FRIENDS2_END: i32 = 800;
const CC_FRIENDS2_UPDATE_START: i32 = 801;
const CC_FRIENDS2_UPDATE_END: i32 = 900;
const CC_IGNORES_START: i32 = 401;
const CC_IGNORES_END: i32 = 500;
const CC_ADD_IGNORE: i32 = 501;
const CC_DEL_IGNORE: i32 = 502;
const CC_IGNORES_SIZE: i32 = 503;

// Player-design client codes, verbatim `ClientCode.ts` (300-327).
const CC_CHANGE_HEAD_L: i32 = 300;
const CC_CHANGE_FEET_R: i32 = 313;
const CC_RECOLOUR_HAIR_L: i32 = 314;
const CC_RECOLOUR_SKIN_R: i32 = 323;
const CC_SWITCH_TO_MALE: i32 = 324;
const CC_SWITCH_TO_FEMALE: i32 = 325;
const CC_ACCEPT_DESIGN: i32 = 326;
const CC_DESIGN_PREVIEW: i32 = 327;

/// `combatColourCode` from Client.ts (9868-9897): the @-colour prefix for
/// the level-relative combat level shown on npc/player menu options.
fn combat_colour_code(viewer_level: i32, other_level: i32) -> &'static str {
    let diff = viewer_level - other_level;
    if diff < -9 {
        "@red@"
    } else if diff < -6 {
        "@or3@"
    } else if diff < -3 {
        "@or2@"
    } else if diff < 0 {
        "@or1@"
    } else if diff > 9 {
        "@gre@"
    } else if diff > 6 {
        "@gr3@"
    } else if diff > 3 {
        "@gr2@"
    } else if diff > 0 {
        "@gr1@"
    } else {
        "@yel@"
    }
}

/// `OP_LOC1..5`/`OP_NPC1..5`/`OP_OBJ1..5`/`OP_PLAYER1..5` by zero-based op
/// index. The ids are not consecutive, so the TS if/else chain becomes a
/// lookup (TS 9289-9298, 9485-9494, 9623-9632, and the obj/player arms).
const LOC_OP_ACTIONS: [i32; 5] = [
    MiniMenuAction::OP_LOC1,
    MiniMenuAction::OP_LOC2,
    MiniMenuAction::OP_LOC3,
    MiniMenuAction::OP_LOC4,
    MiniMenuAction::OP_LOC5,
];
const NPC_OP_ACTIONS: [i32; 5] = [
    MiniMenuAction::OP_NPC1,
    MiniMenuAction::OP_NPC2,
    MiniMenuAction::OP_NPC3,
    MiniMenuAction::OP_NPC4,
    MiniMenuAction::OP_NPC5,
];
const OBJ_OP_ACTIONS: [i32; 5] = [
    MiniMenuAction::OP_OBJ1,
    MiniMenuAction::OP_OBJ2,
    MiniMenuAction::OP_OBJ3,
    MiniMenuAction::OP_OBJ4,
    MiniMenuAction::OP_OBJ5,
];
const PLAYER_OP_ACTIONS: [i32; 5] = [
    MiniMenuAction::OP_PLAYER1,
    MiniMenuAction::OP_PLAYER2,
    MiniMenuAction::OP_PLAYER3,
    MiniMenuAction::OP_PLAYER4,
    MiniMenuAction::OP_PLAYER5,
];

/// Client code of the Report abuse button; `chatModeLoop` records
/// `main_modal_id` from the first interface with this code (TS
/// `ClientCode.CC_REPORT_INPUT`).
const CC_REPORT_INPUT: i32 = 600;

/// Index of the local player in `players` (`Client.ts` `LOCAL_PLAYER_INDEX`);
/// `game_draw_main`'s `addPlayers` uses it for the local-player typecode.
pub(crate) const LOCAL_PLAYER_INDEX: i32 = 2047;

/// `PlayerUpdate` mask enum from client-ts `dash3d/ClientPlayer.ts`.
mod player_update {
    pub const APPEARANCE: i32 = 0x1;
    pub const ANIM: i32 = 0x2;
    pub const FACEENTITY: i32 = 0x4;
    pub const SAY: i32 = 0x8;
    pub const HITMARK: i32 = 0x10;
    pub const FACESQUARE: i32 = 0x20;
    pub const CHAT: i32 = 0x40;
    pub const BIG_UPDATE: i32 = 0x80;
    pub const SPOTANIM: i32 = 0x100;
    pub const EXACTMOVE: i32 = 0x200;
    pub const HITMARK2: i32 = 0x400;
}

/// `NpcUpdate` mask enum from client-ts `dash3d/ClientNpc.ts`.
mod npc_update {
    pub const HITMARK2: i32 = 0x1;
    pub const ANIM: i32 = 0x2;
    pub const FACEENTITY: i32 = 0x4;
    pub const SAY: i32 = 0x8;
    pub const HITMARK: i32 = 0x10;
    pub const CHANGETYPE: i32 = 0x20;
    pub const SPOTANIM: i32 = 0x40;
    pub const FACESQUARE: i32 = 0x80;
}

/// Side of the build area, `BuildArea.SIZE` (13 << 3) in client-ts.
const BUILD_AREA_SIZE: i32 = 104;
const BUILD_AREA_TILES: usize = (BUILD_AREA_SIZE * BUILD_AREA_SIZE) as usize;
const ROUTE_BUFFER: usize = 4000;

/// JAG archives whose CRC values go out in the login wrapper; slot 0 of the
/// 9-slot `getJagChecksums` layout has no pack file and stays 0.
const JAG_FILES: [&str; 8] = [
    "title",
    "config",
    "interface",
    "media",
    "versionlist",
    "textures",
    "wordenc",
    "sounds",
];

/// The `Midi` backend for a fresh client: rustysynth behind `audio`, headless
/// `NullMidi` otherwise. Shared with the audio-output thread, which renders
/// through the same instance (`Midi::render`).
fn midi_backend(cache_dir: &str) -> Arc<Mutex<dyn Midi>> {
    #[cfg(feature = "audio")]
    {
        Arc::new(Mutex::new(crate::sound::RustyMidi::new(cache_dir)))
    }
    #[cfg(not(feature = "audio"))]
    {
        let _ = cache_dir;
        Arc::new(Mutex::new(crate::sound::NullMidi))
    }
}

type GroundObjCell = Option<Box<LinkList<ClientObj>>>;
type GroundObjGrid = Box<[[[GroundObjCell; 104]; 104]; 4]>;
type LoadedCache = (Cache, Vec<Option<Box<IfType>>>, Vec<Option<Arc<IfTypeMut>>>);
type ProgressCb<'a> = Option<&'a mut dyn FnMut(&mut Client, &str, i32)>;

/// `groundObj` grid from client-ts (`new Array(4)` of `new Array(104)` of
/// null rows), every cell `None`. Empty cells are a fat pointer, not an
/// inline `LinkList` (that was 3.8 MB × N clients). Assembled through
/// `Vec` because the `array::from_fn` / const-repeat forms materialize the
/// grid in a stack temporary, which overflows the 2 MB test-thread stack.
fn empty_ground_obj() -> GroundObjGrid {
    let mut rows: Vec<[GroundObjCell; 104]> = Vec::with_capacity(104 * 4);
    for _ in 0..104 * 4 {
        rows.push([const { None }; 104]);
    }
    // Length-checked box of 416 rows, then re-grouped as 4 levels of 104
    // rows each. `[[T; 104]; 416]` and `[[[T; 104]; 104]; 4]` have the same
    // size and alignment, so the sole-owner allocation can be re-typed
    // without copying.
    let boxed: Box<[[GroundObjCell; 104]; 416]> =
        rows.into_boxed_slice().try_into().map_err(|_| ()).unwrap();
    // SAFETY: the box holds exactly 416 row arrays, which is the same
    // memory (size, alignment, cell layout) as 4 levels of 104 rows; the
    // re-typed box keeps sole ownership and its drop glue walks the same
    // cells.
    unsafe { Box::from_raw(Box::into_raw(boxed) as *mut [[[GroundObjCell; 104]; 104]; 4]) }
}

#[cfg(test)]
mod ground_obj_size_tests {
    use super::empty_ground_obj;

    #[test]
    fn empty_ground_obj_grid_is_under_400kb() {
        let g = empty_ground_obj();
        let bytes = std::mem::size_of_val(&*g);
        assert!(
            bytes < 400_000,
            "empty ground-obj cells must be fat-pointers, not inline LinkLists; got {bytes} B"
        );
    }
}

/// Period 274 applet size (engine `bot.html` canvas and title.dat).
/// The title JPEG / in-game chrome are 765×503; 789×532 was leftover
/// webclient padding around that art.
pub const APPLET_W: i32 = 765;
pub const APPLET_H: i32 = 503;

/// Packet-family generations: monotonic counters the host reads to know
/// which world slices changed since its last poll. `handle_packet` bumps one
/// family per applied packet; `REBUILD_NORMAL` and `logout()` (T1/T2/LOGOUT)
/// bump all.
#[derive(Default, Clone, Copy)]
pub struct ClientGens {
    pub npc: u64,
    pub player: u64,
    pub inv: u64,
    pub varp: u64,
    pub stat: u64,
    pub chat: u64,
    pub scene: u64,
    pub iface: u64,
    pub camera: u64,
    pub map_flag: u64,
    pub world: u64,
}

pub struct Client {
    pub shell: GameShell,
    /// The frame target the driver attaches (task 6 `PresentTarget`). `run`
    /// polls it for events each frame and hands it the rendered frame after
    /// the redraw; a closed window (`poll` false) sets `shell.state = -1` to
    /// stop the machine. Headless builds keep this `None`; a `Textures`
    /// host attaches a target to receive frames without a window.
    pub present: Option<Box<dyn crate::client::present::PresentTarget>>,
    pub config: ClientConfig,
    /// Config type tables (`obj`, `npc`, `loc`, ...), unpacked from the
    /// `config` jag by `Cache::unpack`; empty until loaded. Shared with
    /// every client via `Arc` (the tables are immutable once unpacked);
    /// the per-client mutable state lives in `ifaces_mut`.
    pub cache: Arc<Cache>,
    /// Interface components (`IfType` decode: type, scripts, children,
    /// names, colours, model ids), unpacked from the `interface` jag.
    /// Shared by every client via `Arc` — the host unpacks once per
    /// `cache_dir` and fifty slots point at the same immutable table.
    pub ifaces: Arc<Vec<Option<Box<IfType>>>>,
    /// The per-client mutable overlay: one small dense `IfTypeMut` per
    /// slot (hide, scroll, anim frames, inv slot arrays, live text,
    /// `IF_SET*` writes). Host construct shares one template `Arc` across
    /// slots; a write COWs only its one slot, so unwritten slots stay on
    /// the shared template.
    pub ifaces_mut: Arc<Vec<Option<Arc<IfTypeMut>>>>,
    /// True when the cache was injected already-unpacked via `from_shared`
    /// (host-side `load_cache`). `maininit` skips its `load_cache` re-unpack
    /// so the shared `Arc<Cache>` survives; `Client::new` stays false and
    /// still re-unpacks after its fresh jag fetch.
    pub cache_from_shared: bool,

    pub ingame: bool,
    /// `draw`: CPU-save switch — when false, `mainredraw` skips the frame
    /// render. Independent of the window: `client-play` sets it true after
    /// `WindowTarget::open`; headless bots keep it false.
    pub draw: bool,
    /// The nav-debug paint the host publishes (`set_nav_debug_paint`);
    /// drawn by the wgpu scene stage after the 3D world. `None` by
    /// default — a skip-paint slot or a CPU-only client never paints, but
    /// the store is always accepted.
    pub nav_debug_paint: Option<NavDebugPaint>,
    pub scene_state: i32,
    /// `inMultizone` from client-ts (TS 132): set by `SET_MULTIWAY`.
    pub in_multizone: i32,
    /// `Client.buildMinusedlevel` from client-ts: the `minusedlevel` the
    /// current scene was built for. `check_minimap`'s low-memory rebuild
    /// compares it against `self.minusedlevel`.
    pub build_minusedlevel: i32,
    pub local_player: Option<ClientPlayer>,
    /// Boxed so 2048 empty slots are 16 KB, not ~2 MB of `ClientPlayer`.
    pub players: Vec<Option<Box<ClientPlayer>>>,
    /// Boxed so 16384 empty slots are 128 KB, not ~6 MB of `ClientNpc`.
    pub npc: Vec<Option<Box<ClientNpc>>>,

    /// `PLAYER_INFO`/`NPC_INFO` bookkeeping from client-ts: `playerCount`/
    /// `playerIds`, `npcCount`/`npcIds`, the removal/update id lists, and
    /// the raw appearance blocks re-applied when a player re-enters.
    pub player_count: i32,
    pub player_ids: Vec<i32>,
    pub npc_count: i32,
    pub npc_ids: Vec<i32>,
    pub entity_removal_count: i32,
    pub entity_removal_ids: Vec<i32>,
    pub entity_update_count: i32,
    pub entity_update_ids: Vec<i32>,
    pub player_appearance_buffer: Vec<Option<Packet>>,

    /// Scene height map, `groundh[level][x][z]` sized `[4][105][105]`; owned
    /// here and mirrored into `world` after each `map_build` load/fade pass
    /// (Java shares the one array; `ClientBuild` owns the write side).
    pub groundh: LevelHeightmaps,
    /// Map-land flags `mapl[level][x][z]` (`MapFlag` bits), sized
    /// `[BuildArea.LEVELS][BuildArea.SIZE][BuildArea.SIZE]`; written by
    /// `ClientBuild::load_ground`, read by `get_av_h` (`LinkBelow` lands in
    /// Task 10) and the minimap (Task 7).
    pub mapl: Vec<Vec<Vec<u8>>>,
    pub world: World,
    /// One collision grid per level, `CollisionMap` for the 4 build levels.
    pub collision: [CollisionMap; 4],
    pub minusedlevel: i32,
    pub zone_update_x: i32,
    pub zone_update_z: i32,
    /// `groundObj` from client-ts: per-level tile grid of loc-change
    /// object lists, sized `[LEVELS][SIZE][SIZE]`, every cell initially
    /// `None`. Populated by the zone task.
    pub ground_obj: GroundObjGrid,
    pub loc_changes: LinkList<LocChange>,
    pub projectiles: LinkList<ClientProj>,
    pub spotanims: LinkList<MapSpotAnim>,
    /// `selfSlot`/`membersAccount` from `UPDATE_PID`; `worldUpdateNum`
    /// counts `gameLoop` passes while drawing (zeroed when not drawing and
    /// at the end of `game_draw`). `cycleLogic1` (the loc-change scene
    /// cycle counter) lives on `Renderer`. Slice 2.
    pub self_slot: i32,
    pub members_account: i32,
    pub world_update_num: i32,

    pub stat_base_level: Vec<i32>,
    pub stat_effective_level: Vec<i32>,
    pub stat_xp: Vec<i32>,
    pub var: Vec<i32>,
    /// Server-authoritative var values (`varServ` from client-ts); `var`
    /// follows them once `VARP_SYNC` confirms.
    pub var_serv: Vec<i32>,
    pub runenergy: i32,
    /// `runweight` from client-ts (143): the carried-weight stat, sent by
    /// `UPDATE_RUNWEIGHT` as a signed g2.
    pub runweight: i32,

    /// Music control plane (`Client.ts` `midiActive`/`midiSong`/...; Java
    /// `midivol` ladder 0 / -400 / -800 / -1200). `midi` is the backend
    /// (`NullMidi` headless, `RustyMidi` behind `audio`).
    pub midi_active: bool,
    pub midi_song: i32,
    pub next_midi_song: i32,
    pub next_music_delay: i32,
    pub midi_fading: bool,
    pub midi_volume: i32,
    pub midi: Arc<Mutex<dyn Midi>>,
    /// A zone-song change held by `saveMidi(fading=true)`: `(bytes, midivol)`
    /// played by `music_tick` once the current song has faded out to the
    /// floor.
    pub midi_pending: Option<(Vec<u8>, i32)>,
    /// True once the first song/jingle has been handed to the backend; the
    /// first song plays immediately (Java first-song short-circuit), later
    /// zone-song changes fade the current song out first.
    pub midi_playing: bool,

    /// The period fade and the JagFX wave queue, shared with the audio
    /// output thread. `saveMidi`/`stopMidi`/`setMidiVolume` arm the fade;
    /// `AudioOut` steps it from the device clock and drains the queue.
    pub fade: Arc<Mutex<Fade>>,
    pub waves: Arc<Mutex<Vec<i16>>>,

    /// `SYNTH_SOUND` queue (`Client.ts` `waveEnabled`/`waveIds`/...).
    pub wave_enabled: bool,
    /// `waveVolume` from client-ts (clientcode 4): 0/-400/-800/-1200; 4 mutes.
    pub wave_volume: i32,
    /// `chatEffects` from client-ts (clientcode 6).
    pub chat_effects: i32,
    pub wave_count: i32,
    pub wave_ids: Vec<i32>,
    pub wave_loops: Vec<i32>,
    pub wave_delay: Vec<i32>,
    /// The `JagFX` table (`sounds.dat` init lands with the loading task).
    pub jagfx: JagFX,

    pub menu_num_entries: i32,
    pub menu_option: Vec<String>,
    pub menu_action: Vec<i32>,
    pub menu_param_a: Vec<i32>,
    pub menu_param_b: Vec<i32>,
    pub menu_param_c: Vec<i32>,
    /// Minimenu chrome (`Client.ts` `isMenuOpen`/`menuArea`/`menuX`/...,
    /// 444-450): `open_menu` opens the menu in the panel holding the click
    /// (`menu_area` 0 viewport, 1 side, 2 chat) and clamps its geometry
    /// into that panel; `draw_minimenu` renders it there.
    pub is_menu_open: bool,
    pub menu_area: i32,
    pub menu_x: i32,
    pub menu_y: i32,
    pub menu_width: i32,
    pub menu_height: i32,
    /// `objSelectedName`/`targetOp`/`targetMask` (TS 440/447/449): the
    /// Use/Target hint text and the spell target mask for `draw_feedback`;
    /// set by the `USEHELD_START`/`TGT_*` doAction arms (Task 3).
    pub obj_selected_name: String,
    pub target_op: String,
    pub target_mask: i32,
    /// `crossX`/`crossY`/`crossMode`/`crossCycle` (TS 373-376): the
    /// crosshair position for the last op click, set by the `doAction` /
    /// `interactWithLoc` arms (mode 2) and the walk consume (mode 1). The
    /// 8 sprite frames plot here live on `Renderer`.
    pub cross_x: i32,
    pub cross_y: i32,
    pub cross_mode: i32,
    pub cross_cycle: i32,
    /// Anticheat oplogic counters (Java `Client.java` static fields),
    /// accumulated inside the `doAction` arms and flushed as the
    /// `ANTICHEAT_OPLOGIC*` packets when they pass their thresholds.
    pub oplogic1: i32,
    pub oplogic2: i32,
    pub oplogic3: i32,
    pub oplogic4: i32,
    pub oplogic5: i32,
    pub oplogic6: i32,
    pub oplogic7: i32,
    pub oplogic8: i32,
    pub oplogic9: i32,
    pub cyclelogic2: i32,
    /// `Client.cyclelogic6` from client-ts (a TS static, instance here):
    /// anticheat counter sent with `ANTICHEAT_CYCLELOGIC6` from
    /// `addPlayers` when the dest flag is cleared on arrival.
    pub cyclelogic6: i32,
    /// `reportAbuseInput`/`reportAbuseMuteOption`/`reportAbuseComId` (TS):
    /// the report-abuse form state set by the `ABUSE_REPORT` doAction arm.
    pub report_abuse_input: String,
    pub report_abuse_mute_option: bool,
    pub report_abuse_com_id: i32,

    pub dir_map: Vec<i32>,
    pub dist_map: Vec<i32>,
    pub route_x: Vec<i32>,
    pub route_z: Vec<i32>,
    /// Every scene tile of the last successful `tryMove` BFS (src → dest).
    /// The `route_x`/`route_z` lists below that are only direction-change
    /// waypoints for the MOVE packet (capped at 25). Nav debug paints this
    /// full path; it is empty until the first accepted click.
    pub try_move_path: Vec<(i32, i32)>,
    pub try_move_nearest: i32,
    pub minimap_flag_x: i32,
    pub minimap_flag_z: i32,
    pub map_build_base_x: i32,
    pub map_build_base_z: i32,
    pub map_build_centre_zone_x: i32,
    pub map_build_centre_zone_z: i32,
    pub map_build_prev_base_x: i32,
    pub map_build_prev_base_z: i32,
    pub within_tutorial_island: bool,
    pub awaiting_player_info: bool,
    pub scene_load_start_time: Instant,
    pub map_build_index: Vec<i32>,
    pub map_build_ground_file: Vec<i32>,
    pub map_build_location_file: Vec<i32>,
    pub map_build_ground_data: Vec<Option<Vec<u8>>>,
    pub map_build_location_data: Vec<Option<Vec<u8>>>,

    pub use_mode: i32,
    pub target_mode: i32,
    /// `oneMouseButton` (Java `Client.java` 808): the one-button-mouse
    /// option (`WINDOW_STATUS`), 0 off 1 on — a left click on a multi-entry
    /// menu opens it instead of firing the last entry (TS 8370-8372).
    pub one_mouse_button: i32,
    /// `playerOp`/`playerOpPriority` (TS 412-413): the five right-click
    /// options on other players, set by `SET_PLAYER_OP` and consumed by
    /// `add_player_options`.
    pub player_op: [Option<String>; 5],
    pub player_op_priority: [bool; 5],
    pub redraw_side: bool,
    /// Side-tab state (`Client.ts` `sideIcon`/`activeIcon`): `side_icon[icon]`
    /// is the interface id drawn on tab `icon` (-1 hidden), `active_icon` the
    /// selected tab. The modal ids are the open side/chat interfaces (-1 none);
    /// `IF_OPENSIDE`/`IF_CLOSE` populate them in the HUD task.
    pub active_icon: i32,
    pub side_icon: [i32; 14],
    pub side_modal_id: i32,
    pub chat_modal_id: i32,
    /// The open main modal (Java `mainModalId`), set by the report-abuse
    /// button in `chat_mode_loop` and cleared by `close_modal` (-1 none).
    pub main_modal_id: i32,
    /// `mainOverlayId` (TS): the interface drawn above the game view,
    /// written by `IF_OPENOVERLAY` (-1 none).
    pub main_overlay_id: i32,
    /// `tutComId` (TS): the tutorial chat interface, set by `TUT_OPEN`
    /// (-1 none).
    pub tut_com_id: i32,
    /// `tutFlashIcon` (TS): the flashing tutorial side tab, set by
    /// `TUT_FLASH` (-1 none).
    pub tut_flash_icon: i32,
    /// `dialogInputOpen` (TS): the chat-mode enter-name dialog is up.
    pub dialog_input_open: bool,
    /// `dialogInput` (TS 138): the enter-name amount typed into the
    /// `P_COUNTDIALOG` prompt, sent back with `RESUME_P_COUNTDIALOG`.
    pub dialog_input: String,
    /// `resumedPauseButton` (TS): the pause button latched since the last
    /// modal transition.
    pub resumed_pause_button: bool,
    /// `overMainComId`/`overSideComId`/`overChatComId` (TS 453-456): the
    /// component under the pointer in each region, from the `update_if_pointer`
    /// walk (TS `buildMinimenu` 2524-2566). Defaults 0 like TS — 0 means
    /// nothing hovered; a hidden layer still draws while its id matches.
    pub over_main_com_id: i32,
    pub over_side_com_id: i32,
    pub over_chat_com_id: i32,
    /// `lastOverComId` (TS 453): the walk's running value, reset between the
    /// main/side/chat regions.
    pub last_over_com_id: i32,
    /// `hoveredSlot`/`hoveredSlotComId` (TS 385/389): the TYPE_INV slot under
    /// the pointer — set even on empty slots (the Task 8 drop target).
    pub hovered_slot: i32,
    pub hovered_slot_com_id: i32,
    pub target_com_id: i32,
    pub obj_com_id: i32,
    pub obj_selected_slot: i32,
    pub obj_selected_com_id: i32,
    /// `selectedArea`/`selectedComId`/`selectedItem`/`selectedCycle`
    /// (TS 378-381): the last OP_HELD slot and its 15-cycle outline
    /// timeout, cleared in `game_loop`.
    pub selected_area: i32,
    pub selected_com_id: i32,
    pub selected_item: i32,
    pub selected_cycle: i32,
    /// `objDragArea`/`objDragComId`/`objDragSlot`/`objGrabX`/`objGrabY`/
    /// `objGrabThreshold`/`objDragCycles` (TS 383-391): the in-flight
    /// inventory drag grabbed from a TYPE_INV click. `obj_drag_area` is
    /// 1 main modal, 2 side panel, 3 chat modal, 0 none.
    pub obj_drag_area: i32,
    pub obj_drag_com_id: i32,
    pub obj_drag_slot: i32,
    pub obj_grab_x: i32,
    pub obj_grab_y: i32,
    pub obj_grab_threshold: bool,
    pub obj_drag_cycles: i32,
    /// `bankArrangeMode` (TS): the bank's arrange-mode toggle; when 1,
    /// drops into a bank interface insert instead of swapping.
    pub bank_arrange_mode: i32,

    pub out: Packet,
    pub r#in: Packet,
    pub ptype: i32,
    pub ptype0: i32,
    pub ptype1: i32,
    pub ptype2: i32,
    pub psize: i32,

    pub stream: Option<ClientStream>,
    /// OnDemand worker (TS `onDemand`); `None` without a `versionlist` pack
    /// in the cache dir, which behaves like TS `onDemand === null`.
    pub on_demand: Option<OnDemand>,
    pub staffmodlevel: i32,
    pub mouse_tracked: bool,
    pub random_in: Option<Isaac>,
    pub jag_checksum: [i32; 9],

    /// Per-client login uid, sent in the 274 handshake RSA block. `new`
    /// fills it with a random non-zero i32 (`login_uid()`); the host may
    /// overwrite it with a profile uid before `login`.
    pub login_uid: i32,
    pub login_user: String,
    pub login_pass: String,
    pub login_mes1: String,
    pub login_mes2: String,
    pub loop_cycle: i32,

    /// `lastProgressPercent`/`lastProgressMessage` from client-ts (144-145):
    /// the most recent `draw_progress` values, readable even headless.
    /// `http_port` is the web-origin port of the later HTTP jag fetch (TS
    /// `getJagChecksums` downloads `/crc` from it); default 80, tests that
    /// stub HTTP set it, and `client-play --http-port` overrides it for a
    /// non-privileged local web server.
    pub last_progress_percent: i32,
    pub last_progress_message: String,
    pub http_port: u16,
    /// `alreadyStarted` from client-ts: set at the start of `maininit`;
    /// a second `maininit` call is a no-op.
    pub already_started: bool,
    /// Base wait between `maininit` HTTP retries (TS `getJagChecksums`/
    /// `getJagFile` start at 5 s and double to a 60 s cap), spent as one
    /// countdown tick per second (`retry_countdown`). Tests that stub HTTP
    /// set it small so retry paths do not sleep.
    pub fetch_retry_wait: Duration,

    /// Render-adjacent bridges to the separate `Renderer` (task 2b): the
    /// 3D pick list `game_draw_main` mirrors onto `Client` so the sim's
    /// `build_minimenu`/`add_world_options` can read it without a renderer
    /// handle, the brightness change `apply_clientcode` defers for the
    /// renderer to gamma-correct the texture palettes at the next draw, and
    /// the scrollbar input state (`chat_interface`/`scroll_grabbed`/
    /// `scroll_input_padding`/`scroll_cycle`) that the sim's menu walk
    /// (`add_component_options` → `do_scrollbar`) reads/writes.
    pub pick_count: i32,
    pub pick_typecodes: Vec<i32>,
    pub pending_brightness: Option<f64>,
    /// Shared texture-average cache: `finish_build` (sim, scene build)
    /// reads the per-texture average brightness for the ground overlays;
    /// the renderer refreshes it whenever it re-gammas the texture palettes
    /// (`prepare_game`, brightness changes). Zeroed before any renderer ran,
    /// which matches `Pix3DDraw::get_texture_average` for missing palettes.
    pub tex_average: [i32; 50],
    /// `chatInterface` from client-ts (480): the synthetic IfType the chat
    /// scrollbar reads/writes (not in the jag), synced to the chat scroll
    /// state by `game_draw`/`draw_chat`.
    pub chat_interface: IfTypeMut,
    /// Scrollbar input state (`scrollGrabbed`/`scrollInputPadding`/
    /// `scrollCycle` from client-ts 338-340): `scroll_grabbed` widens the
    /// track hit area to 32 px while held, and `scroll_cycle` is the
    /// mouse-held repeat (set from `shell.mouse_button` at the top of
    /// `game_draw`, since the TS GameShell already ticks it).
    pub scroll_grabbed: bool,
    pub scroll_input_padding: i32,
    pub scroll_cycle: i32,
    /// `loginscreen` from client-ts (1378-1416): the title-screen state
    /// (0 = login form, 2 = invalid, 3 = connecting); written by
    /// `title_screen_loop`/`fail_title_login`, read by `title_screen_draw`.
    pub loginscreen: i32,
    /// `loginSelect` from client-ts: the focused login field (0 user,
    /// 1 pass).
    pub login_select: i32,
    /// `redrawFrame` from client-ts: the title frame redraw latch.
    pub redraw_frame: bool,

    /// Camera state (`Client.ts` `gameDrawMain`/`camFollow`/`followCamera`,
    /// 3222-4465): the orbit camera the 3D pass follows and the per-frame
    /// eye it produces. `orbit_camera_pitch/yaw/x/z`, the two velocity
    /// fields, `camera_pitch_clamp`, and `macro_camera_angle` default as
    /// the TS field initializers; `macro_camera_x/z` exist for
    /// `followCamera` but stay 0 (the TS random-drift block is not ported);
    /// `cam_*` holds the `camFollow` result.
    pub cam_x: i32,
    pub cam_y: i32,
    pub cam_z: i32,
    pub cam_pitch: i32,
    pub cam_yaw: i32,
    /// Cutscene camera from client-ts (`cinemaCam`/`camLookAt*`/
    /// `camMoveTo*` 132-146, `camShake*` 147-156): the CAM_* packets drive
    /// `cinema_camera()`, and `cam_shake_*` jitter the rendered eye each
    /// frame. The payload mapping keeps the TS field names even though it
    /// is shifted: `cam_shake_axis` holds the packet's `ran` byte,
    /// `cam_shake_ran` its `amp` byte and `cam_shake_amp` its `rate` byte.
    /// The `rand` source the jitter uses lives on `Renderer`.
    pub cinema_cam: bool,
    pub cam_look_at_lx: i32,
    pub cam_look_at_lz: i32,
    pub cam_look_at_hei: i32,
    pub cam_look_at_rate: i32,
    pub cam_look_at_rate2: i32,
    pub cam_move_to_lx: i32,
    pub cam_move_to_lz: i32,
    pub cam_move_to_hei: i32,
    pub cam_move_to_rate: i32,
    pub cam_move_to_rate2: i32,
    pub cam_shake: [bool; 5],
    pub cam_shake_axis: [i32; 5],
    pub cam_shake_ran: [i32; 5],
    pub cam_shake_amp: [i32; 5],
    pub cam_shake_cycle: [i32; 5],
    pub orbit_camera_pitch: i32,
    pub orbit_camera_yaw: i32,
    pub orbit_camera_yaw_velocity: i32,
    pub orbit_camera_pitch_velocity: i32,
    pub orbit_camera_x: i32,
    pub orbit_camera_z: i32,
    pub macro_camera_x: i32,
    pub macro_camera_z: i32,
    pub camera_pitch_clamp: i32,
    pub macro_camera_angle: i32,
    pub redraw_icons: bool,
    pub redraw_chat: bool,
    pub redraw_chat_mode: bool,
    /// Minimap state kept on `Client` because the sim reads/writes it:
    /// `minimap_state` (the click gate, set by `SET_MINIMAP_STATE`),
    /// `minimap_level` (the level the minimap buffer was built for, reset
    /// by `login`/`map_build`), and the macro angle/zoom read by
    /// `minimap_loop`. The minimap buffers/sprites live on `Renderer`.
    pub minimap_state: i32,
    pub minimap_level: i32,
    pub macro_minimap_angle: i32,
    pub macro_minimap_zoom: i32,
    /// The hint fields (TS 161-165) gate the `minimapDrawArrow` branch
    /// (`hintType` 0 → skipped).
    pub hint_type: i32,
    pub hint_npc: i32,
    pub hint_player: i32,
    pub hint_tile_x: i32,
    pub hint_tile_z: i32,
    /// `hintHeight`/`hintOffsetX`/`hintOffsetZ` from client-ts (161-165):
    /// the tile-hint arrow's height above the tile and the per-type tile
    /// corner offset, read by `HINT_ARROW`.
    pub hint_height: i32,
    pub hint_offset_x: i32,
    pub hint_offset_z: i32,
    pub chat_public_mode: i32,
    pub chat_private_mode: i32,
    pub chat_trade_mode: i32,
    /// Chat history (`Client.ts` `chatType`/`chatUsername`/`chatText`, 100
    /// slots) and the input line. `chat_scroll_height` defaults to 78 and is
    /// recomputed by `draw_chat` as TS does.
    pub chat_type: [i32; 100],
    pub chat_username: [String; 100],
    pub chat_text: [String; 100],
    pub chat_input: String,
    pub chat_scroll_pos: i32,
    pub chat_scroll_height: i32,
    /// Stable per-message id: bumped once per `add_chat` append so hosts can
    /// refer to a chat line without snapshot-owned counters.
    pub chat_seq: u64,
    /// Ignore list and `chatDisabled` from client-ts (`ignoreCount`/
    /// `ignoreUserhash[100]`/`chatDisabled`). The CHAT mask reads them for
    /// the `type <= 1` skip; the list itself is filled by the social slice.
    pub ignore_count: i32,
    pub ignore_userhash: [i64; 100],
    pub chat_disabled: i32,
    /// Friend list and PM state from client-ts (`friendCount`/
    /// `friendUserhash[200]`/`friendUsername[200]`/`friendNodeId[200]`/
    /// `friendServerStatus`/`privateMessageIds[100]`/`privateMessageCount`)
    /// plus the local `nodeId` (static in TS, default 10) the friend sort
    /// compares against.
    pub friend_count: i32,
    pub friend_userhash: [i64; 200],
    pub friend_username: [String; 200],
    pub friend_node_id: [i32; 200],
    pub friend_server_status: i32,
    pub private_message_ids: [i32; 100],
    pub private_message_count: i32,
    pub node_id: i32,
    /// Social enter-name dialog from client-ts (`socialInputOpen`/
    /// `socialInput`/`socialInputType` 1-5/`socialInputHeader`/
    /// `socialUserhash`): the add/del friend (1/2), PM (3) and add/del
    /// ignore (4/5) prompts. `splitPrivateChat` (clientcode 8) splits the
    /// PM overlay out of the main chat area.
    pub social_input_open: bool,
    pub social_input: String,
    pub social_input_type: i32,
    pub social_input_header: String,
    pub social_userhash: i64,
    pub split_private_chat: i32,
    /// Player-design (300-327) state from client-ts 559-564: the gender
    /// flag, the preview redraw latch, and the kit/colour selection.
    /// `idk_design_part` inits at -1 (the brief's choice over TS's
    /// zero-filled `Int32Array`, so an empty idk table never indexes kit 0);
    /// `idk_design_button1/2` snapshot the male/female switch-button
    /// graphics (TS 10822-10836).
    pub idk_design_gender: bool,
    pub idk_design_redraw: bool,
    pub idk_design_part: [i32; 7],
    pub idk_design_colour: [i32; 5],
    pub idk_design_button1: Option<String>,
    pub idk_design_button2: Option<String>,

    /// Reconnect flag of the most recent `login` call (`None` until the
    /// first login). `lostCon` reestablishes with `reconnect = true`
    /// (wrapper opcode 18); the flag is how the reconnect path is observed.
    pub last_login_reconnect: Option<bool>,
    /// Socket-adopt flag: when true, the next `login(reconnect = true)`
    /// reuses `stream` (`Client::adopt_from`) instead of opening a new TCP
    /// — the opcode-18 handshake runs in place so a channel-head tune swaps
    /// net+sim without a TCP drop. Cleared at the start of that login.
    pub baton: bool,
    /// `logoutTimer` from Java: frames remaining until a requested logout.
    pub logout_timer: i32,
    /// `rebootTimer` from client-ts (140): seconds until the server reboot,
    /// sent by `UPDATE_REBOOT_TIMER` scaled by 30 (the `gameLoop` tick
    /// counts it down in the overlay).
    pub reboot_timer: i32,
    /// Wall-clock dead-server watchdog: `Instant` of the last full in-game
    /// packet (or login grant). `gameLoop` calls `lostCon` once the stamp
    /// is older than [`SERVER_TIMEOUT`] — elapsed time, not a pass count,
    /// so a parked host slot (one `gameLoop` pass per ~600 ms, not 20 ms)
    /// still detects a dead server in ~15 s, not ~450 s. `None` until the
    /// first grant/packet (never trips the watchdog).
    pub last_response: Option<Instant>,
    /// `noTimeoutTimer` from Java: frames since the last outbound flush;
    /// `gameLoop` writes `NO_TIMEOUT` past 50 (~1 s at 20 ms).
    pub no_timeout_timer: i32,
    /// `errorLoading` from Java/TS: missing required cache jag or a failed
    /// map request. `mainloop` returns immediately; framerate is 1.
    pub error_loading: bool,
    /// Packet-family generations (`ClientGens`): bumped by `handle_packet`
    /// after every applied packet so the host can tell which world slices
    /// changed since its last poll.
    pub gens: ClientGens,
}

/// Dead-server watchdog bound: the Java client's 750 `gameLoop` passes at
/// 20 ms (~15 s), but measured in elapsed time so the bound holds at any
/// pass cadence (a parked host slot runs `gameLoop` once per ~600 ms).
const SERVER_TIMEOUT: Duration = Duration::from_secs(15);

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        // TS `getJagChecksums` downloads `/crc` from the web origin (port 80).
        // Local pack/client is missing `wordenc`, so file CRCs fail the
        // engine's CrcBuffer32 check (login code 6). Prefer /crc; fall back
        // to files for tests without a web server.
        let jag_checksum = Self::get_jag_checksums(&config.host, 80)
            .unwrap_or_else(|_| Self::read_jag_checksums(&config.cache_dir));
        let (cache, ifaces, ifaces_mut, error_loading) = match Self::load_cache(&config.cache_dir) {
            Ok((cache, ifaces, ifaces_mut)) => (cache, ifaces, Arc::new(ifaces_mut), false),
            Err(()) => (Cache::default(), Vec::new(), Arc::new(Vec::new()), true),
        };
        Self::construct(
            config,
            Arc::new(cache),
            Arc::new(ifaces),
            ifaces_mut,
            error_loading,
            jag_checksum,
        )
    }

    /// Host construct: inject a process-wide `Arc<Cache>`, the shared
    /// iface decode `Arc` and the per-client mut-overlay template. Skips
    /// `load_cache` and the `/crc` probe; the host unpacks once per
    /// `cache_dir` and every client short-circuits the `maininit` re-unpack
    /// via `cache_from_shared`. `error_loading` is false so `mainloop` is
    /// not a no-op after a successful inject.
    pub fn from_shared(
        config: ClientConfig,
        cache: Arc<Cache>,
        ifaces: Arc<Vec<Option<Box<IfType>>>>,
        ifaces_mut: impl Into<Arc<Vec<Option<Arc<IfTypeMut>>>>>,
    ) -> Self {
        let jag_checksum = Self::read_jag_checksums(&config.cache_dir);
        let mut client = Self::construct(
            config,
            cache,
            ifaces,
            ifaces_mut.into(),
            false,
            jag_checksum,
        );
        client.cache_from_shared = true;
        client
    }

    fn construct(
        config: ClientConfig,
        cache: Arc<Cache>,
        ifaces: Arc<Vec<Option<Box<IfType>>>>,
        ifaces_mut: Arc<Vec<Option<Arc<IfTypeMut>>>>,
        error_loading: bool,
        jag_checksum: [i32; 9],
    ) -> Self {
        let on_demand = Self::load_on_demand(&config);
        let midi = midi_backend(&config.cache_dir);
        let jagfx = Self::unpack_jagfx(&config.cache_dir, config.lowmem);
        let groundh: LevelHeightmaps =
            vec![
                vec![vec![0i32; (BUILD_AREA_SIZE + 1) as usize]; (BUILD_AREA_SIZE + 1) as usize];
                BuildArea::LEVELS as usize
            ];
        let mut client = Client {
            shell: GameShell::new(),
            present: None,
            config,
            cache,
            ifaces,
            ifaces_mut,
            cache_from_shared: false,

            ingame: false,
            draw: false,
            nav_debug_paint: None,
            scene_state: 0,
            in_multizone: 0,
            build_minusedlevel: 0,
            local_player: None,
            players: vec![None; MAX_PLAYER_COUNT],
            npc: vec![None; MAX_NPC_COUNT],

            player_count: 0,
            player_ids: vec![0; MAX_PLAYER_COUNT],
            npc_count: 0,
            npc_ids: vec![0; MAX_NPC_COUNT],
            entity_removal_count: 0,
            entity_removal_ids: vec![0; 1000],
            entity_update_count: 0,
            entity_update_ids: vec![0; MAX_PLAYER_COUNT],
            player_appearance_buffer: (0..MAX_PLAYER_COUNT).map(|_| None).collect(),

            groundh: groundh.clone(),
            mapl: vec![
                vec![vec![0u8; BuildArea::SIZE as usize]; BuildArea::SIZE as usize];
                BuildArea::LEVELS as usize
            ],
            world: World::new(groundh, BuildArea::SIZE, BuildArea::LEVELS, BuildArea::SIZE),
            collision: [
                CollisionMap::new(),
                CollisionMap::new(),
                CollisionMap::new(),
                CollisionMap::new(),
            ],
            minusedlevel: 0,
            zone_update_x: 0,
            zone_update_z: 0,
            ground_obj: empty_ground_obj(),
            loc_changes: LinkList::new(),
            projectiles: LinkList::new(),
            spotanims: LinkList::new(),
            self_slot: -1,
            members_account: 0,
            world_update_num: 0,

            stat_base_level: vec![0; Skill::count],
            stat_effective_level: vec![0; Skill::count],
            stat_xp: vec![0; Skill::count],
            var: Vec::new(),
            var_serv: Vec::new(),
            runenergy: 0,
            runweight: 0,

            midi_active: true,
            midi_song: -1,
            next_midi_song: -1,
            next_music_delay: 0,
            midi_fading: true,
            midi_volume: 0,
            midi,
            midi_pending: None,
            midi_playing: false,
            fade: Arc::new(Mutex::new(Fade::new())),
            waves: Arc::new(Mutex::new(Vec::new())),

            wave_enabled: true,
            wave_volume: 0,
            chat_effects: 0,
            wave_count: 0,
            wave_ids: vec![0; 50],
            wave_loops: vec![0; 50],
            wave_delay: vec![0; 50],
            jagfx,

            menu_num_entries: 0,
            menu_option: vec![String::new(); MENU_CAPACITY],
            menu_action: vec![0; MENU_CAPACITY],
            menu_param_a: vec![0; MENU_CAPACITY],
            menu_param_b: vec![0; MENU_CAPACITY],
            menu_param_c: vec![0; MENU_CAPACITY],
            is_menu_open: false,
            menu_area: 0,
            menu_x: 0,
            menu_y: 0,
            menu_width: 0,
            menu_height: 0,
            obj_selected_name: String::new(),
            target_op: String::new(),
            target_mask: 0,
            cross_x: 0,
            cross_y: 0,
            cross_mode: 0,
            cross_cycle: 0,
            oplogic1: 0,
            oplogic2: 0,
            oplogic3: 0,
            oplogic4: 0,
            oplogic5: 0,
            oplogic6: 0,
            oplogic7: 0,
            oplogic8: 0,
            oplogic9: 0,
            cyclelogic2: 0,
            cyclelogic6: 0,
            report_abuse_input: String::new(),
            report_abuse_mute_option: false,
            report_abuse_com_id: 0,

            dir_map: vec![0; BUILD_AREA_TILES],
            dist_map: vec![0; BUILD_AREA_TILES],
            route_x: vec![0; ROUTE_BUFFER],
            route_z: vec![0; ROUTE_BUFFER],
            try_move_path: Vec::new(),
            try_move_nearest: 0,
            minimap_flag_x: 0,
            minimap_flag_z: 0,
            map_build_base_x: 0,
            map_build_base_z: 0,
            map_build_centre_zone_x: 0,
            map_build_centre_zone_z: 0,
            map_build_prev_base_x: 0,
            map_build_prev_base_z: 0,
            within_tutorial_island: false,
            awaiting_player_info: false,
            scene_load_start_time: Instant::now(),
            map_build_index: Vec::new(),
            map_build_ground_file: Vec::new(),
            map_build_location_file: Vec::new(),
            map_build_ground_data: Vec::new(),
            map_build_location_data: Vec::new(),

            use_mode: 0,
            target_mode: 0,
            one_mouse_button: 0,
            player_op: Default::default(),
            player_op_priority: Default::default(),
            redraw_side: false,
            active_icon: 3,
            side_icon: [-1; 14],
            side_modal_id: -1,
            chat_modal_id: -1,
            main_modal_id: -1,
            main_overlay_id: -1,
            tut_com_id: -1,
            tut_flash_icon: -1,
            dialog_input_open: false,
            dialog_input: String::new(),
            resumed_pause_button: false,
            over_main_com_id: 0,
            over_side_com_id: 0,
            over_chat_com_id: 0,
            last_over_com_id: 0,
            hovered_slot: 0,
            hovered_slot_com_id: 0,
            target_com_id: 0,
            obj_com_id: 0,
            obj_selected_slot: 0,
            obj_selected_com_id: 0,
            selected_area: 0,
            selected_com_id: 0,
            selected_item: 0,
            selected_cycle: 0,
            obj_drag_area: 0,
            obj_drag_com_id: 0,
            obj_drag_slot: 0,
            obj_grab_x: 0,
            obj_grab_y: 0,
            obj_grab_threshold: false,
            obj_drag_cycles: 0,
            bank_arrange_mode: 0,

            out: Packet::alloc(1),
            r#in: Packet::alloc(1),
            ptype: 0,
            ptype0: 0,
            ptype1: 0,
            ptype2: 0,
            psize: 0,

            stream: None,
            on_demand,
            staffmodlevel: 0,
            mouse_tracked: false,
            random_in: None,
            jag_checksum,

            login_user: String::new(),
            login_pass: String::new(),
            login_mes1: String::new(),
            login_mes2: String::new(),
            loop_cycle: 0,
            last_progress_percent: 0,
            last_progress_message: String::new(),
            http_port: 80,
            already_started: false,
            fetch_retry_wait: Duration::from_secs(5),
            pick_count: 0,
            pick_typecodes: vec![0; 1000],
            pending_brightness: None,
            tex_average: [0; 50],
            chat_interface: IfTypeMut::default(),
            scroll_grabbed: false,
            scroll_input_padding: 0,
            scroll_cycle: 0,
            loginscreen: 0,
            login_select: 0,
            redraw_frame: true,
            cam_x: 0,
            cam_y: 0,
            cam_z: 0,
            cam_pitch: 0,
            cam_yaw: 0,
            cinema_cam: false,
            cam_look_at_lx: 0,
            cam_look_at_lz: 0,
            cam_look_at_hei: 0,
            cam_look_at_rate: 0,
            cam_look_at_rate2: 0,
            cam_move_to_lx: 0,
            cam_move_to_lz: 0,
            cam_move_to_hei: 0,
            cam_move_to_rate: 0,
            cam_move_to_rate2: 0,
            cam_shake: [false; 5],
            cam_shake_axis: [0; 5],
            cam_shake_ran: [0; 5],
            cam_shake_amp: [0; 5],
            cam_shake_cycle: [0; 5],
            orbit_camera_pitch: 128,
            orbit_camera_yaw: 0,
            orbit_camera_yaw_velocity: 0,
            orbit_camera_pitch_velocity: 0,
            orbit_camera_x: 0,
            orbit_camera_z: 0,
            macro_camera_x: 0,
            macro_camera_z: 0,
            camera_pitch_clamp: 0,
            macro_camera_angle: 0,
            minimap_state: 0,
            minimap_level: -1,
            macro_minimap_angle: 0,
            macro_minimap_zoom: 0,
            redraw_icons: false,
            redraw_chat: false,
            redraw_chat_mode: false,
            hint_type: 0,
            hint_npc: 0,
            hint_player: 0,
            hint_tile_x: 0,
            hint_tile_z: 0,
            hint_height: 0,
            hint_offset_x: 0,
            hint_offset_z: 0,
            chat_public_mode: 0,
            chat_private_mode: 0,
            chat_trade_mode: 0,
            chat_type: [0; 100],
            chat_username: [const { String::new() }; 100],
            chat_text: [const { String::new() }; 100],
            chat_input: String::new(),
            chat_scroll_pos: 0,
            chat_scroll_height: 78,
            chat_seq: 0,
            ignore_count: 0,
            ignore_userhash: [0; 100],
            chat_disabled: 0,
            friend_count: 0,
            friend_userhash: [0; 200],
            friend_username: [const { String::new() }; 200],
            friend_node_id: [0; 200],
            friend_server_status: 0,
            private_message_ids: [0; 100],
            private_message_count: 0,
            node_id: 10,
            social_input_open: false,
            social_input: String::new(),
            social_input_type: 0,
            social_input_header: String::new(),
            social_userhash: 0,
            split_private_chat: 0,
            idk_design_gender: true,
            idk_design_redraw: false,
            idk_design_part: [-1; 7],
            idk_design_colour: [0; 5],
            idk_design_button1: None,
            idk_design_button2: None,
            last_login_reconnect: None,
            baton: false,
            logout_timer: 0,
            reboot_timer: 0,
            last_response: None,
            no_timeout_timer: 0,
            error_loading,
            gens: ClientGens::default(),
            login_uid: login_uid(),
        };
        if client.error_loading {
            client.shell.set_framerate(1);
        }
        // World::new defaults overlay_mesh on for direct set_ground tests.
        // A host Client starts unheaded (`draw` false); keep the mesh gate
        // in lockstep so a first map_build before observe cannot write
        // headed overlay verts.
        client.world.overlay_mesh = client.draw;
        client
    }

    /// Interface component by id (the old `Cache::if_`), as the combined
    /// view: the shared decode (`Deref` fields) plus this client's
    /// `IfTypeMut` overlay (hide/scroll/anim/inv/text/`IF_SET*` values).
    pub fn if_(&self, id: usize) -> Option<IfTypeView<'_>> {
        let base = self.ifaces.get(id).and_then(|o| o.as_deref())?;
        let ov: &IfTypeMut = match self.ifaces_mut.get(id).and_then(|o| o.as_deref()) {
            Some(ov) => ov,
            None => default_mut(),
        };
        Some(IfTypeView::new(base, ov))
    }

    /// Mutable interface component by id, from this client's dense
    /// `IfTypeMut` overlay (never the shared decode, so `hide` on A never
    /// reaches B). Missing slots get a default overlay (test seeding).
    /// Holds `&mut self`, so prefer `Arc::make_mut(&mut self.ifaces_mut).get_mut(id).and_then(|o| o.as_mut()).map(Arc::make_mut)` inside code that
    /// also reads other `self` state. First write `Arc::make_mut`s the
    /// template vec (pointer copies only) and COWs just the written slot.
    pub fn iface_mut(&mut self, id: usize) -> Option<&mut IfTypeMut> {
        let slots = Arc::make_mut(&mut self.ifaces_mut);
        if slots.len() <= id {
            slots.resize(id + 1, None);
        }
        if slots[id].is_none() {
            slots[id] = Some(Arc::new(IfTypeMut::default()));
        }
        slots[id].as_mut().map(Arc::make_mut)
    }

    /// Mutable overlay slot if it already exists (no allocate). First
    /// write COWs the shared template; a missing id does not clone.
    pub fn overlay_mut(&mut self, id: usize) -> Option<&mut IfTypeMut> {
        self.ifaces_mut.get(id).and_then(|o| o.as_ref())?;
        Arc::make_mut(&mut self.ifaces_mut)
            .get_mut(id)
            .and_then(|o| o.as_mut())
            .map(Arc::make_mut)
    }

    /// Seed/test helper: place the decode part of `com` at `id` and a
    /// default `IfTypeMut` beside it. Only valid for a client that owns
    /// its table (tests — `Client::new`); the shared decode is immutable
    /// once several clients share the `Arc`.
    pub fn set_iface(&mut self, id: usize, com: IfType) {
        if self.ifaces.len() <= id {
            Arc::make_mut(&mut self.ifaces).resize(id + 1, None);
        }
        Arc::make_mut(&mut self.ifaces)[id] = Some(Box::new(com));
        self.set_iface_mut(id, IfTypeMut::default());
    }

    /// Seed/test helper: put `m` at `id` in this client's overlay.
    pub fn set_iface_mut(&mut self, id: usize, m: IfTypeMut) {
        let slots = Arc::make_mut(&mut self.ifaces_mut);
        if slots.len() <= id {
            slots.resize(id + 1, None);
        }
        slots[id] = Some(Arc::new(m));
    }

    /// Seed/test helper: append the decode part of `com` (and a default
    /// overlay) at the next free id.
    pub fn push_iface(&mut self, com: IfType) -> usize {
        let id = self.ifaces_mut.len();
        self.set_iface(id, com);
        id
    }

    /// The id of the first component satisfying `pred` in the combined
    /// view, in id order including holes — the old
    /// `ifaces.iter().position(...)`.
    pub fn iface_id(&self, pred: impl Fn(&IfType) -> bool) -> Option<usize> {
        let n = self.ifaces_len();
        (0..n).find(|&id| self.if_(id).is_some_and(|v| pred(&v)))
    }

    /// The combined component count (max of overlay and shared decode),
    /// for id-indexed tables that used `ifaces.len()`.
    pub fn ifaces_len(&self) -> usize {
        self.ifaces.len().max(self.ifaces_mut.len())
    }

    /// Every component in the combined view, in id order (the old
    /// `ifaces.iter().flatten()` walk over the per-client copy).
    pub fn ifaces_merged(&self) -> impl Iterator<Item = IfTypeView<'_>> {
        let n = self.ifaces_len();
        (0..n).filter_map(|id| self.if_(id))
    }

    /// TS maininit 1168-1171: unpack `sounds.dat` from the `sounds` jag
    /// unless lowmem. Missing/corrupt file stays an empty table.
    fn unpack_jagfx(cache_dir: &str, lowmem: bool) -> JagFX {
        JagFX::load_shared(cache_dir, lowmem)
    }

    /// Unpack `config` (and `interface` when present) from `cache_dir`,
    /// returning the shared `Cache` tables, the iface decode (`Client`
    /// wraps the latter in the shared `Arc`) and the per-client
    /// mut-overlay template. An empty dir (tests, no pack) yields
    /// `Cache::default()` with empty ifaces. A real cache missing the
    /// required `config` jag — or one whose bytes are not a valid jag
    /// (dummy test files) — is `Err`, which becomes `errorLoading`.
    fn load_cache(cache_dir: &str) -> Result<LoadedCache, ()> {
        let cache_present = JAG_FILES
            .iter()
            .any(|name| Path::new(&format!("{cache_dir}/{name}")).is_file());
        if !cache_present {
            return Ok((Cache::default(), Vec::new(), Vec::new()));
        }
        let bytes = std::fs::read(format!("{cache_dir}/config")).map_err(|_| ())?;
        let cache = catch_unwind(AssertUnwindSafe(|| Cache::unpack(&JagFile::new(bytes))))
            .map_err(|_| ())?;
        let mut ifaces = Vec::new();
        let mut ifaces_mut = Vec::new();
        if let Ok(iface_bytes) = std::fs::read(format!("{cache_dir}/interface")) {
            if let Ok((unpacked, unpacked_mut)) = catch_unwind(AssertUnwindSafe(|| {
                IfType::unpack(&JagFile::new(iface_bytes))
            })) {
                ifaces = unpacked;
                // Box→Arc once here: every client then shares the same
                // per-slot `Arc`s and a write COWs one slot, not the vec.
                ifaces_mut = unpacked_mut
                    .into_iter()
                    .map(|o| o.map(|b| Arc::new(*b)))
                    .collect();
            }
        }
        Ok((cache, ifaces, ifaces_mut))
    }

    /// HTTP/1.0 `GET {path}` returning the response body, headers split on
    /// `\r\n\r\n` (client-ts `getJagChecksums`/`getJagFile` fetch the same
    /// way). `None` on connect/read failure or a bodyless response.
    fn http_get(host: &str, port: u16, path: &str) -> Option<Vec<u8>> {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect((host, port)).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
        write!(stream, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").ok()?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).ok()?;
        let split = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
        Some(buf[split + 4..].to_vec())
    }

    /// TS `getJagChecksums`: GET `/crc` (9×g4 + hash). Hash check matches
    /// client-ts (`1234`, `hash = (hash << 1) + crc[i]`). `Err` carries the
    /// TS retry-message label (`connection problem` on a failed fetch,
    /// `checksum problem` on a bad parse), which `fetch_jag_checksums`
    /// shows in the per-second countdown.
    fn get_jag_checksums(host: &str, port: u16) -> Result<[i32; 9], &'static str> {
        let body = Self::http_get(host, port, "/crc").ok_or("connection problem")?;
        if body.len() < 40 {
            return Err("checksum problem");
        }
        let mut p = Packet::new(body[..40].to_vec());
        let mut checksum = [0i32; 9];
        for slot in &mut checksum {
            *slot = p.g4();
        }
        let expected = p.g4();
        let mut calculated: i32 = 1234;
        for c in checksum {
            calculated = calculated.wrapping_shl(1).wrapping_add(c);
        }
        if expected != calculated {
            return Err("checksum problem");
        }
        Ok(checksum)
    }

    /// TS `getJagFile`: serve `{filename}` from the cache when its CRC
    /// matches `checksums[index]`; otherwise GET `/{filename}{crc}`, verify
    /// the CRC, persist to `cache_dir`, and return the bytes. A CRC mismatch
    /// is `None` (the caller retries the fetch). JAG archives sit at checksum
    /// slots 1-8 (`JAG_FILES`); title is slot 1.
    pub fn get_jag_file(
        cache_dir: &str,
        host: &str,
        port: u16,
        filename: &str,
        index: usize,
        checksums: &[i32; 9],
    ) -> Option<Vec<u8>> {
        let &crc = checksums.get(index)?;
        let cached = std::fs::read(format!("{cache_dir}/{filename}")).ok();
        if let Some(bytes) = cached {
            if Packet::getcrc(&bytes, 0, bytes.len()) == crc {
                return Some(bytes);
            }
        }
        let bytes = Self::http_get(host, port, &format!("/{filename}{crc}"))?;
        if Packet::getcrc(&bytes, 0, bytes.len()) != crc {
            return None;
        }
        let _ = std::fs::write(format!("{cache_dir}/{filename}"), &bytes);
        Some(bytes)
    }

    /// CRC of each JAG pack file under `cache_dir`, in the 9-slot layout the
    /// login wrapper sends (title..sounds at slots 1-8, slot 0 = 0). Missing
    /// files read as 0, matching a client with an empty cache.
    fn read_jag_checksums(cache_dir: &str) -> [i32; 9] {
        let mut checksum = [0i32; 9];
        for (slot, name) in JAG_FILES.iter().enumerate() {
            if let Ok(bytes) = std::fs::read(format!("{cache_dir}/{name}")) {
                checksum[slot + 1] = Packet::getcrc(&bytes, 0, bytes.len());
            }
        }
        checksum
    }

    /// The Task 2 snapshot root (`~/.274bot/unpack`), matching `unpack-cache`
    /// when `HOME` is unset.
    fn unpack_snapshot_dir() -> String {
        match std::env::var("HOME") {
            Ok(home) => format!("{home}/.274bot/unpack"),
            Err(_) => ".274bot/unpack".to_string(),
        }
    }

    /// Start the OnDemand worker when the cache dir has a `versionlist` pack
    /// (the TS update-server fetch is a cache read here; the engine packs the
    /// same file). Missing cache → `None`, matching TS `onDemand === null`.
    /// A `versionlist` that is not a valid jag (dummy test files) also reads
    /// as `None` — `JagFile`/`OnDemand::new` panic on garbage offsets, so
    /// the whole parse is unwind-caught.
    fn load_on_demand(config: &ClientConfig) -> Option<OnDemand> {
        let bytes = std::fs::read(format!("{}/versionlist", config.cache_dir)).ok()?;
        catch_unwind(AssertUnwindSafe(|| {
            let versionlist = JagFile::new(bytes);
            OnDemand::new(
                &versionlist,
                &config.host,
                config.port,
                &config.cache_dir,
                Arc::new(AtomicBool::new(false)),
            )
        }))
        .ok()
        .flatten()
    }

    /// Record a loading-progress point on `Client` (`last_progress_percent`
    /// / `last_progress_message`), mirroring what `Renderer::draw_progress`
    /// records. `maininit`/`fetch_*` call this at every progress point so a
    /// headless client records progress without any `Renderer`.
    pub fn set_progress(&mut self, message: &str, percent: i32) {
        self.last_progress_percent = percent;
        self.last_progress_message = message.to_string();
    }

    /// Record a progress point (`set_progress`) and, when a progress
    /// callback is attached (headed `run` / client-play), notify it so the
    /// driver can draw the loading bar synchronously. Recording is
    /// sim-side, drawing is render-side.
    fn report_progress(&mut self, progress: &mut ProgressCb<'_>, message: &str, percent: i32) {
        self.set_progress(message, percent);
        if let Some(cb) = progress.as_deref_mut() {
            cb(self, message, percent);
        }
    }

    /// TS `Client.maininit` (819-1178): the one-shot loading screen — fetch
    /// the 8 JAG archives over HTTP (CRC-hit on the local cache), unpack
    /// config/interface, start OnDemand from the versionlist, and prefetch
    /// anims/models. Renderer-free: progress is recorded on `Client`
    /// (`last_progress_*`); `maininit_with_progress` additionally notifies
    /// an optional callback so a headed driver can draw the bar
    /// synchronously. `already_started` is set first, so a second call is a
    /// no-op (TS `alreadyStarted`). A failed or invalid jag sets
    /// `error_loading` but does not abort the fetch loop; progress reaches
    /// 100 only when the `/crc` fetch succeeds — the checksum-fail path
    /// returns early with `error_loading` and `last_progress_percent` left
    /// at 10.
    pub fn maininit(&mut self) {
        self.maininit_with_progress(None);
    }

    /// The `maininit` body: every progress point records `last_progress_*`
    /// via `set_progress` and, when `progress` is `Some`, invokes the
    /// callback so a headed driver can draw `Renderer::draw_progress`
    /// synchronously without `maininit` naming `Renderer`.
    pub fn maininit_with_progress(&mut self, mut progress: ProgressCb<'_>) {
        if self.already_started {
            return;
        }
        self.already_started = true;
        // TS produces `errorLoading` only inside `maininit`; `Client::new`'s
        // pre-maininit unpack may have set it (and framerate 1) for a cache
        // that `maininit` can repair, so reset both before fetching.
        self.error_loading = false;
        self.shell.set_framerate(50);

        self.report_progress(&mut progress, "Loading...", 0);

        // TS `getJagChecksums` (694-748): `/crc` retried with a 5 s wait
        // doubling to 60 s, spent as one countdown tick per second and
        // capped at 10 retries ("Game updated - please reload page") so a
        // dead web server cannot hang the caller; tests plant a listener so
        // the first attempt succeeds.
        let checksums = match self.fetch_jag_checksums(&mut progress) {
            Some(c) => c,
            None => {
                self.error_loading = true;
                self.shell.set_framerate(1);
                return;
            }
        };
        self.jag_checksum = checksums;

        // TS maininit fetch order/progress: title 25, config 30, interface
        // 35, media 40, textures 45, wordenc 50, sounds 55, versionlist 60
        // (checksum slots 1-8 of `JAG_FILES`). `wordenc` is fetched and
        // persisted; `WordFilter.unpack` reads it back after load_cache.
        const JAG_FETCH: [(&str, &str, usize, i32); 8] = [
            ("title screen", "title", 1, 25),
            ("config", "config", 2, 30),
            ("interface", "interface", 3, 35),
            ("2d graphics", "media", 4, 40),
            ("textures", "textures", 6, 45),
            ("chat system", "wordenc", 7, 50),
            ("sound effects", "sounds", 8, 55),
            ("update list", "versionlist", 5, 60),
        ];
        for (display, filename, index, pct) in JAG_FETCH {
            if self
                .fetch_jag_file(&mut progress, display, pct, filename, index, &checksums)
                .is_none()
            {
                self.error_loading = true;
            }
        }

        // Unpack config/interface from the files now on disk (`load_cache`
        // reads the same persisted paths `get_jag_file` wrote). A missing or
        // invalid config jag is `Err` → `errorLoading`. Skipped when the
        // cache was injected already-unpacked via `from_shared`: the host
        // unpacked once per `cache_dir`, the shared `Arc<Cache>` is current,
        // and re-unpacking would throw it away. `Client::new` (marker false)
        // still re-unpacks after the fresh jag fetch — that unpack is
        // intentional and must not be skipped.
        if !self.cache_from_shared {
            match Self::load_cache(&self.config.cache_dir) {
                Ok((cache, ifaces, ifaces_mut)) => {
                    self.cache = Arc::new(cache);
                    self.ifaces = Arc::new(ifaces);
                    self.ifaces_mut = Arc::new(ifaces_mut);
                }
                Err(()) => {
                    self.error_loading = true;
                    self.shell.set_framerate(1);
                }
            }
        }

        // TS 1168-1171: unpack sounds.dat after the jag is on disk.
        if !self.config.lowmem {
            self.jagfx = Self::unpack_jagfx(&self.config.cache_dir, false);
        }

        // Java unpacks textures during maininit (progress 45) before any
        // scene build. Unheaded slots never run `prepare_game`, so bake
        // the averages the sim's `finish_build` reads for overlay rgb.
        self.load_tex_averages();

        // TS maininit 1236 `WordFilter.unpack(wordenc)`: read the jag the
        // fetch persisted. A missing or corrupt file is skipped — the
        // filter stays identity and maininit must not fail. `unpack` is
        // idempotent (OnceLock), so repeated maininit calls are no-ops.
        let wordenc_path = format!("{}/wordenc", self.config.cache_dir);
        if let Ok(bytes) = std::fs::read(&wordenc_path) {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                WordFilter::unpack(&JagFile::new(bytes))
            }));
        }

        self.on_demand = Self::load_on_demand(&self.config);

        // Java 5164-5182: request scape_main on *this* OnDemand (maininit
        // replaces any Client::new worker) and wait remaining()==0 so
        // onDemandLoop → saveMidi fires before anim/model flood. A prior
        // prepare_title request on the discarded worker must not skip this
        // (`midi_song` may already be 0).
        if !self.config.lowmem {
            if let Some(od) = &mut self.on_demand {
                self.midi_song = 0;
                self.midi_fading = true;
                od.request(2, 0);
            }
            while self.on_demand.as_ref().is_some_and(|od| od.remaining() > 0) {
                // Report each pass (TS 874) so the progress callback pumps
                // the window/audio through the OnDemand wait instead of a
                // silent `thread::sleep` block.
                self.report_progress(&mut progress, "Connecting to update server", 60);
                self.on_demand_loop();
                thread::sleep(Duration::from_millis(100));
                if self.on_demand.as_ref().is_some_and(|od| od.fail_count > 3) {
                    self.error_loading = true;
                    self.shell.set_framerate(1);
                    return;
                }
            }
        }

        // TS maininit 886-888: `AnimFrame.init`/`Model.init` size the
        // process-wide stores from the versionlist before any prefetch.
        // `Model.init` also wires the provider that routes `requestDownload`
        // archive-0 requests back to this OnDemand's worker.
        if let Some(od) = self.on_demand.as_ref() {
            AnimFrame::init(od.get_anim_frame_count());
            if let Some(provider) = od.model_provider() {
                Model::init(od.get_file_count(0), provider);
            }
        }

        // Task 2 boot inject: load the Task 1 snapshot (models + anims) first
        // so every model/anim is available before the scene places its locs.
        // Non-fatal: a missing/empty snapshot falls back to the live-cache
        // unpack and the OnDemand floods below unchanged.
        let snapshot_loaded = match crate::unpack::load_snapshot_once(
            &self.config.cache_dir,
            &Self::unpack_snapshot_dir(),
        ) {
            Ok((loaded, first)) => {
                if first {
                    eprintln!(
                        "274bot: loaded snapshot ({} models, {} anim records)",
                        loaded.models, loaded.anim_records
                    );
                }
                true
            }
            Err(e) => {
                eprintln!("274bot: snapshot load skipped: {e}");
                false
            }
        };

        // Fallback when the snapshot is absent: unpack every model already in
        // the local file store (idx1) the server served — no
        // OnDemand/server round-trip — so a random-event model (the Maze
        // walls) is available at boot. The OnDemand request below still
        // fetches anything the live cache lacks.
        if !snapshot_loaded {
            if let Some(od) = self.on_demand.as_ref() {
                od.unpack_models_from_cache(&self.config.cache_dir);
            }
        }

        // TS anim/model prefetch (893-960): request every anim, then the
        // in-use models, draining with `on_demand_loop` until the request
        // lists empty. Skipped when OnDemand is `None` (no versionlist —
        // the dummy-file tests) or when the snapshot already injected them.
        if self.on_demand.is_some() {
            if !snapshot_loaded {
                self.report_progress(&mut progress, "Requesting animations", 65);
                let anim_count = self.on_demand.as_ref().unwrap().get_file_count(1);
                for i in 0..anim_count {
                    self.on_demand.as_mut().unwrap().request(1, i);
                }
                while self.on_demand.as_ref().unwrap().remaining() > 0 {
                    let done = anim_count - self.on_demand.as_ref().unwrap().remaining() as i32;
                    // Report every pass (not only on progress): the callback
                    // pumps the window/audio, so a prefetch stall cannot
                    // beachball the headed client.
                    self.report_progress(
                        &mut progress,
                        &format!("Loading animations - {}%", (done * 100) / anim_count),
                        65,
                    );
                    self.on_demand_loop();
                    thread::sleep(Duration::from_millis(100));
                }

                self.report_progress(&mut progress, "Requesting models", 70);
                // Java 5206-5210: remaining()==0 only for `getModelUse & 1`.
                // Other use bits + maps + midi jingles are prefetchPriority
                // after the bar (5251-5285). Title `titleScreenDraw` plots
                // `onDemand.message` ("Loading extra files - x%") under the
                // two login buttons while those drain (Java 3927, colour
                // 7711145). Live-verify used to urgent-request every
                // `priority != 0` model on the bar; that skipped the title
                // extra-files pass and made startup slower than Java.
                self.on_demand.as_mut().unwrap().request_all_models();
                let model_total = self.on_demand.as_ref().unwrap().remaining() as i32;
                while self.on_demand.as_ref().unwrap().remaining() > 0 {
                    let done = model_total - self.on_demand.as_ref().unwrap().remaining() as i32;
                    self.report_progress(
                        &mut progress,
                        &format!("Loading models - {}%", (done * 100) / model_total.max(1)),
                        70,
                    );
                    self.on_demand_loop();
                    thread::sleep(Duration::from_millis(100));
                }
            }

            // Java `Client.java:5224-5250`: urgent Lumbridge starter maps,
            // waited on the loading bar (`remaining() == 0`) before title.
            self.report_progress(&mut progress, "Requesting maps", 75);
            const LUMBRIDGE_SQUARES: [(i32, i32); 6] =
                [(47, 48), (48, 48), (49, 48), (47, 47), (48, 47), (48, 148)];
            for (x, z) in LUMBRIDGE_SQUARES {
                for ty in [0, 1] {
                    let file = self.on_demand.as_ref().unwrap().get_map_file(x, z, ty);
                    if file != -1 {
                        self.on_demand.as_mut().unwrap().request(3, file);
                    }
                }
            }
            let map_total = self.on_demand.as_ref().unwrap().remaining() as i32;
            while self.on_demand.as_ref().unwrap().remaining() > 0 {
                let done = map_total - self.on_demand.as_ref().unwrap().remaining() as i32;
                self.report_progress(
                    &mut progress,
                    &format!("Loading maps - {}%", (done * 100) / map_total.max(1)),
                    75,
                );
                self.on_demand_loop();
                thread::sleep(Duration::from_millis(100));
            }

            // Snapshot already injected every model/anim. Prefetching the
            // rest of the map archive on every slot is ~15 MB × N clients
            // sitting in OnDemand worker queues (skip-paint heads still
            // paid it). Lumbridge urgent maps above are enough to scene 2.
            if !snapshot_loaded {
                let members = self.config.members;
                let lowmem = self.config.lowmem;
                self.on_demand
                    .as_mut()
                    .unwrap()
                    .prefetch_extra_files(members, lowmem);
                // Prefetch is not in remaining(). Cache-hit extra models post
                // Completed on the worker; drain them so CLI login (no title
                // extra-files window) still unpacks before the first frame.
                let started = Instant::now();
                let deadline = started + Duration::from_secs(2);
                let mut idle = 0;
                while Instant::now() < deadline {
                    self.report_progress(&mut progress, "Loading extra files", 70);
                    let n = self.on_demand_loop();
                    if n == 0 {
                        idle += 1;
                    } else {
                        idle = 0;
                    }
                    if idle >= 5 && started.elapsed() > Duration::from_millis(150) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }

        self.report_progress(&mut progress, "Preparing game engine", 100);
    }

    /// Java `getJagChecksums` (deob 11211-11268) / TS 694-748: attempt
    /// `/crc`, then spend the retry wait as one `messageBox` countdown tick
    /// per second (`"...Will retry in N secs."`, TS text), doubling 5 s to
    /// the 60 s cap. Past 10 failed attempts the countdown switches to
    /// "Game updated - please reload page"; Java loops there forever, this
    /// port draws it once and returns `None` so `maininit` fails with
    /// `errorLoading` instead of hanging. Every countdown tick reports
    /// progress, so a headed driver pumps the window/audio through the wait
    /// instead of beachballing.
    fn fetch_jag_checksums(&mut self, progress: &mut ProgressCb<'_>) -> Option<[i32; 9]> {
        let mut wait = self.fetch_retry_wait;
        let mut retries = 0;
        loop {
            self.report_progress(progress, "Connecting to web server", 10);
            let error = match Self::get_jag_checksums(&self.config.host, self.http_port) {
                Ok(checksums) => return Some(checksums),
                Err(error) => error,
            };
            retries += 1;
            if retries >= 10 {
                // Java deob 11252-11257 / TS 730-733: at `retries >= 10`
                // the countdown message becomes "Game updated - please
                // reload page".
                self.report_progress(progress, "Game updated - please reload page", 10);
                return None;
            }
            self.retry_countdown(progress, wait, error, 10);
            wait = (wait * 2).min(Duration::from_secs(60));
        }
    }

    /// Java `getJagChecksums`/`getJagFile` countdown (deob 11245-11263 /
    /// TS 729-737): the retry wait is spent as one `messageBox` per second,
    /// not a single blocking sleep. Each tick reports progress so a headed
    /// driver pumps the window/audio through the wait. Sub-second
    /// `fetch_retry_wait` (the stubbed-HTTP tests) collapses to one tick.
    fn retry_countdown(
        &mut self,
        progress: &mut ProgressCb<'_>,
        wait: Duration,
        message: &str,
        pct: i32,
    ) {
        let ticks = wait.as_secs().max(1);
        let step = wait / ticks as u32;
        for remaining in (1..=ticks).rev() {
            self.report_progress(
                progress,
                &format!("{message} - Will retry in {remaining} secs."),
                pct,
            );
            thread::sleep(step);
        }
    }

    /// Java `getJagFile` (deob 4817-4933) / TS 749-817: GET
    /// `/{filename}{crc}` with the same per-second countdown and doubling
    /// wait as the checksum fetch. A CRC mismatch is handled inside
    /// `get_jag_file` (bytes discarded, `None` returned) and retried here,
    /// so a transient failure or corrupted download recovers instead of
    /// erroring the client. Past the 10-retry cap the countdown switches to
    /// Java's "Game updated - please reload page" and `None` lets `maininit`
    /// set `errorLoading`; a dead server cannot hang the caller. Each
    /// countdown tick reports progress (window/audio pump); the message
    /// text is the TS one (`Error loading - Will retry in N secs.`).
    fn fetch_jag_file(
        &mut self,
        progress: &mut ProgressCb<'_>,
        display: &str,
        pct: i32,
        filename: &str,
        index: usize,
        checksums: &[i32; 9],
    ) -> Option<Vec<u8>> {
        let mut wait = self.fetch_retry_wait;
        let mut retries = 0;
        loop {
            self.report_progress(progress, &format!("Requesting {display}"), pct);
            if let Some(bytes) = Self::get_jag_file(
                &self.config.cache_dir,
                &self.config.host,
                self.http_port,
                filename,
                index,
                checksums,
            ) {
                return Some(bytes);
            }
            retries += 1;
            if retries >= 10 {
                self.report_progress(progress, "Game updated - please reload page", pct);
                return None;
            }
            self.retry_countdown(progress, wait, "Error loading", pct);
            wait = (wait * 2).min(Duration::from_secs(60));
        }
    }

    /// Adopt another `Client`'s live game socket and ISAAC cursors (bothost
    /// `Lean::from_client` semantics, Client→Client): `other`'s `stream`,
    /// `random_in`, outbound cursor (`out`), inbound buffer, and
    /// partial-frame state (`ptype`/`psize`) move here. `other` must not
    /// touch the game stream afterwards (drop it or reuse it for another
    /// account); no TCP is closed. Arms `baton`, so the next
    /// `login(..., reconnect = true)` runs the opcode-18 handshake over the
    /// adopted socket instead of a fresh TCP. Returns `None` when `other`
    /// has no live stream.
    pub fn adopt_from(&mut self, other: &mut Client) -> Option<()> {
        self.stream = Some(other.stream.take()?);
        self.random_in = other.random_in.take();
        self.out = std::mem::replace(&mut other.out, Packet::alloc(1));
        self.r#in = std::mem::replace(&mut other.r#in, Packet::alloc(1));
        self.ptype = std::mem::replace(&mut other.ptype, 0);
        self.ptype0 = std::mem::replace(&mut other.ptype0, 0);
        self.ptype1 = std::mem::replace(&mut other.ptype1, 0);
        self.ptype2 = std::mem::replace(&mut other.ptype2, 0);
        self.psize = std::mem::replace(&mut other.psize, 0);
        self.baton = true;
        Some(())
    }

    /// Write the RSA plaintext block for the 274 login wrapper into
    /// `self.out`: opcode 10, the four Isaac seeds, `login_uid`, username,
    /// password. `login` encrypts it in place with `rsaenc`. Public so
    /// tests can pin the per-client uid in the wrapper bytes (the uid sits
    /// at offset 17, inside the block `rsaenc` ciphertexts).
    pub fn write_login_block(&mut self, seed: [i32; 4], username: &str, password: &str) {
        self.out.pos = 0;
        self.out.p1(10);
        self.out.p4(seed[0]);
        self.out.p4(seed[1]);
        self.out.p4(seed[2]);
        self.out.p4(seed[3]);
        self.out.p4(self.login_uid);
        self.out.pjstr(username);
        self.out.pjstr(password);
    }

    /// Login handshake, 1:1 of `Client.ts` `login` (1719-1867) / Java
    /// `Client.login`: probe, seed, RSA blob, opcode 16/18 wrapper. Response 1
    /// waits 2 s and retries the same attempt; response 2 enters the game;
    /// response 15 re-enters the game on a reconnect (`lostCon`) without
    /// replacing `localPlayer` (Java `Client.java` 3737); anything else is
    /// `LoginError` with the code and title-screen messages.
    pub fn login(
        &mut self,
        username: &str,
        password: &str,
        reconnect: bool,
    ) -> Result<(), LoginError> {
        // Headless has no title UI; persist here so `lostCon` reconnects
        // with the same credentials (TS writes these from the title fields).
        self.login_user = username.to_string();
        self.login_pass = password.to_string();
        self.last_login_reconnect = Some(reconnect);
        if !reconnect {
            self.login_mes1.clear();
            self.login_mes2 = "Connecting to server...".into();
        }

        // Socket-adopt reverse (channel-head tune): an adopted live socket
        // already holds this account's TCP (`baton`). Reuse it and run the
        // opcode-18 handshake in place so the server dumps region/player
        // state without a DC. Every other login (including `lost_con`)
        // still `connect`s a fresh socket.
        let reuse = self.baton;
        self.baton = false;
        let mut stream = if reuse {
            match self.stream.take() {
                Some(s) => s,
                None => match ClientStream::connect(&self.config.host, self.config.port) {
                    Ok(s) => s,
                    Err(_) => return Err(self.fail_title_login(io_error(), reconnect)),
                },
            }
        } else {
            match ClientStream::connect(&self.config.host, self.config.port) {
                Ok(s) => s,
                Err(_) => return Err(self.fail_title_login(io_error(), reconnect)),
            }
        };

        let userhash = JString::to_userhash(username);
        let login_server = ((userhash >> 16) & 0x1f) as i32;

        self.out.pos = 0;
        self.out.p1(14);
        self.out.p1(login_server);
        stream
            .write(self.out.data(), 2)
            .map_err(|_| self.fail_title_login(io_error(), reconnect))?;

        for _ in 0..8 {
            stream
                .read()
                .map_err(|_| self.fail_title_login(io_error(), reconnect))?;
        }
        let mut response = stream
            .read()
            .map_err(|_| self.fail_title_login(io_error(), reconnect))?;

        if response == 0 {
            stream
                .read_bytes(self.r#in.data_mut(), 0, 8)
                .map_err(|_| self.fail_title_login(io_error(), reconnect))?;
            self.r#in.pos = 0;
            let login_seed = self.r#in.g8();
            let mut seed = [
                login_random(),
                login_random(),
                (login_seed >> 32) as i32,
                (login_seed & 0xffff_ffff) as i32,
            ];

            self.write_login_block(seed, username, password);
            let (n, e) = login_rsa::active_biguints();
            self.out.rsaenc(&n, &e);

            let mut loginout = Packet::alloc(1);
            if reconnect {
                loginout.p1(18);
            } else {
                loginout.p1(16);
            }
            loginout.p1((self.out.pos + 36 + 1 + 1 + 2) as i32);
            loginout.p1(255);
            loginout.p2(CLIENT_VERSION);
            loginout.p1(if self.config.lowmem { 1 } else { 0 });
            for i in 0..9 {
                loginout.p4(self.jag_checksum[i]);
            }
            loginout.pdata(self.out.data(), 0, self.out.pos);

            self.out.random = Some(Isaac::new(&seed));
            for s in seed.iter_mut() {
                *s = s.wrapping_add(50);
            }
            self.random_in = Some(Isaac::new(&seed));

            stream
                .write(loginout.data(), loginout.pos)
                .map_err(|_| self.fail_title_login(io_error(), reconnect))?;
            response = stream
                .read()
                .map_err(|_| self.fail_title_login(io_error(), reconnect))?;
        }

        if response == 1 {
            thread::sleep(Duration::from_millis(2000));
            // old stream is dropped (closed); each attempt opens a fresh one
            return self.login(username, password, reconnect);
        }

        if response == 2 {
            self.staffmodlevel = stream
                .read()
                .map_err(|_| self.fail_title_login(io_error(), reconnect))?;
            self.mouse_tracked = stream
                .read()
                .map_err(|_| self.fail_title_login(io_error(), reconnect))?
                == 1;
            self.ingame = true;
            self.out.pos = 0;
            self.r#in.pos = 0;
            self.ptype = -1;
            self.ptype0 = -1;
            self.ptype1 = -1;
            self.ptype2 = -1;
            self.psize = 0;
            self.scene_state = 0;
            self.menu_num_entries = 0;
            self.last_response = Some(Instant::now());
            self.logout_timer = 0;
            self.no_timeout_timer = 0;
            // Java `Client.java` 3630-3699: a cold login restores the tab,
            // modals, minimap, and chat defaults a previous logout left in
            // place (`sideTab = 3`, closed modals, empty chat, no flag).
            self.active_icon = 3;
            self.side_modal_id = -1;
            self.chat_modal_id = -1;
            self.main_modal_id = -1;
            self.tut_com_id = -1;
            self.tut_flash_icon = -1;
            self.minimap_level = -1;
            self.minimap_flag_x = 0;
            self.minimap_flag_z = 0;
            for entry in self.chat_text.iter_mut() {
                *entry = String::new();
            }
            self.redraw_frame = true;
            self.redraw_side = true;
            self.redraw_icons = true;
            // Java `Client.java` 3647-3656: a cold login zeroes the entity
            // counts and nulls every player/npc slot (logout leaves the
            // tables in place), so a second login does not draw leftover
            // first-session NPCs/players.
            self.player_count = 0;
            self.npc_count = 0;
            for slot in self.players.iter_mut() {
                *slot = None;
            }
            for slot in self.npc.iter_mut() {
                *slot = None;
            }
            for slot in self.player_appearance_buffer.iter_mut() {
                *slot = None;
            }
            // Java `Client.java` 3682-3686 — cold login resets the player
            // design (TS 1883-1887: male gender, kits revalidated, colours
            // zeroed).
            self.reset_idk_design();
            // Client.ts:1889-1892 — cold login clears the player right-click
            // options (the server re-sends SET_PLAYER_OP).
            self.player_op = Default::default();
            self.player_op_priority = Default::default();
            // Client.ts:1853 — localPlayer = players[LOCAL_PLAYER_INDEX] = new
            let player = ClientPlayer::default();
            self.players[LOCAL_PLAYER_INDEX as usize] = Some(Box::new(player.clone()));
            self.local_player = Some(player);
            // Java `Client.java` 3700: `prepareGame()` rebuilds the game
            // frame the title draw consumed (Task 4b nulls the game areas,
            // so the `area_chat` gate does not fire after a title frame).
            // The renderer owns `prepare_game` (its areas are renderer
            // state); `game_draw` runs it on the first in-game frame, so
            // the sim-side eager call is not needed.
            self.stream = Some(stream);
            return Ok(());
        }

        if response == 15 {
            // Java `Client.java` 3737: reconnect grant. Same buffer/state
            // reset as response 2 minus the cold-login field init — Java
            // keeps `localPlayer`/`players` and never touches `logoutTimer`.
            self.ingame = true;
            self.out.pos = 0;
            self.r#in.pos = 0;
            self.ptype = -1;
            self.ptype0 = -1;
            self.ptype1 = -1;
            self.ptype2 = -1;
            self.psize = 0;
            self.last_response = Some(Instant::now());
            self.menu_num_entries = 0;
            self.scene_load_start_time = Instant::now();
            self.stream = Some(stream);
            return Ok(());
        }

        let (mes1, mes2): (String, String) = match response {
            3 => (String::new(), "Invalid username or password.".into()),
            4 => (
                "Your account has been disabled.".into(),
                "Please check your message-centre for details.".into(),
            ),
            5 => (
                "Your account is already logged in.".into(),
                "Try again in 60 secs...".into(),
            ),
            6 => (
                "RuneScape has been updated!".into(),
                "Wrong RSA key - run tools/redeploy.sh and rebuild.".into(),
            ),
            7 => (
                "This world is full.".into(),
                "Please use a different world.".into(),
            ),
            8 => ("Unable to connect.".into(), "Login server offline.".into()),
            9 => (
                "Login limit exceeded.".into(),
                "Too many connections from your address.".into(),
            ),
            10 => ("Unable to connect.".into(), "Bad session id.".into()),
            11 => (String::new(), "Please try again.".into()),
            12 => (
                "You need a members account to login to this world.".into(),
                "Please subscribe, or use a different world.".into(),
            ),
            13 => (
                "Could not complete login.".into(),
                "Please try using a different world.".into(),
            ),
            14 => (
                "The server is being updated.".into(),
                "Please wait 1 minute and try again.".into(),
            ),
            16 => (
                "Login attempts exceeded.".into(),
                "Please wait 1 minute and try again.".into(),
            ),
            17 => (
                "You are standing in a members-only area.".into(),
                "To play on this world move to a free area first".into(),
            ),
            20 => (
                "Invalid loginserver requested".into(),
                "Please try using a different world.".into(),
            ),
            -1 => (
                "No response from server".into(),
                "Please try using a different world.".into(),
            ),
            _ => (
                "Unexpected server response".into(),
                "Please try using a different world.".into(),
            ),
        };
        self.stream = Some(stream);
        Err(self.fail_title_login(
            LoginError {
                code: response,
                mes1,
                mes2,
            },
            reconnect,
        ))
    }

    /// Cold-login failures land on the title form (`loginscreen` 2) with
    /// the Java `loginMes` lines so CLI `--user/--pass` can retry instead
    /// of dying, and a title Login click already on screen 2 just refreshes
    /// the messages. Reconnect is `lost_con`'s problem.
    fn fail_title_login(&mut self, e: LoginError, reconnect: bool) -> LoginError {
        if !reconnect {
            self.login_mes1 = e.mes1.clone();
            self.login_mes2 = e.mes2.clone();
            self.loginscreen = 2;
        }
        e
    }

    /// Linear build-area index, `CollisionMap.index(x, z) = x * SIZE + z`.
    fn collision_index(x: i32, z: i32) -> usize {
        (x * BUILD_AREA_SIZE + z) as usize
    }

    /// Walk `dir_map` from dest back to src, recording every tile, then
    /// reverse so the path is src → dest. This is the BFS the click
    /// actually walked, not the direction-change waypoints in `route_x`.
    fn try_move_tiles(
        dir_map: &[i32],
        mut x: i32,
        mut z: i32,
        src_x: i32,
        src_z: i32,
    ) -> Vec<(i32, i32)> {
        let cap = (BUILD_AREA_SIZE * BUILD_AREA_SIZE) as usize;
        let mut tiles = Vec::with_capacity(64);
        tiles.push((x, z));
        while (x != src_x || z != src_z) && tiles.len() < cap {
            let prev = (x, z);
            let next = dir_map[Self::collision_index(x, z)];
            if next & DirectionFlag::EAST != 0 {
                x += 1;
            } else if next & DirectionFlag::WEST != 0 {
                x -= 1;
            }
            if next & DirectionFlag::NORTH != 0 {
                z += 1;
            } else if next & DirectionFlag::SOUTH != 0 {
                z -= 1;
            }
            if (x, z) == prev {
                break;
            }
            tiles.push((x, z));
        }
        tiles.reverse();
        tiles
    }

    /// 274 pack `option_run` (`varp.pack` 173, `clientcode=7`). Same id as
    /// OSRS TOGGLE_RUN. 1 = run on.
    pub const RUN_VARP: usize = 173;
    /// Controls overlay: run-off orb (`controls:com_4`).
    const RUN_ORB_OFF: usize = 152;
    /// Controls overlay: run-on orb (`controls:com_5`).
    const RUN_ORB_ON: usize = 153;

    /// Run toggle for nav debug two-tone trail. Prefers varp 173; falls
    /// back to the 274 orb hide flags (visible off-orb means run is on).
    /// Not the run animation — that is only true while a run cycle plays.
    pub fn run_enabled(&self) -> bool {
        if self.var.get(Self::RUN_VARP).copied() == Some(1) {
            return true;
        }
        let off = self.if_(Self::RUN_ORB_OFF);
        let on = self.if_(Self::RUN_ORB_ON);
        match (off, on) {
            (Some(off), Some(on)) if off.hide != on.hide => !off.hide,
            _ => false,
        }
    }

    /// Menu dispatch, port of client-ts `Client.ts` `doAction` (8548-9273).
    /// Friends/PM (`FRIENDLIST_*`, `IGNORELIST_*`, `MESSAGE_PRIVATE`) are
    /// slice 5; the anticheat oplogic counters are `Client` fields (the TS
    /// statics). The `USEHELD_START`/`TGT_BUTTON` arms return before the
    /// trailing `use_mode`/`target_mode` wipe so Use/Target stays armed.
    #[allow(non_snake_case)] // Java name kept for the RawClient mapping
    pub fn doAction(&mut self, option_id: i32) {
        if option_id < 0 {
            return;
        }

        // TS 8553-8556: an open enter-name dialog closes on any action.
        if self.dialog_input_open {
            self.dialog_input_open = false;
            self.redraw_chat = true;
        }

        let mut action = self.menu_action[option_id as usize];
        let a = self.menu_param_a[option_id as usize];
        let b = self.menu_param_b[option_id as usize];
        let c = self.menu_param_c[option_id as usize];

        if action >= MiniMenuAction::_PRIORITY {
            action -= MiniMenuAction::_PRIORITY;
        }

        if OBJ_OP_ACTIONS.contains(&action) {
            // TS 8568-8623: walk to the obj tile (with the 1x1 retry),
            // arm the crosshair, then the per-op anticheat preamble.
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let Some((px, pz)) = local_route {
                if !self.tryMove(px, pz, b, c, false, 0, 0, 0, 0, 0, 2) {
                    self.tryMove(px, pz, b, c, false, 1, 1, 0, 0, 0, 2);
                }

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                if action == MiniMenuAction::OP_OBJ1 {
                    if (b & 0x3) == 0 {
                        self.oplogic7 += 1;
                    }
                    if self.oplogic7 >= 123 {
                        self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC7.id);
                        self.out.p4(0);
                    }
                    self.out.p1_enc(ClientProt::OPOBJ1.id);
                }
                if action == MiniMenuAction::OP_OBJ2 {
                    self.out.p1_enc(ClientProt::OPOBJ2.id);
                }
                if action == MiniMenuAction::OP_OBJ3 {
                    self.out.p1_enc(ClientProt::OPOBJ3.id);
                }
                if action == MiniMenuAction::OP_OBJ4 {
                    self.oplogic8 += c;
                    if self.oplogic8 >= 75 {
                        self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC8.id);
                        self.out.p1(19);
                    }
                    self.out.p1_enc(ClientProt::OPOBJ4.id);
                }
                if action == MiniMenuAction::OP_OBJ5 {
                    self.oplogic3 += self.map_build_base_z;
                    if self.oplogic3 >= 118 {
                        self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC3.id);
                        self.out.p4(0);
                    }
                    self.out.p1_enc(ClientProt::OPOBJ5.id);
                }

                self.out.p2(b + self.map_build_base_x);
                self.out.p2(c + self.map_build_base_z);
                self.out.p2(a);
            }
        }

        if action == MiniMenuAction::OP_OBJ6 {
            let obj = self.cache.objs.get(a as usize).cloned().unwrap_or_default();
            let examine = if obj.desc.is_empty() {
                format!("It's a {}.", obj.name)
            } else {
                obj.desc
            };
            self.add_chat(0, &examine, "");
        }

        if action == MiniMenuAction::TGT_OBJ {
            // TS 8638-8654: walk, crosshair, then OPOBJT.
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let Some((px, pz)) = local_route {
                if !self.tryMove(px, pz, b, c, false, 0, 0, 0, 0, 0, 2) {
                    self.tryMove(px, pz, b, c, false, 1, 1, 0, 0, 0, 2);
                }

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                self.out.p1_enc(ClientProt::OPOBJT.id);
                self.out.p2(b + self.map_build_base_x);
                self.out.p2(c + self.map_build_base_z);
                self.out.p2(a);
                self.out.p2(self.target_com_id);
            }
        }

        if action == MiniMenuAction::USEHELD_ONOBJ {
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let Some((px, pz)) = local_route {
                if !self.tryMove(px, pz, b, c, false, 0, 0, 0, 0, 0, 2) {
                    self.tryMove(px, pz, b, c, false, 1, 1, 0, 0, 0, 2);
                }

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                self.out.p1_enc(ClientProt::OPOBJU.id);
                self.out.p2(b + self.map_build_base_x);
                self.out.p2(c + self.map_build_base_z);
                self.out.p2(a);
                self.out.p2(self.obj_com_id);
                self.out.p2(self.obj_selected_slot);
                self.out.p2(self.obj_selected_com_id);
            }
        }

        if action == MiniMenuAction::OP_NPC1
            || action == MiniMenuAction::OP_NPC2
            || action == MiniMenuAction::OP_NPC3
            || action == MiniMenuAction::OP_NPC4
            || action == MiniMenuAction::OP_NPC5
        {
            // The original guards on the NPC entity and walks to its tile
            // first (MOVE_OPCLICK), then encodes the action opcode.
            let npc_route = self
                .npc
                .get(a as usize)
                .and_then(|n| n.as_ref())
                .map(|n| (n.route_x[0], n.route_z[0]));
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let (Some((npc_x, npc_z)), Some((px, pz))) = (npc_route, local_route) {
                self.tryMove(px, pz, npc_x, npc_z, false, 1, 1, 0, 0, 0, 2);

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                let opcode = match action {
                    MiniMenuAction::OP_NPC1 => ClientProt::OPNPC1.id,
                    MiniMenuAction::OP_NPC2 => ClientProt::OPNPC2.id,
                    MiniMenuAction::OP_NPC3 => ClientProt::OPNPC3.id,
                    MiniMenuAction::OP_NPC4 => ClientProt::OPNPC4.id,
                    _ => ClientProt::OPNPC5.id,
                };
                self.out.p1_enc(opcode);
                self.out.p2(a);
            }
        }

        if action == MiniMenuAction::OP_NPC6 {
            let examine = self
                .npc
                .get(a as usize)
                .and_then(|n| n.as_ref())
                .and_then(|n| n.r#type)
                .and_then(|id| self.cache.npcs.get(id))
                .map(|t| {
                    if t.desc.is_empty() {
                        format!("It's a {}.", t.name)
                    } else {
                        t.desc.clone()
                    }
                });
            if let Some(examine) = examine {
                self.add_chat(0, &examine, "");
            }
        }

        if action == MiniMenuAction::TGT_NPC {
            let npc_route = self
                .npc
                .get(a as usize)
                .and_then(|n| n.as_ref())
                .map(|n| (n.route_x[0], n.route_z[0]));
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let (Some((npc_x, npc_z)), Some((px, pz))) = (npc_route, local_route) {
                self.tryMove(px, pz, npc_x, npc_z, false, 1, 1, 0, 0, 0, 2);

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                self.out.p1_enc(ClientProt::OPNPCT.id);
                self.out.p2(a);
                self.out.p2(self.target_com_id);
            }
        }

        if action == MiniMenuAction::USEHELD_ONNPC {
            let npc_route = self
                .npc
                .get(a as usize)
                .and_then(|n| n.as_ref())
                .map(|n| (n.route_x[0], n.route_z[0]));
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let (Some((npc_x, npc_z)), Some((px, pz))) = (npc_route, local_route) {
                self.tryMove(px, pz, npc_x, npc_z, false, 1, 1, 0, 0, 0, 2);

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                self.out.p1_enc(ClientProt::OPNPCU.id);
                self.out.p2(a);
                self.out.p2(self.obj_com_id);
                self.out.p2(self.obj_selected_slot);
                self.out.p2(self.obj_selected_com_id);
            }
        }

        if action == MiniMenuAction::OP_LOC1 {
            self.interact_with_loc(b, c, a, ClientProt::OPLOC1.id);
        }

        if action == MiniMenuAction::OP_LOC2 {
            self.oplogic1 += c;
            if self.oplogic1 >= 139 {
                self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC1.id);
                self.out.p4(0);
            }
            self.interact_with_loc(b, c, a, ClientProt::OPLOC2.id);
        }

        if action == MiniMenuAction::OP_LOC3 {
            self.oplogic2 += 1;
            if self.oplogic2 >= 124 {
                self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC2.id);
                self.out.p2(37954);
            }
            self.interact_with_loc(b, c, a, ClientProt::OPLOC3.id);
        }

        if action == MiniMenuAction::OP_LOC4 {
            self.interact_with_loc(b, c, a, ClientProt::OPLOC4.id);
        }

        if action == MiniMenuAction::OP_LOC5 {
            self.interact_with_loc(b, c, a, ClientProt::OPLOC5.id);
        }

        if action == MiniMenuAction::OP_LOC6 {
            let loc_id = (a >> 14) & 0x7fff;
            let examine = self.cache.locs.get(loc_id as usize).map(|loc| {
                if loc.desc.is_empty() {
                    format!("It's a {}.", loc.name)
                } else {
                    loc.desc.clone()
                }
            });
            if let Some(examine) = examine {
                self.add_chat(0, &examine, "");
            }
        }

        if action == MiniMenuAction::TGT_LOC
            && self.interact_with_loc(b, c, a, ClientProt::OPLOCT.id)
        {
            self.out.p2(self.target_com_id);
        }

        if action == MiniMenuAction::USEHELD_ONLOC
            && self.interact_with_loc(b, c, a, ClientProt::OPLOCU.id)
        {
            self.out.p2(self.obj_com_id);
            self.out.p2(self.obj_selected_slot);
            self.out.p2(self.obj_selected_com_id);
        }

        if PLAYER_OP_ACTIONS.contains(&action) {
            // TS 8824-8868: walk to the player, crosshair, then the
            // per-op anticheat preamble and the player index.
            let player_route = self
                .players
                .get(a as usize)
                .and_then(|p| p.as_ref())
                .map(|p| (p.route_x[0], p.route_z[0]));
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let (Some((tx, tz)), Some((px, pz))) = (player_route, local_route) {
                self.tryMove(px, pz, tx, tz, false, 1, 1, 0, 0, 0, 2);

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                if action == MiniMenuAction::OP_PLAYER1 {
                    self.oplogic4 += 1;
                    if self.oplogic4 >= 52 {
                        self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC4.id);
                        self.out.p1(131);
                    }
                    self.out.p1_enc(ClientProt::OPPLAYER1.id);
                }
                if action == MiniMenuAction::OP_PLAYER2 {
                    self.out.p1_enc(ClientProt::OPPLAYER2.id);
                }
                if action == MiniMenuAction::OP_PLAYER3 {
                    self.out.p1_enc(ClientProt::OPPLAYER3.id);
                }
                if action == MiniMenuAction::OP_PLAYER4 {
                    self.oplogic5 += a;
                    if self.oplogic5 >= 66 {
                        self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC5.id);
                        self.out.p1(154);
                    }
                    self.out.p1_enc(ClientProt::OPPLAYER4.id);
                }
                if action == MiniMenuAction::OP_PLAYER5 {
                    self.out.p1_enc(ClientProt::OPPLAYER5.id);
                }
                self.out.p2(a);
            }
        }

        if action == MiniMenuAction::ACCEPT_TRADEREQ || action == MiniMenuAction::ACCEPT_DUELREQ {
            // TS 8870-8914: the `@whi@`-tagged option name resolves to the
            // matching tracked player; ACCEPT_TRADEREQ is OPPPLAYER4,
            // ACCEPT_DUELREQ OPPPLAYER1.
            let option = self.menu_option[option_id as usize].clone();
            if let Some(tag) = option.find("@whi@") {
                let name = option[tag + 5..].trim().to_string();
                let name = JString::to_screen_name(&JString::to_raw_username(
                    JString::to_userhash(&name) as i64,
                ));
                let mut found = false;
                for i in 0..self.player_count as usize {
                    let index = self.player_ids[i] as usize;
                    let player_route = self
                        .players
                        .get(index)
                        .and_then(|p| p.as_ref())
                        .filter(|p| {
                            p.name
                                .as_deref()
                                .is_some_and(|n| n.to_lowercase() == name.to_lowercase())
                        })
                        .map(|p| (p.route_x[0], p.route_z[0]));
                    let local_route = self
                        .local_player
                        .as_ref()
                        .map(|p| (p.route_x[0], p.route_z[0]));
                    if let (Some((tx, tz)), Some((px, pz))) = (player_route, local_route) {
                        self.tryMove(px, pz, tx, tz, false, 1, 1, 0, 0, 0, 2);

                        if action == MiniMenuAction::ACCEPT_TRADEREQ {
                            self.oplogic5 += a;
                            if self.oplogic5 >= 66 {
                                self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC5.id);
                                self.out.p1(154);
                            }
                            self.out.p1_enc(ClientProt::OPPLAYER4.id);
                        }
                        if action == MiniMenuAction::ACCEPT_DUELREQ {
                            self.oplogic4 += 1;
                            if self.oplogic4 >= 52 {
                                self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC4.id);
                                self.out.p1(131);
                            }
                            self.out.p1_enc(ClientProt::OPPLAYER1.id);
                        }
                        self.out.p2(index as i32);
                        found = true;
                        break;
                    }
                }
                if !found {
                    self.add_chat(0, &format!("Unable to find {name}"), "");
                }
            }
        }

        if action == MiniMenuAction::TGT_PLAYER {
            let player_route = self
                .players
                .get(a as usize)
                .and_then(|p| p.as_ref())
                .map(|p| (p.route_x[0], p.route_z[0]));
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let (Some((tx, tz)), Some((px, pz))) = (player_route, local_route) {
                self.tryMove(px, pz, tx, tz, false, 1, 1, 0, 0, 0, 2);

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                self.out.p1_enc(ClientProt::OPPLAYERT.id);
                self.out.p2(a);
                self.out.p2(self.target_com_id);
            }
        }

        if action == MiniMenuAction::USEHELD_ONPLAYER {
            let player_route = self
                .players
                .get(a as usize)
                .and_then(|p| p.as_ref())
                .map(|p| (p.route_x[0], p.route_z[0]));
            let local_route = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            if let (Some((tx, tz)), Some((px, pz))) = (player_route, local_route) {
                self.tryMove(px, pz, tx, tz, false, 1, 1, 0, 0, 0, 2);

                self.cross_x = self.shell.mouse_click_x;
                self.cross_y = self.shell.mouse_click_y;
                self.cross_mode = 2;
                self.cross_cycle = 0;

                self.out.p1_enc(ClientProt::OPPLAYERU.id);
                self.out.p2(a);
                self.out.p2(self.obj_com_id);
                self.out.p2(self.obj_selected_slot);
                self.out.p2(self.obj_selected_com_id);
            }
        }

        if action == MiniMenuAction::OP_HELD1
            || action == MiniMenuAction::OP_HELD2
            || action == MiniMenuAction::OP_HELD3
            || action == MiniMenuAction::OP_HELD4
            || action == MiniMenuAction::OP_HELD5
        {
            // TS 8956-8997: p2(obj) p2(slot) p2(com), then the selected
            // outline fields.
            if action == MiniMenuAction::OP_HELD1 {
                self.out.p1_enc(ClientProt::OPHELD1.id);
            }
            if action == MiniMenuAction::OP_HELD2 {
                self.out.p1_enc(ClientProt::OPHELD2.id);
            }
            if action == MiniMenuAction::OP_HELD3 {
                self.out.p1_enc(ClientProt::OPHELD3.id);
            }
            if action == MiniMenuAction::OP_HELD4 {
                self.oplogic9 += 1;
                if self.oplogic9 >= 116 {
                    self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC9.id);
                    self.out.p3(13018169);
                }
                self.out.p1_enc(ClientProt::OPHELD4.id);
            }
            if action == MiniMenuAction::OP_HELD5 {
                self.out.p1_enc(ClientProt::OPHELD5.id);
            }
            self.out.p2(a);
            self.out.p2(b);
            self.out.p2(c);
            self.mark_selected(b, c);
        }

        if action == MiniMenuAction::OP_HELD6 {
            let obj = self.cache.objs.get(a as usize).cloned().unwrap_or_default();
            // TS 8999-9010: a com-link count >= 100000 reports "<n> x <name>"
            let examine = self
                .if_(c as usize)
                .and_then(|com| com.link_obj_number.as_ref())
                .and_then(|numbers| numbers.get(b as usize).copied())
                .filter(|&n| n >= 100000)
                .map(|n| format!("{n} x {}", obj.name))
                .unwrap_or_else(|| {
                    if obj.desc.is_empty() {
                        format!("It's a {}.", obj.name)
                    } else {
                        obj.desc
                    }
                });
            self.add_chat(0, &examine, "");
        }

        if action == MiniMenuAction::USEHELD_START {
            // TS 9013-9022: arms Use mode and returns before the wipe.
            self.use_mode = 1;
            self.obj_selected_slot = b;
            self.obj_selected_com_id = c;
            self.obj_com_id = a;
            self.obj_selected_name = self
                .cache
                .objs
                .get(a as usize)
                .map(|o| o.name.clone())
                .unwrap_or_default();
            self.target_mode = 0;
            self.redraw_side = true;
            return;
        }

        if action == MiniMenuAction::TGT_BUTTON {
            // TS 9024-9050: target mode for the spell, `targetOp` from the
            // verb prefix/suffix and base; returns before the wipe. The
            // reads run before the stores below (a view borrows the whole
            // client).
            let target_mask = self.if_(c as usize).map(|com| com.target_mask).unwrap_or(0);
            let (prefix, suffix) = self
                .if_(c as usize)
                .map(|com| {
                    let verb = com.target_verb.clone();
                    match verb.find(' ') {
                        Some(space) => (verb[..space].to_string(), verb[space + 1..].to_string()),
                        None => (verb.clone(), verb),
                    }
                })
                .unwrap_or_default();
            let base = self
                .if_(c as usize)
                .map(|com| com.target_base.clone())
                .unwrap_or_default();
            self.target_mode = 1;
            self.target_com_id = c;
            self.target_mask = target_mask;
            self.use_mode = 0;
            self.redraw_side = true;
            self.target_op = format!("{prefix} {base} {suffix}");

            if self.target_mask == 0x10 {
                self.redraw_side = true;
                self.active_icon = 3;
                self.redraw_icons = true;
            }
            return;
        }

        if action == MiniMenuAction::TGT_HELD {
            self.out.p1_enc(ClientProt::OPHELDT.id);
            self.out.p2(a);
            self.out.p2(b);
            self.out.p2(c);
            self.out.p2(self.target_com_id);
            self.mark_selected(b, c);
        }

        if action == MiniMenuAction::USEHELD_ONHELD {
            self.out.p1_enc(ClientProt::OPHELDU.id);
            self.out.p2(a);
            self.out.p2(b);
            self.out.p2(c);
            self.out.p2(self.obj_com_id);
            self.out.p2(self.obj_selected_slot);
            self.out.p2(self.obj_selected_com_id);
            self.mark_selected(b, c);
        }

        if action == MiniMenuAction::INV_BUTTON1
            || action == MiniMenuAction::INV_BUTTON2
            || action == MiniMenuAction::INV_BUTTON3
            || action == MiniMenuAction::INV_BUTTON4
            || action == MiniMenuAction::INV_BUTTON5
        {
            // TS 9096-9141: p2(obj) p2(slot) p2(com), then the selected
            // outline fields.
            if action == MiniMenuAction::INV_BUTTON1 {
                if (a & 0x3) == 0 {
                    self.oplogic6 += 1;
                }
                if self.oplogic6 >= 133 {
                    self.out.p1_enc(ClientProt::ANTICHEAT_OPLOGIC6.id);
                    self.out.p2(6118);
                }
                self.out.p1_enc(ClientProt::INV_BUTTON1.id);
            }
            if action == MiniMenuAction::INV_BUTTON2 {
                self.out.p1_enc(ClientProt::INV_BUTTON2.id);
            }
            if action == MiniMenuAction::INV_BUTTON3 {
                self.out.p1_enc(ClientProt::INV_BUTTON3.id);
            }
            if action == MiniMenuAction::INV_BUTTON4 {
                self.out.p1_enc(ClientProt::INV_BUTTON4.id);
            }
            if action == MiniMenuAction::INV_BUTTON5 {
                self.out.p1_enc(ClientProt::INV_BUTTON5.id);
            }
            self.out.p2(a);
            self.out.p2(b);
            self.out.p2(c);
            self.mark_selected(b, c);
        }

        if action == MiniMenuAction::IF_BUTTON {
            // TS 9144-9154: `clientButton` runs for any positive client
            // code and can veto the send. Handled codes either open a
            // prompt (social), arm a timer (logout) or cycle the design
            // kit; the logout/accept-design arms return `true` so the
            // click is sent, everything else returns `false`.
            let client_code = self
                .if_(c as usize)
                .map(|com| com.client_code)
                .unwrap_or(-1);
            let mut notify = true;
            if client_code > 0 {
                notify = self.client_button(c);
            }
            if notify {
                self.out.p1_enc(ClientProt::IF_BUTTON.id);
                self.out.p2(c);
            }
        }

        if action == MiniMenuAction::TOGGLE_BUTTON {
            self.out.p1_enc(ClientProt::IF_BUTTON.id);
            self.out.p2(c);
            // An owned script copy: the varp writes below need `&mut self`
            // while the script is read (the view borrow would conflict).
            let script = self
                .if_(c as usize)
                .and_then(|com| com.scripts.as_ref().and_then(|s| s.first()).cloned());
            if let Some(script) = script {
                // TS 9163-9169: scripts[0][0] == 5 flips varp scripts[0][1].
                if script.first() == Some(&5) {
                    let varp = script.get(1).copied().unwrap_or(0);
                    let current = self.var.get(varp as usize).copied().unwrap_or(0);
                    grow_write(&mut self.var, varp, 1 - current);
                    self.client_var(varp);
                    self.redraw_side = true;
                }
            }
        }

        if action == MiniMenuAction::SELECT_BUTTON {
            self.out.p1_enc(ClientProt::IF_BUTTON.id);
            self.out.p2(c);
            let script = self
                .if_(c as usize)
                .and_then(|com| com.scripts.as_ref().and_then(|s| s.first()).cloned());
            let operand = self
                .if_(c as usize)
                .and_then(|com| com.script_operand.as_ref().and_then(|o| o.first()).copied());
            if let Some(script) = script {
                // TS 9172-9183: scripts[0][0] == 5 sets varp scripts[0][1]
                // to scriptOperand[0] when it differs.
                if script.first() == Some(&5) {
                    let varp = script.get(1).copied().unwrap_or(0);
                    if let Some(operand) = operand {
                        if self.var.get(varp as usize).copied() != Some(operand) {
                            grow_write(&mut self.var, varp, operand);
                            self.client_var(varp);
                            self.redraw_side = true;
                        }
                    }
                }
            }
        }

        if action == MiniMenuAction::PAUSE_BUTTON {
            // TS 9186-9191: RESUME_PAUSEBUTTON, not IF_BUTTON.
            if !self.resumed_pause_button {
                self.out.p1_enc(ClientProt::RESUME_PAUSEBUTTON.id);
                self.out.p2(c);
                self.resumed_pause_button = true;
            }
        }

        if action == MiniMenuAction::CLOSE_BUTTON {
            self.close_modal();
        }

        if action == MiniMenuAction::ABUSE_REPORT {
            let option = self.menu_option[option_id as usize].clone();
            if let Some(tag) = option.find("@whi@") {
                self.close_modal();
                self.report_abuse_input = option[tag + 5..].trim().to_string();
                self.report_abuse_mute_option = false;
                let report = self
                    .ifaces_merged()
                    .find(|com| com.client_code == CC_REPORT_INPUT)
                    .map(|com| com.layer_id);
                if let Some(layer_id) = report {
                    self.report_abuse_com_id = layer_id;
                    self.main_modal_id = layer_id;
                }
            }
        }

        if action == MiniMenuAction::WALK {
            // `World.updateMousePicking` from Client.ts doAction (9217-9222):
            // the menu-open row click uses the stored param coords; the
            // closed-menu last-entry path uses the live click. The ground
            // answer is consumed into MOVE_GAMECLICK by `game_loop`.
            if self.is_menu_open {
                self.world.update_mouse_picking(b - 4, c - 4);
            } else {
                self.world.update_mouse_picking(
                    self.shell.mouse_click_x - 4,
                    self.shell.mouse_click_y - 4,
                );
            }
        }

        if action == MiniMenuAction::FRIENDLIST_ADD
            || action == MiniMenuAction::IGNORELIST_ADD
            || action == MiniMenuAction::FRIENDLIST_DEL
            || action == MiniMenuAction::IGNORELIST_DEL
        {
            // TS 9226-9240: the `@whi@`-tagged option name resolves to the
            // userhash of the target player.
            let option = self.menu_option[option_id as usize].clone();
            if let Some(tag) = option.find("@whi@") {
                let username = JString::to_userhash(option[tag + 5..].trim()) as i64;
                if action == MiniMenuAction::FRIENDLIST_ADD {
                    self.add_friend(username);
                } else if action == MiniMenuAction::IGNORELIST_ADD {
                    self.add_ignore(username);
                } else if action == MiniMenuAction::FRIENDLIST_DEL {
                    self.del_friend(username);
                } else if action == MiniMenuAction::IGNORELIST_DEL {
                    self.del_ignore(username);
                }
            }
        }

        if action == MiniMenuAction::MESSAGE_PRIVATE {
            // TS 9242-9266: open the PM social prompt only when the
            // resolved friend is online (`friendNodeId > 0`).
            let option = self.menu_option[option_id as usize].clone();
            if let Some(tag) = option.find("@whi@") {
                let userhash = JString::to_userhash(option[tag + 5..].trim()) as i64;
                let mut friend = -1;
                for i in 0..self.friend_count {
                    if self.friend_userhash[i as usize] == userhash {
                        friend = i;
                        break;
                    }
                }
                if friend != -1 && self.friend_node_id[friend as usize] > 0 {
                    self.redraw_chat = true;
                    self.dialog_input_open = false;
                    self.social_input_open = true;
                    self.social_input = String::new();
                    self.social_input_type = 3;
                    self.social_userhash = self.friend_userhash[friend as usize];
                    self.social_input_header = format!(
                        "Enter message to send to {}",
                        self.friend_username[friend as usize]
                    );
                }
            }
        }

        self.use_mode = 0;
        self.target_mode = 0;
        self.redraw_side = true;
    }

    /// `interactWithLoc` from client-ts (5535-5606): resolve the loc id
    /// from the pick typecode, walk to its tile with the shape/angle-aware
    /// `tryMove` arguments, then write `p1_enc(opcode) p2(x + base)
    /// p2(z + base) p2(locId)`. The `ANTICHEAT_CYCLELOGIC2` blob's
    /// `Math.random` payload is written with fixed values (deterministic;
    /// the threshold is far beyond any test run).
    #[allow(non_snake_case)] // Java name kept for the RawClient mapping
    fn interact_with_loc(&mut self, x: i32, z: i32, typecode: i32, opcode: i32) -> bool {
        let Some((px, pz)) = self
            .local_player
            .as_ref()
            .map(|p| (p.route_x[0], p.route_z[0]))
        else {
            return false;
        };

        let loc_id = (typecode >> 14) & 0x7fff;
        let info = self.world.type_code2(self.minusedlevel, x, z, typecode);
        if info == -1 {
            return false;
        }

        let shape = info & 0x1f;
        let angle = (info >> 6) & 0x3;

        self.cyclelogic2 += 1;
        if self.cyclelogic2 > 1086 {
            self.cyclelogic2 = 0;
            self.out.p1_enc(ClientProt::ANTICHEAT_CYCLELOGIC2.id);
            self.out.p1(0);
            let start = self.out.pos;
            // the Math.random draws become 0 and both 2.0-roll conditionals
            // take their first branch (TS 5554-5568), kept deterministic
            self.out.p2(16791);
            self.out.p1(254);
            self.out.p2(0);
            self.out.p2(16128);
            self.out.p2(52610);
            self.out.p2(0);
            self.out.p2(55420);
            self.out.p2(35025);
            self.out.p2(46628);
            self.out.p1(0);
            self.out.psize1((self.out.pos - start) as i32);
        }

        if shape == LocShape::CENTREPIECE_STRAIGHT
            || shape == LocShape::CENTREPIECE_DIAGONAL
            || shape == LocShape::GROUND_DECOR
        {
            let (loc_width, loc_length, loc_forceapproach) = self
                .cache
                .locs
                .get(loc_id as usize)
                .map(|loc| (loc.width, loc.length, loc.forceapproach))
                .unwrap_or((0, 0, 0));
            let (width, height) = if angle == LocAngle::WEST || angle == LocAngle::EAST {
                (loc_width, loc_length)
            } else {
                (loc_length, loc_width)
            };
            let mut forceapproach = loc_forceapproach;
            if angle != 0 {
                forceapproach = ((forceapproach << angle) & 0xf) + (forceapproach >> (4 - angle));
            }
            self.tryMove(px, pz, x, z, false, width, height, 0, 0, forceapproach, 2);
        } else {
            self.tryMove(px, pz, x, z, false, 0, 0, angle, shape + 1, 0, 2);
        }

        self.cross_x = self.shell.mouse_click_x;
        self.cross_y = self.shell.mouse_click_y;
        self.cross_mode = 2;
        self.cross_cycle = 0;

        self.out.p1_enc(opcode);
        self.out.p2(x + self.map_build_base_x);
        self.out.p2(z + self.map_build_base_z);
        self.out.p2(loc_id);
        true
    }

    /// `selectedArea`/`selectedComId`/`selectedItem`/`selectedCycle`
    /// write-back shared by the OP_HELD/TGT_HELD/USEHELD_ONHELD/INV_BUTTON
    /// arms (TS 8981-8996 etc.): default area 2, then 1 or 3 when the
    /// component's layer is the main or chat modal.
    fn mark_selected(&mut self, b: i32, c: i32) {
        self.selected_cycle = 0;
        self.selected_com_id = c;
        self.selected_item = b;
        self.selected_area = 2;
        let layer_id = self.if_(c as usize).map(|com| com.layer_id);
        if layer_id == Some(self.main_modal_id) {
            self.selected_area = 1;
        }
        if layer_id == Some(self.chat_modal_id) {
            self.selected_area = 3;
        }
    }

    /// Walk-path encode, port of client-ts `Client.ts` `tryMove` (5608-5869)
    /// against `collision[minusedlevel]`: the BFS honours the `CollisionMap`
    /// flags and the loc-shape / loc-width arrival shortcuts
    /// (`testWall`/`testWDecor`/`testLoc`), then writes the
    /// MOVE_GAMECLICK / MOVE_MINIMAPCLICK / MOVE_OPCLICK packet as the
    /// original. `r#type` matches the TS `type` parameter (0 walk, 1 minimap,
    /// 2 op).
    #[allow(non_snake_case)] // Java name kept for the RawClient mapping
    #[allow(clippy::too_many_arguments)] // Java/TS tryMove signature is fixed
    pub fn tryMove(
        &mut self,
        src_x: i32,
        src_z: i32,
        dx: i32,
        dz: i32,
        try_nearest: bool,
        loc_width: i32,
        loc_length: i32,
        loc_angle: i32,
        loc_shape: i32,
        forceapproach: i32,
        r#type: i32,
    ) -> bool {
        let collision_map = &self.collision[self.minusedlevel as usize];
        let scene_width = BuildArea::SIZE;
        let scene_length = BuildArea::SIZE;

        for x in 0..scene_width {
            for z in 0..scene_length {
                let index = Self::collision_index(x, z);
                self.dir_map[index] = 0;
                self.dist_map[index] = 99999999;
            }
        }

        let mut x = src_x;
        let mut z = src_z;

        self.dir_map[Self::collision_index(src_x, src_z)] = 99;
        self.dist_map[Self::collision_index(src_x, src_z)] = 0;

        let mut steps: usize = 0;
        let mut length: usize = 0;
        self.route_x[steps] = src_x;
        self.route_z[steps] = src_z;
        steps += 1;

        let mut arrived = false;
        let buffer_size = self.route_x.len();
        let flags = &collision_map.flags;

        while length != steps {
            x = self.route_x[length];
            z = self.route_z[length];
            length = (length + 1) % buffer_size;

            if x == dx && z == dz {
                arrived = true;
                break;
            }

            if loc_shape != LocShape::WALL_STRAIGHT {
                if (loc_shape < LocShape::WALLDECOR_STRAIGHT_OFFSET
                    || loc_shape == LocShape::CENTREPIECE_STRAIGHT)
                    && collision_map.test_wall(x, z, dx, dz, loc_shape - 1, loc_angle)
                {
                    arrived = true;
                    break;
                }

                if loc_shape < LocShape::CENTREPIECE_STRAIGHT
                    && collision_map.test_w_decor(x, z, dx, dz, loc_shape - 1, loc_angle)
                {
                    arrived = true;
                    break;
                }
            }

            if loc_width != 0
                && loc_length != 0
                && collision_map.test_loc(x, z, dx, dz, loc_width, loc_length, forceapproach)
            {
                arrived = true;
                break;
            }

            let next_cost = self.dist_map[Self::collision_index(x, z)] + 1;

            if x > 0 {
                let index = Self::collision_index(x - 1, z);
                if self.dir_map[index] == 0
                    && (flags[x as usize - 1][z as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x - 1;
                    self.route_z[steps] = z;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 2;
                    self.dist_map[index] = next_cost;
                }
            }

            if x < scene_width - 1 {
                let index = Self::collision_index(x + 1, z);
                if self.dir_map[index] == 0
                    && (flags[x as usize + 1][z as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x + 1;
                    self.route_z[steps] = z;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 8;
                    self.dist_map[index] = next_cost;
                }
            }

            if z > 0 {
                let index = Self::collision_index(x, z - 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize][z as usize - 1] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x;
                    self.route_z[steps] = z - 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 1;
                    self.dist_map[index] = next_cost;
                }
            }

            if z < scene_length - 1 {
                let index = Self::collision_index(x, z + 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize][z as usize + 1] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x;
                    self.route_z[steps] = z + 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 4;
                    self.dist_map[index] = next_cost;
                }
            }

            if x > 0 && z > 0 {
                let index = Self::collision_index(x - 1, z - 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize - 1][z as usize - 1] & CollisionFlag::PL_WALK_NE) == 0
                    && (flags[x as usize - 1][z as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize - 1] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x - 1;
                    self.route_z[steps] = z - 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 3;
                    self.dist_map[index] = next_cost;
                }
            }

            if x < scene_width - 1 && z > 0 {
                let index = Self::collision_index(x + 1, z - 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize + 1][z as usize - 1] & CollisionFlag::PL_WALK_NW) == 0
                    && (flags[x as usize + 1][z as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize - 1] & CollisionFlag::PL_WALK_N)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x + 1;
                    self.route_z[steps] = z - 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 9;
                    self.dist_map[index] = next_cost;
                }
            }

            if x > 0 && z < scene_length - 1 {
                let index = Self::collision_index(x - 1, z + 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize - 1][z as usize + 1] & CollisionFlag::PL_WALK_SE) == 0
                    && (flags[x as usize - 1][z as usize] & CollisionFlag::PL_WALK_E)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize + 1] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x - 1;
                    self.route_z[steps] = z + 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 6;
                    self.dist_map[index] = next_cost;
                }
            }

            if x < scene_width - 1 && z < scene_length - 1 {
                let index = Self::collision_index(x + 1, z + 1);
                if self.dir_map[index] == 0
                    && (flags[x as usize + 1][z as usize + 1] & CollisionFlag::PL_WALK_SW) == 0
                    && (flags[x as usize + 1][z as usize] & CollisionFlag::PL_WALK_W)
                        == CollisionFlag::_OPEN
                    && (flags[x as usize][z as usize + 1] & CollisionFlag::PL_WALK_S)
                        == CollisionFlag::_OPEN
                {
                    self.route_x[steps] = x + 1;
                    self.route_z[steps] = z + 1;
                    steps = (steps + 1) % buffer_size;
                    self.dir_map[index] = 12;
                    self.dist_map[index] = next_cost;
                }
            }
        }

        self.try_move_nearest = 0;

        if !arrived {
            if try_nearest {
                let mut min = 100;
                for padding in 1..2 {
                    for px in (dx - padding)..=(dx + padding) {
                        for pz in (dz - padding)..=(dz + padding) {
                            if px >= 0 && pz >= 0 && px < scene_width && pz < scene_length {
                                let index = Self::collision_index(px, pz);
                                if self.dist_map[index] < min {
                                    min = self.dist_map[index];
                                    x = px;
                                    z = pz;
                                    self.try_move_nearest = 1;
                                    arrived = true;
                                }
                            }
                        }
                    }
                    if arrived {
                        break;
                    }
                }
            }

            if !arrived {
                return false;
            }
        }

        // Full BFS tile list (every step dest→src, reversed to src→dest)
        // for nav debug. The waypoint loop below only records direction
        // changes for the MOVE packet.
        self.try_move_path = Self::try_move_tiles(&self.dir_map, x, z, src_x, src_z);

        length = 0;
        self.route_x[length] = x;
        self.route_z[length] = z;
        length += 1;

        let mut dir = self.dir_map[Self::collision_index(x, z)];
        let mut next = dir;
        while x != src_x || z != src_z {
            if next != dir {
                dir = next;
                self.route_x[length] = x;
                self.route_z[length] = z;
                length += 1;
            }

            if next & DirectionFlag::EAST != 0 {
                x += 1;
            } else if next & DirectionFlag::WEST != 0 {
                x -= 1;
            }

            if next & DirectionFlag::NORTH != 0 {
                z += 1;
            } else if next & DirectionFlag::SOUTH != 0 {
                z -= 1;
            }

            next = self.dir_map[Self::collision_index(x, z)];
        }

        if length > 0 {
            // max number of turns in a single pf request
            let buffer_size = length.min(25);
            length -= 1;

            let start_x = self.route_x[length];
            let start_z = self.route_z[length];

            match r#type {
                0 => {
                    self.out.p1_enc(ClientProt::MOVE_GAMECLICK.id);
                    self.out.p1((buffer_size + buffer_size + 3) as i32);
                }
                1 => {
                    self.out.p1_enc(ClientProt::MOVE_MINIMAPCLICK.id);
                    self.out.p1((buffer_size + buffer_size + 3 + 14) as i32);
                }
                2 => {
                    self.out.p1_enc(ClientProt::MOVE_OPCLICK.id);
                    self.out.p1((buffer_size + buffer_size + 3) as i32);
                }
                _ => {}
            }

            if self.shell.key_held[5] == 1 {
                self.out.p1(1);
            } else {
                self.out.p1(0);
            }

            self.out.p2(start_x + self.map_build_base_x);
            self.out.p2(start_z + self.map_build_base_z);

            self.minimap_flag_x = self.route_x[0];
            self.minimap_flag_z = self.route_z[0];

            let mut i = 1;
            while i < buffer_size {
                length -= 1;
                self.out.p1(self.route_x[length] - start_x);
                self.out.p1(self.route_z[length] - start_z);
                i += 1;
            }

            return true;
        }

        r#type != 1
    }

    /// In-game inbound read, 1:1 of `Client.ts` `tcpIn` (5871-7150) on the
    /// Java-style blocking stream: Isaac-decoded `ptype`, `psize` from
    /// `SERVER_PROT_SIZES` (-1/-2 variable-length forms), full-payload read,
    /// then the `ptype` switch. Returns false when no complete frame is
    /// available yet; `gameLoop` drives it up to 5 times per frame.
    pub fn tcp_in(&mut self) -> bool {
        match self.read_packet() {
            Ok(true) => {
                // `handle_packet` takes the payload out of `self` (it is
                // callable with an external packet in tests), so swap the
                // `in` buffer out for the dispatch and put it back.
                let ptype = self.ptype;
                let mut packet = std::mem::replace(&mut self.r#in, Packet::alloc(1));
                self.handle_packet(ptype, &mut packet);
                self.r#in = packet;
                true
            }
            Ok(false) => false,
            Err(e) => {
                if e.kind() == io::ErrorKind::Other {
                    // Java `catch (Exception)`: report and log out. The only
                    // `Other` error `read_packet` produces is the oversized
                    // psize that the Java client's AIOOBE hits.
                    eprintln!("T2 - {},{},{}", self.ptype, self.ptype1, self.ptype2);
                    self.logout();
                } else {
                    // Java `catch (IOException)`: drop to the lostCon
                    // reestablish path (login opcode 18).
                    self.lost_con();
                }
                true
            }
        }
    }

    /// Read one frame's header and payload into `in`; `Ok(true)` when a full
    /// packet is ready to dispatch.
    fn read_packet(&mut self) -> io::Result<bool> {
        let stream = match self.stream.as_mut() {
            Some(s) => s,
            None => return Ok(false),
        };

        let mut available = stream.available()?;
        if available == 0 {
            return Ok(false);
        }

        if self.ptype == -1 {
            stream.read_bytes(self.r#in.data_mut(), 0, 1)?;
            self.ptype = self.r#in.data()[0] as i32 & 0xff;
            if let Some(random) = self.random_in.as_mut() {
                self.ptype = self.ptype.wrapping_sub(random.next_int()) & 0xff;
            }
            self.psize = SERVER_PROT_SIZES[self.ptype as usize];
            available -= 1;
        }

        if self.psize == -1 {
            if available <= 0 {
                return Ok(false);
            }
            stream.read_bytes(self.r#in.data_mut(), 0, 1)?;
            self.psize = self.r#in.data()[0] as i32 & 0xff;
            available -= 1;
        }

        if self.psize == -2 {
            if available <= 1 {
                return Ok(false);
            }
            stream.read_bytes(self.r#in.data_mut(), 0, 2)?;
            self.r#in.pos = 0;
            self.psize = self.r#in.g2();
            available -= 2;
        }

        if available < self.psize {
            return Ok(false);
        }

        // a length over the `in` buffer is the same AIOOBE the Java client
        // catches as a logic error
        if self.psize as usize > self.r#in.length() {
            return Err(io::Error::other("packet exceeds in buffer"));
        }

        self.r#in.pos = 0;
        stream.read_bytes(self.r#in.data_mut(), 0, self.psize as usize)?;
        // a full packet restamps the in-game silence watchdog (Java tcpIn)
        self.last_response = Some(Instant::now());
        self.ptype2 = self.ptype1;
        self.ptype1 = self.ptype0;
        self.ptype0 = self.ptype;
        Ok(true)
    }

    /// Inner `ptype` switch, 1:1 of the `Client.ts` `tcpIn` dispatch. Every
    /// handled packet resets `ptype` to -1 as the TS does; every `ServerProt`
    /// id has a branch here or an explicit no-op matching the TS handler's
    /// effect on the ported state. Unknown opcodes report `T1` and log out
    /// exactly like the TS default. Callable from tests without a socket.
    ///
    /// Packet OOB (and any other logic panic) is Java `catch (Exception)`:
    /// T2 + logout, so one short frame cannot take down the OS thread.
    pub fn handle_packet(&mut self, ptype: i32, payload: &mut Packet) {
        let ptype1 = self.ptype1;
        let ptype2 = self.ptype2;
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.dispatch_packet(ptype, payload);
        }));
        if result.is_err() {
            eprintln!("T2 - {ptype},{ptype1},{ptype2}");
            // `logout()` bumps every family (spec: REBUILD/logout → all).
            self.logout();
        } else {
            self.bump_gens(ptype);
        }
    }

    /// Bump the generation counter of `ptype`'s family. `REBUILD_NORMAL`
    /// invalidates every family (new scene). `LOGOUT` and unknown T1
    /// opcodes reach here after `dispatch` already called `logout()`, which
    /// bumps all; they must not double-bump.
    pub fn bump_gens(&mut self, ptype: i32) {
        match ptype {
            ServerProt::NPC_INFO => self.gens.npc += 1,
            ServerProt::PLAYER_INFO => self.gens.player += 1,
            ServerProt::UPDATE_INV_FULL
            | ServerProt::UPDATE_INV_PARTIAL
            | ServerProt::UPDATE_INV_STOP_TRANSMIT => self.gens.inv += 1,
            ServerProt::VARP_SMALL | ServerProt::VARP_LARGE | ServerProt::VARP_SYNC => {
                self.gens.varp += 1
            }
            ServerProt::UPDATE_STAT
            | ServerProt::UPDATE_RUNENERGY
            | ServerProt::UPDATE_RUNWEIGHT => self.gens.stat += 1,
            ServerProt::MESSAGE_GAME | ServerProt::MESSAGE_PRIVATE => self.gens.chat += 1,
            ServerProt::UPDATE_ZONE_PARTIAL_FOLLOWS
            | ServerProt::UPDATE_ZONE_FULL_FOLLOWS
            | ServerProt::UPDATE_ZONE_PARTIAL_ENCLOSED
            | ServerProt::P_LOCMERGE
            | ServerProt::LOC_ANIM
            | ServerProt::OBJ_DEL
            | ServerProt::OBJ_REVEAL
            | ServerProt::LOC_ADD_CHANGE
            | ServerProt::MAP_PROJANIM
            | ServerProt::LOC_DEL
            | ServerProt::OBJ_COUNT
            | ServerProt::MAP_ANIM
            | ServerProt::OBJ_ADD => self.gens.scene += 1,
            ServerProt::IF_OPENCHAT
            | ServerProt::IF_OPENMAIN_SIDE
            | ServerProt::IF_CLOSE
            | ServerProt::IF_SETICON
            | ServerProt::IF_SHOWICON
            | ServerProt::IF_OPENMAIN
            | ServerProt::IF_OPENSIDE
            | ServerProt::IF_OPENOVERLAY
            | ServerProt::IF_SETCOLOUR
            | ServerProt::IF_SETHIDE
            | ServerProt::IF_SETOBJECT
            | ServerProt::IF_SETMODEL
            | ServerProt::IF_SETANIM
            | ServerProt::IF_SETPLAYERHEAD
            | ServerProt::IF_SETTEXT
            | ServerProt::IF_SETNPCHEAD
            | ServerProt::IF_SETPOSITION
            | ServerProt::IF_SETSCROLLPOS
            | ServerProt::P_COUNTDIALOG => self.gens.iface += 1,
            ServerProt::CAM_LOOKAT
            | ServerProt::CAM_SHAKE
            | ServerProt::CAM_MOVETO
            | ServerProt::CAM_RESET => self.gens.camera += 1,
            ServerProt::UNSET_MAP_FLAG => self.gens.map_flag += 1,
            ServerProt::SET_MULTIWAY => self.gens.world += 1,
            ServerProt::REBUILD_NORMAL => self.bump_all_gens(),
            _ => {}
        }
    }

    /// Bump every family generation (`REBUILD_NORMAL` scene rebuilds and
    /// `logout()` / T2 resets make every slice stale).
    fn bump_all_gens(&mut self) {
        self.gens.npc += 1;
        self.gens.player += 1;
        self.gens.inv += 1;
        self.gens.varp += 1;
        self.gens.stat += 1;
        self.gens.chat += 1;
        self.gens.scene += 1;
        self.gens.iface += 1;
        self.gens.camera += 1;
        self.gens.map_flag += 1;
        self.gens.world += 1;
    }

    /// `IF_SETICON` handler (Client.ts 5992): bind interface `com_id` to side
    /// tab `icon`; 65535 clears the slot to -1. A tab index outside 0..14 is
    /// ignored (the TS writes it into a growing array; the fixed-size Rust
    /// array bounds-checks instead). Both side surfaces redraw.
    pub fn apply_if_seticon(&mut self, payload: &mut Packet) {
        let mut com_id = payload.g2();
        let icon = payload.g1();
        if com_id == 65535 {
            com_id = -1;
        }
        if (0..14).contains(&icon) {
            self.side_icon[icon as usize] = com_id;
        }
        self.redraw_side = true;
        self.redraw_icons = true;
    }

    /// `IF_SHOWICON` handler (Client.ts 6058): select side tab `icon`.
    pub fn apply_if_showicon(&mut self, icon: i32) {
        self.active_icon = icon;
        self.redraw_side = true;
        self.redraw_icons = true;
    }

    /// `CAM_LOOKAT` handler (Client.ts 6318-6349): enter the cutscene
    /// camera and aim it at the look-at tile. When `rate2 >= 100` the aim
    /// is applied immediately (the `getAvH` height minus `hei`); otherwise
    /// `cinema_camera()` eases toward it each game loop.
    pub fn apply_cam_lookat(&mut self, payload: &mut Packet) {
        self.cinema_cam = true;

        self.cam_look_at_lx = payload.g1();
        self.cam_look_at_lz = payload.g1();
        self.cam_look_at_hei = payload.g2();
        self.cam_look_at_rate = payload.g1();
        self.cam_look_at_rate2 = payload.g1();

        if self.cam_look_at_rate2 >= 100 {
            let scene_x = self.cam_look_at_lx * 128 + 64;
            let scene_z = self.cam_look_at_lz * 128 + 64;
            let scene_y = get_av_h(
                &self.groundh,
                &self.mapl,
                scene_x,
                scene_z,
                self.minusedlevel,
            ) - self.cam_look_at_hei;

            let delta_x = scene_x - self.cam_x;
            let delta_y = scene_y - self.cam_y;
            let delta_z = scene_z - self.cam_z;

            let distance = (f64::sqrt((delta_x * delta_x + delta_z * delta_z) as f64)) as i32;

            self.cam_pitch = ((delta_y as f64).atan2(distance as f64) * 325.949) as i32 & 0x7ff;
            self.cam_yaw = ((delta_x as f64).atan2(delta_z as f64) * -325.949) as i32 & 0x7ff;

            self.cam_pitch = self.cam_pitch.clamp(128, 383);
        }
    }

    /// `CAM_SHAKE` handler (Client.ts 6352-6365): arm one shake axis. The
    /// TS field mapping is kept verbatim — the packet's `ran`/`amp`/`rate`
    /// bytes land in `cam_shake_axis`/`cam_shake_ran`/`cam_shake_amp`.
    pub fn apply_cam_shake(&mut self, payload: &mut Packet) {
        let axis = payload.g1();
        let ran = payload.g1();
        let amp = payload.g1();
        let rate = payload.g1();

        if !(0..5).contains(&axis) {
            return;
        }
        self.cam_shake[axis as usize] = true;
        self.cam_shake_axis[axis as usize] = ran;
        self.cam_shake_ran[axis as usize] = amp;
        self.cam_shake_amp[axis as usize] = rate;
        self.cam_shake_cycle[axis as usize] = 0;
    }

    /// `CAM_MOVETO` handler (Client.ts 6368-6384): enter the cutscene
    /// camera and move it toward the tile. When `rate2 >= 100` the camera
    /// jumps immediately; otherwise `cinema_camera()` eases each loop.
    pub fn apply_cam_moveto(&mut self, payload: &mut Packet) {
        self.cinema_cam = true;

        self.cam_move_to_lx = payload.g1();
        self.cam_move_to_lz = payload.g1();
        self.cam_move_to_hei = payload.g2();
        self.cam_move_to_rate = payload.g1();
        self.cam_move_to_rate2 = payload.g1();

        if self.cam_move_to_rate2 >= 100 {
            self.cam_x = self.cam_move_to_lx * 128 + 64;
            self.cam_z = self.cam_move_to_lz * 128 + 64;
            self.cam_y = get_av_h(
                &self.groundh,
                &self.mapl,
                self.cam_x,
                self.cam_z,
                self.minusedlevel,
            ) - self.cam_move_to_hei;
        }
    }

    /// `CAM_RESET` handler (Client.ts 6387-6395): leave the cutscene camera
    /// and disarm every shake axis. The packet has no payload.
    pub fn apply_cam_reset(&mut self, _payload: &mut Packet) {
        self.cinema_cam = false;

        for i in 0..5 {
            self.cam_shake[i] = false;
        }
    }

    /// `UPDATE_RUNWEIGHT` handler (Client.ts 6595-6602): the carried weight
    /// as a signed g2, redrawing the side surface when the stats tab (12)
    /// is up.
    pub fn apply_update_runweight(&mut self, payload: &mut Packet) {
        if self.active_icon == 12 {
            self.redraw_side = true;
        }

        self.runweight = payload.g2b();
    }

    /// `HINT_ARROW` handler (Client.ts 6606-6641): set the hint arrow's
    /// target. The frame layout differs per type — type 1 reads an npc,
    /// types 2-6 rewrite `hint_type` to 2 and read a tile + height, type 10
    /// reads a player — and every byte of the frame is consumed so the
    /// inbound stream stays aligned.
    pub fn apply_hint_arrow(&mut self, payload: &mut Packet) {
        self.hint_type = payload.g1();

        if self.hint_type == 1 {
            self.hint_npc = payload.g2();
        }

        if (2..=6).contains(&self.hint_type) {
            match self.hint_type {
                2 => {
                    self.hint_offset_x = 64;
                    self.hint_offset_z = 64;
                }
                3 => {
                    self.hint_offset_x = 0;
                    self.hint_offset_z = 64;
                }
                4 => {
                    self.hint_offset_x = 128;
                    self.hint_offset_z = 64;
                }
                5 => {
                    self.hint_offset_x = 64;
                    self.hint_offset_z = 0;
                }
                _ => {
                    self.hint_offset_x = 64;
                    self.hint_offset_z = 128;
                }
            }

            self.hint_type = 2;
            self.hint_tile_x = payload.g2();
            self.hint_tile_z = payload.g2();
            self.hint_height = payload.g1();
        }

        if self.hint_type == 10 {
            self.hint_player = payload.g2();
        }
    }

    /// `UPDATE_REBOOT_TIMER` handler (Client.ts 6645-6648): the seconds
    /// until the server reboot, sent as a g2 scaled by 30.
    pub fn apply_update_reboot_timer(&mut self, payload: &mut Packet) {
        self.reboot_timer = payload.g2() * 30;
    }

    /// `P_COUNTDIALOG` handler (Client.ts 6762-6773): open the enter-amount
    /// dialog, closing any social prompt. The packet has no payload.
    pub fn apply_p_countdialog(&mut self) {
        self.social_input_open = false;
        self.dialog_input_open = true;
        self.dialog_input.clear();
        self.redraw_chat = true;
    }

    /// `SET_MULTIWAY` handler (Client.ts 6776-6779): mark the current area
    /// as multi-way combat.
    pub fn apply_set_multiway(&mut self, payload: &mut Packet) {
        self.in_multizone = payload.g1();
    }

    /// `MINIMAP_TOGGLE` handler (Client.ts 6801-6804): lock/unlock the
    /// minimap rotation.
    pub fn apply_minimap_toggle(&mut self, payload: &mut Packet) {
        self.minimap_state = payload.g1();
    }

    /// `ifAnimReset` from client-ts (10534): walk `id`'s children, zeroing
    /// each child's `anim_frame`/`anim_cycle` and recursing `TYPE_LAYER`
    /// children (the layer recursion is 0, not TS `type === 1`). Missing
    /// children or missing child ids stop the walk.
    pub fn if_anim_reset(&mut self, id: i32) {
        let Some(children) = self.if_(id as usize).and_then(|com| com.children.clone()) else {
            return;
        };
        for child_id in children {
            if child_id == -1 {
                return;
            }
            let Some(child) = self.if_(child_id as usize) else {
                return;
            };
            if child.r#type == ComponentType::TYPE_LAYER {
                self.if_anim_reset(child.id);
            }
            if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                .get_mut(child_id as usize)
                .and_then(|o| o.as_mut())
                .map(Arc::make_mut)
            {
                com.anim_frame = 0;
                com.anim_cycle = 0;
            }
        }
    }

    /// `IF_OPENCHAT` handler (Client.ts 5925): open `com_id` as the chat
    /// modal, closing any open side modal and the main modal (TS 5926-5938).
    pub fn apply_if_openchat(&mut self, payload: &mut Packet) {
        let com_id = payload.g2();
        self.if_anim_reset(com_id);

        if self.side_modal_id != -1 {
            self.side_modal_id = -1;
            self.redraw_side = true;
            self.redraw_icons = true;
        }
        self.chat_modal_id = com_id;
        self.redraw_chat = true;
        self.main_modal_id = -1;
        self.resumed_pause_button = false;
    }

    /// `IF_OPENMAIN` handler (Client.ts 6008): open `com_id` as the main
    /// modal, closing the side/chat modals and the enter-name dialog.
    pub fn apply_if_openmain(&mut self, payload: &mut Packet) {
        let com_id = payload.g2();
        self.if_anim_reset(com_id);

        if self.side_modal_id != -1 {
            self.side_modal_id = -1;
            self.redraw_side = true;
            self.redraw_icons = true;
        }
        if self.chat_modal_id != -1 {
            self.chat_modal_id = -1;
            self.redraw_chat = true;
        }
        if self.dialog_input_open {
            self.dialog_input_open = false;
            self.redraw_chat = true;
        }
        self.main_modal_id = com_id;
        self.resumed_pause_button = false;
    }

    /// `IF_OPENMAIN_SIDE` handler (Client.ts 5941): open the main and side
    /// modals together, closing the chat modal and the enter-name dialog.
    pub fn apply_if_openmain_side(&mut self, payload: &mut Packet) {
        let main_com_id = payload.g2();
        let side_com_id = payload.g2();

        if self.chat_modal_id != -1 {
            self.chat_modal_id = -1;
            self.redraw_chat = true;
        }
        if self.dialog_input_open {
            self.dialog_input_open = false;
            self.redraw_chat = true;
        }
        self.main_modal_id = main_com_id;
        self.side_modal_id = side_com_id;
        self.redraw_side = true;
        self.redraw_icons = true;
        self.resumed_pause_button = false;
    }

    /// `IF_OPENOVERLAY` handler (Client.ts 6069): set `main_overlay_id`
    /// from the signed byte (g2b); a negative id (65535) clears it and
    /// skips the anim reset.
    pub fn apply_if_openoverlay(&mut self, payload: &mut Packet) {
        let com_id = payload.g2b();
        if com_id >= 0 {
            self.if_anim_reset(com_id);
        }
        self.main_overlay_id = com_id;
    }

    /// `TUT_FLASH` handler (Client.ts 6212): set `tut_flash_icon` and, when
    /// it equals `active_icon`, bounce the active tab to the other tutorial
    /// tab (3 <-> 1) and redraw the side.
    pub fn apply_tut_flash(&mut self, icon: i32) {
        self.tut_flash_icon = icon;
        if self.tut_flash_icon == self.active_icon {
            self.active_icon = if self.tut_flash_icon == 3 { 1 } else { 3 };
            self.redraw_side = true;
        }
    }

    /// `TUT_OPEN` handler (Client.ts 6231): set `tut_com_id` and redraw
    /// the chat area.
    pub fn apply_tut_open(&mut self, payload: &mut Packet) {
        self.tut_com_id = payload.g2b();
        self.redraw_chat = true;
    }

    /// `IF_OPENSIDE` handler (Client.ts 6034): open `com_id` as the side
    /// modal, closing any open chat modal and the enter-name dialog
    /// alongside (TS 6036-6053), and clear the main modal.
    pub fn apply_if_openside(&mut self, payload: &mut Packet) {
        let com_id = payload.g2();
        self.if_anim_reset(com_id);

        if self.chat_modal_id != -1 {
            self.chat_modal_id = -1;
            self.redraw_chat = true;
        }
        if self.dialog_input_open {
            self.dialog_input_open = false;
            self.redraw_chat = true;
        }
        self.side_modal_id = com_id;
        self.redraw_side = true;
        self.redraw_icons = true;
        self.main_modal_id = -1;
        self.resumed_pause_button = false;
    }

    /// `IF_CLOSE` handler (Client.ts 5968): close the side and chat modals,
    /// the enter-name dialog, and the main modal. The packet has no payload
    /// (TS only reads `ptype`).
    pub fn apply_if_close(&mut self) {
        if self.side_modal_id != -1 {
            self.side_modal_id = -1;
            self.redraw_side = true;
            self.redraw_icons = true;
        }
        if self.chat_modal_id != -1 {
            self.chat_modal_id = -1;
            self.redraw_chat = true;
        }
        if self.dialog_input_open {
            self.dialog_input_open = false;
            self.redraw_chat = true;
        }
        self.main_modal_id = -1;
        self.resumed_pause_button = false;
    }

    /// `addChat` from client-ts (11453): shift the 100 chat slots down one
    /// (99→1), write the new line at slot 0, and redraw. The `tutComId`
    /// branch (TS 11454-11458) writes `tutComMessage` and clears the mouse
    /// click; the tutorial message feature is not ported. Each appended line
    /// bumps `chat_seq` once (before the slot shift).
    pub fn add_chat(&mut self, r#type: i32, text: &str, sender: &str) {
        if self.chat_modal_id == -1 {
            self.redraw_chat = true;
        }
        self.chat_seq += 1;
        for i in (1..100).rev() {
            self.chat_type[i] = self.chat_type[i - 1];
            self.chat_username[i] = std::mem::take(&mut self.chat_username[i - 1]);
            self.chat_text[i] = std::mem::take(&mut self.chat_text[i - 1]);
        }
        self.chat_type[0] = r#type;
        self.chat_username[0] = sender.to_string();
        self.chat_text[0] = text.to_string();
    }

    /// `MESSAGE_GAME` handler (Client.ts 6413): read the jstr message and
    /// route it — a `:tradereq:`/`:duelreq:` suffix adds the trade/duel line
    /// with the requester's name unless the requester is on the ignore list
    /// or chat is disabled (TS 6420-6446), and anything else is public chat
    /// type 0 with no sender.
    pub fn apply_message_game(&mut self, payload: &mut Packet) {
        let message = payload.gjstr();

        if message.ends_with(":tradereq:") {
            let player = message[..message.find(':').unwrap_or(0)].to_string();
            let username = JString::to_userhash(&player) as i64;
            let mut ignored = false;
            for i in 0..self.ignore_count {
                if self.ignore_userhash[i as usize] == username {
                    ignored = true;
                    break;
                }
            }
            if !ignored && self.chat_disabled == 0 {
                self.add_chat(4, "wishes to trade with you.", &player);
            }
        } else if message.ends_with(":duelreq:") {
            let player = message[..message.find(':').unwrap_or(0)].to_string();
            let username = JString::to_userhash(&player) as i64;
            let mut ignored = false;
            for i in 0..self.ignore_count {
                if self.ignore_userhash[i as usize] == username {
                    ignored = true;
                    break;
                }
            }
            if !ignored && self.chat_disabled == 0 {
                self.add_chat(8, "wishes to duel with you.", &player);
            }
        } else {
            self.add_chat(0, &message, "");
        }
    }

    /// `isFriend` from client-ts (11474): case-insensitive match of a chat
    /// sender against the friend list, then the local player's own name.
    pub fn is_friend(&self, username: &str) -> bool {
        if username.is_empty() {
            return false;
        }
        for i in 0..self.friend_count {
            if username.eq_ignore_ascii_case(&self.friend_username[i as usize]) {
                return true;
            }
        }
        match &self.local_player {
            None => false,
            Some(p) => p
                .name
                .as_deref()
                .is_some_and(|n| username.eq_ignore_ascii_case(n)),
        }
    }

    /// `addFriend` from client-ts (11492): check the free/member cap, the
    /// friend and ignore lists, then add locally and send `FRIENDLIST_ADD`
    /// with the p8 userhash. A hash of 0 is ignored (TS). The two identical
    /// cap messages are kept as TS branches them.
    #[allow(clippy::if_same_then_else)]
    pub fn add_friend(&mut self, userhash: i64) {
        if userhash == 0 {
            return;
        }
        if self.friend_count >= 100 && self.members_account != 1 {
            self.add_chat(
                0,
                "Your friendlist is full. Max of 100 for free users, and 200 for members",
                "",
            );
            return;
        } else if self.friend_count >= 200 {
            self.add_chat(
                0,
                "Your friendlist is full. Max of 100 for free users, and 200 for members",
                "",
            );
            return;
        }
        let display_name = JString::to_screen_name(&JString::to_raw_username(userhash));
        for i in 0..self.friend_count {
            if self.friend_userhash[i as usize] == userhash {
                self.add_chat(
                    0,
                    &format!("{display_name} is already on your friend list"),
                    "",
                );
                return;
            }
        }
        for i in 0..self.ignore_count {
            if self.ignore_userhash[i as usize] == userhash {
                self.add_chat(
                    0,
                    &format!("Please remove {display_name} from your ignore list first"),
                    "",
                );
                return;
            }
        }
        let Some(local_name) = self.local_player.as_ref().and_then(|p| p.name.clone()) else {
            return;
        };
        if display_name != local_name {
            self.friend_username[self.friend_count as usize] = display_name;
            self.friend_userhash[self.friend_count as usize] = userhash;
            self.friend_node_id[self.friend_count as usize] = 0;
            self.friend_count += 1;
            self.redraw_side = true;
            self.out.p1_enc(ClientProt::FRIENDLIST_ADD.id);
            self.out.p8(userhash);
        }
    }

    /// `addIgnore` from client-ts (11537): cap the ignore list, check both
    /// lists, then add locally and send `IGNORELIST_ADD` with the p8
    /// userhash. A hash of 0 is ignored (TS).
    pub fn add_ignore(&mut self, userhash: i64) {
        if userhash == 0 {
            return;
        }
        if self.ignore_count >= 100 {
            self.add_chat(0, "Your ignore list is full. Max of 100 hit", "");
            return;
        }
        let display_name = JString::to_screen_name(&JString::to_raw_username(userhash));
        for i in 0..self.ignore_count {
            if self.ignore_userhash[i as usize] == userhash {
                self.add_chat(
                    0,
                    &format!("{display_name} is already on your ignore list"),
                    "",
                );
                return;
            }
        }
        for i in 0..self.friend_count {
            if self.friend_userhash[i as usize] == userhash {
                self.add_chat(
                    0,
                    &format!("Please remove {display_name} from your friend list first"),
                    "",
                );
                return;
            }
        }
        self.ignore_userhash[self.ignore_count as usize] = userhash;
        self.ignore_count += 1;
        self.redraw_side = true;
        self.out.p1_enc(ClientProt::IGNORELIST_ADD.id);
        self.out.p8(userhash);
    }

    /// `delFriend` from client-ts (11568): shift the friend arrays down over
    /// the removed hash and send `FRIENDLIST_DEL` with the p8 userhash. A
    /// hash of 0 is ignored (TS).
    pub fn del_friend(&mut self, userhash: i64) {
        if userhash == 0 {
            return;
        }
        for i in 0..self.friend_count {
            if self.friend_userhash[i as usize] == userhash {
                self.friend_count -= 1;
                self.redraw_side = true;
                for j in i..self.friend_count {
                    self.friend_username[j as usize] =
                        std::mem::take(&mut self.friend_username[(j + 1) as usize]);
                    self.friend_node_id[j as usize] = self.friend_node_id[(j + 1) as usize];
                    self.friend_userhash[j as usize] = self.friend_userhash[(j + 1) as usize];
                }
                self.out.p1_enc(ClientProt::FRIENDLIST_DEL.id);
                self.out.p8(userhash);
                return;
            }
        }
    }

    /// `delIgnore` from client-ts (11592): shift the ignore array down over
    /// the removed hash and send `IGNORELIST_DEL` with the p8 userhash. A
    /// hash of 0 is ignored (TS).
    pub fn del_ignore(&mut self, userhash: i64) {
        if userhash == 0 {
            return;
        }
        for i in 0..self.ignore_count {
            if self.ignore_userhash[i as usize] == userhash {
                self.ignore_count -= 1;
                self.redraw_side = true;
                for j in i..self.ignore_count {
                    self.ignore_userhash[j as usize] = self.ignore_userhash[(j + 1) as usize];
                }
                self.out.p1_enc(ClientProt::IGNORELIST_DEL.id);
                self.out.p8(userhash);
                return;
            }
        }
    }

    /// `UPDATE_IGNORELIST` handler (Client.ts 6454): the frame is one p8
    /// userhash per entry; the count is `psize / 8` clamped to the 100-slot
    /// array (the TS typed array silently drops writes past 100).
    pub fn apply_update_ignorelist(&mut self, payload: &mut Packet, psize: i32) {
        self.ignore_count = (psize / 8).min(100);
        for i in 0..self.ignore_count {
            self.ignore_userhash[i as usize] = payload.g8();
        }
    }

    /// `CHAT_FILTER_SETTINGS` handler (Client.ts 6464): three mode bytes
    /// then redraw both the mode strip and the chat.
    pub fn apply_chat_filter_settings(&mut self, payload: &mut Packet) {
        self.chat_public_mode = payload.g1();
        self.chat_private_mode = payload.g1();
        self.chat_trade_mode = payload.g1();
        self.redraw_chat_mode = true;
        self.redraw_chat = true;
    }

    /// `MESSAGE_PRIVATE` handler (Client.ts 6475): dedupe by message id
    /// (rolling 100-slot window), skip senders on the ignore list unless
    /// they are staff (level > 1), then WordPack-unpack the `psize - 13`
    /// byte tail (g8 + g4 + g1) and add a chat line type 3, or type 7 with
    /// the `@cr*` staff prefix.
    pub fn apply_message_private(&mut self, payload: &mut Packet, psize: i32) {
        let from = payload.g8();
        let message_id = payload.g4();
        let staff_mod_level = payload.g1();

        let mut ignored = false;
        for i in 0..100 {
            if self.private_message_ids[i] == message_id {
                ignored = true;
                break;
            }
        }
        if staff_mod_level <= 1 {
            for i in 0..self.ignore_count {
                if self.ignore_userhash[i as usize] == from {
                    ignored = true;
                    break;
                }
            }
        }

        if !ignored && self.chat_disabled == 0 {
            self.private_message_ids[self.private_message_count as usize] = message_id;
            self.private_message_count = (self.private_message_count + 1) % 100;

            let uncompressed = WordPack::unpack(payload, (psize - 13) as usize);
            let filtered = WordFilter::filter(&uncompressed);
            let sender = JString::to_screen_name(&JString::to_raw_username(from));
            if staff_mod_level == 2 || staff_mod_level == 3 {
                self.add_chat(7, &filtered, &format!("@cr2@{sender}"));
            } else if staff_mod_level == 1 {
                self.add_chat(7, &filtered, &format!("@cr1@{sender}"));
            } else {
                self.add_chat(3, &filtered, &sender);
            }
        }
    }

    /// `FRIENDLIST_LOADED` handler (Client.ts 6523): the world-list status
    /// byte (0 = connecting, 1 = connecting slowly, 2 = loaded) and a side
    /// redraw.
    pub fn apply_friendlist_loaded(&mut self, payload: &mut Packet) {
        self.friend_server_status = payload.g1();
        self.redraw_side = true;
    }

    /// `UPDATE_FRIENDLIST` handler (Client.ts 6530): a p8 userhash plus the
    /// p1 world. An existing friend whose world changed gets the login
    /// (`world > 0`) / logout (`world === 0`) type-5 chat and a side redraw;
    /// a new friend is appended (200 cap). Then the bubble sort moves
    /// same-world friends to the front, with offline (world 0) friends last.
    /// The row swap needs temps: two runtime-indexed array elements cannot
    /// be mutably borrowed at once.
    #[allow(clippy::manual_swap)]
    pub fn apply_update_friendlist(&mut self, payload: &mut Packet) {
        let username = payload.g8();
        let world = payload.g1();

        let mut display_name = Some(JString::to_screen_name(&JString::to_raw_username(username)));
        for i in 0..self.friend_count {
            if self.friend_userhash[i as usize] == username {
                if self.friend_node_id[i as usize] != world {
                    self.friend_node_id[i as usize] = world;
                    self.redraw_side = true;
                    if world > 0 {
                        let name = display_name.as_deref().unwrap_or("");
                        self.add_chat(5, &format!("{name} has logged in."), "");
                    }
                    if world == 0 {
                        let name = display_name.as_deref().unwrap_or("");
                        self.add_chat(5, &format!("{name} has logged out."), "");
                    }
                }
                display_name = None;
                break;
            }
        }

        if let Some(display_name) = display_name {
            if self.friend_count < 200 {
                self.friend_userhash[self.friend_count as usize] = username;
                self.friend_username[self.friend_count as usize] = display_name;
                self.friend_node_id[self.friend_count as usize] = world;
                self.friend_count += 1;
                self.redraw_side = true;
            }
        }

        let mut sorted = false;
        while !sorted {
            sorted = true;
            for i in 0..self.friend_count - 1 {
                if (self.friend_node_id[i as usize] != self.node_id
                    && self.friend_node_id[(i + 1) as usize] == self.node_id)
                    || (self.friend_node_id[i as usize] == 0
                        && self.friend_node_id[(i + 1) as usize] != 0)
                {
                    let old_world = self.friend_node_id[i as usize];
                    self.friend_node_id[i as usize] = self.friend_node_id[(i + 1) as usize];
                    self.friend_node_id[(i + 1) as usize] = old_world;

                    let old_name = std::mem::take(&mut self.friend_username[i as usize]);
                    self.friend_username[i as usize] =
                        std::mem::take(&mut self.friend_username[(i + 1) as usize]);
                    self.friend_username[(i + 1) as usize] = old_name;

                    let old_userhash = self.friend_userhash[i as usize];
                    self.friend_userhash[i as usize] = self.friend_userhash[(i + 1) as usize];
                    self.friend_userhash[(i + 1) as usize] = old_userhash;

                    self.redraw_side = true;
                    sorted = false;
                }
            }
        }
    }

    /// `iconLoop` from client-ts (2787): the side-tab hit boxes, one per
    /// tab with a bound interface (`side_icon[i] != -1`). On a latched
    /// click inside a box, select that tab and redraw both the side panel
    /// and the icon strips. Hit boxes verbatim from 2792-2844.
    pub fn handle_tab_clicks(&mut self) {
        if self.shell.mouse_click_button == 0 {
            return;
        }
        let (x, y) = (self.shell.mouse_click_x, self.shell.mouse_click_y);

        if (539..=573).contains(&x) && (169..205).contains(&y) && self.side_icon[0] != -1 {
            self.active_icon = 0;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (569..=599).contains(&x) && (168..205).contains(&y) && self.side_icon[1] != -1 {
            self.active_icon = 1;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (597..=627).contains(&x) && (168..205).contains(&y) && self.side_icon[2] != -1 {
            self.active_icon = 2;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (625..=669).contains(&x) && (168..203).contains(&y) && self.side_icon[3] != -1 {
            self.active_icon = 3;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (666..=696).contains(&x) && (168..205).contains(&y) && self.side_icon[4] != -1 {
            self.active_icon = 4;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (694..=724).contains(&x) && (168..205).contains(&y) && self.side_icon[5] != -1 {
            self.active_icon = 5;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (722..=756).contains(&x) && (169..205).contains(&y) && self.side_icon[6] != -1 {
            self.active_icon = 6;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (540..=574).contains(&x) && (466..502).contains(&y) && self.side_icon[7] != -1 {
            self.active_icon = 7;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (572..=602).contains(&x) && (466..503).contains(&y) && self.side_icon[8] != -1 {
            self.active_icon = 8;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (599..=629).contains(&x) && (466..503).contains(&y) && self.side_icon[9] != -1 {
            self.active_icon = 9;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (627..=671).contains(&x) && (467..502).contains(&y) && self.side_icon[10] != -1 {
            self.active_icon = 10;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (669..=699).contains(&x) && (466..503).contains(&y) && self.side_icon[11] != -1 {
            self.active_icon = 11;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (696..=726).contains(&x) && (466..503).contains(&y) && self.side_icon[12] != -1 {
            self.active_icon = 12;
            self.redraw_side = true;
            self.redraw_icons = true;
        } else if (724..=758).contains(&x) && (466..502).contains(&y) && self.side_icon[13] != -1 {
            self.active_icon = 13;
            self.redraw_side = true;
            self.redraw_icons = true;
        }
    }

    /// TS 2229-2300: the in-flight inventory drag tick, run from
    /// `game_loop` before `handle_tab_clicks` (TS 2214-2300). Cycles count
    /// frames, the grab threshold arms once the pointer moves past ±5 px,
    /// and release reads `mouse_button == 0` (held state, not the click
    /// latch). A real drop (threshold + 5 held cycles) moves the item
    /// (`obj_replace` copy, bank-arrange insert, or `swap_slots`) and
    /// writes `INV_BUTTOND`; a quick release without a grab falls back to
    /// `openMenu`/`doAction` like TS 2291-2296.
    pub fn handle_obj_drag(&mut self) {
        if self.obj_drag_area == 0 {
            return;
        }
        self.obj_drag_cycles += 1;
        if self.shell.mouse_x > self.obj_grab_x + 5
            || self.shell.mouse_x < self.obj_grab_x - 5
            || self.shell.mouse_y > self.obj_grab_y + 5
            || self.shell.mouse_y < self.obj_grab_y - 5
        {
            self.obj_grab_threshold = true;
        }
        if self.shell.mouse_button != 0 {
            return;
        }
        if self.obj_drag_area == 2 {
            self.redraw_side = true;
        }
        if self.obj_drag_area == 3 {
            self.redraw_chat = true;
        }
        self.obj_drag_area = 0;
        if self.obj_grab_threshold && self.obj_drag_cycles >= 5 {
            // drop: re-walk the pointer so `hovered_slot`/`hovered_slot_com_id`
            // are the live drop target, not the pre-drag hover (TS 2247-2248).
            self.hovered_slot_com_id = -1;
            self.update_if_pointer();
            if self.hovered_slot_com_id == self.obj_drag_com_id
                && self.hovered_slot != self.obj_drag_slot
            {
                let com_id = self.obj_drag_com_id as usize;
                let src = self.obj_drag_slot as usize;
                let dst = self.hovered_slot as usize;
                // Decode reads (bank flag, replace flag) before the overlay
                // borrow below.
                let client_code = self.if_(com_id).map(|com| com.client_code).unwrap_or(-1);
                let obj_replace = self.if_(com_id).map(|com| com.obj_replace).unwrap_or(false);
                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    let mut mode = 0;
                    if self.bank_arrange_mode == 1 && client_code == CC_BANKMODE {
                        mode = 1;
                    }
                    if com
                        .link_obj_type
                        .as_ref()
                        .is_some_and(|t| t.get(dst).copied().unwrap_or(0) <= 0)
                    {
                        mode = 0;
                    }
                    // TS 2261-2281: `obj_replace` moves src into dst (copy,
                    // then clear src); bank arrange inserts by bubbling the
                    // item toward dst; otherwise the two slots swap.
                    match (
                        obj_replace,
                        com.link_obj_type.as_mut(),
                        com.link_obj_number.as_mut(),
                    ) {
                        (true, Some(t), Some(n)) => {
                            t[dst] = t[src];
                            n[dst] = n[src];
                            t[src] = -1;
                            n[src] = 0;
                        }
                        (_, _, _) if mode == 1 => {
                            let mut s = self.obj_drag_slot;
                            while s != self.hovered_slot {
                                if s > self.hovered_slot {
                                    com.swap_slots(s as usize, (s - 1) as usize);
                                    s -= 1;
                                } else {
                                    com.swap_slots(s as usize, (s + 1) as usize);
                                    s += 1;
                                }
                            }
                        }
                        _ => {
                            com.swap_slots(src, dst);
                        }
                    }
                    self.out.p1_enc(ClientProt::INV_BUTTOND.id);
                    self.out.p2(self.obj_drag_com_id);
                    self.out.p2(self.obj_drag_slot);
                    self.out.p2(self.hovered_slot);
                    self.out.p1(mode);
                }
            }
        } else if (self.one_mouse_button == 1
            || self.is_add_friend_option(self.menu_num_entries - 1))
            && self.menu_num_entries > 2
        {
            // TS 2291-2293: a quick release without a grab opens the
            // multi-entry menu instead.
            self.open_menu();
        } else if self.menu_num_entries > 0 {
            // TS 2294-2296: a quick release falls back to the last menu
            // entry (the left-click Wear/Eat/Drop the drag grabbed).
            self.doAction(self.menu_num_entries - 1);
        }
        // TS 2298-2299: with the drop consumed, reset the outline timeout
        // and clear the click so the tab/side/main/chat click handlers that
        // run after this tick don't also fire on the release frame.
        self.selected_cycle = 10;
        self.shell.mouse_click_button = 0;
    }

    /// TS `mouseLoop` drag start (8343-8368), ported without the minimenu:
    /// a left click on a TYPE_INV slot holding an item (`obj_swap` or
    /// `obj_replace`) grabs it. Returns true when a drag started so the
    /// click handler returns without hitting `IF_BUTTON`.
    fn obj_drag_start(&mut self, com_id: i32, slot: i32, x: i32, y: i32) -> bool {
        let Some(com) = self.if_(com_id as usize) else {
            return false;
        };
        if !(com.obj_swap || com.obj_replace) {
            return false;
        }
        if com
            .link_obj_type
            .as_ref()
            .is_some_and(|t| t.get(slot as usize).copied().unwrap_or(0) <= 0)
        {
            return false;
        }
        // Copy the modal layer before the writes: `if_` borrows the whole
        // client (the shared decode is behind an `Arc`), so the mutable
        // drag-state stores below need the borrow to end first.
        let layer_id = com.layer_id;
        self.obj_grab_threshold = false;
        self.obj_drag_cycles = 0;
        self.obj_drag_com_id = com_id;
        self.obj_drag_slot = slot;
        self.obj_drag_area = 2;
        self.obj_grab_x = x;
        self.obj_grab_y = y;
        if layer_id == self.main_modal_id {
            self.obj_drag_area = 1;
        }
        if layer_id == self.chat_modal_id {
            self.obj_drag_area = 3;
        }
        true
    }

    /// `clientButton` from Java (`Client.java` 8725-8841) / Client.ts
    /// 10960-11080: CC_ADD/DEL_FRIEND (201/202, only while the friend server
    /// is connected) open the add/delete-friend social prompts, CC_LOGOUT
    /// (205) arms `logoutTimer`, CC_ADD/DEL_IGNORE (501/502) open the
    /// add/delete-ignore prompts, the player-design codes 300-327 cycle
    /// kit/colour, switch gender and send `IDK_SAVEDESIGN`, and the
    /// report-abuse codes (601-613) are slice 6. Social codes return
    /// `false` so the `doAction` IF_BUTTON arm skips the send (TS sets the
    /// prompt and falls through); logout and accept-design return `true`
    /// and the click is sent.
    pub fn client_button(&mut self, com_id: i32) -> bool {
        let Some(client_code) = self.if_(com_id as usize).map(|com| com.client_code) else {
            return false;
        };
        if self.friend_server_status == 2 {
            if client_code == CC_ADD_FRIEND {
                self.redraw_chat = true;
                self.dialog_input_open = false;
                self.social_input_open = true;
                self.social_input = String::new();
                self.social_input_type = 1;
                self.social_input_header = "Enter name of friend to add to list".into();
            } else if client_code == CC_DEL_FRIEND {
                self.redraw_chat = true;
                self.dialog_input_open = false;
                self.social_input_open = true;
                self.social_input = String::new();
                self.social_input_type = 2;
                self.social_input_header = "Enter name of friend to delete from list".into();
            }
        }

        if client_code == CC_LOGOUT {
            self.logout_timer = 250;
            return true;
        } else if client_code == CC_ADD_IGNORE {
            self.redraw_chat = true;
            self.dialog_input_open = false;
            self.social_input_open = true;
            self.social_input = String::new();
            self.social_input_type = 4;
            self.social_input_header = "Enter name of player to add to list".into();
        } else if client_code == CC_DEL_IGNORE {
            self.redraw_chat = true;
            self.dialog_input_open = false;
            self.social_input_open = true;
            self.social_input = String::new();
            self.social_input_type = 5;
            self.social_input_header = "Enter name of player to delete from list".into();
        } else if (CC_CHANGE_HEAD_L..=CC_CHANGE_FEET_R).contains(&client_code) {
            // TS 10998-11025: the 7 kit arrows step the current kit up/down
            // to the next non-disabled kit of the gender's part. The TS
            // `while(true)` is bounded by the table size here (a real table
            // always has a match; an empty one must not spin forever).
            let part = ((client_code - CC_CHANGE_HEAD_L) / 2) as usize;
            let direction = client_code & 0x1;
            let mut kit = self.idk_design_part[part];
            if kit != -1 {
                let want = part as i32 + if self.idk_design_gender { 0 } else { 7 };
                for _ in 0..self.cache.idks.len() {
                    if direction == 0 {
                        kit -= 1;
                        if kit < 0 {
                            kit = self.cache.idks.len() as i32 - 1;
                        }
                    } else {
                        kit += 1;
                        if kit >= self.cache.idks.len() as i32 {
                            kit = 0;
                        }
                    }
                    if let Some(idk) = self.cache.idks.get(kit as usize) {
                        if !idk.disable && idk.part == want {
                            self.idk_design_part[part] = kit;
                            self.idk_design_redraw = true;
                            break;
                        }
                    }
                }
            }
        } else if (CC_RECOLOUR_HAIR_L..=CC_RECOLOUR_SKIN_R).contains(&client_code) {
            // TS 11026-11046: the 5 colour arrows step the colour index
            // around the `recol1d` table for the part.
            let part = ((client_code - CC_RECOLOUR_HAIR_L) / 2) as usize;
            let direction = client_code & 0x1;
            let mut colour = self.idk_design_colour[part];
            if direction == 0 {
                colour -= 1;
                if colour < 0 {
                    colour = recol1d()[part].len() as i32 - 1;
                }
            } else {
                colour += 1;
                if colour >= recol1d()[part].len() as i32 {
                    colour = 0;
                }
            }
            self.idk_design_colour[part] = colour;
            self.idk_design_redraw = true;
        } else if client_code == CC_SWITCH_TO_MALE && !self.idk_design_gender {
            self.idk_design_gender = true;
            self.validate_idk_design();
        } else if client_code == CC_SWITCH_TO_FEMALE && self.idk_design_gender {
            self.idk_design_gender = false;
            self.validate_idk_design();
        } else if client_code == CC_ACCEPT_DESIGN {
            // TS 11053-11065: IDK_SAVEDESIGN (id 125, length 13) carries
            // the gender byte, 7 kit bytes and 5 colour bytes.
            self.out.p1_enc(ClientProt::IDK_SAVEDESIGN.id);
            self.out.p1(if self.idk_design_gender { 0 } else { 1 });
            for i in 0..7 {
                self.out.p1(self.idk_design_part[i]);
            }
            for i in 0..5 {
                self.out.p1(self.idk_design_colour[i]);
            }
            return true;
        }
        false
    }

    /// `validateIdkDesign` from Client.ts 11082-11096: re-pick the first
    /// non-disabled kit of each part for the current gender and re-arm the
    /// preview redraw. A no-op against an empty idk table (parts stay -1).
    fn validate_idk_design(&mut self) {
        self.idk_design_redraw = true;
        for i in 0..7 {
            self.idk_design_part[i] = -1;
            for (j, idk) in self.cache.idks.iter().enumerate() {
                if !idk.disable && idk.part == i as i32 + if self.idk_design_gender { 0 } else { 7 }
                {
                    self.idk_design_part[i] = j as i32;
                    break;
                }
            }
        }
    }

    /// Java `Client.java` 3682-3686 / Client.ts 1883-1887: cold login
    /// resets the player-design state — male gender, kits revalidated for
    /// it, all colours back to the default. Public because the leftover
    /// test drives the exact reset path the login uses.
    pub fn reset_idk_design(&mut self) {
        self.idk_design_gender = true;
        self.validate_idk_design();
        self.idk_design_colour = [0; 5];
    }

    /// TS `clientComponent` CC_DESIGN_PREVIEW build (10777-10819): combine
    /// the selected idk kits, apply the recolour tables and the local
    /// player's readyanim pose, and hand the model back for caching under
    /// `getModel(5, 0)`. Runs before the `com` borrow in
    /// `client_component` because both touch `cache`.
    fn design_preview_model(&mut self) -> Option<Model> {
        if !self.idk_design_redraw {
            return None;
        }
        for i in 0..7 {
            let kit = self.idk_design_part[i];
            if kit >= 0 && !self.cache.idks[kit as usize].check_model() {
                return None;
            }
        }
        self.idk_design_redraw = false;

        let mut models: [Option<Model>; 7] = Default::default();
        let mut model_count = 0;
        for part in 0..7 {
            let kit = self.idk_design_part[part];
            if kit >= 0 {
                models[model_count] = self.cache.idks[kit as usize].get_model_no_check();
                model_count += 1;
            }
        }
        let mut model = Model::combine_for_anim(&models, model_count);
        for part in 0..5 {
            let colour = self.idk_design_colour[part];
            if colour != 0 {
                model.recolour(recol1d()[part][0], recol1d()[part][colour as usize]);
                if part == 1 {
                    model.recolour(recol2d()[0], recol2d()[colour as usize]);
                }
            }
        }
        model.prepare_anim();
        model.calculate_normals(64, 850, -30, -50, -30, true);
        if let Some(local) = &self.local_player {
            if let Some(frame) = self
                .cache
                .seqs
                .get(local.readyanim as usize)
                .and_then(|seq| seq.frames.as_ref())
                .and_then(|frames| frames.first().copied())
            {
                model.animate(frame);
            }
        }
        Some(model)
    }

    /// `clientComponent` from Client.ts (10687-10842), friends/ignore/size
    /// plus the player-design preview and switch-button arms: fill
    /// `text`/`button_type`/`scroll_height` on the friend name (1..=100,
    /// 701..=800), world/offline update (101..=200, 801..=900), friend list
    /// size (203), ignore names (401..=500) and ignore list size (503)
    /// components; CC_DESIGN_PREVIEW (327) rotates the kit preview and
    /// rebuilds its temp model, CC_SWITCH_TO_MALE/FEMALE (324/325) swap the
    /// button graphic to the gender being switched to. Called from
    /// `draw_interface` before each child plots (TS 9926).
    pub fn client_component(&mut self, com_id: i32) {
        // The CC_DESIGN_PREVIEW build needs the idk/seq tables while `com`
        // is read, so it runs before the overlay borrow (TS 10777-10819;
        // `com.model_xan/yan` are set after). `height` (decode) and the
        // client code are read before the borrow too.
        let preview_model = match self.if_(com_id as usize).map(|com| com.client_code) {
            Some(CC_DESIGN_PREVIEW) => self.design_preview_model(),
            _ => None,
        };
        let Some(mut client_code) = self.if_(com_id as usize).map(|com| com.client_code) else {
            return;
        };
        let height = self.if_(com_id as usize).map(|com| com.height).unwrap_or(0);
        // Decode read for the switch-button arms (graphic2 lives on the
        // shared table; the overlay borrow below cannot see it).
        let graphic2_name = self
            .if_(com_id as usize)
            .map(|com| com.graphic2_name.clone())
            .unwrap_or_default();
        let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
            .get_mut(com_id as usize)
            .and_then(|o| o.as_mut())
            .map(Arc::make_mut)
        else {
            return;
        };
        if (CC_FRIENDS_START..=CC_FRIENDS_END).contains(&client_code)
            || (CC_FRIENDS2_START..=CC_FRIENDS2_END).contains(&client_code)
        {
            if client_code == CC_FRIENDS_START && self.friend_server_status == 0 {
                com.text = "Loading friend list".into();
                com.button_type = 0;
            } else if client_code == CC_FRIENDS_START && self.friend_server_status == 1 {
                com.text = "Connecting to friendserver".into();
                com.button_type = 0;
            } else if client_code == 2 && self.friend_server_status != 2 {
                com.text = "Please wait...".into();
                com.button_type = 0;
            } else {
                let mut count = self.friend_count;
                if self.friend_server_status != 2 {
                    count = 0;
                }
                if client_code > 700 {
                    client_code -= 601;
                } else {
                    client_code -= 1;
                }
                if client_code >= count {
                    com.text = String::new();
                    com.button_type = 0;
                } else {
                    com.text = self.friend_username[client_code as usize].clone();
                    com.button_type = 1;
                }
            }
        } else if (CC_FRIENDS_UPDATE_START..=CC_FRIENDS_UPDATE_END).contains(&client_code)
            || (CC_FRIENDS2_UPDATE_START..=CC_FRIENDS2_UPDATE_END).contains(&client_code)
        {
            let mut count = self.friend_count;
            if self.friend_server_status != 2 {
                count = 0;
            }
            if client_code > 800 {
                client_code -= 701;
            } else {
                client_code -= 101;
            }
            if client_code >= count {
                com.text = String::new();
                com.button_type = 0;
            } else {
                if self.friend_node_id[client_code as usize] == 0 {
                    com.text = "@red@Offline".into();
                } else if self.friend_node_id[client_code as usize] == self.node_id {
                    com.text = format!(
                        "@gre@World-{}",
                        self.friend_node_id[client_code as usize] - 9
                    );
                } else {
                    com.text = format!(
                        "@yel@World-{}",
                        self.friend_node_id[client_code as usize] - 9
                    );
                }
                com.button_type = 1;
            }
        } else if client_code == CC_FRIENDS_SIZE {
            let mut count = self.friend_count;
            if self.friend_server_status != 2 {
                count = 0;
            }
            com.scroll_height = count * 15 + 20;
            if com.scroll_height <= height {
                com.scroll_height = height + 1;
            }
        } else if (CC_IGNORES_START..=CC_IGNORES_END).contains(&client_code) {
            client_code -= CC_IGNORES_START;
            if client_code >= self.ignore_count {
                com.text = String::new();
                com.button_type = 0;
            } else {
                com.text = JString::to_screen_name(&JString::to_raw_username(
                    self.ignore_userhash[client_code as usize],
                ));
                com.button_type = 1;
            }
        } else if client_code == CC_IGNORES_SIZE {
            com.scroll_height = self.ignore_count * 15 + 20;
            if com.scroll_height <= height {
                com.scroll_height = height + 1;
            }
        } else if client_code == CC_DESIGN_PREVIEW {
            // TS 10773-10820: spin the preview model around the y axis
            // every frame; the combined kit model (built above) is cached
            // under `getModel(5, 0)` when the design changed.
            com.model_xan = 150;
            com.model_yan = ((self.loop_cycle as f64 / 40.0).sin() * 256.0) as i32 & 0x7ff;
            if let Some(model) = preview_model {
                com.model1_type = 5;
                com.model1_id = 0;
                IfType::cache_model(model, 5, 0);
            }
        } else if client_code == CC_SWITCH_TO_MALE {
            // TS 10821-10831: snapshot the two switch-button graphics once,
            // then show the one for the gender being switched to.
            if self.idk_design_button1.is_none() {
                self.idk_design_button1 = Some(com.graphic_name.clone());
                self.idk_design_button2 = Some(graphic2_name.clone());
            }
            if self.idk_design_gender {
                com.graphic_name = self.idk_design_button2.clone().unwrap_or_default();
            } else {
                com.graphic_name = self.idk_design_button1.clone().unwrap_or_default();
            }
        } else if client_code == CC_SWITCH_TO_FEMALE {
            // TS 10832-10842.
            if self.idk_design_button1.is_none() {
                self.idk_design_button1 = Some(com.graphic_name.clone());
                self.idk_design_button2 = Some(graphic2_name.clone());
            }
            if self.idk_design_gender {
                com.graphic_name = self.idk_design_button1.clone().unwrap_or_default();
            } else {
                com.graphic_name = self.idk_design_button2.clone().unwrap_or_default();
            }
        }
    }

    /// TS has no `handleSideIfClicks`: every button click flows through
    /// `buildMinimenu` + `mouseLoop`, which fires the last menu entry via
    /// `doAction` (IF_BUTTON/CLOSE/TOGGLE/SELECT/PAUSE arms). The pre-menu
    /// click handlers would double-send (here then `mouse_loop`), so they
    /// are no-ops; `handle_tab_clicks` still handles the side tabs.
    pub fn handle_side_if_clicks(&mut self) {}

    /// See `handle_side_if_clicks`: main-modal button clicks flow through
    /// `build_minimenu` + `mouse_loop`/`doAction`.
    pub fn handle_main_if_clicks(&mut self) {}

    /// See `handle_side_if_clicks`: chat-modal button clicks flow through
    /// `build_minimenu` + `mouse_loop`/`doAction`.
    pub fn handle_chat_if_clicks(&mut self) {}

    /// `closeModal` from client-ts (10941-10958): send CLOSE_MODAL and
    /// close the side and chat modals locally; `main_modal_id` is reset
    /// like Java `closeModal` (Client.java 3353-3367). `resumed_pause_button`
    /// is cleared like TS 10947/10954 (also written by the IF_* handlers);
    /// the per-modal redraw flags mirror TS
    /// `redrawSide`/`redrawIcons`/`redrawChat`. Not to be confused with the
    /// incoming-server `apply_if_close`.
    fn close_modal(&mut self) {
        self.out.p1_enc(ClientProt::CLOSE_MODAL.id);
        if self.side_modal_id != -1 {
            self.side_modal_id = -1;
            self.redraw_side = true;
            self.redraw_icons = true;
        }
        if self.chat_modal_id != -1 {
            self.chat_modal_id = -1;
            self.redraw_chat = true;
        }
        self.main_modal_id = -1;
        self.resumed_pause_button = false;
    }

    /// `buildMinimenu` from Client.ts (2514-2599): rebuild the minimenu
    /// from the pointer position every frame (called from `other_overlays`
    /// when no menu is open). Returns immediately while an inventory drag
    /// is in flight (`obj_drag_area != 0`, TS 2515). Seeds Cancel at [0]
    /// with `menu_num_entries = 1`, then fills the three regions — the
    /// viewport walks the world picks (`add_world_options`) or the main
    /// modal, the side walks the side modal or the active-tab interface,
    /// the chat walks the chat modal or the chat lines (`add_chat_options`).
    /// The walk also does the slice-3 hover writes (`over_*_com_id`,
    /// scrollbar steps, `hovered_slot`). Finally the bubble sort (TS
    /// 2569-2598) swaps adjacent `<1000`/`>1000` action pairs so the 1000+
    /// examine entries sink toward the bottom of the menu.
    pub fn build_minimenu(&mut self) {
        if self.obj_drag_area != 0 {
            return;
        }

        self.menu_option[0] = "Cancel".into();
        self.menu_action[0] = MiniMenuAction::CANCEL;
        self.menu_num_entries = 1;

        self.add_private_chat_options();
        self.last_over_com_id = 0;

        // Viewport 4..516 x 4..338: the main modal, else the world picks.
        if self.shell.mouse_x > 4
            && self.shell.mouse_y > 4
            && self.shell.mouse_x < 516
            && self.shell.mouse_y < 338
        {
            if self.main_modal_id == -1 {
                self.add_world_options();
            } else {
                self.add_component_options(
                    self.main_modal_id,
                    self.shell.mouse_x,
                    self.shell.mouse_y,
                    4,
                    4,
                    0,
                );
            }
        }
        if self.last_over_com_id != self.over_main_com_id {
            self.over_main_com_id = self.last_over_com_id;
        }

        // Side 553..743 x 205..466: the side modal, else the active tab.
        self.last_over_com_id = 0;
        if self.shell.mouse_x > 553
            && self.shell.mouse_y > 205
            && self.shell.mouse_x < 743
            && self.shell.mouse_y < 466
        {
            if self.side_modal_id != -1 {
                self.add_component_options(
                    self.side_modal_id,
                    self.shell.mouse_x,
                    self.shell.mouse_y,
                    553,
                    205,
                    0,
                );
            } else {
                let icon_id = self
                    .side_icon
                    .get(self.active_icon as usize)
                    .copied()
                    .unwrap_or(-1);
                if icon_id != -1 {
                    self.add_component_options(
                        icon_id,
                        self.shell.mouse_x,
                        self.shell.mouse_y,
                        553,
                        205,
                        0,
                    );
                }
            }
        }
        if self.last_over_com_id != self.over_side_com_id {
            self.redraw_side = true;
            self.over_side_com_id = self.last_over_com_id;
        }

        // Chat 17..496 x 357..453: the chat modal, else the chat lines.
        self.last_over_com_id = 0;
        if self.shell.mouse_x > 17
            && self.shell.mouse_y > 357
            && self.shell.mouse_x < 496
            && self.shell.mouse_y < 453
        {
            if self.chat_modal_id != -1 {
                self.add_component_options(
                    self.chat_modal_id,
                    self.shell.mouse_x,
                    self.shell.mouse_y,
                    17,
                    357,
                    0,
                );
            } else if self.shell.mouse_y < 434 && self.shell.mouse_x < 426 {
                self.add_chat_options(self.shell.mouse_x - 17, self.shell.mouse_y - 357);
            }
        }
        if self.chat_modal_id != -1 && self.last_over_com_id != self.over_chat_com_id {
            self.redraw_chat = true;
            self.over_chat_com_id = self.last_over_com_id;
        }

        // Bubble sort (TS 2569-2598): a `<1000` action directly above a
        // `>1000` one swaps all five fields, so the 1000+ examine entries
        // sink toward the bottom of the drawn menu (index 0 is Cancel).
        let mut sorted = false;
        while !sorted {
            sorted = true;
            for i in 0..(self.menu_num_entries - 1) {
                let (l, r) = (i as usize, (i + 1) as usize);
                if self.menu_action[l] < 1000 && self.menu_action[r] > 1000 {
                    self.menu_option.swap(l, r);
                    self.menu_action.swap(l, r);
                    self.menu_param_b.swap(l, r);
                    self.menu_param_c.swap(l, r);
                    self.menu_param_a.swap(l, r);
                    sorted = false;
                }
            }
        }
    }

    /// `addPrivateChatOptions` from Client.ts (2600-2657): the split
    /// private-chat overlay's menu, active only when `split_private_chat`
    /// is set by clientcode 8. Incoming (3/7) lines offer Report abuse
    /// (staff), Add ignore and Add friend; sent (5/6) lines only count
    /// towards the 5-line cap. An active `rebootTimer` shifts the rows
    /// down one line (TS 2607-2609) so the hover bands match the drawn
    /// rows under the "System update in" line. The add-friend/ignore options
    /// carry `_PRIORITY` like TS 2635/2641.
    fn add_private_chat_options(&mut self) {
        if self.split_private_chat == 0 {
            return;
        }

        let mut line = if self.reboot_timer != 0 { 1 } else { 0 };
        for i in 0..100 {
            if self.chat_text[i].is_empty() {
                continue;
            }
            let r#type = self.chat_type[i];
            let mut sender = self.chat_username[i].clone();
            let mut _mod = false;
            if sender.starts_with("@cr1@") || sender.starts_with("@cr2@") {
                sender = sender[5..].to_string();
                _mod = true;
            }

            if (r#type == 3 || r#type == 7)
                && (r#type == 7
                    || self.chat_private_mode == 0
                    || (self.chat_private_mode == 1 && self.is_friend(&sender)))
            {
                let y = 329 - line * 13;
                if self.shell.mouse_x > 4
                    && self.shell.mouse_x < 516
                    && self.shell.mouse_y - 4 > y - 10
                    && self.shell.mouse_y - 4 <= y + 3
                {
                    if self.staffmodlevel != 0 {
                        let option = format!("Report abuse @whi@{sender}");
                        self.push_option(
                            option,
                            MiniMenuAction::_PRIORITY + MiniMenuAction::ABUSE_REPORT,
                            0,
                            0,
                            0,
                        );
                    }
                    let option = format!("Add ignore @whi@{sender}");
                    self.push_option(
                        option,
                        MiniMenuAction::_PRIORITY + MiniMenuAction::IGNORELIST_ADD,
                        0,
                        0,
                        0,
                    );
                    let option = format!("Add friend @whi@{sender}");
                    self.push_option(
                        option,
                        MiniMenuAction::_PRIORITY + MiniMenuAction::FRIENDLIST_ADD,
                        0,
                        0,
                        0,
                    );
                }
                line += 1;
                if line >= 5 {
                    return;
                }
            } else if (r#type == 5 || r#type == 6) && self.chat_private_mode < 2 {
                line += 1;
                if line >= 5 {
                    return;
                }
            }
        }
    }

    /// `addChatOptions` from Client.ts (2658-2740): the chat-line menu.
    /// Public lines (1/2) and private lines (3/7, only while the split
    /// overlay is off) offer Report abuse (staff), Add ignore and
    /// Add friend; trade (4) / duel (8) reqs offer Accept; sent PM lines
    /// (5/6) count only. The mode-1 `isFriend(sender)` gates match
    /// `draw_chat`, so hover bands line up with the drawn rows.
    fn add_chat_options(&mut self, _mouse_x: i32, mouse_y: i32) {
        let mut line = 0;
        for i in 0..100 {
            if self.chat_text[i].is_empty() {
                continue;
            }
            let r#type = self.chat_type[i];
            let y = self.chat_scroll_pos + 70 + 4 - line * 14;
            if y < -20 {
                break;
            }
            let mut sender = self.chat_username[i].clone();
            let mut _mod = false;
            if sender.starts_with("@cr1@") || sender.starts_with("@cr2@") {
                sender = sender[5..].to_string();
                _mod = true;
            }

            if r#type == 0 {
                line += 1;
            } else if (r#type == 1 || r#type == 2)
                && (r#type == 1
                    || self.chat_public_mode == 0
                    || (self.chat_public_mode == 1 && self.is_friend(&sender)))
            {
                // TS 2687: `localPlayer && sender !== localPlayer.name`.
                let not_self = match &self.local_player {
                    None => false,
                    Some(p) => p.name.as_deref() != Some(sender.as_str()),
                };
                if mouse_y > y - 14 && mouse_y <= y && not_self {
                    if self.staffmodlevel >= 1 {
                        let option = format!("Report abuse @whi@{sender}");
                        self.push_option(option, MiniMenuAction::ABUSE_REPORT, 0, 0, 0);
                    }
                    let option = format!("Add ignore @whi@{sender}");
                    self.push_option(option, MiniMenuAction::IGNORELIST_ADD, 0, 0, 0);
                    let option = format!("Add friend @whi@{sender}");
                    self.push_option(option, MiniMenuAction::FRIENDLIST_ADD, 0, 0, 0);
                }
                line += 1;
            } else if (r#type == 3 || r#type == 7)
                && self.split_private_chat == 0
                && (r#type == 7
                    || self.chat_private_mode == 0
                    || (self.chat_private_mode == 1 && self.is_friend(&sender)))
            {
                if mouse_y > y - 14 && mouse_y <= y {
                    if self.staffmodlevel >= 1 {
                        let option = format!("Report abuse @whi@{sender}");
                        self.push_option(option, MiniMenuAction::ABUSE_REPORT, 0, 0, 0);
                    }
                    let option = format!("Add ignore @whi@{sender}");
                    self.push_option(option, MiniMenuAction::IGNORELIST_ADD, 0, 0, 0);
                    let option = format!("Add friend @whi@{sender}");
                    self.push_option(option, MiniMenuAction::FRIENDLIST_ADD, 0, 0, 0);
                }
                line += 1;
            } else if r#type == 4
                && (self.chat_trade_mode == 0
                    || (self.chat_trade_mode == 1 && self.is_friend(&sender)))
            {
                if mouse_y > y - 14 && mouse_y <= y {
                    let option = format!("Accept trade @whi@{sender}");
                    self.push_option(option, MiniMenuAction::ACCEPT_TRADEREQ, 0, 0, 0);
                }
                line += 1;
            } else if (r#type == 5 || r#type == 6)
                && self.split_private_chat == 0
                && self.chat_private_mode < 2
            {
                line += 1;
            } else if r#type == 8
                && (self.chat_trade_mode == 0
                    || (self.chat_trade_mode == 1 && self.is_friend(&sender)))
            {
                if mouse_y > y - 14 && mouse_y <= y {
                    let option = format!("Accept duel @whi@{sender}");
                    self.push_option(option, MiniMenuAction::ACCEPT_DUELREQ, 0, 0, 0);
                }
                line += 1;
            }
        }
    }

    /// Slice-3 hover entry point, kept for the obj-drag drop re-walk and
    /// the hud tests: the pointer walk now lives in `build_minimenu`.
    pub fn update_if_pointer(&mut self) {
        self.build_minimenu();
    }

    /// `addComponentOptions` from Client.ts (9628-9841): the hover walk
    /// plus the minimenu option strings for the tree rooted at `com_id`.
    /// A pointer inside a child with `over_layer_id` or `colour_over`
    /// records `last_over_com_id` (the `over_layer_id` when set, else the
    /// child id). `TYPE_LAYER` children recurse with their scroll, then a
    /// scrollable layer (`scroll_height > height`) steps its `do_scrollbar`.
    /// `TYPE_INV` children record the slot under the pointer
    /// (`hovered_slot`/`hovered_slot_com_id`) even when the slot is empty
    /// (the Task 8 drop target), and push the held/button options for an
    /// occupied slot — Use/TGT, obj iop 4..3 with the Drop fallback, Use,
    /// obj iop 2..0, `child.iop` INV_BUTTONs, then Examine (TS 9684-9790).
    /// Non-inv children under the pointer push their button option
    /// (OK/CLOSE/TOGGLE/SELECT/CONTINUE/TARGET, TS 9795-9839); the social
    /// (friend/ignore) override is slice 5, so BUTTON_OK always uses
    /// `button_text`.
    fn add_component_options(
        &mut self,
        com_id: i32,
        mouse_x: i32,
        mouse_y: i32,
        x: i32,
        y: i32,
        scroll: i32,
    ) {
        let Some(com) = self.if_(com_id as usize) else {
            return;
        };
        if com.r#type != ComponentType::TYPE_LAYER
            || com.hide
            || mouse_x < x
            || mouse_y < y
            || mouse_x > x + com.width
            || mouse_y > y + com.height
        {
            return;
        }
        let children = match &com.children {
            Some(c) => c.clone(),
            None => return,
        };
        let child_x = match &com.child_x {
            Some(c) => c.clone(),
            None => return,
        };
        let child_y = match &com.child_y {
            Some(c) => c.clone(),
            None => return,
        };
        for i in 0..children.len() {
            let child_id = children[i];
            // An owned merged snapshot: the option pushes below call
            // `push_option` (`&mut self`) while the walk reads the child
            // fields (a view borrow of the whole client cannot span them).
            let Some(child) = self.if_(child_id as usize).map(|v| IfTypeOwned::from(&v)) else {
                continue;
            };
            let child_x = child_x[i] + x + child.x;
            let child_y = child_y[i] + y - scroll + child.y;

            if (child.over_layer_id >= 0 || child.colour_over != 0)
                && mouse_x >= child_x
                && mouse_y >= child_y
                && mouse_x < child_x + child.width
                && mouse_y < child_y + child.height
            {
                self.last_over_com_id = if child.over_layer_id >= 0 {
                    child.over_layer_id
                } else {
                    child.id
                };
            }

            match child.r#type {
                ComponentType::TYPE_LAYER => {
                    self.add_component_options(
                        child_id,
                        mouse_x,
                        mouse_y,
                        child_x,
                        child_y,
                        child.scroll_pos,
                    );
                    let (child_w, child_h, child_sh) = self
                        .if_(child_id as usize)
                        .map(|c| (c.width, c.height, c.scroll_height))
                        .unwrap_or((0, 0, 0));
                    if child_sh > child_h {
                        self.do_scrollbar(
                            mouse_x,
                            mouse_y,
                            child_sh,
                            child_h,
                            true,
                            child_x + child_w,
                            child_y,
                            child_id,
                        );
                    }
                }
                ComponentType::TYPE_INV => {
                    let child_id = child.id;
                    let inv_iop = child.iop.clone();
                    let obj_ops = child.obj_ops;
                    let obj_use = child.obj_use;
                    let mut slot = 0;
                    for row in 0..child.height {
                        for col in 0..child.width {
                            let mut slot_x = child_x + col * (child.margin_x + 32);
                            let mut slot_y = child_y + row * (child.margin_y + 32);
                            if slot < 20 {
                                if let Some(xs) = &child.inv_background_x {
                                    slot_x += xs[slot as usize];
                                }
                                if let Some(ys) = &child.inv_background_y {
                                    slot_y += ys[slot as usize];
                                }
                            }
                            if mouse_x < slot_x
                                || mouse_y < slot_y
                                || mouse_x >= slot_x + 32
                                || mouse_y >= slot_y + 32
                            {
                                slot += 1;
                                continue;
                            }
                            self.hovered_slot = slot;
                            self.hovered_slot_com_id = child_id;

                            // TS 9678: empty slots (no link) stop here.
                            let Some(obj_id) = child
                                .link_obj_type
                                .as_ref()
                                .and_then(|t| t.get(slot as usize))
                                .copied()
                                .filter(|&id| id > 0)
                            else {
                                slot += 1;
                                continue;
                            };
                            let obj_id = obj_id - 1;
                            let Some((obj_name, obj_iop)) = self
                                .cache
                                .objs
                                .get(obj_id as usize)
                                .map(|o| (o.name.clone(), o.iop.clone()))
                            else {
                                slot += 1;
                                continue;
                            };

                            if self.use_mode == 1 && obj_ops {
                                // the selected slot itself has no Use option
                                // (TS 9686)
                                if child_id != self.obj_selected_com_id
                                    || slot != self.obj_selected_slot
                                {
                                    let option = format!(
                                        "Use {} with @lre@{}",
                                        self.obj_selected_name, obj_name
                                    );
                                    self.push_option(
                                        option,
                                        MiniMenuAction::USEHELD_ONHELD,
                                        obj_id,
                                        slot,
                                        child_id,
                                    );
                                }
                            } else if self.target_mode == 1 && obj_ops {
                                if (self.target_mask & 0x10) == 0x10 {
                                    let option = format!("{} @lre@{}", self.target_op, obj_name);
                                    self.push_option(
                                        option,
                                        MiniMenuAction::TGT_HELD,
                                        obj_id,
                                        slot,
                                        child_id,
                                    );
                                }
                            } else {
                                // obj iop 4..3 with the Drop fallback at 4
                                // (TS 9706-9724)
                                if obj_ops {
                                    for op in (3..=4).rev() {
                                        if let Some(text) = &obj_iop[op as usize] {
                                            let option = format!("{text} @lre@{obj_name}");
                                            let action = if op == 3 {
                                                MiniMenuAction::OP_HELD4
                                            } else {
                                                MiniMenuAction::OP_HELD5
                                            };
                                            self.push_option(
                                                option, action, obj_id, slot, child_id,
                                            );
                                        } else if op == 4 {
                                            let option = format!("Drop @lre@{obj_name}");
                                            self.push_option(
                                                option,
                                                MiniMenuAction::OP_HELD5,
                                                obj_id,
                                                slot,
                                                child_id,
                                            );
                                        }
                                    }
                                }

                                if obj_use {
                                    let option = format!("Use @lre@{obj_name}");
                                    self.push_option(
                                        option,
                                        MiniMenuAction::USEHELD_START,
                                        obj_id,
                                        slot,
                                        child_id,
                                    );
                                }

                                // obj iop 2..0 (TS 9737-9753)
                                if obj_ops {
                                    for op in (0..=2).rev() {
                                        if let Some(text) = &obj_iop[op as usize] {
                                            let option = format!("{text} @lre@{obj_name}");
                                            let action = match op {
                                                0 => MiniMenuAction::OP_HELD1,
                                                1 => MiniMenuAction::OP_HELD2,
                                                _ => MiniMenuAction::OP_HELD3,
                                            };
                                            self.push_option(
                                                option, action, obj_id, slot, child_id,
                                            );
                                        }
                                    }
                                }

                                // the component's own iop 4..0 (TS 9759-9781)
                                for op in (0..=4).rev() {
                                    if let Some(text) = &inv_iop[op as usize] {
                                        let option = format!("{text} @lre@{obj_name}");
                                        let action = match op {
                                            0 => MiniMenuAction::INV_BUTTON1,
                                            1 => MiniMenuAction::INV_BUTTON2,
                                            2 => MiniMenuAction::INV_BUTTON3,
                                            3 => MiniMenuAction::INV_BUTTON4,
                                            _ => MiniMenuAction::INV_BUTTON5,
                                        };
                                        self.push_option(option, action, obj_id, slot, child_id);
                                    }
                                }

                                let option = format!("Examine @lre@{obj_name}");
                                self.push_option(
                                    option,
                                    MiniMenuAction::OP_HELD6,
                                    obj_id,
                                    slot,
                                    child_id,
                                );
                            }

                            slot += 1;
                        }
                    }
                }
                _ => {
                    if mouse_x >= child_x
                        && mouse_y >= child_y
                        && mouse_x < child_x + child.width
                        && mouse_y < child_y + child.height
                    {
                        if child.button_type == ButtonType::BUTTON_OK {
                            // TS 9798-9807: a friend/ignore-list component
                            // overrides the button text with Remove/Message
                            // options; otherwise the label fires IF_BUTTON.
                            let mut override_ = false;
                            if child.client_code != 0 {
                                override_ = self.add_social_options(&child);
                            }
                            // Java 5962-5966: no empty-text gate. Emote
                            // tiles on the player-controls panel are OK
                            // graphics with empty option strings.
                            if !override_ {
                                self.push_option(
                                    child.button_text.clone(),
                                    MiniMenuAction::IF_BUTTON,
                                    0,
                                    0,
                                    child.id,
                                );
                            }
                        } else if child.button_type == ButtonType::BUTTON_TARGET
                            && self.target_mode == 0
                        {
                            // prefix is the first word of `target_verb`
                            // (TS 9808-9811)
                            let mut prefix = child.target_verb.clone();
                            if let Some(space) = prefix.find(' ') {
                                prefix.truncate(space);
                            }
                            let option = format!("{} @gre@{}", prefix, child.target_base);
                            self.push_option(option, MiniMenuAction::TGT_BUTTON, 0, 0, child.id);
                        } else if child.button_type == ButtonType::BUTTON_CLOSE {
                            self.push_option(
                                "Close".into(),
                                MiniMenuAction::CLOSE_BUTTON,
                                0,
                                0,
                                child.id,
                            );
                        } else if child.button_type == ButtonType::BUTTON_TOGGLE
                            && !child.button_text.is_empty()
                        {
                            self.push_option(
                                child.button_text.clone(),
                                MiniMenuAction::TOGGLE_BUTTON,
                                0,
                                0,
                                child.id,
                            );
                        } else if child.button_type == ButtonType::BUTTON_SELECT
                            && !child.button_text.is_empty()
                        {
                            self.push_option(
                                child.button_text.clone(),
                                MiniMenuAction::SELECT_BUTTON,
                                0,
                                0,
                                child.id,
                            );
                        } else if child.button_type == ButtonType::BUTTON_CONTINUE
                            && !self.resumed_pause_button
                            && !child.button_text.is_empty()
                        {
                            self.push_option(
                                child.button_text.clone(),
                                MiniMenuAction::PAUSE_BUTTON,
                                0,
                                0,
                                child.id,
                            );
                        }
                    }
                }
            }
        }
    }

    /// `addSocialOptions` from Client.ts (9844-9873): a friend/ignore-list
    /// BUTTON_OK component's right-click options. The friend ranges
    /// (1..=200, 701..=900) push Remove and Message with the friend name
    /// (the index arithmetic is TS 9847-9855, including the 701/801
    /// offsets); the ignore range (401..=500) pushes Remove with the
    /// component's own text. Returns true when the button text is
    /// overridden, false otherwise.
    fn add_social_options(&mut self, component: &IfTypeOwned) -> bool {
        let mut client_code = component.client_code;
        if (CC_FRIENDS_START..=CC_FRIENDS_UPDATE_END).contains(&client_code)
            || (CC_FRIENDS2_START..=CC_FRIENDS2_UPDATE_END).contains(&client_code)
        {
            if client_code >= 801 {
                client_code -= 701;
            } else if client_code >= 701 {
                client_code -= 601;
            } else if client_code >= CC_FRIENDS_UPDATE_START {
                client_code -= CC_FRIENDS_UPDATE_START;
            } else {
                client_code -= 1;
            }

            let option = format!("Remove @whi@{}", self.friend_username[client_code as usize]);
            self.push_option(option, MiniMenuAction::FRIENDLIST_DEL, 0, 0, 0);

            let option = format!(
                "Message @whi@{}",
                self.friend_username[client_code as usize]
            );
            self.push_option(option, MiniMenuAction::MESSAGE_PRIVATE, 0, 0, 0);
            return true;
        } else if (CC_IGNORES_START..=CC_IGNORES_END).contains(&client_code) {
            let option = format!("Remove @whi@{}", component.text);
            self.push_option(option, MiniMenuAction::IGNORELIST_DEL, 0, 0, 0);
            return true;
        }

        false
    }

    fn dispatch_packet(&mut self, ptype: i32, payload: &mut Packet) {
        match ptype {
            ServerProt::REBUILD_NORMAL => {
                let zone_x = payload.g2();
                let zone_z = payload.g2();

                if self.map_build_centre_zone_x == zone_x
                    && self.map_build_centre_zone_z == zone_z
                    && self.scene_state == 2
                {
                    self.ptype = -1;
                    return;
                }

                self.map_build_centre_zone_x = zone_x;
                self.map_build_centre_zone_z = zone_z;
                self.map_build_base_x = (self.map_build_centre_zone_x - 6) * 8;
                self.map_build_base_z = (self.map_build_centre_zone_z - 6) * 8;

                self.within_tutorial_island = ((self.map_build_centre_zone_x / 8 == 48
                    || self.map_build_centre_zone_x / 8 == 49)
                    && self.map_build_centre_zone_z / 8 == 48)
                    || (self.map_build_centre_zone_x / 8 == 48
                        && self.map_build_centre_zone_z / 8 == 148);

                self.scene_state = 1;
                self.scene_load_start_time = Instant::now();

                // The loading splash is drawn by the renderer: `check_minimap`
                // paints it (and builds the scene) on the next `mainredraw`.

                let start_x = (self.map_build_centre_zone_x - 6) / 8;
                let end_x = (self.map_build_centre_zone_x + 6) / 8;
                let start_z = (self.map_build_centre_zone_z - 6) / 8;
                let end_z = (self.map_build_centre_zone_z + 6) / 8;
                let regions = ((end_x - start_x + 1) * (end_z - start_z + 1)) as usize;

                self.map_build_ground_data = vec![None; regions];
                self.map_build_location_data = vec![None; regions];
                self.map_build_index = vec![0; regions];
                self.map_build_ground_file = vec![0; regions];
                self.map_build_location_file = vec![0; regions];

                let mut map_count = 0;
                for x in start_x..=end_x {
                    for z in start_z..=end_z {
                        self.map_build_index[map_count] = (x << 8) + z;

                        if self.within_tutorial_island
                            && (z == 49 || z == 149 || z == 147 || x == 50 || (x == 49 && z == 47))
                        {
                            self.map_build_ground_file[map_count] = -1;
                            self.map_build_location_file[map_count] = -1;
                            map_count += 1;
                        } else if let Some(od) = &mut self.on_demand {
                            let land_file = od.get_map_file(x, z, 0);
                            self.map_build_ground_file[map_count] = land_file;
                            if land_file != -1 {
                                od.request(3, land_file);
                            }
                            let loc_file = od.get_map_file(x, z, 1);
                            self.map_build_location_file[map_count] = loc_file;
                            if loc_file != -1 {
                                od.request(3, loc_file);
                            }
                            map_count += 1;
                        }
                    }
                }

                let dx = self.map_build_base_x - self.map_build_prev_base_x;
                let dz = self.map_build_base_z - self.map_build_prev_base_z;
                self.map_build_prev_base_x = self.map_build_base_x;
                self.map_build_prev_base_z = self.map_build_base_z;

                for npc in self.npc.iter_mut().flatten() {
                    for j in 0..10 {
                        npc.route_x[j] -= dx;
                        npc.route_z[j] -= dz;
                    }
                    npc.x -= dx * 128;
                    npc.z -= dz * 128;
                }

                for player in self.players.iter_mut().flatten() {
                    for j in 0..10 {
                        player.route_x[j] -= dx;
                        player.route_z[j] -= dz;
                    }
                    player.x -= dx * 128;
                    player.z -= dz * 128;
                }

                // Java `localPlayer` IS `players[LOCAL_PLAYER_INDEX]`, so the
                // shift loop above also moves the local body with the build
                // origin; the Rust clone must follow or NPC_INFO places new
                // NPCs relative to an unshifted local.
                if let Some(local) = self.local_player.as_mut() {
                    for j in 0..10 {
                        local.route_x[j] -= dx;
                        local.route_z[j] -= dz;
                    }
                    local.x -= dx * 128;
                    local.z -= dz * 128;
                }

                self.awaiting_player_info = true;

                // TS 6907-6948: carry groundObj and locChanges across the
                // build-area move. The scan runs in the signed direction of
                // dx/dz so a positive delta copies tiles that are still
                // needed; a naive `0..SIZE` sweep would overwrite them.
                // A zero delta is a no-op (TS self-assigns, preserving every
                // stacked item), so skip the whole shift.
                if dx != 0 || dz != 0 {
                    let mut start_tile_x = 0;
                    let mut end_tile_x = BuildArea::SIZE;
                    let mut dir_x = 1;
                    if dx < 0 {
                        start_tile_x = BuildArea::SIZE - 1;
                        end_tile_x = -1;
                        dir_x = -1;
                    }

                    let mut start_tile_z = 0;
                    let mut end_tile_z = BuildArea::SIZE;
                    let mut dir_z = 1;
                    if dz < 0 {
                        start_tile_z = BuildArea::SIZE - 1;
                        end_tile_z = -1;
                        dir_z = -1;
                    }

                    let mut x = start_tile_x;
                    while x != end_tile_x {
                        let mut z = start_tile_z;
                        while z != end_tile_z {
                            let last_x = x + dx;
                            let last_z = z + dz;
                            for level in 0..BuildArea::LEVELS {
                                let cell = if last_x >= 0
                                    && last_z >= 0
                                    && last_x < BuildArea::SIZE
                                    && last_z < BuildArea::SIZE
                                {
                                    self.ground_obj[level as usize][last_x as usize]
                                        [last_z as usize]
                                        .take()
                                } else {
                                    None
                                };
                                self.ground_obj[level as usize][x as usize][z as usize] = cell;
                            }
                            z += dir_z;
                        }
                        x += dir_x;
                    }

                    let mut node = self.loc_changes.head();
                    while let Some(loc) = node {
                        loc.x -= dx;
                        loc.z -= dz;
                        if loc.x < 0
                            || loc.z < 0
                            || loc.x >= BuildArea::SIZE
                            || loc.z >= BuildArea::SIZE
                        {
                            self.loc_changes.unlink_last();
                        }
                        node = self.loc_changes.next_node();
                    }

                    if self.minimap_flag_x != 0 {
                        self.minimap_flag_x -= dx;
                        self.minimap_flag_z -= dz;
                    }
                }

                self.ptype = -1;
            }

            // interface draw/modal state (mainModalId, sideIcon, activeIcon,
            // chatModalId, tutComId) handlers.
            ServerProt::IF_SETICON => {
                self.apply_if_seticon(payload);
                self.ptype = -1;
            }

            ServerProt::IF_SHOWICON => {
                self.apply_if_showicon(payload.g1());
                self.ptype = -1;
            }

            // IF_OPENSIDE carries a 2-byte com_id the skip-arm must not eat
            // unread.
            ServerProt::IF_OPENSIDE => {
                self.apply_if_openside(payload);
                self.ptype = -1;
            }

            ServerProt::IF_CLOSE => {
                self.apply_if_close();
                self.ptype = -1;
            }

            ServerProt::IF_OPENCHAT => {
                self.apply_if_openchat(payload);
                self.ptype = -1;
            }

            ServerProt::IF_OPENMAIN_SIDE => {
                self.apply_if_openmain_side(payload);
                self.ptype = -1;
            }

            ServerProt::IF_OPENMAIN => {
                self.apply_if_openmain(payload);
                self.ptype = -1;
            }

            ServerProt::IF_OPENOVERLAY => {
                self.apply_if_openoverlay(payload);
                self.ptype = -1;
            }

            ServerProt::TUT_FLASH => {
                self.apply_tut_flash(payload.g1());
                self.ptype = -1;
            }

            ServerProt::TUT_OPEN => {
                self.apply_tut_open(payload);
                self.ptype = -1;
            }

            ServerProt::IF_SETCOLOUR => {
                let com_id = payload.g2();
                let colour = payload.g2();

                let r = (colour >> 10) & 0x1f;
                let g = (colour >> 5) & 0x1f;
                let b = colour & 0x1f;

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.colour = (r << 19) + (g << 11) + (b << 3);
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETHIDE => {
                let com_id = payload.g2();
                let hide = payload.g1() == 1;

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.hide = hide;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETOBJECT => {
                let com_id = payload.g2();
                let obj_id = payload.g2();
                let zoom = payload.g2();

                let (xan2d, yan2d, zoom2d) = if (obj_id as usize) < self.cache.objs.len() {
                    let r#type = self.cache.obj(obj_id as usize);
                    (r#type.xan2d, r#type.yan2d, r#type.zoom2d)
                } else {
                    (0, 0, 0)
                };

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.model1_type = 4;
                    com.model1_id = obj_id;
                    com.model_xan = xan2d;
                    com.model_yan = yan2d;
                    // JS `(x * 100) / zoom | 0` with zoom 0 is Infinity | 0 = 0
                    com.model_zoom = if zoom == 0 { 0 } else { (zoom2d * 100) / zoom };
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETMODEL => {
                let com_id = payload.g2();
                let model_id = payload.g2();

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.model1_type = 1;
                    com.model1_id = model_id;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETANIM => {
                let com_id = payload.g2();
                let seq_id = payload.g2b();

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.model_anim = seq_id;
                    if seq_id == -1 {
                        com.anim_frame = 0;
                        com.anim_cycle = 0;
                    }
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETPLAYERHEAD => {
                let com_id = payload.g2();
                let head = self.local_player.as_ref().map(|local| {
                    (local.appearance[8] as i32) << 6
                        | (local.appearance[0] as i32) << 12
                        | (local.colour[0] as i32) << 24
                        | (local.colour[4] as i32) << 18
                        | local.appearance[11] as i32
                });

                if let (Some(com), Some(model1_id)) = (
                    Arc::make_mut(&mut self.ifaces_mut)
                        .get_mut(com_id as usize)
                        .and_then(|o| o.as_mut())
                        .map(Arc::make_mut),
                    head,
                ) {
                    com.model1_type = 3;
                    com.model1_id = model1_id;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETTEXT => {
                let com_id = payload.g2();
                let text = payload.gjstr();
                // Decode read (`layer_id`) before the overlay borrow.
                let on_active_tab = self
                    .if_(com_id as usize)
                    .is_some_and(|com| com.layer_id == self.side_icon[self.active_icon as usize]);

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.text = text;
                    // TS 6164: redraw the side when the edited text sits on
                    // the active tab's interface.
                    if on_active_tab {
                        self.redraw_side = true;
                    }
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETNPCHEAD => {
                let com_id = payload.g2();
                let npc_id = payload.g2();

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.model1_type = 2;
                    com.model1_id = npc_id;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETPOSITION => {
                let com_id = payload.g2();
                let x = payload.g2b();
                let y = payload.g2b();

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    com.x = x;
                    com.y = y;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETSCROLLPOS => {
                let com_id = payload.g2();
                let mut pos = payload.g2();
                // Decode reads (type, height) before the overlay borrow.
                let layer = self
                    .if_(com_id as usize)
                    .map(|com| (com.r#type, com.height))
                    .unwrap_or((-1, 0));

                if let Some(com) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    if layer.0 == ComponentType::TYPE_LAYER {
                        if pos < 0 {
                            pos = 0;
                        }
                        if pos > com.scroll_height - layer.1 {
                            pos = com.scroll_height - layer.1;
                        }
                        com.scroll_pos = pos;
                    }
                }
                self.ptype = -1;
            }

            // TS `IfType.list[comId].linkObj*` writes; the interface table is
            // not loaded yet so these are inert until the interface jag task.
            ServerProt::UPDATE_INV_STOP_TRANSMIT => {
                let com_id = payload.g2();

                if let Some(inv) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    if let Some(link_types) = inv.link_obj_type.as_mut() {
                        // [sic] TS writes -1 then 0; the final value is 0
                        for t in link_types.iter_mut() {
                            *t = 0;
                        }
                    }
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_INV_FULL => {
                self.redraw_side = true;

                let com_id = payload.g2();
                let size = payload.g1();

                // Always consume the frame (TS still reads when the iface is
                // missing; skipping here would desync once ifaces load).
                let mut slots = Vec::with_capacity(size as usize);
                for _ in 0..size {
                    slots.push(Self::read_inv_count(payload));
                }

                if let Some(inv) = Arc::make_mut(&mut self.ifaces_mut)
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                    .map(Arc::make_mut)
                {
                    if let (Some(link_types), Some(link_numbers)) =
                        (inv.link_obj_type.as_mut(), inv.link_obj_number.as_mut())
                    {
                        let n = size.min(link_types.len() as i32) as usize;
                        for i in 0..n {
                            link_types[i] = slots[i].0;
                            link_numbers[i] = slots[i].1;
                        }
                        for i in n..link_types.len() {
                            link_types[i] = 0;
                            link_numbers[i] = 0;
                        }
                    }
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_INV_PARTIAL => {
                self.redraw_side = true;

                let com_id = payload.g2();
                let end = self.inbound_end(payload);

                // Consume every slot even if the component is missing.
                while payload.pos < end {
                    let slot = payload.g1();
                    let (id, count) = Self::read_inv_count(payload);

                    if let Some(inv) = Arc::make_mut(&mut self.ifaces_mut)
                        .get_mut(com_id as usize)
                        .and_then(|o| o.as_mut())
                        .map(Arc::make_mut)
                    {
                        if let (Some(link_types), Some(link_numbers)) =
                            (inv.link_obj_type.as_mut(), inv.link_obj_number.as_mut())
                        {
                            if slot >= 0 && (slot as usize) < link_types.len() {
                                link_types[slot as usize] = id;
                                link_numbers[slot as usize] = count;
                            }
                        }
                    }
                }
                self.ptype = -1;
            }

            ServerProt::CAM_LOOKAT => {
                self.apply_cam_lookat(payload);
                self.ptype = -1;
            }

            ServerProt::CAM_SHAKE => {
                self.apply_cam_shake(payload);
                self.ptype = -1;
            }

            ServerProt::CAM_MOVETO => {
                self.apply_cam_moveto(payload);
                self.ptype = -1;
            }

            ServerProt::CAM_RESET => {
                self.apply_cam_reset(payload);
                self.ptype = -1;
            }

            ServerProt::NPC_INFO => {
                self.get_npc_pos(payload, self.psize);
                self.ptype = -1;
            }

            ServerProt::PLAYER_INFO => {
                self.get_player_pos(payload, self.psize);
                self.awaiting_player_info = false;
                self.ptype = -1;
            }

            ServerProt::MESSAGE_GAME => {
                self.apply_message_game(payload);
                self.ptype = -1;
            }

            ServerProt::UPDATE_IGNORELIST => {
                self.apply_update_ignorelist(payload, self.psize);
                self.ptype = -1;
            }

            ServerProt::CHAT_FILTER_SETTINGS => {
                self.apply_chat_filter_settings(payload);
                self.ptype = -1;
            }

            ServerProt::MESSAGE_PRIVATE => {
                self.apply_message_private(payload, self.psize);
                self.ptype = -1;
            }

            ServerProt::FRIENDLIST_LOADED => {
                self.apply_friendlist_loaded(payload);
                self.ptype = -1;
            }

            ServerProt::UPDATE_FRIENDLIST => {
                self.apply_update_friendlist(payload);
                self.ptype = -1;
            }

            ServerProt::UNSET_MAP_FLAG => {
                self.minimap_flag_x = 0;
                self.ptype = -1;
            }

            ServerProt::UPDATE_RUNWEIGHT => {
                self.apply_update_runweight(payload);
                self.ptype = -1;
            }

            ServerProt::HINT_ARROW => {
                self.apply_hint_arrow(payload);
                self.ptype = -1;
            }

            ServerProt::UPDATE_REBOOT_TIMER => {
                self.apply_update_reboot_timer(payload);
                self.ptype = -1;
            }

            ServerProt::UPDATE_STAT => {
                self.redraw_side = true;

                let stat = payload.g1();
                let xp = payload.g4();
                let level = payload.g1();

                // TS Int32Array stores silently ignore out-of-range stats
                if stat >= 0 && (stat as usize) < self.stat_xp.len() {
                    self.stat_xp[stat as usize] = xp;
                    self.stat_effective_level[stat as usize] = level;
                    let mut base: i32 = 1;
                    for i in 0..98 {
                        if xp >= level_experience()[i] {
                            base = (i + 2) as i32;
                        }
                    }
                    self.stat_base_level[stat as usize] = base;
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_RUNENERGY => {
                if self.active_icon == 12 {
                    self.redraw_side = true;
                }
                self.runenergy = payload.g1();
                self.ptype = -1;
            }

            ServerProt::RESET_ANIMS => {
                for player in self.players.iter_mut().flatten() {
                    player.primary_anim = -1;
                }
                for npc in self.npc.iter_mut().flatten() {
                    npc.primary_anim = -1;
                }
                self.ptype = -1;
            }

            ServerProt::LAST_LOGIN_INFO => {
                self.ptype = -1;
            }

            ServerProt::P_COUNTDIALOG => {
                self.apply_p_countdialog();
                self.ptype = -1;
            }

            ServerProt::SET_MULTIWAY => {
                self.apply_set_multiway(payload);
                self.ptype = -1;
            }

            ServerProt::MINIMAP_TOGGLE => {
                self.apply_minimap_toggle(payload);
                self.ptype = -1;
            }

            ServerProt::UPDATE_PID => {
                // TS reads selfSlot (g2) then membersAccount (g1) straight
                // off the payload.
                self.self_slot = payload.g2();
                self.members_account = payload.g1();
                self.ptype = -1;
            }

            ServerProt::SET_PLAYER_OP => {
                // TS 6789-6800: `SET_PLAYER_OP` fills `playerOp`/`playerOpPriority`.
                let index = payload.g1();
                let priority = payload.g1();
                let op = payload.gjstr();
                if (1..=5).contains(&index) {
                    let op = if op.eq_ignore_ascii_case("null") {
                        None
                    } else {
                        Some(op)
                    };
                    self.player_op[(index - 1) as usize] = op;
                    self.player_op_priority[(index - 1) as usize] = priority == 0;
                }
                self.ptype = -1;
            }

            ServerProt::LOGOUT => {
                self.logout();
                self.ptype = -1;
            }

            ServerProt::VARP_SMALL => {
                let varp_id = payload.g2();
                let value = payload.g1b();

                grow_write(&mut self.var_serv, varp_id, value);
                if self.var.get(varp_id as usize).copied() != Some(value) {
                    grow_write(&mut self.var, varp_id, value);
                    // TS also calls clientVar(varpId) here (varbit/stat
                    // mapping still lands with the varp task; the midi
                    // clientcode 3 branch is live).
                    self.client_var(varp_id);
                    self.redraw_side = true;
                }
                self.ptype = -1;
            }

            ServerProt::VARP_LARGE => {
                let varp_id = payload.g2();
                let value = payload.g4();

                grow_write(&mut self.var_serv, varp_id, value);
                if self.var.get(varp_id as usize).copied() != Some(value) {
                    grow_write(&mut self.var, varp_id, value);
                    self.client_var(varp_id);
                    self.redraw_side = true;
                }
                self.ptype = -1;
            }

            ServerProt::VARP_SYNC => {
                // "Resetting variables to authoritative set"
                for i in 0..self.var.len() {
                    if self.var_serv.get(i).copied() != Some(self.var[i]) {
                        if let Some(&value) = self.var_serv.get(i) {
                            self.var[i] = value;
                        } else {
                            // JS `var[i] = varServ[i]` with a missing entry
                            // stores undefined; i32 cannot, so leave 0.
                            self.var[i] = 0;
                        }
                        self.client_var(i as i32);
                        self.redraw_side = true;
                    }
                }
                self.ptype = -1;
            }

            ServerProt::SYNTH_SOUND => {
                let sound_id = payload.g2();
                let loops = payload.g1();
                let delay = payload.g2();

                if self.wave_enabled && !self.config.lowmem && self.wave_count < 50 {
                    let delay = delay
                        + self
                            .jagfx
                            .delays
                            .get(sound_id as usize)
                            .copied()
                            .unwrap_or(0);
                    self.wave_ids[self.wave_count as usize] = sound_id;
                    self.wave_loops[self.wave_count as usize] = loops;
                    self.wave_delay[self.wave_count as usize] = delay;
                    self.wave_count += 1;
                }
                self.ptype = -1;
            }

            ServerProt::MIDI_SONG => {
                let mut song_id = payload.g2();
                if song_id == 65535 {
                    song_id = -1;
                }

                if self.next_midi_song != song_id
                    && self.midi_active
                    && !self.config.lowmem
                    && self.next_music_delay == 0
                {
                    self.midi_song = song_id;
                    self.midi_fading = true;
                    if let Some(od) = &mut self.on_demand {
                        od.request(2, self.midi_song);
                    }
                }

                self.next_midi_song = song_id;
                self.ptype = -1;
            }

            ServerProt::MIDI_JINGLE => {
                let jingle_id = payload.g2();
                let delay = payload.g2();

                if self.midi_active && !self.config.lowmem {
                    self.midi_song = jingle_id;
                    self.midi_fading = false;
                    if let Some(od) = &mut self.on_demand {
                        od.request(2, self.midi_song);
                    }
                    self.next_music_delay = delay;
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_ZONE_PARTIAL_FOLLOWS => {
                self.zone_update_x = payload.g1();
                self.zone_update_z = payload.g1();
                self.ptype = -1;
            }

            ServerProt::UPDATE_ZONE_FULL_FOLLOWS => {
                self.zone_update_x = payload.g1();
                self.zone_update_z = payload.g1();

                // TS 7075-7096: null every groundObj cell in the 8x8 zone on
                // minusedlevel and expire loc changes inside it. The zone
                // origin comes from the packet, so cells outside 0..SIZE are
                // skipped (the TS array would grow; the Rust one bounds-checks).
                for x in self.zone_update_x..self.zone_update_x + 8 {
                    for z in self.zone_update_z..self.zone_update_z + 8 {
                        if (0..BuildArea::SIZE).contains(&x)
                            && (0..BuildArea::SIZE).contains(&z)
                            && self.ground_obj[self.minusedlevel as usize][x as usize][z as usize]
                                .take()
                                .is_some()
                        {
                            self.show_object(x, z);
                        }
                    }
                }

                let mut node = self.loc_changes.head();
                while let Some(loc) = node {
                    if loc.x >= self.zone_update_x
                        && loc.x < self.zone_update_x + 8
                        && loc.z >= self.zone_update_z
                        && loc.z < self.zone_update_z + 8
                        && loc.level == self.minusedlevel
                    {
                        loc.end_time = 0;
                    }
                    node = self.loc_changes.next_node();
                }
                self.ptype = -1;
            }

            ServerProt::UPDATE_ZONE_PARTIAL_ENCLOSED => {
                self.zone_update_x = payload.g1();
                self.zone_update_z = payload.g1();

                // TS loop bound is `in.pos < psize`. Over the socket `in` is
                // the 5000-byte alloc, so use psize when it is the frame size.
                let end = self.inbound_end(payload);
                while payload.pos < end {
                    let opcode = payload.g1();
                    self.zone_packet(payload, opcode);
                }
                self.ptype = -1;
            }

            // zone protocol, direct dispatch like the TS
            ServerProt::OBJ_COUNT
            | ServerProt::P_LOCMERGE
            | ServerProt::OBJ_REVEAL
            | ServerProt::MAP_ANIM
            | ServerProt::MAP_PROJANIM
            | ServerProt::OBJ_DEL
            | ServerProt::OBJ_ADD
            | ServerProt::LOC_ANIM
            | ServerProt::LOC_DEL
            | ServerProt::LOC_ADD_CHANGE => {
                self.zone_packet(payload, ptype);
                self.ptype = -1;
            }

            _ => {
                // Java/TS report unknown opcodes to the world and log out
                eprintln!(
                    "T1 - {ptype},{} - {},{}",
                    self.psize, self.ptype1, self.ptype2
                );
                self.logout();
            }
        }
    }

    /// `getPlayerPos(buf, size)` from client-ts: unpack the `PLAYER_INFO`
    /// frame into the local player and the `players` table.
    fn get_player_pos(&mut self, buf: &mut Packet, size: i32) {
        self.entity_removal_count = 0;
        self.entity_update_count = 0;

        self.get_player_pos_local(buf);
        self.get_player_pos_old_vis(buf);
        self.get_player_pos_new_vis(buf, size);
        self.get_player_pos_extended(buf);

        for i in 0..self.entity_removal_count as usize {
            let index = self.entity_removal_ids[i] as usize;
            if index < self.players.len() {
                if let Some(player) = &self.players[index] {
                    if player.cycle != self.loop_cycle {
                        self.players[index] = None;
                    }
                }
            }
        }

        if buf.pos != size as usize {
            // TS throws here; the tcpIn catch reports T2 and logs out
            eprintln!(
                "T2 - Error packet size mismatch in getplayer pos:{} psize:{}",
                buf.pos, size
            );
            self.logout();
            return;
        }

        for i in 0..self.player_count as usize {
            let index = self.player_ids[i] as usize;
            if self.players.get(index).is_none_or(|p| p.is_none()) {
                eprintln!(
                    "T2 - {} null entry in pl list - pos:{} size:{}",
                    self.login_user, i, self.player_count
                );
                self.logout();
                return;
            }
        }
    }

    /// `getPlayerPosLocal` from client-ts.
    fn get_player_pos_local(&mut self, buf: &mut Packet) {
        buf.gbit_start();

        let info = buf.gbit(1);
        if info != 0 {
            let op = buf.gbit(2);

            if op == 0 {
                self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                self.entity_update_count += 1;
            } else if op == 1 {
                let walk_dir = buf.gbit(3);
                if let Some(local) = self.local_player.as_mut() {
                    local.move_code(&self.cache, false, walk_dir);
                }

                let extended_info = buf.gbit(1);
                if extended_info == 1 {
                    self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                    self.entity_update_count += 1;
                }
            } else if op == 2 {
                let walk_dir = buf.gbit(3);
                if let Some(local) = self.local_player.as_mut() {
                    local.move_code(&self.cache, true, walk_dir);
                }

                let run_dir = buf.gbit(3);
                if let Some(local) = self.local_player.as_mut() {
                    local.move_code(&self.cache, true, run_dir);
                }

                let extended_info = buf.gbit(1);
                if extended_info == 1 {
                    self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                    self.entity_update_count += 1;
                }
            } else if op == 3 {
                self.minusedlevel = buf.gbit(2);
                let local_x = buf.gbit(7);
                let local_z = buf.gbit(7);
                let jump = buf.gbit(1);

                if let Some(local) = self.local_player.as_mut() {
                    local.teleport(&self.cache, jump == 1, local_x, local_z);
                }

                let extended_info = buf.gbit(1);
                if extended_info == 1 {
                    self.entity_update_ids[self.entity_update_count as usize] = LOCAL_PLAYER_INDEX;
                    self.entity_update_count += 1;
                }
            }
        }
    }

    /// `getPlayerPosOldVis` from client-ts.
    fn get_player_pos_old_vis(&mut self, buf: &mut Packet) {
        let count = buf.gbit(8);

        if count < self.player_count {
            for i in count as usize..self.player_count as usize {
                self.entity_removal_ids[self.entity_removal_count as usize] = self.player_ids[i];
                self.entity_removal_count += 1;
            }
        }

        if count > self.player_count {
            eprintln!("T2 - {} Too many players", self.login_user);
            self.logout();
            return;
        }

        self.player_count = 0;
        for i in 0..count as usize {
            let index = self.player_ids[i] as usize;

            let info = buf.gbit(1);
            if info == 0 {
                self.player_ids[self.player_count as usize] = index as i32;
                self.player_count += 1;
                if let Some(player) = self.players[index].as_mut() {
                    player.cycle = self.loop_cycle;
                }
            } else {
                let op = buf.gbit(2);

                if op == 0 {
                    self.player_ids[self.player_count as usize] = index as i32;
                    self.player_count += 1;
                    if let Some(player) = self.players[index].as_mut() {
                        player.cycle = self.loop_cycle;
                    }
                    self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                    self.entity_update_count += 1;
                } else if op == 1 {
                    self.player_ids[self.player_count as usize] = index as i32;
                    self.player_count += 1;
                    if let Some(player) = self.players[index].as_mut() {
                        player.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(player) = self.players[index].as_mut() {
                        player.move_code(&self.cache, false, walk_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else if op == 2 {
                    self.player_ids[self.player_count as usize] = index as i32;
                    self.player_count += 1;
                    if let Some(player) = self.players[index].as_mut() {
                        player.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(player) = self.players[index].as_mut() {
                        player.move_code(&self.cache, true, walk_dir);
                    }

                    let run_dir = buf.gbit(3);
                    if let Some(player) = self.players[index].as_mut() {
                        player.move_code(&self.cache, true, run_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else {
                    self.entity_removal_ids[self.entity_removal_count as usize] = index as i32;
                    self.entity_removal_count += 1;
                }
            }
        }
    }

    /// `getPlayerPosNewVis` from client-ts.
    fn get_player_pos_new_vis(&mut self, buf: &mut Packet, size: i32) {
        while buf.bit_pos + 10 < (size as usize) * 8 {
            let index = buf.gbit(11);
            if index == 2047 {
                break;
            }
            let index = index as usize;

            if self.players[index].is_none() {
                self.players[index] = Some(Box::new(ClientPlayer::default()));

                if let Some(appearance) = self.player_appearance_buffer[index].take() {
                    if let Some(player) = self.players[index].as_mut() {
                        let mut appearance = appearance;
                        appearance.pos = 0;
                        player.set_appearance(&mut appearance, &self.cache);
                        self.player_appearance_buffer[index] = Some(appearance);
                    }
                }
            }

            self.player_ids[self.player_count as usize] = index as i32;
            self.player_count += 1;
            if let Some(player) = self.players[index].as_mut() {
                player.cycle = self.loop_cycle;
            }

            let mut dx = buf.gbit(5);
            if dx > 15 {
                dx -= 32;
            }
            let mut dz = buf.gbit(5);
            if dz > 15 {
                dz -= 32;
            }
            let jump = buf.gbit(1);

            if let Some(local) = &self.local_player {
                if let Some(player) = self.players[index].as_mut() {
                    player.teleport(
                        &self.cache,
                        jump == 1,
                        local.route_x[0] + dx,
                        local.route_z[0] + dz,
                    );
                }
            }

            let extended_info = buf.gbit(1);
            if extended_info == 1 {
                self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                self.entity_update_count += 1;
            }
        }

        buf.gbit_end();
    }

    /// `getPlayerPosExtended` from client-ts.
    fn get_player_pos_extended(&mut self, buf: &mut Packet) {
        for i in 0..self.entity_update_count as usize {
            let index = self.entity_update_ids[i] as usize;
            if self.players[index].is_none() {
                continue;
            }

            let mut mask = buf.g1();
            if mask & player_update::BIG_UPDATE != 0 {
                mask = mask.wrapping_add(buf.g1() << 8);
            }

            self.get_player_pos_decode_extended(index, mask, buf);
        }
    }

    /// `getPlayerPosDecodeExtended` from client-ts.
    fn get_player_pos_decode_extended(&mut self, index: usize, mask: i32, buf: &mut Packet) {
        // Java's `localPlayer` is `players[LOCAL_PLAYER_INDEX]` itself, so
        // these mask writes already hit the drawn entity; the Rust login
        // keeps a separate clone, so they must target `local_player` here.
        // x/z/route are not copied — `players[2047]` stays the stale walk
        // slot and the live walk lives on `local_player`.
        let mut player = if index == LOCAL_PLAYER_INDEX as usize {
            self.local_player.as_mut()
        } else {
            self.players[index].as_deref_mut()
        };

        if mask & player_update::APPEARANCE != 0 {
            let length = buf.g1() as usize;

            let mut data = vec![0u8; length];
            buf.gdata(length, 0, &mut data);

            self.player_appearance_buffer[index] = Some(Packet::new(data));
            if let Some(player) = player.as_mut() {
                let mut appearance = self.player_appearance_buffer[index].take().unwrap();
                appearance.pos = 0;
                player.set_appearance(&mut appearance, &self.cache);
                self.player_appearance_buffer[index] = Some(appearance);
            }
        }

        if mask & player_update::ANIM != 0 {
            let mut seq_id = buf.g2();
            if seq_id == 65535 {
                seq_id = -1;
            }

            if let Some(player) = player.as_mut() {
                if seq_id == player.primary_anim {
                    player.primary_anim_loop = 0;
                }

                let delay = buf.g1();
                if player.primary_anim == seq_id && seq_id != -1 {
                    if (seq_id as usize) < self.cache.seqs.len() {
                        let restart_mode = self.cache.seq(seq_id as usize).duplicatebehaviour;
                        if restart_mode == RESTART_RESET {
                            player.primary_anim_frame = 0;
                            player.primary_anim_cycle = 0;
                            player.primary_anim_delay = delay;
                            player.primary_anim_loop = 0;
                        } else if restart_mode == RESTART_RESETLOOP {
                            player.primary_anim_loop = 0;
                        }
                    }
                } else if seq_id == -1
                    || player.primary_anim == -1
                    // unloaded seq table: TS would resolve the priority via
                    // SeqType.list; treat as "priority passes" to keep state
                    || (seq_id as usize) >= self.cache.seqs.len()
                    || (player.primary_anim as usize) >= self.cache.seqs.len()
                    || self.cache.seq(seq_id as usize).priority
                        >= self.cache.seq(player.primary_anim as usize).priority
                {
                    player.primary_anim = seq_id;
                    player.primary_anim_frame = 0;
                    player.primary_anim_cycle = 0;
                    player.primary_anim_delay = delay;
                    player.primary_anim_loop = 0;
                    player.preanim_route_length = player.route_length;
                }
            }
        }

        if mask & player_update::FACEENTITY != 0 {
            let mut face_entity = buf.g2();
            if face_entity == 65535 {
                face_entity = -1;
            }
            if let Some(player) = player.as_mut() {
                player.face_entity = face_entity;
            }
        }

        if mask & player_update::SAY != 0 {
            let message = buf.gjstr();
            if let Some(player) = player.as_mut() {
                player.chat_message = Some(message);
                player.chat_colour = 0;
                player.chat_effect = 0;
                player.chat_timer = 150;
                // TS also addChat(2, message, name) when the player has a
                // name; the chat display lands with the chat task.
            }
        }

        if mask & player_update::HITMARK != 0 {
            let damage = buf.g1();
            let damage_type = buf.g1();
            if let Some(player) = player.as_mut() {
                player.add_hitmark(self.loop_cycle, damage_type, damage);
                player.combat_cycle = self.loop_cycle + 400;
                player.health = buf.g1();
                player.total_health = buf.g1();
            }
        }

        if mask & player_update::FACESQUARE != 0 {
            let x = buf.g2();
            let z = buf.g2();
            if let Some(player) = player.as_mut() {
                player.face_square_x = x;
                player.face_square_z = z;
            }
        }

        if mask & player_update::CHAT != 0 {
            let colour_effect = buf.g2();
            let chat_type = buf.g1();
            let length = buf.g1();
            let start = buf.pos;

            // TS 7907-7941: unpack/filter the WordPacked chat and addChat
            // it. `add_chat` needs a whole-`self` borrow, so the shared
            // `player` borrow ends at the name/ready snapshot and is
            // re-acquired below for the bubble fields. The ignore list is
            // filled by the social slice; `ignore_count` stays 0 until then.
            let (name, ready) = match &player {
                Some(p) => (p.name.clone(), p.ready),
                None => (None, false),
            };
            let mut filtered = None;
            if let (Some(name), true) = (name, ready) {
                let username = JString::to_userhash(&name);
                let mut ignored = false;
                if chat_type <= 1 {
                    for i in 0..self.ignore_count {
                        if self.ignore_userhash[i as usize] == username as i64 {
                            ignored = true;
                            break;
                        }
                    }
                }
                if !ignored && self.chat_disabled == 0 {
                    let uncompressed = WordPack::unpack(buf, length as usize);
                    filtered = Some(WordFilter::filter(&uncompressed));
                    if chat_type == 2 || chat_type == 3 {
                        self.add_chat(1, filtered.as_deref().unwrap(), &format!("@cr2@{name}"));
                    } else if chat_type == 1 {
                        self.add_chat(1, filtered.as_deref().unwrap(), &format!("@cr1@{name}"));
                    } else {
                        self.add_chat(2, filtered.as_deref().unwrap(), &name);
                    }
                }
            }
            // Ignored/disabled chat still skips the payload (unpack stops
            // early past 100 chars, so re-align like TS).
            buf.pos = start + length as usize;

            // Re-acquire the player (the shared borrow ended at the
            // snapshot above) for the chat bubble fields, as TS 7928-7931.
            let player = if index == LOCAL_PLAYER_INDEX as usize {
                self.local_player.as_mut()
            } else {
                self.players[index].as_deref_mut()
            };
            if let Some(filtered) = filtered {
                if let Some(player) = player {
                    player.chat_message = Some(filtered.clone());
                    player.chat_colour = colour_effect >> 8;
                    player.chat_effect = colour_effect & 0xff;
                    player.chat_timer = 150;
                }
            }
        }

        // The shared `player` binding's last use was the CHAT snapshot
        // above (`add_chat` needed a whole-`self` borrow), so it is
        // re-acquired for the remaining masks.
        let mut player = if index == LOCAL_PLAYER_INDEX as usize {
            self.local_player.as_mut()
        } else {
            self.players[index].as_deref_mut()
        };

        if mask & player_update::SPOTANIM != 0 {
            let spotanim_id = buf.g2();
            let height_delay = buf.g4();
            if let Some(player) = player.as_mut() {
                player.spotanim_id = spotanim_id;
                player.spotanim_height = height_delay >> 16;
                player.spotanim_last_cycle = self.loop_cycle + (height_delay & 0xffff);
                player.spotanim_frame = 0;
                player.spotanim_cycle = 0;

                if player.spotanim_last_cycle > self.loop_cycle {
                    player.spotanim_frame = -1;
                }

                if spotanim_id == 65535 {
                    player.spotanim_id = -1;
                }
            }
        }

        if mask & player_update::EXACTMOVE != 0 {
            let exact_start_x = buf.g1();
            let exact_start_z = buf.g1();
            let exact_end_x = buf.g1();
            let exact_end_z = buf.g1();
            let exact_move_end = buf.g2() + self.loop_cycle;
            let exact_move_start = buf.g2() + self.loop_cycle;
            let exact_move_facing = buf.g1();
            if let Some(player) = player.as_mut() {
                player.exact_start_x = exact_start_x;
                player.exact_start_z = exact_start_z;
                player.exact_end_x = exact_end_x;
                player.exact_end_z = exact_end_z;
                player.exact_move_end = exact_move_end;
                player.exact_move_start = exact_move_start;
                player.exact_move_facing = exact_move_facing;

                player.abort_route();
            }
        }

        if mask & player_update::HITMARK2 != 0 {
            let damage = buf.g1();
            let damage_type = buf.g1();
            if let Some(player) = player.as_mut() {
                player.add_hitmark(self.loop_cycle, damage_type, damage);
                player.combat_cycle = self.loop_cycle + 400;
                player.health = buf.g1();
                player.total_health = buf.g1();
            }
        }
    }

    /// `getNpcPos(buf, size)` from client-ts: unpack the `NPC_INFO` frame
    /// into the `npc` table.
    fn get_npc_pos(&mut self, buf: &mut Packet, size: i32) {
        self.entity_removal_count = 0;
        self.entity_update_count = 0;

        self.get_npc_pos_old_vis(buf);
        self.get_npc_pos_new_vis(buf, size);
        self.get_npc_pos_extended(buf);

        for i in 0..self.entity_removal_count as usize {
            let index = self.entity_removal_ids[i] as usize;
            if index < self.npc.len() {
                if let Some(npc) = &self.npc[index] {
                    if npc.cycle != self.loop_cycle {
                        self.npc[index] = None;
                    }
                }
            }
        }

        if buf.pos != size as usize {
            // TS throws here; the tcpIn catch reports T2 and logs out
            eprintln!(
                "T2 - {} size mismatch in getnpcpos - pos:{} psize:{}",
                self.login_user, buf.pos, size
            );
            self.logout();
            return;
        }

        for i in 0..self.npc_count as usize {
            let index = self.npc_ids[i] as usize;
            if self.npc.get(index).is_none_or(|n| n.is_none()) {
                eprintln!(
                    "T2 - {} null entry in npc list - pos:{} size:{}",
                    self.login_user, i, self.npc_count
                );
                self.logout();
                return;
            }
        }
    }

    /// `getNpcPosOldVis` from client-ts.
    fn get_npc_pos_old_vis(&mut self, buf: &mut Packet) {
        buf.gbit_start();

        let count = buf.gbit(8);
        if count < self.npc_count {
            for i in count as usize..self.npc_count as usize {
                self.entity_removal_ids[self.entity_removal_count as usize] = self.npc_ids[i];
                self.entity_removal_count += 1;
            }
        }

        if count > self.npc_count {
            eprintln!("T2 - {} Too many npcs", self.login_user);
            self.logout();
            return;
        }

        self.npc_count = 0;
        for i in 0..count as usize {
            let index = self.npc_ids[i] as usize;

            let info = buf.gbit(1);
            if info == 0 {
                self.npc_ids[self.npc_count as usize] = index as i32;
                self.npc_count += 1;
                if let Some(npc) = self.npc[index].as_mut() {
                    npc.cycle = self.loop_cycle;
                }
            } else {
                let op = buf.gbit(2);

                if op == 0 {
                    self.npc_ids[self.npc_count as usize] = index as i32;
                    self.npc_count += 1;
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.cycle = self.loop_cycle;
                    }
                    self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                    self.entity_update_count += 1;
                } else if op == 1 {
                    self.npc_ids[self.npc_count as usize] = index as i32;
                    self.npc_count += 1;
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.move_code(&self.cache, false, walk_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else if op == 2 {
                    self.npc_ids[self.npc_count as usize] = index as i32;
                    self.npc_count += 1;
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.cycle = self.loop_cycle;
                    }

                    let walk_dir = buf.gbit(3);
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.move_code(&self.cache, true, walk_dir);
                    }

                    let run_dir = buf.gbit(3);
                    if let Some(npc) = self.npc[index].as_mut() {
                        npc.move_code(&self.cache, true, run_dir);
                    }

                    let extended_info = buf.gbit(1);
                    if extended_info == 1 {
                        self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                        self.entity_update_count += 1;
                    }
                } else {
                    self.entity_removal_ids[self.entity_removal_count as usize] = index as i32;
                    self.entity_removal_count += 1;
                }
            }
        }
    }

    /// `getNpcPosNewVis` from client-ts.
    fn get_npc_pos_new_vis(&mut self, buf: &mut Packet, size: i32) {
        while buf.bit_pos + 21 < (size as usize) * 8 {
            let index = buf.gbit(14);
            if index == 16383 {
                break;
            }
            let index = index as usize;

            if self.npc[index].is_none() {
                self.npc[index] = Some(Box::new(ClientNpc::default()));
            }

            let npc = self.npc[index].as_mut().unwrap();
            self.npc_ids[self.npc_count as usize] = index as i32;
            self.npc_count += 1;
            npc.cycle = self.loop_cycle;

            let type_id = buf.gbit(11);
            if type_id >= 0 && (type_id as usize) < self.cache.npcs.len() {
                let r#type = self.cache.npc(type_id as usize);
                npc.r#type = Some(type_id as usize);
                npc.size = r#type.size;
                npc.turnspeed = r#type.turnspeed;
                npc.walkanim = r#type.walkanim;
                npc.walkanim_b = r#type.walkanim_b;
                // [sic] TS swaps left/right here
                npc.walkanim_l = r#type.walkanim_r;
                npc.walkanim_r = r#type.walkanim_l;
                npc.readyanim = r#type.readyanim;
            } else {
                npc.r#type = None;
            }

            let mut dx = buf.gbit(5);
            if dx > 15 {
                dx -= 32;
            }
            let mut dz = buf.gbit(5);
            if dz > 15 {
                dz -= 32;
            }
            let jump = buf.gbit(1);
            if let Some(local) = &self.local_player {
                npc.teleport(
                    &self.cache,
                    jump == 1,
                    local.route_x[0] + dx,
                    local.route_z[0] + dz,
                );
            }

            let extended_info = buf.gbit(1);
            if extended_info == 1 {
                self.entity_update_ids[self.entity_update_count as usize] = index as i32;
                self.entity_update_count += 1;
            }
        }

        buf.gbit_end();
    }

    /// `getNpcPosExtended` from client-ts.
    fn get_npc_pos_extended(&mut self, buf: &mut Packet) {
        for i in 0..self.entity_update_count as usize {
            let id = self.entity_update_ids[i] as usize;
            if self.npc[id].is_none() {
                continue;
            }

            let mask = buf.g1();

            if mask & npc_update::HITMARK2 != 0 {
                let damage = buf.g1();
                let damage_type = buf.g1();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.add_hitmark(self.loop_cycle, damage_type, damage);
                    npc.combat_cycle = self.loop_cycle + 400;
                    npc.health = buf.g1();
                    npc.total_health = buf.g1();
                }
            }

            if mask & npc_update::ANIM != 0 {
                let mut anim = buf.g2();
                if anim == 65535 {
                    anim = -1;
                }

                if let Some(npc) = self.npc[id].as_mut() {
                    if anim == npc.primary_anim {
                        npc.primary_anim_loop = 0;
                    }

                    let delay = buf.g1();
                    if npc.primary_anim == anim && anim != -1 {
                        if (anim as usize) < self.cache.seqs.len() {
                            let restart_mode = self.cache.seq(anim as usize).duplicatebehaviour;
                            if restart_mode == RESTART_RESET {
                                npc.primary_anim_frame = 0;
                                npc.primary_anim_cycle = 0;
                                npc.primary_anim_delay = delay;
                                npc.primary_anim_loop = 0;
                            } else if restart_mode == RESTART_RESETLOOP {
                                npc.primary_anim_loop = 0;
                            }
                        }
                    } else if anim == -1
                        || npc.primary_anim == -1
                        // unloaded seq table: treat as "priority passes"
                        || (anim as usize) >= self.cache.seqs.len()
                        || (npc.primary_anim as usize) >= self.cache.seqs.len()
                        || self.cache.seq(anim as usize).priority
                            >= self.cache.seq(npc.primary_anim as usize).priority
                    {
                        npc.primary_anim = anim;
                        npc.primary_anim_frame = 0;
                        npc.primary_anim_cycle = 0;
                        npc.primary_anim_delay = delay;
                        npc.primary_anim_loop = 0;
                        npc.preanim_route_length = npc.route_length;
                    }
                }
            }

            if mask & npc_update::FACEENTITY != 0 {
                let mut face_entity = buf.g2();
                if face_entity == 65535 {
                    face_entity = -1;
                }
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.face_entity = face_entity;
                }
            }

            if mask & npc_update::SAY != 0 {
                let message = buf.gjstr();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.chat_message = Some(message);
                    npc.chat_timer = 100;
                }
            }

            if mask & npc_update::HITMARK != 0 {
                let damage = buf.g1();
                let damage_type = buf.g1();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.add_hitmark(self.loop_cycle, damage_type, damage);
                    npc.combat_cycle = self.loop_cycle + 400;
                    npc.health = buf.g1();
                    npc.total_health = buf.g1();
                }
            }

            if mask & npc_update::CHANGETYPE != 0 {
                let type_id = buf.g2();
                if let Some(npc) = self.npc[id].as_mut() {
                    if type_id >= 0 && (type_id as usize) < self.cache.npcs.len() {
                        let r#type = self.cache.npc(type_id as usize);
                        npc.r#type = Some(type_id as usize);
                        npc.size = r#type.size;
                        npc.turnspeed = r#type.turnspeed;
                        npc.walkanim = r#type.walkanim;
                        npc.walkanim_b = r#type.walkanim_b;
                        // [sic] TS swaps left/right here
                        npc.walkanim_l = r#type.walkanim_r;
                        npc.walkanim_r = r#type.walkanim_l;
                        npc.readyanim = r#type.readyanim;
                    } else {
                        npc.r#type = None;
                    }
                }
            }

            if mask & npc_update::SPOTANIM != 0 {
                let spotanim_id = buf.g2();
                let info = buf.g4();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.spotanim_id = spotanim_id;
                    npc.spotanim_height = info >> 16;
                    npc.spotanim_last_cycle = self.loop_cycle + (info & 0xffff);
                    npc.spotanim_frame = 0;
                    npc.spotanim_cycle = 0;

                    if npc.spotanim_last_cycle > self.loop_cycle {
                        npc.spotanim_frame = -1;
                    }

                    if spotanim_id == 65535 {
                        npc.spotanim_id = -1;
                    }
                }
            }

            if mask & npc_update::FACESQUARE != 0 {
                let x = buf.g2();
                let z = buf.g2();
                if let Some(npc) = self.npc[id].as_mut() {
                    npc.face_square_x = x;
                    npc.face_square_z = z;
                }
            }
        }
    }

    /// `zonePacket(buf, opcode)` from client-ts: reads the 8-tile zone
    /// position byte and the TS field widths for each opcode, then applies
    /// the change — loc adds/dels/animations, ground-object adds/dels/
    /// reveals, projectiles, map spot anims, and loc merges — with the
    /// same bounds gates as the TS (tile ops inside `0..SIZE`; loc ops
    /// also gated by the `LOC_SHAPE_TO_LAYER` table length).
    fn zone_packet(&mut self, buf: &mut Packet, opcode: i32) {
        let pos = buf.g1();
        let x = self.zone_update_x + ((pos >> 4) & 0x7);
        let z = self.zone_update_z + (pos & 0x7);

        match opcode {
            ServerProt::LOC_ADD_CHANGE => {
                let info = buf.g1();
                let id = buf.g2();

                let shape = info >> 2;
                let rotate = info & 0x3;

                // TS yields `undefined` for shape >= table length; skip the
                // apply instead of panicking on the index.
                if shape < LOC_SHAPE_TO_LAYER.len() as i32
                    && x >= 0
                    && z >= 0
                    && x < BuildArea::SIZE
                    && z < BuildArea::SIZE
                {
                    let layer = LOC_SHAPE_TO_LAYER[shape as usize];
                    self.loc_change_create(
                        self.minusedlevel,
                        x,
                        z,
                        layer,
                        id,
                        shape,
                        rotate,
                        0,
                        -1,
                    );
                }
            }
            ServerProt::LOC_DEL => {
                let info = buf.g1();

                let shape = info >> 2;
                let rotate = info & 0x3;

                // TS yields `undefined` for shape >= table length; skip the
                // apply instead of panicking on the index.
                if shape < LOC_SHAPE_TO_LAYER.len() as i32
                    && x >= 0
                    && z >= 0
                    && x < BuildArea::SIZE
                    && z < BuildArea::SIZE
                {
                    let layer = LOC_SHAPE_TO_LAYER[shape as usize];
                    self.loc_change_create(
                        self.minusedlevel,
                        x,
                        z,
                        layer,
                        -1,
                        shape,
                        rotate,
                        0,
                        -1,
                    );
                }
            }
            ServerProt::LOC_ANIM => {
                let info = buf.g1();
                let seq = buf.g2();

                let shape = info >> 2;
                let rotate = info & 0x3;

                // TS yields `undefined` for shape >= table length; skip the
                // apply instead of panicking on the index. The +1 height
                // reads need x+1/z+1 inside groundh (sized SIZE+1), so the
                // same 0..SIZE bounds guard covers them.
                if shape < LOC_SHAPE_TO_LAYER.len() as i32
                    && x >= 0
                    && z >= 0
                    && x < BuildArea::SIZE
                    && z < BuildArea::SIZE
                {
                    let layer = LOC_SHAPE_TO_LAYER[shape as usize];
                    let level = self.minusedlevel as usize;
                    let height_sw = self.groundh[level][x as usize][z as usize];
                    let height_se = self.groundh[level][x as usize + 1][z as usize];
                    let height_ne = self.groundh[level][x as usize + 1][z as usize + 1];
                    let height_nw = self.groundh[level][x as usize][z as usize + 1];

                    // Task 3b: the ClientLocAnim itself is materialised by
                    // the render side on its next draw of the tile; the sim
                    // records the anim seq/shape/angle and the packet-time
                    // heights on the tile (and bumps the model stamp so the
                    // renderer's lazy cache re-resolves).
                    match layer {
                        LocLayer::WALL => {
                            if let Some(wall) = self.world.get_wall_mut(self.minusedlevel, x, z) {
                                wall.anim_seq = seq;
                                wall.anim_shape = shape;
                                wall.anim_angle = rotate;
                                wall.h_sw = height_sw;
                                wall.h_se = height_se;
                                wall.h_ne = height_ne;
                                wall.h_nw = height_nw;
                                self.world.bump_tile_stamp(self.minusedlevel, x, z);
                            }
                        }
                        LocLayer::WALL_DECOR => {
                            // `getDecor(level, z, x)` in the TS swaps its
                            // parameter names; it indexes by tile x,z.
                            if let Some(decor) = self.world.get_decor_mut(self.minusedlevel, x, z) {
                                decor.anim_seq = seq;
                                decor.anim_shape = 4;
                                decor.anim_angle = 0;
                                // [sic] TS passes heightNE in the SE slot.
                                decor.h_sw = height_sw;
                                decor.h_se = height_ne;
                                decor.h_ne = height_ne;
                                decor.h_nw = height_nw;
                                self.world.bump_tile_stamp(self.minusedlevel, x, z);
                            }
                        }
                        LocLayer::GROUND => {
                            let shape = if shape == 11 { 10 } else { shape };
                            if let Some(sprite) = self.world.get_scene_mut(self.minusedlevel, x, z)
                            {
                                sprite.anim_seq = seq;
                                sprite.anim_shape = shape;
                                sprite.anim_angle = rotate;
                                sprite.h_sw = height_sw;
                                sprite.h_se = height_se;
                                sprite.h_ne = height_ne;
                                sprite.h_nw = height_nw;
                                sprite.model_stamp = sprite.model_stamp.wrapping_add(1);
                            }
                        }
                        LocLayer::GROUND_DECOR => {
                            if let Some(decor) = self.world.get_gd_mut(self.minusedlevel, x, z) {
                                decor.anim_seq = seq;
                                decor.anim_shape = LocShape::GROUND_DECOR;
                                decor.anim_angle = rotate;
                                decor.h_sw = height_sw;
                                decor.h_se = height_se;
                                decor.h_ne = height_ne;
                                decor.h_nw = height_nw;
                                self.world.bump_tile_stamp(self.minusedlevel, x, z);
                            }
                        }
                        _ => {}
                    }
                }
            }
            ServerProt::OBJ_ADD => {
                let obj_type = buf.g2();
                let count = buf.g2();

                if x >= 0 && z >= 0 && x < BuildArea::SIZE && z < BuildArea::SIZE {
                    let level = self.minusedlevel as usize;
                    {
                        let objs = self.ground_obj[level][x as usize][z as usize]
                            .get_or_insert_with(|| Box::new(LinkList::new()));
                        objs.push(ClientObj::new(obj_type, count));
                    }
                    self.show_object(x, z);
                }
            }
            ServerProt::OBJ_DEL => {
                let obj_type = buf.g2();

                if x >= 0 && z >= 0 && x < BuildArea::SIZE && z < BuildArea::SIZE {
                    let level = self.minusedlevel as usize;
                    // `None` means the cell held no list; TS keeps
                    // `showObject` inside the `if (objs)` block.
                    let emptied = {
                        let objs = self.ground_obj[level][x as usize][z as usize].as_mut();
                        if let Some(objs) = objs {
                            let mut node = objs.head();
                            while let Some(o) = node {
                                if o.id == (obj_type & 0x7fff) {
                                    objs.unlink_last();
                                    break;
                                }
                                node = objs.next_node();
                            }
                            Some(objs.head().is_none())
                        } else {
                            None
                        }
                    };
                    if let Some(emptied) = emptied {
                        if emptied {
                            self.ground_obj[level][x as usize][z as usize] = None;
                        }
                        self.show_object(x, z);
                    }
                }
            }
            ServerProt::MAP_PROJANIM => {
                let x2 = x + buf.g1b();
                let z2 = z + buf.g1b();
                let target = buf.g2b();
                let spotanim = buf.g2();
                // TS 7265-7266: `h1`/`h2` are `g1() * 4`.
                let h1 = buf.g1() * 4;
                let h2 = buf.g1() * 4;
                let t1 = buf.g2();
                let t2 = buf.g2();
                let angle = buf.g1();
                let startpos = buf.g1();

                // TS 7271-7279: src and dest tiles must both be in
                // `0..104`; heights are `getAvH(scene) - h` with `t1`/`t2`
                // shifted by `loop_cycle`.
                if x >= 0
                    && z >= 0
                    && x < BuildArea::SIZE
                    && z < BuildArea::SIZE
                    && x2 >= 0
                    && z2 >= 0
                    && x2 < BuildArea::SIZE
                    && z2 < BuildArea::SIZE
                {
                    let x = x * 128 + 64;
                    let z = z * 128 + 64;
                    let x2 = x2 * 128 + 64;
                    let z2 = z2 * 128 + 64;

                    let mut proj = ClientProj::new(
                        spotanim,
                        self.minusedlevel,
                        x,
                        get_av_h(&self.groundh, &self.mapl, x, z, self.minusedlevel) - h1,
                        z,
                        t1 + self.loop_cycle,
                        t2 + self.loop_cycle,
                        angle,
                        startpos,
                        target,
                        h2,
                    );
                    proj.set_target(
                        x2 as f64,
                        (get_av_h(&self.groundh, &self.mapl, x2, z2, self.minusedlevel) - h2)
                            as f64,
                        z2 as f64,
                        t1 + self.loop_cycle,
                    );
                    proj.bind_seq(&self.cache);
                    self.projectiles.push(proj);
                }
            }
            ServerProt::OBJ_REVEAL => {
                let id = buf.g2();
                let count = buf.g2();
                let pid = buf.g2();

                if x >= 0
                    && z >= 0
                    && x < BuildArea::SIZE
                    && z < BuildArea::SIZE
                    && pid != self.self_slot
                {
                    let level = self.minusedlevel as usize;
                    {
                        let objs = self.ground_obj[level][x as usize][z as usize]
                            .get_or_insert_with(|| Box::new(LinkList::new()));
                        objs.push(ClientObj::new(id, count));
                    }
                    self.show_object(x, z);
                }
            }
            ServerProt::MAP_ANIM => {
                let spotanim = buf.g2();
                // TS 7284: packet `height` is a plain `g1()`, not ×4 like
                // the MAP_PROJANIM heights.
                let height = buf.g1();
                let time = buf.g2();

                if x >= 0 && z >= 0 && x < BuildArea::SIZE && z < BuildArea::SIZE {
                    let x = x * 128 + 64;
                    let z = z * 128 + 64;
                    self.spotanims.push(MapSpotAnim::new(
                        spotanim,
                        self.minusedlevel,
                        x,
                        z,
                        get_av_h(&self.groundh, &self.mapl, x, z, self.minusedlevel) - height,
                        self.loop_cycle,
                        time,
                    ));
                }
            }
            ServerProt::P_LOCMERGE => {
                let info = buf.g1();
                let shape = info >> 2;
                let rotate = info & 0x3;

                let id = buf.g2();
                let t1 = buf.g2();
                let t2 = buf.g2();
                let pid = buf.g2();
                let mut east = buf.g1b();
                let mut south = buf.g1b();
                let mut west = buf.g1b();
                let mut north = buf.g1b();

                // TS yields `undefined` for shape >= table length; skip the
                // apply instead of panicking on the index. The +1 height
                // reads need x+1/z+1 inside groundh (sized SIZE+1), so the
                // same 0..SIZE bounds guard covers them.
                if shape < LOC_SHAPE_TO_LAYER.len() as i32
                    && x >= 0
                    && z >= 0
                    && x < BuildArea::SIZE
                    && z < BuildArea::SIZE
                {
                    let layer = LOC_SHAPE_TO_LAYER[shape as usize];

                    // TS 7328-7331: the player is resolved before any apply;
                    // a pid that resolves to no player stops the whole arm
                    // (bytes already consumed) — `cache.loc(id)` and
                    // `loc_change_create` never run for missing players.
                    let has_player = if pid == self.self_slot {
                        self.local_player.is_some()
                    } else {
                        self.players
                            .get(pid as usize)
                            .and_then(|p| p.as_ref())
                            .is_some()
                    };
                    if has_player {
                        let level = self.minusedlevel as usize;
                        let height_sw = self.groundh[level][x as usize][z as usize];
                        let height_se = self.groundh[level][x as usize + 1][z as usize];
                        let height_ne = self.groundh[level][x as usize + 1][z as usize + 1];
                        let height_nw = self.groundh[level][x as usize][z as usize + 1];

                        // TS 7333-7336: `loc.getModel(..., -1)`; when the
                        // model is not ready (None) the apply stops — no
                        // locChange and no player loc_* writes.
                        if let Some(model) = self.cache.loc(id as usize).get_model(
                            &self.cache,
                            shape,
                            rotate,
                            height_sw,
                            height_se,
                            height_ne,
                            height_nw,
                            -1,
                        ) {
                            self.loc_change_create(
                                self.minusedlevel,
                                x,
                                z,
                                layer,
                                -1,
                                0,
                                0,
                                t1 + 1,
                                t2 + 1,
                            );

                            let loc = self.cache.loc(id as usize);
                            let mut width = loc.width;
                            let mut length = loc.length;
                            if rotate == LocAngle::NORTH || rotate == LocAngle::SOUTH {
                                width = loc.length;
                                length = loc.width;
                            }

                            // The gate above guarantees the player still
                            // resolves here (nothing between the two moved
                            // `local_player`/`players`).
                            let player = if pid == self.self_slot {
                                self.local_player.as_mut()
                            } else {
                                self.players
                                    .get_mut(pid as usize)
                                    .and_then(|p| p.as_deref_mut())
                            };
                            if let Some(player) = player {
                                player.loc_start_cycle = t1 + self.loop_cycle;
                                player.loc_stop_cycle = t2 + self.loop_cycle;
                                player.loc_model = Some(Box::new(model));

                                player.loc_offset_x = x * 128 + width * 64;
                                player.loc_offset_z = z * 128 + length * 64;
                                player.loc_offset_y = get_av_h(
                                    &self.groundh,
                                    &self.mapl,
                                    player.loc_offset_x,
                                    player.loc_offset_z,
                                    self.minusedlevel,
                                );

                                if east > west {
                                    std::mem::swap(&mut east, &mut west);
                                }
                                if south > north {
                                    std::mem::swap(&mut south, &mut north);
                                }

                                player.min_tile_x = x + east;
                                player.max_tile_x = x + west;
                                player.min_tile_z = z + south;
                                player.max_tile_z = z + north;
                            }
                        }
                    }
                }
            }
            ServerProt::OBJ_COUNT => {
                let obj_type = buf.g2();
                let ocount = buf.g2();
                let count = buf.g2();

                if x >= 0 && z >= 0 && x < BuildArea::SIZE && z < BuildArea::SIZE {
                    let level = self.minusedlevel as usize;
                    let had_list = {
                        let objs = self.ground_obj[level][x as usize][z as usize].as_mut();
                        if let Some(objs) = objs {
                            let mut node = objs.head();
                            while let Some(o) = node {
                                if o.id == (obj_type & 0x7fff) && o.count == ocount {
                                    o.count = count;
                                    break;
                                }
                                node = objs.next_node();
                            }
                            true
                        } else {
                            false
                        }
                    };
                    if had_list {
                        self.show_object(x, z);
                    }
                }
            }
            _ => {}
        }
    }

    /// `doScrollbar` from client-ts (10291-10329): the up/down arrows step
    /// `scroll_pos` by `scroll_cycle*4`, and a press in the track/grip
    /// jumps to the grip position (grabbing it widens the track hit area to
    /// 32 px next call). The target is `ifaces[com_id]`, or
    /// `chat_interface` for a negative `com_id` (the chat scrollbar is a
    /// synthetic interface, TS `chatInterface`, not in the jag).
    ///
    /// Stays on `Client` (task 2b): the sim's menu walk
    /// (`add_component_options`) applies the scrollbar input each frame;
    /// the renderer's `draw_side` calls it for the chat scrollbar.
    #[allow(clippy::too_many_arguments)]
    pub fn do_scrollbar(
        &mut self,
        x: i32,
        y: i32,
        scrollable_height: i32,
        height: i32,
        redraw: bool,
        left: i32,
        top: i32,
        com_id: i32,
    ) {
        if self.scroll_grabbed {
            self.scroll_input_padding = 32;
        } else {
            self.scroll_input_padding = 0;
        }
        self.scroll_grabbed = false;

        let com = if com_id < 0 {
            Some(&mut self.chat_interface)
        } else {
            Arc::make_mut(&mut self.ifaces_mut)
                .get_mut(com_id as usize)
                .and_then(|o| o.as_mut())
                .map(Arc::make_mut)
        };
        let Some(com) = com else {
            return;
        };

        if x >= left && x < left + 16 && y >= top && y < top + 16 {
            com.scroll_pos -= self.scroll_cycle * 4;
            if redraw {
                self.redraw_side = true;
            }
        } else if x >= left && x < left + 16 && y >= top + height - 16 && y < top + height {
            com.scroll_pos += self.scroll_cycle * 4;
            if redraw {
                self.redraw_side = true;
            }
        } else if x >= left - self.scroll_input_padding
            && x < left + self.scroll_input_padding + 16
            && y >= top + 16
            && y < top + height - 16
            && self.scroll_cycle > 0
        {
            let mut grip_size = ((height - 32) * height) / scrollable_height;
            if grip_size < 8 {
                grip_size = 8;
            }
            let grip_y = y - top - (grip_size / 2) - 16;
            let max_y = height - grip_size - 32;
            com.scroll_pos = ((scrollable_height - height) * grip_y) / max_y;
            if redraw {
                self.redraw_side = true;
            }
            self.scroll_grabbed = true;
        }
    }

    /// `logout()` from client-ts: close the stream and return to the login
    /// screen. Java also stops the midi and clears the music state, and
    /// drops back to the welcome screen (`loginscreen = 0`). The title
    /// rebuild mirrors Java `prepareGame`+`prepareTitle`: `redraw_frame`
    /// forces the full recomposite, and the renderer's `title_screen_draw`
    /// performs the paint teardown (`unload_title`, a nulled `image_title2`
    /// so the next `prepare_title` reallocates the 9 regions, and a
    /// one-shot `draw_area` cls so no game-frame viewport/chat/side pixel
    /// survives).
    pub fn logout(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.close();
        }
        // The engine drops the update connection on logout, so the OnDemand
        // worker must drop its stream too: a dead socket would otherwise
        // stall the next login's downloads on the 750-cycle timeout and the
        // 4 s reopen gate. The worker itself stays alive (Java `unload`
        // stops OnDemand, not `logout`).
        if let Some(od) = self.on_demand.as_ref() {
            od.drop_socket();
        }
        self.ingame = false;
        self.loginscreen = 0;
        self.login_user.clear();
        self.login_pass.clear();
        // TS logout (Client.ts 2001-2013): drop the tutorial chat overlay
        // and flash, or a later login still draws TUT_OPEN from the
        // previous session (tabs unlocked, island modal still up).
        self.tut_com_id = -1;
        self.tut_flash_icon = -1;
        self.chat_modal_id = -1;
        self.main_modal_id = -1;
        self.side_modal_id = -1;
        self.world.reset_map();
        self.projectiles.clear();
        self.spotanims.clear();
        self.loc_changes = LinkList::new();
        self.friend_server_status = 0;
        self.friend_count = 0;
        self.social_input_open = false;
        for level in 0..BuildArea::LEVELS {
            for x in 0..BuildArea::SIZE {
                for z in 0..BuildArea::SIZE {
                    self.ground_obj[level as usize][x as usize][z as usize] = None;
                }
            }
        }
        for collision in &mut self.collision {
            collision.reset();
        }
        self.stop_midi();
        self.next_midi_song = -1;
        self.midi_song = -1;
        self.next_music_delay = 0;
        self.redraw_frame = true;
        // Spec: REBUILD/logout → all. T1, tcp_in T2, in-band PLAYER/NPC T2,
        // and lost_con all wipe through here; the host snapshot follows gens.
        self.bump_all_gens();
    }

    /// Whether the server has gone silent past the wall-clock
    /// [`SERVER_TIMEOUT`] bound since the last full packet / login grant.
    fn dead_server(&self) -> bool {
        self.last_response
            .map(|t| t.elapsed() > SERVER_TIMEOUT)
            .unwrap_or(false)
    }

    /// `lostCon` from Java (`Client.java` 6147): in-game connection loss. A
    /// pending logout request (`logoutTimer > 0`) logs out immediately;
    /// otherwise drop to the title state and re-establish with
    /// `login(loginUser, loginPass, true)` (wrapper opcode 18). A failed
    /// reestablish logs out, as Java. The old stream is replaced by `login`
    /// on success or closed by `logout` on failure, matching Java's
    /// save-and-close of the old `ClientStream`. The "Connection lost"
    /// viewport text is not drawn (headless).
    pub fn lost_con(&mut self) {
        if self.logout_timer > 0 {
            self.logout();
            return;
        }
        self.ingame = false;
        let user = self.login_user.clone();
        let pass = self.login_pass.clone();
        let _ = self.login(&user, &pass, true);
        if !self.ingame {
            self.logout();
        }
    }

    /// `mainloop` from Java (`Client.java` 1823): one 20 ms pass. An
    /// `errorLoading` flag (missing required jag / failed map) returns
    /// immediately like TS. In-game runs `gameLoop`; on the title screen
    /// `titleScreenLoop`; always drain OnDemand completions via
    /// `onDemandLoop` (not the bare worker heartbeat).
    pub fn mainloop(&mut self) {
        if self.error_loading {
            return;
        }
        self.loop_cycle = self.loop_cycle.wrapping_add(1);
        if self.ingame {
            self.game_loop();
        } else {
            self.title_screen_loop();
        }
        self.on_demand_loop();
        self.music_tick();
    }

    /// `titleScreenLoop` from client-ts (1378): the title-screen input pass,
    /// 1:1 port of the click regions, field selection, and the CHARSET
    /// filtered key entry. Clicks arrive latched on `shell.mouse_click_*`
    /// (GameShell.run 186-190); keys via `shell.poll_key`. A Login click
    /// runs the full handshake (blocking) and returns once `ingame`; on
    /// failure `login_mes1/2` carry the error to the title draw, as TS.
    /// Coordinates use the 765×503 applet (`sWid`/`sHei`).
    pub fn title_screen_loop(&mut self) {
        if self.loginscreen == 0 {
            let mut x = (APPLET_W / 2) - 80;
            let mut y = (APPLET_H / 2) + 20;

            y += 20;
            if title_button_clicked(
                self.shell.mouse_click_button,
                self.shell.mouse_click_x,
                self.shell.mouse_click_y,
                x,
                y,
            ) {
                self.loginscreen = 3;
                self.login_select = 0;
            }

            x = (APPLET_W / 2) + 80;
            if title_button_clicked(
                self.shell.mouse_click_button,
                self.shell.mouse_click_x,
                self.shell.mouse_click_y,
                x,
                y,
            ) {
                self.login_mes1.clear();
                self.login_mes2 = "Enter your username & password.".into();
                self.loginscreen = 2;
                self.login_select = 0;
            }
        } else if self.loginscreen == 2 {
            let mut y = (APPLET_H / 2) - 40;
            y += 30;

            y += 25;
            if self.shell.mouse_click_button == 1
                && self.shell.mouse_click_y >= y - 15
                && self.shell.mouse_click_y < y
            {
                self.login_select = 0;
            }

            y += 15;
            if self.shell.mouse_click_button == 1
                && self.shell.mouse_click_y >= y - 15
                && self.shell.mouse_click_y < y
            {
                self.login_select = 1;
            }
            // y += 15; dead code

            let mut x = (APPLET_W / 2) - 80;
            y = (APPLET_H / 2) + 50;
            y += 20;

            if title_button_clicked(
                self.shell.mouse_click_button,
                self.shell.mouse_click_x,
                self.shell.mouse_click_y,
                x,
                y,
            ) {
                let user = self.login_user.clone();
                let pass = self.login_pass.clone();
                let _ = self.login(&user, &pass, false);
                if self.ingame {
                    return;
                }
            }

            x = (APPLET_W / 2) + 80;
            if title_button_clicked(
                self.shell.mouse_click_button,
                self.shell.mouse_click_x,
                self.shell.mouse_click_y,
                x,
                y,
            ) {
                self.loginscreen = 0;
                self.login_user.clear();
                self.login_pass.clear();
            }

            loop {
                let key = self.shell.poll_key();
                if key == -1 {
                    break;
                }
                let valid = char::from_u32(key as u32).is_some_and(|c| TITLE_CHARSET.contains(c));

                if self.login_select == 0 {
                    if key == 8 && !self.login_user.is_empty() {
                        self.login_user.pop();
                    }

                    if key == 9 || key == 10 || key == 13 {
                        self.login_select = 1;
                    }

                    if valid {
                        self.login_user.push(char::from_u32(key as u32).unwrap());
                    }

                    if self.login_user.len() > 12 {
                        self.login_user.truncate(12);
                    }
                } else if self.login_select == 1 {
                    if key == 8 && !self.login_pass.is_empty() {
                        self.login_pass.pop();
                    }

                    if key == 9 || key == 10 || key == 13 {
                        self.login_select = 0;
                    }

                    if valid {
                        self.login_pass.push(char::from_u32(key as u32).unwrap());
                    }

                    if self.login_pass.len() > 20 {
                        self.login_pass.truncate(20);
                    }
                }
            }
        } else if self.loginscreen == 3 {
            let x = APPLET_W / 2;
            let mut y = (APPLET_H / 2) + 50;

            y += 20;
            if title_button_clicked(
                self.shell.mouse_click_button,
                self.shell.mouse_click_x,
                self.shell.mouse_click_y,
                x,
                y,
            ) {
                self.loginscreen = 0;
            }
        }
    }

    /// `chatModeLoop` (Java `Client.java` 2755-2800), verbatim: a left
    /// click in the Public (6..106), Private (135..235) or Trade/duel
    /// (273..373) button strip at y 467..499 cycles the mode, re-runs the
    /// mode labels (`redrawPrivacySettings`/`redrawChatback`) and sends
    /// `CHAT_SETMODE` with the three modes; the Report abuse button
    /// (412..512) closes the modals and records `main_modal_id` from the
    /// first interface with client code 600. `reportAbuseInput`,
    /// `reportAbuseMuteOption` and `reportAbuseComId` are not ported.
    pub fn chat_mode_loop(&mut self) {
        if self.shell.mouse_click_button != 1 {
            return;
        }
        if self.shell.mouse_click_x >= 6
            && self.shell.mouse_click_x <= 106
            && self.shell.mouse_click_y >= 467
            && self.shell.mouse_click_y <= 499
        {
            self.chat_public_mode = (self.chat_public_mode + 1) % 4;
            self.redraw_chat_mode = true;
            self.redraw_chat = true;
            self.out.p1_enc(ClientProt::CHAT_SETMODE.id);
            self.out.p1(self.chat_public_mode);
            self.out.p1(self.chat_private_mode);
            self.out.p1(self.chat_trade_mode);
        }
        if self.shell.mouse_click_x >= 135
            && self.shell.mouse_click_x <= 235
            && self.shell.mouse_click_y >= 467
            && self.shell.mouse_click_y <= 499
        {
            self.chat_private_mode = (self.chat_private_mode + 1) % 3;
            self.redraw_chat_mode = true;
            self.redraw_chat = true;
            self.out.p1_enc(ClientProt::CHAT_SETMODE.id);
            self.out.p1(self.chat_public_mode);
            self.out.p1(self.chat_private_mode);
            self.out.p1(self.chat_trade_mode);
        }
        if self.shell.mouse_click_x >= 273
            && self.shell.mouse_click_x <= 373
            && self.shell.mouse_click_y >= 467
            && self.shell.mouse_click_y <= 499
        {
            self.chat_trade_mode = (self.chat_trade_mode + 1) % 3;
            self.redraw_chat_mode = true;
            self.redraw_chat = true;
            self.out.p1_enc(ClientProt::CHAT_SETMODE.id);
            self.out.p1(self.chat_public_mode);
            self.out.p1(self.chat_private_mode);
            self.out.p1(self.chat_trade_mode);
        }
        if self.shell.mouse_click_x < 412
            || self.shell.mouse_click_x > 512
            || self.shell.mouse_click_y < 467
            || self.shell.mouse_click_y > 499
        {
            return;
        }
        self.close_modal();
        let report = self
            .ifaces_merged()
            .find(|e| e.client_code == CC_REPORT_INPUT)
            .map(|e| e.layer_id);
        if let Some(layer_id) = report {
            self.main_modal_id = layer_id;
        }
    }

    /// `handleInputKey` from client-ts (2937), chat branch: poll queued
    /// keys. An open social prompt (TS 2962-3020) consumes printable
    /// 32..=122 into `social_input` and 10/13 fires the add/del
    /// friend/ignore or PM send; an open amount dialog (TS 3022-3047)
    /// consumes digits into `dialog_input` and 10/13 sends
    /// `RESUME_P_COUNTDIALOG`; otherwise keys reach the public input
    /// while no chat modal is open. Printable 32..=122 (up to 126 once the
    /// input starts with `::`) appends below 80 chars, 8 backspaces,
    /// 10/13 sends. A `::` command goes out as `CLIENT_CHEAT`
    /// (TS 3093-3095); anything else packs `MESSAGE_PUBLIC` (TS 3158-3165)
    /// with the colour/effect prefixes parsed as TS 3097-3155 and the text
    /// WordPacked. The own message echoes locally as
    /// `toSentenceCase` + `WordFilter.filter` + `add_chat(2, ...)`
    /// (TS 3169-3179).
    pub fn handle_chat_input(&mut self) {
        loop {
            let key = self.shell.poll_key();
            if key == -1 {
                break;
            }

            if self.social_input_open {
                if (32..=122).contains(&key) && self.social_input.len() < 80 {
                    self.social_input.push(char::from_u32(key as u32).unwrap());
                    self.redraw_chat = true;
                }
                if key == 8 && !self.social_input.is_empty() {
                    self.social_input.pop();
                    self.redraw_chat = true;
                }
                if key == 13 || key == 10 {
                    self.social_input_open = false;
                    self.redraw_chat = true;

                    if self.social_input_type == 1 {
                        let userhash = JString::to_userhash(&self.social_input) as i64;
                        self.add_friend(userhash);
                    }
                    if self.social_input_type == 2 && self.friend_count > 0 {
                        let userhash = JString::to_userhash(&self.social_input) as i64;
                        self.del_friend(userhash);
                    }
                    if self.social_input_type == 3
                        && !self.social_input.is_empty()
                        && self.social_userhash != 0
                    {
                        self.out.p1_enc(ClientProt::MESSAGE_PRIVATE.id);
                        self.out.p1(0);
                        let start = self.out.pos;
                        self.out.p8(self.social_userhash);
                        WordPack::pack(&mut self.out, &self.social_input);
                        self.out.psize1((self.out.pos - start) as i32);

                        let mut text = JString::to_sentence_case(&self.social_input);
                        text = WordFilter::filter(&text);
                        let screen_name = JString::to_screen_name(&JString::to_raw_username(
                            self.social_userhash,
                        ));
                        self.add_chat(6, &text, &screen_name);

                        if self.chat_private_mode == 2 {
                            self.chat_private_mode = 1;
                            self.redraw_chat_mode = true;
                            self.out.p1_enc(ClientProt::CHAT_SETMODE.id);
                            self.out.p1(self.chat_public_mode);
                            self.out.p1(self.chat_private_mode);
                            self.out.p1(self.chat_trade_mode);
                        }
                    }
                    if self.social_input_type == 4 && self.ignore_count < 100 {
                        let userhash = JString::to_userhash(&self.social_input) as i64;
                        self.add_ignore(userhash);
                    }
                    if self.social_input_type == 5 && self.ignore_count > 0 {
                        let userhash = JString::to_userhash(&self.social_input) as i64;
                        self.del_ignore(userhash);
                    }
                }
                continue;
            }

            // TS 3022-3047: the enter-amount dialog — digits append up to
            // 10 chars, 8 backspaces, and 10/13 sends the amount back with
            // RESUME_P_COUNTDIALOG.
            if self.dialog_input_open {
                if (48..=57).contains(&key) && self.dialog_input.len() < 10 {
                    self.dialog_input.push(char::from_u32(key as u32).unwrap());
                    self.redraw_chat = true;
                }

                if key == 8 && !self.dialog_input.is_empty() {
                    self.dialog_input.pop();
                    self.redraw_chat = true;
                }

                if key == 13 || key == 10 {
                    if !self.dialog_input.is_empty() {
                        let value: i32 = self.dialog_input.parse().unwrap_or(0);
                        self.out.p1_enc(ClientProt::RESUME_P_COUNTDIALOG.id);
                        self.out.p4(value);
                    }

                    self.dialog_input_open = false;
                    self.redraw_chat = true;
                }
                continue;
            }

            if self.chat_modal_id != -1 {
                continue;
            }

            if key >= 32
                && (key <= 122 || (self.chat_input.starts_with("::") && key <= 126))
                && self.chat_input.len() < 80
            {
                self.chat_input.push(char::from_u32(key as u32).unwrap());
                self.redraw_chat = true;
            }

            if key == 8 && !self.chat_input.is_empty() {
                self.chat_input.pop();
                self.redraw_chat = true;
            }

            if (key == 13 || key == 10) && !self.chat_input.is_empty() {
                if self.chat_input.starts_with("::") {
                    self.out.p1_enc(ClientProt::CLIENT_CHEAT.id);
                    self.out.p1((self.chat_input.len() - 2 + 1) as i32);
                    self.out.pjstr(&self.chat_input[2..]);
                } else {
                    let mut text = self.chat_input.clone();
                    let mut colour = 0;
                    // TS 3097-3145 colour prefixes (sequential ifs, as TS).
                    if text.starts_with("yellow:") {
                        text = text[7..].to_string();
                    }
                    if text.starts_with("red:") {
                        colour = 1;
                        text = text[4..].to_string();
                    }
                    if text.starts_with("green:") {
                        colour = 2;
                        text = text[6..].to_string();
                    }
                    if text.starts_with("cyan:") {
                        colour = 3;
                        text = text[5..].to_string();
                    }
                    if text.starts_with("purple:") {
                        colour = 4;
                        text = text[7..].to_string();
                    }
                    if text.starts_with("white:") {
                        colour = 5;
                        text = text[6..].to_string();
                    }
                    if text.starts_with("flash1:") {
                        colour = 6;
                        text = text[7..].to_string();
                    }
                    if text.starts_with("flash2:") {
                        colour = 7;
                        text = text[7..].to_string();
                    }
                    if text.starts_with("flash3:") {
                        colour = 8;
                        text = text[7..].to_string();
                    }
                    if text.starts_with("glow1:") {
                        colour = 9;
                        text = text[6..].to_string();
                    }
                    if text.starts_with("glow2:") {
                        colour = 10;
                        text = text[6..].to_string();
                    }
                    if text.starts_with("glow3:") {
                        colour = 11;
                        text = text[6..].to_string();
                    }
                    // TS 3147-3155 effect prefixes.
                    let mut effect = 0;
                    if text.starts_with("wave:") {
                        effect = 1;
                        text = text[5..].to_string();
                    }
                    if text.starts_with("scroll:") {
                        effect = 2;
                        text = text[7..].to_string();
                    }

                    self.out.p1_enc(ClientProt::MESSAGE_PUBLIC.id);
                    self.out.p1(0);
                    let start = self.out.pos;
                    self.out.p1(colour);
                    self.out.p1(effect);
                    WordPack::pack(&mut self.out, &text);
                    self.out.psize1((self.out.pos - start) as i32);

                    // Echo the own message as TS 3169-3179:
                    // toSentenceCase then WordFilter (identity until the
                    // wordenc jag loads), stamping the local player's chat
                    // bubble only once it has a name (TS 3168-3174). Name
                    // falls back to the login user, then "player", so the
                    // echo is never dropped pre-spawn.
                    text = JString::to_sentence_case(&text);
                    text = WordFilter::filter(&text);
                    if let Some(p) = self.local_player.as_mut() {
                        if p.name.is_some() {
                            p.chat_message = Some(text.clone());
                            p.chat_colour = colour;
                            p.chat_effect = effect;
                            p.chat_timer = 150;
                        }
                    }
                    let name = self
                        .local_player
                        .as_ref()
                        .and_then(|p| p.name.clone())
                        .or_else(|| {
                            let user = self.login_user.clone();
                            (!user.is_empty()).then_some(user)
                        })
                        .unwrap_or_else(|| "player".to_string());
                    let sender = if self.staffmodlevel == 2 {
                        format!("@cr2@{name}")
                    } else if self.staffmodlevel == 1 {
                        format!("@cr1@{name}")
                    } else {
                        name
                    };
                    self.add_chat(2, &text, &sender);

                    // TS 3183-3189: a public send while the mode is "off"
                    // (2) auto-hides the bubble by switching to "friends"
                    // (3) and telling the server.
                    if self.chat_public_mode == 2 {
                        self.chat_public_mode = 3;
                        self.redraw_chat_mode = true;
                        self.out.p1_enc(ClientProt::CHAT_SETMODE.id);
                        self.out.p1(self.chat_public_mode);
                        self.out.p1(self.chat_private_mode);
                        self.out.p1(self.chat_trade_mode);
                    }
                }
                self.chat_input.clear();
                self.redraw_chat = true;
            }
        }
    }

    /// `mouseLoop` from Client.ts (8256-8380): the minimenu click pass.
    /// `build_minimenu` must have run this frame (`game_draw` calls it from
    /// `other_overlays` when no menu is open) so the entry arrays are
    /// populated. An in-flight inventory drag returns immediately — the
    /// drag owns the click (TS 8258-8260). While the menu is open a left
    /// click on an option row fires it through `doAction` and closes the
    /// menu; any other click (or hover with no click) that leaves the menu
    /// plus a 10px gutter closes it without an action (TS 8293-8337). With
    /// no menu open, a left click whose last entry is a held/inv action on
    /// a swap/replace component starts an inventory drag (TS 8339-8368),
    /// `oneMouseButton`/add-friend remaps left to right for multi-entry
    /// menus, otherwise left fires the last entry and right opens the menu
    /// (TS 8370-8380). The mobile `dialogInputOpen` skip (8261-8263) is not
    /// ported.
    pub fn mouse_loop(&mut self) {
        if self.obj_drag_area != 0 {
            return;
        }

        let mut button = self.shell.mouse_click_button;
        // TS 8263-8265: a target-mode click in the 516..765 x 160..205
        // strip (minimap/feedback area) is swallowed.
        if self.target_mode == 1
            && self.shell.mouse_click_x >= 516
            && self.shell.mouse_click_y >= 160
            && self.shell.mouse_click_x <= 765
            && self.shell.mouse_click_y <= 205
        {
            button = 0;
        }

        if self.is_menu_open {
            if button == 1 {
                let menu_x = self.menu_x;
                let menu_y = self.menu_y;
                let menu_width = self.menu_width;

                let mut click_x = self.shell.mouse_click_x;
                let mut click_y = self.shell.mouse_click_y;
                if self.menu_area == 0 {
                    click_x -= 4;
                    click_y -= 4;
                } else if self.menu_area == 1 {
                    click_x -= 553;
                    click_y -= 205;
                } else if self.menu_area == 2 {
                    click_x -= 17;
                    click_y -= 357;
                }

                let mut option = -1;
                for i in 0..self.menu_num_entries {
                    let option_y = menu_y + (self.menu_num_entries - 1 - i) * 15 + 31;
                    if click_x > menu_x
                        && click_x < menu_x + menu_width
                        && click_y > option_y - 13
                        && click_y < option_y + 3
                    {
                        option = i;
                    }
                }

                if option != -1 {
                    self.doAction(option);
                }

                self.is_menu_open = false;
                if self.menu_area == 1 {
                    self.redraw_side = true;
                } else if self.menu_area == 2 {
                    self.redraw_chat = true;
                }
            } else {
                // TS 8293-8325: leave-check on the live pointer position.
                let mut x = self.shell.mouse_x;
                let mut y = self.shell.mouse_y;
                if self.menu_area == 0 {
                    x -= 4;
                    y -= 4;
                } else if self.menu_area == 1 {
                    x -= 553;
                    y -= 205;
                } else if self.menu_area == 2 {
                    x -= 17;
                    y -= 357;
                }

                if x < self.menu_x - 10
                    || x > self.menu_x + self.menu_width + 10
                    || y < self.menu_y - 10
                    || y > self.menu_y + self.menu_height + 10
                {
                    self.is_menu_open = false;
                    if self.menu_area == 1 {
                        self.redraw_side = true;
                    }
                    if self.menu_area == 2 {
                        self.redraw_chat = true;
                    }
                }
            }
        } else {
            // TS 8339-8368: drag start from the last entry when it is a
            // held/inv action on a swap/replace component (`paramB` slot,
            // `paramC` com id, grab at the click coords).
            if button == 1 && self.menu_num_entries > 0 {
                let last = self.menu_num_entries as usize - 1;
                let action = self.menu_action[last];
                if Self::is_drag_start_action(action) {
                    let slot = self.menu_param_b[last];
                    let com_id = self.menu_param_c[last];
                    if self.obj_drag_start(
                        com_id,
                        slot,
                        self.shell.mouse_click_x,
                        self.shell.mouse_click_y,
                    ) {
                        return;
                    }
                }
            }

            // TS 8370-8372: a one-button mouse (or an add-friend last
            // entry) left-click opens a multi-entry menu instead.
            if button == 1
                && (self.one_mouse_button == 1
                    || self.is_add_friend_option(self.menu_num_entries - 1))
                && self.menu_num_entries > 2
            {
                button = 2;
            }

            if button == 1 && self.menu_num_entries > 0 {
                self.doAction(self.menu_num_entries - 1);
            } else if button == 2 && self.menu_num_entries > 0 {
                self.open_menu();
            }
        }
    }

    /// The TS 8339-8344 drag-start action set: `INV_BUTTON1..5`,
    /// `OP_HELD1..5`, `USEHELD_START`, `OP_HELD6`.
    fn is_drag_start_action(action: i32) -> bool {
        matches!(
            action,
            MiniMenuAction::INV_BUTTON1
                | MiniMenuAction::INV_BUTTON2
                | MiniMenuAction::INV_BUTTON3
                | MiniMenuAction::INV_BUTTON4
                | MiniMenuAction::INV_BUTTON5
                | MiniMenuAction::OP_HELD1
                | MiniMenuAction::OP_HELD2
                | MiniMenuAction::OP_HELD3
                | MiniMenuAction::OP_HELD4
                | MiniMenuAction::OP_HELD5
                | MiniMenuAction::USEHELD_START
                | MiniMenuAction::OP_HELD6
        )
    }

    /// `isAddFriendOption` from Java (Client.java 5513-5522): the option is
    /// `FRIENDLIST_ADD` (605 after the priority suffix).
    fn is_add_friend_option(&self, option: i32) -> bool {
        if option < 0 {
            return false;
        }
        let mut action = self.menu_action[option as usize];
        if action >= MiniMenuAction::_PRIORITY {
            action -= MiniMenuAction::_PRIORITY;
        }
        action == MiniMenuAction::FRIENDLIST_ADD
    }

    /// `openMenu` from client-ts (8442-8546): size the menu to the widest
    /// option (`b12.string_wid`; 0+8 when no font), then clamp it into the
    /// first panel holding the click — viewport 0 (512×334), side 1
    /// (190×261), chat 2 (479×96). The `menu_num_entries * 15 + 21` local
    /// fits the y-clamp; the stored `menu_height` is `entries * 15 + 22`,
    /// both verbatim from TS.
    ///
    /// The `b12` font lives on the separate `Renderer`, so the sim measures
    /// without it (width 8); `draw_minimenu` re-measures from the font and
    /// re-clamps `menu_x`/`menu_width` on the first draw, which always
    /// precedes a click on an open menu.
    pub fn open_menu(&mut self) {
        let width: i32 = 8;

        let height: i32 = self.menu_num_entries * 15 + 21;

        let (click_x, click_y) = (self.shell.mouse_click_x, self.shell.mouse_click_y);

        // the viewport (TS 8463-8482)
        if click_x > 4 && click_y > 4 && click_x < 516 && click_y < 338 {
            let mut x = click_x - (width / 2) - 4;
            if x + width > 512 {
                x = 512 - width;
            }
            if x < 0 {
                x = 0;
            }

            let mut y = click_y - 4;
            if y + height > 334 {
                y = 334 - height;
            }
            if y < 0 {
                y = 0;
            }

            self.is_menu_open = true;
            self.menu_area = 0;
            self.menu_x = x;
            self.menu_y = y;
            self.menu_width = width;
            self.menu_height = self.menu_num_entries * 15 + 22;
        }

        // the sidebar/tabs area (TS 8485-8508)
        if click_x > 553 && click_y > 205 && click_x < 743 && click_y < 466 {
            let mut x = click_x - (width / 2) - 553;
            if x < 0 {
                x = 0;
            } else if x + width > 190 {
                x = 190 - width;
            }

            let mut y = click_y - 205;
            if y < 0 {
                y = 0;
            } else if y + height > 261 {
                y = 261 - height;
            }

            self.is_menu_open = true;
            self.menu_area = 1;
            self.menu_x = x;
            self.menu_y = y;
            self.menu_width = width;
            self.menu_height = self.menu_num_entries * 15 + 22;
        }

        // the chatbox area (TS 8511-8533)
        if click_x > 17 && click_y > 357 && click_x < 496 && click_y < 453 {
            let mut x = click_x - (width / 2) - 17;
            if x < 0 {
                x = 0;
            } else if x + width > 479 {
                x = 479 - width;
            }

            let mut y = click_y - 357;
            if y < 0 {
                y = 0;
            } else if y + height > 96 {
                y = 96 - height;
            }

            self.is_menu_open = true;
            self.menu_area = 2;
            self.menu_x = x;
            self.menu_y = y;
            self.menu_width = width;
            self.menu_height = self.menu_num_entries * 15 + 22;
        }
    }

    /// `addWorldOptions` from Client.ts (9276-9457): fill the menu from the
    /// `pix3d` pick list. Walk here goes first when no item is held and no
    /// target is armed; each picked typecode decodes into a loc, npc,
    /// player, or ground-obj option block (duplicate typecodes skipped).
    pub fn add_world_options(&mut self) {
        if self.use_mode == 0 && self.target_mode == 0 {
            let option = "Walk here".to_string();
            self.push_option(
                option,
                MiniMenuAction::WALK,
                0,
                self.shell.mouse_x,
                self.shell.mouse_y,
            );
        }

        let mut last_typecode = -1i32;
        for picked in 0..self.pick_count {
            let typecode = self.pick_typecodes[picked as usize];
            let x = typecode & 0x7f;
            let z = (typecode >> 7) & 0x7f;
            let entity_type = (typecode >> 29) & 0x3;
            let type_id = (typecode >> 14) & 0x7fff;

            if typecode == last_typecode {
                continue;
            }
            last_typecode = typecode;

            if entity_type == 2 && self.world.type_code2(self.minusedlevel, x, z, typecode) >= 0 {
                // loc: ops 4..0, then Examine; Use/TGT replace them
                let loc = self.cache.locs.get(type_id as usize);
                if let Some(loc) = loc {
                    let loc_name = loc.name.clone();
                    let loc_ops = loc.op.clone();
                    if self.use_mode == 1 {
                        let option =
                            format!("Use {} with @cya@{}", self.obj_selected_name, loc_name);
                        self.push_option(option, MiniMenuAction::USEHELD_ONLOC, typecode, x, z);
                    } else if self.target_mode == 1 {
                        if (self.target_mask & 0x4) == 0x4 {
                            let option = format!("{} @cya@{}", self.target_op, loc_name);
                            self.push_option(option, MiniMenuAction::TGT_LOC, typecode, x, z);
                        }
                    } else {
                        for i in (0..=4).rev() {
                            if let Some(op) = loc_ops.get(i).and_then(|o| o.clone()) {
                                let option = format!("{op} @cya@{loc_name}");
                                self.push_option(option, LOC_OP_ACTIONS[i], typecode, x, z);
                            }
                        }
                        let option = format!("Examine @cya@{loc_name}");
                        self.push_option(option, MiniMenuAction::OP_LOC6, typecode, x, z);
                    }
                }
            } else if entity_type == 1 {
                // npc: stacked npcs sharing the tile, then the picked one
                let npc = self.npc.get(type_id as usize).and_then(|n| n.as_ref());
                if let Some(npc) = npc {
                    let npc_type = npc.r#type;
                    let (npc_x, npc_z) = (npc.x, npc.z);
                    if let Some(npc_type) = npc_type {
                        let size_one = self
                            .cache
                            .npcs
                            .get(npc_type)
                            .map(|t| t.size == 1)
                            .unwrap_or(false);
                        if size_one && (npc_x & 0x7f) == 64 && (npc_z & 0x7f) == 64 {
                            for i in 0..self.npc_count {
                                let id = self.npc_ids[i as usize];
                                if id == type_id {
                                    continue;
                                }
                                let other = self.npc.get(id as usize).and_then(|n| n.as_ref());
                                if let Some(other) = other {
                                    if let Some(other_type) = other.r#type {
                                        let size_one = self
                                            .cache
                                            .npcs
                                            .get(other_type)
                                            .map(|t| t.size == 1)
                                            .unwrap_or(false);
                                        if size_one && other.x == npc_x && other.z == npc_z {
                                            self.add_npc_options(other_type as i32, id, x, z);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(npc_type) = npc_type {
                        self.add_npc_options(npc_type as i32, type_id, x, z);
                    }
                }
            } else if entity_type == 0 {
                // player: stacked npcs/players sharing the tile, then self
                let player = self.players.get(type_id as usize).and_then(|p| p.as_ref());
                if let Some(player) = player {
                    let (player_x, player_z) = (player.x, player.z);
                    if (player_x & 0x7f) == 64 && (player_z & 0x7f) == 64 {
                        for i in 0..self.npc_count {
                            let id = self.npc_ids[i as usize];
                            let other = self.npc.get(id as usize).and_then(|n| n.as_ref());
                            if let Some(other) = other {
                                if let Some(other_type) = other.r#type {
                                    let size_one = self
                                        .cache
                                        .npcs
                                        .get(other_type)
                                        .map(|t| t.size == 1)
                                        .unwrap_or(false);
                                    if size_one && other.x == player_x && other.z == player_z {
                                        self.add_npc_options(other_type as i32, id, x, z);
                                    }
                                }
                            }
                        }
                        for i in 0..self.player_count {
                            let id = self.player_ids[i as usize];
                            if id == type_id {
                                continue;
                            }
                            let other = self.players.get(id as usize).and_then(|p| p.as_ref());
                            if let Some(other) = other {
                                if other.x == player_x && other.z == player_z {
                                    self.add_player_options(id, x, z);
                                }
                            }
                        }
                    }
                    self.add_player_options(type_id, x, z);
                }
            } else if entity_type == 3 {
                // ground objs: iterate the tile list tail->prev
                let level = self.minusedlevel as usize;
                let mut cell = self.ground_obj[level][x as usize][z as usize].take();
                if let Some(objs) = cell.as_mut() {
                    let mut node = objs.tail();
                    while let Some(obj) = node {
                        let obj_id = obj.id;
                        let type_name = self
                            .cache
                            .objs
                            .get(obj_id as usize)
                            .map(|t| t.name.clone())
                            .unwrap_or_default();
                        let type_ops = self
                            .cache
                            .objs
                            .get(obj_id as usize)
                            .map(|t| t.op.clone())
                            .unwrap_or_default();
                        if self.use_mode == 1 {
                            let option =
                                format!("Use {} with @lre@{}", self.obj_selected_name, type_name);
                            self.push_option(option, MiniMenuAction::USEHELD_ONOBJ, obj_id, x, z);
                        } else if self.target_mode == 1 {
                            if (self.target_mask & 0x1) == 0x1 {
                                let option = format!("{} @lre@{}", self.target_op, type_name);
                                self.push_option(option, MiniMenuAction::TGT_OBJ, obj_id, x, z);
                            }
                        } else {
                            for op in (0..=4).rev() {
                                if let Some(o) = type_ops[op].as_deref() {
                                    let option = format!("{o} @lre@{type_name}");
                                    self.push_option(option, OBJ_OP_ACTIONS[op], obj_id, x, z);
                                } else if op == 2 {
                                    let option = format!("Take @lre@{type_name}");
                                    self.push_option(option, MiniMenuAction::OP_OBJ3, obj_id, x, z);
                                }
                            }
                            let option = format!("Examine @lre@{type_name}");
                            self.push_option(option, MiniMenuAction::OP_OBJ6, obj_id, x, z);
                        }
                        node = objs.prev();
                    }
                }
                self.ground_obj[level][x as usize][z as usize] = cell;
            }
        }
    }

    /// `addNpcOptions` from Client.ts (9459-9535): one npc's menu block.
    /// `npc_id` indexes `cache.npcs`, `a` is the npc's slot in `self.npc`
    /// (sent in the OP_NPC packets). Attack is deferred past the other ops
    /// and priority-pinned when it outlevels the local player.
    pub fn add_npc_options(&mut self, npc_id: i32, a: i32, b: i32, c: i32) {
        if self.menu_num_entries >= 400 {
            return;
        }
        let npc_type = match self.cache.npcs.get(npc_id as usize) {
            Some(t) => t,
            None => return,
        };
        let name = npc_type.name.clone();
        let vislevel = npc_type.vislevel;
        let ops = npc_type.op.clone();

        let mut tooltip = name;
        if vislevel != 0 {
            if let Some(local) = &self.local_player {
                tooltip.push_str(combat_colour_code(local.combat_level, vislevel));
                tooltip.push_str(&format!(" (level-{vislevel})"));
            }
        }

        if self.use_mode == 1 {
            let option = format!("Use {} with @yel@{}", self.obj_selected_name, tooltip);
            self.push_option(option, MiniMenuAction::USEHELD_ONNPC, a, b, c);
        } else if self.target_mode == 1 {
            if (self.target_mask & 0x2) == 0x2 {
                let option = format!("{} @yel@{}", self.target_op, tooltip);
                self.push_option(option, MiniMenuAction::TGT_NPC, a, b, c);
            }
        } else {
            for i in (0..=4).rev() {
                let Some(op) = ops.get(i).and_then(|o| o.clone()) else {
                    continue;
                };
                if op.eq_ignore_ascii_case("attack") {
                    continue;
                }
                let option = format!("{op} @yel@{tooltip}");
                self.push_option(option, NPC_OP_ACTIONS[i], a, b, c);
            }
            for i in (0..=4).rev() {
                let Some(op) = ops.get(i).and_then(|o| o.clone()) else {
                    continue;
                };
                if !op.eq_ignore_ascii_case("attack") {
                    continue;
                }
                let mut priority = 0;
                if let Some(local) = &self.local_player {
                    if vislevel > local.combat_level {
                        priority = MiniMenuAction::_PRIORITY;
                    }
                }
                let option = format!("{op} @yel@{tooltip}");
                self.push_option(option, priority + NPC_OP_ACTIONS[i], a, b, c);
            }
            let option = format!("Examine @yel@{tooltip}");
            self.push_option(option, MiniMenuAction::OP_NPC6, a, b, c);
        }
    }

    /// `addPlayerOptions` from Client.ts (9537-9616): the local player is
    /// skipped (`players[LOCAL_PLAYER_INDEX]`), the `player_op` list fills
    /// the menu, and the Walk here entry is renamed with the player name.
    pub fn add_player_options(&mut self, a: i32, b: i32, c: i32) {
        if a == LOCAL_PLAYER_INDEX || self.menu_num_entries >= 400 {
            return;
        }
        let player = match self.players.get(a as usize).and_then(|p| p.as_ref()) {
            Some(p) => p,
            None => return,
        };
        let name = player.name.clone().unwrap_or_default();
        let combat_level = player.combat_level;
        let skill_level = player.skill_level;

        let tooltip = if skill_level == 0 {
            if let Some(local) = &self.local_player {
                format!(
                    "{name}{} (level-{combat_level})",
                    combat_colour_code(local.combat_level, combat_level)
                )
            } else {
                format!("{name} (skill-{skill_level})")
            }
        } else {
            format!("{name} (skill-{skill_level})")
        };

        if self.use_mode == 1 {
            let option = format!("Use {} with @whi@{}", self.obj_selected_name, tooltip);
            self.push_option(option, MiniMenuAction::USEHELD_ONPLAYER, a, b, c);
        } else if self.target_mode == 1 {
            if (self.target_mask & 0x8) == 0x8 {
                let option = format!("{} @whi@{}", self.target_op, tooltip);
                self.push_option(option, MiniMenuAction::TGT_PLAYER, a, b, c);
            }
        } else {
            for i in (0..=4).rev() {
                let Some(op) = self.player_op[i].clone() else {
                    continue;
                };
                let option = format!("{op} @whi@{tooltip}");
                let mut priority = 0;
                if op.eq_ignore_ascii_case("attack") {
                    if let Some(local) = &self.local_player {
                        if combat_level > local.combat_level {
                            priority = MiniMenuAction::_PRIORITY;
                        }
                    }
                } else if self.player_op_priority[i] {
                    priority = MiniMenuAction::_PRIORITY;
                }
                self.push_option(option, priority + PLAYER_OP_ACTIONS[i], a, b, c);
            }
        }

        for i in 0..self.menu_num_entries {
            if self.menu_action[i as usize] == MiniMenuAction::WALK {
                self.menu_option[i as usize] = format!("Walk here @whi@{tooltip}");
                break;
            }
        }
    }

    /// Append one menu row. `MENU_CAPACITY` bounds the fixed arrays; real
    /// menus never approach it (npc/player adders cap at 400 like TS).
    fn push_option(&mut self, option: String, action: i32, a: i32, b: i32, c: i32) {
        if self.menu_num_entries as usize >= MENU_CAPACITY {
            return;
        }
        let index = self.menu_num_entries as usize;
        self.menu_option[index] = option;
        self.menu_action[index] = action;
        self.menu_param_a[index] = a;
        self.menu_param_b[index] = b;
        self.menu_param_c[index] = c;
        self.menu_num_entries += 1;
    }

    /// `minimapLoop` from Client.ts (2742): a left click inside the
    /// 146×151 map ring is converted through the orbit yaw and minimap
    /// angle/zoom into a destination tile, then `tryMove(..., 1)` writes
    /// MOVE_MINIMAPCLICK and the 14 trailing bytes.
    pub fn minimap_loop(&mut self) {
        if self.minimap_state != 0 || self.shell.mouse_click_button != 1 {
            return;
        }
        let (px, pz, src_x, src_z) = match &self.local_player {
            Some(p) => (p.x, p.z, p.route_x[0], p.route_z[0]),
            None => return,
        };

        let x = self.shell.mouse_click_x - 25 - 550;
        let y = self.shell.mouse_click_y - 4 - 4;
        if x < 0 || y < 0 || x >= 146 || y >= 151 {
            return;
        }
        let x = x - 73;
        let y = y - 75;

        let yaw = (self.orbit_camera_yaw + self.macro_minimap_angle) & 0x7ff;
        let mut sin_yaw = Pix3D::sin_table().get(yaw as usize).copied().unwrap_or(0);
        let mut cos_yaw = Pix3D::cos_table().get(yaw as usize).copied().unwrap_or(0);
        sin_yaw = (sin_yaw * (self.macro_minimap_zoom + 256)) >> 8;
        cos_yaw = (cos_yaw * (self.macro_minimap_zoom + 256)) >> 8;

        let rel_x = (y * sin_yaw + x * cos_yaw) >> 11;
        let rel_y = (y * cos_yaw - x * sin_yaw) >> 11;

        let tile_x = (px + rel_x) >> 7;
        let tile_z = (pz - rel_y) >> 7;

        if self.tryMove(src_x, src_z, tile_x, tile_z, true, 0, 0, 0, 0, 0, 1) {
            // the 14 bytes trailing MOVE_MINIMAPCLICK, as TS 2773-2781
            self.out.p1(x);
            self.out.p1(y);
            self.out.p2(self.orbit_camera_yaw);
            self.out.p1(57);
            self.out.p1(self.macro_minimap_angle);
            self.out.p1(self.macro_minimap_zoom);
            self.out.p1(89);
            self.out.p2(px);
            self.out.p2(pz);
            self.out.p1(self.try_move_nearest);
            self.out.p1(63);
        }
    }

    /// SIM half of `checkMinimap` from client-ts (5076): a low-memory
    /// level change re-enters the loading state (`scene_state = 1`), and
    /// while loading `check_scene` → `map_build` is polled each loop —
    /// independent of `draw`, so a headless client still builds the scene,
    /// emits `MAP_BUILD_COMPLETE` and reaches `scene_state == 2`. The
    /// draw-only halves (the loading splash and the minimap *image* build)
    /// run from `Renderer::mainredraw`.
    fn check_minimap(&mut self) {
        if self.config.lowmem
            && self.scene_state == 2
            && self.build_minusedlevel != self.minusedlevel
        {
            self.scene_state = 1;
            self.scene_load_start_time = Instant::now();
        }

        if self.scene_state == 1 {
            // TS logs a "glcfb" hang line when checkScene stalls past
            // 360 s; the console write is not ported.
            let _status = self.check_scene();
        }
    }

    /// `checkScene` from client-ts (5101): waits on the requested map
    /// squares, then builds the scene. Returns -1/-2 while ground/location
    /// data is still loading, -3 when a loc's models are not all
    /// available, -4 while player info is pending; on success sets
    /// `scene_state = 2`, runs `map_build` and emits MAP_BUILD_COMPLETE.
    pub fn check_scene(&mut self) -> i32 {
        if self.map_build_index.is_empty()
            || self.map_build_ground_data.is_empty()
            || self.map_build_location_data.is_empty()
        {
            return -1000; // custom
        }

        for i in 0..self.map_build_ground_data.len() {
            if self.map_build_ground_data[i].is_none() && self.map_build_ground_file[i] != -1 {
                return -1;
            }

            if self.map_build_location_data[i].is_none() && self.map_build_location_file[i] != -1 {
                return -2;
            }
        }

        // Only `lowMem` is consulted while waiting, so use the associated
        // `checkLocations` variant instead of a full `ClientBuild` (four
        // 4×104×104 grids plus the shadow/mapo scratch) every frame.
        let mut ready = true;
        for i in 0..self.map_build_ground_data.len() {
            if let Some(data) = &self.map_build_location_data[i] {
                let x = (self.map_build_index[i] >> 8) * 64 - self.map_build_base_x;
                let z = (self.map_build_index[i] & 0xff) * 64 - self.map_build_base_z;
                if !ClientBuild::check_locations_low_mem(
                    self.config.lowmem,
                    &self.cache,
                    data,
                    x,
                    z,
                ) {
                    ready = false;
                }
            }
        }

        if !ready {
            return -3;
        } else if self.awaiting_player_info {
            return -4;
        }

        self.scene_state = 2;
        self.map_build();
        self.out.p1_enc(ClientProt::MAP_BUILD_COMPLETE.id);
        0
    }

    /// Model ids referenced by the locs placed in the current build (walls,
    /// decor, scenery, ground decor). The render resolves those models
    /// lazily from typecodes, so lowmem's post-build unload must keep them.
    /// `pub` for the `client_build` integration tests.
    pub fn scene_model_ids(&self, model_count: usize) -> Vec<bool> {
        let mut used = vec![false; model_count];
        let mark = |used: &mut [bool], typecode: i32| {
            let loc_id = (typecode >> 14) & 0x7fff;
            if (loc_id as usize) >= self.cache.locs.len() {
                return;
            }
            let loc = self.cache.loc(loc_id as usize);
            if let Some(models) = &loc.model {
                for &m in models {
                    let id = (m & 0xffff) as usize;
                    if id < used.len() {
                        used[id] = true;
                    }
                }
            }
        };
        for level in 0..BuildArea::LEVELS {
            for x in 0..BuildArea::SIZE {
                for z in 0..BuildArea::SIZE {
                    let mut cursor = self.world.square(level, x, z);
                    while let Some(tile) = cursor {
                        if let Some(w) = tile.wall.as_deref() {
                            mark(&mut used, w.typecode);
                        }
                        if let Some(d) = tile.decor.as_deref() {
                            mark(&mut used, d.typecode);
                        }
                        if let Some(g) = tile.ground_decor.as_deref() {
                            mark(&mut used, g.typecode);
                        }
                        for i in 0..tile.sprite_count as usize {
                            if let Some(idx) = tile.sprite(i) {
                                if let Some(sprite) =
                                    self.world.sprites.get(idx).and_then(|s| s.as_ref())
                                {
                                    if (sprite.typecode >> 29) & 0x3 == 2 {
                                        mark(&mut used, sprite.typecode);
                                    }
                                }
                            }
                        }
                        cursor = tile.linked_square.as_deref();
                    }
                }
            }
        }
        used
    }

    /// Java unloads unused models after `mapBuild` (one client; models
    /// already decoded onto tiles). Our `GeometryStore` is process-wide
    /// and loc models are lazy: a slot's 104 must not `Model::unload`
    /// ids another slot still names — that is why stress50 walls vanished
    /// until a tele re-fetched them. Keep the snapshot.
    pub fn unload_unused_lowmem_models(&self) {}

    /// `mapBuild` from client-ts (5141): reset the scene grids, decode the
    /// requested map squares (`load_ground`/`fade_adjacent`/`load_locations`),
    /// run `finish_build`, then re-init the texture pool and prefetch the
    /// edge map files. The `showObject`/`locChangePostBuildCorrect` passes
    /// are slice-2 stubs; the TS entity/clear-cache lines around them are
    /// not ported.
    fn map_build(&mut self) {
        self.minimap_level = -1;
        self.spotanims.clear();
        self.projectiles.clear();
        self.world.reset_map();

        // Task 5 rule 6: build the render-only overlay meshes only when
        // headed; an unheaded slot keeps the tile stamps and materializes
        // on attach (the first paint consumes `overlay_pending`).
        self.world.overlay_mesh = self.draw;

        for level in 0..BuildArea::LEVELS {
            self.collision[level as usize].reset();
        }

        let mut build = ClientBuild::new();
        build.low_mem = self.config.lowmem;
        build.minusedlevel = self.minusedlevel;

        // underground pass check (TS 5163-5171): the Lumbridge caves square
        // forces high detail.
        for &index in &self.map_build_index {
            let x = index >> 8;
            let z = index & 0xff;
            if x == 33 && (71..=73).contains(&z) {
                build.low_mem = false;
                break;
            }
        }

        if build.low_mem {
            self.world.fill_base_level(self.minusedlevel);
        } else {
            self.world.fill_base_level(0);
        }

        if !self.map_build_ground_data.is_empty() {
            self.out.p1_enc(ClientProt::NO_TIMEOUT.id);

            for i in 0..self.map_build_ground_data.len() {
                let x = (self.map_build_index[i] >> 8) * 64 - self.map_build_base_x;
                let z = (self.map_build_index[i] & 0xff) * 64 - self.map_build_base_z;
                if let Some(data) = &self.map_build_ground_data[i] {
                    build.load_ground(
                        &mut self.groundh,
                        &mut self.mapl,
                        data,
                        (self.map_build_centre_zone_x - 6) * 8,
                        (self.map_build_centre_zone_z - 6) * 8,
                        x,
                        z,
                    );
                }
            }

            // missing land squares fade into the neighbouring heights, but
            // only outside the deep underground (TS 5187-5193).
            for i in 0..self.map_build_ground_data.len() {
                let x = (self.map_build_index[i] >> 8) * 64 - self.map_build_base_x;
                let z = (self.map_build_index[i] & 0xff) * 64 - self.map_build_base_z;
                if self.map_build_ground_data[i].is_none() && self.map_build_centre_zone_z < 800 {
                    build.fade_adjacent(&mut self.groundh, z, x, 64, 64);
                }
            }
        }

        // Java hands `World` the one `groundh` array Client writes; mirror
        // the decoded heights so the render pass (`render_quick_ground`)
        // reads the same ground the camera's `get_av_h` does.
        self.world.groundh.clone_from(&self.groundh);

        if !self.map_build_location_data.is_empty() {
            self.out.p1_enc(ClientProt::NO_TIMEOUT.id);

            for i in 0..self.map_build_location_data.len() {
                if let Some(data) = &self.map_build_location_data[i] {
                    let x = (self.map_build_index[i] >> 8) * 64 - self.map_build_base_x;
                    let z = (self.map_build_index[i] & 0xff) * 64 - self.map_build_base_z;
                    build.load_locations(
                        &self.cache,
                        &mut self.world,
                        &mut self.collision,
                        &self.groundh,
                        &self.mapl,
                        data,
                        x,
                        z,
                        self.loop_cycle,
                    );
                }
            }
        }

        self.out.p1_enc(ClientProt::NO_TIMEOUT.id);

        build.finish_build(
            &self.cache,
            &self.tex_average,
            &mut self.world,
            &mut self.collision,
            &self.groundh,
            &self.mapl,
        );
        // Keep the parked land/loc bytes: lowmem same-zone level change
        // (ladder) re-enters `scene_state = 1` and `check_scene` rebuilds
        // from them. `REBUILD_NORMAL` replaces the vecs on a zone change.

        // The world is now re-stamped with this build's locs. The snapshot's
        // loc family is gated on `gens.scene`, which the server bumps on the
        // map-build packet — that can land before this local re-stamp runs.
        // Bump it again so any snapshot read after this tick rebuilds the
        // loc list from the fresh world, never the previous build's locs.
        self.gens.scene += 1;

        self.out.p1_enc(ClientProt::NO_TIMEOUT.id);

        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                self.show_object(x, z);
            }
        }

        self.loc_change_post_build();
        self.build_minusedlevel = self.minusedlevel;

        self.unload_unused_lowmem_models();

        if let Some(od) = self.on_demand.as_mut() {
            od.clear_prefetches();
        }

        // TS 5264-5290: prefetch the map files one zone beyond the build
        // area's edge (the tutorial island pins a fixed 2x2 window). The
        // TS `| 0` truncations are identity on i32 here.
        let mut left = ((self.map_build_centre_zone_x - 6) / 8) - 1;
        let mut right = ((self.map_build_centre_zone_x + 6) / 8) + 1;
        let mut bottom = ((self.map_build_centre_zone_z - 6) / 8) - 1;
        let mut top = ((self.map_build_centre_zone_z + 6) / 8) + 1;

        if self.within_tutorial_island {
            left = 49;
            right = 50;
            bottom = 49;
            top = 50;
        }

        if let Some(od) = self.on_demand.as_mut() {
            for x in left..=right {
                for z in bottom..=top {
                    if left == x || right == x || bottom == z || top == z {
                        let land = od.get_map_file(x, z, 0);
                        if land != -1 {
                            od.prefetch(3, land);
                        }
                        let loc = od.get_map_file(x, z, 1);
                        if loc != -1 {
                            od.prefetch(3, loc);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn show_object(&mut self, x: i32, z: i32) {
        let level = self.minusedlevel as usize;
        if self.ground_obj[level][x as usize][z as usize].is_none() {
            self.world.del_obj(self.minusedlevel, x, z);
            return;
        }

        let (top, middle, bottom) = {
            let objs = self.ground_obj[level][x as usize][z as usize]
                .as_mut()
                .expect("cell checked above");

            // First walk: the highest-cost object is the stack top; stackable
            // costs scale with the stack size (`count + 1`).
            let mut top_cost = -99_999_999;
            let mut top_id = 0;
            let mut top_count = 0;
            let mut has_top = false;
            let mut node = objs.head();
            while let Some(o) = node {
                let id = o.id;
                let count = o.count;
                let typ = self.cache.obj(id as usize);
                let mut cost = typ.cost;
                if typ.stackable {
                    cost *= count + 1;
                }
                if cost > top_cost {
                    top_cost = cost;
                    top_id = id;
                    top_count = count;
                    has_top = true;
                }
                node = objs.next_node();
            }
            if !has_top {
                return; // custom: TS 7591
            }

            // Second walk: re-insert the top node as the head (TS
            // `objs.pushFront(topObj)` of the in-list node — never a
            // duplicate).
            let mut node = objs.head();
            while let Some(o) = node {
                if o.id == top_id && o.count == top_count {
                    objs.move_last_to_front();
                    break;
                }
                node = objs.next_node();
            }

            // Third walk from the head: the first other id is the bottom of
            // the stack, the first third id the middle.
            let top = ClientObj::new(top_id, top_count);
            let mut bottom: Option<ClientObj> = None;
            let mut middle: Option<ClientObj> = None;
            let mut node = objs.head();
            while let Some(o) = node {
                if o.id != top.id && bottom.is_none() {
                    bottom = Some(o.clone());
                }
                if o.id != top.id && middle.is_none() && matches!(&bottom, Some(b) if o.id != b.id)
                {
                    middle = Some(o.clone());
                }
                node = objs.next_node();
            }
            (top, middle, bottom)
        };

        let typecode = x.wrapping_add(z << 7).wrapping_add(0x6000_0000);
        let h = get_av_h(
            &self.groundh,
            &self.mapl,
            x * 128 + 64,
            z * 128 + 64,
            self.minusedlevel,
        );
        self.world.set_obj(
            x,
            z,
            h,
            self.minusedlevel,
            typecode,
            Some((top.id, top.count)),
            middle.map(|o| (o.id, o.count)),
            bottom.map(|o| (o.id, o.count)),
        );
    }

    /// `locChangePostBuildCorrect()` from client-ts (7422): reconcile the
    /// pending loc-change queue with the fresh scene. Permanent changes
    /// (`end_time == -1`) re-snapshot the old appearance and become due
    /// next tick; timed ones are dropped.
    pub(crate) fn loc_change_post_build(&mut self) {
        let mut node = self.loc_changes.head();
        while let Some(loc) = node {
            if loc.end_time == -1 {
                loc.start_time = 0;
                Self::loc_change_set_old(&self.world, loc);
            } else {
                self.loc_changes.unlink_last();
            }
            node = self.loc_changes.next_node();
        }
    }

    /// `locChangeSetOld(loc)` from client-ts (7433): snapshot the tile's
    /// current appearance onto a loc-change node. Takes `&World` so the
    /// caller can pass a node already owned by `loc_changes` (the
    /// field-disjoint borrow) or a local not yet pushed.
    fn loc_change_set_old(world: &World, loc: &mut LocChange) {
        let mut typecode = 0;
        let mut other_id = -1;
        let mut other_shape = 0;
        let mut other_angle = 0;

        if loc.layer == LocLayer::WALL {
            typecode = world.wall_type(loc.level, loc.x, loc.z);
        } else if loc.layer == LocLayer::WALL_DECOR {
            // TS `decorType(level, z, x)` swaps its parameter names but
            // indexes `squares[level][x][z]`; call with tile x, z.
            typecode = world.decor_type(loc.level, loc.x, loc.z);
        } else if loc.layer == LocLayer::GROUND {
            typecode = world.scene_type(loc.level, loc.x, loc.z);
        } else if loc.layer == LocLayer::GROUND_DECOR {
            typecode = world.gd_type(loc.level, loc.x, loc.z);
        }

        if typecode != 0 {
            let other_info = world.type_code2(loc.level, loc.x, loc.z, typecode);
            other_id = (typecode >> 14) & 0x7fff;
            other_shape = other_info & 0x1f;
            other_angle = other_info >> 6;
        }

        loc.old_type = other_id;
        loc.old_shape = other_shape;
        loc.old_angle = other_angle;
    }

    /// `locChangeCreate(level, x, z, layer, type, shape, angle, startTime,
    /// endTime)` from client-ts (7396): reuse the tile's queued node when
    /// one exists, otherwise snapshot the old appearance and push a new one.
    #[allow(clippy::field_reassign_with_default, clippy::too_many_arguments)]
    fn loc_change_create(
        &mut self,
        level: i32,
        x: i32,
        z: i32,
        layer: i32,
        r#type: i32,
        shape: i32,
        angle: i32,
        start_time: i32,
        end_time: i32,
    ) {
        let mut next = self.loc_changes.head();
        while let Some(loc) = next {
            if loc.level == self.minusedlevel && loc.x == x && loc.z == z && loc.layer == layer {
                loc.new_type = r#type;
                loc.new_shape = shape;
                loc.new_angle = angle;
                loc.start_time = start_time;
                loc.end_time = end_time;
                return;
            }
            next = self.loc_changes.next_node();
        }

        let mut loc = LocChange::default();
        loc.level = level;
        loc.layer = layer;
        loc.x = x;
        loc.z = z;
        Self::loc_change_set_old(&self.world, &mut loc);
        loc.new_type = r#type;
        loc.new_shape = shape;
        loc.new_angle = angle;
        loc.start_time = start_time;
        loc.end_time = end_time;
        self.loc_changes.push(loc);
    }

    /// `locChangeUnchecked(level, layer, x, z, id, shape, angle)` from
    /// client-ts (7497): delete whatever occupies the tile on that layer
    /// (plus its collision), then place `id` when non-negative. The GROUND
    /// overflow check returns after the world delete, before any collision
    /// or placement work.
    #[allow(clippy::too_many_arguments)]
    fn loc_change_unchecked(
        &mut self,
        level: i32,
        layer: i32,
        x: i32,
        z: i32,
        id: i32,
        shape: i32,
        angle: i32,
    ) {
        if x < 1 || z < 1 || x > 102 || z > 102 {
            return;
        }

        if self.config.lowmem && level != self.minusedlevel {
            return;
        }

        let mut typecode = 0;
        if layer == LocLayer::WALL {
            typecode = self.world.wall_type(level, x, z);
        } else if layer == LocLayer::WALL_DECOR {
            // TS `decorType(level, z, x)` swaps its parameter names but
            // indexes `squares[level][x][z]`; call with tile x, z.
            typecode = self.world.decor_type(level, x, z);
        } else if layer == LocLayer::GROUND {
            typecode = self.world.scene_type(level, x, z);
        } else if layer == LocLayer::GROUND_DECOR {
            typecode = self.world.gd_type(level, x, z);
        }

        if typecode != 0 {
            let other_info = self.world.type_code2(level, x, z, typecode);
            let other_id = (typecode >> 14) & 0x7fff;
            let other_shape = other_info & 0x1f;
            let other_angle = other_info >> 6;

            if layer == LocLayer::WALL {
                self.world.del_wall(level, x, z);

                let r#type = self.cache.loc(other_id as usize);
                if r#type.blockwalk {
                    self.collision[level as usize].del_wall(
                        x,
                        z,
                        other_shape,
                        other_angle,
                        r#type.blockrange,
                    );
                }
            } else if layer == LocLayer::WALL_DECOR {
                self.world.del_decor(level, x, z);
            } else if layer == LocLayer::GROUND {
                self.world.del_loc(level, x, z);

                let r#type = self.cache.loc(other_id as usize);
                if x + r#type.width > BuildArea::SIZE - 1
                    || z + r#type.width > BuildArea::SIZE - 1
                    || x + r#type.length > BuildArea::SIZE - 1
                    || z + r#type.length > BuildArea::SIZE - 1
                {
                    return;
                }

                if r#type.blockwalk {
                    self.collision[level as usize].del_loc(
                        x,
                        z,
                        r#type.width,
                        r#type.length,
                        other_angle,
                        r#type.blockrange,
                    );
                }
            } else if layer == LocLayer::GROUND_DECOR {
                self.world.del_ground_decor(level, x, z);

                let r#type = self.cache.loc(other_id as usize);
                if r#type.blockwalk && r#type.active {
                    self.collision[level as usize].unblock_ground(x, z);
                }
            }
        }

        if id >= 0 {
            let mut tile_level = level;
            if level < 3 && (self.mapl[1][x as usize][z as usize] as i32 & MapFlag::LINK_BELOW) != 0
            {
                tile_level = level + 1;
            }

            ClientBuild::change_loc_unchecked(
                &self.cache,
                &mut self.world,
                Some(&mut self.collision[level as usize]),
                &self.groundh,
                level,
                x,
                z,
                id,
                shape,
                angle,
                tile_level,
                self.loop_cycle,
            );
        }
    }

    /// `locChangeDoQueue()` from client-ts (7465): step the loc-change
    /// queue. Once the scene is ready (`scene_state == 2`), apply the new
    /// appearance when its model is available, or restore the old one.
    fn loc_change_do_queue(&mut self) {
        if self.scene_state != 2 {
            return;
        }

        let mut node = self.loc_changes.head();
        while let Some(loc) = node {
            if loc.end_time > 0 {
                loc.end_time -= 1;
            }

            if loc.end_time != 0 {
                if loc.start_time > 0 {
                    loc.start_time -= 1;
                }

                if loc.start_time == 0
                    && loc.x >= 1
                    && loc.z >= 1
                    && loc.x <= 102
                    && loc.z <= 102
                    && (loc.new_type < 0
                        || ClientBuild::change_loc_available(
                            &self.cache,
                            loc.new_type,
                            loc.new_shape,
                        ))
                {
                    let level = loc.level;
                    let layer = loc.layer;
                    let x = loc.x;
                    let z = loc.z;
                    let new_type = loc.new_type;
                    let new_shape = loc.new_shape;
                    let new_angle = loc.new_angle;
                    let unlink = (loc.old_type == loc.new_type && loc.old_type == -1)
                        || (loc.old_type == loc.new_type
                            && loc.old_angle == loc.new_angle
                            && loc.old_shape == loc.new_shape);
                    loc.start_time = -1;
                    self.loc_change_unchecked(level, layer, x, z, new_type, new_shape, new_angle);
                    if unlink {
                        self.loc_changes.unlink_last();
                    }
                }
            } else if loc.old_type < 0
                || ClientBuild::change_loc_available(&self.cache, loc.old_type, loc.old_shape)
            {
                let level = loc.level;
                let layer = loc.layer;
                let x = loc.x;
                let z = loc.z;
                let old_type = loc.old_type;
                let old_shape = loc.old_shape;
                let old_angle = loc.old_angle;
                self.loc_change_unchecked(level, layer, x, z, old_type, old_shape, old_angle);
                self.loc_changes.unlink_last();
            }

            node = self.loc_changes.next_node();
        }
    }

    /// `cinemaCamera` from client-ts (3305): the cutscene camera eases
    /// `cam_move_to_*` toward the move target (rate plus a rate2 fraction of
    /// the remaining distance), then eases `cam_pitch`/`cam_yaw` toward the
    /// look-at target. The lerp is TS 1:1, including the yaw wrap (delta
    /// over 1024 wraps by 2048) and the final overshoot snap.
    pub fn cinema_camera(&mut self) {
        let mut x = self.cam_move_to_lx * 128 + 64;
        let mut z = self.cam_move_to_lz * 128 + 64;
        let mut y =
            get_av_h(&self.groundh, &self.mapl, x, z, self.minusedlevel) - self.cam_move_to_hei;

        if self.cam_x < x {
            self.cam_x +=
                self.cam_move_to_rate + (((x - self.cam_x) * self.cam_move_to_rate2) / 1000);
            if self.cam_x > x {
                self.cam_x = x;
            }
        }

        if self.cam_x > x {
            self.cam_x -=
                self.cam_move_to_rate + (((self.cam_x - x) * self.cam_move_to_rate2) / 1000);
            if self.cam_x < x {
                self.cam_x = x;
            }
        }

        if self.cam_y < y {
            self.cam_y +=
                self.cam_move_to_rate + (((y - self.cam_y) * self.cam_move_to_rate2) / 1000);
            if self.cam_y > y {
                self.cam_y = y;
            }
        }

        if self.cam_y > y {
            self.cam_y -=
                self.cam_move_to_rate + (((self.cam_y - y) * self.cam_move_to_rate2) / 1000);
            if self.cam_y < y {
                self.cam_y = y;
            }
        }

        if self.cam_z < z {
            self.cam_z +=
                self.cam_move_to_rate + (((z - self.cam_z) * self.cam_move_to_rate2) / 1000);
            if self.cam_z > z {
                self.cam_z = z;
            }
        }

        if self.cam_z > z {
            self.cam_z -=
                self.cam_move_to_rate + (((self.cam_z - z) * self.cam_move_to_rate2) / 1000);
            if self.cam_z < z {
                self.cam_z = z;
            }
        }

        x = self.cam_look_at_lx * 128 + 64;
        z = self.cam_look_at_lz * 128 + 64;
        y = get_av_h(&self.groundh, &self.mapl, x, z, self.minusedlevel) - self.cam_look_at_hei;

        let dx = x - self.cam_x;
        let dy = y - self.cam_y;
        let dz = z - self.cam_z;

        let distance = (f64::sqrt((dx * dx + dz * dz) as f64)) as i32;
        let mut pitch = ((dy as f64).atan2(distance as f64) * 325.949) as i32 & 0x7ff;
        let yaw = ((dx as f64).atan2(dz as f64) * -325.949) as i32 & 0x7ff;

        pitch = pitch.clamp(128, 383);

        if self.cam_pitch < pitch {
            self.cam_pitch += self.cam_look_at_rate
                + (((pitch - self.cam_pitch) * self.cam_look_at_rate2) / 1000);
            if self.cam_pitch > pitch {
                self.cam_pitch = pitch;
            }
        }

        if self.cam_pitch > pitch {
            self.cam_pitch -= self.cam_look_at_rate
                + (((self.cam_pitch - pitch) * self.cam_look_at_rate2) / 1000);
            if self.cam_pitch < pitch {
                self.cam_pitch = pitch;
            }
        }

        let mut delta_yaw = yaw - self.cam_yaw;
        if delta_yaw > 1024 {
            delta_yaw -= 2048;
        } else if delta_yaw < -1024 {
            delta_yaw += 2048;
        }

        if delta_yaw > 0 {
            self.cam_yaw += self.cam_look_at_rate + ((delta_yaw * self.cam_look_at_rate2) / 1000);
            self.cam_yaw &= 0x7ff;
        }

        if delta_yaw < 0 {
            self.cam_yaw -=
                self.cam_look_at_rate + (((-delta_yaw) * self.cam_look_at_rate2) / 1000);
            self.cam_yaw &= 0x7ff;
        }

        let mut tmp = yaw - self.cam_yaw;
        if tmp > 1024 {
            tmp -= 2048;
        } else if tmp < -1024 {
            tmp += 2048;
        }

        if (tmp < 0 && delta_yaw > 0) || (tmp > 0 && delta_yaw < 0) {
            self.cam_yaw = yaw;
        }
    }

    /// `gameLoop` from Java (`Client.java` 9341): count down a pending
    /// logout request, read up to five TCP packets, the side-tab click
    /// pass (TS `iconLoop`), the side/main/chat interface button passes
    /// (`buildMinimenu` branches), the chat key pass (TS
    /// `handleInputKey`), the
    /// in-game silence watchdog (`timeoutTimer > 750` → `lostCon`), then
    /// idle `NO_TIMEOUT` and flush `out` through `ClientStream::write`.
    /// Write errors are `lostCon` (Java `catch (IOException)`).
    pub fn game_loop(&mut self) {
        // TS 2043-2048: the reboot countdown holds at 1 while the logout
        // request counts down.
        if self.reboot_timer > 1 {
            self.reboot_timer -= 1;
        }
        if self.logout_timer > 0 {
            self.logout_timer -= 1;
        }
        for _ in 0..5 {
            if !self.tcp_in() {
                break;
            }
        }
        if !self.ingame {
            return;
        }
        // TS 2191-2192: the scene/minimap pass after the inbound reads.
        // The SIM half runs here unconditionally — `check_scene` →
        // `map_build` (ground, collision, `MAP_BUILD_COMPLETE`,
        // `scene_state = 2`) — independent of `draw`, so a headless client
        // still builds the scene. The draw-only halves (the loading splash
        // and the minimap *image* build) run from `Renderer::mainredraw`.
        self.check_minimap();
        self.loc_change_do_queue();
        // Java 9461 / TS 2193: drain SYNTH_SOUND into the mixer.
        self.sounds_do_queue();
        // TS 2191-2192: the loc-change pass then the world-update counter;
        // headless loops (no draw) keep it zeroed.
        self.world_update_num += 1;
        if !self.draw {
            self.world_update_num = 0;
        }
        // TS 2206-2211: the click crosshair fades — `cross_cycle` advances
        // 20 per loop and clears `cross_mode` once it passes 400.
        if self.cross_mode != 0 {
            self.cross_cycle += 20;
            if self.cross_cycle >= 400 {
                self.cross_mode = 0;
            }
        }
        // TS 2214-2226: the OP_HELD outline timeout — `selected_cycle`
        // counts frames and clears `selected_area` at 15 with a redraw.
        if self.selected_area != 0 {
            self.selected_cycle += 1;
            if self.selected_cycle >= 15 {
                if self.selected_area == 2 {
                    self.redraw_side = true;
                }
                if self.selected_area == 3 {
                    self.redraw_chat = true;
                }
                self.selected_area = 0;
            }
        }
        // TS 2229-2300: the in-flight obj-drag tick runs before the click
        // handlers so a release consumes the click before `handle_tab_clicks`.
        self.handle_obj_drag();
        self.handle_tab_clicks();
        self.handle_side_if_clicks();
        self.handle_main_if_clicks();
        self.handle_chat_if_clicks();
        self.chat_mode_loop();
        self.handle_chat_input();
        // consume the previous frame's `World` ground pick (TS 2310-2323)
        // into a MOVE_GAMECLICK walk, then the click passes
        if self.world.ground_x != -1 {
            let src = self
                .local_player
                .as_ref()
                .map(|p| (p.route_x[0], p.route_z[0]));
            let ground_x = self.world.ground_x;
            let ground_z = self.world.ground_z;
            self.world.ground_x = -1;
            if let Some((src_x, src_z)) = src {
                // TS 2317-2322: a successful walk re-arms the crosshair at
                // the clicked point (mode 1, cycle 0).
                if self.tryMove(src_x, src_z, ground_x, ground_z, true, 0, 0, 0, 0, 0, 0) {
                    self.cross_x = self.shell.mouse_click_x;
                    self.cross_y = self.shell.mouse_click_y;
                    self.cross_mode = 1;
                    self.cross_cycle = 0;
                }
            }
        }
        self.mouse_loop();
        self.minimap_loop();
        // Java 9466-9467 then 9580: the entity movement pass runs before
        // the camera pass, so the orbit camera and minimap follow the walk.
        self.move_players();
        self.move_npcs();
        // Java 9468 / TS 2202: `timeoutChat` after move, so overhead bubbles
        // expire (`chatTimer` 150 → 0 then `chatMessage = null`).
        self.timeout_chat();
        // TS 2346-2353: `followCamera` (renderer-owned now; `mainredraw`
        // runs it ahead of `game_draw`) eases the orbit camera to the local
        // player, then the cutscene camera when a CAM_* packet has set one,
        // and the shake cycles tick every loop.
        if self.scene_state == 2 && self.cinema_cam {
            self.cinema_camera();
        }
        for i in 0..5 {
            self.cam_shake_cycle[i] += 1;
        }
        // Dead-server watchdog, wall-clock: the 20 ms pass count is not a
        // clock once the host parks the slot (750 passes at one pass per
        // ~600 ms would take ~450 s); elapsed time since the last response
        // holds the ~15 s bound at any cadence. Checked every wake.
        if self.dead_server() {
            self.lost_con();
        }

        self.no_timeout_timer += 1;
        if self.no_timeout_timer > 50 {
            self.out.p1_enc(ClientProt::NO_TIMEOUT.id);
        }

        let write_result = if let Some(stream) = self.stream.as_mut() {
            if self.out.pos > 0 {
                Some(stream.write(self.out.data(), self.out.pos))
            } else {
                None
            }
        } else {
            None
        };
        match write_result {
            Some(Ok(())) => {
                self.out.pos = 0;
                self.no_timeout_timer = 0;
            }
            Some(Err(_)) => self.lost_con(),
            None => {}
        }
    }

    /// `movePlayers` from Java (`Client.java` 7559): one movement step for
    /// the local player — the `i == -1` slot. Rust login clones
    /// `players[LOCAL_PLAYER_INDEX]` into `local_player`, so the live walk
    /// interpolates `local_player`, not the stale clone — then every
    /// tracked player in `playerIds` order.
    pub fn move_players(&mut self) {
        if let Some(local) = self.local_player.as_ref() {
            let mut e = local.entity.clone();
            self.move_entity(true, &mut e);
            self.local_player.as_mut().unwrap().entity = e;
        }
        for i in 0..self.player_count as usize {
            let index = self.player_ids[i] as usize;
            let Some(player) = self.players.get(index).and_then(|p| p.as_ref()) else {
                continue;
            };
            let mut e = player.entity.clone();
            self.move_entity(false, &mut e);
            if let Some(slot) = self.players.get_mut(index).and_then(|p| p.as_mut()) {
                slot.entity = e;
            }
        }
    }

    /// `timeoutChat` from Java (`Client.java` 9152-9177): one tick of every
    /// live overhead bubble. Local player is the `i == -1` slot (Rust's
    /// `local_player`, not the stale `players[2047]` clone); then tracked
    /// players and NPCs. `chatTimer == 0` clears `chatMessage`.
    pub fn timeout_chat(&mut self) {
        if let Some(player) = self.local_player.as_mut() {
            Self::timeout_entity_chat(&mut player.entity);
        }
        for i in 0..self.player_count as usize {
            let index = self.player_ids[i] as usize;
            if let Some(player) = self.players.get_mut(index).and_then(|p| p.as_mut()) {
                Self::timeout_entity_chat(&mut player.entity);
            }
        }
        for i in 0..self.npc_count as usize {
            let index = self.npc_ids[i] as usize;
            if let Some(npc) = self.npc.get_mut(index).and_then(|n| n.as_mut()) {
                Self::timeout_entity_chat(&mut npc.entity);
            }
        }
    }

    fn timeout_entity_chat(e: &mut ClientEntity) {
        if e.chat_timer > 0 {
            e.chat_timer -= 1;
            if e.chat_timer == 0 {
                e.chat_message = None;
            }
        }
    }

    /// `moveNpcs` from Java (`Client.java` 10547): one movement step for
    /// every tracked NPC in `npcIds` order.
    pub fn move_npcs(&mut self) {
        for i in 0..self.npc_count as usize {
            let index = self.npc_ids[i] as usize;
            let Some(npc) = self.npc.get(index).and_then(|n| n.as_ref()) else {
                continue;
            };
            let mut e = npc.entity.clone();
            self.move_entity(false, &mut e);
            if let Some(slot) = self.npc.get_mut(index).and_then(|n| n.as_mut()) {
                slot.entity = e;
            }
        }
    }

    /// `moveEntity(e)` from Java (`Client.java` 10558): snap out-of-bounds
    /// entities back to their route head, then step an exact move
    /// (`exact_move_1` before it starts, `exact_move_2` during it) or a
    /// `route_move` walk, and finish with `entity_face` + `entity_anim`.
    /// `is_local` marks the live local player (the Java
    /// `arg0 == localPlayer` bounds check and the facing-self read).
    fn move_entity(&self, is_local: bool, e: &mut ClientEntity) {
        if e.x < 128 || e.z < 128 || e.x >= 13184 || e.z >= 13184 {
            e.primary_anim = -1;
            e.spotanim_id = -1;
            e.exact_move_start = 0;
            e.exact_move_end = 0;
            e.x = e.route_x[0] * 128 + e.size * 64;
            e.z = e.route_z[0] * 128 + e.size * 64;
            e.abort_route();
        }
        if is_local && (e.x < 1536 || e.z < 1536 || e.x >= 11776 || e.z >= 11776) {
            e.primary_anim = -1;
            e.spotanim_id = -1;
            e.exact_move_start = 0;
            e.exact_move_end = 0;
            e.x = e.route_x[0] * 128 + e.size * 64;
            e.z = e.route_z[0] * 128 + e.size * 64;
            e.abort_route();
        }
        // TS `Client.ts` 3508 branch order: the packet decode (TS naming)
        // assigns the first g2 to `exact_move_end`, so it is the earlier
        // cycle here — the Java `exactMoveStart` role.
        if e.exact_move_end > self.loop_cycle {
            self.exact_move_1(e);
        } else if e.exact_move_start >= self.loop_cycle {
            self.exact_move_2(e);
        } else {
            e.route_move(&self.cache);
        }
        self.entity_face(is_local, e);
        e.entity_anim(&self.cache, self.loop_cycle);
    }

    /// `exactMove1(e)` from Java (`Client.java` 10589): the pre-move phase
    /// creeps the entity toward the start tile as the move approaches.
    fn exact_move_1(&self, e: &mut ClientEntity) {
        let delta = e.exact_move_end - self.loop_cycle;
        let dst_x = e.exact_start_x * 128 + e.size * 64;
        let dst_z = e.exact_start_z * 128 + e.size * 64;
        e.x += (dst_x - e.x) / delta;
        e.z += (dst_z - e.z) / delta;
        e.anim_delay_move = 0;
        if e.exact_move_facing == 0 {
            e.dst_yaw = 1024;
        }
        if e.exact_move_facing == 1 {
            e.dst_yaw = 1536;
        }
        if e.exact_move_facing == 2 {
            e.dst_yaw = 0;
        }
        if e.exact_move_facing == 3 {
            e.dst_yaw = 512;
        }
    }

    /// `exactMove2(e)` from Java (`Client.java` 10610): the moving phase
    /// interpolates between the start and end tiles over the move duration.
    fn exact_move_2(&self, e: &mut ClientEntity) {
        if e.exact_move_start == self.loop_cycle
            || e.primary_anim == -1
            || e.primary_anim_delay != 0
            || e.primary_anim_cycle + 1
                > self
                    .cache
                    .seq(e.primary_anim as usize)
                    .get_delay(e.primary_anim_frame)
        {
            let duration = e.exact_move_start - e.exact_move_end;
            let delta = self.loop_cycle - e.exact_move_end;
            let start_x = e.exact_start_x * 128 + e.size * 64;
            let start_z = e.exact_start_z * 128 + e.size * 64;
            let end_x = e.exact_end_x * 128 + e.size * 64;
            let end_z = e.exact_end_z * 128 + e.size * 64;
            // Java divides by `duration` here; a zero-duration exact move
            // (fresh entity at loop_cycle 0) would throw where the client
            // cannot — skip the interpolation instead.
            if duration != 0 {
                e.x = (start_x * (duration - delta) + end_x * delta) / duration;
                e.z = (start_z * (duration - delta) + end_z * delta) / duration;
            }
        }
        e.anim_delay_move = 0;
        if e.exact_move_facing == 0 {
            e.dst_yaw = 1024;
        }
        if e.exact_move_facing == 1 {
            e.dst_yaw = 1536;
        }
        if e.exact_move_facing == 2 {
            e.dst_yaw = 0;
        }
        if e.exact_move_facing == 3 {
            e.dst_yaw = 512;
        }
        e.yaw = e.dst_yaw;
    }

    /// `entityFace(e)` from Java (`Client.java` 10755): resolve the face
    /// target — an NPC, a player, or the last loc-square click — into a
    /// `dst_yaw`, then turn `yaw` toward it at `turnspeed`, switching to
    /// the turn anim. The target positions are read while the entity is a
    /// detached clone, so the pass never aliases `&mut players[i]` with a
    /// read of `players[j]`.
    fn entity_face(&self, is_local: bool, e: &mut ClientEntity) {
        if e.turnspeed == 0 {
            return;
        }
        if e.face_entity != -1 && e.face_entity < 32768 {
            if let Some(npc) = self
                .npc
                .get(e.face_entity as usize)
                .and_then(|n| n.as_ref())
            {
                let dx = e.x - npc.x;
                let dz = e.z - npc.z;
                if dx != 0 || dz != 0 {
                    e.dst_yaw = ((dx as f64).atan2(dz as f64) * 325.949) as i32 & 0x7ff;
                }
            }
        }
        if e.face_entity >= 32768 {
            let mut index = e.face_entity - 32768;
            if index == self.self_slot {
                index = LOCAL_PLAYER_INDEX;
            }
            let target = if index == LOCAL_PLAYER_INDEX {
                if is_local {
                    // Java reads `players[LOCAL_PLAYER_INDEX]`, which is the
                    // moving entity itself here.
                    Some((e.x, e.z))
                } else {
                    self.local_player.as_ref().map(|local| (local.x, local.z))
                }
            } else {
                self.players
                    .get(index as usize)
                    .and_then(|p| p.as_ref())
                    .map(|player| (player.x, player.z))
            };
            if let Some((player_x, player_z)) = target {
                let dx = e.x - player_x;
                let dz = e.z - player_z;
                if dx != 0 || dz != 0 {
                    e.dst_yaw = ((dx as f64).atan2(dz as f64) * 325.949) as i32 & 0x7ff;
                }
            }
        }
        if (e.face_square_x != 0 || e.face_square_z != 0)
            && (e.route_length == 0 || e.anim_delay_move > 0)
        {
            let dx = e.x - (e.face_square_x - self.map_build_base_x - self.map_build_base_x) * 64;
            let dz = e.z - (e.face_square_z - self.map_build_base_z - self.map_build_base_z) * 64;
            if dx != 0 || dz != 0 {
                e.dst_yaw = ((dx as f64).atan2(dz as f64) * 325.949) as i32 & 0x7ff;
            }
            e.face_square_x = 0;
            e.face_square_z = 0;
        }
        let remaining = (e.dst_yaw - e.yaw) & 0x7ff;
        if remaining == 0 {
            return;
        }
        if remaining < e.turnspeed || remaining > 2048 - e.turnspeed {
            e.yaw = e.dst_yaw;
        } else if remaining > 1024 {
            e.yaw -= e.turnspeed;
        } else {
            e.yaw += e.turnspeed;
        }
        e.yaw &= 0x7ff;
        if e.secondary_anim != e.readyanim || e.yaw == e.dst_yaw {
            return;
        }
        if e.turnanim != -1 {
            e.secondary_anim = e.turnanim;
            return;
        }
        e.secondary_anim = e.walkanim;
    }

    /// `mainredraw` from Java — the frame render pass: title screen or
    /// in-game draw into `draw_area` (which the `window` feature presents).
    /// `draw` is the CPU-save switch: false skips the render entirely, so a
    /// headless bot burns no pixels while the network machine keeps running.
    /// Re-homed onto `Renderer` (task 2b); kept here as the doc anchor.
    pub fn set_draw(&mut self, draw: bool) {
        self.draw = draw;
        // `map_build` copies `draw` into `overlay_mesh` at rebuild time.
        // Live attach (panel `set_draw(true)` after an unheaded first
        // scene) must flip the same flag so later `set_ground` writes
        // overlay verts, not stamps-only.
        self.world.overlay_mesh = draw;
        // The minimap pixmap lives on the `Renderer`. Draw-off drops that
        // head; dirty the latch so the next attach recomposes from stamps.
        if !draw {
            self.minimap_level = -1;
        }
    }

    /// Bake `tex_average` from `{cache_dir}/textures` for this mem mode.
    /// `finish_build` reads it for textured overlay rgb; a headed paint
    /// also copies the renderer's table, but unheaded map_build runs first.
    pub fn load_tex_averages(&mut self) {
        self.tex_average =
            crate::graphics::Pix3DDraw::cached_averages(&self.config.cache_dir, self.config.lowmem);
    }

    /// Publish the host's nav-debug paint for this frame. Always stores —
    /// CpuPix3D and skip-paint slots never paint, the wgpu scene stage
    /// draws the stored paint after the 3D world.
    pub fn set_nav_debug_paint(&mut self, paint: Option<NavDebugPaint>) {
        self.nav_debug_paint = paint;
    }

    /// The nav-debug paint published for the current frame.
    pub fn nav_debug_paint(&self) -> Option<&NavDebugPaint> {
        self.nav_debug_paint.as_ref()
    }

    /// Flip the client's `lowmem` mode live (the panel's Music/SFX toggle):
    /// set `config.lowmem` — the single source of truth every lowmem gate
    /// already reads (sound synthesis, the 2D audio UI, player/model
    /// `low_mem`) — and, on the low→high edge, re-run the one-time sound
    /// load the lowmem spawn skipped (`unpack_jagfx` + the midi on-demand
    /// request, mirroring `maininit_with_progress` under `!lowmem`).
    /// Idempotent (early return when unchanged) so it can be called every
    /// frame; the full re-raster (`redraw_frame`, like the brightness
    /// path) makes the current scene and the 2D UI reflect the new mode.
    /// The login handshake is one-time (sent at login) and is not re-sent
    /// here; a later reconnect handshakes the new mode from
    /// `config.lowmem`.
    pub fn set_lowmem(&mut self, lowmem: bool) {
        if self.config.lowmem == lowmem {
            return;
        }
        self.config.lowmem = lowmem;
        self.load_tex_averages();
        if !lowmem {
            self.jagfx = Self::unpack_jagfx(&self.config.cache_dir, false);
            if let Some(od) = &mut self.on_demand {
                self.midi_song = 0;
                self.midi_fading = true;
                od.request(2, 0);
            }
        }
        self.redraw_frame = true;
    }

    /// Drive the 20 ms GameShell machine on the calling thread (spec §3):
    /// one `mainloop` then `mainredraw` per frame with the Java
    /// ratio/count catch-up. `on_loop` runs after each `mainloop` pass so a
    /// driver (client-play) can read Java-public state — e.g. print the
    /// local-player tile for live proof — without a snapshot API.
    ///
    /// The driver holds the `Renderer` beside the client and hands it in:
    /// `run` drives both the sim pass and the render pass.
    ///
    /// With a `PresentTarget` attached the target drives the frame: input is
    /// pumped into the shell before the mainloop pass (via `latch_click`,
    /// GameShell.ts 186-190), and the rendered frame is handed to the target
    /// after the redraw. A window target that reports closed (`poll` false)
    /// sets `shell.state = -1`, which stops the machine on the next iteration
    /// like Java `GameShell.run`.
    pub fn run<F: FnMut(&mut Self)>(&mut self, renderer: &mut Renderer, mut on_loop: F) {
        if !self.already_started {
            self.maininit_with_progress(Some(&mut |c, m, p| renderer.draw_progress(c, m, p)));
        }
        while self.shell.state >= 0 {
            if self.shell.state > 0 {
                self.shell.state -= 1;
                if self.shell.state == 0 {
                    self.shell.stop();
                    return;
                }
            }

            if let Some(present) = self.present.as_mut() {
                if !present.poll(&mut self.shell) {
                    self.shell.state = -1;
                }
            }

            let delta = self.shell.begin_frame();
            if delta > 0 {
                thread::sleep(Duration::from_millis(delta as u64));
            }

            while self.shell.count < 256 {
                self.shell.latch_click();
                self.mainloop();
                self.shell.key_queue_read = self.shell.key_queue_write;
                on_loop(self);
                self.shell.count += self.shell.ratio;
            }
            self.shell.count &= 0xff;
            self.shell.end_frame();

            let output = renderer.mainredraw(self);

            if let Some(present) = self.present.as_mut() {
                present.present(output);
            }
        }

        if self.shell.state == -1 {
            self.shell.stop();
        }
    }

    /// `saveMidi(fading, data)` from Java (`Client.java` 6266): hand the
    /// on-demand archive-2 bytes to the backend (signlink midisave). A
    /// jingle (`fading=false`), the first song, or a zone-song change after
    /// the current song already reached EOF plays immediately at the
    /// `midivol` ladder value (Java first-song short-circuit; midisave
    /// replaces the file now). A zone-song change (`fading=true`) while
    /// something is still playing holds the bytes pending and fades the
    /// current song out, swapping in `music_tick` once the ramp hits the
    /// floor.
    pub fn save_midi(&mut self, data: &[u8], fading: bool) {
        let mut midi = self.midi.lock().unwrap();
        if !fading || !self.midi_playing || !midi.is_playing() {
            if midi.play(data, self.midi_volume, fading) {
                self.fade.lock().unwrap().finish_fade(self.midi_volume);
                self.midi_playing = true;
                self.midi_pending = None;
            }
            // A rejected play keeps the old state: no gain restore, no
            // `midi_playing`, and any held pending swap survives.
        } else {
            self.midi_pending = Some((data.to_vec(), self.midi_volume));
            self.fade.lock().unwrap().fade_out();
        }
    }

    /// `musicTick`: once per mainloop pass, swap the pending zone song in
    /// when the fade-out ramp has reached the floor (Java `midisave` starts
    /// the new song at the current `midivol`, no fade-in). The latch is only
    /// advanced by the audio callback, so headless there is no swap.
    pub fn music_tick(&mut self) {
        if !self.fade.lock().unwrap().swap_due() {
            return;
        }
        if let Some((data, volume)) = self.midi_pending.take() {
            if self.midi.lock().unwrap().play(&data, volume, false) {
                self.fade.lock().unwrap().finish_fade(volume);
            }
            // A rejected swap-in leaves the fade at the floor: the new song
            // never started, so the old song must not come back at volume.
        }
    }

    /// `stopMidi()` from Java (`Client.java` 6272): clear the fade flag and
    /// stop the backend (signlink `midi = "stop"`). The fade hard-cuts and
    /// any pending zone-song swap is dropped.
    pub fn stop_midi(&mut self) {
        self.midi_fading = false;
        self.fade.lock().unwrap().stop_hard();
        self.midi.lock().unwrap().stop();
        self.midi_pending = None;
        self.midi_playing = false;
    }

    /// `setMidiVolume(active, volume)` from Java (`Client.java` 7712): store
    /// the 274 `midivol` and retarget the output fade when the music plane
    /// is live (signlink `midi = "voladjust"`). The ladder is the single
    /// gain path — the backend's `set_volume` is a documented no-op.
    pub fn set_midi_volume(&mut self, active: bool, volume: i32) {
        self.midi_volume = volume;
        if active {
            self.fade.lock().unwrap().set_target_vol(volume);
        }
    }

    /// `clientVar(id)` from client-ts (`Client.ts` 10601): look up the varp's
    /// clientcode and apply it. Only the music clientcode 3 is ported here;
    /// the varbit/stat mapping and the colour-table (1) / wave (4) codes land
    /// with the varp/wave tasks. No-op when the varp table is not loaded.
    pub fn client_var(&mut self, id: i32) {
        let Some(varp) = self.cache.varps.get(id as usize) else {
            return;
        };
        let value = self.var.get(id as usize).copied().unwrap_or(0);
        self.apply_clientcode(varp.clientcode, value);
    }

    /// The clientcode switch from `clientVar` (Java `Client.java` 3032 /
    /// TS 10608-10684). 1 brightness, 3 midi, 4 wave, 5 one-mouse, 6 chat
    /// effects, 8 split-private, 9 bank-arrange.
    pub fn apply_clientcode(&mut self, clientcode: i32, value: i32) {
        if clientcode == 0 {
            return;
        }
        if clientcode == 1 {
            let brightness = match value {
                1 => 0.9,
                2 => 0.8,
                3 => 0.7,
                4 => 0.6,
                _ => return,
            };
            Pix3D::init_colour_table(brightness);
            // The texture-palette half of the brightness change runs on the
            // renderer (the texels are its state); defer it to the next
            // `game_draw` via `pending_brightness`.
            self.pending_brightness = Some(brightness);
            ObjType::clear_sprite_cache();
            self.redraw_frame = true;
            return;
        }
        if clientcode == 3 {
            let last_midi_active = self.midi_active;
            match value {
                0 => {
                    self.set_midi_volume(self.midi_active, 0);
                    self.midi_active = true;
                }
                1 => {
                    self.set_midi_volume(self.midi_active, -400);
                    self.midi_active = true;
                }
                2 => {
                    self.set_midi_volume(self.midi_active, -800);
                    self.midi_active = true;
                }
                3 => {
                    self.set_midi_volume(self.midi_active, -1200);
                    self.midi_active = true;
                }
                4 => {
                    self.midi_active = false;
                }
                _ => {}
            }
            if self.midi_active != last_midi_active && !self.config.lowmem {
                if self.midi_active {
                    self.midi_song = self.next_midi_song;
                    self.midi_fading = true;
                    if let Some(od) = &mut self.on_demand {
                        od.request(2, self.midi_song);
                    }
                } else {
                    self.stop_midi();
                }
                self.next_music_delay = 0;
            }
            return;
        }
        if clientcode == 4 {
            match value {
                0 => {
                    self.wave_volume = 0;
                    self.wave_enabled = true;
                }
                1 => {
                    self.wave_volume = -400;
                    self.wave_enabled = true;
                }
                2 => {
                    self.wave_volume = -800;
                    self.wave_enabled = true;
                }
                3 => {
                    self.wave_volume = -1200;
                    self.wave_enabled = true;
                }
                4 => {
                    self.wave_enabled = false;
                }
                _ => {}
            }
            return;
        }
        if clientcode == 5 {
            self.one_mouse_button = value;
            return;
        }
        if clientcode == 6 {
            self.chat_effects = value;
            return;
        }
        if clientcode == 8 {
            self.split_private_chat = value;
            self.redraw_chat = true;
            return;
        }
        if clientcode == 9 {
            self.bank_arrange_mode = value;
        }
    }

    /// `soundsDoQueue()` from client-ts (`Client.ts` 3413): drain the wave
    /// queue, generating each WAV through `JagFX` and pushing its 8-bit PCM
    /// as i16 samples onto the mixer's wave queue (the `AudioOut` callback
    /// drains that). A missing sound id skips silently, as TS does.
    pub fn sounds_do_queue(&mut self) {
        let mut wave = 0usize;
        while wave < self.wave_count as usize {
            if self.wave_delay[wave] <= 0 {
                let id = self.wave_ids[wave];
                let loops = self.wave_loops[wave];
                if let Some(wav) = self.jagfx.generate(id, loops) {
                    let data = wav.data();
                    let end = wav.pos;
                    // Convert off the lock: a looped generate can be up to
                    // 20 s of samples, and the audio callback needs the
                    // queue lock every buffer.
                    let mut samples = Vec::with_capacity(end - 44);
                    for &b in &data[44..end] {
                        // 8-bit WAV PCM (128 = silence) → full-range i16
                        samples.push(((b as i16) - 128) << 8);
                    }
                    let mut queue = self.waves.lock().unwrap();
                    // 20 s at 22050 Hz, the TS `JagFX.waveBytes` scratch:
                    // any one sound fits, and it bounds the queue when no
                    // output device is draining it.
                    const WAVE_QUEUE_SAMPLES: usize = 22050 * 20;
                    let room = WAVE_QUEUE_SAMPLES.saturating_sub(queue.len());
                    if samples.len() > room {
                        samples.truncate(room);
                    }
                    queue.extend(samples);
                }
                self.wave_count -= 1;
                for i in wave..self.wave_count as usize {
                    self.wave_ids[i] = self.wave_ids[i + 1];
                    self.wave_loops[i] = self.wave_loops[i + 1];
                    self.wave_delay[i] = self.wave_delay[i + 1];
                }
            } else {
                self.wave_delay[wave] -= 1;
                wave += 1;
            }
        }
        // `nextMusicDelay` countdown from Java `soundsDoQueue` (`Client.java`
        // 1997-2008): a jingle's `MIDI_JINGLE` delay ticks down 20 ms per
        // pass; at zero the next zone song is re-requested with a fade.
        if self.next_music_delay > 0 {
            self.next_music_delay -= 20;
            if self.next_music_delay < 0 {
                self.next_music_delay = 0;
            }
            if self.next_music_delay == 0 && self.midi_active && !self.config.lowmem {
                self.midi_song = self.next_midi_song;
                self.midi_fading = true;
                if let Some(od) = &mut self.on_demand {
                    od.request(2, self.midi_song);
                }
            }
        }
    }

    /// `onDemandLoop()` from client-ts: run the ondemand heartbeat, then
    /// dispatch every completed file by the TS archive cases — models (0),
    /// anim frames (1), midi (2), map squares (3) — and the archive-93 map
    /// prefetch completions. Null data skips the dispatch as TS does.
    pub fn on_demand_loop(&mut self) -> usize {
        let mut done = Vec::new();
        if let Some(od) = &mut self.on_demand {
            od.run(self.ingame);
            while let Some(req) = od.loop_request() {
                done.push(req);
            }
        }
        let n = done.len();
        for req in done {
            let Some(data) = req.data else {
                if req.archive == 3 {
                    self.error_loading = true;
                    self.shell.set_framerate(1);
                }
                continue;
            };
            match req.archive {
                0 => {
                    Model::unpack(req.file, Some(data.as_slice()));
                    if self
                        .on_demand
                        .as_ref()
                        .is_some_and(|od| od.get_model_use(req.file) & 0x62 != 0)
                    {
                        self.redraw_side = true;
                    }
                }
                1 => AnimFrame::unpack(&data),
                2 => {
                    if self.midi_song == req.file {
                        self.save_midi(&data, self.midi_fading);
                    }
                }
                3 => self.fill_map_build_square(req.file, data),
                93 => {
                    // archive 93 carries finished map-location prefetches;
                    // decode the loc stream and prefetch each loc's models
                    // (TS `onDemandLoop` 1370-1373).
                    if let Some(od) = self.on_demand.as_mut() {
                        if od.has_map_loc_file(req.file) {
                            ClientBuild::prefetch_locations(
                                &self.cache,
                                &mut Packet::new(data),
                                od,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        n
    }

    /// The TS archive-3 case of `onDemandLoop`: park a finished map square
    /// into the ground or location slot that requested its file.
    fn fill_map_build_square(&mut self, file: i32, data: Vec<u8>) {
        if self.scene_state != 1
            || self.map_build_ground_data.len() != self.map_build_location_data.len()
        {
            return;
        }
        for i in 0..self.map_build_ground_data.len() {
            if self.map_build_ground_file[i] == file {
                if crate::render_debug_enabled() {
                    eprintln!(
                        "[client-map] ground file={file} slot={i} mapsquare=({},{})",
                        self.map_build_index[i] >> 8,
                        self.map_build_index[i] & 0xff
                    );
                }
                self.map_build_ground_data[i] = Some(data);
                return;
            }
            if self.map_build_location_file[i] == file {
                if crate::render_debug_enabled() {
                    eprintln!(
                        "[client-map] loc file={file} slot={i} mapsquare=({},{})",
                        self.map_build_index[i] >> 8,
                        self.map_build_index[i] & 0xff
                    );
                }
                self.map_build_location_data[i] = Some(data);
                return;
            }
        }
    }

    /// Bound for `in.pos < psize` loops. Over the socket `in` is the 5000-byte
    /// alloc so `psize` is the frame; tests that skip the socket use the
    /// payload length when `psize` is unset.
    fn inbound_end(&self, payload: &Packet) -> usize {
        let psize = self.psize as usize;
        if psize > 0 && psize <= payload.length() {
            psize
        } else {
            payload.length()
        }
    }

    /// One `UPDATE_INV_*` slot: `g2` id + `g1` count, with `255` promoting
    /// to `g4` (TS `UPDATE_INV_FULL` / `UPDATE_INV_PARTIAL`).
    fn read_inv_count(payload: &mut Packet) -> (i32, i32) {
        let id = payload.g2();
        let mut count = payload.g1();
        if count == 255 {
            count = payload.g4();
        }
        (id, count)
    }
}

fn io_error() -> LoginError {
    LoginError {
        code: -1,
        mes1: String::new(),
        mes2: "Error connecting to server.".into(),
    }
}

/// `Client.CHARSET` from client-ts (118): the characters the title fields
/// accept. UTF-8 source is fine — the literal is the TS string verbatim.
const TITLE_CHARSET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!\"£$%^&*()-_=+[{]};:'@#~,<.>/?\\| ";

/// The TS title button hit test: a left click within ±75×±20 of the
/// button's centre (the `titlebutton` sprite is 150×40).
fn title_button_clicked(button: i32, x: i32, y: i32, centre_x: i32, centre_y: i32) -> bool {
    button == 1
        && x >= centre_x - 75
        && x <= centre_x + 75
        && y >= centre_y - 20
        && y <= centre_y + 20
}

/// `Client.levelExperience` from client-ts: cumulative XP thresholds for
/// levels 1..99, computed in the TS static initializer.
pub(crate) fn level_experience() -> &'static [i32; 99] {
    static TABLE: OnceLock<[i32; 99]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0i32; 99];
        let mut acc: i32 = 0;
        for (i, entry) in table.iter_mut().enumerate() {
            let level = (i + 1) as f64;
            // TS `(level + Math.pow(2.0, level / 7.0) * 300.0) | 0`
            let delta = (level + 2.0f64.powf(level / 7.0) * 300.0) as i32;
            acc += delta;
            // TS `(acc / 4) | 0`
            *entry = acc / 4;
        }
        table
    })
}

/// Grow-on-write store matching the TS plain-array assignment `var[id] = v`
/// (plain arrays extend on out-of-range writes).
fn grow_write(table: &mut Vec<i32>, id: i32, value: i32) {
    let index = id as usize;
    if index >= table.len() {
        table.resize(index + 1, 0);
    }
    table[index] = value;
}

/// Stand-in for Java `(int)(Math.random() * 99999999)`: a non-negative
/// value below 100_000_000 for the login Isaac seed.
fn login_random() -> i32 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    let r = x.wrapping_mul(0x2545_f491_4f6c_dd1d);
    ((r >> 32) % 99_999_999) as i32
}

/// Per-client login uid for the 274 handshake RSA block (Java 274
/// `loginUid`): clock nanos XOR an `AtomicU64` (like `login_random`), retried
/// if the mix lands on 0 or the old shared `1337` constant. The counter
/// advances on every attempt so concurrent `Client::new` cannot collide on
/// the same tick and a 0/1337 retry cannot livelock.
fn login_uid() -> i32 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let mut x = now ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
        x ^= x >> 16;
        x = x.wrapping_mul(0x7feb_352d);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846c_a68b);
        x ^= x >> 16;
        let uid = x as i32;
        if uid != 0 && uid != 1337 {
            return uid;
        }
    }
}

#[cfg(test)]
mod zone_post_build {
    use super::*;

    #[test]
    fn loc_change_post_build_keeps_permanent_and_drops_timed() {
        let mut c = Client::new(ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        });
        c.loc_changes.push(LocChange {
            end_time: -1,
            x: 2,
            z: 2,
            ..LocChange::default()
        });
        c.loc_changes.push(LocChange {
            end_time: 5,
            x: 3,
            z: 3,
            ..LocChange::default()
        });
        c.loc_change_post_build();
        let mut n = 0;
        if c.loc_changes.head().is_some() {
            n += 1;
        }
        while c.loc_changes.next_node().is_some() {
            n += 1;
        }
        assert_eq!(n, 1);
        assert_eq!(c.loc_changes.head().unwrap().start_time, 0);
    }
}

#[cfg(test)]
mod try_move_path {
    use super::*;

    #[test]
    fn try_move_tiles_records_every_step_src_to_dest() {
        // src (0,0) → dest (3,0) walking east: reverse dir on dest cells is WEST.
        let mut dir_map = vec![0; (BUILD_AREA_SIZE * BUILD_AREA_SIZE) as usize];
        for x in 1..=3 {
            dir_map[(x * BUILD_AREA_SIZE) as usize] = DirectionFlag::WEST;
        }
        dir_map[0] = 99;
        let path = Client::try_move_tiles(&dir_map, 3, 0, 0, 0);
        assert_eq!(path, vec![(0, 0), (1, 0), (2, 0), (3, 0)]);
    }

    #[test]
    fn try_move_tiles_records_a_long_bfs_not_nine() {
        // Entity walk buffer is 9; MOVE waypoints cap at 25. The paint
        // trail is every BFS tile (a 20-step click is 21 tiles).
        let mut dir_map = vec![0; (BUILD_AREA_SIZE * BUILD_AREA_SIZE) as usize];
        let dest = 20;
        for x in 1..=dest {
            dir_map[(x * BUILD_AREA_SIZE) as usize] = DirectionFlag::WEST;
        }
        dir_map[0] = 99;
        let path = Client::try_move_tiles(&dir_map, dest, 0, 0, 0);
        assert_eq!(path.len(), dest as usize + 1);
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(dest, 0)));
    }

    fn empty_client() -> Client {
        Client::new(ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        })
    }

    #[test]
    fn run_enabled_reads_varp_173() {
        let mut c = empty_client();
        assert!(!c.run_enabled());
        if c.var.len() <= Client::RUN_VARP {
            c.var.resize(Client::RUN_VARP + 1, 0);
        }
        c.var[Client::RUN_VARP] = 1;
        assert!(c.run_enabled());
    }

    #[test]
    fn run_enabled_reads_visible_run_off_orb() {
        // 274 controls overlay: visible off-orb (152) + hidden on-orb (153)
        // means run is on (host `run_echo`). hide is per-client mut, so it
        // seeds the overlay (`set_iface` puts a default overlay beside the
        // decode slot).
        let mut c = empty_client();
        c.set_iface(152, IfType::default());
        c.set_iface(153, IfType::default());
        c.iface_mut(152).unwrap().hide = false;
        c.iface_mut(153).unwrap().hide = true;
        assert!(c.run_enabled());
        c.iface_mut(152).unwrap().hide = true;
        c.iface_mut(153).unwrap().hide = false;
        assert!(!c.run_enabled());
    }
}

#[cfg(test)]
mod dead_server_watchdog {
    use super::*;

    /// A config pointed at a port nothing is listening on, so a watchdog
    /// `lostCon` → re-login gets an immediate `ECONNREFUSED` (no hang, no
    /// real server involved).
    fn client_at_dead_port() -> Client {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        Client::new(ClientConfig {
            host: "127.0.0.1".into(),
            port,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: true,
        })
    }

    #[test]
    fn stale_last_response_fires_lost_con_in_one_game_loop_pass() {
        let mut c = client_at_dead_port();
        c.ingame = true;
        c.last_response = Some(Instant::now() - Duration::from_secs(16));
        // One pass (the cadence of a parked host slot) must fire the
        // watchdog — the wall-clock fix: 750 pass-counted frames at one
        // pass per ~600 ms would have taken ~450 s instead.
        c.game_loop();
        assert!(
            !c.ingame,
            "a dead server must drop the connection in one pass"
        );
    }

    #[test]
    fn fresh_last_response_does_not_trip_the_watchdog() {
        let mut c = client_at_dead_port();
        c.ingame = true;
        c.last_response = Some(Instant::now());
        c.game_loop();
        assert!(c.ingame, "a live server must not trip the watchdog");
    }

    #[test]
    fn watchdog_stays_quiet_until_the_bound_elapses() {
        let mut c = client_at_dead_port();
        c.ingame = true;
        c.last_response = Some(Instant::now() - Duration::from_secs(14));
        c.game_loop();
        assert!(c.ingame, "14 s (< 15 s bound) must not trip the watchdog");
    }

    #[test]
    fn reading_a_full_packet_restamps_the_watchdog() {
        use std::io::Write;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut c = client_at_dead_port();
        let stream = ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap();
        c.stream = Some(stream);
        c.ptype = -1;
        c.ingame = true;
        c.last_response = Some(Instant::now() - Duration::from_secs(30));
        let (mut server, _) = listener.accept().unwrap();
        // UPDATE_REBOOT_TIMER: one header byte + two payload bytes.
        server.write_all(&[89, 0, 10]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            c.tcp_in();
            if c.last_response
                .is_some_and(|t| t.elapsed() < Duration::from_secs(5))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "a full packet never restamped the watchdog"
            );
        }
    }
}

#[cfg(test)]
mod audio_toggle {
    use super::*;
    use std::io::Write;

    /// A `sounds` jag with one `sounds.dat` entry (sound id 0, ten empty
    /// tones), so `unpack_jagfx` yields a non-empty table. Stored in a
    /// per-process temp dir; no `versionlist`, so `load_on_demand` stays
    /// `None` and nothing touches the network.
    fn sounds_cache_dir() -> std::path::PathBuf {
        let mut dat = Vec::new();
        dat.extend_from_slice(&0u16.to_be_bytes()); // sound id 0
        dat.extend_from_slice(&[0u8; 10]); // ten empty tones
        dat.extend_from_slice(&0u16.to_be_bytes()); // loop_begin
        dat.extend_from_slice(&0u16.to_be_bytes()); // loop_end
        dat.extend_from_slice(&0xffffu16.to_be_bytes()); // end of table
        let dir =
            std::env::temp_dir().join(format!("274bot-client-audio-toggle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sounds"), jag(&[("sounds.dat", &dat)])).unwrap();
        dir
    }

    fn lowmem_client_with_sounds() -> Client {
        let dir = sounds_cache_dir();
        Client::from_shared(
            ClientConfig {
                host: "127.0.0.1".into(),
                port: 43594,
                cache_dir: dir.to_str().unwrap().into(),
                members: true,
                lowmem: true,
            },
            Arc::new(Cache::default()),
            Arc::new(Vec::new()),
            Vec::new(),
        )
    }

    fn synth_sound(client: &mut Client, sound_id: i32) {
        let mut payload = Packet::alloc(0);
        payload.p2(sound_id);
        payload.p1(1);
        payload.p2(0);
        payload.pos = 0;
        client.handle_packet(ServerProt::SYNTH_SOUND, &mut payload);
    }

    fn midi_song(client: &mut Client, song_id: i32) {
        let mut payload = Packet::alloc(0);
        payload.p2(song_id);
        payload.pos = 0;
        client.handle_packet(ServerProt::MIDI_SONG, &mut payload);
    }

    #[test]
    fn set_lowmem_false_loads_sound_and_opens_gates() {
        let mut c = lowmem_client_with_sounds();
        // Spawned lowmem: table empty, both gates closed.
        assert!(c.jagfx.synth.iter().all(|s| s.is_none()));
        synth_sound(&mut c, 0);
        assert_eq!(c.wave_count, 0, "lowmem must gate SYNTH_SOUND");
        midi_song(&mut c, 7);
        assert_eq!(c.midi_song, -1, "lowmem must gate MIDI_SONG");
        assert!(c.config.lowmem);

        // Toggle on (highmem) live — no respawn.
        c.set_lowmem(false);
        assert!(!c.config.lowmem);
        assert!(
            c.jagfx.synth.iter().any(|s| s.is_some()),
            "set_lowmem(false) must load the JagFX table the lowmem spawn skipped"
        );
        assert!(c.redraw_frame, "the mode flip must re-raster");
        // Idempotent: a repeated unchanged call stays quiet.
        c.redraw_frame = false;
        c.set_lowmem(false);
        assert!(!c.redraw_frame, "set_lowmem must be idempotent");

        synth_sound(&mut c, 0);
        assert_eq!(c.wave_count, 1, "highmem must accept SYNTH_SOUND");
        midi_song(&mut c, 8);
        assert_eq!(c.midi_song, 8, "highmem must accept MIDI_SONG");

        // Toggle back off (lowmem): the gates close again.
        c.set_lowmem(true);
        assert!(c.config.lowmem);
        synth_sound(&mut c, 0);
        assert_eq!(c.wave_count, 1, "lowmem must gate SYNTH_SOUND again");
    }

    /// Pack a JAG container with bz2-compressed payloads (the shape the
    /// real `/crc`-fetched pack files take on disk).
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
