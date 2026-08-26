// Task 7 (wgpu backend) selection: `client-play --window` prefers the wgpu
// backend where an adapter exists, and still produces a frame either way.
// The force-failure fallback is pinned in `render_backend.rs` (a separate
// process, so the process-wide GPU context and the `R274_TEST_FORCE_NO_GPU`
// hook cannot race this test). The mesh-builder test lives in `gpu_mesh.rs`
// for the same reason (it pins the colour-table brightness).
use client::render::backend::{BackendKind, FrameOutput, RenderBackend};
use client::render::Renderer;

/// On a machine with an adapter the renderer selects `GpuBackend` and both
/// a title frame and an in-game frame come back as owned full-frame
/// textures (no readback — `finish` returns `FrameOutput::Texture`); on a
/// machine without one it degrades to the CPU backend — the same graceful
/// fallback the force-failure test pins.
#[test]
fn gpu_backend_selected_when_adapter_available() {
    Renderer::set_prefer_gpu(true);
    let mut r = Renderer::new(false);
    if r.backend_kind() != BackendKind::Gpu {
        eprintln!("no adapter on this machine; the GPU selection test skips (CPU fallback active)");
        Renderer::set_prefer_gpu(false);
        return;
    }

    let mut c = client();
    c.set_draw(true);
    let title = r.title_screen_draw(&mut c);
    let FrameOutput::Texture(title_handle) = title else {
        panic!("the GPU title frame must be a full-frame texture");
    };
    assert_eq!((title_handle.width, title_handle.height), (765, 503));

    c.ingame = true;
    let game = r.game_draw(&mut c);
    let FrameOutput::Texture(game_handle) = game else {
        panic!("the GPU in-game frame must be a full-frame texture (no scene readback)");
    };
    assert_eq!((game_handle.width, game_handle.height), (765, 503));

    Renderer::set_prefer_gpu(false);
}

fn client() -> client::client::Client {
    client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// The real client cache (`title`/`media` jags), when present — the title
/// chrome (sprites + fonts) needs it, the same skip pattern as title.rs.
fn real_cache() -> Option<String> {
    let cache = std::env::var("HOME").ok()? + "/experiments/Server/engine/data/pack/client";
    if std::path::Path::new(&cache).join("title").is_file() {
        Some(cache)
    } else {
        None
    }
}

/// The 2D UI composites as a CPU texture upload (the RuneLite pattern): a
/// title frame drawn through the wgpu backend runs the CPU chrome body
/// (the titlebox sprite, the login text glyphs draw into `draw_area`),
/// `finish` uploads that `draw_area` and draws it over the frame, and the
/// full-frame texture carries the chrome content. Skips without an
/// adapter or the real cache.
#[test]
fn chrome_composites_from_the_cpu_draw_area() {
    Renderer::set_prefer_gpu(true);
    let mut r = Renderer::new(false);
    if r.backend_kind() != BackendKind::Gpu {
        eprintln!("no adapter on this machine; the chrome-composite test skips");
        Renderer::set_prefer_gpu(false);
        return;
    }
    let Some(cache) = real_cache() else {
        eprintln!("no client cache; the chrome-composite test skips");
        Renderer::set_prefer_gpu(false);
        return;
    };
    let mut c = client::client::Client::new(client::client::ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.set_draw(true);

    let title = r.title_screen_draw(&mut c);
    let FrameOutput::Texture(handle) = title else {
        panic!("the GPU title frame must be a texture");
    };

    // The composite: the full-frame texture carries the chrome — the login
    // box region (image_title4 blits at (202, 171)) must be non-black.
    let pixels = handle.read_back();
    assert_eq!(pixels.len(), 765 * 503, "the composite is a full-frame texture");
    let mut titlebox_pixels = 0;
    for y in 171..371 {
        for x in 202..562 {
            if pixels[y * 765 + x] != 0 {
                titlebox_pixels += 1;
            }
        }
    }
    assert!(
        titlebox_pixels > 1000,
        "the composited frame must show the titlebox chrome (got {titlebox_pixels} px)"
    );

    // The in-game chrome (side panel, chat, icons, minimap frame) draws
    // into `draw_area` through the same CPU chrome body.
    c.ingame = true;
    let game = r.game_draw(&mut c);
    let FrameOutput::Texture(game_handle) = game else {
        panic!("the GPU in-game frame must be a texture");
    };
    let game_pixels = game_handle.read_back();
    // The side panel (area_side blits at (553, 205)) is non-black with the
    // invback sprite + the active tab's interface.
    let mut side_pixels = 0;
    for y in 205..466 {
        for x in 553..743 {
            if game_pixels[y * 765 + x] != 0 {
                side_pixels += 1;
            }
        }
    }
    assert!(
        side_pixels > 1000,
        "the composited game frame must show the side panel (got {side_pixels} px)"
    );

    Renderer::set_prefer_gpu(false);
}

/// The CPU-upload composite is pixel-exact and needs no cache: paint a
/// known chrome pixel straight into the persistent `draw_area` (what the
/// CPU chrome body writes), finish a frame with no scene, and the read-back
/// texture must carry that pixel — a black `draw_area` outside a scene is
/// opaque chrome, so nothing is lost in the upload.
#[test]
fn draw_area_upload_carries_known_chrome_pixels() {
    let Ok(mut backend) = client::render::backend::GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the draw-area upload test skips");
        return;
    };
    let mut r = Renderer::new(false);
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        // A red rect in the side-panel region and a white pixel in the
        // chat region — the same calls the chrome body makes.
        surface.fill_rect(560, 210, 32, 32, 0xff0000);
        surface.fill_rect(10, 460, 16, 4, 0xffffff);
    }
    let FrameOutput::Texture(handle) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };
    let pixels = handle.read_back();
    assert_eq!(pixels.len(), 765 * 503);
    for y in 210..242 {
        for x in 560..592 {
            assert_eq!(
                pixels[y * 765 + x], 0xff0000,
                "a CPU-drawn chrome rect must composite into the GPU frame at ({x}, {y})"
            );
        }
    }
    for x in 10..26 {
        assert_eq!(
            pixels[460 * 765 + x], 0xffffff,
            "a CPU-drawn chrome row must composite into the GPU frame at ({x}, 460)"
        );
    }
    Renderer::set_prefer_gpu(false);
}

/// The composite produces one full-frame texture: the scene (when built)
/// sits at (4, 4) and the chrome surrounds it, all in the single 765×503
/// texture `finish` returns — no separate scene readback.
#[test]
fn composite_produces_one_full_frame_texture() {
    Renderer::set_prefer_gpu(true);
    let mut r = Renderer::new(false);
    if r.backend_kind() != BackendKind::Gpu {
        eprintln!("no adapter on this machine; the composite test skips");
        Renderer::set_prefer_gpu(false);
        return;
    }
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2; // the full game frame: scene stage + overlays + chrome
    let game = r.game_draw(&mut c);
    let FrameOutput::Texture(handle) = game else {
        panic!("the GPU in-game frame must be a texture");
    };
    assert_eq!((handle.width, handle.height), (765, 503));
    // The frame texture is a single full-frame composite (one 765×503
    // resource, no pixmap half).
    assert_eq!(handle.view.texture().width(), 765);
    assert_eq!(handle.view.texture().height(), 503);
    Renderer::set_prefer_gpu(false);
}
