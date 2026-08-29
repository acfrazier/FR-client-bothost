//! The **sim half** of the scene world (Task 3 split of
//! `dash3d/world.rs`): per-tile typecodes, `Square` placement, collision
//! adjacency, ground heights and the pick-arm state. `Client.world` is this
//! struct; `tryMove`/`doAction`/`interact_with_loc` and the scene build
//! (`ClientBuild.add_loc`/`finish_build`) only ever touch this half, so a
//! headless bot runs the full sim with no renderer and no decoded models.
//!
//! The render half (`render::world::RenderWorld`) owns the 3D pass
//! machinery (`render_all`/`fill`, the visibility backing, occluders,
//! minimap ground pass) and reads `Client.world` for the per-tile data.
//! The per-tile structs (`Wall`/`Decor`/`Sprite`/`GroundObject`/
//! `GroundDecor`) keep only typecodes, placement and the small decode
//! ints (heights, LOC_ANIM state); the **models themselves are resolved
//! lazily by the render side** (Task 3b), so a client that never draws
//! never decodes a model. Per-frame render stamps (`Square.draw_front`,
//! `Sprite.cycle`, ...) still ride on the sim tiles like they did in the
//! Task 3 split.
//!
//! `share_light` moved to the render half with the models. `finish_build`
//! sets `share_light_pending`; the renderer runs the pass on its first
//! frame after a build (a headless client leaves it pending forever).

// Ported verbatim from dash3d/world.rs (the TS port keeps these structures);
// the dash3d module-level clippy allows follow the code to its new home.
#![allow(clippy::manual_range_contains)]
#![allow(clippy::too_many_arguments)]

use crate::dash3d::TerrainOverlayShape;
use crate::dash3d::{
    Decor, Ground, GroundDecor, GroundObject, Occlude, QuickGround, Sprite, Square, Wall,
};

pub const OCCLUDER_LEVELS: usize = 4;
pub const MAX_OCCLUDERS: usize = 500;
pub const MAX_DYNAMIC_SPRITES: usize = 5000;
pub const MAX_ACTIVE_OCCLUDERS: usize = 500;

/// `levelHeightmaps[level][x][z]` ground heights, sized
/// `[maxLevel][maxTileX + 1][maxTileZ + 1]` (one extra row/column of corners).
pub type LevelHeightmaps = Vec<Vec<Vec<i32>>>;

/// The sim half of the scene world. See the module docs for what stays
/// here versus `render::world::RenderWorld`.
pub struct World {
    pub(crate) min_level: i32,
    pub(crate) max_tile_level: i32,
    pub(crate) max_tile_x: i32,
    pub(crate) max_tile_z: i32,
    /// Read by the render pass (`render_quick_ground`, the `render_all`
    /// visibility gate) and mirrored by `Client::map_build` after the
    /// load/fade pass (Java shares the one array between Client and World).
    pub(crate) groundh: LevelHeightmaps,
    /// Boxed so an empty tile is 8 bytes, not a 680-byte `Square`. Fifty
    /// skip-paint clients otherwise keep ~29 MB of unused tile structs each.
    pub(crate) squares: Vec<Vec<Vec<Option<Box<Square>>>>>,
    /// The sprite arena: tiles hold arena indices, matching the TS sharing
    /// of one sprite object across every tile it spans. The render pass
    /// (`fill`) reads the same arena for the scene sprites.
    pub(crate) sprites: Vec<Option<Sprite>>,
    pub(crate) dynamic_count: i32,
    pub(crate) dynamic_sprites: Vec<Option<usize>>,
    /// Occluder table. Written by the sim build (`ClientBuild::finish_build`
    /// `set_occlude`), consumed by the render pass (`calc_occlude`). Per-
    /// scene mutable state (the TS `World.occluders`/`numOccluders`
    /// statics). Heap-backed because a by-value `[[Option<Occlude>; 500]; 4]`
    /// overflows small test-thread stacks in debug builds.
    pub(crate) num_occluders: [i32; OCCLUDER_LEVELS],
    pub(crate) occluders: Vec<Option<Occlude>>,
    /// TS `World.occlusionCycle`, `[maxLevel][maxTileX + 1][maxTileZ + 1]`
    /// i32s, flat. The render pass (`ground_occluded`/`wall_occluded`/
    /// `sprite_occluded`) stamps ±`cycle_no` to cache per-frame occlusion
    /// answers; never cleared (like TS), stale stamps are aged out by the
    /// cycle comparison.
    pub(crate) occlusion_cycle: Vec<i32>,
    /// Pick-arm state: `doAction` (sim) arms a ground click, the render
    /// pass raycasts it and writes `ground_x`/`ground_z`, and `game_loop`
    /// (sim) consumes the answer into a `MOVE_GAMECLICK` walk. The state is
    /// genuinely shared across the boundary, so it stays on the sim half.
    pub click: bool,
    pub click_x: i32,
    pub click_y: i32,
    pub ground_x: i32,
    pub ground_z: i32,
    /// Bumped by `reset_map`; the render side clears its lazy model cache
    /// when it changes (Task 3b).
    pub(crate) build_generation: u64,
    /// Set by `ClientBuild::finish_build` in place of the old sim-side
    /// `share_light` call; the render side runs the share-light pass over
    /// its lazily-resolved models on the first frame after a build
    /// (`render_all` consumes it via `take_share_light_pending`). A
    /// headless client never consumes it and never decodes a model.
    pub share_light_pending: bool,
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
        World {
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
            // A fresh world starts at generation 0 so a fresh `RenderWorld`
            // (which also starts at 0) does not clear its model cache on the
            // first `render_all`; every `map_build` calls `reset_map`, which
            // bumps it and invalidates the render-side cache. The
            // constructor needs no reset (all grids are already empty).
            build_generation: 0,
            share_light_pending: false,
        }
    }

    pub fn reset_map(&mut self) {
        self.build_generation = self.build_generation.wrapping_add(1);

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
    }

    pub fn fill_base_level(&mut self, level: i32) {
        self.min_level = level;

        for stx in 0..self.max_tile_x {
            for stz in 0..self.max_tile_z {
                self.squares[level as usize][stx as usize][stz as usize] =
                    Some(Box::new(Square::new(level, stx, stz)));
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
            self.squares[0][stx as usize][stz as usize] =
                Some(Box::new(Square::new(0, stx, stz)));
        }

        let tile = &mut self.squares[0][stx as usize][stz as usize];
        if let Some(tile) = tile {
            tile.linked_square = below;
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
                self.squares[l as usize][x as usize][z as usize] =
                    Some(Box::new(Square::new(l, x, z)));
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
        tile_level: i32,
        tile_x: i32,
        tile_z: i32,
        y: i32,
        typecode: i32,
        typecode2: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) {
        if self.squares[tile_level as usize][tile_x as usize][tile_z as usize].is_none() {
            self.squares[tile_level as usize][tile_x as usize][tile_z as usize] =
                Some(Box::new(Square::new(tile_level, tile_x, tile_z)));
        }

        let tile = &mut self.squares[tile_level as usize][tile_x as usize][tile_z as usize];
        if let Some(tile) = tile {
            tile.ground_decor = Some(GroundDecor::new(
                y,
                tile_x * 128 + 64,
                tile_z * 128 + 64,
                typecode,
                typecode2,
                h_sw,
                h_se,
                h_ne,
                h_nw,
            ));
            tile.model_stamp = tile.model_stamp.wrapping_add(1);
        }
    }

    pub fn del_ground_decor(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.ground_decor = None;
        tile.model_stamp = tile.model_stamp.wrapping_add(1);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_obj(
        &mut self,
        stx: i32,
        stz: i32,
        y: i32,
        level: i32,
        typecode: i32,
        top: Option<(i32, i32)>,
        middle: Option<(i32, i32)>,
        bottom: Option<(i32, i32)>,
    ) {
        if self.squares[level as usize][stx as usize][stz as usize].is_none() {
            self.squares[level as usize][stx as usize][stz as usize] =
                Some(Box::new(Square::new(level, stx, stz)));
        }

        let tile = &mut self.squares[level as usize][stx as usize][stz as usize];
        if let Some(tile) = tile {
            tile.ground_object = Some(GroundObject::new(
                y,
                stx * 128 + 64,
                stz * 128 + 64,
                typecode,
                top,
                middle,
                bottom,
            ));
            tile.model_stamp = tile.model_stamp.wrapping_add(1);
        }
    }

    pub fn del_obj(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.ground_object = None;
        tile.model_stamp = tile.model_stamp.wrapping_add(1);
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
        typecode1: i32,
        typecode2: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) {
        for l in (0..=level).rev() {
            if self.squares[l as usize][tile_x as usize][tile_z as usize].is_none() {
                self.squares[l as usize][tile_x as usize][tile_z as usize] =
                    Some(Box::new(Square::new(l, tile_x, tile_z)));
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
                typecode1,
                typecode2,
                h_sw,
                h_se,
                h_ne,
                h_nw,
            ));
            tile.model_stamp = tile.model_stamp.wrapping_add(1);
        }
    }

    pub fn del_wall(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.wall = None;
        tile.model_stamp = tile.model_stamp.wrapping_add(1);
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
        typecode2: i32,
        angle: i32,
        wshape: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) {
        for l in (0..=level).rev() {
            if self.squares[l as usize][tile_x as usize][tile_z as usize].is_none() {
                self.squares[l as usize][tile_x as usize][tile_z as usize] =
                    Some(Box::new(Square::new(l, tile_x, tile_z)));
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
                typecode,
                typecode2,
                h_sw,
                h_se,
                h_ne,
                h_nw,
            ));
            tile.model_stamp = tile.model_stamp.wrapping_add(1);
        }
    }

    pub fn del_decor(&mut self, level: i32, x: i32, z: i32) {
        let tile = &mut self.squares[level as usize][x as usize][z as usize];
        let Some(tile) = tile else { return };
        tile.decor = None;
        tile.model_stamp = tile.model_stamp.wrapping_add(1);
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

    #[allow(clippy::too_many_arguments)]
    pub fn add_scenery(
        &mut self,
        level: i32,
        tile_x: i32,
        tile_z: i32,
        y: i32,
        typecode: i32,
        info: i32,
        width: i32,
        length: i32,
        yaw: i32,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) -> bool {
        let scene_x = tile_x * 128 + width * 64;
        let scene_z = tile_z * 128 + length * 64;
        self.set_sprite(
            scene_x, scene_z, y, level, tile_x, tile_z, width, length, typecode, info, yaw,
            false, h_sw, h_se, h_ne, h_nw,
        )
        .is_some()
    }

    /// `addDynamic` from client-ts: place a dynamic sprite (players/npcs/
    /// projectiles/spot-anims) and return its arena index so the render
    /// side can attach the (already-built, entity-state) `SceneModel` to
    /// its parallel `sprite_models` arena.
    #[allow(clippy::too_many_arguments)]
    pub fn add_dynamic(
        &mut self,
        level: i32,
        x: i32,
        y: i32,
        z: i32,
        typecode: i32,
        yaw: i32,
        padding: i32,
        forward_padding: bool,
    ) -> Option<usize> {
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

        self.set_sprite(x, z, y, level, x0, z0, x1 + 1 - x0, z1 - z0 + 1, typecode, 0, yaw, true, 0, 0, 0, 0)
    }

    #[allow(clippy::too_many_arguments)]
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
        typecode: i32,
        yaw: i32,
    ) -> Option<usize> {
        self.set_sprite(
            x,
            z,
            y,
            level,
            min_tile_x,
            min_tile_z,
            max_tile_x + 1 - min_tile_x,
            max_tile_z - min_tile_z + 1,
            typecode,
            0,
            yaw,
            true,
            0,
            0,
            0,
            0,
        )
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

    /// Read access to one tile (`squares` is private; the scene tests and
    /// `mapBuild`-adjacent queries read through this).
    pub fn square(&self, level: i32, x: i32, z: i32) -> Option<&Square> {
        self.squares[level as usize][x as usize][z as usize].as_deref()
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

    /// The arena index of the scene sprite on a tile (`get_scene`'s
    /// match), for the render side / tests to attach the sprite's model to
    /// the parallel `sprite_models` arena.
    pub fn scene_sprite_index(&self, level: i32, x: i32, z: i32) -> Option<usize> {
        let tile = self.squares[level as usize][x as usize][z as usize].as_ref()?;
        for l in 0..tile.sprite_count as usize {
            if let Some(idx) = tile.sprites[l] {
                if let Some(sprite) = &self.sprites[idx] {
                    if (sprite.typecode >> 29) & 0x3 == 2
                        && sprite.min_tile_x == x
                        && sprite.min_tile_z == z
                    {
                        return Some(idx);
                    }
                }
            }
        }
        None
    }

    /// The most recently pushed sprite arena index (every sprite push
    /// appends; `del_sprite` only nulls slots). The tests attach render-side
    /// models for synthetic sprites through this.
    pub fn last_sprite_index(&self) -> Option<usize> {
        self.sprites.len().checked_sub(1)
    }

    /// The model stamp of the tile, for the render side's lazy model cache
    /// (`None` tiles report `i32::MIN`, which never matches a resolved slot).
    pub fn tile_model_stamp(&self, level: i32, x: i32, z: i32) -> i32 {
        tile_at(&self.squares, level, x, z)
            .map(|t| t.model_stamp)
            .unwrap_or(i32::MIN)
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
        typecode: i32,
        info: i32,
        yaw: i32,
        dynamic: bool,
        h_sw: i32,
        h_se: i32,
        h_ne: i32,
        h_nw: i32,
    ) -> Option<usize> {
        for tx in tile_x..tile_x + tile_size_x {
            for tz in tile_z..tile_z + tile_size_z {
                if tx < 0 || tz < 0 || tx >= self.max_tile_x || tz >= self.max_tile_z {
                    return None;
                }
                if let Some(tile) = &self.squares[level as usize][tx as usize][tz as usize] {
                    if tile.sprite_count >= 5 {
                        return None;
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
            yaw,
            tile_x,
            tile_x + tile_size_x - 1,
            tile_z,
            tile_z + tile_size_z - 1,
            typecode,
            info,
            h_sw,
            h_se,
            h_ne,
            h_nw,
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
                            Some(Box::new(Square::new(l, tx, tz)));
                    }
                }

                let tile = &mut self.squares[level as usize][tx as usize][tz as usize];
                if let Some(tile) = tile {
                    tile.sprites[tile.sprite_count as usize] = Some(index);
                    tile.sprite_span[tile.sprite_count as usize] = spans;
                    tile.sprite_spans |= spans;
                    tile.sprite_count += 1;
                    // A sprite does not invalidate the tile's wall/decor/
                    // ground-decor/obj model cache: those resolve from the
                    // tile typecodes, while scene sprites resolve from the
                    // sprite's own `model_stamp` (dynamic sprites attach via
                    // `set_sprite_model`). Bumping here made a moving
                    // player/NPC re-decode the tile's already-lit wall as an
                    // unlit model, which emitted no faces — the black-wall
                    // bug.
                }
            }
        }

        if dynamic {
            self.dynamic_sprites[self.dynamic_count as usize] = Some(index);
            self.dynamic_count += 1;
        }

        Some(index)
    }

    /// Bump the tile's model stamp (called by the LOC_ANIM arm after it
    /// rewrites the tile's wall/decor/ground-decor anim state).
    pub fn bump_tile_stamp(&mut self, level: i32, x: i32, z: i32) {
        if let Some(tile) = tile_at_mut(&mut self.squares, level, x, z) {
            tile.model_stamp = tile.model_stamp.wrapping_add(1);
        }
    }

    /// The render side's pending share-light flag (set by `finish_build`);
    /// consumed by the first `render_all` after a build.
    pub fn take_share_light_pending(&mut self) -> bool {
        let pending = self.share_light_pending;
        self.share_light_pending = false;
        pending
    }

    pub(crate) fn del_sprite(&mut self, index: usize) {
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

    /// `updateMousePicking(mouseX, mouseY)` from client-ts: arm the pick and
    /// reset the ground answer. Stays on the sim half — `doAction` (sim)
    /// arms it with the click coords; the render pass raycasts it and
    /// writes `ground_x`/`ground_z` for `game_loop` (sim) to consume.
    pub fn update_mouse_picking(&mut self, mouse_x: i32, mouse_y: i32) {
        self.click = true;
        self.click_x = mouse_x;
        self.click_y = mouse_y;
        self.ground_x = -1;
        self.ground_z = -1;
    }
}

/// Guarded tile lookup; the free function keeps the borrow scoped to the
/// `squares` field so the fill loop can touch other `World` fields between
/// tile accesses (a `&mut self` helper would hold the whole struct).
pub(crate) fn tile_at(
    squares: &[Vec<Vec<Option<Box<Square>>>>],
    level: i32,
    x: i32,
    z: i32,
) -> Option<&Square> {
    if level < 0 || x < 0 || z < 0 {
        return None;
    }
    squares
        .get(level as usize)?
        .get(x as usize)?
        .get(z as usize)?
        .as_deref()
}

pub(crate) fn tile_at_mut(
    squares: &mut [Vec<Vec<Option<Box<Square>>>>],
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
        .as_deref_mut()
}

/// `groundh[level][x][z]` read (guarded; TS typed-array OOB is undefined).
pub(crate) fn ground_h(world: &World, level: i32, x: i32, z: i32) -> i32 {
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

#[cfg(test)]
mod size_tests {
    use crate::dash3d::Square;

    #[test]
    fn empty_tile_grid_is_pointer_sized() {
        let tiles = 4 * 104 * 104;
        let grid = tiles * std::mem::size_of::<Option<Box<Square>>>();
        assert_eq!(
            std::mem::size_of::<Option<Box<Square>>>(),
            8,
            "empty tiles must be a pointer, not an inlined Square"
        );
        assert!(
            grid < 400_000,
            "boxed empty grid should be ~346 KB, got {grid}"
        );
    }
}
