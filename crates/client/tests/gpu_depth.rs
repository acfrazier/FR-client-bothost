//! Phase-1 probe: does the GPU depth buffer occlude a bank-booth model
//! that sits *flush* against a bank-wall model (the live Varrock layout)?
//!
//! The previous synthetic (real 2212 one full tile in front of real 1270)
//! showed 0 booth pixels — depth works across a gap. This binary checks
//! the coincident-face case the live scene actually has.
//!
//! `cargo test -p client --test gpu_depth -- --nocapture`

use std::path::Path;

use client::config::Cache;
use client::core::World;
use client::dash3d::{LocAngle, LocShape, Model, SceneModel, TerrainOverlayShape};
use client::graphics::{Pix2D, Pix3D, Pix3DDraw, PixMap};
use client::io::JagFile;
use client::render::backend::GpuBackend;
use client::render::world::SceneMesh;
use client::render::RenderWorld;
use client::unpack::version_hash;

const SHADE: i32 = 200 * 128 + 100;
/// Saturated red / green so a read-back can count them without a palette.
const BOOTH_RED: i32 = (7 << 7) | 100;
const WALL_GREEN: i32 = (21 << 10) | (7 << 7) | 100;

const WALL_MODEL: i32 = 2212; // loc 1602
const BOOTH_MODEL: i32 = 1270; // loc 2213
const SCENE_W: usize = 512;
const SCENE_H: usize = 334;

fn cache_dir() -> Option<String> {
    let _home = std::env::var("HOME").ok()?;
    let cache = client::cache_dir().display().to_string();
    Path::new(&cache)
        .join("versionlist")
        .is_file()
        .then_some(cache)
}

fn load_snapshot_models(pack: &str) -> Option<()> {
    let home = std::env::var("HOME").ok()?;
    let versionlist = std::fs::read(format!("{pack}/versionlist")).ok()?;
    let version = version_hash(&versionlist);
    let models_bin = format!("{home}/.274bot/unpack/{version}/models.bin");
    let bytes = std::fs::read(&models_bin).ok()?;
    let mut pos = 0usize;
    while pos + 8 <= bytes.len() {
        let id = u32::from_le_bytes(bytes[pos..pos + 4].try_into().ok()?) as i32;
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        pos += 8;
        Model::unpack(id, Some(&bytes[pos..pos + len]));
        pos += len;
    }
    Some(())
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

fn flat_world(max_tile: i32) -> World {
    let max_level: i32 = 1;
    let groundh =
        vec![vec![vec![2000i32; max_tile as usize + 1]; max_tile as usize + 1]; max_level as usize];
    let mut world = World::new(groundh, max_tile, max_level, max_tile);
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
    world
}

fn prep_model(id: i32, angle: i32) -> Model {
    let mut model = Model::load(id).unwrap_or_else(|| panic!("model {id} missing from snapshot"));
    for _ in 0..angle {
        model.rotate90();
    }
    model.calculate_normals(64, 768, -50, -10, -50, true);
    model
}

fn bounds(model: &Model) -> (i32, i32, i32, i32, i32, i32) {
    let xs = model.point_x.as_ref().unwrap();
    let ys = model.point_y.as_ref().unwrap();
    let zs = model.point_z.as_ref().unwrap();
    (
        *xs.iter().min().unwrap(),
        *xs.iter().max().unwrap(),
        *ys.iter().min().unwrap(),
        *ys.iter().max().unwrap(),
        *zs.iter().min().unwrap(),
        *zs.iter().max().unwrap(),
    )
}

fn paint_flat(model: &mut Model, shade: i32) {
    let n = model.num_faces as usize;
    if let Some(rt) = model.face_render_type.as_mut() {
        for t in rt.iter_mut() {
            if *t != -1 {
                *t = 1;
            }
        }
    } else {
        model.face_render_type = Some(vec![1; n]);
    }
    model.face_colour_a = Some(vec![shade; n]);
    model.face_colour_b = Some(vec![shade; n]);
    model.face_colour_c = Some(vec![shade; n]);
    model.face_alpha = None;
}

fn dump_model(tag: &str, model: &Model) {
    let (xmin, xmax, ymin, ymax, zmin, zmax) = bounds(model);
    eprintln!(
        "{tag}: points={} faces={} x=[{xmin},{xmax}] y=[{ymin},{ymax}] z=[{zmin},{zmax}]",
        model.num_points, model.num_faces
    );
    if let Some(fp) = model.face_priority.as_ref() {
        let mut hist = [0u32; 12];
        for &p in fp {
            if (0..12).contains(&p) {
                hist[p as usize] += 1;
            }
        }
        eprintln!(
            "{tag} face_priority hist={hist:?} model.priority={}",
            model.priority
        );
    } else {
        eprintln!("{tag} face_priority=none model.priority={}", model.priority);
    }
    let hidden = model
        .face_render_type
        .as_ref()
        .map(|rt| rt.iter().filter(|&&t| t == -1).count())
        .unwrap_or(0);
    let trans = model
        .face_alpha
        .as_ref()
        .map(|a| a.iter().filter(|&&t| t != 0).count())
        .unwrap_or(0);
    eprintln!("{tag} hidden={hidden} translucent_faces={trans}");
}

fn save_ppm(pixels: &[i32], path: &str) {
    let mut ppm = format!("P6\n{SCENE_W} {SCENE_H}\n255\n").into_bytes();
    for &p in pixels {
        ppm.push(((p >> 16) & 0xff) as u8);
        ppm.push(((p >> 8) & 0xff) as u8);
        ppm.push((p & 0xff) as u8);
    }
    std::fs::write(path, ppm).unwrap();
    eprintln!("wrote {path}");
}

fn count_rgb(scene: &[i32]) -> (usize, usize, usize) {
    let mut red = 0usize;
    let mut green = 0usize;
    let mut other = 0usize;
    for &rgb in scene {
        let r = (rgb >> 16) & 0xff;
        let g = (rgb >> 8) & 0xff;
        let b = rgb & 0xff;
        // Shader gamma 0.6 turns sat-green into ~#b7fdb8 (r stays high);
        // classify by channel dominance, not a tight max-channel cap.
        if g > r + 20 && g > b + 20 && g > 180 {
            green += 1;
        } else if r > g + 20 && r > b + 20 && r > 140 {
            red += 1;
        } else if rgb != 0 {
            other += 1;
        }
    }
    (red, green, other)
}

fn dump_wall_planes(model: &Model) {
    let xs = model.point_x.as_ref().unwrap();
    let ys = model.point_y.as_ref().unwrap();
    let zs = model.point_z.as_ref().unwrap();
    let a = model.face_vertex_a.as_ref().unwrap();
    let b = model.face_vertex_b.as_ref().unwrap();
    let c = model.face_vertex_c.as_ref().unwrap();
    let fp = model.face_priority.as_ref();
    let xmin = *xs.iter().min().unwrap();
    let xmax = *xs.iter().max().unwrap();
    let mut west_w = 0u32;
    let mut west_e = 0u32;
    let mut east_w = 0u32;
    let mut east_e = 0u32;
    let mut other = 0u32;
    for f in 0..model.num_faces as usize {
        let ia = a[f] as usize;
        let ib = b[f] as usize;
        let ic = c[f] as usize;
        let prio = fp.and_then(|p| p.get(f)).copied().unwrap_or(-1);
        let rt = model
            .face_render_type
            .as_ref()
            .and_then(|r| r.get(f))
            .copied()
            .unwrap_or(0);
        let tex = model
            .face_colour
            .as_ref()
            .and_then(|c| c.get(f))
            .copied()
            .unwrap_or(-1);
        // Model-space x-normal: (b-a) × (c-a). Positive nx faces +x (east).
        let _abx = xs[ib] - xs[ia];
        let aby = ys[ib] - ys[ia];
        let abz = zs[ib] - zs[ia];
        let _acx = xs[ic] - xs[ia];
        let acy = ys[ic] - ys[ia];
        let acz = zs[ic] - zs[ia];
        let nx = aby * acz - abz * acy;
        let fx = [xs[ia], xs[ib], xs[ic]];
        let faces_east = nx > 0;
        if fx.iter().all(|&x| x == xmin) {
            if faces_east {
                west_e += 1;
            } else {
                west_w += 1;
            }
            eprintln!(
                "  west-plane f{f} prio={prio} rt={rt} tex={tex} nx={nx} faces={}",
                if faces_east { "east" } else { "west" }
            );
        } else if fx.iter().all(|&x| x == xmax) {
            if faces_east {
                east_e += 1;
            } else {
                east_w += 1;
            }
            eprintln!(
                "  east-plane f{f} prio={prio} nx={nx} faces={}",
                if faces_east { "east" } else { "west" }
            );
        } else {
            other += 1;
        }
    }
    eprintln!(
        "wall planes: west-facing-west={west_w} west-facing-east={west_e} east-facing-west={east_w} east-facing-east={east_e} other={other}"
    );
    // Centroid-x of every face, bucketed, so we can see whether prio-11
    // timber sits in front of prio-3 plaster.
    for f in 0..model.num_faces as usize {
        let ia = a[f] as usize;
        let ib = b[f] as usize;
        let ic = c[f] as usize;
        let cx = (xs[ia] + xs[ib] + xs[ic]) / 3;
        let prio = fp.and_then(|p| p.get(f)).copied().unwrap_or(-1);
        if prio == 11 || (f < 6) {
            eprintln!("  centroid f{f} prio={prio} x={cx}");
        }
    }
    let mut prio11_x = Vec::new();
    let mut prio3_x = Vec::new();
    for f in 0..model.num_faces as usize {
        let ia = a[f] as usize;
        let ib = b[f] as usize;
        let ic = c[f] as usize;
        let cx = (xs[ia] + xs[ib] + xs[ic]) / 3;
        let prio = fp.and_then(|p| p.get(f)).copied().unwrap_or(-1);
        if prio == 11 {
            prio11_x.push(cx);
        } else if prio == 3 {
            prio3_x.push(cx);
        }
    }
    prio11_x.sort();
    prio3_x.sort();
    eprintln!("prio11 centroid-x={prio11_x:?}");
    eprintln!(
        "prio3 centroid-x min/max={:?}/{:?} n={}",
        prio3_x.first(),
        prio3_x.last(),
        prio3_x.len()
    );
}

fn shade_z_range(mesh: &SceneMesh, shade: i32) -> (f32, f32, usize) {
    let mut min_z = f32::MAX;
    let mut max_z = f32::MIN;
    let mut n = 0usize;
    for v in mesh.clone().vertices() {
        if (v.abhsl & 0xffff) as i32 != shade {
            continue;
        }
        n += 1;
        min_z = min_z.min(v.z);
        max_z = max_z.max(v.z);
    }
    (min_z, max_z, n)
}

fn scene_typecode(loc_id: i32) -> i32 {
    0x4000_0000i32 | (loc_id << 14)
}

struct Scene {
    world: World,
    rw: RenderWorld,
}

fn make_scene(booth_tile_x: i32, include_wall: bool, include_booth: bool) -> Scene {
    let wall_tile = 2i32;
    let z_tile = 2i32;
    let mut world = flat_world(6);
    if include_wall {
        world.set_wall(
            0,
            wall_tile,
            z_tile,
            2000,
            4,
            0,
            scene_typecode(1602),
            0,
            2000,
            2000,
            2000,
            2000,
        );
    }
    if include_booth {
        assert!(world.add_scenery(
            0,
            booth_tile_x,
            z_tile,
            2000,
            scene_typecode(2213),
            (LocAngle::EAST << 6) + 10,
            1,
            1,
            0,
            2000,
            2000,
            2000,
            2000,
        ));
    }
    let mut rw = RenderWorld::new();
    if include_wall {
        let mut wall = prep_model(WALL_MODEL, LocAngle::EAST);
        paint_flat(&mut wall, WALL_GREEN);
        rw.set_wall_model(
            &world,
            0,
            wall_tile,
            z_tile,
            Some(SceneModel::Model(wall)),
            None,
        );
    }
    if include_booth {
        let sprite_index = world.last_sprite_index().expect("booth sprite");
        let mut booth = prep_model(BOOTH_MODEL, LocAngle::EAST);
        paint_flat(&mut booth, BOOTH_RED);
        rw.set_sprite_model(&world, sprite_index, Some(SceneModel::Model(booth)));
    }
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    Scene { world, rw }
}

#[allow(clippy::too_many_arguments)]
fn gpu_render(
    backend: &mut GpuBackend,
    scene: &mut Scene,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    yaw: i32,
    pitch: i32,
    label: &str,
) -> (usize, usize, usize) {
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    scene.rw.prepare_scene(
        &mut scene.world,
        &Cache::default(),
        0,
        eye_x,
        eye_y,
        eye_z,
        3,
        yaw,
        pitch,
    );
    let mesh = scene
        .rw
        .build_scene_mesh(&mut scene.world, &Cache::default(), 0, &mut pix);
    let (wmin, wmax, wn) = shade_z_range(&mesh, WALL_GREEN);
    let (bmin, bmax, bn) = shade_z_range(&mesh, BOOTH_RED);
    eprintln!("{label} mesh wall verts={wn} z=[{wmin},{wmax}] booth verts={bn} z=[{bmin},{bmax}]");
    let pixels = backend.render_scene_for_test(mesh, &pix);
    save_ppm(&pixels, &format!("/tmp/gpu_depth_{label}.ppm"));
    let counts = count_rgb(&pixels);
    eprintln!(
        "{label} pixels: booth_red={} wall_green={} other_nonzero={}",
        counts.0, counts.1, counts.2
    );
    counts
}

/// Flush placement (live bank): wall east-edge of tile 2, booth on tile 3.
/// Separated control: booth on tile 4 (one extra tile of air).
#[test]
fn flush_booth_occluded_by_wall() {
    Pix3D::init_colour_table(0.6);
    let Some(pack) = cache_dir() else {
        eprintln!("config jag missing; skip");
        return;
    };
    if load_snapshot_models(&pack).is_none() {
        eprintln!("274 snapshot models.bin missing; skip");
        return;
    }

    if let Ok(bytes) = std::fs::read(format!("{pack}/config")) {
        let cache = Cache::unpack(&JagFile::new(bytes));
        for id in [1602usize, 2213, 2214, 2215] {
            if id >= cache.locs.len() {
                continue;
            }
            let loc = cache.loc(id);
            eprintln!(
                "loc {id} name={:?} sharelight={} hillskew={} occlude={} models={:?} shapes={:?} ambient={} contrast={} wallwidth={} offset=({},{},{}) resize=({},{},{})",
                loc.name, loc.sharelight, loc.hillskew, loc.occlude, loc.model, loc.shape, loc.ambient, loc.contrast, loc.wallwidth,
                loc.offsetx, loc.offsety, loc.offsetz, loc.resizex, loc.resizey, loc.resizez
            );
        }
    }

    let wall = prep_model(WALL_MODEL, LocAngle::EAST);
    dump_model("wall", &wall);
    dump_wall_planes(&wall);
    let booth = prep_model(BOOTH_MODEL, LocAngle::EAST);
    dump_model("booth", &booth);
    let wall_origin_x = 2 * 128 + 64;
    let booth_origin_x = 3 * 128 + 64;
    let (_, wall_xmax, _, _, _, _) = bounds(&wall);
    let (booth_xmin, _, _, _, _, _) = bounds(&booth);
    eprintln!(
        "flush gap: wall_east={} booth_west={} gap={}",
        wall_origin_x + wall_xmax,
        booth_origin_x + booth_xmin,
        (booth_origin_x + booth_xmin) - (wall_origin_x + wall_xmax)
    );

    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skip GPU readback");
        return;
    };

    // Control: booth alone must actually paint red, otherwise the colour
    // threshold is lying.
    let mut booth_only = make_scene(3, false, true);
    let booth_ctrl = gpu_render(
        &mut backend,
        &mut booth_only,
        192,
        1950,
        320,
        1536,
        128,
        "booth_only",
    );
    assert!(
        booth_ctrl.0 > 100,
        "booth-only control must draw red (got {} red px)",
        booth_ctrl.0
    );

    // Dead-on east-looking: the previous agent already showed a 128-unit
    // gap occludes. Confirm flush at the same camera.
    let mut sep = make_scene(4, true, true);
    let sep_c = gpu_render(
        &mut backend,
        &mut sep,
        192,
        1950,
        320,
        1536,
        128,
        "separated",
    );
    let mut flush = make_scene(3, true, true);
    let flush_c = gpu_render(
        &mut backend,
        &mut flush,
        192,
        1950,
        320,
        1536,
        128,
        "flush_headon",
    );
    eprintln!(
        "head-on RESULT separated red={} green={} | flush red={} green={}",
        sep_c.0, sep_c.1, flush_c.0, flush_c.1
    );

    // Live-like: stand southwest of the wall and sweep yaw. The live
    // poke-through is an angled view of the west plaster, not head-on.
    eprintln!("--- yaw sweep, camera (192,1880,192), flush ---");
    for yaw in [0, 256, 512, 768, 1024, 1280, 1536, 1792] {
        let mut s = make_scene(3, true, true);
        let c = gpu_render(
            &mut backend,
            &mut s,
            192,
            1880,
            192,
            yaw,
            160,
            &format!("flush_yaw{yaw}"),
        );
        eprintln!("yaw {yaw}: red={} green={}", c.0, c.1);
    }

    // Interior poke-through = red pixels in the combined render that were
    // green in the wall-only render. Red that appears only around the
    // silhouette is "seeing around the edge", not the live plaster bug.
    for (tag, eye_x, eye_y, eye_z, yaw, pitch) in [
        ("headon", 192, 1950, 320, 1536, 128),
        ("sw_edge", 192, 1880, 192, 1536, 160),
        ("front_south", 128, 1850, 280, 1536, 180),
        ("front_closer", 80, 1800, 320, 1536, 200),
        ("liveish", 64, 1750, 240, 1600, 180),
    ] {
        overlap_test(&mut backend, tag, eye_x, eye_y, eye_z, yaw, pitch);
    }
}

fn render_pixels(
    backend: &mut GpuBackend,
    scene: &mut Scene,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    yaw: i32,
    pitch: i32,
) -> Vec<i32> {
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    scene.rw.prepare_scene(
        &mut scene.world,
        &Cache::default(),
        0,
        eye_x,
        eye_y,
        eye_z,
        3,
        yaw,
        pitch,
    );
    let mesh = scene
        .rw
        .build_scene_mesh(&mut scene.world, &Cache::default(), 0, &mut pix);
    backend.render_scene_for_test(mesh, &pix)
}

fn is_green(rgb: i32) -> bool {
    let r = (rgb >> 16) & 0xff;
    let g = (rgb >> 8) & 0xff;
    let b = rgb & 0xff;
    g > r + 20 && g > b + 20 && g > 180
}

fn is_red(rgb: i32) -> bool {
    let r = (rgb >> 16) & 0xff;
    let g = (rgb >> 8) & 0xff;
    let b = rgb & 0xff;
    r > g + 20 && r > b + 20 && r > 140
}

fn overlap_test(
    backend: &mut GpuBackend,
    tag: &str,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    yaw: i32,
    pitch: i32,
) {
    let mut wall_only = make_scene(3, true, false);
    let mut booth_only = make_scene(3, false, true);
    let mut both = make_scene(3, true, true);
    let wall = render_pixels(backend, &mut wall_only, eye_x, eye_y, eye_z, yaw, pitch);
    let booth = render_pixels(backend, &mut booth_only, eye_x, eye_y, eye_z, yaw, pitch);
    let comb = render_pixels(backend, &mut both, eye_x, eye_y, eye_z, yaw, pitch);
    save_ppm(&comb, &format!("/tmp/gpu_depth_overlap_{tag}.ppm"));
    let mut poke = 0usize;
    let mut around = 0usize;
    let mut wall_px = 0usize;
    let mut booth_px = 0usize;
    for i in 0..comb.len() {
        if is_green(wall[i]) {
            wall_px += 1;
        }
        if is_red(booth[i]) {
            booth_px += 1;
        }
        if is_red(comb[i]) {
            if is_green(wall[i]) {
                poke += 1;
            } else {
                around += 1;
            }
        }
    }
    eprintln!(
        "OVERLAP {tag} cam=({eye_x},{eye_y},{eye_z}) yaw={yaw}: wall_px={wall_px} booth_px={booth_px} poke_through={poke} around_edge={around}"
    );
}

fn wall_typecode2() -> i32 {
    (LocAngle::EAST << 6) + LocShape::WALL_STRAIGHT
}

fn booth_typecode2() -> i32 {
    (LocAngle::EAST << 6) + LocShape::CENTREPIECE_STRAIGHT
}

fn place_wall_run(world: &mut World, z_tiles: std::ops::RangeInclusive<i32>, booth: bool) {
    for z in z_tiles {
        world.set_wall(
            0,
            2,
            z,
            2000,
            4,
            0,
            scene_typecode(1602),
            wall_typecode2(),
            2000,
            2000,
            2000,
            2000,
        );
    }
    if booth {
        assert!(world.add_scenery(
            0,
            3,
            2,
            2000,
            scene_typecode(2213),
            booth_typecode2(),
            1,
            1,
            0,
            2000,
            2000,
            2000,
            2000,
        ));
    }
    world.share_light_pending = true;
}

fn dump_hidden(tag: &str, model: &Model) {
    let hidden = model
        .face_render_type
        .as_ref()
        .map(|rt| rt.iter().filter(|&&t| t == -1).count())
        .unwrap_or(0);
    eprintln!("{tag}: faces={} hidden={hidden}", model.num_faces);
    let Some(rt) = model.face_render_type.as_ref() else {
        return;
    };
    let xs = model.point_x.as_ref().unwrap();
    let a = model.face_vertex_a.as_ref().unwrap();
    let b = model.face_vertex_b.as_ref().unwrap();
    let c = model.face_vertex_c.as_ref().unwrap();
    let fp = model.face_priority.as_ref();
    let xmin = *xs.iter().min().unwrap();
    let xmax = *xs.iter().max().unwrap();
    let mut west = 0u32;
    let mut east = 0u32;
    let mut other = 0u32;
    for f in 0..model.num_faces as usize {
        if rt[f] != -1 {
            continue;
        }
        let fx = [xs[a[f] as usize], xs[b[f] as usize], xs[c[f] as usize]];
        let prio = fp.and_then(|p| p.get(f)).copied().unwrap_or(-1);
        let cx = (fx[0] + fx[1] + fx[2]) / 3;
        if fx.iter().all(|&x| x == xmin) {
            west += 1;
            eprintln!("  HIDDEN west-plane f{f} prio={prio} centroid_x={cx}");
        } else if fx.iter().all(|&x| x == xmax) {
            east += 1;
            eprintln!("  HIDDEN east-plane f{f} prio={prio} centroid_x={cx}");
        } else {
            other += 1;
            eprintln!("  HIDDEN other f{f} prio={prio} centroid_x={cx} xs={fx:?}");
        }
    }
    eprintln!("{tag} hidden planes: west={west} east={east} other={other}");
}

fn textured_pix(pack: &str) -> Pix3DDraw {
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    if let Ok(bytes) = std::fs::read(format!("{pack}/textures")) {
        pix.unpack_textures(&JagFile::new(bytes));
        pix.init_pool(20);
        pix.init_texture_palettes(0.6);
    }
    pix
}

#[allow(clippy::too_many_arguments)]
fn cpu_render(
    scene: &mut Scene,
    cache: &Cache,
    pix: &mut Pix3DDraw,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    yaw: i32,
    pitch: i32,
    label: &str,
) -> Vec<i32> {
    let mut map = PixMap::new(512, 334);
    {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        pix.set_render_clipping(&surface);
        scene.rw.render_all(
            &mut scene.world,
            pix,
            &mut surface,
            cache,
            0,
            eye_x,
            eye_y,
            eye_z,
            3,
            yaw,
            pitch,
        );
    }
    save_ppm(&map.pixels, &format!("/tmp/gpu_depth_cpu_{label}.ppm"));
    map.pixels
}

#[allow(clippy::too_many_arguments)]
fn gpu_render_cache(
    backend: &mut GpuBackend,
    scene: &mut Scene,
    cache: &Cache,
    pix: &mut Pix3DDraw,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    yaw: i32,
    pitch: i32,
    label: &str,
) -> Vec<i32> {
    scene.rw.prepare_scene(
        &mut scene.world,
        cache,
        0,
        eye_x,
        eye_y,
        eye_z,
        3,
        yaw,
        pitch,
    );
    let mesh = scene.rw.build_scene_mesh(&mut scene.world, cache, 0, pix);
    let pixels = backend.render_scene_for_test(mesh, pix);
    save_ppm(&pixels, &format!("/tmp/gpu_depth_gpu_{label}.ppm"));
    pixels
}

/// Production path: loc 1602 (sharelight) + loc 2213 resolved from the
/// config jag, share-light pending like `finishBuild`, then CPU vs GPU at
/// the same camera.
#[test]
fn sharelight_wall_run_cpu_vs_gpu() {
    Pix3D::init_colour_table(0.6);
    let Some(pack) = cache_dir() else {
        eprintln!("config jag missing; skip");
        return;
    };
    if load_snapshot_models(&pack).is_none() {
        eprintln!("274 snapshot models.bin missing; skip");
        return;
    }
    let Ok(bytes) = std::fs::read(format!("{pack}/config")) else {
        eprintln!("config jag unreadable; skip");
        return;
    };
    let cache = Cache::unpack(&JagFile::new(bytes));
    {
        let loc = cache.loc(1602);
        eprintln!(
            "loc 1602 sharelight={} hillskew={} wallwidth={} offset=({},{},{}) resize=({},{},{}) raiseobject={}",
            loc.sharelight, loc.hillskew, loc.wallwidth, loc.offsetx, loc.offsety, loc.offsetz,
            loc.resizex, loc.resizey, loc.resizez, loc.raiseobject
        );
    }
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skip");
        return;
    };

    for (tag, z_lo, z_hi, booth) in [
        ("one_wall", 2, 2, true),
        ("five_wall", 2, 6, true),
        ("five_wall_nobooth", 2, 6, false),
    ] {
        let mut world = flat_world(8);
        place_wall_run(&mut world, z_lo..=z_hi, booth);
        let mut rw = RenderWorld::new();
        rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
        let mut pix = textured_pix(&pack);
        rw.prepare_scene(&mut world, &cache, 0, 192, 1950, 320, 3, 1536, 128);
        for z in z_lo..=z_hi {
            if let Some(SceneModel::Model(model)) = rw.wall_model1(&world, &cache, 0, 0, 2, z) {
                dump_hidden(&format!("{tag} wall z={z}"), model);
            } else {
                eprintln!("{tag} wall z={z}: no model");
            }
        }
        let mut scene = Scene { world, rw };
        let gpu = gpu_render_cache(
            &mut backend,
            &mut scene,
            &cache,
            &mut pix,
            192,
            1950,
            320,
            1536,
            128,
            tag,
        );

        // Fresh world for the CPU oracle (GPU emit stamps sprite.cycle).
        let mut world = flat_world(8);
        place_wall_run(&mut world, z_lo..=z_hi, booth);
        let mut rw = RenderWorld::new();
        rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
        let mut scene = Scene { world, rw };
        let cpu = cpu_render(&mut scene, &cache, &mut pix, 192, 1950, 320, 1536, 128, tag);

        let mut differ = 0usize;
        let mut gpu_only = 0usize;
        let mut cpu_only = 0usize;
        for i in 0..gpu.len() {
            let g = gpu[i];
            let c = cpu[i];
            if g == c {
                continue;
            }
            differ += 1;
            let gr = (g >> 16) & 0xff;
            let gg = (g >> 8) & 0xff;
            let gb = g & 0xff;
            let cr = (c >> 16) & 0xff;
            let cg = (c >> 8) & 0xff;
            let cb = c & 0xff;
            if (gr - cr).abs() + (gg - cg).abs() + (gb - cb).abs() > 40 {
                if g != 0 {
                    gpu_only += 1;
                } else {
                    cpu_only += 1;
                }
            }
        }
        eprintln!(
            "DIFF {tag}: changed={differ}/{} strong={gpu_only} cpu_nonzero_gpu_zero_ish={cpu_only}",
            gpu.len()
        );
    }

    // Angled / farther cameras of the five-wall+booth layout — the live
    // poke-through is an exterior view of a plaster panel, not head-on at
    // 176 units.
    eprintln!("--- five_wall+booth camera sweep (original models) ---");
    for (tag, eye_x, eye_y, eye_z, yaw, pitch) in [
        ("front_south", 128, 1850, 280, 1536, 180),
        ("liveish", 64, 1750, 240, 1600, 180),
        ("sw_edge", 192, 1880, 192, 1536, 160),
        ("street", 0, 1700, 320, 1536, 200),
        ("street_south", 0, 1700, 192, 1600, 200),
    ] {
        let mut world = flat_world(8);
        place_wall_run(&mut world, 2..=6, true);
        let mut rw = RenderWorld::new();
        rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
        let mut pix = textured_pix(&pack);
        let mut scene = Scene { world, rw };
        let gpu = gpu_render_cache(
            &mut backend,
            &mut scene,
            &cache,
            &mut pix,
            eye_x,
            eye_y,
            eye_z,
            yaw,
            pitch,
            tag,
        );
        let mut world = flat_world(8);
        place_wall_run(&mut world, 2..=6, true);
        let mut rw = RenderWorld::new();
        rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
        let mut scene = Scene { world, rw };
        let cpu = cpu_render(
            &mut scene, &cache, &mut pix, eye_x, eye_y, eye_z, yaw, pitch, tag,
        );
        let mut strong = 0usize;
        for i in 0..gpu.len() {
            let g = gpu[i];
            let c = cpu[i];
            if g == c {
                continue;
            }
            let dr = ((g >> 16) & 0xff) - ((c >> 16) & 0xff);
            let dg = ((g >> 8) & 0xff) - ((c >> 8) & 0xff);
            let db = (g & 0xff) - (c & 0xff);
            if dr.abs() + dg.abs() + db.abs() > 40 {
                strong += 1;
            }
        }
        eprintln!("DIFF {tag} cam=({eye_x},{eye_y},{eye_z}) yaw={yaw}: strong={strong}");
    }

    // Same-tile south wall + booth: the booth's z[-56,56] overlaps the
    // south-edge wall's inner 8 units (wall z[-64,-48] after SOUTH rotate).
    eprintln!("--- same-tile SOUTH wall + booth ---");
    let mut world = flat_world(8);
    world.set_wall(
        0,
        2,
        2,
        2000,
        8, // WSHAPE0[SOUTH]
        0,
        scene_typecode(1602),
        (LocAngle::SOUTH << 6) + LocShape::WALL_STRAIGHT,
        2000,
        2000,
        2000,
        2000,
    );
    assert!(world.add_scenery(
        0,
        2,
        2,
        2000,
        scene_typecode(2213),
        (LocAngle::EAST << 6) + LocShape::CENTREPIECE_STRAIGHT,
        1,
        1,
        0,
        2000,
        2000,
        2000,
        2000,
    ));
    world.share_light_pending = true;
    let wall_m = cache
        .loc(1602)
        .get_model(
            &cache,
            LocShape::WALL_STRAIGHT,
            LocAngle::SOUTH,
            2000,
            2000,
            2000,
            2000,
            -1,
        )
        .expect("south wall model");
    let booth_m = cache
        .loc(2213)
        .get_model(
            &cache,
            LocShape::CENTREPIECE_STRAIGHT,
            LocAngle::EAST,
            2000,
            2000,
            2000,
            2000,
            -1,
        )
        .expect("booth model");
    dump_model("south-wall", &wall_m);
    dump_model("south-booth", &booth_m);
    let (_, _, _, _, wz0, wz1) = bounds(&wall_m);
    let (bx0, bx1, _, _, bz0, bz1) = bounds(&booth_m);
    eprintln!(
        "south-wall z=[{wz0},{wz1}] booth x=[{bx0},{bx1}] z=[{bz0},{bz1}] overlap_z={}",
        wz1.max(bz0) - wz0.min(bz1)
    );
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = textured_pix(&pack);
    let mut scene = Scene { world, rw };
    // South of the tile, looking north (yaw 0).
    let gpu = gpu_render_cache(
        &mut backend,
        &mut scene,
        &cache,
        &mut pix,
        320,
        1850,
        192,
        0,
        160,
        "south_same",
    );
    let mut world = flat_world(8);
    world.set_wall(
        0,
        2,
        2,
        2000,
        8,
        0,
        scene_typecode(1602),
        (LocAngle::SOUTH << 6) + LocShape::WALL_STRAIGHT,
        2000,
        2000,
        2000,
        2000,
    );
    assert!(world.add_scenery(
        0,
        2,
        2,
        2000,
        scene_typecode(2213),
        (LocAngle::EAST << 6) + LocShape::CENTREPIECE_STRAIGHT,
        1,
        1,
        0,
        2000,
        2000,
        2000,
        2000,
    ));
    world.share_light_pending = true;
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut scene = Scene { world, rw };
    let cpu = cpu_render(
        &mut scene,
        &cache,
        &mut pix,
        320,
        1850,
        192,
        0,
        160,
        "south_same",
    );
    let mut strong = 0usize;
    for i in 0..gpu.len() {
        let g = gpu[i];
        let c = cpu[i];
        let dr = ((g >> 16) & 0xff) - ((c >> 16) & 0xff);
        let dg = ((g >> 8) & 0xff) - ((c >> 8) & 0xff);
        let db = (g & 0xff) - (c & 0xff);
        if dr.abs() + dg.abs() + db.abs() > 40 {
            strong += 1;
        }
    }
    eprintln!("DIFF south_same strong={strong}");
    let _ = wall_m;

    // Control: same camera, wall only. If the gray bands remain, they are
    // wall self-z-fight, not the booth.
    let mut world = flat_world(8);
    world.set_wall(
        0,
        2,
        2,
        2000,
        8,
        0,
        scene_typecode(1602),
        (LocAngle::SOUTH << 6) + LocShape::WALL_STRAIGHT,
        2000,
        2000,
        2000,
        2000,
    );
    world.share_light_pending = true;
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = textured_pix(&pack);
    let mut scene = Scene { world, rw };
    let gpu_w = gpu_render_cache(
        &mut backend,
        &mut scene,
        &cache,
        &mut pix,
        320,
        1850,
        192,
        0,
        160,
        "south_nobooth",
    );
    let mut world = flat_world(8);
    world.set_wall(
        0,
        2,
        2,
        2000,
        8,
        0,
        scene_typecode(1602),
        (LocAngle::SOUTH << 6) + LocShape::WALL_STRAIGHT,
        2000,
        2000,
        2000,
        2000,
    );
    world.share_light_pending = true;
    let mut rw = RenderWorld::new();
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut scene = Scene { world, rw };
    let cpu_w = cpu_render(
        &mut scene,
        &cache,
        &mut pix,
        320,
        1850,
        192,
        0,
        160,
        "south_nobooth",
    );
    let mut strong = 0usize;
    for i in 0..gpu_w.len() {
        let g = gpu_w[i];
        let c = cpu_w[i];
        let dr = ((g >> 16) & 0xff) - ((c >> 16) & 0xff);
        let dg = ((g >> 8) & 0xff) - ((c >> 8) & 0xff);
        let db = (g & 0xff) - (c & 0xff);
        if dr.abs() + dg.abs() + db.abs() > 40 {
            strong += 1;
        }
    }
    eprintln!("DIFF south_nobooth strong={strong}");

    // Recolor the booth red so poke-through is a pixel count, not a vibe.
    let mut world = flat_world(8);
    world.set_wall(
        0,
        2,
        2,
        2000,
        8,
        0,
        scene_typecode(1602),
        (LocAngle::SOUTH << 6) + LocShape::WALL_STRAIGHT,
        2000,
        2000,
        2000,
        2000,
    );
    assert!(world.add_scenery(
        0,
        2,
        2,
        2000,
        scene_typecode(2213),
        (LocAngle::EAST << 6) + LocShape::CENTREPIECE_STRAIGHT,
        1,
        1,
        0,
        2000,
        2000,
        2000,
        2000,
    ));
    let sprite_index = world.last_sprite_index().expect("booth");
    let wall = cache
        .loc(1602)
        .get_model(
            &cache,
            LocShape::WALL_STRAIGHT,
            LocAngle::SOUTH,
            2000,
            2000,
            2000,
            2000,
            -1,
        )
        .expect("wall");
    let mut booth = cache
        .loc(2213)
        .get_model(
            &cache,
            LocShape::CENTREPIECE_STRAIGHT,
            LocAngle::EAST,
            2000,
            2000,
            2000,
            2000,
            -1,
        )
        .expect("booth");
    paint_flat(&mut booth, BOOTH_RED);
    let mut rw = RenderWorld::new();
    rw.set_wall_model(&world, 0, 2, 2, Some(SceneModel::Model(wall)), None);
    rw.set_sprite_model(&world, sprite_index, Some(SceneModel::Model(booth)));
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    let mut pix = Pix3DDraw::default();
    pix.set_clipping(512, 334);
    rw.prepare_scene(&mut world, &Cache::default(), 0, 320, 1850, 192, 3, 0, 160);
    let mesh = rw.build_scene_mesh(&mut world, &Cache::default(), 0, &mut pix);
    let gpu_red = backend.render_scene_for_test(mesh, &pix);
    save_ppm(&gpu_red, "/tmp/gpu_depth_gpu_south_red.ppm");
    let (red, green, other) = count_rgb(&gpu_red);
    eprintln!("south_red GPU pixels: booth_red={red} wall_green={green} other_nonzero={other}");
}

/// Live Varrock west facade: loc 1602 WALL_STRAIGHT angle=WEST and the
/// bank booth CENTREPIECE angle=EAST on the **same tile**. The booth's
/// x[-64,64] fills the wall's x[-64,-48] thickness; CPU plaster covers
/// the booth, GPU currently lets the booth poke through the panel.
fn make_same_tile_west(include_wall: bool, include_booth: bool) -> Scene {
    let tile_x = 2i32;
    let tile_z = 2i32;
    let mut world = flat_world(6);
    if include_wall {
        world.set_wall(
            0,
            tile_x,
            tile_z,
            2000,
            1, // WSHAPE0[WEST]
            0,
            scene_typecode(1602),
            LocAngle::WEST,
            2000,
            2000,
            2000,
            2000,
        );
    }
    if include_booth {
        assert!(world.add_scenery(
            0,
            tile_x,
            tile_z,
            2000,
            scene_typecode(2213),
            (LocAngle::EAST << 6) + LocShape::CENTREPIECE_STRAIGHT,
            1,
            1,
            0,
            2000,
            2000,
            2000,
            2000,
        ));
    }
    let mut rw = RenderWorld::new();
    if include_wall {
        let mut wall = prep_model(WALL_MODEL, LocAngle::WEST);
        paint_flat(&mut wall, WALL_GREEN);
        rw.set_wall_model(
            &world,
            0,
            tile_x,
            tile_z,
            Some(SceneModel::Model(wall)),
            None,
        );
    }
    if include_booth {
        let sprite_index = world.last_sprite_index().expect("booth sprite");
        let mut booth = prep_model(BOOTH_MODEL, LocAngle::EAST);
        paint_flat(&mut booth, BOOTH_RED);
        rw.set_sprite_model(&world, sprite_index, Some(SceneModel::Model(booth)));
    }
    rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
    Scene { world, rw }
}

#[allow(clippy::too_many_arguments)]
fn overlap_counts(
    backend: &mut GpuBackend,
    wall_only: &mut Scene,
    booth_only: &mut Scene,
    both: &mut Scene,
    eye_x: i32,
    eye_y: i32,
    eye_z: i32,
    yaw: i32,
    pitch: i32,
    tag: &str,
) -> (usize, usize, usize, usize) {
    let wall = render_pixels(backend, wall_only, eye_x, eye_y, eye_z, yaw, pitch);
    let booth = render_pixels(backend, booth_only, eye_x, eye_y, eye_z, yaw, pitch);
    let comb = render_pixels(backend, both, eye_x, eye_y, eye_z, yaw, pitch);
    save_ppm(&comb, &format!("/tmp/gpu_depth_overlap_{tag}.ppm"));
    let mut poke = 0usize;
    let mut around = 0usize;
    let mut wall_px = 0usize;
    let mut booth_px = 0usize;
    for i in 0..comb.len() {
        if is_green(wall[i]) {
            wall_px += 1;
        }
        if is_red(booth[i]) {
            booth_px += 1;
        }
        if is_red(comb[i]) {
            if is_green(wall[i]) {
                poke += 1;
            } else {
                around += 1;
            }
        }
    }
    eprintln!(
        "OVERLAP {tag}: wall_px={wall_px} booth_px={booth_px} poke_through={poke} around_edge={around}"
    );
    (wall_px, booth_px, poke, around)
}

/// Same-tile WEST wall + EAST booth (the live hanging-sign facade):
/// the wall plaster must occlude the booth on the GPU.
#[test]
fn same_tile_west_wall_occludes_booth() {
    Pix3D::init_colour_table(0.6);
    let Some(pack) = cache_dir() else {
        eprintln!("config jag missing; skip");
        return;
    };
    if load_snapshot_models(&pack).is_none() {
        eprintln!("274 snapshot models.bin missing; skip");
        return;
    }
    let Ok(mut backend) = GpuBackend::try_new() else {
        eprintln!("no adapter; skip");
        return;
    };

    let wall = prep_model(WALL_MODEL, LocAngle::WEST);
    dump_model("west-wall", &wall);
    let booth = prep_model(BOOTH_MODEL, LocAngle::EAST);
    dump_model("east-booth", &booth);
    let (wx0, wx1, _, _, _, _) = bounds(&wall);
    let (bx0, bx1, _, _, _, _) = bounds(&booth);
    eprintln!(
        "same-tile WEST: wall x=[{wx0},{wx1}] booth x=[{bx0},{bx1}] overlap={}",
        wx1.min(bx1) - wx0.max(bx0)
    );
    let _ = wall;
    let _ = booth;

    // West of the tile, looking east — the live street view of the facade.
    let eye = (192, 1950, 320, 1536, 128);
    let mut wall_only = make_same_tile_west(true, false);
    let mut booth_only = make_same_tile_west(false, true);
    let mut both = make_same_tile_west(true, true);
    let (wall_px, booth_px, poke, around) = overlap_counts(
        &mut backend,
        &mut wall_only,
        &mut booth_only,
        &mut both,
        eye.0,
        eye.1,
        eye.2,
        eye.3,
        eye.4,
        "same_tile_west",
    );
    assert!(
        wall_px > 100,
        "wall-only must draw the green plaster (got {wall_px})"
    );
    assert!(
        booth_px > 100,
        "booth-only must draw the red booth (got {booth_px})"
    );
    assert!(
        poke == 0,
        "same-tile WEST plaster must occlude the booth (poke_through={poke} around_edge={around} wall_px={wall_px})"
    );
}
