//! Port of `~/experiments/Server/webclient/src/client/ClientBuild.ts` — the
//! map-build scratch arrays and the ground decode (`loadGround`, plus the
//! perlin-noise terrain fallback it calls). The scene build
//! (`loadLocations`/`addLoc`) is ported here too, as is `finishBuild`
//! (the light/occlusion passes) and `fadeAdjacent`. `loadLocations` writes
//! into the `World`/`CollisionMap`s of the owning `Client`, and `addLoc`
//! records the per-tile shadow/occlusion bits `finishBuild` consumes.
//!
//! `loadGround` writes the client's `groundh` and `mapl` directly (the TS
//! passes its `Client.groundh`/`Client.mapl` references into the
//! constructor); the per-build floor arrays live on this struct as in TS,
//! and so do the `shadow`/`mapo` scratch grids (`finishBuild` reads them).
use crate::config::Cache;
use crate::dash3d::world::LevelHeightmaps;
use crate::dash3d::{
    BuildArea, ClientLocAnim, CollisionMap, LocAngle, LocShape, MapFlag, SceneModel,
    TerrainOverlayShape, World,
};
use crate::graphics::{Colour, Pix3D, Pix3DDraw};
use crate::io::{OnDemand, Packet};

/// `ClientBuild.WSHAPE0` from client-ts.
const WSHAPE0: [i32; 4] = [1, 2, 4, 8];
/// `ClientBuild.WSHAPE1` from client-ts.
const WSHAPE1: [i32; 4] = [16, 32, 64, 128];
/// `ClientBuild.DECORXOF` from client-ts.
const DECORXOF: [i32; 4] = [1, 0, -1, 0];
/// `ClientBuild.DECORZOF` from client-ts.
const DECORZOF: [i32; 4] = [0, -1, 0, 1];

/// Stand-in for JS `Math.random()` (returns `[0, 1)`), seeded from the
/// clock like the other client random stand-ins; `finishBuild`'s
/// hue/lighting jitter is not reproducible in TS either.
pub(crate) fn random_float() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ((nanos >> 20) % 1_000_000) as f64 / 1_000_000.0
}

pub struct ClientBuild {
    /// `ClientBuild.lowMem` Java/TS static default is `true` until
    /// `setHighMem`/`setLowMem`. Live `client-play` is highmem
    /// (`ClientConfig.lowmem = false`); `map_build` copies that here before
    /// `addLoc` / `finishBuild`.
    pub low_mem: bool,
    /// Skip loc mesh decode in `addLoc`: the typecode, collision and
    /// shadow/mapo side-effects still run, but every model comes back
    /// `None` (host-side world reads place typecodes without meshes).
    pub skip_loc_models: bool,
    /// `ClientBuild.minusedlevel` from client-ts; consumed by `addLoc`.
    pub minusedlevel: i32,
    /// `ClientBuild.hueOff` from client-ts (a TS static, instance here);
    /// `finishBuild` random-walks and clamps it to the TS bounds.
    pub hue_off: i32,
    /// `ClientBuild.ligOff` from client-ts (a TS static, instance here).
    pub lig_off: i32,
    /// `floort1[level][x][z]` / `floort2[level][x][z]` floor type ids.
    floort1: Vec<Vec<Vec<u8>>>,
    floort2: Vec<Vec<Vec<u8>>>,
    /// `floors[level][x][z]` overlay shape, `floorr[level][x][z]` rotation.
    floors: Vec<Vec<Vec<u8>>>,
    floorr: Vec<Vec<Vec<u8>>>,
    /// `this.shadow[level][x][z]` shade values written by `addLoc`, sized
    /// `[LEVELS][SIZE + 1][SIZE + 1]`; `finishBuild` reads them and
    /// `fadeAdjacent` pins the level-0 seam.
    pub shadow: Vec<Vec<Vec<u8>>>,
    /// `this.mapo[level][x][z]` map-occlusion bits written by `addLoc`;
    /// `finishBuild` consumes them in the occluder pass.
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
            skip_loc_models: false,
            minusedlevel: 0,
            hue_off: ((random_float() * 17.0) as i32) - 8,
            lig_off: ((random_float() * 33.0) as i32) - 16,
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

    /// `finishBuild`'s ground-tile gate (Java `ClientBuild.java:972`):
    /// interior z (`1..SIZE-1`), and in low-mem the tile must not be
    /// ForceHighDetail and must resolve to `minusedlevel`. The x gate
    /// (`1..SIZE-1`) is the outer loop's, kept in `finish_build`. `pub`
    /// rather than `pub(crate)` because the `tests/client_build.rs`
    /// integration tests call it.
    pub fn ground_tile_visible(
        mapl: &[Vec<Vec<u8>>],
        level: i32,
        x: i32,
        z: i32,
        low_mem: bool,
        minusedlevel: i32,
    ) -> bool {
        (1..BuildArea::SIZE - 1).contains(&z)
            && (!low_mem
                || (mapl[level as usize][x as usize][z as usize] as i32
                    & MapFlag::FORCE_HIGH_DETAIL
                    == 0
                    && Self::get_vis_below_level(mapl, level, x, z) == minusedlevel))
    }

    /// `checkLocations(src, xOffset, zOffset)` from client-ts: decode the
    /// gsmart loc-id/pos delta stream and report whether every in-area loc's
    /// models are downloaded. Out-of-area tiles still consume the packet
    /// bytes, and `skip` state tracks per-loc model-check progress.
    pub fn check_locations(&self, cache: &Cache, src: &[u8], x_offset: i32, z_offset: i32) -> bool {
        Self::check_locations_low_mem(self.low_mem, cache, src, x_offset, z_offset)
    }

    /// `checkLocations` without a `ClientBuild` instance: only `lowMem` is
    /// read, so `checkScene`'s per-frame wait loop calls this instead of
    /// allocating a full build-area of scratch grids every frame.
    pub fn check_locations_low_mem(
        low_mem: bool,
        cache: &Cache,
        src: &[u8],
        x_offset: i32,
        z_offset: i32,
    ) -> bool {
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
                        if shape != 22 || !low_mem || loc.active || loc.forcedecor {
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

    /// `skip_loc_models` gate for one loc model: return `None` without
    /// running the builder so host-side placement keeps the typecode without
    /// touching the mesh cache or decoding animated models.
    fn take_model<F>(&self, build: F) -> Option<SceneModel>
    where
        F: FnOnce() -> Option<SceneModel>,
    {
        if self.skip_loc_models {
            None
        } else {
            build()
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
                let model = self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 22, angle, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 22, shape, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                });

                world.set_ground_decor(model, level, x, z, y, typecode, typecode2);

                if loc.blockwalk && loc.active {
                    if let Some(cmap) = collision.as_mut() {
                        cmap.block_ground(x, z);
                    }
                }
            }
        } else if shape == LocShape::CENTREPIECE_STRAIGHT || shape == LocShape::CENTREPIECE_DIAGONAL {
            let model = self.take_model(|| {
                if loc.anim == -1 {
                    loc.get_model(cache, 10, angle, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 10, angle, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                }
            });

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
            let model = self.take_model(|| {
                if loc.anim == -1 {
                    loc.get_model(cache, shape, angle, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, shape, angle, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                }
            });

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
            let model = self.take_model(|| {
                if loc.anim == -1 {
                    loc.get_model(cache, 0, angle, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 0, angle, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                }
            });

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
            let model = self.take_model(|| {
                if loc.anim == -1 {
                    loc.get_model(cache, 1, angle, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 1, angle, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                }
            });

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

            let (model1, model2) = (
                self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 2, angle + 4, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 2, angle + 4, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                }),
                self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 2, offset, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 2, offset, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                }),
            );

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
            let model = self.take_model(|| {
                if loc.anim == -1 {
                    loc.get_model(cache, 3, angle, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, 3, angle, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                }
            });

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
            let model = self.take_model(|| {
                if loc.anim == -1 {
                    loc.get_model(cache, shape, angle, height_sw, height_se, height_ne, height_nw, -1)
                        .map(SceneModel::Model)
                } else {
                    Some(SceneModel::LocAnim(ClientLocAnim::new(
                        cache, loc_id, shape, angle, height_sw, height_se, height_ne, height_nw,
                        loc.anim as usize, true, loop_cycle,
                    )))
                }
            });

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
                let model = self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                });

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

                let model = self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                });

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
                let model = self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                });

                world.set_decor(level, x, z, y, 0, 0, typecode, model, typecode2, angle, 256);
            } else if shape == LocShape::WALLDECOR_DIAGONAL_NOOFFSET {
                let model = self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                });

                world.set_decor(level, x, z, y, 0, 0, typecode, model, typecode2, angle, 512);
            } else if shape == LocShape::WALLDECOR_DIAGONAL_BOTH {
                let model = self.take_model(|| {
                    if loc.anim == -1 {
                        loc.get_model(cache, 4, 0, height_sw, height_se, height_ne, height_nw, -1)
                            .map(SceneModel::Model)
                    } else {
                        Some(SceneModel::LocAnim(ClientLocAnim::new(
                            cache, loc_id, 4, 0, height_sw, height_se, height_ne, height_nw,
                            loc.anim as usize, true, loop_cycle,
                        )))
                    }
                });

                world.set_decor(level, x, z, y, 0, 0, typecode, model, typecode2, angle, 768);
            }
        }
    }

    /// `changeLocAvailable(id, shape)` from client-ts (ClientBuild.ts
    /// 1160-1168): remap the shape codes the server sends to the shape the
    /// loc's model table keys on, then report whether that model is ready.
    pub fn change_loc_available(cache: &Cache, id: i32, mut shape: i32) -> bool {
        if shape == 11 {
            shape = 10;
        }
        if (5..=8).contains(&shape) {
            shape = 4;
        }
        cache.loc(id as usize).check_model(shape)
    }

    /// `changeLocUnchecked(...)` from client-ts (ClientBuild.ts
    /// 1171-1392): place one loc from a change-loc packet into the world
    /// and collision map, with no low-mem visibility gating (unlike
    /// `addLoc`). Heights come from `true_level` while the world writes use
    /// `level`; the height variables follow the TS names (`heightNW` is
    /// `[x+1][z+1]`, `heightNE` is `[x][z+1]`, the reverse of `addLoc`).
    /// `loop_cycle` stands in for the TS `Client.loopCycle`. With
    /// `skip_loc_models` every model comes back `None` (host-side placement
    /// without mesh decode), as in `addLoc`.
    #[allow(clippy::too_many_arguments)]
    pub fn change_loc_unchecked(
        cache: &Cache,
        world: &mut World,
        mut cmap: Option<&mut CollisionMap>,
        groundh: &LevelHeightmaps,
        level: i32,
        x: i32,
        z: i32,
        loc_id: i32,
        shape: i32,
        angle: i32,
        true_level: i32,
        loop_cycle: i32,
        skip_loc_models: bool,
    ) {
        let mut height_sw = groundh[true_level as usize][x as usize][z as usize];
        let mut height_se = groundh[true_level as usize][(x + 1) as usize][z as usize];
        let mut height_nw = groundh[true_level as usize][(x + 1) as usize][(z + 1) as usize];
        let mut height_ne = groundh[true_level as usize][x as usize][(z + 1) as usize];
        let y = (height_sw + height_se + height_nw + height_ne) >> 2;

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
            let model = if skip_loc_models {
                None
            } else if loc.anim == -1 {
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
                if let Some(cmap) = cmap.as_deref_mut() {
                    cmap.block_ground(x, z);
                }
            }
        } else if shape == LocShape::CENTREPIECE_STRAIGHT || shape == LocShape::CENTREPIECE_DIAGONAL {
            let model = if skip_loc_models {
                None
            } else if loc.anim == -1 {
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

                world.add_scenery(level, x, z, y, Some(model), typecode, typecode2, width, height, yaw);
            }

            if loc.blockwalk {
                if let Some(cmap) = cmap.as_deref_mut() {
                    cmap.add_loc(x, z, loc.width, loc.length, angle, loc.blockrange);
                }
            }
        } else if shape >= LocShape::ROOF_STRAIGHT {
            let model = if skip_loc_models {
                None
            } else if loc.anim == -1 {
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
                if let Some(cmap) = cmap.as_deref_mut() {
                    cmap.add_loc(x, z, loc.width, loc.length, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_STRAIGHT {
            let model = if skip_loc_models {
                None
            } else if loc.anim == -1 {
                loc.get_model(cache, 0, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, 0, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.set_wall(level, x, z, y, WSHAPE0[angle as usize], 0, model, None, typecode, typecode2);

            if loc.blockwalk {
                if let Some(cmap) = cmap.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_DIAGONAL_CORNER {
            let model = if skip_loc_models {
                None
            } else if loc.anim == -1 {
                loc.get_model(cache, 1, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, 1, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.set_wall(level, x, z, y, WSHAPE1[angle as usize], 0, model, None, typecode, typecode2);

            if loc.blockwalk {
                if let Some(cmap) = cmap.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_L {
            let offset = (angle + 1) & 0x3;

            let (model1, model2) = if skip_loc_models {
                (None, None)
            } else if loc.anim == -1 {
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

            if loc.blockwalk {
                if let Some(cmap) = cmap.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_SQUARE_CORNER {
            let model = if skip_loc_models {
                None
            } else if loc.anim == -1 {
                loc.get_model(cache, 3, angle, height_sw, height_se, height_ne, height_nw, -1)
                    .map(SceneModel::Model)
            } else {
                Some(SceneModel::LocAnim(ClientLocAnim::new(
                    cache, loc_id, 3, angle, height_sw, height_se, height_ne, height_nw,
                    loc.anim as usize, true, loop_cycle,
                )))
            };

            world.set_wall(level, x, z, y, WSHAPE1[angle as usize], 0, model, None, typecode, typecode2);

            if loc.blockwalk {
                if let Some(cmap) = cmap.as_deref_mut() {
                    cmap.add_wall(x, z, shape, angle, loc.blockrange);
                }
            }
        } else if shape == LocShape::WALL_DIAGONAL {
            let model = if skip_loc_models {
                None
            } else if loc.anim == -1 {
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
                // last `cmap` use: `as_mut` (as in `addLoc`) — a reborrow
                // would otherwise be a clippy `needless_option_as_deref`
                if let Some(cmap) = cmap.as_mut() {
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
                let model = if skip_loc_models {
                    None
                } else if loc.anim == -1 {
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

                let model = if skip_loc_models {
                    None
                } else if loc.anim == -1 {
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
                let model = if skip_loc_models {
                    None
                } else if loc.anim == -1 {
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
                let model = if skip_loc_models {
                    None
                } else if loc.anim == -1 {
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
                let model = if skip_loc_models {
                    None
                } else if loc.anim == -1 {
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

    /// `finishBuild(world, collision)` from client-ts (ClientBuild.ts
    /// 75-497): the ground-colour/light pass, the layer pass, `pushDown`
    /// for `LinkBelow` tiles, and the occluder pass. `cache` supplies the
    /// flo table, `pix3d` the texture averages (TS `Pix3D.getTextureAverage`
    /// is a per-client `Pix3DDraw` here), `groundh`/`mapl` are the
    /// `Client`'s scene grids read through the TS constructor references.
    /// Ends with the `World.shareLight` call (TS 331) so loc/wall/decor
    /// models are lit as the TS `finishBuild` does.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_build(
        &mut self,
        cache: &Cache,
        pix3d: &mut Pix3DDraw,
        world: &mut World,
        collisions: &mut [CollisionMap; 4],
        groundh: &LevelHeightmaps,
        mapl: &[Vec<Vec<u8>>],
    ) {
        // TS 76-90: `MapFlag.Block` tiles block their LinkBelow-corrected
        // level's collision grid.
        for level in 0..BuildArea::LEVELS {
            for x in 0..BuildArea::SIZE {
                for z in 0..BuildArea::SIZE {
                    if mapl[level as usize][x as usize][z as usize] as i32 & MapFlag::BLOCK != 0 {
                        let mut true_level = level;
                        if mapl[1][x as usize][z as usize] as i32 & MapFlag::LINK_BELOW != 0 {
                            true_level -= 1;
                        }
                        if true_level >= 0 {
                            collisions[true_level as usize].block_ground(x, z);
                        }
                    }
                }
            }
        }

        // TS 92-104: hue/lighting jitter, clamped to the TS bounds.
        self.hue_off += ((random_float() * 5.0) as i32) - 2;
        self.hue_off = self.hue_off.clamp(-8, 8);
        self.lig_off += ((random_float() * 5.0) as i32) - 2;
        self.lig_off = self.lig_off.clamp(-16, 16);

        // TS scratch totals (ClientBuild.ts 65-71), one per finishBuild.
        let mut huetot = vec![0i32; BuildArea::SIZE as usize];
        let mut sattot = vec![0i32; BuildArea::SIZE as usize];
        let mut ligtot = vec![0i32; BuildArea::SIZE as usize];
        let mut comtot = vec![0i32; BuildArea::SIZE as usize];
        let mut tot = vec![0i32; BuildArea::SIZE as usize];

        for level in 0..BuildArea::LEVELS {
            // TS 108-132: shadow -> lightmap.
            let shademap = &self.shadow[level as usize];
            const LIGHT_AMBIENT: i32 = 96;
            const LIGHT_ATTENUATION: i32 = 768;
            const LIGHT_X: i32 = -50;
            const LIGHT_Y: i32 = -10;
            const LIGHT_Z: i32 = -50;
            let light_mag = (f64::sqrt(
                (LIGHT_X * LIGHT_X + LIGHT_Y * LIGHT_Y + LIGHT_Z * LIGHT_Z) as f64,
            )) as i32;
            let light_magnitude = (LIGHT_ATTENUATION * light_mag) >> 8;
            let mut lightmap = vec![
                vec![0i32; (BuildArea::SIZE + 1) as usize];
                (BuildArea::SIZE + 1) as usize
            ];

            for z in 1..BuildArea::SIZE - 1 {
                for x in 1..BuildArea::SIZE - 1 {
                    let dx = groundh[level as usize][(x + 1) as usize][z as usize]
                        - groundh[level as usize][(x - 1) as usize][z as usize];
                    let dz = groundh[level as usize][x as usize][(z + 1) as usize]
                        - groundh[level as usize][x as usize][(z - 1) as usize];
                    let len = (f64::sqrt((dx * dx + 65536 + dz * dz) as f64)) as i32;
                    let normal_x = (dx << 8) / len;
                    let normal_y = 65536 / len;
                    let normal_z = (dz << 8) / len;
                    let light = LIGHT_AMBIENT
                        + ((LIGHT_X * normal_x + LIGHT_Y * normal_y + LIGHT_Z * normal_z)
                            / light_magnitude);
                    let shade = (shademap[(x - 1) as usize][z as usize] >> 2) as i32
                        + (shademap[(x + 1) as usize][z as usize] >> 3) as i32
                        + (shademap[x as usize][(z - 1) as usize] >> 2) as i32
                        + (shademap[x as usize][(z + 1) as usize] >> 3) as i32
                        + (shademap[x as usize][z as usize] >> 1) as i32;
                    lightmap[x as usize][z as usize] = light - shade;
                }
            }

            // TS 138-143: zero the running floor-type totals per level.
            huetot.fill(0);
            sattot.fill(0);
            ligtot.fill(0);
            comtot.fill(0);
            tot.fill(0);

            // TS 145-322: the 10x10 blend window over floort1, setting the
            // ground squares. The `| 0` truncations are plain i32 division
            // (truncation toward zero, same as JS); `blendCom`/`blendTot`
            // are nonzero whenever t1 > 0 because the tile itself is inside
            // its own window.
            for x0 in -5..BuildArea::SIZE + 5 {
                for z0 in 0..BuildArea::SIZE {
                    let x1 = x0 + 5;
                    if (0..BuildArea::SIZE).contains(&x1) {
                        let t1 = self.floort1[level as usize][x1 as usize][z0 as usize] as i32;
                        if t1 > 0 {
                            let flo = cache.flo((t1 - 1) as usize);
                            huetot[z0 as usize] += flo.underlay_hue;
                            sattot[z0 as usize] += flo.saturation;
                            ligtot[z0 as usize] += flo.lightness;
                            comtot[z0 as usize] += flo.chroma;
                            tot[z0 as usize] += 1;
                        }
                    }

                    let x2 = x0 - 5;
                    if (0..BuildArea::SIZE).contains(&x2) {
                        let t1 = self.floort1[level as usize][x2 as usize][z0 as usize] as i32;
                        if t1 > 0 {
                            let flo = cache.flo((t1 - 1) as usize);
                            huetot[z0 as usize] -= flo.underlay_hue;
                            sattot[z0 as usize] -= flo.saturation;
                            ligtot[z0 as usize] -= flo.lightness;
                            comtot[z0 as usize] -= flo.chroma;
                            tot[z0 as usize] -= 1;
                        }
                    }
                }

                if (1..BuildArea::SIZE - 1).contains(&x0) {
                    let mut blend_hue = 0;
                    let mut blend_sat = 0;
                    let mut blend_lig = 0;
                    let mut blend_com = 0;
                    let mut blend_tot = 0;

                    for z0 in -5..BuildArea::SIZE + 5 {
                        let dz1 = z0 + 5;
                        if (0..BuildArea::SIZE).contains(&dz1) {
                            blend_hue += huetot[dz1 as usize];
                            blend_sat += sattot[dz1 as usize];
                            blend_lig += ligtot[dz1 as usize];
                            blend_com += comtot[dz1 as usize];
                            blend_tot += tot[dz1 as usize];
                        }

                        let dz2 = z0 - 5;
                        if (0..BuildArea::SIZE).contains(&dz2) {
                            blend_hue -= huetot[dz2 as usize];
                            blend_sat -= sattot[dz2 as usize];
                            blend_lig -= ligtot[dz2 as usize];
                            blend_com -= comtot[dz2 as usize];
                            blend_tot -= tot[dz2 as usize];
                        }

                        // TS short-circuits left to right: the mapl reads
                        // below only run for interior z0 (TS 200-205).
                        if Self::ground_tile_visible(mapl, level, x0, z0, self.low_mem, self.minusedlevel) {
                            let t1 =
                                self.floort1[level as usize][x0 as usize][z0 as usize] as i32;
                            let t2 =
                                self.floort2[level as usize][x0 as usize][z0 as usize] as i32;

                            if t1 > 0 || t2 > 0 {
                                let height_sw = groundh[level as usize][x0 as usize][z0 as usize];
                                let height_se =
                                    groundh[level as usize][(x0 + 1) as usize][z0 as usize];
                                let height_ne =
                                    groundh[level as usize][(x0 + 1) as usize][(z0 + 1) as usize];
                                let height_nw =
                                    groundh[level as usize][x0 as usize][(z0 + 1) as usize];

                                let light_sw = lightmap[x0 as usize][z0 as usize];
                                let light_se = lightmap[(x0 + 1) as usize][z0 as usize];
                                let light_ne = lightmap[(x0 + 1) as usize][(z0 + 1) as usize];
                                let light_nw = lightmap[x0 as usize][(z0 + 1) as usize];

                                let mut t1_colour = -1;
                                let mut t1_rand_colour = -1;

                                if t1 > 0 {
                                    let hue = (blend_hue * 256) / blend_com;
                                    let sat = blend_sat / blend_tot;
                                    let lig = blend_lig / blend_tot;
                                    t1_colour = Self::get_table(hue, sat, lig);

                                    let random_hue = (hue + self.hue_off) & 0xff;
                                    let random_lig = (lig + self.lig_off).clamp(0, 255);
                                    t1_rand_colour = Self::get_table(random_hue, sat, random_lig);
                                }

                                if level > 0 {
                                    let mut occludes = t1 != 0
                                        || self.floors[level as usize][x0 as usize][z0 as usize]
                                            == TerrainOverlayShape::PLAIN as u8;

                                    if t2 > 0 && !cache.flo((t2 - 1) as usize).occlude {
                                        occludes = false;
                                    }

                                    // occludes && flat
                                    if occludes
                                        && height_sw == height_se
                                        && height_sw == height_ne
                                        && height_sw == height_nw
                                    {
                                        self.mapo[level as usize][x0 as usize][z0 as usize] |=
                                            0x924;
                                    }
                                }

                                let mut underlay = 0;
                                if t1_colour != -1 {
                                    underlay = Pix3D::colour_table()
                                        [Self::get_ucol(t1_rand_colour, 96) as usize];
                                }

                                if t2 == 0 {
                                    world.set_ground(
                                        level,
                                        x0,
                                        z0,
                                        TerrainOverlayShape::PLAIN,
                                        LocAngle::WEST,
                                        -1,
                                        height_sw,
                                        height_se,
                                        height_ne,
                                        height_nw,
                                        Self::get_ucol(t1_colour, light_sw),
                                        Self::get_ucol(t1_colour, light_se),
                                        Self::get_ucol(t1_colour, light_ne),
                                        Self::get_ucol(t1_colour, light_nw),
                                        Colour::BLACK,
                                        Colour::BLACK,
                                        Colour::BLACK,
                                        Colour::BLACK,
                                        underlay,
                                        Colour::BLACK,
                                    );
                                } else {
                                    let shape = self.floors[level as usize][x0 as usize]
                                        [z0 as usize] as i32
                                        + 1;
                                    let rotation =
                                        self.floorr[level as usize][x0 as usize][z0 as usize]
                                            as i32;
                                    let flo = cache.flo((t2 - 1) as usize);

                                    let mut texture = flo.texture;
                                    let t2_colour;
                                    let overlay;
                                    if texture >= 0 {
                                        overlay = pix3d.get_texture_average(texture);
                                        t2_colour = -1;
                                    } else if flo.colour == Colour::MAGENTA {
                                        overlay = 0;
                                        t2_colour = -2;
                                        texture = -1;
                                    } else {
                                        t2_colour =
                                            Self::get_table(flo.hue, flo.saturation, flo.lightness);
                                        overlay = Pix3D::colour_table()
                                            [Self::get_ocol(flo.overlay_hsl, 96) as usize];
                                    }

                                    world.set_ground(
                                        level,
                                        x0,
                                        z0,
                                        shape,
                                        rotation,
                                        texture,
                                        height_sw,
                                        height_se,
                                        height_ne,
                                        height_nw,
                                        Self::get_ucol(t1_colour, light_sw),
                                        Self::get_ucol(t1_colour, light_se),
                                        Self::get_ucol(t1_colour, light_ne),
                                        Self::get_ucol(t1_colour, light_nw),
                                        Self::get_ocol(t2_colour, light_sw),
                                        Self::get_ocol(t2_colour, light_se),
                                        Self::get_ocol(t2_colour, light_ne),
                                        Self::get_ocol(t2_colour, light_nw),
                                        underlay,
                                        overlay,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // TS 325-329: per-tile draw layer.
            for stz in 1..BuildArea::SIZE - 1 {
                for stx in 1..BuildArea::SIZE - 1 {
                    world.set_layer(level, stx, stz, Self::get_vis_below_level(mapl, level, stx, stz));
                }
            }
        }

        // TS 331: `world?.shareLight(64, 768, -50, -10, -50)`.
        world.share_light(64, 768, -50, -10, -50);

        // TS 333-339: `LinkBelow` tiles are pushed down a level.
        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                if mapl[1][x as usize][z as usize] as i32 & MapFlag::LINK_BELOW != 0 {
                    world.push_down(x, z);
                }
            }
        }

        // TS 341-497: the occluder pass. `wall0`/`wall1`/`floor` are the
        // per-top-level occlusion bits (shifted 3 per level); runs of >= 8
        // (walls) or >= 4 (floors) tiles become `World.setOcclude` boxes.
        let mut wall0 = 0x1; // set by walls with rotation 0 or 2
        let mut wall1 = 0x2; // set by walls with rotation 1 or 3
        let mut floor = 0x4; // set by floors which are flat

        for top_level in 0..BuildArea::LEVELS {
            if top_level > 0 {
                wall0 <<= 3;
                wall1 <<= 3;
                floor <<= 3;
            }

            for level in 0..=top_level {
                for tile_z in 0..=BuildArea::SIZE {
                    for tile_x in 0..=BuildArea::SIZE {
                        if self.mapo[level as usize][tile_x as usize][tile_z as usize] & wall0 != 0 {
                            let mut min_tile_z = tile_z;
                            let mut max_tile_z = tile_z;
                            let mut min_level = level;
                            let mut max_level = level;

                            while min_tile_z > 0
                                && self.mapo[level as usize][tile_x as usize]
                                    [(min_tile_z - 1) as usize]
                                    & wall0
                                    != 0
                            {
                                min_tile_z -= 1;
                            }

                            while max_tile_z < BuildArea::SIZE
                                && self.mapo[level as usize][tile_x as usize]
                                    [(max_tile_z + 1) as usize]
                                    & wall0
                                    != 0
                            {
                                max_tile_z += 1;
                            }

                            while min_level > 0 {
                                let mut blocked = false;
                                for z in min_tile_z..=max_tile_z {
                                    if self.mapo[(min_level - 1) as usize][tile_x as usize]
                                        [z as usize]
                                        & wall0
                                        == 0
                                    {
                                        blocked = true;
                                        break;
                                    }
                                }
                                if blocked {
                                    break;
                                }
                                min_level -= 1;
                            }

                            while max_level < top_level {
                                let mut blocked = false;
                                for z in min_tile_z..=max_tile_z {
                                    if self.mapo[(max_level + 1) as usize][tile_x as usize]
                                        [z as usize]
                                        & wall0
                                        == 0
                                    {
                                        blocked = true;
                                        break;
                                    }
                                }
                                if blocked {
                                    break;
                                }
                                max_level += 1;
                            }

                            let area = (max_level + 1 - min_level) * (max_tile_z + 1 - min_tile_z);
                            if area >= 8 {
                                let min_y = groundh[max_level as usize][tile_x as usize]
                                    [min_tile_z as usize]
                                    - 240;
                                let max_x = groundh[min_level as usize][tile_x as usize]
                                    [min_tile_z as usize];

                                world.set_occlude(
                                    top_level,
                                    1,
                                    tile_x * 128,
                                    min_y,
                                    min_tile_z * 128,
                                    tile_x * 128,
                                    max_x,
                                    max_tile_z * 128 + 128,
                                );

                                for l in min_level..=max_level {
                                    for z in min_tile_z..=max_tile_z {
                                        self.mapo[l as usize][tile_x as usize][z as usize] &= !wall0;
                                    }
                                }
                            }
                        }

                        if self.mapo[level as usize][tile_x as usize][tile_z as usize] & wall1 != 0 {
                            let mut min_tile_x = tile_x;
                            let mut max_tile_x = tile_x;
                            let mut min_level = level;
                            let mut max_level = level;

                            while min_tile_x > 0
                                && self.mapo[level as usize][(min_tile_x - 1) as usize]
                                    [tile_z as usize]
                                    & wall1
                                    != 0
                            {
                                min_tile_x -= 1;
                            }

                            while max_tile_x < BuildArea::SIZE
                                && self.mapo[level as usize][(max_tile_x + 1) as usize]
                                    [tile_z as usize]
                                    & wall1
                                    != 0
                            {
                                max_tile_x += 1;
                            }

                            while min_level > 0 {
                                let mut blocked = false;
                                for x in min_tile_x..=max_tile_x {
                                    if self.mapo[(min_level - 1) as usize][x as usize]
                                        [tile_z as usize]
                                        & wall1
                                        == 0
                                    {
                                        blocked = true;
                                        break;
                                    }
                                }
                                if blocked {
                                    break;
                                }
                                min_level -= 1;
                            }

                            while max_level < top_level {
                                let mut blocked = false;
                                for x in min_tile_x..=max_tile_x {
                                    if self.mapo[(max_level + 1) as usize][x as usize]
                                        [tile_z as usize]
                                        & wall1
                                        == 0
                                    {
                                        blocked = true;
                                        break;
                                    }
                                }
                                if blocked {
                                    break;
                                }
                                max_level += 1;
                            }

                            let area = (max_level + 1 - min_level) * (max_tile_x + 1 - min_tile_x);

                            if area >= 8 {
                                let min_y = groundh[max_level as usize][min_tile_x as usize]
                                    [tile_z as usize]
                                    - 240;
                                let max_y = groundh[min_level as usize][min_tile_x as usize]
                                    [tile_z as usize];

                                world.set_occlude(
                                    top_level,
                                    2,
                                    min_tile_x * 128,
                                    min_y,
                                    tile_z * 128,
                                    max_tile_x * 128 + 128,
                                    max_y,
                                    tile_z * 128,
                                );

                                for l in min_level..=max_level {
                                    for x in min_tile_x..=max_tile_x {
                                        self.mapo[l as usize][x as usize][tile_z as usize] &= !wall1;
                                    }
                                }
                            }
                        }

                        if self.mapo[level as usize][tile_x as usize][tile_z as usize] & floor != 0 {
                            let mut min_tile_x = tile_x;
                            let mut max_tile_x = tile_x;
                            let mut min_tile_z = tile_z;
                            let mut max_tile_z = tile_z;

                            while min_tile_z > 0
                                && self.mapo[level as usize][tile_x as usize]
                                    [(min_tile_z - 1) as usize]
                                    & floor
                                    != 0
                            {
                                min_tile_z -= 1;
                            }

                            while max_tile_z < BuildArea::SIZE
                                && self.mapo[level as usize][tile_x as usize]
                                    [(max_tile_z + 1) as usize]
                                    & floor
                                    != 0
                            {
                                max_tile_z += 1;
                            }

                            while min_tile_x > 0 {
                                let mut blocked = false;
                                for z in min_tile_z..=max_tile_z {
                                    if self.mapo[level as usize][(min_tile_x - 1) as usize]
                                        [z as usize]
                                        & floor
                                        == 0
                                    {
                                        blocked = true;
                                        break;
                                    }
                                }
                                if blocked {
                                    break;
                                }
                                min_tile_x -= 1;
                            }

                            while max_tile_x < BuildArea::SIZE {
                                let mut blocked = false;
                                for z in min_tile_z..=max_tile_z {
                                    if self.mapo[level as usize][(max_tile_x + 1) as usize]
                                        [z as usize]
                                        & floor
                                        == 0
                                    {
                                        blocked = true;
                                        break;
                                    }
                                }
                                if blocked {
                                    break;
                                }
                                max_tile_x += 1;
                            }

                            if (max_tile_x + 1 - min_tile_x) * (max_tile_z + 1 - min_tile_z) >= 4 {
                                let y = groundh[level as usize][min_tile_x as usize]
                                    [min_tile_z as usize];

                                world.set_occlude(
                                    top_level,
                                    4,
                                    min_tile_x * 128,
                                    y,
                                    min_tile_z * 128,
                                    max_tile_x * 128 + 128,
                                    y,
                                    max_tile_z * 128 + 128,
                                );

                                for x in min_tile_x..=max_tile_x {
                                    for z in min_tile_z..=max_tile_z {
                                        self.mapo[level as usize][x as usize][z as usize] &= !floor;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// `fadeAdjacent(startZ, startX, endZ, endX)` from client-ts
    /// (ClientBuild.ts 543-565): smooth the level-0 seam around a map
    /// square with no ground data — the border tiles inherit their
    /// neighbour's height and the square's shadow is pinned at 127.
    pub fn fade_adjacent(
        &mut self,
        groundh: &mut LevelHeightmaps,
        start_z: i32,
        start_x: i32,
        end_z: i32,
        end_x: i32,
    ) {
        for z in start_z..=start_z + end_z {
            for x in start_x..=start_x + end_x {
                if (0..BuildArea::SIZE).contains(&x) && (0..BuildArea::SIZE).contains(&z) {
                    self.shadow[0][x as usize][z as usize] = 127;

                    if start_x == x && x > 0 {
                        groundh[0][x as usize][z as usize] = groundh[0][(x - 1) as usize][z as usize];
                    }

                    if start_x + end_x == x && x < BuildArea::SIZE - 1 {
                        groundh[0][x as usize][z as usize] = groundh[0][(x + 1) as usize][z as usize];
                    }

                    if start_z == z && z > 0 {
                        groundh[0][x as usize][z as usize] = groundh[0][x as usize][(z - 1) as usize];
                    }

                    if start_z + end_z == z && z < BuildArea::SIZE - 1 {
                        groundh[0][x as usize][z as usize] = groundh[0][x as usize][(z + 1) as usize];
                    }
                }
            }
        }
    }

    /// `getTable(hue, saturation, lightness)` from client-ts
    /// (ClientBuild.ts 1145-1158): the HSL-packed colour-table index.
    fn get_table(hue: i32, mut saturation: i32, lightness: i32) -> i32 {
        if lightness > 179 {
            saturation /= 2;
        }
        if lightness > 192 {
            saturation /= 2;
        }
        if lightness > 217 {
            saturation /= 2;
        }
        if lightness > 243 {
            saturation /= 2;
        }
        ((hue / 4) << 10) + ((saturation / 32) << 7) + (lightness / 2)
    }

    /// `getUCol(hsl, lightness)` from client-ts (ClientBuild.ts 1397-1408):
    /// replace the low 7 lightness bits of an HSL colour, with the -1
    /// "no colour" sentinel and the 2..126 clamp.
    fn get_ucol(hsl: i32, mut lightness: i32) -> i32 {
        if hsl == -1 {
            return 12345678;
        }

        lightness = (lightness * (hsl & 0x7f)) / 128;
        lightness = lightness.clamp(2, 126);

        (hsl & 0xff80) + lightness
    }

    /// `getOCol(hsl, scalar)` from client-ts (ClientBuild.ts 1412-1431):
    /// the overlay-colour variant of `getUCol`, with the -2 magenta
    /// sentinel and the -1 "scalar as grey" case.
    fn get_ocol(hsl: i32, mut scalar: i32) -> i32 {
        if hsl == -2 {
            return 12345678;
        }

        if hsl == -1 {
            scalar = scalar.clamp(0, 127);
            return 127 - scalar;
        }

        scalar = (scalar * (hsl & 0x7f)) / 128;
        scalar = scalar.clamp(2, 126);

        (hsl & 0xff80) + scalar
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_model_does_not_run_builder_when_skipping() {
        let mut b = ClientBuild::new();
        b.skip_loc_models = true;
        let mut ran = false;
        let out = b.take_model(|| {
            ran = true;
            None
        });
        assert!(out.is_none());
        assert!(!ran);
    }
}
