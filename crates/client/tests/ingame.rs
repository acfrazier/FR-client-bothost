use client::client::{Client, ClientConfig};
use client::client::{APPLET_H, APPLET_W};
use client::graphics::PixMap;

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
    // scene state 2: gameDrawMain is not ported, the viewport stays a black
    // hole blitted at (4, 4)
    c.scene_state = 2;
    c.game_draw();
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
