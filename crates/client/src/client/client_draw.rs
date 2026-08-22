//! Frame draw (Tasks 4/5: title, then in-game). 1:1 port of `Client.ts`
//! `prepareTitle`, `loadTitleBackground`, `loadTitleImages`, `TitleFlames`,
//! `titleScreenDraw` (1489–1694), `gameDraw` with its `prepareGame`/
//! `drawSide`/`drawChat` helpers (3890–4170, 2001, 11098, 11125), and the
//! `gameDrawMain` 3D pass (4172–4251): `addPlayers`/`addNpcs`, the orbit
//! camera (`camFollow`), `World.resetVisCalc` + `render_all` into
//! `area_game`, and the (4, 4) blit. `minimapDraw` (11279) rotates the
//! composed minimap buffer and the compass into `area_map` (blitted at
//! (550, 4) under the chrome; the `area_backvmid1` strip is re-blitted on
//! top as a z-order guard). Draws always into `Client::draw_area`
//! (765×503 `PixMap`); present (feature `window`) only blits.
//!
//! The minimap `mapback` ring and mask build (1180–1216) land in
//! `prepare_game`; `drawInterface` draws the side-tab interfaces
//! (`TYPE_LAYER`/`TYPE_RECT`/`TYPE_TEXT`/`TYPE_GRAPHIC`).

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::client::client::{level_experience, Client};
use crate::client::client_build::random_float;
use crate::client::skill::Skill;
use crate::client::title_flames::TitleFlames;
use crate::config::if_type::{ButtonType, ComponentType, IfType};
use crate::config::{Cache, ObjType};
use crate::dash3d::world::LevelHeightmaps;
use crate::dash3d::{BuildArea, LocAngle, LocShape, MapFlag, SceneModel, World};
use crate::graphics::{Colour, Pix2D, Pix3D, Pix32, Pix8, PixFont, PixMap};
use crate::io::{ClientProt, JagFile};
use crate::util::JString;

fn try_title_jag(cache_dir: &str) -> Option<JagFile> {
    let bytes = std::fs::read(format!("{cache_dir}/title")).ok()?;
    catch_unwind(AssertUnwindSafe(|| JagFile::new(bytes))).ok()
}

fn plot_title_bg(map: &mut Option<PixMap>, background: &Pix32, x: i32, y: i32) {
    if let Some(map) = map {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        background.quick_plot_sprite(&mut surface, x, y);
    }
}

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

impl Client {
    /// `titleScreenDraw` from client-ts (1489): draw the login UI into
    /// `image_title4` (360×200), then composite the title regions into
    /// `draw_area`; the 2/3/5/6/7/8 regions redraw only while `redraw_frame`
    /// is set.
    pub fn title_screen_draw(&mut self) {
        self.prepare_title();

        let w = 360;
        let h = 200;
        if let Some(map4) = self.image_title4.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut map4.pixels, w, h);

            if let Some(titlebox) = &self.image_titlebox {
                titlebox.plot_sprite(&mut surface, 0, 0);
            }

            if self.loginscreen == 0 {
                let extra_y = (h / 2) + 80;
                let mut y = (h / 2) - 20;

                if self.on_demand.is_some() {
                    let message = self.on_demand.as_ref().unwrap().message.clone();
                    if let Some(p11) = self.p11.as_mut() {
                        p11.centre_string_tag(&mut surface, &message, w / 2, extra_y, 0x75a9a9, true);
                    }
                }

                if let Some(b12) = self.b12.as_mut() {
                    b12.centre_string_tag(&mut surface, "Welcome to RuneScape", w / 2, y, Colour::YELLOW, true);
                }

                let mut x = (w / 2) - 80;
                y = (h / 2) + 20;
                if let Some(button) = &self.image_titlebutton {
                    button.plot_sprite(&mut surface, x - 73, y - 20);
                }
                if let Some(b12) = self.b12.as_mut() {
                    b12.centre_string_tag(&mut surface, "New User", x, y + 5, Colour::WHITE, true);
                }

                x = (w / 2) + 80;
                if let Some(button) = &self.image_titlebutton {
                    button.plot_sprite(&mut surface, x - 73, y - 20);
                }
                if let Some(b12) = self.b12.as_mut() {
                    b12.centre_string_tag(&mut surface, "Existing User", x, y + 5, Colour::WHITE, true);
                }
            } else if self.loginscreen == 2 {
                let mut y = (h / 2) - 40;
                if let Some(b12) = self.b12.as_mut() {
                    if self.login_mes1.is_empty() {
                        b12.centre_string_tag(&mut surface, &self.login_mes2, w / 2, y - 7, Colour::YELLOW, true);
                    } else {
                        b12.centre_string_tag(&mut surface, &self.login_mes1, w / 2, y - 15, Colour::YELLOW, true);
                        b12.centre_string_tag(&mut surface, &self.login_mes2, w / 2, y, Colour::YELLOW, true);
                    }
                    y += 30;

                    let user_line = format!(
                        "Username: {}{}",
                        self.login_user,
                        if self.login_select == 0 && self.loop_cycle % 40 < 20 { "@yel@|" } else { "" }
                    );
                    b12.draw_string_tag(&mut surface, &user_line, w / 2 - 90, y, Colour::WHITE, true);
                    y += 15;

                    let pass_line = format!(
                        "Password: {}{}",
                        JString::get_repeated_character(&self.login_pass),
                        if self.login_select == 1 && self.loop_cycle % 40 < 20 { "@yel@|" } else { "" }
                    );
                    b12.draw_string_tag(&mut surface, &pass_line, w / 2 - 88, y, Colour::WHITE, true);
                }

                let x = (w / 2) - 80;
                let y = (h / 2) + 50;
                if let Some(button) = &self.image_titlebutton {
                    button.plot_sprite(&mut surface, x - 73, y - 20);
                }
                if let Some(b12) = self.b12.as_mut() {
                    b12.centre_string_tag(&mut surface, "Login", x, y + 5, Colour::WHITE, true);
                }

                let x = (w / 2) + 80;
                if let Some(button) = &self.image_titlebutton {
                    button.plot_sprite(&mut surface, x - 73, y - 20);
                }
                if let Some(b12) = self.b12.as_mut() {
                    b12.centre_string_tag(&mut surface, "Cancel", x, y + 5, Colour::WHITE, true);
                }
            } else if self.loginscreen == 3 {
                let x = w / 2;
                let mut y = (h / 2) - 60;
                if let Some(b12) = self.b12.as_mut() {
                    b12.centre_string_tag(&mut surface, "Create a free account", x, y, Colour::YELLOW, true);

                    y = (h / 2) - 35;
                    b12.centre_string_tag(&mut surface, "To create a new account you need to", x, y, Colour::WHITE, true);
                    y += 15;
                    b12.centre_string_tag(&mut surface, "go back to the main RuneScape webpage", x, y, Colour::WHITE, true);
                    y += 15;
                    b12.centre_string_tag(&mut surface, "and choose the red 'create account'", x, y, Colour::WHITE, true);
                    y += 15;
                    b12.centre_string_tag(&mut surface, "button at the top right of that page.", x, y, Colour::WHITE, true);
                }

                let x = w / 2;
                let y = (h / 2) + 50;
                if let Some(button) = &self.image_titlebutton {
                    button.plot_sprite(&mut surface, x - 73, y - 20);
                }
                if let Some(b12) = self.b12.as_mut() {
                    b12.centre_string_tag(&mut surface, "Cancel", x, y + 5, Colour::WHITE, true);
                }
            }
        }

        if let Some(t4) = &self.image_title4 {
            t4.blit_into(&mut self.draw_area, 202, 171);
        }

        if self.redraw_frame {
            self.redraw_frame = false;
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

        // TitleFlames.ts drawFlames: the torch columns redraw every frame
        // (TS PixMap.draw onto the canvas). Inactive flames still blit the
        // JPEG background that loadTitleBackground plotted into 0/1.
        self.tick_title_flames();
        if let Some(t0) = &self.image_title0 {
            t0.blit_into(&mut self.draw_area, 0, 0);
        }
        if let Some(t1) = &self.image_title1 {
            t1.blit_into(&mut self.draw_area, 637, 0);
        }
    }

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
    pub fn draw_progress(&mut self, message: &str, progress: i32) {
        self.last_progress_percent = progress;
        self.last_progress_message = message.to_string();

        if !self.draw {
            return;
        }

        // Java `Client.messageBox`: prepareTitle() then the title-framed bar
        // with b12. Without this, maininit stays on the GameShell fallback
        // (no fonts) after the title jag is already on disk.
        self.prepare_title();

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
            if let Some(b12) = self.b12.as_ref() {
                b12.centre_string(&mut surface, Some(message), mid_x, y + 22, Colour::WHITE);
            }
            self.present_progress();
            return;
        }

        // TS 3840-3866: the loading bar on image_title4.
        let w = 360;
        let h = 200;
        let offset_y = 20;
        if let Some(map4) = self.image_title4.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut map4.pixels, w, h);
            if let Some(b12) = self.b12.as_ref() {
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
            if let Some(b12) = self.b12.as_ref() {
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

        if self.redraw_frame {
            self.redraw_frame = false;
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

        self.present_progress();
    }

    /// Java `messageBox` presents immediately. `run` only blits after
    /// maininit, so the loading bar has to blit here or the title looks
    /// like loading already finished.
    fn present_progress(&mut self) {
        #[cfg(feature = "window")]
        if let Some(present) = self.present.as_mut() {
            let _ = present.poll(&mut self.shell);
        }
        #[cfg(feature = "window")]
        if let Some(present) = self.present.as_mut() {
            present.blit(
                &self.draw_area.pixels,
                self.draw_area.width as u32,
                self.draw_area.height as u32,
            );
        }
    }

    fn tick_title_flames(&mut self) {
        let Some(flames) = self.title_flames.as_mut() else {
            return;
        };
        if !flames.active {
            return;
        }
        let (Some(left), Some(right)) = (self.image_title0.as_mut(), self.image_title1.as_mut()) else {
            return;
        };
        flames.render_flames(left, right, self.loop_cycle);
    }

    /// `prepareTitle` from client-ts (1579): create the 9 title `PixMap`
    /// regions (sizes as TS) on the first frame, load the `title` jag from
    /// the cache, the four fonts, and the titlebox/titlebutton sprites.
    /// `logout` nulls `image_title2` (the gate below), so the next title
    /// draw reallocates the regions like Java `prepareTitle` after
    /// `prepareGame` dropped them.
    fn prepare_title(&mut self) {
        if self.image_title2.is_some() {
            return;
        }

        // Java `prepareTitle` (Client.java 1481-1488) nulls the game-frame
        // areas before allocating the title regions, so a second login
        // re-runs `prepareGame` instead of early-returning on a surviving
        // `areaChatback`. `draw_area` stays: Rust keeps one compositor
        // PixMap and `logout` cls it.
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

        if self.title.is_none() {
            self.title = try_title_jag(&self.config.cache_dir);
        }

        // `take` so the jag outlives the `&mut self` loads (TS loads it in
        // `maininit` and passes it around freely).
        self.load_fonts();
        if let Some(jag) = self.title.take() {
            self.load_title_background(&jag);
            self.load_title_images(&jag);
            self.title = Some(jag);
        }

        self.redraw_frame = true;
    }

    /// Fonts from the `title` jag, loaded once (TS `maininit` 848 loads the
    /// four fonts before both title and game draw). Loads the jag from the
    /// cache when `prepare_title` has not run yet, so an in-game client that
    /// never drew the title still has `p12` for the chat-mode labels.
    fn load_fonts(&mut self) {
        if self.p11.is_some() {
            return;
        }
        if self.title.is_none() {
            self.title = try_title_jag(&self.config.cache_dir);
        }
        if let Some(jag) = self.title.take() {
            self.p11 = PixFont::depack(&jag, "p11_full", false).ok();
            self.p12 = PixFont::depack(&jag, "p12_full", false).ok();
            self.b12 = PixFont::depack(&jag, "b12_full", false).ok();
            self.q8 = PixFont::depack(&jag, "q8_full", true).ok();
            self.title = Some(jag);
        }
    }

    /// `loadTitleBackground` from client-ts (1627): JPEG `title.dat` tiled
    /// across the 9 title regions, mirrored, then the `logo` sprite.
    fn load_title_background(&mut self, jag: &JagFile) {
        if let Ok(mut background) = Pix32::from_jpeg(jag, "title.dat") {
            plot_title_bg(&mut self.image_title0, &background, 0, 0);
            plot_title_bg(&mut self.image_title1, &background, -637, 0);
            plot_title_bg(&mut self.image_title2, &background, -128, 0);
            plot_title_bg(&mut self.image_title3, &background, -202, -371);
            plot_title_bg(&mut self.image_title4, &background, -202, -171);
            plot_title_bg(&mut self.image_title5, &background, 0, -265);
            plot_title_bg(&mut self.image_title6, &background, -562, -265);
            plot_title_bg(&mut self.image_title7, &background, -128, -171);
            plot_title_bg(&mut self.image_title8, &background, -562, -171);

            background.hflip();

            plot_title_bg(&mut self.image_title0, &background, 382, 0);
            plot_title_bg(&mut self.image_title1, &background, -255, 0);
            plot_title_bg(&mut self.image_title2, &background, 254, 0);
            plot_title_bg(&mut self.image_title3, &background, 180, -371);
            plot_title_bg(&mut self.image_title4, &background, 180, -171);
            plot_title_bg(&mut self.image_title5, &background, 382, -265);
            plot_title_bg(&mut self.image_title6, &background, -180, -265);
            plot_title_bg(&mut self.image_title7, &background, 254, -171);
            plot_title_bg(&mut self.image_title8, &background, -180, -171);
        }

        if let Ok(logo) = Pix32::depack(jag, "logo", 0) {
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
    /// `titlebutton` sprites plus the 12 `runes` sprites (`fl_icon` param
    /// default 0 → sprites 0..11).
    fn load_title_images(&mut self, jag: &JagFile) {
        self.image_titlebox = Pix8::depack(jag, "titlebox", 0).ok();
        self.image_titlebutton = Pix8::depack(jag, "titlebutton", 0).ok();

        // TS: `flameIcon = this.getIntParam('fl_icon')` — no param plumbing,
        // the fallback 0 means sprites 0..11.
        self.image_runes.clear();
        for i in 0..12 {
            if let Ok(rune) = Pix8::depack(jag, "runes", i) {
                self.image_runes.push(rune);
            }
        }
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

    /// `gameDraw` from client-ts (3890): the in-game frame. `gameDrawMain`
    /// (4172, the 3D pass) renders the world into `area_game` when
    /// `scene_state` is 2, and `minimapDraw` (11279) fills `area_map`
    /// before its (550, 4) blit. The chrome strips, side, chat,
    /// icon-strip backgrounds, and chat-mode panels draw 1:1.
    pub fn game_draw(&mut self) {
        // TS GameShell ticks `scrollCycle` each mainloop pass while the
        // mouse is held (Client.ts 2341-2343); 0/1 here is enough for the
        // held-arrow scrollbar repeat.
        self.scroll_cycle = if self.shell.mouse_button != 0 { 1 } else { 0 };
        // TS `buildMinimenu` (hover walk + menu options) runs from
        // `other_overlays` when no menu is open (Client.ts 4865-4867),
        // ahead of this frame's side/chat draws.
        self.prepare_game();

        if self.redraw_frame {
            self.redraw_frame = false;

            if let Some(b) = &self.area_backleft1 {
                b.blit_into(&mut self.draw_area, 0, 4);
            }
            if let Some(b) = &self.area_backleft2 {
                b.blit_into(&mut self.draw_area, 0, 357);
            }
            if let Some(b) = &self.area_backright1 {
                b.blit_into(&mut self.draw_area, 722, 4);
            }
            if let Some(b) = &self.area_backright2 {
                b.blit_into(&mut self.draw_area, 743, 205);
            }
            if let Some(b) = &self.area_backtop1 {
                b.blit_into(&mut self.draw_area, 0, 0);
            }
            if let Some(b) = &self.area_backvmid1 {
                b.blit_into(&mut self.draw_area, 516, 4);
            }
            if let Some(b) = &self.area_backvmid2 {
                b.blit_into(&mut self.draw_area, 516, 205);
            }
            if let Some(b) = &self.area_backvmid3 {
                b.blit_into(&mut self.draw_area, 496, 357);
            }
            if let Some(b) = &self.area_backhmid2 {
                b.blit_into(&mut self.draw_area, 0, 338);
            }

            self.redraw_icons = true;
            self.redraw_side = true;
            self.redraw_chat = true;
            self.redraw_chat_mode = true;

            if self.scene_state != 2 {
                if let Some(g) = &self.area_game {
                    g.blit_into(&mut self.draw_area, 4, 4);
                }
                if let Some(m) = &self.area_map {
                    m.blit_into(&mut self.draw_area, 550, 4);
                }
                // Map under chrome: `area_backvmid1` (34×156 at (516, 4))
                // borders the 172×156 rect. In the 274 layout the strips do
                // not overlap it, so this re-blit is a no-op guard that
                // keeps an opaque `area_map` from covering the chrome.
                if let Some(b) = &self.area_backvmid1 {
                    b.blit_into(&mut self.draw_area, 516, 4);
                }
            }
        }

        if self.scene_state == 2 {
            self.game_draw_main();
        }

        // TS 3924-3926: an open side minimenu redraws the side panel every
        // frame so the hover-highlighted option rows track the pointer.
        if self.is_menu_open && self.menu_area == 1 {
            self.redraw_side = true;
        }

        // `sideModalId`/`animateInterface` redrawSide trigger (TS 3931-3937).
        if self.side_modal_id != -1
            && self.animate_interface(self.side_modal_id, self.world_update_num)
        {
            self.redraw_side = true;
        }

        // TS 3935-3941: the OP_HELD outline and the in-flight obj drag
        // redraw the side panel every frame.
        if self.selected_area == 2 {
            self.redraw_side = true;
        }
        if self.obj_drag_area == 2 {
            self.redraw_side = true;
        }

        if self.redraw_side {
            self.draw_side();
            self.redraw_side = false;
        }

        // `chatModalId`/`animateInterface` redrawChat trigger (TS 3966-3971).

        // TS 3948-3967: with no chat modal the chat scrollbar is live. The
        // held-arrow step goes through `chat_interface` (a synthetic IfType,
        // `com_id` -1), then `chat_scroll_pos` is re-derived from it.
        if self.chat_modal_id == -1 {
            self.chat_interface.scroll_pos = self.chat_scroll_height - self.chat_scroll_pos - 77;
            self.chat_interface.scroll_height = self.chat_scroll_height;
            if self.shell.mouse_x > 448 && self.shell.mouse_x < 560 && self.shell.mouse_y > 332 {
                self.do_scrollbar(
                    self.shell.mouse_x - 17,
                    self.shell.mouse_y - 357,
                    self.chat_scroll_height,
                    77,
                    false,
                    463,
                    0,
                    -1,
                );
            }
            let mut offset = self.chat_scroll_height - self.chat_interface.scroll_pos - 77;
            if offset < 0 {
                offset = 0;
            }
            if offset > self.chat_scroll_height - 77 {
                offset = self.chat_scroll_height - 77;
            }
            if self.chat_scroll_pos != offset {
                self.chat_scroll_pos = offset;
                self.redraw_chat = true;
            }
        }

        if self.chat_modal_id != -1
            && self.animate_interface(self.chat_modal_id, self.world_update_num)
        {
            self.redraw_chat = true;
        }

        // TS 3977-3982: the OP_HELD outline and the in-flight obj drag
        // redraw the chat panel every frame.
        if self.selected_area == 3 {
            self.redraw_chat = true;
        }
        if self.obj_drag_area == 3 {
            self.redraw_chat = true;
        }

        // TS 3989-3991: an open chat minimenu redraws the chat panel every
        // frame so the hover-highlighted option rows track the pointer.
        if self.is_menu_open && self.menu_area == 2 {
            self.redraw_chat = true;
        }

        if self.redraw_chat {
            self.draw_chat();
            self.redraw_chat = false;
        }

        // `minimapDraw` (11279) into `area_map`, then the (550, 4) blit
        // (TS 3999-4001), then the chrome re-blit guard (see the
        // `redraw_frame` path above).
        if self.scene_state == 2 {
            self.minimap_draw();
            if let Some(m) = &self.area_map {
                m.blit_into(&mut self.draw_area, 550, 4);
            }
            if let Some(b) = &self.area_backvmid1 {
                b.blit_into(&mut self.draw_area, 516, 4);
            }
        }

        // `tutFlashIcon !== -1` redrawIcons trigger (TS 4003-4004).
        if self.tut_flash_icon != -1 {
            self.redraw_icons = true;
        }

        if self.redraw_icons {
            self.draw_icons();
            self.redraw_icons = false;
        }

        if self.redraw_chat_mode {
            self.redraw_chat_mode = false;
            // TS (4122): the chat mode buttons on `backbase1`, blitted at
            // (0, 453).
            if let Some(base) = self.area_backbase1.as_mut() {
                let w = base.width;
                let h = base.height;
                let mut surface = Pix2D::with_pixels(&mut base.pixels, w, h);
                if let Some(backbase1) = &self.backbase1 {
                    backbase1.plot_sprite(&mut surface, 0, 0);
                }
                if let Some(p12) = self.p12.as_mut() {
                    p12.centre_string_tag(&mut surface, "Public chat", 55, 28, Colour::WHITE, true);
                    let (label, rgb) = match self.chat_public_mode {
                        1 => ("Friends", Colour::YELLOW),
                        2 => ("Off", Colour::RED),
                        3 => ("Hide", Colour::CYAN),
                        _ => ("On", Colour::GREEN),
                    };
                    p12.centre_string_tag(&mut surface, label, 55, 41, rgb, true);
                    p12.centre_string_tag(&mut surface, "Private chat", 184, 28, Colour::WHITE, true);
                    let (label, rgb) = match self.chat_private_mode {
                        1 => ("Friends", Colour::YELLOW),
                        2 => ("Off", Colour::RED),
                        _ => ("On", Colour::GREEN),
                    };
                    p12.centre_string_tag(&mut surface, label, 184, 41, rgb, true);
                    p12.centre_string_tag(&mut surface, "Trade/duel", 324, 28, Colour::WHITE, true);
                    let (label, rgb) = match self.chat_trade_mode {
                        1 => ("Friends", Colour::YELLOW),
                        2 => ("Off", Colour::RED),
                        _ => ("On", Colour::GREEN),
                    };
                    p12.centre_string_tag(&mut surface, label, 324, 41, rgb, true);
                    p12.centre_string_tag(&mut surface, "Report abuse", 458, 33, Colour::WHITE, true);
                }
            }
            if let Some(base) = &self.area_backbase1 {
                base.blit_into(&mut self.draw_area, 0, 453);
            }
        }

        // TS 4169: `worldUpdateNum = 0` at the end of the drawn frame.
        self.world_update_num = 0;
    }

    /// `gameDrawMain` from client-ts (4172): the 3D pass. Adds the players,
    /// NPCs and projectiles as dynamic sprites, follows the orbit camera
    /// (or the cutscene camera while `cinema_cam`), applies the per-frame
    /// `camShake` jitter, renders the world into `area_game`
    /// (`Pix2D.cls()` + `render_all` + `removeSprites`, the TS 4238-4245
    /// sequence) and blits it at (4, 4). `World.resetVisCalc` runs once on
    /// the first pass (TS runs it from the game-loading flow) so
    /// `render_all`'s visibility backing is populated. The overlay passes
    /// are no-ops while their lists/sprites are not ported; the minimenu
    /// and the fps pass are not ported either. `otherOverlays` (the main
    /// overlay and modal, TS 4250) draws into `area_game` before the blit.
    fn game_draw_main(&mut self) {
        self.scene_cycle += 1;

        self.add_players(true);
        self.add_npcs(true);
        self.add_players(false);
        self.add_npcs(false);
        self.add_projectiles();
        self.add_map_anim();

        // Camera (TS 4183-4195): a cutscene camera skips the orbit follow;
        // otherwise the orbit camera follows the local player.
        let mut pitch = self.orbit_camera_pitch;
        if self.camera_pitch_clamp / 256 > pitch {
            pitch = self.camera_pitch_clamp / 256;
        }
        if self.cam_shake[4] && self.cam_shake_ran[4] + 128 > pitch {
            pitch = self.cam_shake_ran[4] + 128;
        }
        let yaw = (self.orbit_camera_yaw + self.macro_camera_angle) & 0x7ff;

        if !self.cinema_cam {
            if let Some(player) = &self.local_player {
                let target_y = get_av_h(&self.groundh, &self.mapl, player.x, player.z, self.minusedlevel) - 50;
                self.cam_follow(
                    pitch,
                    yaw,
                    self.orbit_camera_x,
                    target_y,
                    self.orbit_camera_z,
                    pitch * 3 + 600,
                );
            }
        }

        // TS 4197-4203: the cutscene camera uses roofCheck2's eye height.
        let level = if self.cinema_cam {
            self.roof_check2()
        } else {
            self.roof_check()
        };

        // TS 4205-4209: snapshot the pre-jitter eye so it can be restored
        // after the pass (the camShake jitter is per-frame only).
        let eye_x = self.cam_x;
        let eye_y = self.cam_y;
        let eye_z = self.cam_z;
        let eye_pitch = self.cam_pitch;
        let eye_yaw = self.cam_yaw;

        // TS 4211-4235: the camShake jitter applies to the rendered eye.
        let (cam_x, cam_y, cam_z, cam_pitch, cam_yaw) =
            self.cam_shake_jitter(eye_x, eye_y, eye_z, eye_pitch, eye_yaw);

        // `World.resetVisCalc` (Client.ts loadGame 1222-1235): once per
        // game, so `vis_backing` is populated before `render_all` binds its
        // pitch/yaw row.
        if !self.vis_calc_done {
            self.vis_calc_done = true;
            let mut distance = [0i32; 9];
            for (x, slot) in distance.iter_mut().enumerate() {
                let angle = x as i32 * 32 + 128 + 15;
                let offset = angle * 3 + 600;
                let sin = Pix3D::sin_table().get(angle as usize).copied().unwrap_or(0);
                *slot = (offset * sin) >> 16;
            }
            self.world.reset_vis_calc(&distance, 500, 800, 512, 334);
        }

        // TS 4238-4242: the model picking state for this frame.
        let cycle = self.pix3d.cycle;
        self.pix3d.mouse_check = true;
        self.pix3d.picked_count = 0;
        self.pix3d.mouse_x = self.shell.mouse_x - 4;
        self.pix3d.mouse_y = self.shell.mouse_y - 4;

        // `Pix2D.cls()` on area_game, `Pix3D.setClipping(512, 334)`, then
        // the world pass (TS 4238-4245).
        let cache = &self.cache;
        let loop_cycle = self.loop_cycle;
        let (pix3d, world) = (&mut self.pix3d, &mut self.world);
        if let Some(game) = self.area_game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            surface.cls();
            pix3d.set_clipping(game.width, game.height);
            world.render_all(
                pix3d, &mut surface, cache, loop_cycle, cam_x, cam_y, cam_z, level, cam_yaw,
                cam_pitch,
            );
        }
        world.remove_sprites();

        self.entity_overlays();
        self.coord_arrow();
        self.texture_run_anims(cycle);
        self.other_overlays();

        // TS 4252-4257: restore the pre-jitter eye.
        self.cam_x = eye_x;
        self.cam_y = eye_y;
        self.cam_z = eye_z;
        self.cam_pitch = eye_pitch;
        self.cam_yaw = eye_yaw;

        if let Some(game) = &self.area_game {
            game.blit_into(&mut self.draw_area, 4, 4);
        }
    }

    /// `drawPrivateMessages` from Client.ts (4915-4986): the split
    /// private-chat overlay, drawn into `area_game` when clientcode 8 set
    /// `split_private_chat`. Incoming (3/7) and sent (5/6) lines stack
    /// bottom-up from y 329 with the double-shadowed cyan text; the
    /// `modIcons` sprite advance is kept without the sprite (like
    /// `draw_chat`). The `rebootTimer` line offset is 0.
    fn draw_private_messages(&mut self, surface: &mut Pix2D) {
        if self.split_private_chat == 0 {
            return;
        }

        let mut line_offset = 0;
        for i in 0..100 {
            if self.chat_text[i].is_empty() {
                continue;
            }
            let r#type = self.chat_type[i];
            let mut sender = self.chat_username[i].clone();
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
                    || self.chat_private_mode == 0
                    || (self.chat_private_mode == 1 && self.is_friend(&sender)))
            {
                let y = 329 - line_offset * 13;
                let mut x = 4;
                if let Some(font) = self.p12.as_ref() {
                    font.draw_string(surface, Some("From"), 4, y, Colour::BLACK);
                    font.draw_string(surface, Some("From"), 4, y - 1, Colour::CYAN);
                    x += font.string_wid(Some("From "));
                }
                if modlevel == 1 || modlevel == 2 {
                    x += 14;
                }
                if let Some(font) = self.p12.as_ref() {
                    font.draw_string(surface, Some(&format!("{sender}: {}", self.chat_text[i])), x, y, Colour::BLACK);
                    font.draw_string(surface, Some(&format!("{sender}: {}", self.chat_text[i])), x, y - 1, Colour::CYAN);
                }
                line_offset += 1;
                if line_offset >= 5 {
                    return;
                }
            } else if r#type == 5 && self.chat_private_mode < 2 {
                let y = 329 - line_offset * 13;
                if let Some(font) = self.p12.as_ref() {
                    font.draw_string(surface, Some(&self.chat_text[i]), 4, y, Colour::BLACK);
                    font.draw_string(surface, Some(&self.chat_text[i]), 4, y - 1, Colour::CYAN);
                }
                line_offset += 1;
                if line_offset >= 5 {
                    return;
                }
            } else if r#type == 6 && self.chat_private_mode < 2 {
                let y = 329 - line_offset * 13;
                if let Some(font) = self.p12.as_ref() {
                    font.draw_string(surface, Some(&format!("To {sender}: {}", self.chat_text[i])), 4, y, Colour::BLACK);
                    font.draw_string(surface, Some(&format!("To {sender}: {}", self.chat_text[i])), 4, y - 1, Colour::CYAN);
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
    fn other_overlays(&mut self) {
        let mut game = self.area_game.take();
        if let Some(game) = game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            // `draw_interface`'s TYPE_MODEL arm rasters into this surface:
            // bind `pix3d` clipping to it once before drawing.
            self.pix3d.set_clipping(surface.width, surface.height);
            // TS 4838: the split private-chat overlay draws first.
            self.draw_private_messages(&mut surface);
            if self.main_overlay_id != -1 {
                self.animate_interface(self.main_overlay_id, self.world_update_num);
                self.draw_interface(self.main_overlay_id, 0, 0, 0, &mut surface);
            }
            if self.main_modal_id != -1 {
                self.animate_interface(self.main_modal_id, self.world_update_num);
                self.draw_interface(self.main_modal_id, 0, 0, 0, &mut surface);
            }
            if self.is_menu_open && self.menu_area == 0 {
                self.draw_minimenu(&mut surface);
            }
        }
        self.area_game = game;

        if !self.is_menu_open {
            self.build_minimenu();
            self.draw_feedback();
        }
    }

    /// `drawMinimenu` from client-ts (8383-8418): the menu box — `0x5d5447`
    /// fill, black title bar with `Choose Option`, then the options
    /// bottom-to-top (`menu_num_entries - 1 - i`), yellow when the pointer
    /// (offset to the panel's origin) sits in the option's row. The caller
    /// binds the panel surface holding the menu (0 viewport, 1 side,
    /// 2 chat).
    fn draw_minimenu(&mut self, surface: &mut Pix2D) {
        let x = self.menu_x;
        let y = self.menu_y;
        let w = self.menu_width;
        let h = self.menu_height;
        let background: i32 = 0x5d5447;

        surface.fill_rect(x, y, w, h, background);
        surface.fill_rect(x + 1, y + 1, w - 2, 16, Colour::BLACK);
        surface.draw_rect(x + 1, y + 18, w - 2, h - 19, Colour::BLACK);

        let mut mouse_x = self.shell.mouse_x;
        let mut mouse_y = self.shell.mouse_y;
        if self.menu_area == 0 {
            mouse_x -= 4;
            mouse_y -= 4;
        } else if self.menu_area == 1 {
            mouse_x -= 553;
            mouse_y -= 205;
        } else if self.menu_area == 2 {
            mouse_x -= 17;
            mouse_y -= 357;
        }

        if let Some(b12) = &mut self.b12 {
            b12.draw_string(surface, Some("Choose Option"), x + 3, y + 14, background);

            for i in 0..self.menu_num_entries {
                let option_y = y + (self.menu_num_entries - 1 - i) * 15 + 31;

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
                    &self.menu_option[i as usize],
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
    fn draw_feedback(&mut self) {
        if self.menu_num_entries < 2 && self.use_mode == 0 && self.target_mode == 0 {
            return;
        }

        let tooltip = if self.use_mode == 1 && self.menu_num_entries < 2 {
            format!("Use {} with...", self.obj_selected_name)
        } else if self.target_mode == 1 && self.menu_num_entries < 2 {
            format!("{}...", self.target_op)
        } else {
            self.menu_option[(self.menu_num_entries - 1) as usize].clone()
        };

        let tooltip = if self.menu_num_entries > 2 {
            format!(
                "{}@whi@ / {} more options",
                tooltip,
                self.menu_num_entries - 2
            )
        } else {
            tooltip
        };

        let mut game = self.area_game.take();
        if let Some(game) = game.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut game.pixels, game.width, game.height);
            if let Some(b12) = &mut self.b12 {
                b12.draw_string_anti_macro(
                    &mut surface,
                    &tooltip,
                    4,
                    15,
                    Colour::WHITE,
                    true,
                    (self.loop_cycle / 1000) as i32,
                );
            }
        }
        self.area_game = game;
    }

    /// `addPlayers` from client-ts (4260): add the local player (or every
    /// player) as a dynamic sprite at its ground height. The minimap-flag
    /// reset and its `ANTICHEAT_CYCLELOGIC6` send are minimap scope (Task
    /// 6/7); the tile-occupancy stamp is kept so a second entity on a tile
    /// defers to the first this cycle.
    fn add_players(&mut self, add_self: bool) {
        if self.local_player.is_none() {
            return;
        }

        let count = if add_self { 1 } else { self.player_count };
        for i in 0..count as usize {
            let (player, id) = if add_self {
                let Some(player) = self.local_player.as_mut() else {
                    continue;
                };
                (player, crate::client::client::LOCAL_PLAYER_INDEX << 14)
            } else {
                let player_id = self.player_ids[i];
                let Some(player) = self.players.get_mut(player_id as usize).and_then(|p| p.as_mut())
                else {
                    continue;
                };
                (player, player_id << 14)
            };

            if !player.is_ready() {
                continue;
            }
            player.low_memory = false;
            if ((self.config.lowmem && self.player_count > 50) || self.player_count > 200)
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

            let y = get_av_h(&self.groundh, &self.mapl, player.x, player.z, self.minusedlevel);
            let model = Some(SceneModel::Player(player.clone()));

            if player.loc_model.is_none()
                || self.loop_cycle < player.loc_start_cycle
                || self.loop_cycle >= player.loc_stop_cycle
            {
                if (player.x & 0x7f) == 64 && (player.z & 0x7f) == 64 {
                    let tile = (stx * BuildArea::SIZE + stz) as usize;
                    if self.tile_last_occupied_cycle[tile] == self.scene_cycle {
                        continue;
                    }
                    self.tile_last_occupied_cycle[tile] = self.scene_cycle;
                }

                player.y = y;
                self.world.add_dynamic(
                    self.minusedlevel,
                    player.x,
                    player.y,
                    player.z,
                    model,
                    id,
                    player.yaw,
                    60,
                    player.needs_forward_draw_padding,
                );
            } else {
                player.low_memory = false;
                player.y = y;
                self.world.add_dynamic2(
                    self.minusedlevel,
                    player.x,
                    player.y,
                    player.z,
                    player.min_tile_x,
                    player.min_tile_z,
                    player.max_tile_x,
                    player.max_tile_z,
                    model,
                    id,
                    player.yaw,
                );
            }
        }
    }

    /// `addNpcs` from client-ts (4328): add every NPC as a dynamic sprite,
    /// split by the `alwaysontop` flag.
    fn add_npcs(&mut self, alwaysontop: bool) {
        for i in 0..self.npc_count as usize {
            let npc_id = self.npc_ids[i];
            let typecode = (npc_id << 14) + 0x2000_0000;
            let Some(npc) = self.npc.get_mut(npc_id as usize).and_then(|n| n.as_mut()) else {
                continue;
            };
            let Some(npc_type_id) = npc.r#type else {
                continue;
            };
            if self.cache.npc(npc_type_id).alwaysontop != alwaysontop {
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

            let y = get_av_h(&self.groundh, &self.mapl, npc.x, npc.z, self.minusedlevel);
            self.world.add_dynamic(
                self.minusedlevel,
                npc.x,
                y,
                npc.z,
                Some(SceneModel::Npc(npc.clone())),
                typecode,
                npc.yaw,
                (npc.size - 1) * 64 + 60,
                npc.needs_forward_draw_padding,
            );
        }
    }

    /// `addProjectiles` from client-ts (4356): unlink projectiles whose
    /// level no longer matches or whose flight window passed, retarget the
    /// rest onto their npc/player target, advance them by
    /// `world_update_num`, and re-add them as dynamic sprites. The
    /// `cyclelogic1` anticheat payload (TS 4387-4413) writes a length-
    /// prefixed random blob.
    fn add_projectiles(&mut self) {
        let mut node = self.projectiles.head();
        while let Some(proj) = node {
            if proj.level != self.minusedlevel || self.loop_cycle > proj.t2 {
                self.projectiles.unlink_last();
            } else if self.loop_cycle >= proj.t1 {
                if proj.target > 0 {
                    let index = (proj.target - 1) as usize;
                    if let Some(npc) = self.npc.get(index).and_then(|n| n.as_ref()) {
                        let h2 = proj.h2;
                        let level = proj.level;
                        let y = get_av_h(&self.groundh, &self.mapl, npc.x, npc.z, level) - h2;
                        proj.set_target(npc.x as f64, y as f64, npc.z as f64, self.loop_cycle);
                    }
                }

                if proj.target < 0 {
                    let index = -proj.target - 1;
                    let player = if index == self.self_slot {
                        self.local_player.as_ref()
                    } else {
                        self.players.get(index as usize).and_then(|p| p.as_ref())
                    };
                    if let Some(player) = player {
                        let h2 = proj.h2;
                        let level = proj.level;
                        let y = get_av_h(&self.groundh, &self.mapl, player.x, player.z, level) - h2;
                        proj.set_target(
                            player.x as f64,
                            y as f64,
                            player.z as f64,
                            self.loop_cycle,
                        );
                    }
                }

                proj.move_by(self.world_update_num);
                // TS 4382-4383: `proj.x | 0` (Rust `as i32`), typecode -1,
                // padding 60, no forward padding.
                let (x, y, z, yaw) = (proj.x as i32, proj.y as i32, proj.z as i32, proj.yaw);
                self.world.add_dynamic(
                    self.minusedlevel,
                    x,
                    y,
                    z,
                    Some(SceneModel::Proj(proj.clone())),
                    -1,
                    yaw,
                    60,
                    false,
                );
            }
            node = self.projectiles.next();
        }

        // TS 4387-4413: `cyclelogic1` anticheat every 1175 cycles.
        self.cyclelogic1 += 1;
        if self.cyclelogic1 > 1174 {
            self.cyclelogic1 = 0;

            self.out.p1_enc(ClientProt::ANTICHEAT_CYCLELOGIC1.id);
            self.out.p1(0);
            let start = self.out.pos;
            if (random_float() * 2.0) as i32 == 0 {
                self.out.p2(11499);
            }
            self.out.p2(10548);
            if (random_float() * 2.0) as i32 == 0 {
                self.out.p1(139);
            }
            if (random_float() * 2.0) as i32 == 0 {
                self.out.p1(94);
            }
            self.out.p2(51693);
            self.out.p1(16);
            self.out.p2(15036);
            if (random_float() * 2.0) as i32 == 0 {
                self.out.p1(65);
            }
            self.out.p1((random_float() * 256.0) as i32);
            self.out.p2(22990);
            self.out.psize1((self.out.pos - start) as i32);
        }
    }

    /// `addMapAnim` from client-ts (4416): unlink spots on the wrong level
    /// or already complete; otherwise advance (`update` with
    /// `world_update_num`), unlink when that completes the anim, else place
    /// the spot as a dynamic sprite (typecode -1, yaw 0, padding 60).
    fn add_map_anim(&mut self) {
        let mut node = self.spotanims.head();
        while let Some(spot) = node {
            if spot.level != self.minusedlevel || spot.anim_complete {
                self.spotanims.unlink_last();
            } else if self.loop_cycle >= spot.start_cycle {
                spot.update(&self.cache, self.world_update_num);
                if spot.anim_complete {
                    self.spotanims.unlink_last();
                } else {
                    let (level, x, y, z) = (spot.level, spot.x, spot.y, spot.z);
                    self.world.add_dynamic(
                        level,
                        x,
                        y,
                        z,
                        Some(SceneModel::SpotAnim(spot.clone())),
                        -1,
                        0,
                        60,
                        false,
                    );
                }
            }
            node = self.spotanims.next();
        }
    }

    /// `camFollow` from client-ts (4432): position the eye at `distance`
    /// along the inverse pitch/yaw from the target.
    fn cam_follow(
        &mut self,
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

        self.cam_x = target_x - x;
        self.cam_y = target_y - y;
        self.cam_z = target_z - z;
        self.cam_pitch = pitch;
        self.cam_yaw = yaw;
    }

    /// `camShake` jitter from client-ts (4211-4235): for each active axis,
    /// add `random * (ran*2 + 1) - ran + sin(cycle * amp/100) * shakeRan`
    /// to the rendered eye — x/y/z positions, the 11-bit yaw, or the pitch
    /// clamped to 128..383. TS mutates `cam*` in place for `renderAll` and
    /// restores the pre-jitter snapshot afterwards; the caller passes that
    /// snapshot in and receives the jittered eye to render with.
    pub fn cam_shake_jitter(
        &mut self,
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
            if !self.cam_shake[axis] {
                continue;
            }

            let jitter = (self.rand.next_double()
                * (self.cam_shake_axis[axis] * 2 + 1) as f64
                - self.cam_shake_axis[axis] as f64
                + (self.cam_shake_cycle[axis] as f64 * (self.cam_shake_amp[axis] as f64 / 100.0))
                    .sin()
                    * self.cam_shake_ran[axis] as f64) as i32;

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
    pub fn follow_camera(&mut self) {
        let Some(player) = &self.local_player else {
            return;
        };
        let orbit_x = player.x + self.macro_camera_x;
        let orbit_z = player.z + self.macro_camera_z;

        if self.orbit_camera_x - orbit_x < -500
            || self.orbit_camera_x - orbit_x > 500
            || self.orbit_camera_z - orbit_z < -500
            || self.orbit_camera_z - orbit_z > 500
        {
            self.orbit_camera_x = orbit_x;
            self.orbit_camera_z = orbit_z;
        }

        if self.orbit_camera_x != orbit_x {
            self.orbit_camera_x += (orbit_x - self.orbit_camera_x) / 16;
        }
        if self.orbit_camera_z != orbit_z {
            self.orbit_camera_z += (orbit_z - self.orbit_camera_z) / 16;
        }

        if self.shell.key_held[1] == 1 {
            self.orbit_camera_yaw_velocity += (-self.orbit_camera_yaw_velocity - 24) / 2;
        } else if self.shell.key_held[2] == 1 {
            self.orbit_camera_yaw_velocity += (24 - self.orbit_camera_yaw_velocity) / 2;
        } else {
            self.orbit_camera_yaw_velocity /= 2;
        }

        if self.shell.key_held[3] == 1 {
            self.orbit_camera_pitch_velocity += (12 - self.orbit_camera_pitch_velocity) / 2;
        } else if self.shell.key_held[4] == 1 {
            self.orbit_camera_pitch_velocity += (-self.orbit_camera_pitch_velocity - 12) / 2;
        } else {
            self.orbit_camera_pitch_velocity /= 2;
        }

        self.orbit_camera_yaw = (self.orbit_camera_yaw + self.orbit_camera_yaw_velocity / 2) & 0x7ff;
        self.orbit_camera_pitch =
            (self.orbit_camera_pitch + self.orbit_camera_pitch_velocity / 2).clamp(128, 383);

        let orbit_tile_x = self.orbit_camera_x >> 7;
        let orbit_tile_z = self.orbit_camera_z >> 7;
        let orbit_y = get_av_h(&self.groundh, &self.mapl, self.orbit_camera_x, self.orbit_camera_z, self.minusedlevel);
        let mut max_y = 0;
        if orbit_tile_x > 3 && orbit_tile_z > 3 && orbit_tile_x < 100 && orbit_tile_z < 100 {
            for x in (orbit_tile_x - 4)..=(orbit_tile_x + 4) {
                for z in (orbit_tile_z - 4)..=(orbit_tile_z + 4) {
                    let y = orbit_y
                        - self.groundh[self.minusedlevel as usize][x as usize][z as usize];
                    if y > max_y {
                        max_y = y;
                    }
                }
            }
        }

        let clamp = (max_y * 192).clamp(32768, 98048);

        if clamp > self.camera_pitch_clamp {
            self.camera_pitch_clamp += (clamp - self.camera_pitch_clamp) / 24;
        } else if clamp < self.camera_pitch_clamp {
            self.camera_pitch_clamp += (clamp - self.camera_pitch_clamp) / 80;
        }
    }

    /// `roofCheck` from client-ts (4476): the highest level drawn this
    /// frame. The `mapl` `RemoveRoof` guards are not ported (roof removal
    /// is a later slice), so the answer is the TS no-removal fallback 3.
    fn roof_check(&self) -> i32 {
        3
    }

    /// `roofCheck2` from client-ts (4467): the cutscene-camera roof level.
    /// The `mapl` `RemoveRoof` guards are not ported, so like `roof_check`
    /// this is the TS no-removal fallback 3.
    fn roof_check2(&self) -> i32 {
        3
    }

    /// `entityOverlays` from client-ts (4573): headicons/chat overlays are
    /// a no-op while the overlay sprites are not ported.
    fn entity_overlays(&mut self) {}

    /// `coordArrow` from client-ts (4781): a no-op while `hintType`/
    /// `headicons` are not ported.
    fn coord_arrow(&mut self) {}

    /// `textureRunAnims` from client-ts (4794): a no-op while the animated
    /// texture buffers are not ported.
    fn texture_run_anims(&mut self, _cycle: i32) {}

    /// `prepareGame` from client-ts (2001): allocate the in-game `PixMap`
    /// areas and load the `media` jag sprites, lazily on the first
    /// `game_draw` (TS calls it after a successful login; this crate has no
    /// game-loading flow yet). Sized as TS: `area_game` 512×334 and the
    /// constructor-sized areas as `prepareGame`, the `areaBack*` strips at
    /// their sprites with `quickPlotSprite` (0, 0) as 1098. `area_map`
    /// gets the `mapback` ring plotted at (0, 0) as TS 2022-2023, and the
    /// minimap/compass scanline masks are built from `mapback.data` as TS
    /// 1180-1216 (TS builds them in maininit; the depacks live here in this
    /// port). A missing `media` pack skips the sprite loads — `game_draw`
    /// still draws the panels that are present. The title is unloaded and
    /// `image_title2` nulled as Java `prepareGame` (`Client.java` 6919);
    /// `logout` nulls it again so a later `prepare_title` reallocates the
    /// regions from the `title` jag. `pub(super)` so the cold-login path
    /// (`client.rs` response 2) can call it as Java `login` does.
    pub(super) fn prepare_game(&mut self) {
        if self.area_chat.is_some() {
            return;
        }

        self.unload_title();
        self.image_title2 = None;
        self.load_fonts();

        self.area_game = Some(PixMap::new(512, 334));
        self.area_map = Some(PixMap::new(172, 156));
        self.area_side = Some(PixMap::new(190, 261));
        self.area_chat = Some(PixMap::new(479, 96));
        self.area_backbase1 = Some(PixMap::new(496, 50));
        self.area_backbase2 = Some(PixMap::new(269, 37));
        self.area_backhmid1 = Some(PixMap::new(249, 45));

        let path = format!("{}/media", self.config.cache_dir);
        if let Ok(bytes) = std::fs::read(&path) {
            let jag = JagFile::new(bytes);
            self.invback = Pix8::depack(&jag, "invback", 0).ok();
            self.scrollbar1 = Pix8::depack(&jag, "scrollbar", 0).ok();
            self.scrollbar2 = Pix8::depack(&jag, "scrollbar", 1).ok();
            self.chatback = Pix8::depack(&jag, "chatback", 0).ok();
            self.backbase1 = Pix8::depack(&jag, "backbase1", 0).ok();
            self.backbase2 = Pix8::depack(&jag, "backbase2", 0).ok();
            self.backhmid1 = Pix8::depack(&jag, "backhmid1", 0).ok();
            for i in 0..13 {
                self.sideicons[i] = Pix8::depack(&jag, "sideicons", i as i32).ok();
            }
            // redstone1..2hv as Client.ts 1068-1093: the flipped copies are
            // fresh depacks of the base sprite, hflip/vflip'd in place.
            self.redstone1 = Pix8::depack(&jag, "redstone1", 0).ok();
            self.redstone2 = Pix8::depack(&jag, "redstone2", 0).ok();
            self.redstone3 = Pix8::depack(&jag, "redstone3", 0).ok();
            self.redstone1h = self.redstone1.clone();
            if let Some(s) = self.redstone1h.as_mut() {
                s.hflip();
            }
            self.redstone2h = self.redstone2.clone();
            if let Some(s) = self.redstone2h.as_mut() {
                s.hflip();
            }
            self.redstone1v = self.redstone1.clone();
            if let Some(s) = self.redstone1v.as_mut() {
                s.vflip();
            }
            self.redstone2v = self.redstone2.clone();
            if let Some(s) = self.redstone2v.as_mut() {
                s.vflip();
            }
            self.redstone3v = self.redstone3.clone();
            if let Some(s) = self.redstone3v.as_mut() {
                s.vflip();
            }
            self.redstone1hv = self.redstone1.clone();
            if let Some(s) = self.redstone1hv.as_mut() {
                s.hflip();
            }
            if let Some(s) = self.redstone1hv.as_mut() {
                s.vflip();
            }
            self.redstone2hv = self.redstone2.clone();
            if let Some(s) = self.redstone2hv.as_mut() {
                s.hflip();
            }
            if let Some(s) = self.redstone2hv.as_mut() {
                s.vflip();
            }
            self.area_backleft1 = Self::chrome_area(&jag, "backleft1");
            self.area_backleft2 = Self::chrome_area(&jag, "backleft2");
            self.area_backright1 = Self::chrome_area(&jag, "backright1");
            self.area_backright2 = Self::chrome_area(&jag, "backright2");
            self.area_backtop1 = Self::chrome_area(&jag, "backtop1");
            self.area_backvmid1 = Self::chrome_area(&jag, "backvmid1");
            self.area_backvmid2 = Self::chrome_area(&jag, "backvmid2");
            self.area_backvmid3 = Self::chrome_area(&jag, "backvmid3");
            self.area_backhmid2 = Self::chrome_area(&jag, "backhmid2");

            // Minimap sprites (TS maininit 1006-1063): the `mapback` ring
            // (the scanline mask), the composed-map/compass/edge sprites and
            // the map dots/markers. `minimap` itself was allocated in
            // `Client::new` as TS maininit 868.
            self.mapback = Pix8::depack(&jag, "mapback", 0).ok();
            self.compass = Pix32::depack(&jag, "compass", 0).ok();
            self.mapedge = Pix32::depack(&jag, "mapedge", 0).ok();
            if let Some(edge) = self.mapedge.as_mut() {
                edge.trim();
            }
            self.mapmarker1 = Pix32::depack(&jag, "mapmarker", 0).ok();
            self.mapmarker2 = Pix32::depack(&jag, "mapmarker", 1).ok();
            self.mapdots1 = Pix32::depack(&jag, "mapdots", 0).ok();
            self.mapdots2 = Pix32::depack(&jag, "mapdots", 1).ok();
            self.mapdots3 = Pix32::depack(&jag, "mapdots", 2).ok();
            self.mapdots4 = Pix32::depack(&jag, "mapdots", 3).ok();

            // TS maininit 1020-1035: the minimap wall/scene icons; a sprite
            // the jag lacks stays `None` and `draw_detail` skips its plot.
            for i in 0..50 {
                self.mapscene[i] = Pix8::depack(&jag, "mapscene", i as i32).ok();
            }
            for i in 0..50 {
                self.mapfunction[i] = Pix32::depack(&jag, "mapfunction", i as i32).ok();
            }

            // TS prepareGame 2022-2023: `area_map` starts as the `mapback`
            // ring (a fresh `PixMap` is already zeroed, so the `cls()` is a
            // no-op here). `minimapDraw` rotates the map inside the ring.
            if let Some(map) = self.area_map.as_mut() {
                let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
                if let Some(mapback) = &self.mapback {
                    mapback.plot_sprite(&mut surface, 0, 0);
                }
            }
        }

        // Run unconditionally so a missing `media` pack still leaves the
        // masks sized (zeroed) for `minimap_draw`'s rotate-plots.
        self.build_minimap_masks();

        // TS maininit 1152-1154 `unpackTextures` / `initColourTable` /
        // `initPool`: depack the 50 textures from the `textures` jag, then
        // initialise the texel pool and the gamma-corrected per-texture
        // palettes (the palette half of `initColourTable`; the global
        // colour table was built in `Client::new`). A missing `textures`
        // jag skips the depacks — textured ground then falls back to the
        // average-colour gouraud branch instead of drawing nothing.
        let textures_path = format!("{}/textures", self.config.cache_dir);
        if let Ok(bytes) = std::fs::read(&textures_path) {
            let jag = JagFile::new(bytes);
            self.pix3d.unpack_textures(&jag);
        }
        self.pix3d.init_pool(20);
        self.pix3d.init_texture_palettes(0.8);

        self.redraw_frame = true;
    }

    /// TS 1098 construction for the `areaBack*` strips: a `PixMap` at the
    /// sprite's own size with the sprite `quickPlotSprite`d at (0, 0).
    fn chrome_area(jag: &JagFile, name: &str) -> Option<PixMap> {
        let sprite = Pix32::depack(jag, name, 0).ok()?;
        let mut area = PixMap::new(sprite.wi, sprite.hi);
        let mut surface = Pix2D::with_pixels(&mut area.pixels, area.width, area.height);
        sprite.quick_plot_sprite(&mut surface, 0, 0);
        Some(area)
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
    fn build_minimap_masks(&mut self) {
        self.compass_mask_line_offsets = vec![0; 33];
        self.compass_mask_line_lengths = vec![0; 33];
        self.minimap_mask_line_offsets = vec![0; 151];
        self.minimap_mask_line_lengths = vec![0; 151];
        let Some(mapback) = &self.mapback else {
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
    fn draw_side(&mut self) {
        let mut side = self.area_side.take();
        if let Some(side) = side.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut side.pixels, side.width, side.height);
            // `draw_interface`'s TYPE_MODEL arm rasters into this surface:
            // bind `pix3d` clipping to it once before drawing.
            self.pix3d.set_clipping(surface.width, surface.height);
            if let Some(invback) = &self.invback {
                invback.plot_sprite(&mut surface, 0, 0);
            }
            if self.side_modal_id != -1 {
                self.draw_interface(self.side_modal_id, 0, 0, 0, &mut surface);
            } else if self.side_icon.get(self.active_icon as usize).copied() != Some(-1) {
                self.draw_interface(self.side_icon[self.active_icon as usize], 0, 0, 0, &mut surface);
            }
            if self.is_menu_open && self.menu_area == 1 {
                self.draw_minimenu(&mut surface);
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
    pub fn inv_number(&self, amount: i32) -> String {
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
    pub fn nice_number(&self, amount: i32) -> String {
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
    pub fn draw_interface(&mut self, com_id: i32, x: i32, y: i32, scroll_y: i32, surface: &mut Pix2D) {
        let Some(com) = self.cache.ifaces.get(com_id as usize).and_then(|o| o.as_ref()) else {
            return;
        };
        // TS 9901-9905: only TYPE_LAYER draws; a hidden layer still draws
        // while its id is hovered (the `over*ComId` pointer state).
        if com.r#type != ComponentType::TYPE_LAYER
            || (com.hide
                && self.over_main_com_id != com.id
                && self.over_side_com_id != com.id
                && self.over_chat_com_id != com.id)
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
            self.client_component(child_id as i32);
            let Some(child) = self.cache.ifaces.get(child_id).and_then(|o| o.as_ref()) else {
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
                        if let Some(c) = self.cache.ifaces.get_mut(child_id).and_then(|o| o.as_mut()) {
                            c.scroll_pos = scroll_pos;
                        }
                    }
                    self.draw_interface(children[i], child_x, child_y, scroll_pos, surface);
                    // drawScrollbar (TS 9941): a scrollable layer draws its
                    // scrollbar after the recurse.
                    let (child_w, child_h, child_sh) = self
                        .cache
                        .ifaces
                        .get(child_id)
                        .and_then(|o| o.as_ref())
                        .map(|c| (c.width, c.height, c.scroll_height))
                        .unwrap_or((0, 0, 0));
                    if child_sh > child_h {
                        self.draw_scrollbar(
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
                    let hovered = self.over_main_com_id == child.id
                        || self.over_side_com_id == child.id
                        || self.over_chat_com_id == child.id;
                    let colour = if self.get_if_active(child) {
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
                    let active = self.get_if_active(child);
                    let mut text = child.text.clone();
                    // TS 10077-10098: hovered picks `colour_over`/
                    // `colour2_over`; an active text renders `text2`.
                    let hovered = self.over_main_com_id == child.id
                        || self.over_side_com_id == child.id
                        || self.over_chat_com_id == child.id;
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
                    if child.button_type == ButtonType::BUTTON_CONTINUE && self.resumed_pause_button {
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
                                let value = self.get_if_var(child, n).unwrap_or(-2);
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
                        1 => self.p12.as_mut(),
                        2 => self.b12.as_mut(),
                        3 => self.q8.as_mut(),
                        _ => self.p11.as_mut(),
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
                        if self.get_if_active(child) && !child.graphic2_name.is_empty() {
                            &child.graphic2_name
                        } else {
                            &child.graphic_name
                        };
                    // "name,index" as unpacked from IfType.ts 251-262.
                    if let Some((name, index)) = graphic_name.rsplit_once(',') {
                        if let Ok(index) = index.trim().parse::<i32>() {
                            if let Some(sprite) = Self::graphic_sprite(
                                &mut self.graphic_sprites,
                                &self.config.cache_dir,
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
                                let dragging = self.obj_drag_area != 0
                                    && self.obj_drag_slot == slot
                                    && self.obj_drag_com_id == child.id;
                                // TS 9967-9968: the dragged slot draws even
                                // outside the clip rect (it follows the
                                // pointer past the panel edge).
                                if (slot_x > surface.clip_min_x - 32
                                    && slot_x < surface.clip_max_x
                                    && slot_y > surface.clip_min_y - 32
                                    && slot_y < surface.clip_max_y)
                                    || (self.obj_drag_area != 0 && self.obj_drag_slot == slot)
                                {
                                    let outline = if self.use_mode == 1
                                        && self.obj_selected_slot == slot
                                        && self.obj_selected_com_id == child.id
                                    {
                                        16777215
                                    } else {
                                        0
                                    };
                                    if let Some(sprite) = ObjType::get_sprite(
                                        &self.cache,
                                        &mut self.pix3d,
                                        id,
                                        outline,
                                        count,
                                    ) {
                                        if dragging {
                                            // TS 9975-9989: the grab offset,
                                            // snapped to 0 under ±5px and
                                            // before 5 held cycles.
                                            dx = self.shell.mouse_x - self.obj_grab_x;
                                            dy = self.shell.mouse_y - self.obj_grab_y;
                                            if dx < 5 && dx > -5 {
                                                dx = 0;
                                            }
                                            if dy < 5 && dy > -5 {
                                                dy = 0;
                                            }
                                            if self.obj_drag_cycles < 5 {
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
                                                        * self.world_update_num)
                                                        / 3;
                                                if autoscroll > self.world_update_num * 10 {
                                                    autoscroll = self.world_update_num * 10;
                                                }
                                                if autoscroll > layer_scroll {
                                                    autoscroll = layer_scroll;
                                                }
                                                layer_scroll -= autoscroll;
                                                self.obj_grab_y += autoscroll;
                                            }
                                            if slot_y + dy + 32 > surface.clip_max_y
                                                && layer_scroll < layer_sh - height
                                            {
                                                let mut autoscroll = ((slot_y + dy + 32
                                                    - surface.clip_max_y)
                                                    * self.world_update_num)
                                                    / 3;
                                                if autoscroll > self.world_update_num * 10 {
                                                    autoscroll = self.world_update_num * 10;
                                                }
                                                if autoscroll
                                                    > layer_sh - height - layer_scroll
                                                {
                                                    autoscroll =
                                                        layer_sh - height - layer_scroll;
                                                }
                                                layer_scroll += autoscroll;
                                                self.obj_grab_y -= autoscroll;
                                            }
                                        } else if self.selected_area != 0
                                            && self.selected_item == slot
                                            && self.selected_com_id == child.id
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
                                            let text = self.inv_number(count);
                                            if let Some(p11) = self.p11.as_mut() {
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
                                                    &self.config.cache_dir,
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
                                if id as usize >= self.cache.objs.len() {
                                    slot += 1;
                                    continue;
                                }
                                let count = link_obj_number[slot as usize];
                                let mut text = self.cache.objs[id as usize].name.clone();
                                if self.cache.objs[id as usize].stackable || count != 1 {
                                    text.push_str(&format!(" x{}", self.nice_number(count)));
                                }
                                let text_x = child_x + col * (child.margin_x + 115);
                                let text_y = child_y + row * (child.margin_y + 12);
                                let font = match child.font {
                                    1 => self.p12.as_mut(),
                                    2 => self.b12.as_mut(),
                                    3 => self.q8.as_mut(),
                                    _ => self.p11.as_mut(),
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
                    let child = child.clone();
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

                    let active = self.get_if_active(&child);
                    let model_anim = if active { child.model_anim2 } else { child.model_anim };
                    let local_player = self.local_player.as_ref();
                    let model = if model_anim == -1 {
                        child.get_temp_model(&self.cache, local_player, -1, -1, active)
                    } else if (model_anim as usize) < self.cache.seqs.len() {
                        let seq = &self.cache.seqs[model_anim as usize];
                        let frame = child.anim_frame as usize;
                        match (seq.frames.as_ref(), seq.iframes.as_ref()) {
                            (Some(frames), Some(iframes))
                                if frame < frames.len() && frame < iframes.len() =>
                            {
                                child.get_temp_model(
                                    &self.cache,
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
            if let Some(c) = self.cache.ifaces.get_mut(com_id as usize).and_then(|o| o.as_mut()) {
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
        &mut self,
        surface: &mut Pix2D,
        x: i32,
        y: i32,
        scroll_y: i32,
        scroll_height: i32,
        height: i32,
    ) {
        if let Some(sprite) = &self.scrollbar1 {
            sprite.plot_sprite(surface, x, y);
        }
        if let Some(sprite) = &self.scrollbar2 {
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

    /// `doScrollbar` from client-ts (10291-10329): the up/down arrows step
    /// `scroll_pos` by `scroll_cycle*4`, and a press in the track/grip
    /// jumps to the grip position (grabbing it widens the track hit area to
    /// 32 px next call). The target is `cache.ifaces[com_id]`, or
    /// `chat_interface` for a negative `com_id` (the chat scrollbar is a
    /// synthetic interface, TS `chatInterface`, not in the jag).
    pub fn do_scrollbar(
        &mut self,
        x: i32,
        y: i32,
        scrollable_height: i32,
        height: i32,
        redraw: bool,
        left: i32,
        top: i32,
        com_id: i32,
    ) {
        if self.scroll_grabbed {
            self.scroll_input_padding = 32;
        } else {
            self.scroll_input_padding = 0;
        }
        self.scroll_grabbed = false;

        let com = if com_id < 0 {
            Some(&mut self.chat_interface)
        } else {
            self.cache
                .ifaces
                .get_mut(com_id as usize)
                .and_then(|o| o.as_mut())
        };
        let Some(com) = com else {
            return;
        };

        if x >= left && x < left + 16 && y >= top && y < top + 16 {
            com.scroll_pos -= self.scroll_cycle * 4;
            if redraw {
                self.redraw_side = true;
            }
        } else if x >= left && x < left + 16 && y >= top + height - 16 && y < top + height {
            com.scroll_pos += self.scroll_cycle * 4;
            if redraw {
                self.redraw_side = true;
            }
        } else if x >= left - self.scroll_input_padding
            && x < left + self.scroll_input_padding + 16
            && y >= top + 16
            && y < top + height - 16
            && self.scroll_cycle > 0
        {
            let mut grip_size = ((height - 32) * height) / scrollable_height;
            if grip_size < 8 {
                grip_size = 8;
            }
            let grip_y = y - top - (grip_size / 2) - 16;
            let max_y = height - grip_size - 32;
            com.scroll_pos = ((scrollable_height - height) * grip_y) / max_y;
            if redraw {
                self.redraw_side = true;
            }
            self.scroll_grabbed = true;
        }
    }

    /// `IfType.getSprite` from client-ts (IfType.ts 232): depack a `Pix32`
    /// from the `media` jag on demand, cached per `(name, index)` so the
    /// jag is only read on a miss. A failed depack caches as `None`, so a
    /// sprite missing from the pack does not re-read the jag every draw.
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
    /// `pub(super)` so `client.rs`'s `animate_interface` can select the
    /// active model anim.
    pub(super) fn get_if_active(&self, com: &IfType) -> bool {
        let Some(comparator) = &com.script_comparator else {
            return false;
        };
        let Some(operand) = &com.script_operand else {
            return false;
        };
        for i in 0..comparator.len() {
            let Some(value) = self.get_if_var(com, i as i32) else {
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
    pub fn get_if_var(&self, com: &IfType, script_id: i32) -> Option<i32> {
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
                    register = self.stat_effective_level.get(skill as usize).copied().unwrap_or(0);
                }
                2 => {
                    // stat_base_level {skill}
                    let Some(skill) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    register = self.stat_base_level.get(skill as usize).copied().unwrap_or(0);
                }
                3 => {
                    // stat_xp {skill}
                    let Some(skill) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    register = self.stat_xp.get(skill as usize).copied().unwrap_or(0);
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
                    let Some(com) = self.cache.ifaces.get(com_id as usize).and_then(|o| o.as_ref())
                    else {
                        return Some(-1);
                    };
                    if let (Some(link_obj_type), Some(link_obj_number)) =
                        (&com.link_obj_type, &com.link_obj_number)
                    {
                        if obj >= 0
                            && (obj as usize) < self.cache.objs.len()
                            && (!self.cache.objs[obj as usize].members || self.config.members)
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
                    register = self.var.get(id as usize).copied().unwrap_or(0);
                }
                6 => {
                    // stat_xp_remaining {skill}
                    let Some(skill) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let base = self.stat_base_level.get(skill as usize).copied().unwrap_or(0);
                    register = level_experience().get((base - 1) as usize).copied().unwrap_or(0);
                }
                7 => {
                    let Some(id) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let value = self.var.get(id as usize).copied().unwrap_or(0);
                    // TS `((var * 100) / 46875) | 0`
                    register = ((value as i64 * 100) / 46875) as i32;
                }
                8 => {
                    // combat level: `this.localPlayer?.combatLevel || 0`
                    register = self.local_player.as_ref().map(|p| p.combat_level).unwrap_or(0);
                }
                9 => {
                    // total level
                    for i in 0..Skill::count {
                        if Skill::used[i] {
                            register += self.stat_base_level.get(i).copied().unwrap_or(0);
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
                    let Some(com) = self.cache.ifaces.get(com_id as usize).and_then(|o| o.as_ref())
                    else {
                        return Some(-1);
                    };
                    if let Some(link_obj_type) = &com.link_obj_type {
                        if obj >= 0
                            && (obj as usize) < self.cache.objs.len()
                            && (!self.cache.objs[obj as usize].members || self.config.members)
                            && link_obj_type.contains(&obj)
                        {
                            register = 999_999_999;
                        }
                    }
                }
                11 => {
                    // runenergy
                    register = self.runenergy;
                }
                12 => {
                    // runweight is not ported yet (client.rs)
                    register = 0;
                }
                13 => {
                    // testbit {varp} {bit: 0..31}
                    let Some(varp) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let Some(lsb) = next_operand(script, &mut pc) else {
                        return Some(-1);
                    };
                    let varp = self.var.get(varp as usize).copied().unwrap_or(0);
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
                    let Some(varbit) = self.cache.varbits.get(id as usize) else {
                        return Some(-1);
                    };
                    let value = self.var.get(varbit.basevar as usize).copied().unwrap_or(0);
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
                    if let Some(player) = &self.local_player {
                        register = (player.x >> 7) + self.map_build_base_x;
                    }
                }
                19 => {
                    // coordz
                    if let Some(player) = &self.local_player {
                        register = (player.z >> 7) + self.map_build_base_z;
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

impl Client {
    /// `drawChat` from client-ts (11125): plot `chatback` into `area_chat`,
    /// then the social prompt (TS 11133-11135) or the plain chat branch
    /// (TS 11149-11267): clip (0,0,463,77), the 100 chat lines as
    /// TS 11152-11244, the `username:` + `chat_input + '*'` input line at
    /// y=90, and the `hline` at 77; blit at (17, 357). Deviations: a chat
    /// modal or tutorial interface (TS 11142-11146) draws into `area_chat`
    /// in place of the plain chat; the `modIcons`/`drawScrollbar` sprites
    /// load with Task 14, and the trailing `areaGame.setPixels()` is a
    /// no-op here (no global Pix2D target).
    fn draw_chat(&mut self) {
        let mut chat = self.area_chat.take();
        if let Some(chat) = chat.as_mut() {
            let w = chat.width;
            let h = chat.height;
            let mut surface = Pix2D::with_pixels(&mut chat.pixels, w, h);
            if let Some(chatback) = &self.chatback {
                chatback.plot_sprite(&mut surface, 0, 0);
            }
            if self.social_input_open {
                // TS 11133-11135: the social prompt replaces the chat lines.
                if let Some(b12) = self.b12.as_ref() {
                    b12.centre_string(&mut surface, Some(&self.social_input_header), 239, 40, Colour::BLACK);
                    b12.centre_string(&mut surface, Some(&format!("{}*", self.social_input)), 239, 60, Colour::DARKBLUE);
                }
            } else if self.chat_modal_id != -1 {
                // TS 11142-11146: a chat interface replaces the chat lines
                // (the chatback frame still plots underneath it).
                self.pix3d.set_clipping(surface.width, surface.height);
                self.draw_interface(self.chat_modal_id, 0, 0, 0, &mut surface);
            } else if self.tut_com_id != -1 {
                self.pix3d.set_clipping(surface.width, surface.height);
                self.draw_interface(self.tut_com_id, 0, 0, 0, &mut surface);
            } else {
                let font = self.p12.as_ref();
                let mut line = 0;
                surface.set_clipping(0, 0, 463, 77);

                for i in 0..100 {
                    let message = self.chat_text[i].clone();
                    if message.is_empty() {
                        continue;
                    }
                    let r#type = self.chat_type[i];
                    let y = self.chat_scroll_pos + 70 - line * 14;

                    let mut sender = self.chat_username[i].clone();
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
                            if let Some(font) = font {
                                font.draw_string(&mut surface, Some(&message), 4, y, Colour::BLACK);
                            }
                        }
                        line += 1;
                    } else if (r#type == 1 || r#type == 2)
                        && (r#type == 1
                            || self.chat_public_mode == 0
                            || (self.chat_public_mode == 1 && self.is_friend(&sender)))
                    {
                        if y > 0 && y < 110 {
                            let mut x = 4;
                            // TS plots modIcons[0]/[1] here for modlevel 1/2;
                            // the sprites are not ported, so the 14px advance is
                            // kept without an icon.
                            if modlevel == 1 || modlevel == 2 {
                                x += 14;
                            }
                            if let Some(font) = font {
                                font.draw_string(&mut surface, Some(&format!("{sender}:")), x, y, Colour::BLACK);
                                x += font.string_wid(Some(&sender)) + 8;
                                font.draw_string(&mut surface, Some(&message), x, y, Colour::BLUE);
                            }
                        }
                        line += 1;
                    } else if (r#type == 3 || r#type == 7)
                        && self.split_private_chat == 0
                        && (r#type == 7
                            || self.chat_private_mode == 0
                            || (self.chat_private_mode == 1 && self.is_friend(&sender)))
                    {
                        if y > 0 && y < 110 {
                            let mut x = 4;
                            if let Some(font) = font {
                                font.draw_string(&mut surface, Some("From"), x, y, Colour::BLACK);
                                x += font.string_wid(Some("From "));
                            }
                            if modlevel == 1 || modlevel == 2 {
                                x += 14;
                            }
                            if let Some(font) = font {
                                font.draw_string(&mut surface, Some(&format!("{sender}:")), x, y, Colour::BLACK);
                                x += font.string_wid(Some(&sender)) + 8;
                                font.draw_string(&mut surface, Some(&message), x, y, Colour::DARKRED);
                            }
                        }
                        line += 1;
                    } else if r#type == 4 && (self.chat_trade_mode == 0 || (self.chat_trade_mode == 1 && self.is_friend(&sender))) {
                        if y > 0 && y < 110 {
                            if let Some(font) = font {
                                font.draw_string(&mut surface, Some(&format!("{sender} {message}")), 4, y, 0x800080);
                            }
                        }
                        line += 1;
                    } else if r#type == 5 && self.split_private_chat == 0 && self.chat_private_mode < 2 {
                        if y > 0 && y < 110 {
                            if let Some(font) = font {
                                font.draw_string(&mut surface, Some(&message), 4, y, Colour::DARKRED);
                            }
                        }
                        line += 1;
                    } else if r#type == 6 && self.split_private_chat == 0 && self.chat_private_mode < 2 {
                        if y > 0 && y < 110 {
                            if let Some(font) = font {
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
                    } else if r#type == 8 && (self.chat_trade_mode == 0 || (self.chat_trade_mode == 1 && self.is_friend(&sender))) {
                        if y > 0 && y < 110 {
                            if let Some(font) = font {
                                font.draw_string(&mut surface, Some(&format!("{sender} {message}")), 4, y, 0x7e3200);
                            }
                        }
                        line += 1;
                    }
                }

                surface.reset_clipping();

                self.chat_scroll_height = line * 14 + 7;
                if self.chat_scroll_height < 78 {
                    self.chat_scroll_height = 78;
                }
                // drawScrollbar (TS 11252): the chat scrollbar, scrolled
                // from the bottom (scroll_y is 77 at scroll_pos 0).
                self.draw_scrollbar(
                    &mut surface,
                    463,
                    0,
                    self.chat_scroll_height - self.chat_scroll_pos - 77,
                    self.chat_scroll_height,
                    77,
                );

                let username = match self.local_player.as_ref().and_then(|p| p.name.as_ref()) {
                    Some(name) => name.clone(),
                    None => JString::to_screen_name(&self.login_user),
                };

                if let Some(font) = self.p12.as_ref() {
                    font.draw_string(&mut surface, Some(&format!("{username}:")), 4, 90, Colour::BLACK);
                    let input_x = font.string_wid(Some(&format!("{username}: "))) + 6;
                    font.draw_string(&mut surface, Some(&format!("{}*", self.chat_input)), input_x, 90, Colour::BLUE);
                }

                surface.hline(0, 77, 479, Colour::BLACK);
            }

            if self.is_menu_open && self.menu_area == 2 {
                self.draw_minimenu(&mut surface);
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
    fn draw_icons(&mut self) {
        if self.tut_flash_icon != -1 && self.tut_flash_icon == self.active_icon {
            self.tut_flash_icon = -1;
            self.out.p1_enc(ClientProt::TUT_CLICKSIDE.id);
            self.out.p1(self.active_icon);
        }
        if let Some(area) = self.area_backhmid1.as_mut() {
            if let Some(backhmid1) = &self.backhmid1 {
                let w = area.width;
                let h = area.height;
                let mut surface = Pix2D::with_pixels(&mut area.pixels, w, h);
                backhmid1.plot_sprite(&mut surface, 0, 0);
                if self.side_modal_id == -1 {
                    // TS reads `sideIcon[activeIcon]` as undefined (true) out
                    // of bounds; `get().copied() != Some(-1)` matches.
                    if self.side_icon.get(self.active_icon as usize).copied() != Some(-1) {
                        // redstone for the top row, tabs 0-6 (4018-4034);
                        // tabs 7-13 plot on `area_backbase2` below.
                        let (redstone, x, y) = match self.active_icon {
                            0 => (&self.redstone1, 22, 10),
                            1 => (&self.redstone2, 54, 8),
                            2 => (&self.redstone2, 82, 8),
                            3 => (&self.redstone3, 110, 8),
                            4 => (&self.redstone2h, 153, 8),
                            5 => (&self.redstone2h, 181, 8),
                            6 => (&self.redstone1h, 209, 9),
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
                        if self.side_icon[icon] != -1
                            && (self.tut_flash_icon != icon as i32 || self.loop_cycle % 20 < 10)
                        {
                            if let Some(s) = &self.sideicons[icon] {
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
            if let Some(backbase2) = &self.backbase2 {
                let w = area.width;
                let h = area.height;
                let mut surface = Pix2D::with_pixels(&mut area.pixels, w, h);
                backbase2.plot_sprite(&mut surface, 0, 0);
                if self.side_modal_id == -1 {
                    // redstone for the bottom row, tabs 7-13 (4072-4088).
                    if self.side_icon.get(self.active_icon as usize).copied() != Some(-1) {
                        let (redstone, x, y) = match self.active_icon {
                            7 => (&self.redstone1v, 42, 0),
                            8 => (&self.redstone2v, 74, 0),
                            9 => (&self.redstone2v, 102, 0),
                            10 => (&self.redstone3v, 130, 1),
                            11 => (&self.redstone2hv, 173, 0),
                            12 => (&self.redstone2hv, 201, 0),
                            13 => (&self.redstone1hv, 229, 0),
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
                        if self.side_icon[guard] != -1
                            && (self.tut_flash_icon != guard as i32 || self.loop_cycle % 20 < 10)
                        {
                            if let Some(s) = &self.sideicons[sprite] {
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
    fn minimap_draw(&mut self) {
        let Some(player) = self.local_player.as_ref() else {
            return;
        };
        let player_x = player.x;
        let player_z = player.z;
        let Some(map) = self.area_map.as_mut() else {
            return;
        };
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);

        if self.minimap_state == 2 {
            if let Some(mapback) = &self.mapback {
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
            if let Some(compass) = &self.compass {
                compass.scanline_rotate_plot_sprite(
                    &mut surface,
                    0,
                    0,
                    33,
                    33,
                    25,
                    25,
                    self.orbit_camera_yaw as f64,
                    256,
                    &self.compass_mask_line_offsets,
                    &self.compass_mask_line_lengths,
                );
            }
            return;
        }

        let angle = (self.orbit_camera_yaw + self.macro_minimap_angle) & 0x7ff;
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
                self.macro_minimap_zoom + 256,
                &self.minimap_mask_line_offsets,
                &self.minimap_mask_line_lengths,
            );
        }
        if let Some(compass) = &self.compass {
            compass.scanline_rotate_plot_sprite(
                &mut surface,
                0,
                0,
                33,
                33,
                25,
                25,
                self.orbit_camera_yaw as f64,
                256,
                &self.compass_mask_line_offsets,
                &self.compass_mask_line_lengths,
            );
        }

        let dot_angle = angle;
        let dot_zoom = self.macro_minimap_zoom + 256;
        let mapback = self.mapback.as_ref();
        let mapdots1 = self.mapdots1.as_ref();
        let mapdots2 = self.mapdots2.as_ref();
        let mapdots3 = self.mapdots3.as_ref();

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
                if self.world.ground_object_at(self.minusedlevel, ltx, ltz).is_some() {
                    let dot_x = ltx * 4 + 2 - (player_x / 32);
                    let dot_y = ltz * 4 + 2 - (player_z / 32);
                    minimap_draw_dot(&mut surface, dot_y, mapdots1, dot_x, mapback, dot_angle, dot_zoom);
                }
            }
        }

        // TS 11327-11335: NPCs with a minimap flag.
        for i in 0..self.npc_count as usize {
            let npc_id = self.npc_ids[i];
            let Some(npc) = self.npc.get(npc_id as usize).and_then(|n| n.as_ref()) else {
                continue;
            };
            let Some(npc_type_id) = npc.r#type else {
                continue;
            };
            if npc.is_ready() && self.cache.npc(npc_type_id).minimap {
                let dot_x = (npc.x / 32) - (player_x / 32);
                let dot_y = (npc.z / 32) - (player_z / 32);
                minimap_draw_dot(&mut surface, dot_y, mapdots2, dot_x, mapback, dot_angle, dot_zoom);
            }
        }

        // TS 11337-11357: players (friends split onto dots4; no friend list
        // is ported, so everyone draws dots3).
        for i in 0..self.player_count as usize {
            let player_id = self.player_ids[i];
            let Some(p) = self.players.get(player_id as usize).and_then(|p| p.as_ref()) else {
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
        if self.hint_type != 0 && self.loop_cycle % 20 < 10 {
            if self.hint_type == 1
                && self.hint_npc >= 0
                && (self.hint_npc as usize) < self.npc.len()
            {
                if let Some(npc) = self.npc[self.hint_npc as usize].as_ref() {
                    let arrow_x = (npc.x / 32) - (player_x / 32);
                    let arrow_y = (npc.z / 32) - (player_z / 32);
                    minimap_draw_arrow(
                        &mut surface,
                        arrow_x,
                        arrow_y,
                        self.mapmarker2.as_ref(),
                        mapback,
                        self.mapedge.as_ref(),
                        dot_angle,
                        dot_zoom,
                    );
                }
            } else if self.hint_type == 2 {
                let arrow_x = (self.hint_tile_x - self.map_build_base_x) * 4 + 2 - (player_x / 32);
                let arrow_y = (self.hint_tile_z - self.map_build_base_z) * 4 + 2 - (player_z / 32);
                minimap_draw_arrow(
                    &mut surface,
                    arrow_x,
                    arrow_y,
                    self.mapmarker2.as_ref(),
                    mapback,
                    self.mapedge.as_ref(),
                    dot_angle,
                    dot_zoom,
                );
            } else if self.hint_type == 10
                && self.hint_player >= 0
                && (self.hint_player as usize) < self.players.len()
            {
                if let Some(player) = self.players[self.hint_player as usize].as_ref() {
                    let arrow_x = (player.x / 32) - (player_x / 32);
                    let arrow_y = (player.z / 32) - (player_z / 32);
                    minimap_draw_arrow(
                        &mut surface,
                        arrow_x,
                        arrow_y,
                        self.mapmarker2.as_ref(),
                        mapback,
                        self.mapedge.as_ref(),
                        dot_angle,
                        dot_zoom,
                    );
                }
            }
        }

        // TS 11383-11388: the walk-flag marker.
        if self.minimap_flag_x != 0 {
            let dot_x = self.minimap_flag_x * 4 + 2 - (player_x / 32);
            let dot_y = self.minimap_flag_z * 4 + 2 - (player_z / 32);
            minimap_draw_dot(
                &mut surface,
                dot_y,
                self.mapmarker1.as_ref(),
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
