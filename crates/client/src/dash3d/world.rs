// Port of `~/experiments/Server/webclient/src/dash3d/World.ts` — the scene
// graph half (ground, walls, decor, objects, sprites, occluders). The render
// pass (`renderAll`, `fill`, `renderGround`/`renderQuickGround`, occlusion
// tests, `shareLight`, minimap drawing, mouse picking) lands with the render
// task; every method that builds or queries the scene is here.
//
// Sprites are arena-allocated (`World.sprites`) and tiles hold arena indices,
// matching the TS sharing of one sprite object across every tile it spans.
// The TS static `lowMem`, camera state, fill queue and sprite buffer are
// render-only and omitted; `numOccluders`/`occluders` (per-scene mutable)
// live on the `World` instance.
use crate::dash3d::TerrainOverlayShape;
use crate::dash3d::{
    Decor, Ground, GroundDecor, GroundObject, Occlude, QuickGround, SceneModel, Sprite, Square,
    Wall,
};

const OCCLUDER_LEVELS: usize = 4;
const MAX_OCCLUDERS: usize = 500;
const MAX_DYNAMIC_SPRITES: usize = 5000;

/// `levelHeightmaps[level][x][z]` ground heights, sized
/// `[maxLevel][maxTileX + 1][maxTileZ + 1]` (one extra row/column of corners).
pub type LevelHeightmaps = Vec<Vec<Vec<i32>>>;

pub struct World {
    min_level: i32,
    max_tile_level: i32,
    max_tile_x: i32,
    max_tile_z: i32,
    /// Read by the deferred render/lighting pass (`renderGround`, `shareLight`).
    #[allow(dead_code)]
    groundh: LevelHeightmaps,
    squares: Vec<Vec<Vec<Option<Square>>>>,
    sprites: Vec<Option<Sprite>>,
    dynamic_count: i32,
    dynamic_sprites: Vec<Option<usize>>,
    /// Occluder table for the (deferred) render pass. Per-scene mutable
    /// state (the TS `World.occluders`/`numOccluders` statics), so it lives
    /// on the `World` instance. Heap-backed because a by-value
    /// `[[Option<Occlude>; 500]; 4]` overflows small test-thread stacks in
    /// debug builds.
    num_occluders: [i32; OCCLUDER_LEVELS],
    occluders: Vec<Option<Occlude>>,
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

    pub fn get_wall(&self, level: i32, x: i32, z: i32) -> Option<&Wall> {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.wall.as_ref())
    }

    pub fn get_decor(&self, level: i32, x: i32, z: i32) -> Option<&Decor> {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.decor.as_ref())
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

    pub fn get_gd(&self, level: i32, x: i32, z: i32) -> Option<&GroundDecor> {
        self.squares[level as usize][x as usize][z as usize]
            .as_ref()
            .and_then(|tile| tile.ground_decor.as_ref())
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
}
