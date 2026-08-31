// Port of `~/experiments/Server/webclient/src/dash3d/ClientPlayer.ts`. The
// TS statics `recol1d`/`recol2d`/`modelCache` and the `Client.loopCycle`
// static move here: `modelCache` is a process-wide `Mutex<LruCache>` (the TS
// static, kept `Send`), and the model methods take `cache` + `loop_cycle`
// because the config tables and `loopCycle` live on the `Client`.
use std::sync::{Mutex, OnceLock};

use crate::config::Cache;
use crate::dash3d::client_entity::ClientEntity;
use crate::dash3d::{AnimFrame, Model};
use crate::datastruct::LruCache;
use crate::io::Packet;
use crate::util::JString;

// Process-wide by design: an LRU of decoded, immutable player models shared
// by every client (the TS `ClientPlayer.modelCache` static). Cache
// bookkeeping, not per-client draw state; eviction is LRU so clients only
// contend on the lock.
fn model_cache() -> &'static Mutex<LruCache<Model>> {
    static CACHE: OnceLock<Mutex<LruCache<Model>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LruCache::new(200)))
}

// `ClientPlayer.recol1d`/`recol2d` from client-ts. `pub(crate)`: the
// design-preview `clientComponent` arm re-colours the kit model with the
// same tables (TS 10797-10804).
pub(crate) fn recol1d() -> &'static [Vec<i32>; 5] {
    static TABLES: OnceLock<[Vec<i32>; 5]> = OnceLock::new();
    TABLES.get_or_init(|| {
        [
            vec![
                6798, 107, 10283, 16, 4797, 7744, 5799, 4634, 33697, 22433, 2983, 54193,
            ], // hair
            vec![
                8741, 12, 64030, 43162, 7735, 8404, 1701, 38430, 24094, 10153, 56621, 4783, 1341,
                16578, 35003, 25239,
            ], // torso
            vec![
                25238, 8742, 12, 64030, 43162, 7735, 8404, 1701, 38430, 24094, 10153, 56621, 4783,
                1341, 16578, 35003,
            ], // legs
            vec![4626, 11146, 6439, 12, 4758, 10270], // feet
            vec![4550, 4537, 5681, 5673, 5790, 6806, 8076, 4574], // skin
        ]
    })
}

pub(crate) fn recol2d() -> &'static [i32; 16] {
    static TABLE: OnceLock<[i32; 16]> = OnceLock::new();
    TABLE.get_or_init(|| {
        [
            9104, 10275, 7595, 3610, 7975, 8526, 918, 38802, 24466, 10145, 58654, 5027, 1457,
            16565, 34991, 25486,
        ]
    })
}

#[derive(Clone)]
pub struct ClientPlayer {
    pub entity: ClientEntity,
    pub name: Option<String>,
    pub ready: bool,
    pub gender: i32,
    pub headicons: i32,
    pub appearance: [u16; 12],
    pub colour: [u16; 5],
    pub combat_level: i32,
    pub base_id: i64,
    pub low_memory: bool,
    pub model_cache_key: i64,
    pub y: i32,
    pub loc_start_cycle: i32,
    pub loc_stop_cycle: i32,
    pub loc_offset_x: i32,
    pub loc_offset_y: i32,
    pub loc_offset_z: i32,
    pub loc_model: Option<Box<Model>>,
    pub min_tile_x: i32,
    pub min_tile_z: i32,
    pub max_tile_x: i32,
    pub max_tile_z: i32,
    pub transmog: Option<usize>,
    pub skill_level: i32,
    /// TS `ModelSource.minY` (default 1000, updated by `worldRender`).
    pub min_y: i32,
}

impl std::ops::Deref for ClientPlayer {
    type Target = ClientEntity;
    fn deref(&self) -> &ClientEntity {
        &self.entity
    }
}

impl std::ops::DerefMut for ClientPlayer {
    fn deref_mut(&mut self) -> &mut ClientEntity {
        &mut self.entity
    }
}

impl Default for ClientPlayer {
    fn default() -> Self {
        ClientPlayer {
            entity: ClientEntity::default(),
            name: None,
            ready: false,
            gender: 0,
            headicons: 0,
            appearance: [0; 12],
            colour: [0; 5],
            combat_level: 0,
            base_id: 0,
            low_memory: false,
            model_cache_key: -1,
            y: 0,
            loc_start_cycle: 0,
            loc_stop_cycle: 0,
            loc_offset_x: 0,
            loc_offset_y: 0,
            loc_offset_z: 0,
            loc_model: None,
            min_tile_x: 0,
            min_tile_z: 0,
            max_tile_x: 0,
            max_tile_z: 0,
            transmog: None,
            skill_level: 0,
            min_y: 1000,
        }
    }
}

impl ClientPlayer {
    /// Test/headless constructor: a ready-looking local player on a tile.
    pub fn at(x: i32, z: i32) -> Self {
        let mut player = ClientPlayer::default();
        player.entity.route_x[0] = x;
        player.entity.route_z[0] = z;
        player
    }

    /// `setAppearance(buf)` from client-ts; reads the appearance block of a
    /// `PLAYER_INFO` entry.
    pub fn set_appearance(&mut self, buf: &mut Packet, cache: &Cache) {
        buf.pos = 0;

        self.gender = buf.g1();
        self.headicons = buf.g1();
        self.transmog = None;

        for part in 0..12 {
            let msb = buf.g1();
            if msb == 0 {
                self.appearance[part] = 0;
            } else {
                self.appearance[part] = ((msb as u16) << 8) + buf.g1() as u16;
                if part == 0 && self.appearance[0] == 0xffff {
                    let npc_id = buf.g2();
                    if npc_id >= 0 && (npc_id as usize) < cache.npcs.len() {
                        self.transmog = Some(npc_id as usize);
                    }
                    break;
                }
            }
        }

        for part in 0..5 {
            let mut colour = buf.g1();
            if colour < 0 || colour as usize >= recol1d()[part].len() {
                colour = 0;
            }
            self.colour[part] = colour as u16;
        }

        self.readyanim = buf.g2();
        if self.readyanim == 65535 {
            self.readyanim = -1;
        }
        self.turnanim = buf.g2();
        if self.turnanim == 65535 {
            self.turnanim = -1;
        }
        self.walkanim = buf.g2();
        if self.walkanim == 65535 {
            self.walkanim = -1;
        }
        self.walkanim_b = buf.g2();
        if self.walkanim_b == 65535 {
            self.walkanim_b = -1;
        }
        self.walkanim_l = buf.g2();
        if self.walkanim_l == 65535 {
            self.walkanim_l = -1;
        }
        self.walkanim_r = buf.g2();
        if self.walkanim_r == 65535 {
            self.walkanim_r = -1;
        }
        self.runanim = buf.g2();
        if self.runanim == 65535 {
            self.runanim = -1;
        }

        let raw_name = buf.g8();
        self.name = Some(JString::to_screen_name(&JString::to_raw_username(raw_name)));
        self.combat_level = buf.g1();
        self.skill_level = buf.g2();
        self.ready = true;

        self.base_id = 0;
        for part in 0..12 {
            self.base_id <<= 0x4;
            if self.appearance[part] >= 256 {
                self.base_id = self
                    .base_id
                    .wrapping_add(self.appearance[part] as i64 - 256);
            }
        }
        if self.appearance[0] >= 256 {
            self.base_id = self
                .base_id
                .wrapping_add((self.appearance[0] as i64 - 256) >> 4);
        }
        if self.appearance[1] >= 256 {
            self.base_id = self
                .base_id
                .wrapping_add((self.appearance[1] as i64 - 256) >> 8);
        }
        for part in 0..5 {
            self.base_id <<= 0x3;
            self.base_id = self.base_id.wrapping_add(self.colour[part] as i64);
        }
        self.base_id <<= 0x1;
        self.base_id = self.base_id.wrapping_add(self.gender as i64);
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache, loop_cycle: i32) -> Option<Model> {
        if !self.ready {
            return None;
        }

        let mut model = self.get_temp_model2(cache, loop_cycle)?;
        self.height = model.min_y;
        model.use_aabb_mouse_check = true;

        if self.low_memory {
            return Some(model);
        }

        if self.spotanim_id != -1 && self.spotanim_frame != -1 {
            let spot = cache.spot(self.spotanim_id as usize);
            if let Some(spot_model) = spot.get_temp_model2(cache) {
                let mut temp = Model::copy_for_anim(
                    &spot_model,
                    true,
                    AnimFrame::animate_transparencies(self.spotanim_frame),
                    false,
                );

                temp.translate(-self.spotanim_height, 0, 0);
                temp.prepare_anim();

                if let Some(seq) = spot.seq {
                    if let Some(frames) = &cache.seq(seq).frames {
                        let frame = frames[self.spotanim_frame as usize];
                        temp.animate(frame);
                    }
                }

                temp.label_faces = None;
                temp.label_vertices = None;

                if spot.resizeh != 128 || spot.resizev != 128 {
                    temp.resize(spot.resizev, spot.resizeh, spot.resizeh);
                }

                temp.calculate_normals(spot.ambient + 64, spot.contrast + 850, -30, -50, -30, true);

                model = Model::combine(&[model, temp], 2);
            }
        }

        if self.loc_model.is_some() {
            if loop_cycle >= self.loc_stop_cycle {
                self.loc_model = None;
            }

            if loop_cycle >= self.loc_start_cycle && loop_cycle < self.loc_stop_cycle {
                if let Some(loc) = self.loc_model.clone() {
                    let mut loc = *loc;
                    loc.translate(
                        self.loc_offset_y - self.y,
                        self.loc_offset_x - self.x,
                        self.loc_offset_z - self.z,
                    );

                    if self.dst_yaw == 512 {
                        loc.rotate90();
                        loc.rotate90();
                        loc.rotate90();
                    } else if self.dst_yaw == 1024 {
                        loc.rotate90();
                        loc.rotate90();
                    } else if self.dst_yaw == 1536 {
                        loc.rotate90();
                    }

                    model = Model::combine(&[model, loc.clone()], 2);

                    if self.dst_yaw == 512 {
                        loc.rotate90();
                    } else if self.dst_yaw == 1024 {
                        loc.rotate90();
                        loc.rotate90();
                    } else if self.dst_yaw == 1536 {
                        loc.rotate90();
                        loc.rotate90();
                        loc.rotate90();
                    }

                    loc.translate(
                        self.y - self.loc_offset_y,
                        self.x - self.loc_offset_x,
                        self.z - self.loc_offset_z,
                    );
                    self.loc_model = Some(Box::new(loc));
                }
            }
        }

        model.use_aabb_mouse_check = true;
        Some(model)
    }

    /// `getTempModel2()` from client-ts.
    fn get_temp_model2(&mut self, cache: &Cache, _loop_cycle: i32) -> Option<Model> {
        if let Some(npc_id) = self.transmog {
            let mut transform_id = -1;
            if self.primary_anim >= 0 && self.primary_anim_delay == 0 {
                if let Some(frames) = &cache.seq(self.primary_anim as usize).frames {
                    transform_id = frames[self.primary_anim_frame as usize];
                }
            } else if self.secondary_anim >= 0 {
                if let Some(frames) = &cache.seq(self.secondary_anim as usize).frames {
                    transform_id = frames[self.secondary_anim_frame as usize];
                }
            }
            return cache
                .npc(npc_id)
                .get_temp_model(cache, transform_id, -1, None);
        }

        let mut hash = self.base_id;
        let mut primary_transform_id = -1;
        let mut secondary_transform_id = -1;
        let mut left_hand_value = -1;
        let mut right_hand_value = -1;

        if self.primary_anim >= 0 && self.primary_anim_delay == 0 {
            let seq = cache.seq(self.primary_anim as usize);

            if let Some(frames) = &seq.frames {
                primary_transform_id = frames[self.primary_anim_frame as usize];
            }

            if self.secondary_anim >= 0 && self.secondary_anim != self.readyanim {
                if let Some(second_frames) = &cache.seq(self.secondary_anim as usize).frames {
                    secondary_transform_id = second_frames[self.secondary_anim_frame as usize];
                }
            }

            if seq.replaceheldleft >= 0 {
                left_hand_value = seq.replaceheldleft;
                hash =
                    hash.wrapping_add((left_hand_value as i64 - self.appearance[5] as i64) << 40);
            }

            if seq.replaceheldright >= 0 {
                right_hand_value = seq.replaceheldright;
                hash =
                    hash.wrapping_add((right_hand_value as i64 - self.appearance[3] as i64) << 48);
            }
        } else if self.secondary_anim >= 0 {
            if let Some(second_frames) = &cache.seq(self.secondary_anim as usize).frames {
                primary_transform_id = second_frames[self.secondary_anim_frame as usize];
            }
        }

        let mut model = {
            let mut model_cache = model_cache().lock().unwrap();
            model_cache.find(hash).map(|m| m.clone())
        };

        if model.is_none() {
            let mut needs_model = false;

            for slot in 0..12 {
                let mut value = self.appearance[slot] as i32;

                if right_hand_value >= 0 && slot == 3 {
                    value = right_hand_value;
                }
                if left_hand_value >= 0 && slot == 5 {
                    value = left_hand_value;
                }

                if value >= 0x100
                    && value < 0x200
                    && !cache.idk((value - 0x100) as usize).check_model()
                {
                    needs_model = true;
                }

                if value >= 0x200
                    && !cache
                        .obj((value - 0x200) as usize)
                        .check_wear_model(self.gender)
                {
                    needs_model = true;
                }
            }

            if needs_model {
                if self.model_cache_key != -1 {
                    let mut model_cache = model_cache().lock().unwrap();
                    model = model_cache.find(self.model_cache_key).map(|m| m.clone());
                }

                if model.is_none() {
                    return None;
                }
            }
        }

        if model.is_none() {
            let mut models: Vec<Option<Model>> = Vec::new();
            let mut model_count = 0usize;

            for part in 0..12 {
                let mut value = self.appearance[part] as i32;

                if right_hand_value >= 0 && part == 3 {
                    value = right_hand_value;
                }
                if left_hand_value >= 0 && part == 5 {
                    value = left_hand_value;
                }

                if value >= 256 && value < 512 {
                    if let Some(idk_model) = cache.idk((value - 256) as usize).get_model_no_check()
                    {
                        models.push(Some(idk_model));
                        model_count += 1;
                    }
                }

                if value >= 512 {
                    if let Some(obj_model) = cache
                        .obj((value - 512) as usize)
                        .get_wear_model_no_check(self.gender)
                    {
                        models.push(Some(obj_model));
                        model_count += 1;
                    }
                }
            }

            let mut combined = Model::combine_for_anim(&models, model_count);
            for part in 0..5 {
                if self.colour[part] == 0 {
                    continue;
                }
                combined.recolour(
                    recol1d()[part][0],
                    recol1d()[part][self.colour[part] as usize],
                );
                if part == 1 {
                    combined.recolour(recol2d()[0], recol2d()[self.colour[part] as usize]);
                }
            }

            combined.prepare_anim();
            combined.calculate_normals(64, 850, -30, -50, -30, true);
            {
                let mut model_cache = model_cache().lock().unwrap();
                model_cache.put(combined.clone(), hash);
            }
            self.model_cache_key = hash;
            model = Some(combined);
        }

        let model = model?;

        if self.low_memory {
            return Some(model);
        }

        let mut tmp = Model::temp_model();
        tmp.set(
            &model,
            AnimFrame::animate_transparencies(primary_transform_id)
                && AnimFrame::animate_transparencies(secondary_transform_id),
        );

        if primary_transform_id != -1 && secondary_transform_id != -1 {
            let walkmerge = cache.seq(self.primary_anim as usize).walkmerge.clone();
            tmp.mask_animate(
                primary_transform_id,
                secondary_transform_id,
                walkmerge.as_deref(),
            );
        } else if primary_transform_id != -1 {
            tmp.animate(primary_transform_id);
        }

        tmp.calc_bounding_cylinder();
        tmp.label_faces = None;
        tmp.label_vertices = None;
        Some(tmp)
    }

    /// `getHeadModel()` from client-ts.
    pub fn get_head_model(&self, cache: &Cache) -> Option<Model> {
        if !self.ready {
            return None;
        }

        let mut needs_model = false;

        for i in 0..12 {
            let part = self.appearance[i] as i32;

            if part >= 0x100 && part < 0x200 && !cache.idk((part - 0x100) as usize).check_head() {
                needs_model = true;
            }

            if part >= 0x200
                && !cache
                    .obj((part - 0x200) as usize)
                    .check_head_model(self.gender)
            {
                needs_model = true;
            }
        }

        if needs_model {
            return None;
        }

        let mut models: Vec<Option<Model>> = Vec::new();
        let mut model_count = 0usize;
        for part in 0..12 {
            let value = self.appearance[part] as i32;

            if value >= 256 && value < 512 {
                if let Some(idk_model) = cache.idk((value - 256) as usize).get_head_no_check() {
                    models.push(Some(idk_model));
                    model_count += 1;
                }
            }

            if value >= 512 {
                if let Some(head_model) = cache
                    .obj((value - 512) as usize)
                    .get_head_model_no_check(self.gender)
                {
                    models.push(Some(head_model));
                    model_count += 1;
                }
            }
        }

        let mut tmp = Model::combine_for_anim(&models, model_count);
        for part in 0..5 {
            if self.colour[part] == 0 {
                continue;
            }
            tmp.recolour(
                recol1d()[part][0],
                recol1d()[part][self.colour[part] as usize],
            );
            if part == 1 {
                tmp.recolour(recol2d()[0], recol2d()[self.colour[part] as usize]);
            }
        }

        Some(tmp)
    }

    /// `isReady()` from client-ts.
    pub fn is_ready(&self) -> bool {
        self.ready
    }
}
