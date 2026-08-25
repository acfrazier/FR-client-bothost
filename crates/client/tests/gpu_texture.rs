// Task 5b (GPU-chrome campaign): the model-texture sampling (white-buildings
// fix). Textured faces carry a per-face texture index + projective UV and
// sample the shared model atlas in the scene shader — they are no longer
// flat-shaded. Two checks: (1) the scene mesh carries the texture id and a
// finite, non-degenerate UV for a multi-texture model (pure CPU math, like
// gpu_mesh.rs); (2) a real `GpuBackend` renders that mesh and the read-back
// scene contains the two textures' colours (red + blue), so each texture
// samples non-white texels. The GPU check needs an adapter and skips
// without one. This binary owns the textured-mesh colour-table brightness,
// so it pins `Pix3D::init_colour_table(0.6)` itself.
use client::config::Cache;
use client::core::World;
use client::dash3d::{SceneModel, TerrainOverlayShape};
use client::graphics::{Pix3D, Pix3DDraw, Pix8};
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
    for v in vertices.iter() {
        if v.tex_id == TEXTURE_RED as u32 {
            found_red = true;
        }
        if v.tex_id == TEXTURE_BLUE as u32 {
            found_blue = true;
        }
        if v.tex_id != u32::MAX {
            assert!(
                v.u_num.is_finite() && v.u_den.is_finite() && v.v_num.is_finite() && v.v_den.is_finite(),
                "a textured vertex must carry a finite UV"
            );
            assert!(
                v.u_den.abs() > 1e-3 && v.v_den.abs() > 1e-3,
                "a visible textured face must have a non-degenerate UV denominator"
            );
        } else {
            assert_eq!(v.u_num, 0.0, "untextured vertices carry no UV");
        }
    }
    assert!(found_red, "the red-textured face must be in the mesh");
    assert!(found_blue, "the blue-textured face must be in the mesh");

    // The two textures resolve to different atlas cells (the shader derives
    // the cell from the id), so a red face and a blue face sample different
    // regions.
    assert_ne!(
        TEXTURE_RED % 8 + 8 * (TEXTURE_RED / 8),
        TEXTURE_BLUE % 8 + 8 * (TEXTURE_BLUE / 8),
        "the two textures must occupy different atlas cells"
    );
}

/// End-to-end: a real `GpuBackend` renders the textured-wall mesh and the
/// read-back scene contains the two textures' colours — red *and* blue
/// texels, so textured faces sample the atlas instead of flat-shading
/// white. Skips on machines without an adapter.
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

/// The low-mem path (the default bot config): a halved 64×64 texture and
/// `pix.low_mem` set. The atlas bakes every cell at 128×128, so the UV
/// numerator scale must be 128 regardless of the memory mode — a scale-64
/// mesh would sample only the top-left quarter of the cell, stretched 2×.
/// The quadrant texture makes that visible: the full 64×64 texture (all
/// four quadrants) must appear on the face.
#[test]
fn gpu_lowmem_texture_samples_the_full_128px_cell() {
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

    // Mesh-level: the textured vertices' UV must span the full 128px cell
    // in both axes even on the low-mem path (a scale-64 bug caps at 64).
    let mut max_u = 0.0f32;
    let mut max_v = 0.0f32;
    for v in mesh.clone().vertices().iter().filter(|v| v.tex_id == 7) {
        max_u = max_u.max((v.u_num / v.u_den).abs());
        max_v = max_v.max((v.v_num / v.v_den).abs());
    }
    assert!(
        max_u > 110.0 && max_v > 110.0,
        "the low-mem mesh UV must span the full 128px cell (got u≤{max_u}, v≤{max_v})"
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
        "the low-mem textured face must sample more than the top-left quarter of the 128px cell (seen {seen:?})"
    );
}

/// The composite: after a scene render, `finish` returns one full-frame
/// 765×503 texture carrying the scene at its (4, 4) point — the scene and
/// the (empty, here) chrome quads land in the same texture, no readback.
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
