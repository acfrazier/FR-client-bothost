//! Task 20: `lostCon` reconnect (Java `Client.java` 6147). `lost_con` must
//! re-establish with `login(..., reconnect = true)` (wrapper opcode 18); a
//! pending logout request (`logoutTimer > 0`) logs out instead; the in-game
//! silence watchdog (`timeoutTimer > 750`, ~15 s at 20 ms) drives it.
use client::client::{Client, ClientConfig};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

fn serve_login_success(s: &mut TcpStream) {
    let mut hdr = [0u8; 2];
    s.read_exact(&mut hdr).unwrap();
    for _ in 0..8 {
        let _ = s.write_all(&[0]);
    }
    s.write_all(&[0]).unwrap();
    s.write_all(&[0u8; 8]).unwrap();
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).unwrap();
    assert!(n > 0);
    s.write_all(&[2, 0, 0]).unwrap();
}

fn serve_login_reject(s: &mut TcpStream) {
    let mut hdr = [0u8; 2];
    let _ = s.read_exact(&mut hdr);
    for _ in 0..8 {
        let _ = s.write_all(&[0]);
    }
    let _ = s.write_all(&[6]);
}

/// First connection is a successful cold login; the second is a reconnect
/// rejected with code 6 (so `lost_con` logs out).
fn login_then_reject() -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        serve_login_success(&mut s);
        let (mut s2, _) = listener.accept().unwrap();
        serve_login_reject(&mut s2);
        drop(s);
    });
    (addr, server)
}

#[test]
fn lost_con_uses_reconnect_login() {
    let (addr, server) = login_then_reject();
    let mut c = client();
    c.config.host = addr.ip().to_string();
    c.config.port = addr.port();
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.login_user, "bob");
    c.lost_con();
    // the reestablish attempted login(..., reconnect = true) → opcode 18
    assert_eq!(c.last_login_reconnect, Some(true));
    // the rejecting server made the reestablish fail, so the client logs out
    assert!(!c.ingame);
    assert!(c.login_user.is_empty());
    server.join().unwrap();
}

#[test]
fn lost_con_with_pending_logout_logs_out_without_reconnecting() {
    let mut c = client();
    c.ingame = true;
    c.login_user = "bob".into();
    c.login_pass = "pw".into();
    c.logout_timer = 250;
    c.lost_con();
    assert_eq!(c.last_login_reconnect, None);
    assert!(!c.ingame);
    assert!(c.login_user.is_empty());
}

#[test]
fn silence_watchdog_calls_lost_con_after_750_frames() {
    let (addr, server) = login_then_reject();
    let mut c = client();
    c.config.host = addr.ip().to_string();
    c.config.port = addr.port();
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    // 750 frames without a full packet; the 751st trips the watchdog
    for _ in 0..751 {
        c.game_loop();
    }
    assert_eq!(c.last_login_reconnect, Some(true));
    assert!(!c.ingame);
    server.join().unwrap();
}
