//! Task 11: login response 6 ("RuneScape has been updated!") refreshes the
//! RSA modulus from the web origin and retries the handshake once, exactly
//! like rs2b0t `loginKey.ts`. This test runs the whole chain against a
//! tiny HTTP/1.0 `/loginkey` stub and a game server that answers 6 then a
//! full grant.

use client::client::{Client, ClientConfig};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

/// Read a request line + headers off the stub socket (a client `write_all`
/// can still be delivered in pieces).
fn read_request(s: &mut std::net::TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let n = s.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    buf
}

/// A fresh 300-digit modulus (rs2b2t's rotated keys are 1024-bit-ish).
fn fresh_key() -> String {
    "9".repeat(300)
}

#[test]
fn login_key_code_6_refreshes_modulus_and_retries_once() {
    let key = fresh_key();

    // Web origin: plain decimal `/loginkey`, HTTP/1.0 (client-ts / rs2b0t
    // `loginKey.ts` fetch the same way).
    let http = TcpListener::bind("127.0.0.1:0").unwrap();
    let http_addr = http.local_addr().unwrap();
    let stub_key = key.clone();
    let http_server = thread::spawn(move || {
        let (mut s, _) = http.accept().unwrap();
        let req = read_request(&mut s);
        assert!(
            String::from_utf8_lossy(&req).contains("/loginkey"),
            "code-6 refresh must GET /loginkey, got: {}",
            String::from_utf8_lossy(&req)
        );
        write!(s, "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\n{stub_key}\r\n").unwrap();
    });

    // Game server: first handshake answers 6, the retried handshake must
    // encrypt with the *refreshed* modulus (125/126-byte RSA block, not
    // the 64/65-byte local default) and grants.
    let game = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = game.local_addr().unwrap();
    let game_server = thread::spawn(move || {
        let (mut s, _) = game.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14); // login server probe
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[6]).unwrap(); // "RuneScape has been updated!"

        let (mut s2, _) = game.accept().unwrap();
        let mut hdr = [0u8; 2];
        s2.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14);
        for _ in 0..8 {
            let _ = s2.write_all(&[0]);
        }
        s2.write_all(&[0]).unwrap(); // response 0 → send seed
        s2.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap(); // g8 seed
        let mut buf = [0u8; 1024];
        let n = s2.read(&mut buf).unwrap();
        assert_eq!(buf[0], 16); // cold login
        let rsa_len = buf[42] as usize;
        assert!(
            rsa_len == 125 || rsa_len == 126,
            "refreshed-modulus RSA block must be 125/126 bytes, got {rsa_len}"
        );
        assert_eq!(n, 2 + 40 + 1 + rsa_len);
        s2.write_all(&[2, 0, 0]).unwrap(); // response 2, staff=0, mouseTrack=0
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.http_port = http_addr.port();
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(
        client::login_rsa::login_modulus(),
        key,
        "the refreshed modulus must be live for later handshakes"
    );
    http_server.join().unwrap();
    game_server.join().unwrap();
}
