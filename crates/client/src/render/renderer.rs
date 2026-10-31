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

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::client::client::Client;
use crate::dash3d::BuildArea;
use crate::graphics::{Pix32, Pix3D, Pix3DDraw, Pix8, PixMap};
use crate::render::backend::{
    BackendKind, CpuBackend, FrameKind, FrameOutput, GpuBackend, RenderBackend, RenderFault,
    SceneProgress,
};
use crate::render::budget::{BudgetCharge, RenderBudget, RenderBytes, RenderLimit, RenderResource};
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
    /// Java `Client.textureBuffer`: the fixed 128×128 byte scratch that
    /// `textureRunAnims` swaps with animated textures 17 and 24.
    pub(crate) texture_buffer: Vec<i8>,
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
    pub(crate) graphic_sprites: crate::render::graphic_sprites::GraphicSpriteCache,
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
    last_frame_fault: Option<RenderFault>,
    chat_charge: Option<BudgetCharge>,
}

#[cfg(feature = "render-test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTestRendererLayout {
    pub scalar_state_bytes: usize,
    pub cpu_surface_bytes: usize,
    pub game_surface_bytes: usize,
    pub title_surface_bytes: usize,
    pub pix3d_game_bytes: usize,
    pub texture_buffer_bytes: usize,
    pub tile_occupancy_bytes: usize,
    pub minimap_bytes: usize,
    pub minimap_mask_bytes: usize,
    pub active_map_function_slots: usize,
    pub active_map_function_bytes: usize,
    pub active_map_function_payload_bytes: usize,
    pub image_runes_bytes: usize,
    pub title_icon_bytes: usize,
    pub title_flames_bytes: usize,
    pub chat_slots: usize,
    pub chat_text_capacity_bytes: usize,
    pub max_chat_message_bytes: usize,
}

#[cfg(feature = "render-test-hooks")]
pub fn render_test_content_layout(
    media: &Media,
    textures: Option<&crate::io::JagFile>,
) -> Option<RenderTestRendererLayout> {
    use crate::render::budget::vec_capacity_bytes;

    fn pixmap_bytes(width: i32, height: i32) -> Option<usize> {
        let map = PixMap::new(width, height);
        map.pixels
            .capacity()
            .checked_mul(std::mem::size_of::<i32>())
    }

    fn pix8_bytes(sprite: &Pix8) -> Option<usize> {
        sprite.data.capacity().checked_add(
            sprite
                .bpal
                .capacity()
                .checked_mul(std::mem::size_of::<i32>())?,
        )
    }

    let applet_bytes = pixmap_bytes(
        crate::client::client::APPLET_W,
        crate::client::client::APPLET_H,
    )?;
    let game_dimensions = [
        (512, 334),
        (172, 156),
        (190, 261),
        (479, 96),
        (496, 50),
        (269, 37),
        (249, 45),
    ];
    let game_surface_bytes = game_dimensions
        .into_iter()
        .map(|(width, height)| pixmap_bytes(width, height))
        .try_fold(0usize, |total, bytes| total.checked_add(bytes?))?;
    let title_dimensions = [
        (128, 265),
        (128, 265),
        (509, 171),
        (360, 132),
        (360, 200),
        (202, 238),
        (203, 238),
        (74, 94),
        (75, 94),
    ];
    let title_surface_bytes = title_dimensions
        .into_iter()
        .map(|(width, height)| pixmap_bytes(width, height))
        .try_fold(0usize, |total, bytes| total.checked_add(bytes?))?;

    let mut pix3d = Pix3DDraw::default();
    if let Some(textures) = textures {
        pix3d.unpack_textures(textures);
    }
    pix3d.set_clipping(512, 334);
    pix3d.init_pool(20);
    pix3d.init_texture_palettes(0.8);
    let pix3d_game_bytes = pix3d.retained_storage_bytes();

    let texture_buffer_bytes = vec_capacity_bytes(&vec![0i8; 128 * 128]);
    let tile_occupancy_bytes =
        vec_capacity_bytes(&vec![0i32; (BuildArea::SIZE * BuildArea::SIZE) as usize]);
    let minimap_bytes = vec_capacity_bytes(&Pix32::new(512, 512).data);
    let minimap_mask_bytes = [
        vec_capacity_bytes(&vec![0i32; 33]),
        vec_capacity_bytes(&vec![0i32; 33]),
        vec_capacity_bytes(&vec![0i32; 151]),
        vec_capacity_bytes(&vec![0i32; 151]),
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)?;
    let active_map_function_slots = 1000usize;
    let active_map_function_bytes = active_map_function_slots
        .checked_mul(std::mem::size_of::<Option<Pix32>>())?
        .checked_add(
            active_map_function_slots
                .checked_mul(std::mem::size_of::<i32>())?
                .checked_mul(2)?,
        )?;
    let active_map_function_payload_bytes = active_map_function_slots.checked_mul(
        media
            .mapfunction
            .iter()
            .flatten()
            .map(|sprite| vec_capacity_bytes(&sprite.data))
            .max()
            .unwrap_or(0),
    )?;

    let image_runes_bytes = vec_capacity_bytes(&media.runes).checked_add(
        media
            .runes
            .iter()
            .map(pix8_bytes)
            .try_fold(0usize, |total, bytes| total.checked_add(bytes?))?,
    )?;
    let title_icon_bytes = [&media.titlebox, &media.titlebutton]
        .into_iter()
        .flatten()
        .map(pix8_bytes)
        .try_fold(0usize, |total, bytes| total.checked_add(bytes?))?;
    let title_left = PixMap::new(128, 265);
    let title_right = PixMap::new(128, 265);
    let mut title_flames = crate::client::title_flames::TitleFlames::new(media.runes.clone());
    title_flames.setup_fire(&title_left, &title_right);
    let title_flames_bytes = title_flames.retained_storage_bytes();
    let max_chat_message_bytes = crate::io::Packet::render_test_capacities()
        .into_iter()
        .max()?;
    let mut chat = String::new();
    chat.try_reserve_exact(max_chat_message_bytes).ok()?;
    let chat_text_capacity_bytes = 50usize.checked_mul(chat.capacity())?;

    Some(RenderTestRendererLayout {
        scalar_state_bytes: std::mem::size_of::<Renderer>() + std::mem::size_of::<CpuBackend>(),
        cpu_surface_bytes: applet_bytes.checked_add(game_surface_bytes.max(title_surface_bytes))?,
        game_surface_bytes,
        title_surface_bytes,
        pix3d_game_bytes,
        texture_buffer_bytes,
        tile_occupancy_bytes,
        minimap_bytes,
        minimap_mask_bytes,
        active_map_function_slots,
        active_map_function_bytes,
        active_map_function_payload_bytes,
        image_runes_bytes,
        title_icon_bytes,
        title_flames_bytes,
        chat_slots: 50,
        chat_text_capacity_bytes,
        max_chat_message_bytes,
    })
}

#[cfg(feature = "render-test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTestGraphicSpriteLayout {
    pub unique_keys: usize,
    pub entry_bytes: usize,
    pub key_bytes: usize,
    pub sprite_pixel_bytes: usize,
    pub retained_bytes: usize,
    pub temp_models_peak_bytes: usize,
}

#[cfg(feature = "render-test-hooks")]
pub fn render_test_graphic_sprite_layout(
    cache_dir: &str,
    keys: &[(String, i32)],
) -> Option<RenderTestGraphicSpriteLayout> {
    let layout =
        crate::render::graphic_sprites::render_test_scoped_content_layout(cache_dir, keys)?;
    Some(RenderTestGraphicSpriteLayout {
        unique_keys: layout.unique_keys,
        entry_bytes: layout.entry_bytes,
        key_bytes: layout.key_bytes,
        sprite_pixel_bytes: layout.sprite_pixel_bytes,
        retained_bytes: layout.retained_bytes,
        temp_models_peak_bytes: layout.temp_models_peak_bytes,
    })
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
            texture_buffer: vec![0; 128 * 128],
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
            graphic_sprites: crate::render::graphic_sprites::GraphicSpriteCache::new(),
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
            last_frame_fault: None,
            chat_charge: None,
        };
        Pix3D::init_colour_table(0.8);
        renderer.pix3d.low_mem = lowmem;
        renderer.backend = Some(if prefer_gpu {
            match GpuBackend::try_new() {
                Ok(backend) => Box::new(backend),
                Err(_) => Box::new(CpuBackend::default()),
            }
        } else {
            Box::new(CpuBackend::default())
        });
        #[cfg(feature = "render-diagnostics")]
        {
            let kind = match renderer.backend_kind() {
                crate::render::backend::BackendKind::Cpu => "cpu",
                crate::render::backend::BackendKind::Gpu => "gpu",
            };
            crate::render::diagnostics::backend(kind);
        }
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
    /// Release GPU nav scratch and its target without painting a frame.
    /// Hosts call this on focus loss, including when the head is parked.
    /// CPU rendering keeps its existing behaviour.
    pub fn release_nav_debug_overlay(&mut self) {
        if let Some(backend) = self.backend.as_deref_mut() {
            backend.release_nav_debug_overlay();
        }
    }

    /// Scene progress and validity from the backend for the current paint.
    pub fn scene_progress(&self) -> SceneProgress {
        if self.last_frame_fault.is_some() {
            return if self.has_valid_scene() {
                SceneProgress::Retained
            } else {
                SceneProgress::NoScene
            };
        }
        self.backend
            .as_deref()
            .map(RenderBackend::scene_progress)
            .unwrap_or(SceneProgress::NoScene)
    }

    pub fn has_valid_scene(&self) -> bool {
        self.backend
            .as_deref()
            .is_some_and(RenderBackend::has_valid_scene)
    }

    /// Fault from the last paint, even when a recovered CPU frame is usable.
    pub fn render_fault(&self) -> Option<RenderFault> {
        self.last_frame_fault
    }

    /// Allocations owned only by the CPU clean-scene retention buffer.
    pub fn retained_storage_allocations(&self) -> u64 {
        self.backend
            .as_deref()
            .map_or(0, RenderBackend::retained_storage_allocations)
    }

    pub fn retained_storage_bytes(&self) -> usize {
        self.backend
            .as_deref()
            .map_or(0, RenderBackend::retained_storage_bytes)
    }

    /// Traverse actual capacities only when a diagnostic requests a census.
    /// Shared process rows must be deduplicated by the caller, not summed for
    /// every head. The slot adds its mailbox and retained nav input separately.
    pub fn render_bytes(&self) -> RenderBytes {
        use crate::render::budget::vec_capacity_bytes as bytes;
        let mut sample = self
            .backend
            .as_deref()
            .map_or_else(RenderBytes::default, RenderBackend::render_bytes);
        let maps = [
            &self.image_title0,
            &self.image_title1,
            &self.image_title2,
            &self.image_title3,
            &self.image_title4,
            &self.image_title5,
            &self.image_title6,
            &self.image_title7,
            &self.image_title8,
            &self.area_game,
            &self.area_map,
            &self.area_side,
            &self.area_chat,
            &self.area_backbase1,
            &self.area_backbase2,
            &self.area_backhmid1,
        ];
        sample.cpu_surfaces = bytes(&self.draw_area.pixels)
            + maps
                .into_iter()
                .flatten()
                .map(|map| bytes(&map.pixels))
                .sum::<usize>();
        let (head_models, world_misc) = self.world.retained_render_bytes();
        sample.head_models += head_models;
        sample.head_misc += world_misc
            + self.pix3d.retained_storage_bytes()
            + bytes(&self.texture_buffer)
            + bytes(&self.tile_last_occupied_cycle)
            + bytes(&self.compass_mask_line_offsets)
            + bytes(&self.compass_mask_line_lengths)
            + bytes(&self.minimap_mask_line_offsets)
            + bytes(&self.minimap_mask_line_lengths)
            + bytes(&self.active_map_function_x)
            + bytes(&self.active_map_function_z)
            + bytes(&self.active_map_functions)
            + self
                .active_map_functions
                .iter()
                .flatten()
                .map(|pix| bytes(&pix.data))
                .sum::<usize>()
            + self.minimap.as_ref().map_or(0, |pix| bytes(&pix.data))
            + self.chats.iter().map(String::capacity).sum::<usize>()
            + self.graphic_sprites.retained_bytes()
            + bytes(&self.image_runes)
            + self
                .image_runes
                .iter()
                .map(|pix| bytes(&pix.data) + bytes(&pix.bpal))
                .sum::<usize>()
            + [&self.image_titlebox, &self.image_titlebutton]
                .into_iter()
                .flatten()
                .map(|pix| bytes(&pix.data) + bytes(&pix.bpal))
                .sum::<usize>()
            + self
                .title_flames
                .as_ref()
                .map_or(0, |flames| flames.retained_storage_bytes());
        sample.capture += self
            .pix3d
            .capture
            .as_ref()
            .map_or(0, |capture| capture.retained_bytes());
        let (shared_models, shared_sprites) = crate::render::store::shared_render_bytes();
        sample.shared_models = shared_models;
        sample.shared_sprites = shared_sprites;
        sample.context_cpu += self.media.retained_process_storage_bytes();
        sample.scalar_state += std::mem::size_of::<Self>();
        sample
    }

    pub(crate) fn set_chat_message(
        &mut self,
        index: usize,
        message: &str,
    ) -> Result<(), RenderLimit> {
        if let Some(budget) = RenderBudget::current_resource(RenderResource::HeadMisc) {
            let old_total = self.chats.iter().map(String::capacity).sum::<usize>();
            let charge = match self.chat_charge.as_mut() {
                Some(charge) => {
                    charge.resize(old_total)?;
                    charge
                }
                None => self.chat_charge.insert(budget.reserve(old_total)?),
            };
            if message.len() > self.chats[index].capacity() {
                let peak = old_total.checked_add(message.len()).ok_or_else(|| {
                    RenderLimit::new(RenderResource::HeadMisc, usize::MAX, budget.maximum())
                })?;
                charge.resize(peak)?;
                let mut replacement = String::with_capacity(message.len());
                replacement.push_str(message);
                let old = std::mem::replace(&mut self.chats[index], replacement);
                let final_total = old_total - old.capacity() + self.chats[index].capacity();
                drop(old);
                charge
                    .resize(final_total)
                    .expect("chat replacement only releases reserved bytes");
                return Ok(());
            }
        }
        self.chats[index].clear();
        self.chats[index].push_str(message);
        Ok(())
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
    pub(super) fn render_frame(
        &mut self,
        client: &mut Client,
        kind: FrameKind,
        recovery_fault: Option<RenderLimit>,
    ) -> FrameOutput {
        self.last_frame_fault = recovery_fault
            .or_else(|| client.nav_debug_render_limit())
            .map(RenderFault::Budget);
        if self.backend_kind() == BackendKind::Gpu {
            if let Some(fault) = self.last_frame_fault {
                return FrameOutput::Refused(fault);
            }
        }
        let mut guard = FrameBackend::new(self);
        let backend = guard.backend.as_mut().expect("render backend present");
        let renderer = &mut *guard.renderer;
        backend.begin(client, renderer, kind);
        #[derive(Clone, Copy)]
        struct SceneTraceSample {
            state: i32,
            camera: [i32; 5],
            click: bool,
        }

        let host_render_active = crate::render::backend::cpu::host_overlays_active();
        let trace_sample = if host_render_active {
            client
                .shell
                .ground_trace
                .as_ref()
                .filter(|t| t.active() && t.armed && !t.rendered)
                .map(|_| SceneTraceSample {
                    state: client.scene_state,
                    camera: [
                        client.cam_x,
                        client.cam_y,
                        client.cam_z,
                        client.cam_pitch,
                        client.cam_yaw,
                    ],
                    click: client.world.click,
                })
        } else {
            None
        };
        if !host_render_active {
            if let Some(t) = client
                .shell
                .ground_trace
                .as_mut()
                .filter(|t| t.active() && t.armed && !t.rendered)
            {
                t.event(
                    "scene",
                    &[
                        ("state", client.scene_state as i64),
                        ("camera_x", client.cam_x as i64),
                        ("camera_y", client.cam_y as i64),
                        ("camera_z", client.cam_z as i64),
                        ("pitch", client.cam_pitch as i64),
                        ("yaw", client.cam_yaw as i64),
                        ("click", client.world.click as i64),
                    ],
                );
            }
        }
        backend.scene(client, renderer, kind);
        if host_render_active {
            if backend.scene_progress() == SceneProgress::Rebuilt {
                if let Some(sample) = trace_sample {
                    if let Some(t) = client
                        .shell
                        .ground_trace
                        .as_mut()
                        .filter(|t| t.active() && t.armed && !t.rendered)
                    {
                        let [camera_x, camera_y, camera_z, pitch, yaw] = sample.camera;
                        t.event(
                            "scene",
                            &[
                                ("state", sample.state as i64),
                                ("camera_x", camera_x as i64),
                                ("camera_y", camera_y as i64),
                                ("camera_z", camera_z as i64),
                                ("pitch", pitch as i64),
                                ("yaw", yaw as i64),
                                ("click", sample.click as i64),
                            ],
                        );
                        t.rendered = true;
                        t.event(
                            "post_render",
                            &[
                                ("x", client.world.ground_x as i64),
                                ("z", client.world.ground_z as i64),
                                ("click", client.world.click as i64),
                                ("scene", client.scene_state as i64),
                            ],
                        );
                        if client.world.ground_x == -1 {
                            t.complete("no_pick_first_render");
                        }
                    }
                }
            }
        } else if let Some(t) = client
            .shell
            .ground_trace
            .as_mut()
            .filter(|t| t.active() && t.armed && !t.rendered)
        {
            t.rendered = true;
            t.event(
                "post_render",
                &[
                    ("x", client.world.ground_x as i64),
                    ("z", client.world.ground_z as i64),
                    ("click", client.world.click as i64),
                    ("scene", client.scene_state as i64),
                ],
            );
            if client.world.ground_x == -1 {
                t.complete("no_pick_first_render");
            }
        }
        backend.composite_scene(client, renderer, kind);
        backend.chrome(client, renderer, kind);
        let output = backend.finish(renderer);
        let frame_fault = backend.render_fault().or_else(|| output.render_fault());
        drop(guard);
        self.last_frame_fault = frame_fault;
        output
    }

    /// `gameDraw` from client-ts (3890): the in-game frame. Delegates the
    /// frame stages to the render backend (task 4); the bodies live in
    /// `render/backend/cpu.rs`.
    pub fn game_draw(&mut self, client: &mut Client) -> FrameOutput {
        self.render_frame(client, FrameKind::Game, None)
    }

    /// `titleScreenDraw` from client-ts (1489): the login frame. Delegates
    /// to the render backend (task 4).
    pub fn title_screen_draw(&mut self, client: &mut Client) -> FrameOutput {
        self.render_frame(client, FrameKind::Title, None)
    }
}

#[cfg(test)]
mod owner_bytes_tests {
    use super::*;
    use crate::render::budget::RenderBudgetLimits;

    fn head_misc_budget(maximum: usize) -> RenderBudget {
        RenderBudget::new(RenderBudgetLimits::new(
            0, 0, 0, 0, 0, 0, maximum, 0, 0, 0, 0, 0, 0,
        ))
    }

    #[test]
    fn chat_growth_checks_old_plus_new_and_reuses_warmed_storage() {
        let mut renderer = Renderer::new_prefer(false, false);
        let budget = head_misc_budget(63);
        let baseline = renderer.render_bytes().head_misc();
        let _scope = budget.scope();
        renderer.set_chat_message(0, "abcdefghijklmnop").unwrap();
        renderer
            .set_chat_message(1, "abcdefghijklmnopqrstuvwx")
            .unwrap();
        let first = renderer.chats[0].as_ptr();
        let second = renderer.chats[1].as_ptr();
        assert_eq!(
            budget
                .for_resource(RenderResource::HeadMisc)
                .expect("HeadMisc is a byte-budget resource")
                .live(),
            40
        );
        assert_eq!(renderer.render_bytes().head_misc() - baseline, 40);

        let error = renderer
            .set_chat_message(0, "abcdefghijklmnopqrstuvwxyz012345")
            .unwrap_err();
        assert_eq!(error.resource(), RenderResource::HeadMisc);
        assert_eq!(
            error.requested(),
            72,
            "growth admits old plus replacement, not only final capacity"
        );
        assert_eq!(renderer.chats[0], "abcdefghijklmnop");
        assert_eq!(renderer.chats[0].as_ptr(), first);
        assert_eq!(renderer.chats[1].as_ptr(), second);
        assert_eq!(
            budget
                .for_resource(RenderResource::HeadMisc)
                .expect("HeadMisc is a byte-budget resource")
                .live(),
            40
        );
        for _ in 0..100 {
            renderer.set_chat_message(0, "ponmlkjihgfedcba").unwrap();
            renderer
                .set_chat_message(1, "xwvutsrqponmlkjihgfedcba")
                .unwrap();
            assert_eq!(renderer.chats[0].as_ptr(), first);
            assert_eq!(renderer.chats[1].as_ptr(), second);
        }
        assert_eq!(
            budget
                .for_resource(RenderResource::HeadMisc)
                .expect("HeadMisc is a byte-budget resource")
                .peak(),
            40
        );
        assert_eq!(renderer.render_bytes().head_misc() - baseline, 40);
        drop(renderer);
        assert_eq!(
            budget
                .for_resource(RenderResource::HeadMisc)
                .expect("HeadMisc is a byte-budget resource")
                .live(),
            0
        );
    }

    #[test]
    fn renderer_owner_census_reconciles_live_allocations() {
        const CHILD: &str = "T44_ISOLATED_OWNER_CENSUS";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "render::renderer::owner_bytes_tests::renderer_owner_census_reconciles_live_allocations",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let mut renderer = Renderer::new_prefer(false, false);
        renderer.active_map_functions = Vec::new();
        renderer.image_runes = Vec::new();
        renderer.pix3d.texel_pool = None;
        let baseline = renderer.render_bytes();
        let budget = head_misc_budget(65_536);
        let _scope = budget.scope();
        let _unbacked_charge = budget
            .for_resource(RenderResource::HeadMisc)
            .expect("HeadMisc is a byte-budget resource")
            .reserve(65_536)
            .unwrap();
        assert_eq!(renderer.render_bytes().head_misc(), baseline.head_misc());
        assert_eq!(renderer.render_bytes().head_total(), baseline.head_total());

        let mut sprite = Pix32::new(1, 1);
        sprite.data = Vec::with_capacity(11);
        renderer.active_map_functions = Vec::with_capacity(7);
        renderer.active_map_functions.push(Some(sprite));
        let map_bytes = renderer.active_map_functions.capacity()
            * std::mem::size_of::<Option<Pix32>>()
            + renderer.active_map_functions[0]
                .as_ref()
                .unwrap()
                .data
                .capacity()
                * std::mem::size_of::<i32>();

        let mut rune = Pix8::new(1, 1, Vec::with_capacity(13));
        rune.data = Vec::with_capacity(17);
        renderer.image_runes = Vec::with_capacity(3);
        renderer.image_runes.push(rune);
        let rune_bytes = renderer.image_runes.capacity() * std::mem::size_of::<Pix8>()
            + renderer.image_runes[0].data.capacity()
            + renderer.image_runes[0].bpal.capacity() * std::mem::size_of::<i32>();

        let mut pool = Vec::with_capacity(5);
        pool.push(Vec::<i32>::with_capacity(19));
        let pool_bytes = pool.capacity() * std::mem::size_of::<Vec<i32>>()
            + pool[0].capacity() * std::mem::size_of::<i32>();
        renderer.pix3d.texel_pool = Some(pool);
        let sample = renderer.render_bytes();
        assert_eq!(
            sample.head_misc() - baseline.head_misc(),
            map_bytes + rune_bytes + pool_bytes
        );
        assert_eq!(
            sample.head_total() - baseline.head_total(),
            map_bytes + rune_bytes + pool_bytes
        );
        assert_eq!(
            sample.scalar_state(),
            std::mem::size_of::<Renderer>() + std::mem::size_of::<CpuBackend>()
        );

        let mut surface = PixMap::new(1, 1);
        surface.pixels = Vec::with_capacity(23);
        let surface_bytes = surface.pixels.capacity() * std::mem::size_of::<i32>();
        renderer.area_game = Some(surface);
        assert_eq!(
            renderer.render_bytes().cpu_surfaces() - baseline.cpu_surfaces(),
            surface_bytes
        );
        assert_eq!(
            renderer.render_bytes().head_total() - baseline.head_total(),
            map_bytes + rune_bytes + pool_bytes + surface_bytes
        );
        renderer.active_map_functions = Vec::new();
        renderer.image_runes = Vec::new();
        renderer.pix3d.texel_pool = None;
        renderer.area_game = None;
        assert_eq!(renderer.render_bytes(), baseline);
    }
}
