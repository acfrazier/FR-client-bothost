// Port of `~/experiments/Server/webclient/src/dash3d/ClientNpc.ts`.
use crate::config::Cache;
use crate::dash3d::client_entity::ClientEntity;
use crate::dash3d::{AnimFrame, Model};

#[derive(Clone, Default)]
pub struct ClientNpc {
    pub entity: ClientEntity,
    /// Index of the `NpcType` in the client `Cache` (the TS holds the type
    /// object directly).
    pub r#type: Option<usize>,
}

impl std::ops::Deref for ClientNpc {
    type Target = ClientEntity;
    fn deref(&self) -> &ClientEntity {
        &self.entity
    }
}

impl std::ops::DerefMut for ClientNpc {
    fn deref_mut(&mut self) -> &mut ClientEntity {
        &mut self.entity
    }
}

impl ClientNpc {
    /// Test/headless constructor: an NPC standing on a tile.
    pub fn at(x: i32, z: i32) -> Self {
        let mut npc = ClientNpc::default();
        npc.entity.route_x[0] = x;
        npc.entity.route_z[0] = z;
        npc
    }

    /// `getTempModel()` from client-ts.
    pub fn get_temp_model(&mut self, cache: &Cache, _loop_cycle: i32) -> Option<Model> {
        let npc_id = self.r#type?;

        let mut model = self.get_temp_model2(cache)?;
        self.height = model.min_y;

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
                        temp.animate(frames[self.spotanim_frame as usize]);
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

        if cache.npc(npc_id).size == 1 {
            model.use_aabb_mouse_check = true;
        }

        Some(model)
    }

    /// `getTempModel2()` from client-ts.
    fn get_temp_model2(&mut self, cache: &Cache) -> Option<Model> {
        let npc_id = self.r#type?;

        if self.primary_anim < 0 || self.primary_anim_delay != 0 {
            let mut secondary_transform = -1;
            if self.secondary_anim >= 0 {
                if let Some(frames) = &cache.seq(self.secondary_anim as usize).frames {
                    secondary_transform = frames[self.secondary_anim_frame as usize];
                }
            }

            cache.npc(npc_id).get_temp_model(cache, secondary_transform, -1, None)
        } else {
            let primary_seq = cache.seq(self.primary_anim as usize);
            let mut primary_transform = -1;
            if let Some(frames) = &primary_seq.frames {
                primary_transform = frames[self.primary_anim_frame as usize];
            }

            let mut secondary_transform = -1;
            if self.secondary_anim >= 0 && self.secondary_anim != self.readyanim {
                if let Some(frames) = &cache.seq(self.secondary_anim as usize).frames {
                    secondary_transform = frames[self.secondary_anim_frame as usize];
                }
            }

            cache
                .npc(npc_id)
                .get_temp_model(cache, primary_transform, secondary_transform, primary_seq.walkmerge.as_deref())
        }
    }

    /// `isReady()` from client-ts.
    pub fn is_ready(&self) -> bool {
        self.r#type.is_some()
    }
}
