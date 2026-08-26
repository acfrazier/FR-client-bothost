//! Task 1: one-shot cache unpacker. Snapshot the local 274 cache once into
//! an immutable versioned dir, then verify the record stream is real
//! gunzipped data (not the gzip + 2-byte trailer read off disk).

use std::path::Path;

use client::config::Cache;
use client::core::World;
use client::dash3d::Model;
use client::dash3d::{LocAngle, SceneModel, TerrainOverlayShape};
use client::graphics::{Pix2D, Pix3D, Pix3DDraw, PixMap};
use client::io::JagFile;
use client::render::RenderWorld;
use client::unpack::{unpack_cache, version_hash};

fn cache_dir() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let cache = format!("{home}/experiments/Server/engine/data/pack/client");
    Path::new(&cache)
        .join("versionlist")
        .is_file()
        .then_some(cache)
}

#[test]
fn unpacks_versioned_snapshot() {
    let Some(cache) = cache_dir() else {
        return;
    };

    // Versionlist model count and the idx1 size==0 (never-preserved) count.
    let versionlist = std::fs::read(format!("{cache}/versionlist")).unwrap();
    let jag = JagFile::new(versionlist);
    let model_version = jag.read("model_version").expect("model_version table");
    let model_total = model_version.len() / 2;
    let idx1 = std::fs::read(
        Path::new(&cache)
            .parent()
            .unwrap()
            .join("main_file_cache.idx1"),
    )
    .unwrap();
    let size_zero = idx1
        .chunks(6)
        .filter(|r| r.len() == 6 && ((r[0] as u32) << 16) + ((r[1] as u32) << 8) + r[2] as u32 == 0)
        .count();

    let tmp = std::env::temp_dir().join(format!("274bot-unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let manifest = unpack_cache(&cache, tmp.to_str().unwrap()).unwrap();

    let dir = Path::new(&manifest.dir);
    assert!(dir.join("models.bin").is_file(), "models.bin exists");
    assert!(dir.join("anims.bin").is_file(), "anims.bin exists");
    assert!(dir.join("midi.bin").is_file(), "midi.bin exists");
    assert!(dir.join("maps.bin").is_file(), "maps.bin exists");
    assert!(dir.join("manifest").is_file(), "manifest exists");

    assert_eq!(manifest.models.total as usize, model_total);
    assert_eq!(manifest.models.unpacked as usize, model_total - size_zero);
    assert!(manifest.models.unpacked > 0, "models must unpack");
    assert_eq!(manifest.models.skipped as usize, size_zero);

    // Read one record back: gunzipped bytes are non-empty and no longer
    // start with the gzip magic (proving the strip + gunzip happened).
    let bytes = std::fs::read(dir.join("models.bin")).unwrap();
    assert!(bytes.len() >= 8, "record header present");
    let len = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    assert!(bytes.len() >= 8 + len, "record body present");
    let data = &bytes[8..8 + len];
    assert!(!data.is_empty(), "gunzipped model bytes non-empty");
    assert!(
        !data.starts_with(&[0x1f, 0x8b]),
        "record must be gunzipped, not gzip + trailer"
    );
}

/// Load every model record from the versioned snapshot into the process-wide
/// model store (the boot-inject path; the anim half is irrelevant here).
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

/// The wall-texture load-path check: the Maze wall loc 3626 (models
/// 3693-3697, `sharelight`) renders through the CPU's gouraud/flat path —
/// its faces are `faceRenderType` 0 and 1, none `& 0x3 == 2`, so the CPU
/// `texture_triangle`/`get_texels` path is never consulted for it. What
/// must hold so no *textured* wall is dropped at draw time: every loc
/// model's textured faces reference texture ids within 0..49 (the
/// `get_texels` `id >= 50` guard), and every such id is `Some` in both
/// `textures` and `tex_pal` after the `prepare_game` unpack sequence.
#[test]
fn maze_wall_texture_ids_in_range_and_loaded() {
    let Some(pack) = cache_dir() else {
        return;
    };
    if load_snapshot_models(&pack).is_none() {
        eprintln!("274 snapshot models.bin missing; skip");
        return;
    }

    let config = std::fs::read(format!("{pack}/config")).unwrap();
    let cache = Cache::unpack(&JagFile::new(config));

    // Model 3693 (loc 3626 "Wall") itself: any textured face it had would
    // have to reference a loadable id. The 274 data has none today.
    let model3693 = Model::load(3693).expect("maze wall model 3693 in snapshot");
    let mut missing_ids = std::collections::BTreeSet::new();
    if let (Some(rt), Some(fc)) = (
        model3693.face_render_type.as_ref(),
        model3693.face_colour.as_ref(),
    ) {
        for (&t, &c) in rt.iter().zip(fc.iter()) {
            if t & 0x3 == 2 && (c < 0 || c >= 50) {
                missing_ids.insert(c);
            }
        }
    }
    assert!(
        missing_ids.is_empty(),
        "model 3693 textured faces reference out-of-range texture ids: {missing_ids:?}"
    );

    // Config-wide: no loc model may reference an out-of-range texture id
    // (the CPU `get_texels` returns None for `id >= 50`, dropping the face
    // exactly like Java `getTexels`).
    let mut out_of_range: std::collections::BTreeMap<i32, std::collections::BTreeSet<i32>> =
        std::collections::BTreeMap::new();
    let mut textured_models = 0usize;
    for loc in &cache.locs {
        let Some(models) = &loc.model else { continue };
        for &mid in models {
            let Some(m) = Model::load(mid) else { continue };
            if let (Some(rt), Some(fc)) = (m.face_render_type.as_ref(), m.face_colour.as_ref()) {
                let mut any = false;
                for (&t, &c) in rt.iter().zip(fc.iter()) {
                    if t & 0x3 == 2 {
                        any = true;
                        if c < 0 || c >= 50 {
                            out_of_range.entry(mid).or_default().insert(c);
                        }
                    }
                }
                if any {
                    textured_models += 1;
                }
            }
        }
    }
    assert!(
        textured_models > 0,
        "the 274 config must contain textured loc models for this check to mean anything"
    );
    assert!(
        out_of_range.is_empty(),
        "loc models reference out-of-range texture ids: {out_of_range:?}; the CPU get_texels would drop those faces"
    );

    // The `prepare_game` sequence (`unpack_textures` + `init_pool` +
    // `init_texture_palettes`): every id 0..49 must be Some in both
    // `textures` and `tex_pal`, so the `get_texels` None branch never
    // fires for an in-range id.
    let textures = std::fs::read(format!("{pack}/textures")).unwrap();
    let mut pix = Pix3DDraw::default();
    pix.unpack_textures(&JagFile::new(textures));
    pix.init_pool(20);
    pix.init_texture_palettes(0.8);
    let absent: Vec<i32> = (0..50)
        .filter(|&id| pix.textures[id as usize].is_none() || pix.tex_pal[id as usize].is_none())
        .collect();
    assert!(
        absent.is_empty(),
        "textures absent after unpack_textures/init_texture_palettes: {absent:?}"
    );
}

/// Regression guard for the wall report: the Maze wall (loc 3626, model
/// 3693-3697, untextured gouraud/flat) and a real textured door (loc 3,
/// texture id 0) must both paint pixels through the CPU backend with the
/// standard texture unpack, i.e. neither the gouraud/flat wall path nor
/// the `texture_triangle` path drops them.
#[test]
fn cpu_renders_maze_wall_and_textured_door() {
    let Some(pack) = cache_dir() else {
        return;
    };
    if load_snapshot_models(&pack).is_none() {
        eprintln!("274 snapshot models.bin missing; skip");
        return;
    }
    let config = std::fs::read(format!("{pack}/config")).unwrap();
    let cache = Cache::unpack(&JagFile::new(config));
    let textures = std::fs::read(format!("{pack}/textures")).unwrap();
    let jag = JagFile::new(textures);
    Pix3D::init_colour_table(0.6);

    for (label, loc_id, wall_type, angle) in [
        ("maze wall", 3626, 8, LocAngle::SOUTH),
        ("textured door", 3, 8, LocAngle::SOUTH),
    ] {
        let loc = &cache.locs[loc_id];
        let Some(model) = loc.get_model(&cache, 0, angle, 2000, 2000, 2000, 2000, -1) else {
            panic!("loc {loc_id} shape 0 model must decode");
        };
        let mut world = flat_world();
        let mut rw = RenderWorld::new();
        rw.reset_vis_calc(&game_distance_table(), 500, 800, 512, 334);
        world.set_wall(0, 1, 2, 2000, wall_type, 0, loc_id as i32, 0, 0, 0, 0, 0);
        rw.set_wall_model(&world, 0, 1, 2, Some(SceneModel::Model(model)), None);
        rw.share_light(&mut world, 64, 768, -50, -10, -50);
        let mut pix = textured_pix(&jag);
        let mut map = PixMap::new(512, 334);
        {
            let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
            viewport(&mut pix, &mut surface);
            rw.render_all(&mut world, &mut pix, &mut surface, &cache, 0, 192, 1950, 192, 3, 0, 128);
        }
        let ground = Pix3D::colour_table()[SHADE as usize];
        let n = map
            .pixels
            .iter()
            .filter(|&&p| p != 0 && p != 1 && p != ground)
            .count();
        assert!(
            n > 50,
            "{label} painted {n} CPU pixels; the CPU backend must render it (wall-drop regression)"
        );
    }
}

const SHADE: i32 = 200 * 128 + 100;

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
                0, x, z, TerrainOverlayShape::PLAIN, 0, -1, 0, 0, 0, 0,
                SHADE, SHADE, SHADE, SHADE, SHADE, SHADE, SHADE, SHADE, 0, 0,
            );
        }
    }
    world
}

fn viewport(pix: &mut Pix3DDraw, surface: &mut Pix2D) {
    pix.set_render_clipping(surface);
    pix.trans = 0;
    pix.low_mem = false;
}

fn textured_pix(textures: &JagFile) -> Pix3DDraw {
    let mut pix = Pix3DDraw::default();
    pix.low_mem = false;
    pix.unpack_textures(textures);
    pix.init_pool(20);
    pix.init_texture_palettes(0.8);
    pix
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
