//! Render-only state, held by the driver beside its `Client` (task 2b: the
//! draw path in `render/draw.rs`, `scene_loading_splash`, `mainredraw`,
//! frame present) writes or reads to paint a frame: the CPU framebuffer, the
//! Pix3D raster state, the fonts/sprites, and the title/minimap/HUD paint
//! state. The sim paths (packet apply, `doAction`, `tryMove`, `login`,
//! `mainloop` input) do not touch these fields; fields they share with the
//! draw (crosshair click position, minimenu arrays, the scrollbar input
//! state, `world`/`collision`/`local_player`/`ifaces`/`vars`/`stats`) stay
//! on `Client` until the shared-state step.
//!
//! Task 4: the frame-stage rasterization lives in `render/backend`
//! (`CpuBackend` behind the `RenderBackend` trait); this struct is the
//! state it draws into, and `game_draw`/`title_screen_draw` delegate to
//! the `backend` field.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::client::client::Client;
use crate::dash3d::BuildArea;
use crate::graphics::{Pix32, Pix3D, Pix3DDraw, Pix8, PixMap};
use crate::render::backend::{
    BackendKind, CpuBackend, FrameKind, FrameOutput, GpuBackend, RenderBackend,
};
use crate::render::media::Media;
use crate::render::world::RenderWorld;
use crate::util::JavaRandom;

/// Process-wide GPU preference (task 7): the window driver sets it; the
/// backend selection in `Renderer::new` consults it. Headless builds and
/// tests never set it, so `CpuBackend` stays the default test path.
static PREFER_GPU: AtomicBool = AtomicBool::new(false);

/// How many `Renderer`s this process has constructed (task 8). The
/// headless proof (`tests/headless.rs`) asserts a pure `Client` run keeps
/// it at 0 — no renderer, no `RenderBackend`, no wgpu device.
static RENDERER_CONSTRUCTED: AtomicUsize = AtomicUsize::new(0);

fn prefer_gpu() -> bool {
    if std::env::var("BOT_CPU").map(|v| v == "1").unwrap_or(false) {
        return false;
    }
    PREFER_GPU.load(Ordering::Relaxed)
}

/// The `Client` render-only field set (see module docs).
pub struct Renderer {
    /// The render half of the scene world (Task 3): the 3D-pass machinery
    /// (`render_all`/`fill`, visibility backing, occluder selection,
    /// minimap ground pass). The per-tile data it draws lives in
    /// `Client.world` (the sim half); the render methods take it as a
    /// parameter.
    pub world: RenderWorld,
    /// `drawArea` from client-ts: the 765×503 CPU framebuffer every frame
    /// draws into (`client.ts` `titleScreenDraw`/`gameDraw`); handed to the
    /// present target (`Window`/`Textures`) by the frame consumer.
    pub draw_area: PixMap,
    /// Per-client Pix3D raster state (TS `Pix3D` mutable statics: the
    /// `scanline`, `originX/Y`, `trans`, `cycle`, and the texture pool).
    /// `Pix3D::init_colour_table` stays process-wide; the 3D pass binds
    /// `area_game` to it via `set_clipping` before `World::render_all`.
    pub pix3d: Pix3DDraw,
    /// The process-wide fonts + static media sprites (task 6: one copy of
    /// immutable; attaching a head clones a pointer). The first frame
    /// swaps this for the shared depacked copy; a never-drawn renderer
    /// keeps the shared empty placeholder.
    pub media: Arc<Media>,
    /// `imageTitle0..8` from client-ts: the 9 title `PixMap` regions (0/1
    /// are the flame frames — the per-head copies the torch paints into;
    /// `image_title2` is the per-head reallocation gate).
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
    /// `projectX`/`projectY` from Java (322-325): the last `getOverlayPos`
    /// screen projection, consumed by `entity_overlays`; -1 when the point
    /// is off the playable scene or behind the camera.
    pub project_x: i32,
    pub project_y: i32,
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
    /// viewport/side/chat/chrome `PixMap` areas the `media` sprites plot
    /// into. Lazy-allocated on the first `game_draw`; a missing `media`
    /// pack leaves the areas black. The `areaBack*` strips themselves are
    /// shared (`Media::area_back*`).
    pub area_game: Option<PixMap>,
    pub area_map: Option<PixMap>,
    pub area_side: Option<PixMap>,
    pub area_chat: Option<PixMap>,
    pub area_backbase1: Option<PixMap>,
    pub area_backbase2: Option<PixMap>,
    pub area_backhmid1: Option<PixMap>,
    /// Depacked `TYPE_GRAPHIC` sprites (`IfType.graphic`/`graphic2` from
    /// client-ts), keyed by the `"name,index"` of `graphic_name`/
    /// `graphic2_name`. Filled lazily from `{cache_dir}/media` by
    /// `draw_interface`; a failed depack caches as `None` so a missing
    /// sprite does not re-read the jag every draw.
    pub graphic_sprites: HashMap<(String, i32), Option<Pix32>>,
    /// Minimap state (`Client.ts` `minimapDraw` 11279): the composed map
    /// buffer (the compass/mapedge/mapdot/mapmarker sprites and `mapback`
    /// ring are the shared `Media::*`) and the per-row scanline masks
    /// built from `Media::mapback.data` (TS 1180-1216). `minimap` is
    /// allocated as TS maininit (868) does and composed by
    /// `minimap_build_buffer` (5280).
    pub minimap: Option<Pix32>,
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
    /// The rasterizer this renderer routes frames through (task 4). Held
    /// in an `Option` so a frame can temporarily take the backend out (see
    /// `FrameBackend`): the stage methods borrow the renderer's own state,
    /// which a live borrow of `self.backend` would conflict with. The
    /// guard puts it back on drop, so a stage panic cannot leave it `None`.
    backend: Option<Box<dyn RenderBackend>>,
}

/// Takes `Renderer::backend` out of the struct for the duration of a frame
/// and holds the renderer so the stage methods can borrow it; re-installs
/// the backend on drop. If a stage panics, the unwind runs this `Drop`
/// before leaving `render_frame`, so `Renderer::backend` is never left
/// `None` by a caught panic.
struct FrameBackend<'a> {
    renderer: &'a mut Renderer,
    backend: Option<Box<dyn RenderBackend>>,
}

impl<'a> FrameBackend<'a> {
    fn new(renderer: &'a mut Renderer) -> FrameBackend<'a> {
        FrameBackend {
            backend: renderer.backend.take(),
            renderer,
        }
    }
}

impl Drop for FrameBackend<'_> {
    fn drop(&mut self) {
        self.renderer.backend = self.backend.take();
    }
}

impl Renderer {
    /// Construct the renderer the driver holds beside its `Client` (the
    /// task-2b shape: `client.mainloop()` / `renderer.redraw(&mut client)`).
    /// `lowmem` mirrors the config the way `Client::new` used to; the
    /// process-wide `Pix3D::init_colour_table(0.8)` also moves here so the
    /// first shaded triangle of any 3D pass has a table.
    ///
    /// Backend selection (task 7): when the driver asked for a GPU
    /// (`set_prefer_gpu`), select `GpuBackend` first and fall back to
    /// `CpuBackend` on any wgpu init/device failure (logged once, never
    /// fatal). Headless builds and tests never set the preference, so the
    /// CPU path stays the default and the fidelity path.
    pub fn new(lowmem: bool) -> Self {
        Self::new_prefer(lowmem, prefer_gpu())
    }

    /// Like [`Self::new`] but the GPU/CPU choice is per-slot, not the
    /// process `BOT_CPU` / `set_prefer_gpu` latch.
    pub fn new_prefer(lowmem: bool, prefer_gpu: bool) -> Self {
        RENDERER_CONSTRUCTED.fetch_add(1, Ordering::Relaxed);
        let mut renderer = Renderer {
            world: RenderWorld::new(),
            draw_area: PixMap::new(
                crate::client::client::APPLET_W,
                crate::client::client::APPLET_H,
            ),
            pix3d: Pix3DDraw::default(),
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
            project_x: -1,
            project_y: -1,
            chat_count: 0,
            chat_x: [0; 50],
            chat_y: [0; 50],
            chat_width: [0; 50],
            chat_height: [0; 50],
            chat_colour: [0; 50],
            chat_effect: [0; 50],
            chat_timer: [0; 50],
            chats: [const { String::new() }; 50],
            scene_cycle: 0,
            tile_last_occupied_cycle: vec![0; (BuildArea::SIZE * BuildArea::SIZE) as usize],
            vis_calc_done: false,
            area_game: None,
            area_map: None,
            area_side: None,
            area_chat: None,
            area_backbase1: None,
            area_backbase2: None,
            area_backhmid1: None,
            graphic_sprites: HashMap::new(),
            minimap: Some(Pix32::new(512, 512)),
            compass_mask_line_offsets: Vec::new(),
            compass_mask_line_lengths: Vec::new(),
            minimap_mask_line_offsets: Vec::new(),
            minimap_mask_line_lengths: Vec::new(),
            active_map_function_count: 0,
            active_map_function_x: vec![0; 1000],
            active_map_function_z: vec![0; 1000],
            active_map_functions: vec![None; 1000],
            rand: JavaRandom::now(),
            cyclelogic1: 0,
            cyclelogic3: 0,
            media: Media::shared_empty(),
            backend: None,
        };
        Pix3D::init_colour_table(0.8);
        renderer.pix3d.low_mem = lowmem;
        renderer.backend = Some(if prefer_gpu {
            match GpuBackend::try_new() {
                Ok(backend) => Box::new(backend),
                Err(_) => Box::new(CpuBackend),
            }
        } else {
            Box::new(CpuBackend)
        });
        renderer
    }

    /// The window driver opts a process into the wgpu backend; headless
    /// builds and tests never call it, so `Renderer::new` keeps the CPU
    /// fidelity path by default.
    pub fn set_prefer_gpu(prefer: bool) {
        PREFER_GPU.store(prefer, Ordering::Relaxed);
    }

    /// `Renderer`s constructed in this process (the task-8 headless
    /// counter; `with_backend` routes through `new` too).
    pub fn constructed() -> usize {
        RENDERER_CONSTRUCTED.load(Ordering::Relaxed)
    }

    /// Which backend this renderer routes frames through (the selection
    /// test and the window driver use it to log the active path).
    pub fn backend_kind(&self) -> BackendKind {
        self.backend
            .as_deref()
            .map(RenderBackend::kind)
            .unwrap_or(BackendKind::Cpu)
    }

    /// Construct a renderer that routes frames through `backend` instead
    /// of the default `CpuBackend` (the `renderer_backend_selection` test
    /// injects a stub this way).
    pub fn with_backend(backend: Box<dyn RenderBackend>, lowmem: bool) -> Self {
        let mut renderer = Renderer::new(lowmem);
        renderer.backend = Some(backend);
        renderer
    }

    /// Run one frame through the backend: `begin` → `scene` →
    /// `composite_scene` → `chrome` → `finish`, and return the
    /// backend-owned output. `FrameBackend` holds the renderer's own state
    /// for the stage calls and re-installs the backend on drop (normal or
    /// unwinding).
    fn render_frame(&mut self, client: &mut Client, kind: FrameKind) -> FrameOutput {
        let mut guard = FrameBackend::new(self);
        let backend = guard.backend.as_mut().expect("render backend present");
        let renderer = &mut *guard.renderer;
        backend.begin(client, renderer, kind);
        backend.scene(client, renderer, kind);
        backend.composite_scene(client, renderer, kind);
        backend.chrome(client, renderer, kind);
        let output = backend.finish(renderer);
        drop(guard);
        output
    }

    /// `gameDraw` from client-ts (3890): the in-game frame. Delegates the
    /// frame stages to the render backend (task 4); the bodies live in
    /// `render/backend/cpu.rs`.
    pub fn game_draw(&mut self, client: &mut Client) -> FrameOutput {
        self.render_frame(client, FrameKind::Game)
    }

    /// `titleScreenDraw` from client-ts (1489): the login frame. Delegates
    /// to the render backend (task 4).
    pub fn title_screen_draw(&mut self, client: &mut Client) -> FrameOutput {
        self.render_frame(client, FrameKind::Title)
    }
}
