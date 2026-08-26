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
use client::graphics::{Pix2D, Pix3D, Pix3DDraw, Pix8};
use client::render::backend::{FrameOutput, GpuBackend, RenderBackend};
use client::render::world::GpuVertex;
use client::render::{RenderWorld, Renderer};

const SHADE: i32 = 200 * 128 + 100;
const TEXTURE_RED: i32 = 7;
const TEXTURE_BLUE: i32 = 12;

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
    model.face_colour = Some(vec![TEXTURE_RED, TEXTURE_BLUE]);
    model.face_colour_a = Some(vec![SHADE, SHADE]);
    model.face_colour_b = Some(vec![SHADE, SHADE]);
    model.face_colour_c = Some(vec![SHADE, SHADE]);
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
fn textured_wall_mesh(pix: &mut Pix3DDraw) -> (RenderWorld, World, client::render::world::SceneMesh) {
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
            assert_eq!((v.abhsl & 0xffff) as i32, SHADE, "a textured vertex carries the raw shade");
            saw_nonzero_u |= (v.uv_tex >> 16) != 0;
            saw_nonzero_v |= v.v != 0;
        } else {
            assert_eq!(v.uv_tex, 0, "untextured vertices carry no texture");
            assert_eq!(v.v, 0, "untextured vertices carry no v");
        }
    }
    assert!(found_red, "the red-textured face must be in the mesh");
    assert!(found_blue, "the blue-textured face must be in the mesh");
    assert!(saw_nonzero_u, "a textured face must pack a nonzero u (high 16 bits)");
    assert!(saw_nonzero_v, "a textured face must pack a nonzero v");

    // The texture array has one 128×128 layer per texture id, so ids 7
    // and 12 sample different layers (no shared-atlas cell derivation).
    assert!((0..50).contains(&TEXTURE_RED), "the fixture texture id is in the valid range");
    assert!((0..50).contains(&TEXTURE_BLUE), "the fixture texture id is in the valid range");
    assert_ne!(
        TEXTURE_RED, TEXTURE_BLUE,
        "the two textures must sample different array layers"
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

    // The shade (SHADE = 0x6464) selects brightness level 1 (~7/8), so the
    // rendered red/blue are the palette colours scaled, never white.
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
    assert!(found_red + found_blue > 500, "the textured wall must cover screen area");
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
    pix.textures[7] = Some(tex.clone());
    pix.tex_pal[7] = Some(vec![0, 0xff0000, 0x00ff00, 0x0000ff, 0xffff00]);

    let mut world = flat_world();
    world.set_wall(0, 1, 2, 2000, 8, 0, 0, 0, 0, 0, 0, 0);
    let mut rw = RenderWorld::new();
    let mut model = textured_wall_model();
    model.face_colour = Some(vec![7, 7]); // both faces use the quadrant texture
    rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 192, 1950, 192, 3, 0, 128);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);

    // Mesh-level: the textured vertices' fixed-point UV must span the full
    // 128px layer in both axes even on the low-mem path (a scale-64 bug caps
    // at 64, i.e. u/v ≤ 128 here).
    let mut max_u = 0u32;
    let mut max_v = 0u32;
    for v in mesh.clone().vertices().iter().filter(|v| (v.uv_tex & 0xffff) == 8) {
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

/// The composite: after a scene render, `finish` returns one full-frame
/// 765×503 texture carrying the scene at its (4, 4) point — the scene and
/// the (empty, here) chrome quads land in the same texture, no readback.
/// The CPU-drawn `draw_area` composites over the scene: a black pixel in
/// the scene window is the scene's transparent hole, and a painted
/// (opaque) pixel covers the scene.
#[test]
fn composite_lands_the_scene_in_the_full_frame() {
    Pix3D::init_colour_table(0.6);
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the composite test skips");
        return;
    };
    let mut pix = textured_pix();
    let (_rw, _world, mesh) = textured_wall_mesh(&mut pix);
    backend.render_scene_for_test(mesh, &pix);

    let mut r = Renderer::new(false);
    // Paint a known chrome rect inside the scene window (x∈[4,516),
    // y∈[4,338)): with the scene ready it must stay opaque and cover the
    // scene, while the surrounding black `draw_area` stays transparent.
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        surface.fill_rect(100, 100, 16, 16, 0xffffff);
    }
    let FrameOutput::Texture(handle) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };
    assert_eq!((handle.width, handle.height), (765, 503));
    let pixels = handle.read_back();
    // The textured wall renders inside the (4, 4) scene region (not the
    // raw 512×334 scene texture — the full frame).
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
    // The painted rect is opaque chrome over the scene window (alpha 255),
    // not a transparent hole.
    for y in 100..116 {
        for x in 100..116 {
            assert_eq!(
                pixels[y * 765 + x], 0xffffff,
                "a non-black draw_area pixel in the scene window must cover the scene at ({x}, {y})"
            );
        }
    }
    // Nothing outside the frame's own bounds.
    assert_eq!(pixels.len(), 765 * 503);
}

/// The `GpuVertex` layout stays `bytemuck`-clean for the raw upload (the
/// extra UV/shade/tex-id fields must not break the Pod contract).
#[test]
fn gpu_vertex_stays_pod() {
    fn assert_pod<T: bytemuck::Pod + bytemuck::Zeroable>() {}
    assert_pod::<GpuVertex>();
}
