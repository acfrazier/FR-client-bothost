// Task 7 (wgpu backend) selection: `client-play --window` prefers the wgpu
// backend where an adapter exists, and still produces a frame either way.
// The force-failure fallback is pinned in `render_backend.rs` (a separate
// process, so the process-wide GPU context and the `R274_TEST_FORCE_NO_GPU`
// hook cannot race this test). The mesh-builder test lives in `gpu_mesh.rs`
// for the same reason (it pins the colour-table brightness).
use client::render::backend::{BackendKind, FrameOutput};
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

/// The chrome renders as GPU quads: a title frame drawn through the wgpu
/// backend records quads against the shared atlases (the titlebox sprite,
/// the login text glyphs) and the full-frame texture carries the chrome
/// content, composited on the GPU. Skips without an adapter or the real
/// cache.
#[test]
fn chrome_renders_as_quads_against_the_atlas() {
    Renderer::set_prefer_gpu(true);
    let mut r = Renderer::new(false);
    if r.backend_kind() != BackendKind::Gpu {
        eprintln!("no adapter on this machine; the chrome-quads test skips");
        Renderer::set_prefer_gpu(false);
        return;
    }
    let Some(cache) = real_cache() else {
        eprintln!("no client cache; the chrome-quads test skips");
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
    assert!(
        client::render::backend::GpuBackend::chrome_quad_count() > 0,
        "the title chrome must record quads (sprite + glyph + rect)"
    );

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

    // The in-game chrome (side panel, chat, icons, minimap frame) records
    // quads too — the same recording layer behind draw_side/draw_chat.
    c.ingame = true;
    let game = r.game_draw(&mut c);
    let FrameOutput::Texture(game_handle) = game else {
        panic!("the GPU in-game frame must be a texture");
    };
    assert!(
        client::render::backend::GpuBackend::chrome_quad_count() > 0,
        "the in-game chrome must record quads"
    );
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
