//! The **sim half** of the scene world (Task 3 split of
//! `dash3d/world.rs`): per-tile typecodes, `Square` placement, collision
//! adjacency, ground heights and the pick-arm state. `Client.world` is this
//! struct; `tryMove`/`doAction`/`interact_with_loc` and the scene build
//! (`ClientBuild.add_loc`/`finish_build`) only ever touch this half, so a
//! headless bot runs the full sim with no renderer.
//!
//! The render half (`render::world::RenderWorld`) owns the 3D pass
//! machinery (`render_all`/`fill`, the visibility backing, occluders,
//! minimap ground pass) and reads `Client.world` for the per-tile data.
//! The per-tile structs (`Wall`/`Decor`/`Sprite`/`GroundObject`/
//! `GroundDecor`) keep their typecode + model together here because the
//! sim loop creates and mutates them (LOC_ANIM, `show_object`, the
//! `add_loc` build) and must do so with no renderer constructed; the
//! render half borrows them through the `&mut World` parameters of its
//! methods instead of duplicating the tile grid.
//!
//! `share_light` and the pick arm (`update_mouse_picking`, `click`,
//! `ground_x/z`) also stay here: `finish_build` runs `share_light` in the
//! renderer-free sim loop, and `doAction`/`game_loop` arm and consume the
//! ground pick across the render pass.

// Ported verbatim from dash3d/world.rs (the TS port keeps these structures);
// the dash3d module-level clippy allows follow the code to its new home.
#![allow(clippy::manual_range_contains)]
#![allow(clippy::too_many_arguments)]

use crate::dash3d::TerrainOverlayShape;
use crate::dash3d::{
    Decor, Ground, GroundDecor, GroundObject, Model, Occlude, QuickGround, SceneModel, Sprite,
    Square, Wall,
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
    pub(crate) squares: Vec<Vec<Vec<Option<Square>>>>,
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
    /// TS `World.shareTic`/`shareMap`/`shareMap2` (World.ts 145-147): the
    /// share-light merge stamps. `share_light` runs from the sim build
    /// (`finish_build`), so the scratch lives here.
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
                model,
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
                top_obj,
                middle_obj,
                bottom_obj,
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
                model1,
                model2,
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
                model,
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
                        if let Some(SceneModel::Model(model1)) = wall.model1.as_mut() {
                            if model1.point_normal.is_some() {
                                self.share_light_loc(level, tile_x, tile_z, 1, 1, model1);
                                if let Some(SceneModel::Model(model2)) = wall.model2.as_mut() {
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
                        if let Some(SceneModel::Model(model)) = gd.model.as_mut() {
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
                    gd.as_mut().and_then(|g| g.model.as_mut())
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
                    gd.as_mut().and_then(|g| g.model.as_mut())
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
                    gd.as_mut().and_then(|g| g.model.as_mut())
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
                    gd.as_mut().and_then(|g| g.model.as_mut())
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
                        if let Some(SceneModel::Model(model_b)) = wall.model1.as_mut() {
                            if model_b.point_normal.is_some() {
                                self.model_share_light(
                                    model_a, model_b, offset_x, offset_y, offset_z,
                                    allow_face_removal,
                                );
                            }
                        }
                        if let Some(SceneModel::Model(model_b)) = wall.model2.as_mut() {
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
pub(crate) fn tile_at(squares: &[Vec<Vec<Option<Square>>>], level: i32, x: i32, z: i32) -> Option<&Square> {
    if level < 0 || x < 0 || z < 0 {
        return None;
    }
    squares
        .get(level as usize)?
        .get(x as usize)?
        .get(z as usize)?
        .as_ref()
}

pub(crate) fn tile_at_mut(
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
