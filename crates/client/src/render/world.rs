//! The **render half** of the scene world (Task 3 split of
//! `dash3d/world.rs`), owned by `Renderer`. It holds the 3D-pass machinery
//! the TS `World` statics describe: the visibility backing
//! (`resetVisCalc`), the fill queue and camera, the occluder selection,
//! the sprite draw buffer, the ground draw vertex scratch, the ground
//! minimap pass (`render2DGround`) and the share-light merge scratch is
//! *not* here (see below). The per-tile data — typecodes, models, the
//! sprite arena, occluders, `occlusion_cycle`, `groundh` — lives in the
//! sim half (`core::world::World`); every render method takes that world
//! and reads the tiles through it, exactly as the brief's "the renderer
//! reads `Client.world` for typecodes".
//!
//! `share_light`, `update_mouse_picking` and the pick state
//! (`click`/`ground_x/z`) stay on the sim half: `ClientBuild::finish_build`
//! runs `share_light` inside the renderer-free sim loop, and `doAction`/
//! `game_loop` arm and consume the ground pick across the render pass.

// Ported verbatim from dash3d/world.rs (the TS port keeps these structures);
// the dash3d module-level clippy allows follow the code to its new home.
#![allow(clippy::if_same_then_else)]
#![allow(clippy::manual_clamp)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::too_many_arguments)]

use std::collections::VecDeque;

use crate::config::Cache;
use crate::core::world::{
    ground_h, tile_at, tile_at_mut, World, MAX_ACTIVE_OCCLUDERS, MAX_OCCLUDERS, OCCLUDER_LEVELS,
};
use crate::dash3d::{
    wrapping_cross, ClientLocAnim, ClientObj, Ground, LocAngle, LocShape, Model, QuickGround,
    SceneModel,
};
use crate::graphics::{Pix2D, Pix3D, Pix3DDraw};

const MAX_SPRITE_BUFFER: usize = 100;

/// `World.visBacking[8][32][51][51]` flat row offset of the pitch/yaw pair
/// that `render_all` binds as `visBackingDirty`.
const VIS_ROW_SIZE: usize = 51 * 51;

// prettier-ignore
const PRETAB: [i32; 9] = [19, 55, 38, 155, 255, 110, 137, 205, 76];
// prettier-ignore
const MIDTAB: [i32; 9] = [160, 192, 80, 96, 0, 144, 80, 48, 160];
// prettier-ignore
const POSTTAB: [i32; 9] = [76, 8, 137, 4, 0, 1, 38, 2, 19];

// prettier-ignore
const MIDDEP_16: [i32; 9] = [0, 0, 2, 0, 0, 2, 1, 1, 0];
// prettier-ignore
const MIDDEP_32: [i32; 9] = [2, 0, 0, 2, 0, 0, 0, 4, 4];
// prettier-ignore
const MIDDEP_64: [i32; 9] = [0, 4, 4, 8, 0, 0, 8, 0, 0];
// prettier-ignore
const MIDDEP_128: [i32; 9] = [1, 1, 0, 0, 0, 8, 0, 0, 8];

// prettier-ignore
const DECORXOF: [i32; 4] = [53, -53, -53, 53];
// prettier-ignore
const DECORZOF: [i32; 4] = [-53, -53, 53, 53];
// prettier-ignore
const DECORXOF2: [i32; 4] = [-45, 45, 45, -45];
// prettier-ignore
const DECORZOF2: [i32; 4] = [45, 45, -45, -45];

// prettier-ignore
const TEXTURE_AVERAGE: [i32; 50] = [
    41, 39248, // water
    41, 4643, // planks
    41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 43086, // marble
    41, 41, 41, 41, 41, 41, 41, 8602, // mossybricks
    41, 28992, // gungywater
    41, 41, 41, 41, 41, 5056, // lava
    41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 3131, // pebblefloor
    41, 41, 41,
];

// prettier-ignore
/// `MINIMAP_SHAPE` from World.ts 39-51: the minimap overlay masks, indexed
/// by `Ground.overlay_shape` (TerrainOverlayShape).
const MINIMAP_SHAPE: [[i32; 16]; 13] = [
    [0; 16],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1], // PLAIN_SHAPE
    [1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1], // DIAGONAL_SHAPE
    [1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], // LEFT_SEMI_DIAGONAL_SMALL_SHAPE
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1], // RIGHT_SEMI_DIAGONAL_SMALL_SHAPE
    [0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1], // LEFT_SEMI_DIAGONAL_BIG_SHAPE
    [1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1], // RIGHT_SEMI_DIAGONAL_BIG_SHAPE
    [1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0], // HALF_SQUARE_SHAPE
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0], // CORNER_SMALL_SHAPE
    [1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0, 1, 1], // CORNER_BIG_SHAPE
    [1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], // FAN_SMALL_SHAPE
    [0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1], // FAN_BIG_SHAPE
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 1, 1, 1], // TRAPEZIUM_SHAPE
];

// prettier-ignore
/// `MINIMAP_ROTATE` from World.ts 53-61: the minimap overlay rotations,
/// indexed by `Ground.overlay_rotation`.
const MINIMAP_ROTATE: [[usize; 16]; 4] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [12, 8, 4, 0, 13, 9, 5, 1, 14, 10, 6, 2, 15, 11, 7, 3],
    [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    [3, 7, 11, 15, 2, 6, 10, 14, 1, 5, 9, 13, 0, 4, 8, 12],
];

/// The render-pass half of the scene world: per-frame camera/fill state,
/// the visibility backing and the occlusion/picking scratch. Owned by
/// `Renderer`; every method takes the sim `World` for the per-tile data.
pub struct RenderWorld {
    camera_sin_x: i32,
    camera_cos_x: i32,
    camera_sin_y: i32,
    camera_cos_y: i32,
    fill_left: i32,
    /// TS `World.fillQueue` (`LinkList<Square>`): `(level, x, z, stamp)`.
    /// `LinkList.push` unlinks a Square already in the list and appends it
    /// at the tail; `fill_stamp` plus this deque stamp drop older copies.
    fill_queue: VecDeque<(i32, i32, i32, i32)>,
    fill_gen: i32,
    max_level: i32,
    /// Frames rendered: increments once per `render_all` (the TS
    /// `World.cycleNo` static). The draw tests use it to prove the 3D pass
    /// ran.
    cycle_no: i32,
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
    gx: i32,
    gz: i32,
    cx: i32,
    cy: i32,
    cz: i32,
    /// TS `World.visBacking`, flat `[8][32][51][51]`; written by the
    /// deferred `resetVisCalc` port, read here via `vis_backing_dirty`.
    vis_backing: Vec<bool>,
    /// Flat row offset (into `vis_backing`) of the current pitch/yaw, or
    /// `None` for the TS `null` (every visibility test reads false).
    vis_backing_dirty: Option<usize>,
    /// TS `World.xClip/yClip/xClip2/yClip2/xOrig/yOrig`: the viewport
    /// `resetVisCalc`'s `testPoint` projects against.
    x_clip: i32,
    y_clip: i32,
    x_clip2: i32,
    y_clip2: i32,
    x_orig: i32,
    y_orig: i32,
    num_active_occluders: i32,
    /// TS `World.activeOccluders`, as indices into the sim world's
    /// `occluders` arena.
    active_occluders: Vec<Option<usize>>,
    /// TS `World.spriteBuffer`, as indices into the sim world's `sprites`.
    sprite_buffer: Vec<Option<usize>>,
    /// TS `Ground.drawVertexX/Y` and `drawTextureVertexX/Y/Z` statics.
    ground_draw_vertex_x: [i32; 6],
    ground_draw_vertex_y: [i32; 6],
    ground_draw_texture_vertex_x: [i32; 6],
    ground_draw_texture_vertex_y: [i32; 6],
    ground_draw_texture_vertex_z: [i32; 6],
    /// Task 3b lazy model cache. The sim world keeps only typecodes and
    /// placement; the meshes/sprites are decoded from the config `Cache`
    /// on first draw, keyed by tile (walls/decor/ground-decor/objects) or
    /// by sprite-arena index, and re-decoded when the tile's `model_stamp`
    /// (or a scene sprite's `model_stamp`) changes. Cleared whenever the
    /// sim world's `build_generation` changes, so a headless client that
    /// never draws never decodes a model.
    tile_models: Vec<Option<Box<TileModels>>>,
    /// The same cache for `Square.linked_square` walls (pushed-down
    /// level-0 content); never mutated after the build, so each slot
    /// resolves once.
    linked_models: Vec<Option<Box<TileModels>>>,
    /// Parallel to the sim `World.sprites` arena: each sprite's resolved
    /// `SceneModel`. Scene sprites decode from their typecode; dynamic
    /// sprites (players/npcs/projectiles/spot-anims) get their model
    /// attached by the entity passes in `render/draw.rs`.
    sprite_models: Vec<Option<SceneModel>>,
    /// Parallel to the sim arena: the `Sprite.model_stamp` each
    /// `sprite_models` entry was resolved for (`i32::MIN` = unresolved).
    sprite_stamps: Vec<i32>,
    /// TS `World.shareTic`/`shareMap`/`shareMap2` (World.ts 145-147):
    /// the share-light merge scratch (moved here with the pass).
    share_tic: i32,
    share_map: Vec<i32>,
    share_map2: Vec<i32>,
    /// The sim `World.build_generation` the caches were last cleared for.
    last_build_generation: u64,
}

/// The lazily-resolved per-tile models (Task 3b), held by `RenderWorld`
/// keyed by the sim tile grid coordinates. `model_stamp` records the tile
/// `Square.model_stamp` the fields were resolved for (`i32::MIN` = never
/// resolved), so LOC_ANIM / loc-change / `show_object` mutations on the
/// sim side invalidate the cache through the tile stamp.
#[derive(Default)]
struct TileModels {
    model_stamp: i32,
    wall_model1: Option<SceneModel>,
    wall_model2: Option<SceneModel>,
    decor_model: Option<SceneModel>,
    gd_model: Option<SceneModel>,
    obj_bottom: Option<SceneModel>,
    obj_middle: Option<SceneModel>,
    obj_top: Option<SceneModel>,
}

impl RenderWorld {
    pub fn new() -> Self {
        RenderWorld {
            camera_sin_x: 0,
            camera_cos_x: 0,
            camera_sin_y: 0,
            camera_cos_y: 0,
            fill_left: 0,
            fill_queue: VecDeque::new(),
            fill_gen: 0,
            max_level: 0,
            cycle_no: 0,
            min_x: 0,
            max_x: 0,
            min_z: 0,
            max_z: 0,
            gx: 0,
            gz: 0,
            cx: 0,
            cy: 0,
            cz: 0,
            vis_backing: vec![false; 8 * 32 * VIS_ROW_SIZE],
            vis_backing_dirty: None,
            x_clip: 0,
            y_clip: 0,
            x_clip2: 0,
            y_clip2: 0,
            x_orig: 0,
            y_orig: 0,
            num_active_occluders: 0,
            active_occluders: vec![None; MAX_ACTIVE_OCCLUDERS],
            sprite_buffer: vec![None; MAX_SPRITE_BUFFER],
            ground_draw_vertex_x: [0; 6],
            ground_draw_vertex_y: [0; 6],
            ground_draw_texture_vertex_x: [0; 6],
            ground_draw_texture_vertex_y: [0; 6],
            ground_draw_texture_vertex_z: [0; 6],
            tile_models: Vec::new(),
            linked_models: Vec::new(),
            sprite_models: Vec::new(),
            sprite_stamps: Vec::new(),
            share_tic: 0,
            share_map: Vec::new(),
            share_map2: Vec::new(),
            last_build_generation: 0,
        }
    }

    /// `render2DGround(level, x, z, dst, offset, step)` from World.ts
    /// 798-856: plot one tile's minimap colours into `dst` (the 512×512
    /// minimap buffer). A `QuickGround` tile fills a solid 4×4 block with
    /// `minimap_rgb`; a `Ground` tile plots the overlay/underlay through
    /// the `MINIMAP_SHAPE`/`MINIMAP_ROTATE` masks. `offset` is the first
    /// pixel, `step` the row stride (512).
    pub fn render_2d_ground(
        &self,
        world: &World,
        level: i32,
        x: i32,
        z: i32,
        dst: &mut [i32],
        mut offset: i32,
        step: i32,
    ) {
        let Some(tile) = world.squares[level as usize][x as usize][z as usize].as_ref() else {
            return;
        };

        if let Some(quick_ground) = &tile.quick_ground {
            let rgb = quick_ground.minimap_rgb;
            if rgb != 0 {
                for _ in 0..4 {
                    dst[offset as usize] = rgb;
                    dst[offset as usize + 1] = rgb;
                    dst[offset as usize + 2] = rgb;
                    dst[offset as usize + 3] = rgb;
                    offset += step;
                }
            }
            return;
        }

        if let Some(ground) = &tile.ground {
            let shape = ground.overlay_shape;
            let rotation = ground.overlay_rotation;
            let overlay = ground.minimap_overlay;
            let underlay = ground.minimap_underlay;
            let minimap_shape = MINIMAP_SHAPE[shape as usize];
            let minimap_rotation = MINIMAP_ROTATE[rotation as usize];

            let mut off = 0usize;
            if overlay != 0 {
                for _ in 0..4 {
                    dst[offset as usize] = if minimap_shape[minimap_rotation[off]] == 0 {
                        overlay
                    } else {
                        underlay
                    };
                    off += 1;
                    dst[offset as usize + 1] = if minimap_shape[minimap_rotation[off]] == 0 {
                        overlay
                    } else {
                        underlay
                    };
                    off += 1;
                    dst[offset as usize + 2] = if minimap_shape[minimap_rotation[off]] == 0 {
                        overlay
                    } else {
                        underlay
                    };
                    off += 1;
                    dst[offset as usize + 3] = if minimap_shape[minimap_rotation[off]] == 0 {
                        overlay
                    } else {
                        underlay
                    };
                    off += 1;
                    offset += step;
                }
                return;
            }

            for _ in 0..4 {
                if minimap_shape[minimap_rotation[off]] != 0 {
                    dst[offset as usize] = underlay;
                }
                off += 1;
                if minimap_shape[minimap_rotation[off]] != 0 {
                    dst[offset as usize + 1] = underlay;
                }
                off += 1;
                if minimap_shape[minimap_rotation[off]] != 0 {
                    dst[offset as usize + 2] = underlay;
                }
                off += 1;
                if minimap_shape[minimap_rotation[off]] != 0 {
                    dst[offset as usize + 3] = underlay;
                }
                off += 1;
                offset += step;
            }
        }
    }

    /// `resetVisCalc(pitchDistance, frustumStart, frustumEnd, viewportWidth,
    /// viewportHeight)` from client-ts (World.ts 858): precompute
    /// `vis_backing` for every pitch/yaw pair by projecting a ±26-tile grid
    /// into the viewport and eroding with the 3×3 pitch/yaw-neighbourhood
    /// merge pass. TS calls it once per game load (`Client.ts` loadGame);
    /// `render_all` binds the current pitch/yaw row via `vis_backing_dirty`.
    /// The `pitch_distance[pitchLevel]` argument is the camera distance for
    /// that pitch level (the TS `distance` table at Client.ts 1225).
    pub fn reset_vis_calc(
        &mut self,
        pitch_distance: &[i32],
        frustum_start: i32,
        frustum_end: i32,
        viewport_width: i32,
        viewport_height: i32,
    ) {
        self.x_clip = 0;
        self.y_clip = 0;
        self.x_clip2 = viewport_width;
        self.y_clip2 = viewport_height;
        self.x_orig = viewport_width / 2;
        self.y_orig = viewport_height / 2;

        // scratch[9][32][53][53]: the ±26-tile sample grid with a 1-tile
        // margin so the merge pass can read its 3×3 neighbourhood.
        let mut scratch = vec![false; 9 * 32 * 53 * 53];
        for pitch in (128..=384).step_by(32) {
            self.camera_sin_x = Pix3D::sin_table()[pitch as usize];
            self.camera_cos_x = Pix3D::cos_table()[pitch as usize];
            for yaw in (0..2048).step_by(64) {
                self.camera_sin_y = Pix3D::sin_table()[yaw as usize];
                self.camera_cos_y = Pix3D::cos_table()[yaw as usize];

                let pitch_level = ((pitch - 128) / 32) as usize;
                let yaw_level = (yaw / 64) as usize;
                let distance = pitch_distance[pitch_level];
                for dx in -26..=26 {
                    for dz in -26..=26 {
                        let x = dx * 128;
                        let z = dz * 128;
                        let mut visible = false;
                        let mut y = -frustum_start;
                        while y <= frustum_end {
                            if self.test_point(x, z, distance + y) {
                                visible = true;
                                break;
                            }
                            y += 128;
                        }
                        scratch[(pitch_level * 32 + yaw_level) * 53 * 53
                            + (dx + 26) as usize * 53
                            + (dz + 26) as usize] = visible;
                    }
                }
            }
        }

        // TS 894-929: a backing cell is visible if any sampled cell in its
        // 3×3 neighbourhood is visible from this or the adjacent pitch/yaw
        // (`(yawLevel + 1) % 31` — the modulo is 31 in the TS, kept as-is).
        for pitch_level in 0..8 {
            for yaw_level in 0..32 {
                for x in -25..25 {
                    for z in -25..25 {
                        let mut visible = false;
                        'check: for dx in -1..=1 {
                            for dz in -1..=1 {
                                let xi = (x + dx + 26) as usize;
                                let zi = (z + dz + 26) as usize;
                                for (pl, yl) in [
                                    (pitch_level, yaw_level),
                                    (pitch_level, (yaw_level + 1) % 31),
                                    (pitch_level + 1, yaw_level),
                                    (pitch_level + 1, (yaw_level + 1) % 31),
                                ] {
                                    if scratch[(pl * 32 + yl) * 53 * 53 + xi * 53 + zi] {
                                        visible = true;
                                        break 'check;
                                    }
                                }
                            }
                        }
                        let idx = (pitch_level * 32 + yaw_level) * VIS_ROW_SIZE
                            + (x + 25) as usize * 51
                            + (z + 25) as usize;
                        self.vis_backing[idx] = visible;
                    }
                }
            }
        }
    }

    /// `World.testPoint` from client-ts (931): project a scene point into
    /// the `resetVisCalc` viewport and answer whether it lands inside it.
    fn test_point(&self, x: i32, z: i32, y: i32) -> bool {
        let px = (z * self.camera_sin_y + x * self.camera_cos_y) >> 16;
        let tmp = (z * self.camera_cos_y - x * self.camera_sin_y) >> 16;
        let pz = (y * self.camera_sin_x + tmp * self.camera_cos_x) >> 16;
        let py = (y * self.camera_cos_x - tmp * self.camera_sin_x) >> 16;

        if pz < 50 || pz > 3500 {
            return false;
        }

        let viewport_x = self.x_orig + ((px << 9) / pz);
        let viewport_y = self.y_orig + ((py << 9) / pz);
        viewport_x >= self.x_clip
            && viewport_x <= self.x_clip2
            && viewport_y >= self.y_clip
            && viewport_y <= self.y_clip2
    }

    /// Frames rendered: `cycle_no` increments once per `render_all` (the TS
    /// `World.cycleNo` static). The draw tests use it to prove the 3D pass
    /// ran.
    pub fn render_count(&self) -> i32 {
        self.cycle_no
    }

    /// `removeSprites` from client-ts: delete every dynamic sprite and
    /// clear the dynamic arena (run once per frame by the render pass, the
    /// TS `gameDrawMain`).
    pub fn remove_sprites(&mut self, world: &mut World) {
        let dynamic: Vec<usize> = world.dynamic_sprites[..world.dynamic_count as usize]
            .iter()
            .flatten()
            .copied()
            .collect();
        for index in dynamic {
            world.del_sprite(index);
            if let Some(slot) = self.sprite_models.get_mut(index) {
                *slot = None;
            }
        }
        for slot in world.dynamic_sprites.iter_mut() {
            *slot = None;
        }
        world.dynamic_count = 0;
    }

    // --- Task 3b: lazy per-tile model resolution ---
    //
    // The sim world stores only typecodes, placement and the small decode
    // ints; every mesh/sprite is decoded here from the config `Cache` on
    // first draw and cached for the life of the build. `resolve_*` mirrors
    // the `ClientBuild.addLoc` model branches exactly (shape/angle decode
    // mapping, the animated-loc `ClientLocAnim` construction and the
    // packet-time heights the sim recorded). The draw tests inject models
    // with `set_wall_model`/`set_sprite_model`; those slots carry the
    // tile's current `model_stamp`, so resolution skips them.

    /// Flat index into `tile_models`/`linked_models` for a tile.
    fn tile_index(&self, world: &World, level: i32, x: i32, z: i32) -> usize {
        ((level * world.max_tile_x + x) * world.max_tile_z + z) as usize
    }

    fn grow_tile_models(&mut self, index: usize) {
        while self.tile_models.len() <= index {
            self.tile_models.push(None);
        }
    }

    fn grow_linked_models(&mut self, index: usize) {
        while self.linked_models.len() <= index {
            self.linked_models.push(None);
        }
    }

    fn grow_sprite_arrays(&mut self, index: usize) {
        while self.sprite_models.len() <= index {
            self.sprite_models.push(None);
        }
        while self.sprite_stamps.len() <= index {
            self.sprite_stamps.push(i32::MIN);
        }
    }

    fn slot(&mut self, world: &World, level: i32, x: i32, z: i32) -> &mut TileModels {
        let index = self.tile_index(world, level, x, z);
        self.grow_tile_models(index);
        self.tile_models[index].get_or_insert_with(Default::default)
    }

    fn loc_at(cache: &Cache, id: i32) -> Option<&crate::config::LocType> {
        if id < 0 || id as usize >= cache.locs.len() {
            return None;
        }
        Some(&cache.locs[id as usize])
    }

    /// Mirror of the `addLoc` model branches for one wall/decor/scenery
    /// slot: the base model, or a `ClientLocAnim` when the loc is
    /// animated. `loc == None` (id outside the cache) decodes nothing.
    #[allow(clippy::too_many_arguments)]
    fn decode_loc_model(
        loc_id: i32,
        loc: Option<&crate::config::LocType>,
        cache: &Cache,
        shape: i32,
        angle: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
        loop_cycle: i32,
    ) -> Option<SceneModel> {
        let loc = loc?;
        if loc.anim == -1 {
            let model = loc.get_model(cache, shape, angle, h_sw, h_se, h_ne, h_nw, -1);
            if crate::render_debug_enabled() {
                if let Some(m) = &model {
                    if let (Some(rt), Some(fc)) = (&m.face_render_type, &m.face_colour) {
                        let mut textured = Vec::new();
                        for (f, &t) in rt.iter().enumerate() {
                            if t & 0x3 == 2 || t & 0x3 == 3 {
                                textured.push((t & 0x3, fc.get(f).copied().unwrap_or(-1)));
                            }
                        }
                        eprintln!(
                            "[render] loc {loc_id} ({}) sharelight={} model textured={:?}",
                            loc.name, loc.sharelight, textured
                        );
                    }
                }
            }
            model.map(SceneModel::Model)
        } else {
            Some(SceneModel::LocAnim(ClientLocAnim::new(
                cache,
                loc_id,
                shape,
                angle,
                h_sw,
                h_se,
                h_ne,
                h_nw,
                loc.anim as usize,
                true,
                loop_cycle,
            )))
        }
    }

    /// Resolve (and cache) every model the sim tile's typecodes decode to.
    /// The tests drive this to inspect the lazily-materialised models.
    pub fn resolve_tile(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) {
        self.resolve_wall_models(world, cache, loop_cycle, level, x, z);
        self.resolve_decor_model(world, cache, loop_cycle, level, x, z);
        self.resolve_gd_model(world, cache, loop_cycle, level, x, z);
        self.resolve_objs(world, level, x, z);
        self.slot(world, level, x, z).model_stamp = world.tile_model_stamp(level, x, z);
    }

    /// Re-resolve the tile's cached models only when its sim-side
    /// `model_stamp` changed since the slot was filled.
    fn ensure_tile_resolved(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) {
        let stamp = world.tile_model_stamp(level, x, z);
        let old = self.slot(world, level, x, z).model_stamp;
        if old != stamp {
            if crate::render_debug_enabled() {
                eprintln!("[resolve] tile ({x},{z}) level={level} stamp {old}->{stamp}");
            }
            self.resolve_tile(world, cache, loop_cycle, level, x, z);
        }
    }

    fn resolve_wall_models(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) {
        let Some(wall) = tile_at(&world.squares, level, x, z).and_then(|t| t.wall.as_ref()) else {
            return;
        };
        let loc_id = (wall.typecode >> 14) & 0x7fff;
        let info = wall.typecode2 & 0xff;
        let angle = info >> 6;
        let shape = info & 0x1f;
        let (h_sw, h_se, h_ne, h_nw) = (wall.h_sw, wall.h_se, wall.h_ne, wall.h_nw);

        let (model1, model2) = if wall.anim_seq != -1 {
            // LOC_ANIM override: the "two models" decision keys off the
            // *packet* shape (recorded in `anim_shape`), exactly like the
            // original `locAnimChange` `if shape == 2` — the placement
            // shape in `typecode2` may differ from the packet's.
            if wall.anim_shape == LocShape::WALL_L {
                (
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache,
                        loc_id,
                        2,
                        wall.anim_angle + 4,
                        h_sw,
                        h_se,
                        h_ne,
                        h_nw,
                        wall.anim_seq as usize,
                        false,
                        loop_cycle,
                    ))),
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache,
                        loc_id,
                        2,
                        (wall.anim_angle + 1) & 0x3,
                        h_sw,
                        h_se,
                        h_ne,
                        h_nw,
                        wall.anim_seq as usize,
                        false,
                        loop_cycle,
                    ))),
                )
            } else {
                (
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache,
                        loc_id,
                        wall.anim_shape,
                        wall.anim_angle,
                        h_sw,
                        h_se,
                        h_ne,
                        h_nw,
                        wall.anim_seq as usize,
                        false,
                        loop_cycle,
                    ))),
                    None,
                )
            }
        } else {
            let loc = Self::loc_at(cache, loc_id);
            match shape {
                LocShape::WALL_L => {
                    let offset = (angle + 1) & 0x3;
                    (
                        Self::decode_loc_model(
                            loc_id,
                            loc,
                            cache,
                            2,
                            angle + 4,
                            h_sw,
                            h_se,
                            h_ne,
                            h_nw,
                            loop_cycle,
                        ),
                        Self::decode_loc_model(
                            loc_id, loc, cache, 2, offset, h_sw, h_se, h_ne, h_nw, loop_cycle,
                        ),
                    )
                }
                LocShape::WALL_STRAIGHT
                | LocShape::WALL_DIAGONAL_CORNER
                | LocShape::WALL_SQUARE_CORNER => (
                    Self::decode_loc_model(
                        loc_id, loc, cache, shape, angle, h_sw, h_se, h_ne, h_nw, loop_cycle,
                    ),
                    None,
                ),
                _ => (None, None),
            }
        };

        let slot = self.slot(world, level, x, z);
        slot.wall_model1 = model1;
        slot.wall_model2 = model2;
    }

    fn resolve_decor_model(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) {
        let Some(decor) = tile_at(&world.squares, level, x, z).and_then(|t| t.decor.as_ref())
        else {
            return;
        };
        let loc_id = (decor.typecode >> 14) & 0x7fff;
        let model = if decor.anim_seq != -1 {
            // LOC_ANIM: shape 4, angle 0; the [sic] NE-in-SE height
            // transpose was applied when the sim recorded the heights.
            Some(SceneModel::LocAnim(ClientLocAnim::new(
                cache,
                loc_id,
                4,
                0,
                decor.h_sw,
                decor.h_se,
                decor.h_ne,
                decor.h_nw,
                decor.anim_seq as usize,
                false,
                loop_cycle,
            )))
        } else {
            let loc = Self::loc_at(cache, loc_id);
            Self::decode_loc_model(
                loc_id, loc, cache, 4, 0, decor.h_sw, decor.h_se, decor.h_ne, decor.h_nw,
                loop_cycle,
            )
        };
        self.slot(world, level, x, z).decor_model = model;
    }

    fn resolve_gd_model(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) {
        let Some(gd) = tile_at(&world.squares, level, x, z).and_then(|t| t.ground_decor.as_ref())
        else {
            return;
        };
        let loc_id = (gd.typecode >> 14) & 0x7fff;
        let info = gd.typecode2 & 0xff;
        let angle = info >> 6;
        let model = if gd.anim_seq != -1 {
            Some(SceneModel::LocAnim(ClientLocAnim::new(
                cache,
                loc_id,
                LocShape::GROUND_DECOR,
                gd.anim_angle,
                gd.h_sw,
                gd.h_se,
                gd.h_ne,
                gd.h_nw,
                gd.anim_seq as usize,
                false,
                loop_cycle,
            )))
        } else if let Some(loc) = Self::loc_at(cache, loc_id) {
            if loc.anim == -1 {
                loc.get_model(
                    cache,
                    LocShape::GROUND_DECOR,
                    angle,
                    gd.h_sw,
                    gd.h_se,
                    gd.h_ne,
                    gd.h_nw,
                    -1,
                )
                .map(SceneModel::Model)
            } else {
                // [sic] `addLoc` passes the shape as the `ClientLocAnim`
                // angle for animated ground decor.
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache,
                    loc_id,
                    LocShape::GROUND_DECOR,
                    LocShape::GROUND_DECOR,
                    gd.h_sw,
                    gd.h_se,
                    gd.h_ne,
                    gd.h_nw,
                    loc.anim as usize,
                    true,
                    loop_cycle,
                )))
            }
        } else {
            None
        };
        self.slot(world, level, x, z).gd_model = model;
    }

    /// Materialise the ground-object stack's `ClientObj` models from the
    /// `(id, count)` descriptors `showObject` stored on the sim tile.
    fn resolve_objs(&mut self, world: &World, level: i32, x: i32, z: i32) {
        let Some(go) = tile_at(&world.squares, level, x, z).and_then(|t| t.ground_object.as_ref())
        else {
            return;
        };
        let bottom = go
            .bottom
            .map(|(id, count)| SceneModel::Obj(ClientObj::new(id, count)));
        let middle = go
            .middle
            .map(|(id, count)| SceneModel::Obj(ClientObj::new(id, count)));
        let top = go
            .top
            .map(|(id, count)| SceneModel::Obj(ClientObj::new(id, count)));
        let slot = self.slot(world, level, x, z);
        slot.obj_bottom = bottom;
        slot.obj_middle = middle;
        slot.obj_top = top;
    }

    fn resolve_sprite(&mut self, world: &World, cache: &Cache, loop_cycle: i32, index: usize) {
        // Only scene sprites (bit 30) decode from a loc typecode; dynamic
        // sprites (players/npcs/projectiles/spot-anims) get their models
        // attached by the entity passes in `render/draw.rs`.
        let Some(sprite) = world.sprites.get(index).and_then(|s| s.as_ref()) else {
            return;
        };
        if (sprite.typecode >> 29) & 0x3 != 2 {
            return;
        }
        let loc_id = (sprite.typecode >> 14) & 0x7fff;
        if crate::render_debug_enabled() {
            let name = Self::loc_at(cache, loc_id)
                .map(|l| l.name.as_str())
                .unwrap_or("");
            eprintln!(
                "[sprite] loc {loc_id} ({name}) tiles ({},{})..({},{})",
                sprite.min_tile_x, sprite.min_tile_z, sprite.max_tile_x, sprite.max_tile_z
            );
        }
        let info = sprite.typecode2 & 0xff;
        let mut shape = info & 0x1f;
        if shape == LocShape::CENTREPIECE_DIAGONAL {
            shape = LocShape::CENTREPIECE_STRAIGHT;
        }
        let angle = info >> 6;
        let model = if sprite.anim_seq != -1 {
            Some(SceneModel::LocAnim(ClientLocAnim::new(
                cache,
                loc_id,
                sprite.anim_shape,
                sprite.anim_angle,
                sprite.h_sw,
                sprite.h_se,
                sprite.h_ne,
                sprite.h_nw,
                sprite.anim_seq as usize,
                false,
                loop_cycle,
            )))
        } else {
            let loc = Self::loc_at(cache, loc_id);
            Self::decode_loc_model(
                loc_id,
                loc,
                cache,
                shape,
                angle,
                sprite.h_sw,
                sprite.h_se,
                sprite.h_ne,
                sprite.h_nw,
                loop_cycle,
            )
        };
        self.grow_sprite_arrays(index);
        self.sprite_models[index] = model;
        if self.sprite_stamps.len() <= index {
            self.sprite_stamps.resize(index + 1, i32::MIN);
        }
        self.sprite_stamps[index] = sprite.model_stamp;
    }

    fn resolve_linked_wall(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) {
        let Some(wall) = tile_at(&world.squares, level, x, z)
            .and_then(|t| t.linked_square.as_ref())
            .and_then(|ls| ls.wall.as_ref())
        else {
            return;
        };
        let loc_id = (wall.typecode >> 14) & 0x7fff;
        let info = wall.typecode2 & 0xff;
        let angle = info >> 6;
        let shape = info & 0x1f;
        let (h_sw, h_se, h_ne, h_nw) = (wall.h_sw, wall.h_se, wall.h_ne, wall.h_nw);
        let loc = Self::loc_at(cache, loc_id);
        let (model1, model2) = match shape {
            LocShape::WALL_L => {
                let offset = (angle + 1) & 0x3;
                (
                    Self::decode_loc_model(
                        loc_id,
                        loc,
                        cache,
                        2,
                        angle + 4,
                        h_sw,
                        h_se,
                        h_ne,
                        h_nw,
                        loop_cycle,
                    ),
                    Self::decode_loc_model(
                        loc_id, loc, cache, 2, offset, h_sw, h_se, h_ne, h_nw, loop_cycle,
                    ),
                )
            }
            LocShape::WALL_STRAIGHT
            | LocShape::WALL_DIAGONAL_CORNER
            | LocShape::WALL_SQUARE_CORNER => (
                Self::decode_loc_model(
                    loc_id, loc, cache, shape, angle, h_sw, h_se, h_ne, h_nw, loop_cycle,
                ),
                None,
            ),
            _ => (None, None),
        };
        let index = self.tile_index(world, level, x, z);
        self.grow_linked_models(index);
        let slot = self.linked_models[index].get_or_insert_with(Default::default);
        slot.model_stamp = 0;
        slot.wall_model1 = model1;
        slot.wall_model2 = model2;
    }

    /// The resolved wall models of a tile (resolving from the sim typecodes
    /// on first draw, re-resolving when the tile's model stamp changes).
    fn wall_models_mut(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> (&mut Option<SceneModel>, &mut Option<SceneModel>) {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        let slot = self.slot(world, level, x, z);
        (&mut slot.wall_model1, &mut slot.wall_model2)
    }

    fn decor_model_mut(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> &mut Option<SceneModel> {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        &mut self.slot(world, level, x, z).decor_model
    }

    fn gd_model_mut(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> &mut Option<SceneModel> {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        &mut self.slot(world, level, x, z).gd_model
    }

    /// The live loc model at scene tile (`tile_x`, `tile_z`) whose loc id
    /// is `loc_id` — the nav-debug hull target. Resolves the
    /// wall/decor/ground-decor slot *or* the tile's grounded scene loc
    /// (the entity-2 sprites `add_scenery` places, typical type-10
    /// scenery) exactly like the draw pass (Task 3b cache) and returns the
    /// placement scene position, the render yaw index and a copy of the
    /// current temp model. Reads the AABB only; the loc's
    /// `use_aabb_mouse_check` flag is never touched, so hull paint cannot
    /// change loc picking.
    pub fn loc_model_at(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        loc_id: i32,
    ) -> Option<(i32, i32, i32, i32, Model)> {
        let tile = tile_at(&world.squares, level, tile_x, tile_z)?;
        if let Some(wall) = tile.wall.as_ref() {
            if (wall.typecode >> 14) & 0x7fff == loc_id {
                let (model1, model2) =
                    self.wall_models_mut(world, cache, loop_cycle, level, tile_x, tile_z);
                for model in [model1, model2].into_iter().flatten() {
                    if let Some(model) = model.get_temp_model(cache, loop_cycle) {
                        return Some((wall.x, wall.y, wall.z, 0, model));
                    }
                }
            }
        }
        if let Some(decor) = tile.decor.as_ref() {
            if (decor.typecode >> 14) & 0x7fff == loc_id {
                if let Some(model) = self
                    .decor_model_mut(world, cache, loop_cycle, level, tile_x, tile_z)
                    .as_mut()
                {
                    if let Some(model) = model.get_temp_model(cache, loop_cycle) {
                        return Some((decor.x, decor.y, decor.z, decor.angle, model));
                    }
                }
            }
        }
        if let Some(gd) = tile.ground_decor.as_ref() {
            if (gd.typecode >> 14) & 0x7fff == loc_id {
                if let Some(model) = self
                    .gd_model_mut(world, cache, loop_cycle, level, tile_x, tile_z)
                    .as_mut()
                {
                    if let Some(model) = model.get_temp_model(cache, loop_cycle) {
                        return Some((gd.x, gd.y, gd.z, 0, model));
                    }
                }
            }
        }
        // Grounded tile locs: the tile's scene sprites (entity 2) carry
        // the loc id, resolved exactly like the draw pass.
        for i in 0..tile.sprite_count as usize {
            let Some(index) = tile.sprites[i] else {
                continue;
            };
            let Some(sprite) = world.sprites.get(index).and_then(|s| s.as_ref()) else {
                continue;
            };
            if (sprite.typecode >> 29) & 0x3 != 2 || (sprite.typecode >> 14) & 0x7fff != loc_id {
                continue;
            }
            let (sx, sy, sz, yaw) = (sprite.x, sprite.y, sprite.z, sprite.yaw);
            if let Some(model) = self.sprite_model_mut(world, cache, loop_cycle, index).as_mut() {
                if let Some(model) = model.get_temp_model(cache, loop_cycle) {
                    return Some((sx, sy, sz, yaw, model));
                }
            }
        }
        None
    }

    fn obj_models_mut(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> (
        &mut Option<SceneModel>,
        &mut Option<SceneModel>,
        &mut Option<SceneModel>,
    ) {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        let slot = self.slot(world, level, x, z);
        (
            &mut slot.obj_bottom,
            &mut slot.obj_middle,
            &mut slot.obj_top,
        )
    }

    /// The linked square's wall models (resolved once — a linked square is
    /// never mutated after `push_down`).
    fn linked_wall_models_mut(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> (&mut Option<SceneModel>, &mut Option<SceneModel>) {
        let index = self.tile_index(world, level, x, z);
        self.grow_linked_models(index);
        let stale = self.linked_models[index]
            .as_ref()
            .is_none_or(|s| s.model_stamp == i32::MIN);
        if stale {
            self.resolve_linked_wall(world, cache, loop_cycle, level, x, z);
        }
        let slot = self.linked_models[index].get_or_insert_with(Default::default);
        (&mut slot.wall_model1, &mut slot.wall_model2)
    }

    /// The resolved model of a sprite-arena slot (scene sprites decode on
    /// first draw; dynamic sprites were attached with `set_sprite_model`).
    fn sprite_model_mut(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        index: usize,
    ) -> &mut Option<SceneModel> {
        self.grow_sprite_arrays(index);
        let stamp = world
            .sprites
            .get(index)
            .and_then(|s| s.as_ref())
            .map(|s| s.model_stamp)
            .unwrap_or(i32::MIN);
        if self.sprite_stamps[index] != stamp {
            self.resolve_sprite(world, cache, loop_cycle, index);
        }
        &mut self.sprite_models[index]
    }

    /// The ground-object stack offset. The sim used to read it from the
    /// tile's sprite models at `setObj` time; now it is computed at draw
    /// time from the same lazily-resolved sprite models (`obj_raise` is
    /// non-zero only for locs with `raiseobject == 1`). The value is
    /// frozen per draw rather than per `setObj`; the common table-sprite
    /// case is identical.
    fn ground_object_height(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> i32 {
        let mut stack_offset = 0;
        let Some(tile) = tile_at(&world.squares, level, x, z) else {
            return 0;
        };
        for i in 0..tile.sprite_count as usize {
            let Some(index) = tile.sprites[i] else {
                continue;
            };
            if let Some(SceneModel::Model(model)) = self
                .sprite_model_mut(world, cache, loop_cycle, index)
                .as_ref()
            {
                if model.obj_raise > stack_offset {
                    stack_offset = model.obj_raise;
                }
            }
        }
        stack_offset
    }

    /// Attach a sprite model to the render-side arena (the entity passes
    /// and tests use this; scene sprites decode on first draw instead).
    pub fn set_sprite_model(&mut self, world: &World, index: usize, model: Option<SceneModel>) {
        self.grow_sprite_arrays(index);
        let stamp = world
            .sprites
            .get(index)
            .and_then(|s| s.as_ref())
            .map(|s| s.model_stamp)
            .unwrap_or(i32::MIN);
        self.sprite_stamps[index] = stamp;
        self.sprite_models[index] = model;
    }

    /// Inject a wall's models on the render side (tests place synthetic
    /// models this way; production resolves from the sim typecodes).
    pub fn set_wall_model(
        &mut self,
        world: &World,
        level: i32,
        x: i32,
        z: i32,
        model1: Option<SceneModel>,
        model2: Option<SceneModel>,
    ) {
        let slot = self.slot(world, level, x, z);
        slot.model_stamp = world.tile_model_stamp(level, x, z);
        slot.wall_model1 = model1;
        slot.wall_model2 = model2;
    }

    /// Inject a decor's model on the render side (tests only).
    pub fn set_decor_model(
        &mut self,
        world: &World,
        level: i32,
        x: i32,
        z: i32,
        model: SceneModel,
    ) {
        let slot = self.slot(world, level, x, z);
        slot.model_stamp = world.tile_model_stamp(level, x, z);
        slot.decor_model = Some(model);
    }

    /// Inject a ground-decor's model on the render side (tests only).
    pub fn set_gd_model(
        &mut self,
        world: &World,
        level: i32,
        x: i32,
        z: i32,
        model: Option<SceneModel>,
    ) {
        let slot = self.slot(world, level, x, z);
        slot.model_stamp = world.tile_model_stamp(level, x, z);
        slot.gd_model = model;
    }

    /// Read accessors (the draw tests assert on the resolved models after
    /// `share_light`).
    pub fn wall_model1(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> Option<&SceneModel> {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        self.slot(world, level, x, z).wall_model1.as_ref()
    }

    pub fn wall_model2(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> Option<&SceneModel> {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        self.slot(world, level, x, z).wall_model2.as_ref()
    }

    pub fn gd_model(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> Option<&SceneModel> {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        self.slot(world, level, x, z).gd_model.as_ref()
    }

    pub fn decor_model(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        level: i32,
        x: i32,
        z: i32,
    ) -> Option<&SceneModel> {
        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
        self.slot(world, level, x, z).decor_model.as_ref()
    }

    pub fn sprite_model(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        index: usize,
    ) -> Option<&SceneModel> {
        self.sprite_model_mut(world, cache, loop_cycle, index)
            .as_ref()
    }

    /// Materialise a sprite's current frame model exactly as the draw path
    /// does (`SceneModel::worldRender` → `ClientLocAnim::get_temp_model`).
    /// The tests read the animated-loc frames back through this to assert
    /// the share-light pass lit them (the `LocAnim` descriptor itself holds
    /// no `Model` to inspect).
    pub fn sprite_frame_model(
        &mut self,
        world: &World,
        cache: &Cache,
        loop_cycle: i32,
        index: usize,
    ) -> Option<Model> {
        match self
            .sprite_model_mut(world, cache, loop_cycle, index)
            .as_mut()?
        {
            SceneModel::LocAnim(anim) => anim.get_temp_model(cache, loop_cycle),
            _ => None,
        }
    }

    /// `shareLight(ambient, contrast, lightSrcX, lightSrcY, lightSrcZ)`
    /// from World.ts 589-628, moved here with the models (Task 3b): resolve
    /// every tile's models, merge touching vertices' normals across
    /// walls/sprites/ground-decor and then run `Model.light` over every
    /// placed model. The sim's `finishBuild` flags the pass and the first
    /// `render_all` after a build runs it; the draw tests call it directly
    /// with their injected models.
    pub fn share_light(
        &mut self,
        world: &mut World,
        ambient: i32,
        contrast: i32,
        light_src_x: i32,
        light_src_y: i32,
        light_src_z: i32,
    ) {
        self.run_share_light(world, &Cache::default(), 0);
        self.apply_share_light(
            world,
            ambient,
            contrast,
            light_src_x,
            light_src_y,
            light_src_z,
        );
    }

    /// Resolve every tile's models and sprites (and the linked-square
    /// walls/sprites) ahead of the share-light merge. `render_all` runs
    /// this once per build; the tests' injected models are already current
    /// and are left untouched. Scene sprites must be resolved here: the
    /// merge pass below runs before the lazy per-draw resolution, and the
    /// original `finishBuild` shared light over every placed sprite.
    fn run_share_light(&mut self, world: &World, cache: &Cache, loop_cycle: i32) {
        for level in 0..world.max_tile_level {
            for x in 0..world.max_tile_x {
                for z in 0..world.max_tile_z {
                    if let Some(t) = tile_at(&world.squares, level, x, z) {
                        self.ensure_tile_resolved(world, cache, loop_cycle, level, x, z);
                        for i in 0..t.sprite_count as usize {
                            if let Some(index) = t.sprites[i] {
                                self.sprite_model_mut(world, cache, loop_cycle, index);
                            }
                        }
                    }
                }
            }
        }
        for x in 0..world.max_tile_x {
            for z in 0..world.max_tile_z {
                if let Some(linked) =
                    tile_at(&world.squares, 0, x, z).and_then(|t| t.linked_square.as_ref())
                {
                    let index = self.tile_index(world, 0, x, z);
                    self.grow_linked_models(index);
                    if self.linked_models[index]
                        .as_ref()
                        .is_none_or(|s| s.model_stamp == i32::MIN)
                    {
                        self.resolve_linked_wall(world, cache, loop_cycle, 0, x, z);
                    }
                    for i in 0..linked.sprite_count as usize {
                        if let Some(sprite_index) = linked.sprites[i] {
                            self.sprite_model_mut(world, cache, loop_cycle, sprite_index);
                        }
                    }
                }
            }
        }
    }

    /// The merge + light pass over the resolved tile/sprite models. Each
    /// tile is taken out of the cache for the duration of its own pass
    /// because the helpers need `&mut self` (share scratch, other tiles)
    /// and `&mut Model` simultaneously; `share_light_loc` never revisits
    /// the current tile (its own tile is inside the `allowFaceRemoval`
    /// skip or on another level), so the hole is never observed.
    fn apply_share_light(
        &mut self,
        world: &World,
        ambient: i32,
        contrast: i32,
        light_src_x: i32,
        light_src_y: i32,
        light_src_z: i32,
    ) {
        let light_magnitude = ((light_src_x * light_src_x
            + light_src_y * light_src_y
            + light_src_z * light_src_z) as f64)
            .sqrt() as i32;
        let attenuation = (contrast * light_magnitude) >> 8;

        for level in 0..world.max_tile_level {
            for tile_x in 0..world.max_tile_x {
                for tile_z in 0..world.max_tile_z {
                    let index = self.tile_index(world, level, tile_x, tile_z);
                    let mut tile = self.tile_models.get_mut(index).and_then(|t| t.take());

                    if let Some(tile) = tile.as_mut() {
                        if let Some(SceneModel::Model(model1)) = tile.wall_model1.as_mut() {
                            if model1.point_normal.is_some() {
                                self.share_light_loc(world, level, tile_x, tile_z, 1, 1, model1);
                                if let Some(SceneModel::Model(model2)) = tile.wall_model2.as_mut() {
                                    if model2.point_normal.is_some() {
                                        self.share_light_loc(
                                            world, level, tile_x, tile_z, 1, 1, model2,
                                        );
                                        self.model_share_light(model1, model2, 0, 0, 0, false);
                                        model2.light(
                                            ambient,
                                            attenuation,
                                            light_src_x,
                                            light_src_y,
                                            light_src_z,
                                        );
                                    }
                                }
                                model1.light(
                                    ambient,
                                    attenuation,
                                    light_src_x,
                                    light_src_y,
                                    light_src_z,
                                );
                            }
                        }

                        if let Some(SceneModel::Model(model)) = tile.gd_model.as_mut() {
                            if model.point_normal.is_some() {
                                self.share_light_gd(world, level, tile_x, tile_z, model);
                                model.light(
                                    ambient,
                                    attenuation,
                                    light_src_x,
                                    light_src_y,
                                    light_src_z,
                                );
                            }
                        }

                        if crate::render_debug_enabled() {
                            let (typecode, lit, hidden, unlit) = tile
                                .wall_model1
                                .as_ref()
                                .map(|m| match m {
                                    SceneModel::Model(model) => {
                                        let lit = model
                                            .face_colour_a
                                            .as_ref()
                                            .map(|f| f.iter().filter(|&&c| c != 0).count())
                                            .unwrap_or(0);
                                        let hidden = model
                                            .face_render_type
                                            .as_ref()
                                            .map(|rt| rt.iter().filter(|&&t| t == -1).count())
                                            .unwrap_or(0);
                                        let unlit = model
                                            .face_colour_a
                                            .as_ref()
                                            .map(|f| f.iter().filter(|&&c| c == 0).count())
                                            .unwrap_or(0);
                                        (0, lit, hidden, unlit)
                                    }
                                    _ => (-1, 0, 0, 0),
                                })
                                .unwrap_or((-1, 0, 0, 0));
                            let _ = typecode;
                            eprintln!(
                                "[share-light] tile ({tile_x},{tile_z}) wall faces={} lit={lit} hidden={hidden} unlit={unlit}",
                                lit + hidden + unlit
                            );
                        }
                    }
                    if let Some(tile) = tile {
                        self.tile_models[index] = Some(tile);
                    }

                    // The tile's scene sprites (shared across the tiles a
                    // multi-tile sprite spans, so a large loc merges from
                    // each of its footprint tiles as the TS does).
                    if let Some(t) = tile_at(&world.squares, level, tile_x, tile_z) {
                        for i in 0..t.sprite_count as usize {
                            let Some(sprite_index) = t.sprites[i] else {
                                continue;
                            };
                            let mut sprite = self
                                .sprite_models
                                .get_mut(sprite_index)
                                .and_then(|s| s.take());
                            if let Some(SceneModel::Model(model)) = sprite.as_mut() {
                                if model.point_normal.is_some() {
                                    if let Some(s) =
                                        world.sprites.get(sprite_index).and_then(|s| s.as_ref())
                                    {
                                        let size_x = s.max_tile_x + 1 - s.min_tile_x;
                                        let size_z = s.max_tile_z + 1 - s.min_tile_z;
                                        self.share_light_loc(
                                            world, level, tile_x, tile_z, size_x, size_z, model,
                                        );
                                    }
                                    model.light(
                                        ambient,
                                        attenuation,
                                        light_src_x,
                                        light_src_y,
                                        light_src_z,
                                    );
                                }
                            }
                            self.grow_sprite_arrays(sprite_index);
                            self.sprite_models[sprite_index] = sprite;
                        }
                    }
                }
            }
        }

        // Linked squares (pushed-down level-0 content). The original ran
        // `shareLight` before `push_down`, so the wall and sprites that
        // later become a linked square were merged and lit as ordinary
        // level-0 tiles; merge + light them here too (their `tile_models`
        // counterparts cover the pushed-down content).
        for tile_x in 0..world.max_tile_x {
            for tile_z in 0..world.max_tile_z {
                let Some(linked) = tile_at(&world.squares, 0, tile_x, tile_z)
                    .and_then(|t| t.linked_square.as_ref())
                else {
                    continue;
                };

                let index = self.tile_index(world, 0, tile_x, tile_z);
                let mut tile = self.linked_models.get_mut(index).and_then(|t| t.take());
                if let Some(tile) = tile.as_mut() {
                    if let Some(SceneModel::Model(model1)) = tile.wall_model1.as_mut() {
                        if model1.point_normal.is_some() {
                            self.share_light_loc(world, 0, tile_x, tile_z, 1, 1, model1);
                            if let Some(SceneModel::Model(model2)) = tile.wall_model2.as_mut() {
                                if model2.point_normal.is_some() {
                                    self.share_light_loc(world, 0, tile_x, tile_z, 1, 1, model2);
                                    self.model_share_light(model1, model2, 0, 0, 0, false);
                                    model2.light(
                                        ambient,
                                        attenuation,
                                        light_src_x,
                                        light_src_y,
                                        light_src_z,
                                    );
                                }
                            }
                            model1.light(
                                ambient,
                                attenuation,
                                light_src_x,
                                light_src_y,
                                light_src_z,
                            );
                        }
                    }
                }
                if let Some(tile) = tile {
                    self.linked_models[index] = Some(tile);
                }

                for i in 0..linked.sprite_count as usize {
                    let Some(sprite_index) = linked.sprites[i] else {
                        continue;
                    };
                    let mut sprite = self
                        .sprite_models
                        .get_mut(sprite_index)
                        .and_then(|s| s.take());
                    if let Some(SceneModel::Model(model)) = sprite.as_mut() {
                        if model.point_normal.is_some() {
                            if let Some(s) =
                                world.sprites.get(sprite_index).and_then(|s| s.as_ref())
                            {
                                let size_x = s.max_tile_x + 1 - s.min_tile_x;
                                let size_z = s.max_tile_z + 1 - s.min_tile_z;
                                self.share_light_loc(
                                    world, 0, tile_x, tile_z, size_x, size_z, model,
                                );
                            }
                            model.light(
                                ambient,
                                attenuation,
                                light_src_x,
                                light_src_y,
                                light_src_z,
                            );
                        }
                    }
                    self.grow_sprite_arrays(sprite_index);
                    self.sprite_models[sprite_index] = sprite;
                }
            }
        }
    }

    /// `shareLightGd(level, tileX, tileZ, model)` from World.ts 630-658:
    /// merge the ground-decor model with the four diagonal/east/south
    /// neighbours' ground-decor models. The neighbour bounds checks are
    /// replicated verbatim (including the TS `tileZ < maxTileX` quirk at
    /// 638); the world is square so they coincide.
    fn share_light_gd(
        &mut self,
        world: &World,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        model: &mut Model,
    ) {
        if tile_x < world.max_tile_x {
            let index = self.tile_index(world, level, tile_x + 1, tile_z);
            let mut tile = self.tile_models.get_mut(index).and_then(|t| t.take());
            if let Some(SceneModel::Model(model_b)) =
                tile.as_mut().and_then(|t| t.gd_model.as_mut())
            {
                if model_b.point_normal.is_some() {
                    self.model_share_light(model, model_b, 128, 0, 0, true);
                }
            }
            if let Some(tile) = tile {
                self.tile_models[index] = Some(tile);
            }
        }

        if tile_z < world.max_tile_x {
            let index = self.tile_index(world, level, tile_x, tile_z + 1);
            let mut tile = self.tile_models.get_mut(index).and_then(|t| t.take());
            if let Some(SceneModel::Model(model_b)) =
                tile.as_mut().and_then(|t| t.gd_model.as_mut())
            {
                if model_b.point_normal.is_some() {
                    self.model_share_light(model, model_b, 0, 0, 128, true);
                }
            }
            if let Some(tile) = tile {
                self.tile_models[index] = Some(tile);
            }
        }

        if tile_x < world.max_tile_x && tile_z < world.max_tile_z {
            let index = self.tile_index(world, level, tile_x + 1, tile_z + 1);
            let mut tile = self.tile_models.get_mut(index).and_then(|t| t.take());
            if let Some(SceneModel::Model(model_b)) =
                tile.as_mut().and_then(|t| t.gd_model.as_mut())
            {
                if model_b.point_normal.is_some() {
                    self.model_share_light(model, model_b, 128, 0, 128, true);
                }
            }
            if let Some(tile) = tile {
                self.tile_models[index] = Some(tile);
            }
        }

        if tile_x < world.max_tile_x && tile_z > 0 {
            let index = self.tile_index(world, level, tile_x + 1, tile_z - 1);
            let mut tile = self.tile_models.get_mut(index).and_then(|t| t.take());
            if let Some(SceneModel::Model(model_b)) =
                tile.as_mut().and_then(|t| t.gd_model.as_mut())
            {
                if model_b.point_normal.is_some() {
                    self.model_share_light(model, model_b, 128, 0, -128, true);
                }
            }
            if let Some(tile) = tile {
                self.tile_models[index] = Some(tile);
            }
        }
    }

    /// `shareLightLoc(level, tileX, tileZ, tileSizeX, tileSizeZ, model)`
    /// from World.ts 660-719: merge `model`'s normals with every wall and
    /// sprite model in the 3×3(+1) neighbourhood (the current tile itself
    /// is always skipped by the `allowFaceRemoval` gate, and the second
    /// pass runs on `level + 1`). Candidate tiles and arena sprites are
    /// taken out for the duration of their own merge, mirroring the TS
    /// object aliasing with disjoint borrows.
    fn share_light_loc(
        &mut self,
        world: &World,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        tile_size_x: i32,
        tile_size_z: i32,
        model_a: &mut Model,
    ) {
        let mut allow_face_removal = true;

        let mut min_tile_x = tile_x;
        let max_tile_x = tile_x + tile_size_x;
        let min_tile_z = tile_z - 1;
        let max_tile_z = tile_z + tile_size_z;

        for l in level..=level + 1 {
            if l == world.max_tile_level {
                continue;
            }

            for x in min_tile_x..=max_tile_x {
                if x < 0 || x >= world.max_tile_x {
                    continue;
                }

                for z in min_tile_z..=max_tile_z {
                    if z < 0
                        || z >= world.max_tile_z
                        || (allow_face_removal
                            && x < max_tile_x
                            && z < max_tile_z
                            && (z >= tile_z || x == tile_x))
                    {
                        continue;
                    }

                    let offset_x = (x - tile_x) * 128 + (1 - tile_size_x) * 64;
                    let offset_z = (z - tile_z) * 128 + (1 - tile_size_z) * 64;
                    let offset_y = ((ground_h(world, l, x, z)
                        + ground_h(world, l, x + 1, z)
                        + ground_h(world, l, x, z + 1)
                        + ground_h(world, l, x + 1, z + 1))
                        / 4)
                        - ((ground_h(world, level, tile_x, tile_z)
                            + ground_h(world, level, tile_x + 1, tile_z)
                            + ground_h(world, level, tile_x, tile_z + 1)
                            + ground_h(world, level, tile_x + 1, tile_z + 1))
                            / 4);

                    let candidate_index = self.tile_index(world, l, x, z);
                    let mut candidate = self
                        .tile_models
                        .get_mut(candidate_index)
                        .and_then(|t| t.take());

                    if let Some(candidate) = candidate.as_mut() {
                        if let Some(SceneModel::Model(model_b)) = candidate.wall_model1.as_mut() {
                            if model_b.point_normal.is_some() {
                                self.model_share_light(
                                    model_a,
                                    model_b,
                                    offset_x,
                                    offset_y,
                                    offset_z,
                                    allow_face_removal,
                                );
                            }
                        }
                        if let Some(SceneModel::Model(model_b)) = candidate.wall_model2.as_mut() {
                            if model_b.point_normal.is_some() {
                                self.model_share_light(
                                    model_a,
                                    model_b,
                                    offset_x,
                                    offset_y,
                                    offset_z,
                                    allow_face_removal,
                                );
                            }
                        }
                    }
                    if let Some(candidate) = candidate {
                        self.tile_models[candidate_index] = Some(candidate);
                    }

                    if let Some(t) = tile_at(&world.squares, l, x, z) {
                        for i in 0..t.sprite_count as usize {
                            let Some(sprite_index) = t.sprites[i] else {
                                continue;
                            };
                            let mut sprite = self
                                .sprite_models
                                .get_mut(sprite_index)
                                .and_then(|s| s.take());
                            if let Some(SceneModel::Model(model_b)) = sprite.as_mut() {
                                if model_b.point_normal.is_some() {
                                    let (min_sx, min_sz, size_x, size_z) = if let Some(s) =
                                        world.sprites.get(sprite_index).and_then(|s| s.as_ref())
                                    {
                                        (
                                            s.min_tile_x,
                                            s.min_tile_z,
                                            s.max_tile_x + 1 - s.min_tile_x,
                                            s.max_tile_z + 1 - s.min_tile_z,
                                        )
                                    } else {
                                        (0, 0, 1, 1)
                                    };
                                    let sx = (min_sx - tile_x) * 128 + (size_x - tile_size_x) * 64;
                                    let sz = (min_sz - tile_z) * 128 + (size_z - tile_size_z) * 64;
                                    self.model_share_light(
                                        model_a,
                                        model_b,
                                        sx,
                                        offset_y,
                                        sz,
                                        allow_face_removal,
                                    );
                                }
                            }
                            self.grow_sprite_arrays(sprite_index);
                            self.sprite_models[sprite_index] = sprite;
                        }
                    }
                }
            }

            min_tile_x -= 1;
            allow_face_removal = false;
        }
    }

    /// `modelShareLight(modelA, modelB, offsetX, offsetY, offsetZ,
    /// allowFaceRemoval)` from World.ts 722-794: merge the normals of
    /// coincident vertices of the two models, then hide the faces whose
    /// vertices all merged (3+ merges, unless face removal is disabled).
    fn model_share_light(
        &mut self,
        model_a: &mut Model,
        model_b: &mut Model,
        offset_x: i32,
        offset_y: i32,
        offset_z: i32,
        allow_face_removal: bool,
    ) {
        self.share_tic += 1;

        let mut merged = 0;
        let vertex_count_b = model_b.num_points;

        if self.share_map.len() < model_a.num_points as usize {
            self.share_map.resize(model_a.num_points as usize, 0);
        }
        if self.share_map2.len() < vertex_count_b as usize {
            self.share_map2.resize(vertex_count_b as usize, 0);
        }

        if model_a.point_normal.is_some() && model_a.shared_point_normal.is_some() {
            let point_normal_a = model_a.point_normal.as_mut().unwrap();
            let shared_normal_a = model_a.shared_point_normal.as_ref().unwrap();
            let point_x_a = model_a.point_x.as_ref().unwrap();
            let point_y_a = model_a.point_y.as_ref().unwrap();
            let point_z_a = model_a.point_z.as_ref().unwrap();
            let max_y_b = model_b.max_y;
            let min_x_b = model_b.min_x;
            let max_x_b = model_b.max_x;
            let min_z_b = model_b.min_z;
            let max_z_b = model_b.max_z;

            for vertex_a in 0..model_a.num_points as usize {
                let mut normal_a = point_normal_a[vertex_a].as_mut();
                let Some(original_normal_a) = shared_normal_a[vertex_a].as_ref() else {
                    continue;
                };
                if original_normal_a.w != 0 {
                    let y = point_y_a[vertex_a] - offset_y;
                    if y > max_y_b {
                        continue;
                    }

                    let x = point_x_a[vertex_a] - offset_x;
                    if x < min_x_b || x > max_x_b {
                        continue;
                    }

                    let z = point_z_a[vertex_a] - offset_z;
                    if z < min_z_b || z > max_z_b {
                        continue;
                    }

                    if model_b.point_normal.is_some() && model_b.shared_point_normal.is_some() {
                        let point_normal_b = model_b.point_normal.as_mut().unwrap();
                        let shared_normal_b = model_b.shared_point_normal.as_ref().unwrap();
                        let point_x_b = model_b.point_x.as_ref().unwrap();
                        let point_y_b = model_b.point_y.as_ref().unwrap();
                        let point_z_b = model_b.point_z.as_ref().unwrap();

                        for vertex_b in 0..vertex_count_b as usize {
                            let mut normal_b = point_normal_b[vertex_b].as_mut();
                            let original_normal_b = shared_normal_b[vertex_b].as_ref();
                            if x != point_x_b[vertex_b]
                                || z != point_z_b[vertex_b]
                                || y != point_y_b[vertex_b]
                                || original_normal_b.map_or(false, |n| n.w == 0)
                            {
                                continue;
                            }

                            if let (Some(normal_a), Some(normal_b), Some(original_normal_b)) = (
                                normal_a.as_deref_mut(),
                                normal_b.as_deref_mut(),
                                original_normal_b,
                            ) {
                                normal_a.x += original_normal_b.x;
                                normal_a.y += original_normal_b.y;
                                normal_a.z += original_normal_b.z;
                                normal_a.w += original_normal_b.w;
                                normal_b.x += original_normal_a.x;
                                normal_b.y += original_normal_a.y;
                                normal_b.z += original_normal_a.z;
                                normal_b.w += original_normal_a.w;
                                merged += 1;
                            }

                            self.share_map[vertex_a] = self.share_tic;
                            self.share_map2[vertex_b] = self.share_tic;
                        }
                    }
                }
            }
        }

        if merged < 3 || !allow_face_removal {
            return;
        }

        if let Some(face_render_type) = model_a.face_render_type.as_mut() {
            let face_vertex_a = model_a.face_vertex_a.as_ref().unwrap();
            let face_vertex_b = model_a.face_vertex_b.as_ref().unwrap();
            let face_vertex_c = model_a.face_vertex_c.as_ref().unwrap();
            for i in 0..model_a.num_faces as usize {
                if self.share_map[face_vertex_a[i] as usize] == self.share_tic
                    && self.share_map[face_vertex_b[i] as usize] == self.share_tic
                    && self.share_map[face_vertex_c[i] as usize] == self.share_tic
                {
                    face_render_type[i] = -1;
                }
            }
        }

        if let Some(face_render_type) = model_b.face_render_type.as_mut() {
            let face_vertex_a = model_b.face_vertex_a.as_ref().unwrap();
            let face_vertex_b = model_b.face_vertex_b.as_ref().unwrap();
            let face_vertex_c = model_b.face_vertex_c.as_ref().unwrap();
            for i in 0..model_b.num_faces as usize {
                if self.share_map2[face_vertex_a[i] as usize] == self.share_tic
                    && self.share_map2[face_vertex_b[i] as usize] == self.share_tic
                    && self.share_map2[face_vertex_c[i] as usize] == self.share_tic
                {
                    face_render_type[i] = -1;
                }
            }
        }
    }

    /// The pre-fill half of `render_all` (task 7 split): drop the previous
    /// build's lazily-resolved models when the sim world was reset, run
    /// the `finishBuild` share-light pass (flagged by the sim) over the
    /// freshly-resolved models once — resolve every tile/wall/sprite model,
    /// then merge + light them (the TS 331 `shareLight(64, 768, -50, -10,
    /// -50)` constants) — clamp the eye, bind the camera trig/visibility
    /// backing, set the viewport bounds, run `calcOcclude`, and mark every
    /// tile in the camera's 51×51 window drawable this frame. `render_all`
    /// then runs the two fill passes over the marked tiles; the wgpu
    /// backend (task 7) builds its scene mesh from the same marks.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_scene(
        &mut self,
        world: &mut World,
        cache: &Cache,
        loop_cycle: i32,
        mut eye_x: i32,
        eye_y: i32,
        mut eye_z: i32,
        max_level: i32,
        eye_yaw: i32,
        eye_pitch: i32,
    ) {
        if world.build_generation != self.last_build_generation {
            self.tile_models.clear();
            self.linked_models.clear();
            self.sprite_models.clear();
            self.sprite_stamps.clear();
            self.last_build_generation = world.build_generation;
        }
        if world.take_share_light_pending() {
            self.run_share_light(world, cache, loop_cycle);
            self.apply_share_light(world, 64, 768, -50, -10, -50);
        }
        // Task 5 rule 6: an unheaded build recorded only the tile stamps;
        // the first paint after the build materializes the overlay meshes.
        if world.take_overlay_pending() {
            world.materialize_overlay();
        }

        if eye_x < 0 {
            eye_x = 0;
        } else if eye_x >= world.max_tile_x * 128 {
            eye_x = world.max_tile_x * 128 - 1;
        }

        if eye_z < 0 {
            eye_z = 0;
        } else if eye_z >= world.max_tile_z * 128 {
            eye_z = world.max_tile_z * 128 - 1;
        }

        self.cycle_no += 1;
        self.camera_sin_x = Pix3D::sin_table()
            .get(eye_pitch as usize)
            .copied()
            .unwrap_or(0);
        self.camera_cos_x = Pix3D::cos_table()
            .get(eye_pitch as usize)
            .copied()
            .unwrap_or(0);
        self.camera_sin_y = Pix3D::sin_table()
            .get(eye_yaw as usize)
            .copied()
            .unwrap_or(0);
        self.camera_cos_y = Pix3D::cos_table()
            .get(eye_yaw as usize)
            .copied()
            .unwrap_or(0);

        // TS `World.visBackingDirty = World.visBacking[pitchLevel][yawLevel]`
        // with `pitchLevel` 0..7 for the clamped 128..383 camera pitch; an
        // out-of-range pair binds `null` (all visibility reads false) rather
        // than the TS undefined-index throw.
        let pitch_level = ((eye_pitch - 128) / 32) as usize;
        let yaw_level = (eye_yaw / 64) as usize;
        self.vis_backing_dirty = if pitch_level < 8 && yaw_level < 32 {
            Some((pitch_level * 32 + yaw_level) * VIS_ROW_SIZE)
        } else {
            None
        };

        self.cx = eye_x;
        self.cy = eye_y;
        self.cz = eye_z;
        self.gx = eye_x / 128;
        self.gz = eye_z / 128;
        self.max_level = max_level;

        self.min_x = self.gx - 25;
        if self.min_x < 0 {
            self.min_x = 0;
        }

        self.min_z = self.gz - 25;
        if self.min_z < 0 {
            self.min_z = 0;
        }

        self.max_x = self.gx + 25;
        if self.max_x > world.max_tile_x {
            self.max_x = world.max_tile_x;
        }

        self.max_z = self.gz + 25;
        if self.max_z > world.max_tile_z {
            self.max_z = world.max_tile_z;
        }

        self.calc_occlude(world);
        self.fill_left = 0;

        for level in world.min_level..world.max_tile_level {
            for x in self.min_x..self.max_x {
                for z in self.min_z..self.max_z {
                    let tile = tile_at(&world.squares, level, x, z);
                    let Some(tile) = tile else {
                        continue;
                    };

                    let visible = tile.draw_level <= max_level
                        && (vis_backing_at(self, x + 25 - self.gx, z + 25 - self.gz)
                            || ground_h(world, level, x, z) - eye_y >= 2000);
                    if let Some(tile) = tile_at_mut(&mut world.squares, level, x, z) {
                        if visible {
                            tile.draw_front = true;
                            tile.draw_back = true;
                            tile.draw_sprites = tile.sprite_count > 0;
                        } else {
                            tile.draw_front = false;
                            tile.draw_back = false;
                            tile.corner_sides = 0;
                        }
                    }
                    if visible {
                        self.fill_left += 1;
                    }
                }
            }
        }
    }

    /// `renderAll(eyeX, eyeY, eyeZ, maxLevel, eyeYaw, eyePitch)` from
    /// client-ts. The `cache` and `loop_cycle` parameters are required, not
    /// optional: the TS `sprite.model?.worldRender(...)` chain calls
    /// `ModelSource.worldRender` -> `getTempModel()`, which rebuilds
    /// player/npc/loc-anim models from the config `Cache` and
    /// `Client.loopCycle` during the pass. `pix.hclip` is set per face as
    /// TS does (the Task 2 raster contract).
    #[allow(clippy::too_many_arguments)]
    pub fn render_all(
        &mut self,
        world: &mut World,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        cache: &Cache,
        loop_cycle: i32,
        eye_x: i32,
        eye_y: i32,
        eye_z: i32,
        max_level: i32,
        eye_yaw: i32,
        eye_pitch: i32,
    ) {
        // The pre-fill state (task 7): the build/share-light check, camera
        // fields, viewport bounds and the per-tile draw-front marking. The
        // wgpu backend runs this alone and rasterizes the marked tiles
        // itself; the CPU path runs it here ahead of the fill passes.
        self.prepare_scene(
            world, cache, loop_cycle, eye_x, eye_y, eye_z, max_level, eye_yaw, eye_pitch,
        );

        // Two fill passes, nearest-to-farthest ring order (`true` then
        // `false` for `checkAdjacent`), aborting when every tile is drawn.
        for level in world.min_level..world.max_tile_level {
            for dx in -25..=0 {
                let right_tile_x = self.gx + dx;
                let left_tile_x = self.gx - dx;

                if right_tile_x < self.min_x && left_tile_x >= self.max_x {
                    continue;
                }

                for dz in -25..=0 {
                    let forward_tile_z = self.gz + dz;
                    let backward_tile_z = self.gz - dz;

                    if right_tile_x >= self.min_x {
                        if forward_tile_z >= self.min_z {
                            let tile = tile_at(&world.squares, level, right_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, right_tile_x, forward_tile_z),
                                    true,
                                );
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile =
                                tile_at(&world.squares, level, right_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, right_tile_x, backward_tile_z),
                                    true,
                                );
                            }
                        }
                    }

                    if left_tile_x < self.max_x {
                        if forward_tile_z >= self.min_z {
                            let tile = tile_at(&world.squares, level, left_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, left_tile_x, forward_tile_z),
                                    true,
                                );
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile = tile_at(&world.squares, level, left_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, left_tile_x, backward_tile_z),
                                    true,
                                );
                            }
                        }
                    }

                    if self.fill_left == 0 {
                        world.click = false;
                        return;
                    }
                }
            }
        }

        for level in world.min_level..world.max_tile_level {
            for dx in -25..=0 {
                let right_tile_x = self.gx + dx;
                let left_tile_x = self.gx - dx;

                if right_tile_x < self.min_x && left_tile_x >= self.max_x {
                    continue;
                }

                for dz in -25..=0 {
                    let forward_tile_z = self.gz + dz;
                    let backward_tile_z = self.gz - dz;

                    if right_tile_x >= self.min_x {
                        if forward_tile_z >= self.min_z {
                            let tile = tile_at(&world.squares, level, right_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, right_tile_x, forward_tile_z),
                                    false,
                                );
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile =
                                tile_at(&world.squares, level, right_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, right_tile_x, backward_tile_z),
                                    false,
                                );
                            }
                        }
                    }

                    if left_tile_x < self.max_x {
                        if forward_tile_z >= self.min_z {
                            let tile = tile_at(&world.squares, level, left_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, left_tile_x, forward_tile_z),
                                    false,
                                );
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile = tile_at(&world.squares, level, left_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(
                                    world,
                                    pix,
                                    surface,
                                    cache,
                                    loop_cycle,
                                    (level, left_tile_x, backward_tile_z),
                                    false,
                                );
                            }
                        }
                    }

                    if self.fill_left == 0 {
                        world.click = false;
                        return;
                    }
                }
            }
        }

        // A successful pick this frame has already written `ground_x`.
        // Drop `click` so the next frame cannot re-raycast the same
        // screen coords as the camera follows the player (dest-flag hop).
        // Incomplete fills that never picked keep `click` so the next
        // frame retries, matching Java when `fillLeft` never hits 0.
        if world.ground_x != -1 {
            world.click = false;
        }
    }

    // --- Task 7: the GPU scene mesh ---
    //
    // The wgpu backend rasterizes the same scene graph the CPU `fill`
    // draws, but as a triangle list: every ground quad and
    // wall/decor/gd/object/sprite model of the `prepare_scene`-marked
    // tiles, transformed to camera space with the exact `world_render`
    // fixed-point math, backface-culled with the CPU winding test, and
    // shaded from the same colour table. The vertex shader divides by the
    // view depth; the depth buffer replaces the painter's per-face
    // priority merge. Documented divergences: loc mouse picks use the CPU
    // AABB pre-test plus the render2 face bbox (RuneLite GPU never
    // replaces clickboxes; AABB-only loc picks open doors on walk-by
    // clicks). Occluder tests are skipped (the depth buffer occludes).
    // Textured faces (models and ground) carry the
    // projective UV of the CPU `texture_triangle` and sample the shared
    // model atlas in the scene shader — they are not flat-shaded.

    /// Build the GPU scene mesh for the tiles `prepare_scene` marked
    /// drawable this frame. Mutates only what the CPU fill would: the
    /// lazily-resolved model caches, the sprite draw-once stamps and the
    /// ground click pick. The returned mesh is opaque-first so the backend
    /// can draw two ranges (depth-write on / alpha-blend).
    pub fn build_scene_mesh(
        &mut self,
        world: &mut World,
        cache: &Cache,
        loop_cycle: i32,
        pix: &mut Pix3DDraw,
    ) -> SceneMesh {
        let cam = SceneCam {
            eye_x: self.cx,
            eye_y: self.cy,
            eye_z: self.cz,
            sin_pitch: self.camera_sin_x,
            cos_pitch: self.camera_cos_x,
            sin_yaw: self.camera_sin_y,
            cos_yaw: self.camera_cos_y,
        };
        let cycle_no = self.cycle_no;
        let mut mesh = SceneMesh::default();
        for level in world.min_level..world.max_tile_level {
            for x in self.min_x..self.max_x {
                for z in self.min_z..self.max_z {
                    let draw = tile_at(&world.squares, level, x, z)
                        .map(|t| t.draw_front || t.draw_back)
                        .unwrap_or(false);
                    if !draw {
                        continue;
                    }
                    self.emit_tile(
                        world, cache, loop_cycle, pix, &mut mesh, &cam, level, x, z, cycle_no,
                    );
                }
            }
        }
        // Mirror `render_all`'s tail: a successful ground pick this frame
        // drops `click` so the next frame cannot re-raycast the same
        // screen coords.
        if world.ground_x != -1 {
            world.click = false;
        }
        mesh.sort_opaque_far_first();
        mesh
    }

    /// One tile of the `fill` pass, meshed instead of rasterised: linked
    /// square content, the tile ground, walls, decor, ground decor/objects
    /// and the scene/dynamic sprites. Wall/door/decor placement mirrors
    /// `fill` (the PRETAB direction gating, the DECORXOF branches); the
    /// occluder tests are skipped (the depth buffer occludes).
    #[allow(clippy::too_many_arguments)]
    fn emit_tile(
        &mut self,
        world: &mut World,
        cache: &Cache,
        loop_cycle: i32,
        pix: &mut Pix3DDraw,
        mesh: &mut SceneMesh,
        cam: &SceneCam,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        cycle_no: i32,
    ) {
        let original_level = tile_at(&world.squares, level, tile_x, tile_z)
            .map(|t| t.original_level)
            .unwrap_or(level);

        // Linked square (a level pushed down under this tile). Copy the
        // content out first: the emits below borrow the world mutably.
        let linked_quick = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.linked_square.as_ref())
            .and_then(|ls| ls.quick_ground);
        let linked_ground = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.linked_square.as_ref())
            .and_then(|ls| ls.ground.as_deref().cloned());
        let linked_wall = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.linked_square.as_ref())
            .and_then(|ls| ls.wall.as_ref())
            .map(|w| (w.typecode, w.x, w.y, w.z));
        let linked_sprites: Vec<usize> = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.linked_square.as_ref())
            .map(|ls| {
                (0..ls.sprite_count as usize)
                    .filter_map(|i| ls.sprites[i])
                    .collect()
            })
            .unwrap_or_default();

        if let Some(quick) = linked_quick {
            emit_quick_ground(world, pix, mesh, cam, quick, 0, tile_x, tile_z);
        } else if let Some(ground) = linked_ground {
            emit_ground(world, pix, mesh, cam, ground, tile_x, tile_z);
        }
        if let Some((typecode, wall_x, wall_y, wall_z)) = linked_wall {
            let (wall_x, wall_y, wall_z) =
                (wall_x - cam.eye_x, wall_y - cam.eye_y, wall_z - cam.eye_z);
            if let Some(model) = self
                .linked_wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                .0
                .as_mut()
            {
                emit_scene_model(
                    model, cache, loop_cycle, pix, mesh, cam, 0, wall_x, wall_y, wall_z, typecode,
                    true,
                );
            }
        }
        for index in linked_sprites {
            self.emit_sprite(world, cache, loop_cycle, pix, mesh, cam, index, cycle_no);
        }

        // The tile's own ground.
        let quick = tile_at(&world.squares, level, tile_x, tile_z).and_then(|t| t.quick_ground);
        if let Some(quick) = quick {
            emit_quick_ground(world, pix, mesh, cam, quick, original_level, tile_x, tile_z);
        } else if let Some(ground) = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.ground.as_deref().cloned())
        {
            emit_ground(world, pix, mesh, cam, ground, tile_x, tile_z);
        }

        // Walls: both slots, unconditionally (the CPU's `front_wall_types`
        // gate is a painter optimisation). They are meshed into the wall
        // bucket so they draw *after* scenery — the CPU's back-wall pass
        // overwrites a same-tile booth that occupies the wall's thickness.
        let wall_data = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.wall.as_ref())
            .map(|w| {
                (
                    w.typecode,
                    w.x - cam.eye_x,
                    w.y - cam.eye_y,
                    w.z - cam.eye_z,
                )
            });
        if let Some((typecode, wall_x, wall_y, wall_z)) = wall_data {
            if let Some(model) = self
                .wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                .0
                .as_mut()
            {
                emit_scene_model(
                    model, cache, loop_cycle, pix, mesh, cam, 0, wall_x, wall_y, wall_z, typecode,
                    true,
                );
            }
            if let Some(model) = self
                .wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                .1
                .as_mut()
            {
                emit_scene_model(
                    model, cache, loop_cycle, pix, mesh, cam, 0, wall_x, wall_y, wall_z, typecode,
                    true,
                );
            }
        }

        // Decor: the `fill` placement branches verbatim (front wall types
        // vs the DECORXOF/DECORXOF2 offset corners).
        let decor_data = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.decor.as_ref())
            .map(|d| {
                (
                    d.wshape,
                    d.angle,
                    d.typecode,
                    d.x - cam.eye_x,
                    d.y - cam.eye_y,
                    d.z - cam.eye_z,
                )
            });
        if let Some((wshape, angle, typecode, decor_x, decor_y, decor_z)) = decor_data {
            let mut direction = 0i32;
            let gx = cam.eye_x / 128;
            let gz = cam.eye_z / 128;
            if gx == tile_x {
                direction += 1;
            } else if gx < tile_x {
                direction += 2;
            }
            if gz == tile_z {
                direction += 3;
            } else if gz > tile_z {
                direction += 6;
            }
            let front_wall_types = PRETAB.get(direction as usize).copied().unwrap_or(0);
            if (wshape & front_wall_types) != 0 {
                if let Some(decor) = self
                    .decor_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                    .as_mut()
                {
                    emit_scene_model(
                        decor, cache, loop_cycle, pix, mesh, cam, angle, decor_x, decor_y, decor_z,
                        typecode, true,
                    );
                }
            } else if (wshape & 0x300) != 0 {
                let nearest_x = if angle == LocAngle::NORTH || angle == LocAngle::EAST {
                    -decor_x
                } else {
                    decor_x
                };
                let nearest_z = if angle == LocAngle::EAST || angle == LocAngle::SOUTH {
                    -decor_z
                } else {
                    decor_z
                };
                if (wshape & 0x100) != 0 && nearest_z < nearest_x {
                    let draw_x = decor_x + DECORXOF.get(angle as usize).copied().unwrap_or(0);
                    let draw_z = decor_z + DECORZOF.get(angle as usize).copied().unwrap_or(0);
                    if let Some(decor) = self
                        .decor_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                        .as_mut()
                    {
                        emit_scene_model(
                            decor,
                            cache,
                            loop_cycle,
                            pix,
                            mesh,
                            cam,
                            angle * 512 + 256,
                            draw_x,
                            decor_y,
                            draw_z,
                            typecode,
                            true,
                        );
                    }
                }
                if (wshape & 0x200) != 0 && nearest_z > nearest_x {
                    let draw_x = decor_x + DECORXOF2.get(angle as usize).copied().unwrap_or(0);
                    let draw_z = decor_z + DECORZOF2.get(angle as usize).copied().unwrap_or(0);
                    if let Some(decor) = self
                        .decor_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                        .as_mut()
                    {
                        emit_scene_model(
                            decor,
                            cache,
                            loop_cycle,
                            pix,
                            mesh,
                            cam,
                            (angle * 512 + 1280) & 0x7ff,
                            draw_x,
                            decor_y,
                            draw_z,
                            typecode,
                            true,
                        );
                    }
                }
            }
        }

        // Ground decor + ground objects (stack height 0).
        if let Some((typecode, gd_x, gd_y, gd_z)) = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.ground_decor.as_ref())
            .map(|gd| {
                (
                    gd.typecode,
                    gd.x - cam.eye_x,
                    gd.y - cam.eye_y,
                    gd.z - cam.eye_z,
                )
            })
        {
            if let Some(model) = self
                .gd_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                .as_mut()
            {
                emit_scene_model(
                    model, cache, loop_cycle, pix, mesh, cam, 0, gd_x, gd_y, gd_z, typecode, false,
                );
            }
        }
        if let Some((typecode, ox, oy, oz)) = tile_at(&world.squares, level, tile_x, tile_z)
            .and_then(|t| t.ground_object.as_ref())
            .map(|o| {
                (
                    o.typecode,
                    o.x - cam.eye_x,
                    o.y - cam.eye_y,
                    o.z - cam.eye_z,
                )
            })
        {
            let height =
                self.ground_object_height(&*world, cache, loop_cycle, level, tile_x, tile_z);
            if height == 0 {
                let (bottom, middle, top) =
                    self.obj_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z);
                if let Some(model) = bottom.as_mut() {
                    emit_scene_model(
                        model, cache, loop_cycle, pix, mesh, cam, 0, ox, oy, oz, typecode, false,
                    );
                }
                if let Some(model) = middle.as_mut() {
                    emit_scene_model(
                        model, cache, loop_cycle, pix, mesh, cam, 0, ox, oy, oz, typecode, false,
                    );
                }
                if let Some(model) = top.as_mut() {
                    emit_scene_model(
                        model, cache, loop_cycle, pix, mesh, cam, 0, ox, oy, oz, typecode, false,
                    );
                }
            }
        }

        // The tile's sprites (scene + dynamic). The `sprite.cycle` stamp
        // keeps a multi-tile sprite meshed once per frame, exactly as the
        // CPU fill's sprite buffer does.
        let sprite_indices: Vec<usize> = tile_at(&world.squares, level, tile_x, tile_z)
            .map(|t| {
                (0..t.sprite_count as usize)
                    .filter_map(|i| t.sprites[i])
                    .collect()
            })
            .unwrap_or_default();
        for index in sprite_indices {
            self.emit_sprite(world, cache, loop_cycle, pix, mesh, cam, index, cycle_no);
        }
    }

    /// Mesh one sprite-arena slot (a scene sprite or a dynamic entity):
    /// resolve/decode its temp model, place it at the sprite's position
    /// with the sprite's yaw, and stamp the once-per-frame cycle mark.
    fn emit_sprite(
        &mut self,
        world: &mut World,
        cache: &Cache,
        loop_cycle: i32,
        pix: &mut Pix3DDraw,
        mesh: &mut SceneMesh,
        cam: &SceneCam,
        index: usize,
        cycle_no: i32,
    ) {
        let Some(sprite) = world.sprites.get(index).and_then(|s| s.as_ref()) else {
            return;
        };
        if sprite.cycle == cycle_no {
            return;
        }
        let (yaw, typecode, x, y, z) = (sprite.yaw, sprite.typecode, sprite.x, sprite.y, sprite.z);
        let info = sprite.typecode2 & 0xff;
        let (level, min_tile_x, max_tile_x, min_tile_z, max_tile_z) = (
            sprite.level,
            sprite.min_tile_x,
            sprite.max_tile_x,
            sprite.min_tile_z,
            sprite.max_tile_z,
        );
        let scene_sprite = (typecode >> 29) & 0x3 == 2;

        // Scene sprites are occluded exactly like the CPU fill: skip one
        // fully behind a wall so the depth buffer does not let a scenery
        // model's back face poke through the wall it stands against (the
        // bank booth through the bank wall). Dynamic sprites are attached
        // by the entity passes and are never occluder-tested.
        if scene_sprite {
            let model_min_y = self
                .sprite_model_mut(&*world, cache, loop_cycle, index)
                .as_ref()
                .map(|m| m.min_y())
                .unwrap_or(0);
            let occluded = self.sprite_occluded2(
                world,
                level,
                min_tile_x,
                max_tile_x,
                min_tile_z,
                max_tile_z,
                model_min_y,
            );
            if crate::render_debug_enabled() {
                let name = cache
                    .locs
                    .get(((typecode >> 14) & 0x7fff) as usize)
                    .map(|l| l.name.as_str())
                    .unwrap_or("");
                eprintln!(
                    "[sprite-occl] loc {name} tiles ({min_tile_x},{min_tile_z})..({max_tile_x},{max_tile_z}) angle={} yaw={yaw} occluded={occluded}",
                    info >> 6
                );
            }
            if occluded {
                if let Some(sprite) = world.sprites.get_mut(index).and_then(|s| s.as_mut()) {
                    sprite.cycle = cycle_no;
                }
                return;
            }
        }

        // 274 loc 1602 is wallwidth=8 with a 16-unit-thick model; a 1×1
        // centrepiece on the same tile (the bank booth) fills that
        // thickness. RuneLite's GPU plugin (vert.glsl `screenPos.z +=
        // bias/128`, GL_GREATER) relies on OSRS placements that do not
        // overlap like this — the same bias cannot beat 16 units of
        // camera z. CPU 274 paints the wall over the overlap in the
        // back-wall pass; slide the scenery inward so the depth buffer
        // sees the adjacent-tile layout that already works.
        let (nudge_x, nudge_z) = if scene_sprite {
            scenery_inward_nudge(world, level, min_tile_x, max_tile_x, min_tile_z, max_tile_z)
        } else {
            (0, 0)
        };
        if let Some(model) = self
            .sprite_model_mut(&*world, cache, loop_cycle, index)
            .as_mut()
        {
            emit_scene_model(
                model,
                cache,
                loop_cycle,
                pix,
                mesh,
                cam,
                yaw,
                x - cam.eye_x + nudge_x,
                y - cam.eye_y,
                z - cam.eye_z + nudge_z,
                typecode,
                false,
            );
        }
        if let Some(sprite) = world.sprites.get_mut(index).and_then(|s| s.as_mut()) {
            sprite.cycle = cycle_no;
        }
    }

    /// `calcOcclude()` from client-ts: pick the occluders whose tiles are
    /// visible this frame and pre-compute their frustum-edge deltas. The TS
    /// `World.activeOccluders` holds object references; here they are arena
    /// indices into the sim world's `occluders`.
    fn calc_occlude(&mut self, world: &mut World) {
        let level = self.max_level;
        if level < 0 || level as usize >= OCCLUDER_LEVELS {
            return;
        }
        let count = world.num_occluders[level as usize];
        self.num_active_occluders = 0;

        'occluder: for i in 0..count as usize {
            let index = level as usize * MAX_OCCLUDERS + i;
            let Some(occluder) = world.occluders.get(index).and_then(|o| o.as_ref()) else {
                continue;
            };
            let (r#type, min_tile_x, max_tile_x, min_tile_z, max_tile_z) = (
                occluder.r#type,
                occluder.min_tile_x,
                occluder.max_tile_x,
                occluder.min_tile_z,
                occluder.max_tile_z,
            );
            let (min_x, max_x, min_z, max_z, min_y, max_y) = (
                occluder.min_x,
                occluder.max_x,
                occluder.min_z,
                occluder.max_z,
                occluder.min_y,
                occluder.max_y,
            );

            let mut mode = 0i32;
            let mut min_delta_x = 0i32;
            let mut max_delta_x = 0i32;
            let mut min_delta_z = 0i32;
            let mut max_delta_z = 0i32;
            let mut min_delta_y = 0i32;
            let mut max_delta_y = 0i32;
            let mut active = false;

            if r#type == 1 {
                let delta_max_y = min_tile_x + 25 - self.gx;
                if delta_max_y >= 0 && delta_max_y <= 50 {
                    let mut delta_min_tile_z = min_tile_z + 25 - self.gz;
                    if delta_min_tile_z < 0 {
                        delta_min_tile_z = 0;
                    }

                    let mut delta_max_tile_z = max_tile_z + 25 - self.gz;
                    if delta_max_tile_z > 50 {
                        delta_max_tile_z = 50;
                    }

                    let mut ok = false;
                    while delta_min_tile_z <= delta_max_tile_z {
                        if vis_backing_at(self, delta_max_y, delta_min_tile_z) {
                            ok = true;
                            break;
                        }
                        delta_min_tile_z += 1;
                    }

                    if ok {
                        let mut delta_max_tile_x = self.cx - min_x;
                        if delta_max_tile_x > 32 {
                            mode = 1;
                        } else {
                            if delta_max_tile_x >= -32 {
                                continue 'occluder;
                            }

                            mode = 2;
                            delta_max_tile_x = -delta_max_tile_x;
                        }

                        min_delta_z =
                            ((min_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_z =
                            ((max_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        min_delta_y =
                            ((min_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_y =
                            ((max_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        active = true;
                    }
                }
            } else if r#type == 2 {
                let delta_max_y = min_tile_z + 25 - self.gz;

                if delta_max_y >= 0 && delta_max_y <= 50 {
                    let mut delta_min_tile_z = min_tile_x + 25 - self.gx;
                    if delta_min_tile_z < 0 {
                        delta_min_tile_z = 0;
                    }

                    let mut delta_max_tile_z = max_tile_x + 25 - self.gx;
                    if delta_max_tile_z > 50 {
                        delta_max_tile_z = 50;
                    }

                    let mut ok = false;
                    while delta_min_tile_z <= delta_max_tile_z {
                        if vis_backing_at(self, delta_min_tile_z, delta_max_y) {
                            ok = true;
                            break;
                        }
                        delta_min_tile_z += 1;
                    }

                    if ok {
                        let mut delta_max_tile_x = self.cz - min_z;
                        if delta_max_tile_x > 32 {
                            mode = 3;
                        } else {
                            if delta_max_tile_x >= -32 {
                                continue 'occluder;
                            }

                            mode = 4;
                            delta_max_tile_x = -delta_max_tile_x;
                        }

                        min_delta_x =
                            ((min_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_x =
                            ((max_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        min_delta_y =
                            ((min_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_y =
                            ((max_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        active = true;
                    }
                }
            } else if r#type == 4 {
                let delta_max_y = min_y - self.cy;

                if delta_max_y > 128 {
                    let mut delta_min_tile_z = min_tile_z + 25 - self.gz;
                    if delta_min_tile_z < 0 {
                        delta_min_tile_z = 0;
                    }

                    let mut delta_max_tile_z = max_tile_z + 25 - self.gz;
                    if delta_max_tile_z > 50 {
                        delta_max_tile_z = 50;
                    }

                    if delta_min_tile_z <= delta_max_tile_z {
                        let mut delta_min_tile_x = min_tile_x + 25 - self.gx;
                        if delta_min_tile_x < 0 {
                            delta_min_tile_x = 0;
                        }

                        let mut delta_max_tile_x = max_tile_x + 25 - self.gx;
                        if delta_max_tile_x > 50 {
                            delta_max_tile_x = 50;
                        }

                        let mut ok = false;
                        'find_visible_tile: for x in delta_min_tile_x..=delta_max_tile_x {
                            for z in delta_min_tile_z..=delta_max_tile_z {
                                if vis_backing_at(self, x, z) {
                                    ok = true;
                                    break 'find_visible_tile;
                                }
                            }
                        }

                        if ok {
                            mode = 5;
                            min_delta_x =
                                ((min_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_y);
                            max_delta_x =
                                ((max_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_y);
                            min_delta_z =
                                ((min_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_y);
                            max_delta_z =
                                ((max_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_y);
                            active = true;
                        }
                    }
                }
            }

            if active {
                let occluder = world.occluders.get_mut(index).and_then(|o| o.as_mut());
                if let Some(occluder) = occluder {
                    occluder.mode = mode;
                    occluder.min_delta_x = min_delta_x;
                    occluder.max_delta_x = max_delta_x;
                    occluder.min_delta_z = min_delta_z;
                    occluder.max_delta_z = max_delta_z;
                    occluder.min_delta_y = min_delta_y;
                    occluder.max_delta_y = max_delta_y;
                }
                if let Some(slot) = self
                    .active_occluders
                    .get_mut(self.num_active_occluders as usize)
                {
                    *slot = Some(index);
                }
                self.num_active_occluders += 1;
            }
        }

        if crate::render_debug_enabled() {
            let mut tiles = Vec::new();
            for i in 0..self.num_active_occluders as usize {
                if let Some(index) = self.active_occluders.get(i).copied().flatten() {
                    if let Some(o) = world.occluders.get(index).and_then(|o| o.as_ref()) {
                        tiles.push((o.min_tile_x, o.min_tile_z, o.max_tile_x, o.max_tile_z));
                    }
                }
            }
            eprintln!(
                "[occluder] active={} tiles={tiles:?}",
                self.num_active_occluders
            );
        }
    }

    /// Java/TS `LinkList.push`: if the square is already queued, unlink it
    /// and append at the tail. Stamping the square and the deque entry makes
    /// the older copy a no-op when popped.
    fn enqueue_fill(&mut self, world: &mut World, level: i32, x: i32, z: i32) {
        self.fill_gen = self.fill_gen.wrapping_add(1);
        if self.fill_gen == 0 {
            self.fill_gen = 1;
        }
        let stamp = self.fill_gen;
        if let Some(tile) = tile_at_mut(&mut world.squares, level, x, z) {
            tile.fill_stamp = stamp;
        }
        self.fill_queue.push_back((level, x, z, stamp));
    }

    /// `fill(next, checkAdjacent)` from client-ts 1389-1923. Queue entries
    /// are `(level, x, z, stamp)`; tiles are re-fetched by coordinate.
    /// Java/TS `LinkList.push` moves an already-queued Square to the tail.
    #[allow(clippy::too_many_arguments)]
    fn fill(
        &mut self,
        world: &mut World,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        cache: &Cache,
        loop_cycle: i32,
        next: (i32, i32, i32),
        mut check_adjacent: bool,
    ) {
        self.enqueue_fill(world, next.0, next.1, next.2);

        'fill: loop {
            // `do { tile = popFront(); if (!tile) return; } while (!tile.drawBack)`
            let (tile_x, tile_z, level) = loop {
                let Some((level, x, z, stamp)) = self.fill_queue.pop_front() else {
                    return;
                };
                let tile = tile_at(&world.squares, level, x, z);
                if !tile.is_some_and(|t| t.draw_back && t.fill_stamp == stamp) {
                    continue;
                }
                if let Some(tile) = tile_at_mut(&mut world.squares, level, x, z) {
                    tile.fill_stamp = 0;
                }
                break (x, z, level);
            };

            // TS `World` statics that never change during a fill.
            let gx = self.gx;
            let gz = self.gz;
            let cx = self.cx;
            let cy = self.cy;
            let cz = self.cz;
            let cycle_no = self.cycle_no;
            let min_x = self.min_x;
            let max_x = self.max_x;
            let min_z = self.min_z;
            let max_z = self.max_z;
            let sin_pitch = self.camera_sin_x;
            let cos_pitch = self.camera_cos_x;
            let sin_yaw = self.camera_sin_y;
            let cos_yaw = self.camera_cos_y;
            let original_level = tile_at(&world.squares, level, tile_x, tile_z)
                .map(|t| t.original_level)
                .unwrap_or(level);

            let draw_front = tile_at(&world.squares, level, tile_x, tile_z)
                .map(|t| t.draw_front)
                .unwrap_or(false);

            if draw_front {
                if check_adjacent {
                    if level > 0 {
                        let above = tile_at(&world.squares, level - 1, tile_x, tile_z);
                        if above.is_some_and(|t| t.draw_back) {
                            continue 'fill;
                        }
                    }

                    let sprite_spans = tile_at(&world.squares, level, tile_x, tile_z)
                        .map(|t| t.sprite_spans)
                        .unwrap_or(0);

                    if tile_x <= gx && tile_x > min_x {
                        let adjacent = tile_at(&world.squares, level, tile_x - 1, tile_z);
                        if adjacent.is_some_and(|t| {
                            t.draw_back && (t.draw_front || (sprite_spans & 0x1) == 0)
                        }) {
                            continue 'fill;
                        }
                    }

                    if tile_x >= gx && tile_x < max_x - 1 {
                        let adjacent = tile_at(&world.squares, level, tile_x + 1, tile_z);
                        if adjacent.is_some_and(|t| {
                            t.draw_back && (t.draw_front || (sprite_spans & 0x4) == 0)
                        }) {
                            continue 'fill;
                        }
                    }

                    if tile_z <= gz && tile_z > min_z {
                        let adjacent = tile_at(&world.squares, level, tile_x, tile_z - 1);
                        if adjacent.is_some_and(|t| {
                            t.draw_back && (t.draw_front || (sprite_spans & 0x8) == 0)
                        }) {
                            continue 'fill;
                        }
                    }

                    if tile_z >= gz && tile_z < max_z - 1 {
                        let adjacent = tile_at(&world.squares, level, tile_x, tile_z + 1);
                        if adjacent.is_some_and(|t| {
                            t.draw_back && (t.draw_front || (sprite_spans & 0x2) == 0)
                        }) {
                            continue 'fill;
                        }
                    }
                } else {
                    check_adjacent = true;
                }

                if let Some(tile) = tile_at_mut(&mut world.squares, level, tile_x, tile_z) {
                    tile.draw_front = false;
                }

                // Linked square (a level pushed down under this tile).
                let linked_quick = tile_at(&world.squares, level, tile_x, tile_z)
                    .and_then(|t| t.linked_square.as_ref())
                    .and_then(|ls| ls.quick_ground);
                if let Some(quick) = linked_quick {
                    if !self.ground_occluded(world, 0, tile_x, tile_z) {
                        self.render_quick_ground(
                            world, pix, surface, quick, 0, tile_x, tile_z, sin_pitch, cos_pitch,
                            sin_yaw, cos_yaw,
                        );
                    }
                } else {
                    let linked_ground = tile_at(&world.squares, level, tile_x, tile_z)
                        .and_then(|t| t.linked_square.as_ref())
                        .and_then(|ls| ls.ground.as_deref().cloned());
                    if let Some(ground) = linked_ground {
                        if !self.ground_occluded(world, 0, tile_x, tile_z) {
                            self.render_ground(
                                world, pix, surface, tile_x, tile_z, ground, sin_pitch, cos_pitch,
                                sin_yaw, cos_yaw,
                            );
                        }
                    }
                }

                {
                    let linked = tile_at(&world.squares, level, tile_x, tile_z)
                        .and_then(|t| t.linked_square.as_ref())
                        .and_then(|ls| ls.wall.as_ref())
                        .map(|w| (w.typecode, w.x - cx, w.y - cy, w.z - cz));
                    if let Some((typecode, wall_x, wall_y, wall_z)) = linked {
                        if let Some(model) = self
                            .linked_wall_models_mut(
                                &*world, cache, loop_cycle, level, tile_x, tile_z,
                            )
                            .0
                            .as_mut()
                        {
                            model.world_render(
                                cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw,
                                cos_yaw, wall_x, wall_y, wall_z, typecode,
                            );
                        }
                    }
                }

                let linked_sprites: Vec<usize> = tile_at(&world.squares, level, tile_x, tile_z)
                    .and_then(|t| t.linked_square.as_ref())
                    .map(|ls| {
                        (0..ls.sprite_count as usize)
                            .filter_map(|i| ls.sprites[i])
                            .collect()
                    })
                    .unwrap_or_default();
                for index in linked_sprites {
                    if let Some(sprite) = world.sprites.get(index).and_then(|s| s.as_ref()) {
                        let (yaw, typecode, x, y, z) =
                            (sprite.yaw, sprite.typecode, sprite.x, sprite.y, sprite.z);
                        if let Some(model) = self
                            .sprite_model_mut(&*world, cache, loop_cycle, index)
                            .as_mut()
                        {
                            model.world_render(
                                cache,
                                loop_cycle,
                                pix,
                                surface,
                                yaw,
                                sin_pitch,
                                cos_pitch,
                                sin_yaw,
                                cos_yaw,
                                x - cx,
                                y - cy,
                                z - cz,
                                typecode,
                            );
                        }
                    }
                }

                // The tile's own ground.
                let mut tile_drawn = false;
                let quick =
                    tile_at(&world.squares, level, tile_x, tile_z).and_then(|t| t.quick_ground);
                if let Some(quick) = quick {
                    if !self.ground_occluded(world, original_level, tile_x, tile_z) {
                        tile_drawn = true;
                        self.render_quick_ground(
                            world,
                            pix,
                            surface,
                            quick,
                            original_level,
                            tile_x,
                            tile_z,
                            sin_pitch,
                            cos_pitch,
                            sin_yaw,
                            cos_yaw,
                        );
                    }
                } else {
                    let ground = tile_at(&world.squares, level, tile_x, tile_z)
                        .and_then(|t| t.ground.as_deref().cloned());
                    if let Some(ground) = ground {
                        if !self.ground_occluded(world, original_level, tile_x, tile_z) {
                            tile_drawn = true;
                            self.render_ground(
                                world, pix, surface, tile_x, tile_z, ground, sin_pitch, cos_pitch,
                                sin_yaw, cos_yaw,
                            );
                        }
                    }
                }

                // `direction`/`frontWallTypes`/`backWallTypes` from the
                // camera-relative tile position.
                let mut direction = 0i32;
                let mut front_wall_types = 0i32;
                let has_wall_or_decor = tile_at(&world.squares, level, tile_x, tile_z)
                    .is_some_and(|t| t.wall.is_some() || t.decor.is_some());
                if has_wall_or_decor {
                    if gx == tile_x {
                        direction += 1;
                    } else if gx < tile_x {
                        direction += 2;
                    }

                    if gz == tile_z {
                        direction += 3;
                    } else if gz > tile_z {
                        direction += 6;
                    }

                    front_wall_types = PRETAB.get(direction as usize).copied().unwrap_or(0);
                    if let Some(tile) = tile_at_mut(&mut world.squares, level, tile_x, tile_z) {
                        tile.back_wall_types =
                            POSTTAB.get(direction as usize).copied().unwrap_or(0);
                    }
                }

                // Wall corner-sides bookkeeping and the front wall renders.
                let wall_data = tile_at(&world.squares, level, tile_x, tile_z)
                    .and_then(|t| t.wall.as_ref())
                    .map(|w| (w.angle1, w.angle2, w.typecode, w.x - cx, w.y - cy, w.z - cz));
                if let Some((angle1, angle2, typecode, wall_x, wall_y, wall_z)) = wall_data {
                    if let Some(tile) = tile_at_mut(&mut world.squares, level, tile_x, tile_z) {
                        if (angle1 & MIDTAB[direction as usize]) == 0 {
                            tile.corner_sides = 0;
                        } else if angle1 == 16 {
                            tile.corner_sides = 3;
                            tile.sides_before_corner = MIDDEP_16[direction as usize];
                            tile.sides_after_corner = 3 - tile.sides_before_corner;
                        } else if angle1 == 32 {
                            tile.corner_sides = 6;
                            tile.sides_before_corner = MIDDEP_32[direction as usize];
                            tile.sides_after_corner = 6 - tile.sides_before_corner;
                        } else if angle1 == 64 {
                            tile.corner_sides = 12;
                            tile.sides_before_corner = MIDDEP_64[direction as usize];
                            tile.sides_after_corner = 12 - tile.sides_before_corner;
                        } else {
                            tile.corner_sides = 9;
                            tile.sides_before_corner = MIDDEP_128[direction as usize];
                            tile.sides_after_corner = 9 - tile.sides_before_corner;
                        }
                    }

                    if (angle1 & front_wall_types) != 0
                        && !self.wall_occluded(world, original_level, tile_x, tile_z, angle1)
                    {
                        if let Some(model) = self
                            .wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                            .0
                            .as_mut()
                        {
                            model.world_render(
                                cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw,
                                cos_yaw, wall_x, wall_y, wall_z, typecode,
                            );
                        }
                    }

                    if (angle2 & front_wall_types) != 0
                        && !self.wall_occluded(world, original_level, tile_x, tile_z, angle2)
                    {
                        if let Some(model) = self
                            .wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                            .1
                            .as_mut()
                        {
                            model.world_render(
                                cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw,
                                cos_yaw, wall_x, wall_y, wall_z, typecode,
                            );
                        }
                    }
                }

                // Decor on the near side of the tile.
                let decor_data = tile_at(&world.squares, level, tile_x, tile_z)
                    .and_then(|t| t.decor.as_ref())
                    .map(|d| (d.wshape, d.angle, d.typecode, d.x - cx, d.y - cy, d.z - cz));
                if let Some((wshape, angle, typecode, decor_x, decor_y, decor_z)) = decor_data {
                    let min_y = self
                        .decor_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                        .as_ref()
                        .map(|m| m.min_y())
                        .unwrap_or(1000);
                    if !self.sprite_occluded(world, original_level, tile_x, tile_z, min_y) {
                        if (wshape & front_wall_types) != 0 {
                            if let Some(decor) = self
                                .decor_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                                .as_mut()
                            {
                                decor.world_render(
                                    cache, loop_cycle, pix, surface, angle, sin_pitch, cos_pitch,
                                    sin_yaw, cos_yaw, decor_x, decor_y, decor_z, typecode,
                                );
                            }
                        } else if (wshape & 0x300) != 0 {
                            let nearest_x = if angle == LocAngle::NORTH || angle == LocAngle::EAST {
                                -decor_x
                            } else {
                                decor_x
                            };

                            let nearest_z = if angle == LocAngle::EAST || angle == LocAngle::SOUTH {
                                -decor_z
                            } else {
                                decor_z
                            };

                            if (wshape & 0x100) != 0 && nearest_z < nearest_x {
                                let draw_x =
                                    decor_x + DECORXOF.get(angle as usize).copied().unwrap_or(0);
                                let draw_z =
                                    decor_z + DECORZOF.get(angle as usize).copied().unwrap_or(0);
                                if let Some(decor) = self
                                    .decor_model_mut(
                                        &*world, cache, loop_cycle, level, tile_x, tile_z,
                                    )
                                    .as_mut()
                                {
                                    decor.world_render(
                                        cache,
                                        loop_cycle,
                                        pix,
                                        surface,
                                        angle * 512 + 256,
                                        sin_pitch,
                                        cos_pitch,
                                        sin_yaw,
                                        cos_yaw,
                                        draw_x,
                                        decor_y,
                                        draw_z,
                                        typecode,
                                    );
                                }
                            }

                            if (wshape & 0x200) != 0 && nearest_z > nearest_x {
                                let draw_x =
                                    decor_x + DECORXOF2.get(angle as usize).copied().unwrap_or(0);
                                let draw_z =
                                    decor_z + DECORZOF2.get(angle as usize).copied().unwrap_or(0);
                                if let Some(decor) = self
                                    .decor_model_mut(
                                        &*world, cache, loop_cycle, level, tile_x, tile_z,
                                    )
                                    .as_mut()
                                {
                                    decor.world_render(
                                        cache,
                                        loop_cycle,
                                        pix,
                                        surface,
                                        (angle * 512 + 1280) & 0x7ff,
                                        sin_pitch,
                                        cos_pitch,
                                        sin_yaw,
                                        cos_yaw,
                                        draw_x,
                                        decor_y,
                                        draw_z,
                                        typecode,
                                    );
                                }
                            }
                        }
                    }
                }

                // Ground decor + ground objects (stack height 0) on a drawn tile.
                if tile_drawn {
                    let ground_decor_data = tile_at(&world.squares, level, tile_x, tile_z)
                        .and_then(|t| t.ground_decor.as_ref())
                        .map(|gd| (gd.typecode, gd.x - cx, gd.y - cy, gd.z - cz));
                    if let Some((typecode, gd_x, gd_y, gd_z)) = ground_decor_data {
                        if let Some(model) = self
                            .gd_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                            .as_mut()
                        {
                            model.world_render(
                                cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw,
                                cos_yaw, gd_x, gd_y, gd_z, typecode,
                            );
                        }
                    }

                    let ground_object_data = tile_at(&world.squares, level, tile_x, tile_z)
                        .and_then(|t| t.ground_object.as_ref())
                        .map(|o| (o.typecode, o.x - cx, o.y - cy, o.z - cz));
                    if let Some((typecode, ox, oy, oz)) = ground_object_data {
                        let height = self.ground_object_height(
                            &*world, cache, loop_cycle, level, tile_x, tile_z,
                        );
                        if height == 0 {
                            let (bottom, middle, top) = self
                                .obj_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z);
                            if let Some(model) = bottom.as_mut() {
                                model.world_render(
                                    cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch,
                                    sin_yaw, cos_yaw, ox, oy, oz, typecode,
                                );
                            }

                            if let Some(model) = middle.as_mut() {
                                model.world_render(
                                    cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch,
                                    sin_yaw, cos_yaw, ox, oy, oz, typecode,
                                );
                            }

                            if let Some(model) = top.as_mut() {
                                model.world_render(
                                    cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch,
                                    sin_yaw, cos_yaw, ox, oy, oz, typecode,
                                );
                            }
                        }
                    }
                }

                // Sprite-span adjacency: queue tiles the sprite covers.
                let spans = tile_at(&world.squares, level, tile_x, tile_z)
                    .map(|t| t.sprite_spans)
                    .unwrap_or(0);
                if spans != 0 {
                    if tile_x < gx && (spans & 0x4) != 0 {
                        let adjacent = tile_at(&world.squares, level, tile_x + 1, tile_z);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(world, level, tile_x + 1, tile_z);
                        }
                    }

                    if tile_z < gz && (spans & 0x2) != 0 {
                        let adjacent = tile_at(&world.squares, level, tile_x, tile_z + 1);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(world, level, tile_x, tile_z + 1);
                        }
                    }

                    if tile_x > gx && (spans & 0x1) != 0 {
                        let adjacent = tile_at(&world.squares, level, tile_x - 1, tile_z);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(world, level, tile_x - 1, tile_z);
                        }
                    }

                    if tile_z > gz && (spans & 0x8) != 0 {
                        let adjacent = tile_at(&world.squares, level, tile_x, tile_z - 1);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(world, level, tile_x, tile_z - 1);
                        }
                    }
                }
            }

            // Corner-side walls draw after every sprite slot on the tile has
            // been considered.
            let (corner_sides, sprite_pairs, sides_before, sides_after) =
                tile_at(&world.squares, level, tile_x, tile_z)
                    .map(|t| {
                        (
                            t.corner_sides,
                            (0..t.sprite_count as usize)
                                .map(|i| (t.sprites[i], t.sprite_span[i]))
                                .collect::<Vec<_>>(),
                            t.sides_before_corner,
                            t.sides_after_corner,
                        )
                    })
                    .unwrap_or((0, Vec::new(), 0, 0));
            if corner_sides != 0 {
                let mut draw = true;
                for (sprite_index, span) in &sprite_pairs {
                    let Some(sprite_index) = sprite_index else {
                        continue;
                    };
                    let Some(sprite) = world.sprites.get(*sprite_index).and_then(|s| s.as_ref())
                    else {
                        continue;
                    };

                    if sprite.cycle != cycle_no && (span & corner_sides) == sides_before {
                        draw = false;
                        break;
                    }
                }

                if draw {
                    let wall_data = tile_at(&world.squares, level, tile_x, tile_z)
                        .and_then(|t| t.wall.as_ref())
                        .map(|w| (w.angle1, w.typecode, w.x - cx, w.y - cy, w.z - cz));
                    if let Some((angle1, typecode, wall_x, wall_y, wall_z)) = wall_data {
                        if !self.wall_occluded(world, original_level, tile_x, tile_z, angle1) {
                            if let Some(model) = self
                                .wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                                .0
                                .as_mut()
                            {
                                model.world_render(
                                    cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch,
                                    sin_yaw, cos_yaw, wall_x, wall_y, wall_z, typecode,
                                );
                            }
                        }
                    }

                    if let Some(tile) = tile_at_mut(&mut world.squares, level, tile_x, tile_z) {
                        tile.corner_sides = 0;
                    }
                }
            }

            // Sprite drawing: buffer this tile's sprites (farthest first),
            // render each once per cycle, and requeue the tiles they cover.
            let mut draw_sprites = tile_at(&world.squares, level, tile_x, tile_z)
                .map(|t| t.draw_sprites)
                .unwrap_or(false);
            if draw_sprites {
                let sprite_count = tile_at(&world.squares, level, tile_x, tile_z)
                    .map(|t| t.sprite_count)
                    .unwrap_or(0);
                if let Some(tile) = tile_at_mut(&mut world.squares, level, tile_x, tile_z) {
                    tile.draw_sprites = false;
                }
                let mut sprite_buffer_size = 0i32;

                'iterate_sprites: for i in 0..sprite_count as usize {
                    let sprite_index = tile_at(&world.squares, level, tile_x, tile_z)
                        .and_then(|t| t.sprites.get(i).copied())
                        .flatten();
                    let Some(sprite_index) = sprite_index else {
                        continue;
                    };
                    let Some(sprite) = world.sprites.get(sprite_index).and_then(|s| s.as_ref())
                    else {
                        continue;
                    };

                    if sprite.cycle == cycle_no {
                        continue;
                    }

                    let min_x = sprite.min_tile_x;
                    let max_x = sprite.max_tile_x;
                    let min_z = sprite.min_tile_z;
                    let max_z = sprite.max_tile_z;

                    let mut skip = false;
                    'sprite_bounds: for x in min_x..=max_x {
                        for z in min_z..=max_z {
                            let Some(other) = tile_at(&world.squares, level, x, z) else {
                                continue;
                            };

                            if other.draw_front {
                                draw_sprites = true;
                                if let Some(tile) =
                                    tile_at_mut(&mut world.squares, level, tile_x, tile_z)
                                {
                                    tile.draw_sprites = true;
                                }
                                skip = true;
                                break 'sprite_bounds;
                            }

                            if other.corner_sides == 0 {
                                continue;
                            }

                            let mut spans = 0i32;
                            if x > min_x {
                                spans += 1;
                            }
                            if x < max_x {
                                spans += 4;
                            }
                            if z > min_z {
                                spans += 8;
                            }
                            if z < max_z {
                                spans += 2;
                            }

                            if (spans & other.corner_sides) != sides_after {
                                continue;
                            }
                        }
                    }

                    if skip {
                        continue 'iterate_sprites;
                    }

                    if let Some(slot) = self.sprite_buffer.get_mut(sprite_buffer_size as usize) {
                        *slot = Some(sprite_index);
                    }
                    sprite_buffer_size += 1;

                    let mut min_tile_distance_x = gx - sprite.min_tile_x;
                    let max_tile_distance_x = sprite.max_tile_x - gx;
                    if max_tile_distance_x > min_tile_distance_x {
                        min_tile_distance_x = max_tile_distance_x;
                    }

                    let min_tile_distance_z = gz - sprite.min_tile_z;
                    let max_tile_distance_z = sprite.max_tile_z - gz;
                    if let Some(sprite) =
                        world.sprites.get_mut(sprite_index).and_then(|s| s.as_mut())
                    {
                        if max_tile_distance_z > min_tile_distance_z {
                            sprite.distance = min_tile_distance_x + max_tile_distance_z;
                        } else {
                            sprite.distance = min_tile_distance_x + min_tile_distance_z;
                        }
                    }
                }

                // TS `while (spriteBufferSize > 0)`: the size never changes
                // in the body, the loop exits when no farther sprite is
                // left, so `loop` with the same break is equivalent.
                loop {
                    let mut farthest_distance = -50i32;
                    let mut farthest_index = -1i32;

                    for index in 0..sprite_buffer_size as usize {
                        let Some(sprite) = self.sprite_buffer.get(index).copied().flatten() else {
                            continue;
                        };
                        let Some(sprite) = world.sprites.get(sprite).and_then(|s| s.as_ref())
                        else {
                            continue;
                        };

                        if sprite.distance > farthest_distance && sprite.cycle != cycle_no {
                            farthest_distance = sprite.distance;
                            farthest_index = index as i32;
                        }
                    }

                    if farthest_index == -1 {
                        break;
                    }

                    let farthest = self.sprite_buffer[farthest_index as usize].unwrap();
                    if let Some(sprite) = world.sprites.get_mut(farthest).and_then(|s| s.as_mut()) {
                        sprite.cycle = cycle_no;
                    }

                    let (min_x, max_x, min_z, max_z, yaw, typecode, sx, sy, sz) = {
                        let Some(sprite) = world.sprites.get(farthest).and_then(|s| s.as_ref())
                        else {
                            continue;
                        };
                        (
                            sprite.min_tile_x,
                            sprite.max_tile_x,
                            sprite.min_tile_z,
                            sprite.max_tile_z,
                            sprite.yaw,
                            sprite.typecode,
                            sprite.x,
                            sprite.y,
                            sprite.z,
                        )
                    };
                    let model_min_y = self
                        .sprite_model_mut(&*world, cache, loop_cycle, farthest)
                        .as_ref()
                        .map(|m| m.min_y())
                        .unwrap_or(0);

                    if !self.sprite_occluded2(
                        world,
                        original_level,
                        min_x,
                        max_x,
                        min_z,
                        max_z,
                        model_min_y,
                    ) {
                        if let Some(model) = self
                            .sprite_model_mut(&*world, cache, loop_cycle, farthest)
                            .as_mut()
                        {
                            model.world_render(
                                cache,
                                loop_cycle,
                                pix,
                                surface,
                                yaw,
                                sin_pitch,
                                cos_pitch,
                                sin_yaw,
                                cos_yaw,
                                sx - cx,
                                sy - cy,
                                sz - cz,
                                typecode,
                            );
                        }
                    }

                    for x in min_x..=max_x {
                        for z in min_z..=max_z {
                            let Some(occupied) = tile_at(&world.squares, level, x, z) else {
                                continue;
                            };
                            let corner_sides = occupied.corner_sides;
                            let draw_back = occupied.draw_back;

                            if corner_sides != 0 {
                                self.enqueue_fill(world, level, x, z);
                            } else if (x != tile_x || z != tile_z) && draw_back {
                                self.enqueue_fill(world, level, x, z);
                            }
                        }
                    }
                }

                if draw_sprites {
                    continue 'fill;
                }
            }

            // Drop the tile (unless a wall corner still occludes it).
            // Java `continue` unlinks the Square; vis-window only retries
            // `drawFront`. A neighbour that already finished RING
            // (`drawBack && !drawFront`) will not be visited again, so
            // push those onto the queue (move-to-tail) and do not push
            // self — re-pushing self spins fill() and never returns to
            // the vis-window RING walk.
            let (draw_back, corner_sides) = tile_at(&world.squares, level, tile_x, tile_z)
                .map(|t| (t.draw_back, t.corner_sides))
                .unwrap_or((false, 0));
            if !draw_back || corner_sides != 0 {
                continue 'fill;
            }

            let mut stuck = Vec::new();
            let mut blocked = false;
            if tile_x <= gx && tile_x > min_x {
                if let Some(adjacent) = tile_at(&world.squares, level, tile_x - 1, tile_z) {
                    if adjacent.draw_back {
                        blocked = true;
                        if !adjacent.draw_front {
                            stuck.push((level, tile_x - 1, tile_z));
                        }
                    }
                }
            }

            if tile_x >= gx && tile_x < max_x - 1 {
                if let Some(adjacent) = tile_at(&world.squares, level, tile_x + 1, tile_z) {
                    if adjacent.draw_back {
                        blocked = true;
                        if !adjacent.draw_front {
                            stuck.push((level, tile_x + 1, tile_z));
                        }
                    }
                }
            }

            if tile_z <= gz && tile_z > min_z {
                if let Some(adjacent) = tile_at(&world.squares, level, tile_x, tile_z - 1) {
                    if adjacent.draw_back {
                        blocked = true;
                        if !adjacent.draw_front {
                            stuck.push((level, tile_x, tile_z - 1));
                        }
                    }
                }
            }

            if tile_z >= gz && tile_z < max_z - 1 {
                if let Some(adjacent) = tile_at(&world.squares, level, tile_x, tile_z + 1) {
                    if adjacent.draw_back {
                        blocked = true;
                        if !adjacent.draw_front {
                            stuck.push((level, tile_x, tile_z + 1));
                        }
                    }
                }
            }

            if blocked {
                for (bl, bx, bz) in stuck {
                    self.enqueue_fill(world, bl, bx, bz);
                }
                continue 'fill;
            }

            if let Some(tile) = tile_at_mut(&mut world.squares, level, tile_x, tile_z) {
                tile.draw_back = false;
            }
            self.fill_left -= 1;

            // Stacked ground objects (height != 0) render once the tile
            // behind them is done.
            let ground_object_data = tile_at(&world.squares, level, tile_x, tile_z)
                .and_then(|t| t.ground_object.as_ref())
                .map(|o| (o.typecode, o.x - cx, o.y - cy, o.z - cz));
            if let Some((typecode, ox, oy, oz)) = ground_object_data {
                let height =
                    self.ground_object_height(&*world, cache, loop_cycle, level, tile_x, tile_z);
                if height != 0 {
                    let (bottom, middle, top) =
                        self.obj_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z);
                    if let Some(model) = bottom.as_mut() {
                        model.world_render(
                            cache,
                            loop_cycle,
                            pix,
                            surface,
                            0,
                            sin_pitch,
                            cos_pitch,
                            sin_yaw,
                            cos_yaw,
                            ox,
                            oy - height,
                            oz,
                            typecode,
                        );
                    }

                    if let Some(model) = middle.as_mut() {
                        model.world_render(
                            cache,
                            loop_cycle,
                            pix,
                            surface,
                            0,
                            sin_pitch,
                            cos_pitch,
                            sin_yaw,
                            cos_yaw,
                            ox,
                            oy - height,
                            oz,
                            typecode,
                        );
                    }

                    if let Some(model) = top.as_mut() {
                        model.world_render(
                            cache,
                            loop_cycle,
                            pix,
                            surface,
                            0,
                            sin_pitch,
                            cos_pitch,
                            sin_yaw,
                            cos_yaw,
                            ox,
                            oy - height,
                            oz,
                            typecode,
                        );
                    }
                }
            }

            // Back-wall decor + walls, drawn after the tile drops.
            let back_wall_types = tile_at(&world.squares, level, tile_x, tile_z)
                .map(|t| t.back_wall_types)
                .unwrap_or(0);
            if back_wall_types != 0 {
                let decor_data = tile_at(&world.squares, level, tile_x, tile_z)
                    .and_then(|t| t.decor.as_ref())
                    .map(|d| (d.wshape, d.angle, d.typecode, d.x - cx, d.y - cy, d.z - cz));
                if let Some((wshape, angle, typecode, decor_x, decor_y, decor_z)) = decor_data {
                    let min_y = self
                        .decor_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                        .as_ref()
                        .map(|m| m.min_y())
                        .unwrap_or(1000);
                    if !self.sprite_occluded(world, original_level, tile_x, tile_z, min_y) {
                        if (wshape & back_wall_types) != 0 {
                            if let Some(decor) = self
                                .decor_model_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                                .as_mut()
                            {
                                decor.world_render(
                                    cache, loop_cycle, pix, surface, angle, sin_pitch, cos_pitch,
                                    sin_yaw, cos_yaw, decor_x, decor_y, decor_z, typecode,
                                );
                            }
                        } else if (wshape & 0x300) != 0 {
                            let nearest_x = if angle == LocAngle::NORTH || angle == LocAngle::EAST {
                                -decor_x
                            } else {
                                decor_x
                            };

                            let nearest_z = if angle == LocAngle::EAST || angle == LocAngle::SOUTH {
                                -decor_z
                            } else {
                                decor_z
                            };

                            if (wshape & 0x100) != 0 && nearest_z >= nearest_x {
                                let draw_x =
                                    decor_x + DECORXOF.get(angle as usize).copied().unwrap_or(0);
                                let draw_z =
                                    decor_z + DECORZOF.get(angle as usize).copied().unwrap_or(0);
                                if let Some(decor) = self
                                    .decor_model_mut(
                                        &*world, cache, loop_cycle, level, tile_x, tile_z,
                                    )
                                    .as_mut()
                                {
                                    decor.world_render(
                                        cache,
                                        loop_cycle,
                                        pix,
                                        surface,
                                        angle * 512 + 256,
                                        sin_pitch,
                                        cos_pitch,
                                        sin_yaw,
                                        cos_yaw,
                                        draw_x,
                                        decor_y,
                                        draw_z,
                                        typecode,
                                    );
                                }
                            }

                            if (wshape & 0x200) != 0 && nearest_z <= nearest_x {
                                let draw_x =
                                    decor_x + DECORXOF2.get(angle as usize).copied().unwrap_or(0);
                                let draw_z =
                                    decor_z + DECORZOF2.get(angle as usize).copied().unwrap_or(0);
                                if let Some(decor) = self
                                    .decor_model_mut(
                                        &*world, cache, loop_cycle, level, tile_x, tile_z,
                                    )
                                    .as_mut()
                                {
                                    decor.world_render(
                                        cache,
                                        loop_cycle,
                                        pix,
                                        surface,
                                        (angle * 512 + 1280) & 0x7ff,
                                        sin_pitch,
                                        cos_pitch,
                                        sin_yaw,
                                        cos_yaw,
                                        draw_x,
                                        decor_y,
                                        draw_z,
                                        typecode,
                                    );
                                }
                            }
                        }
                    }
                }

                let wall_data = tile_at(&world.squares, level, tile_x, tile_z)
                    .and_then(|t| t.wall.as_ref())
                    .map(|w| (w.angle1, w.angle2, w.typecode, w.x - cx, w.y - cy, w.z - cz));
                if let Some((angle1, angle2, typecode, wall_x, wall_y, wall_z)) = wall_data {
                    if (angle2 & back_wall_types) != 0
                        && !self.wall_occluded(world, original_level, tile_x, tile_z, angle2)
                    {
                        if let Some(model) = self
                            .wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                            .1
                            .as_mut()
                        {
                            model.world_render(
                                cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw,
                                cos_yaw, wall_x, wall_y, wall_z, typecode,
                            );
                        }
                    }

                    if (angle1 & back_wall_types) != 0
                        && !self.wall_occluded(world, original_level, tile_x, tile_z, angle1)
                    {
                        if let Some(model) = self
                            .wall_models_mut(&*world, cache, loop_cycle, level, tile_x, tile_z)
                            .0
                            .as_mut()
                        {
                            model.world_render(
                                cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw,
                                cos_yaw, wall_x, wall_y, wall_z, typecode,
                            );
                        }
                    }
                }
            }

            // Queue the level above and the four neighbours.
            if level < world.max_tile_level - 1 {
                let above = tile_at(&world.squares, level + 1, tile_x, tile_z);
                if above.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(world, level + 1, tile_x, tile_z);
                }
            }

            if tile_x < gx {
                let adjacent = tile_at(&world.squares, level, tile_x + 1, tile_z);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(world, level, tile_x + 1, tile_z);
                }
            }

            if tile_z < gz {
                let adjacent = tile_at(&world.squares, level, tile_x, tile_z + 1);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(world, level, tile_x, tile_z + 1);
                }
            }

            if tile_x > gx {
                let adjacent = tile_at(&world.squares, level, tile_x - 1, tile_z);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(world, level, tile_x - 1, tile_z);
                }
            }

            if tile_z > gz {
                let adjacent = tile_at(&world.squares, level, tile_x, tile_z - 1);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(world, level, tile_x, tile_z - 1);
                }
            }
        }
    }

    /// `renderQuickGround(...)` from client-ts 1923-2088. The TS projection
    /// names are scrambled (the `pz0`/`px1`/`py1`/`pz1` locals hold the
    /// south-east corner's screen x/y and so on); they are kept verbatim for
    /// 1:1 review.
    #[allow(clippy::too_many_arguments)]
    fn render_quick_ground(
        &mut self,
        world: &mut World,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        ground: QuickGround,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        sin_eye_pitch: i32,
        cos_eye_pitch: i32,
        sin_eye_yaw: i32,
        cos_eye_yaw: i32,
    ) {
        let mut x0 = (tile_x << 7) - self.cx;
        let mut x1 = x0 + 128;
        let mut x2 = x1;
        let mut x3 = x0;
        let mut z0 = (tile_z << 7) - self.cz;
        let mut z1 = z0;
        let mut z2 = z0 + 128;
        let mut z3 = z2;

        let mut y0 = ground_h(world, level, tile_x, tile_z) - self.cy;
        let mut y1 = ground_h(world, level, tile_x + 1, tile_z) - self.cy;
        let mut y2 = ground_h(world, level, tile_x + 1, tile_z + 1) - self.cy;
        let mut y3 = ground_h(world, level, tile_x, tile_z + 1) - self.cy;

        let mut tmp = (z0
            .wrapping_mul(sin_eye_yaw)
            .wrapping_add(x0.wrapping_mul(cos_eye_yaw)))
            >> 16;
        z0 = (z0
            .wrapping_mul(cos_eye_yaw)
            .wrapping_sub(x0.wrapping_mul(sin_eye_yaw)))
            >> 16;
        x0 = tmp;

        tmp = (y0
            .wrapping_mul(cos_eye_pitch)
            .wrapping_sub(z0.wrapping_mul(sin_eye_pitch)))
            >> 16;
        z0 = (y0
            .wrapping_mul(sin_eye_pitch)
            .wrapping_add(z0.wrapping_mul(cos_eye_pitch)))
            >> 16;
        y0 = tmp;

        if z0 < 50 {
            return;
        }

        tmp = (z1
            .wrapping_mul(sin_eye_yaw)
            .wrapping_add(x1.wrapping_mul(cos_eye_yaw)))
            >> 16;
        z1 = (z1
            .wrapping_mul(cos_eye_yaw)
            .wrapping_sub(x1.wrapping_mul(sin_eye_yaw)))
            >> 16;
        x1 = tmp;

        tmp = (y1
            .wrapping_mul(cos_eye_pitch)
            .wrapping_sub(z1.wrapping_mul(sin_eye_pitch)))
            >> 16;
        z1 = (y1
            .wrapping_mul(sin_eye_pitch)
            .wrapping_add(z1.wrapping_mul(cos_eye_pitch)))
            >> 16;
        y1 = tmp;

        if z1 < 50 {
            return;
        }

        tmp = (z2
            .wrapping_mul(sin_eye_yaw)
            .wrapping_add(x2.wrapping_mul(cos_eye_yaw)))
            >> 16;
        z2 = (z2
            .wrapping_mul(cos_eye_yaw)
            .wrapping_sub(x2.wrapping_mul(sin_eye_yaw)))
            >> 16;
        x2 = tmp;

        tmp = (y2
            .wrapping_mul(cos_eye_pitch)
            .wrapping_sub(z2.wrapping_mul(sin_eye_pitch)))
            >> 16;
        z2 = (y2
            .wrapping_mul(sin_eye_pitch)
            .wrapping_add(z2.wrapping_mul(cos_eye_pitch)))
            >> 16;
        y2 = tmp;

        if z2 < 50 {
            return;
        }

        tmp = (z3
            .wrapping_mul(sin_eye_yaw)
            .wrapping_add(x3.wrapping_mul(cos_eye_yaw)))
            >> 16;
        z3 = (z3
            .wrapping_mul(cos_eye_yaw)
            .wrapping_sub(x3.wrapping_mul(sin_eye_yaw)))
            >> 16;
        x3 = tmp;

        tmp = (y3
            .wrapping_mul(cos_eye_pitch)
            .wrapping_sub(z3.wrapping_mul(sin_eye_pitch)))
            >> 16;
        z3 = (y3
            .wrapping_mul(sin_eye_pitch)
            .wrapping_add(z3.wrapping_mul(cos_eye_pitch)))
            >> 16;
        y3 = tmp;

        if z3 < 50 {
            return;
        }

        let px0 = pix.origin_x + x0.wrapping_shl(9).checked_div(z0).unwrap_or(0);
        let py0 = pix.origin_y + y0.wrapping_shl(9).checked_div(z0).unwrap_or(0);
        let pz0 = pix.origin_x + x1.wrapping_shl(9).checked_div(z1).unwrap_or(0);
        let px1 = pix.origin_y + y1.wrapping_shl(9).checked_div(z1).unwrap_or(0);
        let py1 = pix.origin_x + x2.wrapping_shl(9).checked_div(z2).unwrap_or(0);
        let pz1 = pix.origin_y + y2.wrapping_shl(9).checked_div(z2).unwrap_or(0);
        let px3 = pix.origin_x + x3.wrapping_shl(9).checked_div(z3).unwrap_or(0);
        let py3 = pix.origin_y + y3.wrapping_shl(9).checked_div(z3).unwrap_or(0);

        pix.trans = 0;

        if wrapping_cross(
            py1.wrapping_sub(px3),
            px1.wrapping_sub(py3),
            pz1.wrapping_sub(py3),
            pz0.wrapping_sub(px3),
        ) > 0
        {
            pix.hclip = py1 < 0
                || px3 < 0
                || pz0 < 0
                || py1 > surface.size_x
                || px3 > surface.size_x
                || pz0 > surface.size_x;

            if world.click
                && inside_triangle(world.click_x, world.click_y, pz1, py3, px1, py1, px3, pz0)
            {
                world.ground_x = tile_x;
                world.ground_z = tile_z;
            }

            if ground.texture != -1 {
                if !pix.low_mem {
                    if ground.flat {
                        pix.texture_triangle(
                            surface,
                            py1,
                            px3,
                            pz0,
                            pz1,
                            py3,
                            px1,
                            ground.colour_ne,
                            ground.colour_nw,
                            ground.colour_se,
                            x0,
                            y0,
                            z0,
                            x1,
                            x3,
                            y1,
                            y3,
                            z1,
                            z3,
                            ground.texture,
                        );
                    } else {
                        pix.texture_triangle(
                            surface,
                            py1,
                            px3,
                            pz0,
                            pz1,
                            py3,
                            px1,
                            ground.colour_ne,
                            ground.colour_nw,
                            ground.colour_se,
                            x2,
                            y2,
                            z2,
                            x3,
                            x1,
                            y3,
                            y1,
                            z3,
                            z1,
                            ground.texture,
                        );
                    }
                } else {
                    let texture_average = TEXTURE_AVERAGE
                        .get(ground.texture as usize)
                        .copied()
                        .unwrap_or(41);
                    pix.gouraud_triangle(
                        surface,
                        py1,
                        px3,
                        pz0,
                        pz1,
                        py3,
                        px1,
                        get_table(texture_average, ground.colour_ne),
                        get_table(texture_average, ground.colour_nw),
                        get_table(texture_average, ground.colour_se),
                    );
                }
            } else if ground.colour_ne != 12345678 {
                pix.gouraud_triangle(
                    surface,
                    py1,
                    px3,
                    pz0,
                    pz1,
                    py3,
                    px1,
                    ground.colour_ne,
                    ground.colour_nw,
                    ground.colour_se,
                );
            }
        }

        if wrapping_cross(
            px0.wrapping_sub(pz0),
            py3.wrapping_sub(px1),
            py0.wrapping_sub(px1),
            px3.wrapping_sub(pz0),
        ) > 0
        {
            pix.hclip = px0 < 0
                || pz0 < 0
                || px3 < 0
                || px0 > surface.size_x
                || pz0 > surface.size_x
                || px3 > surface.size_x;

            if world.click
                && inside_triangle(world.click_x, world.click_y, py0, px1, py3, px0, pz0, px3)
            {
                world.ground_x = tile_x;
                world.ground_z = tile_z;
            }

            if ground.texture != -1 {
                if !pix.low_mem {
                    pix.texture_triangle(
                        surface,
                        px0,
                        pz0,
                        px3,
                        py0,
                        px1,
                        py3,
                        ground.colour_sw,
                        ground.colour_se,
                        ground.colour_nw,
                        x0,
                        y0,
                        z0,
                        x1,
                        x3,
                        y1,
                        y3,
                        z1,
                        z3,
                        ground.texture,
                    );
                } else {
                    let texture_average = TEXTURE_AVERAGE
                        .get(ground.texture as usize)
                        .copied()
                        .unwrap_or(41);
                    pix.gouraud_triangle(
                        surface,
                        px0,
                        pz0,
                        px3,
                        py0,
                        px1,
                        py3,
                        get_table(texture_average, ground.colour_sw),
                        get_table(texture_average, ground.colour_se),
                        get_table(texture_average, ground.colour_nw),
                    );
                }
            } else if ground.colour_sw != 12345678 {
                pix.gouraud_triangle(
                    surface,
                    px0,
                    pz0,
                    px3,
                    py0,
                    px1,
                    py3,
                    ground.colour_sw,
                    ground.colour_se,
                    ground.colour_nw,
                );
            }
        }
    }

    /// `renderGround(tileX, tileZ, ground, ...)` from client-ts 2089-2188.
    /// The `Ground.drawVertex*` TS statics are the `ground_draw_*` fields.
    #[allow(clippy::too_many_arguments)]
    fn render_ground(
        &mut self,
        world: &mut World,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        tile_x: i32,
        tile_z: i32,
        ground: Ground,
        sin_eye_pitch: i32,
        cos_eye_pitch: i32,
        sin_eye_yaw: i32,
        cos_eye_yaw: i32,
    ) {
        let mut vertex_count = ground.vertices();

        for i in 0..vertex_count {
            let mut x = ground.vertex_x[i] - self.cx;
            let mut y = ground.vertex_y[i] - self.cy;
            let mut z = ground.vertex_z[i] - self.cz;

            let mut tmp = (z
                .wrapping_mul(sin_eye_yaw)
                .wrapping_add(x.wrapping_mul(cos_eye_yaw)))
                >> 16;
            z = (z
                .wrapping_mul(cos_eye_yaw)
                .wrapping_sub(x.wrapping_mul(sin_eye_yaw)))
                >> 16;
            x = tmp;

            tmp = (y
                .wrapping_mul(cos_eye_pitch)
                .wrapping_sub(z.wrapping_mul(sin_eye_pitch)))
                >> 16;
            z = (y
                .wrapping_mul(sin_eye_pitch)
                .wrapping_add(z.wrapping_mul(cos_eye_pitch)))
                >> 16;
            y = tmp;

            if z < 50 {
                return;
            }

            if ground.face_texture.is_some() {
                if let Some(slot) = self.ground_draw_texture_vertex_x.get_mut(i) {
                    *slot = x;
                }
                if let Some(slot) = self.ground_draw_texture_vertex_y.get_mut(i) {
                    *slot = y;
                }
                if let Some(slot) = self.ground_draw_texture_vertex_z.get_mut(i) {
                    *slot = z;
                }
            }

            if let Some(slot) = self.ground_draw_vertex_x.get_mut(i) {
                *slot = pix.origin_x + x.wrapping_shl(9).checked_div(z).unwrap_or(0);
            }
            if let Some(slot) = self.ground_draw_vertex_y.get_mut(i) {
                *slot = pix.origin_y + y.wrapping_shl(9).checked_div(z).unwrap_or(0);
            }
        }

        pix.trans = 0;

        vertex_count = ground.faces();
        for v in 0..vertex_count {
            let a = ground.face_vertex_a[v] as usize;
            let b = ground.face_vertex_b[v] as usize;
            let c = ground.face_vertex_c[v] as usize;

            let (Some(&x0), Some(&x1), Some(&x2)) = (
                self.ground_draw_vertex_x.get(a),
                self.ground_draw_vertex_x.get(b),
                self.ground_draw_vertex_x.get(c),
            ) else {
                continue;
            };
            let (Some(&y0), Some(&y1), Some(&y2)) = (
                self.ground_draw_vertex_y.get(a),
                self.ground_draw_vertex_y.get(b),
                self.ground_draw_vertex_y.get(c),
            ) else {
                continue;
            };

            if wrapping_cross(
                x0.wrapping_sub(x1),
                y2.wrapping_sub(y1),
                y0.wrapping_sub(y1),
                x2.wrapping_sub(x1),
            ) > 0
            {
                pix.hclip = x0 < 0
                    || x1 < 0
                    || x2 < 0
                    || x0 > surface.size_x
                    || x1 > surface.size_x
                    || x2 > surface.size_x;

                if world.click
                    && inside_triangle(world.click_x, world.click_y, y0, y1, y2, x0, x1, x2)
                {
                    world.ground_x = tile_x;
                    world.ground_z = tile_z;
                }

                let face_texture = ground.face_texture.as_ref().and_then(|t| t.get(v)).copied();
                let textured = match face_texture {
                    Some(t) if t != -1 => Some(t),
                    _ => None,
                };
                let colour_a = ground.face_colour_a.get(v).copied().unwrap_or(0);
                let colour_b = ground.face_colour_b.get(v).copied().unwrap_or(0);
                let colour_c = ground.face_colour_c.get(v).copied().unwrap_or(0);

                if let Some(tex) = textured {
                    if !pix.low_mem {
                        if ground.flat {
                            pix.texture_triangle(
                                surface,
                                x0,
                                x1,
                                x2,
                                y0,
                                y1,
                                y2,
                                colour_a,
                                colour_b,
                                colour_c,
                                self.ground_draw_texture_vertex_x[0],
                                self.ground_draw_texture_vertex_y[0],
                                self.ground_draw_texture_vertex_z[0],
                                self.ground_draw_texture_vertex_x[1],
                                self.ground_draw_texture_vertex_x[3],
                                self.ground_draw_texture_vertex_y[1],
                                self.ground_draw_texture_vertex_y[3],
                                self.ground_draw_texture_vertex_z[1],
                                self.ground_draw_texture_vertex_z[3],
                                tex,
                            );
                        } else {
                            pix.texture_triangle(
                                surface,
                                x0,
                                x1,
                                x2,
                                y0,
                                y1,
                                y2,
                                colour_a,
                                colour_b,
                                colour_c,
                                self.ground_draw_texture_vertex_x[a],
                                self.ground_draw_texture_vertex_y[a],
                                self.ground_draw_texture_vertex_z[a],
                                self.ground_draw_texture_vertex_x[b],
                                self.ground_draw_texture_vertex_x[c],
                                self.ground_draw_texture_vertex_y[b],
                                self.ground_draw_texture_vertex_y[c],
                                self.ground_draw_texture_vertex_z[b],
                                self.ground_draw_texture_vertex_z[c],
                                tex,
                            );
                        }
                    } else {
                        let texture_average =
                            TEXTURE_AVERAGE.get(tex as usize).copied().unwrap_or(41);
                        pix.gouraud_triangle(
                            surface,
                            x0,
                            x1,
                            x2,
                            y0,
                            y1,
                            y2,
                            get_table(texture_average, colour_a),
                            get_table(texture_average, colour_b),
                            get_table(texture_average, colour_c),
                        );
                    }
                } else if colour_a != 12345678 {
                    pix.gouraud_triangle(
                        surface, x0, x1, x2, y0, y1, y2, colour_a, colour_b, colour_c,
                    );
                }
            }
        }
    }

    /// `groundOccluded(level, x, z)` from client-ts 2189-2215.
    fn ground_occluded(&mut self, world: &mut World, level: i32, x: i32, z: i32) -> bool {
        let stride_z = world.max_tile_z + 1;
        let stride_x = world.max_tile_x + 1;
        let index = (level * stride_x + x) * stride_z + z;
        let cycle = world
            .occlusion_cycle
            .get(index as usize)
            .copied()
            .unwrap_or(0);
        if cycle == -self.cycle_no {
            return false;
        } else if cycle == self.cycle_no {
            return true;
        } else {
            let sx = x << 7;
            let sz = z << 7;
            if self.occluded(world, sx + 1, ground_h(world, level, x, z), sz + 1)
                && self.occluded(
                    world,
                    sx + 128 - 1,
                    ground_h(world, level, x + 1, z),
                    sz + 1,
                )
                && self.occluded(
                    world,
                    sx + 128 - 1,
                    ground_h(world, level, x + 1, z + 1),
                    sz + 128 - 1,
                )
                && self.occluded(
                    world,
                    sx + 1,
                    ground_h(world, level, x, z + 1),
                    sz + 128 - 1,
                )
            {
                if let Some(slot) = world.occlusion_cycle.get_mut(index as usize) {
                    *slot = self.cycle_no;
                }
                return true;
            } else {
                if let Some(slot) = world.occlusion_cycle.get_mut(index as usize) {
                    *slot = -self.cycle_no;
                }
                return false;
            }
        }
    }

    /// `wallOccluded(level, x, z, type)` from client-ts 2216-2310.
    fn wall_occluded(
        &mut self,
        world: &mut World,
        level: i32,
        x: i32,
        z: i32,
        r#type: i32,
    ) -> bool {
        if !self.ground_occluded(world, level, x, z) {
            return false;
        }

        let scene_x = x << 7;
        let scene_z = z << 7;
        let scene_y = ground_h(world, level, x, z) - 1;
        let y0 = scene_y - 120;
        let y1 = scene_y - 230;
        let y2 = scene_y - 238;
        if r#type < 16 {
            if r#type == 1 {
                if scene_x > self.cx {
                    if !self.occluded(world, scene_x, scene_y, scene_z) {
                        return false;
                    }
                    if !self.occluded(world, scene_x, scene_y, scene_z + 128) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(world, scene_x, y0, scene_z) {
                        return false;
                    }
                    if !self.occluded(world, scene_x, y0, scene_z + 128) {
                        return false;
                    }
                }
                if !self.occluded(world, scene_x, y1, scene_z) {
                    return false;
                }
                return self.occluded(world, scene_x, y1, scene_z + 128);
            }
            if r#type == 2 {
                if scene_z < self.cz {
                    if !self.occluded(world, scene_x, scene_y, scene_z + 128) {
                        return false;
                    }
                    if !self.occluded(world, scene_x + 128, scene_y, scene_z + 128) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(world, scene_x, y0, scene_z + 128) {
                        return false;
                    }
                    if !self.occluded(world, scene_x + 128, y0, scene_z + 128) {
                        return false;
                    }
                }
                if !self.occluded(world, scene_x, y1, scene_z + 128) {
                    return false;
                }
                return self.occluded(world, scene_x + 128, y1, scene_z + 128);
            }
            if r#type == 4 {
                if scene_x < self.cx {
                    if !self.occluded(world, scene_x + 128, scene_y, scene_z) {
                        return false;
                    }
                    if !self.occluded(world, scene_x + 128, scene_y, scene_z + 128) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(world, scene_x + 128, y0, scene_z) {
                        return false;
                    }
                    if !self.occluded(world, scene_x + 128, y0, scene_z + 128) {
                        return false;
                    }
                }
                if !self.occluded(world, scene_x + 128, y1, scene_z) {
                    return false;
                }
                return self.occluded(world, scene_x + 128, y1, scene_z + 128);
            }
            if r#type == 8 {
                if scene_z > self.cz {
                    if !self.occluded(world, scene_x, scene_y, scene_z) {
                        return false;
                    }
                    if !self.occluded(world, scene_x + 128, scene_y, scene_z) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(world, scene_x, y0, scene_z) {
                        return false;
                    }
                    if !self.occluded(world, scene_x + 128, y0, scene_z) {
                        return false;
                    }
                }
                if !self.occluded(world, scene_x, y1, scene_z) {
                    return false;
                }
                return self.occluded(world, scene_x + 128, y1, scene_z);
            }
        }

        if !self.occluded(world, scene_x + 64, y2, scene_z + 64) {
            return false;
        } else if r#type == 16 {
            return self.occluded(world, scene_x, y1, scene_z + 128);
        } else if r#type == 32 {
            return self.occluded(world, scene_x + 128, y1, scene_z + 128);
        } else if r#type == 64 {
            return self.occluded(world, scene_x + 128, y1, scene_z);
        } else if r#type == 128 {
            return self.occluded(world, scene_x, y1, scene_z);
        }

        // TS `console.warn('Warning unsupported wall type')`, then true.
        true
    }

    /// `spriteOccluded(level, tileX, tileZ, y)` from client-ts 2311-2328.
    fn sprite_occluded(
        &mut self,
        world: &mut World,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        y: i32,
    ) -> bool {
        if self.ground_occluded(world, level, tile_x, tile_z) {
            let x = tile_x << 7;
            let z = tile_z << 7;
            return self.occluded(
                world,
                x + 1,
                ground_h(world, level, tile_x, tile_z) - y,
                z + 1,
            ) && self.occluded(
                world,
                x + 128 - 1,
                ground_h(world, level, tile_x + 1, tile_z) - y,
                z + 1,
            ) && self.occluded(
                world,
                x + 128 - 1,
                ground_h(world, level, tile_x + 1, tile_z + 1) - y,
                z + 128 - 1,
            ) && self.occluded(
                world,
                x + 1,
                ground_h(world, level, tile_x, tile_z + 1) - y,
                z + 128 - 1,
            );
        }
        false
    }

    /// `spriteOccluded2(level, minX, maxX, minZ, maxZ, y)` from client-ts
    /// 2329-2373. The TS `z` local holds the min-x scene coordinate; kept
    /// verbatim.
    fn sprite_occluded2(
        &mut self,
        world: &mut World,
        level: i32,
        min_x: i32,
        max_x: i32,
        min_z: i32,
        max_z: i32,
        y: i32,
    ) -> bool {
        let x: i32;
        let z: i32;
        if min_x != max_x || min_z != max_z {
            for x0 in min_x..=max_x {
                for z0 in min_z..=max_z {
                    if self.occ_cycle(world, level, x0, z0) == -self.cycle_no {
                        return false;
                    }
                }
            }

            z = (min_x << 7) + 1;
            let z0 = (min_z << 7) + 2;
            let y0 = ground_h(world, level, min_x, min_z) - y;
            if !self.occluded(world, z, y0, z0) {
                return false;
            }

            let x1 = (max_x << 7) - 1;
            if !self.occluded(world, x1, y0, z0) {
                return false;
            }

            let z1 = (max_z << 7) - 1;
            if !self.occluded(world, z, y0, z1) {
                return false;
            } else if self.occluded(world, x1, y0, z1) {
                return true;
            } else {
                return false;
            }
        } else if self.ground_occluded(world, level, min_x, min_z) {
            x = min_x << 7;
            z = min_z << 7;
            return self.occluded(
                world,
                x + 1,
                ground_h(world, level, min_x, min_z) - y,
                z + 1,
            ) && self.occluded(
                world,
                x + 128 - 1,
                ground_h(world, level, min_x + 1, min_z) - y,
                z + 1,
            ) && self.occluded(
                world,
                x + 128 - 1,
                ground_h(world, level, min_x + 1, min_z + 1) - y,
                z + 128 - 1,
            ) && self.occluded(
                world,
                x + 1,
                ground_h(world, level, min_x, min_z + 1) - y,
                z + 128 - 1,
            );
        }
        false
    }

    /// `occlusionCycle[level][x][z]` read (guarded; TS typed-array OOB is 0).
    fn occ_cycle(&self, world: &World, level: i32, x: i32, z: i32) -> i32 {
        let stride_z = world.max_tile_z + 1;
        let stride_x = world.max_tile_x + 1;
        let index = (level * stride_x + x) * stride_z + z;
        world
            .occlusion_cycle
            .get(index as usize)
            .copied()
            .unwrap_or(0)
    }

    /// `occluded(x, y, z)` from client-ts 2374-2442: test a point against
    /// every active occluder frustum.
    fn occluded(&self, world: &World, x: i32, y: i32, z: i32) -> bool {
        for i in 0..self.num_active_occluders as usize {
            let Some(occluder_index) = self.active_occluders.get(i).copied().flatten() else {
                continue;
            };
            let Some(occluder) = world.occluders.get(occluder_index).and_then(|o| o.as_ref())
            else {
                continue;
            };

            if occluder.mode == 1 {
                let dx = occluder.min_x - x;
                if dx > 0 {
                    let min_z = occluder.min_z + ((occluder.min_delta_z.wrapping_mul(dx)) >> 8);
                    let max_z = occluder.max_z + ((occluder.max_delta_z.wrapping_mul(dx)) >> 8);
                    let min_y = occluder.min_y + ((occluder.min_delta_y.wrapping_mul(dx)) >> 8);
                    let max_y = occluder.max_y + ((occluder.max_delta_y.wrapping_mul(dx)) >> 8);
                    if z >= min_z && z <= max_z && y >= min_y && y <= max_y {
                        return true;
                    }
                }
            } else if occluder.mode == 2 {
                let dx = x - occluder.min_x;
                if dx > 0 {
                    let min_z = occluder.min_z + ((occluder.min_delta_z.wrapping_mul(dx)) >> 8);
                    let max_z = occluder.max_z + ((occluder.max_delta_z.wrapping_mul(dx)) >> 8);
                    let min_y = occluder.min_y + ((occluder.min_delta_y.wrapping_mul(dx)) >> 8);
                    let max_y = occluder.max_y + ((occluder.max_delta_y.wrapping_mul(dx)) >> 8);
                    if z >= min_z && z <= max_z && y >= min_y && y <= max_y {
                        return true;
                    }
                }
            } else if occluder.mode == 3 {
                let dz = occluder.min_z - z;
                if dz > 0 {
                    let min_x = occluder.min_x + ((occluder.min_delta_x.wrapping_mul(dz)) >> 8);
                    let max_x = occluder.max_x + ((occluder.max_delta_x.wrapping_mul(dz)) >> 8);
                    let min_y = occluder.min_y + ((occluder.min_delta_y.wrapping_mul(dz)) >> 8);
                    let max_y = occluder.max_y + ((occluder.max_delta_y.wrapping_mul(dz)) >> 8);
                    if x >= min_x && x <= max_x && y >= min_y && y <= max_y {
                        return true;
                    }
                }
            } else if occluder.mode == 4 {
                let dz = z - occluder.min_z;
                if dz > 0 {
                    let min_x = occluder.min_x + ((occluder.min_delta_x.wrapping_mul(dz)) >> 8);
                    let max_x = occluder.max_x + ((occluder.max_delta_x.wrapping_mul(dz)) >> 8);
                    let min_y = occluder.min_y + ((occluder.min_delta_y.wrapping_mul(dz)) >> 8);
                    let max_y = occluder.max_y + ((occluder.max_delta_y.wrapping_mul(dz)) >> 8);
                    if x >= min_x && x <= max_x && y >= min_y && y <= max_y {
                        return true;
                    }
                }
            } else if occluder.mode == 5 {
                let dy = y - occluder.min_y;
                if dy > 0 {
                    let min_x = occluder.min_x + ((occluder.min_delta_x.wrapping_mul(dy)) >> 8);
                    let max_x = occluder.max_x + ((occluder.max_delta_x.wrapping_mul(dy)) >> 8);
                    let min_z = occluder.min_z + ((occluder.min_delta_z.wrapping_mul(dy)) >> 8);
                    let max_z = occluder.max_z + ((occluder.max_delta_z.wrapping_mul(dy)) >> 8);
                    if x >= min_x && x <= max_x && z >= min_z && z <= max_z {
                        return true;
                    }
                }
            }
        }
        false
    }
}

impl Default for RenderWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// `World.visBackingDirty[x][z]` read (TS `null` reads false everywhere).
fn vis_backing_at(world: &RenderWorld, dx: i32, dz: i32) -> bool {
    let Some(row) = world.vis_backing_dirty else {
        return false;
    };
    let index = row + dx as usize * 51 + dz as usize;
    world.vis_backing.get(index).copied().unwrap_or(false)
}

/// `World.insideTriangle(...)` from client-ts 2443-2461. i64 cross products
/// (screen coordinates can reach ±335k, so the products overflow i32).
#[allow(clippy::too_many_arguments)]
fn inside_triangle(x: i32, y: i32, y0: i32, y1: i32, y2: i32, x0: i32, x1: i32, x2: i32) -> bool {
    if y < y0 && y < y1 && y < y2 {
        return false;
    } else if y > y0 && y > y1 && y > y2 {
        return false;
    } else if x < x0 && x < x1 && x < x2 {
        return false;
    } else if x > x0 && x > x1 && x > x2 {
        return false;
    }

    let cross_product_01 = (y as i64 - y0 as i64) * (x1 as i64 - x0 as i64)
        - (x as i64 - x0 as i64) * (y1 as i64 - y0 as i64);
    let cross_product_20 = (y as i64 - y2 as i64) * (x0 as i64 - x2 as i64)
        - (x as i64 - x2 as i64) * (y0 as i64 - y2 as i64);
    let cross_product_12 = (y as i64 - y1 as i64) * (x2 as i64 - x1 as i64)
        - (x as i64 - x1 as i64) * (y2 as i64 - y1 as i64);
    cross_product_01 * cross_product_12 > 0 && cross_product_12 * cross_product_20 > 0
}

/// `World.getTable(hsl, lightness)` from client-ts 2462-2474.
fn get_table(hsl: i32, lightness: i32) -> i32 {
    let inv_lightness = 127 - lightness;
    let mut lightness = (inv_lightness * (hsl & 0x7f)) / 160;
    if lightness < 2 {
        lightness = 2;
    } else if lightness > 126 {
        lightness = 126;
    }
    (hsl & 0xff80) + lightness
}

/// The per-frame camera the GPU mesh builder projects with (the
/// `prepare_scene` fixed-point trig + eye).
#[derive(Clone, Copy)]
struct SceneCam {
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    sin_pitch: i32,
    cos_pitch: i32,
    sin_yaw: i32,
    cos_yaw: i32,
}

/// One triangle vertex of the GPU scene mesh: a camera-space position plus
/// the packed RuneLite-GPU-plugin attributes — `abhsl` (alpha << 24 | bias
/// << 16 | the raw 16-bit face shade) and, for textured faces, the
/// fixed-point 0..255 texture `u`/`v` and the `texture id + 1` (`0` means
/// flat, i.e. untextured). `bytemuck::Pod` so the wgpu backend uploads the
/// mesh as raw bytes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuVertex {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub abhsl: u32,
    pub uv_tex: u32,
    pub v: u32,
}

impl GpuVertex {
    /// Pack `alpha` (0..255, 255 = opaque), the face priority `bias`
    /// (0..255) and the raw 16-bit `shade` into one word, exactly the
    /// RuneLite `alphaBias | color` layout.
    fn pack(alpha: u8, bias: u32, shade: i32) -> u32 {
        ((alpha as u32) << 24) | ((bias & 0xff) << 16) | (shade as u32 & 0xffff)
    }

    /// A flat (untextured) camera-space vertex. `shade` is the raw 16-bit
    /// colour-table index — the shader converts it via `hslToRgb`.
    fn new(x: i32, y: i32, z: i32, shade: i32, alpha: u8, bias: u32) -> GpuVertex {
        GpuVertex {
            x: x as f32,
            y: y as f32,
            z: z as f32,
            abhsl: Self::pack(alpha, bias, shade),
            uv_tex: 0,
            v: 0,
        }
    }

    /// A textured-face vertex: fixed-point `u`/`v` (0..255) on the model
    /// texture, the raw 16-bit `shade` for the texel brightness, and
    /// `tex_id_plus_1` (texture id + 1; `0` would read as flat).
    fn textured(
        x: i32,
        y: i32,
        z: i32,
        u: u32,
        v: u32,
        shade: i32,
        alpha: u8,
        bias: u32,
        tex_id_plus_1: u32,
    ) -> GpuVertex {
        GpuVertex {
            x: x as f32,
            y: y as f32,
            z: z as f32,
            abhsl: Self::pack(alpha, bias, shade),
            uv_tex: ((u & 0xffff) << 16) | (tex_id_plus_1 & 0xffff),
            v: v & 0xffff,
        }
    }
}

/// The GPU scene mesh: scenery/ground opaque first, then walls, then
/// translucent. Walls draw after scenery so a same-tile booth that
/// occupies the wall's thickness loses `LessEqual` to the wall facade
/// (the CPU's back-wall pass after sprites).
#[derive(Default, Clone)]
pub struct SceneMesh {
    opaque: Vec<GpuVertex>,
    walls: Vec<GpuVertex>,
    translucent: Vec<GpuVertex>,
}

impl SceneMesh {
    /// The mesh as one opaque-first vertex list (scenery, then walls,
    /// then translucent).
    pub fn vertices(self) -> Vec<GpuVertex> {
        let mut all = self.opaque;
        all.extend(self.walls);
        all.extend(self.translucent);
        all
    }

    /// Sort each opaque group far-first (descending minimum camera-space
    /// z). Walls stay a separate group so they are still written after
    /// scenery; `LessEqual` then lets a coplanar wall overwrite a booth.
    pub fn sort_opaque_far_first(&mut self) {
        fn sort_group(verts: &mut Vec<GpuVertex>) {
            let mut tris: Vec<(f32, [GpuVertex; 3])> = verts
                .chunks_exact(3)
                .map(|c| (c[0].z.min(c[1].z).min(c[2].z), [c[0], c[1], c[2]]))
                .collect();
            tris.sort_by(|a, b| b.0.total_cmp(&a.0));
            *verts = tris.into_iter().flat_map(|(_, t)| t).collect();
        }
        sort_group(&mut self.opaque);
        sort_group(&mut self.walls);
    }

    /// Vertex count of the opaque prefix (`vertices()[..n]`), including
    /// the wall bucket.
    pub fn opaque_len(&self) -> usize {
        self.opaque.len() + self.walls.len()
    }

    fn push(&mut self, v0: GpuVertex, v1: GpuVertex, v2: GpuVertex, translucent: bool, wall: bool) {
        if translucent {
            self.translucent.extend([v0, v1, v2]);
        } else if wall {
            self.walls.extend([v0, v1, v2]);
        } else {
            self.opaque.extend([v0, v1, v2]);
        }
    }
}

/// RuneLite `computeFaceUvs` (model space): project the face's three actual
/// vertices (`a`/`b`/`c`) onto the texture triangle's plane (`t_a`/`t_b`/`t_c`)
/// and return each vertex's texture coordinate as fixed-point 0..255.
/// `point_x/y/z` are the model's local vertex positions, before any camera
/// or entity transform.
#[allow(clippy::too_many_arguments)]
fn compute_face_uvs(
    point_x: &[i32],
    point_y: &[i32],
    point_z: &[i32],
    a: usize,
    b: usize,
    c: usize,
    t_a: usize,
    t_b: usize,
    t_c: usize,
) -> ([u32; 3], [u32; 3]) {
    let v1x = point_x[t_a] as f32;
    let v1y = point_y[t_a] as f32;
    let v1z = point_z[t_a] as f32;
    let v2x = point_x[t_b] as f32 - v1x;
    let v2y = point_y[t_b] as f32 - v1y;
    let v2z = point_z[t_b] as f32 - v1z;
    let v3x = point_x[t_c] as f32 - v1x;
    let v3y = point_y[t_c] as f32 - v1y;
    let v3z = point_z[t_c] as f32 - v1z;

    let v4x = point_x[a] as f32 - v1x;
    let v4y = point_y[a] as f32 - v1y;
    let v4z = point_z[a] as f32 - v1z;
    let v5x = point_x[b] as f32 - v1x;
    let v5y = point_y[b] as f32 - v1y;
    let v5z = point_z[b] as f32 - v1z;
    let v6x = point_x[c] as f32 - v1x;
    let v6y = point_y[c] as f32 - v1y;
    let v6z = point_z[c] as f32 - v1z;

    let v7x = v2y * v3z - v2z * v3y;
    let v7y = v2z * v3x - v2x * v3z;
    let v7z = v2x * v3y - v2y * v3x;

    let mut v8x = v3y * v7z - v3z * v7y;
    let mut v8y = v3z * v7x - v3x * v7z;
    let mut v8z = v3x * v7y - v3y * v7x;

    let f = 1.0 / (v8x * v2x + v8y * v2y + v8z * v2z);
    let u0 = (v8x * v4x + v8y * v4y + v8z * v4z) * f;
    let u1 = (v8x * v5x + v8y * v5y + v8z * v5z) * f;
    let u2 = (v8x * v6x + v8y * v6y + v8z * v6z) * f;

    v8x = v2y * v7z - v2z * v7y;
    v8y = v2z * v7x - v2x * v7z;
    v8z = v2x * v7y - v2y * v7x;

    let f = 1.0 / (v8x * v3x + v8y * v3y + v8z * v3z);
    let v0 = (v8x * v4x + v8y * v4y + v8z * v4z) * f;
    let v1 = (v8x * v5x + v8y * v5y + v8z * v5z) * f;
    let v2 = (v8x * v6x + v8y * v6y + v8z * v6z) * f;

    // RuneLite `computeFaceUvs` stores `(int)(u * 256)` unclamped; the
    // sampler ClampToEdge handles out-of-range, matching `vert.glsl`.
    let pack = |x: f32| (x * 256.0) as i32 as u32;
    (
        [pack(u0), pack(u1), pack(u2)],
        [pack(v0), pack(v1), pack(v2)],
    )
}

/// One camera-space face vertex plus the per-vertex attributes that must be
/// interpolated when a face is clipped against the near plane (z = 50).
#[derive(Clone, Copy)]
struct ClipVertex {
    x: i32,
    y: i32,
    z: i32,
    shade: i32,
    u: u32,
    v: u32,
}

/// Clip a triangle against the near plane `z = 50` exactly like the CPU's
/// `render3_z_clip`: process A, then B, then C; for each vertex keep it when
/// in front, otherwise push the intersections with the *other* in-front
/// vertices (for A: C then B; for B: A then C; for C: B then A). The output
/// order matters — the winding test and the triangle fan consume it — so a
/// Sutherland–Hodgman traversal with a different order would flip the
/// winding and intermittently drop the face as the camera pans.
fn clip_near_plane(verts: [ClipVertex; 3]) -> Vec<ClipVertex> {
    let [a, b, c] = verts;
    let mut out = Vec::with_capacity(4);
    if a.z >= 50 {
        out.push(a);
    } else {
        if c.z >= 50 {
            out.push(clip_near_intersection(c, a));
        }
        if b.z >= 50 {
            out.push(clip_near_intersection(b, a));
        }
    }
    if b.z >= 50 {
        out.push(b);
    } else {
        if a.z >= 50 {
            out.push(clip_near_intersection(a, b));
        }
        if c.z >= 50 {
            out.push(clip_near_intersection(c, b));
        }
    }
    if c.z >= 50 {
        out.push(c);
    } else {
        if b.z >= 50 {
            out.push(clip_near_intersection(b, c));
        }
        if a.z >= 50 {
            out.push(clip_near_intersection(a, c));
        }
    }
    out
}

/// The point where the `outside → inside` edge crosses `z = 50`, with all
/// interpolated attributes. `t = (50 - zOutside) * 65536 / (zInside - zOutside)`,
/// then each value is `outside + (inside - outside) * t >> 16`.
fn clip_near_intersection(inside: ClipVertex, outside: ClipVertex) -> ClipVertex {
    let dz = (inside.z - outside.z).max(1) as i64;
    let t = (50 - outside.z) as i64 * 65536 / dz;
    let lerp = |a: i32, b: i32| -> i32 {
        let a = a as i64;
        let b = b as i64;
        (a + ((b - a) * t >> 16)) as i32
    };
    ClipVertex {
        x: lerp(outside.x, inside.x),
        y: lerp(outside.y, inside.y),
        z: 50,
        shade: lerp(outside.shade, inside.shade),
        u: lerp(outside.u as i32, inside.u as i32) as u32,
        v: lerp(outside.v as i32, inside.v as i32) as u32,
    }
}

#[cfg(test)]
mod near_plane_tests {
    use super::{clip_near_intersection, clip_near_plane, ClipVertex};

    fn v(x: i32, y: i32, z: i32) -> ClipVertex {
        ClipVertex {
            x,
            y,
            z,
            shade: 0,
            u: 0,
            v: 0,
        }
    }

    /// A triangle with one vertex behind the near plane: the clipped output
    /// must match `render3_z_clip`'s A/B/C order — the winding test and the
    /// triangle fan consume it, so the order is load-bearing.
    #[test]
    fn a_behind_clips_in_abc_order() {
        // A behind, B and C in front.
        let clipped = clip_near_plane([v(0, 0, 10), v(10, 0, 100), v(0, 10, 100)]);
        assert_eq!(clipped.len(), 4, "one-behind clips to a quad");
        // render3_z_clip order: clip(A→C), clip(A→B), B, C.
        assert_eq!(clipped[0].z, 50);
        assert_eq!(clipped[1].z, 50);
        assert_eq!(clipped[2].z, 100);
        assert_eq!(clipped[3].z, 100);
        // The two intersections are distinct: on edge A→C and edge A→B.
        let (i_ac, i_ab) = (clipped[0], clipped[1]);
        assert!(
            i_ab.x > 0 && i_ab.y == 0,
            "A→B intersection lies on y=0, x>0"
        );
        assert!(
            i_ac.x == 0 && i_ac.y > 0,
            "A→C intersection lies on x=0, y>0"
        );
    }

    #[test]
    fn two_behind_clips_to_a_triangle() {
        let clipped = clip_near_plane([v(0, 0, 10), v(10, 0, 10), v(0, 10, 100)]);
        assert_eq!(clipped.len(), 3, "two-behind clips to a triangle");
        assert!(clipped.iter().all(|p| p.z >= 50));
    }

    #[test]
    fn all_behind_clips_to_nothing() {
        let clipped = clip_near_plane([v(0, 0, 10), v(10, 0, 10), v(0, 10, 10)]);
        assert!(clipped.is_empty(), "a fully-behind triangle is invisible");
    }

    #[test]
    fn intersection_lands_on_the_near_plane_with_interpolated_xy() {
        let p = clip_near_intersection(v(100, 0, 100), v(0, 0, 0));
        assert_eq!(p.z, 50);
        assert_eq!(p.x, 50, "half-way between x=0 and x=100");
        assert_eq!(p.y, 0);
    }
}

/// `render2`/`render3_z_clip`'s winding test on the first three screen-space
/// vertices of a (possibly clipped) polygon: keep `cross > 0`.
fn face_winding_passes(pix: &Pix3DDraw, clipped: &[ClipVertex]) -> bool {
    let sx = |v: &ClipVertex| pix.origin_x + v.x.wrapping_shl(9).wrapping_div(v.z);
    let sy = |v: &ClipVertex| pix.origin_y + v.y.wrapping_shl(9).wrapping_div(v.z);
    let (x0, y0) = (sx(&clipped[0]), sy(&clipped[0]));
    let (x1, y1) = (sx(&clipped[1]), sy(&clipped[1]));
    let (x2, y2) = (sx(&clipped[2]), sy(&clipped[2]));
    wrapping_cross(x0 - x1, y2 - y1, y0 - y1, x2 - x1) > 0
}

/// World-space shift that slides a scene sprite off a same-tile wall's
/// thickness (16 units, the loc model span) toward the tile interior.
/// `WSHAPE0` bits: west=1 north=2 east=4 south=8.
fn scenery_inward_nudge(
    world: &World,
    level: i32,
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
) -> (i32, i32) {
    const THICK: i32 = 16;
    let mut dx = 0i32;
    let mut dz = 0i32;
    for tx in min_x..=max_x {
        for tz in min_z..=max_z {
            let Some(wall) = tile_at(&world.squares, level, tx, tz).and_then(|t| t.wall.as_ref())
            else {
                continue;
            };
            let bits = wall.angle1 | wall.angle2;
            if bits & 1 != 0 {
                dx = THICK;
            }
            if bits & 2 != 0 {
                dz = -THICK;
            }
            if bits & 4 != 0 {
                dx = -THICK;
            }
            if bits & 8 != 0 {
                dz = THICK;
            }
        }
    }
    (dx, dz)
}

/// `ModelSource.worldRender` for the GPU path: fetch the temp model,
/// record its `minY`, then mesh its faces. The `Model` variant meshes its
/// geometry directly (the TS `Model` override).
#[allow(clippy::too_many_arguments)]
fn emit_scene_model(
    model: &mut SceneModel,
    cache: &Cache,
    loop_cycle: i32,
    pix: &mut Pix3DDraw,
    mesh: &mut SceneMesh,
    cam: &SceneCam,
    yaw: i32,
    rel_x: i32,
    rel_y: i32,
    rel_z: i32,
    typecode: i32,
    wall: bool,
) {
    if crate::render_debug_enabled() {
        let loc_id = (typecode >> 14) & 0x7fff;
        static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i32>>> =
            std::sync::OnceLock::new();
        let seen = SEEN.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
        let mut seen = seen.lock().unwrap();
        if seen.insert(loc_id) {
            let model = match model {
                SceneModel::Model(m) => m.clone(),
                _ => model.get_temp_model(cache, loop_cycle).unwrap_or_default(),
            };
            if let (Some(rt), Some(fc), Some(fca)) = (
                &model.face_render_type,
                &model.face_colour,
                &model.face_colour_a,
            ) {
                let mut sample = Vec::new();
                for (f, &t) in rt.iter().enumerate() {
                    let ty = t & 0x3;
                    if ty == 2 || ty == 3 {
                        sample.push(format!(
                            "(ty{ty} tex{} shade{} pr{})",
                            fc.get(f).copied().unwrap_or(-1),
                            fca.get(f).copied().unwrap_or(-1),
                            model
                                .face_priority
                                .as_ref()
                                .and_then(|p| p.get(f))
                                .copied()
                                .unwrap_or(-1)
                        ));
                    }
                }
                let hidden = model
                    .face_render_type
                    .as_ref()
                    .map(|rt| rt.iter().filter(|&&t| t == -1).count())
                    .unwrap_or(0);
                eprintln!(
                    "[gpu-emit] loc {loc_id}: faces={} hidden={hidden} {}",
                    model.num_faces,
                    sample.join(" ")
                );
            }
        }
        // A static model that reaches the emitter unlit (`face_colour_a`
        // absent) emits no faces — the wall becomes the scene's black clear
        // colour. Report it once per loc id to trace the re-resolve path.
        if let SceneModel::Model(m) = &*model {
            if m.face_colour_a.is_none() {
                static UNLIT: std::sync::OnceLock<
                    std::sync::Mutex<std::collections::HashSet<i32>>,
                > = std::sync::OnceLock::new();
                let unlit =
                    UNLIT.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
                if unlit.lock().unwrap().insert(loc_id) {
                    eprintln!("[unlit] loc {loc_id} reached emitter with no face_colour_a (faces skipped)");
                }
            }
        }
    }
    match model {
        SceneModel::Model(model) => {
            emit_model_faces(
                model, pix, mesh, cam, yaw, rel_x, rel_y, rel_z, typecode, wall,
            );
        }
        _ => {
            if let Some(temp) = model.get_temp_model(cache, loop_cycle) {
                let min_y = temp.min_y;
                match model {
                    SceneModel::Obj(obj) => obj.min_y = min_y,
                    SceneModel::LocAnim(anim) => anim.min_y = min_y,
                    SceneModel::Player(player) => player.min_y = min_y,
                    SceneModel::Npc(npc) => npc.min_y = min_y,
                    SceneModel::Proj(proj) => proj.min_y = min_y,
                    SceneModel::SpotAnim(anim) => anim.min_y = min_y,
                    SceneModel::Model(_) => unreachable!(),
                }
                emit_model_faces(
                    &temp, pix, mesh, cam, yaw, rel_x, rel_y, rel_z, typecode, wall,
                );
            }
        }
    }
}

/// `worldRender` + `render2`/`render3` for the GPU path: cull and pick the
/// model AABB, transform every vertex to camera space with the exact
/// fixed-point math, then emit each visible face with its per-vertex
/// colour-table shades. The depth-buffer pipeline replaces the CPU's
/// depth/priority bucket merge; textured faces (`renderType & 0x3 == 2`)
/// carry the projective UV of the CPU `texture_triangle` and sample the
/// shared model atlas.
#[allow(clippy::too_many_arguments)]
fn emit_model_faces(
    model: &Model,
    pix: &mut Pix3DDraw,
    mesh: &mut SceneMesh,
    cam: &SceneCam,
    yaw: i32,
    rel_x: i32,
    rel_y: i32,
    rel_z: i32,
    typecode: i32,
    wall: bool,
) {
    // The area_game viewport the projection origin was set for.
    const SCENE_W: i32 = 512;
    const SCENE_H: i32 = 334;

    let (sin_yaw_m, cos_yaw_m) = if yaw != 0 {
        (
            Pix3D::sin_table().get(yaw as usize).copied().unwrap_or(0),
            Pix3D::cos_table().get(yaw as usize).copied().unwrap_or(0),
        )
    } else {
        (0, 0)
    };

    // `worldRender`'s model-space bounding-box test.
    let z_prime = (rel_z
        .wrapping_mul(cam.cos_yaw)
        .wrapping_sub(rel_x.wrapping_mul(cam.sin_yaw)))
        >> 16;
    let mid_z = rel_y
        .wrapping_mul(cam.sin_pitch)
        .wrapping_add(z_prime.wrapping_mul(cam.cos_pitch))
        >> 16;
    let radius_cos_pitch = (model.radius.wrapping_mul(cam.cos_pitch)) >> 16;
    let max_z = mid_z + radius_cos_pitch;
    if max_z <= 50 || mid_z >= 3500 {
        return;
    }

    let mid_x = (rel_z
        .wrapping_mul(cam.sin_yaw)
        .wrapping_add(rel_x.wrapping_mul(cam.cos_yaw)))
        >> 16;
    let mut left_x = (mid_x.wrapping_sub(model.radius)) << 9;
    if left_x.wrapping_div(max_z) >= SCENE_W {
        return;
    }
    let mut right_x = (mid_x.wrapping_add(model.radius)) << 9;
    if right_x.wrapping_div(max_z) <= -SCENE_W {
        return;
    }

    let mid_y = rel_y
        .wrapping_mul(cam.cos_pitch)
        .wrapping_sub(z_prime.wrapping_mul(cam.sin_pitch))
        >> 16;
    let radius_sin_pitch = (model.radius.wrapping_mul(cam.sin_pitch)) >> 16;
    let mut bottom_y = (mid_y.wrapping_add(radius_sin_pitch)) << 9;
    if bottom_y.wrapping_div(max_z) <= -SCENE_H {
        return;
    }
    let y_prime = radius_sin_pitch + ((model.min_y.wrapping_mul(cam.cos_pitch)) >> 16);
    let mut top_y = (mid_y.wrapping_sub(y_prime)) << 9;
    if top_y.wrapping_div(max_z) >= SCENE_H {
        return;
    }

    // `worldRender` AABB pre-test. Locs (`use_aabb_mouse_check == false`)
    // then require a projected face to contain the mouse, matching CPU
    // `render2`. Entities that set the AABB flag (objs/npcs/players) pick
    // here. AABB-only loc picks are what open a door on a walk-by click;
    // RuneLite's GPU plugin never replaces clickboxes.
    let mut picking = false;
    if typecode > 0 && pix.mouse_check {
        let mut z = mid_z - radius_cos_pitch;
        if z <= 50 {
            z = 50;
        }
        if mid_x > 0 {
            left_x = left_x.wrapping_div(max_z);
            right_x = right_x.wrapping_div(z);
        } else {
            right_x = right_x.wrapping_div(max_z);
            left_x = left_x.wrapping_div(z);
        }
        if mid_y > 0 {
            top_y = top_y.wrapping_div(max_z);
            bottom_y = bottom_y.wrapping_div(z);
        } else {
            bottom_y = bottom_y.wrapping_div(max_z);
            top_y = top_y.wrapping_div(z);
        }
        let mouse_x = pix.mouse_x - pix.origin_x;
        let mouse_y = pix.mouse_y - pix.origin_y;
        if mouse_x > left_x && mouse_x < right_x && mouse_y > top_y && mouse_y < bottom_y {
            if model.use_aabb_mouse_check {
                Model::pick(pix, typecode);
            } else {
                picking = true;
            }
        }
    }

    // Per-vertex camera-space transform (the `worldRender` loop).
    let (Some(point_x), Some(point_y), Some(point_z)) =
        (&model.point_x, &model.point_y, &model.point_z)
    else {
        return;
    };
    let vertex_count = model.num_points as usize;
    let mut cam_x = vec![0i32; vertex_count];
    let mut cam_y = vec![0i32; vertex_count];
    let mut cam_z = vec![0i32; vertex_count];
    for v in 0..vertex_count {
        let (Some(&x0), Some(&y0), Some(&z0)) = (point_x.get(v), point_y.get(v), point_z.get(v))
        else {
            continue;
        };
        let (mut x, mut y, mut z) = (x0, y0, z0);
        if yaw != 0 {
            let temp = (z
                .wrapping_mul(sin_yaw_m)
                .wrapping_add(x.wrapping_mul(cos_yaw_m)))
                >> 16;
            z = (z
                .wrapping_mul(cos_yaw_m)
                .wrapping_sub(x.wrapping_mul(sin_yaw_m)))
                >> 16;
            x = temp;
        }
        x = x.wrapping_add(rel_x);
        y = y.wrapping_add(rel_y);
        z = z.wrapping_add(rel_z);
        let temp = (z
            .wrapping_mul(cam.sin_yaw)
            .wrapping_add(x.wrapping_mul(cam.cos_yaw)))
            >> 16;
        z = (z
            .wrapping_mul(cam.cos_yaw)
            .wrapping_sub(x.wrapping_mul(cam.sin_yaw)))
            >> 16;
        x = temp;
        let temp = (y
            .wrapping_mul(cam.cos_pitch)
            .wrapping_sub(z.wrapping_mul(cam.sin_pitch)))
            >> 16;
        z = (y
            .wrapping_mul(cam.sin_pitch)
            .wrapping_add(z.wrapping_mul(cam.cos_pitch)))
            >> 16;
        y = temp;
        cam_x[v] = x;
        cam_y[v] = y;
        cam_z[v] = z;
    }

    let (Some(face_vertex_a), Some(face_vertex_b), Some(face_vertex_c)) = (
        &model.face_vertex_a,
        &model.face_vertex_b,
        &model.face_vertex_c,
    ) else {
        return;
    };
    let face_colour_a = model.face_colour_a.as_ref();
    let face_colour_b = model.face_colour_b.as_ref();
    let face_colour_c = model.face_colour_c.as_ref();

    let mut winding_culled = 0usize;
    let mut emitted_min_z = f32::MAX;
    for f in 0..model.num_faces as usize {
        if model
            .face_render_type
            .as_ref()
            .and_then(|rt| rt.get(f))
            .copied()
            .unwrap_or(0)
            == -1
        {
            continue;
        }
        let (Some(&a), Some(&b), Some(&c)) = (
            face_vertex_a.get(f),
            face_vertex_b.get(f),
            face_vertex_c.get(f),
        ) else {
            continue;
        };
        let (a, b, c) = (a as usize, b as usize, c as usize);
        let (x_a, y_a, z_a) = (cam_x[a], cam_y[a], cam_z[a]);
        let (x_b, y_b, z_b) = (cam_x[b], cam_y[b], cam_z[b]);
        let (x_c, y_c, z_c) = (cam_x[c], cam_y[c], cam_z[c]);

        // CPU `render2` loc pick: projected-face bbox, first hit wins.
        if picking && z_a >= 50 && z_b >= 50 && z_c >= 50 {
            let sx_a = pix.origin_x + x_a.wrapping_shl(9).wrapping_div(z_a);
            let sy_a = pix.origin_y + y_a.wrapping_shl(9).wrapping_div(z_a);
            let sx_b = pix.origin_x + x_b.wrapping_shl(9).wrapping_div(z_b);
            let sy_b = pix.origin_y + y_b.wrapping_shl(9).wrapping_div(z_b);
            let sx_c = pix.origin_x + x_c.wrapping_shl(9).wrapping_div(z_c);
            let sy_c = pix.origin_y + y_c.wrapping_shl(9).wrapping_div(z_c);
            if model.is_mouse_roughly_inside_triangle(
                pix.mouse_x,
                pix.mouse_y,
                sy_a,
                sy_b,
                sy_c,
                sx_a,
                sx_b,
                sx_c,
            ) {
                Model::pick(pix, typecode);
                picking = false;
            }
        }

        // `render3`: per-vertex shades, flat for render types 1 and 3.
        let (Some(fca), Some(fcb), Some(fcc)) = (face_colour_a, face_colour_b, face_colour_c)
        else {
            continue;
        };
        let render_type = model
            .face_render_type
            .as_ref()
            .and_then(|rt| rt.get(f))
            .copied()
            .unwrap_or(0);
        let r#type = render_type & 0x3;
        let (shade_a, shade_b, shade_c) = if r#type == 1 || r#type == 3 {
            let shade = fca.get(f).copied().unwrap_or(0);
            (shade, shade, shade)
        } else {
            (
                fca.get(f).copied().unwrap_or(0),
                fcb.get(f).copied().unwrap_or(0),
                fcc.get(f).copied().unwrap_or(0),
            )
        };

        let trans = model
            .face_alpha
            .as_ref()
            .and_then(|a| a.get(f))
            .copied()
            .unwrap_or(0);
        let alpha = if trans == 0 {
            255
        } else {
            (256 - trans).clamp(0, 255) as u8
        };

        // The face priority becomes the shader's depth bias (the CPU's
        // priority-bucket painter merge, replaced by the depth buffer).
        let bias = model
            .face_priority
            .as_ref()
            .and_then(|p| p.get(f))
            .copied()
            .unwrap_or(0)
            .clamp(0, 255) as u32;

        // Textured faces (`renderType & 0x3 == 2` or `3`): the texture id is
        // `faceColour[face]`; the three texture-mapping vertices
        // `faceTextureP/M/N[renderType >> 2]` define the texture plane.
        // Type 3 is the flat-shaded texture variant (the CPU `render3`'s
        // `else` branch shades it with a single `face_colour_a`). The
        // per-vertex UV is RuneLite's `computeFaceUvs` in model space
        // (a→(0,0), b→(0,1), c→(1,0) in the texture), packed 0..255.
        let textured = if r#type == 2 || r#type == 3 {
            let Some(face_colour) = &model.face_colour else {
                continue;
            };
            let Some(&tex_id) = face_colour.get(f) else {
                continue;
            };
            // RuneLite-faithful (`ModelUploader` stores `faceTexture + 1`
            // and the shader samples the texture array with a clamped id):
            // an out-of-range id — the CPU's `getTexels` returns None past
            // 49 — is clamped into the valid range, never dropped, so a
            // textured wall/fence/door still emits vertices on the GPU path.
            let tex_id = tex_id.clamp(0, 49) as u32;
            let textured_face = (render_type >> 2) as usize;
            let (uvs, vvs) = match (
                &model.face_texture_p,
                &model.face_texture_m,
                &model.face_texture_n,
            ) {
                (Some(tex_p), Some(tex_m), Some(tex_n)) => {
                    match (
                        tex_p.get(textured_face),
                        tex_m.get(textured_face),
                        tex_n.get(textured_face),
                    ) {
                        (Some(&t_a), Some(&t_b), Some(&t_c)) => compute_face_uvs(
                            point_x,
                            point_y,
                            point_z,
                            a,
                            b,
                            c,
                            t_a as usize,
                            t_b as usize,
                            t_c as usize,
                        ),
                        // The texture-vertex index is out of range: fall
                        // back to RuneLite `computeFaceUvs`'s no-texture-
                        // face basis (a→(0,0), b→(1,0), c→(0,1)) rather
                        // than dropping the face.
                        _ => ([0, 255, 0], [0, 0, 255]),
                    }
                }
                _ => ([0, 255, 0], [0, 0, 255]),
            };
            Some((tex_id + 1, uvs, vvs))
        } else {
            None
        };

        let verts = [
            ClipVertex {
                x: x_a,
                y: y_a,
                z: z_a,
                shade: shade_a,
                u: textured.map_or(0, |(_, u, _)| u[0]),
                v: textured.map_or(0, |(_, _, v)| v[0]),
            },
            ClipVertex {
                x: x_b,
                y: y_b,
                z: z_b,
                shade: shade_b,
                u: textured.map_or(0, |(_, u, _)| u[1]),
                v: textured.map_or(0, |(_, _, v)| v[1]),
            },
            ClipVertex {
                x: x_c,
                y: y_c,
                z: z_c,
                shade: shade_c,
                u: textured.map_or(0, |(_, u, _)| u[2]),
                v: textured.map_or(0, |(_, _, v)| v[2]),
            },
        ];

        // The CPU clips faces crossing the near plane (`render3_z_clip`),
        // never drops them; do the same here so a wall the camera walks up
        // to does not vanish to the scene's black clear colour.
        let clipped = if verts.iter().any(|v| v.z < 50) {
            clip_near_plane(verts)
        } else {
            verts.to_vec()
        };
        if clipped.len() < 3 || !face_winding_passes(pix, &clipped) {
            winding_culled += 1;
            continue;
        }

        let translucent = alpha != 255;
        for i in 1..clipped.len() - 1 {
            let (v0, v1, v2) = (clipped[0], clipped[i], clipped[i + 1]);
            emitted_min_z = emitted_min_z
                .min(v0.z as f32)
                .min(v1.z as f32)
                .min(v2.z as f32);
            if let Some((tex_id_plus_1, _, _)) = textured {
                mesh.push(
                    GpuVertex::textured(
                        v0.x,
                        v0.y,
                        v0.z,
                        v0.u,
                        v0.v,
                        v0.shade,
                        alpha,
                        bias,
                        tex_id_plus_1,
                    ),
                    GpuVertex::textured(
                        v1.x,
                        v1.y,
                        v1.z,
                        v1.u,
                        v1.v,
                        v1.shade,
                        alpha,
                        bias,
                        tex_id_plus_1,
                    ),
                    GpuVertex::textured(
                        v2.x,
                        v2.y,
                        v2.z,
                        v2.u,
                        v2.v,
                        v2.shade,
                        alpha,
                        bias,
                        tex_id_plus_1,
                    ),
                    translucent,
                    wall,
                );
            } else {
                mesh.push(
                    GpuVertex::new(v0.x, v0.y, v0.z, v0.shade, alpha, bias),
                    GpuVertex::new(v1.x, v1.y, v1.z, v1.shade, alpha, bias),
                    GpuVertex::new(v2.x, v2.y, v2.z, v2.shade, alpha, bias),
                    translucent,
                    wall,
                );
            }
        }
    }

    if crate::render_debug_enabled() {
        let loc_id = (typecode >> 14) & 0x7fff;
        if matches!(loc_id, 1602 | 2213 | 2215) {
            eprintln!(
                "[winding] loc {loc_id}: faces={} winding_culled={winding_culled} min_z={emitted_min_z}",
                model.num_faces
            );
        }
    }
}

/// The RuneLite projective UV: a point `p`'s (u, v) in the texture plane
/// spanned by basis `origin`/`b`/`c`, packed 0..255 like the CPU's
/// fixed-point texture coordinates (the `abs` mirrors the CPU's masked
/// texel wrap outside the parallelogram).
fn projective_uv(
    origin: (f32, f32, f32),
    b: (f32, f32, f32),
    c: (f32, f32, f32),
    p: (f32, f32, f32),
) -> (u32, u32) {
    let (ax, ay, az) = origin;
    let (bx, by, bz) = b;
    let (cx, cy, cz) = c;
    let (px, py, pz) = p;
    let (hx, hy, hz) = (cx - ax, cy - ay, cz - az);
    let (vx, vy, vz) = (bx - ax, by - ay, bz - az);
    let (dx, dy, dz) = (hy * vz - hz * vy, hz * vx - hx * vz, hx * vy - hy * vx);
    let (nux, nuy, nuz) = (az * vy - ay * vz, ax * vz - az * vx, ay * vx - ax * vy);
    let (nvx, nvy, nvz) = (hz * ay - hy * az, hx * az - hz * ax, hy * ax - hx * ay);
    let den = (dx * px + dy * py + dz * pz).max(1e-6);
    let u = (nux * px + nuy * py + nuz * pz) / den;
    let v = (nvx * px + nvy * py + nvz * pz) / den;
    let pack = |t: f32| (t.abs() * 256.0).clamp(0.0, 255.0) as u32;
    (pack(u), pack(v))
}

/// `renderGround` for the GPU path: transform the ground's vertices to
/// camera space, then mesh every face with its per-vertex colour-table
/// shades (textured ground samples the model atlas with the quad-corner
/// projective UV). The ground click raycast is kept: the sim's walk-dest
/// pick reads `world.ground_x/z`.
#[allow(clippy::too_many_arguments)]
fn emit_ground(
    world: &mut World,
    pix: &mut Pix3DDraw,
    mesh: &mut SceneMesh,
    cam: &SceneCam,
    ground: Ground,
    tile_x: i32,
    tile_z: i32,
) {
    let vertex_count = ground.vertices();
    let mut cam_x = Vec::with_capacity(vertex_count);
    let mut cam_y = Vec::with_capacity(vertex_count);
    let mut cam_z = Vec::with_capacity(vertex_count);
    for i in 0..vertex_count {
        let mut x = ground.vertex_x[i] - cam.eye_x;
        let mut y = ground.vertex_y[i] - cam.eye_y;
        let mut z = ground.vertex_z[i] - cam.eye_z;
        let tmp = (z
            .wrapping_mul(cam.sin_yaw)
            .wrapping_add(x.wrapping_mul(cam.cos_yaw)))
            >> 16;
        z = (z
            .wrapping_mul(cam.cos_yaw)
            .wrapping_sub(x.wrapping_mul(cam.sin_yaw)))
            >> 16;
        x = tmp;
        let tmp = (y
            .wrapping_mul(cam.cos_pitch)
            .wrapping_sub(z.wrapping_mul(cam.sin_pitch)))
            >> 16;
        z = (y
            .wrapping_mul(cam.sin_pitch)
            .wrapping_add(z.wrapping_mul(cam.cos_pitch)))
            >> 16;
        y = tmp;
        cam_x.push(x);
        cam_y.push(y);
        cam_z.push(z);
    }

    let face_count = ground.faces();
    for f in 0..face_count {
        let a = ground.face_vertex_a[f] as usize;
        let b = ground.face_vertex_b[f] as usize;
        let c = ground.face_vertex_c[f] as usize;
        let (x0, y0, z0) = (cam_x[a], cam_y[a], cam_z[a]);
        let (x1, y1, z1) = (cam_x[b], cam_y[b], cam_z[b]);
        let (x2, y2, z2) = (cam_x[c], cam_y[c], cam_z[c]);
        if z0 < 50 || z1 < 50 || z2 < 50 {
            continue;
        }
        let sx0 = pix.origin_x + x0.wrapping_shl(9).wrapping_div(z0);
        let sy0 = pix.origin_y + y0.wrapping_shl(9).wrapping_div(z0);
        let sx1 = pix.origin_x + x1.wrapping_shl(9).wrapping_div(z1);
        let sy1 = pix.origin_y + y1.wrapping_shl(9).wrapping_div(z1);
        let sx2 = pix.origin_x + x2.wrapping_shl(9).wrapping_div(z2);
        let sy2 = pix.origin_y + y2.wrapping_shl(9).wrapping_div(z2);
        if wrapping_cross(sx0 - sx1, sy2 - sy1, sy0 - sy1, sx2 - sx1) <= 0 {
            continue;
        }

        if world.click
            && inside_triangle(world.click_x, world.click_y, sy0, sy1, sy2, sx0, sx1, sx2)
        {
            world.ground_x = tile_x;
            world.ground_z = tile_z;
        }

        let colour_a = ground.face_colour_a.get(f).copied().unwrap_or(0);
        if colour_a == 12345678 {
            continue;
        }
        let colour_b = ground.face_colour_b.get(f).copied().unwrap_or(0);
        let colour_c = ground.face_colour_c.get(f).copied().unwrap_or(0);

        // Textured ground faces sample the model atlas with the same
        // projective UV the CPU ground raster uses. The texture plane is
        // the quad corners 0/1/3 for a flat ground (both triangles share
        // the basis, so the texture is continuous across the diagonal),
        // or the face's own corners for a non-flat ground, exactly like
        // the CPU `texture_triangle` calls.
        let face_texture = ground.face_texture.as_ref().and_then(|t| t.get(f)).copied();
        // Low-mem textured ground is flat-shaded from the texture's average
        // colour, exactly like the CPU `render_ground` (`get_table` over
        // `TEXTURE_AVERAGE[texture]`) — it does not sample the atlas. The
        // water/lava edge tiles are the visible case: CPU low-mem paints a
        // shade of blue where GPU was still sampling the texture.
        let tex_average = if pix.low_mem {
            match face_texture {
                Some(t) if t != -1 => Some(TEXTURE_AVERAGE.get(t as usize).copied().unwrap_or(41)),
                _ => None,
            }
        } else {
            None
        };
        let textured = match face_texture {
            Some(t) if t != -1 && t < 50 && tex_average.is_none() => Some(t as u32),
            _ => None,
        };
        if let Some(tex) = textured {
            let (ba, bb, bc) = if ground.flat {
                (0usize, 1usize, 3usize)
            } else {
                (a, b, c)
            };
            let (ax, ay, az) = (cam_x[ba] as f32, cam_y[ba] as f32, cam_z[ba] as f32);
            let (bx, by, bz) = (cam_x[bb] as f32, cam_y[bb] as f32, cam_z[bb] as f32);
            let (cx, cy, cz) = (cam_x[bc] as f32, cam_y[bc] as f32, cam_z[bc] as f32);
            let (hx, hy, hz) = (cx - ax, cy - ay, cz - az);
            let (vx, vy, vz) = (bx - ax, by - ay, bz - az);
            let (dx, dy, dz) = (hy * vz - hz * vy, hz * vx - hx * vz, hx * vy - hy * vx);
            let (nux, nuy, nuz) = (az * vy - ay * vz, ax * vz - az * vx, ay * vx - ax * vy);
            let (nvx, nvy, nvz) = (hz * ay - hy * az, hx * az - hz * ax, hy * ax - hx * ay);
            // The CPU `texture_triangle`'s projective UV, packed to the
            // fixed-point 0..255 form the shader samples. The `abs` mirrors
            // UV regions outside the texture's parallelogram (the second
            // triangle of a quad), matching the CPU's masked texel wrap.
            let uv = |x: f32, y: f32, z: f32| -> (u32, u32) {
                let den = (dx * x + dy * y + dz * z).max(1e-6);
                let u = (nux * x + nuy * y + nuz * z) / den;
                let v = (nvx * x + nvy * y + nvz * z) / den;
                let pack = |t: f32| (t.abs() * 256.0).clamp(0.0, 255.0) as u32;
                (pack(u), pack(v))
            };
            let (u0, v0) = uv(x0 as f32, y0 as f32, z0 as f32);
            let (u1, v1) = uv(x1 as f32, y1 as f32, z1 as f32);
            let (u2, v2) = uv(x2 as f32, y2 as f32, z2 as f32);
            mesh.push(
                GpuVertex::textured(x0, y0, z0, u0, v0, colour_a, 255, 0, tex + 1),
                GpuVertex::textured(x1, y1, z1, u1, v1, colour_b, 255, 0, tex + 1),
                GpuVertex::textured(x2, y2, z2, u2, v2, colour_c, 255, 0, tex + 1),
                false,
                false,
            );
        } else {
            let shade_of = |c: i32| match tex_average {
                Some(avg) => get_table(avg, c),
                None => c,
            };
            mesh.push(
                GpuVertex::new(x0, y0, z0, shade_of(colour_a), 255, 0),
                GpuVertex::new(x1, y1, z1, shade_of(colour_b), 255, 0),
                GpuVertex::new(x2, y2, z2, shade_of(colour_c), 255, 0),
                false,
                false,
            );
        }
    }
}

/// `renderQuickGround` for the GPU path: the 4-corner quad (two
/// triangles) with the corner shade indices.
fn emit_quick_ground(
    world: &mut World,
    pix: &mut Pix3DDraw,
    mesh: &mut SceneMesh,
    cam: &SceneCam,
    ground: QuickGround,
    level: i32,
    tile_x: i32,
    tile_z: i32,
) {
    let mut x = [0i32; 4];
    let mut y = [0i32; 4];
    let mut z = [0i32; 4];
    x[0] = (tile_x << 7) - cam.eye_x;
    x[1] = x[0] + 128;
    x[2] = x[1];
    x[3] = x[0];
    z[0] = (tile_z << 7) - cam.eye_z;
    z[1] = z[0];
    z[2] = z[0] + 128;
    z[3] = z[2];
    y[0] = ground_h(world, level, tile_x, tile_z) - cam.eye_y;
    y[1] = ground_h(world, level, tile_x + 1, tile_z) - cam.eye_y;
    y[2] = ground_h(world, level, tile_x + 1, tile_z + 1) - cam.eye_y;
    y[3] = ground_h(world, level, tile_x, tile_z + 1) - cam.eye_y;

    let mut sx = [0i32; 4];
    let mut sy = [0i32; 4];
    for i in 0..4 {
        let tmp = (z[i]
            .wrapping_mul(cam.sin_yaw)
            .wrapping_add(x[i].wrapping_mul(cam.cos_yaw)))
            >> 16;
        z[i] = (z[i]
            .wrapping_mul(cam.cos_yaw)
            .wrapping_sub(x[i].wrapping_mul(cam.sin_yaw)))
            >> 16;
        x[i] = tmp;
        let tmp = (y[i]
            .wrapping_mul(cam.cos_pitch)
            .wrapping_sub(z[i].wrapping_mul(cam.sin_pitch)))
            >> 16;
        z[i] = (y[i]
            .wrapping_mul(cam.sin_pitch)
            .wrapping_add(z[i].wrapping_mul(cam.cos_pitch)))
            >> 16;
        y[i] = tmp;
        if z[i] < 50 {
            return;
        }
        sx[i] = pix.origin_x + x[i].wrapping_shl(9).wrapping_div(z[i]);
        sy[i] = pix.origin_y + y[i].wrapping_shl(9).wrapping_div(z[i]);
    }

    // A textured quick ground (water, lava) is flat-shaded with the
    // texture's average colour in low-mem, mirroring the CPU
    // `render_quick_ground` (`get_table(TEXTURE_AVERAGE[texture], colour)`).
    // High-mem samples the atlas with the quad-corner projective UV.
    let lowmem_average = if ground.texture >= 0 && ground.texture < 50 && pix.low_mem {
        TEXTURE_AVERAGE.get(ground.texture as usize).copied()
    } else {
        None
    };
    let highmem_texture = if ground.texture >= 0 && ground.texture < 50 && !pix.low_mem {
        Some(ground.texture as u32)
    } else {
        None
    };
    let shade_of = |colour: i32| -> i32 {
        match lowmem_average {
            Some(avg) => get_table(avg, colour),
            None => colour,
        }
    };
    let corner = |k: usize| (x[k] as f32, y[k] as f32, z[k] as f32);

    // Triangle 1: corners (2, 3, 1), shades (ne, nw, se).
    if wrapping_cross(sx[2] - sx[3], sy[1] - sy[3], sy[2] - sy[3], sx[1] - sx[3]) > 0 {
        if world.click
            && inside_triangle(
                world.click_x,
                world.click_y,
                sy[2],
                sy[3],
                sy[1],
                sx[2],
                sx[3],
                sx[1],
            )
        {
            world.ground_x = tile_x;
            world.ground_z = tile_z;
        }
        if ground.colour_ne != 12345678 {
            if let Some(tex) = highmem_texture {
                let (ba, bb, bc) = if ground.flat { (0, 1, 3) } else { (2, 3, 1) };
                let (u2, v2) = projective_uv(corner(ba), corner(bb), corner(bc), corner(2));
                let (u3, v3) = projective_uv(corner(ba), corner(bb), corner(bc), corner(3));
                let (u1, v1) = projective_uv(corner(ba), corner(bb), corner(bc), corner(1));
                mesh.push(
                    GpuVertex::textured(
                        x[2],
                        y[2],
                        z[2],
                        u2,
                        v2,
                        ground.colour_ne,
                        255,
                        0,
                        tex + 1,
                    ),
                    GpuVertex::textured(
                        x[3],
                        y[3],
                        z[3],
                        u3,
                        v3,
                        ground.colour_nw,
                        255,
                        0,
                        tex + 1,
                    ),
                    GpuVertex::textured(
                        x[1],
                        y[1],
                        z[1],
                        u1,
                        v1,
                        ground.colour_se,
                        255,
                        0,
                        tex + 1,
                    ),
                    false,
                    false,
                );
            } else {
                mesh.push(
                    GpuVertex::new(x[2], y[2], z[2], shade_of(ground.colour_ne), 255, 0),
                    GpuVertex::new(x[3], y[3], z[3], shade_of(ground.colour_nw), 255, 0),
                    GpuVertex::new(x[1], y[1], z[1], shade_of(ground.colour_se), 255, 0),
                    false,
                    false,
                );
            }
        }
    }

    // Triangle 2: corners (0, 1, 3), shades (sw, se, nw).
    if wrapping_cross(sx[0] - sx[1], sy[3] - sy[1], sy[0] - sy[1], sx[3] - sx[1]) > 0 {
        if world.click
            && inside_triangle(
                world.click_x,
                world.click_y,
                sy[0],
                sy[1],
                sy[3],
                sx[0],
                sx[1],
                sx[3],
            )
        {
            world.ground_x = tile_x;
            world.ground_z = tile_z;
        }
        if ground.colour_sw != 12345678 {
            if let Some(tex) = highmem_texture {
                let (u0, v0) = projective_uv(corner(0), corner(1), corner(3), corner(0));
                let (u1, v1) = projective_uv(corner(0), corner(1), corner(3), corner(1));
                let (u3, v3) = projective_uv(corner(0), corner(1), corner(3), corner(3));
                mesh.push(
                    GpuVertex::textured(
                        x[0],
                        y[0],
                        z[0],
                        u0,
                        v0,
                        ground.colour_sw,
                        255,
                        0,
                        tex + 1,
                    ),
                    GpuVertex::textured(
                        x[1],
                        y[1],
                        z[1],
                        u1,
                        v1,
                        ground.colour_se,
                        255,
                        0,
                        tex + 1,
                    ),
                    GpuVertex::textured(
                        x[3],
                        y[3],
                        z[3],
                        u3,
                        v3,
                        ground.colour_nw,
                        255,
                        0,
                        tex + 1,
                    ),
                    false,
                    false,
                );
            } else {
                mesh.push(
                    GpuVertex::new(x[0], y[0], z[0], shade_of(ground.colour_sw), 255, 0),
                    GpuVertex::new(x[1], y[1], z[1], shade_of(ground.colour_se), 255, 0),
                    GpuVertex::new(x[3], y[3], z[3], shade_of(ground.colour_nw), 255, 0),
                    false,
                    false,
                );
            }
        }
    }
}
