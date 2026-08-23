// The `draw` CPU-save switch: `mainredraw` skips the whole frame render
// when `draw` is false, independent of the window (`client-play` sets it
// true after `Present::open`; headless bots on later 50-client hosts keep
// it false). The /tmp cache has no packs, so `Client::new` falls back to
// `Cache::default()` and never touches the network (see hud.rs).
use client::client::{Client, ClientConfig};

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
