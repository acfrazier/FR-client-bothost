//! Canonical shared store for decoded model/sprite geometry (Task 5).
//!
//! The per-process LRUs (`config::obj_type`'s `model_cache`/`sprite_cache`,
//! `config::loc_type`'s `mc1`/`mc2`, ...) cache *transformed* products keyed
//! by obj/typecode. This store sits one level down, at the raw-decode
//! boundary: model id → `Arc<Model>`, sprite id → `Arc<Pix32>`, bounded by an
//! LRU (dropping the store's `Arc` frees the geometry once no renderer holds
//! a clone — the refcount bound). `Model::load` serves from it, so two
//! renderers that resolve the same model id decode once and share the same
//! `Arc`; each request clones the immutable geometry out before applying its
//! own transforms, so per-client draw state is unaffected (the store shares
//! only the already-decoded geometry). `load_counts` records how many times a
//! key was actually decoded from the packed source, for the sharing tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::dash3d::model::Model;
use crate::datastruct::linkable::{LinkableTrait, Links};
use crate::datastruct::LruCache;
use crate::graphics::Pix32;

/// The model side holds up to `MODEL_CAPACITY` decoded models — the largest
/// per-process model LRU today (`LocType.mc1`) is 500.
const MODEL_CAPACITY: usize = 500;
/// The sprite side keeps the same 100 entries the `ObjType.spriteCache`
/// held.
const SPRITE_CAPACITY: usize = 100;

/// LRU node for a shared model: the `Arc` plus the link state the cache
/// bookkeeping needs (the payload inside the `Arc` is immutable, so the
/// links cannot ride inside it the way `Model`'s own `Links` does).
struct SharedModel {
    model: Arc<Model>,
    links: Links,
}

impl SharedModel {
    fn new(model: Arc<Model>) -> Self {
        SharedModel { model, links: Links::new(0) }
    }
}

impl LinkableTrait for SharedModel {
    fn links(&self) -> &Links {
        &self.links
    }
    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }
    fn sentinel() -> Self {
        SharedModel { model: Arc::new(Model::default()), links: Links::new(0) }
    }
}

/// LRU node for a shared sprite (same shape as `SharedModel`).
struct SharedSprite {
    sprite: Arc<Pix32>,
    links: Links,
}

impl SharedSprite {
    fn new(sprite: Arc<Pix32>) -> Self {
        SharedSprite { sprite, links: Links::new(0) }
    }
}

impl LinkableTrait for SharedSprite {
    fn links(&self) -> &Links {
        &self.links
    }
    fn links_mut(&mut self) -> &mut Links {
        &mut self.links
    }
    fn sentinel() -> Self {
        SharedSprite { sprite: Arc::new(Pix32::new(0, 0)), links: Links::new(0) }
    }
}

/// The canonical store. Access is through `ModelStore::instance()`
/// (process-wide by design: decoded geometry is shared by every renderer).
pub struct ModelStore {
    /// Decoded models by model id.
    models: LruCache<SharedModel>,
    /// Rendered sprites by sprite id (the `ObjType.getSprite` cache
    /// generalised to Arc-shared entries).
    sprites: LruCache<SharedSprite>,
    /// Times each key was actually decoded from the packed source.
    load_counts: HashMap<i64, usize>,
}

impl ModelStore {
    fn new() -> Self {
        ModelStore {
            models: LruCache::new(MODEL_CAPACITY),
            sprites: LruCache::new(SPRITE_CAPACITY),
            load_counts: HashMap::new(),
        }
    }

    /// The process-wide canonical store (the TS `modelCache`/`spriteCache`
    /// statics generalised to Arc-shared decoded geometry).
    pub fn instance() -> &'static Mutex<ModelStore> {
        static STORE: OnceLock<Mutex<ModelStore>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(ModelStore::new()))
    }

    /// The decoded model for `id` as the store's shared `Arc`. The model is
    /// decoded from the packed source on the first request only; every
    /// later request (from any renderer) gets the same `Arc` without
    /// re-decoding.
    pub fn model(&mut self, id: i32) -> Option<Arc<Model>> {
        if let Some(shared) = self.find_model(id) {
            return Some(shared);
        }
        let model = Model::load_raw(id)?;
        Some(self.publish_model(id, model))
    }

    /// LRU lookup for a decoded model (moves the node to the MRU end).
    pub(crate) fn find_model(&mut self, id: i32) -> Option<Arc<Model>> {
        self.models.find(id as i64).map(|node| node.model.clone())
    }

    /// Publish a decoded model under its id and count the decode (the
    /// "loaded once" evidence the sharing tests assert on).
    pub(crate) fn publish_model(&mut self, id: i32, model: Model) -> Arc<Model> {
        let shared = Arc::new(model);
        let key = id as i64;
        *self.load_counts.entry(key).or_insert(0) += 1;
        self.models.put(SharedModel::new(shared.clone()), key);
        shared
    }

    /// Drop the decoded model for `id` (`Model::unload` makes the next
    /// `load` re-request from the provider, so a stale decode must not be
    /// served from here).
    pub(crate) fn remove_model(&mut self, id: i32) {
        self.models.remove(id as i64);
    }

    /// How many times `id` was actually decoded (1 = every renderer shared
    /// one decode).
    pub fn model_load_count(&self, id: i32) -> usize {
        self.load_counts.get(&(id as i64)).copied().unwrap_or(0)
    }

    /// The rendered sprite for `id` from the shared cache, or `None` on a
    /// miss.
    pub fn sprite(&mut self, id: i32) -> Option<Arc<Pix32>> {
        self.sprites.find(id as i64).map(|node| node.sprite.clone())
    }

    /// Cache a rendered sprite under its id.
    pub fn put_sprite(&mut self, sprite: Arc<Pix32>, id: i32) {
        self.sprites.put(SharedSprite::new(sprite), id as i64);
    }

    /// Detach the stale sprite node for `id` from the hash-table chain (Java
    /// `Linkable.unlink()`; `getSprite` re-puts after an ohi mismatch).
    pub fn unlink_sprite(&mut self, id: i32) {
        self.sprites.unlink_key(id as i64);
    }

    /// Forget every cached sprite (the `ObjType.spriteCache.clear()` a
    /// brightness change triggers).
    pub fn clear_sprites(&mut self) {
        self.sprites.clear();
    }

    /// Forget every cached model/sprite and the decode counts (tests).
    pub fn clear(&mut self) {
        self.models.clear();
        self.sprites.clear();
        self.load_counts.clear();
    }
}

#[cfg(test)]
pub mod tests {
    use std::sync::Mutex;

    /// The process-wide `ModelStore` and the config model LRUs are shared
    /// by every test in the binary, so tests that clear them or depend on
    /// their contents must not interleave (a concurrent clear would evict a
    /// hit mid-test). A failed test poisons the lock; recover so one
    /// failure does not cascade into the rest.
    pub static CACHE_LOCK: Mutex<()> = Mutex::new(());
}
