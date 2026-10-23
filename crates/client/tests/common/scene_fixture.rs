use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use client::client::{Client, ClientConfig, ClientPlayer, ClientRevision};
use client::dash3d::TerrainOverlayShape;
use client::graphics::{Pix3D, PixMap};
use client::render::backend::{FrameOutput, GpuBackend};
use client::render::Renderer;

static CACHE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

/// Construct the real GPU backend and the cache-free 3×3 terrain fixture
/// used by `ground_input_composed`. `None` means the adapter is unavailable.
pub fn gpu_scene() -> Option<(Client, Renderer)> {
    if crate::gpu_serial::skipped() {
        return None;
    }

    let backend = GpuBackend::try_new().ok()?;
    let sequence = CACHE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let cache_dir = format!(
        "{}/../../target/scene-hook-absent-cache-{}-{sequence}",
        env!("CARGO_MANIFEST_DIR"),
        std::process::id()
    );
    assert!(
        !Path::new(&cache_dir).exists(),
        "the scene-hook fixture must not reuse a client cache"
    );
    let mut client = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir,
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    let mut renderer = Renderer::with_backend(Box::new(backend), false);
    Pix3D::init_colour_table(0.6);
    client.ingame = true;
    client.draw = true;
    client.scene_state = 2;
    client.local_player = Some(ClientPlayer::at(50, 50));
    client.cinema_cam = true;
    client.cam_x = 50 * 128 + 64;
    client.cam_z = 50 * 128 + 64;
    client.cam_y = 0;
    client.cam_pitch = 512;
    client.cam_yaw = 0;
    client.world = client::core::World::new(vec![vec![vec![2000; 105]; 105]; 4], 104, 4, 104);
    client.world.fill_base_level(0);
    for x in 49..=51 {
        for z in 49..=51 {
            client.world.set_ground(
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
    renderer.area_game = Some(PixMap::new(512, 334));
    Some((client, renderer))
}

/// Read the terrain pixel pinned by `ground_input_composed::draw`.
pub fn terrain_pixel(output: FrameOutput) -> i32 {
    let FrameOutput::Texture(handle) = output else {
        panic!("the actual GPU scene fixture must return a texture frame");
    };
    handle.read_back()[138 * 765 + 260]
}
