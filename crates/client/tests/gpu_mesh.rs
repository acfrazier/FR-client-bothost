// Task 7 (wgpu backend): the GPU scene mesh must cover the same scene the
// CPU fill rasterizes — ground quads + placed model faces, transformed to
// camera space and shaded from the colour table — because it is the exact
// input the wgpu backend rasterizes. Runs without any GPU (pure CPU math).
// This binary owns the mesh-builder test only: it pins the process-wide
// colour table to one brightness, and `Renderer::new` (which re-inits the
// table at 0.8) must not run in the same process — so the backend-selection
// tests live in `gpu_backend.rs` instead.
use client::client::{Client, ClientConfig};
use client::config::Cache;
use client::core::World;
use client::dash3d::{SceneModel, TerrainOverlayShape};
use client::graphics::{Pix3D, Pix3DDraw};
use client::render::nav_debug::{NavDebugHull, NavDebugPaint};
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

    assert!(
        !vertices.is_empty(),
        "a camera-facing world must mesh geometry"
    );
    assert!(
        opaque_len > 0 && opaque_len <= vertices.len(),
        "the opaque prefix must cover the (all-opaque) mesh"
    );

    let mut found_ground = false;
    let mut found_wall = false;
    for v in vertices.iter() {
        let shade = (v.abhsl & 0xffff) as i32;
        found_ground |= shade == SHADE;
        found_wall |= shade == WALL_SHADE;
        assert!(
            v.z.is_finite() && v.x.is_finite() && v.y.is_finite(),
            "every mesh vertex is a finite camera-space position"
        );
        assert_eq!(v.uv_tex, 0, "flat faces carry no texture");
        assert_eq!(v.v, 0, "flat faces carry no v");
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

/// A vertical south-facing wall quad whose two faces are textured with
/// out-of-range texture ids (60, past the 50-texture range, and -1): the
/// GPU emitter must clamp the ids (49 and 0) and still emit the faces.
fn out_of_range_texture_wall_model() -> client::dash3d::Model {
    let mut model = client::dash3d::Model::default();
    model.num_points = 4;
    model.point_x = Some(vec![-60, 60, 60, -60]);
    model.point_y = Some(vec![0, 0, -180, -180]);
    model.point_z = Some(vec![0, 0, 0, 0]);
    model.num_faces = 2;
    model.face_vertex_a = Some(vec![0, 0]);
    model.face_vertex_b = Some(vec![1, 2]);
    model.face_vertex_c = Some(vec![2, 3]);
    // `renderType & 0x3 == 2` = textured; `>> 2` = texture-vertex index 0.
    model.face_render_type = Some(vec![2, 2]);
    model.face_colour = Some(vec![60, -1]);
    model.face_colour_a = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.face_colour_b = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.face_colour_c = Some(vec![WALL_SHADE, WALL_SHADE]);
    model.face_texture_p = Some(vec![0, 0]);
    model.face_texture_m = Some(vec![1, 1]);
    model.face_texture_n = Some(vec![2, 2]);
    model.calc_bounding_cylinder();
    model
}

/// A textured face whose texture id is outside the 50-texture range must
/// still emit vertices: the id is clamped into the valid range (RuneLite's
/// `ModelUploader` stores `faceTexture + 1` for any id — no drop), so a
/// textured wall/fence/door never vanishes on the GPU path.
#[test]
fn textured_face_with_out_of_range_tex_id_still_emits() {
    Pix3D::init_colour_table(0.6);
    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(
        &world,
        0,
        1,
        2,
        Some(SceneModel::Model(out_of_range_texture_wall_model())),
        None,
    );
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);

    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    let mut found_clamped_high = false;
    let mut found_clamped_low = false;
    let mut found_unclamped = false;
    for v in mesh.vertices() {
        let tex_plus = v.uv_tex & 0xffff;
        // tex_id 60 → clamped 49 → packs 50; tex_id -1 → clamped 0 → packs 1.
        if tex_plus == 50 {
            found_clamped_high = true;
        }
        if tex_plus == 1 {
            found_clamped_low = true;
        }
        if tex_plus == 61 {
            found_unclamped = true;
        }
    }
    assert!(
        found_clamped_high,
        "a face with tex_id >= 50 must emit vertices with the clamped id 49 (packs 50), not drop"
    );
    assert!(
        found_clamped_low,
        "a face with tex_id < 0 must emit vertices with the clamped id 0 (packs 1), not drop"
    );
    assert!(
        !found_unclamped,
        "the raw out-of-range texture id must never be packed into the vertex"
    );
}

/// `get_table` inline (the world.rs helper is private): combine an HSL
/// texture average with a ground lightness into a colour-table index —
/// the CPU `render_quick_ground` low-mem textured-ground shade.
fn get_table(hsl: i32, lightness: i32) -> i32 {
    let inv = 127 - lightness;
    let mut l = (inv * (hsl & 0x7f)) / 160;
    if l < 2 {
        l = 2;
    } else if l > 126 {
        l = 126;
    }
    (hsl & 0xff80) + l
}

/// A textured quick ground (water, `texture != -1`) must shade from the
/// texture's average colour in low-mem, not from the raw ground lightness —
/// the GPU `emit_quick_ground` bug rendered water flat gray. `TEXTURE_AVERAGE[1]`
/// (water) is 39248; the fixture shade is `SHADE`.
#[test]
fn lowmem_textured_quick_ground_shades_with_the_texture_average() {
    Pix3D::init_colour_table(0.6);
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
                TerrainOverlayShape::DIAGONAL,
                0,
                1, // water
                2000,
                2000,
                2000,
                2000,
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
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    pix.low_mem = true;
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    let expected_shade = get_table(39248, SHADE);
    assert_ne!(
        Pix3D::colour_table()[expected_shade as usize],
        Pix3D::colour_table()[SHADE as usize],
        "the texture-average shade must differ from the raw shade"
    );

    let mut found_average = false;
    for v in mesh.vertices() {
        let shade = (v.abhsl & 0xffff) as i32;
        assert_ne!(
            shade, SHADE,
            "a low-mem textured quick ground must not use the raw ground shade"
        );
        if shade == expected_shade {
            found_average = true;
        }
    }
    assert!(
        found_average,
        "the low-mem textured quick ground must shade with the texture average"
    );
}

/// A high-mem textured quick ground (the flat water tiles) must sample the
/// atlas, not fall through to the low-mem average-colour branch: the GPU
/// `emit_quick_ground` follow-up rendered high-mem water flat gray.
#[test]
fn highmem_textured_quick_ground_samples_the_atlas() {
    Pix3D::init_colour_table(0.6);
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
                TerrainOverlayShape::DIAGONAL,
                0,
                1, // water
                2000,
                2000,
                2000,
                2000,
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
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    pix.low_mem = false;
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    let mut textured = 0usize;
    for v in mesh.vertices() {
        if v.uv_tex & 0xffff != 0 {
            textured += 1;
        }
    }
    assert!(
        textured >= 3,
        "a high-mem textured quick ground must emit textured vertices, not flat (got {textured})"
    );
}

/// A low-mem *non-quick* textured ground face (the water/lava edge tiles
/// that build a `Ground` rather than a `QuickGround`) must flat-shade with
/// the texture average, not sample the atlas — the GPU `emit_ground` bug
/// textured those edges where the CPU paints a shade of blue. Shape 2
/// (`LEFT_SEMI_DIAGONAL_SMALL`) has one primary (untextured) face and one
/// secondary (texture 1) face, so both the raw and the average shade must
/// appear, and no textured vertex may.
#[test]
fn lowmem_textured_ground_face_shades_with_the_texture_average() {
    Pix3D::init_colour_table(0.6);
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
        2,
        TerrainOverlayShape::LEFT_SEMI_DIAGONAL_SMALL,
        0,
        1, // water texture
        2000,
        2000,
        2000,
        2000,
        WALL_SHADE,
        WALL_SHADE,
        WALL_SHADE,
        WALL_SHADE,
        SHADE,
        SHADE,
        SHADE,
        SHADE,
        0,
        0,
    );

    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    pix.low_mem = true;
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    let expected_average = get_table(39248, SHADE);
    let mut found_average = false;
    let mut found_raw = false;
    for v in mesh.vertices() {
        assert_eq!(
            v.uv_tex, 0,
            "a low-mem textured ground face must stay flat (no atlas sample)"
        );
        let shade = (v.abhsl & 0xffff) as i32;
        if shade == expected_average {
            found_average = true;
        }
        if shade == WALL_SHADE {
            found_raw = true;
        }
    }
    assert!(
        found_average,
        "the low-mem textured ground face must shade with the texture average"
    );
    assert!(
        found_raw,
        "the low-mem primary ground face must keep its raw shade"
    );
}

/// The WGSL `hslToRgb` (the `build_colour_table` / `hsl_to_rgb.glsl` port)
/// mirrored in Rust so the cross-check below stays GPU-free. `shade` is the
/// raw 16-bit colour-table index; the returned channels are 0..1, i.e.
/// `colour_table()[shade] / 256.0`.
fn hsl_to_rgb_wgsl(shade: u32, brightness: f32) -> [f32; 3] {
    let hue = ((shade >> 10) & 0x3f) as f32 / 64.0 + 0.0078125;
    let sat = ((shade >> 7) & 0x7) as f32 / 8.0 + 0.0625;
    let lum = (shade & 0x7f) as f32;
    let var11 = lum / 128.0;

    let var19 = if var11 < 0.5 {
        var11 * (1.0 + sat)
    } else {
        var11 + sat - var11 * sat
    };
    let var21 = 2.0 * var11 - var19;
    let mut var23 = hue + 0.3333333333333333;
    if var23 > 1.0 {
        var23 -= 1.0;
    }
    let mut var27 = hue - 0.3333333333333333;
    if var27 < 0.0 {
        var27 += 1.0;
    }

    let channel = |phase: f32| {
        if 6.0 * phase < 1.0 {
            var21 + (var19 - var21) * 6.0 * phase
        } else if 2.0 * phase < 1.0 {
            var19
        } else if 3.0 * phase < 2.0 {
            var21 + (var19 - var21) * (0.6666666666666666 - phase) * 6.0
        } else {
            var21
        }
    };

    // `build_colour_table` truncates each channel to `(x * 256)` before
    // gamma, so mirror the double rounding or dark shades drift off.
    let r = (channel(var23) * 256.0).floor() / 256.0;
    let g = (channel(hue) * 256.0).floor() / 256.0;
    let b = (channel(var27) * 256.0).floor() / 256.0;
    [r.powf(brightness), g.powf(brightness), b.powf(brightness)]
}

/// The scene shader's `hslToRgb` must reproduce `Pix3D::colour_table()` for
/// a spread of shades (flat faces are shaded from the raw 16-bit index in
/// the shader now, not the CPU table). `colour_table` truncates `r*256`
/// before gamma and uses f64, so compare with a small per-channel tolerance.
#[test]
fn hsl_to_rgb_matches_colour_table() {
    Pix3D::init_colour_table(0.6);
    let brightness = Pix3D::colour_brightness() as f32;
    let shades = [
        0u32,
        1,
        127,
        128,
        200 * 128 + 100,
        40 * 128 + 80,
        0x6464,
        0xffff,
    ];
    for shade in shades {
        let table = Pix3D::colour_table()[shade as usize];
        let got = hsl_to_rgb_wgsl(shade, brightness);
        let expected = [
            ((table >> 16) & 0xff) as f32,
            ((table >> 8) & 0xff) as f32,
            (table & 0xff) as f32,
        ];
        for (got_ch, exp_ch) in got.iter().zip(expected.iter()) {
            let delta = (got_ch * 256.0 - exp_ch).abs();
            assert!(
                delta <= 2.0,
                "shade {shade}: hslToRgb channel {got_ch} (x256 {}) != colour_table {exp_ch}",
                got_ch * 256.0
            );
        }
    }
}

/// A 128-wide south-facing door. The XZ cylinder is ~64, so the worldRender
/// AABB inflates on the near-z side when the loc sits off-centre (the
/// grazing-angle walk-by case). Locs keep `use_aabb_mouse_check = false`:
/// RuneLite's GPU plugin never replaces clickboxes, and the CPU path only
/// AABB-pretests then requires the projected face to contain the mouse.
fn angled_door_model() -> client::dash3d::Model {
    let mut model = client::dash3d::Model::default();
    model.num_points = 4;
    model.point_x = Some(vec![-64, 64, 64, -64]);
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

/// Packed loc typecode: entity 2, loc id 1530, scene tile (1, 2).
const DOOR_TYPECODE: i32 = (2 << 29) | (1530 << 14) | (2 << 7) | 1;

/// Off-centre identity-camera placement that inflates the loc AABB to the
/// right of the projected faces. Mouse at this screen point is inside the
/// AABB and outside the face bbox — CPU locs miss, AABB-only hits.
const DOOR_REL_X: i32 = 200;
const DOOR_REL_Z: i32 = 300;
const DOOR_BESIDE_MOUSE_X: i32 = 760;
const DOOR_BESIDE_MOUSE_Y: i32 = 160;

/// GPU loc picking must match the CPU loc path (AABB is only a pre-test;
/// the face has to contain the mouse). Clicking the ground beside a door
/// must not append the loc — that is what left-clicks Open instead of
/// Walk here.
#[test]
fn gpu_loc_pick_is_per_face_not_aabb() {
    Pix3D::init_colour_table(0.6);

    let door = angled_door_model();
    let mut pix = Pix3DDraw::default();
    let mut map = client::graphics::PixMap::new(512, 334);
    {
        let mut surface =
            client::graphics::Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        pix.set_render_clipping(&surface);
        pix.mouse_check = true;
        pix.mouse_x = DOOR_BESIDE_MOUSE_X;
        pix.mouse_y = DOOR_BESIDE_MOUSE_Y;
        door.world_render(
            &mut pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            DOOR_REL_X,
            0,
            DOOR_REL_Z,
            DOOR_TYPECODE,
        );
    }
    assert_eq!(
        pix.picked_count, 0,
        "CPU loc pick is per-face: mouse beside the door must miss"
    );

    let mut aabb_door = angled_door_model();
    aabb_door.use_aabb_mouse_check = true;
    pix.picked_count = 0;
    {
        let mut surface =
            client::graphics::Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        pix.set_render_clipping(&surface);
        pix.mouse_check = true;
        pix.mouse_x = DOOR_BESIDE_MOUSE_X;
        pix.mouse_y = DOOR_BESIDE_MOUSE_Y;
        aabb_door.world_render(
            &mut pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            DOOR_REL_X,
            0,
            DOOR_REL_Z,
            DOOR_TYPECODE,
        );
    }
    assert!(
        pix.picked_count >= 1,
        "AABB-only would swallow a click beside the door (the GPU bug)"
    );

    // GPU path: vis-test camera that meshes a wall at tile (1,2). Relative
    // to that camera the door sits at (0, 50, 128), pitch 128. Viewport
    // centre is inside the inflated AABB and outside the projected faces
    // (CPU loc pick misses; GPU AABB currently hits).
    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, DOOR_TYPECODE, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(
        &world,
        0,
        1,
        2,
        Some(SceneModel::Model(angled_door_model())),
        None,
    );
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);

    pix.picked_count = 0;
    {
        let mut surface =
            client::graphics::Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        pix.set_render_clipping(&surface);
        pix.mouse_check = true;
        pix.mouse_x = 256;
        pix.mouse_y = 160;
        door.world_render(
            &mut pix,
            &mut surface,
            0,
            Pix3D::sin_table()[128],
            Pix3D::cos_table()[128],
            0,
            65536,
            0,
            50,
            128,
            DOOR_TYPECODE,
        );
    }
    assert_eq!(
        pix.picked_count, 0,
        "CPU loc pick misses at the vis-camera viewport centre"
    );

    let mut gpu_pix = Pix3DDraw::default();
    gpu_pix.set_clipping(512, 334);
    gpu_pix.mouse_check = true;
    gpu_pix.mouse_x = 256;
    gpu_pix.mouse_y = 160;
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut gpu_pix);
    assert!(
        mesh.vertices()
            .iter()
            .any(|v| (v.abhsl & 0xffff) as i32 == WALL_SHADE),
        "the door loc must actually be in the GPU mesh or the pick miss is a vis false-pass"
    );
    assert_eq!(
        gpu_pix.picked_count, 0,
        "GPU loc pick must be per-face like the CPU; AABB-only opens doors on walk-by clicks"
    );
}

/// The nav-debug paint must never enable AABB loc picking: with a hull
/// paint stored for the door, the CPU per-face pick and the GPU mesh pick
/// still miss beside the door. The loc's `use_aabb_mouse_check` stays
/// false — AABB-only loc picks open doors on walk-by clicks.
#[test]
fn nav_debug_paint_does_not_enable_aabb_loc_pick() {
    Pix3D::init_colour_table(0.6);

    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.set_nav_debug_paint(Some(NavDebugPaint {
        hulls: vec![NavDebugHull {
            loc_id: 1530,
            scene_x: 1,
            scene_z: 2,
        }],
        show_hulls: true,
        ..Default::default()
    }));

    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, DOOR_TYPECODE, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(
        &world,
        0,
        1,
        2,
        Some(SceneModel::Model(angled_door_model())),
        None,
    );
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);

    // The hull paint's model resolution (the draw's hull path) must not
    // set the AABB pick flag on the loc.
    let (x, y, z, _yaw, model) = rw
        .loc_model_at(&world, &Cache::default(), 0, 0, 1, 2, 1530)
        .expect("the door hull must resolve to the live loc model");
    assert!(
        !model.use_aabb_mouse_check,
        "nav-debug hull paint must never set use_aabb_mouse_check on a loc"
    );
    assert!(model.num_points >= 4, "the hull AABB needs the model points");
    assert!(x >= 0 && y > 0 && z >= 0, "the hull must carry a scene position");

    // Mouse beside the door still must miss.
    let door = angled_door_model();
    let mut pix = Pix3DDraw::default();
    let mut map = client::graphics::PixMap::new(512, 334);
    {
        let mut surface =
            client::graphics::Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        pix.set_render_clipping(&surface);
        pix.mouse_check = true;
        pix.mouse_x = DOOR_BESIDE_MOUSE_X;
        pix.mouse_y = DOOR_BESIDE_MOUSE_Y;
        door.world_render(
            &mut pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            DOOR_REL_X,
            0,
            DOOR_REL_Z,
            DOOR_TYPECODE,
        );
    }
    assert_eq!(
        pix.picked_count, 0,
        "CPU loc pick is per-face even with a hull paint stored"
    );

    let mut gpu_pix = Pix3DDraw::default();
    gpu_pix.set_clipping(512, 334);
    gpu_pix.mouse_check = true;
    gpu_pix.mouse_x = 256;
    gpu_pix.mouse_y = 160;
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut gpu_pix);
    assert!(
        mesh.vertices()
            .iter()
            .any(|v| (v.abhsl & 0xffff) as i32 == WALL_SHADE),
        "the door loc must actually be in the GPU mesh or the pick miss is a vis false-pass"
    );
    assert_eq!(
        gpu_pix.picked_count, 0,
        "GPU loc pick must stay per-face with a hull paint stored"
    );
}

/// `set_nav_debug_paint` round-trips on the client: the paint stores and
/// clears (the host publishes `None` by default so the tree builds).
#[test]
fn set_nav_debug_paint_roundtrips_on_client() {
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    assert!(
        c.nav_debug_paint().is_none(),
        "a fresh client has no nav debug paint"
    );
    c.set_nav_debug_paint(Some(NavDebugPaint {
        show_path: true,
        ..Default::default()
    }));
    assert!(c.nav_debug_paint().is_some_and(|p| p.show_path));
    c.set_nav_debug_paint(None);
    assert!(c.nav_debug_paint().is_none());
}
