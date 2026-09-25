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

/// Depth must not move the CPU's eight-pixel lighting bands. A solid texel
/// removes UV/filtering from the comparison; the shade varies on both axes.
#[test]
fn textured_lighting_matches_cpu_scanline_bands() {
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skipping textured lighting readback");
        return;
    };
    let mut model = Model {
        num_points: 3,
        point_x: Some(vec![-192, -384, 384]),
        point_y: Some(vec![-103, 226, -206]),
        point_z: Some(vec![-256, 256, 256]),
        num_faces: 1,
        num_t: 1,
        face_vertex_a: Some(vec![0]),
        face_vertex_b: Some(vec![1]),
        face_vertex_c: Some(vec![2]),
        face_render_type: Some(vec![2]),
        face_colour: Some(vec![24]),
        face_colour_a: Some(vec![0]),
        face_colour_b: Some(vec![64]),
        face_colour_c: Some(vec![127]),
        face_texture_p: Some(vec![0]),
        face_texture_m: Some(vec![1]),
        face_texture_n: Some(vec![2]),
        ..Default::default()
    };
    model.calc_bounding_cylinder();
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
        let mut pix = Pix3DDraw::default();
        pix.low_mem = low_mem;
        let size = if low_mem { 64 } else { 128 };
        pix.textures[24] = Some(client::graphics::Pix8::new(size, size, vec![colour]));
        pix.tex_pal[24] = Some(vec![colour]);
        pix.init_pool(1);
        let mut cpu = vec![0; 512 * 334];
        let mut surface = Pix2D::with_pixels(&mut cpu, 512, 334);
        pix.set_render_clipping(&surface);
        model.world_render(
            &mut pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            offset_x,
            offset_y,
            768,
            0,
        );
        pix.capture = Some(SceneMesh::default());
        model.world_render(
            &mut pix,
            &mut surface,
            0,
            0,
            65536,
            0,
            65536,
            offset_x,
            offset_y,
            768,
            0,
        );
        let mesh = pix.capture.take().unwrap();
        let gpu = backend.render_scene_for_test(&mesh, &pix);
        let mut different = 0;
        let mut common = 0;
        for (&cpu, &gpu) in cpu.iter().zip(&gpu) {
            if cpu != 0 && gpu != 0 {
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
