use client::client::Client;
use client::client::ClientConfig;
use client::graphics::PixMap;

#[test]
fn title_draw_writes_pixels() {
    let cache = std::env::var("HOME").unwrap() + "/experiments/Server/engine/data/pack/client";
    if !std::path::Path::new(&cache).join("title").is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.draw_area = PixMap::new(789, 532);
    c.title_screen_draw();
    assert!(c.draw_area.pixels.iter().any(|&p| p != 0));
}
