//! Cache-free production overlay/compositor regression. Requires a working GPU;
//! adapter failure is a failure, never a silently successful skipped proof.
use super::*;
use crate::client::{ClientConfig, ClientRevision};
use crate::dash3d::ClientNpc;
use crate::graphics::Pix32;

fn frame(backend: &mut GpuBackend, core: &mut Client, r: &mut Renderer) -> Vec<i32> {
    assert!(!core.redraw_frame && !core.redraw_side && !core.redraw_chat);
    assert!(!core.redraw_icons && !core.redraw_chat_mode);
    assert_eq!((core.main_modal_id, core.main_overlay_id), (-1, -1));
    if core.scene_state == 1 {
        // Exercise the actual freeze entry, not a test imitation of it.
        backend.scene(core, r, FrameKind::Game);
    } else {
        // Hold the 3D texture constant to isolate overlay vs mesh cadence.
        backend.draw_scene_overlays(core, r, SceneCompose::Retained);
    }
    backend.composite_scene(core, r, FrameKind::Game);
    backend.chrome(core, r, FrameKind::Game);
    assert!(
        !backend.chrome_upload_pending,
        "ordinary chrome must stay clean"
    );
    let FrameOutput::Texture(handle) = backend.finish(r) else {
        panic!("GPU finish must return a texture");
    };
    assert!(backend.scene_ready, "scene-window alpha must be exercised");
    handle.read_back()
}

#[test]
#[ignore = "requires a GPU adapter; invoke explicitly for the production overlay proof"]
fn npc_hint_production_gpu_blink_move_clear_cache_and_freeze() {
    let mut backend = GpuBackend::try_new().expect("GPU required for overlay proof");
    let mut r = Renderer::new(false);
    let cache = std::env::temp_dir().join(format!("npc-hint-no-cache-{}", std::process::id()));
    assert!(!cache.exists(), "fixture must not use an incidental cache");
    let mut core = Client::new_with_revision(
        ClientConfig {
            host: "127.0.0.1".into(),
            port: 43594,
            cache_dir: cache.display().to_string(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    r.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut crown = Pix32::new(3, 3);
    crown.data.fill(0xff00ff);
    let mut media = crate::render::media::Media::empty();
    media.headicons[2] = Some(crown);
    let mut cross = Pix32::new(3, 3);
    cross.data.fill(0x00ffff);
    media.cross[0] = Some(cross);
    r.media = std::sync::Arc::new(media);
    let mut npc = ClientNpc {
        r#type: Some(0), // is_ready; no NPC config/model needed for a hint
        ..Default::default()
    };
    npc.entity.x = 384;
    npc.entity.z = 1280;
    npc.entity.height = 100;
    core.npc[0] = Some(Box::new(npc));
    core.npc_ids = vec![0];
    core.npc_count = 1;
    core.hint_type = 1;
    core.hint_npc = 0;
    core.cam_x = 0;
    core.cam_y = 0;
    core.cam_z = 0;
    core.cam_pitch = 0;
    core.cam_yaw = 0;
    core.scene_state = 2;
    core.ingame = true;
    core.redraw_frame = false;
    core.redraw_side = false;
    core.redraw_chat = false;
    core.redraw_icons = false;
    core.redraw_chat_mode = false;
    core.chat_scroll_height = 77;
    core.chat_scroll_pos = 0;
    core.chat_interface.scroll_pos = 0;
    backend.last_kind = FrameKind::Game;

    // A nonblack, stable GPU scene control. Only the 3D target is seeded;
    // every overlay pixel and coverage byte must come from production drawing.
    let mut encoder = backend
        .context
        .device
        .create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backend.scene_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    backend.context.queue.submit([encoder.finish()]);
    backend.scene_ready = true;
    // Warm atlas with no visible hint; subsequent frames have no chrome flags.
    core.loop_cycle = 10;
    backend.draw_scene_overlays(&mut core, &mut r, SceneCompose::Rebuilt);
    backend.composite_scene(&mut core, &mut r, FrameKind::Game);
    backend.chrome(&mut core, &mut r, FrameKind::Game);
    let FrameOutput::Texture(warm) = backend.finish(&mut r) else {
        panic!("GPU required")
    };
    let background = warm.read_back();
    let mut uploads = backend.chrome_upload_count();
    assert_eq!(uploads, 1);
    let at = |x: usize, y: usize| (y + 4) * FRAME_W as usize + x + 4;
    // Independent integer projection: origin (256,167), height115,
    // x384/z1280 => (409,121), then crown offset(-12,-28).
    let old = at(397, 93);
    let moved = at(436, 101); // x576/z1536 => (448,129)
    assert_eq!(background[old], 0x0000ff);

    core.loop_cycle = 0;
    let on = frame(&mut backend, &mut core, &mut r);
    assert_eq!(
        on[old], 0xff00ff,
        "blink-on must reach GPU without chrome dirtiness"
    );
    assert_eq!(on.iter().filter(|&&p| p == 0xff00ff).count(), 9);
    uploads += 1;
    assert_eq!(backend.chrome_upload_count(), uploads);
    core.loop_cycle = 1;
    assert_eq!(frame(&mut backend, &mut core, &mut r), on);
    assert_eq!(
        backend.chrome_upload_count(),
        uploads,
        "unchanged hint retains atlas"
    );

    core.npc[0].as_mut().unwrap().entity.x = 576;
    core.npc[0].as_mut().unwrap().entity.z = 1536;
    let motion = frame(&mut backend, &mut core, &mut r);
    assert_eq!(motion[old], background[old], "movement clears old pixels");
    assert_eq!(
        motion[moved], 0xff00ff,
        "current entity x/z reach GPU this frame"
    );
    assert_eq!(motion.iter().filter(|&&p| p == 0xff00ff).count(), 9);
    uploads += 1;
    assert_eq!(backend.chrome_upload_count(), uploads);

    core.loop_cycle = 10;
    assert_eq!(
        frame(&mut backend, &mut core, &mut r),
        background,
        "blink-off clears last crown"
    );
    uploads += 1;
    assert_eq!(backend.chrome_upload_count(), uploads);
    core.loop_cycle = 11;
    assert_eq!(frame(&mut backend, &mut core, &mut r), background);
    assert_eq!(backend.chrome_upload_count(), uploads);
    core.loop_cycle = 20;
    assert_eq!(
        frame(&mut backend, &mut core, &mut r),
        motion,
        "blink returns at new position"
    );
    core.hint_type = 0;
    assert_eq!(
        frame(&mut backend, &mut core, &mut r),
        background,
        "disabled hint clears coverage"
    );

    // other_overlays runs AFTER entity_overlays: catch an early signature.
    core.cross_mode = 1;
    core.cross_cycle = 0;
    core.cross_x = 100;
    core.cross_y = 100;
    let cross_frame = frame(&mut backend, &mut core, &mut r);
    assert_eq!(
        cross_frame[at(88, 88)],
        0x00ffff,
        "late overlay writer must invalidate atlas"
    );
    core.cross_mode = 0;
    assert_eq!(frame(&mut backend, &mut core, &mut r), background);

    // Seed a distinctive minimap through the live production layer, then
    // poison the corresponding chrome pixel. A freeze overlay upload must
    // keep punching the held minimap rather than exposing the poison.
    backend.chrome(&mut core, &mut r, FrameKind::Game);
    r.area_map = Some(PixMap::new(MINIMAP_W as i32, MINIMAP_H as i32));
    r.area_map.as_mut().unwrap().pixels.fill(0x00aa11);
    let FrameOutput::Texture(live_minimap) = backend.finish(&mut r) else {
        panic!("GPU required")
    };
    let minimap_pixel = (MINIMAP_Y * FRAME_W + MINIMAP_X) as usize;
    assert_eq!(live_minimap.read_back()[minimap_pixel], 0x00aa11);
    assert!(backend.minimap_held, "live minimap must be held");

    core.scene_state = 1;
    core.hint_type = 1;
    core.loop_cycle = 0;
    r.draw_area.pixels[minimap_pixel] = 0xcc2200;
    let scene_cycle = r.scene_cycle;
    uploads = backend.chrome_upload_count();
    let minimap_uploads = backend.minimap_upload_count();
    let freeze_hint = frame(&mut backend, &mut core, &mut r);
    assert_eq!(
        freeze_hint[moved], 0xff00ff,
        "real NPC hint must update over the frozen scene"
    );
    assert_eq!(
        freeze_hint[minimap_pixel], 0x00aa11,
        "freeze overlay upload must retain the distinctive held minimap"
    );
    assert!(
        backend.minimap_held,
        "overlay update must preserve minimap hold"
    );
    assert_eq!(r.scene_cycle, scene_cycle, "freeze must not rebuild scene");
    assert_eq!(backend.chrome_upload_count(), uploads + 1);
    assert_eq!(backend.minimap_upload_count(), minimap_uploads);

    core.loop_cycle = 1;
    uploads = backend.chrome_upload_count();
    assert_eq!(frame(&mut backend, &mut core, &mut r), freeze_hint);
    assert_eq!(
        backend.chrome_upload_count(),
        uploads,
        "unchanged freeze overlay stays lazy"
    );
    eprintln!("GPU overlay proof executed: blink, x/z movement, clear, cache, late writer, held minimap, last-FBO freeze");
}

#[test]
#[ignore = "requires a GPU adapter; invoke explicitly for the production overlay proof"]
fn busy_nav_does_not_accumulate_into_retained_scene() {
    let mut backend = GpuBackend::try_new().expect("GPU required for nav overlay proof");
    let mut renderer = Renderer::new(false);
    renderer.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut core = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("nav-overlay-no-cache-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    core.cam_x = 0;
    core.cam_y = -500;
    core.cam_z = 0;
    core.cam_pitch = 96;
    core.cam_yaw = 0;
    core.scene_state = 2;
    let paint = crate::render::nav_debug::NavDebugPaint {
        path: vec![(3, 10, false)],
        show_path: true,
        ..Default::default()
    };
    core.set_nav_debug_paint(Some(paint.clone()));
    install_nav_scene_world(&mut core);

    let mut encoder = backend
        .context
        .device
        .create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backend.scene_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    backend.context.queue.submit([encoder.finish()]);
    backend.scene_ready = true;
    backend.last_kind = FrameKind::Game;

    backend.draw_scene_overlays(&mut core, &mut renderer, SceneCompose::Rebuilt);
    assert!(
        !backend.nav_mesh.vertices.is_empty(),
        "retained nav facts must become GPU primitives"
    );
    let scene = TextureHandle::untracked(
        backend.context.device.clone(),
        backend.context.queue.clone(),
        backend.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back();
    assert!(
        scene.iter().any(|&pixel| pixel != 0x0000ff),
        "GPU nav primitives must change the scene target"
    );
    for _ in 0..4 {
        backend.draw_scene_overlays(&mut core, &mut renderer, SceneCompose::Retained);
    }
    let retained = TextureHandle::untracked(
        backend.context.device.clone(),
        backend.context.queue.clone(),
        backend.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back();
    assert_eq!(
        retained, scene,
        "Retained overlay passes must not composite nav into the scene again"
    );

    assert!(
        renderer
            .area_game
            .as_ref()
            .expect("game surface")
            .pixels
            .iter()
            .all(|&pixel| pixel == 0),
        "nav geometry must not CPU-raster into area_game"
    );
    assert!(
        backend.overlay_coverage.iter().all(|&alpha| alpha == 0),
        "nav geometry must not enter the CPU chrome coverage upload"
    );

    let _admission = SceneAdmissionScope::enter(SceneAdmission::Standalone);
    let live_scene_pixels = render_live_scene(&mut backend, &mut core, &mut renderer);
    assert_ne!(
        live_scene_pixels, scene,
        "Busy fairness starts from the actual rendered Rebuild"
    );
    let scene_cycle = renderer.scene_cycle;
    let a_nav_vertices = backend.nav_mesh.vertices.as_ptr();
    let a_nav_bytes = backend.nav_mesh.retained_capacity();
    let vertex_capacity = backend.vertex_buf_capacity;
    let a_busy_nav = TextureHandle::untracked(
        backend.context.device.clone(),
        backend.context.queue.clone(),
        backend.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back();
    {
        let _busy = SceneAdmissionScope::enter(SceneAdmission::Retained);
        let _host = HostOverlayScope::enter();
        for _ in 0..4 {
            backend.begin(&mut core, &mut renderer, FrameKind::Game);
            backend.scene(&mut core, &mut renderer, FrameKind::Game);
            assert_eq!(backend.scene_progress, SceneProgress::Retained);
            assert_eq!(
                renderer.scene_cycle, scene_cycle,
                "Busy never captures a scene"
            );
        }
    }
    let a_busy_nav_after = TextureHandle::untracked(
        backend.context.device.clone(),
        backend.context.queue.clone(),
        backend.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back();
    assert_eq!(
        a_busy_nav_after, a_busy_nav,
        "repeated Busy paints retain the last Rebuild's nav/scene pixels"
    );
    assert_eq!(backend.nav_mesh.vertices.as_ptr(), a_nav_vertices);
    assert_eq!(backend.vertex_buf_capacity, vertex_capacity);
    assert_eq!(backend.nav_mesh.retained_capacity(), a_nav_bytes);

    assert!(
        core.world
            .add_scenery(0, 2, 3, 0, (2 << 29) | 1, 10, 1, 1, 0, 0, 0, 0, 0,),
        "static loc fixture must be added"
    );
    backend.nav_mesh.vertices.clear();
    backend.draw_scene_overlays(&mut core, &mut renderer, SceneCompose::Rebuilt);
    assert!(
        !backend.nav_mesh.vertices.is_empty(),
        "a static loc change must invalidate projected nav geometry"
    );

    core.set_nav_debug_paint(Some(Default::default()));
    backend.draw_scene_overlays(&mut core, &mut renderer, SceneCompose::Rebuilt);
    assert_eq!(
        backend.nav_mesh.retained_capacity(),
        0,
        "empty retained paint must release projection scratch"
    );
    assert!(
        backend.nav_target.is_none(),
        "empty retained paint must release the retained GPU overlay"
    );

    core.set_nav_debug_paint(Some(paint));
    backend.draw_scene_overlays(&mut core, &mut renderer, SceneCompose::Rebuilt);
    assert!(!backend.nav_mesh.vertices.is_empty());
    core.set_nav_debug_paint(None);
    backend.draw_scene_overlays(&mut core, &mut renderer, SceneCompose::Rebuilt);
    assert_eq!(
        backend.nav_mesh.retained_capacity(),
        0,
        "turning paint off must release projection scratch"
    );
    assert!(
        backend.nav_target.is_none(),
        "turning paint off must release the retained GPU overlay"
    );
}

fn install_nav_scene_world(core: &mut Client) {
    use crate::dash3d::TerrainOverlayShape;

    core.ingame = true;
    core.scene_state = 2;
    core.cinema_cam = true;
    core.cam_x = 0;
    core.cam_y = -500;
    core.cam_z = 0;
    core.cam_pitch = 96;
    core.cam_yaw = 0;
    core.world = crate::core::World::new(vec![vec![vec![2000; 105]; 105]; 4], 104, 4, 104);
    core.set_draw(true);
    core.world.fill_base_level(0);
    for x in 2..=4 {
        for z in 9..=11 {
            core.world.set_ground(
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
                25_700,
                25_700,
                25_700,
                25_700,
                25_700,
                25_700,
                25_700,
                25_700,
                0,
                0,
            );
        }
    }
}

fn render_live_scene(
    backend: &mut GpuBackend,
    core: &mut Client,
    renderer: &mut Renderer,
) -> Vec<i32> {
    backend.begin(core, renderer, FrameKind::Game);
    backend.scene(core, renderer, FrameKind::Game);
    assert!(
        backend.scene_ready,
        "the ground fixture produces a real scene"
    );
    TextureHandle::untracked(
        backend.context.device.clone(),
        backend.context.queue.clone(),
        backend.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back()
}

#[test]
#[ignore = "requires a GPU adapter; invoke explicitly for two-head nav storage proof"]
fn two_heads_nav_upload_and_growth_never_share_storage() {
    let _admission = SceneAdmissionScope::enter(SceneAdmission::Standalone);
    let mut backend_a = GpuBackend::try_new().expect("GPU required for head A");
    let mut renderer_a = Renderer::new(false);
    renderer_a.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut core_a = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("nav-growth-head-a-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    install_nav_scene_world(&mut core_a);
    core_a.set_nav_debug_paint(Some(crate::render::nav_debug::NavDebugPaint {
        path: vec![(3, 10, false)],
        show_path: true,
        ..Default::default()
    }));
    seed_retained_scene(&mut backend_a, [0, -500, 0, 96, 0]);
    backend_a.draw_scene_overlays(&mut core_a, &mut renderer_a, SceneCompose::Rebuilt);
    let a_nav_upload = TextureHandle::untracked(
        backend_a.context.device.clone(),
        backend_a.context.queue.clone(),
        backend_a.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back();
    assert!(
        a_nav_upload.iter().any(|&pixel| pixel != 0x0000ff),
        "A's nav upload changes its actual GPU scene target"
    );
    let a_nav_vertices = backend_a.nav_mesh.vertices.as_ptr();
    let a_nav_capacity = backend_a.nav_mesh.retained_capacity();
    assert!(a_nav_capacity > 0);

    let mut backend_b = GpuBackend::try_new().expect("GPU required for head B");
    let mut renderer_b = Renderer::new(false);
    renderer_b.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut core_b = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("nav-growth-head-b-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    install_nav_scene_world(&mut core_b);
    let mut paint_b = crate::render::nav_debug::NavDebugPaint {
        path: vec![(4, 10, false)],
        show_path: true,
        ..Default::default()
    };
    paint_b.colors.path = [0, 255, 0];
    core_b.set_nav_debug_paint(Some(paint_b));

    // B's real scene consumes only its own nav mesh while A's upload stays live.
    let b_pixels_before = render_live_scene(&mut backend_b, &mut core_b, &mut renderer_b);
    let b_nav_vertices = backend_b.nav_mesh.vertices.as_ptr();
    let b_nav_capacity = backend_b.nav_mesh.retained_capacity();
    assert!(!backend_b.nav_mesh.vertices.is_empty());
    assert_ne!(backend_b.nav_mesh.vertices.as_ptr(), a_nav_vertices);
    assert_eq!(backend_a.nav_mesh.vertices.as_ptr(), a_nav_vertices);
    assert_eq!(backend_a.nav_mesh.retained_capacity(), a_nav_capacity);

    let a_pixels_before = render_live_scene(&mut backend_a, &mut core_a, &mut renderer_a);
    assert_eq!(backend_a.scene_progress, SceneProgress::Rebuilt);
    let a_capacity_before = backend_a.vertex_buf_capacity;
    let b_capacity_before = backend_b.vertex_buf_capacity;
    let a_buffer_before = backend_a.vertex_buf.size();
    let b_buffer_before = backend_b.vertex_buf.size();
    assert_eq!(a_buffer_before, a_capacity_before as u64);
    assert_eq!(b_buffer_before, b_capacity_before as u64);
    backend_a
        .ensure_vertex_buffer(a_capacity_before + 1)
        .expect("A's production vertex-buffer growth succeeds");
    assert!(backend_a.vertex_buf_capacity > a_capacity_before);
    assert_ne!(
        backend_a.vertex_buf_capacity, backend_b.vertex_buf_capacity,
        "A and B retain distinct GPU vertex storage"
    );
    assert_eq!(backend_b.vertex_buf_capacity, b_capacity_before);
    assert_eq!(
        backend_a.vertex_buf.size(),
        backend_a.vertex_buf_capacity as u64
    );
    assert_ne!(
        backend_a.vertex_buf.size(),
        b_buffer_before,
        "A's growth changes its GPU allocation, not B's"
    );
    assert_eq!(backend_b.vertex_buf.size(), b_buffer_before);

    let a_pixels_after = render_live_scene(&mut backend_a, &mut core_a, &mut renderer_a);
    let b_pixels_after = TextureHandle::untracked(
        backend_b.context.device.clone(),
        backend_b.context.queue.clone(),
        backend_b.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back();
    assert_eq!(
        a_pixels_after, a_pixels_before,
        "A's matching scene bind group renders identical pixels after growth"
    );
    assert_eq!(
        b_pixels_after, b_pixels_before,
        "A's growth cannot change B's already submitted scene"
    );
    assert_ne!(
        a_pixels_after, b_pixels_after,
        "each head paints its own nav payload"
    );
    assert_eq!(backend_b.vertex_buf_capacity, b_capacity_before);
    assert_eq!(backend_a.nav_mesh.vertices.as_ptr(), a_nav_vertices);
    assert_eq!(backend_b.nav_mesh.vertices.as_ptr(), b_nav_vertices);
    assert_eq!(backend_b.nav_mesh.retained_capacity(), b_nav_capacity);
}

fn loading_splash_renderer() -> Renderer {
    let mut renderer = Renderer::new(false);
    renderer.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut font = crate::graphics::PixFont::new();
    for byte in b"Loading - please wait." {
        let index = *byte as usize;
        font.char_mask[index] = vec![1];
        font.char_mask_width[index] = 1;
        font.char_mask_height[index] = 1;
        font.char_advance[index] = 1;
    }
    font.height = 1;
    let mut media = crate::render::media::Media::empty();
    media.p12 = Some(font);
    renderer.media = std::sync::Arc::new(media);
    renderer
}

fn assert_splash_cleared_poison(renderer: &Renderer, poison: i32) {
    let pixels = &renderer.area_game.as_ref().expect("scene surface").pixels;
    assert!(
        !pixels.contains(&poison),
        "loading clears prior scene pixels first"
    );
    assert!(
        pixels.contains(&0x00ff_ffff),
        "the P12 loading text is drawn after the clear"
    );
}

#[test]
#[ignore = "requires a GPU adapter; invoke explicitly for the host overlay state matrix"]
fn host_overlay_state_matrix_preserves_splash_and_empty_scene() {
    let _admission = SceneAdmissionScope::enter(SceneAdmission::Retained);
    let mut gpu = GpuBackend::try_new().expect("GPU required for the overlay state matrix");
    let mut gpu_renderer = loading_splash_renderer();
    let mut gpu_core = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("gpu-overlay-state-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    gpu_core.scene_state = 1;
    let poison = 0x123456;
    gpu_renderer.area_game.as_mut().unwrap().pixels.fill(poison);
    gpu.scene(&mut gpu_core, &mut gpu_renderer, FrameKind::Game);
    assert!(gpu_renderer
        .area_game
        .as_ref()
        .unwrap()
        .pixels
        .iter()
        .all(|&pixel| pixel == poison));
    assert!(
        !gpu.scene_ready,
        "PendingScene does not invent retained validity"
    );

    let mut cpu = super::super::cpu::CpuBackend::default();
    let mut cpu_renderer = loading_splash_renderer();
    let mut cpu_core = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("cpu-overlay-state-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    cpu_core.scene_state = 1;
    cpu_renderer.area_game.as_mut().unwrap().pixels.fill(poison);
    cpu.scene(&mut cpu_core, &mut cpu_renderer, FrameKind::Game);
    assert!(cpu_renderer
        .area_game
        .as_ref()
        .unwrap()
        .pixels
        .iter()
        .all(|&pixel| pixel == poison));

    let _host = HostOverlayScope::enter();
    gpu_renderer.area_game.as_mut().unwrap().pixels.fill(poison);
    gpu.scene(&mut gpu_core, &mut gpu_renderer, FrameKind::Game);
    assert_splash_cleared_poison(&gpu_renderer, poison);
    assert!(
        !gpu.scene_ready,
        "loading without a prior scene stays PendingScene"
    );

    cpu_renderer.area_game.as_mut().unwrap().pixels.fill(poison);
    cpu.scene(&mut cpu_core, &mut cpu_renderer, FrameKind::Game);
    assert_splash_cleared_poison(&cpu_renderer, poison);

    gpu_core.scene_state = 2;
    gpu_renderer.area_game.as_mut().unwrap().pixels.fill(poison);
    gpu.scene(&mut gpu_core, &mut gpu_renderer, FrameKind::Game);
    assert!(
        !gpu_renderer
            .area_game
            .as_ref()
            .unwrap()
            .pixels
            .contains(&0x00ff_ffff),
        "Busy state 2 never paints the loading splash"
    );
    assert!(
        !gpu.scene_ready,
        "Busy cannot turn PendingScene into a valid scene"
    );

    gpu_core.scene_state = 0;
    gpu_renderer.area_game.as_mut().unwrap().pixels.fill(poison);
    gpu.scene(&mut gpu_core, &mut gpu_renderer, FrameKind::Game);
    assert!(
        !gpu.scene_ready,
        "state zero invalidates GPU scene authority"
    );
    assert!(gpu_renderer
        .area_game
        .as_ref()
        .unwrap()
        .pixels
        .iter()
        .all(|&pixel| pixel == poison));

    cpu_core.scene_state = 0;
    cpu_renderer.area_game.as_mut().unwrap().pixels.fill(poison);
    cpu.scene(&mut cpu_core, &mut cpu_renderer, FrameKind::Game);
    assert!(
        !cpu.has_valid_scene(),
        "CPU state zero invalidates scene authority independently of storage"
    );
    assert!(cpu_renderer
        .area_game
        .as_ref()
        .unwrap()
        .pixels
        .iter()
        .all(|&pixel| pixel == poison));
}

fn parity_clear(backend: &GpuBackend, rgb: (f64, f64, f64)) {
    let mut encoder = backend
        .context
        .device
        .create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backend.scene_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: rgb.0,
                        g: rgb.1,
                        b: rgb.2,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    backend.context.queue.submit([encoder.finish()]);
}

fn parity_read(backend: &GpuBackend) -> Vec<i32> {
    TextureHandle::untracked(
        backend.context.device.clone(),
        backend.context.queue.clone(),
        backend.scene_view.clone(),
        SCENE_W,
        SCENE_H,
    )
    .read_back()
}

fn parity_close(a: i32, b: i32, tol: i32) -> bool {
    (0..3).all(|s| (((a >> (s * 8)) & 0xff) - ((b >> (s * 8)) & 0xff)).abs() <= tol)
}

#[test]
#[ignore = "requires a GPU adapter; invoke explicitly for Pix2D pixel parity"]
fn nav_overlay_matches_pix2d_reference() {
    use crate::render::nav_debug::{
        NavDebugCell, NavDebugPaint, CORNER_NE, CORNER_SW, FACE_E, FACE_N, FACE_S, FACE_W,
    };
    let mut backend = GpuBackend::try_new().expect("GPU");
    let mut r = Renderer::new(false);
    r.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut font = crate::graphics::PixFont::new();
    let code = b'I' as usize;
    font.char_mask[code] = vec![1, 1, 1, 1, 1];
    font.char_mask_width[code] = 1;
    font.char_mask_height[code] = 5;
    font.char_advance[code] = 3;
    font.height = 5;
    let mut media = crate::render::media::Media::empty();
    media.b12 = Some(font);
    r.media = std::sync::Arc::new(media);
    let mut core = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("nav-parity-no-cache-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    core.scene_state = 2;
    let mut collision = Vec::new();
    for lz in 2..20 {
        for lx in 0..14 {
            let k = (lx * 7 + lz * 13) % 11;
            let bits = match k {
                0 => FACE_N | FACE_E,
                1 => FACE_S,
                2 => FACE_W | CORNER_NE,
                3 => CORNER_SW,
                _ => 0,
            };
            let blocked = matches!(k, 0 | 4 | 5);
            if bits != 0 || blocked {
                collision.push(NavDebugCell {
                    lx,
                    lz,
                    bits,
                    blocked,
                    reach: true,
                });
            }
        }
    }
    collision.push(NavDebugCell {
        lx: 6,
        lz: 9,
        bits: 0,
        blocked: false,
        reach: false,
    });
    let base_paint = NavDebugPaint {
        collision,
        path: (0..10).map(|i| (5, 3 + i, i == 5)).collect(),
        trail: (0..8).map(|i| (3 + i / 2, 5 + i, i % 2 == 0)).collect(),
        click: Some((7, 12)),
        show_collision: true,
        show_nsew: true,
        show_path: true,
        show_trail: true,
        ..Default::default()
    };
    for variant in ["all", "fills", "collision", "route", "captions"] {
        let mut paint = base_paint.clone();
        match variant {
            "fills" => {
                paint.show_path = false;
                paint.show_trail = false;
                paint.show_nsew = false;
                paint.click = None;
                for c in &mut paint.collision {
                    c.bits = 0;
                }
            }
            "collision" => {
                paint.show_path = false;
                paint.show_trail = false;
                paint.click = None;
            }
            "route" => {
                // Planned path and client trail over the same walkable tiles.
                paint.collision.clear();
                paint.show_collision = false;
                paint.show_nsew = false;
                paint.path = (0..10).map(|i| (5, 3 + i, false)).collect();
                paint.trail = (0..10).map(|i| (5, 3 + i, false)).collect();
                paint.click = Some((5, 12));
            }
            "captions" => {
                paint.collision.clear();
                paint.path = vec![(5, 6, true), (5, 10, true)];
                paint.trail.clear();
                paint.labels = vec![(5, 6, "I".into()), (5, 10, "I".into())];
                paint.show_collision = false;
                paint.show_nsew = false;
                paint.show_path = true;
                paint.show_trail = false;
                paint.click = None;
            }
            _ => {}
        }
        eprintln!("variant={variant}");
        let bg = (0.25, 0.5, 0.35);
        let cams = [
            (832, -600, 0, 128, 0),
            (832, -450, -300, 256, 100),
            (1100, -700, 400, 200, 1900),
        ];
        for (ci, &(cx, cy, cz, pitch, yaw)) in cams.iter().enumerate() {
            core.cam_x = cx;
            core.cam_y = cy;
            core.cam_z = cz;
            core.cam_pitch = pitch;
            core.cam_yaw = yaw;
            r.pix3d.set_clipping(SCENE_W as i32, SCENE_H as i32);
            core.set_nav_debug_paint(Some(paint.clone()));

            // Old path: Pix2D raster + coverage, composited with the chrome
            // blend (src*a + dst*(1-a)) over the same background.
            parity_clear(&backend, bg);
            let bgpx = parity_read(&backend);
            let mut old_rgb = vec![0i32; (SCENE_W * SCENE_H) as usize];
            let mut cov = vec![0u8; (SCENE_W * SCENE_H) as usize];
            {
                let _g = crate::graphics::pix2d::coverage_guard(&mut cov, SCENE_W, SCENE_H);
                let mut s = Pix2D::with_pixels(&mut old_rgb, SCENE_W as i32, SCENE_H as i32);
                crate::render::nav_debug_old::draw(&mut core, &mut r, &mut s);
            }
            let old: Vec<i32> = (0..old_rgb.len())
                .map(|i| {
                    let a = cov[i] as i32;
                    let mut o = 0;
                    for s in 0..3 {
                        let src = (old_rgb[i] >> (s * 8)) & 0xff;
                        let dst = (bgpx[i] >> (s * 8)) & 0xff;
                        o |= ((src * a + dst * (255 - a) + 127) / 255) << (s * 8);
                    }
                    o
                })
                .collect();

            // New path: the production GPU overlay.
            parity_clear(&backend, bg);
            backend.scene_ready = true;
            backend.last_kind = FrameKind::Game;
            backend.render_nav_overlay(&mut core, &mut r);
            let new = parity_read(&backend);
            let verts = backend.nav_mesh.vertices.len();

            let old_nav = cov.iter().filter(|&&a| a > 0).count();
            let new_nav = (0..new.len())
                .filter(|&i| !parity_close(new[i], bgpx[i], 0))
                .count();
            let mut best = (usize::MAX, 0, 0);
            for dy in -1..=1i32 {
                for dx in -1..=1i32 {
                    let mut n = 0;
                    for y in 1..SCENE_H as i32 - 1 {
                        for x in 1..SCENE_W as i32 - 1 {
                            let i = (y * SCENE_W as i32 + x) as usize;
                            let j = ((y + dy) * SCENE_W as i32 + (x + dx)) as usize;
                            if !parity_close(old[i], new[j], 3) {
                                n += 1;
                            }
                        }
                    }
                    if dx == 0 && dy == 0 {
                        eprintln!("cam{ci}: shift(0,0) mismatches={n}");
                    }
                    if n < best.0 {
                        best = (n, dx, dy);
                    }
                }
            }
            // Pixels where both paint but colours disagree (overlap blending).
            let both_diff = (0..new.len())
                .filter(|&i| {
                    cov[i] > 0
                        && !parity_close(new[i], bgpx[i], 0)
                        && !parity_close(old[i], new[i], 3)
                })
                .count();
            let both_gross = (0..new.len())
                .filter(|&i| {
                    cov[i] > 0
                        && !parity_close(new[i], bgpx[i], 0)
                        && !parity_close(old[i], new[i], 12)
                })
                .count();
            eprintln!("cam{ci}: both_painted_colour_diff_gt12={both_gross}");
            let old_only = (0..new.len())
                .filter(|&i| cov[i] > 0 && parity_close(new[i], bgpx[i], 0))
                .count();
            let new_only = (0..new.len())
                .filter(|&i| cov[i] == 0 && !parity_close(new[i], bgpx[i], 0))
                .count();
            eprintln!(
            "cam{ci}: verts={verts} old_nav_px={old_nav} new_nav_px={new_nav} old_only={old_only} new_only={new_only} both_but_colour_differs={both_diff} best_shift=({},{}) mismatches_at_best={}",
            best.1, best.2, best.0
        );
            if old_nav != 0 {
                assert_eq!(
                    (best.1, best.2),
                    (0, 0),
                    "GPU paint must use the Pix2D pixel-centre convention"
                );
            }
            let parity_budget = (old_nav / 100).max(16);
            assert!(
            old_only + new_only <= parity_budget,
            "GPU paint footprint diverged from Pix2D beyond 1%: old_only={old_only}, new_only={new_only}, budget={parity_budget}"
        );
            assert!(
            both_gross <= parity_budget,
            "GPU paint must preserve Pix2D last-writer-wins colour within 1%: {both_gross} gross pixel differences, budget={parity_budget}"
        );

            // Chunk equivalence: force tiny chunks through the same buffer.
            if ci == 0 {
                let saved = backend.vertex_buf_capacity;
                backend.vertex_buf_capacity = 12 * 3 * 5;
                backend.nav_projection_key = None;
                parity_clear(&backend, bg);
                backend.render_nav_overlay(&mut core, &mut r);
                let chunked = parity_read(&backend);
                backend.vertex_buf_capacity = saved;
                let differing = (0..new.len()).filter(|&i| chunked[i] != new[i]).count();
                assert_eq!(
                    differing, 0,
                    "{variant} must render identically across triangle chunks"
                );
            }
        }
    }
}
thread_local! {
    static HOST_OVERLAYS_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
thread_local! {
    static GPU_TEST_ADMISSION: std::cell::Cell<SceneAdmission> =
        const { std::cell::Cell::new(SceneAdmission::Standalone) };
}

fn gpu_test_scene_admission() -> SceneAdmission {
    GPU_TEST_ADMISSION.with(std::cell::Cell::get)
}

struct SceneAdmissionScope(SceneAdmission);

impl SceneAdmissionScope {
    fn enter(admission: SceneAdmission) -> Self {
        super::set_scene_admission_hook(gpu_test_scene_admission);
        Self(GPU_TEST_ADMISSION.with(|current| current.replace(admission)))
    }
}

impl Drop for SceneAdmissionScope {
    fn drop(&mut self) {
        GPU_TEST_ADMISSION.with(|current| current.set(self.0));
    }
}

fn gpu_test_host_overlays_active() -> bool {
    HOST_OVERLAYS_ACTIVE.with(std::cell::Cell::get)
}

struct HostOverlayScope(bool);

impl HostOverlayScope {
    fn enter() -> Self {
        super::super::cpu::set_cpu_host_overlay_hook(gpu_test_host_overlays_active);
        Self(HOST_OVERLAYS_ACTIVE.with(|active| active.replace(true)))
    }
}

impl Drop for HostOverlayScope {
    fn drop(&mut self) {
        HOST_OVERLAYS_ACTIVE.with(|active| active.set(self.0));
    }
}

fn seed_retained_scene(backend: &mut GpuBackend, camera: [i32; 5]) {
    let mut encoder = backend
        .context
        .device
        .create_command_encoder(&Default::default());
    {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backend.scene_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    backend.context.queue.submit([encoder.finish()]);
    backend.scene_ready = true;
    backend.last_kind = FrameKind::Game;
    backend.last_rebuild_camera = Some(camera);
}

fn install_ship_glider_modal_fixture(core: &mut Client) {
    use crate::config::if_type::ComponentType;
    use crate::config::IfType;

    const SHIP_ROOT: usize = 3281;
    const GLIDER_ROOT: usize = 802;
    const SHIP_RECT: usize = 3282;
    const GLIDER_RECT: usize = 3283;

    let layer = |id: usize, child: usize, child_x: i32| IfType {
        id: id as i32,
        r#type: ComponentType::TYPE_LAYER,
        width: SCENE_W as i32,
        height: SCENE_H as i32,
        children: Some(vec![child as i32]),
        child_x: Some(vec![child_x]),
        child_y: Some(vec![50]),
        ..Default::default()
    };
    let rect = |id: usize| IfType {
        id: id as i32,
        r#type: ComponentType::TYPE_RECT,
        width: 12,
        height: 8,
        fill: true,
        ..Default::default()
    };

    let mut interfaces = Vec::new();
    interfaces.resize_with(GLIDER_RECT + 1, || None);
    interfaces[SHIP_ROOT] = Some(Box::new(layer(SHIP_ROOT, SHIP_RECT, 100)));
    interfaces[GLIDER_ROOT] = Some(Box::new(layer(GLIDER_ROOT, GLIDER_RECT, 150)));
    interfaces[SHIP_RECT] = Some(Box::new(rect(SHIP_RECT)));
    interfaces[GLIDER_RECT] = Some(Box::new(rect(GLIDER_RECT)));
    core.ifaces = std::sync::Arc::new(interfaces);
    core.ifaces_mut = std::sync::Arc::new(Vec::new());
    core.iface_mut(SHIP_RECT)
        .expect("ship rectangle state")
        .colour = 0xff0000;
    core.iface_mut(GLIDER_RECT)
        .expect("glider rectangle state")
        .colour = 0x00ff00;
    core.main_overlay_id = -1;
    core.main_modal_id = SHIP_ROOT as i32;
}

fn render_retained_overlay_frame(
    backend: &mut GpuBackend,
    core: &mut Client,
    renderer: &mut Renderer,
) -> Vec<i32> {
    backend.begin(core, renderer, FrameKind::Game);
    backend.scene(core, renderer, FrameKind::Game);
    assert!(backend.scene_ready, "retained frame keeps the seeded scene");
    assert_eq!(backend.scene_progress, SceneProgress::Retained);
    backend.composite_scene(core, renderer, FrameKind::Game);
    backend.chrome(core, renderer, FrameKind::Game);
    let FrameOutput::Texture(frame) = backend.finish(renderer) else {
        panic!("GPU overlay frame must be a texture");
    };
    frame.read_back()
}

#[test]
#[ignore = "requires a GPU adapter; invoke explicitly for retained overlay fairness"]
fn busy_ship_glider_modal_minimenu_layers_are_current() {
    let _admission = SceneAdmissionScope::enter(SceneAdmission::Retained);
    let _host = HostOverlayScope::enter();
    let mut backend = GpuBackend::try_new().expect("GPU required for retained overlay proof");
    let mut renderer = Renderer::new(false);
    renderer.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut media = crate::render::media::Media::empty();
    let mut crown = Pix32::new(3, 3);
    crown.data.fill(0xff00ff);
    media.headicons[2] = Some(crown);
    renderer.media = std::sync::Arc::new(media);
    let mut core = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("gpu-retained-overlay-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    install_ship_glider_modal_fixture(&mut core);
    core.scene_state = 2;
    core.ingame = true;
    core.loop_cycle = 0;
    core.cam_x = 9000;
    core.cam_y = 1000;
    core.cam_z = 7000;
    core.cam_pitch = 512;
    core.cam_yaw = 1200;
    seed_retained_scene(&mut backend, [0; 5]);
    core.npc[0] = Some(Box::new(ClientNpc {
        r#type: Some(0),
        ..Default::default()
    }));
    let npc = core.npc[0].as_mut().expect("NPC fixture");
    npc.entity.x = 384;
    npc.entity.z = 1280;
    npc.entity.height = 100;
    core.npc_ids = vec![0];
    core.npc_count = 1;
    core.hint_type = 1;
    core.hint_npc = 0;
    core.is_menu_open = true;
    core.menu_area = 0;
    core.menu_num_entries = 1;
    core.menu_x = 200;
    core.menu_y = 150;
    core.menu_width = 80;
    core.menu_height = 52;

    let live_scene_cycle = renderer.scene_cycle;
    let at = |x: usize, y: usize| (y + 4) * FRAME_W as usize + x + 4;
    let npc_old = at(397, 93);
    let npc_moved = at(436, 101);
    let ship_modal = at(104, 54);
    let glider_modal = at(154, 54);
    let menu_fill = at(220, 180);
    let ship = render_retained_overlay_frame(&mut backend, &mut core, &mut renderer);
    assert_eq!(
        renderer.scene_cycle, live_scene_cycle,
        "Busy must not advance scene_cycle"
    );
    assert_eq!(
        ship[npc_old], 0xff00ff,
        "current NPC hint overlays retained 3D"
    );
    assert_eq!(
        ship[ship_modal], 0xff0000,
        "ship modal rectangle is current"
    );
    assert_eq!(
        ship[menu_fill], 0x5d5447,
        "minimenu is above the modal layer"
    );
    assert_eq!(
        [
            core.cam_x,
            core.cam_y,
            core.cam_z,
            core.cam_pitch,
            core.cam_yaw
        ],
        [9000, 1000, 7000, 512, 1200],
        "overlay camera scope must restore the simulation camera"
    );

    core.npc[0].as_mut().unwrap().entity.x = 576;
    core.npc[0].as_mut().unwrap().entity.z = 1536;
    core.main_modal_id = 802; // the packed glidermap root
    let glider = render_retained_overlay_frame(&mut backend, &mut core, &mut renderer);
    assert_eq!(
        renderer.scene_cycle, live_scene_cycle,
        "Busy must not advance scene_cycle"
    );
    assert_eq!(
        glider[npc_old], 0x0000ff,
        "moved entity leaves no stale hint"
    );
    assert_eq!(
        glider[npc_moved], 0xff00ff,
        "entity uses the last Rebuild camera"
    );
    assert_eq!(
        glider[ship_modal], 0x0000ff,
        "ship modal removal clears its pixels"
    );
    assert_eq!(glider[menu_fill], 0x5d5447, "open minimenu remains current");
    assert_eq!(glider[glider_modal], 0x00ff00, "glider modal is current");

    core.scene_state = 1;
    let loading = render_retained_overlay_frame(&mut backend, &mut core, &mut renderer);
    assert_eq!(
        loading[npc_moved], 0xff00ff,
        "loading keeps the current entity overlay"
    );
    assert_eq!(
        loading[glider_modal], 0x00ff00,
        "loading keeps the current glider modal"
    );
    assert_eq!(
        loading[menu_fill], 0x5d5447,
        "loading keeps the current minimenu"
    );
    core.main_modal_id = -1;
    core.is_menu_open = false;
    core.hint_type = 0;
    let cleared = render_retained_overlay_frame(&mut backend, &mut core, &mut renderer);
    assert_eq!(
        cleared[glider_modal], 0x0000ff,
        "removed glider modal leaves no trail"
    );
    assert_eq!(
        cleared[ship_modal], 0x0000ff,
        "removed ship modal leaves no trail"
    );
    assert_eq!(
        cleared[menu_fill], 0x0000ff,
        "closed minimenu leaves no trail"
    );
}

#[test]
fn cpu_freeze_ship_glider_modal_minimenu_layers_are_current() {
    let _host = HostOverlayScope::enter();
    let mut backend = super::super::cpu::CpuBackend::default();
    let mut renderer = Renderer::new(false);
    renderer.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut media = crate::render::media::Media::empty();
    let mut crown = Pix32::new(3, 3);
    crown.data.fill(0xff00ff);
    media.headicons[2] = Some(crown);
    renderer.media = std::sync::Arc::new(media);
    let mut core = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("cpu-retained-overlay-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    install_ship_glider_modal_fixture(&mut core);
    core.scene_state = 2;
    core.ingame = true;
    core.loop_cycle = 0;
    core.cinema_cam = true;
    [
        core.cam_x,
        core.cam_y,
        core.cam_z,
        core.cam_pitch,
        core.cam_yaw,
    ] = [0; 5];
    std::sync::Arc::get_mut(&mut core.cache)
        .expect("unique CPU overlay fixture cache")
        .npcs
        .push(crate::config::NpcType::default());
    core.npc[0] = Some(Box::new(ClientNpc {
        r#type: Some(0),
        ..Default::default()
    }));
    let npc = core.npc[0].as_mut().expect("NPC fixture");
    npc.entity.x = 384;
    npc.entity.z = 1280;
    npc.entity.height = 100;
    core.npc_ids = vec![0];
    core.npc_count = 1;
    core.hint_type = 1;
    core.hint_npc = 0;
    core.is_menu_open = true;
    core.menu_area = 0;
    core.menu_num_entries = 1;
    core.menu_x = 200;
    core.menu_y = 150;
    core.menu_width = 80;
    core.menu_height = 52;

    backend.begin(&mut core, &mut renderer, FrameKind::Game);
    backend.scene(&mut core, &mut renderer, FrameKind::Game);
    assert_eq!(backend.scene_progress(), SceneProgress::Rebuilt);
    assert_eq!(backend.retained_storage_allocations(), 1);
    let live_scene_cycle = renderer.scene_cycle;
    assert_eq!(
        renderer.area_game.as_ref().expect("game surface").pixels[93 * SCENE_W as usize + 397],
        0xff00ff,
        "the completed scene contains the original hint at the fixed camera projection"
    );

    core.scene_state = 1;
    core.cam_x = 9000;
    core.cam_y = 1000;
    core.cam_z = 7000;
    core.cam_pitch = 512;
    core.cam_yaw = 1200;
    core.npc[0].as_mut().unwrap().entity.x = 576;
    core.npc[0].as_mut().unwrap().entity.z = 1536;
    core.main_modal_id = 802;
    backend.begin(&mut core, &mut renderer, FrameKind::Game);
    backend.scene(&mut core, &mut renderer, FrameKind::Game);
    assert_eq!(backend.scene_progress(), SceneProgress::Retained);
    assert_eq!(
        renderer.scene_cycle, live_scene_cycle,
        "CPU freeze must not advance scene_cycle"
    );
    let surface = &renderer.area_game.as_ref().expect("game surface").pixels;
    let at = |x: usize, y: usize| y * SCENE_W as usize + x;
    let npc_old = at(397, 93);
    let npc_moved = at(436, 101);
    let ship_modal = at(104, 54);
    let glider_modal = at(154, 54);
    let menu_fill = at(220, 180);
    assert_ne!(
        surface[npc_old], 0xff00ff,
        "old hint pixels are restored away"
    );
    assert_eq!(
        surface[npc_moved], 0xff00ff,
        "current entity uses retained camera"
    );
    assert_ne!(
        surface[ship_modal], 0xff0000,
        "old ship modal pixels are restored away"
    );
    assert_eq!(
        surface[glider_modal], 0x00ff00,
        "current glider modal is drawn"
    );
    assert_eq!(surface[menu_fill], 0x5d5447, "current minimenu is drawn");
    assert_eq!(
        [
            core.cam_x,
            core.cam_y,
            core.cam_z,
            core.cam_pitch,
            core.cam_yaw
        ],
        [9000, 1000, 7000, 512, 1200],
        "overlay camera scope must restore the simulation camera"
    );

    core.main_modal_id = -1;
    core.is_menu_open = false;
    core.hint_type = 0;
    backend.begin(&mut core, &mut renderer, FrameKind::Game);
    backend.scene(&mut core, &mut renderer, FrameKind::Game);
    let surface = &renderer.area_game.as_ref().expect("game surface").pixels;
    assert_ne!(surface[npc_moved], 0xff00ff, "removed hint leaves no trail");
    assert_ne!(
        surface[glider_modal], 0x00ff00,
        "removed modal leaves no trail"
    );
    assert_ne!(
        surface[menu_fill], 0x5d5447,
        "closed minimenu leaves no trail"
    );
}

#[test]
fn embedded_cpu_never_takes_host_retention_path() {
    let _host = HostOverlayScope::enter();
    assert!(super::super::cpu::host_overlays_active());
    let mut backend = super::super::cpu::CpuBackend::embedded();
    let mut renderer = Renderer::new(false);
    renderer.area_game = Some(PixMap::new(SCENE_W as i32, SCENE_H as i32));
    let mut core = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: std::env::temp_dir()
            .join(format!("gpu-embedded-cpu-{}", std::process::id()))
            .display()
            .to_string(),
        members: true,
        lowmem: false,
    });
    core.ingame = true;

    for scene_state in [2, 1] {
        core.scene_state = scene_state;
        backend.begin(&mut core, &mut renderer, FrameKind::Game);
        backend.scene(&mut core, &mut renderer, FrameKind::Game);
        if scene_state == 2 {
            assert_eq!(
                backend.scene_progress(),
                SceneProgress::Rebuilt,
                "fixture must complete a live CPU scene"
            );
        } else {
            assert_eq!(
                backend.scene_progress(),
                SceneProgress::Retained,
                "loading must retain the live scene state"
            );
        }
        assert_eq!(backend.retained_storage_bytes(), 0);
        assert_eq!(backend.retained_storage_allocations(), 0);
        backend.composite_scene(&mut core, &mut renderer, FrameKind::Game);
        backend.chrome(&mut core, &mut renderer, FrameKind::Game);
        assert!(matches!(
            backend.finish(&mut renderer),
            FrameOutput::PixMap(_)
        ));
        assert_eq!(backend.retained_storage_bytes(), 0);
        assert_eq!(backend.retained_storage_allocations(), 0);
    }
}
