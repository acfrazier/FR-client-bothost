use client::core::World;
use client::dash3d::LocLayer;

fn small_world() -> World {
    World::new(vec![vec![vec![0; 105]; 105]; 4], 104, 4, 104)
}

/// The previous API oracle mixed the three tile loc-layer stamps, using the
/// missing-square sentinel before folding all 104×104 positions.
fn legacy_loc_hash(world: &World, level: i32) -> u64 {
    let mut hash = 0u64;
    for x in 0..104 {
        for z in 0..104 {
            let stamp = if world.square(level, x, z).is_some() {
                world
                    .loc_model_stamp(level, x, z, LocLayer::WALL)
                    .wrapping_add(world.loc_model_stamp(level, x, z, LocLayer::WALL_DECOR))
                    .wrapping_add(world.loc_model_stamp(level, x, z, LocLayer::GROUND_DECOR))
            } else {
                i32::MIN
            };
            hash = hash.wrapping_mul(31).wrapping_add(stamp as u64);
        }
    }
    hash
}

#[derive(Clone, Copy)]
struct ObservationKey {
    old_hash: u64,
    static_generation: u64,
    level: i32,
    world_generation: u64,
}

fn old_key(world: &World, level: i32) -> ObservationKey {
    ObservationKey {
        old_hash: legacy_loc_hash(world, level),
        static_generation: world.static_loc_generation(),
        level,
        world_generation: world.loc_model_generation(),
    }
}

fn assert_transition(world: &World, old: ObservationKey, level: i32, label: &str) {
    let old_dirty = old.old_hash != legacy_loc_hash(world, level)
        || old.static_generation != world.static_loc_generation()
        || old.level != level;
    let new_dirty = old.world_generation != world.loc_model_generation()
        || old.static_generation != world.static_loc_generation()
        || old.level != level;
    assert_eq!(
        new_dirty, old_dirty,
        "{label}: old hash and generation key disagree"
    );
}

#[test]
fn loc_generation_matches_legacy_oracle_across_stamp_and_scene_transitions() {
    let mut world = small_world();
    let mut old = old_key(&world, 0);

    world.set_wall(0, 2, 3, 0, 0, 0, 0x4000_4000, 0, 0, 0, 0, 0);
    assert_transition(&world, old, 0, "wall creation");
    old = old_key(&world, 0);
    world.set_wall(0, 2, 3, 0, 0, 0, 0x4000_8000, 0, 0, 0, 0, 0);
    assert_transition(&world, old, 0, "wall replacement");
    old = old_key(&world, 0);
    world.del_wall(0, 2, 3);
    assert_transition(&world, old, 0, "wall clear");

    old = old_key(&world, 0);
    world.set_decor(0, 3, 4, 0, 0, 0, 0x4000_4000, 0, 0, 0, 0, 0, 0, 0);
    assert_transition(&world, old, 0, "decor creation");
    old = old_key(&world, 0);
    world.del_decor(0, 3, 4);
    assert_transition(&world, old, 0, "decor removal");

    old = old_key(&world, 0);
    world.set_ground_decor(0, 4, 5, 0, 0x4000_4000, 0, 0, 0, 0, 0);
    assert_transition(&world, old, 0, "ground-decor creation");
    old = old_key(&world, 0);
    world.del_ground_decor(0, 4, 5);
    assert_transition(&world, old, 0, "ground-decor clear");

    old = old_key(&world, 0);
    world.set_wall(0, 2, 3, 0, 0, 0, 0x4000_4000, 0, 0, 0, 0, 0);
    assert_transition(&world, old, 0, "new square creation");
    old = old_key(&world, 0);
    world.bump_loc_stamp(0, 2, 3, LocLayer::WALL);
    assert_transition(&world, old, 0, "animation stamp");

    old = old_key(&world, 0);
    world.set_obj(2, 3, 0, 0, 1, None, None, None);
    assert_eq!(
        world.loc_model_generation(),
        old.world_generation,
        "object replacement on existing square is not a loc invalidation"
    );
    assert_eq!(
        legacy_loc_hash(&world, 0),
        old.old_hash,
        "object replacement does not change the old loc oracle"
    );
    world.del_obj(0, 2, 3);
    assert_eq!(
        world.loc_model_generation(),
        old.world_generation,
        "object clear does not invalidate locs"
    );

    old = old_key(&world, 0);
    assert!(world.add_scenery(0, 5, 5, 0, 0x4000_4000, 0, 1, 1, 0, 0, 0, 0, 0));
    assert_transition(&world, old, 0, "static scenery addition");
    old = old_key(&world, 0);
    world.del_loc(0, 5, 5);
    assert_transition(&world, old, 0, "static scenery removal");

    old = old_key(&world, 0);
    world.set_wall(1, 2, 3, 0, 0, 0, 0x4000_4000, 0, 0, 0, 0, 0);
    assert_transition(&world, old, 1, "active level transition");
    old = old_key(&world, 1);
    world.reset_map();
    assert_transition(&world, old, 1, "map reset");

    let replaced = small_world();
    assert_ne!(
        world.loc_model_generation(),
        replaced.loc_model_generation()
    );
    assert_eq!(legacy_loc_hash(&world, 0), legacy_loc_hash(&replaced, 0));
}

#[test]
fn stamp_writes_are_owned_by_world_mutators() {
    fn inspect(directory: &std::path::Path, root: &std::path::Path) {
        for entry in std::fs::read_dir(directory).expect("read client source directory") {
            let entry = entry.expect("read client source entry");
            let path = entry.path();
            if path.is_dir() {
                inspect(&path, root);
                continue;
            }
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
                continue;
            }
            let relative = path.strip_prefix(root).expect("source is under client/src");
            if relative == std::path::Path::new("core/world.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read Rust source");
            for (line_number, line) in source.lines().enumerate() {
                let target = line.split_once('=').map_or("", |(left, _)| left);
                let stamp_field = [
                    ".wall_model_stamp",
                    ".decor_model_stamp",
                    ".gd_model_stamp",
                    ".obj_model_stamp",
                ]
                .iter()
                .any(|field| target.contains(field));
                let assignment = [" = ", "+=", "-=", "*=", "/=", "%="]
                    .iter()
                    .any(|operator| line.contains(operator));
                let writes_stamp = stamp_field && assignment;
                let renderer_cache_write = relative == std::path::Path::new("render/world.rs")
                    && line.trim_start().starts_with("slot.");
                assert!(
                    !writes_stamp || renderer_cache_write,
                    "stamp write outside core/world.rs: {}:{}: {}",
                    relative.display(),
                    line_number + 1,
                    line.trim()
                );
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    inspect(&root, &root);
}

#[test]
fn reports_world_and_square_layout() {
    println!(
        "size_of::<World>()={} size_of::<Square>()={}",
        std::mem::size_of::<World>(),
        std::mem::size_of::<client::dash3d::Square>()
    );
}
