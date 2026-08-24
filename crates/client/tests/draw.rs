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
    let before = c.renderer.draw_area.pixels.clone();
    c.ingame = true;
    c.mainredraw();
    assert_eq!(c.renderer.draw_area.pixels, before);
    // draw=false skips the frame render entirely: prepare_game never ran
    assert!(c.renderer.area_game.is_none(), "mainredraw must skip with draw=false");
}

#[test]
fn set_draw_true_allows_game_draw_without_present() {
    let mut c = client();
    c.set_draw(true);
    c.ingame = true;
    c.mainredraw(); // must not panic; prepare_game may run
    assert!(
        c.renderer.area_game.is_some(),
        "mainredraw with draw=true runs the game draw"
    );
}
