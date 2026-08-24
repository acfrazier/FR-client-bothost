//! Render-only state, owned by `Client` as `renderer: Renderer`.
//!
//! Everything the draw path (`render/draw.rs`, `scene_loading_splash`,
//! `mainredraw`, present/blit) writes or reads to paint a frame: the CPU
//! framebuffer, the Pix3D raster state, the fonts/sprites, and the
//! title/minimap/HUD paint state. The sim paths (packet apply,
//! `doAction`, `tryMove`, `login`, `mainloop` input) do not touch these
//! fields; fields they share with the draw (crosshair click position,
//! minimenu arrays, `world`/`collision`/`local_player`/`ifaces`/`vars`/
//! `stats`) stay on `Client` until the shared-state step.

use std::collections::HashMap;

use crate::config::if_type::IfType;
use crate::graphics::{Pix32, Pix3DDraw, Pix8, PixFont, PixMap};
use crate::io::JagFile;
use crate::util::JavaRandom;

/// The `Client` render-only field set (see module docs).
pub struct Renderer {
    /// `drawArea` from client-ts: the 765×503 CPU framebuffer every frame
    /// draws into (`client.ts` `titleScreenDraw`/`gameDraw`); blitted to
    /// the window by `Present`.
    pub draw_area: PixMap,
    /// Per-client Pix3D raster state (TS `Pix3D` mutable statics: the
    /// `scanline`, `originX/Y`, `trans`, `cycle`, and the texture pool).
    /// `Pix3D::init_colour_table` stays process-wide; the 3D pass binds
    /// `area_game` to it via `set_clipping` before `World::render_all`.
    pub pix3d: Pix3DDraw,
    /// `title` from client-ts: the title jag with the fonts and sprites.
    pub title: Option<JagFile>,
    /// `p11`/`p12`/`b12`/`q8` from client-ts: the chat and title `PixFont`s
    /// (`p12` also draws the loading splash).
    pub p11: Option<PixFont>,
    pub p12: Option<PixFont>,
    pub b12: Option<PixFont>,
    pub q8: Option<PixFont>,
    /// `imageTitle0..8` from client-ts: the 9 title `PixMap` regions (0/1
    /// are the flame frames — empty here, `TitleFlames` is out of scope).
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
    /// `cross` (TS 1056-1058): the 8 click-crosshair frames from the
    /// `media` pack, `Pix32.depack('cross', i)`. Mode 1 walks plot
    /// `[cross_cycle/100]`, mode 2 ops `[cross_cycle/100 + 4]`; a cache
    /// without the pack leaves the sprites `None` (no-op plot).
    pub cross: [Option<Pix32>; 8],
    /// `projectX`/`projectY` from Java (322-325): the last `getOverlayPos`
    /// screen projection, consumed by `entity_overlays`; -1 when the point
    /// is off the playable scene or behind the camera.
    pub project_x: i32,
    pub project_y: i32,
    /// `hitmarks`/`headicons` from Java (256, 328): the damage hitmark and
    /// overhead prayer sprites from the `media` jag, depacked by
    /// `prepare_game` (Java 5311-5320). `None` entries skip their plot.
    pub hitmarks: [Option<Pix32>; 20],
    pub headicons: [Option<Pix32>; 20],
    /// Overlay chat stack from Java (`MAX_CHATS` 50, 409-433): the bubbles
    /// `entityOverlays` collects per frame and draws over the scene.
    pub chat_count: i32,
    pub chat_x: [i32; 50],
    pub chat_y: [i32; 50],
    pub chat_width: [i32; 50],
    pub chat_height: [i32; 50],
    pub chat_colour: [i32; 50],
    pub chat_effect: [i32; 50],
    pub chat_timer: [i32; 50],
    pub chats: [String; 50],
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
    /// that plot into them. Lazy-allocated on the first `game_draw`; a
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
    /// `scrollbar1`/`scrollbar2` from client-ts (1065-1066): the top and
    /// bottom cap sprites of an interface/chat scrollbar. Missing sprites
    /// are `None` (a cache without the `media` pack); `draw_scrollbar`
    /// still fills the track and grip.
    pub scrollbar1: Option<Pix8>,
    pub scrollbar2: Option<Pix8>,
    /// `sideicons`/`redstone1..2hv` from client-ts (1013, 1068-1093): the
    /// side-tab sprites and the redstone highlight for `active_icon`. The
    /// `*h`/`*v`/`*hv` copies are the same sprites hflip/vflip'd. Missing
    /// sprites are `None` (a cache without the `media` pack).
    pub sideicons: [Option<Pix8>; 13],
    /// `modIcons` from client-ts (267): the gold (@cr1@) and silver (@cr2@)
    /// staff crowns plotted ahead of chat senders in `draw_chat` and
    /// `draw_private_messages`. Missing sprites are `None` (a cache without
    /// the `media` pack); the 14px advance is kept without an icon.
    pub mod_icons: [Option<Pix8>; 2],
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
    /// (5280).
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
    /// `activeMapFunctions` from client-ts (508-513): filled by
    /// `minimapBuildBuffer`; sized 1000 as TS.
    pub active_map_function_count: i32,
    pub active_map_function_x: Vec<i32>,
    pub active_map_function_z: Vec<i32>,
    pub active_map_functions: Vec<Option<Pix32>>,
    /// `chatInterface` from client-ts (480): the synthetic IfType the chat
    /// scrollbar reads/writes (not in the jag), synced to the chat scroll
    /// state by `game_draw`/`draw_chat`.
    pub chat_interface: IfType,
    /// Scrollbar input state (`scrollGrabbed`/`scrollInputPadding`/
    /// `scrollCycle` from client-ts 338-340): `scroll_grabbed` widens the
    /// track hit area to 32 px while held, and `scroll_cycle` is the
    /// mouse-held repeat (set from `shell.mouse_button` at the top of
    /// `game_draw`, since the TS GameShell already ticks it).
    pub scroll_grabbed: bool,
    pub scroll_input_padding: i32,
    pub scroll_cycle: i32,
    /// `rand` from client-ts: the `Math.random` source the camera-shake
    /// jitter uses (`cam_follow`).
    pub rand: JavaRandom,
    /// `cycleLogic1` from client-ts: the loc-change scene cycle counter,
    /// sent as the `cyclelogic1` anticheat payload from `add_projectiles`.
    pub cyclelogic1: i32,
    /// `Client.cyclelogic3` from client-ts (a TS static, instance here):
    /// anticheat counter sent with `ANTICHEAT_CYCLELOGIC3` every 113
    /// `minimapBuildBuffer` runs.
    pub cyclelogic3: i32,
}
