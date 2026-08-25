//! Canonical shared store for decoded model/sprite geometry (Task 5).
//!
//! The store type itself lives in `dash3d/store.rs`, next to the decode it
//! shares (`Model::load` must serve from it, and `dash3d` cannot depend on
//! `render`). This module is the render-facing home the task brief asks for
//! and carries the cross-renderer sharing tests.
pub use crate::dash3d::store::ModelStore;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Cache, LocType};
    use crate::core::world::World;
    use crate::dash3d::store::tests::CACHE_LOCK;
    use crate::dash3d::{LocShape, Model};
    use crate::render::RenderWorld;
    use std::sync::{Arc, MutexGuard};

    /// The process-wide `ModelStore` and the config model LRUs are shared
    /// by every test in the binary; the store tests serialize on the same
    /// lock as the `obj_type` cache tests.
    fn lock_caches() -> MutexGuard<'static, ()> {
        CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn store() -> MutexGuard<'static, ModelStore> {
        ModelStore::instance().lock().unwrap_or_else(|e| e.into_inner())
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
        assert_eq!(store().model_load_count(0), 1, "the first request decodes the model once");

        // renderer 2 asks for the same id: same Arc, no second decode.
        let b = store().model(0).expect("model 0 shared");
        assert!(Arc::ptr_eq(&a, &b), "both renderers share the same decoded Arc");
        assert_eq!(store().model_load_count(0), 1, "the store resolved the id once");

        // the render-side decode path (`RenderWorld::resolve_*` ->
        // `LocType::get_model` -> `Model::load`) serves from the store too:
        // a fresh `load` reuses the decode instead of re-running it.
        let via_load = Model::load(0).expect("render-path load");
        assert_eq!(store().model_load_count(0), 1, "a renderer load reuses the store");
        assert_eq!(via_load.num_points, a.num_points, "the renderer sees the decoded geometry");

        let shared_a = Model::load_shared(0).expect("shared lookup a");
        let shared_b = Model::load_shared(0).expect("shared lookup b");
        assert!(Arc::ptr_eq(&shared_a, &shared_b), "load_shared hands out the same Arc");
        assert_eq!(store().model_load_count(0), 1, "still one decode for the id");
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
        rw1.resolve_tile(&world, &cache, 0, 0, 0, 0);
        rw1.resolve_tile(&world, &cache, 0, 0, 1, 0);
        assert_eq!(store().model_load_count(0), 1, "both walls decode model 0 once");

        let mut rw2 = RenderWorld::new();
        rw2.resolve_tile(&world, &cache, 0, 0, 0, 0);
        rw2.resolve_tile(&world, &cache, 0, 0, 1, 0);
        assert_eq!(
            store().model_load_count(0),
            1,
            "the second renderer's resolve shares the decoded geometry"
        );

        // both renderers resolved a wall model for the tile.
        assert!(rw1.wall_model1(&world, &cache, 0, 0, 0, 0).is_some());
        assert!(rw2.wall_model1(&world, &cache, 0, 0, 0, 0).is_some());
    }
}
