// Task 7 (wgpu backend): the GPU scene mesh must cover the same scene the
// CPU fill rasterizes — ground quads + placed model faces, transformed to
// camera space and shaded from the colour table — because it is the exact
// input the wgpu backend rasterizes. Runs without any GPU (pure CPU math).
// This binary owns the mesh-builder test only: it pins the process-wide
// colour table to one brightness, and `Renderer::new` (which re-inits the
// table at 0.8) must not run in the same process — so the backend-selection
// tests live in `gpu_backend.rs` instead.
use client::config::Cache;
use client::core::World;
use client::dash3d::{SceneModel, TerrainOverlayShape};
use client::graphics::{Pix3D, Pix3DDraw};
use client::render::RenderWorld;

const SHADE: i32 = 200 * 128 + 100;
const WALL_SHADE: i32 = 40 * 128 + 80;

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

/// Vertical south-facing wall quad (X/Y at z=0), shaded WALL_SHADE.
fn south_wall_model() -> client::dash3d::Model {
    let mut model = client::dash3d::Model::default();
    model.num_points = 4;
    model.point_x = Some(vec![-60, 60, 60, -60]);
    model.point_y = Some(vec![0, 0, -180, -180]);
    model.point_z = Some(vec![0, 0, 0, 0]);
    model.num_faces = 2;
    model.face_vertex_a = Some(vec![0, 0]);
    model.face_vertex_b = Some(vec![1, 2]);
    model.face_vertex_c = Some(vec![2, 3]);
    model.face_colour_a = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.face_colour_b = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.face_colour_c = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.calc_bounding_cylinder();
    model
}

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

/// The GPU scene mesh (the wgpu backend's rasterization input) must cover
/// the ground quads and the placed model faces. The wall is on tile (1, 2)
/// in front of the vis-test camera, the same placement the world.rs CPU
/// path tests use; the mesh must carry both shades.
#[test]
fn scene_mesh_builds_ground_and_wall_triangles() {
    Pix3D::init_colour_table(0.6);
    let ground_rgb = Pix3D::colour_table()[SHADE as usize];
    let wall_rgb = Pix3D::colour_table()[WALL_SHADE as usize];
    assert_ne!(ground_rgb, wall_rgb, "the fixture shades must differ");

    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(
        &world,
        0,
        1,
        2,
        Some(SceneModel::Model(south_wall_model())),
        None,
    );
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);

    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    // The CPU-path eye: 192, 1950, 192, max_level 3, yaw 0, pitch 128.
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);
    let opaque_len = mesh.opaque_len();
    let vertices = mesh.vertices();

    assert!(!vertices.is_empty(), "a camera-facing world must mesh geometry");
    assert!(
        opaque_len > 0 && opaque_len <= vertices.len(),
        "the opaque prefix must cover the (all-opaque) mesh"
    );

    let mut found_ground = false;
    let mut found_wall = false;
    for v in vertices.iter() {
        let rgb = ((v.r as i32) << 16) | ((v.g as i32) << 8) | v.b as i32;
        found_ground |= rgb == ground_rgb;
        found_wall |= rgb == wall_rgb;
        assert!(
            v.z.is_finite() && v.x.is_finite() && v.y.is_finite(),
            "every mesh vertex is a finite camera-space position"
        );
    }
    assert!(
        found_ground,
        "the ground quads must be in the mesh (shade {SHADE})"
    );
    assert!(
        found_wall,
        "the placed wall must be in the mesh (shade {WALL_SHADE})"
    );
}
