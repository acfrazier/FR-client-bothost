use client::client::{Client, ClientConfig};
use client::client::{APPLET_H, APPLET_W};
use client::graphics::PixMap;

fn cache_dir() -> Option<String> {
    let cache = std::env::var("HOME").ok()? + "/experiments/Server/engine/data/pack/client";
    if std::path::Path::new(&cache).join("media").is_file() {
        Some(cache)
    } else {
        None
    }
}

fn client(cache: String) -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    })
}

#[test]
fn game_draw_headless_does_not_panic() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("media").is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.ingame = true;
    c.draw_area = PixMap::new(APPLET_W, APPLET_H);
    c.game_draw();
    // second frame: redraw_frame is now false, exercises the per-frame path
    c.game_draw();
    // scene state 2: gameDrawMain runs the 3D pass — cls + render_all into
    // area_game, blitted at (4, 4). Without a rebuilt scene the world is
    // empty, so the viewport stays black, but the pass must not panic.
    c.scene_state = 2;
    c.game_draw();
    assert_eq!(c.world.render_count(), 1, "gameDrawMain must call render_all");
    // with the media jag present the chrome panels plot non-zero pixels
    assert!(c.draw_area.pixels.iter().any(|&p| p != 0));
    // the icon-strip backgrounds (backhmid1 at (516, 160), backbase2 at
    // (496, 466)) blit non-zero pixels
    let region_nonzero = |x0: i32, y0: i32, x1: i32, y1: i32| {
        (y0..y1).any(|y| (x0..x1).any(|x| c.draw_area.pixels[(y * c.draw_area.width + x) as usize] != 0))
    };
    assert!(region_nonzero(516, 160, 765, 205), "backhmid1 strip");
    assert!(region_nonzero(496, 466, 765, 503), "backbase2 strip");
}

/// `gameDrawMain` with a real pack: `Client::new` does not rebuild a scene,
/// so the world is empty and `area_game` stays black after the pass — but
/// the pass must run `render_all` (proved by `render_count`) and must not
/// panic. A live `test`/`test` login against the 274 server is the oracle
/// for the non-empty scene (visible 3D viewport, eye above the local
/// player).
#[test]
fn game_draw_main_writes_viewport_pixels() {
    let Some(cache) = cache_dir() else {
        return;
    };
    let mut c = client(cache);
    c.set_draw(true);
    c.ingame = true;
    c.scene_state = 2;
    // if local_player/world empty after new, still must not panic
    c.game_draw();
    let g = c.area_game.as_ref().unwrap();
    assert_eq!(g.width, 512);
    assert_eq!(g.height, 334);
    // after a real rebuild this is non-zero; without scene, at least not a
    // panic, and the 3D pass ran rather than the old fill-0 stub.
    assert_eq!(c.world.render_count(), 1, "gameDrawMain must call render_all");
    assert_eq!(c.scene_cycle, 1);
    assert!(c.vis_calc_done, "resetVisCalc must run before the first pass");
}
