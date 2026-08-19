use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use client::client::{Client, ClientConfig};
use client::io::Packet;

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

/// One-shot HTTP/1.0 server: replies to a single connection and closes.
fn serve_once(body: Vec<u8>) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        // Read the whole request before responding: closing with unread data
        // in the receive buffer sends RST (not FIN) on macOS, discarding the
        // response the client is waiting for.
        let mut req = Vec::new();
        let mut buf = [0u8; 1024];
        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
            match s.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => req.extend_from_slice(&buf[..n]),
            }
        }
        let resp = [
            b"HTTP/1.0 200 OK\r\nContent-Length: ".as_slice(),
            body.len().to_string().as_bytes(),
            b"\r\n\r\n",
            body.as_slice(),
        ]
        .concat();
        let _ = s.write_all(&resp);
    });
    (port, h)
}

#[test]
fn get_jag_file_crc_hit_skips_http() {
    let dir = std::env::temp_dir().join("274-jag-hit");
    let _ = std::fs::create_dir_all(&dir);
    let bytes = b"not-a-real-jag-but-stable".to_vec();
    std::fs::write(dir.join("title"), &bytes).unwrap();
    let crc = Packet::getcrc(&bytes, 0, bytes.len());
    let mut checksums = [0i32; 9];
    checksums[1] = crc;
    // port 1 will fail if HTTP is attempted
    let got = Client::get_jag_file(dir.to_str().unwrap(), "127.0.0.1", 1, "title", 1, &checksums);
    assert_eq!(got.as_deref(), Some(bytes.as_slice()));
}

#[test]
fn get_jag_file_http_persists() {
    let dir = std::env::temp_dir().join("274-jag-http");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = b"fetched-jag-bytes".to_vec();
    let crc = Packet::getcrc(&bytes, 0, bytes.len());
    let (port, h) = serve_once(bytes.clone());
    let mut checksums = [0i32; 9];
    checksums[1] = crc;
    let got = Client::get_jag_file(dir.to_str().unwrap(), "127.0.0.1", port, "title", 1, &checksums);
    h.join().unwrap();
    assert_eq!(got.as_deref(), Some(bytes.as_slice()));
    assert_eq!(std::fs::read(dir.join("title")).unwrap(), bytes);
}
