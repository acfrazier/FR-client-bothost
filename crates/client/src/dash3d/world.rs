// Port of `~/experiments/Server/webclient/src/dash3d/World.ts` — the scene
// graph half (ground, walls, decor, objects, sprites, occluders) and the
// render pass (`renderAll`, `fill`, `renderGround`/`renderQuickGround`,
// occlusion tests, mouse picking). Every TS `World` static that the render
// pass reads or writes (`click`, camera sin/cos, `cx/cy/cz`, `fillLeft`,
// `fillQueue`, `cycleNo`, `visBacking`, the sprite buffer, active
// occluders) is an instance field here — never process-global. The `Ground`
// draw buffers (TS `Ground` statics) live on the `World` instance too.
// `renderAll` takes the config `Cache` and `loopCycle` because the TS
// `ModelSource.worldRender` -> `getTempModel()` chain rebuilds player/npc/
// loc-anim models from config tables during the pass.
//
// Sprites are arena-allocated (`World.sprites`) and tiles hold arena indices,
// matching the TS sharing of one sprite object across every tile it spans.
// The TS static `lowMem` (client config on `Pix3DDraw`) and the minimap
// tables are not part of the render pass; `numOccluders`/`occluders`
// (per-scene mutable) live on the `World` instance.
use std::collections::VecDeque;

use crate::config::Cache;
use crate::dash3d::TerrainOverlayShape;
use crate::dash3d::{
    Decor, Ground, GroundDecor, GroundObject, LocAngle, Model, Occlude, QuickGround, SceneModel,
    Sprite, Square, Wall,
};
use crate::graphics::{Pix2D, Pix3D, Pix3DDraw};

const OCCLUDER_LEVELS: usize = 4;
const MAX_OCCLUDERS: usize = 500;
const MAX_DYNAMIC_SPRITES: usize = 5000;
const MAX_SPRITE_BUFFER: usize = 100;
const MAX_ACTIVE_OCCLUDERS: usize = 500;

const PRETAB: [i32; 9] = [19, 55, 38, 155, 255, 110, 137, 205, 76];
const MIDTAB: [i32; 9] = [160, 192, 80, 96, 0, 144, 80, 48, 160];
const POSTTAB: [i32; 9] = [76, 8, 137, 4, 0, 1, 38, 2, 19];

const MIDDEP_16: [i32; 9] = [0, 0, 2, 0, 0, 2, 1, 1, 0];
const MIDDEP_32: [i32; 9] = [2, 0, 0, 2, 0, 0, 0, 4, 4];
const MIDDEP_64: [i32; 9] = [0, 4, 4, 8, 0, 0, 8, 0, 0];
const MIDDEP_128: [i32; 9] = [1, 1, 0, 0, 0, 8, 0, 0, 8];

const DECORXOF: [i32; 4] = [53, -53, -53, 53];
const DECORZOF: [i32; 4] = [-53, -53, 53, 53];
const DECORXOF2: [i32; 4] = [-45, 45, 45, -45];
const DECORZOF2: [i32; 4] = [45, 45, -45, -45];

// prettier-ignore
const TEXTURE_AVERAGE: [i32; 50] = [
    41,
    39248, // water
    41,
    4643, // planks
    41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41,
    43086, // marble
    41, 41, 41, 41, 41, 41, 41,
    8602, // mossybricks
    41,
    28992, // gungywater
    41, 41, 41, 41, 41,
    5056, // lava
    41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41, 41,
    3131, // pebblefloor
    41, 41, 41,
];

/// `World.visBacking[8][32][51][51]` flat row offset of the pitch/yaw pair
/// that `renderAll` binds as `visBackingDirty`.
const VIS_ROW_SIZE: usize = 51 * 51;

/// `levelHeightmaps[level][x][z]` ground heights, sized
/// `[maxLevel][maxTileX + 1][maxTileZ + 1]` (one extra row/column of corners).
pub type LevelHeightmaps = Vec<Vec<Vec<i32>>>;

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

pub struct World {
    min_level: i32,
    max_tile_level: i32,
    max_tile_x: i32,
    max_tile_z: i32,
    /// Read by the render pass (`renderGround`, `renderQuickGround`, the
    /// `renderAll` visibility gate). `pub(crate)` so `Client::map_build`
    /// mirrors `Client.groundh` after the load/fade pass (Java shares the
    /// one array between Client and World).
    pub(crate) groundh: LevelHeightmaps,
    squares: Vec<Vec<Vec<Option<Square>>>>,
    sprites: Vec<Option<Sprite>>,
    dynamic_count: i32,
    dynamic_sprites: Vec<Option<usize>>,
    /// Occluder table for the render pass. Per-scene mutable
    /// state (the TS `World.occluders`/`numOccluders` statics), so it lives
    /// on the `World` instance. Heap-backed because a by-value
    /// `[[Option<Occlude>; 500]; 4]` overflows small test-thread stacks in
    /// debug builds.
    num_occluders: [i32; OCCLUDER_LEVELS],
    occluders: Vec<Option<Occlude>>,
    /// TS `World.occlusionCycle`, `[maxLevel][maxTileX + 1][maxTileZ + 1]`
    /// i32s, flat. `groundOccluded` stamps ±`cycle_no` to cache per-frame
    /// occlusion answers; never cleared (like TS), stale stamps are aged out
    /// by the cycle comparison.
    occlusion_cycle: Vec<i32>,
    /// The render-pass TS `World` statics, per-scene here (see the header).
    pub click: bool,
    pub click_x: i32,
    pub click_y: i32,
    pub ground_x: i32,
    pub ground_z: i32,
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
    /// TS `World.activeOccluders`, as indices into `occluders` (arena
    /// indices instead of object references).
    active_occluders: Vec<Option<usize>>,
    /// TS `World.spriteBuffer`, as indices into `sprites`.
    sprite_buffer: Vec<Option<usize>>,
    /// TS `Ground.drawVertexX/Y` and `drawTextureVertexX/Y/Z` statics.
    ground_draw_vertex_x: [i32; 6],
    ground_draw_vertex_y: [i32; 6],
    ground_draw_texture_vertex_x: [i32; 6],
    ground_draw_texture_vertex_y: [i32; 6],
    ground_draw_texture_vertex_z: [i32; 6],
    /// TS `World.shareTic`/`shareMap`/`shareMap2` (World.ts 145-147): the
    /// share-light merge stamps. The maps grow to the largest model seen
    /// instead of TS's fixed 10000-slot arrays (stale stamps are aged out
    /// by the `share_tic` comparison, exactly like TS).
    share_tic: i32,
    share_map: Vec<i32>,
    share_map2: Vec<i32>,
}

impl World {
    pub fn new(
        groundh: LevelHeightmaps,
        max_tile_z: i32,
        max_level: i32,
        max_tile_x: i32,
    ) -> Self {
        let mut squares = Vec::with_capacity(max_level as usize);
        for _ in 0..max_level {
            let mut level = Vec::with_capacity(max_tile_x as usize);
            for _ in 0..max_tile_x {
                level.push((0..max_tile_z).map(|_| None).collect::<Vec<_>>());
            }
            squares.push(level);
        }
        let mut world = World {
            min_level: 0,
            max_tile_level: max_level,
            max_tile_x,
            max_tile_z,
            groundh,
            squares,
            sprites: Vec::new(),
            dynamic_count: 0,
            dynamic_sprites: vec![None; MAX_DYNAMIC_SPRITES],
            num_occluders: [0; OCCLUDER_LEVELS],
            occluders: (0..OCCLUDER_LEVELS * MAX_OCCLUDERS).map(|_| None).collect(),
            occlusion_cycle: vec![0; (max_level * (max_tile_x + 1) * (max_tile_z + 1)) as usize],
            click: false,
            click_x: 0,
            click_y: 0,
            ground_x: -1,
            ground_z: -1,
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
            share_tic: 0,
            share_map: Vec::new(),
            share_map2: Vec::new(),
        };
        world.reset_map();
        world
    }

    pub fn reset_map(&mut self) {
        for level in 0..self.max_tile_level {
            for x in 0..self.max_tile_x {
                for z in 0..self.max_tile_z {
                    self.squares[level as usize][x as usize][z as usize] = None;
                }
            }
        }

        for l in 0..OCCLUDER_LEVELS {
            for o in 0..self.num_occluders[l] as usize {
                self.occluders[l * MAX_OCCLUDERS + o] = None;
            }
            self.num_occluders[l] = 0;
        }

        for sprite in self.dynamic_sprites.iter_mut() {
            *sprite = None;
        }
        self.dynamic_count = 0;
        self.sprites.clear();
        // TS `resetMap` also clears the render-pass sprite buffer.
        self.sprite_buffer.fill(None);
    }

    pub fn fill_base_level(&mut self, level: i32) {
        self.min_level = level;

        for stx in 0..self.max_tile_x {
            for stz in 0..self.max_tile_z {
                self.squares[level as usize][stx as usize][stz as usize] =
                    Some(Square::new(level, stx, stz));
            }
        }
    }

    pub fn push_down(&mut self, stx: i32, stz: i32) {
        let below = self.squares[0][stx as usize][stz as usize].take();

        for level in 0..3 {
            self.squares[level as usize][stx as usize][stz as usize] =
                self.squares[level as usize + 1][stx as usize][stz as usize].take();

            let tile = &mut self.squares[level as usize][stx as usize][stz as usize];
            if let Some(tile) = tile {
                tile.level -= 1;

                for i in 0..tile.sprite_count as usize {
                    if let Some(sprite_index) = tile.sprites[i] {
                        if let Some(sprite) = self.sprites[sprite_index].as_mut() {
                            if (sprite.typecode >> 29) & 0x3 == 2
                                && sprite.min_tile_x == stx
                                && sprite.min_tile_z == stz
                            {
                                sprite.level -= 1;
                            }
                        }
                    }
                }
            }
        }

        if self.squares[0][stx as usize][stz as usize].is_none() {
            self.squares[0][stx as usize][stz as usize] = Some(Square::new(0, stx, stz));
        }

        let tile = &mut self.squares[0][stx as usize][stz as usize];
        if let Some(tile) = tile {
            tile.linked_square = below.map(Box::new);
        }

        self.squares[3][stx as usize][stz as usize] = None;
    }

    pub fn set_occlude(
        &mut self,
        level: i32,
        r#type: i32,
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) {
        let count = self.num_occluders[level as usize] as usize;
        if count >= MAX_OCCLUDERS {
            return;
        }
        self.occluders[level as usize * MAX_OCCLUDERS + count] = Some(Occlude::new(
            min_x / 128,
            max_x / 128,
            min_z / 128,
            max_z / 128,
            r#type,
            min_x,
            max_x,
            min_z,
            max_z,
            min_y,
            max_y,
        ));
        self.num_occluders[level as usize] += 1;
    }

    pub fn set_layer(&mut self, level: i32, stx: i32, stz: i32, draw_level: i32) {
        let tile = &mut self.squares[level as usize][stx as usize][stz as usize];
        if let Some(tile) = tile {
            tile.draw_level = draw_level;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_ground(
        &mut self,
        level: i32,
        x: i32,
        z: i32,
        shape: i32,
        rotation: i32,
        texture: i32,
        height_sw: i32,
        height_se: i32,
        height_ne: i32,
        height_nw: i32,
        colour_sw: i32,
        colour_se: i32,
        colour_ne: i32,
        colour_nw: i32,
        colour2_sw: i32,
        colour2_se: i32,
        colour2_ne: i32,
        colour2_nw: i32,
        overlay: i32,
        underlay: i32,
    ) {
        for l in (0..=level).rev() {
            if self.squares[l as usize][x as usize][z as usize].is_none() {
                self.squares[l as usize][x as usize][z as usize] = Some(Square::new(l, x, z));
            }
        }

        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };

        if shape == TerrainOverlayShape::PLAIN {
            tile.quick_ground = Some(QuickGround::new(
                colour_sw,
                colour_se,
                colour_ne,
                colour_nw,
                -1,
                overlay,
                false,
            ));
        } else if shape == TerrainOverlayShape::DIAGONAL {
            tile.quick_ground = Some(QuickGround::new(
                colour2_sw,
                colour2_se,
                colour2_ne,
                colour2_nw,
                texture,
                underlay,
                height_sw == height_se && height_sw == height_ne && height_sw == height_nw,
            ));
        } else {
            tile.ground = Some(Ground::new(
                x,
                z,
                shape,
                rotation,
                texture,
                height_sw,
                height_se,
                height_ne,
                height_nw,
                colour_sw,
                colour_se,
                colour_ne,
                colour_nw,
                colour2_sw,
                colour2_se,
                colour2_ne,
                colour2_nw,
                overlay,
                underlay,
            ));
        }
    }

    pub fn set_ground_decor(
        &mut self,
        model: Option<SceneModel>,
        tile_level: i32,
        tile_x: i32,
        tile_z: i32,
        y: i32,
        typecode: i32,
        typecode2: i32,
    ) {
        if model.is_none() {
            return;
        }

        if self.squares[tile_level as usize][tile_x as usize][tile_z as usize].is_none() {
            self.squares[tile_level as usize][tile_x as usize][tile_z as usize] =
                Some(Square::new(tile_level, tile_x, tile_z));
        }

        let tile = &mut self.squares[tile_level as usize][tile_x as usize][tile_z as usize];
        if let Some(tile) = tile {
            tile.ground_decor = Some(GroundDecor::new(
                y,
                tile_x * 128 + 64,
                tile_z * 128 + 64,
                model.map(Box::new),
                typecode,
                typecode2,
            ));
        }
    }

    pub fn del_ground_decor(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.ground_decor = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_obj(
        &mut self,
        stx: i32,
        stz: i32,
        y: i32,
        level: i32,
        typecode: i32,
        top_obj: Option<SceneModel>,
        middle_obj: Option<SceneModel>,
        bottom_obj: Option<SceneModel>,
    ) {
        let mut stack_offset = 0i32;

        if self.squares[level as usize][stx as usize][stz as usize].is_some() {
            let tile = &self.squares[level as usize][stx as usize][stz as usize];
            if let Some(tile) = tile {
                for l in 0..tile.sprite_count as usize {
                    if let Some(idx) = tile.sprites[l] {
                        if let Some(SceneModel::Model(model)) = &self.sprites[idx].as_ref().unwrap().model
                        {
                            let height = model.obj_raise;
                            if height > stack_offset {
                                stack_offset = height;
                            }
                        }
                    }
                }
            }
        } else {
            self.squares[level as usize][stx as usize][stz as usize] =
                Some(Square::new(level, stx, stz));
        }

        let tile = &mut self.squares[level as usize][stx as usize][stz as usize];
        if let Some(tile) = tile {
            tile.ground_object = Some(GroundObject::new(
                y,
                stx * 128 + 64,
                stz * 128 + 64,
                top_obj.map(Box::new),
                middle_obj.map(Box::new),
                bottom_obj.map(Box::new),
                typecode,
                stack_offset,
            ));
        }
    }

    pub fn del_obj(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.ground_object = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_wall(
        &mut self,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        y: i32,
        angle1: i32,
        angle2: i32,
        model1: Option<SceneModel>,
        model2: Option<SceneModel>,
        typecode1: i32,
        typecode2: i32,
    ) {
        if model1.is_none() && model2.is_none() {
            return;
        }

        for l in (0..=level).rev() {
            if self.squares[l as usize][tile_x as usize][tile_z as usize].is_none() {
                self.squares[l as usize][tile_x as usize][tile_z as usize] =
                    Some(Square::new(l, tile_x, tile_z));
            }
        }

        let tile = &mut self.squares[level as usize][tile_x as usize][tile_z as usize];
        if let Some(tile) = tile {
            tile.wall = Some(Wall::new(
                y,
                tile_x * 128 + 64,
                tile_z * 128 + 64,
                angle1,
                angle2,
                model1.map(Box::new),
                model2.map(Box::new),
                typecode1,
                typecode2,
            ));
        }
    }

    pub fn del_wall(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.wall = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_decor(
        &mut self,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        y: i32,
        offset_x: i32,
        offset_z: i32,
        typecode: i32,
        model: Option<SceneModel>,
        info: i32,
        angle: i32,
        wshape: i32,
    ) {
        let Some(model) = model else { return };

        for l in (0..=level).rev() {
            if self.squares[l as usize][tile_x as usize][tile_z as usize].is_none() {
                self.squares[l as usize][tile_x as usize][tile_z as usize] =
                    Some(Square::new(l, tile_x, tile_z));
            }
        }

        let tile = &mut self.squares[level as usize][tile_x as usize][tile_z as usize];
        if let Some(tile) = tile {
            tile.decor = Some(Decor::new(
                y,
                tile_x * 128 + offset_x + 64,
                tile_z * 128 + offset_z + 64,
                wshape,
                angle,
                Box::new(model),
                typecode,
                info,
            ));
        }
    }

    pub fn del_decor(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.decor = None;
    }

    pub fn move_decor(&mut self, level: i32, x: i32, z: i32, offset: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        let Some(decor) = tile.decor.as_mut() else { return };

        let sx = x * 128 + 64;
        let sz = z * 128 + 64;
        decor.x = sx + (((decor.x - sx) * offset) / 16);
        decor.z = sz + (((decor.z - sz) * offset) / 16);
    }

    pub fn add_scenery(
        &mut self,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        y: i32,
        model: Option<SceneModel>,
        typecode: i32,
        info: i32,
        width: i32,
        length: i32,
        yaw: i32,
    ) -> bool {
        let Some(model) = model else { return true };

        let scene_x = tile_x * 128 + width * 64;
        let scene_z = tile_z * 128 + length * 64;
        self.set_sprite(scene_x, scene_z, y, level, tile_x, tile_z, width, length, Some(model), typecode, info, yaw, false)
    }

    pub fn add_dynamic(
        &mut self,
        level: i32,
        x: i32,
        y: i32,
        z: i32,
        model: Option<SceneModel>,
        typecode: i32,
        yaw: i32,
        padding: i32,
        forward_padding: bool,
    ) -> bool {
        let Some(model) = model else { return true };

        let mut x0 = x - padding;
        let mut z0 = z - padding;
        let mut x1 = x + padding;
        let mut z1 = z + padding;

        if forward_padding {
            if yaw > 640 && yaw < 1408 {
                z1 += 128;
            }
            if yaw > 1152 && yaw < 1920 {
                x1 += 128;
            }
            if yaw > 1664 || yaw < 384 {
                z0 -= 128;
            }
            if yaw > 128 && yaw < 896 {
                x0 -= 128;
            }
        }

        x0 /= 128;
        z0 /= 128;
        x1 /= 128;
        z1 /= 128;

        self.set_sprite(x, z, y, level, x0, z0, x1 + 1 - x0, z1 - z0 + 1, Some(model), typecode, 0, yaw, true)
    }

    pub fn add_dynamic2(
        &mut self,
        level: i32,
        x: i32,
        y: i32,
        z: i32,
        min_tile_x: i32,
        min_tile_z: i32,
        max_tile_x: i32,
        max_tile_z: i32,
        model: Option<SceneModel>,
        typecode: i32,
        yaw: i32,
    ) -> bool {
        match model {
            None => true,
            Some(model) => self.set_sprite(
                x,
                z,
                y,
                level,
                min_tile_x,
                min_tile_z,
                max_tile_x + 1 - min_tile_x,
                max_tile_z - min_tile_z + 1,
                Some(model),
                typecode,
                0,
                yaw,
                true,
            ),
        }
    }

    pub fn del_loc(&mut self, level: i32, x: i32, z: i32) {
        let index = {
            let tile = self.squares[level as usize][x as usize][z as usize].as_ref();
            let Some(tile) = tile else { return };
            let mut found = None;
            for l in 0..tile.sprite_count as usize {
                if let Some(idx) = tile.sprites[l] {
                    if let Some(sprite) = &self.sprites[idx] {
                        if (sprite.typecode >> 29) & 0x3 == 2
                            && sprite.min_tile_x == x
                            && sprite.min_tile_z == z
                        {
                            found = Some(idx);
                            break;
                        }
                    }
                }
            }
            found
        };
        if let Some(index) = index {
            self.del_sprite(index);
        }
    }

    pub fn remove_sprites(&mut self) {
        let dynamic: Vec<usize> = self.dynamic_sprites[..self.dynamic_count as usize]
            .iter()
            .flatten()
            .copied()
            .collect();
        for index in dynamic {
            self.del_sprite(index);
        }
        for slot in self.dynamic_sprites.iter_mut() {
            *slot = None;
        }
        self.dynamic_count = 0;
    }

    /// Read access to one tile (`squares` is private; the scene tests and
    /// `mapBuild`-adjacent queries read through this).
    pub fn square(&self, level: i32, x: i32, z: i32) -> Option<&Square> {
        self.squares[level as usize][x as usize][z as usize].as_ref()
    }

    /// `groundh[level][x][z]` read (guarded like `ground_h`; the field is
    /// `pub(crate)` for `Client::map_build`, external tests read through
    /// this).
    pub fn groundh_at(&self, level: i32, x: i32, z: i32) -> i32 {
        ground_h(self, level, x, z)
    }

    /// Number of live dynamic sprites (`dynamic_sprites` is private).
    pub fn dynamic_count(&self) -> i32 {
        self.dynamic_count
    }

    pub fn get_wall(&self, level: i32, x: i32, z: i32) -> Option<&Wall> {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.wall.as_ref())
    }

    pub fn get_wall_mut(&mut self, level: i32, x: i32, z: i32) -> Option<&mut Wall> {
        self.squares[level as usize][x as usize][z as usize]
            .as_mut()
            .and_then(|tile| tile.wall.as_mut())
    }

    pub fn get_decor(&self, level: i32, x: i32, z: i32) -> Option<&Decor> {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.decor.as_ref())
    }

    pub fn get_decor_mut(&mut self, level: i32, x: i32, z: i32) -> Option<&mut Decor> {
        self.squares[level as usize][x as usize][z as usize]
            .as_mut()
            .and_then(|tile| tile.decor.as_mut())
    }

    pub fn get_scene(&self, level: i32, x: i32, z: i32) -> Option<&Sprite> {
        let tile = self.squares[level as usize][x as usize][z as usize].as_ref()?;
        for l in 0..tile.sprite_count as usize {
            if let Some(idx) = tile.sprites[l] {
                if let Some(sprite) = &self.sprites[idx] {
                    if (sprite.typecode >> 29) & 0x3 == 2
                        && sprite.min_tile_x == x
                        && sprite.min_tile_z == z
                    {
                        return Some(sprite);
                    }
                }
            }
        }
        None
    }

    pub fn get_scene_mut(&mut self, level: i32, x: i32, z: i32) -> Option<&mut Sprite> {
        let tile = self.squares[level as usize][x as usize][z as usize].as_ref()?;
        let mut found = None;
        for l in 0..tile.sprite_count as usize {
            if let Some(idx) = tile.sprites[l] {
                if let Some(sprite) = &self.sprites[idx] {
                    if (sprite.typecode >> 29) & 0x3 == 2
                        && sprite.min_tile_x == x
                        && sprite.min_tile_z == z
                    {
                        found = Some(idx);
                        break;
                    }
                }
            }
        }
        self.sprites[found?].as_mut()
    }

    pub fn get_gd(&self, level: i32, x: i32, z: i32) -> Option<&GroundDecor> {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.ground_decor.as_ref())
    }

    pub fn get_gd_mut(&mut self, level: i32, x: i32, z: i32) -> Option<&mut GroundDecor> {
        self.squares[level as usize][x as usize][z as usize]
            .as_mut()
            .and_then(|tile| tile.ground_decor.as_mut())
    }

    pub fn wall_type(&self, level: i32, x: i32, z: i32) -> i32 {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.wall.as_ref())
            .map_or(0, |wall| wall.typecode)
    }

    pub fn decor_type(&self, level: i32, x: i32, z: i32) -> i32 {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.decor.as_ref())
            .map_or(0, |decor| decor.typecode)
    }

    /// `Client.groundObj[level][x][z]` presence for the minimap dots (TS
    /// minimapDraw 11317-11325). Bounds-checked like `tile_at`.
    pub fn ground_object_at(&self, level: i32, x: i32, z: i32) -> Option<&GroundObject> {
        tile_at(&self.squares, level, x, z).and_then(|tile| tile.ground_object.as_ref())
    }

    pub fn scene_type(&self, level: i32, x: i32, z: i32) -> i32 {
        let Some(tile) = self.squares[level as usize][x as usize][z as usize].as_ref() else {
            return 0;
        };
        for l in 0..tile.sprite_count as usize {
            if let Some(idx) = tile.sprites[l] {
                if let Some(sprite) = &self.sprites[idx] {
                    if (sprite.typecode >> 29) & 0x3 == 2
                        && sprite.min_tile_x == x
                        && sprite.min_tile_z == z
                    {
                        return sprite.typecode;
                    }
                }
            }
        }
        0
    }

    pub fn gd_type(&self, level: i32, x: i32, z: i32) -> i32 {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.ground_decor.as_ref())
            .map_or(0, |gd| gd.typecode)
    }

    pub fn type_code2(&self, level: i32, x: i32, z: i32, typecode: i32) -> i32 {
        let Some(tile) = self.squares[level as usize][x as usize][z as usize].as_ref() else {
            return -1;
        };
        if let Some(wall) = &tile.wall {
            if wall.typecode == typecode {
                return wall.typecode2 & 0xff;
            }
        }
        if let Some(decor) = &tile.decor {
            if decor.typecode == typecode {
                return decor.typecode2 & 0xff;
            }
        }
        if let Some(gd) = &tile.ground_decor {
            if gd.typecode == typecode {
                return gd.typecode2 & 0xff;
            }
        }
        for i in 0..tile.sprite_count as usize {
            if let Some(idx) = tile.sprites[i] {
                if let Some(sprite) = &self.sprites[idx] {
                    if sprite.typecode == typecode {
                        return sprite.typecode2 & 0xff;
                    }
                }
            }
        }
        -1
    }

    /// `shareLight(ambient, contrast, lightSrcX, lightSrcY, lightSrcZ)`
    /// from World.ts 589-628: after `calculateNormals` (which keeps
    /// `shared_point_normal` when the loc shares light), merge touching
    /// vertices' normals across walls/sprites/ground-decor and then run
    /// `Model.light` over every placed model. Each tile is taken out of
    /// `squares` for the duration of its pass because the helpers need
    /// `&mut self` (share scratch, other tiles) and `&mut Model`
    /// simultaneously; `shareLightLoc` never revisits the current tile
    /// (its own tile is inside the `allowFaceRemoval` skip or on another
    /// level), so the hole is never observed.
    pub fn share_light(
        &mut self,
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

        for level in 0..self.max_tile_level {
            for tile_x in 0..self.max_tile_x {
                for tile_z in 0..self.max_tile_z {
                    let tile = self.squares[level as usize][tile_x as usize][tile_z as usize]
                        .take();
                    let Some(mut tile) = tile else { continue };

                    if let Some(wall) = tile.wall.as_mut() {
                        if let Some(SceneModel::Model(model1)) = wall.model1.as_deref_mut() {
                            if model1.point_normal.is_some() {
                                self.share_light_loc(level, tile_x, tile_z, 1, 1, model1);
                                if let Some(SceneModel::Model(model2)) = wall.model2.as_deref_mut() {
                                    if model2.point_normal.is_some() {
                                        self.share_light_loc(level, tile_x, tile_z, 1, 1, model2);
                                        self.model_share_light(model1, model2, 0, 0, 0, false);
                                        model2.light(ambient, attenuation, light_src_x, light_src_y, light_src_z);
                                    }
                                }
                                model1.light(ambient, attenuation, light_src_x, light_src_y, light_src_z);
                            }
                        }
                    }

                    for i in 0..tile.sprite_count as usize {
                        let Some(idx) = tile.sprites[i] else { continue };
                        let sprite = self.sprites[idx].take();
                        let Some(mut sprite) = sprite else { continue };
                        if let Some(SceneModel::Model(model)) = sprite.model.as_mut() {
                            if model.point_normal.is_some() {
                                self.share_light_loc(
                                    level,
                                    tile_x,
                                    tile_z,
                                    sprite.max_tile_x + 1 - sprite.min_tile_x,
                                    sprite.max_tile_z - sprite.min_tile_z + 1,
                                    model,
                                );
                                model.light(ambient, attenuation, light_src_x, light_src_y, light_src_z);
                            }
                        }
                        self.sprites[idx] = Some(sprite);
                    }

                    if let Some(gd) = tile.ground_decor.as_mut() {
                        if let Some(SceneModel::Model(model)) = gd.model.as_deref_mut() {
                            if model.point_normal.is_some() {
                                self.share_light_gd(level, tile_x, tile_z, model);
                                model.light(ambient, attenuation, light_src_x, light_src_y, light_src_z);
                            }
                        }
                    }

                    self.squares[level as usize][tile_x as usize][tile_z as usize] = Some(tile);
                }
            }
        }
    }

    /// `shareLightGd(level, tileX, tileZ, model)` from World.ts 630-658:
    /// merge the ground-decor model with the four diagonal/east/south
    /// neighbours' ground-decor models. The neighbour bounds checks are
    /// replicated verbatim (including the TS `tileZ < maxTileX` quirk at
    /// 638); the world is square so they coincide, and `take_square` keeps
    /// an OOB `tileZ + 1` a no-op instead of a TS typed-array miss.
    fn share_light_gd(&mut self, level: i32, tile_x: i32, tile_z: i32, model: &mut Model) {
        if tile_x < self.max_tile_x {
            let mut tile = self.take_square(level, tile_x + 1, tile_z);
            if let Some(tile) = tile.as_mut() {
                let mut gd = tile.ground_decor.take();
                if let Some(SceneModel::Model(model_b)) =
                    gd.as_mut().and_then(|g| g.model.as_deref_mut())
                {
                    if model_b.point_normal.is_some() {
                        self.model_share_light(model, model_b, 128, 0, 0, true);
                    }
                }
                tile.ground_decor = gd;
            }
            self.put_square(level, tile_x + 1, tile_z, tile);
        }

        if tile_z < self.max_tile_x {
            let mut tile = self.take_square(level, tile_x, tile_z + 1);
            if let Some(tile) = tile.as_mut() {
                let mut gd = tile.ground_decor.take();
                if let Some(SceneModel::Model(model_b)) =
                    gd.as_mut().and_then(|g| g.model.as_deref_mut())
                {
                    if model_b.point_normal.is_some() {
                        self.model_share_light(model, model_b, 0, 0, 128, true);
                    }
                }
                tile.ground_decor = gd;
            }
            self.put_square(level, tile_x, tile_z + 1, tile);
        }

        if tile_x < self.max_tile_x && tile_z < self.max_tile_z {
            let mut tile = self.take_square(level, tile_x + 1, tile_z + 1);
            if let Some(tile) = tile.as_mut() {
                let mut gd = tile.ground_decor.take();
                if let Some(SceneModel::Model(model_b)) =
                    gd.as_mut().and_then(|g| g.model.as_deref_mut())
                {
                    if model_b.point_normal.is_some() {
                        self.model_share_light(model, model_b, 128, 0, 128, true);
                    }
                }
                tile.ground_decor = gd;
            }
            self.put_square(level, tile_x + 1, tile_z + 1, tile);
        }

        if tile_x < self.max_tile_x && tile_z > 0 {
            let mut tile = self.take_square(level, tile_x + 1, tile_z - 1);
            if let Some(tile) = tile.as_mut() {
                let mut gd = tile.ground_decor.take();
                if let Some(SceneModel::Model(model_b)) =
                    gd.as_mut().and_then(|g| g.model.as_deref_mut())
                {
                    if model_b.point_normal.is_some() {
                        self.model_share_light(model, model_b, 128, 0, -128, true);
                    }
                }
                tile.ground_decor = gd;
            }
            self.put_square(level, tile_x + 1, tile_z - 1, tile);
        }
    }

    /// Bounds-checked take of one square slot. `shareLightGd`'s neighbours
    /// can land outside the grid because the TS guards compare against
    /// `maxTileX` while indexing the `maxTileZ` axis; OOB reads return
    /// `None` like a TS typed-array miss.
    fn take_square(&mut self, level: i32, x: i32, z: i32) -> Option<Square> {
        self.squares
            .get_mut(level as usize)
            .and_then(|l| l.get_mut(x as usize))
            .and_then(|r| r.get_mut(z as usize))
            .and_then(Option::take)
    }

    fn put_square(&mut self, level: i32, x: i32, z: i32, square: Option<Square>) {
        if let Some(slot) = self
            .squares
            .get_mut(level as usize)
            .and_then(|l| l.get_mut(x as usize))
            .and_then(|r| r.get_mut(z as usize))
        {
            *slot = square;
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
            if l == self.max_tile_level {
                continue;
            }

            for x in min_tile_x..=max_tile_x {
                if x < 0 || x >= self.max_tile_x {
                    continue;
                }

                for z in min_tile_z..=max_tile_z {
                    if z < 0
                        || z >= self.max_tile_z
                        || (allow_face_removal
                            && x < max_tile_x
                            && z < max_tile_z
                            && (z >= tile_z || x == tile_x))
                    {
                        continue;
                    }

                    let offset_x = (x - tile_x) * 128 + (1 - tile_size_x) * 64;
                    let offset_z = (z - tile_z) * 128 + (1 - tile_size_z) * 64;
                    let offset_y = ((ground_h(self, l, x, z)
                        + ground_h(self, l, x + 1, z)
                        + ground_h(self, l, x, z + 1)
                        + ground_h(self, l, x + 1, z + 1))
                        / 4)
                        - ((ground_h(self, level, tile_x, tile_z)
                            + ground_h(self, level, tile_x + 1, tile_z)
                            + ground_h(self, level, tile_x, tile_z + 1)
                            + ground_h(self, level, tile_x + 1, tile_z + 1))
                            / 4);

                    let candidate =
                        self.squares[l as usize][x as usize][z as usize].take();
                    let Some(mut candidate) = candidate else { continue };

                    if let Some(wall) = candidate.wall.as_mut() {
                        if let Some(SceneModel::Model(model_b)) = wall.model1.as_deref_mut() {
                            if model_b.point_normal.is_some() {
                                self.model_share_light(
                                    model_a, model_b, offset_x, offset_y, offset_z,
                                    allow_face_removal,
                                );
                            }
                        }
                        if let Some(SceneModel::Model(model_b)) = wall.model2.as_deref_mut() {
                            if model_b.point_normal.is_some() {
                                self.model_share_light(
                                    model_a, model_b, offset_x, offset_y, offset_z,
                                    allow_face_removal,
                                );
                            }
                        }
                    }

                    for i in 0..candidate.sprite_count as usize {
                        let Some(idx) = candidate.sprites[i] else { continue };
                        let sprite = self.sprites[idx].take();
                        let Some(mut sprite) = sprite else { continue };
                        if let Some(SceneModel::Model(model_b)) = sprite.model.as_mut() {
                            if model_b.point_normal.is_some() {
                                let size_x = sprite.max_tile_x + 1 - sprite.min_tile_x;
                                let size_z = sprite.max_tile_z + 1 - sprite.min_tile_z;
                                let sx = (sprite.min_tile_x - tile_x) * 128
                                    + (size_x - tile_size_x) * 64;
                                let sz = (sprite.min_tile_z - tile_z) * 128
                                    + (size_z - tile_size_z) * 64;
                                self.model_share_light(
                                    model_a, model_b, sx, offset_y, sz, allow_face_removal,
                                );
                            }
                        }
                        self.sprites[idx] = Some(sprite);
                    }

                    self.squares[l as usize][x as usize][z as usize] = Some(candidate);
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

    /// `render2DGround(level, x, z, dst, offset, step)` from World.ts
    /// 798-856: plot one tile's minimap colours into `dst` (the 512×512
    /// minimap buffer). A `QuickGround` tile fills a solid 4×4 block with
    /// `minimap_rgb`; a `Ground` tile plots the overlay/underlay through
    /// the `MINIMAP_SHAPE`/`MINIMAP_ROTATE` masks. `offset` is the first
    /// pixel, `step` the row stride (512).
    pub fn render_2d_ground(
        &self,
        level: i32,
        x: i32,
        z: i32,
        dst: &mut [i32],
        mut offset: i32,
        step: i32,
    ) {
        let Some(tile) = self.squares[level as usize][x as usize][z as usize].as_ref() else {
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
                    dst[offset as usize] =
                        if minimap_shape[minimap_rotation[off]] == 0 { overlay } else { underlay };
                    off += 1;
                    dst[offset as usize + 1] =
                        if minimap_shape[minimap_rotation[off]] == 0 { overlay } else { underlay };
                    off += 1;
                    dst[offset as usize + 2] =
                        if minimap_shape[minimap_rotation[off]] == 0 { overlay } else { underlay };
                    off += 1;
                    dst[offset as usize + 3] =
                        if minimap_shape[minimap_rotation[off]] == 0 { overlay } else { underlay };
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

    #[allow(clippy::too_many_arguments)]
    fn set_sprite(
        &mut self,
        x: i32,
        z: i32,
        y: i32,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        tile_size_x: i32,
        tile_size_z: i32,
        model: Option<SceneModel>,
        typecode: i32,
        info: i32,
        yaw: i32,
        dynamic: bool,
    ) -> bool {
        let Some(model) = model else { return false };

        for tx in tile_x..tile_x + tile_size_x {
            for tz in tile_z..tile_z + tile_size_z {
                if tx < 0 || tz < 0 || tx >= self.max_tile_x || tz >= self.max_tile_z {
                    return false;
                }
                if let Some(tile) = &self.squares[level as usize][tx as usize][tz as usize] {
                    if tile.sprite_count >= 5 {
                        return false;
                    }
                }
            }
        }

        let index = self.sprites.len();
        self.sprites.push(Some(Sprite::new(
            level,
            y,
            x,
            z,
            Some(model),
            yaw,
            tile_x,
            tile_x + tile_size_x - 1,
            tile_z,
            tile_z + tile_size_z - 1,
            typecode,
            info,
        )));

        for tx in tile_x..tile_x + tile_size_x {
            for tz in tile_z..tile_z + tile_size_z {
                let mut spans = 0i32;
                if tx > tile_x {
                    spans |= 0x1;
                }
                if tx < tile_x + tile_size_x - 1 {
                    spans += 0x4;
                }
                if tz > tile_z {
                    spans += 0x8;
                }
                if tz < tile_z + tile_size_z - 1 {
                    spans += 0x2;
                }

                for l in (0..=level).rev() {
                    if self.squares[l as usize][tx as usize][tz as usize].is_none() {
                        self.squares[l as usize][tx as usize][tz as usize] =
                            Some(Square::new(l, tx, tz));
                    }
                }

                let tile = &mut self.squares[level as usize][tx as usize][tz as usize];
                if let Some(tile) = tile {
                    tile.sprites[tile.sprite_count as usize] = Some(index);
                    tile.sprite_span[tile.sprite_count as usize] = spans;
                    tile.sprite_spans |= spans;
                    tile.sprite_count += 1;
                }
            }
        }

        if dynamic {
            self.dynamic_sprites[self.dynamic_count as usize] = Some(index);
            self.dynamic_count += 1;
        }

        true
    }

    fn del_sprite(&mut self, index: usize) {
        let Some(sprite) = self.sprites.get(index).and_then(|s| s.as_ref()) else {
            return;
        };
        let min_x = sprite.min_tile_x;
        let max_x = sprite.max_tile_x;
        let min_z = sprite.min_tile_z;
        let max_z = sprite.max_tile_z;
        let level = sprite.level;

        for tx in min_x..=max_x {
            for tz in min_z..=max_z {
                let Some(tile) = &mut self.squares[level as usize][tx as usize][tz as usize] else {
                    continue;
                };

                for i in 0..tile.sprite_count as usize {
                    if tile.sprites[i] == Some(index) {
                        tile.sprite_count -= 1;
                        for j in i..tile.sprite_count as usize {
                            tile.sprites[j] = tile.sprites[j + 1];
                            tile.sprite_span[j] = tile.sprite_span[j + 1];
                        }
                        tile.sprites[tile.sprite_count as usize] = None;
                        break;
                    }
                }

                tile.sprite_spans = 0;
                for i in 0..tile.sprite_count as usize {
                    tile.sprite_spans |= tile.sprite_span[i];
                }
            }
        }

        self.sprites[index] = None;
    }

    // ---------------------------------------------------------------------
    // Render pass (Task 4): `updateMousePicking`, `renderAll` and every
    // helper it calls, 1:1 from `World.ts` 947-2484.
    // ---------------------------------------------------------------------

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
                        scratch[(pitch_level * 32 + yaw_level) * 53 * 53 + (dx + 26) as usize * 53
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

    /// `updateMousePicking(mouseX, mouseY)` from client-ts: arm the pick and
    /// reset the ground answer.
    pub fn update_mouse_picking(&mut self, mouse_x: i32, mouse_y: i32) {
        self.click = true;
        self.click_x = mouse_x;
        self.click_y = mouse_y;
        self.ground_x = -1;
        self.ground_z = -1;
    }

    /// `renderAll(eyeX, eyeY, eyeZ, maxLevel, eyeYaw, eyePitch)` from
    /// client-ts. The `cache` and `loop_cycle` parameters are required, not
    /// optional: the TS `sprite.model?.worldRender(...)` chain calls
    /// `ModelSource.worldRender` -> `getTempModel()`, which rebuilds
    /// player/npc/loc-anim models from the config `Cache` and
    /// `Client.loopCycle` during the pass (the brief's signature predates
    /// the scene-graph `SceneModel` enum). Task 5's `game_draw_main` passes
    /// `client.cache` and `client.loop_cycle`. `pix.hclip` is set per face
    /// as TS does (the Task 2 raster contract).
    #[allow(clippy::too_many_arguments)]
    pub fn render_all(
        &mut self,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        cache: &Cache,
        loop_cycle: i32,
        mut eye_x: i32,
        eye_y: i32,
        mut eye_z: i32,
        max_level: i32,
        eye_yaw: i32,
        eye_pitch: i32,
    ) {
        if eye_x < 0 {
            eye_x = 0;
        } else if eye_x >= self.max_tile_x * 128 {
            eye_x = self.max_tile_x * 128 - 1;
        }

        if eye_z < 0 {
            eye_z = 0;
        } else if eye_z >= self.max_tile_z * 128 {
            eye_z = self.max_tile_z * 128 - 1;
        }

        self.cycle_no += 1;
        self.camera_sin_x = Pix3D::sin_table().get(eye_pitch as usize).copied().unwrap_or(0);
        self.camera_cos_x = Pix3D::cos_table().get(eye_pitch as usize).copied().unwrap_or(0);
        self.camera_sin_y = Pix3D::sin_table().get(eye_yaw as usize).copied().unwrap_or(0);
        self.camera_cos_y = Pix3D::cos_table().get(eye_yaw as usize).copied().unwrap_or(0);

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
        if self.max_x > self.max_tile_x {
            self.max_x = self.max_tile_x;
        }

        self.max_z = self.gz + 25;
        if self.max_z > self.max_tile_z {
            self.max_z = self.max_tile_z;
        }

        self.calc_occlude();
        self.fill_left = 0;

        // Mark every tile in the camera's 51×51 window drawable this frame.
        for level in self.min_level..self.max_tile_level {
            for x in self.min_x..self.max_x {
                for z in self.min_z..self.max_z {
                    let tile = tile_at(&self.squares, level, x, z);
                    let Some(tile) = tile else {
                        continue;
                    };

                    let visible = tile.draw_level <= max_level
                        && (vis_backing_at(self, x + 25 - self.gx, z + 25 - self.gz)
                            || ground_h(self, level, x, z) - eye_y >= 2000);
                    if let Some(tile) = tile_at_mut(&mut self.squares, level, x, z) {
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

        // Two fill passes, nearest-to-farthest ring order (`true` then
        // `false` for `checkAdjacent`), aborting when every tile is drawn.
        for level in self.min_level..self.max_tile_level {
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
                            let tile = tile_at(&self.squares, level, right_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, right_tile_x, forward_tile_z), true);
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile = tile_at(&self.squares, level, right_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, right_tile_x, backward_tile_z), true);
                            }
                        }
                    }

                    if left_tile_x < self.max_x {
                        if forward_tile_z >= self.min_z {
                            let tile = tile_at(&self.squares, level, left_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, left_tile_x, forward_tile_z), true);
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile = tile_at(&self.squares, level, left_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, left_tile_x, backward_tile_z), true);
                            }
                        }
                    }

                    if self.fill_left == 0 {
                        self.click = false;
                        return;
                    }
                }
            }
        }

        for level in self.min_level..self.max_tile_level {
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
                            let tile = tile_at(&self.squares, level, right_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, right_tile_x, forward_tile_z), false);
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile = tile_at(&self.squares, level, right_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, right_tile_x, backward_tile_z), false);
                            }
                        }
                    }

                    if left_tile_x < self.max_x {
                        if forward_tile_z >= self.min_z {
                            let tile = tile_at(&self.squares, level, left_tile_x, forward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, left_tile_x, forward_tile_z), false);
                            }
                        }

                        if backward_tile_z < self.max_z {
                            let tile = tile_at(&self.squares, level, left_tile_x, backward_tile_z);
                            if tile.is_some_and(|t| t.draw_front) {
                                self.fill(pix, surface, cache, loop_cycle, (level, left_tile_x, backward_tile_z), false);
                            }
                        }
                    }

                    if self.fill_left == 0 {
                        self.click = false;
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
        if self.ground_x != -1 {
            self.click = false;
        }
    }

    /// `calcOcclude()` from client-ts: pick the occluders whose tiles are
    /// visible this frame and pre-compute their frustum-edge deltas. The TS
    /// `World.activeOccluders` holds object references; here they are arena
    /// indices into `occluders`.
    fn calc_occlude(&mut self) {
        let level = self.max_level;
        if level < 0 || level as usize >= OCCLUDER_LEVELS {
            return;
        }
        let count = self.num_occluders[level as usize];
        self.num_active_occluders = 0;

        'occluder: for i in 0..count as usize {
            let index = level as usize * MAX_OCCLUDERS + i;
            let Some(occluder) = self.occluders.get(index).and_then(|o| o.as_ref()) else {
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

                        min_delta_z = ((min_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_z = ((max_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        min_delta_y = ((min_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_y = ((max_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
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

                        min_delta_x = ((min_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_x = ((max_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        min_delta_y = ((min_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
                        max_delta_y = ((max_y - self.cy).wrapping_shl(8)).wrapping_div(delta_max_tile_x);
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
                            min_delta_x = ((min_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_y);
                            max_delta_x = ((max_x - self.cx).wrapping_shl(8)).wrapping_div(delta_max_y);
                            min_delta_z = ((min_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_y);
                            max_delta_z = ((max_z - self.cz).wrapping_shl(8)).wrapping_div(delta_max_y);
                            active = true;
                        }
                    }
                }
            }

            if active {
                let occluder = self.occluders.get_mut(index).and_then(|o| o.as_mut());
                if let Some(occluder) = occluder {
                    occluder.mode = mode;
                    occluder.min_delta_x = min_delta_x;
                    occluder.max_delta_x = max_delta_x;
                    occluder.min_delta_z = min_delta_z;
                    occluder.max_delta_z = max_delta_z;
                    occluder.min_delta_y = min_delta_y;
                    occluder.max_delta_y = max_delta_y;
                }
                if let Some(slot) = self.active_occluders.get_mut(self.num_active_occluders as usize) {
                    *slot = Some(index);
                }
                self.num_active_occluders += 1;
            }
        }
    }

    /// Java/TS `LinkList.push`: if the square is already queued, unlink it
    /// and append at the tail. Stamping the square and the deque entry makes
    /// the older copy a no-op when popped.
    fn enqueue_fill(&mut self, level: i32, x: i32, z: i32) {
        self.fill_gen = self.fill_gen.wrapping_add(1);
        if self.fill_gen == 0 {
            self.fill_gen = 1;
        }
        let stamp = self.fill_gen;
        if let Some(tile) = tile_at_mut(&mut self.squares, level, x, z) {
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
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        cache: &Cache,
        loop_cycle: i32,
        next: (i32, i32, i32),
        mut check_adjacent: bool,
    ) {
        self.enqueue_fill(next.0, next.1, next.2);

        'fill: loop {
            // `do { tile = popFront(); if (!tile) return; } while (!tile.drawBack)`
            let (tile_x, tile_z, level) = loop {
                let Some((level, x, z, stamp)) = self.fill_queue.pop_front() else {
                    return;
                };
                let tile = tile_at(&self.squares, level, x, z);
                if !tile.is_some_and(|t| t.draw_back && t.fill_stamp == stamp) {
                    continue;
                }
                if let Some(tile) = tile_at_mut(&mut self.squares, level, x, z) {
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
            let original_level = tile_at(&self.squares, level, tile_x, tile_z)
                .map(|t| t.original_level)
                .unwrap_or(level);

            let draw_front = tile_at(&self.squares, level, tile_x, tile_z)
                .map(|t| t.draw_front)
                .unwrap_or(false);

            if draw_front {
                if check_adjacent {
                    if level > 0 {
                        let above = tile_at(&self.squares, level - 1, tile_x, tile_z);
                        if above.is_some_and(|t| t.draw_back) {
                            continue 'fill;
                        }
                    }

                    let sprite_spans = tile_at(&self.squares, level, tile_x, tile_z)
                        .map(|t| t.sprite_spans)
                        .unwrap_or(0);

                    if tile_x <= gx && tile_x > min_x {
                        let adjacent = tile_at(&self.squares, level, tile_x - 1, tile_z);
                        if adjacent.is_some_and(|t| t.draw_back && (t.draw_front || (sprite_spans & 0x1) == 0)) {
                            continue 'fill;
                        }
                    }

                    if tile_x >= gx && tile_x < max_x - 1 {
                        let adjacent = tile_at(&self.squares, level, tile_x + 1, tile_z);
                        if adjacent.is_some_and(|t| t.draw_back && (t.draw_front || (sprite_spans & 0x4) == 0)) {
                            continue 'fill;
                        }
                    }

                    if tile_z <= gz && tile_z > min_z {
                        let adjacent = tile_at(&self.squares, level, tile_x, tile_z - 1);
                        if adjacent.is_some_and(|t| t.draw_back && (t.draw_front || (sprite_spans & 0x8) == 0)) {
                            continue 'fill;
                        }
                    }

                    if tile_z >= gz && tile_z < max_z - 1 {
                        let adjacent = tile_at(&self.squares, level, tile_x, tile_z + 1);
                        if adjacent.is_some_and(|t| t.draw_back && (t.draw_front || (sprite_spans & 0x2) == 0)) {
                            continue 'fill;
                        }
                    }
                } else {
                    check_adjacent = true;
                }

                if let Some(tile) = tile_at_mut(&mut self.squares, level, tile_x, tile_z) {
                    tile.draw_front = false;
                }

                // Linked square (a level pushed down under this tile).
                let linked_quick = tile_at(&self.squares, level, tile_x, tile_z)
                    .and_then(|t| t.linked_square.as_ref())
                    .and_then(|ls| ls.quick_ground);
                if let Some(quick) = linked_quick {
                    if !self.ground_occluded(0, tile_x, tile_z) {
                        self.render_quick_ground(pix, surface, quick, 0, tile_x, tile_z, sin_pitch, cos_pitch, sin_yaw, cos_yaw);
                    }
                } else {
                    let linked_ground = tile_at(&self.squares, level, tile_x, tile_z)
                        .and_then(|t| t.linked_square.as_ref())
                        .and_then(|ls| ls.ground.clone());
                    if let Some(ground) = linked_ground {
                        if !self.ground_occluded(0, tile_x, tile_z) {
                            self.render_ground(pix, surface, tile_x, tile_z, ground, sin_pitch, cos_pitch, sin_yaw, cos_yaw);
                        }
                    }
                }

                {
                    let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                    if let Some(wall) = tile.and_then(|t| t.linked_square.as_mut()).and_then(|ls| ls.wall.as_mut()) {
                        if let Some(model) = wall.model1.as_mut() {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, wall.x - cx, wall.y - cy, wall.z - cz, wall.typecode);
                        }
                    }
                }

                let linked_sprites: Vec<usize> = tile_at(&self.squares, level, tile_x, tile_z)
                    .and_then(|t| t.linked_square.as_ref())
                    .map(|ls| {
                        (0..ls.sprite_count as usize).filter_map(|i| ls.sprites[i]).collect()
                    })
                    .unwrap_or_default();
                for index in linked_sprites {
                    if let Some(sprite) = self.sprites.get_mut(index).and_then(|s| s.as_mut()) {
                        if let Some(model) = sprite.model.as_mut() {
                            model.world_render(cache, loop_cycle, pix, surface, sprite.yaw, sin_pitch, cos_pitch, sin_yaw, cos_yaw, sprite.x - cx, sprite.y - cy, sprite.z - cz, sprite.typecode);
                        }
                    }
                }

                // The tile's own ground.
                let mut tile_drawn = false;
                let quick = tile_at(&self.squares, level, tile_x, tile_z)
                    .and_then(|t| t.quick_ground);
                if let Some(quick) = quick {
                    if !self.ground_occluded(original_level, tile_x, tile_z) {
                        tile_drawn = true;
                        self.render_quick_ground(pix, surface, quick, original_level, tile_x, tile_z, sin_pitch, cos_pitch, sin_yaw, cos_yaw);
                    }
                } else {
                    let ground = tile_at(&self.squares, level, tile_x, tile_z)
                        .and_then(|t| t.ground.clone());
                    if let Some(ground) = ground {
                        if !self.ground_occluded(original_level, tile_x, tile_z) {
                            tile_drawn = true;
                            self.render_ground(pix, surface, tile_x, tile_z, ground, sin_pitch, cos_pitch, sin_yaw, cos_yaw);
                        }
                    }
                }

                // `direction`/`frontWallTypes`/`backWallTypes` from the
                // camera-relative tile position.
                let mut direction = 0i32;
                let mut front_wall_types = 0i32;
                let has_wall_or_decor = tile_at(&self.squares, level, tile_x, tile_z)
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
                    if let Some(tile) = tile_at_mut(&mut self.squares, level, tile_x, tile_z) {
                        tile.back_wall_types = POSTTAB.get(direction as usize).copied().unwrap_or(0);
                    }
                }

                // Wall corner-sides bookkeeping and the front wall renders.
                let wall_data = tile_at(&self.squares, level, tile_x, tile_z)
                    .and_then(|t| t.wall.as_ref())
                    .map(|w| (w.angle1, w.angle2, w.typecode, w.x - cx, w.y - cy, w.z - cz));
                if let Some((angle1, angle2, typecode, wall_x, wall_y, wall_z)) = wall_data {
                    if let Some(tile) = tile_at_mut(&mut self.squares, level, tile_x, tile_z) {
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

                    if (angle1 & front_wall_types) != 0 && !self.wall_occluded(original_level, tile_x, tile_z, angle1) {
                        let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                        if let Some(model) = tile.and_then(|t| t.wall.as_mut()).and_then(|w| w.model1.as_mut()) {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, wall_x, wall_y, wall_z, typecode);
                        }
                    }

                    if (angle2 & front_wall_types) != 0 && !self.wall_occluded(original_level, tile_x, tile_z, angle2) {
                        let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                        if let Some(model) = tile.and_then(|t| t.wall.as_mut()).and_then(|w| w.model2.as_mut()) {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, wall_x, wall_y, wall_z, typecode);
                        }
                    }
                }

                // Decor on the near side of the tile.
                let decor_data = tile_at(&self.squares, level, tile_x, tile_z)
                    .and_then(|t| t.decor.as_ref())
                    .map(|d| {
                        (
                            d.wshape,
                            d.angle,
                            d.typecode,
                            d.x - cx,
                            d.y - cy,
                            d.z - cz,
                            d.model.min_y(),
                        )
                    });
                if let Some((wshape, angle, typecode, decor_x, decor_y, decor_z, min_y)) = decor_data {
                    if !self.sprite_occluded(original_level, tile_x, tile_z, min_y) {
                        if (wshape & front_wall_types) != 0 {
                            let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                            if let Some(decor) = tile.and_then(|t| t.decor.as_mut()) {
                                decor.model.world_render(cache, loop_cycle, pix, surface, angle, sin_pitch, cos_pitch, sin_yaw, cos_yaw, decor_x, decor_y, decor_z, typecode);
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
                                let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                                if let Some(decor) = tile.and_then(|t| t.decor.as_mut()) {
                                    decor.model.world_render(cache, loop_cycle, pix, surface, angle * 512 + 256, sin_pitch, cos_pitch, sin_yaw, cos_yaw, draw_x, decor_y, draw_z, typecode);
                                }
                            }

                            if (wshape & 0x200) != 0 && nearest_z > nearest_x {
                                let draw_x = decor_x + DECORXOF2.get(angle as usize).copied().unwrap_or(0);
                                let draw_z = decor_z + DECORZOF2.get(angle as usize).copied().unwrap_or(0);
                                let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                                if let Some(decor) = tile.and_then(|t| t.decor.as_mut()) {
                                    decor.model.world_render(cache, loop_cycle, pix, surface, (angle * 512 + 1280) & 0x7ff, sin_pitch, cos_pitch, sin_yaw, cos_yaw, draw_x, decor_y, draw_z, typecode);
                                }
                            }
                        }
                    }
                }

                // Ground decor + ground objects (stack height 0) on a drawn tile.
                if tile_drawn {
                    let ground_decor_data = tile_at(&self.squares, level, tile_x, tile_z)
                        .and_then(|t| t.ground_decor.as_ref())
                        .map(|gd| (gd.typecode, gd.x - cx, gd.y - cy, gd.z - cz));
                    if let Some((typecode, gd_x, gd_y, gd_z)) = ground_decor_data {
                        let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                        if let Some(model) = tile.and_then(|t| t.ground_decor.as_mut()).and_then(|gd| gd.model.as_mut()) {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, gd_x, gd_y, gd_z, typecode);
                        }
                    }

                    let ground_object_data = tile_at(&self.squares, level, tile_x, tile_z)
                        .and_then(|t| t.ground_object.as_ref())
                        .map(|o| (o.height, o.typecode, o.x - cx, o.y - cy, o.z - cz));
                    if let Some((height, typecode, ox, oy, oz)) = ground_object_data {
                        if height == 0 {
                            let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                            if let Some(objs) = tile.and_then(|t| t.ground_object.as_mut()) {
                                if let Some(model) = objs.bottom_obj.as_mut() {
                                    model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, ox, oy, oz, typecode);
                                }

                                if let Some(model) = objs.middle_obj.as_mut() {
                                    model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, ox, oy, oz, typecode);
                                }

                                if let Some(model) = objs.top_obj.as_mut() {
                                    model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, ox, oy, oz, typecode);
                                }
                            }
                        }
                    }
                }

                // Sprite-span adjacency: queue tiles the sprite covers.
                let spans = tile_at(&self.squares, level, tile_x, tile_z)
                    .map(|t| t.sprite_spans)
                    .unwrap_or(0);
                if spans != 0 {
                    if tile_x < gx && (spans & 0x4) != 0 {
                        let adjacent = tile_at(&self.squares, level, tile_x + 1, tile_z);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(level, tile_x + 1, tile_z);
                        }
                    }

                    if tile_z < gz && (spans & 0x2) != 0 {
                        let adjacent = tile_at(&self.squares, level, tile_x, tile_z + 1);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(level, tile_x, tile_z + 1);
                        }
                    }

                    if tile_x > gx && (spans & 0x1) != 0 {
                        let adjacent = tile_at(&self.squares, level, tile_x - 1, tile_z);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(level, tile_x - 1, tile_z);
                        }
                    }

                    if tile_z > gz && (spans & 0x8) != 0 {
                        let adjacent = tile_at(&self.squares, level, tile_x, tile_z - 1);
                        if adjacent.is_some_and(|t| t.draw_back) {
                            self.enqueue_fill(level, tile_x, tile_z - 1);
                        }
                    }
                }
            }

            // Corner-side walls draw after every sprite slot on the tile has
            // been considered.
            let (corner_sides, sprite_pairs, sides_before, sides_after) = tile_at(&self.squares, level, tile_x, tile_z)
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
                    let Some(sprite) = self.sprites.get(*sprite_index).and_then(|s| s.as_ref()) else {
                        continue;
                    };

                    if sprite.cycle != cycle_no && (span & corner_sides) == sides_before {
                        draw = false;
                        break;
                    }
                }

                if draw {
                    let wall_data = tile_at(&self.squares, level, tile_x, tile_z)
                        .and_then(|t| t.wall.as_ref())
                        .map(|w| (w.angle1, w.typecode, w.x - cx, w.y - cy, w.z - cz));
                    if let Some((angle1, typecode, wall_x, wall_y, wall_z)) = wall_data {
                        if !self.wall_occluded(original_level, tile_x, tile_z, angle1) {
                            let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                            if let Some(model) = tile.and_then(|t| t.wall.as_mut()).and_then(|w| w.model1.as_mut()) {
                                model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, wall_x, wall_y, wall_z, typecode);
                            }
                        }
                    }

                    if let Some(tile) = tile_at_mut(&mut self.squares, level, tile_x, tile_z) {
                        tile.corner_sides = 0;
                    }
                }
            }

            // Sprite drawing: buffer this tile's sprites (farthest first),
            // render each once per cycle, and requeue the tiles they cover.
            let mut draw_sprites = tile_at(&self.squares, level, tile_x, tile_z)
                .map(|t| t.draw_sprites)
                .unwrap_or(false);
            if draw_sprites {
                let sprite_count = tile_at(&self.squares, level, tile_x, tile_z)
                    .map(|t| t.sprite_count)
                    .unwrap_or(0);
                if let Some(tile) = tile_at_mut(&mut self.squares, level, tile_x, tile_z) {
                    tile.draw_sprites = false;
                }
                let mut sprite_buffer_size = 0i32;

                'iterate_sprites: for i in 0..sprite_count as usize {
                    let sprite_index = tile_at(&self.squares, level, tile_x, tile_z)
                        .and_then(|t| t.sprites.get(i).copied())
                        .flatten();
                    let Some(sprite_index) = sprite_index else {
                        continue;
                    };
                    let Some(sprite) = self.sprites.get(sprite_index).and_then(|s| s.as_ref()) else {
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
                            let Some(other) = tile_at(&self.squares, level, x, z) else {
                                continue;
                            };

                            if other.draw_front {
                                draw_sprites = true;
                                if let Some(tile) = tile_at_mut(&mut self.squares, level, tile_x, tile_z) {
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
                    if let Some(sprite) = self.sprites.get_mut(sprite_index).and_then(|s| s.as_mut()) {
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
                        let Some(sprite) = self.sprites.get(sprite).and_then(|s| s.as_ref()) else {
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
                    if let Some(sprite) = self.sprites.get_mut(farthest).and_then(|s| s.as_mut()) {
                        sprite.cycle = cycle_no;
                    }

                    let Some(sprite) = self.sprites.get(farthest).and_then(|s| s.as_ref()) else {
                        continue;
                    };
                    let min_x = sprite.min_tile_x;
                    let max_x = sprite.max_tile_x;
                    let min_z = sprite.min_tile_z;
                    let max_z = sprite.max_tile_z;
                    let model_min_y = sprite.model.as_ref().map(|m| m.min_y()).unwrap_or(0);

                    if !self.sprite_occluded2(original_level, min_x, max_x, min_z, max_z, model_min_y) {
                        if let Some(sprite) = self.sprites.get_mut(farthest).and_then(|s| s.as_mut()) {
                            if let Some(model) = sprite.model.as_mut() {
                                model.world_render(cache, loop_cycle, pix, surface, sprite.yaw, sin_pitch, cos_pitch, sin_yaw, cos_yaw, sprite.x - cx, sprite.y - cy, sprite.z - cz, sprite.typecode);
                            }
                        }
                    }

                    for x in min_x..=max_x {
                        for z in min_z..=max_z {
                            let Some(occupied) = tile_at(&self.squares, level, x, z) else {
                                continue;
                            };
                            let corner_sides = occupied.corner_sides;
                            let draw_back = occupied.draw_back;

                            if corner_sides != 0 {
                                self.enqueue_fill(level, x, z);
                            } else if (x != tile_x || z != tile_z) && draw_back {
                                self.enqueue_fill(level, x, z);
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
            let (draw_back, corner_sides) = tile_at(&self.squares, level, tile_x, tile_z)
                .map(|t| (t.draw_back, t.corner_sides))
                .unwrap_or((false, 0));
            if !draw_back || corner_sides != 0 {
                continue 'fill;
            }

            let mut stuck = Vec::new();
            let mut blocked = false;
            if tile_x <= gx && tile_x > min_x {
                if let Some(adjacent) = tile_at(&self.squares, level, tile_x - 1, tile_z) {
                    if adjacent.draw_back {
                        blocked = true;
                        if !adjacent.draw_front {
                            stuck.push((level, tile_x - 1, tile_z));
                        }
                    }
                }
            }

            if tile_x >= gx && tile_x < max_x - 1 {
                if let Some(adjacent) = tile_at(&self.squares, level, tile_x + 1, tile_z) {
                    if adjacent.draw_back {
                        blocked = true;
                        if !adjacent.draw_front {
                            stuck.push((level, tile_x + 1, tile_z));
                        }
                    }
                }
            }

            if tile_z <= gz && tile_z > min_z {
                if let Some(adjacent) = tile_at(&self.squares, level, tile_x, tile_z - 1) {
                    if adjacent.draw_back {
                        blocked = true;
                        if !adjacent.draw_front {
                            stuck.push((level, tile_x, tile_z - 1));
                        }
                    }
                }
            }

            if tile_z >= gz && tile_z < max_z - 1 {
                if let Some(adjacent) = tile_at(&self.squares, level, tile_x, tile_z + 1) {
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
                    self.enqueue_fill(bl, bx, bz);
                }
                continue 'fill;
            }

            if let Some(tile) = tile_at_mut(&mut self.squares, level, tile_x, tile_z) {
                tile.draw_back = false;
            }
            self.fill_left -= 1;

            // Stacked ground objects (height != 0) render once the tile
            // behind them is done.
            let ground_object_data = tile_at(&self.squares, level, tile_x, tile_z)
                .and_then(|t| t.ground_object.as_ref())
                .map(|o| (o.height, o.typecode, o.x - cx, o.y - cy, o.z - cz));
            if let Some((height, typecode, ox, oy, oz)) = ground_object_data {
                if height != 0 {
                    let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                    if let Some(objs) = tile.and_then(|t| t.ground_object.as_mut()) {
                        if let Some(model) = objs.bottom_obj.as_mut() {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, ox, oy - height, oz, typecode);
                        }

                        if let Some(model) = objs.middle_obj.as_mut() {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, ox, oy - height, oz, typecode);
                        }

                        if let Some(model) = objs.top_obj.as_mut() {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, ox, oy - height, oz, typecode);
                        }
                    }
                }
            }

            // Back-wall decor + walls, drawn after the tile drops.
            let back_wall_types = tile_at(&self.squares, level, tile_x, tile_z)
                .map(|t| t.back_wall_types)
                .unwrap_or(0);
            if back_wall_types != 0 {
                let decor_data = tile_at(&self.squares, level, tile_x, tile_z)
                    .and_then(|t| t.decor.as_ref())
                    .map(|d| {
                        (
                            d.wshape,
                            d.angle,
                            d.typecode,
                            d.x - cx,
                            d.y - cy,
                            d.z - cz,
                            d.model.min_y(),
                        )
                    });
                if let Some((wshape, angle, typecode, decor_x, decor_y, decor_z, min_y)) = decor_data {
                    if !self.sprite_occluded(original_level, tile_x, tile_z, min_y) {
                        if (wshape & back_wall_types) != 0 {
                            let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                            if let Some(decor) = tile.and_then(|t| t.decor.as_mut()) {
                                decor.model.world_render(cache, loop_cycle, pix, surface, angle, sin_pitch, cos_pitch, sin_yaw, cos_yaw, decor_x, decor_y, decor_z, typecode);
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
                                let draw_x = decor_x + DECORXOF.get(angle as usize).copied().unwrap_or(0);
                                let draw_z = decor_z + DECORZOF.get(angle as usize).copied().unwrap_or(0);
                                let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                                if let Some(decor) = tile.and_then(|t| t.decor.as_mut()) {
                                    decor.model.world_render(cache, loop_cycle, pix, surface, angle * 512 + 256, sin_pitch, cos_pitch, sin_yaw, cos_yaw, draw_x, decor_y, draw_z, typecode);
                                }
                            }

                            if (wshape & 0x200) != 0 && nearest_z <= nearest_x {
                                let draw_x = decor_x + DECORXOF2.get(angle as usize).copied().unwrap_or(0);
                                let draw_z = decor_z + DECORZOF2.get(angle as usize).copied().unwrap_or(0);
                                let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                                if let Some(decor) = tile.and_then(|t| t.decor.as_mut()) {
                                    decor.model.world_render(cache, loop_cycle, pix, surface, (angle * 512 + 1280) & 0x7ff, sin_pitch, cos_pitch, sin_yaw, cos_yaw, draw_x, decor_y, draw_z, typecode);
                                }
                            }
                        }
                    }
                }

                let wall_data = tile_at(&self.squares, level, tile_x, tile_z)
                    .and_then(|t| t.wall.as_ref())
                    .map(|w| (w.angle1, w.angle2, w.typecode, w.x - cx, w.y - cy, w.z - cz));
                if let Some((angle1, angle2, typecode, wall_x, wall_y, wall_z)) = wall_data {
                    if (angle2 & back_wall_types) != 0 && !self.wall_occluded(original_level, tile_x, tile_z, angle2) {
                        let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                        if let Some(model) = tile.and_then(|t| t.wall.as_mut()).and_then(|w| w.model2.as_mut()) {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, wall_x, wall_y, wall_z, typecode);
                        }
                    }

                    if (angle1 & back_wall_types) != 0 && !self.wall_occluded(original_level, tile_x, tile_z, angle1) {
                        let tile = tile_at_mut(&mut self.squares, level, tile_x, tile_z);
                        if let Some(model) = tile.and_then(|t| t.wall.as_mut()).and_then(|w| w.model1.as_mut()) {
                            model.world_render(cache, loop_cycle, pix, surface, 0, sin_pitch, cos_pitch, sin_yaw, cos_yaw, wall_x, wall_y, wall_z, typecode);
                        }
                    }
                }
            }

            // Queue the level above and the four neighbours.
            if level < self.max_tile_level - 1 {
                let above = tile_at(&self.squares, level + 1, tile_x, tile_z);
                if above.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(level + 1, tile_x, tile_z);
                }
            }

            if tile_x < gx {
                let adjacent = tile_at(&self.squares, level, tile_x + 1, tile_z);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(level, tile_x + 1, tile_z);
                }
            }

            if tile_z < gz {
                let adjacent = tile_at(&self.squares, level, tile_x, tile_z + 1);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(level, tile_x, tile_z + 1);
                }
            }

            if tile_x > gx {
                let adjacent = tile_at(&self.squares, level, tile_x - 1, tile_z);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(level, tile_x - 1, tile_z);
                }
            }

            if tile_z > gz {
                let adjacent = tile_at(&self.squares, level, tile_x, tile_z - 1);
                if adjacent.is_some_and(|t| t.draw_back) {
                    self.enqueue_fill(level, tile_x, tile_z - 1);
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

        let mut y0 = ground_h(self, level, tile_x, tile_z) - self.cy;
        let mut y1 = ground_h(self, level, tile_x + 1, tile_z) - self.cy;
        let mut y2 = ground_h(self, level, tile_x + 1, tile_z + 1) - self.cy;
        let mut y3 = ground_h(self, level, tile_x, tile_z + 1) - self.cy;

        let mut tmp = (z0.wrapping_mul(sin_eye_yaw).wrapping_add(x0.wrapping_mul(cos_eye_yaw))) >> 16;
        z0 = (z0.wrapping_mul(cos_eye_yaw).wrapping_sub(x0.wrapping_mul(sin_eye_yaw))) >> 16;
        x0 = tmp;

        tmp = (y0.wrapping_mul(cos_eye_pitch).wrapping_sub(z0.wrapping_mul(sin_eye_pitch))) >> 16;
        z0 = (y0.wrapping_mul(sin_eye_pitch).wrapping_add(z0.wrapping_mul(cos_eye_pitch))) >> 16;
        y0 = tmp;

        if z0 < 50 {
            return;
        }

        tmp = (z1.wrapping_mul(sin_eye_yaw).wrapping_add(x1.wrapping_mul(cos_eye_yaw))) >> 16;
        z1 = (z1.wrapping_mul(cos_eye_yaw).wrapping_sub(x1.wrapping_mul(sin_eye_yaw))) >> 16;
        x1 = tmp;

        tmp = (y1.wrapping_mul(cos_eye_pitch).wrapping_sub(z1.wrapping_mul(sin_eye_pitch))) >> 16;
        z1 = (y1.wrapping_mul(sin_eye_pitch).wrapping_add(z1.wrapping_mul(cos_eye_pitch))) >> 16;
        y1 = tmp;

        if z1 < 50 {
            return;
        }

        tmp = (z2.wrapping_mul(sin_eye_yaw).wrapping_add(x2.wrapping_mul(cos_eye_yaw))) >> 16;
        z2 = (z2.wrapping_mul(cos_eye_yaw).wrapping_sub(x2.wrapping_mul(sin_eye_yaw))) >> 16;
        x2 = tmp;

        tmp = (y2.wrapping_mul(cos_eye_pitch).wrapping_sub(z2.wrapping_mul(sin_eye_pitch))) >> 16;
        z2 = (y2.wrapping_mul(sin_eye_pitch).wrapping_add(z2.wrapping_mul(cos_eye_pitch))) >> 16;
        y2 = tmp;

        if z2 < 50 {
            return;
        }

        tmp = (z3.wrapping_mul(sin_eye_yaw).wrapping_add(x3.wrapping_mul(cos_eye_yaw))) >> 16;
        z3 = (z3.wrapping_mul(cos_eye_yaw).wrapping_sub(x3.wrapping_mul(sin_eye_yaw))) >> 16;
        x3 = tmp;

        tmp = (y3.wrapping_mul(cos_eye_pitch).wrapping_sub(z3.wrapping_mul(sin_eye_pitch))) >> 16;
        z3 = (y3.wrapping_mul(sin_eye_pitch).wrapping_add(z3.wrapping_mul(cos_eye_pitch))) >> 16;
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

        if crate::dash3d::wrapping_cross(py1.wrapping_sub(px3), px1.wrapping_sub(py3), pz1.wrapping_sub(py3), pz0.wrapping_sub(px3)) > 0 {
            pix.hclip = py1 < 0 || px3 < 0 || pz0 < 0 || py1 > surface.size_x || px3 > surface.size_x || pz0 > surface.size_x;

            if self.click && inside_triangle(self.click_x, self.click_y, pz1, py3, px1, py1, px3, pz0) {
                self.ground_x = tile_x;
                self.ground_z = tile_z;
            }

            if ground.texture != -1 {
                if !pix.low_mem {
                    if ground.flat {
                        pix.texture_triangle(
                            surface,
                            py1, px3, pz0,
                            pz1, py3, px1,
                            ground.colour_ne, ground.colour_nw, ground.colour_se,
                            x0, y0, z0,
                            x1, x3,
                            y1, y3,
                            z1, z3,
                            ground.texture,
                        );
                    } else {
                        pix.texture_triangle(
                            surface,
                            py1, px3, pz0,
                            pz1, py3, px1,
                            ground.colour_ne, ground.colour_nw, ground.colour_se,
                            x2, y2, z2,
                            x3, x1,
                            y3, y1,
                            z3, z1,
                            ground.texture,
                        );
                    }
                } else {
                    let texture_average = TEXTURE_AVERAGE.get(ground.texture as usize).copied().unwrap_or(41);
                    pix.gouraud_triangle(
                        surface,
                        py1, px3, pz0,
                        pz1, py3, px1,
                        get_table(texture_average, ground.colour_ne),
                        get_table(texture_average, ground.colour_nw),
                        get_table(texture_average, ground.colour_se),
                    );
                }
            } else if ground.colour_ne != 12345678 {
                pix.gouraud_triangle(
                    surface,
                    py1, px3, pz0,
                    pz1, py3, px1,
                    ground.colour_ne, ground.colour_nw, ground.colour_se,
                );
            }
        }

        if crate::dash3d::wrapping_cross(px0.wrapping_sub(pz0), py3.wrapping_sub(px1), py0.wrapping_sub(px1), px3.wrapping_sub(pz0)) > 0 {
            pix.hclip = px0 < 0 || pz0 < 0 || px3 < 0 || px0 > surface.size_x || pz0 > surface.size_x || px3 > surface.size_x;

            if self.click && inside_triangle(self.click_x, self.click_y, py0, px1, py3, px0, pz0, px3) {
                self.ground_x = tile_x;
                self.ground_z = tile_z;
            }

            if ground.texture != -1 {
                if !pix.low_mem {
                    pix.texture_triangle(
                        surface,
                        px0, pz0, px3,
                        py0, px1, py3,
                        ground.colour_sw, ground.colour_se, ground.colour_nw,
                        x0, y0, z0,
                        x1, x3,
                        y1, y3,
                        z1, z3,
                        ground.texture,
                    );
                } else {
                    let texture_average = TEXTURE_AVERAGE.get(ground.texture as usize).copied().unwrap_or(41);
                    pix.gouraud_triangle(
                        surface,
                        px0, pz0, px3,
                        py0, px1, py3,
                        get_table(texture_average, ground.colour_sw),
                        get_table(texture_average, ground.colour_se),
                        get_table(texture_average, ground.colour_nw),
                    );
                }
            } else if ground.colour_sw != 12345678 {
                pix.gouraud_triangle(
                    surface,
                    px0, pz0, px3,
                    py0, px1, py3,
                    ground.colour_sw, ground.colour_se, ground.colour_nw,
                );
            }
        }
    }

    /// `renderGround(tileX, tileZ, ground, ...)` from client-ts 2089-2188.
    /// The `Ground.drawVertex*` TS statics are the `ground_draw_*` fields.
    #[allow(clippy::too_many_arguments)]
    fn render_ground(
        &mut self,
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
        let mut vertex_count = ground.vertex_x.len();

        for i in 0..vertex_count {
            let mut x = ground.vertex_x[i] - self.cx;
            let mut y = ground.vertex_y[i] - self.cy;
            let mut z = ground.vertex_z[i] - self.cz;

            let mut tmp = (z.wrapping_mul(sin_eye_yaw).wrapping_add(x.wrapping_mul(cos_eye_yaw))) >> 16;
            z = (z.wrapping_mul(cos_eye_yaw).wrapping_sub(x.wrapping_mul(sin_eye_yaw))) >> 16;
            x = tmp;

            tmp = (y.wrapping_mul(cos_eye_pitch).wrapping_sub(z.wrapping_mul(sin_eye_pitch))) >> 16;
            z = (y.wrapping_mul(sin_eye_pitch).wrapping_add(z.wrapping_mul(cos_eye_pitch))) >> 16;
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

        vertex_count = ground.face_vertex_a.len();
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

            if crate::dash3d::wrapping_cross(x0.wrapping_sub(x1), y2.wrapping_sub(y1), y0.wrapping_sub(y1), x2.wrapping_sub(x1)) > 0 {
                pix.hclip = x0 < 0 || x1 < 0 || x2 < 0 || x0 > surface.size_x || x1 > surface.size_x || x2 > surface.size_x;

                if self.click && inside_triangle(self.click_x, self.click_y, y0, y1, y2, x0, x1, x2) {
                    self.ground_x = tile_x;
                    self.ground_z = tile_z;
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
                                x0, x1, x2,
                                y0, y1, y2,
                                colour_a, colour_b, colour_c,
                                self.ground_draw_texture_vertex_x[0], self.ground_draw_texture_vertex_y[0], self.ground_draw_texture_vertex_z[0],
                                self.ground_draw_texture_vertex_x[1], self.ground_draw_texture_vertex_x[3],
                                self.ground_draw_texture_vertex_y[1], self.ground_draw_texture_vertex_y[3],
                                self.ground_draw_texture_vertex_z[1], self.ground_draw_texture_vertex_z[3],
                                tex,
                            );
                        } else {
                            pix.texture_triangle(
                                surface,
                                x0, x1, x2,
                                y0, y1, y2,
                                colour_a, colour_b, colour_c,
                                self.ground_draw_texture_vertex_x[a], self.ground_draw_texture_vertex_y[a], self.ground_draw_texture_vertex_z[a],
                                self.ground_draw_texture_vertex_x[b], self.ground_draw_texture_vertex_x[c],
                                self.ground_draw_texture_vertex_y[b], self.ground_draw_texture_vertex_y[c],
                                self.ground_draw_texture_vertex_z[b], self.ground_draw_texture_vertex_z[c],
                                tex,
                            );
                        }
                    } else {
                        let texture_average = TEXTURE_AVERAGE.get(tex as usize).copied().unwrap_or(41);
                        pix.gouraud_triangle(
                            surface,
                            x0, x1, x2,
                            y0, y1, y2,
                            get_table(texture_average, colour_a),
                            get_table(texture_average, colour_b),
                            get_table(texture_average, colour_c),
                        );
                    }
                } else if colour_a != 12345678 {
                    pix.gouraud_triangle(
                        surface,
                        x0, x1, x2,
                        y0, y1, y2,
                        colour_a, colour_b, colour_c,
                    );
                }
            }
        }
    }

    /// `groundOccluded(level, x, z)` from client-ts 2189-2215.
    fn ground_occluded(&mut self, level: i32, x: i32, z: i32) -> bool {
        let stride_z = self.max_tile_z + 1;
        let stride_x = self.max_tile_x + 1;
        let index = (level * stride_x + x) * stride_z + z;
        let cycle = self.occlusion_cycle.get(index as usize).copied().unwrap_or(0);
        if cycle == -self.cycle_no {
            return false;
        } else if cycle == self.cycle_no {
            return true;
        } else {
            let sx = x << 7;
            let sz = z << 7;
            if self.occluded(sx + 1, ground_h(self, level, x, z), sz + 1)
                && self.occluded(sx + 128 - 1, ground_h(self, level, x + 1, z), sz + 1)
                && self.occluded(sx + 128 - 1, ground_h(self, level, x + 1, z + 1), sz + 128 - 1)
                && self.occluded(sx + 1, ground_h(self, level, x, z + 1), sz + 128 - 1)
            {
                if let Some(slot) = self.occlusion_cycle.get_mut(index as usize) {
                    *slot = self.cycle_no;
                }
                return true;
            } else {
                if let Some(slot) = self.occlusion_cycle.get_mut(index as usize) {
                    *slot = -self.cycle_no;
                }
                return false;
            }
        }
    }

    /// `wallOccluded(level, x, z, type)` from client-ts 2216-2310.
    fn wall_occluded(&mut self, level: i32, x: i32, z: i32, r#type: i32) -> bool {
        if !self.ground_occluded(level, x, z) {
            return false;
        }

        let scene_x = x << 7;
        let scene_z = z << 7;
        let scene_y = ground_h(self, level, x, z) - 1;
        let y0 = scene_y - 120;
        let y1 = scene_y - 230;
        let y2 = scene_y - 238;
        if r#type < 16 {
            if r#type == 1 {
                if scene_x > self.cx {
                    if !self.occluded(scene_x, scene_y, scene_z) {
                        return false;
                    }
                    if !self.occluded(scene_x, scene_y, scene_z + 128) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(scene_x, y0, scene_z) {
                        return false;
                    }
                    if !self.occluded(scene_x, y0, scene_z + 128) {
                        return false;
                    }
                }
                if !self.occluded(scene_x, y1, scene_z) {
                    return false;
                }
                return self.occluded(scene_x, y1, scene_z + 128);
            }
            if r#type == 2 {
                if scene_z < self.cz {
                    if !self.occluded(scene_x, scene_y, scene_z + 128) {
                        return false;
                    }
                    if !self.occluded(scene_x + 128, scene_y, scene_z + 128) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(scene_x, y0, scene_z + 128) {
                        return false;
                    }
                    if !self.occluded(scene_x + 128, y0, scene_z + 128) {
                        return false;
                    }
                }
                if !self.occluded(scene_x, y1, scene_z + 128) {
                    return false;
                }
                return self.occluded(scene_x + 128, y1, scene_z + 128);
            }
            if r#type == 4 {
                if scene_x < self.cx {
                    if !self.occluded(scene_x + 128, scene_y, scene_z) {
                        return false;
                    }
                    if !self.occluded(scene_x + 128, scene_y, scene_z + 128) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(scene_x + 128, y0, scene_z) {
                        return false;
                    }
                    if !self.occluded(scene_x + 128, y0, scene_z + 128) {
                        return false;
                    }
                }
                if !self.occluded(scene_x + 128, y1, scene_z) {
                    return false;
                }
                return self.occluded(scene_x + 128, y1, scene_z + 128);
            }
            if r#type == 8 {
                if scene_z > self.cz {
                    if !self.occluded(scene_x, scene_y, scene_z) {
                        return false;
                    }
                    if !self.occluded(scene_x + 128, scene_y, scene_z) {
                        return false;
                    }
                }
                if level > 0 {
                    if !self.occluded(scene_x, y0, scene_z) {
                        return false;
                    }
                    if !self.occluded(scene_x + 128, y0, scene_z) {
                        return false;
                    }
                }
                if !self.occluded(scene_x, y1, scene_z) {
                    return false;
                }
                return self.occluded(scene_x + 128, y1, scene_z);
            }
        }

        if !self.occluded(scene_x + 64, y2, scene_z + 64) {
            return false;
        } else if r#type == 16 {
            return self.occluded(scene_x, y1, scene_z + 128);
        } else if r#type == 32 {
            return self.occluded(scene_x + 128, y1, scene_z + 128);
        } else if r#type == 64 {
            return self.occluded(scene_x + 128, y1, scene_z);
        } else if r#type == 128 {
            return self.occluded(scene_x, y1, scene_z);
        }

        // TS `console.warn('Warning unsupported wall type')`, then true.
        true
    }

    /// `spriteOccluded(level, tileX, tileZ, y)` from client-ts 2311-2328.
    fn sprite_occluded(&mut self, level: i32, tile_x: i32, tile_z: i32, y: i32) -> bool {
        if self.ground_occluded(level, tile_x, tile_z) {
            let x = tile_x << 7;
            let z = tile_z << 7;
            return self.occluded(x + 1, ground_h(self, level, tile_x, tile_z) - y, z + 1)
                && self.occluded(x + 128 - 1, ground_h(self, level, tile_x + 1, tile_z) - y, z + 1)
                && self.occluded(x + 128 - 1, ground_h(self, level, tile_x + 1, tile_z + 1) - y, z + 128 - 1)
                && self.occluded(x + 1, ground_h(self, level, tile_x, tile_z + 1) - y, z + 128 - 1);
        }
        false
    }

    /// `spriteOccluded2(level, minX, maxX, minZ, maxZ, y)` from client-ts
    /// 2329-2373. The TS `z` local holds the min-x scene coordinate; kept
    /// verbatim.
    fn sprite_occluded2(&mut self, level: i32, min_x: i32, max_x: i32, min_z: i32, max_z: i32, y: i32) -> bool {
        let x: i32;
        let z: i32;
        if min_x != max_x || min_z != max_z {
            for x0 in min_x..=max_x {
                for z0 in min_z..=max_z {
                    if self.occ_cycle(level, x0, z0) == -self.cycle_no {
                        return false;
                    }
                }
            }

            z = (min_x << 7) + 1;
            let z0 = (min_z << 7) + 2;
            let y0 = ground_h(self, level, min_x, min_z) - y;
            if !self.occluded(z, y0, z0) {
                return false;
            }

            let x1 = (max_x << 7) - 1;
            if !self.occluded(x1, y0, z0) {
                return false;
            }

            let z1 = (max_z << 7) - 1;
            if !self.occluded(z, y0, z1) {
                return false;
            } else if self.occluded(x1, y0, z1) {
                return true;
            } else {
                return false;
            }
        } else if self.ground_occluded(level, min_x, min_z) {
            x = min_x << 7;
            z = min_z << 7;
            return self.occluded(x + 1, ground_h(self, level, min_x, min_z) - y, z + 1)
                && self.occluded(x + 128 - 1, ground_h(self, level, min_x + 1, min_z) - y, z + 1)
                && self.occluded(x + 128 - 1, ground_h(self, level, min_x + 1, min_z + 1) - y, z + 128 - 1)
                && self.occluded(x + 1, ground_h(self, level, min_x, min_z + 1) - y, z + 128 - 1);
        }
        false
    }

    /// `occlusionCycle[level][x][z]` read (guarded; TS typed-array OOB is 0).
    fn occ_cycle(&self, level: i32, x: i32, z: i32) -> i32 {
        let stride_z = self.max_tile_z + 1;
        let stride_x = self.max_tile_x + 1;
        let index = (level * stride_x + x) * stride_z + z;
        self.occlusion_cycle.get(index as usize).copied().unwrap_or(0)
    }

    /// `occluded(x, y, z)` from client-ts 2374-2442: test a point against
    /// every active occluder frustum.
    fn occluded(&self, x: i32, y: i32, z: i32) -> bool {
        for i in 0..self.num_active_occluders as usize {
            let Some(occluder_index) = self.active_occluders.get(i).copied().flatten() else {
                continue;
            };
            let Some(occluder) = self.occluders.get(occluder_index).and_then(|o| o.as_ref()) else {
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

/// Guarded tile lookup; the free function keeps the borrow scoped to the
/// `squares` field so the fill loop can touch other `World` fields between
/// tile accesses (a `&mut self` helper would hold the whole struct).
fn tile_at(squares: &[Vec<Vec<Option<Square>>>], level: i32, x: i32, z: i32) -> Option<&Square> {
    if level < 0 || x < 0 || z < 0 {
        return None;
    }
    squares
        .get(level as usize)?
        .get(x as usize)?
        .get(z as usize)?
        .as_ref()
}

fn tile_at_mut(
    squares: &mut [Vec<Vec<Option<Square>>>],
    level: i32,
    x: i32,
    z: i32,
) -> Option<&mut Square> {
    if level < 0 || x < 0 || z < 0 {
        return None;
    }
    squares
        .get_mut(level as usize)?
        .get_mut(x as usize)?
        .get_mut(z as usize)?
        .as_mut()
}

/// `groundh[level][x][z]` read (guarded; TS typed-array OOB is undefined).
fn ground_h(world: &World, level: i32, x: i32, z: i32) -> i32 {
    if level < 0 || x < 0 || z < 0 {
        return 0;
    }
    world
        .groundh
        .get(level as usize)
        .and_then(|l| l.get(x as usize))
        .and_then(|r| r.get(z as usize))
        .copied()
        .unwrap_or(0)
}

/// `World.visBackingDirty[x][z]` read (TS `null` reads false everywhere).
fn vis_backing_at(world: &World, dx: i32, dz: i32) -> bool {
    let Some(row) = world.vis_backing_dirty else {
        return false;
    };
    let index = row + dx as usize * 51 + dz as usize;
    world.vis_backing.get(index).copied().unwrap_or(false)
}

/// `World.insideTriangle(...)` from client-ts 2443-2461. i64 cross products
/// (screen coordinates can reach ±335k, so the products overflow i32).
#[allow(clippy::too_many_arguments)]
fn inside_triangle(
    x: i32,
    y: i32,
    y0: i32,
    y1: i32,
    y2: i32,
    x0: i32,
    x1: i32,
    x2: i32,
) -> bool {
    if y < y0 && y < y1 && y < y2 {
        return false;
    } else if y > y0 && y > y1 && y > y2 {
        return false;
    } else if x < x0 && x < x1 && x < x2 {
        return false;
    } else if x > x0 && x > x1 && x > x2 {
        return false;
    }

    let cross_product_01 =
        (y as i64 - y0 as i64) * (x1 as i64 - x0 as i64) - (x as i64 - x0 as i64) * (y1 as i64 - y0 as i64);
    let cross_product_20 =
        (y as i64 - y2 as i64) * (x0 as i64 - x2 as i64) - (x as i64 - x2 as i64) * (y0 as i64 - y2 as i64);
    let cross_product_12 =
        (y as i64 - y1 as i64) * (x2 as i64 - x1 as i64) - (x as i64 - x1 as i64) * (y2 as i64 - y1 as i64);
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
