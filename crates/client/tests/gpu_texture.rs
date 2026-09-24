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
const TEXTURE_RIPPLED_UNDERLAY: i32 = 1;
const TEXTURE_ANIMATED: i32 = 17;
/// Distinct from [`TEXTURE_RED`]: `GpuAssets::ensure_model_textures` uploads
/// each id once per process. Sibling tests bake id 7 as solid red first, so
/// a quadrant texture on 7 never reaches the GPU and this test only sees red.
const TEXTURE_QUAD: i32 = 31;
// Unused static layers for the high/low-memory controls in the water-filter
// regression. Static atlas layers upload once per process, so each mode owns
// a distinct id while animated layer 17 is refreshed before every submit.
const TEXTURE_FILTER_CONTROL_HIGH: i32 = 43;
const TEXTURE_FILTER_CONTROL_LOW: i32 = 44;
const TEXTURE_ADDRESS_V: i32 = 45;
const TEXTURE_ADDRESS_U: i32 = 46;
const TEXTURE_ADDRESS_SOLID: i32 = 47;

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

/// A red/transparent checker. Its mip levels average RGB with transparent
/// zero neighbours, while LOD0 retains pure red at every covered sample.
fn sparse_filter_texture(size: i32) -> Pix8 {
    let mut tex = Pix8::new(size, size, vec![0, 0xff0000]);
    for y in 0..size {
        for x in 0..size {
            if (x + y) & 1 == 0 {
                tex.data[(y * size + x) as usize] = 1;
            }
        }
    }
    tex
}

fn address_texture(size: i32, vertical_wrap_probe: bool) -> Pix8 {
    let mut tex = Pix8::new(size, size, vec![0, 0xff0000, 0x0000ff, 0x00ff00, 0xffffff]);
    for y in 0..size {
        for x in 0..size {
            let index = if vertical_wrap_probe {
                if y >= size * 7 / 8 {
                    2 // blue clamp edge: the broken V-clamp result
                } else if x < size / 2 {
                    0 // transparent cutout inside every wrapped row
                } else {
                    1 // red wrapped sample
                }
            } else if x >= size * 3 / 4 {
                3 // green U-clamp edge
            } else {
                1 // red would leak if U repeated
            };
            tex.data[(y * size + x) as usize] = index;
        }
    }
    tex
}

fn address_probe_model(texture: i32, vertical_wrap_probe: bool) -> client::dash3d::Model {
    let mut points_x = vec![-60, 60, 60, -60];
    let mut points_y = vec![0, 0, -180, -180];
    let mut points_z = vec![0; 4];
    if vertical_wrap_probe {
        // Face U spans 0..1. V spans -1.75..-1.25, so Java's V mask
        // samples the texture at 0.25..0.75 while a clamp samples its edge.
        points_x.extend([-60, 60, -60]);
        points_y.extend([450, 450, 810]);
    } else {
        // Face U spans 1.25..1.75 and V spans 0.25..0.75. U must retain
        // Java's clamp rather than adopting V's wrapping contract.
        points_x.extend([-360, -120, -360]);
        points_y.extend([90, 90, -270]);
    }
    points_z.extend([0; 3]);
    let mut model = client::dash3d::Model {
        num_points: 7,
        point_x: Some(points_x),
        point_y: Some(points_y),
        point_z: Some(points_z),
        num_faces: 2,
        face_vertex_a: Some(vec![0, 0]),
        face_vertex_b: Some(vec![1, 2]),
        face_vertex_c: Some(vec![2, 3]),
        face_render_type: Some(vec![2, 2]),
        face_colour: Some(vec![texture, texture]),
        face_colour_a: Some(vec![TEX_SHADE, TEX_SHADE]),
        face_colour_b: Some(vec![TEX_SHADE, TEX_SHADE]),
        face_colour_c: Some(vec![TEX_SHADE, TEX_SHADE]),
        face_texture_p: Some(vec![4]),
        face_texture_m: Some(vec![5]),
        face_texture_n: Some(vec![6]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();
    model
}

fn address_probe_mesh(
    pix: &mut Pix3DDraw,
    texture: Option<i32>,
    vertical_wrap_probe: bool,
) -> client::render::world::SceneMesh {
    probe_mesh(
        pix,
        texture.map(|texture| address_probe_model(texture, vertical_wrap_probe)),
    )
}

/// Mesh one wall model (or nothing, for the background) in front of the
/// fixed probe camera.
fn probe_mesh(
    pix: &mut Pix3DDraw,
    model: Option<client::dash3d::Model>,
) -> client::render::world::SceneMesh {
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    if let Some(model) = model {
        world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
        rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
    }
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    rw.build_scene_mesh(&mut world, &Cache::default(), 0, pix)
}

/// The wall quad with a texture basis whose U runs -0.25..0.75 across the
/// quad and V runs from about 0.14 at the base, through 0 at 25 units up,
/// to -0.86 at the top: every face crosses zero in both coordinates, as
/// many real 289 loc faces do by a few texels.
fn zero_crossing_probe_model(texture: i32) -> client::dash3d::Model {
    let mut model = address_probe_model(texture, true);
    let x = model.point_x.as_mut().unwrap();
    let y = model.point_y.as_mut().unwrap();
    (x[4], y[4]) = (-30, -25); // P: (u, v) = (0, 0)
    (x[5], y[5]) = (90, -25); // M: u = 1
    (x[6], y[6]) = (-30, 155); // N: v = 1
    model
}

/// Rows 0..size/2 red, the rest blue, every column alike.
fn row_split_texture(size: i32) -> Pix8 {
    let mut tex = Pix8::new(size, size, vec![0, 0xff0000, 0x0000ff]);
    for y in 0..size {
        for x in 0..size {
            tex.data[(y * size + x) as usize] = if y < size / 2 { 1 } else { 2 };
        }
    }
    tex
}

/// A deliberately minified wall: its 8×12 model-space extent projects to
/// roughly 32×48 pixels while spanning the full 128px atlas layer.
fn filter_probe_model(texture: i32) -> client::dash3d::Model {
    let mut model = textured_wall_model();
    model.point_x = Some(vec![-4, 4, 4, -4]);
    model.point_y = Some(vec![0, 0, -12, -12]);
    model.face_colour = Some(vec![texture, texture]);
    model.calc_bounding_cylinder();
    model
}

fn filter_probe_mesh(
    pix: &mut Pix3DDraw,
    texture: Option<i32>,
) -> client::render::world::SceneMesh {
    let mut world = flat_world();
    let mut rw = RenderWorld::new();
    if let Some(texture) = texture {
        world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
        rw.set_wall_model(
            &world,
            0,
            1,
            2,
            Some(SceneModel::Model(filter_probe_model(texture))),
            None,
        );
    }
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    rw.build_scene_mesh(&mut world, &Cache::default(), 0, pix)
}

/// A realistic high-memory 128×128 animated texture. Eight-row red/green
/// bands make Java's two-row-per-update scroll visible without depending on
/// a single texel or a layer readback hook.
fn animated_texture() -> Pix8 {
    let mut tex = Pix8::new(128, 128, vec![0, 0xff0000, 0x00ff00]);
    for y in 0..128 {
        let index = if (y / 8) % 2 == 0 { 1 } else { 2 };
        for x in 0..128 {
            tex.data[(y * 128 + x) as usize] = index;
        }
    }
    tex
}

fn animated_wall_model() -> client::dash3d::Model {
    let mut model = textured_wall_model();
    model.face_colour = Some(vec![TEXTURE_ANIMATED, TEXTURE_ANIMATED]);
    model
}

fn animated_frame(
    backend: GpuBackend,
    cache_dir: &std::path::Path,
) -> (Renderer, client::client::Client) {
    let mut renderer = Renderer::with_backend(Box::new(backend), false);
    Pix3D::init_colour_table(0.6);
    let mut client = client(cache_dir);
    client.set_draw(true);
    client.ingame = true;
    client.scene_state = 2;
    client.cam_x = 192;
    client.cam_y = 1950;
    client.cam_z = 192;
    client.cam_pitch = 128;
    client.world = flat_world();
    client.world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    renderer.world.set_wall_model(
        &client.world,
        0,
        1,
        2,
        Some(SceneModel::Model(animated_wall_model())),
        None,
    );
    renderer.pix3d.textures[TEXTURE_ANIMATED as usize] = Some(animated_texture());
    renderer.pix3d.tex_pal[TEXTURE_ANIMATED as usize] = Some(vec![0, 0xff0000, 0x00ff00]);
    renderer.pix3d.cycle = 41;
    client.world_update_num = 1;
    (renderer, client)
}

fn scene_window(frame: &[i32]) -> Vec<i32> {
    let mut scene = Vec::with_capacity(512 * 334);
    for y in 4..338 {
        scene.extend_from_slice(&frame[y * 765 + 4..y * 765 + 516]);
    }
    scene
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

/// Java's model raster clamps U but wraps V (`cur_v & 0x3f80` in high
/// memory, `cur_v & 0xfc0` in low memory). Exercise actual GPU sampling
/// with negative V, out-of-range U, and a transparent cutout. Each memory
/// mode runs in a fresh child because static atlas layers upload once.
#[test]
fn gpu_model_texture_wraps_v_clamps_u_and_preserves_cutouts() {
    const CHILD_MODE: &str = "R274_TEXTURE_ADDRESS_CHILD_MODE";
    if let Ok(mode) = std::env::var(CHILD_MODE) {
        Pix3D::init_colour_table(0.6);
        let (low_mem, size) = match mode.as_str() {
            "high" => (false, 128),
            "low" => (true, 64),
            _ => panic!("unexpected texture-address child mode {mode}"),
        };
        let mut backend =
            GpuBackend::try_new().expect("ACTUAL GPU REQUIRED for texture-address regression");
        let mut pix = Pix3DDraw::default();
        pix.set_clipping(512, 334);
        pix.low_mem = low_mem;
        pix.textures[TEXTURE_ADDRESS_V as usize] = Some(address_texture(size, true));
        pix.tex_pal[TEXTURE_ADDRESS_V as usize] =
            Some(vec![0, 0xff0000, 0x0000ff, 0x00ff00, 0xffffff]);
        pix.textures[TEXTURE_ADDRESS_U as usize] = Some(address_texture(size, false));
        pix.tex_pal[TEXTURE_ADDRESS_U as usize] =
            Some(vec![0, 0xff0000, 0x0000ff, 0x00ff00, 0xffffff]);
        pix.textures[TEXTURE_ADDRESS_SOLID as usize] = Some(solid_texture(0xffffff));
        pix.tex_pal[TEXTURE_ADDRESS_SOLID as usize] = Some(vec![0, 0xffffff]);

        let background =
            backend.render_scene_for_test(address_probe_mesh(&mut pix, None, true), &pix);
        let solid = backend.render_scene_for_test(
            address_probe_mesh(&mut pix, Some(TEXTURE_ADDRESS_SOLID), true),
            &pix,
        );
        let wrapped = backend.render_scene_for_test(
            address_probe_mesh(&mut pix, Some(TEXTURE_ADDRESS_V), true),
            &pix,
        );
        let u_clamped = backend.render_scene_for_test(
            address_probe_mesh(&mut pix, Some(TEXTURE_ADDRESS_U), false),
            &pix,
        );
        let wall_mask: Vec<bool> = solid
            .iter()
            .zip(&background)
            .map(|(wall, background)| wall != background)
            .collect();
        let coverage = wall_mask.iter().filter(|&&covered| covered).count();
        assert!(
            coverage > 500,
            "address probe must cover meaningful screen area (low_mem={low_mem}, coverage={coverage})"
        );
        let colour_count = |image: &[i32], channel: usize| {
            image
                .iter()
                .zip(&wall_mask)
                .filter(|(rgb, covered)| {
                    if !**covered {
                        return false;
                    }
                    let channels = [(**rgb >> 16) & 0xff, (**rgb >> 8) & 0xff, **rgb & 0xff];
                    channels[channel] > 128
                        && channels[(channel + 1) % 3] < 64
                        && channels[(channel + 2) % 3] < 64
                })
                .count()
        };
        let wrapped_red = colour_count(&wrapped, 0);
        let wrapped_blue = colour_count(&wrapped, 2);
        let cutouts = wrapped
            .iter()
            .zip(&background)
            .zip(&wall_mask)
            .filter(|((sample, background), covered)| **covered && sample == background)
            .count();
        assert!(
            wrapped_red > 100 && wrapped_blue < 20,
            "negative V must wrap into red texture rows, not clamp to blue edge (low_mem={low_mem}, red={wrapped_red}, blue={wrapped_blue})"
        );
        assert!(
            cutouts > 100,
            "wrapped transparent texels must discard to the background (low_mem={low_mem}, cutouts={cutouts})"
        );

        let clamped_green = colour_count(&u_clamped, 1);
        let repeated_red = colour_count(&u_clamped, 0);
        assert!(
            clamped_green > 100 && repeated_red < 20,
            "out-of-range U must stay clamped to the green edge (low_mem={low_mem}, green={clamped_green}, red={repeated_red})"
        );
        return;
    }

    let executable = std::env::current_exe().expect("current GPU test executable");
    for mode in ["high", "low"] {
        let output = std::process::Command::new(&executable)
            .args([
                "--exact",
                "gpu_model_texture_wraps_v_clamps_u_and_preserves_cutouts",
                "--nocapture",
            ])
            .env(CHILD_MODE, mode)
            .output()
            .unwrap_or_else(|error| panic!("spawn texture-address {mode} child: {error}"));
        assert!(
            output.status.success(),
            "texture-address {mode} child failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

/// A face whose texture coordinates cross zero must sample the few texels
/// between its vertices, as Java does per pixel (U clamped to column 0, V
/// wrapped by the row mask). Many 289 loc faces start a few texels below
/// zero; when the packed coordinate lost its sign, one vertex read ~256
/// texture repeats away, the face hit the smallest mip and drew one flat
/// averaged colour (or aliased noise). Here the rows split red/blue and V
/// runs -0.5..0.5, so the face must show solid red and solid blue bands,
/// never their purple average.
#[test]
fn gpu_model_texture_coordinates_crossing_zero_keep_their_sign() {
    const TEXTURE: i32 = 48;
    Pix3D::init_colour_table(0.6);
    let mut backend =
        GpuBackend::try_new().expect("ACTUAL GPU REQUIRED for zero-crossing UV regression");
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    pix.low_mem = false;
    pix.textures[TEXTURE as usize] = Some(row_split_texture(128));
    pix.tex_pal[TEXTURE as usize] = Some(vec![0, 0xff0000, 0x0000ff]);

    let background = backend.render_scene_for_test(probe_mesh(&mut pix, None), &pix);
    let image = backend.render_scene_for_test(
        probe_mesh(&mut pix, Some(zero_crossing_probe_model(TEXTURE))),
        &pix,
    );
    let covered: Vec<i32> = image
        .iter()
        .zip(&background)
        .filter(|(sample, background)| sample != background)
        .map(|(&sample, _)| sample)
        .collect();
    assert!(
        covered.len() > 500,
        "probe must cover meaningful screen area ({})",
        covered.len()
    );
    // Java samples each texel row band once down a column: at most a red,
    // blue, red sequence. A lost sign repeats the texture hundreds of times.
    let class = |rgb: i32| {
        let (r, g, b) = ((rgb >> 16) & 0xff, (rgb >> 8) & 0xff, rgb & 0xff);
        match (r > 128 && g < 64 && b < 64, b > 128 && g < 64 && r < 64) {
            (true, _) => 1,
            (_, true) => 2,
            _ => 0,
        }
    };
    let (mut red, mut blue, mut max_changes) = (0, 0, 0);
    for x in 0..512usize {
        let mut last = None;
        let mut changes = 0;
        for y in 0..334usize {
            let i = y * 512 + x;
            if image[i] == background[i] {
                continue;
            }
            let c = class(image[i]);
            red += (c == 1) as usize;
            blue += (c == 2) as usize;
            if last.is_some_and(|l| l != c) {
                changes += 1;
            }
            last = Some(c);
        }
        max_changes = max_changes.max(changes);
    }
    assert!(
        red > 100 && blue > 100 && max_changes <= 4,
        "zero-crossing U/V must sample each row band once (red={red}, blue={blue}, max changes down a column={max_changes}, covered={})",
        covered.len()
    );
}

/// Sparse Java-animated water (texture 17) keeps LOD0 colour so transparent
/// RGB-zero neighbours cannot darken retained samples. An otherwise identical
/// non-water layer must retain mip filtering, and both layers must keep the
/// same LOD0-alpha coverage. The two memory modes exercise both atlas upload
/// branches; low-memory temporal scrolling remains intentionally disabled.
#[test]
fn gpu_texture_17_uses_lod0_colour_without_disabling_other_mips() {
    Pix3D::init_colour_table(0.6);
    let mut backend =
        GpuBackend::try_new().expect("ACTUAL GPU REQUIRED for texture-17 filter regression");

    for (low_mem, size, control) in [
        (false, 128, TEXTURE_FILTER_CONTROL_HIGH),
        (true, 64, TEXTURE_FILTER_CONTROL_LOW),
    ] {
        let texture = sparse_filter_texture(size);
        let mut pix = Pix3DDraw::default();
        pix.set_clipping(512, 334);
        pix.low_mem = low_mem;
        pix.textures[TEXTURE_ANIMATED as usize] = Some(texture.clone());
        pix.tex_pal[TEXTURE_ANIMATED as usize] = Some(vec![0, 0xff0000]);
        pix.textures[control as usize] = Some(texture);
        pix.tex_pal[control as usize] = Some(vec![0, 0xff0000]);

        let background = backend.render_scene_for_test(filter_probe_mesh(&mut pix, None), &pix);
        let water = backend
            .render_scene_for_test(filter_probe_mesh(&mut pix, Some(TEXTURE_ANIMATED)), &pix);
        let non_water =
            backend.render_scene_for_test(filter_probe_mesh(&mut pix, Some(control)), &pix);
        let water_mask: Vec<bool> = water
            .iter()
            .zip(&background)
            .map(|(sample, background)| sample != background)
            .collect();
        let non_water_mask: Vec<bool> = non_water
            .iter()
            .zip(&background)
            .map(|(sample, background)| sample != background)
            .collect();

        assert_eq!(
            water_mask, non_water_mask,
            "texture 17 and the non-water control must preserve identical LOD0-alpha coverage (low_mem={low_mem})"
        );
        let covered = water_mask.iter().filter(|&&covered| covered).count();
        assert!(
            covered > 20,
            "the minified sparse fixture must produce meaningful coverage (low_mem={low_mem}, covered={covered})"
        );

        let mean_channel = |image: &[i32], shift: i32| {
            image
                .iter()
                .zip(&water_mask)
                .filter(|(_, covered)| **covered)
                .map(|(rgb, _)| (rgb >> shift) & 0xff)
                .sum::<i32>() as f32
                / covered as f32
        };
        let water_red = mean_channel(&water, 16);
        let water_green = mean_channel(&water, 8);
        let water_blue = mean_channel(&water, 0);
        let non_water_red = mean_channel(&non_water, 16);

        assert!(
            water_red > 245.0 && water_green < 2.0 && water_blue < 2.0,
            "texture 17 retained samples must keep pure LOD0 red (low_mem={low_mem}, rgb={water_red:.1}/{water_green:.1}/{water_blue:.1})"
        );
        assert!(
            non_water_red < water_red - 40.0,
            "the non-water control must remain mip-filtered (low_mem={low_mem}, water={water_red:.1}, control={non_water_red:.1})"
        );
    }
}

/// Texture 1 is the fountain's fine rippled underlay. Like the texture-17
/// flecks above, it keeps LOD0 colour while an otherwise identical non-water
/// layer remains mip-filtered. Texture 1 is a static atlas layer uploaded once
/// per process, so each memory mode runs in a fresh child process rather than
/// accidentally reusing the first mode's layer.
#[test]
fn gpu_texture_1_uses_lod0_colour_in_high_and_low_memory() {
    const CHILD_MODE: &str = "R274_TEXTURE_1_LOD0_CHILD_MODE";
    if let Ok(mode) = std::env::var(CHILD_MODE) {
        Pix3D::init_colour_table(0.6);
        let (low_mem, size, control) = match mode.as_str() {
            "high" => (false, 128, TEXTURE_FILTER_CONTROL_HIGH),
            "low" => (true, 64, TEXTURE_FILTER_CONTROL_LOW),
            _ => panic!("unexpected texture-1 child mode {mode}"),
        };
        let mut backend =
            GpuBackend::try_new().expect("ACTUAL GPU REQUIRED for texture-1 filter regression");
        let texture = sparse_filter_texture(size);
        let mut pix = Pix3DDraw::default();
        pix.set_clipping(512, 334);
        pix.low_mem = low_mem;
        pix.textures[TEXTURE_RIPPLED_UNDERLAY as usize] = Some(texture.clone());
        pix.tex_pal[TEXTURE_RIPPLED_UNDERLAY as usize] = Some(vec![0, 0xff0000]);
        pix.textures[control as usize] = Some(texture);
        pix.tex_pal[control as usize] = Some(vec![0, 0xff0000]);

        let background = backend.render_scene_for_test(filter_probe_mesh(&mut pix, None), &pix);
        let underlay = backend.render_scene_for_test(
            filter_probe_mesh(&mut pix, Some(TEXTURE_RIPPLED_UNDERLAY)),
            &pix,
        );
        let non_water =
            backend.render_scene_for_test(filter_probe_mesh(&mut pix, Some(control)), &pix);
        let underlay_mask: Vec<bool> = underlay
            .iter()
            .zip(&background)
            .map(|(sample, background)| sample != background)
            .collect();
        let non_water_mask: Vec<bool> = non_water
            .iter()
            .zip(&background)
            .map(|(sample, background)| sample != background)
            .collect();
        assert_eq!(
            underlay_mask, non_water_mask,
            "texture 1 and the non-water control must preserve identical LOD0-alpha coverage (low_mem={low_mem})"
        );
        let covered = underlay_mask.iter().filter(|&&covered| covered).count();
        assert!(
            covered > 20,
            "the minified texture-1 fixture must produce meaningful coverage (low_mem={low_mem}, covered={covered})"
        );
        let mean_channel = |image: &[i32], shift: i32| {
            image
                .iter()
                .zip(&underlay_mask)
                .filter(|(_, covered)| **covered)
                .map(|(rgb, _)| (rgb >> shift) & 0xff)
                .sum::<i32>() as f32
                / covered as f32
        };
        let underlay_red = mean_channel(&underlay, 16);
        let underlay_green = mean_channel(&underlay, 8);
        let underlay_blue = mean_channel(&underlay, 0);
        let non_water_red = mean_channel(&non_water, 16);
        assert!(
            underlay_red > 245.0 && underlay_green < 2.0 && underlay_blue < 2.0,
            "texture 1 retained samples must keep pure LOD0 red (low_mem={low_mem}, rgb={underlay_red:.1}/{underlay_green:.1}/{underlay_blue:.1})"
        );
        assert!(
            non_water_red < underlay_red - 40.0,
            "the non-water control must remain mip-filtered (low_mem={low_mem}, underlay={underlay_red:.1}, control={non_water_red:.1})"
        );
        return;
    }

    let executable = std::env::current_exe().expect("current GPU test executable");
    for mode in ["high", "low"] {
        let output = std::process::Command::new(&executable)
            .args([
                "--exact",
                "gpu_texture_1_uses_lod0_colour_in_high_and_low_memory",
                "--nocapture",
            ])
            .env(CHILD_MODE, mode)
            .output()
            .unwrap_or_else(|error| panic!("spawn texture-1 {mode} child: {error}"));
        assert!(
            output.status.success(),
            "texture-1 {mode} child failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
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

/// Production temporal path: the submitted texture-17 mesh stamps Pix3D,
/// the post-render scroll refreshes only that atlas layer for the next
/// paint, and loading reuses the last scene without another scroll. Mesh
/// presence establishes submission, not that every triangle produced a
/// visible fragment; the full-frame Metal readbacks below establish the
/// fixture's rasterized pixels actually changed.
#[test]
fn gpu_texture_17_scrolls_across_two_paints_and_freezes_while_loading() {
    Pix3D::init_colour_table(0.6);
    let Ok(backend) = GpuBackend::try_new() else {
        eprintln!("NO ADAPTER: texture-17 temporal GPU check did not pass");
        return;
    };
    let cache_dir =
        std::env::temp_dir().join(format!("r274-gpu-texture-scroll-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&cache_dir);
    let (mut renderer, mut client) = animated_frame(backend, &cache_dir);
    let original = renderer.pix3d.textures[TEXTURE_ANIMATED as usize]
        .as_ref()
        .unwrap()
        .data
        .clone();
    let cycle_before = renderer.pix3d.cycle;

    let FrameOutput::Texture(first) = renderer.game_draw(&mut client) else {
        panic!("GPU frame must return a texture");
    };
    let first_scene = scene_window(&first.read_back());
    assert_eq!(
        renderer.pix3d.tex_cycle[TEXTURE_ANIMATED as usize], cycle_before,
        "the submitted texture-17 mesh must stamp exactly once this paint"
    );
    assert_eq!(
        renderer.pix3d.cycle,
        cycle_before + 1,
        "duplicate vertices must not stamp a texture more than once"
    );
    assert!(
        renderer.pix3d.active_texels[TEXTURE_ANIMATED as usize].is_none(),
        "the GPU stamp must not materialize a CPU texel row"
    );
    assert_ne!(
        renderer.pix3d.textures[TEXTURE_ANIMATED as usize]
            .as_ref()
            .unwrap()
            .data,
        original,
        "the first live paint must prepare the scrolled Pix8 for the next paint"
    );

    client.world_update_num = 1;
    let FrameOutput::Texture(second) = renderer.game_draw(&mut client) else {
        panic!("GPU frame must return a texture");
    };
    let second_scene = scene_window(&second.read_back());
    let red_green_flips = first_scene
        .iter()
        .zip(&second_scene)
        .filter(|&(&before, &after)| {
            let (br, bg, bb) = ((before >> 16) & 0xff, (before >> 8) & 0xff, before & 0xff);
            let (ar, ag, ab) = ((after >> 16) & 0xff, (after >> 8) & 0xff, after & 0xff);
            (br > 128 && bg < 64 && bb < 64 && ag > 128 && ar < 64 && ab < 64)
                || (bg > 128 && br < 64 && bb < 64 && ar > 128 && ag < 64 && ab < 64)
        })
        .count();
    assert!(
        red_green_flips > 500,
        "two live readbacks must show rasterized red/green texture motion (flips={red_green_flips})"
    );

    let before_freeze = renderer.pix3d.textures[TEXTURE_ANIMATED as usize]
        .as_ref()
        .unwrap()
        .data
        .clone();
    client.scene_state = 1;
    client.world_update_num = 1;
    let FrameOutput::Texture(frozen) = renderer.game_draw(&mut client) else {
        panic!("GPU frame must return a texture");
    };
    let frozen_scene = scene_window(&frozen.read_back());
    assert_eq!(
        renderer.pix3d.textures[TEXTURE_ANIMATED as usize]
            .as_ref()
            .unwrap()
            .data,
        before_freeze,
        "scene_state 1 must not scroll the CPU texture"
    );
    assert_eq!(
        frozen_scene, second_scene,
        "scene_state 1 must composite the last rendered scene unchanged"
    );
}

/// Process-shared `GpuAssets` must stage each slot's current texture-17
/// phase before that slot submits its scene. Slot B and the stationary
/// low-memory slot C both start at phase 0 after slot A advances, so their
/// first paints must still sample phase 0 rather than A's shared-atlas phase.
#[test]
fn gpu_texture_17_shared_assets_isolate_phase_across_backends() {
    Pix3D::init_colour_table(0.6);
    let Ok(backend_a) = GpuBackend::try_new() else {
        eprintln!("NO ADAPTER: shared-assets A/B ownership check did not pass");
        return;
    };
    let Ok(backend_b) = GpuBackend::try_new() else {
        eprintln!("NO ADAPTER: shared-assets A/B ownership check did not pass");
        return;
    };
    let cache_a =
        std::env::temp_dir().join(format!("r274-gpu-texture-ab-a-{}", std::process::id()));
    let cache_b =
        std::env::temp_dir().join(format!("r274-gpu-texture-ab-b-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&cache_a);
    let _ = std::fs::create_dir_all(&cache_b);

    let (mut renderer_a, mut client_a) = animated_frame(backend_a, &cache_a);
    let phase0_cpu = renderer_a.pix3d.textures[TEXTURE_ANIMATED as usize]
        .as_ref()
        .unwrap()
        .data
        .clone();

    let FrameOutput::Texture(first_a) = renderer_a.game_draw(&mut client_a) else {
        panic!("slot A first paint must return a texture");
    };
    let phase0_scene = scene_window(&first_a.read_back());
    assert_ne!(
        renderer_a.pix3d.textures[TEXTURE_ANIMATED as usize]
            .as_ref()
            .unwrap()
            .data,
        phase0_cpu,
        "slot A must scroll Pix8 after the first paint"
    );

    client_a.world_update_num = 1;
    let FrameOutput::Texture(second_a) = renderer_a.game_draw(&mut client_a) else {
        panic!("slot A second paint must return a texture");
    };
    let phase1_scene = scene_window(&second_a.read_back());
    let a_flips = phase0_scene
        .iter()
        .zip(&phase1_scene)
        .filter(|&(&before, &after)| {
            let (br, bg, bb) = ((before >> 16) & 0xff, (before >> 8) & 0xff, before & 0xff);
            let (ar, ag, ab) = ((after >> 16) & 0xff, (after >> 8) & 0xff, after & 0xff);
            (br > 128 && bg < 64 && bb < 64 && ag > 128 && ar < 64 && ab < 64)
                || (bg > 128 && br < 64 && bb < 64 && ar > 128 && ag < 64 && ab < 64)
        })
        .count();
    assert!(
        a_flips > 500,
        "slot A must show temporal scroll between paints (flips={a_flips})"
    );

    let (mut renderer_b, mut client_b) = animated_frame(backend_b, &cache_b);
    assert_eq!(
        renderer_b.pix3d.textures[TEXTURE_ANIMATED as usize]
            .as_ref()
            .unwrap()
            .data,
        phase0_cpu,
        "slot B must start with unscrolled phase-0 Pix8"
    );
    let FrameOutput::Texture(first_b) = renderer_b.game_draw(&mut client_b) else {
        panic!("slot B first paint must return a texture");
    };
    let b_scene = scene_window(&first_b.read_back());
    let b_vs_phase0 = phase0_scene
        .iter()
        .zip(&b_scene)
        .filter(|&(&before, &after)| {
            let (br, bg, bb) = ((before >> 16) & 0xff, (before >> 8) & 0xff, before & 0xff);
            let (ar, ag, ab) = ((after >> 16) & 0xff, (after >> 8) & 0xff, after & 0xff);
            (br > 128 && bg < 64 && bb < 64 && ag > 128 && ar < 64 && ab < 64)
                || (bg > 128 && br < 64 && bb < 64 && ar > 128 && ag < 64 && ab < 64)
        })
        .count();
    assert_eq!(
        b_vs_phase0, 0,
        "slot B's phase-0 Pix8 must replace A's shared-atlas phase before submit"
    );

    let Ok(backend_c) = GpuBackend::try_new() else {
        eprintln!("NO ADAPTER: lowmem sibling ownership check did not pass");
        return;
    };
    let cache_c =
        std::env::temp_dir().join(format!("r274-gpu-texture-ab-c-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&cache_c);
    let (mut renderer_c, mut client_c) = animated_frame(backend_c, &cache_c);
    renderer_c.pix3d.low_mem = true;
    assert_eq!(
        renderer_c.pix3d.textures[TEXTURE_ANIMATED as usize]
            .as_ref()
            .unwrap()
            .data,
        phase0_cpu
    );
    let FrameOutput::Texture(first_c) = renderer_c.game_draw(&mut client_c) else {
        panic!("lowmem sibling paint must return a texture");
    };
    let c_scene = scene_window(&first_c.read_back());
    let c_vs_phase0 = phase0_scene
        .iter()
        .zip(&c_scene)
        .filter(|&(&before, &after)| {
            let (br, bg, bb) = ((before >> 16) & 0xff, (before >> 8) & 0xff, before & 0xff);
            let (ar, ag, ab) = ((after >> 16) & 0xff, (after >> 8) & 0xff, after & 0xff);
            (br > 128 && bg < 64 && bb < 64 && ag > 128 && ar < 64 && ab < 64)
                || (bg > 128 && br < 64 && bb < 64 && ar > 128 && ag < 64 && ab < 64)
        })
        .count();
    assert_eq!(
        c_vs_phase0, 0,
        "lowmem slot's stationary phase-0 Pix8 must replace A's shared-atlas phase before submit"
    );
    assert_eq!(
        renderer_c.pix3d.textures[TEXTURE_ANIMATED as usize]
            .as_ref()
            .unwrap()
            .data,
        phase0_cpu,
        "lowmem must not mutate Pix8 via texture_run_anims"
    );
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
