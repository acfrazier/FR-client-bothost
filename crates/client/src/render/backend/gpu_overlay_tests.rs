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
        backend.draw_scene_overlays(core, r);
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
    backend.draw_scene_overlays(&mut core, &mut r);
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
