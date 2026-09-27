//! Offline fountain differential: the actual cache model is opt-in; the
//! self-contained scanline-lighting regression runs without game assets.
use client::config::Cache;
use client::dash3d::Model;
use client::graphics::{Pix2D, Pix3D, Pix3DDraw};
use client::io::JagFile;
use client::render::backend::GpuBackend;
use client::render::world::SceneMesh;
use std::path::Path;

fn load_model(snapshot: &Path) -> (Cache, JagFile) {
    let bytes = std::fs::read(snapshot.join("models.bin")).unwrap();
    let mut pos = 0;
    while pos + 8 <= bytes.len() {
        let id = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap()) as i32;
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        pos += 8;
        if id == 1497 {
            Model::unpack(id, Some(&bytes[pos..pos + len]));
        }
        pos += len;
    }
    let cache = Cache::unpack(&JagFile::new(
        std::fs::read(snapshot.join("config")).unwrap(),
    ));
    let textures = JagFile::new(std::fs::read(snapshot.join("textures")).unwrap());
    (cache, textures)
}

fn pixels(model: &Model, pix: &mut Pix3DDraw, pitch: i32, gpu: bool) -> (Vec<i32>, SceneMesh) {
    let distance = pitch * 3 + 600;
    let inv = ((2048 - pitch) & 2047) as usize;
    let eye_y = -295 - ((-distance * Pix3D::sin_table()[inv]) >> 16);
    let eye_z = 7232 - ((distance * Pix3D::cos_table()[inv]) >> 16);
    let mut out = vec![0; 512 * 334];
    let mut surface = Pix2D::with_pixels(&mut out, 512, 334);
    pix.set_render_clipping(&surface);
    if gpu {
        pix.capture = Some(SceneMesh::default());
    }
    model.world_render(
        pix,
        &mut surface,
        0,
        Pix3D::sin_table()[pitch as usize],
        Pix3D::cos_table()[pitch as usize],
        0,
        65536,
        6912 - 6720,
        -eye_y,
        7552 - eye_z,
        0,
    );
    (out, pix.capture.take().unwrap_or_default())
}

fn save(path: &Path, pixels: &[i32]) {
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    std::fs::write(path, bytes).unwrap();
}

fn metrics(label: &str, cpu: &[i32], gpu: &[i32]) -> f64 {
    let rgb = |p: i32| [(p >> 16) & 255, (p >> 8) & 255, p & 255];
    let mut error = 0;
    let mut common = 0;
    let mut max = 0;
    let mut delta = [0usize; 2];
    let mut pairs = [0usize; 2];
    for (i, (&a, &b)) in cpu.iter().zip(gpu).enumerate() {
        if a != 0 && b != 0 {
            common += 1;
            for (a, b) in rgb(a).into_iter().zip(rgb(b)) {
                let d = (a - b).unsigned_abs() as usize;
                error += d;
                max = max.max(d);
            }
        }
        if i >= 512 {
            for (k, image) in [cpu, gpu].into_iter().enumerate() {
                if image[i] != 0 && image[i - 512] != 0 {
                    pairs[k] += 3;
                    delta[k] += rgb(image[i])
                        .into_iter()
                        .zip(rgb(image[i - 512]))
                        .map(|(a, b)| (a - b).unsigned_abs() as usize)
                        .sum::<usize>();
                }
            }
        }
    }
    assert!(
        common > 500,
        "{label}: the model must cover a meaningful area"
    );
    let mae = error as f64 / (common * 3) as f64;
    println!("{label}: coverage={}/{}, common={common}, mae={:.4}, max={max}, vertical_delta={:.4}/{:.4}",
        cpu.iter().filter(|&&p| p != 0).count(), gpu.iter().filter(|&&p| p != 0).count(),
        error as f64 / (common * 3) as f64, delta[0] as f64 / pairs[0] as f64, delta[1] as f64 / pairs[1] as f64);
    mae
}

#[test]
#[ignore = "requires FOUNTAIN_SNAPSHOT and FOUNTAIN_OUTPUT; offline real-cache evidence"]
fn offline_fountain_views() {
    let snapshot = std::env::var("FOUNTAIN_SNAPSHOT").unwrap();
    let output = std::env::var("FOUNTAIN_OUTPUT").unwrap();
    std::fs::create_dir_all(&output).unwrap();
    let (cache, textures) = load_model(Path::new(&snapshot));
    Pix3D::init_colour_table(0.8);
    let model = cache.locs[879]
        .get_model(&cache, 10, 0, 0, 0, 0, 0, -1)
        .unwrap();
    assert_eq!(model.num_faces, 302);
    let mut backend = GpuBackend::try_new().expect("actual GPU is required for the offline proof");
    for group in ["full", "texture1", "stone", "opaque1", "constant1"] {
        let mut model = model.clone();
        for f in 0..model.num_faces as usize {
            let textured = model.face_render_type.as_ref().unwrap()[f] & 2 != 0;
            let id = if textured {
                model.face_colour.as_ref().unwrap()[f]
            } else {
                -1
            };
            if (group == "stone" && textured) || (group != "full" && group != "stone" && id != 1) {
                model.face_render_type.as_mut().unwrap()[f] = -1;
            }
            if group == "opaque1" && id == 1 {
                model.face_colour.as_mut().unwrap()[f] = 17;
            }
            if group == "constant1" {
                model.face_colour_a.as_mut().unwrap()[f] = 0;
                model.face_colour_b.as_mut().unwrap()[f] = 0;
                model.face_colour_c.as_mut().unwrap()[f] = 0;
            }
        }
        let mut pix = Pix3DDraw::default();
        pix.low_mem = false;
        pix.unpack_textures(&textures);
        pix.init_pool(20);
        pix.init_texture_palettes(0.8);
        if group == "opaque1" {
            // Animated layer 17 is refreshed for this draw; static layer 1
            // may already belong to the full-model probe in this process.
            pix.tex_pal[17] = Some(vec![0xffffff; pix.tex_pal[1].as_ref().unwrap().len()]);
            pix.textures[17] = pix.textures[1].take();
        }
        for pitch in [128, 256, 383] {
            let (cpu, _) = pixels(&model, &mut pix, pitch, false);
            let (_, mesh) = pixels(&model, &mut pix, pitch, true);
            let gpu = backend.render_scene_for_test(&mesh, &pix);
            let label = format!("{group}-{pitch}");
            let mae = metrics(&label, &cpu, &gpu);
            if group == "opaque1" {
                // Pixel-centre coverage still differs at triangle boundaries,
                // but lighting on the actual basin must stay close to Pix3D.
                assert!(mae <= 3.0, "{label}: lighting-band MAE {mae} exceeds 3/255");
            }
            save(&Path::new(&output).join(format!("{label}-cpu.rgb")), &cpu);
            save(&Path::new(&output).join(format!("{label}-gpu.rgb")), &gpu);
        }
    }
}

fn textured_triangle(x: [i32; 3], y: [i32; 3], z: [i32; 3], shades: [i32; 3]) -> Model {
    let mut model = Model {
        num_points: 3,
        point_x: Some(x.to_vec()),
        point_y: Some(y.to_vec()),
        point_z: Some(z.to_vec()),
        num_faces: 1,
        num_t: 1,
        face_vertex_a: Some(vec![0]),
        face_vertex_b: Some(vec![1]),
        face_vertex_c: Some(vec![2]),
        face_render_type: Some(vec![2]),
        face_colour: Some(vec![24]),
        face_colour_a: Some(vec![shades[0]]),
        face_colour_b: Some(vec![shades[1]]),
        face_colour_c: Some(vec![shades[2]]),
        face_texture_p: Some(vec![0]),
        face_texture_m: Some(vec![1]),
        face_texture_n: Some(vec![2]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();
    model
}

fn triangle_pixels(
    model: &Model,
    pix: &mut Pix3DDraw,
    offset: (i32, i32, i32),
) -> (Vec<i32>, SceneMesh) {
    let mut cpu = vec![0; 512 * 334];
    let mut surface = Pix2D::with_pixels(&mut cpu, 512, 334);
    pix.set_render_clipping(&surface);
    for capture in [false, true] {
        if capture {
            pix.capture = Some(SceneMesh::default());
        }
        model.world_render(
            pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            offset.0,
            offset.1,
            offset.2,
            0,
        );
    }
    (cpu, pix.capture.take().unwrap())
}

fn solid_texture(low_mem: bool, colour: i32) -> Pix3DDraw {
    let mut pix = Pix3DDraw::default();
    pix.low_mem = low_mem;
    let size = if low_mem { 64 } else { 128 };
    pix.textures[24] = Some(client::graphics::Pix8::new(size, size, vec![colour]));
    pix.tex_pal[24] = Some(vec![colour]);
    pix.init_pool(1);
    pix
}

/// Depth must not move the CPU's eight-pixel lighting bands. A solid texel
/// removes UV/filtering from the comparison; the shade varies on both axes.
#[test]
fn textured_lighting_matches_cpu_scanline_bands() {
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skipping textured lighting readback");
        return;
    };
    let mut model = textured_triangle(
        [-192, -384, 384],
        [-103, 226, -206],
        [-256, 256, 256],
        [0, 64, 127],
    );
    for (low_mem, offset_x, offset_y, colour, order) in [
        (false, 0, 0, 0xffffff, [0, 1, 2]),
        (true, 0, 0, 0x759fe0, [0, 1, 2]),
        (false, -240, 0, 0x759fe0, [1, 2, 0]),
        (false, 240, -140, 0xffffff, [2, 0, 1]),
    ] {
        model.face_vertex_a = Some(vec![order[0]]);
        model.face_vertex_b = Some(vec![order[1]]);
        model.face_vertex_c = Some(vec![order[2]]);
        let shades = [0, 64, 127];
        model.face_colour_a = Some(vec![shades[order[0] as usize]]);
        model.face_colour_b = Some(vec![shades[order[1] as usize]]);
        model.face_colour_c = Some(vec![shades[order[2] as usize]]);
        let mut pix = solid_texture(low_mem, colour);
        let (cpu, mesh) = triangle_pixels(&model, &mut pix, (offset_x, offset_y, 768));
        let gpu = backend.render_scene_for_test(&mesh, &pix);
        let mut flat = model.clone();
        flat.face_colour_a = Some(vec![0]);
        flat.face_colour_b = Some(vec![0]);
        flat.face_colour_c = Some(vec![0]);
        let (_, cover_mesh) = triangle_pixels(&flat, &mut pix, (offset_x, offset_y, 768));
        let cover = backend.render_scene_for_test(&cover_mesh, &pix);
        let mut different = 0;
        let mut common = 0;
        for ((&cpu, &gpu), &cover) in cpu.iter().zip(&gpu).zip(&cover) {
            if cpu != 0 && cover != 0 {
                common += 1;
                different += usize::from(cpu != gpu);
            }
        }
        assert!(
            common > 10_000,
            "fixture must cover a range of scanline spans"
        );
        assert_eq!(
            different, 0,
            "CPU lighting bands differ (low_mem={low_mem}, offset={offset_x}/{offset_y})"
        );
    }
}

/// Float GPU coverage includes pixels above the CPU's integer top edge.
/// Those pixels still need a legitimate palette shade, not an extrapolated
/// negative fixed-point shade interpreted as an enormous unsigned shift.
#[test]
fn textured_lighting_stays_in_palette_at_subpixel_edges() {
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skipping subpixel lighting readback");
        return;
    };
    let lit = textured_triangle([-192, -384, 384], [-206, 226, -206], [256; 3], [0, 64, 127]);
    let mut flat = lit.clone();
    flat.face_colour_a = Some(vec![0]);
    flat.face_colour_b = Some(vec![0]);
    flat.face_colour_c = Some(vec![0]);
    let mut pix = solid_texture(false, 0xffffff);
    // These are the eight exact CPU get_texels/texture_scanline colours.
    let palette = [
        0xf8f8ff, 0xd8d8e0, 0xb8b8c0, 0x9898a1, 0x7c7c7f, 0x6c6c70, 0x5c5c60, 0x4c4c50,
    ];
    for y in [-45, -31, -18, -7, 0, 9, 22, 37] {
        let offset = (-17, y, 777);
        let (_, cover_mesh) = triangle_pixels(&flat, &mut pix, offset);
        let cover = backend.render_scene_for_test(&cover_mesh, &pix);
        let (_, mesh) = triangle_pixels(&lit, &mut pix, offset);
        let gpu = backend.render_scene_for_test(&mesh, &pix);
        for (i, (&cover, &pixel)) in cover.iter().zip(&gpu).enumerate() {
            if cover != 0 {
                assert!(
                    palette.contains(&pixel),
                    "invalid shade {pixel:#08x} at ({}, {}) offset_y={y}",
                    i % 512,
                    i / 512,
                );
            }
        }
    }
}

/// The fourth vertex of a near-clipped polygon can require hclip even
/// when all vertices of the first fan triangle are on screen.
#[test]
fn near_clipped_polygon_shares_scanline_clipping() {
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skipping clipped lighting readback");
        return;
    };
    for far_x in [120, 200] {
        // The two crossing edges have dz=256: CPU reciprocal-table and
        // capture divisions agree exactly, isolating the polygon hclip.
        let model = textured_triangle(
            [-10, -100, far_x],
            [0, 60, -60],
            [-132, 124, 124],
            [0, 64, 127],
        );
        let mut pix = solid_texture(false, 0xffffff);
        let (cpu, mesh) = triangle_pixels(&model, &mut pix, (0, 0, 150));
        let gpu = backend.render_scene_for_test(&mesh, &pix);
        assert_eq!(mesh.vertices().len(), 6, "must exercise a clipped quad");
        let screen: Vec<(i32, i32)> = mesh.vertices()[..3]
            .iter()
            .map(|v| {
                (
                    256 + ((v.x as i32) << 9) / v.z as i32,
                    167 + ((v.y as i32) << 9) / v.z as i32,
                )
            })
            .collect();
        let mut interior = 0;
        for (i, (&cpu, &gpu)) in cpu.iter().zip(&gpu).enumerate() {
            let (x, y) = ((i % 512) as i32 * 2 + 1, (i / 512) as i32 * 2 + 1);
            let edges = std::array::from_fn::<_, 3, _>(|j| {
                let (a, b) = (screen[j], screen[(j + 1) % 3]);
                let dx = b.0 - a.0;
                let dy = b.1 - a.1;
                let cross = dx * (y - 2 * a.1) - dy * (x - 2 * a.0);
                (cross, 4 * (dx.abs() + dy.abs()))
            });
            // Keep two pixels away from fan-edge ownership/coverage
            // differences; every interior shade must match exactly.
            if edges.iter().all(|&(c, margin)| c > margin)
                || edges.iter().all(|&(c, margin)| c < -margin)
            {
                interior += 1;
                assert_eq!(
                    gpu,
                    cpu,
                    "clipped-polygon shade differs at ({}, {}), far_x={far_x}",
                    i % 512,
                    i / 512,
                );
            }
        }
        assert!(interior > 1000, "must compare a substantial clipped face");
    }
}

/// Packed shade quantization must not turn a sparse transparent mip into
/// opaque black. A genuinely opaque texel that shades to black still draws.
#[test]
fn zero_shaded_transparent_mip_preserves_background() {
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skipping transparent mip readback");
        return;
    };
    let mut background = textured_triangle([-16, -16, 16], [-16, 16, -16], [0; 3], [0; 3]);
    background.face_colour = Some(vec![17]);
    for transparent in [true, false] {
        let mut pix = solid_texture(false, if transparent { 0 } else { 0x080000 });
        pix.textures[17] = Some(client::graphics::Pix8::new(128, 128, vec![0x2040ff]));
        pix.tex_pal[17] = Some(vec![0x2040ff]);
        if transparent {
            pix.tex_pal[24] = Some(vec![0, 0x101000]);
            let texture = pix.textures[24].as_mut().unwrap();
            for y in (0..128).step_by(2) {
                for x in (0..128).step_by(2) {
                    texture.data[y * 128 + x] = 1;
                }
            }
        }
        pix.init_pool(2);
        let (_, mesh) = triangle_pixels(&background, &mut pix, (0, 0, 512));
        let behind = backend.render_scene_for_test(&mesh, &pix);
        let mut layered = background.clone();
        layered.num_faces = 2;
        layered.face_vertex_a = Some(vec![0, 0]);
        layered.face_vertex_b = Some(vec![1, 1]);
        layered.face_vertex_c = Some(vec![2, 2]);
        layered.face_render_type = Some(vec![2, 2]);
        layered.face_colour = Some(vec![17, 24]);
        let shades = vec![0, if transparent { 0 } else { 32 }];
        layered.face_colour_a = Some(shades.clone());
        layered.face_colour_b = Some(shades.clone());
        layered.face_colour_c = Some(shades);
        let (_, mesh) = triangle_pixels(&layered, &mut pix, (0, 0, 512));
        let gpu = backend.render_scene_for_test(&mesh, &pix);
        assert!(behind.iter().filter(|&&p| p != 0).count() > 100);
        if transparent {
            let changed = gpu.iter().zip(&behind).filter(|(a, b)| a != b).count();
            assert_eq!(
                changed, 0,
                "zero transparent mip must reveal the background"
            );
        } else {
            for (i, &pixel) in behind.iter().enumerate() {
                if pixel != 0 {
                    assert_eq!(gpu[i], 0, "opaque zero shade must cover the background");
                }
            }
        }
    }
}

#[test]
fn textured_lighting_survives_vertex_buffer_growth() {
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skipping vertex buffer growth readback");
        return;
    };
    let model = textured_triangle(
        [-192, -384, 384],
        [-103, 226, -206],
        [-256, 256, 256],
        [0, 64, 127],
    );
    let padding = textured_triangle([-2, -2, 2], [-2, 2, -2], [0; 3], [127; 3]);
    let mut pix = solid_texture(false, 0xffffff);
    let (_, reference) = triangle_pixels(&model, &mut pix, (0, 0, 768));
    let expected = backend.render_scene_for_test(&reference, &pix);
    let mut pixels = vec![0; 512 * 334];
    let mut surface = Pix2D::with_pixels(&mut pixels, 512, 334);
    pix.capture = Some(SceneMesh::default());
    // More than 1 MiB of vertices forces replacement of the streaming
    // buffer. The final lit triangle covers the tiny padding primitives.
    for i in 0..=15_000 {
        let face = if i == 15_000 { &model } else { &padding };
        face.world_render(&mut pix, &mut surface, 0, 0, 65536, 0, 65536, 0, 0, 768, 0);
    }
    let mesh = pix.capture.take().unwrap();
    assert!(std::mem::size_of_val(mesh.vertices()) > 1 << 20);
    let actual = backend.render_scene_for_test(&mesh, &pix);
    let changed = actual.iter().zip(&expected).filter(|(a, b)| a != b).count();
    assert_eq!(
        changed, 0,
        "grown buffer must supply the new triangle shades"
    );
}
