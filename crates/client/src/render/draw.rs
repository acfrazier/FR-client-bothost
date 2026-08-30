//! Frame draw (Tasks 4/5: title, then in-game). 1:1 port of `Client.ts`
//! `prepareTitle`, `loadTitleBackground`, `loadTitleImages`, `TitleFlames`,
//! `titleScreenDraw` (1489–1694), `gameDraw` with its `prepareGame`/
//! `drawSide`/`drawChat` helpers (3890–4170, 2001, 11098, 11125), and the
//! `gameDrawMain` 3D pass (4172–4251): `addPlayers`/`addNpcs`, the orbit
//! camera (`camFollow`), `World.resetVisCalc` + `render_all` into
//! `area_game`, and the (4, 4) blit. `minimapDraw` (11279) rotates the
//! composed minimap buffer and the compass into `area_map` (blitted at
//! (550, 4) under the chrome; the `area_backvmid1` strip is re-blitted on
//! top as a z-order guard). Draws always into `Renderer::draw_area`
//! (765×503 `PixMap`); present (feature `window`) only blits.
//!
//! Task 2b: every method here is `impl Renderer` taking `&mut Client` (or
//! `&Client` for read-only) for the sim state it reads; `Client` no longer
//! owns the renderer. The sim-adjacent scene passes `check_minimap`/
//! `check_scene`/`map_build`/`minimap_build_buffer` and `mainredraw` also
//! live here so `mainloop`/`game_loop` stay renderer-free.
//!
//! Task 4: the frame-stage bodies (`game_draw`/`game_draw_main`/
//! `title_screen_draw`) moved to `render/backend/cpu.rs` behind the
//! `RenderBackend` trait; `Renderer::game_draw`/`title_screen_draw`
//! delegate. The deep draw helpers below stay here and are reached from
//! the backend through `&mut Renderer` (`pub(crate)`).
//!
//! The minimap `mapback` ring and mask build (1180–1216) land in
//! `prepare_game`; `drawInterface` draws the side-tab interfaces
//! (`TYPE_LAYER`/`TYPE_RECT`/`TYPE_TEXT`/`TYPE_GRAPHIC`).

use std::collections::HashMap;

use crate::client::client::{level_experience, Client};
use crate::client::client_build::random_float;
use crate::config::if_type::{default_mut, ButtonType, ComponentType, IfTypeMut, IfTypeView};
use crate::client::skill::Skill;
use crate::client::title_flames::TitleFlames;
use crate::config::{Cache, ObjType};
use crate::core::world::LevelHeightmaps;
use crate::core::World;
use crate::dash3d::client_entity::ClientEntity;
use crate::dash3d::{BuildArea, CollisionFlag, LocAngle, LocShape, MapFlag, SceneModel};
use crate::graphics::{Colour, Pix2D, Pix3D, Pix32, Pix8, PixMap};
use crate::io::{ClientProt, JagFile};
use crate::render::backend::FrameOutput;
use crate::render::media::Media;
use crate::render::Renderer;
use crate::util::JString;

fn plot_title_bg(map: &mut Option<PixMap>, background: &Pix32, x: i32, y: i32) {
    if let Some(map) = map {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        background.quick_plot_sprite(&mut surface, x, y);
    }
}

/// `CHAT_COLOURS` from Java (307): the six static bubble colours, keyed by
/// `entity.chat_colour` 0-5 (6-11 are the animated ones).
const CHAT_COLOURS: [i32; 6] = [
    Colour::YELLOW,
    Colour::RED,
    Colour::GREEN,
    Colour::CYAN,
    Colour::MAGENTA,
    Colour::WHITE,
];

/// `Client.getAvH` from client-ts (5052): the bilinear ground height at a
/// scene position. A `LinkBelow` flag on the level-1 map lifts the height
/// to the level above, so sprites on a tall loc's upper floor read the
/// upper-floor height instead of the ground's.
pub(crate) fn get_av_h(
    groundh: &LevelHeightmaps,
    mapl: &[Vec<Vec<u8>>],
    scene_x: i32,
    scene_z: i32,
    level: i32,
) -> i32 {
    let tile_x = scene_x >> 7;
    let tile_z = scene_z >> 7;
    if tile_x < 0 || tile_z < 0 || tile_x > 103 || tile_z > 103 {
        return 0;
    }

    // TS 5052: `level < 3 && mapl[1][tileX][tileZ] & LinkBelow != 0`.
    let real_level = if level < 3
        && (mapl[1][tile_x as usize][tile_z as usize] & MapFlag::LINK_BELOW as u8) != 0
    {
        level + 1
    } else {
        level
    };
    let tile_local_x = scene_x & 0x7f;
    let tile_local_z = scene_z & 0x7f;
    let y00 = (groundh[real_level as usize][tile_x as usize][tile_z as usize]
        * (128 - tile_local_x)
        + groundh[real_level as usize][(tile_x + 1) as usize][tile_z as usize] * tile_local_x)
        >> 7;
    let y11 = (groundh[real_level as usize][tile_x as usize][(tile_z + 1) as usize]
        * (128 - tile_local_x)
        + groundh[real_level as usize][(tile_x + 1) as usize][(tile_z + 1) as usize]
            * tile_local_x)
        >> 7;
    (y00 * (128 - tile_local_z) + y11 * tile_local_z) >> 7
}

impl Renderer {

    /// `drawProgress` from client-ts (3840): the loading-progress bar.
    /// Always records `last_progress_percent`/`last_progress_message`;
    /// with `draw` off that is all it does. With `draw` on and no title
    /// pixmaps yet, paints the `GameShell.drawProgress` fallback (274)
    /// into `draw_area`: 304×34 outline, `progress * 3` × 30 fill in
    /// `0x8c1111`, remainder black, centred at `(width/2, height/2 - 18)`
    /// (the message text no-ops while `b12` is not loaded). With title
    /// pixmaps present, draws the TS 3840 bar on `image_title4` (360×200)
    /// and blits the title regions as TS 3868-3883, compositing
    /// `draw_area` the way `title_screen_draw` does.
    pub fn draw_progress(&mut self, client: &mut Client, message: &str, progress: i32) {
        client.last_progress_percent = progress;
        client.last_progress_message = message.to_string();

        if !client.draw {
            return;
        }

        // Java `Client.messageBox`: prepareTitle() then the title-framed bar
        // with b12. Without this, maininit stays on the GameShell fallback
        // (no fonts) after the title jag is already on disk.
        self.prepare_title(client);

        if self.image_title4.is_none() {
            let width = self.draw_area.width;
            let height = self.draw_area.height;
            let y = (height / 2) - 18;
            let mid_x = width / 2;
            let mut surface = Pix2D::with_pixels(&mut self.draw_area.pixels, width, height);
            surface.draw_rect(mid_x - 152, y, 304, 34, 0x8c1111);
            surface.fill_rect(mid_x - 150, y + 2, progress * 3, 30, 0x8c1111);
            surface.fill_rect(
                mid_x - 150 + progress * 3,
                y + 2,
                300 - progress * 3,
                30,
                Colour::BLACK,
            );
            if let Some(b12) = self.media.b12.as_ref() {
                b12.centre_string(&mut surface, Some(message), mid_x, y + 22, Colour::WHITE);
            }
            self.present_progress(client);
            return;
        }

        // TS 3840-3866: the loading bar on image_title4.
        let w = 360;
        let h = 200;
        let offset_y = 20;
        if let Some(map4) = self.image_title4.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut map4.pixels, w, h);
            if let Some(b12) = self.media.b12.as_ref() {
                b12.centre_string(
                    &mut surface,
                    Some("RuneScape is loading - please wait..."),
                    w / 2,
                    (h / 2) - offset_y - 26,
                    Colour::WHITE,
                );
            }
            let mid_y = (h / 2) - 18 - offset_y;
            surface.draw_rect((w / 2) - 152, mid_y, 304, 34, 0x8c1111);
            surface.draw_rect((w / 2) - 151, mid_y + 1, 302, 32, Colour::BLACK);
            surface.fill_rect((w / 2) - 150, mid_y + 2, progress * 3, 30, 0x8c1111);
            surface.fill_rect(
                (w / 2) - 150 + progress * 3,
                mid_y + 2,
                300 - progress * 3,
                30,
                Colour::BLACK,
            );
            if let Some(b12) = self.media.b12.as_ref() {
                b12.centre_string(
                    &mut surface,
                    Some(message),
                    w / 2,
                    (h / 2) + 5 - offset_y,
                    Colour::WHITE,
                );
            }
        }

        // TS 3868-3883 / Java messageBox: title4 always; chrome 2/3/5/6/7/8
        // on redraw_frame. Java's flame *thread* paints titleLeft/titleRight
        // while loading; we must not `tick_title_flames` here (same thread
        // as OnDemand wait → visible lag). Blit the JPEG already in 0/1
        // from loadTitleBackground; animation starts in title_screen_draw.
        if let Some(t4) = &self.image_title4 {
            t4.blit_into(&mut self.draw_area, 202, 171);
        }

        if client.redraw_frame {
            client.redraw_frame = false;
            if let Some(t2) = &self.image_title2 {
                t2.blit_into(&mut self.draw_area, 128, 0);
            }
            if let Some(t3) = &self.image_title3 {
                t3.blit_into(&mut self.draw_area, 202, 371);
            }
            if let Some(t5) = &self.image_title5 {
                t5.blit_into(&mut self.draw_area, 0, 265);
            }
            if let Some(t6) = &self.image_title6 {
                t6.blit_into(&mut self.draw_area, 562, 265);
            }
            if let Some(t7) = &self.image_title7 {
                t7.blit_into(&mut self.draw_area, 128, 171);
            }
            if let Some(t8) = &self.image_title8 {
                t8.blit_into(&mut self.draw_area, 562, 171);
            }
        }

        if let Some(t0) = &self.image_title0 {
            t0.blit_into(&mut self.draw_area, 0, 0);
        }
        if let Some(t1) = &self.image_title1 {
            t1.blit_into(&mut self.draw_area, 637, 0);
        }

        self.present_progress(client);
    }

    /// Java `messageBox` presents immediately. `run` only presents after
    /// maininit, so the loading bar has to present here or the title looks
    /// like loading already finished.
    fn present_progress(&mut self, client: &mut Client) {
        if let Some(present) = client.present.as_mut() {
            let _ = present.poll(&mut client.shell);
            present.present(FrameOutput::PixMap(self.draw_area.clone()));
        }
    }

    pub(crate) fn tick_title_flames(&mut self, client: &mut Client) {
        let Some(flames) = self.title_flames.as_mut() else {
            return;
        };
        if !flames.active {
            return;
        }
        let (Some(left), Some(right)) = (self.image_title0.as_mut(), self.image_title1.as_mut()) else {
            return;
        };
        flames.render_flames(left, right, client.loop_cycle);
    }

    /// `prepareTitle` from client-ts (1579): create the 9 title `PixMap`
    /// regions (sizes as TS) on the first frame, load the `title` jag from
    /// the cache, the four fonts, and the titlebox/titlebutton sprites.
    /// `title_screen_draw`'s logout teardown nulls `image_title2` (the gate
    /// below), so the next title draw reallocates the regions like Java
    /// `prepareTitle` after `prepareGame` dropped them.
    pub(crate) fn prepare_title(&mut self, client: &mut Client) {
        if self.image_title2.is_some() {
            return;
        }

        // Java `prepareTitle` (Client.java 1481-1488) nulls the game-frame
        // areas before allocating the title regions, so a second login
        // re-runs `prepareGame` instead of early-returning on a surviving
        // `areaChatback`. `draw_area` stays: Rust keeps one compositor
        // PixMap and the logout teardown cls it.
        self.area_chat = None;
        self.area_map = None;
        self.area_side = None;
        self.area_game = None;
        self.area_backbase1 = None;
        self.area_backbase2 = None;
        self.area_backhmid1 = None;

        self.image_title0 = Some(PixMap::new(128, 265));
        self.image_title1 = Some(PixMap::new(128, 265));
        self.image_title2 = Some(PixMap::new(509, 171));
        self.image_title3 = Some(PixMap::new(360, 132));
        self.image_title4 = Some(PixMap::new(360, 200));
        self.image_title5 = Some(PixMap::new(202, 238));
        self.image_title6 = Some(PixMap::new(203, 238));
        self.image_title7 = Some(PixMap::new(74, 94));
        self.image_title8 = Some(PixMap::new(75, 94));

        // The fonts + title content are the process-wide `Media` (task 6):
        // the first title draw decoded them once; each head re-plots its
        // own regions from the shared decodes.
        self.media = Media::process(&client.config.cache_dir);
        self.load_title_background();
        self.load_title_images();

        client.redraw_frame = true;
    }

    /// The fonts live on the process-wide `Media` (task 6): resolve the
    /// shared copy. TS `maininit` 848 loads the four fonts before both
    /// title and game draw, so an in-game client that never drew the title
    /// still has `p12` for the chat-mode labels.
    fn load_fonts(&mut self, client: &mut Client) {
        self.media = Media::process(&client.config.cache_dir);
    }

    /// `loadTitleBackground` from client-ts (1627): JPEG `title.dat` tiled
    /// across the 9 title regions, mirrored, then the `logo` sprite. The
    /// JPEG + mirror are the shared `Media` decodes; the regions are this
    /// head's copies (the torch flames paint into 0/1).
    fn load_title_background(&mut self) {
        if let Some(background) = &self.media.title_dat {
            plot_title_bg(&mut self.image_title0, background, 0, 0);
            plot_title_bg(&mut self.image_title1, background, -637, 0);
            plot_title_bg(&mut self.image_title2, background, -128, 0);
            plot_title_bg(&mut self.image_title3, background, -202, -371);
            plot_title_bg(&mut self.image_title4, background, -202, -171);
            plot_title_bg(&mut self.image_title5, background, 0, -265);
            plot_title_bg(&mut self.image_title6, background, -562, -265);
            plot_title_bg(&mut self.image_title7, background, -128, -171);
            plot_title_bg(&mut self.image_title8, background, -562, -171);
        }
        if let Some(background) = &self.media.title_dat_flipped {
            plot_title_bg(&mut self.image_title0, background, 382, 0);
            plot_title_bg(&mut self.image_title1, background, -255, 0);
            plot_title_bg(&mut self.image_title2, background, 254, 0);
            plot_title_bg(&mut self.image_title3, background, 180, -371);
            plot_title_bg(&mut self.image_title4, background, 180, -171);
            plot_title_bg(&mut self.image_title5, background, 382, -265);
            plot_title_bg(&mut self.image_title6, background, -180, -265);
            plot_title_bg(&mut self.image_title7, background, 254, -171);
            plot_title_bg(&mut self.image_title8, background, -180, -171);
        }

        if let Some(logo) = &self.media.logo {
            if let Some(map2) = self.image_title2.as_mut() {
                let w = map2.width;
                let h = map2.height;
                let mut surface = Pix2D::with_pixels(&mut map2.pixels, w, h);
                logo.plot_sprite(
                    &mut surface,
                    (crate::client::client::APPLET_W / 2) - (logo.wi / 2) - 128,
                    18,
                );
            }
        }
    }

    /// `loadTitleImages` from client-ts (1697): the `titlebox` and
    /// `titlebutton` sprites plus the 12 `runes` sprites, cloned from the
    /// shared `Media` decodes (the shared copies are immutable).
    fn load_title_images(&mut self) {
        self.image_titlebox = self.media.titlebox.clone();
        self.image_titlebutton = self.media.titlebutton.clone();

        self.image_runes = self.media.runes.clone();
        if self.title_flames.is_none() {
            if let (Some(left), Some(right)) = (&self.image_title0, &self.image_title1) {
                let mut flames = TitleFlames::new(self.image_runes.clone());
                flames.setup_fire(left, right);
                flames.start();
                self.title_flames = Some(flames);
            }
        }
    }

    /// `unloadTitle` from client-ts (1992): drop the title sprites and stop
    /// the torch flames. TS calls this from `prepareGame`/`mainquit`.
    pub fn unload_title(&mut self) {
        if let Some(flames) = self.title_flames.as_mut() {
            flames.close();
        }
        self.title_flames = None;
        self.image_titlebox = None;
        self.image_titlebutton = None;
        self.image_runes.clear();
    }

    /// `drawPrivateMessages` from Client.ts (4915-4986): the split
    /// private-chat overlay, drawn into `area_game` when clientcode 8 set
    /// `split_private_chat`. Incoming (3/7) and sent (5/6) lines stack
    /// bottom-up from y 329 with the double-shadowed cyan text; the
    /// `modIcons` crown plots ahead of the sender as Java 6215-6221. An
    /// active `rebootTimer` reserves the first line for the "System update
    /// in" text (TS 4922-4924).
    fn draw_private_messages(&mut self, client: &mut Client, surface: &mut Pix2D) {
        if client.split_private_chat == 0 {
            return;
        }

        let mut line_offset = if client.reboot_timer != 0 { 1 } else { 0 };
        for i in 0..100 {
            if client.chat_text[i].is_empty() {
                continue;
            }
            let r#type = client.chat_type[i];
            let mut sender = client.chat_username[i].clone();
            let mut modlevel = 0;
            if sender.starts_with("@cr1@") {
                sender = sender[5..].to_string();
                modlevel = 1;
            } else if sender.starts_with("@cr2@") {
                sender = sender[5..].to_string();
                modlevel = 2;
            }

            if (r#type == 3 || r#type == 7)
                && (r#type == 7
                    || client.chat_private_mode == 0
                    || (client.chat_private_mode == 1 && client.is_friend(&sender)))
            {
                let y = 329 - line_offset * 13;
                let mut x = 4;
                if let Some(font) = self.media.p12.as_ref() {
                    font.draw_string(surface, Some("From"), 4, y, Colour::BLACK);
                    font.draw_string(surface, Some("From"), 4, y - 1, Colour::CYAN);
                    x += font.string_wid(Some("From "));
                }
                // Java 6215-6221: the crown plots after the "From " label.
                if modlevel == 1 {
                    if let Some(sprite) = &self.media.mod_icons[0] {
                        sprite.plot_sprite(surface, x, y - 12);
                    }
                    x += 14;
                }
                if modlevel == 2 {
                    if let Some(sprite) = &self.media.mod_icons[1] {
                        sprite.plot_sprite(surface, x, y - 12);
                    }
                    x += 14;
                }
                if let Some(font) = self.media.p12.as_ref() {
                    font.draw_string(surface, Some(&format!("{sender}: {}", client.chat_text[i])), x, y, Colour::BLACK);
                    font.draw_string(surface, Some(&format!("{sender}: {}", client.chat_text[i])), x, y - 1, Colour::CYAN);
                }
                line_offset += 1;
                if line_offset >= 5 {
                    return;
                }
            } else if r#type == 5 && client.chat_private_mode < 2 {
                let y = 329 - line_offset * 13;
                if let Some(font) = self.media.p12.as_ref() {
                    font.draw_string(surface, Some(&client.chat_text[i]), 4, y, Colour::BLACK);
                    font.draw_string(surface, Some(&client.chat_text[i]), 4, y - 1, Colour::CYAN);
                }
                line_offset += 1;
                if line_offset >= 5 {
                    return;
                }
            } else if r#type == 6 && client.chat_private_mode < 2 {
                let y = 329 - line_offset * 13;
                if let Some(font) = self.media.p12.as_ref() {
                    font.draw_string(surface, Some(&format!("To {sender}: {}", client.chat_text[i])), 4, y, Colour::BLACK);
                    font.draw_string(surface, Some(&format!("To {sender}: {}", client.chat_text[i])), 4, y - 1, Colour::CYAN);
                }
                line_offset += 1;
                if line_offset >= 5 {
                    return;
                }
            }
        }
    }

    /// `otherOverlays` from client-ts (4853): draw the main overlay then the
    /// main modal into `area_game` at (0, 0), ahead of the (4, 4) blit.
    /// `animateInterface` runs before each draw (TS 4853-4861); with no
    /// menu open `buildMinimenu` rebuilds the menu (the pointer walk plus
    /// the option strings) before `draw_feedback` (TS 4865-4867), and the
    /// open minimenu (area 0) draws after the modal (TS 4868-4870).
    pub(crate) fn other_overlays(&mut self, client: &mut Client) {
        let mut game = self.area_game.take();
        if let Some(game) = game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            // `draw_interface`'s TYPE_MODEL arm rasters into this surface:
            // bind `pix3d` clipping to it once before drawing.
            self.pix3d.set_clipping(surface.width, surface.height);
            // TS 4838: the split private-chat overlay draws first.
            self.draw_private_messages(client, &mut surface);
            // TS 4840-4843: the click crosshair plots the fade frame
            // (mode 1) or the op frame (mode 2) at the click point;
            // `cross_cycle/100` picks the sprite, missing media is a no-op.
            if client.cross_mode == 1 {
                let idx = (client.cross_cycle / 100) as usize;
                if let Some(s) = self.media.cross.get(idx).and_then(|o| o.as_ref()) {
                    s.plot_sprite(&mut surface, client.cross_x - 8 - 4, client.cross_y - 8 - 4);
                }
            } else if client.cross_mode == 2 {
                let idx = (client.cross_cycle / 100) as usize + 4;
                if let Some(s) = self.media.cross.get(idx).and_then(|o| o.as_ref()) {
                    s.plot_sprite(&mut surface, client.cross_x - 8 - 4, client.cross_y - 8 - 4);
                }
            }
            if client.main_overlay_id != -1 {
                self.animate_interface(client, client.main_overlay_id, client.world_update_num);
                self.draw_interface(client, client.main_overlay_id, 0, 0, 0, &mut surface);
            }
            if client.main_modal_id != -1 {
                self.animate_interface(client, client.main_modal_id, client.world_update_num);
                self.draw_interface(client, client.main_modal_id, 0, 0, 0, &mut surface);
            }
            if client.is_menu_open && client.menu_area == 0 {
                self.draw_minimenu(client, &mut surface);
            }
            // TS 4901-4911: the reboot countdown line, drawn after the
            // private-chat overlay (which reserves its row when active).
            if client.reboot_timer != 0 {
                let mut seconds = client.reboot_timer / 50;
                let minutes = seconds / 60;
                seconds %= 60;
                if let Some(p12) = self.media.p12.as_ref() {
                    if seconds < 10 {
                        p12.draw_string(
                            &mut surface,
                            Some(&format!("System update in: {minutes}:0{seconds}")),
                            4,
                            329,
                            Colour::YELLOW,
                        );
                    } else {
                        p12.draw_string(
                            &mut surface,
                            Some(&format!("System update in: {minutes}:{seconds}")),
                            4,
                            329,
                            Colour::YELLOW,
                        );
                    }
                }
            }
        }
        self.area_game = game;

        if !client.is_menu_open {
            client.build_minimenu();
            self.draw_feedback(client);
        }
    }

    /// `drawMinimenu` from client-ts (8383-8418): the menu box — `0x5d5447`
    /// fill, black title bar with `Choose Option`, then the options
    /// bottom-to-top (`menu_num_entries - 1 - i`), yellow when the pointer
    /// (offset to the panel's origin) sits in the option's row. The caller
    /// binds the panel surface holding the menu (0 viewport, 1 side,
    /// 2 chat).
    fn draw_minimenu(&mut self, client: &mut Client, surface: &mut Pix2D) {
        // Task 2b: `open_menu` (sim) measures without the `b12` font (it
        // lives here); re-measure from the font and re-clamp `menu_x`/
        // `menu_width` so the box and click rows use the real widths.
        if let Some(b12) = &self.media.b12 {
            let mut width = b12.string_wid(Some("Choose Option"));
            for i in 0..client.menu_num_entries {
                let w = b12.string_wid(Some(&client.menu_option[i as usize]));
                if w > width {
                    width = w;
                }
            }
            let width = width + 8;
            if width != client.menu_width {
                client.menu_width = width;
                let (origin, max) = match client.menu_area {
                    0 => (4, 512),
                    1 => (553, 190),
                    _ => (17, 479),
                };
                let click_x = client.shell.mouse_click_x;
                let mut x = click_x - (width / 2) - origin;
                if x + width > max {
                    x = max - width;
                }
                if x < 0 {
                    x = 0;
                }
                client.menu_x = x;
            }
        }
        let x = client.menu_x;
        let y = client.menu_y;
        let w = client.menu_width;
        let h = client.menu_height;
        let background: i32 = 0x5d5447;

        surface.fill_rect(x, y, w, h, background);
        surface.fill_rect(x + 1, y + 1, w - 2, 16, Colour::BLACK);
        surface.draw_rect(x + 1, y + 18, w - 2, h - 19, Colour::BLACK);

        let mut mouse_x = client.shell.mouse_x;
        let mut mouse_y = client.shell.mouse_y;
        if client.menu_area == 0 {
            mouse_x -= 4;
            mouse_y -= 4;
        } else if client.menu_area == 1 {
            mouse_x -= 553;
            mouse_y -= 205;
        } else if client.menu_area == 2 {
            mouse_x -= 17;
            mouse_y -= 357;
        }

        if let Some(b12) = &self.media.b12 {
            b12.draw_string(surface, Some("Choose Option"), x + 3, y + 14, background);

            for i in 0..client.menu_num_entries {
                let option_y = y + (client.menu_num_entries - 1 - i) * 15 + 31;

                let mut rgb = Colour::WHITE;
                if mouse_x > x
                    && mouse_x < x + w
                    && mouse_y > option_y - 13
                    && mouse_y < option_y + 3
                {
                    rgb = Colour::YELLOW;
                }

                b12.draw_string_tag(
                    surface,
                    &client.menu_option[i as usize],
                    x + 3,
                    option_y,
                    rgb,
                    true,
                );
            }
        }
    }

    /// `drawFeedback` from client-ts (8421-8439): the tooltip line — the
    /// last menu option, or the Use/Target hint — into `area_game` at
    /// (4, 15) via `b12` anti-macro. Drawn when no menu is open.
    fn draw_feedback(&mut self, client: &mut Client) {
        if client.menu_num_entries < 2 && client.use_mode == 0 && client.target_mode == 0 {
            return;
        }

        let tooltip = if client.use_mode == 1 && client.menu_num_entries < 2 {
            format!("Use {} with...", client.obj_selected_name)
        } else if client.target_mode == 1 && client.menu_num_entries < 2 {
            format!("{}...", client.target_op)
        } else {
            client.menu_option[(client.menu_num_entries - 1) as usize].clone()
        };

        let tooltip = if client.menu_num_entries > 2 {
            format!(
                "{}@whi@ / {} more options",
                tooltip,
                client.menu_num_entries - 2
            )
        } else {
            tooltip
        };

        let mut game = self.area_game.take();
        if let Some(game) = game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            if let Some(b12) = &self.media.b12 {
                b12.draw_string_anti_macro(
                    &mut surface,
                    &tooltip,
                    4,
                    15,
                    Colour::WHITE,
                    true,
                    (client.loop_cycle / 1000) as i32,
                );
            }
        }
        self.area_game = game;
    }

    /// `addPlayers` from client-ts (4260): add the local player (or every
    /// player) as a dynamic sprite at its ground height. Java
    /// `Client.addPlayers` (7576-7584) / TS 4265-4275: arriving on the dest
    /// tile clears `minimapFlagX` (the draw gate) and ticks
    /// `ANTICHEAT_CYCLELOGIC6`. The tile-occupancy stamp is kept so a
    /// second entity on a tile defers to the first this cycle.
    pub(crate) fn add_players(&mut self, client: &mut Client, add_self: bool) {
        if client.local_player.is_none() {
            return;
        }

        // Arrival-clear is independent of `isReady` and of `add_self`:
        // both `addPlayers(true)` and `addPlayers(false)` run it.
        if let Some(player) = client.local_player.as_ref() {
            if player.x >> 7 == client.minimap_flag_x && player.z >> 7 == client.minimap_flag_z {
                client.minimap_flag_x = 0;
                client.cyclelogic6 += 1;
                if client.cyclelogic6 > 122 {
                    client.cyclelogic6 = 0;
                    client.out.p1_enc(ClientProt::ANTICHEAT_CYCLELOGIC6.id);
                    client.out.p1(62);
                }
            }
        }

        let count = if add_self { 1 } else { client.player_count };
        for i in 0..count as usize {
            let (player, id) = if add_self {
                let Some(player) = client.local_player.as_mut() else {
                    continue;
                };
                (player, crate::client::client::LOCAL_PLAYER_INDEX << 14)
            } else {
                let player_id = client.player_ids[i];
                let Some(player) = client.players.get_mut(player_id as usize).and_then(|p| p.as_deref_mut())
                else {
                    continue;
                };
                (player, player_id << 14)
            };

            if !player.is_ready() {
                continue;
            }
            player.low_memory = false;
            if ((client.config.lowmem && client.player_count > 50) || client.player_count > 200)
                && !add_self
                && player.secondary_anim == player.readyanim
            {
                player.low_memory = true;
            }

            let stx = player.x >> 7;
            let stz = player.z >> 7;
            if stx < 0 || stx >= BuildArea::SIZE || stz < 0 || stz >= BuildArea::SIZE {
                continue;
            }

            let y = get_av_h(&client.groundh, &client.mapl, player.x, player.z, client.minusedlevel);
            // Java stamps `entity.height = model.minY` on the live entity
            // during the render pass (ClientPlayer.getTempModel); the scene
            // sprite holds a clone, so stamp the live player here or
            // `entity_overlays` projects from a height of 0 (Java 8870).
            player.get_temp_model(&client.cache, client.loop_cycle);
            let model = Some(SceneModel::Player(player.clone()));

            if player.loc_model.is_none()
                || client.loop_cycle < player.loc_start_cycle
                || client.loop_cycle >= player.loc_stop_cycle
            {
                if (player.x & 0x7f) == 64 && (player.z & 0x7f) == 64 {
                    let tile = (stx * BuildArea::SIZE + stz) as usize;
                    if self.tile_last_occupied_cycle[tile] == self.scene_cycle {
                        continue;
                    }
                    self.tile_last_occupied_cycle[tile] = self.scene_cycle;
                }

                player.y = y;
                if let Some(index) = client.world.add_dynamic(
                    client.minusedlevel,
                    player.x,
                    player.y,
                    player.z,
                    id,
                    player.yaw,
                    60,
                    player.needs_forward_draw_padding,
                ) {
                    self.world.set_sprite_model(&client.world, index, model);
                }
            } else {
                player.low_memory = false;
                player.y = y;
                if let Some(index) = client.world.add_dynamic2(
                    client.minusedlevel,
                    player.x,
                    player.y,
                    player.z,
                    player.min_tile_x,
                    player.min_tile_z,
                    player.max_tile_x,
                    player.max_tile_z,
                    id,
                    player.yaw,
                ) {
                    self.world.set_sprite_model(&client.world, index, model);
                }
            }
        }
    }

    /// `addNpcs` from client-ts (4328): add every NPC as a dynamic sprite,
    /// split by the `alwaysontop` flag.
    pub(crate) fn add_npcs(&mut self, client: &mut Client, alwaysontop: bool) {
        for i in 0..client.npc_count as usize {
            let npc_id = client.npc_ids[i];
            let typecode = (npc_id << 14) + 0x2000_0000;
            let Some(npc) = client.npc.get_mut(npc_id as usize).and_then(|n| n.as_deref_mut()) else {
                continue;
            };
            let Some(npc_type_id) = npc.r#type else {
                continue;
            };
            if client.cache.npc(npc_type_id).alwaysontop != alwaysontop {
                continue;
            }

            let stx = npc.x >> 7;
            let stz = npc.z >> 7;
            if stx < 0 || stx >= BuildArea::SIZE || stz < 0 || stz >= BuildArea::SIZE {
                continue;
            }

            if npc.size == 1 && (npc.x & 0x7f) == 64 && (npc.z & 0x7f) == 64 {
                let tile = (stx * BuildArea::SIZE + stz) as usize;
                if self.tile_last_occupied_cycle[tile] == self.scene_cycle {
                    continue;
                }
                self.tile_last_occupied_cycle[tile] = self.scene_cycle;
            }

            let y = get_av_h(&client.groundh, &client.mapl, npc.x, npc.z, client.minusedlevel);
            // Same clone-vs-live split as add_players: stamp the live NPC's
            // height so `entity_overlays` sees the Java value.
            npc.get_temp_model(&client.cache, client.loop_cycle);
            let model = Some(SceneModel::Npc(npc.clone()));
            if let Some(index) = client.world.add_dynamic(
                client.minusedlevel,
                npc.x,
                y,
                npc.z,
                typecode,
                npc.yaw,
                (npc.size - 1) * 64 + 60,
                npc.needs_forward_draw_padding,
            ) {
                self.world.set_sprite_model(&client.world, index, model);
            }
        }
    }

    /// `addProjectiles` from client-ts (4356): unlink projectiles whose
    /// level no longer matches or whose flight window passed, retarget the
    /// rest onto their npc/player target, advance them by
    /// `world_update_num`, and re-add them as dynamic sprites. The
    /// `cyclelogic1` anticheat payload (TS 4387-4413) writes a length-
    /// prefixed random blob.
    pub(crate) fn add_projectiles(&mut self, client: &mut Client) {
        let mut node = client.projectiles.head();
        while let Some(proj) = node {
            if proj.level != client.minusedlevel || client.loop_cycle > proj.t2 {
                client.projectiles.unlink_last();
            } else if client.loop_cycle >= proj.t1 {
                if proj.target > 0 {
                    let index = (proj.target - 1) as usize;
                    if let Some(npc) = client.npc.get(index).and_then(|n| n.as_deref()) {
                        let h2 = proj.h2;
                        let level = proj.level;
                        let y = get_av_h(&client.groundh, &client.mapl, npc.x, npc.z, level) - h2;
                        proj.set_target(npc.x as f64, y as f64, npc.z as f64, client.loop_cycle);
                    }
                }

                if proj.target < 0 {
                    let index = -proj.target - 1;
                    let player = if index == client.self_slot {
                        client.local_player.as_ref()
                    } else {
                        client.players.get(index as usize).and_then(|p| p.as_deref())
                    };
                    if let Some(player) = player {
                        let h2 = proj.h2;
                        let level = proj.level;
                        let y = get_av_h(&client.groundh, &client.mapl, player.x, player.z, level) - h2;
                        proj.set_target(
                            player.x as f64,
                            y as f64,
                            player.z as f64,
                            client.loop_cycle,
                        );
                    }
                }

                proj.move_by(client.world_update_num);
                // TS 4382-4383: `proj.x | 0` (Rust `as i32`), typecode -1,
                // padding 60, no forward padding.
                let (x, y, z, yaw) = (proj.x as i32, proj.y as i32, proj.z as i32, proj.yaw);
                let model = Some(SceneModel::Proj(proj.clone()));
                if let Some(index) = client.world.add_dynamic(
                    client.minusedlevel,
                    x,
                    y,
                    z,
                    -1,
                    yaw,
                    60,
                    false,
                ) {
                    self.world.set_sprite_model(&client.world, index, model);
                }
            }
            node = client.projectiles.next();
        }

        // TS 4387-4413: `cyclelogic1` anticheat every 1175 cycles.
        self.cyclelogic1 += 1;
        if self.cyclelogic1 > 1174 {
            self.cyclelogic1 = 0;

            client.out.p1_enc(ClientProt::ANTICHEAT_CYCLELOGIC1.id);
            client.out.p1(0);
            let start = client.out.pos;
            if (random_float() * 2.0) as i32 == 0 {
                client.out.p2(11499);
            }
            client.out.p2(10548);
            if (random_float() * 2.0) as i32 == 0 {
                client.out.p1(139);
            }
            if (random_float() * 2.0) as i32 == 0 {
                client.out.p1(94);
            }
            client.out.p2(51693);
            client.out.p1(16);
            client.out.p2(15036);
            if (random_float() * 2.0) as i32 == 0 {
                client.out.p1(65);
            }
            client.out.p1((random_float() * 256.0) as i32);
            client.out.p2(22990);
            client.out.psize1((client.out.pos - start) as i32);
        }
    }

    /// `addMapAnim` from client-ts (4416): unlink spots on the wrong level
    /// or already complete; otherwise advance (`update` with
    /// `world_update_num`), unlink when that completes the anim, else place
    /// the spot as a dynamic sprite (typecode -1, yaw 0, padding 60).
    pub(crate) fn add_map_anim(&mut self, client: &mut Client) {
        let mut node = client.spotanims.head();
        while let Some(spot) = node {
            if spot.level != client.minusedlevel || spot.anim_complete {
                client.spotanims.unlink_last();
            } else if client.loop_cycle >= spot.start_cycle {
                spot.update(&client.cache, client.world_update_num);
                if spot.anim_complete {
                    client.spotanims.unlink_last();
                } else {
                    let (level, x, y, z) = (spot.level, spot.x, spot.y, spot.z);
                    let model = Some(SceneModel::SpotAnim(spot.clone()));
                    if let Some(index) = client.world.add_dynamic(
                        level,
                        x,
                        y,
                        z,
                        -1,
                        0,
                        60,
                        false,
                    ) {
                        self.world.set_sprite_model(&client.world, index, model);
                    }
                }
            }
            node = client.spotanims.next();
        }
    }

    /// `camFollow` from client-ts (4432): position the eye at `distance`
    /// along the inverse pitch/yaw from the target.
    pub(crate) fn cam_follow(
        &mut self, client: &mut Client,
        pitch: i32,
        yaw: i32,
        target_x: i32,
        target_y: i32,
        target_z: i32,
        distance: i32,
    ) {
        let inv_pitch = (2048 - pitch) & 0x7ff;
        let inv_yaw = (2048 - yaw) & 0x7ff;

        let mut x = 0i32;
        let mut y = 0i32;
        let mut z = distance;

        if inv_pitch != 0 {
            let sin = Pix3D::sin_table()[inv_pitch as usize];
            let cos = Pix3D::cos_table()[inv_pitch as usize];
            let tmp = (y * cos - distance * sin) >> 16;
            z = (y * sin + distance * cos) >> 16;
            y = tmp;
        }

        if inv_yaw != 0 {
            let sin = Pix3D::sin_table()[inv_yaw as usize];
            let cos = Pix3D::cos_table()[inv_yaw as usize];
            let tmp = (z * sin + x * cos) >> 16;
            z = (z * cos - x * sin) >> 16;
            x = tmp;
        }

        client.cam_x = target_x - x;
        client.cam_y = target_y - y;
        client.cam_z = target_z - z;
        client.cam_pitch = pitch;
        client.cam_yaw = yaw;
    }

    /// `camShake` jitter from client-ts (4211-4235): for each active axis,
    /// add `random * (ran*2 + 1) - ran + sin(cycle * amp/100) * shakeRan`
    /// to the rendered eye — x/y/z positions, the 11-bit yaw, or the pitch
    /// clamped to 128..383. TS mutates `cam*` in place for `renderAll` and
    /// restores the pre-jitter snapshot afterwards; the caller passes that
    /// snapshot in and receives the jittered eye to render with.
    pub fn cam_shake_jitter(
        &mut self, client: &mut Client,
        cam_x: i32,
        cam_y: i32,
        cam_z: i32,
        cam_pitch: i32,
        cam_yaw: i32,
    ) -> (i32, i32, i32, i32, i32) {
        let mut cam_x = cam_x;
        let mut cam_y = cam_y;
        let mut cam_z = cam_z;
        let mut cam_pitch = cam_pitch;
        let mut cam_yaw = cam_yaw;

        for axis in 0..5 {
            if !client.cam_shake[axis] {
                continue;
            }

            let jitter = (self.rand.next_double()
                * (client.cam_shake_axis[axis] * 2 + 1) as f64
                - client.cam_shake_axis[axis] as f64
                + (client.cam_shake_cycle[axis] as f64 * (client.cam_shake_amp[axis] as f64 / 100.0))
                    .sin()
                    * client.cam_shake_ran[axis] as f64) as i32;

            match axis {
                0 => cam_x += jitter,
                1 => cam_y += jitter,
                2 => cam_z += jitter,
                3 => cam_yaw = (cam_yaw + jitter) & 0x7ff,
                _ => {
                    cam_pitch += jitter;
                    cam_pitch = cam_pitch.clamp(128, 383);
                }
            }
        }

        (cam_x, cam_y, cam_z, cam_pitch, cam_yaw)
    }

    /// `followCamera` from client-ts (3222), run from `game_loop` (2346)
    /// while `scene_state == 2`: the orbit camera chases the local player
    /// (snapped when more than 500 away, then a `/16` ease), the arrow keys
    /// (`key_held[1..4]`) steer yaw/pitch through the TS velocity fields,
    /// and `camera_pitch_clamp` eases toward the surrounding-terrain clamp.
    /// The `mapl` `VisBelow` level lift is not ported, so the height sample
    /// reads `minusedlevel` like the rest of the port; `macro_camera_x/z`
    /// stay 0, their TS initial values (the macro random-drift block is a
    /// separate gameLoop chunk not ported).
    pub fn follow_camera(&mut self, client: &mut Client) {
        let Some(player) = &client.local_player else {
            return;
        };
        let orbit_x = player.x + client.macro_camera_x;
        let orbit_z = player.z + client.macro_camera_z;

        if client.orbit_camera_x - orbit_x < -500
            || client.orbit_camera_x - orbit_x > 500
            || client.orbit_camera_z - orbit_z < -500
            || client.orbit_camera_z - orbit_z > 500
        {
            client.orbit_camera_x = orbit_x;
            client.orbit_camera_z = orbit_z;
        }

        if client.orbit_camera_x != orbit_x {
            client.orbit_camera_x += (orbit_x - client.orbit_camera_x) / 16;
        }
        if client.orbit_camera_z != orbit_z {
            client.orbit_camera_z += (orbit_z - client.orbit_camera_z) / 16;
        }

        if client.shell.key_held[1] == 1 {
            client.orbit_camera_yaw_velocity += (-client.orbit_camera_yaw_velocity - 24) / 2;
        } else if client.shell.key_held[2] == 1 {
            client.orbit_camera_yaw_velocity += (24 - client.orbit_camera_yaw_velocity) / 2;
        } else {
            client.orbit_camera_yaw_velocity /= 2;
        }

        if client.shell.key_held[3] == 1 {
            client.orbit_camera_pitch_velocity += (12 - client.orbit_camera_pitch_velocity) / 2;
        } else if client.shell.key_held[4] == 1 {
            client.orbit_camera_pitch_velocity += (-client.orbit_camera_pitch_velocity - 12) / 2;
        } else {
            client.orbit_camera_pitch_velocity /= 2;
        }

        client.orbit_camera_yaw = (client.orbit_camera_yaw + client.orbit_camera_yaw_velocity / 2) & 0x7ff;
        client.orbit_camera_pitch =
            (client.orbit_camera_pitch + client.orbit_camera_pitch_velocity / 2).clamp(128, 383);

        let orbit_tile_x = client.orbit_camera_x >> 7;
        let orbit_tile_z = client.orbit_camera_z >> 7;
        let orbit_y = get_av_h(&client.groundh, &client.mapl, client.orbit_camera_x, client.orbit_camera_z, client.minusedlevel);
        let mut max_y = 0;
        if orbit_tile_x > 3 && orbit_tile_z > 3 && orbit_tile_x < 100 && orbit_tile_z < 100 {
            for x in (orbit_tile_x - 4)..=(orbit_tile_x + 4) {
                for z in (orbit_tile_z - 4)..=(orbit_tile_z + 4) {
                    let y = orbit_y
                        - client.groundh[client.minusedlevel as usize][x as usize][z as usize];
                    if y > max_y {
                        max_y = y;
                    }
                }
            }
        }

        let clamp = (max_y * 192).clamp(32768, 98048);

        if clamp > client.camera_pitch_clamp {
            client.camera_pitch_clamp += (clamp - client.camera_pitch_clamp) / 24;
        } else if clamp < client.camera_pitch_clamp {
            client.camera_pitch_clamp += (clamp - client.camera_pitch_clamp) / 80;
        }
    }

    /// `roofCheck` from Java (9248-9327) / client-ts (4476): the highest
    /// level drawn this frame. A `RemoveRoof` (0x4) flag on the camera
    /// tile, on any tile along the Bresenham-style ray from the camera to
    /// the local player, or on the player's own tile drops the roof to
    /// `minusedlevel`.
    pub fn roof_check(&self, client: &Client) -> i32 {
        let mut top = 3;
        let Some(player) = &client.local_player else {
            return top;
        };

        if client.cam_pitch < 310 {
            let mut cam_tile_x = client.cam_x >> 7;
            let mut cam_tile_z = client.cam_z >> 7;
            let player_tile_x = player.x >> 7;
            let player_tile_z = player.z >> 7;

            if self.mapl_remove_roof(client, client.minusedlevel, cam_tile_x, cam_tile_z) {
                top = client.minusedlevel;
            }

            let tile_delta_x = if player_tile_x > cam_tile_x {
                player_tile_x - cam_tile_x
            } else {
                cam_tile_x - player_tile_x
            };
            let tile_delta_z = if player_tile_z > cam_tile_z {
                player_tile_z - cam_tile_z
            } else {
                cam_tile_z - player_tile_z
            };

            // Java 9271-9320: step the dominant axis, cross-stepping the
            // other when the 16.16 accumulator wraps. Camera tile == player
            // tile (both deltas 0) skips the ray — Java's `while` would not
            // run and its division would be `/ 0`.
            if tile_delta_x > tile_delta_z {
                if tile_delta_x != 0 {
                    let delta = tile_delta_z * 65536 / tile_delta_x;
                    let mut accumulator = 32768;
                    while cam_tile_x != player_tile_x {
                        if cam_tile_x < player_tile_x {
                            cam_tile_x += 1;
                        } else {
                            cam_tile_x -= 1;
                        }
                        if self.mapl_remove_roof(client, client.minusedlevel, cam_tile_x, cam_tile_z) {
                            top = client.minusedlevel;
                        }
                        accumulator += delta;
                        if accumulator >= 65536 {
                            accumulator -= 65536;
                            if cam_tile_z < player_tile_z {
                                cam_tile_z += 1;
                            } else if cam_tile_z > player_tile_z {
                                cam_tile_z -= 1;
                            }
                            if self.mapl_remove_roof(client, client.minusedlevel, cam_tile_x, cam_tile_z) {
                                top = client.minusedlevel;
                            }
                        }
                    }
                }
            } else if tile_delta_z != 0 {
                let delta = tile_delta_x * 65536 / tile_delta_z;
                let mut accumulator = 32768;
                while cam_tile_z != player_tile_z {
                    if cam_tile_z < player_tile_z {
                        cam_tile_z += 1;
                    } else if cam_tile_z > player_tile_z {
                        cam_tile_z -= 1;
                    }
                    if self.mapl_remove_roof(client, client.minusedlevel, cam_tile_x, cam_tile_z) {
                        top = client.minusedlevel;
                    }
                    accumulator += delta;
                    if accumulator >= 65536 {
                        accumulator -= 65536;
                        if cam_tile_x < player_tile_x {
                            cam_tile_x += 1;
                        } else if cam_tile_x > player_tile_x {
                            cam_tile_x -= 1;
                        }
                        if self.mapl_remove_roof(client, client.minusedlevel, cam_tile_x, cam_tile_z) {
                            top = client.minusedlevel;
                        }
                    }
                }
            }
        }

        if self.mapl_remove_roof(client, client.minusedlevel, player.x >> 7, player.z >> 7) {
            top = client.minusedlevel;
        }
        top
    }

    /// `roofCheck2` from Java (9329-9331) / client-ts (4467): the
    /// cutscene-camera roof level. `minusedlevel` only while the eye is
    /// under the roof (within 800 of the ground) on a `RemoveRoof` tile; a
    /// high camera or a clean tile draws every level.
    pub fn roof_check2(&self, client: &Client) -> i32 {
        let y = get_av_h(&client.groundh, &client.mapl, client.cam_x, client.cam_z, client.minusedlevel);
        if y - client.cam_y >= 800
            || !self.mapl_remove_roof(client, client.minusedlevel, client.cam_x >> 7, client.cam_z >> 7)
        {
            3
        } else {
            client.minusedlevel
        }
    }

    /// `mapl[level][x][z] & MapFlag::REMOVE_ROOF` with the Java array
    /// bounds (`BuildArea::SIZE` per side); out-of-range reads are "no
    /// flag".
    fn mapl_remove_roof(&self, client: &Client, level: i32, x: i32, z: i32) -> bool {
        if level < 0
            || level >= BuildArea::LEVELS
            || x < 0
            || x >= BuildArea::SIZE
            || z < 0
            || z >= BuildArea::SIZE
        {
            return false;
        }
        (client.mapl[level as usize][x as usize][z as usize] & MapFlag::REMOVE_ROOF as u8) != 0
    }

    /// `getOverlayPosEntity` from Java (2026-2027): project an entity at a
    /// height (the scene coord `entity.x`/`entity.z` in 128ths of a tile).
    pub fn get_overlay_pos_entity(&mut self, client: &mut Client, entity: &ClientEntity, height: i32) {
        self.get_overlay_pos(client, entity.x, entity.z, height);
    }

    /// `getOverlayPos` from Java (2031-2056): project a scene point onto the
    /// screen origin. The 11-bit pitch/yaw rotate keeps Java's i32 wrap on
    /// the `>> 16` products; `project_x`/`project_y` are -1 when the point
    /// is off the playable scene or behind the camera (`z' < 50`).
    pub fn get_overlay_pos(&mut self, client: &mut Client, x: i32, z: i32, height: i32) {
        let (px, py) = self.project_overlay(client, x, z, height);
        self.project_x = px;
        self.project_y = py;
    }

    /// The `getOverlayPos` math (Java 2031-2056) as a pure read, so
    /// `entity_overlays` can project while holding entity borrows. The
    /// nav-debug paint reuses it for the tile/hull projections.
    pub(crate) fn project_overlay(&self, client: &Client, x: i32, z: i32, height: i32) -> (i32, i32) {
        if x < 128 || z < 128 || x > 13056 || z > 13056 {
            return (-1, -1);
        }
        let y = get_av_h(&client.groundh, &client.mapl, x, z, client.minusedlevel) - height;
        let dx = x - client.cam_x;
        let dy = y - client.cam_y;
        let dz = z - client.cam_z;
        let sin_pitch = Pix3D::sin_table()[(client.cam_pitch & 0x7ff) as usize];
        let cos_pitch = Pix3D::cos_table()[(client.cam_pitch & 0x7ff) as usize];
        let sin_yaw = Pix3D::sin_table()[(client.cam_yaw & 0x7ff) as usize];
        let cos_yaw = Pix3D::cos_table()[(client.cam_yaw & 0x7ff) as usize];
        // Java 2039-2044: the wrapped products only feed the `>> 16`
        // (Java int arithmetic wraps; Rust debug builds would panic).
        let var13 = dz
            .wrapping_mul(sin_yaw)
            .wrapping_add(dx.wrapping_mul(cos_yaw))
            >> 16;
        let var14 = dz
            .wrapping_mul(cos_yaw)
            .wrapping_sub(dx.wrapping_mul(sin_yaw))
            >> 16;
        let var16 = dy
            .wrapping_mul(cos_pitch)
            .wrapping_sub(var14.wrapping_mul(sin_pitch))
            >> 16;
        let var17 = dy
            .wrapping_mul(sin_pitch)
            .wrapping_add(var14.wrapping_mul(cos_pitch))
            >> 16;
        if var17 >= 50 {
            (
                self.pix3d.origin_x + (var13 << 9) / var17,
                self.pix3d.origin_y + (var16 << 9) / var17,
            )
        } else {
            (-1, -1)
        }
    }

    /// `entityOverlays` from Java (8870-end): the overhead prayer headicons,
    /// the hint crowns, the chat bubbles, the health bars and the hitmarks
    /// over every ready entity, drawn into `area_game` (bound here — `take`
    /// /put back as `other_overlays` does, since `game_draw_main` drops its
    /// bind before this runs). Chat bubbles collect into the `chat_*` stacks
    /// and are pushed up past each other before drawing. `coord_arrow` is a
    /// separate stub.
    pub fn entity_overlays(&mut self, client: &mut Client) {
        self.chat_count = 0;
        let mut game = self.area_game.take();
        if let Some(game) = game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            // `Pix3D.setClipping(512, 334)` (TS 4238-4245): the projection
            // origin reads the 3D target even when only this pass draws.
            self.pix3d.set_clipping(surface.width, surface.height);
            let loop_cycle = client.loop_cycle;
            let scene_cycle = self.scene_cycle;
            let player_count = client.player_count;
            let npc_count = client.npc_count;
            let chat_effects = client.chat_effects;
            let chat_public_mode = client.chat_public_mode;
            let hint_type = client.hint_type;
            let hint_npc = client.hint_npc;
            let hint_player = client.hint_player;

            for index in -1..(player_count + npc_count) {
                // Java 8873-8881: -1 is the local player, then the players
                // and npcs by their id lists.
                let (entity, ready, player_chat_name, player_headicons, npc_type) = if index == -1 {
                    match &client.local_player {
                        Some(p) => (&p.entity, p.is_ready(), p.name.as_deref(), p.headicons, None),
                        None => continue,
                    }
                } else if index < player_count {
                    match client
                        .players
                        .get(client.player_ids[index as usize] as usize)
                        .and_then(|o| o.as_ref())
                    {
                        Some(p) => (&p.entity, p.is_ready(), p.name.as_deref(), p.headicons, None),
                        None => continue,
                    }
                } else {
                    match client
                        .npc
                        .get(client.npc_ids[(index - player_count) as usize] as usize)
                        .and_then(|o| o.as_ref())
                    {
                        Some(n) => (&n.entity, n.is_ready(), None, 0, n.r#type),
                        None => continue,
                    }
                };
                if !ready {
                    continue;
                }

                if index >= player_count {
                    // NPC headicon + the type-1 hint crown (Java 8883-8896).
                    let npc_id = client.npc_ids[(index - player_count) as usize];
                    if let Some(npc_type) = npc_type {
                        if npc_type < client.cache.npcs.len() {
                            let headicon = client.cache.npc(npc_type).headicon;
                            if (0..20).contains(&headicon) {
                                let (px, py) = self.project_overlay(client, entity.x, entity.z, entity.height + 15);
                                self.project_x = px;
                                self.project_y = py;
                                if self.project_x > -1 {
                                    if let Some(sprite) = self.media.headicons
                                        .get(headicon as usize)
                                        .and_then(|o| o.as_ref())
                                    {
                                        sprite.plot_sprite(&mut surface, px - 12, py - 30);
                                    }
                                }
                            }
                        }
                    }
                    if hint_type == 1 && hint_npc == npc_id && loop_cycle % 20 < 10 {
                        let (px, py) = self.project_overlay(client, entity.x, entity.z, entity.height + 15);
                        self.project_x = px;
                        self.project_y = py;
                        if self.project_x > -1 {
                            if let Some(sprite) = self.media.headicons.get(2).and_then(|o| o.as_ref()) {
                                sprite.plot_sprite(&mut surface, px - 12, py - 28);
                            }
                        }
                    }
                } else {
                    // Player headicons stack bottom-up from 30 then 25 px
                    // (Java 8897-8915); the type-10 hint crown plots at the
                    // remaining y.
                    let mut y = 30;
                    if player_headicons != 0 {
                        let (px, py) = self.project_overlay(client, entity.x, entity.z, entity.height + 15);
                        self.project_x = px;
                        self.project_y = py;
                        if self.project_x > -1 {
                            for icon in 0..8 {
                                if (player_headicons & (1 << icon)) != 0 {
                                    if let Some(sprite) = self.media.headicons
                                        .get(icon as usize)
                                        .and_then(|o| o.as_ref())
                                    {
                                        sprite.plot_sprite(&mut surface, px - 12, py - y);
                                    }
                                    y -= 25;
                                }
                            }
                        }
                    }
                    if index >= 0 && hint_type == 10 && hint_player == client.player_ids[index as usize] {
                        let (px, py) = self.project_overlay(client, entity.x, entity.z, entity.height + 15);
                        self.project_x = px;
                        self.project_y = py;
                        if self.project_x > -1 {
                            if let Some(sprite) = self.media.headicons.get(7).and_then(|o| o.as_ref()) {
                                sprite.plot_sprite(&mut surface, px - 12, py - y);
                            }
                        }
                    }
                }

                // Chat bubble collect (Java 8917-8936). The bubble shows for
                // npcs always; for players in public modes 0/3 or mode 1
                // when the sender is a friend.
                if entity.chat_message.is_some()
                    && (index >= player_count
                        || chat_public_mode == 0
                        || chat_public_mode == 3
                        || (chat_public_mode == 1
                            && client.is_friend(player_chat_name.unwrap_or(""))))
                {
                    let (px, py) = self.project_overlay(client, entity.x, entity.z, entity.height);
                    self.project_x = px;
                    self.project_y = py;
                    // Java NPEs without the b12 font; the Option guard skips
                    // the collect instead.
                    if self.project_x > -1 && self.chat_count < 50 {
                        if let Some(b12) = &self.media.b12 {
                            let message = entity.chat_message.as_deref().unwrap_or("");
                            let idx = self.chat_count as usize;
                            self.chat_width[idx] = b12.string_wid(Some(message)) / 2;
                            self.chat_height[idx] = b12.height;
                            self.chat_x[idx] = px;
                            self.chat_y[idx] = py;
                            self.chat_colour[idx] = entity.chat_colour;
                            self.chat_effect[idx] = entity.chat_effect;
                            self.chat_timer[idx] = entity.chat_timer;
                            self.chats[idx] = message.to_string();
                            self.chat_count += 1;
                            // Java 8928-8933: the effect 1/2 sizing writes
                            // index the slot AFTER the `++` (one past the
                            // bubble just stored — kept verbatim; a full
                            // stack would overflow the 50-slot arrays).
                            if chat_effects == 0 && entity.chat_effect == 1 && self.chat_count < 50 {
                                self.chat_height[self.chat_count as usize] += 10;
                                self.chat_y[self.chat_count as usize] += 5;
                            }
                            if chat_effects == 0 && entity.chat_effect == 2 && self.chat_count < 50 {
                                self.chat_width[self.chat_count as usize] = 60;
                            }
                        }
                    }
                }

                // Health bar (Java 8938-8945): `combatCycle > loopCycle`
                // (not the TS `+ 100`), green fill then the red remainder.
                if entity.combat_cycle > loop_cycle {
                    let (px, py) = self.project_overlay(client, entity.x, entity.z, entity.height + 15);
                    self.project_x = px;
                    self.project_y = py;
                    if self.project_x > -1 {
                        let mut w = entity.health * 30 / entity.total_health;
                        if w > 30 {
                            w = 30;
                        }
                        surface.fill_rect(px - 15, py - 3, w, 5, Colour::GREEN);
                        surface.fill_rect(px - 15 + w, py - 3, 30 - w, 5, Colour::RED);
                    }
                }

                // Hitmarks (Java 8948-8966): up to 4, offset from the head
                // projection, the damage number in p11 black then white.
                for i in 0..4 {
                    if entity.damage_cycles[i] > loop_cycle {
                        let (mut px, mut py) = self.project_overlay(client, entity.x, entity.z, entity.height / 2);
                        self.project_x = px;
                        self.project_y = py;
                        if self.project_x > -1 {
                            if i == 1 {
                                py -= 20;
                            }
                            if i == 2 {
                                px -= 15;
                                py -= 10;
                            }
                            if i == 3 {
                                px += 15;
                                py -= 10;
                            }
                            if let Some(sprite) = self.media.hitmarks
                                .get(entity.damage_types[i] as usize)
                                .and_then(|o| o.as_ref())
                            {
                                sprite.plot_sprite(&mut surface, px - 12, py - 12);
                            }
                            if let Some(p11) = &self.media.p11 {
                                let text = format!("{}", entity.damage_values[i]);
                                p11.centre_string(&mut surface, Some(&text), px, py + 4, Colour::BLACK);
                                p11.centre_string(&mut surface, Some(&text), px - 1, py + 3, Colour::WHITE);
                            }
                        }
                    }
                }
            }

            // The collected bubbles: push overlapping bubbles up, then draw
            // (Java 8971-end).
            for i in 0..self.chat_count as usize {
                let x = self.chat_x[i];
                let mut y = self.chat_y[i];
                let pad = self.chat_width[i];
                let hgt = self.chat_height[i];
                let mut sorting = true;
                while sorting {
                    sorting = false;
                    for j in 0..i {
                        if y + 2 > self.chat_y[j] - self.chat_height[j]
                            && y - hgt < self.chat_y[j] + 2
                            && x - pad < self.chat_x[j] + self.chat_width[j]
                            && x + pad > self.chat_x[j] - self.chat_width[j]
                            && self.chat_y[j] - self.chat_height[j] < y
                        {
                            y = self.chat_y[j] - self.chat_height[j];
                            sorting = true;
                        }
                    }
                }
                self.project_x = self.chat_x[i];
                self.chat_y[i] = y;
                self.project_y = y;
                let message = self.chats[i].clone();
                if chat_effects != 0 {
                    // TS 4721-4723: global wave effects force the yellow
                    // fallback.
                    if let Some(b12) = &self.media.b12 {
                        b12.centre_string(
                            &mut surface,
                            Some(&message),
                            self.project_x,
                            self.project_y + 1,
                            Colour::BLACK,
                        );
                        b12.centre_string(
                            &mut surface,
                            Some(&message),
                            self.project_x,
                            self.project_y,
                            Colour::YELLOW,
                        );
                    }
                } else {
                    let mut colour = Colour::YELLOW;
                    if (0..6).contains(&self.chat_colour[i]) {
                        colour = CHAT_COLOURS[self.chat_colour[i] as usize];
                    }
                    if self.chat_colour[i] == 6 {
                        colour = if scene_cycle % 20 < 10 { Colour::RED } else { Colour::YELLOW };
                    }
                    if self.chat_colour[i] == 7 {
                        colour = if scene_cycle % 20 < 10 { Colour::BLUE } else { Colour::CYAN };
                    }
                    if self.chat_colour[i] == 8 {
                        colour = if scene_cycle % 20 < 10 { 0xb000 } else { 0x80ff80 };
                    }
                    if self.chat_colour[i] == 9 {
                        let delta = 150 - self.chat_timer[i];
                        if delta < 50 {
                            colour = delta * 1280 + Colour::RED;
                        } else if delta < 100 {
                            colour = Colour::YELLOW - (delta - 50) * 327680;
                        } else if delta < 150 {
                            colour = (delta - 100) * 5 + Colour::GREEN;
                        }
                    }
                    if self.chat_colour[i] == 10 {
                        let delta = 150 - self.chat_timer[i];
                        if delta < 50 {
                            colour = delta * 5 + Colour::RED;
                        } else if delta < 100 {
                            colour = Colour::MAGENTA - (delta - 50) * 327680;
                        } else if delta < 150 {
                            colour = (delta - 100) * 327680 + Colour::BLUE - (delta - 100) * 5;
                        }
                    }
                    if self.chat_colour[i] == 11 {
                        let delta = 150 - self.chat_timer[i];
                        if delta < 50 {
                            colour = Colour::WHITE - delta * 327685;
                        } else if delta < 100 {
                            colour = (delta - 50) * 327685 + Colour::GREEN;
                        } else if delta < 150 {
                            colour = Colour::WHITE - (delta - 100) * 327680;
                        }
                    }
                    if let Some(b12) = &self.media.b12 {
                        match self.chat_effect[i] {
                            1 => {
                                b12.centre_string_wave(
                                    &mut surface,
                                    Some(&message),
                                    self.project_x,
                                    self.project_y + 1,
                                    Colour::BLACK,
                                    scene_cycle,
                                );
                                b12.centre_string_wave(
                                    &mut surface,
                                    Some(&message),
                                    self.project_x,
                                    self.project_y,
                                    colour,
                                    scene_cycle,
                                );
                            }
                            2 => {
                                let w = b12.string_wid(Some(&message));
                                let offset_x = (150 - self.chat_timer[i]) * (w + 100) / 150;
                                // Java 9042-9047 clips to `projectX ± 50`
                                // for the slide-in text.
                                surface.set_clipping(self.project_x - 50, 0, self.project_x + 50, 334);
                                b12.draw_string(
                                    &mut surface,
                                    Some(&message),
                                    self.project_x + 50 - offset_x,
                                    self.project_y + 1,
                                    Colour::BLACK,
                                );
                                b12.draw_string(
                                    &mut surface,
                                    Some(&message),
                                    self.project_x + 50 - offset_x,
                                    self.project_y,
                                    colour,
                                );
                                surface.reset_clipping();
                            }
                            _ => {
                                b12.centre_string(
                                    &mut surface,
                                    Some(&message),
                                    self.project_x,
                                    self.project_y + 1,
                                    Colour::BLACK,
                                );
                                b12.centre_string(
                                    &mut surface,
                                    Some(&message),
                                    self.project_x,
                                    self.project_y,
                                    colour,
                                );
                            }
                        }
                    }
                }
            }
        }
        self.area_game = game;
    }

    /// `coordArrow` from client-ts (4781): a no-op while `hintType`/
    /// `headicons` are not ported.
    pub(crate) fn coord_arrow(&mut self, _client: &mut Client) {}

    /// `textureRunAnims` from client-ts (4794): a no-op while the animated
    /// texture buffers are not ported.
    pub(crate) fn texture_run_anims(&mut self, _client: &mut Client, _cycle: i32) {}

    /// `prepareGame` from client-ts (2001): allocate the in-game `PixMap`
    /// areas, lazily on the first `game_draw` (TS calls it after a
    /// successful login; this crate has no game-loading flow yet). Sized
    /// as TS: `area_game` 512×334 and the constructor-sized areas as
    /// `prepareGame` (the `areaBack*` strips themselves are the shared
    /// `Media` copy). `area_map` gets the `mapback` ring plotted at (0, 0)
    /// as TS 2022-2023, and the minimap/compass scanline masks are built
    /// from the shared `mapback.data` as TS 1180-1216. A missing `media`
    /// pack leaves the sprites `None` — `game_draw` still draws the panels
    /// that are present. The title is unloaded and `image_title2` nulled
    /// as Java `prepareGame` (`Client.java` 6919); `title_screen_draw`'s
    /// logout teardown nulls it again so a later `prepare_title`
    /// reallocates the regions from the shared `title` jag.
    pub(crate) fn prepare_game(&mut self, client: &mut Client) {
        if self.area_chat.is_some() {
            return;
        }

        self.unload_title();
        self.image_title2 = None;
        self.load_fonts(client);

        self.area_game = Some(PixMap::new(512, 334));
        self.area_map = Some(PixMap::new(172, 156));
        self.area_side = Some(PixMap::new(190, 261));
        self.area_chat = Some(PixMap::new(479, 96));
        self.area_backbase1 = Some(PixMap::new(496, 50));
        self.area_backbase2 = Some(PixMap::new(269, 37));
        self.area_backhmid1 = Some(PixMap::new(249, 45));

        // TS prepareGame 2022-2023: `area_map` starts as the `mapback`
        // ring (a fresh `PixMap` is already zeroed, so the `cls()` is a
        // no-op here). `minimapDraw` rotates the map inside the ring. The
        // ring sprite is the shared `Media` copy (task 6).
        if let Some(map) = self.area_map.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
            if let Some(mapback) = &self.media.mapback {
                mapback.plot_sprite(&mut surface, 0, 0);
            }
        }

        // Run unconditionally so a missing `media` pack still leaves the
        // masks sized (zeroed) for `minimap_draw`'s rotate-plots.
        self.build_minimap_masks(client);

        // TS maininit 1152-1154 `unpackTextures` / `initColourTable` /
        // `initPool`: depack the 50 textures from the `textures` jag, then
        // initialise the texel pool and the gamma-corrected per-texture
        // palettes (the palette half of `initColourTable`; the global
        // colour table was built in `Client::new`). A missing `textures`
        // jag skips the depacks — textured ground then falls back to the
        // average-colour gouraud branch instead of drawing nothing.
        let textures_path = format!("{}/textures", client.config.cache_dir);
        if let Ok(bytes) = std::fs::read(&textures_path) {
            let jag = JagFile::new(bytes);
            self.pix3d.unpack_textures(&jag);
        }
        self.pix3d.init_pool(20);
        self.pix3d.init_texture_palettes(0.8);
        // Mirror the texture averages onto `Client` so the sim's
        // `map_build` → `finish_build` ground overlays read them without
        // holding the renderer.
        self.pix3d.refresh_texture_averages();
        client.tex_average = self.pix3d.tex_average;

        client.redraw_frame = true;
    }

    /// TS 1180-1216: the per-row scanline masks from `mapback.data` that
    /// `scanlineRotatePlotSprite` plots through — the compass rows 0..32
    /// over columns 0..33, and the minimap rows 5..155 over columns 25..171
    /// (the `x > 34 || y > 34` gate keeps the compass ring out of the
    /// minimap mask). The masks are offsets/lengths of the transparent runs,
    /// so the plot never paints over the `mapback` ring. A row without a
    /// transparent run keeps `left = 999` (`right - left` negative → the
    /// plot loop is empty). Without `mapback` the masks stay all-zero sized
    /// (33/151), so the rotate-plots are no-ops instead of indexing an empty
    /// slice — a missing `media` pack must not panic `minimap_draw`.
    fn build_minimap_masks(&mut self, _client: &mut Client) {
        self.compass_mask_line_offsets = vec![0; 33];
        self.compass_mask_line_lengths = vec![0; 33];
        self.minimap_mask_line_offsets = vec![0; 151];
        self.minimap_mask_line_lengths = vec![0; 151];
        let Some(mapback) = &self.media.mapback else {
            return;
        };
        for y in 0..33 {
            let mut left = 999;
            let mut right = 0;
            for x in 0..34 {
                if mapback.data[(x + y * mapback.wi) as usize] == 0 {
                    if left == 999 {
                        left = x;
                    }
                } else if left != 999 {
                    right = x;
                    break;
                }
            }
            self.compass_mask_line_offsets[y as usize] = left;
            self.compass_mask_line_lengths[y as usize] = right - left;
        }
        for y in 5..156 {
            let mut left = 999;
            let mut right = 0;
            for x in 25..172 {
                if mapback.data[(x + y * mapback.wi) as usize] == 0 && (x > 34 || y > 34) {
                    if left == 999 {
                        left = x;
                    }
                } else if left != 999 {
                    right = x;
                    break;
                }
            }
            self.minimap_mask_line_offsets[(y - 5) as usize] = left - 25;
            self.minimap_mask_line_lengths[(y - 5) as usize] = right - left;
        }
    }

    /// `drawSide` from client-ts (11098): plot `invback` into `area_side`,
    /// draw the open side interface (`side_modal_id`, else the active tab's
    /// `side_icon`) via `drawInterface` (TS 11106-11110), the side-area
    /// minimenu (TS 11113), and blit at (553, 205). The trailing
    /// `areaGame.setPixels()` (no global Pix2D target) is not ported.
    pub(crate) fn draw_side(&mut self, client: &mut Client) {
        let mut side = self.area_side.take();
        if let Some(side) = side.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut side.pixels, side.width, side.height);
            // `draw_interface`'s TYPE_MODEL arm rasters into this surface:
            // bind `pix3d` clipping to it once before drawing.
            self.pix3d.set_clipping(surface.width, surface.height);
            if let Some(invback) = &self.media.invback {
                invback.plot_sprite(&mut surface, 0, 0);
            }
            if client.side_modal_id != -1 {
                self.draw_interface(client, client.side_modal_id, 0, 0, 0, &mut surface);
            } else if client.side_icon.get(client.active_icon as usize).copied() != Some(-1) {
                self.draw_interface(client, client.side_icon[client.active_icon as usize], 0, 0, 0, &mut surface);
            }
            if client.is_menu_open && client.menu_area == 1 {
                self.draw_minimenu(client, &mut surface);
            }
        }
        if let Some(side) = &side {
            side.blit_into(&mut self.draw_area, 553, 205);
        }
        self.area_side = side;
    }

    /// `Client.invNumber` from the Java oracle (Client.java 1394-1401):
    /// stack counts under 100k as-is, then `K` per 1000, else `M` per
    /// 1000000 (integer division, so 1500 -> "1500"; `K` starts at 100000).
    pub fn inv_number(&self, _client: &Client, amount: i32) -> String {
        if amount < 100000 {
            amount.to_string()
        } else if amount < 10000000 {
            format!("{}K", amount / 1000)
        } else {
            format!("{}M", amount / 1000000)
        }
    }

    /// `Client.niceNumber` from client-ts (10278-10289): comma-group the
    /// decimal every 3 digits from the right, then abbreviate counts over 4
    /// characters as `K` and over 8 as `million`, keeping the grouped value
    /// in parentheses; always prefixed with a space.
    pub fn nice_number(&self, _client: &Client, amount: i32) -> String {
        let mut s = amount.to_string();
        let mut i = s.len() as i32 - 3;
        while i > 0 {
            s.insert(i as usize, ',');
            i -= 3;
        }
        if s.len() > 8 {
            let (prefix, _) = s.split_at(s.len() - 8);
            s = format!("@gre@{prefix} million @whi@({s})");
        } else if s.len() > 4 {
            let (prefix, _) = s.split_at(s.len() - 4);
            s = format!("@cya@{prefix}K @whi@({s})");
        }
        format!(" {s}")
    }

    /// `drawInterface` from client-ts (9900) for the 2D component types:
    /// recurse `TYPE_LAYER` children (drawing a scrollbar after a layer
    /// whose `scroll_height` exceeds its height), draw `TYPE_RECT`
    /// fill/outline, `TYPE_TEXT` with the font at `com.font` index 0-3
    /// (p11/p12/b12/q8) and the `%1`-`%5` `getIfVar` substitution,
    /// `TYPE_GRAPHIC` from its `media` sprite, `TYPE_INV` item icons +
    /// stack counts (Java Client.java 9746-9820), `TYPE_INV_TEXT`
    /// object-name grids (Java Client.java 9972-9994), and `TYPE_MODEL`
    /// (Java Client.java 9944-9970, via `get_temp_model` + `objRender`).
    /// The caller binds `pix3d` to the target surface once
    /// (`set_clipping`); the `clientComponent` scripts load with Task 14.
    pub fn draw_interface(&mut self, client: &mut Client, com_id: i32, x: i32, y: i32, scroll_y: i32, surface: &mut Pix2D) {
        let Some(com) = client.if_(com_id as usize) else {
            return;
        };
        // TS 9901-9905: only TYPE_LAYER draws; a hidden layer still draws
        // while its id is hovered (the `over*ComId` pointer state).
        if com.r#type != ComponentType::TYPE_LAYER
            || (com.hide
                && client.over_main_com_id != com.id
                && client.over_side_com_id != com.id
                && client.over_chat_com_id != com.id)
        {
            return;
        }
        let children = match &com.children {
            Some(c) => c.clone(),
            None => return,
        };
        let child_x = com.child_x.clone().unwrap_or_default();
        let child_y = com.child_y.clone().unwrap_or_default();
        let width = com.width;
        let height = com.height;
        // The dragged icon autoscrolls the layer holding the TYPE_INV
        // (TS 9990-10017); track the live scroll here and write it back
        // after the children loop (the `com` borrow must end first).
        let base_scroll = com.scroll_pos;
        let mut layer_scroll = base_scroll;
        let layer_sh = com.scroll_height;

        let left = surface.clip_min_x;
        let top = surface.clip_min_y;
        let right = surface.clip_max_x;
        let bottom = surface.clip_max_y;
        surface.set_clipping(x, y, x + width, y + height);

        for i in 0..children.len() {
            let child_id = children[i] as usize;
            // TS 9926: `clientComponent(child)` fills the friend/ignore
            // text/button/scroll fields before the child plots.
            client.client_component(child_id as i32);
            // Field-scoped view (shared decode + this client's overlay):
            // the TYPE_INV arm writes `client.obj_grab_y` mid-iteration, so
            // the whole-client `if_()` borrow cannot span the arm.
            let base = client.ifaces.get(child_id).and_then(|o| o.as_deref());
            let ov: &IfTypeMut = match client.ifaces_mut.get(child_id).and_then(|o| o.as_deref()) {
                Some(ov) => ov,
                None => default_mut(),
            };
            let Some(child) = base.map(|base| IfTypeView::new(base, ov)) else {
                continue;
            };
            let child_x = child_x[i] + x + child.x;
            let child_y = child_y[i] + y - scroll_y + child.y;

            match child.r#type {
                ComponentType::TYPE_LAYER => {
                    // TS 9930-9938: clamp the child's scroll position before
                    // recursing with it (max first, then min, sequentially).
                    let mut scroll_pos = child.scroll_pos;
                    if scroll_pos > child.scroll_height - child.height {
                        scroll_pos = child.scroll_height - child.height;
                    }
                    if scroll_pos < 0 {
                        scroll_pos = 0;
                    }
                    if scroll_pos != child.scroll_pos {
                        if let Some(c) = client.ifaces_mut.get_mut(child_id).and_then(|o| o.as_mut()) {
                            c.scroll_pos = scroll_pos;
                        }
                    }
                    self.draw_interface(client, children[i], child_x, child_y, scroll_pos, surface);
                    // drawScrollbar (TS 9941): a scrollable layer draws its
                    // scrollbar after the recurse.
                    let (child_w, child_h, child_sh) = client
                        .if_(child_id)
                        .map(|c| (c.width, c.height, c.scroll_height))
                        .unwrap_or((0, 0, 0));
                    if child_sh > child_h {
                        self.draw_scrollbar(client, 
                            surface,
                            child_x + child_w,
                            child_y,
                            scroll_pos,
                            child_sh,
                            child_h,
                        );
                    }
                }
                ComponentType::TYPE_RECT => {
                    // TS 10041-10059: hovered picks `colour_over`/
                    // `colour2_over`, from the `over*ComId` pointer state.
                    let hovered = client.over_main_com_id == child.id
                        || client.over_side_com_id == child.id
                        || client.over_chat_com_id == child.id;
                    let colour = if self.get_if_active(client, child.id) {
                        if hovered && child.colour2_over != 0 {
                            child.colour2_over
                        } else {
                            child.colour2
                        }
                    } else if hovered && child.colour_over != 0 {
                        child.colour_over
                    } else {
                        child.colour
                    };
                    if child.trans == 0 {
                        if child.fill {
                            surface.fill_rect(child_x, child_y, child.width, child.height, colour);
                        } else {
                            surface.draw_rect(child_x, child_y, child.width, child.height, colour);
                        }
                    } else if child.fill {
                        surface.fill_rect_trans(
                            child_x,
                            child_y,
                            child.width,
                            child.height,
                            colour,
                            256 - (child.trans & 0xff),
                        );
                    } else {
                        surface.draw_rect(child_x, child_y, child.width, child.height, colour);
                        surface.draw_rect_trans(
                            child_x,
                            child_y,
                            child.width,
                            child.height,
                            colour,
                            256 - (child.trans & 0xff),
                        );
                    }
                }
                ComponentType::TYPE_TEXT => {
                    let active = self.get_if_active(client, child.id);
                    let mut text = child.text.to_string();
                    // TS 10077-10098: hovered picks `colour_over`/
                    // `colour2_over`; an active text renders `text2`.
                    let hovered = client.over_main_com_id == child.id
                        || client.over_side_com_id == child.id
                        || client.over_chat_com_id == child.id;
                    let mut colour = if active {
                        if hovered && child.colour2_over != 0 {
                            child.colour2_over
                        } else {
                            child.colour2
                        }
                    } else if hovered && child.colour_over != 0 {
                        child.colour_over
                    } else {
                        child.colour
                    };
                    if active && !child.text2.is_empty() {
                        text = child.text2.clone();
                    }
                    // TS 10101-10104: the latched pause button shows its
                    // wait text in the base colour.
                    if child.button_type == ButtonType::BUTTON_CONTINUE && client.resumed_pause_button {
                        text = "Please wait...".into();
                        colour = child.colour;
                    }

                    // TS 10107-10116: the chat-area colour remap.
                    if surface.width == 479 {
                        if colour == 0xffff00 {
                            colour = 0x0000ff;
                        }
                        if colour == 0x00c000 {
                            colour = 0xffffff;
                        }
                    }

                    // TS 10120-10167: substitute `%1`-`%5` with
                    // `inf(getIfVar(child, n))` before the `\n` split.
                    if text.contains('%') {
                        for n in 0..5 {
                            let token = format!("%{}", n + 1);
                            while let Some(index) = text.find(token.as_str()) {
                                let value = self.get_if_var(client, child.id, n).unwrap_or(-2);
                                text = format!(
                                    "{}{}{}",
                                    &text[..index],
                                    inf(value),
                                    &text[index + 2..]
                                );
                            }
                        }
                    }

                    let font = match child.font {
                        1 => self.media.p12.as_ref(),
                        2 => self.media.b12.as_ref(),
                        3 => self.media.q8.as_ref(),
                        _ => self.media.p11.as_ref(),
                    };
                    let Some(font) = font else {
                        continue;
                    };
                    let mut line_y = child_y + font.height;
                    while !text.is_empty() {
                        let (split, rest) = match text.find("\\n") {
                            Some(nl) => (text[..nl].to_string(), text[nl + 2..].to_string()),
                            None => (text.clone(), String::new()),
                        };
                        if child.centre {
                            font.centre_string_tag(surface, &split, child_x + child.width / 2, line_y, colour, child.shadow);
                        } else {
                            font.draw_string_tag(surface, &split, child_x, line_y, colour, child.shadow);
                        }
                        line_y += font.height;
                        text = rest;
                    }
                }
                ComponentType::TYPE_GRAPHIC => {
                    // TS 10187-10190: `getIfActive` picks graphic2, else
                    // graphic.
                    let graphic_name =
                        if self.get_if_active(client, child.id) && !child.graphic2_name.is_empty() {
                            child.graphic2_name.as_str()
                        } else {
                            child.graphic_name
                        };
                    // "name,index" as unpacked from IfType.ts 251-262.
                    if let Some((name, index)) = graphic_name.rsplit_once(',') {
                        if let Ok(index) = index.trim().parse::<i32>() {
                            if let Some(sprite) = Self::graphic_sprite(
                                &mut self.graphic_sprites,
                                &client.config.cache_dir,
                                name,
                                index,
                            ) {
                                sprite.plot_sprite(surface, child_x, child_y);
                            }
                        }
                    }
                }
                ComponentType::TYPE_INV => {
                    // Java Client.java 9746-9820 / TS 9965-10039: the slot
                    // grid of item icons (lit 2D obj sprites) and
                    // stack-count text. The dragged slot plots
                    // `trans_plot_sprite` at 128 alpha with the grab offset
                    // (and autoscrolls the parent layer at the viewport
                    // edges); the last OP_HELD slot stays translucent until
                    // the `selected_area` timeout; `use_mode == 1` outlines
                    // the selected use target in white (16777215).
                    let Some(link_obj_type) = &child.link_obj_type else {
                        continue;
                    };
                    let Some(link_obj_number) = &child.link_obj_number else {
                        continue;
                    };
                    let mut slot = 0;
                    for row in 0..child.height {
                        for col in 0..child.width {
                            let mut slot_x = child_x + col * (child.margin_x + 32);
                            let mut slot_y = child_y + row * (child.margin_y + 32);
                            if slot < 20 {
                                if let Some(xs) = &child.inv_background_x {
                                    slot_x += xs[slot as usize];
                                }
                                if let Some(ys) = &child.inv_background_y {
                                    slot_y += ys[slot as usize];
                                }
                            }
                            if link_obj_type.get(slot as usize).copied().unwrap_or(0) > 0 {
                                let id = link_obj_type[slot as usize] - 1;
                                let count = link_obj_number[slot as usize];
                                let mut dx = 0;
                                let mut dy = 0;
                                let dragging = client.obj_drag_area != 0
                                    && client.obj_drag_slot == slot
                                    && client.obj_drag_com_id == child.id;
                                // TS 9967-9968: the dragged slot draws even
                                // outside the clip rect (it follows the
                                // pointer past the panel edge).
                                if (slot_x > surface.clip_min_x - 32
                                    && slot_x < surface.clip_max_x
                                    && slot_y > surface.clip_min_y - 32
                                    && slot_y < surface.clip_max_y)
                                    || (client.obj_drag_area != 0 && client.obj_drag_slot == slot)
                                {
                                    let outline = if client.use_mode == 1
                                        && client.obj_selected_slot == slot
                                        && client.obj_selected_com_id == child.id
                                    {
                                        16777215
                                    } else {
                                        0
                                    };
                                    if let Some(sprite) = ObjType::get_sprite(
                                        &client.cache,
                                        &mut self.pix3d,
                                        id,
                                        outline,
                                        count,
                                    ) {
                                        if dragging {
                                            // TS 9975-9989: the grab offset,
                                            // snapped to 0 under ±5px and
                                            // before 5 held cycles.
                                            dx = client.shell.mouse_x - client.obj_grab_x;
                                            dy = client.shell.mouse_y - client.obj_grab_y;
                                            if dx < 5 && dx > -5 {
                                                dx = 0;
                                            }
                                            if dy < 5 && dy > -5 {
                                                dy = 0;
                                            }
                                            if client.obj_drag_cycles < 5 {
                                                dx = 0;
                                                dy = 0;
                                            }
                                            sprite.trans_plot_sprite(
                                                surface,
                                                slot_x + dx,
                                                slot_y + dy,
                                                128,
                                            );
                                            // TS 9990-10017: dragging past
                                            // the layer's viewport edge
                                            // autoscrolls the parent layer
                                            // and shifts the grab point by
                                            // the same amount.
                                            if slot_y + dy < surface.clip_min_y && layer_scroll > 0
                                            {
                                                let mut autoscroll =
                                                    ((surface.clip_min_y - slot_y - dy)
                                                        * client.world_update_num)
                                                        / 3;
                                                if autoscroll > client.world_update_num * 10 {
                                                    autoscroll = client.world_update_num * 10;
                                                }
                                                if autoscroll > layer_scroll {
                                                    autoscroll = layer_scroll;
                                                }
                                                layer_scroll -= autoscroll;
                                                client.obj_grab_y += autoscroll;
                                            }
                                            if slot_y + dy + 32 > surface.clip_max_y
                                                && layer_scroll < layer_sh - height
                                            {
                                                let mut autoscroll = ((slot_y + dy + 32
                                                    - surface.clip_max_y)
                                                    * client.world_update_num)
                                                    / 3;
                                                if autoscroll > client.world_update_num * 10 {
                                                    autoscroll = client.world_update_num * 10;
                                                }
                                                if autoscroll
                                                    > layer_sh - height - layer_scroll
                                                {
                                                    autoscroll =
                                                        layer_sh - height - layer_scroll;
                                                }
                                                layer_scroll += autoscroll;
                                                client.obj_grab_y -= autoscroll;
                                            }
                                        } else if client.selected_area != 0
                                            && client.selected_item == slot
                                            && client.selected_com_id == child.id
                                        {
                                            // TS 10019-10021: the OP_HELD
                                            // slot stays translucent until
                                            // its 15-cycle timeout.
                                            sprite.trans_plot_sprite(surface, slot_x, slot_y, 128);
                                        } else {
                                            sprite.plot_sprite(surface, slot_x, slot_y);
                                        }
                                        // Java 9807-9811: stack counts when
                                        // the sprite is a stack (owi 33) or
                                        // the count isn't 1; the drag offset
                                        // follows the icon (TS 10027-10029).
                                        if sprite.owi == 33 || count != 1 {
                                            let text = self.inv_number(client, count);
                                            if let Some(p11) = self.media.p11.as_ref() {
                                                p11.draw_string_tag(
                                                    surface,
                                                    &text,
                                                    slot_x + dx + 1,
                                                    slot_y + dy + 10,
                                                    0,
                                                    false,
                                                );
                                                p11.draw_string_tag(
                                                    surface,
                                                    &text,
                                                    slot_x + dx,
                                                    slot_y + dy + 9,
                                                    16776960,
                                                    false,
                                                );
                                            }
                                        }
                                    }
                                }
                            } else if slot < 20 {
                                // Java 9813-9818: the slot-frame sprite,
                                // depacked on demand from the "name,index"
                                // kept at unpack (Java IfType.java 287-300).
                                if let Some(names) = &child.inv_background_name {
                                    if let Some(Some(name)) = names.get(slot as usize) {
                                        if let Some((name, index)) = name.rsplit_once(',') {
                                            if let Ok(index) = index.trim().parse::<i32>() {
                                                if let Some(sprite) = Self::graphic_sprite(
                                                    &mut self.graphic_sprites,
                                                    &client.config.cache_dir,
                                                    name,
                                                    index,
                                                ) {
                                                    sprite.plot_sprite(surface, slot_x, slot_y);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            slot += 1;
                        }
                    }
                }
                ComponentType::TYPE_INV_TEXT => {
                    // Java Client.java 9972-9994 / TS 10228-10261: the grid
                    // of object names, with `nice_number` stack counts. The
                    // `cache.obj` index panics out of range, so slots past
                    // the loaded `objs` are skipped.
                    let Some(link_obj_type) = &child.link_obj_type else {
                        continue;
                    };
                    let Some(link_obj_number) = &child.link_obj_number else {
                        continue;
                    };
                    let mut slot = 0;
                    for row in 0..child.height {
                        for col in 0..child.width {
                            if link_obj_type.get(slot as usize).copied().unwrap_or(0) > 0 {
                                let id = link_obj_type[slot as usize] - 1;
                                if id as usize >= client.cache.objs.len() {
                                    slot += 1;
                                    continue;
                                }
                                let count = link_obj_number[slot as usize];
                                let mut text = client.cache.objs[id as usize].name.clone();
                                if client.cache.objs[id as usize].stackable || count != 1 {
                                    text.push_str(&format!(" x{}", self.nice_number(client, count)));
                                }
                                let text_x = child_x + col * (child.margin_x + 115);
                                let text_y = child_y + row * (child.margin_y + 12);
                                let font = match child.font {
                                    1 => self.media.p12.as_ref(),
                                    2 => self.media.b12.as_ref(),
                                    3 => self.media.q8.as_ref(),
                                    _ => self.media.p11.as_ref(),
                                };
                                if let Some(font) = font {
                                    if child.centre {
                                        font.centre_string_tag(
                                            surface,
                                            &text,
                                            text_x + child.width / 2,
                                            text_y,
                                            child.colour,
                                            child.shadow,
                                        );
                                    } else {
                                        font.draw_string_tag(
                                            surface,
                                            &text,
                                            text_x,
                                            text_y,
                                            child.colour,
                                            child.shadow,
                                        );
                                    }
                                }
                            }
                            slot += 1;
                        }
                    }
                }
                ComponentType::TYPE_MODEL => {
                    // Java Client.java 9944-9970 / TS 10200-10217: the 3D
                    // model centred on the component. Save/restore the
                    // raster origin; a missing/unloaded model skips.
                    let saved_origin_x = self.pix3d.origin_x;
                    let saved_origin_y = self.pix3d.origin_y;
                    self.pix3d.origin_x = child_x + child.width / 2;
                    self.pix3d.origin_y = child_y + child.height / 2;

                    let sin_xan = Pix3D::sin_table()
                        .get(child.model_xan as usize)
                        .copied()
                        .unwrap_or(0);
                    let cos_xan = Pix3D::cos_table()
                        .get(child.model_xan as usize)
                        .copied()
                        .unwrap_or(0);
                    let eye_y = sin_xan.wrapping_mul(child.model_zoom) >> 16;
                    let eye_z = cos_xan.wrapping_mul(child.model_zoom) >> 16;

                    let active = self.get_if_active(client, child.id);
                    let model_anim = if active { child.model_anim2 } else { child.model_anim };
                    let local_player = client.local_player.as_ref();
                    let model = if model_anim == -1 {
                        child.get_temp_model(&client.cache, local_player, -1, -1, active)
                    } else if (model_anim as usize) < client.cache.seqs.len() {
                        let seq = &client.cache.seqs[model_anim as usize];
                        let frame = child.anim_frame as usize;
                        match (seq.frames.as_ref(), seq.iframes.as_ref()) {
                            (Some(frames), Some(iframes))
                                if frame < frames.len() && frame < iframes.len() =>
                            {
                                child.get_temp_model(
                                    &client.cache,
                                    local_player,
                                    frames[frame],
                                    iframes[frame],
                                    active,
                                )
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(model) = model {
                        model.obj_render(
                            &mut self.pix3d,
                            surface,
                            0,
                            child.model_yan,
                            0,
                            child.model_xan,
                            0,
                            eye_y,
                            eye_z,
                        );
                    }
                    self.pix3d.origin_x = saved_origin_x;
                    self.pix3d.origin_y = saved_origin_y;
                }
                _ => {}
            }
        }

        // write back the drag-autoscrolled layer scroll (TS 9990-10017).
        if layer_scroll != base_scroll {
            if let Some(c) = client.ifaces_mut.get_mut(com_id as usize).and_then(|o| o.as_mut()) {
                c.scroll_pos = layer_scroll;
            }
        }

        surface.set_clipping(left, top, right, bottom);
    }

    /// `drawScrollbar` from client-ts (10331-10355): the `scrollbar` cap
    /// sprites at `x, y` and `x, y+height-16`, the `0x23201b` track fill,
    /// and the grip with its highlight/lowlight edges. Missing cap sprites
    /// (`scrollbar1`/`scrollbar2` are `None` without the `media` pack) skip
    /// the two Pix8 plots; the track and grip always fill.
    pub fn draw_scrollbar(
        &mut self, _client: &mut Client,
        surface: &mut Pix2D,
        x: i32,
        y: i32,
        scroll_y: i32,
        scroll_height: i32,
        height: i32,
    ) {
        if let Some(sprite) = &self.media.scrollbar1 {
            sprite.plot_sprite(surface, x, y);
        }
        if let Some(sprite) = &self.media.scrollbar2 {
            sprite.plot_sprite(surface, x, y + height - 16);
        }
        surface.fill_rect(x, y + 16, 16, height - 32, 0x23201b);
        let mut grip_size = ((height - 32) * height) / scroll_height;
        if grip_size < 8 {
            grip_size = 8;
        }
        let grip_y = ((height - grip_size - 32) * scroll_y) / (scroll_height - height);
        surface.fill_rect(x, y + grip_y + 16, 16, grip_size, 0x4d4233);
        surface.vline(x, y + grip_y + 16, grip_size, 0x766654);
        surface.vline(x + 1, y + grip_y + 16, grip_size, 0x766654);
        surface.hline(x, y + grip_y + 16, 16, 0x766654);
        surface.hline(x, y + grip_y + 17, 16, 0x766654);
        surface.vline(x + 15, y + grip_y + 16, grip_size, 0x332d25);
        surface.vline(x + 14, y + grip_y + 17, grip_size - 1, 0x332d25);
        surface.hline(x, y + grip_y + grip_size + 15, 16, 0x332d25);
        surface.hline(x + 1, y + grip_y + grip_size + 14, 15, 0x332d25);
    }

    fn graphic_sprite<'a>(
        cache: &'a mut HashMap<(String, i32), Option<Pix32>>,
        cache_dir: &str,
        name: &str,
        index: i32,
    ) -> Option<&'a Pix32> {
        let key = (name.to_string(), index);
        if !cache.contains_key(&key) {
            let sprite = std::fs::read(format!("{cache_dir}/media"))
                .ok()
                .and_then(|bytes| Pix32::depack(&JagFile::new(bytes), name, index).ok());
            cache.insert(key.clone(), sprite);
        }
        cache.get(&key).and_then(|s| s.as_ref())
    }

    /// `getIfActive` from client-ts (10361): comparator scripts pick the
    /// active colour for a component. Every comparator's script runs
    /// through `get_if_var`; a component without scripts reads inactive.
    /// `pub(crate)` so `client.rs`'s `animate_interface` can select the
    /// active model anim.
    pub(crate) fn get_if_active(&self, client: &Client, com_id: i32) -> bool {
        let Some(com) = client.if_(com_id as usize) else {
            return false;
        };
        let Some(comparator) = &com.script_comparator else {
            return false;
        };
        let Some(operand) = &com.script_operand else {
            return false;
        };
        for i in 0..comparator.len() {
            let Some(value) = self.get_if_var(client, com_id, i as i32) else {
                return false;
            };
            match comparator[i] {
                2 => {
                    if value >= operand[i] {
                        return false;
                    }
                }
                3 => {
                    if value <= operand[i] {
                        return false;
                    }
                }
                4 => {
                    if value == operand[i] {
                        return false;
                    }
                }
                _ => {
                    if value != operand[i] {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// `getIfVar` from client-ts (10394): run the IfType script VM (opcodes
    /// 0-20) for the comparator and `%1`-`%5` scripts. `None` stands in for
    /// TS `-2` (no scripts / script id out of range), which keeps
    /// `get_if_active` treating such components as inactive; malformed
    /// scripts map to TS `-1` (the catch).
    pub fn get_if_var(&self, client: &Client, com_id: i32, script_id: i32) -> Option<i32> {
        let com = client.if_(com_id as usize)?;
        let scripts = com.scripts.as_ref()?;
        if script_id < 0 || script_id as usize >= scripts.len() {
            return None;
        }
        let script = &scripts[script_id as usize];
        let mut acc: i32 = 0;
        let mut pc: usize = 0;
        let mut arithmetic: i32 = 0;
        loop {
            let opcode = match script.get(pc) {
                Some(&op) => op,
                None => return Some(-1),
            };
            pc += 1;
            let mut register: i32 = 0;
            let mut next_arithmetic: i32 = 0;
            match opcode {
                0 => return Some(acc),
                1 => {
                    // stat_level {skill}
                    let Some(skill) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    register = client.stat_effective_level.get(skill as usize).copied().unwrap_or(0);
                }
                2 => {
                    // stat_base_level {skill}
                    let Some(skill) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    register = client.stat_base_level.get(skill as usize).copied().unwrap_or(0);
                }
                3 => {
                    // stat_xp {skill}
                    let Some(skill) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    register = client.stat_xp.get(skill as usize).copied().unwrap_or(0);
                }
                4 => {
                    // inv_count {interface id} {obj id}
                    let Some(com_id) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let Some(obj) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let obj = obj + 1;
                    // TS `IfType.list[id]` on an out-of-range id throws,
                    // which the catch maps to -1.
                    let Some(com) = client.if_(com_id as usize) else {
                        return Some(-1);
                    };
                    if let (Some(link_obj_type), Some(link_obj_number)) =
                        (&com.link_obj_type, &com.link_obj_number)
                    {
                        if obj >= 0
                            && (obj as usize) < client.cache.objs.len()
                            && (!client.cache.objs[obj as usize].members || client.config.members)
                        {
                            for (link_type, link_number) in
                                link_obj_type.iter().zip(link_obj_number)
                            {
                                if *link_type == obj {
                                    register += *link_number;
                                }
                            }
                        }
                    }
                }
                5 => {
                    // pushvar {id}
                    let Some(id) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    register = client.var.get(id as usize).copied().unwrap_or(0);
                }
                6 => {
                    // stat_xp_remaining {skill}
                    let Some(skill) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let base = client.stat_base_level.get(skill as usize).copied().unwrap_or(0);
                    register = level_experience().get((base - 1) as usize).copied().unwrap_or(0);
                }
                7 => {
                    let Some(id) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let value = client.var.get(id as usize).copied().unwrap_or(0);
                    // TS `((var * 100) / 46875) | 0`
                    register = ((value as i64 * 100) / 46875) as i32;
                }
                8 => {
                    // combat level: `this.localPlayer?.combatLevel || 0`
                    register = client.local_player.as_ref().map(|p| p.combat_level).unwrap_or(0);
                }
                9 => {
                    // total level
                    for i in 0..Skill::count {
                        if Skill::used[i] {
                            register += client.stat_base_level.get(i).copied().unwrap_or(0);
                        }
                    }
                }
                10 => {
                    // inv_contains {interface id} {obj id}
                    let Some(com_id) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let Some(obj) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let obj = obj + 1;
                    let Some(com) = client.if_(com_id as usize) else {
                        return Some(-1);
                    };
                    if let Some(link_obj_type) = &com.link_obj_type {
                        if obj >= 0
                            && (obj as usize) < client.cache.objs.len()
                            && (!client.cache.objs[obj as usize].members || client.config.members)
                            && link_obj_type.contains(&obj)
                        {
                            register = 999_999_999;
                        }
                    }
                }
                11 => {
                    // runenergy
                    register = client.runenergy;
                }
                12 => {
                    // runweight
                    register = client.runweight;
                }
                13 => {
                    // testbit {varp} {bit: 0..31}
                    let Some(varp) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let Some(lsb) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let varp = client.var.get(varp as usize).copied().unwrap_or(0);
                    // TS `0x1 << lsb` masks the shift to 5 bits
                    let lsb = (lsb & 31) as u32;
                    register = if varp & (1i32 << lsb) == 0 { 0 } else { 1 };
                }
                14 => {
                    // push_varbit {varbit}
                    let Some(id) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    // TS `VarBitType.list[id]` on an out-of-range id throws,
                    // which the catch maps to -1.
                    let Some(varbit) = client.cache.varbits.get(id as usize) else {
                        return Some(-1);
                    };
                    let value = client.var.get(varbit.basevar as usize).copied().unwrap_or(0);
                    // TS `>> startbit` masks the shift to 5 bits
                    let startbit = (varbit.startbit & 31) as u32;
                    // `Client.readbit[endbit - startbit]` from client-ts 120:
                    // readbit[31] = 2^32-1 wraps to -1 in the Int32Array,
                    // and a wider span is undefined in TS.
                    let mask = match varbit.endbit - varbit.startbit {
                        0..=30 => (1 << (varbit.endbit - varbit.startbit + 1)) - 1,
                        _ => -1,
                    };
                    register = (value >> startbit) & mask;
                }
                15 => next_arithmetic = 1, // subtract
                16 => next_arithmetic = 2, // divide
                17 => next_arithmetic = 3, // multiply
                18 => {
                    // coordx
                    if let Some(player) = &client.local_player {
                        register = (player.x >> 7) + client.map_build_base_x;
                    }
                }
                19 => {
                    // coordz
                    if let Some(player) = &client.local_player {
                        register = (player.z >> 7) + client.map_build_base_z;
                    }
                }
                20 => {
                    // push_constant
                    let Some(constant) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    register = constant;
                }
                _ => {}
            }

            if next_arithmetic == 0 {
                match arithmetic {
                    0 => acc = acc.wrapping_add(register),
                    1 => acc = acc.wrapping_sub(register),
                    2 => {
                        if register != 0 {
                            acc = acc.wrapping_div(register);
                        }
                    }
                    3 => acc = acc.wrapping_mul(register),
                    _ => {}
                }
                arithmetic = 0;
            } else {
                arithmetic = next_arithmetic;
            }
        }
    }
}

/// Read the next script operand at `*pc` (TS `script[pc++]`); `None` when
/// the script runs past its end, which the VM maps to TS `-1`.
fn next_operand(script: &[i32], pc: &mut usize) -> Option<i32> {
    let value = script.get(*pc).copied()?;
    *pc += 1;
    Some(value)
}

/// `inf` from client-ts (10357): huge values render as `*`.
fn inf(value: i32) -> String {
    if value >= 999_999_999 {
        "*".into()
    } else {
        value.to_string()
    }
}

impl Renderer {
    /// `drawChat` from client-ts (11125): plot `chatback` into `area_chat`,
    /// then the social prompt (TS 11133-11135), the enter-amount prompt
    /// (TS 11136-11138) or the plain chat branch (TS 11149-11267): clip
    /// (0,0,463,77), the 100 chat lines as
    /// TS 11152-11244, the `username:` + `chat_input + '*'` input line at
    /// y=90, and the `hline` at 77; blit at (17, 357). Deviations: a chat
    /// modal or tutorial interface (TS 11142-11146) draws into `area_chat`
    /// in place of the plain chat; the `modIcons`/`drawScrollbar` sprites
    /// load with Task 14, and the trailing `areaGame.setPixels()` is a
    /// no-op here (no global Pix2D target).
    pub(crate) fn draw_chat(&mut self, client: &mut Client) {
        let mut chat = self.area_chat.take();
        if let Some(chat) = chat.as_mut() {
            let w = chat.width;
            let h = chat.height;
            let mut surface = Pix2D::with_pixels(&mut chat.pixels, w, h);
            if let Some(chatback) = &self.media.chatback {
                chatback.plot_sprite(&mut surface, 0, 0);
            }
            if client.social_input_open {
                // TS 11133-11135: the social prompt replaces the chat lines.
                if let Some(b12) = self.media.b12.as_ref() {
                    b12.centre_string(&mut surface, Some(&client.social_input_header), 239, 40, Colour::BLACK);
                    b12.centre_string(&mut surface, Some(&format!("{}*", client.social_input)), 239, 60, Colour::DARKBLUE);
                }
            } else if client.dialog_input_open {
                // TS 11136-11138: the enter-amount prompt replaces the chat
                // lines.
                if let Some(b12) = self.media.b12.as_ref() {
                    b12.centre_string(&mut surface, Some("Enter amount:"), 239, 40, Colour::BLACK);
                    b12.centre_string(&mut surface, Some(&format!("{}*", client.dialog_input)), 239, 60, Colour::DARKBLUE);
                }
            } else if client.chat_modal_id != -1 {
                // TS 11142-11146: a chat interface replaces the chat lines
                // (the chatback frame still plots underneath it).
                self.pix3d.set_clipping(surface.width, surface.height);
                self.draw_interface(client, client.chat_modal_id, 0, 0, 0, &mut surface);
            } else if client.tut_com_id != -1 {
                self.pix3d.set_clipping(surface.width, surface.height);
                self.draw_interface(client, client.tut_com_id, 0, 0, 0, &mut surface);
            } else {
                let mut line = 0;
                surface.set_clipping(0, 0, 463, 77);

                for i in 0..100 {
                    let message = client.chat_text[i].clone();
                    if message.is_empty() {
                        continue;
                    }
                    let r#type = client.chat_type[i];
                    let y = client.chat_scroll_pos + 70 - line * 14;

                    let mut sender = client.chat_username[i].clone();
                    let mut modlevel = 0;
                    if sender.starts_with("@cr1@") {
                        sender = sender[5..].to_string();
                        modlevel = 1;
                    } else if sender.starts_with("@cr2@") {
                        sender = sender[5..].to_string();
                        modlevel = 2;
                    }

                    if r#type == 0 {
                        if y > 0 && y < 110 {
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some(&message), 4, y, Colour::BLACK);
                            }
                        }
                        line += 1;
                    } else if (r#type == 1 || r#type == 2)
                        && (r#type == 1
                            || client.chat_public_mode == 0
                            || (client.chat_public_mode == 1 && client.is_friend(&sender)))
                    {
                        if y > 0 && y < 110 {
                            let mut x = 4;
                            // Java 5000-5007: plot the mod_icons crown at
                            // y - 12, then advance the 14px gutter.
                            if modlevel == 1 {
                                if let Some(sprite) = &self.media.mod_icons[0] {
                                    sprite.plot_sprite(&mut surface, x, y - 12);
                                }
                                x += 14;
                            }
                            if modlevel == 2 {
                                if let Some(sprite) = &self.media.mod_icons[1] {
                                    sprite.plot_sprite(&mut surface, x, y - 12);
                                }
                                x += 14;
                            }
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some(&format!("{sender}:")), x, y, Colour::BLACK);
                                x += font.string_wid(Some(&sender)) + 8;
                                font.draw_string(&mut surface, Some(&message), x, y, Colour::BLUE);
                            }
                        }
                        line += 1;
                    } else if (r#type == 3 || r#type == 7)
                        && client.split_private_chat == 0
                        && (r#type == 7
                            || client.chat_private_mode == 0
                            || (client.chat_private_mode == 1 && client.is_friend(&sender)))
                    {
                        if y > 0 && y < 110 {
                            let mut x = 4;
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some("From"), x, y, Colour::BLACK);
                                x += font.string_wid(Some("From "));
                            }
                            // Java 5019-5026: the crown plots after the
                            // "From " label.
                            if modlevel == 1 {
                                if let Some(sprite) = &self.media.mod_icons[0] {
                                    sprite.plot_sprite(&mut surface, x, y - 12);
                                }
                                x += 14;
                            }
                            if modlevel == 2 {
                                if let Some(sprite) = &self.media.mod_icons[1] {
                                    sprite.plot_sprite(&mut surface, x, y - 12);
                                }
                                x += 14;
                            }
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some(&format!("{sender}:")), x, y, Colour::BLACK);
                                x += font.string_wid(Some(&sender)) + 8;
                                font.draw_string(&mut surface, Some(&message), x, y, Colour::DARKRED);
                            }
                        }
                        line += 1;
                    } else if r#type == 4 && (client.chat_trade_mode == 0 || (client.chat_trade_mode == 1 && client.is_friend(&sender))) {
                        if y > 0 && y < 110 {
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some(&format!("{sender} {message}")), 4, y, 0x800080);
                            }
                        }
                        line += 1;
                    } else if r#type == 5 && client.split_private_chat == 0 && client.chat_private_mode < 2 {
                        if y > 0 && y < 110 {
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some(&message), 4, y, Colour::DARKRED);
                            }
                        }
                        line += 1;
                    } else if r#type == 6 && client.split_private_chat == 0 && client.chat_private_mode < 2 {
                        if y > 0 && y < 110 {
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some(&format!("To {sender}:")), 4, y, Colour::BLACK);
                                font.draw_string(
                                    &mut surface,
                                    Some(&message),
                                    font.string_wid(Some(&format!("To {sender}"))) + 12,
                                    y,
                                    Colour::DARKRED,
                                );
                            }
                        }
                        line += 1;
                    } else if r#type == 8 && (client.chat_trade_mode == 0 || (client.chat_trade_mode == 1 && client.is_friend(&sender))) {
                        if y > 0 && y < 110 {
                            if let Some(font) = self.media.p12.as_ref() {
                                font.draw_string(&mut surface, Some(&format!("{sender} {message}")), 4, y, 0x7e3200);
                            }
                        }
                        line += 1;
                    }
                }

                surface.reset_clipping();

                client.chat_scroll_height = line * 14 + 7;
                if client.chat_scroll_height < 78 {
                    client.chat_scroll_height = 78;
                }
                // drawScrollbar (TS 11252): the chat scrollbar, scrolled
                // from the bottom (scroll_y is 77 at scroll_pos 0).
                self.draw_scrollbar(client, 
                    &mut surface,
                    463,
                    0,
                    client.chat_scroll_height - client.chat_scroll_pos - 77,
                    client.chat_scroll_height,
                    77,
                );

                let username = match client.local_player.as_ref().and_then(|p| p.name.as_ref()) {
                    Some(name) => name.clone(),
                    None => JString::to_screen_name(&client.login_user),
                };

                if let Some(font) = self.media.p12.as_ref() {
                    font.draw_string(&mut surface, Some(&format!("{username}:")), 4, 90, Colour::BLACK);
                    let input_x = font.string_wid(Some(&format!("{username}: "))) + 6;
                    font.draw_string(&mut surface, Some(&format!("{}*", client.chat_input)), input_x, 90, Colour::BLUE);
                }

                surface.hline(0, 77, 479, Colour::BLACK);
            }

            if client.is_menu_open && client.menu_area == 2 {
                self.draw_minimenu(client, &mut surface);
            }
        }

        if let Some(chat) = &chat {
            chat.blit_into(&mut self.draw_area, 17, 357);
        }
        self.area_chat = chat;
    }

    /// `redrawIcons` from client-ts (4005), 1:1: when the flashing tab is the
    /// active one, clear `tut_flash_icon` and send `TUT_CLICKSIDE` +
    /// `active_icon` (4006-4011). Then plot `backhmid1` into
    /// `area_backhmid1` and, when `side_modal_id == -1`, the redstone
    /// highlight under `active_icon` (tabs 0-6) plus the side icons whose
    /// tab is bound (`side_icon[i] != -1`); blit at (516, 160). Then
    /// `backbase2` into `area_backbase2` with the tabs 7-13 redstone and
    /// icons, and blit at (496, 466). Offsets verbatim from 4018-4112. Each
    /// icon plots only when it is not the flashing tab, or it is on its
    /// blink half-cycle (`loop_cycle % 20 < 10`), TS 4037-4112. The
    /// bottom-row guard index quirk of 4090-4111 (checks `side_icon[8]`
    /// while plotting `sideicons[7]`, and so on) is kept 1:1. The trailing
    /// `areaGame.setPixels()` is a no-op here (no global Pix2D target).
    pub(crate) fn draw_icons(&mut self, client: &mut Client) {
        if client.tut_flash_icon != -1 && client.tut_flash_icon == client.active_icon {
            client.tut_flash_icon = -1;
            client.out.p1_enc(ClientProt::TUT_CLICKSIDE.id);
            client.out.p1(client.active_icon);
        }
        if let Some(area) = self.area_backhmid1.as_mut() {
            if let Some(backhmid1) = &self.media.backhmid1 {
                let w = area.width;
                let h = area.height;
                let mut surface = Pix2D::with_pixels(&mut area.pixels, w, h);
                backhmid1.plot_sprite(&mut surface, 0, 0);
                if client.side_modal_id == -1 {
                    // TS reads `sideIcon[activeIcon]` as undefined (true) out
                    // of bounds; `get().copied() != Some(-1)` matches.
                    if client.side_icon.get(client.active_icon as usize).copied() != Some(-1) {
                        // redstone for the top row, tabs 0-6 (4018-4034);
                        // tabs 7-13 plot on `area_backbase2` below.
                        let (redstone, x, y) = match client.active_icon {
                            0 => (&self.media.redstone1, 22, 10),
                            1 => (&self.media.redstone2, 54, 8),
                            2 => (&self.media.redstone2, 82, 8),
                            3 => (&self.media.redstone3, 110, 8),
                            4 => (&self.media.redstone2h, 153, 8),
                            5 => (&self.media.redstone2h, 181, 8),
                            6 => (&self.media.redstone1h, 209, 9),
                            _ => (&None, 0, 0),
                        };
                        if let Some(s) = redstone {
                            s.plot_sprite(&mut surface, x, y);
                        }
                    }
                    for (icon, x, y) in [
                        (0, 29, 13),
                        (1, 53, 11),
                        (2, 82, 11),
                        (3, 115, 12),
                        (4, 153, 13),
                        (5, 180, 11),
                        (6, 208, 13),
                    ] {
                        // 1:1 with 4037-4049: the flashing tab hides on the
                        // off half-cycle (`loop_cycle % 20 >= 10`).
                        if client.side_icon[icon] != -1
                            && (client.tut_flash_icon != icon as i32 || client.loop_cycle % 20 < 10)
                        {
                            if let Some(s) = &self.media.sideicons[icon] {
                                s.plot_sprite(&mut surface, x, y);
                            }
                        }
                    }
                }
            }
        }
        if let Some(area) = &self.area_backhmid1 {
            area.blit_into(&mut self.draw_area, 516, 160);
        }

        if let Some(area) = self.area_backbase2.as_mut() {
            if let Some(backbase2) = &self.media.backbase2 {
                let w = area.width;
                let h = area.height;
                let mut surface = Pix2D::with_pixels(&mut area.pixels, w, h);
                backbase2.plot_sprite(&mut surface, 0, 0);
                if client.side_modal_id == -1 {
                    // redstone for the bottom row, tabs 7-13 (4072-4088).
                    if client.side_icon.get(client.active_icon as usize).copied() != Some(-1) {
                        let (redstone, x, y) = match client.active_icon {
                            7 => (&self.media.redstone1v, 42, 0),
                            8 => (&self.media.redstone2v, 74, 0),
                            9 => (&self.media.redstone2v, 102, 0),
                            10 => (&self.media.redstone3v, 130, 1),
                            11 => (&self.media.redstone2hv, 173, 0),
                            12 => (&self.media.redstone2hv, 201, 0),
                            13 => (&self.media.redstone1hv, 229, 0),
                            _ => (&None, 0, 0),
                        };
                        if let Some(s) = redstone {
                            s.plot_sprite(&mut surface, x, y);
                        }
                    }
                    // 1:1 with 4090-4111: the guard index trails the sprite
                    // index (tab 7's sprite gated on `side_icon[8]`, ...),
                    // and the flashing tab hides on the off half-cycle.
                    for (guard, sprite, x, y) in [
                        (8, 7, 74, 2),
                        (9, 8, 102, 3),
                        (10, 9, 137, 4),
                        (11, 10, 174, 2),
                        (12, 11, 201, 2),
                        (13, 12, 226, 2),
                    ] {
                        if client.side_icon[guard] != -1
                            && (client.tut_flash_icon != guard as i32 || client.loop_cycle % 20 < 10)
                        {
                            if let Some(s) = &self.media.sideicons[sprite] {
                                s.plot_sprite(&mut surface, x, y);
                            }
                        }
                    }
                }
            }
        }
        if let Some(area) = &self.area_backbase2 {
            area.blit_into(&mut self.draw_area, 496, 466);
        }
    }

    /// `minimapDraw` from client-ts (11279): rotate-plot the composed
    /// minimap buffer (146×151 window) and the compass (33×33) into
    /// `area_map` through the `mapback` scanline masks, then the map
    /// functions, ground-object/npc/player dots, hint arrows, the flag
    /// marker, and the white local-player square. `minimap_state == 2`
    /// blackens the masked-out region and draws only the compass (TS
    /// 11286-11299). Deviations: the friend split always draws `mapdots3`
    /// (no friend list is ported), and the `areaGame.setPixels()` target
    /// switches are no-ops (the `Pix2D` surface is bound here).
    pub(crate) fn minimap_draw(&mut self, client: &mut Client) {
        let Some(player) = client.local_player.as_ref() else {
            return;
        };
        let player_x = player.x;
        let player_z = player.z;
        let Some(map) = self.area_map.as_mut() else {
            return;
        };
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);

        if client.minimap_state == 2 {
            if let Some(mapback) = &self.media.mapback {
                let mask = &mapback.data;
                let pixels = &mut surface.pixels;
                // `min` keeps a mismatched mask from writing past the map
                // (TS typed arrays silently ignore out-of-bounds writes).
                for i in 0..mask.len().min(pixels.len()) {
                    if mask[i] == 0 {
                        pixels[i] = 0;
                    }
                }
            }
            if let Some(compass) = &self.media.compass {
                compass.scanline_rotate_plot_sprite(
                    &mut surface,
                    0,
                    0,
                    33,
                    33,
                    25,
                    25,
                    client.orbit_camera_yaw as f64,
                    256,
                    &self.compass_mask_line_offsets,
                    &self.compass_mask_line_lengths,
                );
            }
            return;
        }

        let angle = (client.orbit_camera_yaw + client.macro_minimap_angle) & 0x7ff;
        let anchor_x = (player_x / 32) + 48;
        let anchor_y = 464 - (player_z / 32);

        if let Some(minimap) = &self.minimap {
            minimap.scanline_rotate_plot_sprite(
                &mut surface,
                25,
                5,
                146,
                151,
                anchor_x,
                anchor_y,
                angle as f64,
                client.macro_minimap_zoom + 256,
                &self.minimap_mask_line_offsets,
                &self.minimap_mask_line_lengths,
            );
        }
        if let Some(compass) = &self.media.compass {
            compass.scanline_rotate_plot_sprite(
                &mut surface,
                0,
                0,
                33,
                33,
                25,
                25,
                client.orbit_camera_yaw as f64,
                256,
                &self.compass_mask_line_offsets,
                &self.compass_mask_line_lengths,
            );
        }

        let dot_angle = angle;
        let dot_zoom = client.macro_minimap_zoom + 256;
        let mapback = self.media.mapback.as_ref();
        let mapdots1 = self.media.mapdots1.as_ref();
        let mapdots2 = self.media.mapdots2.as_ref();
        let mapdots3 = self.media.mapdots3.as_ref();

        // TS 11310-11316: map functions (filled by `minimap_build_buffer`;
        // a `mapfunction` entry that is `None` skips the dot).
        for i in 0..self.active_map_function_count as usize {
            let dot_x = self.active_map_function_x[i] * 4 + 2 - (player_x / 32);
            let dot_y = self.active_map_function_z[i] * 4 + 2 - (player_z / 32);
            minimap_draw_dot(
                &mut surface,
                dot_y,
                self.active_map_functions[i].as_ref(),
                dot_x,
                mapback,
                dot_angle,
                dot_zoom,
            );
        }

        // TS 11317-11325: ground objects, one dot per occupied tile.
        for ltx in 0..BuildArea::SIZE {
            for ltz in 0..BuildArea::SIZE {
                if client.world.ground_object_at(client.minusedlevel, ltx, ltz).is_some() {
                    let dot_x = ltx * 4 + 2 - (player_x / 32);
                    let dot_y = ltz * 4 + 2 - (player_z / 32);
                    minimap_draw_dot(&mut surface, dot_y, mapdots1, dot_x, mapback, dot_angle, dot_zoom);
                }
            }
        }

        // TS 11327-11335: NPCs with a minimap flag.
        for i in 0..client.npc_count as usize {
            let npc_id = client.npc_ids[i];
            let Some(npc) = client.npc.get(npc_id as usize).and_then(|n| n.as_deref()) else {
                continue;
            };
            let Some(npc_type_id) = npc.r#type else {
                continue;
            };
            if npc.is_ready() && client.cache.npc(npc_type_id).minimap {
                let dot_x = (npc.x / 32) - (player_x / 32);
                let dot_y = (npc.z / 32) - (player_z / 32);
                minimap_draw_dot(&mut surface, dot_y, mapdots2, dot_x, mapback, dot_angle, dot_zoom);
            }
        }

        // TS 11337-11357: players (friends split onto dots4; no friend list
        // is ported, so everyone draws dots3).
        for i in 0..client.player_count as usize {
            let player_id = client.player_ids[i];
            let Some(p) = client.players.get(player_id as usize).and_then(|p| p.as_deref()) else {
                continue;
            };
            if p.is_ready() && p.name.is_some() {
                let dot_x = (p.x / 32) - (player_x / 32);
                let dot_y = (p.z / 32) - (player_z / 32);
                minimap_draw_dot(&mut surface, dot_y, mapdots3, dot_x, mapback, dot_angle, dot_zoom);
            }
        }

        // TS 11359-11382: the hint arrow (the branch is dead while
        // `hint_type` stays 0).
        if client.hint_type != 0 && client.loop_cycle % 20 < 10 {
            if client.hint_type == 1
                && client.hint_npc >= 0
                && (client.hint_npc as usize) < client.npc.len()
            {
                if let Some(npc) = client.npc[client.hint_npc as usize].as_ref() {
                    let arrow_x = (npc.x / 32) - (player_x / 32);
                    let arrow_y = (npc.z / 32) - (player_z / 32);
                    minimap_draw_arrow(
                        &mut surface,
                        arrow_x,
                        arrow_y,
                        self.media.mapmarker2.as_ref(),
                        mapback,
                        self.media.mapedge.as_ref(),
                        dot_angle,
                        dot_zoom,
                    );
                }
            } else if client.hint_type == 2 {
                let arrow_x = (client.hint_tile_x - client.map_build_base_x) * 4 + 2 - (player_x / 32);
                let arrow_y = (client.hint_tile_z - client.map_build_base_z) * 4 + 2 - (player_z / 32);
                minimap_draw_arrow(
                    &mut surface,
                    arrow_x,
                    arrow_y,
                    self.media.mapmarker2.as_ref(),
                    mapback,
                    self.media.mapedge.as_ref(),
                    dot_angle,
                    dot_zoom,
                );
            } else if client.hint_type == 10
                && client.hint_player >= 0
                && (client.hint_player as usize) < client.players.len()
            {
                if let Some(player) = client.players[client.hint_player as usize].as_ref() {
                    let arrow_x = (player.x / 32) - (player_x / 32);
                    let arrow_y = (player.z / 32) - (player_z / 32);
                    minimap_draw_arrow(
                        &mut surface,
                        arrow_x,
                        arrow_y,
                        self.media.mapmarker2.as_ref(),
                        mapback,
                        self.media.mapedge.as_ref(),
                        dot_angle,
                        dot_zoom,
                    );
                }
            }
        }

        // TS 11383-11388: the walk-flag marker.
        if client.minimap_flag_x != 0 {
            let dot_x = client.minimap_flag_x * 4 + 2 - (player_x / 32);
            let dot_y = client.minimap_flag_z * 4 + 2 - (player_z / 32);
            minimap_draw_dot(
                &mut surface,
                dot_y,
                self.media.mapmarker1.as_ref(),
                dot_x,
                mapback,
                dot_angle,
                dot_zoom,
            );
        }

        // TS 11389-11390: the white square local player position in the
        // center of the minimap.
        surface.fill_rect(97, 78, 3, 3, Colour::WHITE);
    }
}

/// `minimapDrawDot` from client-ts (11425): rotate a dot sprite onto the
/// minimap; past 2500 it is masked by `mapback` so it never paints over the
/// ring.
fn minimap_draw_dot(
    surface: &mut Pix2D,
    dy: i32,
    image: Option<&Pix32>,
    dx: i32,
    mapback: Option<&Pix8>,
    angle: i32,
    zoom: i32,
) {
    let Some(image) = image else {
        return;
    };
    let distance = dx * dx + dy * dy;
    if distance > 6400 {
        return;
    }
    let mut sin_angle = Pix3D::sin_table()[angle as usize];
    let mut cos_angle = Pix3D::cos_table()[angle as usize];
    sin_angle = (sin_angle * 256) / zoom;
    cos_angle = (cos_angle * 256) / zoom;
    let x = (dy * sin_angle + dx * cos_angle) >> 16;
    let y = (dy * cos_angle - dx * sin_angle) >> 16;
    if distance > 2500 {
        if let Some(mapback) = mapback {
            image.scanline_plot_sprite(
                surface,
                mapback,
                x + 94 - (image.owi / 2) + 4,
                83 - y - (image.ohi / 2) - 4,
            );
            return;
        }
    }
    image.plot_sprite(surface, x + 94 - (image.owi / 2) + 4, 83 - y - (image.ohi / 2) - 4);
}

/// `minimapDrawArrow` from client-ts (11396): a `mapedge` arrow rotated at
/// the hint target, falling back to a dot when it is near or far. The TS
/// `Math.atan2(x, y)` swapped-argument quirk is kept 1:1.
#[allow(clippy::too_many_arguments)]
fn minimap_draw_arrow(
    surface: &mut Pix2D,
    dx: i32,
    dy: i32,
    image: Option<&Pix32>,
    mapback: Option<&Pix8>,
    mapedge: Option<&Pix32>,
    angle: i32,
    zoom: i32,
) {
    let Some(image) = image else {
        return;
    };
    let distance = dx * dx + dy * dy;
    if distance <= 4225 || distance >= 90000 {
        minimap_draw_dot(surface, dy, Some(image), dx, mapback, angle, zoom);
        return;
    }
    let mut sin_angle = Pix3D::sin_table()[angle as usize];
    let mut cos_angle = Pix3D::cos_table()[angle as usize];
    sin_angle = (sin_angle * 256) / zoom;
    cos_angle = (cos_angle * 256) / zoom;
    let x = (dy * sin_angle + dx * cos_angle) >> 16;
    let y = (dy * cos_angle - dx * sin_angle) >> 16;
    let var13 = f64::atan2(x as f64, y as f64);
    let var15 = (f64::sin(var13) * 63.0) as i32;
    let var16 = (f64::cos(var13) * 57.0) as i32;
    if let Some(mapedge) = mapedge {
        mapedge.rotate_plot_sprite(surface, var15 + 94 + 4 - 10, 83 - var16 - 20, 20, 20, 15, 15, var13, 256);
    }
}

/// `drawDetail(level, tileX, tileZ, inactiveRgb, activeRgb)` from client-ts
/// (5389-5533): the minimap wall/scene lines and ground-decor icons plotted
/// by `minimapBuildBuffer`. Wall/scene shapes write `dst` lines at the
/// per-tile 4×4 offset; locs with a `mapscene` plot their sprite instead
/// (`mapscene` entries that are `None` skip the plot). `surface` is bound
/// to the minimap buffer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_detail(
    world: &World,
    cache: &Cache,
    mapscene: &[Option<Pix8>],
    surface: &mut Pix2D,
    level: i32,
    tile_x: i32,
    tile_z: i32,
    inactive_rgb: i32,
    active_rgb: i32,
) {
    let wall_type = world.wall_type(level, tile_x, tile_z);
    if wall_type != 0 {
        let info = world.type_code2(level, tile_x, tile_z, wall_type);
        let angle = (info >> 6) & 0x3;
        let shape = info & 0x1f;
        let mut rgb = inactive_rgb;
        if wall_type > 0 {
            rgb = active_rgb;
        }

        let dst = &mut surface.pixels;
        let offset = tile_x * 4 + (103 - tile_z) * 512 * 4 + 24624;
        let loc_id = (wall_type >> 14) & 0x7fff;

        let loc = cache.loc(loc_id as usize);
        if loc.mapscene != -1 {
            if let Some(scene) = mapscene.get(loc.mapscene as usize).and_then(|s| s.as_ref()) {
                let offset_x = (loc.width * 4 - scene.wi) / 2;
                let offset_y = (loc.length * 4 - scene.hi) / 2;
                scene.plot_sprite(
                    surface,
                    tile_x * 4 + 48 + offset_x,
                    (BuildArea::SIZE - tile_z - loc.length) * 4 + offset_y + 48,
                );
            }
        } else {
            if shape == LocShape::WALL_STRAIGHT || shape == LocShape::WALL_L {
                if angle == LocAngle::WEST {
                    dst[offset as usize] = rgb;
                    dst[offset as usize + 512] = rgb;
                    dst[offset as usize + 1024] = rgb;
                    dst[offset as usize + 1536] = rgb;
                } else if angle == LocAngle::NORTH {
                    dst[offset as usize] = rgb;
                    dst[offset as usize + 1] = rgb;
                    dst[offset as usize + 2] = rgb;
                    dst[offset as usize + 3] = rgb;
                } else if angle == LocAngle::EAST {
                    dst[offset as usize + 3] = rgb;
                    dst[offset as usize + 3 + 512] = rgb;
                    dst[offset as usize + 3 + 1024] = rgb;
                    dst[offset as usize + 3 + 1536] = rgb;
                } else if angle == LocAngle::SOUTH {
                    dst[offset as usize + 1536] = rgb;
                    dst[offset as usize + 1536 + 1] = rgb;
                    dst[offset as usize + 1536 + 2] = rgb;
                    dst[offset as usize + 1536 + 3] = rgb;
                }
            }

            if shape == LocShape::WALL_SQUARE_CORNER {
                if angle == LocAngle::WEST {
                    dst[offset as usize] = rgb;
                } else if angle == LocAngle::NORTH {
                    dst[offset as usize + 3] = rgb;
                } else if angle == LocAngle::EAST {
                    dst[offset as usize + 3 + 1536] = rgb;
                } else if angle == LocAngle::SOUTH {
                    dst[offset as usize + 1536] = rgb;
                }
            }

            if shape == LocShape::WALL_L {
                if angle == LocAngle::SOUTH {
                    dst[offset as usize] = rgb;
                    dst[offset as usize + 512] = rgb;
                    dst[offset as usize + 1024] = rgb;
                    dst[offset as usize + 1536] = rgb;
                } else if angle == LocAngle::WEST {
                    dst[offset as usize] = rgb;
                    dst[offset as usize + 1] = rgb;
                    dst[offset as usize + 2] = rgb;
                    dst[offset as usize + 3] = rgb;
                } else if angle == LocAngle::NORTH {
                    dst[offset as usize + 3] = rgb;
                    dst[offset as usize + 3 + 512] = rgb;
                    dst[offset as usize + 3 + 1024] = rgb;
                    dst[offset as usize + 3 + 1536] = rgb;
                } else if angle == LocAngle::EAST {
                    dst[offset as usize + 1536] = rgb;
                    dst[offset as usize + 1536 + 1] = rgb;
                    dst[offset as usize + 1536 + 2] = rgb;
                    dst[offset as usize + 1536 + 3] = rgb;
                }
            }
        }
    }

    let scene_type = world.scene_type(level, tile_x, tile_z);
    if scene_type != 0 {
        let info = world.type_code2(level, tile_x, tile_z, scene_type);
        let angle = (info >> 6) & 0x3;
        let shape = info & 0x1f;
        let loc_id = (scene_type >> 14) & 0x7fff;

        let loc = cache.loc(loc_id as usize);
        if loc.mapscene != -1 {
            if let Some(scene) = mapscene.get(loc.mapscene as usize).and_then(|s| s.as_ref()) {
                let offset_x = (loc.width * 4 - scene.wi) / 2;
                let offset_y = (loc.length * 4 - scene.hi) / 2;
                scene.plot_sprite(
                    surface,
                    tile_x * 4 + 48 + offset_x,
                    (BuildArea::SIZE - tile_z - loc.length) * 4 + offset_y + 48,
                );
            }
        } else {
            if shape == LocShape::WALL_DIAGONAL {
                let mut rgb = 0xeeeeee;
                if scene_type > 0 {
                    rgb = 0xee0000;
                }

                let dst = &mut surface.pixels;
                let offset = tile_x * 4 + (BuildArea::SIZE - 1 - tile_z) * 512 * 4 + 24624;

                if angle == LocAngle::WEST || angle == LocAngle::EAST {
                    dst[offset as usize + 1536] = rgb;
                    dst[offset as usize + 1024 + 1] = rgb;
                    dst[offset as usize + 512 + 2] = rgb;
                    dst[offset as usize + 3] = rgb;
                } else {
                    dst[offset as usize] = rgb;
                    dst[offset as usize + 512 + 1] = rgb;
                    dst[offset as usize + 1024 + 2] = rgb;
                    dst[offset as usize + 1536 + 3] = rgb;
                }
            }
        }
    }

    let gd_type = world.gd_type(level, tile_x, tile_z);
    if gd_type != 0 {
        let loc_id = (gd_type >> 14) & 0x7fff;

        let loc = cache.loc(loc_id as usize);
        if loc.mapscene != -1 {
            if let Some(scene) = mapscene.get(loc.mapscene as usize).and_then(|s| s.as_ref()) {
                let offset_x = (loc.width * 4 - scene.wi) / 2;
                let offset_y = (loc.length * 4 - scene.hi) / 2;
                scene.plot_sprite(
                    surface,
                    tile_x * 4 + 48 + offset_x,
                    (BuildArea::SIZE - tile_z - loc.length) * 4 + offset_y + 48,
                );
            }
        }
    }
}

impl Renderer {
    /// Java `REBUILD_NORMAL` / `checkMinimap` (Client.java / TS 6832-6835):
    /// bind `area_game` without cls so the last 3D frame stays frozen, plot
    /// "Loading - please wait." on top, blit (4, 4). Missing `p12` skips
    /// the string (the frozen pixels still blit). Gated on `draw`.
    /// `checkMinimap`'s draw-only loading splash (TS 5076): "Loading -
    /// please wait." into `area_game`, blitted at (4, 4) while the scene
    /// is not built. `mainredraw` runs it before the frame stages; the GPU
    /// backend re-runs it under the chrome recorder so the text is part of
    /// the quad frame.
    pub(crate) fn scene_loading_splash(&mut self, client: &mut Client) {
        if let Some(ag) = self.area_game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut ag.pixels, ag.width, ag.height);
            if let Some(p12) = self.media.p12.as_ref() {
                p12.centre_string(&mut surface, Some("Loading - please wait."), 257, 151, Colour::BLACK);
                p12.centre_string(&mut surface, Some("Loading - please wait."), 256, 150, Colour::WHITE);
            }
        }
        if client.draw {
            if let Some(ag) = &self.area_game {
                ag.blit_into(&mut self.draw_area, 4, 4);
            }
        }
    }

    /// Render half of TS `checkMinimap` (5076) — the draw-only parts, run
    /// from `mainredraw` each frame (task-2b fix round 1): while the scene
    /// is loading (`scene_state == 1`) the splash is redrawn, and once the
    /// scene is built the minimap *image* is composed when `minimap_level`
    /// (reset by `login`/`map_build`) lags `minusedlevel`. The SIM half of
    /// `checkMinimap` — the low-mem level transition and `check_scene` →
    /// `map_build` — lives on `Client` and runs from `game_loop`
    /// independent of `draw` (a headless client still builds the scene).
    fn check_minimap(&mut self, client: &mut Client) {
        if client.scene_state == 1 {
            self.scene_loading_splash(client);
        }

        if client.scene_state == 2 && client.minusedlevel != client.minimap_level {
            client.minimap_level = client.minusedlevel;
            self.minimap_build_buffer(client, client.minusedlevel);
        }
    }

    /// `minimapBuildBuffer(level)` from client-ts (5280-5387): compose the
    /// 512×512 minimap buffer from `mapl` (the ground pass through
    /// `render_2d_ground`, then the loc wall/scene lines and icons through
    /// `draw_detail`), scan the ground decors for the active map-function
    /// dots, and send the anticheat cycle counter.
    pub fn minimap_build_buffer(&mut self, client: &mut Client, level: i32) {
        let Some(mm) = self.minimap.as_mut() else {
            return;
        };

        for p in mm.data.iter_mut() {
            *p = 0;
        }

        for z in 1..BuildArea::SIZE - 1 {
            let mut offset = (BuildArea::SIZE - 1 - z) * 512 * 4 + 24628;

            for x in 1..BuildArea::SIZE - 1 {
                if client.mapl[level as usize][x as usize][z as usize] as i32
                    & (MapFlag::VIS_BELOW | MapFlag::FORCE_HIGH_DETAIL)
                    == 0
                {
                    self.world.render_2d_ground(&client.world, level, x, z, &mut mm.data, offset, 512);
                }

                if level < 3
                    && client.mapl[level as usize + 1][x as usize][z as usize] as i32
                        & MapFlag::VIS_BELOW
                        != 0
                {
                    self.world.render_2d_ground(&client.world, level + 1, x, z, &mut mm.data, offset, 512);
                }

                offset += 4;
            }
        }

        let inactive_rgb = ((((random_float() * 20.0) as i32) + 238 - 10) << 16)
            + ((((random_float() * 20.0) as i32) + 238 - 10) << 8)
            + ((random_float() * 20.0) as i32)
            + 238
            - 10;
        let active_rgb = (((random_float() * 20.0) as i32) + 238 - 10) << 16;

        // TS `this.minimap.setPixels()`: bind the buffer for `draw_detail`'s
        // plots. The `areaGame.setPixels()` rebinding is a no-op here — every
        // draw helper in this port takes its surface explicitly.
        let mut surface = Pix2D::with_pixels(&mut mm.data, 512, 512);
        for z in 1..BuildArea::SIZE - 1 {
            for x in 1..BuildArea::SIZE - 1 {
                if client.mapl[level as usize][x as usize][z as usize] as i32
                    & (MapFlag::VIS_BELOW | MapFlag::FORCE_HIGH_DETAIL)
                    == 0
                {
                    draw_detail(
                        &client.world,
                        &client.cache,
                        &self.media.mapscene,
                        &mut surface,
                        level,
                        x,
                        z,
                        inactive_rgb,
                        active_rgb,
                    );
                }

                if level < 3
                    && client.mapl[level as usize + 1][x as usize][z as usize] as i32
                        & MapFlag::VIS_BELOW
                        != 0
                {
                    draw_detail(
                        &client.world,
                        &client.cache,
                        &self.media.mapscene,
                        &mut surface,
                        level + 1,
                        x,
                        z,
                        inactive_rgb,
                        active_rgb,
                    );
                }
            }
        }
        drop(surface);

        self.active_map_function_count = 0;

        for x in 0..BuildArea::SIZE {
            for z in 0..BuildArea::SIZE {
                let typecode = client.world.gd_type(client.minusedlevel, x, z);
                if typecode == 0 {
                    continue;
                }

                let loc_id = (typecode >> 14) & 0x7fff;
                let func = client.cache.loc(loc_id as usize).mapfunction;
                if func < 0 {
                    continue;
                }

                let mut stx = x;
                let mut stz = z;

                if func != 22
                    && func != 29
                    && func != 34
                    && func != 36
                    && func != 46
                    && func != 47
                    && func != 48
                {
                    let max_x = BuildArea::SIZE;
                    let max_z = BuildArea::SIZE;
                    let flags = &client.collision[client.minusedlevel as usize].flags;

                    for _ in 0..10 {
                        let rand = (random_float() * 4.0) as i32;
                        if rand == 0
                            && stx > 0
                            && stx > x - 3
                            && (flags[(stx - 1) as usize][stz as usize] & CollisionFlag::PL_WALK_E)
                                == CollisionFlag::_OPEN
                        {
                            stx -= 1;
                        }

                        if rand == 1
                            && stx < max_x - 1
                            && stx < x + 3
                            && (flags[(stx + 1) as usize][stz as usize] & CollisionFlag::PL_WALK_W)
                                == CollisionFlag::_OPEN
                        {
                            stx += 1;
                        }

                        if rand == 2
                            && stz > 0
                            && stz > z - 3
                            && (flags[stx as usize][(stz - 1) as usize] & CollisionFlag::PL_WALK_N)
                                == CollisionFlag::_OPEN
                        {
                            stz -= 1;
                        }

                        if rand == 3
                            && stz < max_z - 1
                            && stz < z + 3
                            && (flags[stx as usize][(stz + 1) as usize] & CollisionFlag::PL_WALK_S)
                                == CollisionFlag::_OPEN
                        {
                            stz += 1;
                        }
                    }
                }

                // TS writes past the 1000-slot `activeMapFunctions` arrays
                // silently; a Rust panic here is worse, so cap the count.
                let count = self.active_map_function_count as usize;
                if count < self.active_map_functions.len() {
                    self.active_map_functions[count] =
                        self.media.mapfunction.get(func as usize).and_then(|s| s.clone());
                    self.active_map_function_x[count] = stx;
                    self.active_map_function_z[count] = stz;
                    self.active_map_function_count += 1;
                }
            }
        }

        self.cyclelogic3 += 1;
        if self.cyclelogic3 > 112 {
            self.cyclelogic3 = 0;

            client.out.p1_enc(ClientProt::ANTICHEAT_CYCLELOGIC3.id);
            client.out.p1(50);
        }
    }

    /// `mainredraw` from Java — the frame render pass: title screen or
    /// in-game draw into `draw_area`. `draw` is the CPU-save switch: false
    /// skips the render entirely, so a headless bot burns no pixels while
    /// the network machine keeps running. Returns the composited frame (the
    /// backend-owned output, task-4 fix round 1), so the present target can
    /// consume it without re-reading `draw_area`.
    ///
    /// Re-homed onto `Renderer` (task 2b): it also runs the draw-only halves
    /// the sim loop used to touch — `check_minimap`'s render half (the
    /// loading splash and the minimap *image* build; `Client::check_minimap`
    /// runs the scene build on the sim loop) and `follow_camera`.
    pub fn mainredraw(&mut self, client: &mut Client) -> FrameOutput {
        // A live lowmem/highmem flip updates `Client.config.lowmem`; re-init
        // the renderer's texture state — the raster flag, the unpacked
        // textures (lowmem halves 128×128, highmem trims) and the texel pool
        // (whose row length follows the mode) — so both the CPU texel raster
        // and the GPU ground/water branch see the new mode. The backend and
        // `draw_area` stay alive; rebuilding the whole renderer hangs the
        // slot mid-flight.
        if self.pix3d.low_mem != client.config.lowmem {
            self.pix3d.low_mem = client.config.lowmem;
            if let Ok(bytes) = std::fs::read(format!("{}/textures", client.config.cache_dir)) {
                let jag = JagFile::new(bytes);
                self.pix3d.unpack_textures(&jag);
            }
            self.pix3d.reset_pool(20);
            self.pix3d.init_texture_palettes(0.8);
            self.pix3d.refresh_texture_averages();
            client.tex_average = self.pix3d.tex_average;
        }
        if !client.draw {
            return FrameOutput::PixMap(self.draw_area.clone());
        }
        if client.ingame {
            self.check_minimap(client);
            if client.scene_state == 2 {
                self.follow_camera(client);
            }
            self.game_draw(client)
        } else {
            self.title_screen_draw(client)
        }
    }

    /// `animateInterface` from client-ts (10552): advance the animation of
    /// `id`'s children by `delta`, recursing `TYPE_LAYER` children (the
    /// layer recursion is 0, not TS `type === 1`, matching
    /// `if_anim_reset`). A `TYPE_MODEL` child with a model anim selects the
    /// active/inactive seq (`get_if_active` picks `model_anim2` else
    /// `model_anim`), adds `delta` to `anim_cycle`, and steps `anim_frame`
    /// while the cycle exceeds the frame delay, wrapping with `loops` (TS
    /// 10571-10589). Missing children or a missing seq skip. Returns
    /// whether any child frame advanced. Re-homed onto `Renderer` (task 2b)
    /// so it can select the seq via `get_if_active`.
    pub fn animate_interface(&mut self, client: &mut Client, id: i32, delta: i32) -> bool {
        let Some(children) = client
            .if_(id as usize)
            .and_then(|com| com.children.clone())
        else {
            return false;
        };

        let mut updated = false;

        for child_id in children {
            if child_id == -1 {
                break;
            }
            // Copy the fields the recursion/write-back needs, so the view
            // borrow ends before the recursive `&mut client` calls (the
            // old code cloned the whole component for the same reason).
            let Some(child) = client.if_(child_id as usize) else {
                break;
            };
            let is_layer = child.r#type == ComponentType::TYPE_LAYER;
            let is_model = child.r#type == ComponentType::TYPE_MODEL;
            let model_anim = child.model_anim;
            let model_anim2 = child.model_anim2;
            let mut anim_cycle = child.anim_cycle + delta;
            let mut anim_frame = child.anim_frame;
            let active = if is_model && (model_anim != -1 || model_anim2 != -1) {
                self.get_if_active(client, child_id)
            } else {
                false
            };
            if is_layer {
                updated |= self.animate_interface(client, child_id, delta);
            }

            if is_model && (model_anim != -1 || model_anim2 != -1) {
                let seq_id = if active { model_anim2 } else { model_anim };
                if seq_id != -1 && (seq_id as usize) < client.cache.seqs.len() {
                    let seq = &client.cache.seqs[seq_id as usize];
                    // TS 10569: `animCycle += delta` accumulates even when
                    // no frame advances; the cycle/frame write-back below is
                    // the in-place mutation of the TS `child`.
                    let mut advanced = false;
                    while anim_cycle > seq.get_delay(anim_frame) {
                        anim_cycle -= seq.get_delay(anim_frame) + 1;
                        anim_frame += 1;
                        if anim_frame >= seq.num_frames {
                            anim_frame -= seq.loops;
                            if anim_frame < 0 || anim_frame >= seq.num_frames {
                                anim_frame = 0;
                            }
                        }
                        advanced = true;
                    }
                    if let Some(com) = client.ifaces_mut
                        .get_mut(child_id as usize)
                        .and_then(|o| o.as_mut())
                    {
                        com.anim_cycle = anim_cycle;
                        com.anim_frame = anim_frame;
                    }
                    updated |= advanced;
                }
            }
        }

        updated
    }
}
