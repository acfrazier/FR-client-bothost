//! The CPU rasterizer backend: the pixel-faithful Pix3D/Pix2D path moved
//! verbatim out of the renderer (task 4). The frame-stage bodies are the
//! old `game_draw` / `game_draw_main` / `title_screen_draw` split at the
//! `begin`/`scene`/`chrome` boundaries; the deep draw helpers (entity
//! passes, `draw_side`, `draw_interface`, the minimap compose, …) stay on
//! `Renderer` in `render/draw.rs` and are reached through `r`. `self.x`
//! became `r.x` and `client` became `core`; nothing else changed, so the
//! composited `draw_area` is byte-identical to the pre-task renderer.

use crate::client::client::Client;
use crate::graphics::{Colour, Pix2D, Pix3D};
use crate::render::backend::{FrameKind, FrameOutput, RenderBackend};
use crate::render::draw::get_av_h;
use crate::render::Renderer;
use crate::util::JString;

/// The software backend: draws with `Pix3D`/`Pix2D` into the renderer's
/// framebuffers. Holds no state of its own — the renderer's draw state
/// stays on `Renderer` (the tests read `area_game`/`area_side`/`pix3d`…
/// as renderer fields), and each stage operates on it via `r`.
pub struct CpuBackend;

impl RenderBackend for CpuBackend {
    /// Frame start. In-game: `game_draw`'s pre-draw setup — the
    /// `scroll_cycle` tick, the deferred brightness re-gamma, and
    /// `prepare_game` (the chrome areas/sprites), then the `redraw_frame`
    /// compositing of the chrome strips and the frozen frame while the
    /// scene loads. Title screen: the logout teardown (`unload_title`, a
    /// nulled `image_title2` so `prepare_title` reallocates the 9 regions,
    /// and a one-shot `draw_area` cls) and `prepare_title`.
    fn begin(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        if kind == FrameKind::Title {
            if !core.ingame && core.redraw_frame {
                r.unload_title();
                r.image_title2 = None;
                r.draw_area.fill(0);
            }
            r.prepare_title(core);
        } else {
            // TS GameShell ticks `scrollCycle` each mainloop pass while the
            // mouse is held (Client.ts 2341-2343); 0/1 here is enough for the
            // held-arrow scrollbar repeat.
            core.scroll_cycle = if core.shell.mouse_button != 0 { 1 } else { 0 };
            // `apply_clientcode` (sim) defers the brightness re-gamma to the
            // renderer's texel state (task-2b bridge); the texture averages the
            // sim's `finish_build` reads are refreshed with it.
            if let Some(brightness) = core.pending_brightness.take() {
                r.pix3d.init_texture_palettes(brightness);
                r.pix3d.refresh_texture_averages();
                core.tex_average = r.pix3d.tex_average;
            }
            // TS `buildMinimenu` (hover walk + menu options) runs from
            // `other_overlays` when no menu is open (Client.ts 4865-4867),
            // ahead of this frame's side/chat draws.
            r.prepare_game(core);

            if core.redraw_frame {
                core.redraw_frame = false;

                if let Some(b) = &r.media.area_backleft1 {
                    b.blit_into(&mut r.draw_area, 0, 4);
                }
                if let Some(b) = &r.media.area_backleft2 {
                    b.blit_into(&mut r.draw_area, 0, 357);
                }
                if let Some(b) = &r.media.area_backright1 {
                    b.blit_into(&mut r.draw_area, 722, 4);
                }
                if let Some(b) = &r.media.area_backright2 {
                    b.blit_into(&mut r.draw_area, 743, 205);
                }
                if let Some(b) = &r.media.area_backtop1 {
                    b.blit_into(&mut r.draw_area, 0, 0);
                }
                if let Some(b) = &r.media.area_backvmid1 {
                    b.blit_into(&mut r.draw_area, 516, 4);
                }
                if let Some(b) = &r.media.area_backvmid2 {
                    b.blit_into(&mut r.draw_area, 516, 205);
                }
                if let Some(b) = &r.media.area_backvmid3 {
                    b.blit_into(&mut r.draw_area, 496, 357);
                }
                if let Some(b) = &r.media.area_backhmid2 {
                    b.blit_into(&mut r.draw_area, 0, 338);
                }

                core.redraw_icons = true;
                core.redraw_side = true;
                core.redraw_chat = true;
                core.redraw_chat_mode = true;

                if core.scene_state != 2 {
                    if let Some(g) = &r.area_game {
                        g.blit_into(&mut r.draw_area, 4, 4);
                    }
                    if let Some(m) = &r.area_map {
                        m.blit_into(&mut r.draw_area, 550, 4);
                    }
                    // Map under chrome: `area_backvmid1` (34×156 at (516, 4))
                    // borders the 172×156 rect. In the 274 layout the strips do
                    // not overlap it, so this re-blit is a no-op guard that
                    // keeps an opaque `area_map` from covering the chrome.
                    if let Some(b) = &r.media.area_backvmid1 {
                        b.blit_into(&mut r.draw_area, 516, 4);
                    }
                }
            }
        }
    }

    /// `gameDrawMain` from client-ts (4172): the 3D pass, run only
    /// in-game with the scene built. Adds the players, NPCs and projectiles
    /// as dynamic sprites, follows the orbit camera (or the cutscene camera
    /// while `cinema_cam`), applies the per-frame `camShake` jitter,
    /// renders the world into `area_game` (`Pix2D.cls()` + `render_all` +
    /// `removeSprites`, the TS 4238-4245 sequence); `composite_scene`
    /// blits it at (4, 4).
    /// `World.resetVisCalc` runs once on the first pass (TS runs it from the
    /// game-loading flow) so `render_all`'s visibility backing is
    /// populated. The overlay passes are no-ops while their lists/sprites
    /// are not ported; the fps pass is not ported either. `otherOverlays`
    /// (the main overlay and modal, TS 4250) draws into `area_game` before
    /// the blit.
    fn scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        if kind != FrameKind::Game || core.scene_state != 2 {
            return;
        }

        r.scene_cycle += 1;

        r.add_players(core, true);
        r.add_npcs(core, true);
        r.add_players(core, false);
        r.add_npcs(core, false);
        r.add_projectiles(core);
        r.add_map_anim(core);

        // Camera (TS 4183-4195): a cutscene camera skips the orbit follow;
        // otherwise the orbit camera follows the local player.
        let mut pitch = core.orbit_camera_pitch;
        if core.camera_pitch_clamp / 256 > pitch {
            pitch = core.camera_pitch_clamp / 256;
        }
        if core.cam_shake[4] && core.cam_shake_ran[4] + 128 > pitch {
            pitch = core.cam_shake_ran[4] + 128;
        }
        let yaw = (core.orbit_camera_yaw + core.macro_camera_angle) & 0x7ff;

        if !core.cinema_cam {
            if let Some(player) = &core.local_player {
                let target_y = get_av_h(
                    &core.groundh,
                    &core.mapl,
                    player.x,
                    player.z,
                    core.minusedlevel,
                ) - 50;
                r.cam_follow(
                    core,
                    pitch,
                    yaw,
                    core.orbit_camera_x,
                    target_y,
                    core.orbit_camera_z,
                    pitch * 3 + 600,
                );
            }
        }

        // TS 4197-4203: the cutscene camera uses roofCheck2's eye height.
        let level = if core.cinema_cam {
            r.roof_check2(core)
        } else {
            r.roof_check(core)
        };

        // TS 4205-4209: snapshot the pre-jitter eye so it can be restored
        // after the pass (the camShake jitter is per-frame only).
        let eye_x = core.cam_x;
        let eye_y = core.cam_y;
        let eye_z = core.cam_z;
        let eye_pitch = core.cam_pitch;
        let eye_yaw = core.cam_yaw;

        // TS 4211-4235: the camShake jitter applies to the rendered eye.
        let (cam_x, cam_y, cam_z, cam_pitch, cam_yaw) =
            r.cam_shake_jitter(core, eye_x, eye_y, eye_z, eye_pitch, eye_yaw);

        // `World.resetVisCalc` (Client.ts loadGame 1222-1235): once per
        // game, so `vis_backing` is populated before `render_all` binds its
        // pitch/yaw row.
        if !r.vis_calc_done {
            r.vis_calc_done = true;
            let mut distance = [0i32; 9];
            for (x, slot) in distance.iter_mut().enumerate() {
                let angle = x as i32 * 32 + 128 + 15;
                let offset = angle * 3 + 600;
                let sin = Pix3D::sin_table().get(angle as usize).copied().unwrap_or(0);
                *slot = (offset * sin) >> 16;
            }
            r.world.reset_vis_calc(&distance, 500, 800, 512, 334);
        }

        // TS 4238-4242: the model picking state for this frame.
        let cycle = r.pix3d.cycle;
        r.pix3d.mouse_check = true;
        r.pix3d.picked_count = 0;
        r.pix3d.mouse_x = core.shell.mouse_x - 4;
        r.pix3d.mouse_y = core.shell.mouse_y - 4;

        // `Pix2D.cls()` on area_game, `Pix3D.setClipping(512, 334)`, then
        // the world pass (TS 4238-4245).
        let cache = &core.cache;
        let loop_cycle = core.loop_cycle;
        let (pix3d, world) = (&mut r.pix3d, &mut r.world);
        if let Some(game) = r.area_game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            surface.cls();
            pix3d.set_clipping(game.width, game.height);
            world.render_all(
                &mut core.world,
                pix3d,
                &mut surface,
                cache,
                loop_cycle,
                cam_x,
                cam_y,
                cam_z,
                level,
                cam_yaw,
                cam_pitch,
            );
        }
        world.remove_sprites(&mut core.world);

        r.entity_overlays(core);
        r.coord_arrow(core);
        r.texture_run_anims(core, cycle);

        // Mirror this frame's 3D pick list onto `Client` BEFORE the menu
        // build: `other_overlays` → `build_minimenu` → `add_world_options`
        // consumes the picks in the same pass (task-2b fix round 1; the old
        // end-of-frame copy was one frame stale).
        core.pick_count = r.pix3d.picked_count;
        core.pick_typecodes
            .copy_from_slice(&r.pix3d.picked_entity_typecode);
        r.other_overlays(core);

        // TS 4252-4257: restore the pre-jitter eye.
        core.cam_x = eye_x;
        core.cam_y = eye_y;
        core.cam_z = eye_z;
        core.cam_pitch = eye_pitch;
        core.cam_yaw = eye_yaw;
    }

    /// The (4, 4) `area_game` blit — the task-7 composite seam. The 3D
    /// pass rendered into `area_game` (and the overlay passes drew on top
    /// of it); this lands it in `draw_area` ahead of the 2D chrome, which
    /// is the same point the wgpu backend blits its scene texture at.
    fn composite_scene(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        if kind == FrameKind::Game && core.scene_state == 2 {
            if let Some(game) = &r.area_game {
                game.blit_into(&mut r.draw_area, 4, 4);
            }
        }
    }

    /// 2D chrome. In-game: the rest of `game_draw` after the 3D pass — the
    /// side/chat minimenu triggers, the `draw_side`/`draw_chat` redraw
    /// logic (scrollbar, modal, drag), `minimapDraw` (11279) into `area_map`
    /// with its (550, 4) blit and the chrome re-blit guard, the
    /// icon-strip redraw, and the chat-mode buttons on `backbase1`.
    /// Title screen: `titleScreenDraw` (1489)'s compositing — the login UI
    /// into `image_title4` (360×200), the title regions into `draw_area`
    /// (2/3/5/6/7/8 only while `redraw_frame` is set), and the torch
    /// columns over `image_title0/1`.
    fn chrome(&mut self, core: &mut Client, r: &mut Renderer, kind: FrameKind) {
        if kind == FrameKind::Title {
            let w = 360;
            let h = 200;
            if let Some(map4) = r.image_title4.as_mut() {
                let mut surface = Pix2D::with_pixels(&mut map4.pixels, w, h);

                if let Some(titlebox) = &r.image_titlebox {
                    titlebox.plot_sprite(&mut surface, 0, 0);
                }

                if core.loginscreen == 0 {
                    let extra_y = (h / 2) + 80;
                    let mut y = (h / 2) - 20;

                    if let Some(od) = core.on_demand.as_ref() {
                        let message = od.message.clone();
                        if let Some(p11) = r.media.p11.as_ref() {
                            p11.centre_string_tag(
                                &mut surface,
                                &message,
                                w / 2,
                                extra_y,
                                0x75a9a9,
                                true,
                            );
                        }
                    }

                    if let Some(b12) = r.media.b12.as_ref() {
                        b12.centre_string_tag(
                            &mut surface,
                            "Welcome to RuneScape",
                            w / 2,
                            y,
                            Colour::YELLOW,
                            true,
                        );
                    }

                    let mut x = (w / 2) - 80;
                    y = (h / 2) + 20;
                    if let Some(button) = &r.image_titlebutton {
                        button.plot_sprite(&mut surface, x - 73, y - 20);
                    }
                    if let Some(b12) = r.media.b12.as_ref() {
                        b12.centre_string_tag(
                            &mut surface,
                            "New User",
                            x,
                            y + 5,
                            Colour::WHITE,
                            true,
                        );
                    }

                    x = (w / 2) + 80;
                    if let Some(button) = &r.image_titlebutton {
                        button.plot_sprite(&mut surface, x - 73, y - 20);
                    }
                    if let Some(b12) = r.media.b12.as_ref() {
                        b12.centre_string_tag(
                            &mut surface,
                            "Existing User",
                            x,
                            y + 5,
                            Colour::WHITE,
                            true,
                        );
                    }
                } else if core.loginscreen == 2 {
                    let mut y = (h / 2) - 40;
                    if let Some(b12) = r.media.b12.as_ref() {
                        if core.login_mes1.is_empty() {
                            b12.centre_string_tag(
                                &mut surface,
                                &core.login_mes2,
                                w / 2,
                                y - 7,
                                Colour::YELLOW,
                                true,
                            );
                        } else {
                            b12.centre_string_tag(
                                &mut surface,
                                &core.login_mes1,
                                w / 2,
                                y - 15,
                                Colour::YELLOW,
                                true,
                            );
                            b12.centre_string_tag(
                                &mut surface,
                                &core.login_mes2,
                                w / 2,
                                y,
                                Colour::YELLOW,
                                true,
                            );
                        }
                        y += 30;

                        let user_line = format!(
                            "Username: {}{}",
                            core.login_user,
                            if core.login_select == 0 && core.loop_cycle % 40 < 20 {
                                "@yel@|"
                            } else {
                                ""
                            }
                        );
                        b12.draw_string_tag(
                            &mut surface,
                            &user_line,
                            w / 2 - 90,
                            y,
                            Colour::WHITE,
                            true,
                        );
                        y += 15;

                        let pass_line = format!(
                            "Password: {}{}",
                            JString::get_repeated_character(&core.login_pass),
                            if core.login_select == 1 && core.loop_cycle % 40 < 20 {
                                "@yel@|"
                            } else {
                                ""
                            }
                        );
                        b12.draw_string_tag(
                            &mut surface,
                            &pass_line,
                            w / 2 - 88,
                            y,
                            Colour::WHITE,
                            true,
                        );
                    }

                    let x = (w / 2) - 80;
                    let y = (h / 2) + 50;
                    if let Some(button) = &r.image_titlebutton {
                        button.plot_sprite(&mut surface, x - 73, y - 20);
                    }
                    if let Some(b12) = r.media.b12.as_ref() {
                        b12.centre_string_tag(&mut surface, "Login", x, y + 5, Colour::WHITE, true);
                    }

                    let x = (w / 2) + 80;
                    if let Some(button) = &r.image_titlebutton {
                        button.plot_sprite(&mut surface, x - 73, y - 20);
                    }
                    if let Some(b12) = r.media.b12.as_ref() {
                        b12.centre_string_tag(
                            &mut surface,
                            "Cancel",
                            x,
                            y + 5,
                            Colour::WHITE,
                            true,
                        );
                    }
                } else if core.loginscreen == 3 {
                    let x = w / 2;
                    let mut y = (h / 2) - 60;
                    if let Some(b12) = r.media.b12.as_ref() {
                        b12.centre_string_tag(
                            &mut surface,
                            "Create a free account",
                            x,
                            y,
                            Colour::YELLOW,
                            true,
                        );

                        y = (h / 2) - 35;
                        b12.centre_string_tag(
                            &mut surface,
                            "To create a new account you need to",
                            x,
                            y,
                            Colour::WHITE,
                            true,
                        );
                        y += 15;
                        b12.centre_string_tag(
                            &mut surface,
                            "go back to the main RuneScape webpage",
                            x,
                            y,
                            Colour::WHITE,
                            true,
                        );
                        y += 15;
                        b12.centre_string_tag(
                            &mut surface,
                            "and choose the red 'create account'",
                            x,
                            y,
                            Colour::WHITE,
                            true,
                        );
                        y += 15;
                        b12.centre_string_tag(
                            &mut surface,
                            "button at the top right of that page.",
                            x,
                            y,
                            Colour::WHITE,
                            true,
                        );
                    }

                    let x = w / 2;
                    let y = (h / 2) + 50;
                    if let Some(button) = &r.image_titlebutton {
                        button.plot_sprite(&mut surface, x - 73, y - 20);
                    }
                    if let Some(b12) = r.media.b12.as_ref() {
                        b12.centre_string_tag(
                            &mut surface,
                            "Cancel",
                            x,
                            y + 5,
                            Colour::WHITE,
                            true,
                        );
                    }
                }
            }

            if let Some(t4) = &r.image_title4 {
                t4.blit_into(&mut r.draw_area, 202, 171);
            }

            if core.redraw_frame {
                core.redraw_frame = false;
                if let Some(t2) = &r.image_title2 {
                    t2.blit_into(&mut r.draw_area, 128, 0);
                }
                if let Some(t3) = &r.image_title3 {
                    t3.blit_into(&mut r.draw_area, 202, 371);
                }
                if let Some(t5) = &r.image_title5 {
                    t5.blit_into(&mut r.draw_area, 0, 265);
                }
                if let Some(t6) = &r.image_title6 {
                    t6.blit_into(&mut r.draw_area, 562, 265);
                }
                if let Some(t7) = &r.image_title7 {
                    t7.blit_into(&mut r.draw_area, 128, 171);
                }
                if let Some(t8) = &r.image_title8 {
                    t8.blit_into(&mut r.draw_area, 562, 171);
                }
            }

            // TitleFlames.ts drawFlames: the torch columns redraw every frame
            // (TS PixMap.draw onto the canvas). Inactive flames still blit the
            // JPEG background that loadTitleBackground plotted into 0/1.
            r.tick_title_flames(core);
            if let Some(t0) = &r.image_title0 {
                t0.blit_into(&mut r.draw_area, 0, 0);
            }
            if let Some(t1) = &r.image_title1 {
                t1.blit_into(&mut r.draw_area, 637, 0);
            }
        } else {
            // TS 3924-3926: an open side minimenu redraws the side panel every
            // frame so the hover-highlighted option rows track the pointer.
            if core.is_menu_open && core.menu_area == 1 {
                core.redraw_side = true;
            }

            // `sideModalId`/`animateInterface` redrawSide trigger (TS 3931-3937).
            if core.side_modal_id != -1
                && r.animate_interface(core, core.side_modal_id, core.world_update_num)
            {
                core.redraw_side = true;
            }

            // TS 3935-3941: the OP_HELD outline and the in-flight obj drag
            // redraw the side panel every frame.
            if core.selected_area == 2 {
                core.redraw_side = true;
            }
            if core.obj_drag_area == 2 {
                core.redraw_side = true;
            }

            if core.redraw_side {
                r.draw_side(core);
                core.redraw_side = false;
            }

            // `chatModalId`/`animateInterface` redrawChat trigger (TS 3966-3971).

            // TS 3948-3967: with no chat modal the chat scrollbar is live. The
            // held-arrow step goes through `chat_interface` (a synthetic IfType,
            // `com_id` -1), then `chat_scroll_pos` is re-derived from it.
            if core.chat_modal_id == -1 {
                core.chat_interface.scroll_pos =
                    core.chat_scroll_height - core.chat_scroll_pos - 77;
                core.chat_interface.scroll_height = core.chat_scroll_height;
                if core.shell.mouse_x > 448 && core.shell.mouse_x < 560 && core.shell.mouse_y > 332
                {
                    core.do_scrollbar(
                        core.shell.mouse_x - 17,
                        core.shell.mouse_y - 357,
                        core.chat_scroll_height,
                        77,
                        false,
                        463,
                        0,
                        -1,
                    );
                }
                let mut offset = core.chat_scroll_height - core.chat_interface.scroll_pos - 77;
                if offset < 0 {
                    offset = 0;
                }
                if offset > core.chat_scroll_height - 77 {
                    offset = core.chat_scroll_height - 77;
                }
                if core.chat_scroll_pos != offset {
                    core.chat_scroll_pos = offset;
                    core.redraw_chat = true;
                }
            }

            if core.chat_modal_id != -1
                && r.animate_interface(core, core.chat_modal_id, core.world_update_num)
            {
                core.redraw_chat = true;
            }

            // TS 3977-3982: the OP_HELD outline and the in-flight obj drag
            // redraw the chat panel every frame.
            if core.selected_area == 3 {
                core.redraw_chat = true;
            }
            if core.obj_drag_area == 3 {
                core.redraw_chat = true;
            }

            // TS 3989-3991: an open chat minimenu redraws the chat panel every
            // frame so the hover-highlighted option rows track the pointer.
            if core.is_menu_open && core.menu_area == 2 {
                core.redraw_chat = true;
            }

            if core.redraw_chat {
                r.draw_chat(core);
                core.redraw_chat = false;
            }

            // `minimapDraw` (11279) into `area_map`, then the (550, 4) blit
            // (TS 3999-4001), then the chrome re-blit guard (see the
            // `redraw_frame` path above).
            if core.scene_state == 2 {
                r.minimap_draw(core);
                if let Some(m) = &r.area_map {
                    m.blit_into(&mut r.draw_area, 550, 4);
                }
                if let Some(b) = &r.media.area_backvmid1 {
                    b.blit_into(&mut r.draw_area, 516, 4);
                }
            }

            // `tutFlashIcon !== -1` redrawIcons trigger (TS 4003-4004).
            if core.tut_flash_icon != -1 {
                core.redraw_icons = true;
            }

            if core.redraw_icons {
                r.draw_icons(core);
                core.redraw_icons = false;
            }

            if core.redraw_chat_mode {
                core.redraw_chat_mode = false;
                // TS (4122): the chat mode buttons on `backbase1`, blitted at
                // (0, 453).
                if let Some(base) = r.area_backbase1.as_mut() {
                    let w = base.width;
                    let h = base.height;
                    let mut surface = Pix2D::with_pixels(&mut base.pixels, w, h);
                    if let Some(backbase1) = &r.media.backbase1 {
                        backbase1.plot_sprite(&mut surface, 0, 0);
                    }
                    if let Some(p12) = r.media.p12.as_ref() {
                        p12.centre_string_tag(
                            &mut surface,
                            "Public chat",
                            55,
                            28,
                            Colour::WHITE,
                            true,
                        );
                        let (label, rgb) = match core.chat_public_mode {
                            1 => ("Friends", Colour::YELLOW),
                            2 => ("Off", Colour::RED),
                            3 => ("Hide", Colour::CYAN),
                            _ => ("On", Colour::GREEN),
                        };
                        p12.centre_string_tag(&mut surface, label, 55, 41, rgb, true);
                        p12.centre_string_tag(
                            &mut surface,
                            "Private chat",
                            184,
                            28,
                            Colour::WHITE,
                            true,
                        );
                        let (label, rgb) = match core.chat_private_mode {
                            1 => ("Friends", Colour::YELLOW),
                            2 => ("Off", Colour::RED),
                            _ => ("On", Colour::GREEN),
                        };
                        p12.centre_string_tag(&mut surface, label, 184, 41, rgb, true);
                        p12.centre_string_tag(
                            &mut surface,
                            "Trade/duel",
                            324,
                            28,
                            Colour::WHITE,
                            true,
                        );
                        let (label, rgb) = match core.chat_trade_mode {
                            1 => ("Friends", Colour::YELLOW),
                            2 => ("Off", Colour::RED),
                            _ => ("On", Colour::GREEN),
                        };
                        p12.centre_string_tag(&mut surface, label, 324, 41, rgb, true);
                        p12.centre_string_tag(
                            &mut surface,
                            "Report abuse",
                            458,
                            33,
                            Colour::WHITE,
                            true,
                        );
                    }
                }
                if let Some(base) = &r.area_backbase1 {
                    base.blit_into(&mut r.draw_area, 0, 453);
                }
            }

            // TS 4169: `worldUpdateNum = 0` at the end of the drawn frame.
            core.world_update_num = 0;
        }
    }

    /// The composited frame, owned by the backend. `CpuBackend` keeps
    /// compositing into the renderer's `draw_area` (the tests and the
    /// `window` blit read it there) and hands back an owned copy; a
    /// `GpuBackend` (task 7) returns `FrameOutput::Texture` instead.
    fn finish(&mut self, r: &mut Renderer) -> FrameOutput {
        FrameOutput::PixMap(r.draw_area.clone())
    }
}
