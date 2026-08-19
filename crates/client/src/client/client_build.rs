//! Port of `~/experiments/Server/webclient/src/client/ClientBuild.ts` — the
//! map-build scratch arrays and the ground decode (`loadGround`, plus the
//! perlin-noise terrain fallback it calls). The scene build
//! (`loadLocations`/`addLoc`) is ported here too; only `finishBuild` with
//! the light/occlusion passes remains (Task 7). `loadLocations` writes into
//! the `World`/`CollisionMap`s of the owning `Client`, and `addLoc` records
//! the per-tile shadow/occlusion bits `finishBuild` will consume.
//!
//! `loadGround` writes the client's `groundh` and `mapl` directly (the TS
//! passes its `Client.groundh`/`Client.mapl` references into the
//! constructor); the per-build floor arrays live on this struct as in TS,
//! and so do the `shadow`/`mapo` scratch grids (`finishBuild` reads them).
use crate::config::Cache;
use crate::dash3d::world::LevelHeightmaps;
use crate::dash3d::{
    BuildArea, ClientLocAnim, CollisionMap, LocAngle, LocShape, MapFlag, SceneModel, World,
};
use crate::graphics::Pix3D;
use crate::io::{OnDemand, Packet};

/// `ClientBuild.WSHAPE0` from client-ts.
const WSHAPE0: [i32; 4] = [1, 2, 4, 8];
/// `ClientBuild.WSHAPE1` from client-ts.
const WSHAPE1: [i32; 4] = [16, 32, 64, 128];
/// `ClientBuild.DECORXOF` from client-ts.
const DECORXOF: [i32; 4] = [1, 0, -1, 0];
/// `ClientBuild.DECORZOF` from client-ts.
const DECORZOF: [i32; 4] = [0, -1, 0, 1];

pub struct ClientBuild {
    /// `ClientBuild.lowMem` from client-ts (static default true); the
    /// map-build flow sets it from the world/config low-mem setting.
    pub low_mem: bool,
    /// `ClientBuild.minusedlevel` from client-ts; consumed by `addLoc`.
    pub minusedlevel: i32,
    /// `floort1[level][x][z]` / `floort2[level][x][z]` floor type ids.
    floort1: Vec<Vec<Vec<u8>>>,
    floort2: Vec<Vec<Vec<u8>>>,
    /// `floors[level][x][z]` overlay shape, `floorr[level][x][z]` rotation.
    floors: Vec<Vec<Vec<u8>>>,
    floorr: Vec<Vec<Vec<u8>>>,
    /// `this.shadow[level][x][z]` shade values written by `addLoc`, sized
    /// `[LEVELS][SIZE + 1][SIZE + 1]`; `finishBuild` (Task 7) reads them.
    pub shadow: Vec<Vec<Vec<u8>>>,
    /// `this.mapo[level][x][z]` map-occlusion bits written by `addLoc`;
    /// `finishBuild` (Task 7) reads them.
    pub mapo: Vec<Vec<Vec<i32>>>,
}

impl Default for ClientBuild {
    fn default() -> Self {
        ClientBuild::new()
    }
}

impl ClientBuild {
    /// One build-area (`BuildArea.SIZE` tiles square) of scratch arrays, as
    /// `new ClientBuild(BuildArea.SIZE, BuildArea.SIZE, ...)` in client-ts.
    pub fn new() -> Self {
        let grid = || {
            vec![
                vec![vec![0u8; BuildArea::SIZE as usize]; BuildArea::SIZE as usize];
                BuildArea::LEVELS as usize
            ]
        };
        // TS allocates shadow/mapo at maxTile + 1 (ClientBuild.ts 63-64)
        let grid_plus_u8 = || {
            vec![
                vec![vec![0u8; (BuildArea::SIZE + 1) as usize]; (BuildArea::SIZE + 1) as usize];
                BuildArea::LEVELS as usize
            ]
        };
        let grid_plus_i32 = || {
            vec![
                vec![vec![0i32; (BuildArea::SIZE + 1) as usize]; (BuildArea::SIZE + 1) as usize];
                BuildArea::LEVELS as usize
            ]
        };
        ClientBuild {
            low_mem: true,
            minusedlevel: 0,
            floort1: grid(),
            floort2: grid(),
            floors: grid(),
            floorr: grid(),
            shadow: grid_plus_u8(),
            mapo: grid_plus_i32(),
        }
    }

    /// `loadGround(src, originX, originZ, xOffset, zOffset)` from client-ts:
    /// decode one 64x64x4 map square into `groundh` and `mapl`. `origin` is
    /// the build base (`(centreZone - 6) * 8`); `xOffset`/`zOffset` are the
    /// square's local tiles. Out-of-area tiles still consume the packet
    /// bytes.
    pub fn load_ground(
        &mut self,
        groundh: &mut LevelHeightmaps,
        mapl: &mut Vec<Vec<Vec<u8>>>,
        src: &[u8],
        origin_x: i32,
        origin_z: i32,
        x_offset: i32,
        z_offset: i32,
    ) {
        let mut buf = Packet::new(src.to_vec());

        for level in 0..BuildArea::LEVELS {
            for x in 0..64 {
                for z in 0..64 {
                    let stx = x + x_offset;
                    let stz = z + z_offset;

                    if (0..BuildArea::SIZE).contains(&stx) && (0..BuildArea::SIZE).contains(&stz) {
                        mapl[level as usize][stx as usize][stz as usize] = 0;

                        loop {
                            let opcode = buf.g1();
                            if opcode == 0 {
                                if level == 0 {
                                    groundh[0][stx as usize][stz as usize] = -Self::perlin_noise(
                                        stx + origin_x + 932731,
                                        stz + 556238 + origin_z,
                                    ) * 8;
                                } else {
                                    groundh[level as usize][stx as usize][stz as usize] =
                                        groundh[level as usize - 1][stx as usize][stz as usize] - 240;
                                }
                                break;
                            }

                            if opcode == 1 {
                                let mut height = buf.g1();
                                if height == 1 {
                                    height = 0;
                                }
                                if level == 0 {
                                    groundh[0][stx as usize][stz as usize] = -height * 8;
                                } else {
                                    groundh[level as usize][stx as usize][stz as usize] =
                                        groundh[level as usize - 1][stx as usize][stz as usize]
                                            - height * 8;
                                }
                                break;
                            }

                            if opcode <= 49 {
                                // g1b into a Uint8Array: signed byte, stored raw
                                self.floort2[level as usize][stx as usize][stz as usize] =
                                    buf.g1b() as u8;
                                self.floors[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 2) / 4) << 24 >> 24) as u8;
                                self.floorr[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 2) & 0x3) << 24 >> 24) as u8;
                            } else if opcode <= 81 {
                                mapl[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 49) << 24) >> 24) as u8;
                            } else {
                                self.floort1[level as usize][stx as usize][stz as usize] =
                                    (((opcode - 81) << 24) >> 24) as u8;
                            }
                        }
                    } else {
                        loop {
                            let opcode = buf.g1();
                            if opcode == 0 {
                                break;
                            }

                            if opcode == 1 {
                                buf.g1();
                                break;
                            }

                            if opcode <= 49 {
                                buf.g1();
                            }
                        }
                    }
                }
            }
        }
    }

    /// `checkLocations(src, xOffset, zOffset)` from client-ts: decode the
    /// gsmart loc-id/pos delta stream and report whether every in-area loc's
    /// models are downloaded. Out-of-area tiles still consume the packet
    /// bytes, and `skip` state tracks per-loc model-check progress.
    pub fn check_locations(&self, cache: &Cache, src: &[u8], x_offset: i32, z_offset: i32) -> bool {
        let mut buf = Packet::new(src.to_vec());
        let mut ready = true;
        let mut loc_id = -1;

        loop {
            let delta_id = buf.gsmart();
            if delta_id == 0 {
                break;
            }
            loc_id += delta_id;

            let mut loc_pos = 0;
            let mut skip = false;

            loop {
                let delta_pos = buf.gsmart();
                if delta_pos == 0 {
                    break;
                }

                if skip {
                    buf.g1();
                } else {
                    loc_pos += delta_pos - 1;
                    let z = loc_pos & 0x3f;
                    let x = (loc_pos >> 6) & 0x3f;
                    let shape = buf.g1() >> 2;
                    let stx = x_offset + x;
                    let stz = z_offset + z;

                    if stx > 0 && stz > 0 && stx < 103 && stz < 103 {
                        let loc = cache.loc(loc_id as usize);
                        if shape != 22 || !self.low_mem || loc.active || loc.forcedecor {
                            if !loc.check_model_all() {
                                ready = false;
                            }
                            skip = true;
                        }
                    }
                }
            }
        }

        ready
    }

    /// `prefetchLocations(buf, od)` from client-ts: walk the same gsmart loc
    /// stream and prefetch every referenced loc's models.
    pub fn prefetch_locations(cache: &Cache, buf: &mut Packet, od: &mut OnDemand) {
        let mut loc_id = -1;

        loop {
            let delta_id = buf.gsmart();
            if delta_id == 0 {
                return;
            }
            loc_id += delta_id;

            let loc = cache.loc(loc_id as usize);
            loc.prefetch_model_all(od);

            loop {
                let delta_pos = buf.gsmart();
                if delta_pos == 0 {
                    break;
                }
                buf.g1();
            }
        }
    }

    /// `loadLocations(src, xOffset, zOffset, world, collisions)` from
    /// client-ts (ClientBuild.ts 718-763): decode the gsmart loc-id/pos
    /// delta stream and place each in-area loc via `addLoc`. `groundh` and
    /// `mapl` are the `Client`'s scene grids the TS reads through its
    /// constructor references; `loop_cycle` stands in for the TS
    /// `Client.loopCycle` read by `ClientLocAnim`. Out-of-area tiles still
    /// consume the packet bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn load_locations(
        &mut self,
        cache: &Cache,
        world: &mut World,
        collisions: &mut [CollisionMap; 4],
        groundh: &LevelHeightmaps,
        mapl: &[Vec<Vec<u8>>],
        src: &[u8],
        x_offset: i32,
        z_offset: i32,
        loop_cycle: i32,
    ) {
        let mut buf = Packet::new(src.to_vec());
        let mut loc_id = -1;

        loop {
            let delta_id = buf.gsmart();
            if delta_id == 0 {
                return;
            }
            loc_id += delta_id;

            let mut loc_pos = 0;
            loop {
                let delta_pos = buf.gsmart();
                if delta_pos == 0 {
                    break;
                }

                loc_pos += delta_pos - 1;
                let z = loc_pos & 0x3f;
                let x = (loc_pos >> 6) & 0x3f;
                let level = loc_pos >> 12;

                let info = buf.g1();
                let shape = info >> 2;
                let rotation = info & 0x3;
                let stx = x + x_offset;
                let stz = z + z_offset;

                if stx > 0 && stz > 0 && stx < BuildArea::SIZE - 1 && stz < BuildArea::SIZE - 1 {
                    let mut current_level = level;
                    if mapl[1][stx as usize][stz as usize] as i32 & MapFlag::LINK_BELOW != 0 {
                        current_level = level - 1;
                    }

                    let collision = if current_level >= 0 {
                        Some(&mut collisions[current_level as usize])
                    } else {
                        None
                    };

                    self.add_loc(
                        cache, world, collision, groundh, mapl, level, stx, stz, loc_id, shape,
                        rotation, loop_cycle,
                    );
                }
            }
        }
    }

    /// `addLoc(...)` from client-ts (ClientBuild.ts 765-1137): place one loc
    /// into the world (wall/decor/scenery/ground-decor layers) and stamp the
    /// collision map and the `shadow`/`mapo` scratch grids. A missing model
    /// skips the visual placement exactly as the TS null-model paths do (the
    /// collision side-effects still run); never panics.
    #[allow(clippy::too_many_arguments)]
    fn add_loc(
        &mut self,
        cache: &Cache,
        world: &mut World,
        mut collision: Option<&mut CollisionMap>,
        groundh: &LevelHeightmaps,
        mapl: &[Vec<Vec<u8>>],
        level: i32,
        x: i32,
        z: i32,
        loc_id: i32,
        shape: i32,
        angle: i32,
        loop_cycle: i32,
    ) {
        if self.low_mem {
            if mapl[level as usize][x as usize][z as usize] as i32 & MapFlag::FORCE_HIGH_DETAIL != 0 {
                return;
            }

            if Self::get_vis_below_level(mapl, level, x, z) != self.minusedlevel {
                return;
            }
        }

        let mut height_sw = groundh[level as usize][x as usize][z as usize];
        let mut height_se = groundh[level as usize][(x + 1) as usize][z as usize];
        let mut height_ne = groundh[level as usize][(x + 1) as usize][(z + 1) as usize];
        let mut height_nw = groundh[level as usize][x as usize][(z + 1) as usize];
        let y = (height_sw + height_se + height_ne + height_nw) >> 2;

        let loc = cache.loc(loc_id as usize);

        let mut typecode = x
            .wrapping_add(z << 7)
            .wrapping_add(loc_id << 14)
            .wrapping_add(0x40000000);
        if !loc.active {
            typecode = typecode.wrapping_add(i32::MIN);
        }

        let typecode2 = ((angle << 6) + shape).wrapping_shl(24) >> 24;

        if shape == LocShape::GROUND_DECOR {
            if !self.low_mem || loc.active || loc.forcedecor {
                let model = if loc.anim == -1 {
                    loc.get_model(cache, 22, angle, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 22, shape, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                };

                world.set_ground_decor(model, level, x, z, y, typecode, typecode2);

                if loc.blockwalk && loc.active {
                    if let Some(cmap) = collision.as_mut() {
                        cmap.block_ground(x, z);
                    }
                }
            }
        } else if shape == LocShape::CENTREPIECE_STRAIGHT || shape == LocShape::CENTREPIECE_DIAGONAL {
            let model = if loc.anim == -1 {
                loc.get_model(cache, 10, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, 10, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            if let Some(model) = model {
                let mut yaw = 0;
                if shape == LocShape::CENTREPIECE_DIAGONAL {
                    yaw += 256;
                }

                let (width, height) = if angle == LocAngle::NORTH || angle == LocAngle::SOUTH {
                    (loc.length, loc.width)
                } else {
                    (loc.width, loc.length)
                };

                // TS `model2 = model` for plain Models and `getModel` for
                // animated ones (TS 831-838); only `radius` is read, so the
                // plain-model case is captured before the move.
                let radius = match &model {
                    SceneModel::Model(m) => Some(m.radius),
                    _ => None,
                };

                if world.add_scenery(level, x, z, y, Some(model), typecode, typecode2, width, height, yaw)
                    && loc.shadow
                {
                    let radius = radius.or_else(|| {
                        loc.get_model(cache, 10, angle, height_sw, height_se, height_ne, height_nw, -1)
                            .map(|m| m.radius)
                    });

                    if let Some(radius) = radius {
                        let mut shade = radius / 4;
                        if shade > 30 {
                            shade = 30;
                        }

                        // TS Uint8Array writes out of the 105x105 grid are
                        // silently dropped; keep that no-op semantics.
                        for dx in 0..=width {
                            for dz in 0..=height {
                                let sx = x + dx;
                                let sz = z + dz;
                                if (1..=BuildArea::SIZE).contains(&sx)
                                    && (1..=BuildArea::SIZE).contains(&sz)
                                    && shade > self.shadow[level as usize][sx as usize][sz as usize] as i32
                                {
                                    self.shadow[level as usize][sx as usize][sz as usize] = shade as u8;
                                }
                            }
                        }
                    }
                }
            }

            if loc.blockwalk {
                if let Some(cmap) = collision.as_deref_mut() {
                    cmap.add_loc(x, z, loc.width, loc.length, angle, loc.blockrange);
                }
            }
        } else if shape >= LocShape::ROOF_STRAIGHT {
            let model = if loc.anim == -1 {
                loc.get_model(cache, shape, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, shape, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.add_scenery(level, x, z, y, model, typecode, typecode2, 1, 1, 0);

            if (LocShape::ROOF_STRAIGHT..=LocShape::ROOF_FLAT).contains(&shape)
                && shape != LocShape::ROOF_DIAGONAL_WITH_ROOFEDGE
                && level > 0
            {
                self.mapo[level as usize][x as usize][z as usize] |= 0x924;
            }

            if loc.blockwalk {
                if let Some(cmap) = collision.as_deref_mut() {
                    cmap.add_loc(x, z, loc.width, loc.length, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_STRAIGHT {
            let model = if loc.anim == -1 {
                loc.get_model(cache, 0, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, 0, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.set_wall(level, x, z, y, WSHAPE0[angle as usize], 0, model, None, typecode, typecode2);

            if angle == LocAngle::WEST {
                if loc.shadow {
                    self.shadow[level as usize][x as usize][z as usize] = 50;
                    self.shadow[level as usize][x as usize][(z + 1) as usize] = 50;
                }

                if loc.occlude {
                    self.mapo[level as usize][x as usize][z as usize] |= 0x249;
                }
            } else if angle == LocAngle::NORTH {
                if loc.shadow {
                    self.shadow[level as usize][x as usize][(z + 1) as usize] = 50;
                    self.shadow[level as usize][(x + 1) as usize][(z + 1) as usize] = 50;
                }

                if loc.occlude {
                    self.mapo[level as usize][x as usize][(z + 1) as usize] |= 0x492;
                }
            } else if angle == LocAngle::EAST {
                if loc.shadow {
                    self.shadow[level as usize][(x + 1) as usize][z as usize] = 50;
                    self.shadow[level as usize][(x + 1) as usize][(z + 1) as usize] = 50;
                }

                if loc.occlude {
                    self.mapo[level as usize][(x + 1) as usize][z as usize] |= 0x249;
                }
            } else if angle == LocAngle::SOUTH {
                if loc.shadow {
                    self.shadow[level as usize][x as usize][z as usize] = 50;
                    self.shadow[level as usize][(x + 1) as usize][z as usize] = 50;
                }

                if loc.occlude {
                    self.mapo[level as usize][x as usize][z as usize] |= 0x492;
                }
            }

            if loc.blockwalk {
                if let Some(cmap) = collision.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }

            if loc.wallwidth != 16 {
                world.move_decor(level, x, z, loc.wallwidth);
            }
        } else if shape == LocShape::WALL_DIAGONAL_CORNER {
            let model = if loc.anim == -1 {
                loc.get_model(cache, 1, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, 1, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.set_wall(level, x, z, y, WSHAPE1[angle as usize], 0, model, None, typecode, typecode2);

            if loc.shadow {
                if angle == LocAngle::WEST {
                    self.shadow[level as usize][x as usize][(z + 1) as usize] = 50;
                } else if angle == LocAngle::NORTH {
                    self.shadow[level as usize][(x + 1) as usize][(z + 1) as usize] = 50;
                } else if angle == LocAngle::EAST {
                    self.shadow[level as usize][(x + 1) as usize][z as usize] = 50;
                } else if angle == LocAngle::SOUTH {
                    self.shadow[level as usize][x as usize][z as usize] = 50;
                }
            }

            if loc.blockwalk {
                if let Some(cmap) = collision.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_L {
            let offset = (angle + 1) & 0x3;

            let (model1, model2) = if loc.anim == -1 {
                (
                    loc.get_model(cache, 2, angle + 4, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model),
                    loc.get_model(cache, 2, offset, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model),
                )
            } else {
                (
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 2, angle + 4, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    ))),
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 2, offset, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    ))),
                )
            };

            world.set_wall(
                level,
                x,
                z,
                y,
                WSHAPE0[angle as usize],
                WSHAPE0[offset as usize],
                model1,
                model2,
                typecode,
                typecode2,
            );

            if loc.occlude {
                if angle == LocAngle::WEST {
                    self.mapo[level as usize][x as usize][z as usize] |= 0x249;
                    self.mapo[level as usize][x as usize][(z + 1) as usize] |= 0x492;
                } else if angle == LocAngle::NORTH {
                    self.mapo[level as usize][x as usize][(z + 1) as usize] |= 0x492;
                    self.mapo[level as usize][(x + 1) as usize][z as usize] |= 0x249;
                } else if angle == LocAngle::EAST {
                    self.mapo[level as usize][(x + 1) as usize][z as usize] |= 0x249;
                    self.mapo[level as usize][x as usize][z as usize] |= 0x492;
                } else if angle == LocAngle::SOUTH {
                    self.mapo[level as usize][x as usize][z as usize] |= 0x492;
                    self.mapo[level as usize][x as usize][z as usize] |= 0x249;
                }
            }

            if loc.blockwalk {
                if let Some(cmap) = collision.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }

            if loc.wallwidth != 16 {
                world.move_decor(level, x, z, loc.wallwidth);
            }
        } else if shape == LocShape::WALL_SQUARE_CORNER {
            let model = if loc.anim == -1 {
                loc.get_model(cache, 3, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, 3, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.set_wall(level, x, z, y, WSHAPE1[angle as usize], 0, model, None, typecode, typecode2);

            if loc.shadow {
                if angle == LocAngle::WEST {
                    self.shadow[level as usize][x as usize][(z + 1) as usize] = 50;
                } else if angle == LocAngle::NORTH {
                    self.shadow[level as usize][(x + 1) as usize][(z + 1) as usize] = 50;
                } else if angle == LocAngle::EAST {
                    self.shadow[level as usize][(x + 1) as usize][z as usize] = 50;
                } else if angle == LocAngle::SOUTH {
                    self.shadow[level as usize][x as usize][z as usize] = 50;
                }
            }

            if loc.blockwalk {
                if let Some(cmap) = collision.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_DIAGONAL {
            let model = if loc.anim == -1 {
                loc.get_model(cache, shape, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, shape, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.add_scenery(level, x, z, y, model, typecode, typecode2, 1, 1, 0);

            if loc.blockwalk {
                if let Some(cmap) = collision.as_mut() {
                    cmap.add_loc(x, z, loc.width, loc.length, angle, loc.blockrange);
                }
            }
        } else {
            if loc.hillskew {
                if angle == 1 {
                    std::mem::swap(&mut height_nw, &mut height_ne);
                    std::mem::swap(&mut height_ne, &mut height_se);
                    std::mem::swap(&mut height_se, &mut height_sw);
                } else if angle == 2 {
                    std::mem::swap(&mut height_nw, &mut height_se);
                    std::mem::swap(&mut height_ne, &mut height_sw);
                } else if angle == 3 {
                    std::mem::swap(&mut height_nw, &mut height_sw);
                    std::mem::swap(&mut height_sw, &mut height_se);
                    std::mem::swap(&mut height_se, &mut height_ne);
                }
            }

            if shape == LocShape::WALLDECOR_STRAIGHT_NOOFFSET {
                let model = if loc.anim == -1 {
                    loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                };

                world.set_decor(
                    level, x, z, y, 0, 0, typecode, model, typecode2, angle * 512,
                    WSHAPE0[angle as usize],
                );
            } else if shape == LocShape::WALLDECOR_STRAIGHT_OFFSET {
                let mut wallwidth = 16;
                let wall_typecode = world.wall_type(level, x, z);
                if wall_typecode > 0 {
                    wallwidth = cache.loc(((wall_typecode >> 14) & 0x7fff) as usize).wallwidth;
                }

                let model = if loc.anim == -1 {
                    loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                };

                world.set_decor(
                    level,
                    x,
                    z,
                    y,
                    DECORXOF[angle as usize] * wallwidth,
                    DECORZOF[angle as usize] * wallwidth,
                    typecode,
                    model,
                    typecode2,
                    angle * 512,
                    WSHAPE0[angle as usize],
                );
            } else if shape == LocShape::WALLDECOR_DIAGONAL_OFFSET {
                let model = if loc.anim == -1 {
                    loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                };

                world.set_decor(level, x, z, y, 0, 0, typecode, model, typecode2, angle, 256);
            } else if shape == LocShape::WALLDECOR_DIAGONAL_NOOFFSET {
                let model = if loc.anim == -1 {
                    loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                };

                world.set_decor(level, x, z, y, 0, 0, typecode, model, typecode2, angle, 512);
            } else if shape == LocShape::WALLDECOR_DIAGONAL_BOTH {
                let model = if loc.anim == -1 {
                    loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                };

                world.set_decor(level, x, z, y, 0, 0, typecode, model, typecode2, angle, 768);
            }
        }
    }

    /// `getVisBelowLevel(level, stx, stz)` from client-ts (ClientBuild.ts
    /// 1137-1142): which level's collision a loc belongs to.
    fn get_vis_below_level(mapl: &[Vec<Vec<u8>>], level: i32, stx: i32, stz: i32) -> i32 {
        if mapl[level as usize][stx as usize][stz as usize] as i32 & MapFlag::VIS_BELOW == 0 {
            return if level <= 0 || mapl[1][stx as usize][stz as usize] as i32 & MapFlag::LINK_BELOW == 0 {
                level
            } else {
                level - 1
            };
        }

        0
    }

    /// `perlinNoise(x, z)` from client-ts: fallback terrain for map squares
    /// with no ground data (level 0 opcode-0 tiles).
    fn perlin_noise(x: i32, z: i32) -> i32 {
        let value = Self::interpolated_noise(x + 45365, z + 91923, 4)
            + ((Self::interpolated_noise(x + 10294, z + 37821, 2) - 128) >> 1)
            + ((Self::interpolated_noise(x, z, 1) - 128) >> 2)
            - 128;
        let value = ((value as f64 * 0.3) as i32) + 35;
        value.clamp(10, 60)
    }

    fn interpolated_noise(x: i32, z: i32, scale: i32) -> i32 {
        let int_x = x / scale;
        let frac_x = x & (scale - 1);
        let int_z = z / scale;
        let frac_z = z & (scale - 1);
        let v1 = Self::smooth_noise(int_x, int_z);
        let v2 = Self::smooth_noise(int_x + 1, int_z);
        let v3 = Self::smooth_noise(int_x, int_z + 1);
        let v4 = Self::smooth_noise(int_x + 1, int_z + 1);
        let i1 = Self::interpolate(v1, v2, frac_x, scale);
        let i2 = Self::interpolate(v3, v4, frac_x, scale);
        Self::interpolate(i1, i2, frac_z, scale)
    }

    fn interpolate(a: i32, b: i32, x: i32, scale: i32) -> i32 {
        let f = (65536 - Pix3D::cos_table()[((x * 1024) / scale) as usize]) >> 1;
        ((a * (65536 - f)) >> 16) + ((b * f) >> 16)
    }

    fn smooth_noise(x: i32, y: i32) -> i32 {
        let corners = Self::noise(x - 1, y - 1)
            + Self::noise(x + 1, y - 1)
            + Self::noise(x - 1, y + 1)
            + Self::noise(x + 1, y + 1);
        let sides =
            Self::noise(x - 1, y) + Self::noise(x + 1, y) + Self::noise(x, y - 1) + Self::noise(x, y + 1);
        let center = Self::noise(x, y);
        // i32 division truncates toward zero, matching the TS `| 0`
        corners / 16 + sides / 8 + center / 4
    }

    /// `noise(x, y)` from client-ts. The TS uses BigInt for the cubic term
    /// (int32 overflows), so this port computes it in i128 before masking.
    fn noise(x: i32, y: i32) -> i32 {
        let n = x.wrapping_add(y.wrapping_mul(57));
        let n1 = ((n << 13) ^ n) as i128;
        let v = (n1 * (n1 * n1 * 15731 + 789221) + 1376312589) & 0x7fff_ffff;
        ((v >> 19) & 0xff) as i32
    }
}
