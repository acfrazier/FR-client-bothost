//! The process-wide fonts + static media sprites (design rule 5: one copy
//! of immutable; attaching a head clones a pointer). `Renderer` holds an
//! `Arc<Media>`; the first frame (`prepare_game`/`prepare_title`) swaps
//! it for the depacked copy, so fifty heads pay one depack. A missing
//! `title`/`media` pack leaves the sprites `None` — the existing
//! "cache without the pack" behaviour (the draw paths skip the `None`
//! sprites and still draw the panels that are present).

use std::sync::{Arc, Mutex, OnceLock};

use crate::graphics::{Pix2D, Pix32, Pix8, PixFont, PixMap};
use crate::io::{ClientRevision, JagFile};

#[path = "media_377.rs"]
mod media_377;
pub use media_377::Media377;
/// The process-wide fonts + static media sprites, depacked once from the
/// first cache dir that drew a frame. Everything here is immutable after
/// `depack`; the per-head mutable render state (`area_game`, `minimap`,
/// the title regions, the `graphic_sprites` cache) stays on `Renderer`.
pub struct Media {
    /// `p11`/`p12`/`b12`/`q8` from client-ts: the chat and title `PixFont`s
    /// (`p12` also draws the loading splash).
    pub p11: Option<PixFont>,
    pub p12: Option<PixFont>,
    pub b12: Option<PixFont>,
    pub q8: Option<PixFont>,
    /// `title.dat` from the `title` jag, decoded once (JPEG + the mirror
    /// copy the second background pass needs): each head re-plots its own
    /// 9 title regions from these instead of re-reading the jag.
    pub title_dat: Option<Pix32>,
    pub title_dat_flipped: Option<Pix32>,
    /// `logo` from the `title` jag, plotted over `image_title2`.
    pub logo: Option<Pix32>,
    /// `titlebox`/`titlebutton` and the 12 `runes` from the `title` jag.
    pub titlebox: Option<Pix8>,
    pub titlebutton: Option<Pix8>,
    pub runes: Vec<Pix8>,
    /// `cross` (TS 1056-1058): the 8 click-crosshair frames from the
    /// `media` pack, `Pix32.depack('cross', i)`.
    pub cross: [Option<Pix32>; 8],
    pub hitmarks: [Option<Pix32>; 20],
    pub headicons: [Option<Pix32>; 20],
    /// Java `imageHeadiconsHint[32]` from `headicons_hint`; six sprites load,
    /// with indices 0 and 1 used for NPC/player arrows.
    pub headicons_hint: [Option<Pix32>; 32],
    /// `invback`/`chatback`/`backbase1`/`backbase2`/`backhmid1`: the
    /// sprite plot sources for the interface and chat backgrounds.
    pub invback: Option<Pix8>,
    pub chatback: Option<Pix8>,
    pub backbase1: Option<Pix8>,
    pub backbase2: Option<Pix8>,
    pub backhmid1: Option<Pix8>,
    /// `scrollbar1`/`scrollbar2` from client-ts (1065-1066): the top and
    /// bottom cap sprites of an interface/chat scrollbar.
    pub scrollbar1: Option<Pix8>,
    pub scrollbar2: Option<Pix8>,
    /// `sideicons`/`redstone1..2hv` from client-ts (1013, 1068-1093): the
    /// side-tab sprites and the redstone highlight for `active_icon`. The
    /// `*h`/`*v`/`*hv` copies are the same sprites hflip/vflip'd.
    pub sideicons: [Option<Pix8>; 13],
    /// `modIcons` from client-ts (267): the gold (@cr1@) and silver
    /// (@cr2@) staff crowns.
    pub mod_icons: [Option<Pix8>; 2],
    pub redstone1: Option<Pix8>,
    pub redstone2: Option<Pix8>,
    pub redstone3: Option<Pix8>,
    pub redstone1h: Option<Pix8>,
    pub redstone2h: Option<Pix8>,
    pub redstone1v: Option<Pix8>,
    pub redstone2v: Option<Pix8>,
    pub redstone3v: Option<Pix8>,
    pub redstone1hv: Option<Pix8>,
    pub redstone2hv: Option<Pix8>,
    /// The `areaBack*` strips (TS 1098): the chrome `PixMap`s the game
    /// frame blits around `draw_area`. Built once from the `media` sprites
    /// and only ever read after.
    pub area_backleft1: Option<PixMap>,
    pub area_backleft2: Option<PixMap>,
    pub area_backright1: Option<PixMap>,
    pub area_backright2: Option<PixMap>,
    pub area_backtop1: Option<PixMap>,
    pub area_backvmid1: Option<PixMap>,
    pub area_backvmid2: Option<PixMap>,
    pub area_backvmid3: Option<PixMap>,
    pub area_backhmid2: Option<PixMap>,
    /// `mapback`: the minimap ring mask sprite (the per-head scanline
    /// masks in `build_minimap_masks` are built from its `data`).
    pub mapback: Option<Pix8>,
    /// `compass`/`mapedge`/`mapmarker`/`mapdots` from TS maininit
    /// 1006-1063: the minimap sprites.
    pub compass: Option<Pix32>,
    pub mapedge: Option<Pix32>,
    pub mapmarker1: Option<Pix32>,
    pub mapmarker2: Option<Pix32>,
    pub mapdots1: Option<Pix32>,
    pub mapdots2: Option<Pix32>,
    pub mapdots3: Option<Pix32>,
    pub mapdots4: Option<Pix32>,
    /// Java's 100-slot `mapscene`/`mapfunction` tables:
    /// minimap wall/scene icons.
    pub mapscene: Vec<Option<Pix8>>,
    pub mapfunction: Vec<Option<Pix32>>,
    /// Revision-377 sprite tables with Java's distinct capacities/indexes.
    pub sprites_377: Option<Media377>,
}

impl Media {
    /// All `None`/empty: the placeholder renderers hold before the first
    /// frame depacked the real copy (a never-drawn renderer must not hold
    /// a per-head heap copy).
    pub fn empty() -> Media {
        Media {
            p11: None,
            p12: None,
            b12: None,
            q8: None,
            title_dat: None,
            title_dat_flipped: None,
            logo: None,
            titlebox: None,
            titlebutton: None,
            runes: Vec::new(),
            cross: [const { None }; 8],
            hitmarks: [const { None }; 20],
            headicons: [const { None }; 20],
            headicons_hint: [const { None }; 32],
            invback: None,
            chatback: None,
            backbase1: None,
            backbase2: None,
            backhmid1: None,
            scrollbar1: None,
            scrollbar2: None,
            sideicons: [const { None }; 13],
            mod_icons: [const { None }; 2],
            redstone1: None,
            redstone2: None,
            redstone3: None,
            redstone1h: None,
            redstone2h: None,
            redstone1v: None,
            redstone2v: None,
            redstone3v: None,
            redstone1hv: None,
            redstone2hv: None,
            area_backleft1: None,
            area_backleft2: None,
            area_backright1: None,
            area_backright2: None,
            area_backtop1: None,
            area_backvmid1: None,
            area_backvmid2: None,
            area_backvmid3: None,
            area_backhmid2: None,
            mapback: None,
            compass: None,
            mapedge: None,
            mapmarker1: None,
            mapmarker2: None,
            mapdots1: None,
            mapdots2: None,
            mapdots3: None,
            mapdots4: None,
            mapscene: vec![None; 100],
            mapfunction: vec![None; 100],
            sprites_377: None,
        }
    }

    /// The shared empty placeholder (process-wide): a renderer that never
    /// drew shares it instead of owning a private copy.
    pub fn shared_empty() -> Arc<Media> {
        static EMPTY: OnceLock<Arc<Media>> = OnceLock::new();
        EMPTY.get_or_init(|| Arc::new(Media::empty())).clone()
    }

    /// The process copy, keyed by cache dir and explicit revision: the first
    /// head depacks once and every matching head clones the pointer (the
    /// one-copy-of-immutable rule — a second head never re-reads the
    /// jags). A test binary that mixes cache dirs (real pack vs no-pack
    /// temp dirs) gets its own copy per dir instead of poisoning the
    /// shared cell with the first dir's decode.
    pub fn process(cache_dir: &str, revision: ClientRevision) -> Arc<Media> {
        type MediaCell = Mutex<Option<(String, ClientRevision, Arc<Media>)>>;
        static MEDIA: OnceLock<MediaCell> = OnceLock::new();
        let cell = MEDIA.get_or_init(|| Mutex::new(None));
        let mut guard = cell.lock().unwrap();
        if let Some((dir, loaded_revision, media)) = &*guard {
            if dir == cache_dir && *loaded_revision == revision {
                return media.clone();
            }
        }
        let media = Arc::new(Media::depack(cache_dir, revision));
        *guard = Some((cache_dir.to_string(), revision, media.clone()));
        media
    }

    /// The `title` jag fonts and every static `media` jag sprite
    /// (`prepare_game`/`prepare_title` from client-ts, moved to the shared
    /// copy). Runs once per process.
    fn depack(cache_dir: &str, revision: ClientRevision) -> Media {
        let mut media = Media::empty();
        if revision.is_377() {
            media.sprites_377 = Some(Media377::empty());
        }

        // Fonts + title content from the `title` jag, loaded once (TS
        // `maininit` 848 loads the four fonts before both title and game
        // draw). The `catch_unwind` is the pre-existing corrupt-jag guard
        // (`try_title_jag`). The title regions themselves stay per-head
        // (the torch flames paint into them); they re-plot from the shared
        // decodes below.
        if let Ok(bytes) = std::fs::read(format!("{cache_dir}/title")) {
            if let Ok(jag) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| JagFile::new(bytes)))
            {
                media.p11 = PixFont::depack(&jag, "p11_full", false);
                media.p12 = PixFont::depack(&jag, "p12_full", false);
                media.b12 = PixFont::depack(&jag, "b12_full", false);
                media.q8 = PixFont::depack(&jag, "q8_full", true);
                // `loadTitleBackground`: the tiled JPEG (plus the mirrored
                // copy for the second pass) and the logo.
                if let Some(background) = Pix32::from_jpeg(&jag, "title.dat") {
                    let mut flipped = background.clone();
                    flipped.hflip();
                    media.title_dat = Some(background);
                    media.title_dat_flipped = Some(flipped);
                }
                media.logo = Pix32::depack(&jag, "logo", 0);
                // `loadTitleImages`: the titlebox/titlebutton sprites and
                // the 12 runes (`fl_icon` param default 0 → sprites 0..11).
                media.titlebox = Pix8::depack(&jag, "titlebox", 0);
                media.titlebutton = Pix8::depack(&jag, "titlebutton", 0);
                for i in 0..12 {
                    if let Some(rune) = Pix8::depack(&jag, "runes", i) {
                        media.runes.push(rune);
                    }
                }
            }
        }

        // The `media` jag sprites (TS prepareGame 2001 / maininit).
        if let Ok(bytes) = std::fs::read(format!("{cache_dir}/media")) {
            let jag = JagFile::new(bytes);
            media.invback = Pix8::depack(&jag, "invback", 0);
            media.scrollbar1 = Pix8::depack(&jag, "scrollbar", 0);
            media.scrollbar2 = Pix8::depack(&jag, "scrollbar", 1);
            media.chatback = Pix8::depack(&jag, "chatback", 0);
            media.backbase1 = Pix8::depack(&jag, "backbase1", 0);
            media.backbase2 = Pix8::depack(&jag, "backbase2", 0);
            media.backhmid1 = Pix8::depack(&jag, "backhmid1", 0);
            for i in 0..13 {
                media.sideicons[i] = Pix8::depack(&jag, "sideicons", i as i32);
            }
            // `modIcons` as Java 5353-5355 / TS 1095-1097.
            for i in 0..2 {
                media.mod_icons[i] = Pix8::depack(&jag, "mod_icons", i as i32);
            }
            // TS 1056-1058: the click-crosshair frames are Pix32, not Pix8.
            for i in 0..8 {
                media.cross[i] = Pix32::depack(&jag, "cross", i as i32);
            }
            // redstone1..2hv as Client.ts 1068-1093: the flipped copies are
            // fresh depacks of the base sprite, hflip/vflip'd in place.
            media.redstone1 = Pix8::depack(&jag, "redstone1", 0);
            media.redstone2 = Pix8::depack(&jag, "redstone2", 0);
            media.redstone3 = Pix8::depack(&jag, "redstone3", 0);
            media.redstone1h = media.redstone1.clone();
            if let Some(s) = media.redstone1h.as_mut() {
                s.hflip();
            }
            media.redstone2h = media.redstone2.clone();
            if let Some(s) = media.redstone2h.as_mut() {
                s.hflip();
            }
            media.redstone1v = media.redstone1.clone();
            if let Some(s) = media.redstone1v.as_mut() {
                s.vflip();
            }
            media.redstone2v = media.redstone2.clone();
            if let Some(s) = media.redstone2v.as_mut() {
                s.vflip();
            }
            media.redstone3v = media.redstone3.clone();
            if let Some(s) = media.redstone3v.as_mut() {
                s.vflip();
            }
            media.redstone1hv = media.redstone1.clone();
            if let Some(s) = media.redstone1hv.as_mut() {
                s.hflip();
            }
            if let Some(s) = media.redstone1hv.as_mut() {
                s.vflip();
            }
            media.redstone2hv = media.redstone2.clone();
            if let Some(s) = media.redstone2hv.as_mut() {
                s.hflip();
            }
            if let Some(s) = media.redstone2hv.as_mut() {
                s.vflip();
            }
            media.area_backleft1 = Self::chrome_area(&jag, "backleft1");
            media.area_backleft2 = Self::chrome_area(&jag, "backleft2");
            media.area_backright1 = Self::chrome_area(&jag, "backright1");
            media.area_backright2 = Self::chrome_area(&jag, "backright2");
            media.area_backtop1 = Self::chrome_area(&jag, "backtop1");
            media.area_backvmid1 = Self::chrome_area(&jag, "backvmid1");
            media.area_backvmid2 = Self::chrome_area(&jag, "backvmid2");
            media.area_backvmid3 = Self::chrome_area(&jag, "backvmid3");
            media.area_backhmid2 = Self::chrome_area(&jag, "backhmid2");

            // Minimap sprites (TS maininit 1006-1063).
            media.mapback = Pix8::depack(&jag, "mapback", 0);
            media.compass = Pix32::depack(&jag, "compass", 0);
            media.mapedge = Pix32::depack(&jag, "mapedge", 0);
            if let Some(edge) = media.mapedge.as_mut() {
                edge.trim();
            }
            media.mapmarker1 = Pix32::depack(&jag, "mapmarker", 0);
            media.mapmarker2 = Pix32::depack(&jag, "mapmarker", 1);
            media.mapdots1 = Pix32::depack(&jag, "mapdots", 0);
            media.mapdots2 = Pix32::depack(&jag, "mapdots", 1);
            media.mapdots3 = Pix32::depack(&jag, "mapdots", 2);
            media.mapdots4 = Pix32::depack(&jag, "mapdots", 3);

            if revision.is_377() {
                media.sprites_377 = Some(Media377::depack(&jag));
            } else {
                for (i, slot) in media.mapscene.iter_mut().enumerate() {
                    let Some(sprite) = Pix8::depack(&jag, "mapscene", i as i32) else {
                        break;
                    };
                    *slot = Some(sprite);
                }
                for (i, slot) in media.mapfunction.iter_mut().enumerate() {
                    let Some(sprite) = Pix32::depack(&jag, "mapfunction", i as i32) else {
                        break;
                    };
                    *slot = Some(sprite);
                }
            }

            // Java377 loads five hitmarks; legacy loaders retain their 20
            // attempts. The 377 client does not load the bare headicons set.
            let hitmark_count = if revision.is_377() { 5 } else { 20 };
            for i in 0..hitmark_count {
                media.hitmarks[i] = Pix32::depack(&jag, "hitmarks", i as i32);
            }
            if !revision.is_377() {
                for i in 0..20 {
                    media.headicons[i] = Pix32::depack(&jag, "headicons", i as i32);
                }
            }
            if revision.is_377() {
                for i in 0..6 {
                    media.headicons_hint[i] = Pix32::depack(&jag, "headicons_hint", i as i32);
                }
            }
        }

        media
    }

    /// TS 1098 construction for the `areaBack*` strips: a `PixMap` at the
    /// sprite's own size with the sprite `quickPlotSprite`d at (0, 0).
    fn chrome_area(jag: &JagFile, name: &str) -> Option<PixMap> {
        let sprite = Pix32::depack(jag, name, 0)?;
        let mut area = PixMap::new(sprite.wi, sprite.hi);
        let mut surface = Pix2D::with_pixels(&mut area.pixels, area.width, area.height);
        sprite.quick_plot_sprite(&mut surface, 0, 0);
        Some(area)
    }
}

#[cfg(test)]
mod fidelity_tests {
    use super::*;
    use crate::io::synthetic_jag;

    #[test]
    fn fidelity_b10_legacy_loads_high_map_icons_and_retains_empty_tail() {
        for count in [3, 51, 100, 101] {
            let mut index = vec![0, 1, 0, 1, 2, 0x12, 0x34, 0x56];
            for _ in 0..count {
                index.extend([0, 0, 0, 1, 0, 1, 0]);
            }
            let mut data = vec![0, 0];
            data.extend(std::iter::repeat_n(1, count));
            let jag = synthetic_jag(&[
                ("index.dat", &index),
                ("mapscene.dat", &data),
                ("mapfunction.dat", &data),
            ]);
            let dir =
                std::env::temp_dir().join(format!("fidelity-b10-{}-{count}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("media"), jag).unwrap();
            for revision in [ClientRevision::R274, ClientRevision::R289] {
                let media = Media::depack(dir.to_str().unwrap(), revision);
                assert_eq!(media.mapscene.len(), 100);
                assert_eq!(media.mapfunction.len(), 100);
                let slot_bytes = std::mem::size_of_val(media.mapscene.as_slice())
                    + std::mem::size_of_val(media.mapfunction.as_slice());
                assert!(slot_bytes <= 20_160, "shared slots retain 5% headroom");
                println!("Shared map-icon table bytes: {slot_bytes}");
                assert_eq!(media.mapscene.iter().flatten().count(), count.min(100));
                assert_eq!(media.mapfunction.iter().flatten().count(), count.min(100));
                assert_eq!(
                    media.mapscene[count.min(100) - 1].as_ref().unwrap().data,
                    [1]
                );
                assert_eq!(
                    media.mapfunction[count.min(100) - 1].as_ref().unwrap().data,
                    [0x123456]
                );
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
