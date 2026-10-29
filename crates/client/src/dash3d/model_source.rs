// Port of `~/experiments/Server/webclient/src/dash3d/ModelSource.ts`. The TS
// subclasses (`Model`, `ClientObj`, `ClientLocAnim`, `ClientPlayer`,
// `ClientNpc`) are not inheritance here; the scene graph stores them in the
// `SceneModel` enum, which dispatches `getTempModel`. `worldRender` is part
// of the deferred render pass.
use std::sync::Arc;

use crate::config::Cache;
use crate::dash3d::{
    ClientEntity, ClientLocAnim, ClientNpc, ClientObj, ClientPlayer, ClientProj, MapSpotAnim, Model,
};
use crate::datastruct::linkable::{LinkableTrait, Links};
use crate::graphics::{Pix2D, Pix3DDraw};
use crate::render::budget::{
    reserve_vec, BudgetCharge, ByteBudget, RenderBudget, RenderLimit, RenderResource,
};

/// Load a variable model set without allocating its holder outside the
/// caller's render budget. The returned charge must outlive the vector.
pub(crate) fn try_load_models_for_render(
    ids: &[i32],
    budget: &ByteBudget,
) -> Result<(Vec<Option<Model>>, BudgetCharge), RenderLimit> {
    let mut models = Vec::new();
    let mut charge = None;
    reserve_vec(&mut models, ids.len(), &mut charge, Some(budget))?;
    for &id in ids {
        models.push(Model::try_load_for_render(id, budget)?);
    }
    Ok((
        models,
        charge.expect("budgeted model vector has a reservation"),
    ))
}

fn temp_model_budget() -> Result<ByteBudget, RenderLimit> {
    RenderBudget::current_resource(RenderResource::TempModels)
        .ok_or_else(|| RenderLimit::new(RenderResource::TempModels, usize::MAX, 0))
}

pub(crate) fn shared_model_budget(fallback: &ByteBudget) -> ByteBudget {
    RenderBudget::current_resource(RenderResource::SharedModels).unwrap_or_else(|| fallback.clone())
}

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

fn entity_heap_bytes(entity: &ClientEntity) -> usize {
    let route_x_bytes = entity
        .route_x
        .capacity()
        .saturating_mul(std::mem::size_of::<i32>());
    let route_z_bytes = entity
        .route_z
        .capacity()
        .saturating_mul(std::mem::size_of::<i32>());
    let route_run_bytes = entity
        .route_run
        .capacity()
        .saturating_mul(std::mem::size_of::<bool>());
    let chat_bytes = entity
        .chat_message
        .as_ref()
        .map_or(0, |message| message.capacity());
    route_x_bytes
        .saturating_add(route_z_bytes)
        .saturating_add(route_run_bytes)
        .saturating_add(chat_bytes)
}

impl SceneModel {
    /// Retained inline storage and uniquely owned nested heap for this scene
    /// source. Shared model Arc payloads are measured by their process owner.
    pub fn retained_bytes(&self) -> usize {
        let nested_bytes = match self {
            SceneModel::Model(model) => model
                .retained_bytes()
                .saturating_sub(std::mem::size_of::<Model>()),
            SceneModel::Shared(_)
            | SceneModel::Obj(_)
            | SceneModel::LocAnim(_)
            | SceneModel::SpotAnim(_) => 0,
            SceneModel::Player(player) => entity_heap_bytes(&player.entity)
                .saturating_add(player.name.as_ref().map_or(0, |name| name.capacity()))
                .saturating_add(player.loc_model.as_deref().map_or(0, Model::retained_bytes)),
            SceneModel::Npc(npc) => entity_heap_bytes(&npc.entity),
            SceneModel::Proj(proj) => proj.seq_delays.as_ref().map_or(0, |delays| {
                delays.capacity().saturating_mul(std::mem::size_of::<i32>())
            }),
        };
        std::mem::size_of::<Self>().saturating_add(nested_bytes)
    }

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

    /// Obtain mutable geometry for a render-time lighting pass. In an
    /// explicit render scope, copy shared models into the head-owned budget
    /// before changing their variant; standalone rendering keeps Arc's
    /// original copy-on-write behavior.
    pub fn try_as_model_mut_for_render(&mut self) -> Result<Option<&mut Model>, RenderLimit> {
        if let SceneModel::Shared(shared) = &*self {
            if Arc::strong_count(shared) != 1 || Arc::weak_count(shared) != 0 {
                if let Some(budget) = RenderBudget::current_resource(RenderResource::HeadModels) {
                    let owned = shared.try_clone_for_render(&budget)?;
                    *self = SceneModel::Model(owned);
                }
            }
        }
        Ok(match self {
            SceneModel::Model(model) => Some(model),
            SceneModel::Shared(shared) => Some(Arc::make_mut(shared)),
            _ => None,
        })
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

    pub fn try_get_temp_model_for_render(
        &mut self,
        cache: &Cache,
        loop_cycle: i32,
    ) -> Result<Option<Model>, RenderLimit> {
        if !RenderBudget::is_scoped() {
            return Ok(self.get_temp_model(cache, loop_cycle));
        }
        let budget = temp_model_budget()?;
        match self {
            SceneModel::Model(model) => model.try_clone_for_render(&budget).map(Some),
            SceneModel::Shared(model) => model.try_clone_for_render(&budget).map(Some),
            SceneModel::Obj(obj) => obj.try_get_temp_model_for_render(cache, &budget),
            SceneModel::LocAnim(anim) => {
                anim.try_get_temp_model_for_render(cache, loop_cycle, &budget)
            }
            SceneModel::Player(player) => {
                player.try_get_temp_model_for_render(cache, loop_cycle, &budget)
            }
            SceneModel::Npc(npc) => npc.try_get_temp_model_for_render(cache, loop_cycle, &budget),
            SceneModel::Proj(proj) => proj.try_get_temp_model_for_render(cache, &budget),
            SceneModel::SpotAnim(anim) => anim.try_get_temp_model_for_render(cache, &budget),
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
    pub fn try_world_render(
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
    ) -> Result<(), RenderLimit> {
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
            )?;
            return Ok(());
        }
        if let Some(model) = self.try_get_temp_model_for_render(cache, loop_cycle)? {
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
            )?;
        } else {
            #[cfg(feature = "render-diagnostics")]
            crate::render::diagnostics::paint(typecode, "unresolved", 0, 0);
        }
        Ok(())
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
