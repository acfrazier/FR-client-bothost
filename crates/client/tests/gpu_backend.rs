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
    let cache = client::cache_dir().display().to_string();
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
    assert_eq!(
        pixels.len(),
        765 * 503,
        "the composite is a full-frame texture"
    );
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
                pixels[y * 765 + x],
                0xff0000,
                "a CPU-drawn chrome rect must composite into the GPU frame at ({x}, {y})"
            );
        }
    }
    for x in 10..26 {
        assert_eq!(
            pixels[460 * 765 + x],
            0xffffff,
            "a CPU-drawn chrome row must composite into the GPU frame at ({x}, 460)"
        );
    }
    Renderer::set_prefer_gpu(false);
}

/// Ping-pong stays inside the backend for write-while-sample; `finish`
/// always returns one stable present texture so the host's ImGui bind is a
/// no-op after the first registration (not a new `wgpu::Texture` every frame).
#[test]
fn finish_returns_stable_present_texture_identity() {
    let Ok(mut backend) = client::render::backend::GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the stable-present test skips");
        return;
    };
    let mut r = Renderer::new(false);
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        surface.fill_rect(560, 210, 32, 32, 0xff0000);
    }
    let FrameOutput::Texture(first) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };
    let first_tex = first.view.texture().clone();

    for pixel in r.draw_area.pixels.iter_mut() {
        *pixel = 0;
    }
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        surface.fill_rect(560, 210, 32, 32, 0x0000ff);
    }
    backend.mark_chrome_dirty_for_test();
    let FrameOutput::Texture(second) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };
    let second_tex = second.view.texture().clone();

    assert_eq!(
        first_tex, second_tex,
        "consecutive Texture presents must share one stable present texture identity"
    );
    let second_pixels = second.read_back();
    assert_eq!(
        second_pixels[210 * 765 + 560],
        0x0000ff,
        "the stable present must still carry the latest chrome composite"
    );
}

/// Write still ping-pongs off the stable present: a host-held prior frame is
/// not the in-flight write slot. After the second `finish`, the stable
/// present shows the new composite (host samples the same texture).
#[test]
fn finish_does_not_clobber_the_last_presented_frame() {
    let Ok(mut backend) = client::render::backend::GpuBackend::try_new() else {
        eprintln!("no adapter on this machine; the frame ping-pong test skips");
        return;
    };
    let mut r = Renderer::new(false);
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        surface.fill_rect(560, 210, 32, 32, 0xff0000);
    }
    let FrameOutput::Texture(first) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };
    let first_pixels = first.read_back();
    assert_eq!(
        first_pixels[210 * 765 + 560],
        0xff0000,
        "first finish composites red"
    );

    for pixel in r.draw_area.pixels.iter_mut() {
        *pixel = 0;
    }
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        surface.fill_rect(560, 210, 32, 32, 0x0000ff);
    }
    backend.mark_chrome_dirty_for_test();
    let FrameOutput::Texture(second) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };

    assert_eq!(
        first.view.texture(),
        second.view.texture(),
        "stable present: host keeps one texture identity across finishes"
    );
    let second_pixels = second.read_back();
    assert_eq!(
        second_pixels[210 * 765 + 560],
        0x0000ff,
        "the new finish must still composite the latest chrome"
    );
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

/// Chrome atlas is 172×156-free: minimap lives on its own texture.
#[test]
fn minimap_texture_is_172x156() {
    let Ok(backend) = client::render::backend::GpuBackend::try_new() else {
        eprintln!("no adapter; minimap size test skips");
        return;
    };
    assert_eq!(backend.minimap_texture_size(), (172, 156));
}

/// Only minimap motion must not re-upload the full chrome atlas.
#[test]
fn chrome_upload_skipped_when_only_minimap_moved() {
    let Ok(mut backend) = client::render::backend::GpuBackend::try_new() else {
        eprintln!("no adapter; chrome lazy test skips");
        return;
    };
    let mut r = Renderer::new(false);
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        surface.fill_rect(560, 210, 32, 32, 0xff0000);
    }
    let _ = backend.finish(&mut r);
    let chrome_after_first = backend.chrome_upload_count();
    assert!(chrome_after_first >= 1, "first finish uploads chrome");

    // Seed a minimap pixmap and mark only the minimap live — atlas stays clean.
    r.area_map = Some(client::graphics::PixMap::new(172, 156));
    if let Some(map) = r.area_map.as_mut() {
        map.pixels[0] = 0x00_00_ff_00;
    }
    backend.set_minimap_live_for_test(true);
    let _ = backend.finish(&mut r);
    assert_eq!(
        backend.chrome_upload_count(),
        chrome_after_first,
        "minimap-only frame must not re-upload the chrome atlas"
    );
    assert!(
        backend.minimap_upload_count() >= 1,
        "minimap layer must upload when live"
    );
}

/// A chrome redraw flag forces another atlas upload.
#[test]
fn chrome_upload_runs_when_redraw_chat_set() {
    let Ok(mut backend) = client::render::backend::GpuBackend::try_new() else {
        eprintln!("no adapter; chrome redraw test skips");
        return;
    };
    let mut r = Renderer::new(false);
    let _ = backend.finish(&mut r);
    let n = backend.chrome_upload_count();

    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        surface.fill_rect(10, 460, 16, 4, 0xffffff);
    }
    backend.mark_chrome_dirty_for_test();
    let _ = backend.finish(&mut r);
    assert_eq!(
        backend.chrome_upload_count(),
        n + 1,
        "redraw-flagged chrome must upload the atlas again"
    );
}

/// After a punched chrome atlas + live minimap upload, a freeze frame
/// (`minimap_live=false`, chrome not dirty) must still show the last
/// minimap — not a transparent hole over black/3D.
#[test]
fn freeze_keeps_last_minimap_after_punched_chrome() {
    let Ok(mut backend) = client::render::backend::GpuBackend::try_new() else {
        eprintln!("no adapter; freeze minimap hole test skips");
        return;
    };
    let mut r = Renderer::new(false);
    {
        let w = r.draw_area.width;
        let h = r.draw_area.height;
        let mut surface = client::graphics::Pix2D::with_pixels(&mut r.draw_area.pixels, w, h);
        // Non-zero chrome outside the minimap so the atlas is real content.
        surface.fill_rect(560, 210, 32, 32, 0xff0000);
    }
    // Known minimap colour across the 172×156 layer.
    const MAP_RGB: i32 = 0x00_33_cc_66;
    r.area_map = Some(client::graphics::PixMap::new(172, 156));
    if let Some(map) = r.area_map.as_mut() {
        for p in map.pixels.iter_mut() {
            *p = MAP_RGB;
        }
    }
    backend.set_minimap_live_for_test(true);
    let FrameOutput::Texture(live) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };
    let live_px = live.read_back();
    // Sample centre of minimap blit rect (550,4)-(722,160).
    let cx = 550 + 86;
    let cy = 4 + 78;
    assert_eq!(
        live_px[cy * 765 + cx],
        MAP_RGB,
        "live punched frame must composite the minimap colour"
    );
    assert!(
        backend.minimap_upload_count() >= 1,
        "live finish must have uploaded the minimap layer"
    );
    let chrome_n = backend.chrome_upload_count();

    // Freeze: no minimap live, chrome stays clean (scene_state==1 path).
    backend.set_minimap_live_for_test(false);
    let FrameOutput::Texture(frozen) = backend.finish(&mut r) else {
        panic!("finish must return the full-frame texture");
    };
    assert_eq!(
        backend.chrome_upload_count(),
        chrome_n,
        "freeze must not re-upload chrome"
    );
    let frozen_px = frozen.read_back();
    assert_eq!(
        frozen_px[cy * 765 + cx],
        MAP_RGB,
        "freeze after punch must keep the last minimap, not a transparent hole"
    );
    // Spot-check a few more cells in the rect so a single lucky pixel cannot pass.
    for (x, y) in [
        (550 + 10, 4 + 10),
        (550 + 160, 4 + 140),
        (550 + 40, 4 + 100),
    ] {
        assert_eq!(
            frozen_px[y * 765 + x],
            MAP_RGB,
            "freeze minimap hole at ({x},{y})"
        );
    }
}
