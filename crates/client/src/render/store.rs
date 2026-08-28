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

    /// The `model_stamp` tripwire (mid-branch review): every model-bearing
    /// setter on the sim tile bumps the tile stamp, and the render-side
    /// lazy cache re-resolves the geometry when it changes — a stale decode
    /// must never be served after a mutation. Each loc-backed component
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
        let cache = Cache { locs, ..Cache::default() };

        let mut world = World::new(vec![vec![vec![0; 3]; 3]; 1], 2, 1, 2);
        world.fill_base_level(0);
        let mut rw = RenderWorld::new();

        // Wall: resolve model 100, mutate to the model-101 loc, re-resolve.
        world.set_wall(0, 0, 0, 0, 0, 0, 100 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0);
        assert_eq!(
            wall_points(&mut rw, &world, &cache),
            3,
            "the first set_wall resolves the model-100 wall"
        );
        world.set_wall(0, 0, 0, 0, 0, 0, 101 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0);
        assert_eq!(
            wall_points(&mut rw, &world, &cache),
            4,
            "set_wall must bump the tile stamp so the lazy wall re-resolves"
        );

        // Decor.
        world.set_decor(0, 0, 0, 0, 0, 0, 102 << 14, 0, 0, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0);
        assert_eq!(decor_points(&mut rw, &world, &cache), 3, "set_decor resolves model 100");
        world.set_decor(0, 0, 0, 0, 0, 0, 103 << 14, 0, 0, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0);
        assert_eq!(
            decor_points(&mut rw, &world, &cache),
            4,
            "set_decor must invalidate the lazy decor"
        );

        // Ground decor.
        world.set_ground_decor(0, 0, 0, 0, 104 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0);
        assert_eq!(gd_points(&mut rw, &world, &cache), 3, "set_ground_decor resolves model 100");
        world.set_ground_decor(0, 0, 0, 0, 105 << 14, 0, 0, 0, 0, 0);
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0);
        assert_eq!(
            gd_points(&mut rw, &world, &cache),
            4,
            "set_ground_decor must invalidate the lazy ground decor"
        );

        // Scene sprite: the typecode is `0x40000000 | loc_id << 14` with a
        // CENTREPIECE_STRAIGHT typecode2 (the `addLoc` encoding).
        let scene_typecode = |loc_id: i32| 0x4000_0000i32 | (loc_id << 14);
        assert!(world.add_scenery(0, 0, 0, 0, scene_typecode(106), 10, 1, 1, 0, 0, 0, 0, 0));
        let sprite = world.last_sprite_index().expect("a scenery sprite was pushed");
        assert_eq!(sprite_points(&mut rw, &world, &cache, sprite), 3, "the scenery resolves model 100");
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
        rw.resolve_tile(&world, &cache, 0, 0, 0, 0);

        // Mark the resolved wall with a sentinel shade so a re-decode (which
        // rebuilds an unlit model) is observable through the wall accessor.
        let mut marked = Model::load(100).expect("model 100 decodes");
        marked.face_colour_a = Some(vec![12345]);
        rw.set_wall_model(
            &world,
            0,
            0,
            0,
            Some(SceneModel::Model(marked)),
            None,
        );

        // A dynamic sprite steps onto the tile (the player/NPC movement path).
        let index = world.add_dynamic(0, 64, 0, 64, 0, 0, 0, false);
        assert!(index.is_some(), "a dynamic sprite places on the tile");

        match rw.wall_model1(&world, &cache, 0, 0, 0, 0) {
            Some(SceneModel::Model(m)) => assert_eq!(
                m.face_colour_a.as_ref().map(Vec::as_slice),
                Some(&[12345][..]),
                "a dynamic sprite must not re-resolve the tile wall"
            ),
            other => panic!("the tile wall was re-resolved/replaced: {}", other.is_some()),
        }
    }

    /// The resolved wall model's vertex count (0 when none).
    fn wall_points(rw: &mut RenderWorld, world: &World, cache: &Cache) -> i32 {
        model_points(rw.wall_model1(world, cache, 0, 0, 0, 0))
    }

    fn decor_points(rw: &mut RenderWorld, world: &World, cache: &Cache) -> i32 {
        model_points(rw.decor_model(world, cache, 0, 0, 0, 0))
    }

    fn gd_points(rw: &mut RenderWorld, world: &World, cache: &Cache) -> i32 {
        model_points(rw.gd_model(world, cache, 0, 0, 0, 0))
    }

    fn sprite_points(rw: &mut RenderWorld, world: &World, cache: &Cache, index: usize) -> i32 {
        model_points(rw.sprite_model(world, cache, 0, index))
    }

    fn model_points(model: Option<&SceneModel>) -> i32 {
        match model {
            Some(SceneModel::Model(m)) => m.num_points,
            _ => 0,
        }
    }

    /// A second synthetic model: a 4-point quad (two triangles), so the
    /// tripwire can tell two geometries apart by vertex count.
    const MODEL_B: &[u8] = &[
        7, 7, 7, 7, // vertex order: x+y+z deltas for each of 4 vertices
        1, 1, // face index order: fresh a/b/c per face
        // face index deltas (cumulative with the previous face's last):
        // face 0 = (0, 1, 2), face 1 = (0, 2, 3).
        0x40, 0x41, 0x41, 0x3e, 0x42, 0x41,
        0x00, 0xFF, 0x00, 0xFF, // face colours (HSL 255)
        0x40, 0x68, 0x18, 0x40, // vertexX deltas: 0, +40, -40, 0
        0x68, 0x40, 0x18, 0x40, // vertexY deltas: +40, 0, -40, 0
        0x40, 0x40, 0x40, 0x40, // vertexZ deltas: 0, 0, 0, 0
        0, 4, 0, 2, 0, 0, 0, 0, 0, 0, 0, 4, 0, 4, 0, 4, 0, 6, // trailer
    ];
}
