//! Canonical shared store for decoded model/sprite geometry (Task 5).
//!
//! The store type itself lives in `dash3d/store.rs`, next to the decode it
//! shares (`Model::load` must serve from it, and `dash3d` cannot depend on
//! `render`). This module is the render-facing home the task brief asks for
//! and carries the cross-renderer sharing tests.
pub use crate::dash3d::store::ModelStore;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::dash3d::Model;
use crate::graphics::Pix32;
use crate::render::budget::{BudgetCharge, ByteBudget, RenderLimit};

struct TrackedModel {
    id: i32,
    model: Weak<Model>,
}

struct RenderModelIndex {
    models: VecDeque<TrackedModel>,
    capacity_charge: Option<BudgetCharge>,
    budget: Option<ByteBudget>,
}

static RENDER_MODEL_INDEX: OnceLock<Mutex<RenderModelIndex>> = OnceLock::new();

impl RenderModelIndex {
    fn new() -> Self {
        Self {
            models: VecDeque::new(),
            capacity_charge: None,
            budget: None,
        }
    }
}

fn render_model_index() -> &'static Mutex<RenderModelIndex> {
    RENDER_MODEL_INDEX.get_or_init(|| Mutex::new(RenderModelIndex::new()))
}

fn existing_render_model_index() -> Option<&'static Mutex<RenderModelIndex>> {
    RENDER_MODEL_INDEX.get()
}

/// Reserve a shared model or index allocation, evicting only unheld shared
/// LRU payloads charged to this exact budget.
pub(crate) fn reserve_shared_bytes(
    store: &mut ModelStore,
    budget: &ByteBudget,
    bytes: usize,
) -> Result<BudgetCharge, RenderLimit> {
    let mut index = render_model_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    reserve_shared_bytes_with_index(store, budget, bytes, &mut index)
}

fn reserve_shared_bytes_with_index(
    store: &mut ModelStore,
    budget: &ByteBudget,
    bytes: usize,
    index: &mut RenderModelIndex,
) -> Result<BudgetCharge, RenderLimit> {
    loop {
        match budget.reserve(bytes) {
            Ok(charge) => return Ok(charge),
            Err(limit) => {
                if !evict_one_render_model(store, budget, index) {
                    return Err(limit);
                }
            }
        }
    }
}

/// Ensure the byte-index can record a newly visible model before decoding or
/// publishing it. The index capacity is charged as part of SharedModels.
pub(crate) fn prepare_render_model_slot(
    store: &mut ModelStore,
    budget: &ByteBudget,
    id: i32,
    model: Option<&Arc<Model>>,
) -> Result<(), RenderLimit> {
    let mut index = render_model_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(model) = model {
        if let Some(position) = index
            .models
            .iter()
            .position(|entry| entry.model.as_ptr() == Arc::as_ptr(model))
        {
            let mut entry = index.models.remove(position).expect("position exists");
            entry.id = id;
            index.models.push_back(entry);
            return Ok(());
        }
    }

    if index
        .budget
        .as_ref()
        .is_some_and(|existing| !existing.same_budget(budget))
    {
        if !index.models.is_empty() {
            return Err(RenderLimit::new(
                budget.resource(),
                usize::MAX,
                budget.maximum(),
            ));
        }
        let old_models = std::mem::take(&mut index.models);
        drop(old_models);
        index.capacity_charge = None;
        index.budget = None;
    }

    if index.models.len() == index.models.capacity() {
        let minimum_capacity = index
            .models
            .len()
            .checked_add(1)
            .ok_or_else(|| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        let new_capacity = index
            .models
            .capacity()
            .max(1)
            .saturating_mul(2)
            .max(minimum_capacity);
        let new_capacity_bytes = new_capacity
            .checked_mul(std::mem::size_of::<TrackedModel>())
            .ok_or_else(|| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        let new_charge =
            reserve_shared_bytes_with_index(store, budget, new_capacity_bytes, &mut index)?;
        let mut replacement = VecDeque::new();
        replacement
            .try_reserve_exact(new_capacity)
            .map_err(|_| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        if replacement.capacity() < new_capacity {
            return Err(RenderLimit::new(
                budget.resource(),
                usize::MAX,
                budget.maximum(),
            ));
        }
        let replacement_bytes = replacement
            .capacity()
            .checked_mul(std::mem::size_of::<TrackedModel>())
            .ok_or_else(|| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        let mut new_charge = new_charge;
        new_charge.resize(replacement_bytes)?;
        replacement.append(&mut index.models);
        let old_charge = index.capacity_charge.replace(new_charge);
        index.models = replacement;
        drop(old_charge);
        index.budget = Some(budget.clone());
    }
    Ok(())
}

/// Record/touch a model after its Arc has been published in ModelStore.
pub(crate) fn track_render_model(id: i32, model: &Arc<Model>) {
    let mut index = render_model_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let weak = Arc::downgrade(model);
    if let Some(position) = index
        .models
        .iter()
        .position(|entry| entry.model.ptr_eq(&weak))
    {
        let entry = index.models.remove(position).expect("position exists");
        index.models.push_back(TrackedModel { id, model: weak });
        drop(entry);
    } else {
        debug_assert!(index.models.len() < index.models.capacity());
        index.models.push_back(TrackedModel { id, model: weak });
    }
}

pub(crate) fn reset_render_model_index() {
    let mut index = render_model_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let old_models = std::mem::take(&mut index.models);
    drop(old_models);
    index.capacity_charge = None;
    index.budget = None;
}

fn prune_dead_models(index: &mut RenderModelIndex) {
    index.models.retain(|entry| entry.model.strong_count() != 0);
}

fn evict_one_render_model(
    store: &mut ModelStore,
    budget: &ByteBudget,
    index: &mut RenderModelIndex,
) -> bool {
    prune_dead_models(index);
    let attempts = index.models.len();
    for _ in 0..attempts {
        let Some(entry) = index.models.pop_front() else {
            return false;
        };
        let id = entry.id;
        let Some(model) = store.find_model(id) else {
            index.models.push_back(entry);
            continue;
        };
        if entry.model.as_ptr() != Arc::as_ptr(&model) {
            index.models.push_back(entry);
            continue;
        }
        if model.has_render_charge_for(budget) && Arc::strong_count(&model) == 2 {
            drop(model);
            store.remove_model(id);
            return true;
        }
        index.models.push_back(entry);
    }
    false
}

/// Whether this exact model Arc is owned by the process-wide ModelStore or
/// retained in its live weak index. This query never initializes either.
pub(crate) fn model_is_shared_process_owner(model: &Arc<Model>) -> bool {
    let store = ModelStore::existing_instance().map(|store_mutex| {
        store_mutex
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    });
    let mut in_store = false;
    if let Some(store) = store.as_deref() {
        store.for_each_model_node(|_, cached, sentinel| {
            if !sentinel && Arc::ptr_eq(cached, model) {
                in_store = true;
            }
        });
    }
    if in_store {
        return true;
    }

    let Some(index_mutex) = existing_render_model_index() else {
        return false;
    };
    let index = index_mutex
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let pointer = Arc::as_ptr(model);
    index
        .models
        .iter()
        .any(|entry| entry.model.as_ptr() == pointer)
}

/// Return logical retained bytes for the process-wide shared model/sprite
/// owners without using budget counters or allocating scratch storage.
pub(crate) fn shared_render_bytes() -> (usize, usize) {
    let Some(store_mutex) = ModelStore::existing_instance() else {
        return (0, 0);
    };
    let store = store_mutex
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (model_capacity_bytes, sprite_capacity_bytes) = store.allocation_capacity_bytes();
    let mut model_bytes = store
        .metadata_capacity_bytes()
        .saturating_add(model_capacity_bytes);
    let mut sprite_bytes = sprite_capacity_bytes;

    store.for_each_model_node(|id, model, sentinel| {
        if sentinel || !has_earlier_model_arc(&store, id, model) {
            model_bytes = model_bytes
                .saturating_add(Model::shared_bytes_for_model(model).unwrap_or(usize::MAX));
        }
    });
    store.for_each_sprite_node(|id, sprite, sentinel| {
        if sentinel || !has_earlier_sprite_arc(&store, id, sprite) {
            sprite_bytes = sprite_bytes.saturating_add(shared_sprite_payload_bytes(sprite));
        }
    });

    let model_index = render_model_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    model_bytes = model_bytes
        .saturating_add(std::mem::size_of::<Mutex<RenderModelIndex>>())
        .saturating_add(
            model_index
                .models
                .capacity()
                .saturating_mul(std::mem::size_of::<TrackedModel>()),
        )
        .saturating_add(shared_model_orphan_bytes(&store, &model_index));

    let sprite_index = render_sprite_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    sprite_bytes = sprite_bytes
        .saturating_add(std::mem::size_of::<Mutex<RenderSpriteIndex>>())
        .saturating_add(
            sprite_index
                .sprites
                .capacity()
                .saturating_mul(std::mem::size_of::<TrackedSprite>()),
        )
        .saturating_add(shared_sprite_orphan_bytes(&store, &sprite_index));

    (model_bytes, sprite_bytes)
}

/// Shared raw-model and sprite owner-index limits for the ignored full-content
/// census. Capacity simulation mirrors the production reserve-before-push
/// growth path without initializing or mutating either process index.
#[cfg(feature = "render-test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderTestSharedIndexLayout {
    pub model_index_mutex_bytes: usize,
    pub sprite_index_mutex_bytes: usize,
    pub model_index_entry_bytes: usize,
    pub sprite_index_entry_bytes: usize,
    pub model_index_capacity_bytes: usize,
    pub sprite_index_capacity_bytes: usize,
    pub model_orphan_bytes_per_dead_arc: usize,
    pub sprite_orphan_bytes_per_dead_arc: usize,
}

#[cfg(feature = "render-test-hooks")]
pub fn render_test_shared_index_layout(
    model_entries: usize,
    sprite_entries: usize,
) -> Option<RenderTestSharedIndexLayout> {
    Some(RenderTestSharedIndexLayout {
        model_index_mutex_bytes: std::mem::size_of::<Mutex<RenderModelIndex>>(),
        sprite_index_mutex_bytes: std::mem::size_of::<Mutex<RenderSpriteIndex>>(),
        model_index_entry_bytes: std::mem::size_of::<TrackedModel>(),
        sprite_index_entry_bytes: std::mem::size_of::<TrackedSprite>(),
        model_index_capacity_bytes: simulated_index_capacity_bytes::<TrackedModel>(model_entries)?,
        sprite_index_capacity_bytes: simulated_index_capacity_bytes::<TrackedSprite>(
            sprite_entries,
        )?,
        model_orphan_bytes_per_dead_arc: std::mem::size_of::<Model>()
            .checked_add(2 * std::mem::size_of::<usize>())?,
        sprite_orphan_bytes_per_dead_arc: std::mem::size_of::<Pix32>()
            .checked_add(2 * std::mem::size_of::<usize>())?,
    })
}

#[cfg(feature = "render-test-hooks")]
fn simulated_index_capacity_bytes<T>(entries: usize) -> Option<usize> {
    let mut values = VecDeque::<std::mem::MaybeUninit<T>>::new();
    for _ in 0..entries {
        if values.len() == values.capacity() {
            let minimum_capacity = values.len().checked_add(1)?;
            let new_capacity = values
                .capacity()
                .max(1)
                .checked_mul(2)?
                .max(minimum_capacity);
            let mut replacement = VecDeque::new();
            replacement.try_reserve_exact(new_capacity).ok()?;
            if replacement.capacity() < new_capacity {
                return None;
            }
            replacement.append(&mut values);
            values = replacement;
        }
        values.push_back(std::mem::MaybeUninit::uninit());
    }
    values.capacity().checked_mul(std::mem::size_of::<T>())
}

fn has_earlier_model_arc(store: &ModelStore, id: usize, model: &Arc<Model>) -> bool {
    let mut duplicate = false;
    store.for_each_model_node(|prior_id, prior, sentinel| {
        if !sentinel && prior_id < id && Arc::ptr_eq(prior, model) {
            duplicate = true;
        }
    });
    duplicate
}

fn has_earlier_sprite_arc(store: &ModelStore, id: usize, sprite: &Arc<Pix32>) -> bool {
    let mut duplicate = false;
    store.for_each_sprite_node(|prior_id, prior, sentinel| {
        if !sentinel && prior_id < id && Arc::ptr_eq(prior, sprite) {
            duplicate = true;
        }
    });
    duplicate
}

fn shared_sprite_payload_bytes(sprite: &Pix32) -> usize {
    std::mem::size_of::<Pix32>()
        .saturating_add(
            sprite
                .data
                .capacity()
                .saturating_mul(std::mem::size_of::<i32>()),
        )
        .saturating_add(2usize.saturating_mul(std::mem::size_of::<usize>()))
}

fn shared_model_orphan_bytes(store: &ModelStore, index: &RenderModelIndex) -> usize {
    let mut bytes = 0usize;
    for (position, entry) in index.models.iter().enumerate() {
        let pointer = entry.model.as_ptr();
        if index
            .models
            .iter()
            .take(position)
            .any(|earlier| earlier.model.as_ptr() == pointer)
        {
            continue;
        }
        let Some(model) = entry.model.upgrade() else {
            bytes = bytes.saturating_add(
                std::mem::size_of::<Model>()
                    .saturating_add(2usize.saturating_mul(std::mem::size_of::<usize>())),
            );
            continue;
        };
        let mut retained_by_store = false;
        store.for_each_model_node(|_, cached, sentinel| {
            if !sentinel && Arc::ptr_eq(&model, cached) {
                retained_by_store = true;
            }
        });
        if !retained_by_store {
            bytes =
                bytes.saturating_add(Model::shared_bytes_for_model(&model).unwrap_or(usize::MAX));
        }
    }
    bytes
}

fn shared_sprite_orphan_bytes(store: &ModelStore, index: &RenderSpriteIndex) -> usize {
    let mut bytes = 0usize;
    for (position, entry) in index.sprites.iter().enumerate() {
        let pointer = entry.sprite.as_ptr();
        if index
            .sprites
            .iter()
            .take(position)
            .any(|earlier| earlier.sprite.as_ptr() == pointer)
        {
            continue;
        }
        let Some(sprite) = entry.sprite.upgrade() else {
            bytes = bytes.saturating_add(
                std::mem::size_of::<Pix32>()
                    .saturating_add(2usize.saturating_mul(std::mem::size_of::<usize>())),
            );
            continue;
        };
        let mut retained_by_store = false;
        store.for_each_sprite_node(|_, cached, sentinel| {
            if !sentinel && Arc::ptr_eq(&sprite, cached) {
                retained_by_store = true;
            }
        });
        if !retained_by_store {
            bytes = bytes.saturating_add(shared_sprite_payload_bytes(&sprite));
        }
    }
    bytes
}

struct TrackedSprite {
    id: i32,
    sprite: Weak<Pix32>,
    charge: BudgetCharge,
}

struct RenderSpriteIndex {
    sprites: VecDeque<TrackedSprite>,
    capacity_charge: Option<BudgetCharge>,
    budget: Option<ByteBudget>,
}

impl RenderSpriteIndex {
    fn new() -> Self {
        Self {
            sprites: VecDeque::new(),
            capacity_charge: None,
            budget: None,
        }
    }
}

fn render_sprite_index() -> &'static Mutex<RenderSpriteIndex> {
    static INDEX: OnceLock<Mutex<RenderSpriteIndex>> = OnceLock::new();
    INDEX.get_or_init(|| Mutex::new(RenderSpriteIndex::new()))
}

/// Bytes retained by a shared sprite Arc and its LRU owner node.
pub(crate) fn shared_sprite_bytes(sprite: &Pix32) -> Option<usize> {
    std::mem::size_of::<Pix32>()
        .checked_add(
            sprite
                .data
                .capacity()
                .checked_mul(std::mem::size_of::<i32>())?,
        )?
        .checked_add(std::mem::size_of::<usize>().checked_mul(2)?)?
        .checked_add(std::mem::size_of::<Arc<Pix32>>())?
        .checked_add(std::mem::size_of::<crate::datastruct::linkable::Links>())
}

pub(crate) fn shared_sprite_copy_bytes(sprite: &Pix32) -> Option<usize> {
    std::mem::size_of::<Pix32>()
        .checked_add(sprite.data.len().checked_mul(std::mem::size_of::<i32>())?)?
        .checked_add(std::mem::size_of::<usize>().checked_mul(2)?)?
        .checked_add(std::mem::size_of::<Arc<Pix32>>())?
        .checked_add(std::mem::size_of::<crate::datastruct::linkable::Links>())
}

/// Ensure the shared sprite owner index can record a new cache entry.
pub(crate) fn prepare_render_sprite_slot(
    store: &mut ModelStore,
    budget: &ByteBudget,
) -> Result<(), RenderLimit> {
    let mut index = render_sprite_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    prune_dead_sprites(&mut index);
    reset_empty_sprite_index(&mut index);

    if index
        .budget
        .as_ref()
        .is_some_and(|existing| !existing.same_budget(budget))
    {
        if !index.sprites.is_empty() {
            return Err(RenderLimit::new(
                budget.resource(),
                usize::MAX,
                budget.maximum(),
            ));
        }
        reset_empty_sprite_index(&mut index);
    }

    if index.sprites.len() == index.sprites.capacity() {
        let minimum_capacity = index
            .sprites
            .len()
            .checked_add(1)
            .ok_or_else(|| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        let new_capacity = index
            .sprites
            .capacity()
            .max(1)
            .saturating_mul(2)
            .max(minimum_capacity);
        let new_capacity_bytes = new_capacity
            .checked_mul(std::mem::size_of::<TrackedSprite>())
            .ok_or_else(|| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        let new_charge =
            reserve_shared_sprite_bytes_with_index(store, budget, new_capacity_bytes, &mut index)?;
        let mut replacement = VecDeque::new();
        replacement
            .try_reserve_exact(new_capacity)
            .map_err(|_| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        if replacement.capacity() < new_capacity {
            return Err(RenderLimit::new(
                budget.resource(),
                usize::MAX,
                budget.maximum(),
            ));
        }
        let replacement_bytes = replacement
            .capacity()
            .checked_mul(std::mem::size_of::<TrackedSprite>())
            .ok_or_else(|| RenderLimit::new(budget.resource(), usize::MAX, budget.maximum()))?;
        let mut new_charge = new_charge;
        new_charge.resize(replacement_bytes)?;
        replacement.append(&mut index.sprites);
        let old_charge = index.capacity_charge.replace(new_charge);
        index.sprites = replacement;
        index.budget = Some(budget.clone());
        drop(old_charge);
    }
    Ok(())
}

/// Reserve a shared sprite payload, evicting the shared sprite LRU if needed.
pub(crate) fn reserve_shared_sprite_bytes(
    store: &mut ModelStore,
    budget: &ByteBudget,
    bytes: usize,
) -> Result<BudgetCharge, RenderLimit> {
    let mut index = render_sprite_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    reserve_shared_sprite_bytes_with_index(store, budget, bytes, &mut index)
}

fn reserve_shared_sprite_bytes_with_index(
    store: &mut ModelStore,
    budget: &ByteBudget,
    bytes: usize,
    index: &mut RenderSpriteIndex,
) -> Result<BudgetCharge, RenderLimit> {
    loop {
        match budget.reserve(bytes) {
            Ok(charge) => return Ok(charge),
            Err(limit) => {
                if evict_one_render_sprite(store, budget, index) {
                    continue;
                }
                let before_sprites = index.sprites.len();
                let before_capacity = index
                    .capacity_charge
                    .as_ref()
                    .map(BudgetCharge::bytes)
                    .unwrap_or(0);
                store.clear_sprites();
                prune_dead_sprites(index);
                reset_empty_sprite_index(index);
                let released_sprites = index.sprites.len() < before_sprites;
                let released_capacity = before_capacity > 0 && index.capacity_charge.is_none();
                if !released_sprites && !released_capacity {
                    return Err(limit);
                }
            }
        }
    }
}

fn evict_one_render_sprite(
    store: &mut ModelStore,
    budget: &ByteBudget,
    index: &mut RenderSpriteIndex,
) -> bool {
    prune_dead_sprites(index);
    let attempts = index.sprites.len();
    for _ in 0..attempts {
        let Some(entry) = index.sprites.pop_front() else {
            return false;
        };
        if !entry.charge.belongs_to(budget) {
            index.sprites.push_back(entry);
            continue;
        }
        let Some(sprite) = store.sprite(entry.id) else {
            index.sprites.push_back(entry);
            continue;
        };
        let weak = Arc::downgrade(&sprite);
        if entry.sprite.ptr_eq(&weak) && Arc::strong_count(&sprite) == 2 {
            drop(sprite);
            store.remove_sprite(entry.id);
            drop(entry);
            if index.sprites.is_empty() {
                reset_empty_sprite_index(index);
            }
            return true;
        }
        index.sprites.push_back(entry);
    }
    false
}

pub(crate) fn render_sprite_is_charged(sprite: &Arc<Pix32>, budget: &ByteBudget) -> bool {
    let mut index = render_sprite_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    prune_dead_sprites(&mut index);
    let weak = Arc::downgrade(sprite);
    let Some(position) = index
        .sprites
        .iter()
        .position(|entry| entry.sprite.ptr_eq(&weak))
    else {
        return false;
    };
    let entry = index.sprites.remove(position).expect("position exists");
    let charged = entry.charge.belongs_to(budget);
    index.sprites.push_back(entry);
    charged
}

pub(crate) fn track_render_sprite(id: i32, sprite: &Arc<Pix32>, charge: BudgetCharge) {
    let mut index = render_sprite_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let weak = Arc::downgrade(sprite);
    if let Some(position) = index
        .sprites
        .iter()
        .position(|entry| entry.sprite.ptr_eq(&weak))
    {
        let old = index.sprites.remove(position).expect("position exists");
        index.sprites.push_back(TrackedSprite {
            id,
            sprite: weak,
            charge,
        });
        drop(old);
    } else {
        debug_assert!(index.sprites.len() < index.sprites.capacity());
        index.sprites.push_back(TrackedSprite {
            id,
            sprite: weak,
            charge,
        });
    }
}

pub(crate) fn forget_dead_render_sprites() {
    let mut index = render_sprite_index()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    prune_dead_sprites(&mut index);
    reset_empty_sprite_index(&mut index);
}

fn prune_dead_sprites(index: &mut RenderSpriteIndex) {
    index
        .sprites
        .retain(|entry| entry.sprite.strong_count() != 0);
}

fn reset_empty_sprite_index(index: &mut RenderSpriteIndex) {
    if index.sprites.is_empty() {
        let old_sprites = std::mem::take(&mut index.sprites);
        drop(old_sprites);
        index.capacity_charge = None;
        index.budget = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Cache, LocType};
    use crate::core::world::World;
    use crate::dash3d::store::tests::CACHE_LOCK;
    use crate::dash3d::{LocShape, Model, SceneModel};
    use crate::render::RenderWorld;
    use std::sync::{Arc, MutexGuard};

    /// The process-wide `ModelStore` and the config model LRUs are shared
    /// by every test in the binary; the store tests serialize on the same
    /// lock as the `obj_type` cache tests.
    fn lock_caches() -> MutexGuard<'static, ()> {
        CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn store() -> MutexGuard<'static, ModelStore> {
        ModelStore::instance()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// One 3-vertex, 1-face triangle (the same synthetic model the
    /// `obj_type` sprite tests unpack).
    const MODEL: &[u8] = &[
        7, 7, 7, // vertex order: x+y+z deltas for each of 3 vertices
        1, // face index order: a,b,c are all deltas
        0x40, 0x41, 0x41, // face index deltas: a=0, b=1, c=2 (cumulative)
        0x00, 0xFF, // face colour (HSL 255)
        0x40, 0x68, 0x18, // vertexX deltas: 0, +40, -40
        0x68, 0x40, 0x18, // vertexY deltas: +40, 0, -40
        0x40, 0x40, 0x40, // vertexZ deltas: 0, 0, 0
        0, 3, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 3, 0, 3, 0, 3, // trailer
    ];

    /// Two renderers request the same model id: the store decodes it once
    /// (load-count == 1) and both get the same `Arc`.
    #[test]
    fn model_store_shared_across_renderers() {
        let _guard = lock_caches();
        store().clear();
        Model::unpack(0, Some(MODEL));

        // renderer 1 asks for model id 0: the store decodes it once.
        let a = store().model(0).expect("model 0 decodes");
        assert_eq!(
            store().model_load_count(0),
            1,
            "the first request decodes the model once"
        );

        // renderer 2 asks for the same id: same Arc, no second decode.
        let b = store().model(0).expect("model 0 shared");
        assert!(
            Arc::ptr_eq(&a, &b),
            "both renderers share the same decoded Arc"
        );
        assert_eq!(
            store().model_load_count(0),
            1,
            "the store resolved the id once"
        );

        // the render-side decode path (`RenderWorld::resolve_*` ->
        // `LocType::get_model` -> `Model::load`) serves from the store too:
        // a fresh `load` reuses the decode instead of re-running it.
        let via_load = Model::load(0).expect("render-path load");
        assert_eq!(
            store().model_load_count(0),
            1,
            "a renderer load reuses the store"
        );
        assert_eq!(
            via_load.num_points, a.num_points,
            "the renderer sees the decoded geometry"
        );

        let shared_a = Model::load_shared(0).expect("shared lookup a");
        let shared_b = Model::load_shared(0).expect("shared lookup b");
        assert!(
            Arc::ptr_eq(&shared_a, &shared_b),
            "load_shared hands out the same Arc"
        );
        assert_eq!(
            store().model_load_count(0),
            1,
            "still one decode for the id"
        );
    }

    /// Two `RenderWorld`s resolve tiles whose walls both build from the
    /// same model id; the decoded geometry is shared (one decode total).
    #[test]
    fn render_worlds_share_one_decode_per_model_id() {
        let _guard = lock_caches();
        store().clear();
        Model::unpack(0, Some(MODEL));

        // Two locs building from model 0. The second is mirrored so its
        // `mc1` key differs and the decode really reaches `Model::load`
        // again (with the same key the transformed loc cache alone would
        // serve the second request).
        let cache = Cache {
            locs: vec![
                LocType {
                    id: 0,
                    model: Some(vec![0]),
                    shape: Some(vec![LocShape::WALL_STRAIGHT]),
                    ..LocType::default()
                },
                LocType {
                    id: 1,
                    model: Some(vec![0]),
                    shape: Some(vec![LocShape::WALL_STRAIGHT]),
                    mirror: true,
                    ..LocType::default()
                },
            ],
            ..Cache::default()
        };

        let mut world = World::new(vec![vec![vec![0; 3]; 3]; 1], 2, 1, 2);
        world.fill_base_level(0);
        world.set_wall(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        world.set_wall(0, 1, 0, 0, 0, 0, 1 << 14, 0, 0, 0, 0, 0);

        let mut rw1 = RenderWorld::new();
        rw1.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile resolve stays within its render budget");
        rw1.resolve_tile(&world, &cache, 0, 0, 1, 0)
            .expect("unscoped tile resolve stays within its render budget");
        assert_eq!(
            store().model_load_count(0),
            1,
            "both walls decode model 0 once"
        );

        let mut rw2 = RenderWorld::new();
        rw2.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile resolve stays within its render budget");
        rw2.resolve_tile(&world, &cache, 0, 0, 1, 0)
            .expect("unscoped tile resolve stays within its render budget");
        assert_eq!(
            store().model_load_count(0),
            1,
            "the second renderer's resolve shares the decoded geometry"
        );

        // both renderers resolved a wall model for the tile.
        assert!(rw1
            .wall_model1(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped wall lookup succeeds")
            .is_some());
        assert!(rw2
            .wall_model1(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped wall lookup succeeds")
            .is_some());

        // Task 6: the cached loc is one GeometryStore Arc, not a per-head
        // owned clone sitting in TileModels.
        let a = match rw1
            .wall_model1(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped wall lookup succeeds")
        {
            Some(SceneModel::Shared(m)) => Arc::clone(m),
            _ => panic!("wall 0 must decode to a shared loc Arc"),
        };
        let b = match rw2
            .wall_model1(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped wall lookup succeeds")
        {
            Some(SceneModel::Shared(m)) => Arc::clone(m),
            _ => panic!("wall 1 must decode to a shared loc Arc"),
        };
        assert!(
            Arc::ptr_eq(&a, &b),
            "two RenderWorlds must share the loc Arc, not two owned clones"
        );
    }

    /// Each model-bearing setter bumps its layer's generation, and the
    /// render-side lazy cache re-resolves that layer's geometry when it changes —
    /// a stale decode must never be served after a mutation. Each loc-backed component
    /// (wall, decor, ground decor, scene sprite) is resolved, mutated to
    /// the other synthetic model, and re-resolved. The loc/model ids are
    /// 100+ so the `LocType` `mc1` transformed-model cache cannot serve a
    /// stale entry written by another test (the existing store tests use
    /// ids 0/1).
    #[test]
    fn tile_mutation_reresolves_lazy_models() {
        let _guard = lock_caches();
        store().clear();
        Model::unpack(100, Some(MODEL)); // 3-point triangle
        Model::unpack(101, Some(MODEL_B)); // 4-point quad

        let mut locs: Vec<LocType> = std::iter::repeat_with(LocType::default).take(108).collect();
        locs[100] = LocType {
            id: 100,
            model: Some(vec![100]),
            shape: Some(vec![LocShape::WALL_STRAIGHT]),
            ..LocType::default()
        };
        locs[101] = LocType {
            id: 101,
            model: Some(vec![101]),
            shape: Some(vec![LocShape::WALL_STRAIGHT]),
            ..LocType::default()
        };
        // decor (shape 4, WALLDECOR_STRAIGHT_NOOFFSET).
        locs[102] = LocType {
            id: 102,
            model: Some(vec![100]),
            shape: Some(vec![LocShape::WALLDECOR_STRAIGHT_NOOFFSET]),
            ..LocType::default()
        };
        locs[103] = LocType {
            id: 103,
            model: Some(vec![101]),
            shape: Some(vec![LocShape::WALLDECOR_STRAIGHT_NOOFFSET]),
            ..LocType::default()
        };
        // ground decor (shape 22, GROUND_DECOR).
        locs[104] = LocType {
            id: 104,
            model: Some(vec![100]),
            shape: Some(vec![LocShape::GROUND_DECOR]),
            ..LocType::default()
        };
        locs[105] = LocType {
            id: 105,
            model: Some(vec![101]),
            shape: Some(vec![LocShape::GROUND_DECOR]),
            ..LocType::default()
        };
        // scene sprites (shape 10, CENTREPIECE_STRAIGHT).
        locs[106] = LocType {
            id: 106,
            model: Some(vec![100]),
            shape: Some(vec![LocShape::CENTREPIECE_STRAIGHT]),
            ..LocType::default()
        };
        locs[107] = LocType {
            id: 107,
            model: Some(vec![101]),
            shape: Some(vec![LocShape::CENTREPIECE_STRAIGHT]),
            ..LocType::default()
        };
        let cache = Cache {
            locs,
            ..Cache::default()
        };

        let mut world = World::new(vec![vec![vec![0; 3]; 3]; 1], 2, 1, 2);
        world.fill_base_level(0);
        let mut rw = RenderWorld::new();

        // Wall: resolve model 100, mutate to the model-101 loc, re-resolve.
        world.set_wall(0, 0, 0, 0, 0, 0, 100 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile resolve stays within its render budget");
        assert_eq!(
            wall_points(&mut rw, &world, &cache),
            3,
            "the first set_wall resolves the model-100 wall"
        );
        world.set_wall(0, 0, 0, 0, 0, 0, 101 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile re-resolve stays within its render budget");
        assert_eq!(
            wall_points(&mut rw, &world, &cache),
            4,
            "set_wall must bump the tile stamp so the lazy wall re-resolves"
        );

        // Decor.
        world.set_decor(0, 0, 0, 0, 0, 0, 102 << 14, 0, 0, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile re-resolve stays within its render budget");
        assert_eq!(
            decor_points(&mut rw, &world, &cache),
            3,
            "set_decor resolves model 100"
        );
        world.set_decor(0, 0, 0, 0, 0, 0, 103 << 14, 0, 0, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile re-resolve stays within its render budget");
        assert_eq!(
            decor_points(&mut rw, &world, &cache),
            4,
            "set_decor must invalidate the lazy decor"
        );

        // Ground decor.
        world.set_ground_decor(0, 0, 0, 0, 104 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile re-resolve stays within its render budget");
        assert_eq!(
            gd_points(&mut rw, &world, &cache),
            3,
            "set_ground_decor resolves model 100"
        );
        world.set_ground_decor(0, 0, 0, 0, 105 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile re-resolve stays within its render budget");
        assert_eq!(
            gd_points(&mut rw, &world, &cache),
            4,
            "set_ground_decor must invalidate the lazy ground decor"
        );

        // Scene sprite: the typecode is `0x40000000 | loc_id << 14` with a
        // CENTREPIECE_STRAIGHT typecode2 (the `addLoc` encoding).
        let scene_typecode = |loc_id: i32| 0x4000_0000i32 | (loc_id << 14);
        assert!(world.add_scenery(0, 0, 0, 0, scene_typecode(106), 10, 1, 1, 0, 0, 0, 0, 0));
        let sprite = world
            .last_sprite_index()
            .expect("a scenery sprite was pushed");
        assert_eq!(
            sprite_points(&mut rw, &world, &cache, sprite),
            3,
            "the scenery resolves model 100"
        );
        assert!(world.add_scenery(0, 0, 0, 0, scene_typecode(107), 10, 1, 1, 0, 0, 0, 0, 0));
        let sprite_b = world.last_sprite_index().expect("a second scenery sprite");
        assert_eq!(
            sprite_points(&mut rw, &world, &cache, sprite_b),
            4,
            "a scenery typecode change must resolve the new geometry"
        );
    }

    /// A dynamic sprite (player/NPC) stepping onto a tile must not invalidate
    /// the tile's already-resolved wall: sprites resolve from the sprite's
    /// own `model_stamp` (and dynamic sprites attach via `set_sprite_model`),
    /// not the tile stamp. Bumping the tile stamp on sprite placement made a
    /// moving player/NPC re-decode the tile's lit wall as an unlit model
    /// (`face_colour_a` absent), which emitted no faces — the black-wall bug.
    #[test]
    fn dynamic_sprite_does_not_reresolve_wall() {
        let _guard = lock_caches();
        store().clear();
        Model::unpack(100, Some(MODEL));

        let cache = Cache {
            locs: {
                let mut locs: Vec<LocType> =
                    std::iter::repeat_with(LocType::default).take(101).collect();
                locs[100] = LocType {
                    id: 100,
                    model: Some(vec![100]),
                    shape: Some(vec![LocShape::WALL_STRAIGHT]),
                    ..LocType::default()
                };
                locs
            },
            ..Cache::default()
        };
        let mut world = World::new(vec![vec![vec![0; 3]; 3]; 1], 2, 1, 2);
        world.fill_base_level(0);
        world.set_wall(0, 0, 0, 0, 0, 0, 100 << 14, 0, 0, 0, 0, 0);
        let mut rw = RenderWorld::new();
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped tile resolve stays within its render budget");

        // Mark the resolved wall with a sentinel shade so a re-decode (which
        // rebuilds an unlit model) is observable through the wall accessor.
        let mut marked = Model::load(100).expect("model 100 decodes");
        marked.face_colour_a = Some(vec![12345]);
        rw.set_wall_model(&world, 0, 0, 0, Some(SceneModel::Model(marked)), None)
            .expect("unscoped wall mark stays within its render budget");

        // A dynamic sprite steps onto the tile (the player/NPC movement path).
        let index = world.add_dynamic(0, 64, 0, 64, 0, 0, 0, false);
        assert!(index.is_some(), "a dynamic sprite places on the tile");

        match rw
            .wall_model1(&world, &cache, 0, 0, 0, 0)
            .expect("unscoped wall lookup succeeds")
        {
            Some(SceneModel::Model(m)) => assert_eq!(
                m.face_colour_a.as_deref(),
                Some(&[12345][..]),
                "a dynamic sprite must not re-resolve the tile wall"
            ),
            other => panic!(
                "the tile wall was re-resolved/replaced: {}",
                other.is_some()
            ),
        }
    }

    /// The resolved wall model's vertex count (0 when none).
    fn wall_points(rw: &mut RenderWorld, world: &World, cache: &Cache) -> i32 {
        model_points(
            rw.wall_model1(world, cache, 0, 0, 0, 0)
                .expect("unscoped wall lookup succeeds"),
        )
    }

    fn decor_points(rw: &mut RenderWorld, world: &World, cache: &Cache) -> i32 {
        model_points(
            rw.decor_model(world, cache, 0, 0, 0, 0)
                .expect("unscoped decor lookup succeeds"),
        )
    }

    fn gd_points(rw: &mut RenderWorld, world: &World, cache: &Cache) -> i32 {
        model_points(
            rw.gd_model(world, cache, 0, 0, 0, 0)
                .expect("unscoped ground-decor lookup succeeds"),
        )
    }

    fn sprite_points(rw: &mut RenderWorld, world: &World, cache: &Cache, index: usize) -> i32 {
        model_points(
            rw.sprite_model(world, cache, 0, index)
                .expect("unscoped sprite lookup succeeds"),
        )
    }

    fn model_points(model: Option<&SceneModel>) -> i32 {
        model
            .and_then(|m| m.as_model())
            .map(|m| m.num_points)
            .unwrap_or(0)
    }

    /// A second synthetic model: a 4-point quad (two triangles), so the
    /// tripwire can tell two geometries apart by vertex count.
    const MODEL_B: &[u8] = &[
        7, 7, 7, 7, // vertex order: x+y+z deltas for each of 4 vertices
        1, 1, // face index order: fresh a/b/c per face
        // face index deltas (cumulative with the previous face's last):
        // face 0 = (0, 1, 2), face 1 = (0, 2, 3).
        0x40, 0x41, 0x41, 0x3e, 0x42, 0x41, 0x00, 0xFF, 0x00, 0xFF, // face colours (HSL 255)
        0x40, 0x68, 0x18, 0x40, // vertexX deltas: 0, +40, -40, 0
        0x68, 0x40, 0x18, 0x40, // vertexY deltas: +40, 0, -40, 0
        0x40, 0x40, 0x40, 0x40, // vertexZ deltas: 0, 0, 0, 0
        0, 4, 0, 2, 0, 0, 0, 0, 0, 0, 0, 4, 0, 4, 0, 4, 0, 6, // trailer
    ];

    #[test]
    fn shared_sprite_budget_releases_lru_payload_before_admitting_replacement() {
        let _guard = lock_caches();
        let mut shared = store();
        shared.clear_sprites();
        forget_dead_render_sprites();

        let sprite = Arc::new(Pix32::new(32, 32));
        let sprite_bytes = shared_sprite_bytes(&sprite).expect("sprite byte size");
        let index_bytes = 2 * std::mem::size_of::<TrackedSprite>();
        let budget = ByteBudget::new(
            crate::render::budget::RenderResource::SharedSprites,
            sprite_bytes + index_bytes,
        );
        prepare_render_sprite_slot(&mut shared, &budget).expect("index reservation");
        let charge =
            reserve_shared_sprite_bytes(&mut shared, &budget, sprite_bytes).expect("sprite charge");
        shared.put_sprite(Arc::clone(&sprite), 41);
        track_render_sprite(41, &sprite, charge);
        drop(sprite);
        assert_eq!(budget.live(), sprite_bytes + index_bytes);

        let replacement = reserve_shared_sprite_bytes(&mut shared, &budget, sprite_bytes)
            .expect("eligible LRU was released");
        assert!(shared.sprite(41).is_none());
        assert_eq!(budget.live(), sprite_bytes);
        drop(replacement);
        assert_eq!(budget.live(), 0);

        shared.clear_sprites();
        forget_dead_render_sprites();
    }

    #[test]
    fn shared_render_census_counts_evicted_models_held_by_heads() {
        let _guard = lock_caches();
        store().clear();
        reset_render_model_index();
        forget_dead_render_sprites();

        let mut model = Model::default();
        model.point_x = Some(Vec::with_capacity(8));
        let retained_vector_bytes =
            model.point_x.as_ref().unwrap().capacity() * std::mem::size_of::<i32>();
        let budget = ByteBudget::new(
            crate::render::budget::RenderResource::SharedModels,
            1024 * 1024,
        );
        let unrelated_model = Arc::new(Model::default());
        assert!(!model_is_shared_process_owner(&unrelated_model));
        drop(unrelated_model);
        let shared_model = {
            let mut shared = store();
            prepare_render_model_slot(&mut shared, &budget, 42, None).expect("weak registry slot");
            let model = shared.publish_model(42, model);
            track_render_model(42, &model);
            model
        };

        assert!(model_is_shared_process_owner(&shared_model));
        let in_store = shared_render_bytes().0;
        store().remove_model(42);
        assert!(model_is_shared_process_owner(&shared_model));
        let evicted_but_held = shared_render_bytes().0;
        assert!(evicted_but_held >= in_store);

        drop(shared_model);
        let weak_only = shared_render_bytes().0;
        assert_eq!(evicted_but_held - weak_only, retained_vector_bytes);

        reset_render_model_index();
        store().clear();
        forget_dead_render_sprites();
    }
}
