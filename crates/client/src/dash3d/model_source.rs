// Port of `~/experiments/Server/webclient/src/dash3d/ModelSource.ts`. The TS
// subclasses (`Model`, `ClientObj`, `ClientLocAnim`, `ClientPlayer`,
// `ClientNpc`) are not inheritance here; the scene graph stores them in the
// `SceneModel` enum, which dispatches `getTempModel`. `worldRender` is part
// of the deferred render pass.
use crate::config::Cache;
use crate::dash3d::{ClientLocAnim, ClientNpc, ClientObj, ClientPlayer, Model};
use crate::datastruct::linkable::{LinkableTrait, Links};

/// Any model that can be placed in the `World` scene graph. The `Model`
/// variant carries the full geometry, so variants differ widely in size (the
/// TS heap-allocates subclasses).
#[allow(clippy::large_enum_variant)]
pub enum SceneModel {
    Model(Model),
    Obj(ClientObj),
    LocAnim(ClientLocAnim),
    Player(ClientPlayer),
    Npc(ClientNpc),
}

impl SceneModel {
    /// `ModelSource.getTempModel()` from client-ts, dispatched over the
    /// concrete model source held by the scene slot.
    pub fn get_temp_model(&mut self, cache: &Cache, loop_cycle: i32) -> Option<Model> {
        match self {
            SceneModel::Model(model) => Some(model.clone()),
            SceneModel::Obj(obj) => obj.get_temp_model(cache, loop_cycle),
            SceneModel::LocAnim(anim) => anim.get_temp_model(cache, loop_cycle),
            SceneModel::Player(player) => player.get_temp_model(cache, loop_cycle),
            SceneModel::Npc(npc) => npc.get_temp_model(cache, loop_cycle),
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
        ModelSource { links: Links::new(0), min_y: 1000 }
    }
}
