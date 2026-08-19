//! Frame draw (Tasks 4/5: title, then in-game). 1:1 port of `Client.ts`
//! `prepareTitle`, `loadTitleBackground`, `loadTitleImages`, `TitleFlames`,
//! `titleScreenDraw` (1489–1694), and `gameDraw` with its `prepareGame`/
//! `drawSide`/`drawChat` helpers (3890–4170, 2001, 11098, 11125). Draws
//! always into `Client::draw_area` (765×503 `PixMap`); present (feature
//! `window`) only blits.
//!
//! The in-game `gameDrawMain` 3D pass (`World::render_all` not implemented)
//! leaves `area_game` a black hole at (4, 4). `drawInterface` draws the
//! side-tab interfaces (`TYPE_LAYER`/`TYPE_RECT`/`TYPE_TEXT`); the minimap is
//! not ported.

use std::path::Path;

use crate::client::client::Client;
use crate::client::title_flames::TitleFlames;
use crate::config::if_type::{ComponentType, IfType};
use crate::graphics::{Colour, Pix2D, Pix32, Pix8, PixFont, PixMap};
use crate::io::JagFile;
use crate::util::JString;

fn plot_title_bg(map: &mut Option<PixMap>, background: &Pix32, x: i32, y: i32) {
    if let Some(map) = map {
        let mut surface = Pix2D::with_pixels(&mut map.pixels, map.width, map.height);
        background.quick_plot_sprite(&mut surface, x, y);
    }
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
    fn prepare_title(&mut self) {
        if self.image_title2.is_some() {
            return;
        }

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
            let path = format!("{}/title", self.config.cache_dir);
            if Path::new(&path).is_file() {
                if let Ok(bytes) = std::fs::read(&path) {
                    self.title = Some(JagFile::new(bytes));
                }
            }
        }

        // `take` so the jag outlives the `&mut self` loads (TS loads it in
        // `maininit` and passes it around freely).
        self.load_fonts();
        if let Some(jag) = self.title.take() {
            self.load_title_background(&jag);
            self.load_title_images(&jag);
            self.title = Some(jag);
        }
        self.request_scape_main();

        self.redraw_frame = true;
    }

    /// Client.ts maininit (!lowMem): scape_main (midiSong = 0) with fade,
    /// requested from on-demand archive 2 — but only after the title assets
    /// above have loaded, so the music starts once the title is prepared
    /// instead of at `Client::new`. The `midi_song < 0` guard makes it a
    /// one-shot (the first prepare arms it; later prepares keep song 0).
    fn request_scape_main(&mut self) {
        if self.config.lowmem || self.midi_song >= 0 {
            return;
        }
        if let Some(od) = &mut self.on_demand {
            self.midi_song = 0;
            self.midi_fading = true;
            od.request(2, 0);
        }
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
            let path = format!("{}/title", self.config.cache_dir);
            if Path::new(&path).is_file() {
                if let Ok(bytes) = std::fs::read(&path) {
                    self.title = Some(JagFile::new(bytes));
                }
            }
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
    /// (4172, the 3D pass) is not ported, so when `scene_state` is 2 the
    /// viewport is a black hole: `area_game` filled black and blitted at
    /// (4, 4). The minimap is out of scope, so its redraw triggers are
    /// dropped with it; the chrome strips, side, chat, icon-strip
    /// backgrounds, and chat-mode panels draw 1:1.
    pub fn game_draw(&mut self) {
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
            }
        }

        if self.scene_state == 2 {
            // `gameDrawMain` (4172): the 3D pass. `World::render_all` is not
            // implemented, so keep the viewport a black hole instead of
            // stale pixels.
            if let Some(g) = self.area_game.as_mut() {
                g.fill(0);
            }
            if let Some(g) = &self.area_game {
                g.blit_into(&mut self.draw_area, 4, 4);
            }
        }

        // `isMenuOpen`/`sideModalId`/`animateInterface`/`selectedArea`/
        // `objDragArea` redrawSide triggers: interface state not ported.

        if self.redraw_side {
            self.draw_side();
            self.redraw_side = false;
        }

        // `chatInterface.scrollPos`/`doScrollbar` and the chatModal/
        // selectedArea/objDragArea/tutComMessage redrawChat triggers: not
        // ported.

        if self.redraw_chat {
            self.draw_chat();
            self.redraw_chat = false;
        }

        // `minimapDraw` (11279): compass/minimap helpers not ported; the map
        // area stays black until then.

        // `tutFlashIcon !== -1` redrawIcons trigger: not ported.

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
    }

    /// `prepareGame` from client-ts (2001): allocate the in-game `PixMap`
    /// areas and load the `media` jag sprites, lazily on the first
    /// `game_draw` (TS calls it after a successful login; this crate has no
    /// game-loading flow yet). Sized as TS: `area_game` 512×334 and the
    /// constructor-sized areas as `prepareGame`, the `areaBack*` strips at
    /// their sprites with `quickPlotSprite` (0, 0) as 1098. A missing
    /// `media` pack skips the sprite loads — `game_draw` still draws the
    /// panels that are present. Out of scope: `mapback` (minimap not ported).
    /// The title sprites are kept (deviation from TS `unloadTitle`: a logout
    /// back to the title screen still draws).
    fn prepare_game(&mut self) {
        if self.area_chat.is_some() {
            return;
        }

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
        }

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

    /// `drawSide` from client-ts (11098): plot `invback` into `area_side`,
    /// draw the open side interface (`side_modal_id`, else the active tab's
    /// `side_icon`) via `drawInterface` (TS 11106-11110), and blit at
    /// (553, 205). The minimenu branch and the trailing `areaGame.setPixels()`
    /// (no global Pix2D target) are not ported.
    fn draw_side(&mut self) {
        let mut side = self.area_side.take();
        if let Some(side) = side.as_mut() {
            let mut surface = Pix2D::with_pixels(&mut side.pixels, side.width, side.height);
            if let Some(invback) = &self.invback {
                invback.plot_sprite(&mut surface, 0, 0);
            }
            if self.side_modal_id != -1 {
                self.draw_interface(self.side_modal_id, 0, 0, 0, &mut surface);
            } else if self.side_icon.get(self.active_icon as usize).copied() != Some(-1) {
                self.draw_interface(self.side_icon[self.active_icon as usize], 0, 0, 0, &mut surface);
            }
        }
        if let Some(side) = &side {
            side.blit_into(&mut self.draw_area, 553, 205);
        }
        self.area_side = side;
    }

    /// `drawInterface` from client-ts (9900) for the 2D component types:
    /// recurse `TYPE_LAYER` children, draw `TYPE_RECT` fill/outline and
    /// `TYPE_TEXT` with the font at `com.font` index 0-3 (p11/p12/b12/q8).
    /// `TYPE_INV` item sprites, `TYPE_GRAPHIC`, and `TYPE_MODEL` (3D) are
    /// skipped; the `clientComponent` scripts and `drawScrollbar` sprites
    /// load with Task 14, and the `%1`-`%5` `getIfVar` substitution is
    /// skipped with the script VM.
    pub fn draw_interface(&mut self, com_id: i32, x: i32, y: i32, scroll_y: i32, surface: &mut Pix2D) {
        let Some(com) = self.cache.ifaces.get(com_id as usize).and_then(|o| o.as_ref()) else {
            return;
        };
        // TS 9901: only TYPE_LAYER draws; a hidden layer skips unless hovered
        // (the `over*ComId` hover state is not ported, so hide is absolute).
        if com.r#type != ComponentType::TYPE_LAYER || com.hide {
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

        let left = surface.clip_min_x;
        let top = surface.clip_min_y;
        let right = surface.clip_max_x;
        let bottom = surface.clip_max_y;
        surface.set_clipping(x, y, x + width, y + height);

        for i in 0..children.len() {
            let child_id = children[i] as usize;
            let Some(child) = self.cache.ifaces.get(child_id).and_then(|o| o.as_ref()) else {
                continue;
            };
            let child_x = child_x[i] + x + child.x;
            let child_y = child_y[i] + y - scroll_y + child.y;

            // `clientComponent(child)` (TS 9926): scripts not ported.

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
                    // drawScrollbar (TS 9941): scrollbar sprites load with
                    // Task 14.
                }
                ComponentType::TYPE_RECT => {
                    // hovered is false: the `over*ComId` state is not ported.
                    let colour = if self.get_if_active(child) {
                        child.colour2
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
                    let font = match child.font {
                        1 => self.p12.as_mut(),
                        2 => self.b12.as_mut(),
                        3 => self.q8.as_mut(),
                        _ => self.p11.as_mut(),
                    };
                    let Some(font) = font else {
                        continue;
                    };
                    let mut text = child.text.clone();
                    // hovered is false: the `over*ComId` state is not ported.
                    let mut colour = if active { child.colour2 } else { child.colour };

                    // TS 10107-10116: the chat-area colour remap.
                    if surface.width == 479 {
                        if colour == 0xffff00 {
                            colour = 0x0000ff;
                        }
                        if colour == 0x00c000 {
                            colour = 0xffffff;
                        }
                    }

                    // `%1`-`%5` getIfVar substitution is skipped with the
                    // script VM.
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
                _ => {
                    // TYPE_INV item sprites, TYPE_GRAPHIC, TYPE_MODEL (3D),
                    // and TYPE_INV_TEXT are skipped this task.
                }
            }
        }

        surface.set_clipping(left, top, right, bottom);
    }

    /// `getIfActive` from client-ts (10361): comparator scripts pick the
    /// active colour for a component. The IfType script VM is not ported, so
    /// no `var` value is computable (`get_if_var` is `None`): every
    /// comparator reads inactive, and `com.colour`/`com.text` draw.
    fn get_if_active(&self, com: &IfType) -> bool {
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

    /// `getIfVar` from client-ts (10394): the script VM is not ported, so no
    /// value is computable; `None` makes `get_if_active` treat the component
    /// as inactive.
    fn get_if_var(&self, _com: &IfType, _script_id: i32) -> Option<i32> {
        None
    }

    /// `drawChat` from client-ts (11125): plot `chatback` into `area_chat`,
    /// then the plain chat branch (TS 11149-11267): clip (0,0,463,77), the
    /// 100 chat lines as TS 11152-11244, the `username:` + `chat_input + '*'`
    /// input line at y=90, and the `hline` at 77; blit at (17, 357).
    /// Deviations: the social/dialog/tutorial/modal branches are not ported
    /// (`chat_modal_id` is only ever -1 this slice), `is_friend` is always
    /// false (no friend list), the `modIcons`/`drawScrollbar` sprites load
    /// with Task 14, and the trailing `areaGame.setPixels()` is a no-op here
    /// (no global Pix2D target).
    fn draw_chat(&mut self) {
        if let Some(chat) = self.area_chat.as_mut() {
            let w = chat.width;
            let h = chat.height;
            let mut surface = Pix2D::with_pixels(&mut chat.pixels, w, h);
            if let Some(chatback) = &self.chatback {
                chatback.plot_sprite(&mut surface, 0, 0);
            }

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
                        || (self.chat_public_mode == 1 && false))
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
                    && (r#type == 7
                        || self.chat_private_mode == 0
                        || (self.chat_private_mode == 1 && false))
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
                } else if r#type == 4 && (self.chat_trade_mode == 0 || (self.chat_trade_mode == 1 && false)) {
                    if y > 0 && y < 110 {
                        if let Some(font) = font {
                            font.draw_string(&mut surface, Some(&format!("{sender} {message}")), 4, y, 0x800080);
                        }
                    }
                    line += 1;
                } else if r#type == 5 && self.chat_private_mode < 2 {
                    if y > 0 && y < 110 {
                        if let Some(font) = font {
                            font.draw_string(&mut surface, Some(&message), 4, y, Colour::DARKRED);
                        }
                    }
                    line += 1;
                } else if r#type == 6 && self.chat_private_mode < 2 {
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
                } else if r#type == 8 && (self.chat_trade_mode == 0 || (self.chat_trade_mode == 1 && false)) {
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
            // drawScrollbar (TS 11252) is not ported (no scrollbar sprites).

            let username = match self.local_player.as_ref().and_then(|p| p.name.as_ref()) {
                Some(name) => name.clone(),
                None => JString::to_screen_name(&self.login_user),
            };

            if let Some(font) = font {
                font.draw_string(&mut surface, Some(&format!("{username}:")), 4, 90, Colour::BLACK);
                let input_x = font.string_wid(Some(&format!("{username}: "))) + 6;
                font.draw_string(&mut surface, Some(&format!("{}*", self.chat_input)), input_x, 90, Colour::BLUE);
            }

            surface.hline(0, 77, 479, Colour::BLACK);
        }

        if let Some(chat) = &self.area_chat {
            chat.blit_into(&mut self.draw_area, 17, 357);
        }
    }

    /// `redrawIcons` from client-ts (4005), 1:1: plot `backhmid1` into
    /// `area_backhmid1` and, when `side_modal_id == -1`, the redstone
    /// highlight under `active_icon` (tabs 0-6) plus the side icons whose
    /// tab is bound (`side_icon[i] != -1`); blit at (516, 160). Then
    /// `backbase2` into `area_backbase2` with the tabs 7-13 redstone and
    /// icons, and blit at (496, 466). Offsets verbatim from 4018-4112.
    /// Deviations: the `tutFlashIcon` blink conditions are dropped with the
    /// tutorial feature (TS `tutFlashIcon` stays -1, so they were always
    /// true), and the bottom-row guard index quirk of 4090-4111 (checks
    /// `side_icon[8]` while plotting `sideicons[7]`, and so on) is kept 1:1.
    /// The trailing `areaGame.setPixels()` is a no-op here (no global Pix2D
    /// target).
    fn draw_icons(&mut self) {
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
                        if self.side_icon[icon] != -1 {
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
                    // index (tab 7's sprite gated on `side_icon[8]`, ...).
                    for (guard, sprite, x, y) in [
                        (8, 7, 74, 2),
                        (9, 8, 102, 3),
                        (10, 9, 137, 4),
                        (11, 10, 174, 2),
                        (12, 11, 201, 2),
                        (13, 12, 226, 2),
                    ] {
                        if self.side_icon[guard] != -1 {
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
}
