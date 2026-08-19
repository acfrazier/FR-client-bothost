use client::client::{Client, ClientConfig};

fn client_tmp() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn draw_progress_headless_sets_fields_without_touching_pixels() {
    let mut c = client_tmp();
    assert!(!c.draw);
    let before = c.draw_area.pixels.clone();
    c.draw_progress("Unpacking media", 80);
    assert_eq!(c.last_progress_percent, 80);
    assert_eq!(c.last_progress_message, "Unpacking media");
    assert_eq!(c.draw_area.pixels, before);
}

#[test]
fn draw_progress_headed_paints_red_bar() {
    let mut c = client_tmp();
    c.set_draw(true);
    c.draw_progress("Loading...", 10);
    assert_eq!(c.last_progress_percent, 10);
    // TS GameShell: fillRect(width/2 - 150, midY+2, progress*3, 30, 0x8c1111)
    let w = c.draw_area.width;
    let h = c.draw_area.height;
    let mid_y = (h / 2) - 18;
    let x = (w / 2) - 150;
    let y = mid_y + 2;
    let idx = (x + y * w) as usize;
    assert_eq!(c.draw_area.pixels[idx], 0x8c1111);
}
