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

#[test]
#[ignore = "requires a GPU adapter; invoke explicitly for the production overlay proof"]
fn nav_overlay_bypasses_cpu_surface_and_coverage() {
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

    backend.draw_scene_overlays(&mut core, &mut renderer);
    assert!(
        !backend.nav_mesh.vertices.is_empty(),
        "retained nav facts must become GPU primitives"
    );
    let scene = TextureHandle {
        device: backend.context.device.clone(),
        queue: backend.context.queue.clone(),
        view: backend.scene_view.clone(),
        width: SCENE_W,
        height: SCENE_H,
    }
    .read_back();
    assert!(
        scene.iter().any(|&pixel| pixel != 0x0000ff),
        "GPU nav primitives must change the scene target"
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

    assert!(
        core.world
            .add_scenery(0, 2, 3, 0, (2 << 29) | 1, 10, 1, 1, 0, 0, 0, 0, 0,),
        "static loc fixture must be added"
    );
    backend.nav_mesh.vertices.clear();
    backend.draw_scene_overlays(&mut core, &mut renderer);
    assert!(
        !backend.nav_mesh.vertices.is_empty(),
        "a static loc change must invalidate projected nav geometry"
    );

    core.set_nav_debug_paint(Some(Default::default()));
    backend.draw_scene_overlays(&mut core, &mut renderer);
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
    backend.draw_scene_overlays(&mut core, &mut renderer);
    assert!(!backend.nav_mesh.vertices.is_empty());
    core.set_nav_debug_paint(None);
    backend.draw_scene_overlays(&mut core, &mut renderer);
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
    TextureHandle {
        device: backend.context.device.clone(),
        queue: backend.context.queue.clone(),
        view: backend.scene_view.clone(),
        width: SCENE_W,
        height: SCENE_H,
    }
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
