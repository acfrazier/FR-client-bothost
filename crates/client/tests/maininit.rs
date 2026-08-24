use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
    let before = c.renderer.draw_area.pixels.clone();
    c.draw_progress("Unpacking media", 80);
    assert_eq!(c.last_progress_percent, 80);
    assert_eq!(c.last_progress_message, "Unpacking media");
    assert_eq!(c.renderer.draw_area.pixels, before);
}

#[test]
fn draw_progress_headed_paints_red_bar() {
    let mut c = client_tmp();
    c.set_draw(true);
    c.draw_progress("Loading...", 10);
    assert_eq!(c.last_progress_percent, 10);
    // TS GameShell: fillRect(width/2 - 150, midY+2, progress*3, 30, 0x8c1111)
    let w = c.renderer.draw_area.width;
    let h = c.renderer.draw_area.height;
    let mid_y = (h / 2) - 18;
    let x = (w / 2) - 150;
    let y = mid_y + 2;
    let idx = (x + y * w) as usize;
    assert_eq!(c.renderer.draw_area.pixels[idx], 0x8c1111);
}

/// Java `Client.messageBox` calls `prepareTitle()` then draws the stage
/// string with `b12`. Without that, headed maininit keeps the GameShell
/// fallback bar (no fonts) and the operator sees a mute red bar.
#[test]
fn draw_progress_headed_paints_stage_text_once_title_jag_exists() {
    let cache = format!(
        "{}/experiments/Server/engine/data/pack/client",
        std::env::var("HOME").unwrap()
    );
    if !std::path::Path::new(&format!("{cache}/title")).is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.set_draw(true);
    c.draw_progress("Loading models - 50%", 70);
    assert!(c.renderer.b12.is_some(), "messageBox prepareTitle loads b12 from title jag");
    assert!(
        c.renderer.draw_area.pixels.iter().any(|&p| p == 0xffffff),
        "stage text must be plotted in white (Java b12.centreString)"
    );
}

/// Loading-bar torch strips: blit the JPEG already in imageTitle0/1
/// (loadTitleBackground). Do not tick TitleFlames here — that ran on
/// Java's flame thread, not inside messageBox.
#[test]
fn draw_progress_headed_paints_torch_columns() {
    let cache = format!(
        "{}/experiments/Server/engine/data/pack/client",
        std::env::var("HOME").unwrap()
    );
    if !std::path::Path::new(&format!("{cache}/title")).is_file() {
        return;
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: cache,
        members: true,
        lowmem: false,
    });
    c.set_draw(true);
    c.draw_progress("Loading models - 50%", 70);
    let w = c.renderer.draw_area.width;
    let any = (0..265).any(|y| {
        (0..128).any(|x| c.renderer.draw_area.pixels[(y * w + x) as usize] != 0)
    });
    assert!(any, "left torch column must not be black during the loading bar");
}

/// Read the full HTTP request (headers) before responding: closing with
/// unread data in the receive buffer sends RST (not FIN) on macOS,
/// discarding the response the client is waiting for. Returns the raw
/// request text (for path logging).
fn drain_request(s: &mut std::net::TcpStream) -> String {
    let mut req = Vec::new();
    let mut buf = [0u8; 1024];
    while !req.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => req.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&req).to_string()
}

fn respond(s: &mut std::net::TcpStream, body: &[u8]) {
    let resp = [
        b"HTTP/1.0 200 OK\r\nContent-Length: ".as_slice(),
        body.len().to_string().as_bytes(),
        b"\r\n\r\n",
        body,
    ]
    .concat();
    let _ = s.write_all(&resp);
}

/// One-shot HTTP/1.0 server: replies to a single connection and closes.
fn serve_once(body: Vec<u8>) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let h = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        drain_request(&mut s);
        respond(&mut s, &body);
    });
    (port, h)
}

/// Accept one connection, bailing after `ms` so a client that makes fewer
/// requests than expected cannot hang the server thread.
fn accept_timeout(
    listener: &TcpListener,
    ms: u64,
) -> Option<(std::net::TcpStream, std::net::SocketAddr)> {
    listener.set_nonblocking(true).ok()?;
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        match listener.accept() {
            Ok(c) => return Some(c),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return None,
        }
    }
}

/// Multi-shot HTTP/1.0 server: serves one request per connection, in
/// `bodies` order (used for `/crc` then the jag GETs). Returns the port,
/// the thread, and the request paths seen.
fn serve_in_order(bodies: Vec<Vec<u8>>) -> (u16, thread::JoinHandle<()>, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let t_seen = seen.clone();
    let h = thread::spawn(move || {
        for body in bodies {
            let Some((mut s, _)) = accept_timeout(&listener, 8000) else {
                break;
            };
            let req = drain_request(&mut s);
            if let Some(path) = req.split_whitespace().nth(1) {
                t_seen.lock().unwrap().push(path.to_string());
            }
            respond(&mut s, &body);
        }
    });
    (port, h, seen)
}

/// The 9×g4 + hash body `get_jag_checksums` expects from `/crc`.
fn crc_body(checksums: &[i32; 9]) -> Vec<u8> {
    let mut body = client::io::Packet::alloc(0);
    for &c in checksums {
        body.p4(c);
    }
    let mut h = 1234i32;
    for &c in checksums {
        h = h.wrapping_shl(1).wrapping_add(c);
    }
    body.p4(h);
    body.data()[..body.pos].to_vec()
}

#[test]
fn maininit_is_oneshot_and_sets_progress_100_on_crc_hit() {
    let dir = std::env::temp_dir().join("274-maininit");
    let _ = std::fs::create_dir_all(&dir);
    let mut checksums = [0i32; 9];
    let names = [
        "title",
        "config",
        "interface",
        "media",
        "versionlist",
        "textures",
        "wordenc",
        "sounds",
    ];
    for (i, name) in names.iter().enumerate() {
        let bytes = format!("{name}-seed").into_bytes();
        std::fs::write(dir.join(name), &bytes).unwrap();
        checksums[i + 1] = Packet::getcrc(&bytes, 0, bytes.len());
    }
    let (port, th) = serve_once(crc_body(&checksums));

    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_str().unwrap().into(),
        members: true,
        lowmem: false,
    });
    c.http_port = port;
    c.maininit();
    th.join().ok();
    assert!(c.already_started);
    assert_eq!(c.last_progress_percent, 100);
    // dummy jags were all CRC hits: files unchanged, no HTTP after /crc
    for (i, name) in names.iter().enumerate() {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        assert_eq!(bytes, format!("{name}-seed").into_bytes());
        assert_eq!(Packet::getcrc(&bytes, 0, bytes.len()), checksums[i + 1]);
    }
    c.maininit(); // oneshot
    assert_eq!(c.last_progress_percent, 100);
}

#[test]
fn maininit_empty_cache_fetches_all_eight_jags_over_http() {
    let dir = std::env::temp_dir().join("274-maininit-empty");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // maininit's GET order (JAG_FETCH), with the checksum slot per pack.
    let fetch = [
        ("title", 1usize),
        ("config", 2),
        ("interface", 3),
        ("media", 4),
        ("textures", 6),
        ("wordenc", 7),
        ("sounds", 8),
        ("versionlist", 5),
    ];
    let mut checksums = [0i32; 9];
    let mut bodies = Vec::new();
    for (name, slot) in fetch {
        // config must be a valid (empty) jag so `load_cache` succeeds and
        // `errorLoading` stays clear; the rest are dummy bytes that the
        // catch_unwind'd unpack paths tolerate.
        let bytes: Vec<u8> = if name == "config" {
            vec![0u8, 0, 6, 0, 0, 6, 0, 0]
        } else {
            format!("{name}-fetched").into_bytes()
        };
        checksums[slot] = Packet::getcrc(&bytes, 0, bytes.len());
        bodies.push(bytes);
    }
    // /crc, then one GET per jag: an empty cache misses everything.
    let mut responses = vec![crc_body(&checksums)];
    responses.extend(bodies.clone());
    let (port, th, seen) = serve_in_order(responses);

    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_str().unwrap().into(),
        members: true,
        lowmem: false,
    });
    c.http_port = port;
    c.fetch_retry_wait = Duration::from_millis(1);
    c.maininit();
    th.join().ok();
    assert!(c.already_started);
    assert!(!c.error_loading);
    assert_eq!(c.last_progress_percent, 100);
    // every jag landed on disk with the served bytes
    for (name, slot) in fetch {
        let bytes = std::fs::read(dir.join(name)).unwrap();
        assert_eq!(
            Packet::getcrc(&bytes, 0, bytes.len()),
            checksums[slot],
            "{name} was persisted with the fetched bytes"
        );
    }
    // /crc plus exactly one GET per jag, all over HTTP
    let paths = seen.lock().unwrap().clone();
    assert_eq!(paths.len(), 9);
    assert_eq!(paths[0], "/crc");
    for (name, _) in fetch {
        assert!(
            paths.iter().any(|p| p.starts_with(&format!("/{name}"))),
            "missing HTTP GET for {name}"
        );
    }
}

#[test]
fn maininit_retries_jag_get_after_crc_mismatch() {
    let dir = std::env::temp_dir().join("274-maininit-retry");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // title's on-disk bytes are stale, so its served checksum (for `fresh`)
    // forces an HTTP fetch; every other jag is a CRC hit.
    let fresh = b"fresh-title".to_vec();
    let mut checksums = [0i32; 9];
    let names = [
        "title",
        "config",
        "interface",
        "media",
        "versionlist",
        "textures",
        "wordenc",
        "sounds",
    ];
    for (i, name) in names.iter().enumerate() {
        let bytes = if *name == "title" {
            b"stale-title".to_vec()
        } else {
            format!("{name}-seed").into_bytes()
        };
        std::fs::write(dir.join(name), &bytes).unwrap();
        checksums[i + 1] = if *name == "title" {
            Packet::getcrc(&fresh, 0, fresh.len())
        } else {
            Packet::getcrc(&bytes, 0, bytes.len())
        };
    }
    // /crc, then two title GETs: a wrong-CRC body first (the client must
    // discard the bytes and retry), then the correct ones.
    let (port, th, seen) = serve_in_order(vec![
        crc_body(&checksums),
        b"wrong-crc-bytes".to_vec(),
        fresh.clone(),
    ]);

    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_str().unwrap().into(),
        members: true,
        lowmem: false,
    });
    c.http_port = port;
    c.fetch_retry_wait = Duration::from_millis(1);
    c.maininit();
    th.join().ok();
    let title_gets = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.starts_with("/title"))
        .count();
    assert_eq!(title_gets, 2, "CRC mismatch must discard and retry the fetch");
    assert!(c.already_started);
    assert_eq!(c.last_progress_percent, 100);
    // the retried fetch persisted the fresh title
    assert_eq!(std::fs::read(dir.join("title")).unwrap(), fresh);
}

#[test]
fn maininit_clears_error_loading_from_new_unpack() {
    let dir = std::env::temp_dir().join("274-maininit-recover");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A jag container with zero files parses as a valid (empty) config.
    let valid_config = vec![0u8, 0, 6, 0, 0, 6, 0, 0];
    let mut checksums = [0i32; 9];
    let names = [
        "title",
        "config",
        "interface",
        "media",
        "versionlist",
        "textures",
        "wordenc",
        "sounds",
    ];
    for (i, name) in names.iter().enumerate() {
        let bytes = if *name == "config" {
            b"garbage-config".to_vec()
        } else {
            format!("{name}-seed").into_bytes()
        };
        std::fs::write(dir.join(name), &bytes).unwrap();
        checksums[i + 1] = if *name == "config" {
            Packet::getcrc(&valid_config, 0, valid_config.len())
        } else {
            Packet::getcrc(&bytes, 0, bytes.len())
        };
    }
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: dir.to_str().unwrap().into(),
        members: true,
        lowmem: false,
    });
    assert!(c.error_loading, "invalid config on disk sets errorLoading in new");
    // /crc, then /config{crc}: the only HTTP fetch, all other jags are hits.
    let (port, th, _seen) = serve_in_order(vec![crc_body(&checksums), valid_config.clone()]);
    c.http_port = port;
    c.fetch_retry_wait = Duration::from_millis(1);
    c.maininit();
    th.join().ok();
    assert!(
        !c.error_loading,
        "maininit repairing config must clear errorLoading"
    );
    assert!(c.already_started);
    assert_eq!(c.last_progress_percent, 100);
    assert_eq!(std::fs::read(dir.join("config")).unwrap(), valid_config);
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
