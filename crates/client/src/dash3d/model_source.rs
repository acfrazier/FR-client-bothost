// Port of `~/experiments/Server/webclient/src/dash3d/ModelSource.ts`. The TS
// subclasses (`Model`, `ClientObj`, `ClientLocAnim`, `ClientPlayer`,
// `ClientNpc`) are not inheritance here; the scene graph stores them in the
// `SceneModel` enum, which dispatches `getTempModel`. `worldRender` is part
// of the deferred render pass.
use std::sync::Arc;

use crate::config::Cache;
use crate::dash3d::{
    ClientLocAnim, ClientNpc, ClientObj, ClientPlayer, ClientProj, MapSpotAnim, Model,
};
use crate::datastruct::linkable::{LinkableTrait, Links};
use crate::graphics::{Pix2D, Pix3DDraw};

/// Any model that can be placed in the `World` scene graph. The `Model`
/// variant carries the full geometry, so variants differ widely in size (the
/// TS heap-allocates subclasses). `Shared` is the headed loc cache: one
/// `Arc` from the loc LRU / GeometryStore, not a per-tile owned clone.
#[allow(clippy::large_enum_variant)]
pub enum SceneModel {
    Model(Model),
    Shared(Arc<Model>),
    Obj(ClientObj),
    LocAnim(ClientLocAnim),
    Player(ClientPlayer),
    Npc(ClientNpc),
    Proj(ClientProj),
    SpotAnim(MapSpotAnim),
}

impl SceneModel {
    pub fn as_model(&self) -> Option<&Model> {
        match self {
            SceneModel::Model(model) => Some(model),
            SceneModel::Shared(model) => Some(model.as_ref()),
            _ => None,
        }
    }

    pub fn as_model_mut(&mut self) -> Option<&mut Model> {
        match self {
            SceneModel::Model(model) => Some(model),
            SceneModel::Shared(model) => Some(Arc::make_mut(model)),
            _ => None,
        }
    }

    /// `ModelSource.getTempModel()` from client-ts, dispatched over the
    /// concrete model source held by the scene slot.
    pub fn get_temp_model(&mut self, cache: &Cache, loop_cycle: i32) -> Option<Model> {
        match self {
            SceneModel::Model(model) => Some(model.clone()),
            SceneModel::Shared(model) => Some((**model).clone()),
            SceneModel::Obj(obj) => obj.get_temp_model(cache, loop_cycle),
            SceneModel::LocAnim(anim) => anim.get_temp_model(cache, loop_cycle),
            SceneModel::Player(player) => player.get_temp_model(cache, loop_cycle),
            SceneModel::Npc(npc) => npc.get_temp_model(cache, loop_cycle),
            SceneModel::Proj(proj) => proj.get_temp_model(cache),
            SceneModel::SpotAnim(anim) => anim.get_temp_model(cache),
        }
    }

    /// `ModelSource.minY`: the last temp model's `min_y` (1000 until the
    /// first render). `World::fill` reads it for the occlusion tests.
    pub fn min_y(&self) -> i32 {
        match self {
            SceneModel::Model(model) => model.min_y,
            SceneModel::Shared(model) => model.min_y,
            SceneModel::Obj(obj) => obj.min_y,
            SceneModel::LocAnim(anim) => anim.min_y,
            SceneModel::Player(player) => player.min_y,
            SceneModel::Npc(npc) => npc.min_y,
            SceneModel::Proj(proj) => proj.min_y,
            SceneModel::SpotAnim(anim) => anim.min_y,
        }
    }

    /// `ModelSource.worldRender(...)` from client-ts: fetch the temp model,
    /// record its `minY`, then render. The `Model` variant renders its
    /// geometry directly (TS `Model` overrides `worldRender`).
    #[allow(clippy::too_many_arguments)]
    pub fn world_render(
        &mut self,
        cache: &Cache,
        loop_cycle: i32,
        pix: &mut Pix3DDraw,
        surface: &mut Pix2D,
        yaw: i32,
        sin_eye_pitch: i32,
        cos_eye_pitch: i32,
        sin_eye_yaw: i32,
        cos_eye_yaw: i32,
        relative_x: i32,
        relative_y: i32,
        relative_z: i32,
        typecode: i32,
    ) {
        if let Some(model) = self.as_model() {
            model.world_render(
                pix,
                surface,
                yaw,
                sin_eye_pitch,
                cos_eye_pitch,
                sin_eye_yaw,
                cos_eye_yaw,
                relative_x,
                relative_y,
                relative_z,
                typecode,
            );
            return;
        }
        if let Some(model) = self.get_temp_model(cache, loop_cycle) {
            let min_y = model.min_y;
            match self {
                SceneModel::Obj(obj) => obj.min_y = min_y,
                SceneModel::LocAnim(anim) => anim.min_y = min_y,
                SceneModel::Player(player) => player.min_y = min_y,
                SceneModel::Npc(npc) => npc.min_y = min_y,
                SceneModel::Proj(proj) => proj.min_y = min_y,
                SceneModel::SpotAnim(anim) => anim.min_y = min_y,
                SceneModel::Model(_) | SceneModel::Shared(_) => unreachable!(),
            }
            model.world_render(
                pix,
                surface,
                yaw,
                sin_eye_pitch,
                cos_eye_pitch,
                sin_eye_yaw,
                cos_eye_yaw,
                relative_x,
                relative_y,
                relative_z,
                typecode,
            );
        }
    }
}

/// The TS base class fields shared by every model source. Concrete model
/// sources carry their own copies (the TS shares them through inheritance).
pub struct ModelSource {
    pub links: Links,
    pub min_y: i32,
}

impl LinkableTrait for ModelSource {
    fn links(&self) -> &Links {
        &self.links
    }
    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }
    fn sentinel() -> Self {
        Self::default()
    }
}

impl Default for ModelSource {
    fn default() -> Self {
        ModelSource {
            links: Links::new(0),
            min_y: 1000,
        }
    }
}
