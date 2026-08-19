//! Frame draw (Tasks 4/5: title, then in-game). 1:1 port of `Client.ts`
//! `prepareTitle`, `loadTitleBackground` (logo only), `loadTitleImages`,
//! `titleScreenDraw` (1489–1694), and `gameDraw` with its `prepareGame`/
//! `drawSide`/`drawChat` helpers (3890–4170, 2001, 11098, 11125). Draws
//! always into `Client::draw_area` (789×532 `PixMap`); present (feature
//! `window`) only blits.
//!
//! Out of scope: the `title.dat` JPEG background (`Pix32.fromJpeg` — no JPEG
//! crate), so the title regions stay black apart from the logo and the login
//! UI; `TitleFlames` (no type in this crate), so `image_title0/1` stay
//! empty; and the in-game `gameDrawMain` 3D pass (`World::render_all` not
//! implemented), so `area_game` is a black hole blitted at (4, 4). The
//! `drawInterface` panels, the minimap/compass, and the side-icon strip
//! (`sideicons`/`redstone*`) are not ported; the chrome strips, side, chat,
//! icon-strip backgrounds, and chat-mode panels draw 1:1. Both still leave
//! the titlebox/fonts/logo writing non-zero pixels.
//! The TS source's dead `y += 30` / `y += 15` stores (values never read
//! again) are dropped.

use std::path::Path;

use crate::client::client::Client;
use crate::graphics::{Colour, Pix2D, Pix32, Pix8, PixFont, PixMap};
use crate::io::JagFile;
use crate::util::JString;

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
    }

    /// `prepareTitle` from client-ts (1579): create the 9 title `PixMap`
    /// regions (sizes as TS) on the first frame, load the `title` jag from
    /// the cache, the four fonts, and the titlebox/titlebutton sprites.
    /// The `title.dat` JPEG background is skipped; the logo still plots.
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

    /// `loadTitleBackground` from client-ts (1627) minus the JPEG
    /// `title.dat` decode: only the `logo` (a `Pix32` depack) plots, into
    /// `image_title2` at the TS position (`sWid` = 789).
    fn load_title_background(&mut self, jag: &JagFile) {
        if let Ok(logo) = Pix32::depack(jag, "logo", 0) {
            if let Some(map2) = self.image_title2.as_mut() {
                let w = map2.width;
                let h = map2.height;
                let mut surface = Pix2D::with_pixels(&mut map2.pixels, w, h);
                logo.plot_sprite(&mut surface, (789 / 2) - (logo.wi / 2) - 128, 18);
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
        // TS then builds `TitleFlames` over `imageRunes` and
        // `imageTitle0/1` — no such type in this crate, so the flames stay
        // unstarted (and `imageTitle0/1` empty).
    }

    /// `unloadTitle` from client-ts (1992): drop the title sprites. The
    /// `TitleFlames` close is a no-op (no such type in this crate). TS calls
    /// this from `prepareGame`/`mainquit`, neither ported yet.
    pub fn unload_title(&mut self) {
        self.image_titlebox = None;
        self.image_titlebutton = None;
        self.image_runes.clear();
    }

    /// `gameDraw` from client-ts (3890): the in-game frame. `gameDrawMain`
    /// (4172, the 3D pass) is not ported, so when `scene_state` is 2 the
    /// viewport is a black hole: `area_game` filled black and blitted at
    /// (4, 4). `drawInterface` and the minimap are out of scope, so their
    /// redraw triggers are dropped with them; the chrome strips, side, chat,
    /// icon-strip backgrounds, and chat-mode panels draw 1:1.
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
    /// panels that are present. Out of scope: `mapback` (minimap not ported)
    /// and the sideicons/redstone families (icon strip backgrounds blit,
    /// the icons themselves not). The title sprites are kept (deviation from
    /// TS `unloadTitle`: a logout back to the title screen still draws).
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

    /// `drawSide` from client-ts (11098): plot `invback` into `area_side`
    /// and blit it at (553, 205). The `drawInterface` (side modal / active
    /// icon) and minimenu branches are not ported. The trailing
    /// `areaGame.setPixels()` is a no-op here (no global Pix2D target).
    fn draw_side(&mut self) {
        if let Some(side) = self.area_side.as_mut() {
            if let Some(invback) = &self.invback {
                let w = side.width;
                let h = side.height;
                let mut surface = Pix2D::with_pixels(&mut side.pixels, w, h);
                invback.plot_sprite(&mut surface, 0, 0);
            }
        }

        if let Some(side) = &self.area_side {
            side.blit_into(&mut self.draw_area, 553, 205);
        }
    }

    /// `drawChat` from client-ts (11125): plot `chatback` into `area_chat`
    /// and blit it at (17, 357). The social/dialog/tutorial/modal branches,
    /// the chat text loop (no chat text fields yet), the scrollbar, and the
    /// minimenu are not ported. The trailing `areaGame.setPixels()` is a
    /// no-op here (no global Pix2D target).
    fn draw_chat(&mut self) {
        if let Some(chat) = self.area_chat.as_mut() {
            if let Some(chatback) = &self.chatback {
                let w = chat.width;
                let h = chat.height;
                let mut surface = Pix2D::with_pixels(&mut chat.pixels, w, h);
                chatback.plot_sprite(&mut surface, 0, 0);
            }
        }

        if let Some(chat) = &self.area_chat {
            chat.blit_into(&mut self.draw_area, 17, 357);
        }
    }

    /// `redrawIcons` from client-ts (4005), backgrounds only: plot
    /// `backhmid1` into `area_backhmid1` and blit at (516, 160), then
    /// `backbase2` into `area_backbase2` and blit at (496, 466). The
    /// sideicons/redstone sprite families are not loaded, so the icons
    /// themselves are skipped with them. The trailing `areaGame.setPixels()`
    /// is a no-op here (no global Pix2D target).
    fn draw_icons(&mut self) {
        if let Some(area) = self.area_backhmid1.as_mut() {
            if let Some(backhmid1) = &self.backhmid1 {
                let w = area.width;
                let h = area.height;
                let mut surface = Pix2D::with_pixels(&mut area.pixels, w, h);
                backhmid1.plot_sprite(&mut surface, 0, 0);
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
            }
        }
        if let Some(area) = &self.area_backbase2 {
            area.blit_into(&mut self.draw_area, 496, 466);
        }
    }
}
