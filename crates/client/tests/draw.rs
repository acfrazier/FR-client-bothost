// The `draw` CPU-save switch: `mainredraw` skips the whole frame render
// when `draw` is false, independent of the window (`client-play` sets it
// true after `Present::open`; headless bots on later 50-client hosts keep
// it false). The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and never touches the network (see hud.rs).
use client::client::{Client, ClientConfig, ClientPlayer};

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn headless_new_starts_with_draw_off() {
    let mut c = client();
    assert!(!c.draw);
    let before = c.draw_area.pixels.clone();
    c.ingame = true;
    c.mainredraw();
    assert_eq!(c.draw_area.pixels, before);
    // draw=false skips the frame render entirely: prepare_game never ran
    assert!(c.area_game.is_none(), "mainredraw must skip with draw=false");
}

/// Task 2: a Null client (default `draw=false`) must not own the applet
/// framebuffer (765×503), the 512×512 minimap, or the `prepare_game` HUD
/// areas. `set_draw(true)` / login allocate them.
#[test]
fn null_construct_does_not_own_applet_framebuffer() {
    let c = client();
    assert!(!c.draw);
    assert!(
        (c.draw_area.width as i64) * (c.draw_area.height as i64) <= 1,
        "Null must not own 765×503"
    );
    assert!(
        c.minimap.is_none() || c.minimap.as_ref().unwrap().data.len() <= 1,
        "Null must not own the 512×512 minimap"
    );
    assert!(c.area_game.is_none());
}

#[test]
fn set_draw_true_allows_game_draw_without_present() {
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    c.mainredraw(); // must not panic; prepare_game may run
    assert!(
        c.area_game.is_some(),
        "mainredraw with draw=true runs the game draw"
    );
}

#[test]
fn set_draw_rising_edge_sets_draw_true() {
    let mut c = client();
    assert!(!c.draw);
    c.set_draw(true);
    assert!(c.draw);
    c.set_draw(false);
    assert!(!c.draw);
    // empty map_build_index: the rising edge must not run map_build, and
    // check_scene still reports the empty guard.
    assert_eq!(c.check_scene(), -1000);
}

/// Task 4: a channel tune leaves `scene_state != 2`; a drawn frame must
/// show TV static in the viewport (and minimap) instead of the stock
/// "Loading - please wait." splash or a flat fill.
#[test]
fn channel_change_draws_static_not_splash() {
    let mut c = client();
    c.set_draw(true);
    c.scene_state = 1;
    c.game_draw(); // paints the loading fill
    let g = c.area_game.as_ref().unwrap();
    let uniq = g.pixels.iter().copied().collect::<std::collections::HashSet<_>>().len();
    assert!(uniq > 16, "expected noise, not a flat splash");
    let m = c.area_map.as_ref().unwrap();
    let muniq = m.pixels.iter().copied().collect::<std::collections::HashSet<_>>().len();
    assert!(muniq > 16, "expected noise on the minimap too");
}

/// Task 4 + 4.6d: `draw=false` must not fill — a headless pass with the
/// scene loading leaves the viewport untouched and the minimap with only
/// `prepare_game`'s `mapback` ring (no static).
#[test]
fn draw_off_loading_leaves_no_static() {
    let mut c = client();
    c.scene_state = 1;
    c.game_draw(); // prepare_game allocates, but the fill is draw-gated
    assert!(!c.draw);
    let g = c.area_game.as_ref().unwrap();
    assert!(
        g.pixels.iter().all(|&p| p == 0),
        "draw=false must not fill area_game with static"
    );
    // The `mapback` ring is a fixed sprite (≤ 16 colours); the >16-colour
    // static must be absent with draw off.
    let m = c.area_map.as_ref().unwrap();
    let uniq = m.pixels.iter().copied().collect::<std::collections::HashSet<_>>().len();
    assert!(uniq <= 16, "draw=false must not fill the minimap with static");
}

/// Task 4 review: `scene_static` overwrites the whole minimap (ring
/// included), and `prepare_game` plots the `mapback` ring only once — so a
/// finished scene's `minimap_draw` must re-plot the ring, or the frame
/// stays snow. No media pack means there is no ring to restore (skip).
#[test]
fn minimap_ring_restored_after_static() {
    let mut c = client();
    c.set_draw(true); // prepare_game plots the ring into area_map
    let Some(mb) = c.mapback.as_ref() else {
        return;
    };
    let mb = mb.data.clone();
    let ring_ref = c.area_map.as_ref().unwrap().pixels.clone();
    c.scene_state = 1;
    c.game_draw(); // static fills the whole minimap, ring included
    let snow = c.area_map.as_ref().unwrap();
    let mut snowed = 0;
    for (i, &b) in mb.iter().enumerate() {
        if b != 0 && snow.pixels[i] != ring_ref[i] {
            snowed += 1;
        }
    }
    assert!(snowed > 0, "premise: static must overwrite the ring");
    c.scene_state = 2;
    c.local_player = Some(ClientPlayer::default());
    c.game_draw(); // minimap_draw re-plots the ring before drawing
    let after = c.area_map.as_ref().unwrap();
    let mut mism = 0;
    for (i, &b) in mb.iter().enumerate() {
        if b != 0 && after.pixels[i] != ring_ref[i] {
            mism += 1;
        }
    }
    assert_eq!(mism, 0, "mapback ring must be restored, not leftover snow");
}

/// Task 4 (operator): with SFX on, the loading static also feeds white
/// noise onto the `waves` queue.
#[test]
fn static_sfx_feeds_waves_while_scene_loading() {
    let mut c = client();
    c.set_draw(true);
    c.scene_state = 1;
    c.game_draw(); // scene_static pushes one frame of noise
    let q = c.waves.lock().unwrap();
    assert!(!q.is_empty(), "static SFX must feed the waves queue");
}

/// Task 4 (operator): Null (`draw=false`) and low-memory clients never
/// feed static SFX, and feeding stops once the scene is ready.
#[test]
fn static_sfx_skipped_when_draw_off_lowmem_or_scene_ready() {
    // draw=false: scene_static returns before the audio feed
    let mut c = client();
    c.scene_state = 1;
    c.game_draw();
    assert!(
        c.waves.lock().unwrap().is_empty(),
        "draw=false must not feed waves"
    );

    // lowmem=true: no audio even with draw on
    let mut low = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: true,
    });
    low.set_draw(true);
    low.scene_state = 1;
    low.game_draw();
    assert!(low.waves.lock().unwrap().is_empty(), "lowmem must not feed waves");

    // scene ready: a drawn frame stops the feed
    let mut ready = client();
    ready.set_draw(true);
    ready.scene_state = 1;
    ready.game_draw();
    assert!(!ready.waves.lock().unwrap().is_empty());
    ready.waves.lock().unwrap().clear();
    ready.scene_state = 2;
    ready.game_draw();
    assert!(
        ready.waves.lock().unwrap().is_empty(),
        "no static SFX once the scene is ready"
    );
}
