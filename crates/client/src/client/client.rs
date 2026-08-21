//! Client machine: 1:1 skeleton of `webclient/src/client/Client.ts`.
//!
//! Java-public fields used by login and later `RawClient`-style reads start
//! here (`ingame`, `loop_cycle` as an instance field, `loginUser`, `loginPass`,
//! `out`, `in`, menu arrays). `login` runs the 274 handshake (wrapper opcode
//! 16 cold / 18 reconnect) over Java-style TCP `ClientStream`.
//! There is no snapshot/query API.

use std::collections::HashMap;
use std::io;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use num_bigint::BigUint;

use crate::client::client_build::ClientBuild;
use crate::client::client_draw::{draw_detail, get_av_h};
use crate::client::config::ClientConfig;
use crate::client::game_shell::GameShell;
use crate::client::login_error::LoginError;
use crate::client::mini_menu_action::MiniMenuAction;
use crate::client::skill::Skill;
use crate::config::if_type::{ButtonType, ComponentType, IfType};
use crate::config::seq_type::{RESTART_RESET, RESTART_RESETLOOP};
use crate::config::Cache;
use crate::dash3d::world::LevelHeightmaps;
use crate::dash3d::{
    AnimFrame, BuildArea, ClientLocAnim, ClientObj, ClientProj, CollisionFlag, CollisionMap,
    DirectionFlag, LocAngle, LocChange, LocLayer, LocShape, MapFlag, MapSpotAnim, Model,
    SceneModel, World, LOC_SHAPE_TO_LAYER,
};
pub use crate::dash3d::{ClientNpc, ClientPlayer};
use crate::datastruct::LinkList;
use crate::graphics::{Colour, Pix2D, Pix3D, Pix32, Pix3DDraw, Pix8, PixFont, PixMap};
use crate::io::{
    ClientProt, ClientStream, Isaac, JagFile, OnDemand, Packet, ServerProt, SERVER_PROT_SIZES,
};
use crate::login_rsa::{LOGIN_RSAE, LOGIN_RSAN};
use crate::sound::{Fade, JagFX, Midi};
use crate::util::JString;

const MAX_PLAYER_COUNT: usize = 2048;
const MAX_NPC_COUNT: usize = 16384;
const MENU_CAPACITY: usize = 500;
const CLIENT_VERSION: i32 = 274;
const LOGIN_UID: i32 = 1337;

/// Client code of the red "Click here to logout" control; `clientButton`
/// arms `logoutTimer` (Java `Client.java` 8746).
const CC_LOGOUT: i32 = 205;

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

/// `groundObj` grid from client-ts (`new Array(4)` of `new Array(104)` of
/// null rows), every cell `None`. Assembled through `Vec` because the
/// `array::from_fn` / const-repeat forms materialize the 3.8 MB grid in a
/// stack temporary, which overflows the 2 MB test-thread stack. The flat
/// row form also avoids the `levels.push(*level)` by-value argument (a
/// 930 KB stack copy) that kept `Client::new` within ~16 KB of the same
/// limit.
fn empty_ground_obj() -> Box<[[[Option<LinkList<ClientObj>>; 104]; 104]; 4]> {
    let mut rows: Vec<[Option<LinkList<ClientObj>>; 104]> = Vec::with_capacity(104 * 4);
    for _ in 0..104 * 4 {
        rows.push([const { None }; 104]);
    }
    // Length-checked box of 416 rows, then re-grouped as 4 levels of 104
    // rows each. `[[T; 104]; 416]` and `[[[T; 104]; 104]; 4]` have the same
    // size and alignment, so the sole-owner allocation can be re-typed
    // without copying.
    let boxed: Box<[[Option<LinkList<ClientObj>>; 104]; 416]> = rows
        .into_boxed_slice()
        .try_into()
        .map_err(|_| ())
        .unwrap();
    // SAFETY: the box holds exactly 416 row arrays, which is the same
    // memory (size, alignment, cell layout) as 4 levels of 104 rows; the
    // re-typed box keeps sole ownership and its drop glue walks the same
    // cells.
    unsafe {
        Box::from_raw(Box::into_raw(boxed) as *mut [[[Option<LinkList<ClientObj>>; 104]; 104]; 4])
    }
}

/// Period 274 applet size (engine `bot.html` canvas and title.dat).
/// The title JPEG / in-game chrome are 765×503; 789×532 was leftover
/// webclient padding around that art.
pub const APPLET_W: i32 = 765;
pub const APPLET_H: i32 = 503;

pub struct Client {
    pub shell: GameShell,
    /// The `--window` applet (`Present`), opened by the driver. `run` polls
    /// it for events each frame and blits `draw_area` after the redraw; a
    /// closed window sets `shell.state = -1` to stop the machine. Headless
    /// builds keep this `None`.
    #[cfg(feature = "window")]
    pub present: Option<crate::client::present::Present>,
    pub config: ClientConfig,
    /// Config type tables (`obj`, `npc`, `loc`, ...), unpacked from the
    /// `config` jag by `Cache::unpack`; empty until loaded.
    pub cache: Cache,

    pub ingame: bool,
    /// `draw`: CPU-save switch — when false, `mainredraw` skips the frame
    /// render. Independent of the window: `client-play` sets it true after
    /// `Present::open`; headless bots keep it false.
    pub draw: bool,
    pub scene_state: i32,
    /// `Client.buildMinusedlevel` from client-ts: the `minusedlevel` the
    /// current scene was built for. `check_minimap`'s low-memory rebuild
    /// compares it against `self.minusedlevel`.
    pub build_minusedlevel: i32,
    pub local_player: Option<ClientPlayer>,
    pub players: Vec<Option<ClientPlayer>>,
    pub npc: Vec<Option<ClientNpc>>,

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
    /// here and copied into `world` (Task 16's `ClientBuild` owns the write
    /// side and will reconcile the sharing).
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
    pub ground_obj: Box<[[[Option<LinkList<ClientObj>>; 104]; 104]; 4]>,
    pub loc_changes: LinkList<LocChange>,
    pub projectiles: LinkList<ClientProj>,
    pub spotanims: LinkList<MapSpotAnim>,
    /// `selfSlot`/`membersAccount` from `UPDATE_PID`; `worldUpdateNum`
    /// counts `gameLoop` passes while drawing (zeroed when not drawing and
    /// at the end of `game_draw`); `cycleLogic1` is the loc-change scene
    /// cycle counter. Slice 2.
    pub self_slot: i32,
    pub members_account: i32,
    pub world_update_num: i32,
    pub cyclelogic1: i32,

    pub stat_base_level: Vec<i32>,
    pub stat_effective_level: Vec<i32>,
    pub stat_xp: Vec<i32>,
    pub var: Vec<i32>,
    /// Server-authoritative var values (`varServ` from client-ts); `var`
    /// follows them once `VARP_SYNC` confirms.
    pub var_serv: Vec<i32>,
    pub runenergy: i32,

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

    pub dir_map: Vec<i32>,
    pub dist_map: Vec<i32>,
    pub route_x: Vec<i32>,
    pub route_z: Vec<i32>,
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
    pub target_com_id: i32,
    pub obj_com_id: i32,
    pub obj_selected_slot: i32,
    pub obj_selected_com_id: i32,

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

    pub login_user: String,
    pub login_pass: String,
    pub login_mes1: String,
    pub login_mes2: String,
    pub loop_cycle: i32,

    /// `lastProgressPercent`/`lastProgressMessage` from client-ts (144-145):
    /// the most recent `draw_progress` values, readable even headless.
    /// `http_port` is the web-origin port of the later HTTP jag fetch (TS
    /// `getJagChecksums` downloads `/crc` from it); default 80, tests that
    /// stub HTTP set it.
    pub last_progress_percent: i32,
    pub last_progress_message: String,
    pub http_port: u16,
    /// `alreadyStarted` from client-ts: set at the start of `maininit`;
    /// a second `maininit` call is a no-op.
    pub already_started: bool,
    /// Base wait between `maininit` HTTP retries (TS `getJagChecksums`/
    /// `getJagFile` start at 5 s and double to a 60 s cap). Tests that
    /// stub HTTP set it small so retry paths do not sleep.
    pub fetch_retry_wait: Duration,

    /// Title screen state (`Client.ts` `prepareTitle`/`titleScreenDraw`):
    /// the 765×503 CPU framebuffer every frame draws into (`drawArea`), the
    /// `title` jag with the fonts and sprites, the 9 title `PixMap` regions
    /// (0/1 are the flame frames — empty here, `TitleFlames` is out of
    /// scope), and the login UI fields.
    pub draw_area: PixMap,
    /// Per-client Pix3D raster state (TS `Pix3D` mutable statics: the
    /// `scanline`, `originX/Y`, `trans`, `cycle`, and the texture pool).
    /// `Pix3D::init_colour_table` stays process-wide; the 3D pass binds
    /// `area_game` to it via `set_clipping` before `World::render_all`.
    pub pix3d: Pix3DDraw,
    pub title: Option<JagFile>,
    pub p11: Option<PixFont>,
    pub p12: Option<PixFont>,
    pub b12: Option<PixFont>,
    pub q8: Option<PixFont>,
    pub image_title0: Option<PixMap>,
    pub image_title1: Option<PixMap>,
    pub image_title2: Option<PixMap>,
    pub image_title3: Option<PixMap>,
    pub image_title4: Option<PixMap>,
    pub image_title5: Option<PixMap>,
    pub image_title6: Option<PixMap>,
    pub image_title7: Option<PixMap>,
    pub image_title8: Option<PixMap>,
    pub image_titlebox: Option<Pix8>,
    pub image_titlebutton: Option<Pix8>,
    /// `imageRunes` from client-ts: the 12 rune sprites the title flames
    /// animate (loaded with `fl_icon` default 0 → sprites 0..11).
    pub image_runes: Vec<Pix8>,
    /// `titleFlames` from client-ts: the torch animation over imageTitle0/1.
    pub title_flames: Option<crate::client::title_flames::TitleFlames>,
    pub loginscreen: i32,
    pub login_select: i32,
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
    /// `sceneCycle` from client-ts: bumped every `gameDrawMain`; the
    /// tile-occupancy stamps in `add_players`/`add_npcs` compare against it.
    pub scene_cycle: i32,
    /// Last `scene_cycle` that claimed each tile (`tileLastOccupiedCycle`,
    /// flat `SIZE * SIZE`), so a second entity on a tile defers to the first
    /// this cycle.
    pub tile_last_occupied_cycle: Vec<i32>,
    /// `World.resetVisCalc` has populated `vis_backing` for this client
    /// (TS loadGame calls it once per game load; `game_draw_main` runs it
    /// lazily on the first 3D frame).
    pub vis_calc_done: bool,

    /// In-game draw state (`Client.ts` `prepareGame`/`gameDraw`): the
    /// viewport/side/chat/chrome `PixMap` areas and the `media` jag sprites
    /// that plot into them, plus the chat-mode redraw flag and the three
    /// chat mode settings. Lazy-allocated on the first `game_draw`; a
    /// missing `media` pack leaves the sprites `None` and the areas black.
    pub area_game: Option<PixMap>,
    pub area_map: Option<PixMap>,
    pub area_side: Option<PixMap>,
    pub area_chat: Option<PixMap>,
    pub area_backleft1: Option<PixMap>,
    pub area_backleft2: Option<PixMap>,
    pub area_backright1: Option<PixMap>,
    pub area_backright2: Option<PixMap>,
    pub area_backtop1: Option<PixMap>,
    pub area_backvmid1: Option<PixMap>,
    pub area_backvmid2: Option<PixMap>,
    pub area_backvmid3: Option<PixMap>,
    pub area_backhmid2: Option<PixMap>,
    pub area_backbase1: Option<PixMap>,
    pub area_backbase2: Option<PixMap>,
    pub area_backhmid1: Option<PixMap>,
    pub invback: Option<Pix8>,
    pub chatback: Option<Pix8>,
    pub backbase1: Option<Pix8>,
    pub backbase2: Option<Pix8>,
    pub backhmid1: Option<Pix8>,
    /// `sideicons`/`redstone1..2hv` from client-ts (1013, 1068-1093): the
    /// side-tab sprites and the redstone highlight for `active_icon`. The
    /// `*h`/`*v`/`*hv` copies are the same sprites hflip/vflip'd. Missing
    /// sprites are `None` (a cache without the `media` pack).
    pub sideicons: [Option<Pix8>; 13],
    pub redstone1: Option<Pix8>,
    pub redstone2: Option<Pix8>,
    pub redstone3: Option<Pix8>,
    pub redstone1h: Option<Pix8>,
    pub redstone2h: Option<Pix8>,
    pub redstone1v: Option<Pix8>,
    pub redstone2v: Option<Pix8>,
    pub redstone3v: Option<Pix8>,
    pub redstone1hv: Option<Pix8>,
    pub redstone2hv: Option<Pix8>,
    pub redraw_icons: bool,
    pub redraw_chat: bool,
    pub redraw_chat_mode: bool,
    /// Depacked `TYPE_GRAPHIC` sprites (`IfType.graphic`/`graphic2` from
    /// client-ts), keyed by the `"name,index"` of `graphic_name`/
    /// `graphic2_name`. Filled lazily from `{cache_dir}/media` by
    /// `draw_interface`; a failed depack caches as `None` so a missing
    /// sprite does not re-read the jag every draw.
    pub graphic_sprites: HashMap<(String, i32), Option<Pix32>>,
    /// Minimap state (`Client.ts` `minimapDraw` 11279): the composed map
    /// buffer, the compass/mapedge/mapdot/mapmarker sprites, `mapback` (the
    /// ring mask plotted into `area_map`), and the per-row scanline masks
    /// built from `mapback.data` (TS 1180-1216). `minimap` is allocated as
    /// TS maininit (868) does and composed by `minimap_build_buffer`
    /// (5280). `minimap_state`/`minimap_level`/`macro_minimap_*` default as
    /// TS (506, 243-247); `minimap_loop` (2742) is Task 7.
    pub minimap: Option<Pix32>,
    pub compass: Option<Pix32>,
    pub mapedge: Option<Pix32>,
    pub mapmarker1: Option<Pix32>,
    pub mapmarker2: Option<Pix32>,
    pub mapdots1: Option<Pix32>,
    pub mapdots2: Option<Pix32>,
    pub mapdots3: Option<Pix32>,
    pub mapdots4: Option<Pix32>,
    pub mapback: Option<Pix8>,
    /// `mapscene`/`mapfunction` sprites from client-ts (254-255): the
    /// minimap wall/scene icons from the `media` jag, depacked by
    /// `prepare_game`. `None` entries skip the plot in `draw_detail`.
    pub mapscene: Vec<Option<Pix8>>,
    pub mapfunction: Vec<Option<Pix32>>,
    pub compass_mask_line_offsets: Vec<i32>,
    pub compass_mask_line_lengths: Vec<i32>,
    pub minimap_mask_line_offsets: Vec<i32>,
    pub minimap_mask_line_lengths: Vec<i32>,
    pub minimap_state: i32,
    pub minimap_level: i32,
    pub macro_minimap_angle: i32,
    pub macro_minimap_zoom: i32,
    /// `activeMapFunctions`/`minimapFlag` from client-ts (508-513): filled
    /// by `minimapBuildBuffer`/`minimapLoop`; sized 1000 as TS. The hint
    /// fields (TS 161-165) gate the `minimapDrawArrow` branch (`hintType`
    /// 0 → skipped).
    pub active_map_function_count: i32,
    pub active_map_function_x: Vec<i32>,
    pub active_map_function_z: Vec<i32>,
    pub active_map_functions: Vec<Option<Pix32>>,
    pub hint_type: i32,
    pub hint_npc: i32,
    pub hint_player: i32,
    pub hint_tile_x: i32,
    pub hint_tile_z: i32,
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

    /// Reconnect flag of the most recent `login` call (`None` until the
    /// first login). `lostCon` reestablishes with `reconnect = true`
    /// (wrapper opcode 18); the flag is how the reconnect path is observed.
    pub last_login_reconnect: Option<bool>,
    /// `logoutTimer` from Java: frames remaining until a requested logout.
    pub logout_timer: i32,
    /// `Client.cyclelogic3` from client-ts (a TS static, instance here):
    /// anticheat counter sent with `ANTICHEAT_CYCLELOGIC3` every 113
    /// `minimapBuildBuffer` runs.
    pub cyclelogic3: i32,
    /// `timeoutTimer` from Java: frames since the last full in-game packet;
    /// `gameLoop` calls `lostCon` past 750 (~15 s at 20 ms).
    pub timeout_timer: i32,
    /// `noTimeoutTimer` from Java: frames since the last outbound flush;
    /// `gameLoop` writes `NO_TIMEOUT` past 50 (~1 s at 20 ms).
    pub no_timeout_timer: i32,
    /// `errorLoading` from Java/TS: missing required cache jag or a failed
    /// map request. `mainloop` returns immediately; framerate is 1.
    pub error_loading: bool,
}

impl Client {
    pub fn new(config: ClientConfig) -> Self {
        // TS `getJagChecksums` downloads `/crc` from the web origin (port 80).
        // Local pack/client is missing `wordenc`, so file CRCs fail the
        // engine's CrcBuffer32 check (login code 6). Prefer /crc; fall back
        // to files for tests without a web server.
        let jag_checksum = Self::get_jag_checksums(&config.host, 80)
            .unwrap_or_else(|| Self::read_jag_checksums(&config.cache_dir));
        let on_demand = Self::load_on_demand(&config);
        let midi = midi_backend(&config.cache_dir);
        let (cache, error_loading) = match Self::load_cache(&config.cache_dir) {
            Ok(cache) => (cache, false),
            Err(()) => (Cache::default(), true),
        };
        let groundh: LevelHeightmaps =
            vec![
                vec![vec![0i32; (BUILD_AREA_SIZE + 1) as usize]; (BUILD_AREA_SIZE + 1) as usize];
                BuildArea::LEVELS as usize
            ];
        let mut client = Client {
            shell: GameShell::new(),
            #[cfg(feature = "window")]
            present: None,
            config,
            cache,

            ingame: false,
            draw: false,
            scene_state: 0,
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
            cyclelogic1: 0,

            stat_base_level: vec![0; Skill::count],
            stat_effective_level: vec![0; Skill::count],
            stat_xp: vec![0; Skill::count],
            var: Vec::new(),
            var_serv: Vec::new(),
            runenergy: 0,

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
            wave_count: 0,
            wave_ids: vec![0; 50],
            wave_loops: vec![0; 50],
            wave_delay: vec![0; 50],
            jagfx: JagFX::default(),

            menu_num_entries: 0,
            menu_option: vec![String::new(); MENU_CAPACITY],
            menu_action: vec![0; MENU_CAPACITY],
            menu_param_a: vec![0; MENU_CAPACITY],
            menu_param_b: vec![0; MENU_CAPACITY],
            menu_param_c: vec![0; MENU_CAPACITY],

            dir_map: vec![0; BUILD_AREA_TILES],
            dist_map: vec![0; BUILD_AREA_TILES],
            route_x: vec![0; ROUTE_BUFFER],
            route_z: vec![0; ROUTE_BUFFER],
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
            redraw_side: false,
            active_icon: 3,
            side_icon: [-1; 14],
            side_modal_id: -1,
            chat_modal_id: -1,
            main_modal_id: -1,
            target_com_id: 0,
            obj_com_id: 0,
            obj_selected_slot: 0,
            obj_selected_com_id: 0,

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
            draw_area: PixMap::new(APPLET_W, APPLET_H),
            pix3d: Pix3DDraw::default(),
            title: None,
            p11: None,
            p12: None,
            b12: None,
            q8: None,
            image_title0: None,
            image_title1: None,
            image_title2: None,
            image_title3: None,
            image_title4: None,
            image_title5: None,
            image_title6: None,
            image_title7: None,
            image_title8: None,
            image_titlebox: None,
            image_titlebutton: None,
            image_runes: Vec::new(),
            title_flames: None,
            loginscreen: 0,
            login_select: 0,
            redraw_frame: true,
            cam_x: 0,
            cam_y: 0,
            cam_z: 0,
            cam_pitch: 0,
            cam_yaw: 0,
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
            scene_cycle: 0,
            tile_last_occupied_cycle: vec![0; BUILD_AREA_TILES],
            vis_calc_done: false,
            area_game: None,
            area_map: None,
            area_side: None,
            area_chat: None,
            area_backleft1: None,
            area_backleft2: None,
            area_backright1: None,
            area_backright2: None,
            area_backtop1: None,
            area_backvmid1: None,
            area_backvmid2: None,
            area_backvmid3: None,
            area_backhmid2: None,
            area_backbase1: None,
            area_backbase2: None,
            area_backhmid1: None,
            invback: None,
            chatback: None,
            backbase1: None,
            backbase2: None,
            backhmid1: None,
            sideicons: [const { None }; 13],
            redstone1: None,
            redstone2: None,
            redstone3: None,
            redstone1h: None,
            redstone2h: None,
            redstone1v: None,
            redstone2v: None,
            redstone3v: None,
            redstone1hv: None,
            redstone2hv: None,
            redraw_icons: false,
            redraw_chat: false,
            redraw_chat_mode: false,
            graphic_sprites: HashMap::new(),
            minimap: Some(Pix32::new(512, 512)),
            compass: None,
            mapedge: None,
            mapmarker1: None,
            mapmarker2: None,
            mapdots1: None,
            mapdots2: None,
            mapdots3: None,
            mapdots4: None,
            mapback: None,
            mapscene: vec![None; 50],
            mapfunction: vec![None; 50],
            compass_mask_line_offsets: Vec::new(),
            compass_mask_line_lengths: Vec::new(),
            minimap_mask_line_offsets: Vec::new(),
            minimap_mask_line_lengths: Vec::new(),
            minimap_state: 0,
            minimap_level: -1,
            macro_minimap_angle: 0,
            macro_minimap_zoom: 0,
            active_map_function_count: 0,
            active_map_function_x: vec![0; 1000],
            active_map_function_z: vec![0; 1000],
            active_map_functions: vec![None; 1000],
            hint_type: 0,
            hint_npc: 0,
            hint_player: 0,
            hint_tile_x: 0,
            hint_tile_z: 0,
            chat_public_mode: 0,
            chat_private_mode: 0,
            chat_trade_mode: 0,
            chat_type: [0; 100],
            chat_username: [const { String::new() }; 100],
            chat_text: [const { String::new() }; 100],
            chat_input: String::new(),
            chat_scroll_pos: 0,
            chat_scroll_height: 78,
            last_login_reconnect: None,
            logout_timer: 0,
            cyclelogic3: 0,
            timeout_timer: 0,
            no_timeout_timer: 0,
            error_loading,
        };
        if client.error_loading {
            client.shell.set_framerate(1);
        }
        // TS maininit 1153 `Pix3D.initColourTable(0.8)`: process-wide, so
        // the first shaded/gouraud triangle of any 3D pass has a table.
        // `Pix3D.lowMem` comes from the same config the TS constructor
        // takes (`Client.setLowMem`/`setHighMem`); `World.lowMem` reads
        // this through the `pix` handle in `render_all`.
        Pix3D::init_colour_table(0.8);
        client.pix3d.low_mem = client.config.lowmem;
        client
    }

    /// Unpack `config` (and `interface` when present) from `cache_dir`. An
    /// empty dir (tests, no pack) yields `Cache::default()`. A real cache
    /// missing the required `config` jag — or one whose bytes are not a
    /// valid jag (dummy test files) — is `Err`, which becomes
    /// `errorLoading`.
    fn load_cache(cache_dir: &str) -> Result<Cache, ()> {
        let cache_present = JAG_FILES
            .iter()
            .any(|name| Path::new(&format!("{cache_dir}/{name}")).is_file());
        if !cache_present {
            return Ok(Cache::default());
        }
        let bytes = std::fs::read(format!("{cache_dir}/config")).map_err(|_| ())?;
        let mut cache = catch_unwind(AssertUnwindSafe(|| Cache::unpack(&JagFile::new(bytes))))
            .map_err(|_| ())?;
        if let Ok(iface_bytes) = std::fs::read(format!("{cache_dir}/interface")) {
            if let Ok(ifaces) =
                catch_unwind(AssertUnwindSafe(|| IfType::unpack(&JagFile::new(iface_bytes))))
            {
                cache.ifaces = ifaces;
            }
        }
        Ok(cache)
    }

    /// HTTP/1.0 `GET {path}` returning the response body, headers split on
    /// `\r\n\r\n` (client-ts `getJagChecksums`/`getJagFile` fetch the same
    /// way). `None` on connect/read failure or a bodyless response.
    fn http_get(host: &str, port: u16, path: &str) -> Option<Vec<u8>> {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect((host, port)).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .ok()?;
        write!(stream, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").ok()?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).ok()?;
        let split = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
        Some(buf[split + 4..].to_vec())
    }

    /// TS `getJagChecksums`: GET `/crc` (9×g4 + hash). Hash check matches
    /// client-ts (`1234`, `hash = (hash << 1) + crc[i]`).
    fn get_jag_checksums(host: &str, port: u16) -> Option<[i32; 9]> {
        let body = Self::http_get(host, port, "/crc")?;
        if body.len() < 40 {
            return None;
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
            return None;
        }
        Some(checksum)
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
        let Some(&crc) = checksums.get(index) else {
            return None;
        };
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

    /// TS `Client.maininit` (819-1178): the one-shot loading screen — fetch
    /// the 8 JAG archives over HTTP (CRC-hit on the local cache), unpack
    /// config/interface, start OnDemand from the versionlist, and prefetch
    /// anims/models. `already_started` is set first, so a second call is a
    /// no-op (TS `alreadyStarted`). A failed or invalid jag sets
    /// `error_loading` but does not abort the fetch loop; progress reaches
    /// 100 only when the `/crc` fetch succeeds — the checksum-fail path
    /// returns early with `error_loading` and `last_progress_percent` left
    /// at 10.
    pub fn maininit(&mut self) {
        if self.already_started {
            return;
        }
        self.already_started = true;
        // TS produces `errorLoading` only inside `maininit`; `Client::new`'s
        // pre-maininit unpack may have set it (and framerate 1) for a cache
        // that `maininit` can repair, so reset both before fetching.
        self.error_loading = false;
        self.shell.set_framerate(50);

        self.draw_progress("Loading...", 0);

        // TS `getJagChecksums` (694-748): `/crc` retried with a 5 s wait
        // doubling to 60 s, forever. Capped at 10 retries so a dead web
        // server cannot hang the caller; tests plant a listener so the
        // first attempt succeeds.
        let checksums = match self.fetch_jag_checksums() {
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
        // persisted but not unpacked (WordFilter is a later slice).
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
        for (display, filename, index, progress) in JAG_FETCH {
            if self
                .fetch_jag_file(display, progress, filename, index, &checksums)
                .is_none()
            {
                self.error_loading = true;
            }
        }

        // Unpack config/interface from the files now on disk (`load_cache`
        // reads the same persisted paths `get_jag_file` wrote). A missing or
        // invalid config jag is `Err` → `errorLoading`.
        match Self::load_cache(&self.config.cache_dir) {
            Ok(cache) => self.cache = cache,
            Err(()) => {
                self.error_loading = true;
                self.shell.set_framerate(1);
            }
        }

        self.on_demand = Self::load_on_demand(&self.config);

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

        // TS anim/model prefetch (893-960): request every anim, then the
        // in-use models, draining with `on_demand_loop` until the request
        // lists empty. Skipped when OnDemand is `None` (no versionlist —
        // the dummy-file tests).
        if self.on_demand.is_some() {
            self.draw_progress("Requesting animations", 65);
            let anim_count = self.on_demand.as_ref().unwrap().get_file_count(1);
            for i in 0..anim_count {
                self.on_demand.as_mut().unwrap().request(1, i);
            }
            while self.on_demand.as_ref().unwrap().remaining() > 0 {
                let progress = anim_count - self.on_demand.as_ref().unwrap().remaining() as i32;
                if progress > 0 {
                    self.draw_progress(
                        &format!("Loading animations - {}%", (progress * 100) / anim_count),
                        65,
                    );
                }
                self.on_demand_loop();
                thread::sleep(Duration::from_millis(100));
            }

            self.draw_progress("Requesting models", 70);
            let model_count = self.on_demand.as_ref().unwrap().get_file_count(0);
            for i in 0..model_count {
                if self.on_demand.as_ref().unwrap().get_model_use(i) & 0x1 != 0 {
                    self.on_demand.as_mut().unwrap().request(0, i);
                }
            }
            let model_prefetch = self.on_demand.as_ref().unwrap().remaining() as i32;
            while self.on_demand.as_ref().unwrap().remaining() > 0 {
                let progress = model_prefetch - self.on_demand.as_ref().unwrap().remaining() as i32;
                if progress > 0 {
                    self.draw_progress(
                        &format!("Loading models - {}%", (progress * 100) / model_prefetch),
                        70,
                    );
                }
                self.on_demand_loop();
                thread::sleep(Duration::from_millis(100));
            }
        }

        self.draw_progress("Preparing game engine", 100);
    }

    /// TS `getJagChecksums` retry policy (694-748): attempt `/crc`, then
    /// wait `fetch_retry_wait` (doubling to the 60 s cap) before the next
    /// attempt, forever. Capped at 10 retries (TS switches to its "Game
    /// updated" message at `retries >= 10`); `None` lets `maininit` fail
    /// with `errorLoading` instead of hanging. The per-second countdown
    /// messages are not ported.
    fn fetch_jag_checksums(&mut self) -> Option<[i32; 9]> {
        let mut wait = self.fetch_retry_wait;
        let mut retries = 0;
        loop {
            self.draw_progress("Connecting to web server", 10);
            if let Some(checksums) = Self::get_jag_checksums(&self.config.host, self.http_port) {
                return Some(checksums);
            }
            retries += 1;
            if retries >= 10 {
                return None;
            }
            thread::sleep(wait);
            wait = (wait * 2).min(Duration::from_secs(60));
        }
    }

    /// TS `getJagFile` retry loop (749-817): GET `/{filename}{crc}` with the
    /// same doubling wait as the checksum fetch. A CRC mismatch is handled
    /// inside `get_jag_file` (bytes discarded, `None` returned) and retried
    /// here, so a transient failure or corrupted download recovers instead
    /// of erroring the client. Capped at 10 retries like the checksum fetch
    /// so a dead server cannot hang the caller; tests plant a listener or
    /// set `fetch_retry_wait` small. The per-second countdown messages are
    /// not ported.
    fn fetch_jag_file(
        &mut self,
        display: &str,
        progress: i32,
        filename: &str,
        index: usize,
        checksums: &[i32; 9],
    ) -> Option<Vec<u8>> {
        let mut wait = self.fetch_retry_wait;
        let mut retries = 0;
        loop {
            self.draw_progress(&format!("Requesting {display}"), progress);
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
                return None;
            }
            thread::sleep(wait);
            wait = (wait * 2).min(Duration::from_secs(60));
        }
    }

    /// Login handshake, 1:1 of `Client.ts` `login` (1719-1867) / Java
    /// `Client.login`: probe, seed, RSA blob, opcode 16/18 wrapper. Response 1
    /// waits 2 s and retries the same attempt; response 2 enters the game;
    /// anything else is `LoginError` with the code and title-screen messages.
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

        let mut stream = match ClientStream::connect(&self.config.host, self.config.port) {
            Ok(s) => s,
            Err(_) => return Err(io_error()),
        };

        let userhash = JString::to_userhash(username);
        let login_server = ((userhash >> 16) & 0x1f) as i32;

        self.out.pos = 0;
        self.out.p1(14);
        self.out.p1(login_server);
        stream.write(self.out.data(), 2).map_err(|_| io_error())?;

        for _ in 0..8 {
            stream.read().map_err(|_| io_error())?;
        }
        let mut response = stream.read().map_err(|_| io_error())?;

        if response == 0 {
            stream
                .read_bytes(self.r#in.data_mut(), 0, 8)
                .map_err(|_| io_error())?;
            self.r#in.pos = 0;
            let login_seed = self.r#in.g8();
            let mut seed = [
                login_random(),
                login_random(),
                (login_seed >> 32) as i32,
                (login_seed & 0xffff_ffff) as i32,
            ];

            self.out.pos = 0;
            self.out.p1(10);
            self.out.p4(seed[0]);
            self.out.p4(seed[1]);
            self.out.p4(seed[2]);
            self.out.p4(seed[3]);
            self.out.p4(LOGIN_UID);
            self.out.pjstr(username);
            self.out.pjstr(password);
            let n = BigUint::from_str(LOGIN_RSAN).unwrap();
            let e = BigUint::from_str(LOGIN_RSAE).unwrap();
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
                .map_err(|_| io_error())?;
            response = stream.read().map_err(|_| io_error())?;
        }

        if response == 1 {
            thread::sleep(Duration::from_millis(2000));
            // old stream is dropped (closed); each attempt opens a fresh one
            return self.login(username, password, reconnect);
        }

        if response == 2 {
            self.staffmodlevel = stream.read().map_err(|_| io_error())?;
            self.mouse_tracked = stream.read().map_err(|_| io_error())? == 1;
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
            self.timeout_timer = 0;
            self.logout_timer = 0;
            self.no_timeout_timer = 0;
            // Client.ts:1853 — localPlayer = players[LOCAL_PLAYER_INDEX] = new
            let player = ClientPlayer::default();
            self.players[LOCAL_PLAYER_INDEX as usize] = Some(player.clone());
            self.local_player = Some(player);
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
        self.login_mes1 = mes1.clone();
        self.login_mes2 = mes2.clone();
        self.stream = Some(stream);
        Err(LoginError {
            code: response,
            mes1,
            mes2,
        })
    }

    /// Linear build-area index, `CollisionMap.index(x, z) = x * SIZE + z`.
    fn collision_index(x: i32, z: i32) -> usize {
        (x * BUILD_AREA_SIZE + z) as usize
    }

    /// Menu dispatch, port of client-ts `Client.ts` `doAction` (8548-9273).
    /// Headless encode paths only: the branches that need config types, chat,
    /// or the scene (`OP_OBJ*`, `OP_LOC*`, `OP_PLAYER*`, `OP_HELD*`,
    /// `INV_BUTTON*`, buttons, examine, friend/ignore) land with Tasks 13/15.
    #[allow(non_snake_case)] // Java name kept for the RawClient mapping
    pub fn doAction(&mut self, option_id: i32) {
        if option_id < 0 {
            return;
        }

        let mut action = self.menu_action[option_id as usize];
        let a = self.menu_param_a[option_id as usize];
        let b = self.menu_param_b[option_id as usize];
        let c = self.menu_param_c[option_id as usize];

        if action >= MiniMenuAction::_PRIORITY {
            action -= MiniMenuAction::_PRIORITY;
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

        if action == MiniMenuAction::TGT_NPC {
            self.out.p1_enc(ClientProt::OPNPCT.id);
            self.out.p2(a);
            self.out.p2(self.target_com_id);
        }

        if action == MiniMenuAction::USEHELD_ONNPC {
            self.out.p1_enc(ClientProt::OPNPCU.id);
            self.out.p2(a);
            self.out.p2(self.obj_com_id);
            self.out.p2(self.obj_selected_slot);
            self.out.p2(self.obj_selected_com_id);
        }

        if action == MiniMenuAction::WALK {
            // `World.updateMousePicking` from Client.ts doAction (9217-9222):
            // menuParamB/C are the applet click coords (the viewport Walk
            // here entry), stored as mouseX/Y and converted to scene-local
            // pixels here; the ground answer is consumed into MOVE_GAMECLICK
            // by `game_loop` after the next render. The menu is never open
            // (no minimenu chrome), so the `isMenuOpen` branch is not ported.
            self.world.update_mouse_picking(b - 4, c - 4);
        }

        self.use_mode = 0;
        self.target_mode = 0;
        self.redraw_side = true;
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
        // a full packet resets the in-game silence watchdog (Java tcpIn)
        self.timeout_timer = 0;
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
            self.logout();
        }
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

    /// `IF_OPENSIDE` handler (Client.ts 6034): open `com_id` as the side
    /// modal, closing any open chat modal alongside (TS 6038-6046). The
    /// `ifAnimReset` and `mainModalId`/`resumedPauseButton` resets are
    /// dropped with the features they belong to.
    pub fn apply_if_openside(&mut self, payload: &mut Packet) {
        let com_id = payload.g2();
        self.side_modal_id = com_id;
        self.chat_modal_id = -1;
        self.redraw_side = true;
        self.redraw_icons = true;
        self.redraw_chat = true;
    }

    /// `IF_CLOSE` handler (Client.ts 5968): close the side and chat modals.
    /// The packet has no payload (TS only reads `ptype`).
    pub fn apply_if_close(&mut self) {
        self.side_modal_id = -1;
        self.chat_modal_id = -1;
        self.redraw_side = true;
        self.redraw_icons = true;
        self.redraw_chat = true;
    }

    /// `addChat` from client-ts (11453): shift the 100 chat slots down one
    /// (99→1), write the new line at slot 0, and redraw. The `tutComId`
    /// branch (TS 11454-11458) is dropped with the tutorial feature.
    pub fn add_chat(&mut self, r#type: i32, text: &str, sender: &str) {
        if self.chat_modal_id == -1 {
            self.redraw_chat = true;
        }
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
    /// with the requester's name (TS 6420-6446; the ignore list is not
    /// ported, so `ignored` stays false and `chatDisabled` stays 0), and
    /// anything else is public chat type 0 with no sender.
    pub fn apply_message_game(&mut self, payload: &mut Packet) {
        let message = payload.gjstr();

        if message.ends_with(":tradereq:") {
            let player = message[..message.find(':').unwrap_or(0)].to_string();
            self.add_chat(4, "wishes to trade with you.", &player);
        } else if message.ends_with(":duelreq:") {
            let player = message[..message.find(':').unwrap_or(0)].to_string();
            self.add_chat(8, "wishes to duel with you.", &player);
        } else {
            self.add_chat(0, &message, "");
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

    /// `clientButton` from Java (`Client.java` 8725-8747), ported for the
    /// CC_LOGOUT arm only: `if (var3 == 205) { logoutTimer = 250; return
    /// true; }`. Java returns `false` for the other client codes (handled
    /// locally, no `IF_BUTTON`); those handlers are not ported yet
    /// (operator-accepted deferral 2026-08-20, slice 3/5), so unported
    /// codes return `true` to keep the unconditional `IF_BUTTON` send.
    pub fn client_button(&mut self, com: &IfType) -> bool {
        if com.client_code == CC_LOGOUT {
            self.logout_timer = 250;
            return true;
        }
        true
    }

    /// `buildMinimenu` side branch (Client.ts 2540-2547) with the minimenu
    /// collapsed: a left click inside the side panel (553..743 × 205..466)
    /// walks the open interface tree (`side_modal_id`, else the active tab's
    /// `side_icon`) like `draw_interface` and fires the first hit child's
    /// button. BUTTON_OK/TOGGLE/SELECT send `IF_BUTTON` + the child id;
    /// BUTTON_CLOSE sends `CLOSE_MODAL` and clears the modals like TS
    /// `closeModal` (execute 9194-9196). TOGGLE/SELECT also flip/set the
    /// local `var` and apply its clientcode via `clientVar` (execute
    /// 9163-9179) when `scripts[0][0] == 5`; `clientButton` is not ported.
    pub fn handle_side_if_clicks(&mut self) {
        if self.shell.mouse_click_button != 1 {
            return;
        }
        let (x, y) = (self.shell.mouse_click_x, self.shell.mouse_click_y);
        if !(553..743).contains(&x) || !(205..466).contains(&y) {
            return;
        }
        let root_id = if self.side_modal_id != -1 {
            self.side_modal_id
        } else {
            self.side_icon
                .get(self.active_icon as usize)
                .copied()
                .unwrap_or(-1)
        };
        if root_id == -1 {
            return;
        }
        // panel-local coords like the draw_side draw_interface call
        let Some(hit_id) = self.side_hit_test(root_id, x - 553, y - 205, 0, 0, 0) else {
            return;
        };
        // Peek the button type before cloning: BUTTON_CLOSE returns early
        // without deep-copying the IfType (Strings + Vecs).
        let Some(button_type) = self
            .cache
            .ifaces
            .get(hit_id as usize)
            .and_then(|o| o.as_ref())
            .map(|o| o.button_type)
        else {
            return;
        };
        if button_type == ButtonType::BUTTON_CLOSE {
            self.close_modal();
            return;
        }
        let Some(hit) = self
            .cache
            .ifaces
            .get(hit_id as usize)
            .and_then(|o| o.as_ref())
            .cloned()
        else {
            return;
        };
        // Java execute var5 == 231 (Client.java 4557-4567): clientButton is
        // called only from the BUTTON_OK arm and runs first, arming the
        // logout timer for CC_LOGOUT; the ported arm always returns true so
        // the IF_BUTTON send stays unconditional.
        if button_type == ButtonType::BUTTON_OK && hit.client_code > 0 {
            self.client_button(&hit);
        }
        self.out.p1_enc(ClientProt::IF_BUTTON.id);
        self.out.p2(hit_id);
        if button_type != ButtonType::BUTTON_TOGGLE && button_type != ButtonType::BUTTON_SELECT {
            return;
        }
        let Some(script) = hit.scripts.as_ref().and_then(|s| s.first()) else {
            return;
        };
        if script.first() != Some(&5) {
            return;
        }
        let Some(&varp) = script.get(1) else {
            return;
        };
        // TS execute 9163-9179: flip/set the local var, apply its
        // clientcode (clientVar), then redraw.
        let operand = hit.script_operand.as_ref().and_then(|o| o.first()).copied();
        if button_type == ButtonType::BUTTON_TOGGLE {
            let current = self.var.get(varp as usize).copied().unwrap_or(0);
            grow_write(&mut self.var, varp, 1 - current);
            self.client_var(varp);
            self.redraw_side = true;
        } else if let Some(value) = operand {
            if self.var.get(varp as usize).copied() != Some(value) {
                grow_write(&mut self.var, varp, value);
                self.client_var(varp);
                self.redraw_side = true;
            }
        }
    }

    /// `closeModal` from client-ts (10941-10958): send CLOSE_MODAL and
    /// close the side and chat modals locally; `main_modal_id` is reset
    /// like Java `closeModal` (Client.java 3353-3367).
    /// `resumed_pause_button` is not ported; the per-modal redraw flags
    /// mirror TS `redrawSide`/`redrawIcons`/`redrawChat`. Not to be
    /// confused with the incoming-server `apply_if_close`.
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
    }

    /// Walk `com_id`'s children like TS `addComponentOptions` (9628-9841)
    /// and return the first hit child with a BUTTON_OK/CLOSE/TOGGLE/SELECT
    /// `button_type`. Layers recurse with the same scroll clamp as
    /// `draw_interface`; TYPE_INV children are skipped (their menu options
    /// land with the inventory task). The layer must be visible and its rect
    /// must contain the point (TS 9629).
    fn side_hit_test(
        &self,
        com_id: i32,
        mouse_x: i32,
        mouse_y: i32,
        x: i32,
        y: i32,
        scroll: i32,
    ) -> Option<i32> {
        let com = self
            .cache
            .ifaces
            .get(com_id as usize)
            .and_then(|o| o.as_ref())?;
        if com.r#type != ComponentType::TYPE_LAYER || com.hide {
            return None;
        }
        if mouse_x < x || mouse_y < y || mouse_x > x + com.width || mouse_y > y + com.height {
            return None;
        }
        let children = com.children.as_ref()?;
        let child_x = com.child_x.as_ref()?;
        let child_y = com.child_y.as_ref()?;
        for i in 0..children.len() {
            let child_id = children[i] as usize;
            let Some(child) = self.cache.ifaces.get(child_id).and_then(|o| o.as_ref()) else {
                continue;
            };
            let child_x = child_x[i] + x + child.x;
            let child_y = child_y[i] + y - scroll + child.y;
            match child.r#type {
                ComponentType::TYPE_LAYER => {
                    // TS 9930-9938: clamp the child's scroll position before
                    // recursing with it (max first, then min, sequentially).
                    let mut scroll_pos = child.scroll_pos;
                    if scroll_pos > child.scroll_height - child.height {
                        scroll_pos = child.scroll_height - child.height;
                    }
                    if scroll_pos < 0 {
                        scroll_pos = 0;
                    }
                    if let Some(hit) = self.side_hit_test(
                        children[i],
                        mouse_x,
                        mouse_y,
                        child_x,
                        child_y,
                        scroll_pos,
                    ) {
                        return Some(hit);
                    }
                }
                ComponentType::TYPE_INV => {}
                _ => {
                    if mouse_x >= child_x
                        && mouse_y >= child_y
                        && mouse_x < child_x + child.width
                        && mouse_y < child_y + child.height
                        && matches!(
                            child.button_type,
                            ButtonType::BUTTON_OK
                                | ButtonType::BUTTON_CLOSE
                                | ButtonType::BUTTON_TOGGLE
                                | ButtonType::BUTTON_SELECT
                        )
                    {
                        return Some(children[i]);
                    }
                }
            }
        }
        None
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

                self.scene_loading_splash();

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
                                    self.ground_obj[level as usize][last_x as usize][last_z as usize]
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
                        if loc.x < 0 || loc.z < 0 || loc.x >= BuildArea::SIZE || loc.z >= BuildArea::SIZE {
                            self.loc_changes.unlink_last();
                        }
                        node = self.loc_changes.next();
                    }

                    if self.minimap_flag_x != 0 {
                        self.minimap_flag_x -= dx;
                        self.minimap_flag_z -= dz;
                    }
                }

                self.ptype = -1;
            }

            // interface draw/modal state (mainModalId, sideIcon, activeIcon,
            // chatModalId, tutComId) is not ported yet; these reset ptype
            // like their TS handlers.
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

            ServerProt::IF_OPENCHAT
            | ServerProt::IF_OPENMAIN_SIDE
            | ServerProt::IF_OPENMAIN
            | ServerProt::IF_OPENOVERLAY
            | ServerProt::TUT_FLASH
            | ServerProt::TUT_OPEN => {
                self.ptype = -1;
            }

            ServerProt::IF_SETCOLOUR => {
                let com_id = payload.g2();
                let colour = payload.g2();

                let r = (colour >> 10) & 0x1f;
                let g = (colour >> 5) & 0x1f;
                let b = colour & 0x1f;

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.colour = (r << 19) + (g << 11) + (b << 3);
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETHIDE => {
                let com_id = payload.g2();
                let hide = payload.g1() == 1;

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
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

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
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

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.model1_type = 1;
                    com.model1_id = model_id;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETANIM => {
                let com_id = payload.g2();
                let seq_id = payload.g2b();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
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

                if let (Some(com), Some(local)) = (
                    self.cache
                        .ifaces
                        .get_mut(com_id as usize)
                        .and_then(|o| o.as_mut()),
                    self.local_player.as_ref(),
                ) {
                    com.model1_type = 3;
                    com.model1_id = (local.appearance[8] as i32) << 6
                        | (local.appearance[0] as i32) << 12
                        | (local.colour[0] as i32) << 24
                        | (local.colour[4] as i32) << 18
                        | local.appearance[11] as i32;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETTEXT => {
                let com_id = payload.g2();
                let text = payload.gjstr();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.text = text;
                    // TS also redraws the side when
                    // layerId === sideIcon[activeIcon] (not ported yet).
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETNPCHEAD => {
                let com_id = payload.g2();
                let npc_id = payload.g2();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
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

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    com.x = x;
                    com.y = y;
                }
                self.ptype = -1;
            }

            ServerProt::IF_SETSCROLLPOS => {
                let com_id = payload.g2();
                let mut pos = payload.g2();

                if let Some(com) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
                {
                    if com.r#type == ComponentType::TYPE_LAYER {
                        if pos < 0 {
                            pos = 0;
                        }
                        if pos > com.scroll_height - com.height {
                            pos = com.scroll_height - com.height;
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

                if let Some(inv) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
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

                if let Some(inv) = self
                    .cache
                    .ifaces
                    .get_mut(com_id as usize)
                    .and_then(|o| o.as_mut())
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

                    if let Some(inv) = self
                        .cache
                        .ifaces
                        .get_mut(com_id as usize)
                        .and_then(|o| o.as_mut())
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

            // camera state (cinemaCam, camX/Y/Z, camShake*) is not ported yet
            ServerProt::CAM_LOOKAT
            | ServerProt::CAM_SHAKE
            | ServerProt::CAM_MOVETO
            | ServerProt::CAM_RESET => {
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

            // friends and ignore-list state is not ported yet
            ServerProt::UPDATE_IGNORELIST
            | ServerProt::CHAT_FILTER_SETTINGS
            | ServerProt::MESSAGE_PRIVATE
            | ServerProt::FRIENDLIST_LOADED
            | ServerProt::UPDATE_FRIENDLIST => {
                self.ptype = -1;
            }

            ServerProt::UNSET_MAP_FLAG => {
                self.minimap_flag_x = 0;
                self.ptype = -1;
            }

            // runweight, hint arrows and the reboot timer are not ported yet
            ServerProt::UPDATE_RUNWEIGHT
            | ServerProt::HINT_ARROW
            | ServerProt::UPDATE_REBOOT_TIMER => {
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
                // TS also redraws the side when activeIcon === 12 (not ported)
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

            // last-login info, dialog input and friend-slot ops are not
            // ported yet
            ServerProt::UPDATE_PID => {
                // TS reads selfSlot (g2) then membersAccount (g1) straight
                // off the payload.
                self.self_slot = payload.g2();
                self.members_account = payload.g1();
                self.ptype = -1;
            }

            ServerProt::LAST_LOGIN_INFO
            | ServerProt::P_COUNTDIALOG
            | ServerProt::SET_MULTIWAY
            | ServerProt::SET_PLAYER_OP
            | ServerProt::MINIMAP_TOGGLE => {
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
                    node = self.loc_changes.next();
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
                self.players[index] = Some(ClientPlayer::default());

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
        if mask & player_update::APPEARANCE != 0 {
            let length = buf.g1() as usize;

            let mut data = vec![0u8; length];
            buf.gdata(length, 0, &mut data);

            self.player_appearance_buffer[index] = Some(Packet::new(data));
            if let Some(player) = self.players[index].as_mut() {
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

            if let Some(player) = self.players[index].as_mut() {
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
            if let Some(player) = self.players[index].as_mut() {
                player.face_entity = face_entity;
            }
        }

        if mask & player_update::SAY != 0 {
            let message = buf.gjstr();
            if let Some(player) = self.players[index].as_mut() {
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
            if let Some(player) = self.players[index].as_mut() {
                player.add_hitmark(self.loop_cycle, damage_type, damage);
                player.combat_cycle = self.loop_cycle + 400;
                player.health = buf.g1();
                player.total_health = buf.g1();
            }
        }

        if mask & player_update::FACESQUARE != 0 {
            let x = buf.g2();
            let z = buf.g2();
            if let Some(player) = self.players[index].as_mut() {
                player.face_square_x = x;
                player.face_square_z = z;
            }
        }

        if mask & player_update::CHAT != 0 {
            let _colour_effect = buf.g2();
            let _chat_type = buf.g1();
            let length = buf.g1();
            let start = buf.pos;

            // TS unpacks/filters the chat via WordPack/WordFilter and
            // addChat; the chat task owns that. The payload is still skipped
            // so the frame stays aligned.
            buf.pos = start + length as usize;
        }

        if mask & player_update::SPOTANIM != 0 {
            let spotanim_id = buf.g2();
            let height_delay = buf.g4();
            if let Some(player) = self.players[index].as_mut() {
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
            if let Some(player) = self.players[index].as_mut() {
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
            if let Some(player) = self.players[index].as_mut() {
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
                self.npc[index] = Some(ClientNpc::default());
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
                    self.loc_change_create(self.minusedlevel, x, z, layer, id, shape, rotate, 0, -1);
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
                    self.loc_change_create(self.minusedlevel, x, z, layer, -1, shape, rotate, 0, -1);
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
                    let loop_cycle = self.loop_cycle;

                    match layer {
                        LocLayer::WALL => {
                            if let Some(wall) = self.world.get_wall_mut(self.minusedlevel, x, z) {
                                let loc_id = (wall.typecode >> 14) & 0x7fff;
                                if shape == 2 {
                                    wall.model1 = Some(SceneModel::LocAnim(ClientLocAnim::new(
                                        &self.cache, loc_id, 2, rotate + 4, height_sw, height_se,
                                        height_ne, height_nw, seq as usize, false, loop_cycle,
                                    )));
                                    wall.model2 = Some(SceneModel::LocAnim(ClientLocAnim::new(
                                        &self.cache, loc_id, 2, (rotate + 1) & 0x3, height_sw,
                                        height_se, height_ne, height_nw, seq as usize, false,
                                        loop_cycle,
                                    )));
                                } else {
                                    wall.model1 = Some(SceneModel::LocAnim(ClientLocAnim::new(
                                        &self.cache, loc_id, shape, rotate, height_sw, height_se,
                                        height_ne, height_nw, seq as usize, false, loop_cycle,
                                    )));
                                }
                            }
                        }
                        LocLayer::WALL_DECOR => {
                            // `getDecor(level, z, x)` in the TS swaps its
                            // parameter names; it indexes by tile x,z.
                            if let Some(decor) =
                                self.world.get_decor_mut(self.minusedlevel, x, z)
                            {
                                let loc_id = (decor.typecode >> 14) & 0x7fff;
                                // [sic] TS passes heightNE in the SE slot.
                                decor.model = SceneModel::LocAnim(ClientLocAnim::new(
                                    &self.cache, loc_id, 4, 0, height_sw, height_ne, height_ne,
                                    height_nw, seq as usize, false, loop_cycle,
                                ));
                            }
                        }
                        LocLayer::GROUND => {
                            let shape = if shape == 11 { 10 } else { shape };
                            if let Some(sprite) = self.world.get_scene_mut(self.minusedlevel, x, z)
                            {
                                let loc_id = (sprite.typecode >> 14) & 0x7fff;
                                sprite.model = Some(SceneModel::LocAnim(ClientLocAnim::new(
                                    &self.cache, loc_id, shape, rotate, height_sw, height_se,
                                    height_ne, height_nw, seq as usize, false, loop_cycle,
                                )));
                            }
                        }
                        LocLayer::GROUND_DECOR => {
                            if let Some(decor) = self.world.get_gd_mut(self.minusedlevel, x, z) {
                                let loc_id = (decor.typecode >> 14) & 0x7fff;
                                decor.model = Some(SceneModel::LocAnim(ClientLocAnim::new(
                                    &self.cache, loc_id, LocShape::GROUND_DECOR, rotate, height_sw,
                                    height_se, height_ne, height_nw, seq as usize, false,
                                    loop_cycle,
                                )));
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
                            .get_or_insert_with(LinkList::new);
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
                                node = objs.next();
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
                            .get_or_insert_with(LinkList::new);
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
                                    .and_then(|p| p.as_mut())
                            };
                            if let Some(player) = player {
                                player.loc_start_cycle = t1 + self.loop_cycle;
                                player.loc_stop_cycle = t2 + self.loop_cycle;
                                player.loc_model = Some(model);

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
                                node = objs.next();
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

    /// `logout()` from client-ts: close the stream and return to the login
    /// screen. Java also stops the midi and clears the music state.
    pub fn logout(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            stream.close();
        }
        self.ingame = false;
        self.login_user.clear();
        self.login_pass.clear();
        self.world.reset_map();
        self.projectiles.clear();
        self.spotanims.clear();
        self.loc_changes = LinkList::new();
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
        for entry in self.cache.ifaces.iter().flatten() {
            if entry.client_code == CC_REPORT_INPUT {
                self.main_modal_id = entry.layer_id;
                return;
            }
        }
    }

    /// `handleInputKey` from client-ts (2937), chat branch: poll queued
    /// keys while no chat modal is open. Printable 32..=122 (up to 126
    /// once the input starts with `::`) appends below 80 chars, 8
    /// backspaces, 10/13 sends. A `::` command goes out as `CLIENT_CHEAT`
    /// (TS 3093-3095); anything else packs `MESSAGE_PUBLIC` (TS 3158-3165)
    /// with the colour/effect prefixes parsed as TS 3097-3155. `WordPack`
    /// is not ported, so the pack is the plain jstr via `pjstr`, and the
    /// `toSentenceCase`/`WordFilter` steps are skipped; the own message
    /// still echoes locally as `add_chat(2, ...)` as TS 3169-3179.
    pub fn handle_chat_input(&mut self) {
        loop {
            let key = self.shell.poll_key();
            if key == -1 {
                break;
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
                    // WordPack is not ported: pack the plain jstr.
                    self.out.pjstr(&text);
                    self.out.psize1((self.out.pos - start) as i32);

                    // Echo the own message as TS 3169-3179 (the
                    // toSentenceCase/WordFilter steps are not ported).
                    // Name falls back to the login user, then "player", so
                    // the echo is never dropped pre-spawn.
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
                }
                self.chat_input.clear();
                self.redraw_chat = true;
            }
        }
    }

    /// The 3D-viewport arm of `mouseLoop` (Client.ts 8256) with the minimenu
    /// collapsed: a left click inside the viewport builds the `buildMinimenu`
    /// world branch (Cancel + Walk here, `addWorldOptions` 9275-2282 — no
    /// loc/npc/player ops in this port) and auto-fires the top entry
    /// (`doAction(menuNumEntries - 1)`), which arms `World` mouse picking.
    /// HUD clicks lose: `handle_tab_clicks`/`handle_side_if_clicks` run
    /// first and `mouse_loop` only walks inside 4..516 × 4..338.
    pub fn mouse_loop(&mut self) {
        if self.shell.mouse_click_button != 1 {
            return;
        }
        let (x, y) = (self.shell.mouse_click_x, self.shell.mouse_click_y);
        if !(4..516).contains(&x) || !(4..338).contains(&y) {
            return;
        }
        self.menu_num_entries = 2;
        self.menu_option[0] = "Cancel".into();
        self.menu_action[0] = MiniMenuAction::CANCEL;
        self.menu_option[1] = "Walk here".into();
        self.menu_action[1] = MiniMenuAction::WALK;
        self.menu_param_b[1] = x;
        self.menu_param_c[1] = y;
        self.doAction(self.menu_num_entries - 1);
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

    /// `sceneLoadingSplash` draws from client-ts (6832-6835, 5078-5081):
    /// the "Loading - please wait." text centred at (256, 150) on
    /// `area_game` (black at (257, 151) behind the white), then the (4, 4)
    /// blit into `draw_area`. `area_game` is cls'd black first — with
    /// fonts missing (`p12` is `None` without a title jag) the splash is
    /// just the black surface; the blit is gated on the `draw` CPU-save
    /// switch like the rest of the render.
    fn scene_loading_splash(&mut self) {
        if let Some(ag) = self.area_game.as_mut() {
            ag.fill(Colour::BLACK);
            let mut surface = Pix2D::with_pixels(&mut ag.pixels, ag.width, ag.height);
            if let Some(p12) = self.p12.as_ref() {
                p12.centre_string(&mut surface, Some("Loading - please wait."), 257, 151, Colour::BLACK);
                p12.centre_string(&mut surface, Some("Loading - please wait."), 256, 150, Colour::WHITE);
            }
        }
        if self.draw {
            if let Some(ag) = &self.area_game {
                ag.blit_into(&mut self.draw_area, 4, 4);
            }
        }
    }

    /// `checkMinimap` from client-ts (5076): a low-memory level change
    /// re-enters the loading state (`scene_state = 1`), and while loading
    /// the splash is redrawn each frame ahead of `check_scene`. `minimap_level`
    /// tracks the level the minimap buffer was built for.
    fn check_minimap(&mut self) {
        if self.config.lowmem
            && self.scene_state == 2
            && self.build_minusedlevel != self.minusedlevel
        {
            self.scene_state = 1;
            self.scene_load_start_time = Instant::now();
        }

        if self.scene_state == 1 {
            // splash is redrawn every frame the scene is loading
            self.scene_loading_splash();
            // TS logs a "glcfb" hang line when checkScene stalls past
            // 360 s; the console write is not ported.
            let _status = self.check_scene();
        }

        if self.scene_state == 2 && self.minusedlevel != self.minimap_level {
            self.minimap_level = self.minusedlevel;
            self.minimap_build_buffer(self.minusedlevel);
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
                if !ClientBuild::check_locations_low_mem(self.config.lowmem, &self.cache, data, x, z) {
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
        self.pix3d.clear_texels();
        self.world.reset_map();

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
            &mut self.pix3d,
            &mut self.world,
            &mut self.collision,
            &self.groundh,
            &self.mapl,
        );

        self.out.p1_enc(ClientProt::NO_TIMEOUT.id);

        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                self.show_object(x, z);
            }
        }

        self.loc_change_post_build();
        self.build_minusedlevel = self.minusedlevel;

        // TS 5254-5261: low-memory model unload for models the render
        // never uses (flags 0x79 = all render uses).
        if self.config.lowmem && self.on_demand.is_some() {
            let model_count = self.on_demand.as_ref().map(|od| od.get_file_count(0)).unwrap_or(0);
            for i in 0..model_count {
                let flags = self.on_demand.as_ref().map(|od| od.get_model_use(i)).unwrap_or(0);
                if flags & 0x79 == 0 {
                    Model::unload(i);
                }
            }
        }

        self.pix3d.init_pool(20);
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

    /// `minimapBuildBuffer(level)` from client-ts (5280-5387): compose the
    /// 512×512 minimap buffer from `mapl` (the ground pass through
    /// `render_2d_ground`, then the loc wall/scene lines and icons through
    /// `draw_detail`), scan the ground decors for the active map-function
    /// dots, and send the anticheat cycle counter.
    pub fn minimap_build_buffer(&mut self, level: i32) {
        let Some(mm) = self.minimap.as_mut() else {
            return;
        };

        for p in mm.data.iter_mut() {
            *p = 0;
        }

        for z in 1..BuildArea::SIZE - 1 {
            let mut offset = (BuildArea::SIZE - 1 - z) * 512 * 4 + 24628;

            for x in 1..BuildArea::SIZE - 1 {
                if self.mapl[level as usize][x as usize][z as usize] as i32
                    & (MapFlag::VIS_BELOW | MapFlag::FORCE_HIGH_DETAIL)
                    == 0
                {
                    self.world.render_2d_ground(level, x, z, &mut mm.data, offset, 512);
                }

                if level < 3
                    && self.mapl[level as usize + 1][x as usize][z as usize] as i32
                        & MapFlag::VIS_BELOW
                        != 0
                {
                    self.world.render_2d_ground(level + 1, x, z, &mut mm.data, offset, 512);
                }

                offset += 4;
            }
        }

        let inactive_rgb = ((((random_float() * 20.0) as i32) + 238 - 10) << 16)
            + ((((random_float() * 20.0) as i32) + 238 - 10) << 8)
            + ((random_float() * 20.0) as i32)
            + 238
            - 10;
        let active_rgb = (((random_float() * 20.0) as i32) + 238 - 10) << 16;

        // TS `this.minimap.setPixels()`: bind the buffer for `draw_detail`'s
        // plots. The `areaGame.setPixels()` rebinding is a no-op here — every
        // draw helper in this port takes its surface explicitly.
        let mut surface = Pix2D::with_pixels(&mut mm.data, 512, 512);
        for z in 1..BuildArea::SIZE - 1 {
            for x in 1..BuildArea::SIZE - 1 {
                if self.mapl[level as usize][x as usize][z as usize] as i32
                    & (MapFlag::VIS_BELOW | MapFlag::FORCE_HIGH_DETAIL)
                    == 0
                {
                    draw_detail(
                        &self.world,
                        &self.cache,
                        &self.mapscene,
                        &mut surface,
                        level,
                        x,
                        z,
                        inactive_rgb,
                        active_rgb,
                    );
                }

                if level < 3
                    && self.mapl[level as usize + 1][x as usize][z as usize] as i32
                        & MapFlag::VIS_BELOW
                        != 0
                {
                    draw_detail(
                        &self.world,
                        &self.cache,
                        &self.mapscene,
                        &mut surface,
                        level + 1,
                        x,
                        z,
                        inactive_rgb,
                        active_rgb,
                    );
                }
            }
        }
        drop(surface);

        self.active_map_function_count = 0;

        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                let typecode = self.world.gd_type(self.minusedlevel, x, z);
                if typecode == 0 {
                    continue;
                }

                let loc_id = (typecode >> 14) & 0x7fff;
                let func = self.cache.loc(loc_id as usize).mapfunction;
                if func < 0 {
                    continue;
                }

                let mut stx = x;
                let mut stz = z;

                if func != 22
                    && func != 29
                    && func != 34
                    && func != 36
                    && func != 46
                    && func != 47
                    && func != 48
                {
                    let max_x = BuildArea::SIZE;
                    let max_z = BuildArea::SIZE;
                    let flags = &self.collision[self.minusedlevel as usize].flags;

                    for _ in 0..10 {
                        let rand = (random_float() * 4.0) as i32;
                        if rand == 0
                            && stx > 0
                            && stx > x - 3
                            && (flags[(stx - 1) as usize][stz as usize] & CollisionFlag::PL_WALK_E)
                                == CollisionFlag::_OPEN
                        {
                            stx -= 1;
                        }

                        if rand == 1
                            && stx < max_x - 1
                            && stx < x + 3
                            && (flags[(stx + 1) as usize][stz as usize] & CollisionFlag::PL_WALK_W)
                                == CollisionFlag::_OPEN
                        {
                            stx += 1;
                        }

                        if rand == 2
                            && stz > 0
                            && stz > z - 3
                            && (flags[stx as usize][(stz - 1) as usize] & CollisionFlag::PL_WALK_N)
                                == CollisionFlag::_OPEN
                        {
                            stz -= 1;
                        }

                        if rand == 3
                            && stz < max_z - 1
                            && stz < z + 3
                            && (flags[stx as usize][(stz + 1) as usize] & CollisionFlag::PL_WALK_S)
                                == CollisionFlag::_OPEN
                        {
                            stz += 1;
                        }
                    }
                }

                // TS writes past the 1000-slot `activeMapFunctions` arrays
                // silently; a Rust panic here is worse, so cap the count.
                let count = self.active_map_function_count as usize;
                if count < self.active_map_functions.len() {
                    self.active_map_functions[count] =
                        self.mapfunction.get(func as usize).and_then(|s| s.clone());
                    self.active_map_function_x[count] = stx;
                    self.active_map_function_z[count] = stz;
                    self.active_map_function_count += 1;
                }
            }
        }

        self.cyclelogic3 += 1;
        if self.cyclelogic3 > 112 {
            self.cyclelogic3 = 0;

            self.out.p1_enc(ClientProt::ANTICHEAT_CYCLELOGIC3.id);
            self.out.p1(50);
        }
    }

    /// `showObject(x, z)` from client-ts (7569): rebuild one tile's stacked
    /// objects after the scene build. An empty cell clears the World object;
    /// otherwise the top-cost object becomes the list head and the
    /// top/middle/bottom of the stack are cloned into the scene. The walks
    /// stay inside a block so the `ground_obj` borrow ends before the World
    /// writes.
    fn show_object(&mut self, x: i32, z: i32) {
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
                node = objs.next();
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
                node = objs.next();
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
                node = objs.next();
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
            Some(SceneModel::Obj(top)),
            middle.map(SceneModel::Obj),
            bottom.map(SceneModel::Obj),
        );
    }

    /// `locChangePostBuildCorrect()` from client-ts (7422): reconcile the
    /// pending loc-change queue with the fresh scene. Permanent changes
    /// (`end_time == -1`) re-snapshot the old appearance and become due
    /// next tick; timed ones are dropped.
    fn loc_change_post_build(&mut self) {
        let mut node = self.loc_changes.head();
        while let Some(loc) = node {
            if loc.end_time == -1 {
                loc.start_time = 0;
                Self::loc_change_set_old(&self.world, loc);
            } else {
                self.loc_changes.unlink_last();
            }
            node = self.loc_changes.next();
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
            next = self.loc_changes.next();
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
                    self.collision[level as usize]
                        .del_wall(x, z, other_shape, other_angle, r#type.blockrange);
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
            if level < 3
                && (self.mapl[1][x as usize][z as usize] as i32 & MapFlag::LINK_BELOW) != 0
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
                        || ClientBuild::change_loc_available(&self.cache, loc.new_type, loc.new_shape))
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

            node = self.loc_changes.next();
        }
    }

    /// `gameLoop` from Java (`Client.java` 9341): count down a pending
    /// logout request, read up to five TCP packets, the side-tab click
    /// pass (TS `iconLoop`), the side-interface button pass
    /// (`buildMinimenu` side branch), the chat key pass (TS
    /// `handleInputKey`), the
    /// in-game silence watchdog (`timeoutTimer > 750` → `lostCon`), then
    /// idle `NO_TIMEOUT` and flush `out` through `ClientStream::write`.
    /// Write errors are `lostCon` (Java `catch (IOException)`).
    pub fn game_loop(&mut self) {
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
        // TS 2191-2192: scene/minimap pass after the inbound reads, before
        // the silence watchdog.
        self.check_minimap();
        self.loc_change_do_queue();
        // TS 2191-2192: the loc-change pass then the world-update counter;
        // headless loops (no draw) keep it zeroed.
        self.world_update_num += 1;
        if !self.draw {
            self.world_update_num = 0;
        }
        self.handle_tab_clicks();
        self.handle_side_if_clicks();
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
                self.tryMove(src_x, src_z, ground_x, ground_z, true, 0, 0, 0, 0, 0, 0);
            }
        }
        self.mouse_loop();
        self.minimap_loop();
        // TS 2346: `followCamera` in the 3D scene — the orbit camera tracks
        // the local player and the arrow keys rotate yaw/pitch.
        if self.scene_state == 2 {
            self.follow_camera();
        }
        self.timeout_timer += 1;
        if self.timeout_timer > 750 {
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

    /// `mainredraw` from Java — the frame render pass: title screen or
    /// in-game draw into `draw_area` (which the `window` feature presents).
    /// `draw` is the CPU-save switch: false skips the render entirely, so a
    /// headless bot burns no pixels while the network machine keeps running.
    pub fn mainredraw(&mut self) {
        if !self.draw {
            return;
        }
        if self.ingame {
            self.game_draw();
        } else {
            self.title_screen_draw();
        }
    }

    /// Set the `draw` CPU-save switch (`mainredraw` skips the frame render
    /// when false).
    pub fn set_draw(&mut self, draw: bool) {
        self.draw = draw;
    }

    /// Drive the 20 ms GameShell machine on the calling thread (spec §3):
    /// one `mainloop` then `mainredraw` per frame with the Java
    /// ratio/count catch-up. `on_loop` runs after each `mainloop` pass so a
    /// driver (client-play) can read Java-public state — e.g. print the
    /// local-player tile for live proof — without a snapshot API.
    ///
    /// With `window` the `Present` drives the frame: events are pumped into
    /// the shell before the mainloop pass (via `latch_click`, GameShell.ts
    /// 186-190), and `draw_area` blits after the redraw. Closing the window
    /// (`poll` false) sets `shell.state = -1`, which stops the machine on
    /// the next iteration like Java `GameShell.run`.
    pub fn run<F: FnMut(&mut Self)>(&mut self, mut on_loop: F) {
        if !self.already_started {
            self.maininit();
        }
        while self.shell.state >= 0 {
            if self.shell.state > 0 {
                self.shell.state -= 1;
                if self.shell.state == 0 {
                    self.shell.stop();
                    return;
                }
            }

            #[cfg(feature = "window")]
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

            self.mainredraw();

            #[cfg(feature = "window")]
            if let Some(present) = self.present.as_mut() {
                present.blit(
                    &self.draw_area.pixels,
                    self.draw_area.width as u32,
                    self.draw_area.height as u32,
                );
            }
        }

        if self.shell.state == -1 {
            self.shell.stop();
        }
    }

    /// `saveMidi(fading, data)` from Java (`Client.java` 6266): hand the
    /// on-demand archive-2 bytes to the backend (signlink midisave). A
    /// jingle (`fading=false`) or the first song plays immediately at the
    /// `midivol` ladder value (Java first-song short-circuit); a zone-song
    /// change (`fading=true`) holds the bytes pending and fades the current
    /// song out, swapping in `music_tick` once the ramp hits the floor.
    pub fn save_midi(&mut self, data: &[u8], fading: bool) {
        if !fading || !self.midi_playing {
            self.midi.lock().unwrap().play(data, self.midi_volume, fading);
            self.fade.lock().unwrap().finish_fade(self.midi_volume);
            self.midi_playing = true;
            self.midi_pending = None;
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
            self.midi.lock().unwrap().play(&data, volume, false);
            self.fade.lock().unwrap().finish_fade(volume);
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

    /// The clientcode switch from `clientVar` (Java `Client.java` 3032).
    /// clientcode 3 is the midi volume ladder: 0 → +0 dB, 1 → -400, 2 → -800,
    /// 3 → -1200, 4 → mute. Mutating the active state re-requests the next
    /// song (unmute) or stops the midi (mute), guarded by `lowmem` as Java.
    pub fn apply_clientcode(&mut self, clientcode: i32, value: i32) {
        if clientcode != 3 {
            return;
        }
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
                    let end = wav.pos as usize;
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
    }

    /// `onDemandLoop()` from client-ts: run the ondemand heartbeat, then
    /// dispatch every completed file by the TS archive cases — models (0),
    /// anim frames (1), midi (2), map squares (3) — and the archive-93 map
    /// prefetch completions. Null data skips the dispatch as TS does.
    pub fn on_demand_loop(&mut self) {
        let mut done = Vec::new();
        if let Some(od) = &mut self.on_demand {
            od.run(self.ingame);
            while let Some(req) = od.loop_request() {
                done.push(req);
            }
        }
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
                            ClientBuild::prefetch_locations(&self.cache, &mut Packet::new(data), od);
                        }
                    }
                }
                _ => {}
            }
        }
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
                self.map_build_ground_data[i] = Some(data);
                return;
            }
            if self.map_build_location_file[i] == file {
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

/// Stand-in for JS `Math.random()` (returns `[0, 1)`), time-seeded like
/// `client_build`'s; `minimapBuildBuffer`'s colour and dot jitter is not
/// reproducible in TS either.
fn random_float() -> f64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ((nanos >> 20) % 1_000_000) as f64 / 1_000_000.0
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
        while c.loc_changes.next().is_some() {
            n += 1;
        }
        assert_eq!(n, 1);
        assert_eq!(c.loc_changes.head().unwrap().start_time, 0);
    }
}
