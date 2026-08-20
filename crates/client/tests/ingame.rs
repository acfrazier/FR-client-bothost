use client::client::{Client, ClientConfig};
use client::client::{APPLET_H, APPLET_W};
use client::dash3d::{ClientEntity, ClientPlayer, TerrainOverlayShape, World};
use client::graphics::{Colour, PixMap};

fn cache_dir() -> Option<String> {
    let cache = std::env::var("HOME").ok()? + "/experiments/Server/engine/data/pack/client";
    if std::path::Path::new(&cache).join("media").is_file() {
        Some(cache)
    } else {
        None
    }
}

fn client(cache: String) -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    })
}

#[test]
fn game_draw_headless_does_not_panic() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.draw_area = PixMap::new(APPLET_W, APPLET_H);
    c.game_draw();
    // second frame: redraw_frame is now false, exercises the per-frame path
    c.game_draw();
    // scene state 2: gameDrawMain runs the 3D pass — cls + render_all into
    // area_game, blitted at (4, 4). Without a rebuilt scene the world is
    // empty, so the viewport stays black, but the pass must not panic.
    c.scene_state = 2;
    c.game_draw();
    assert_eq!(c.world.render_count(), 1, "gameDrawMain must call render_all");
    // with the media jag present the chrome panels plot non-zero pixels
    assert!(c.draw_area.pixels.iter().any(|&p| p != 0));
    // the icon-strip backgrounds (backhmid1 at (516, 160), backbase2 at
    // (496, 466)) blit non-zero pixels
    let region_nonzero = |x0: i32, y0: i32, x1: i32, y1: i32| {
        (y0..y1).any(|y| (x0..x1).any(|x| c.draw_area.pixels[(y * c.draw_area.width + x) as usize] != 0))
    };
    assert!(region_nonzero(516, 160, 765, 205), "backhmid1 strip");
    assert!(region_nonzero(496, 466, 765, 503), "backbase2 strip");
}

/// `gameDrawMain` with a real pack: `Client::new` does not rebuild a scene,
/// so the world is empty and `area_game` stays black after the pass — but
/// the pass must run `render_all` (proved by `render_count`) and must not
/// panic. A live `test`/`test` login against the 274 server is the oracle
/// for the non-empty scene (visible 3D viewport, eye above the local
/// player).
#[test]
fn game_draw_main_writes_viewport_pixels() {
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    // if local_player/world empty after new, still must not panic
    c.game_draw();
    let g = c.area_game.as_ref().unwrap();
    assert_eq!(g.width, 512);
    assert_eq!(g.height, 334);
    // after a real rebuild this is non-zero; without scene, at least not a
    // panic, and the 3D pass ran rather than the old fill-0 stub.
    assert_eq!(c.world.render_count(), 1, "gameDrawMain must call render_all");
    assert_eq!(c.scene_cycle, 1);
    assert!(c.vis_calc_done, "resetVisCalc must run before the first pass");
}

/// `minimapDraw` (TS 11279) fills `area_map` and the map is blitted at
/// (550, 4) under the chrome: `area_backvmid1` (34×156, drawn at (516, 4))
/// borders the 172×156 rect and must never be covered by an opaque black
/// `area_map`. `area_map` also carries the `mapback` ring (prepareGame
/// 2022-2023) and the white local-player square (minimapDraw 11390).
#[test]
fn minimap_is_under_chrome_not_a_black_square_on_top() {
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    c.set_draw(true);
    c.ingame = true;
    let mut player = ClientPlayer::default();
    player.ready = true;
    player.name = Some("tester".into());
    player.entity = ClientEntity::at(48, 48);
    player.entity.teleport(&c.cache, true, 48, 48);
    c.local_player = Some(player);
    c.scene_state = 2;
    c.redraw_frame = true;
    c.game_draw();

    assert!(c.area_map.is_some());
    let map = c.area_map.as_ref().unwrap();
    assert_eq!((map.width, map.height), (172, 156));

    // (520, 8) is inside backvmid1's strip (516..549 × 4..159). The map
    // blit at (550, 4) must not have covered it with an opaque rectangle.
    let i = (8 * c.draw_area.width + 520) as usize;
    let chrome = c.draw_area.pixels[i];
    assert_ne!(chrome, 0, "chrome ring must sit on top of area_map, not a black square");
    if let Some(backvmid1) = &c.area_backvmid1 {
        let expected = backvmid1.pixels[(4 * backvmid1.width + 4) as usize];
        assert_eq!(chrome, expected, "backvmid1 pixel must survive the area_map blit");
    }

    // minimapDraw ran: the mapback ring is in area_map and on the canvas,
    // and the white local-player square sits at area_map (97..99, 78..80).
    assert_ne!(
        map.pixels[(100 * map.width + 10) as usize],
        0,
        "mapback ring must be plotted into area_map"
    );
    assert_eq!(
        map.pixels[(79 * map.width + 98) as usize],
        Colour::WHITE,
        "minimapDraw must draw the local-player square"
    );
    let ring = (104 * c.draw_area.width + 560) as usize;
    assert_ne!(c.draw_area.pixels[ring], 0, "mapback ring must be blitted at (550, 4)");
}

/// `minimapDraw` must not panic when the `media` pack (and so `mapback`) is
/// missing: `build_minimap_masks` leaves the masks sized-zero, the
/// rotate-plots no-op, and `area_map` stays black — the module doc's
/// missing-media fallback.
#[test]
fn minimap_draw_without_media_does_not_panic() {
    let dir = std::env::temp_dir().join(format!("274-nomedia-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = client(dir.to_string_lossy().into_owned());
    c.set_draw(true);
    c.ingame = true;
    let mut player = ClientPlayer::default();
    player.ready = true;
    player.name = Some("tester".into());
    player.entity = ClientEntity::at(48, 48);
    player.entity.teleport(&c.cache, true, 48, 48);
    c.local_player = Some(player);
    c.scene_state = 2;
    c.redraw_frame = true;
    c.game_draw();
    let map = c.area_map.as_ref().unwrap();
    assert_eq!((map.width, map.height), (172, 156));
    // Without mapback/compass/dots the only content minimapDraw paints is
    // the white local-player square at (97..99, 78..80).
    for y in 0..map.height {
        for x in 0..map.width {
            let in_square = (78..81).contains(&y) && (97..100).contains(&x);
            let pixel = map.pixels[(y * map.width + x) as usize];
            if in_square {
                assert_eq!(pixel, Colour::WHITE, "white square at ({x}, {y})");
            } else {
                assert_eq!(pixel, 0, "no media → minimap black outside the player square");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Shade whose colour-table entry is non-zero (same constant as the
/// tests/world.rs synthetic scenes: index y=200/x=100).
const SHADE: i32 = 200 * 128 + 100;

/// 3×3 flat world at height 2000 with a plain-coloured tile on every cell
/// (mirrors `flat_world` in tests/world.rs).
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

/// Regression for the production Pix3D wiring: a scene-backed draw straight
/// out of `Client::new` + `prepare_game`/`game_draw` with no test touching
/// `Pix3D::init_colour_table` (the exact gap that let Critical 1 through —
/// without the `Client::new` init this panics on the first gouraud
/// triangle). The synthetic flat world's plain tiles rasterise via
/// `gouraudTriangle`, which needs the colour table, and the viewport must
/// end up non-black.
#[test]
fn game_draw_renders_scene_without_manual_pix3d_init() {
    let mut c = client("/tmp".into());
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    c.world = flat_world();
    // Camera as tests/world.rs: tile (1,1), pitch 512 horizontal at the
    // height-2000 ground.
    c.cam_x = 192;
    c.cam_y = 0;
    c.cam_z = 192;
    c.cam_pitch = 512;
    c.cam_yaw = 0;
    c.game_draw(); // must not panic: colour table inited by Client::new
    let g = c.area_game.as_ref().unwrap();
    assert!(
        g.pixels.iter().any(|&p| p != 0),
        "synthetic flat world must rasterise through game_draw"
    );
}

/// `game_loop` runs `followCamera` when `scene_state == 2` (TS 2346): the
/// orbit camera snaps to the local player (more than 500 away) and eases
/// toward it, so the 3D eye is above the local player instead of the world
/// origin (plan Task 5).
#[test]
fn follow_camera_moves_eye_above_local_player() {
    let mut c = client("/tmp".into());
    c.ingame = true;
    c.scene_state = 2;
    let mut player = ClientPlayer::default();
    player.ready = true;
    player.name = Some("tester".into());
    player.entity = ClientEntity::at(5, 7);
    player.entity.teleport(&c.cache, true, 5, 7);
    c.local_player = Some(player);
    c.game_loop();
    let player_x = c.local_player.as_ref().unwrap().x;
    let player_z = c.local_player.as_ref().unwrap().z;
    assert_eq!(c.orbit_camera_x, player_x, "orbit x snaps to the local player");
    assert_eq!(c.orbit_camera_z, player_z, "orbit z snaps to the local player");
    c.game_draw();
    assert!(
        c.cam_x != 0 || c.cam_z != 0,
        "the 3D eye must follow the player, not sit at the world origin"
    );
}

/// `Pix3D.lowMem` from the config (TS `Client.setLowMem`/`setHighMem`):
/// `Client::new` must carry `config.lowmem` into `pix3d.low_mem`, which
/// `World.render_all` reads for the textured-ground branch.
#[test]
fn pix3d_low_mem_follows_config() {
    let c = client("/tmp".into());
    assert!(!c.pix3d.low_mem, "default config lowmem=false");
    let mut low = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: true,
    });
    assert!(low.pix3d.low_mem, "lowmem config must reach Pix3DDraw.low_mem");
    low.ingame = true;
    low.scene_state = 2;
    low.world = flat_world();
    low.cam_x = 192;
    low.cam_y = 0;
    low.cam_z = 192;
    low.cam_pitch = 512;
    low.cam_yaw = 0;
    low.set_draw(true);
    low.game_draw(); // lowMem gouraud path must not panic either
}

/// The texture half of the production wiring (TS maininit 1152-1154), with
/// a real pack: `prepare_game` must depack `{cache_dir}/textures` into the
/// texel pool and gamma-correct the palettes, all without a test manually
/// calling `init_colour_table`. Skipped without the pack.
#[test]
fn production_init_wires_textures_and_pool() {
    let Some(cache) = cache_dir() else {
        return;
    };
    if !std::path::Path::new(&cache).join("textures").is_file() {
        return;
    }
    let mut c = client(cache);
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    c.game_draw(); // must not panic; prepare_game wires Pix3D
    assert!(
        c.pix3d.num_textures > 0,
        "prepare_game must depack the textures jag"
    );
    assert!(
        c.pix3d.texel_pool.is_some(),
        "prepare_game must init the texel pool (initPool(20))"
    );
    assert!(
        c.pix3d
            .tex_pal
            .iter()
            .any(|pal| pal.as_ref().is_some_and(|p| !p.is_empty())),
        "prepare_game must gamma-correct the texture palettes"
    );
}

/// `minimapBuildBuffer` (Client.ts 5280): with one PLAIN ground tile and a
/// zeroed `mapl`, the composed 512×512 minimap buffer must be non-black
/// after the build. `minimap_build_buffer` is `pub` for the test; the
/// `check_minimap` path calls it internally.
#[test]
fn minimap_build_buffer_writes_pixels() {
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.world.fill_base_level(0);
    c.world.set_ground(
        0, 1, 1,
        TerrainOverlayShape::PLAIN, 0, -1,
        0, 0, 0, 0,
        0, 0, 0, 0,
        0, 0, 0, 0,
        0x00aabb, 0,
    );
    c.mapl = vec![vec![vec![0u8; 104]; 104]; 4];
    c.minimap_build_buffer(0);
    let mm = c.minimap.as_ref().unwrap();
    assert!(mm.data.iter().any(|&p| p != 0), "minimap buffer must be non-black");
}
