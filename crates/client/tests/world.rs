// `World::render_all` / `update_mouse_picking` (Task 4). A synthetic 3×3
// flat world at ground height 2000 renders into a 512×334 viewport: the
// `groundh - eyeY >= 2000` gate in `renderAll` marks every tile drawable
// even with an unpopulated `visBacking`, so the whole scene raster path
// (fill → renderQuickGround → gouraud) runs without a pack. A mouse click
// on the projected ground must come back as `ground_x`/`ground_z`.
use client::client::{Client, ClientConfig};
use client::config::{Cache, LocType};
use client::core::World;
use client::dash3d::ground::Ground;
use client::dash3d::LocAngle;
use client::dash3d::{LocShape, Model, SceneModel, TerrainOverlayShape};
use client::graphics::{Pix2D, Pix3D, Pix3DDraw, PixMap};
use client::io::JagFile;
use client::render::RenderWorld;

/// Shade whose colour-table entry is non-zero (same constant as the model
/// tests: index y=200/x=100).
const SHADE: i32 = 200 * 128 + 100;

fn one_face_model() -> Model {
    let mut model = Model {
        num_points: 3,
        point_x: Some(vec![-50, 50, 50]),
        point_y: Some(vec![-50, -50, 50]),
        point_z: Some(vec![0, 0, 0]),
        num_faces: 1,
        face_vertex_a: Some(vec![0]),
        face_vertex_b: Some(vec![2]),
        face_vertex_c: Some(vec![1]),
        face_colour_a: Some(vec![SHADE]),
        face_colour_b: Some(vec![SHADE]),
        face_colour_c: Some(vec![SHADE]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();
    model
}

/// 3×3 flat world at height 2000 with a plain-coloured tile on every cell.
fn flat_world() -> World {
    let max_level: i32 = 1;
    let max_tile_x: i32 = 3;
    let max_tile_z: i32 = 3;
    let groundh = vec![
        vec![vec![2000i32; max_tile_z as usize + 1]; max_tile_x as usize + 1];
        max_level as usize
    ];
    let mut world = World::new(groundh, max_tile_z, max_level, max_tile_x);
    world.fill_base_level(0);
    for x in 0..max_tile_x {
        for z in 0..max_tile_z {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    world
}

/// Task 3b: the sim world holds typecodes/placement only; the tests inject
/// the synthetic models into the render side's lazy cache.
#[allow(clippy::too_many_arguments)]
fn place_wall(
    rw: &mut RenderWorld,
    world: &mut World,
    level: i32,
    x: i32,
    z: i32,
    y: i32,
    angle1: i32,
    angle2: i32,
    model1: Option<SceneModel>,
    model2: Option<SceneModel>,
    typecode: i32,
    typecode2: i32,
) {
    world.set_wall(
        level, x, z, y, angle1, angle2, typecode, typecode2, 0, 0, 0, 0,
    );
    rw.set_wall_model(world, level, x, z, model1, model2);
}

#[allow(clippy::too_many_arguments)]
fn place_scenery(
    rw: &mut RenderWorld,
    world: &mut World,
    level: i32,
    x: i32,
    z: i32,
    y: i32,
    model: SceneModel,
    typecode: i32,
    typecode2: i32,
    width: i32,
    length: i32,
    yaw: i32,
) {
    world.add_scenery(
        level, x, z, y, typecode, typecode2, width, length, yaw, 0, 0, 0, 0,
    );
    let index = world.last_sprite_index().expect("sprite pushed");
    rw.set_sprite_model(world, index, Some(model));
}

/// A one-face model with computed point normals, as a placed loc would be
/// after `calculate_normals(..., doNotShareLight: false)` (LocType
/// `sharelight` path): `shared_point_normal` is retained for the
/// `World.shareLight` merge, `face_colour_a` starts zeroed.
fn normals_model() -> Model {
    let mut model = Model {
        num_points: 3,
        point_x: Some(vec![-50, 50, 50]),
        point_y: Some(vec![-50, -50, 50]),
        point_z: Some(vec![0, 0, 0]),
        num_faces: 1,
        face_vertex_a: Some(vec![0]),
        face_vertex_b: Some(vec![2]),
        face_vertex_c: Some(vec![1]),
        face_colour: Some(vec![SHADE]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();
    model.calculate_normals(64, 768, -50, -10, -50, false);
    model
}

// --- World.shareLight (World.ts 589-796) ---

#[test]
fn share_light_empty_world_is_noop() {
    // No base level filled: every tile is None, so the whole scan must be a
    // no-op without panicking (the brief's empty-world requirement).
    let max_level: i32 = 1;
    let max_tile_x: i32 = 3;
    let max_tile_z: i32 = 3;
    let groundh = vec![
        vec![vec![2000i32; max_tile_z as usize + 1]; max_tile_x as usize + 1];
        max_level as usize
    ];
    let mut world = World::new(groundh, max_tile_z, max_level, max_tile_x);
    RenderWorld::new().share_light(&mut world, 64, 768, -50, -10, -50);
}

#[test]
fn share_light_lights_wall_models_and_consumes_normals() {
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    place_wall(
        &mut rw,
        &mut world,
        0,
        1,
        1,
        0,
        0,
        0,
        Some(SceneModel::Model(normals_model())),
        None,
        0,
        0,
    );

    rw.share_light(&mut world, 64, 768, -50, -10, -50);

    let SceneModel::Model(model) = rw
        .wall_model1(&world, &Cache::default(), 0, 0, 1, 1)
        .expect("wall model1")
    else {
        panic!("wall model1 must be a Model")
    };
    assert!(
        model.point_normal.is_none(),
        "light() must consume point normals"
    );
    assert!(
        model.shared_point_normal.is_none(),
        "light() must consume shared normals"
    );
    let lit = model.face_colour_a.as_ref().expect("lit colour")[0];
    assert_ne!(
        lit, 0,
        "shareLight must light wall vertices (face_colour_a)"
    );
}

#[test]
fn share_light_lights_scenery_sprites() {
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    // typecode bit 30 set so `get_scene` finds the sprite after the pass.
    place_scenery(
        &mut rw,
        &mut world,
        0,
        1,
        1,
        2000,
        SceneModel::Model(normals_model()),
        0x40000000,
        0,
        1,
        1,
        0,
    );

    rw.share_light(&mut world, 64, 768, -50, -10, -50);

    let index = world.scene_sprite_index(0, 1, 1).expect("sprite index");
    let SceneModel::Model(model) = rw
        .sprite_model(&world, &Cache::default(), 0, index)
        .expect("sprite model")
    else {
        panic!("sprite model must be a Model")
    };
    assert!(model.point_normal.is_none());
    assert_ne!(model.face_colour_a.as_ref().unwrap()[0], 0);
}

#[test]
fn share_light_lights_ground_decor() {
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    world.set_ground_decor(0, 1, 1, 2000, 0, 0, 0, 0, 0, 0);
    rw.set_gd_model(&world, 0, 1, 1, Some(SceneModel::Model(normals_model())));

    rw.share_light(&mut world, 64, 768, -50, -10, -50);

    let SceneModel::Model(model) = rw
        .gd_model(&world, &Cache::default(), 0, 0, 1, 1)
        .expect("ground decor")
    else {
        panic!("gd model must be a Model")
    };
    assert!(model.point_normal.is_none());
    assert_ne!(model.face_colour_a.as_ref().unwrap()[0], 0);
}

/// Task 3b fix round 1: the *production* share-light path — `render_all`
/// consuming the sim's pending flag — must resolve and light sharelight
/// scene sprites even though they decode lazily at draw time. The
/// injected-model tests call `rw.share_light` directly with the sprite
/// already present, so they cannot catch a pass that skips un-resolved
/// sprites (the sprite's model comes from the config `Cache` via
/// `Model::load`, exactly like a live build).
#[test]
fn render_all_lights_pending_sharelight_scene_sprites() {
    Pix3D::init_colour_table(0.6);
    let Some(bytes) = loc_ob2("basic_wall_1.ob2") else {
        eprintln!("basic_wall_1.ob2 missing; skip");
        return;
    };
    // Loc ids are 15-bit in the typecode (`& 0x7fff`) and a scene sprite
    // needs bits 29-30 == 2, so the id must stay under 32768.
    const ID: i32 = 9000;
    Model::unpack(ID, Some(&bytes));
    let mut cache = Cache::default();
    while cache.locs.len() <= ID as usize {
        cache.locs.push(LocType::default());
    }
    cache.locs[ID as usize] = LocType {
        id: ID,
        model: Some(vec![ID]),
        shape: Some(vec![LocShape::CENTREPIECE_STRAIGHT]),
        sharelight: true,
        ..LocType::default()
    };

    let mut world = flat_world();
    // A sharelight centriepiece scene sprite (what `addLoc` would place for
    // a loc packet; placed directly here).
    world.add_scenery(
        0,
        1,
        1,
        2000,
        0x4000_0000 + (ID << 14),
        LocShape::CENTREPIECE_STRAIGHT,
        1,
        1,
        0,
        2000,
        2000,
        2000,
        2000,
    );
    // `finishBuild` flags the pass (TS 331); `render_all` consumes it.
    world.share_light_pending = true;

    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &cache,
            0,
            192,
            1950,
            192,
            3,
            0,
            128,
        );
    }

    // The pending pass resolved the scene sprite and lit it: normals
    // consumed, vertices carry lit colours.
    let index = world.scene_sprite_index(0, 1, 1).expect("sprite placed");
    let model = rw
        .sprite_model(&world, &cache, 0, index)
        .expect("sprite model")
        .as_model()
        .expect("sprite model must be a static Model");
    assert!(
        model.point_normal.is_none(),
        "render_all's pending share_light must consume the scene sprite's point normals"
    );
    let lit = model.face_colour_a.as_ref().expect("lit colour")[0];
    assert_ne!(
        lit, 0,
        "render_all's pending share_light must light the scene sprite's vertices"
    );
}

/// One model from the 274 `main_file_cache` (archive 1 = the model files
/// the engine serves for client archive-0 model requests), gzip'd exactly
/// like the OnDemand download path. `None` when the local 274 pack is
/// absent (the `Server` checkout the ob2 fixtures also read).
fn cache_model(cache_dir: &str, id: i32) -> Option<Vec<u8>> {
    use flate2::read::GzDecoder;
    use std::io::{Read, Seek, SeekFrom};
    let mut idx = std::fs::File::open(format!("{cache_dir}/main_file_cache.idx1")).ok()?;
    let mut dat = std::fs::File::open(format!("{cache_dir}/main_file_cache.dat")).ok()?;
    idx.seek(SeekFrom::Start(id as u64 * 6)).ok()?;
    let mut rec = [0u8; 6];
    idx.read_exact(&mut rec).ok()?;
    let size = ((rec[0] as i32) << 16) + ((rec[1] as i32) << 8) + rec[2] as i32;
    let mut sector = ((rec[3] as i32) << 16) + ((rec[4] as i32) << 8) + rec[5] as i32;
    if size <= 0 || size > 2_000_000 {
        return None;
    }
    let mut out = Vec::with_capacity(size as usize);
    let mut part = 0i32;
    while (out.len() as i32) < size {
        if sector == 0 {
            return None;
        }
        dat.seek(SeekFrom::Start(sector as u64 * 520)).ok()?;
        let mut block = [0u8; 520];
        dat.read_exact(&mut block).ok()?;
        let file_id = ((block[0] as i32) << 8) + block[1] as i32;
        let part_id = ((block[2] as i32) << 8) + block[3] as i32;
        let next = ((block[4] as i32) << 16) + ((block[5] as i32) << 8) + block[6] as i32;
        let archive_id = block[7] as i32;
        if file_id != id || part_id != part || archive_id != 2 {
            return None;
        }
        let take = ((size as usize) - out.len()).min(512);
        out.extend_from_slice(&block[8..8 + take]);
        sector = next;
        part += 1;
    }
    let mut plain = Vec::new();
    GzDecoder::new(out.as_slice())
        .read_to_end(&mut plain)
        .ok()?;
    Some(plain)
}

/// The real 274 config cache and loc 1812 ("Portal"): one of the only two
/// `sharelight` + `anim` locs in the data (the other is 1779 "Sails"), so
/// its placements decode to `SceneModel::LocAnim` and the share-light pass
/// skips them. Its base frame model is unpacked from the file store.
fn portal_fixture() -> Option<Cache> {
    let cache_dir = client::engine_dir().join("data/pack").display().to_string();
    let config = std::fs::read(format!("{cache_dir}/client/config")).ok()?;
    let cache = Cache::unpack(&JagFile::new(config));
    let base = *cache.locs[1812].model.as_ref()?.first()?;
    let bytes = cache_model(&cache_dir, base)?;
    Model::unpack(base, Some(&bytes));
    Some(cache)
}

/// Task 1 (GPU-chrome campaign): the black wall. Loc 1812 is `sharelight`
/// and animated (`anim` 491), so every placement decodes to
/// `SceneModel::LocAnim`; the share-light pass only lights
/// `SceneModel::Model`. The frame model must therefore be lit when the
/// animation materialises it — before the fix it kept the zeroed pre-light
/// `face_colour_a` and rendered black.
#[test]
fn render_all_lights_animated_sharelight_loc_frames() {
    let Some(cache) = portal_fixture() else {
        eprintln!("274 config/file store missing; skip");
        return;
    };
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    // Loc 1812 is shape-less (centriepiece only), so `addLoc` places it as
    // a scene sprite (typecode bits 29-30 = 2).
    world.add_scenery(
        0,
        1,
        1,
        2000,
        0x4000_0000 + (1812 << 14),
        LocShape::CENTREPIECE_STRAIGHT,
        1,
        1,
        0,
        2000,
        2000,
        2000,
        2000,
    );
    // `finishBuild` flags the pass; `render_all` consumes it with the real
    // cache, exactly like the live build → draw cycle.
    world.share_light_pending = true;

    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &cache,
            0,
            192,
            1950,
            192,
            3,
            0,
            128,
        );
    }

    let index = world.scene_sprite_index(0, 1, 1).expect("portal placed");
    let frame = rw
        .sprite_frame_model(&world, &cache, 0, index)
        .expect("portal frame model materialises");
    let lit = frame.face_colour_a.as_ref().expect("lit colours");
    assert!(
        lit.iter().any(|&c| c != 0),
        "animated sharelight loc frames must be lit after the share-light pass; black wall regression"
    );
}

/// 512×334 viewport (the `area_game` size) bound as the render target.
/// `low_mem` stays false: `--window` play is highmem. Headless/bots default
/// lowmem (`--lowmem`); this fixture matches the live windowed raster.
fn viewport(pix: &mut Pix3DDraw, surface: &mut Pix2D) {
    pix.set_render_clipping(surface);
    pix.trans = 0;
}

/// Vertical south-facing wall (quad in X/Y at z=0). Y more-negative is up.
/// Unique shade so we can count wall pixels against the ground SHADE.
const WALL_SHADE: i32 = 40 * 128 + 80;

fn south_wall_model(winding_ccw_from_south: bool) -> Model {
    let mut model = Model {
        num_points: 4,
        point_x: Some(vec![-60, 60, 60, -60]),
        point_y: Some(vec![0, 0, -180, -180]),
        point_z: Some(vec![0, 0, 0, 0]),
        num_faces: 2,
        ..Default::default()
    };
    // Viewed from -Z (camera south of the wall, looking north): CCW is
    // (0,1,2)+(0,2,3); CW is the reverse. Live walls being visible from
    // inside but eaten from outside is a winding/cull split.
    let (a, b, c) = if winding_ccw_from_south {
        (vec![0, 0], vec![1, 2], vec![2, 3])
    } else {
        (vec![0, 0], vec![2, 3], vec![1, 2])
    };
    model.face_vertex_a = Some(a);
    model.face_vertex_b = Some(b);
    model.face_vertex_c = Some(c);
    model.face_colour_a = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.face_colour_b = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.face_colour_c = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.calc_bounding_cylinder();
    model
}

fn wall_pixel_count(winding_ccw_from_south: bool) -> usize {
    Pix3D::init_colour_table(0.6);
    let wall_rgb = Pix3D::colour_table()[WALL_SHADE as usize];
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    // SOUTH wall (WSHAPE0[3] = 8) on tile (1,2), in front of the vis-test camera.
    place_wall(
        &mut rw,
        &mut world,
        0,
        1,
        2,
        2000,
        8,
        0,
        Some(SceneModel::Model(south_wall_model(winding_ccw_from_south))),
        None,
        0,
        0,
    );
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            1950,
            192,
            3,
            0,
            128,
        );
    }
    map.pixels.iter().filter(|&&p| p == wall_rgb).count()
}

/// Camera-facing faces of a near loc must still rasterise. Live Seers bank /
/// Varrock fountain: those faces pop out as the camera rotates; a one-sided
/// south wall a few tiles in front of a live `camFollow` eye is the same
/// class of face (large on screen, wrapping i32 cross).
#[test]
fn near_camera_facing_south_wall_still_covers() {
    let ccw = wall_pixel_count_at(true, 192, 1950, 2 * 128 - 80);
    eprintln!("near south wall pixels={ccw} (eye 80 units in front of the wall)");
    assert!(
        ccw > 50,
        "south wall 80 units in front of the camera painted {ccw} pixels; live camera-facing cabinet/fountain faces vanish when they get large on screen"
    );
}

fn wall_pixel_count_at(winding_ccw_from_south: bool, eye_x: i32, eye_y: i32, eye_z: i32) -> usize {
    Pix3D::init_colour_table(0.6);
    let wall_rgb = Pix3D::colour_table()[WALL_SHADE as usize];
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    place_wall(
        &mut rw,
        &mut world,
        0,
        1,
        2,
        2000,
        8,
        0,
        Some(SceneModel::Model(south_wall_model(winding_ccw_from_south))),
        None,
        0,
        0,
    );
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            eye_x,
            eye_y,
            eye_z,
            3,
            0,
            128,
        );
    }
    map.pixels.iter().filter(|&&p| p == wall_rgb).count()
}

/// A south-facing wall on the tile in front of the orbit camera must cover
/// pixels. Live Draynor: stone missing from outside, shutters still there.
#[test]
fn south_facing_wall_covers_pixels_from_outside() {
    let ccw = wall_pixel_count(true);
    let cw = wall_pixel_count(false);
    eprintln!("south wall pixels ccw={ccw} cw={cw}");
    assert!(
        ccw > 50 || cw > 50,
        "south wall painted 0 unique pixels from the outside camera (ccw={ccw}, cw={cw}); live walls are eaten from outside"
    );
}

/// Type-2 occluder on the south wall's plane must not swallow that wall
/// from the outside camera (Java tests points *on* the plane as not hidden).
#[test]
fn south_wall_not_swallowed_by_its_own_occluder() {
    Pix3D::init_colour_table(0.6);
    let wall_rgb = Pix3D::colour_table()[WALL_SHADE as usize];
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    place_wall(
        &mut rw,
        &mut world,
        0,
        1,
        2,
        2000,
        8,
        0,
        Some(SceneModel::Model(south_wall_model(true))),
        None,
        0,
        0,
    );
    // finishBuild wall1 occluder: plane at tile_z*128, type 2, stored on
    // outdoor max_level 3.
    world.set_occlude(3, 2, 0, 2000 - 240, 2 * 128, 3 * 128, 2000, 2 * 128);

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            1950,
            192,
            3,
            0,
            128,
        );
    }
    let n = map.pixels.iter().filter(|&&p| p == wall_rgb).count();
    eprintln!("south wall pixels with own occluder={n}");
    assert!(
        n > 50,
        "own type-2 occluder swallowed the south wall ({n} pixels); that would eat stone from outside"
    );
}

/// Adjacent sharelight walls share an *edge* (2 verts), not a face. Java
/// `modelShareLight` only hides faces when 3+ verts merge. If the offset is
/// wrong and the two walls occupy the same space, every face becomes
/// `faceRenderType = -1` and the stone vanishes while walldecor remains.
#[test]
fn share_light_does_not_delete_adjacent_south_walls() {
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    let mut wall_a = south_wall_model(true);
    wall_a.face_colour = Some(vec![WALL_SHADE, WALL_SHADE]);
    wall_a.calculate_normals(64, 768, -50, -10, -50, false);
    let mut wall_b = south_wall_model(true);
    wall_b.face_colour = Some(vec![WALL_SHADE, WALL_SHADE]);
    wall_b.calculate_normals(64, 768, -50, -10, -50, false);
    place_wall(
        &mut rw,
        &mut world,
        0,
        1,
        2,
        2000,
        8,
        0,
        Some(SceneModel::Model(wall_a)),
        None,
        0,
        0,
    );
    place_wall(
        &mut rw,
        &mut world,
        0,
        2,
        2,
        2000,
        8,
        0,
        Some(SceneModel::Model(wall_b)),
        None,
        0,
        0,
    );
    rw.share_light(&mut world, 64, 768, -50, -10, -50);

    for x in [1, 2] {
        let SceneModel::Model(model) = rw
            .wall_model1(&world, &Cache::default(), 0, 0, x, 2)
            .expect("wall")
        else {
            panic!("model");
        };
        let killed = model
            .face_render_type
            .as_ref()
            .map(|rt| rt.iter().filter(|&&t| t == -1).count())
            .unwrap_or(0);
        assert!(
            killed < model.num_faces as usize,
            "tile ({x},2) shareLight deleted every face ({killed}/{}) — stone would vanish from outside",
            model.num_faces
        );
    }
}

/// Real `basic_wall_1` loc (shape 0 / SOUTH) must cover pixels from the
/// outdoor camera. Live Draynor: stone missing from outside, shutters remain.
fn loc_ob2(rel: &str) -> Option<Vec<u8>> {
    std::fs::read(client::content_dir().join("models/_sort/basic").join(rel)).ok()
}

fn basic_wall_ob2() -> Option<Vec<u8>> {
    loc_ob2("basic_wall_1.ob2")
}

fn textured_pix() -> Pix3DDraw {
    let mut pix = Pix3DDraw::default();
    if let Ok(bytes) = std::fs::read(client::cache_dir().join("textures")) {
        pix.unpack_textures(&JagFile::new(bytes));
        pix.init_pool(20);
        pix.init_texture_palettes(0.8);
    }
    pix
}

fn count_non_ground(
    world: &mut World,
    rw: &mut RenderWorld,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
) -> usize {
    count_non_ground_at(world, rw, eye_x, eye_y, eye_z, 0, 128)
}

fn count_non_ground_at(
    world: &mut World,
    rw: &mut RenderWorld,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    yaw: i32,
    pitch: i32,
) -> usize {
    let mut pix = textured_pix();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut *world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            eye_x,
            eye_y,
            eye_z,
            3,
            yaw,
            pitch,
        );
    }
    let ground = Pix3D::colour_table()[SHADE as usize];
    map.pixels
        .iter()
        .filter(|&&p| p != 0 && p != 1 && p != ground)
        .count()
}

/// `Client.camFollow` eye for an orbit camera (Java 1515).
fn orbit_eye(
    target_x: i32,
    target_y: i32,
    target_z: i32,
    pitch: i32,
    yaw: i32,
    distance: i32,
) -> (i32, i32, i32) {
    let inv_pitch = (2048 - pitch) & 0x7ff;
    let inv_yaw = (2048 - yaw) & 0x7ff;
    let mut x = 0i32;
    let mut y = 0i32;
    let mut z = distance;
    if inv_pitch != 0 {
        let sin = Pix3D::sin_table()[inv_pitch as usize];
        let cos = Pix3D::cos_table()[inv_pitch as usize];
        let tmp = (y * cos - distance * sin) >> 16;
        z = (y * sin + distance * cos) >> 16;
        y = tmp;
    }
    if inv_yaw != 0 {
        let sin = Pix3D::sin_table()[inv_yaw as usize];
        let cos = Pix3D::cos_table()[inv_yaw as usize];
        let tmp = (z * sin + x * cos) >> 16;
        z = (z * cos - x * sin) >> 16;
        x = tmp;
    }
    (target_x - x, target_y - y, target_z - z)
}

/// Draynor house stone is loc 1904–1910 `basic_painted*wall`. The outer faces
/// are gouraud (HSL 10339), not textured. Java `getModel` + `World.shareLight`
/// must light those faces so they cover from the outdoor camera. Unlit they
/// write colour-table 0 and look like holes (furniture shows through).
#[test]
fn painted_house_wall_covers_from_outside_after_share_light() {
    let Some(bytes) = loc_ob2("basic_painted1wall_1.ob2") else {
        return;
    };
    const ID: i32 = 42426;
    Model::unpack(ID, Some(&bytes));
    let loc = LocType {
        id: ID,
        model: Some(vec![ID]),
        shape: Some(vec![0]),
        sharelight: true,
        occlude: true,
        ..LocType::default()
    };
    let model = loc
        .get_model(
            &Cache::default(),
            0,
            LocAngle::SOUTH,
            2000,
            2000,
            2000,
            2000,
            -1,
        )
        .expect("painted1wall get_model");
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    place_wall(
        &mut rw,
        &mut world,
        0,
        1,
        2,
        2000,
        8,
        0,
        Some(SceneModel::Model(model)),
        None,
        0,
        0,
    );
    rw.share_light(&mut world, 64, 768, -50, -10, -50);
    let n = count_non_ground(&mut world, &mut rw, 192, 1950, 64);
    assert!(
        n > 50,
        "painted1wall shareLight painted {n} outdoor pixels; Draynor stone would be eaten from outside"
    );

    // Live orbit: pitch 128, distance pitch*3+600, looking north at the wall.
    let pitch = 128i32;
    let (ex, ey, ez) = orbit_eye(192, 1950, 256, pitch, 0, pitch * 3 + 600);
    let n = count_non_ground_at(&mut world, &mut rw, ex, ey, ez, 0, pitch);
    eprintln!("painted1wall orbit eye=({ex},{ey},{ez}) pixels={n}");
    assert!(
        n > 50,
        "painted1wall orbit camera painted {n} pixels (eye {ex},{ey},{ez}); live south facade is missing"
    );
}

/// Live groundh is `-height*8` (negative). An 8-tile SOUTH wall run becomes
/// a type-2 occluder; Java still draws that wall from outside (points on the
/// plane are not hidden). If we swallow it, stone vanishes while walldecor
/// (spriteOccluded) remains.
#[test]
fn eight_tile_south_occluder_does_not_eat_outside_walls() {
    let Some(bytes) = basic_wall_ob2() else {
        eprintln!("basic_wall_1.ob2 missing; skip");
        return;
    };
    const ID: i32 = 42425;
    Model::unpack(ID, Some(&bytes));
    let loc = LocType {
        id: ID,
        model: Some(vec![ID]),
        shape: Some(vec![0]),
        sharelight: true,
        occlude: true,
        ..LocType::default()
    };

    let groundh = -1600i32;
    let max_level = 4;
    let max_tile = 16;
    let heights = vec![vec![vec![groundh; max_tile + 1]; max_tile + 1]; max_level];
    let mut world = World::new(heights, max_tile as i32, max_level as i32, max_tile as i32);
    world.fill_base_level(0);
    for x in 0..max_tile as i32 {
        for z in 0..max_tile as i32 {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }

    let wall_z = 8i32;
    let mut rw = RenderWorld::new();
    for x in 4..12 {
        let model = loc
            .get_model(
                &Cache::default(),
                0,
                LocAngle::SOUTH,
                groundh,
                groundh,
                groundh,
                groundh,
                -1,
            )
            .expect("wall");
        place_wall(
            &mut rw,
            &mut world,
            0,
            x,
            wall_z,
            groundh,
            8,
            0,
            Some(SceneModel::Model(model)),
            None,
            0,
            0,
        );
    }
    rw.share_light(&mut world, 64, 768, -50, -10, -50);
    // finishBuild type-2 box for the 8-tile SOUTH run, stored on outdoor
    // max_level 3.
    world.set_occlude(
        3,
        2,
        4 * 128,
        groundh - 240,
        wall_z * 128,
        12 * 128,
        groundh,
        wall_z * 128,
    );

    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    Pix3D::init_colour_table(0.6);
    let mut pix = textured_pix();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        // Camera south of the wall run, 50 units above ground, looking north.
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            8 * 128,
            groundh - 50,
            6 * 128,
            3,
            0,
            128,
        );
    }
    let ground = Pix3D::colour_table()[SHADE as usize];
    let n = map
        .pixels
        .iter()
        .filter(|&&p| p != 0 && p != 1 && p != ground)
        .count();
    eprintln!("8-tile south occluder outdoor wall pixels={n}");
    assert!(
        n > 50,
        "type-2 occluder on a negative-groundh 8-tile SOUTH run ate the walls ({n} pixels)"
    );
}

/// A south wall in front of the camera must still cover when an interior
/// scenery sprite sits on the tile behind it. Live Draynor: furniture shows
/// through eaten stone — if fill defers the wall's back-pass until that
/// sprite tile drops and never comes back, the stone vanishes.
fn house_world_with_interior(rw: &mut RenderWorld, scenery: bool) -> World {
    let max_level = 1;
    let max_tile = 16i32;
    let groundh =
        vec![vec![vec![2000i32; max_tile as usize + 1]; max_tile as usize + 1]; max_level];
    let mut world = World::new(groundh, max_tile, max_level as i32, max_tile);
    world.fill_base_level(0);
    for x in 0..max_tile {
        for z in 0..max_tile {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    let wall_z = 8i32;
    for x in 7..9 {
        place_wall(
            &mut *rw,
            &mut world,
            0,
            x,
            wall_z,
            2000,
            8,
            0,
            Some(SceneModel::Model(south_wall_model(true))),
            None,
            0,
            0,
        );
    }
    if scenery {
        place_scenery(
            &mut *rw,
            &mut world,
            0,
            7,
            9,
            2000,
            SceneModel::Model(one_face_model()),
            0,
            0,
            1,
            1,
            0,
        );
    }
    world
}

fn count_wall_shade(
    rw: &mut RenderWorld,
    world: &mut World,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
) -> usize {
    Pix3D::init_colour_table(0.6);
    let wall_rgb = Pix3D::colour_table()[WALL_SHADE as usize];
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut *world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            eye_x,
            eye_y,
            eye_z,
            3,
            0,
            128,
        );
    }
    map.pixels.iter().filter(|&&p| p == wall_rgb).count()
}

#[test]
fn interior_scenery_does_not_eat_south_wall_from_outside() {
    let mut rw_empty = RenderWorld::new();
    let mut empty = house_world_with_interior(&mut rw_empty, false);
    let without = count_wall_shade(&mut rw_empty, &mut empty, 8 * 128, 1950, 6 * 128);
    let mut rw_furnished = RenderWorld::new();
    let mut furnished = house_world_with_interior(&mut rw_furnished, true);
    let with = count_wall_shade(&mut rw_furnished, &mut furnished, 8 * 128, 1950, 6 * 128);
    eprintln!("south wall pixels without interior={without} with interior={with}");
    assert!(
        without > 50,
        "south wall of the house painted {without} pixels with no interior"
    );
    assert!(
        with > 50,
        "interior scenery ate the south wall from outside ({with} pixels, empty house had {without})"
    );
}

/// Live screenshot: west stone visible, south facade gone. Camera is south-
/// *east* of the house (gx != wall tile x), so MIDTAB marks the SOUTH wall
/// as a "corner" and fill's back-pass is skipped — it must still draw via
/// the corner-sides path (Java does).
#[test]
fn south_wall_covers_when_camera_is_southeast() {
    let max_tile = 16i32;
    let groundh = vec![vec![vec![2000i32; max_tile as usize + 1]; max_tile as usize + 1]; 1];
    let mut world = World::new(groundh, max_tile, 1, max_tile);
    world.fill_base_level(0);
    for x in 0..max_tile {
        for z in 0..max_tile {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    let mut rw = RenderWorld::new();
    place_wall(
        &mut rw,
        &mut world,
        0,
        8,
        8,
        2000,
        8,
        0,
        Some(SceneModel::Model(south_wall_model(true))),
        None,
        0,
        0,
    );
    let aligned = count_wall_shade(&mut rw, &mut world, 8 * 128, 1950, 6 * 128);
    // One tile west: gx=7, tile_x=8 → direction 2, SOUTH bit hits MIDTAB.
    let offset = count_wall_shade(&mut rw, &mut world, 7 * 128, 1950, 6 * 128);
    eprintln!("SOUTH wall pixels aligned_cam={aligned} southeast_cam={offset}");
    assert!(
        aligned > 50,
        "SOUTH wall painted {aligned} pixels from a south-aligned camera"
    );
    assert!(
        offset > 50,
        "SOUTH wall painted {offset} pixels from a southeast camera (aligned={aligned}); live south facades are missing"
    );
}

/// Overlay-edge tiles (shape ≥ 2) mix underlay and overlay face colours.
/// Black overlay faces here are the slice-2 "triangles by the bridge" bug.
#[test]
fn overlay_edge_ground_keeps_dirt_and_grass_face_colours() {
    let g = Ground::new(
        1,
        1,
        TerrainOverlayShape::LEFT_SEMI_DIAGONAL_SMALL,
        0,
        -1,
        2000,
        2000,
        2000,
        2000,
        100,
        100,
        100,
        100,
        200,
        200,
        200,
        200,
        0,
        0,
    );
    let colours = &g.face_colour_a[..g.faces()];
    assert!(
        colours.contains(&100),
        "underlay faces must keep grass colour, got {:?}",
        colours
    );
    assert!(
        colours.contains(&200),
        "overlay faces must keep dirt colour (not black), got {:?}",
        colours
    );
    assert!(
        colours.iter().all(|&c| c != 0),
        "no overlay-edge face may be colour 0 (black)"
    );
}

#[test]
fn update_mouse_picking_sets_click_and_clears_ground() {
    let mut w = flat_world();
    w.ground_x = 3;
    w.update_mouse_picking(10, 20);
    assert!(w.click);
    assert_eq!(w.click_x, 10);
    assert_eq!(w.click_y, 20);
    assert_eq!(w.ground_x, -1);
    assert_eq!(w.ground_z, -1);
}

#[test]
fn render_all_writes_pixels_and_picks_ground_tile() {
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        // Camera at (192, 0, 192) (tile 1,1), pitch 512 looking horizontal
        // at the height-2000 ground. Tile (1,2) projects to screen
        // (240..272, 118..151); (256, 134) is inside its first triangle.
        world.update_mouse_picking(256, 134);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            0,
            192,
            0,
            0,
            512,
        );
    }

    assert!(
        map.pixels.iter().any(|&p| p != 0),
        "render_all must draw the ground into the viewport"
    );
    assert_eq!(world.ground_x, 1);
    assert_eq!(world.ground_z, 2);
    assert!(
        !world.click,
        "a successful pick must drop click so the next frame cannot re-pick as the camera moves"
    );

    // Second pass without a new click: must not hop the dest tile.
    world.ground_x = -1;
    world.ground_z = -1;
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            0,
            192,
            0,
            0,
            512,
        );
    }
    assert_eq!(world.ground_x, -1);
    assert_eq!(world.ground_z, -1);
}

#[test]
fn render_all_renders_scenery_sprites() {
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    // A scenery sprite on tile (1,2), in front of the camera. The typecode
    // is a real loc typecode (bits 29-30 = 2, so get_scene finds it).
    let mut rw = RenderWorld::new();
    place_scenery(
        &mut rw,
        &mut world,
        0,
        1,
        2,
        2000,
        SceneModel::Model(one_face_model()),
        0x40000000 + (5 << 14),
        0,
        1,
        1,
        0,
    );
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        world.update_mouse_picking(256, 134);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            0,
            192,
            0,
            0,
            512,
        );
    }

    // The sprite's model is rendered this cycle (cycle stamped) and the
    // viewport got pixels from it and/or the ground.
    let sprite = world.get_scene(0, 1, 2).expect("sprite on tile (1,2)");
    assert_eq!(sprite.cycle, 1, "sprite must be rendered once this cycle");
    assert!(map.pixels.iter().any(|&p| p != 0));
}

/// The `Client.ts` loadGame 1225-1233 camera distance table (`resetVisCalc`'s
/// `pitchDistance` argument).
fn game_distance_table() -> [i32; 9] {
    let mut distance = [0i32; 9];
    for (x, slot) in distance.iter_mut().enumerate() {
        let angle = x as i32 * 32 + 128 + 15;
        let offset = angle * 3 + 600;
        let sin = Pix3D::sin_table()[angle as usize];
        *slot = (offset * sin) >> 16;
    }
    distance
}

/// `resetVisCalc` populates `visBacking`; without it every tile in the 51×51
/// camera window fails the visibility test (Task 4's report). The orbit
/// camera at pitch 128 (looking up 22.5°) sits below the ground plane, so
/// `ground_h - eyeY = 50` never fires the `>= 2000` gate — tile (1,2)
/// renders only because `resetVisCalc` marked it visible.
#[test]
fn reset_vis_calc_marks_near_tiles_visible() {
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);

    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            1950,
            192,
            3,
            0,
            128,
        );
    }
    assert!(
        map.pixels.iter().any(|&p| p != 0),
        "resetVisCalc must mark near tiles visible so render_all draws them"
    );
}

/// A 64×64 checkerboard texture (palette indices 1/2 in 32×32 blocks) so
/// the textured quick-ground path samples visibly different texels for the
/// TS flat vs non-flat texture-corner branches.
fn checkerboard_pix() -> Pix3DDraw {
    let mut texture = client::graphics::Pix8::new(64, 64, vec![0, 0xff0000, 0x0000ff]);
    for y in 0..64 {
        for x in 0..64 {
            texture.data[y * 64 + x] = if (x < 32) != (y < 32) { 1 } else { 2 };
        }
    }
    let mut pix = Pix3DDraw::default();
    pix.textures[0] = Some(texture);
    pix.tex_pal[0] = Some(vec![0, 0xff0000, 0x0000ff]);
    pix.num_textures = 1;
    pix.init_pool(1);
    pix
}

/// A world whose only tile content is a DIAGONAL quick ground at (1,2)
/// textured with id 0. `non_flat` flips only the `QuickGround.flat` flag
/// (via differing `set_ground` heights); `groundh` stays flat at 2000 in
/// both cases so the projection geometry is identical and only the
/// texture-corner branch can differ.
fn diagonal_world(non_flat: bool) -> World {
    let max_level = 1;
    let max_tile_x = 3;
    let max_tile_z = 3;
    let groundh = vec![
        vec![vec![2000i32; max_tile_z as usize + 1]; max_tile_x as usize + 1];
        max_level as usize
    ];
    let mut world = World::new(groundh, max_tile_z, max_level, max_tile_x);
    world.fill_base_level(0);
    let (h_sw, h_se, h_ne, h_nw) = if non_flat {
        (2000, 2000, 2000, 2100)
    } else {
        (2000, 2000, 2000, 2000)
    };
    world.set_ground(
        0,
        1,
        2,
        TerrainOverlayShape::DIAGONAL,
        0,
        0,
        h_sw,
        h_se,
        h_ne,
        h_nw,
        SHADE,
        SHADE,
        SHADE,
        SHADE,
        SHADE,
        SHADE,
        SHADE,
        SHADE,
        0,
        0,
    );
    world
}

#[test]
fn render_all_non_flat_diagonal_quick_ground_uses_own_corners() {
    Pix3D::init_colour_table(0.6);

    let mut flat_world = diagonal_world(false);
    let mut flat_rw = RenderWorld::new();
    let mut flat_pix = checkerboard_pix();
    let mut flat_map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut flat_map.pixels, flat_map.width, flat_map.height);
        flat_pix.set_render_clipping(&surface);
        flat_pix.trans = 0;
        flat_rw.render_all(
            &mut flat_world,
            &mut flat_pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            0,
            192,
            0,
            0,
            512,
        );
    }

    let mut nonflat_world = diagonal_world(true);
    let mut nonflat_rw = RenderWorld::new();
    let mut nonflat_pix = checkerboard_pix();
    let mut nonflat_map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(
            &mut nonflat_map.pixels,
            nonflat_map.width,
            nonflat_map.height,
        );
        nonflat_pix.set_render_clipping(&surface);
        nonflat_pix.trans = 0;
        nonflat_rw.render_all(
            &mut nonflat_world,
            &mut nonflat_pix,
            &mut surface,
            &Cache::default(),
            0,
            192,
            0,
            192,
            0,
            0,
            512,
        );
    }

    // Both renders must actually draw the textured ground ...
    assert!(
        flat_map.pixels.iter().any(|&p| p != 0),
        "flat quick ground must draw"
    );
    assert!(
        nonflat_map.pixels.iter().any(|&p| p != 0),
        "non-flat quick ground must draw"
    );
    // ... and the non-flat tile must map its own corners, not the flat
    // permuted ones (TS 2004-2029): identical geometry, different texture
    // sampling, so the two frames differ.
    assert_ne!(
        flat_map.pixels, nonflat_map.pixels,
        "non-flat DIAGONAL quick ground must use the non-flat texture corners"
    );
}

/// `World.render2DGround` (TS 798-856): a PLAIN tile's `QuickGround`
/// `minimapRgb` (the `overlay` argument of `set_ground`) fills a 4×4 block
/// in the minimap buffer at `offset` with row `step` 512.
#[test]
fn render_2d_ground_plain_quick_fills_4x4() {
    let max_level: i32 = 1;
    let max_tile_x: i32 = 3;
    let max_tile_z: i32 = 3;
    let groundh = vec![
        vec![vec![2000i32; max_tile_z as usize + 1]; max_tile_x as usize + 1];
        max_level as usize
    ];
    let mut world = World::new(groundh, max_tile_z, max_level, max_tile_x);
    world.fill_base_level(0);
    world.set_ground(
        0,
        1,
        1,
        TerrainOverlayShape::PLAIN,
        0,
        -1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0x00aabb,
        0,
    );

    let mut dst = vec![0i32; 512 * 512];
    let rw = RenderWorld::new();
    rw.render_2d_ground(&world, 0, 1, 1, &mut dst, 0, 512);
    assert_ne!(dst[0], 0);
    assert_eq!(dst[0], 0x00aabb);
    // a full 4×4 block, one pixel outside stays untouched
    for i in 0..4 {
        assert_eq!(dst[i * 512], 0x00aabb);
        assert_eq!(dst[i * 512 + 3], 0x00aabb);
    }
    assert_eq!(dst[4], 0);
    assert_eq!(dst[512 * 4], 0);
}

/// Distinct north/south shades so we can tell which side of a 2×2 loc
/// actually rasterised. Live Varrock fountain / Seers cabinets: the
/// camera-facing side pops out while the far side stays.
const SOUTH_SHADE: i32 = 40 * 128 + 80;
const NORTH_SHADE: i32 = 90 * 128 + 40;

/// Axis-aligned 2×2-tile box centred on the origin. South wall at z=-128
/// uses the same CCW-from-south winding as `south_wall_model(true)`; north
/// wall at z=+128 is the reverse so it faces +Z.
fn ns_box_model(sharelight_cube: bool) -> Model {
    let mut model = Model {
        num_points: 8,
        point_x: Some(vec![-128, 128, 128, -128, -128, 128, 128, -128]),
        point_y: Some(vec![0, 0, -200, -200, 0, 0, -200, -200]),
        point_z: Some(vec![-128, -128, -128, -128, 128, 128, 128, 128]),
        num_faces: 4,
        ..Default::default()
    };
    // South (z=-128): (0,1,2)+(0,2,3). North (z=+128): reversed (4,6,5)+(4,7,6).
    model.face_vertex_a = Some(vec![0, 0, 4, 4]);
    model.face_vertex_b = Some(vec![1, 2, 6, 7]);
    model.face_vertex_c = Some(vec![2, 3, 5, 6]);
    model.face_colour_a = Some(vec![SOUTH_SHADE, SOUTH_SHADE, NORTH_SHADE, NORTH_SHADE]);
    model.face_colour_b = Some(vec![SOUTH_SHADE, SOUTH_SHADE, NORTH_SHADE, NORTH_SHADE]);
    model.face_colour_c = Some(vec![SOUTH_SHADE, SOUTH_SHADE, NORTH_SHADE, NORTH_SHADE]);
    if sharelight_cube {
        model.face_colour = Some(vec![SOUTH_SHADE, SOUTH_SHADE, NORTH_SHADE, NORTH_SHADE]);
        model.calculate_normals(64, 768, -50, -10, -50, false);
        model.face_colour_a = Some(vec![SOUTH_SHADE, SOUTH_SHADE, NORTH_SHADE, NORTH_SHADE]);
        model.face_colour_b = Some(vec![SOUTH_SHADE, SOUTH_SHADE, NORTH_SHADE, NORTH_SHADE]);
        model.face_colour_c = Some(vec![SOUTH_SHADE, SOUTH_SHADE, NORTH_SHADE, NORTH_SHADE]);
    } else {
        model.calc_bounding_cylinder();
    }
    model
}

fn count_shades(pixels: &[i32], south_rgb: i32, north_rgb: i32) -> (usize, usize) {
    let south = pixels.iter().filter(|&&p| p == south_rgb).count();
    let north = pixels.iter().filter(|&&p| p == north_rgb).count();
    (south, north)
}

/// Direct `world_render` (no World.fill): if the south face is missing here,
/// the hole is Model projection/winding/depth, not fill order.
fn render_box_in_world(sharelight_cube: bool, yaw: i32, pitch: i32) -> (usize, usize) {
    Pix3D::init_colour_table(0.6);
    let south_rgb = Pix3D::colour_table()[SOUTH_SHADE as usize];
    let north_rgb = Pix3D::colour_table()[NORTH_SHADE as usize];
    let max_tile = 16i32;
    let groundh = vec![vec![vec![2000i32; max_tile as usize + 1]; max_tile as usize + 1]; 1];
    let mut world = World::new(groundh, max_tile, 1, max_tile);
    world.fill_base_level(0);
    for x in 0..max_tile {
        for z in 0..max_tile {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    let mut rw = RenderWorld::new();
    place_scenery(
        &mut rw,
        &mut world,
        0,
        6,
        8,
        2000,
        SceneModel::Model(ns_box_model(sharelight_cube)),
        0,
        0,
        2,
        2,
        0,
    );
    // Corner wall on a footprint tile: Java defers the sprite until this
    // wall's corner-sides handshake completes (`(spans & cornerSides) ==
    // sidesAfterCorner`). Without that defer the loc draws too early and
    // later tiles eat the facing side.
    place_wall(
        &mut rw,
        &mut world,
        0,
        7,
        9,
        2000,
        16,
        0,
        Some(SceneModel::Model(south_wall_model(true))),
        None,
        0,
        0,
    );
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        // South of the loc looking north (yaw=0), or north looking south (yaw=1024).
        let (eye_x, eye_y, eye_z) = if yaw == 0 {
            (7 * 128, 1950, 6 * 128)
        } else {
            (7 * 128, 1950, 11 * 128)
        };
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            eye_x,
            eye_y,
            eye_z,
            3,
            yaw,
            pitch,
        );
    }
    count_shades(&map.pixels, south_rgb, north_rgb)
}

#[test]
fn camera_facing_box_faces_draw_as_2x2_scenery() {
    let pitch = 128i32;
    let (south, north) = render_box_in_world(false, 0, pitch);
    eprintln!("fill cylinder yaw0: south={south} north={north}");
    let (south_c, north_c) = render_box_in_world(true, 0, pitch);
    eprintln!("fill cube yaw0: south={south_c} north={north_c}");
    let (south_back, north_back) = render_box_in_world(true, 1024, pitch);
    eprintln!("fill cube yaw1024: south={south_back} north={north_back}");

    assert!(
        south > 50 || south_c > 50,
        "2x2 scenery south face painted 0 pixels from the south camera (cyl={south} cube={south_c}); live fountain basin / cabinet glass vanish on the facing side"
    );
    // Rotating 180° must not steal the (now) camera-facing north wall.
    assert!(
        north_back > 50,
        "2x2 scenery north face painted {north_back} pixels from a north camera; live cabinets pop in/out as the camera rotates"
    );
}

fn pack_config() -> Option<Cache> {
    let bytes = std::fs::read(client::cache_dir().join("config")).ok()?;
    Some(Cache::unpack(&JagFile::new(bytes)))
}

fn fountain_ob2() -> Option<Vec<u8>> {
    std::fs::read(
        client::content_dir().join("models/_sort/outdoorfurniture/outdoorfurniture_fountain.ob2"),
    )
    .ok()
}

/// Live Varrock plaza: camera-facing basin walls missing, water/statues stay.
/// Loc 879 `outdoorfurniture_fountain` is a 2×2 centrepiece (shape 10).
#[test]
fn varrock_fountain_facing_side_covers() {
    let Some(cache) = pack_config() else {
        eprintln!("config jag missing; skip");
        return;
    };
    let Some(bytes) = fountain_ob2() else {
        eprintln!("fountain ob2 missing; skip");
        return;
    };
    let loc_idx = cache
        .locs
        .iter()
        .position(|l| l.id == 879 || l.name.to_lowercase().contains("fountain"))
        .expect("loc 879 fountain");
    let model_id = cache.locs[loc_idx]
        .model
        .as_ref()
        .and_then(|m| m.first())
        .copied()
        .unwrap_or(1497);
    let width = cache.locs[loc_idx].width.max(1);
    let length = cache.locs[loc_idx].length.max(1);
    {
        let loc = &cache.locs[loc_idx];
        eprintln!(
            "fountain loc id={} name={:?} width={} length={} sharelight={} hillskew={} occlude={} ambient={} contrast={} models={:?} shapes={:?}",
            loc.id, loc.name, loc.width, loc.length, loc.sharelight, loc.hillskew, loc.occlude, loc.ambient, loc.contrast, loc.model, loc.shape
        );
    }
    Model::unpack(model_id, Some(&bytes));
    let groundh = -1600i32;
    let model = cache.locs[loc_idx]
        .get_model(
            &Cache::default(),
            10,
            LocAngle::WEST,
            groundh,
            groundh,
            groundh,
            groundh,
            -1,
        )
        .expect("fountain get_model");
    eprintln!(
        "fountain after get_model: points={} faces={} radius={} min_y={} max_y={} min_depth={} max_depth={} priority={} face_priority={} face_render_type={}",
        model.num_points,
        model.num_faces,
        model.radius,
        model.min_y,
        model.max_y,
        model.min_depth,
        model.max_depth,
        model.priority,
        model.face_priority.as_ref().map(|p| format!("len{} max{:?}", p.len(), p.iter().max())).unwrap_or_else(|| "none".into()),
        model.face_render_type.as_ref().map(|p| format!("len{} killed{}", p.len(), p.iter().filter(|&&t| t == -1).count())).unwrap_or_else(|| "none".into()),
    );
    if let Some(fp) = model.face_priority.as_ref() {
        let mut hist = [0u32; 12];
        for &p in fp {
            if (0..12).contains(&p) {
                hist[p as usize] += 1;
            }
        }
        eprintln!("fountain face_priority hist={hist:?}");
    }

    Pix3D::init_colour_table(0.6);
    let max_tile = 16i32;
    let heights = vec![vec![vec![groundh; max_tile as usize + 1]; max_tile as usize + 1]; 1];
    let mut world = World::new(heights, max_tile, 1, max_tile);
    world.fill_base_level(0);
    for x in 0..max_tile {
        for z in 0..max_tile {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    let mut rw = RenderWorld::new();
    place_scenery(
        &mut rw,
        &mut world,
        0,
        6,
        8,
        groundh,
        SceneModel::Model(model),
        0,
        0,
        width,
        length,
        0,
    );
    rw.share_light(&mut world, 64, 768, -50, -10, -50);

    if let Some(l879) = cache.locs.get(879) {
        eprintln!(
            "loc 879 name={:?} width={} length={} sharelight={} models={:?} shapes={:?}",
            l879.name, l879.width, l879.length, l879.sharelight, l879.model, l879.shape
        );
    }

    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let ground = Pix3D::colour_table()[SHADE as usize];
    let mut count_at = |yaw: i32, eye_x: i32, eye_y: i32, eye_z: i32| -> usize {
        let mut pix = textured_pix();
        let mut map = PixMap::new(512, 334);
        {
            let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
            viewport(&mut pix, &mut surface);
            rw.render_all(
                &mut world,
                &mut pix,
                &mut surface,
                &Cache::default(),
                0,
                eye_x,
                eye_y,
                eye_z,
                3,
                yaw,
                128,
            );
        }
        map.pixels
            .iter()
            .filter(|&&p| p != 0 && p != 1 && p != ground)
            .count()
    };

    // Live `camFollow`: orbit around the fountain centre, pitch 128,
    // distance pitch*3+600. yaw 0 looks north (south face nearer); 1024 looks south.
    let pitch = 128i32;
    let distance = pitch * 3 + 600;
    let (sx, sy, sz) = orbit_eye(896, groundh - 50, 1152, pitch, 0, distance);
    let (nx, ny, nz) = orbit_eye(896, groundh - 50, 1152, pitch, 1024, distance);
    eprintln!("orbit south eye=({sx},{sy},{sz}) north eye=({nx},{ny},{nz})");
    let south = count_at(0, sx, sy, sz);
    let north = count_at(1024, nx, ny, nz);
    eprintln!("fountain pixels from_south={south} from_north={north}");
    assert!(
        south > 50,
        "fountain painted {south} pixels from the south; live basin walls on the facing side are missing"
    );
    assert!(
        north > 50,
        "fountain painted {north} pixels from the north; live cabinets/fountain pop in/out as the camera rotates"
    );
}

/// Live Varrock/Seers: facing faces vanish only in a populated scene.
/// Surround the fountain with SOUTH walls and 1×1 scenery and look from
/// the south; the isolated fountain (above) stays in the 9k pixel range.
#[test]
fn fountain_facing_side_survives_dense_neighbours() {
    let Some(cache) = pack_config() else {
        eprintln!("config jag missing; skip");
        return;
    };
    let Some(bytes) = fountain_ob2() else {
        eprintln!("fountain ob2 missing; skip");
        return;
    };
    let loc_idx = cache
        .locs
        .iter()
        .position(|l| l.id == 879)
        .expect("loc 879");
    let model_id = cache.locs[loc_idx]
        .model
        .as_ref()
        .and_then(|m| m.first())
        .copied()
        .unwrap_or(1497);
    Model::unpack(model_id, Some(&bytes));
    let groundh = -1600i32;
    let model = cache.locs[loc_idx]
        .get_model(
            &Cache::default(),
            10,
            LocAngle::WEST,
            groundh,
            groundh,
            groundh,
            groundh,
            -1,
        )
        .expect("fountain get_model");

    Pix3D::init_colour_table(0.6);
    let max_tile = 32i32;
    let heights = vec![vec![vec![groundh; max_tile as usize + 1]; max_tile as usize + 1]; 4];
    let mut world = World::new(heights, max_tile, 4, max_tile);
    world.fill_base_level(0);
    for x in 0..max_tile {
        for z in 0..max_tile {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    let fx = 14i32;
    let fz = 16i32;
    let mut rw = RenderWorld::new();
    place_scenery(
        &mut rw,
        &mut world,
        0,
        fx,
        fz,
        groundh,
        SceneModel::Model(model),
        0,
        0,
        2,
        2,
        0,
    );
    for x in 8..22 {
        for z in 10..24 {
            if x >= fx && x < fx + 2 && z >= fz && z < fz + 2 {
                continue;
            }
            place_wall(
                &mut rw,
                &mut world,
                0,
                x,
                z,
                groundh,
                8,
                0,
                Some(SceneModel::Model(south_wall_model(true))),
                None,
                0,
                0,
            );
            place_scenery(
                &mut rw,
                &mut world,
                0,
                x,
                z,
                groundh,
                SceneModel::Model(one_face_model()),
                0,
                0,
                1,
                1,
                0,
            );
        }
    }
    rw.share_light(&mut world, 64, 768, -50, -10, -50);
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);

    let pitch = 128i32;
    let (ex, ey, ez) = orbit_eye(
        (fx * 128) + 128,
        groundh - 50,
        fz * 128,
        pitch,
        0,
        pitch * 3 + 600,
    );
    let mut pix = textured_pix();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            ex,
            ey,
            ez,
            3,
            0,
            pitch,
        );
    }
    let ground = Pix3D::colour_table()[SHADE as usize];
    let wall = Pix3D::colour_table()[WALL_SHADE as usize];
    let n = map
        .pixels
        .iter()
        .filter(|&&p| p != 0 && p != 1 && p != ground && p != wall)
        .count();
    eprintln!("dense-neighbour fountain non-wall pixels={n} eye=({ex},{ey},{ez})");
    assert!(
        n > 200,
        "fountain in a dense wall/scenery neighbourhood painted {n} non-wall pixels from the south (isolated orbit is ~9k); live facing basin/glass vanish among neighbours"
    );
}

/// Live is a 104×104 vis window. Java/TS `LinkList.push` unlinks a Square
/// already in `fillQueue` and moves it to the tail; a VecDeque of coordinates
/// that only `push_back`s draws the loc too early so later tiles eat the
/// camera-facing side.
#[test]
fn camera_facing_box_survives_full_vis_window() {
    let pitch = 128i32;
    Pix3D::init_colour_table(0.6);
    let south_rgb = Pix3D::colour_table()[SOUTH_SHADE as usize];
    let max_tile = 104i32;
    let groundh = 2000i32;
    let heights = vec![vec![vec![groundh; max_tile as usize + 1]; max_tile as usize + 1]; 4];
    let mut world = World::new(heights, max_tile, 4, max_tile);
    world.fill_base_level(0);
    for x in 0..max_tile {
        for z in 0..max_tile {
            world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                0,
                0,
                0,
                0,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                SHADE,
                0,
                0,
            );
        }
    }
    let mut box_model = ns_box_model(false);
    // Basin-height walls: live fountain rim is short; a 200-unit south
    // face can survive overdraw that eats a 40-unit face.
    if let Some(y) = box_model.point_y.as_mut() {
        for v in y.iter_mut() {
            if *v < 0 {
                *v = -40;
            }
        }
    }
    box_model.calc_bounding_cylinder();
    let mut rw = RenderWorld::new();
    place_scenery(
        &mut rw,
        &mut world,
        0,
        50,
        52,
        groundh,
        SceneModel::Model(box_model),
        0,
        0,
        2,
        2,
        0,
    );
    for x in 40..60 {
        for z in 42..62 {
            if (50..52).contains(&x) && (52..54).contains(&z) {
                continue;
            }
            place_scenery(
                &mut rw,
                &mut world,
                0,
                x,
                z,
                groundh,
                SceneModel::Model(one_face_model()),
                0,
                0,
                1,
                1,
                0,
            );
        }
    }
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let (ex, ey, ez) = orbit_eye(51 * 128, groundh - 50, 52 * 128, pitch, 0, pitch * 3 + 600);
    let mut pix = Pix3DDraw::default();
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        viewport(&mut pix, &mut surface);
        rw.render_all(
            &mut world,
            &mut pix,
            &mut surface,
            &Cache::default(),
            0,
            ex,
            ey,
            ez,
            3,
            0,
            pitch,
        );
    }
    let south = map.pixels.iter().filter(|&&p| p == south_rgb).count();
    eprintln!("104-map south-face pixels={south} eye=({ex},{ey},{ez})");
    assert!(
        south > 50,
        "2×2 south face painted {south} pixels in a 104×104 vis window; live facing fountain/cabinet/wall faces vanish"
    );
}

// --- Task 3: sim/render world split ---

/// The sim half of the world resolves typecodes with no `Renderer` (and no
/// render world) constructed — the headless bot path. A scene built through
/// the `Client` sim API must answer `wall_type`/`type_code2`/`scene_type`
/// from the per-tile typecodes alone.
#[test]
fn core_world_has_typecodes_without_renderer() {
    use client::core::world::World;
    let max_level: i32 = 1;
    let max_tile: i32 = 3;
    let groundh =
        vec![vec![vec![2000i32; max_tile as usize + 1]; max_tile as usize + 1]; max_level as usize];
    let mut world = World::new(groundh, max_tile, max_level, max_tile);
    world.fill_base_level(0);

    let wall_typecode = 0x4000_0000 + 100;
    world.set_wall(0, 1, 1, 2000, 8, 0, wall_typecode, 0x1f, 0, 0, 0, 0);
    assert_eq!(world.wall_type(0, 1, 1), wall_typecode);
    assert_eq!(world.type_code2(0, 1, 1, wall_typecode), 0x1f);
    assert_eq!(world.type_code2(0, 1, 1, wall_typecode + 1), -1);

    let scene_typecode = 0x4000_0000 + 200;
    assert!(world.add_scenery(0, 1, 1, 2000, scene_typecode, 0x7f, 1, 1, 0, 0, 0, 0, 0));
    assert_eq!(world.scene_type(0, 1, 1), scene_typecode);
    assert_eq!(world.type_code2(0, 1, 1, scene_typecode), 0x7f);

    // The `Client.world` field is the same sim world: a scene placed
    // through the Client resolves typecodes without constructing a
    // renderer or a render world.
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    let client_wall_tc = 0x4000_0000 + 300;
    c.world
        .set_wall(0, 1, 1, 0, 8, 0, client_wall_tc, 0, 0, 0, 0, 0);
    assert_eq!(c.world.wall_type(0, 1, 1), client_wall_tc);
    assert!(c.world.type_code2(0, 1, 1, client_wall_tc) >= 0);
}
