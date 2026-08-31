// Task 5b (GPU-chrome campaign): the model-texture sampling (white-buildings
// fix). Textured faces carry a per-face texture index + projective UV and
// sample the shared model `texture_2d_array` (one 128×128 layer per id) in
// the scene shader — they are no longer flat-shaded. Two checks: (1) the
// scene mesh carries the texture id and a finite, non-degenerate UV for a
// multi-texture model (pure CPU math, like gpu_mesh.rs); (2) a real
// `GpuBackend` renders that mesh and the read-back scene contains the two
// textures' colours (red + blue), so each texture samples non-white texels.
// The GPU check needs an adapter and skips without one. This binary owns the
// textured-mesh colour-table brightness, so it pins
// `Pix3D::init_colour_table(0.6)` itself.
use client::config::Cache;
use client::core::World;
use client::dash3d::{SceneModel, TerrainOverlayShape};
use client::graphics::{Pix3D, Pix3DDraw, Pix8};
use client::render::backend::{FrameOutput, GpuBackend};
use client::render::world::GpuVertex;
use client::render::{RenderWorld, Renderer};

const SHADE: i32 = 200 * 128 + 100;
/// The raw 16-bit shade carried on *textured* vertices. The CPU clamps the
/// texel brightness to 0..127 (`Model.getColour`'s `127 - scalar`), so 0
/// is full brightness — the block-0, no-halving bucket the GPU shader must
/// honour.
const TEX_SHADE: i32 = 0;
const TEXTURE_RED: i32 = 7;
const TEXTURE_BLUE: i32 = 12;
/// Distinct from [`TEXTURE_RED`]: `GpuAssets::ensure_model_textures` uploads
/// each id once per process. Sibling tests bake id 7 as solid red first, so
/// a quadrant texture on 7 never reaches the GPU and this test only sees red.
const TEXTURE_QUAD: i32 = 31;

/// 3×3 flat world at height 2000 with a plain-coloured tile on every cell
/// (same fixture as gpu_mesh.rs).
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

/// A vertical south-facing wall quad whose two faces use *different*
/// model textures (ids 7 and 12) — the multi-texture case. Each face maps
/// the quad through texture-mapping vertices 0/1/2.
fn textured_wall_model() -> client::dash3d::Model {
    let mut model = client::dash3d::Model {
        num_points: 4,
        point_x: Some(vec![-60, 60, 60, -60]),
        point_y: Some(vec![0, 0, -180, -180]),
        point_z: Some(vec![0, 0, 0, 0]),
        num_faces: 2,
        face_vertex_a: Some(vec![0, 0]),
        face_vertex_b: Some(vec![1, 2]),
        face_vertex_c: Some(vec![2, 3]),
        ..Default::default()
    };
    // `renderType & 0x3 == 2` = textured; `>> 2` = texture-vertex index 0.
    model.face_render_type = Some(vec![2, 2]);
    model.face_colour = Some(vec![TEXTURE_RED, TEXTURE_BLUE]);
    model.face_colour_a = Some(vec![TEX_SHADE, TEX_SHADE]);
    model.face_colour_b = Some(vec![TEX_SHADE, TEX_SHADE]);
    model.face_colour_c = Some(vec![TEX_SHADE, TEX_SHADE]);
    model.face_texture_p = Some(vec![0, 0]);
    model.face_texture_m = Some(vec![1, 1]);
    model.face_texture_n = Some(vec![2, 2]);
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

/// A 64×64 solid-colour texture: every texel is palette index 1.
fn solid_texture(rgb: i32) -> Pix8 {
    let mut tex = Pix8::new(64, 64, vec![0, rgb]);
    for p in tex.data.iter_mut() {
        *p = 1;
    }
    tex
}

/// A 64×64 texture with a distinct colour in each 32×32 quadrant (red /
/// green / blue / yellow) — what a low-mem renderer's halved texture looks
/// like. A render proves *which* region of the texture a face samples: a
/// scale-64 bug samples only the top-left quarter (all red).
fn quadrant_texture() -> Pix8 {
    let mut tex = Pix8::new(64, 64, vec![0, 0xff0000, 0x00ff00, 0x0000ff, 0xffff00]);
    for y in 0..64 {
        for x in 0..64 {
            let idx = match (x < 32, y < 32) {
                (true, true) => 1,   // top-left red
                (false, true) => 2,  // top-right green
                (true, false) => 3,  // bottom-left blue
                (false, false) => 4, // bottom-right yellow
            };
            tex.data[(y * 64 + x) as usize] = idx as i8;
        }
    }
    tex
}

/// A `Pix3DDraw` with solid red/blue textures depacked at ids 7 and 12
/// (the `tex_pal` is the baked palette here; gamma is not involved).
fn textured_pix() -> Pix3DDraw {
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    pix.textures[TEXTURE_RED as usize] = Some(solid_texture(0xff0000));
    pix.tex_pal[TEXTURE_RED as usize] = Some(vec![0, 0xff0000]);
    pix.textures[TEXTURE_BLUE as usize] = Some(solid_texture(0x0000ff));
    pix.tex_pal[TEXTURE_BLUE as usize] = Some(vec![0, 0x0000ff]);
    pix
}

/// The mesh for a world with the textured wall placed on tile (1, 2)
/// (the same placement as gpu_mesh.rs).
fn textured_wall_mesh(
    pix: &mut Pix3DDraw,
) -> (RenderWorld, World, client::render::world::SceneMesh) {
    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(
        &world,
        0,
        1,
        2,
        Some(SceneModel::Model(textured_wall_model())),
        None,
    );
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, pix);
    (rw, world, mesh)
}

/// The scene mesh for a multi-texture model must carry the per-face
/// texture index and a finite, non-degenerate projective UV — the input
/// the scene shader samples. Runs without any GPU.
#[test]
fn textured_model_mesh_carries_tex_id_and_uv() {
    Pix3D::init_colour_table(0.6);
    let mut pix = textured_pix();
    let (_rw, _world, mesh) = textured_wall_mesh(&mut pix);
    let vertices = mesh.vertices();

    let mut found_red = false;
    let mut found_blue = false;
    let mut saw_nonzero_u = false;
    let mut saw_nonzero_v = false;
    for v in vertices.iter() {
        let tex_plus = v.uv_tex & 0xffff;
        if tex_plus == TEXTURE_RED as u32 + 1 {
            found_red = true;
        }
        if tex_plus == TEXTURE_BLUE as u32 + 1 {
            found_blue = true;
        }
        if tex_plus != 0 {
            // A textured vertex packs texture id + 1 in the low 16 bits and
            // the fixed-point u in the high 16 bits, plus the raw shade.
            assert_eq!(
                (v.abhsl & 0xffff) as i32,
                TEX_SHADE,
                "a textured vertex carries the raw shade"
            );
            saw_nonzero_u |= (v.uv_tex >> 16) != 0;
            saw_nonzero_v |= v.v != 0;
        } else {
            assert_eq!(v.uv_tex, 0, "untextured vertices carry no texture");
            assert_eq!(v.v, 0, "untextured vertices carry no v");
        }
    }
    assert!(found_red, "the red-textured face must be in the mesh");
    assert!(found_blue, "the blue-textured face must be in the mesh");
    assert!(
        saw_nonzero_u,
        "a textured face must pack a nonzero u (high 16 bits)"
    );
    assert!(saw_nonzero_v, "a textured face must pack a nonzero v");

    // The texture array has one 128×128 layer per texture id, so ids 7
    // and 12 sample different layers (no shared-atlas cell derivation).
    assert!(
        (0..50).contains(&TEXTURE_RED),
        "the fixture texture id is in the valid range"
    );
    assert!(
        (0..50).contains(&TEXTURE_BLUE),
        "the fixture texture id is in the valid range"
    );
    assert_ne!(
        TEXTURE_RED, TEXTURE_BLUE,
        "the two textures must sample different array layers"
    );
}

/// A textured face with `renderType & 0x3 == 3` (textured, single flat
/// shade) must still carry the texture id and UV — the CPU path treats
/// type 3 as textured (`render_triangle`'s else branch). Before the fix the
/// GPU emitter routed it through the flat branch, so books/shelves with
/// flat-textured faces rendered as white void.
#[test]
fn textured_type3_face_carries_tex_id_and_uv() {
    Pix3D::init_colour_table(0.6);
    let mut pix = textured_pix();
    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    let mut model = textured_wall_model();
    // Both faces textured with a flat shade (renderType = 3 | 0<<2).
    model.face_render_type = Some(vec![3, 3]);
    rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    let mut textured = 0usize;
    for v in mesh.vertices() {
        if v.uv_tex & 0xffff != 0 {
            textured += 1;
            assert_eq!(
                (v.abhsl & 0xffff) as i32,
                TEX_SHADE,
                "a type-3 textured vertex carries the flat shade"
            );
        }
    }
    assert!(
        textured >= 3,
        "type-3 textured faces must emit textured vertices, not flat (got {textured})"
    );
}

/// A flat wall face whose near vertices fall behind the near plane (z < 50)
/// must be clipped and still emit vertices, not dropped to the scene's black
/// clear colour. Live: walking up to a wall puts its upper edge past the
/// camera plane while the base is still in front.
#[test]
fn wall_face_crossing_the_near_plane_still_emits() {
    Pix3D::init_colour_table(0.6);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);

    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    let mut model = client::dash3d::Model {
        num_points: 4,
        point_x: Some(vec![-60, 60, 60, -60]),
        point_y: Some(vec![0, 0, -180, -180]),
        point_z: Some(vec![0, 0, 0, 0]),
        num_faces: 2,
        face_vertex_a: Some(vec![0, 0]),
        face_vertex_b: Some(vec![1, 2]),
        face_vertex_c: Some(vec![2, 3]),
        face_colour_a: Some(vec![SHADE, SHADE]),
        face_colour_b: Some(vec![SHADE, SHADE]),
        face_colour_c: Some(vec![SHADE, SHADE]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();
    rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    // Camera 40 units in front of the wall (z=320): the base is in front
    // of the near plane, the top vertices behind it.
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 280, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    let mut wall_verts = 0usize;
    for v in mesh.vertices() {
        if (v.abhsl & 0xffff) as i32 == SHADE {
            wall_verts += 1;
            assert!(
                v.z >= 50.0,
                "clipped wall vertices must stay on the near plane or in front"
            );
        }
    }
    assert!(
        wall_verts >= 3,
        "a wall face crossing the near plane must emit clipped vertices, not vanish (got {wall_verts})"
    );
}

/// A gouraud (untextured) face must shade to the CPU colour-table RGB, not
/// black. Live: the wall beam / fence post / wooden post are gouraud faces
/// that the GPU renders black while the CPU shades them correctly.
#[test]
fn gpu_gouraud_face_shades_like_the_colour_table() {
    Pix3D::init_colour_table(0.6);
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the gouraud shade test skips");
        return;
    };
    // A one-face gouraud wall with a known 16-bit shade.
    let mut model = client::dash3d::Model {
        num_points: 4,
        point_x: Some(vec![-60, 60, 60, -60]),
        point_y: Some(vec![0, 0, -180, -180]),
        point_z: Some(vec![0, 0, 0, 0]),
        num_faces: 2,
        face_vertex_a: Some(vec![0, 0]),
        face_vertex_b: Some(vec![1, 2]),
        face_vertex_c: Some(vec![2, 3]),
        face_colour_a: Some(vec![SHADE, SHADE]),
        face_colour_b: Some(vec![SHADE, SHADE]),
        face_colour_c: Some(vec![SHADE, SHADE]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();

    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);
    let scene = backend.render_scene_for_test(mesh, &pix);

    let expected = Pix3D::colour_table()[SHADE as usize];
    let expected = (
        (expected >> 16) & 0xff,
        (expected >> 8) & 0xff,
        expected & 0xff,
    );
    let mut matched = 0usize;
    for &rgb in &scene {
        let r = (rgb >> 16) & 0xff;
        let g = (rgb >> 8) & 0xff;
        let b = rgb & 0xff;
        if (r - expected.0).abs() <= 2 && (g - expected.1).abs() <= 2 && (b - expected.2).abs() <= 2
        {
            matched += 1;
        }
    }
    assert!(
        matched > 500,
        "gouraud wall must shade to the colour-table RGB (expected {:?}, matched {matched})",
        expected
    );
}

/// End-to-end: a real `GpuBackend` renders the textured-wall mesh and the
/// read-back scene contains the two textures' colours — red *and* blue
/// texels, so textured faces sample the texture array by layer instead of
/// flat-shading white. Skips on machines without an adapter.
#[test]
fn gpu_render_samples_multi_texture_model() {
    Pix3D::init_colour_table(0.6);
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the GPU texture test skips");
        return;
    };
    let mut pix = textured_pix();
    let (_rw, _world, mesh) = textured_wall_mesh(&mut pix);
    let scene = backend.render_scene_for_test(mesh, &pix);

    // The textured shade (TEX_SHADE = 0) is the full-brightness bucket, so
    // the rendered red/blue are the full palette colours, never white.
    let mut found_red = 0usize;
    let mut found_blue = 0usize;
    for &rgb in &scene {
        let r = (rgb >> 16) & 0xff;
        let g = (rgb >> 8) & 0xff;
        let b = rgb & 0xff;
        if r > 128 && g < 64 && b < 64 {
            found_red += 1;
        }
        if b > 128 && r < 64 && g < 64 {
            found_blue += 1;
        }
    }
    assert!(
        found_red > 0,
        "the red-textured wall face must render red texels, not flat-shaded white"
    );
    assert!(
        found_blue > 0,
        "the blue-textured wall face must render blue texels, not flat-shaded white"
    );
    // Sanity: the wall occupies real screen space (not a one-pixel sliver).
    assert!(
        found_red + found_blue > 500,
        "the textured wall must cover screen area"
    );
}

/// The GPU textured path must apply the CPU's 8-level per-texel brightness:
/// bits 4-5 of the 0..127 shade pick one of the four pre-baked texel blocks
/// (1, 7/8, 3/4, 5/8) and bit 6 halves shades >= 64 (`Pix3D.textureRaster`).
/// Before the fix the shader read bits 14-15 of the full 16-bit word — always
/// 0 for a 0..127 texel shade — so indoor/textured faces rendered at full
/// brightness instead of their lit shade.
#[test]
fn gpu_textured_shade_scales_texel_brightness() {
    Pix3D::init_colour_table(0.6);
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the texel-brightness test skips");
        return;
    };
    // (shade, expected red channel): block 0..3 times the >=64 halving.
    let cases: [(i32, i32); 8] = [
        (0, 0xff),
        (16, 0xdf),
        (32, 0xbf),
        (48, 0x9f),
        (64, 0x7f),
        (80, 0x6f),
        (96, 0x5f),
        (112, 0x4f),
    ];

    for (shade, expected) in cases {
        let mut pix = textured_pix();
        let mut world = flat_world();
        world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
        let mut rw = RenderWorld::new();
        let mut model = textured_wall_model();
        // Both faces red, the requested 0..127 texel brightness.
        model.face_colour = Some(vec![TEXTURE_RED, TEXTURE_RED]);
        model.face_colour_a = Some(vec![shade, shade]);
        model.face_colour_b = Some(vec![shade, shade]);
        model.face_colour_c = Some(vec![shade, shade]);
        rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
        rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
        rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
        let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);
        let scene = backend.render_scene_for_test(mesh, &pix);

        // The red-dominant pixels are the textured wall (the ground shades
        // gray); their red channel must be the palette red scaled by the
        // block/halving factor.
        let mut max_red = 0i32;
        for &rgb in &scene {
            let r = (rgb >> 16) & 0xff;
            let g = (rgb >> 8) & 0xff;
            let b = rgb & 0xff;
            if r > g + 40 && r > b + 40 {
                max_red = max_red.max(r);
            }
        }
        assert!(
            (max_red - expected).abs() <= 8,
            "shade {shade} must scale the red texel to ~{expected}, got {max_red}"
        );
    }
}

/// A face whose texture id is past the array's depth (>= 50) still renders:
/// the emitter clamps the id to 49 and the shader clamps the layer to the
/// array depth, so the layer-49 texels appear — the walls/fences/doors
/// never-vanish fix. Skips on machines without an adapter.
#[test]
fn gpu_render_clamps_out_of_range_tex_id() {
    Pix3D::init_colour_table(0.6);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    pix.textures[49] = Some(solid_texture(0x00ff00));
    pix.tex_pal[49] = Some(vec![0, 0x00ff00]);

    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    let mut model = textured_wall_model();
    model.face_colour = Some(vec![60, 60]); // both faces past the 50-texture range
    rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the clamped-tex-id render check skips");
        return;
    };
    let scene = backend.render_scene_for_test(mesh, &pix);

    let mut found_green = 0usize;
    for &rgb in &scene {
        let r = (rgb >> 16) & 0xff;
        let g = (rgb >> 8) & 0xff;
        let b = rgb & 0xff;
        if g > 150 && r < 80 && b < 80 {
            found_green += 1;
        }
    }
    assert!(
        found_green > 500,
        "an out-of-range-texture-id wall face must render the clamped layer-49 texels, not drop (got {found_green} green px)"
    );
}

/// The low-mem path (the default bot config): a halved 64×64 texture and
/// `pix.low_mem` set. The array bakes every layer at 128×128, so the UV
/// numerator scale must be 128 regardless of the memory mode — a scale-64
/// mesh would sample only the top-left quarter of the layer, stretched 2×.
/// The quadrant texture makes that visible: the full 64×64 texture (all
/// four quadrants) must appear on the face.
#[test]
fn gpu_lowmem_texture_samples_the_full_128px_layer() {
    Pix3D::init_colour_table(0.6);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    pix.low_mem = true;
    let tex = quadrant_texture();
    pix.textures[TEXTURE_QUAD as usize] = Some(tex.clone());
    pix.tex_pal[TEXTURE_QUAD as usize] = Some(vec![0, 0xff0000, 0x00ff00, 0x0000ff, 0xffff00]);

    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    let mut model = textured_wall_model();
    model.face_colour = Some(vec![TEXTURE_QUAD, TEXTURE_QUAD]);
    rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    // Mesh-level: the textured vertices' fixed-point UV must span the full
    // 128px layer in both axes even on the low-mem path (a scale-64 bug caps
    // at 64, i.e. u/v ≤ 128 here).
    let mut max_u = 0u32;
    let mut max_v = 0u32;
    for v in mesh
        .clone()
        .vertices()
        .iter()
        .filter(|v| (v.uv_tex & 0xffff) == (TEXTURE_QUAD as u32 + 1))
    {
        max_u = max_u.max(v.uv_tex >> 16);
        max_v = max_v.max(v.v);
    }
    assert!(
        max_u > 220 && max_v > 220,
        "the low-mem mesh UV must span the full 128px layer (got u≤{max_u}, v≤{max_v})"
    );

    // GPU: the render must sample the texture across a quadrant boundary —
    // a scale-64 sample shows only the top-left quarter (all red). The
    // default grazing camera shows the wall's near band, so at least two
    // quadrant colours must appear.
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the low-mem render check skips");
        return;
    };
    let scene = backend.render_scene_for_test(mesh, &pix);
    let mut seen = [false; 4];
    for &rgb in &scene {
        let r = (rgb >> 16) & 0xff;
        let g = (rgb >> 8) & 0xff;
        let b = rgb & 0xff;
        if r > 100 && g < 60 && b < 60 {
            seen[0] = true; // red (top-left quadrant)
        } else if r < 60 && g > 100 && b < 60 {
            seen[1] = true; // green (top-right)
        } else if r < 60 && g < 60 && b > 100 {
            seen[2] = true; // blue (bottom-left)
        } else if r > 100 && g > 100 && b < 60 {
            seen[3] = true; // yellow (bottom-right)
        }
    }
    assert!(
        seen.iter().filter(|&&s| s).count() >= 2,
        "the low-mem textured face must sample more than the top-left quarter of the 128px layer (seen {seen:?})"
    );
}

/// The composite: a real in-game frame through the wgpu backend returns
/// one full-frame 765×503 texture carrying the scene at its (4, 4) point —
/// no readback. The overlay-coverage buffer drives the scene-window
/// transparency: a pixel the overlay pass wrote is opaque *regardless of
/// colour* (the minimenu's black title bar and border stay black over the
/// scene), and an uncovered scene-window pixel shows the scene.
#[test]
fn composite_lands_the_scene_in_the_full_frame() {
    Pix3D::init_colour_table(0.6);
    let Ok(backend) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the composite test skips");
        return;
    };
    // A fresh empty cache dir: `prepare_game` must not overwrite the
    // fixture textures with a real `textures` jag, and no fonts/sprites
    // load (the minimenu rects need neither — the menu stays where the
    // test puts it).
    let cache_dir = std::env::temp_dir().join(format!("r274-gpu-composite-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&cache_dir);
    // Drive the real frame stages through the wgpu backend: a renderer
    // holding it, a client with the textured-wall fixture world + camera,
    // and an open scene-view minimenu (the overlay the coverage pass
    // records).
    let mut r = Renderer::with_backend(Box::new(backend), false);
    Pix3D::init_colour_table(0.6); // Renderer::new re-inits at 0.8; re-pin
    let mut c = client(&cache_dir);
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    c.cam_x = 192;
    c.cam_y = 1950;
    c.cam_z = 192;
    c.cam_pitch = 128;
    // The open scene-view minimenu: `draw_minimenu` fills the brown box,
    // a BLACK title bar and a BLACK border into `area_game` (area 0).
    c.is_menu_open = true;
    c.menu_area = 0;
    c.menu_x = 50;
    c.menu_y = 50;
    c.menu_width = 100;
    c.menu_height = 120;
    c.world = flat_world();
    c.world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    r.world.set_wall_model(
        &c.world,
        0,
        1,
        2,
        Some(SceneModel::Model(textured_wall_model())),
        None,
    );
    r.pix3d.textures[TEXTURE_RED as usize] = Some(solid_texture(0xff0000));
    r.pix3d.tex_pal[TEXTURE_RED as usize] = Some(vec![0, 0xff0000]);
    r.pix3d.textures[TEXTURE_BLUE as usize] = Some(solid_texture(0x0000ff));
    r.pix3d.tex_pal[TEXTURE_BLUE as usize] = Some(vec![0, 0x0000ff]);

    let FrameOutput::Texture(handle) = r.game_draw(&mut c) else {
        panic!("finish must return the full-frame texture");
    };
    assert_eq!((handle.width, handle.height), (765, 503));
    let pixels = handle.read_back();
    // Nothing outside the frame's own bounds.
    assert_eq!(pixels.len(), 765 * 503);

    // The textured wall renders inside the (4, 4) scene region (not the
    // raw 512×334 scene texture — the full frame): the uncovered scene
    // window shows the scene.
    let mut scene_red = 0usize;
    for y in 4..338 {
        for x in 4..516 {
            let rgb = pixels[y * 765 + x];
            let r = (rgb >> 16) & 0xff;
            let g = (rgb >> 8) & 0xff;
            let b = rgb & 0xff;
            if r > 128 && g < 64 && b < 64 {
                scene_red += 1;
            }
        }
    }
    assert!(
        scene_red > 500,
        "the composited full-frame must carry the scene at (4, 4) (got {scene_red} red px)"
    );

    // The minimenu overlay is opaque *regardless of colour*: the brown
    // box, and the BLACK title bar + border — covered pixels stay opaque
    // over the scene (the coverage fix; a colour-sentinel key would punch
    // these black pixels through to the scene).
    // The menu is at area_game (50, 50, 100, 120), blitted at (4, 4):
    // frame box [54,154)×[54,174), title bar [55,153)×[55,71), border
    // cols 55/152 rows 72/172.
    for (fx, fy, expected) in [
        (104, 104, 0x5d5447), // brown box interior
        (104, 59, 0x000000),  // black title bar, covered -> opaque black
        (55, 104, 0x000000),  // black border, covered -> opaque black
    ] {
        assert_eq!(
            pixels[fy * 765 + fx],
            expected,
            "a covered minimenu pixel at ({fx}, {fy}) must be opaque over the scene"
        );
    }
}

/// A client with an empty cache (the GPU fixture frames need only the
/// world/camera; no media sprites/fonts/textures load, so the menu
/// position and the fixture textures stay deterministic).
fn client(cache_dir: &std::path::Path) -> client::client::Client {
    client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache_dir.to_string_lossy().into_owned(),
        members: true,
        lowmem: false,
    })
}

/// The `GpuVertex` layout stays `bytemuck`-clean for the raw upload (the
/// extra UV/shade/tex-id fields must not break the Pod contract).
#[test]
fn gpu_vertex_stays_pod() {
    fn assert_pod<T: bytemuck::Pod + bytemuck::Zeroable>() {}
    assert_pod::<GpuVertex>();
}
