//! Cache-free diagnostic of the real game_loop -> game_draw -> game_loop seam.
//! Geometry translates tests/world.rs's flat-world projection into the interior
//! of a 104-square scene. No expected pick or menu entry is assigned by the test.
use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision, MiniMenuAction};
use client::dash3d::TerrainOverlayShape;
use client::graphics::{Pix3D, PixMap};
use client::render::backend::{FrameOutput, GpuBackend};
use client::render::Renderer;

fn fixture(gpu: bool) -> (Client, Renderer) {
    let cache_dir = format!(
        "{}/../../target/ground-input-absent-cache-{}",
        env!("CARGO_MANIFEST_DIR"),
        std::process::id()
    );
    assert!(!std::path::Path::new(&cache_dir).exists());
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir,
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    let mut r = if gpu {
        Renderer::with_backend(
            Box::new(
                GpuBackend::try_new()
                    .expect("actual GPU adapter required for composed ground proof"),
            ),
            false,
        )
    } else {
        Renderer::new(false)
    };
    Pix3D::init_colour_table(0.6);
    c.ingame = true;
    c.draw = true;
    c.scene_state = 2;
    c.local_player = Some(ClientPlayer::at(50, 50));
    c.cinema_cam = true; // fixed synthetic camera, default move/look rates zero
    c.cam_x = 50 * 128 + 64;
    c.cam_z = 50 * 128 + 64;
    c.cam_y = 0;
    c.cam_pitch = 512;
    c.cam_yaw = 0;
    c.world = client::core::World::new(vec![vec![vec![2000; 105]; 105]; 4], 104, 4, 104);
    c.world.fill_base_level(0);
    for x in 49..=51 {
        for z in 49..=51 {
            c.world.set_ground(
                0,
                x,
                z,
                TerrainOverlayShape::PLAIN,
                0,
                -1,
                2000,
                2000,
                2000,
                2000,
                25700,
                25700,
                25700,
                25700,
                25700,
                25700,
                25700,
                25700,
                0,
                0,
            );
        }
    }
    r.area_game = Some(PixMap::new(512, 334));
    (c, r)
}

fn draw(c: &mut Client, r: &mut Renderer, gpu: bool) {
    let output = r.game_draw(c);
    if gpu {
        let FrameOutput::Texture(handle) = output else {
            panic!("GPU output required")
        };
        assert_ne!(
            handle.read_back()[138 * 765 + 260],
            0,
            "real rendered terrain required"
        );
    } else {
        assert_ne!(
            r.area_game.as_ref().unwrap().pixels[134 * 512 + 256],
            0,
            "real rendered terrain required"
        );
    }
}

fn tick(c: &mut Client) {
    c.shell.latch_click();
    c.game_loop();
}

fn composed(gpu: bool, menu: bool, oblique: bool, blocked: bool) {
    let (mut c, mut r) = fixture(gpu);
    if oblique {
        // A legal game pitch uses real visibility backing, not the 2000-height
        // bypass. At pitch256 equal sin/cos project this tile's Z edges near
        // screen Y151 and123; (256,134) remains strictly inside it.
        c.cam_pitch = 256;
        c.cam_y = 1000;
        c.cam_z -= 1000;
    }
    if blocked {
        // Enclose the player. try_nearest may still succeed at the source,
        // demonstrating that emitted movement does not guarantee displacement.
        for x in 49..=51 {
            for z in 49..=51 {
                if (x, z) != (50, 50) {
                    c.collision[0].block_ground(x, z);
                }
            }
        }
    }
    // Scene-local (256,134) lies strictly inside tile (50,51), translated
    // from the independently projected (1,2) oracle in tests/world.rs.
    c.shell.apply_mouse_move(260, 138);
    draw(&mut c, &mut r, gpu); // real render-time build_minimenu, BEFORE input
    assert_eq!(c.menu_num_entries, 2);
    assert_eq!(c.menu_action[1], MiniMenuAction::WALK);
    c.shell.apply_mouse_down(if menu { 2 } else { 1 }, 260, 138);
    tick(&mut c);
    if menu {
        assert!(c.is_menu_open);
        assert!(!c.world.click);
        draw(&mut c, &mut r, gpu);
        // Select the real top menu row; WALK must use the saved ground
        // coordinates, not this different row-selection position.
        let x = c.menu_x + 5 + 4;
        let y = c.menu_y + 31 + 4;
        c.shell.apply_mouse_move(x, y);
        c.shell.apply_mouse_down(1, x, y);
        tick(&mut c);
        assert!(!c.is_menu_open);
    }
    assert!(c.world.click, "input must arm terrain picking");
    assert_eq!((c.world.click_x, c.world.click_y), (256, 134));
    assert_eq!(c.world.ground_x, -1, "no synthetic ground answer");
    let before = c.out.pos; // retain real click telemetry; no buffer reset
    draw(&mut c, &mut r, gpu);
    assert_eq!((c.world.ground_x, c.world.ground_z), (50, 51));
    assert!(!c.world.click);
    assert_eq!(
        c.out.pos, before,
        "render must defer movement to simulation"
    );
    c.shell.apply_mouse_up();
    tick(&mut c);
    // Primary J10163-10181: opcode234, length5, run0, p2 X then p2 Z.
    let destination_z = if blocked { 50 } else { 51 };
    assert_eq!(
        &c.out.data()[before..c.out.pos],
        &[234, 5, 0, 0, 50, 0, destination_z]
    );
    assert_eq!(c.try_move_nearest, i32::from(blocked));
    eprintln!(
        "gpu={gpu} menu={menu} oblique={oblique} blocked={blocked}: picked (50,51), emitted {:?}",
        &c.out.data()[before..c.out.pos]
    );
    assert_eq!(c.world.ground_x, -1);
    assert_eq!(c.cross_mode, 1);
    let end = c.out.pos;
    draw(&mut c, &mut r, gpu);
    tick(&mut c);
    assert_eq!(c.out.pos, end, "no repeat movement without new mouse-down");
}

#[test]
fn cpu_ground_input_composed() {
    for menu in [false, true] {
        composed(false, menu, false, false);
        composed(false, menu, true, false);
        composed(false, menu, true, true);
    }
}

#[test]
#[ignore = "requires actual GPU adapter; invoke explicitly with --ignored"]
fn gpu_ground_input_composed() {
    for menu in [false, true] {
        composed(true, menu, false, false);
        composed(true, menu, true, false);
        composed(true, menu, true, true);
    }
}
