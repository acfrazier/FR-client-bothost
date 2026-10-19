//! Task 20: `lostCon` reconnect (Java `Client.java` 6147). `lost_con` must
//! re-establish with `login(..., reconnect = true)` (wrapper opcode 18); a
//! pending logout request (`logoutTimer > 0`) logs out instead; the in-game
//! silence watchdog (wall-clock: `last_response` older than the 15 s
//! `SERVER_TIMEOUT` bound, not 750 pass-counted frames) drives it.

#[path = "support/local_stream.rs"]
mod local_stream;
use client::client::client::SessionExitReason;
#[cfg(feature = "standalone-native-transport")]
use client::client::ClientPlayer;
use client::client::{Client, ClientConfig, ClientRevision};
#[cfg(feature = "standalone-native-transport")]
use client::config::Cache;
#[cfg(feature = "standalone-native-transport")]
use client::io::ServerProt;
use client::io::ServerProt289;
use std::io::{Read, Write};
#[cfg(feature = "standalone-native-transport")]
use std::net::{Shutdown, TcpStream};
use std::net::{SocketAddr, TcpListener};
use std::sync::mpsc;
#[cfg(feature = "standalone-native-transport")]
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[cfg(feature = "standalone-native-transport")]
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

#[cfg(feature = "standalone-native-transport")]
fn serve_login_reject(s: &mut TcpStream) {
    let mut hdr = [0u8; 2];
    let _ = s.read_exact(&mut hdr);
    for _ in 0..8 {
        let _ = s.write_all(&[0]);
    }
    let _ = s.write_all(&[6]);
}

/// Reconnect grant: `response 15` (`Client.java` 3737) after the opcode-18
/// wrapper — the live reconnect success, not the cold-login response 2.
#[cfg(feature = "standalone-native-transport")]
fn serve_login_reconnect15(s: &mut TcpStream) {
    let mut hdr = [0u8; 2];
    s.read_exact(&mut hdr).unwrap();
    assert_eq!(hdr[0], 14);
    for _ in 0..8 {
        let _ = s.write_all(&[0]);
    }
    s.write_all(&[0]).unwrap();
    s.write_all(&[0u8; 8]).unwrap();
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).unwrap();
    assert!(n > 0);
    assert_eq!(buf[0], 18); // reconnect wrapper
    s.write_all(&[15]).unwrap();
}

/// First connection is a successful cold login; the second is a reconnect
/// rejected with code 6 (so `lost_con` logs out).
#[cfg(feature = "standalone-native-transport")]
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

#[cfg(feature = "standalone-native-transport")]
#[test]
fn lost_con_uses_reconnect_login() {
    let (addr, server) = login_then_reject();
    let mut c = client();
    c.config.host = addr.ip().to_string();
    c.config.port = addr.port();
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.gens.session, 1);
    assert_eq!(c.login_user, "bob");
    c.lost_con();
    // the reestablish attempted login(..., reconnect = true) → opcode 18
    assert_eq!(c.last_login_reconnect, Some(true));
    // the rejecting server made the reestablish fail, so the client logs out
    assert!(!c.ingame);
    assert!(c.login_user.is_empty());
    assert_eq!(
        c.gens.session, 1,
        "a rejected reconnect is not a successful session"
    );
    server.join().unwrap();
}

#[test]
fn external_reconnect_owner_defers_the_socket_attempt() {
    let mut c = client();
    c.ingame = true;
    c.login_user = "bob".into();
    c.login_pass = "pw".into();
    c.set_external_reconnect_owner(true);
    c.lost_con();
    assert!(!c.ingame);
    assert_eq!(c.last_login_reconnect, None);
    assert_eq!(c.login_user, "bob");
    assert_eq!(c.login_pass, "pw");
    assert!(c.stream.is_none());
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
    assert_eq!(
        c.take_session_exit_reason(),
        Some(SessionExitReason::ConnectionLost)
    );
}

#[test]
fn transport_loss_with_a_written_idle_request_is_not_an_idle_logout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    c.ingame = true;
    c.stream = Some(local_stream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    let (mut server, _) = listener.accept().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();

    c.shell.idle_cycles = 4500;
    c.game_loop();
    let mut opcode = [0; 1];
    server.read_exact(&mut opcode).unwrap();
    assert_eq!(opcode, [145]);

    c.lost_con();
    assert!(!c.ingame);
    assert_eq!(
        c.take_session_exit_reason(),
        Some(SessionExitReason::ConnectionLost)
    );
}

#[cfg(feature = "standalone-native-transport")]
#[test]
fn silence_watchdog_reconnects_with_response_15() {
    // First connection is a successful cold login; the second is a reconnect
    // granted with response 15, so the game resumes (no logout, no
    // "Unexpected server response").
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        serve_login_success(&mut s);
        let (mut s2, _) = listener.accept().unwrap();
        serve_login_reconnect15(&mut s2);
        // keep the grant socket open while the test asserts
        thread::sleep(Duration::from_millis(500));
        drop(s);
    });
    let mut c = client();
    c.config.host = addr.ip().to_string();
    c.config.port = addr.port();
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.gens.session, 1);
    let p = c.local_player.as_mut().unwrap();
    p.y = 77; // marker: the reconnect must not replace localPlayer
              // Age the watchdog past the fixed wall-clock bound, then one pass
              // fires it — a parked host slot runs one `gameLoop` pass per ~600 ms,
              // so 750 pass-counted frames would take ~450 s, not the ~15 s bound.
    c.last_response = Some(Instant::now() - Duration::from_secs(16));
    c.game_loop();
    assert_eq!(c.last_login_reconnect, Some(true));
    assert!(c.ingame);
    assert!(c.stream.is_some());
    assert_eq!(c.login_user, "bob");
    assert_eq!(c.local_player.as_ref().unwrap().y, 77);
    assert_eq!(
        c.gens.session, 2,
        "response 15 publishes a new successful session identity"
    );
    server.join().unwrap();
}

#[cfg(feature = "standalone-native-transport")]
#[test]
fn silence_watchdog_calls_lost_con_after_wall_clock_silence() {
    let (addr, server) = login_then_reject();
    let mut c = client();
    c.config.host = addr.ip().to_string();
    c.config.port = addr.port();
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    // Age the watchdog past the bound: one silent pass (the parked cadence)
    // trips it, where the old 750-pass count would have needed ~450 s.
    c.last_response = Some(Instant::now() - Duration::from_secs(16));
    c.game_loop();
    assert_eq!(c.last_login_reconnect, Some(true));
    assert!(!c.ingame);
    server.join().unwrap();
}

#[cfg(feature = "standalone-native-transport")]
#[test]
fn short_game_frame_does_not_constrain_later_login_seed() {
    for revision in [ClientRevision::R274, ClientRevision::R289] {
        for (reconnect, adopt) in [(false, false), (true, false), (true, true)] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let (finished, wait_finished) = mpsc::channel();
            let opcode = match revision {
                ClientRevision::R274 => ServerProt::UPDATE_RUNENERGY,
                ClientRevision::R289 => ServerProt289::UPDATE_RUNENERGY,
            };
            let server = thread::spawn(move || {
                let (mut game, _) = listener.accept().unwrap();
                game.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                // A real one-byte payload passes through tcp_in before login
                // reuses its input allocation for the eight-byte server seed.
                game.write_all(&[opcode as u8, 42]).unwrap();
                if adopt {
                    serve_login_reconnect15(&mut game);
                } else {
                    let (mut login, _) = listener.accept().unwrap();
                    login
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    if reconnect {
                        serve_login_reconnect15(&mut login);
                    } else {
                        serve_login_success(&mut login);
                    }
                    let _ = wait_finished.recv_timeout(Duration::from_secs(2));
                }
            });
            let config = || ClientConfig {
                host: addr.ip().to_string(),
                port: addr.port(),
                cache_dir: "/tmp".into(),
                members: true,
                lowmem: false,
            };
            let make_client = || {
                Client::from_shared_with_revision(
                    config(),
                    Arc::new(Cache::default()),
                    Arc::new(vec![]),
                    vec![],
                    revision,
                )
            };
            let mut c = make_client();
            c.stream = Some(local_stream::connect(&addr.ip().to_string(), addr.port()).unwrap());
            c.ptype = -1;
            c.ingame = true;
            c.login_user = "bob".into();
            c.login_pass = "pw".into();
            c.local_player = Some(ClientPlayer::at(5, 5));
            c.local_player.as_mut().unwrap().y = 77;
            let deadline = Instant::now() + Duration::from_secs(2);
            while c.runenergy != 42 {
                assert!(
                    Instant::now() < deadline,
                    "short game packet did not arrive"
                );
                c.tcp_in();
                thread::sleep(Duration::from_millis(1));
            }
            if adopt {
                let mut receiver = make_client();
                receiver.local_player = Some(ClientPlayer::at(5, 5));
                receiver.local_player.as_mut().unwrap().y = 77;
                receiver.adopt_from(&mut c).unwrap();
                c = receiver;
                c.login("bob", "pw", true).unwrap();
            } else if reconnect {
                c.lost_con();
            } else {
                c.logout();
                c.login("bob", "pw", false).unwrap();
            }
            assert!(
                c.ingame,
                "revision {revision:?}, reconnect={reconnect}, adopt={adopt}"
            );
            assert_eq!(c.last_login_reconnect, Some(reconnect));
            assert_eq!(c.revision(), revision);
            if reconnect {
                assert_eq!(c.local_player.as_ref().unwrap().y, 77);
            }
            let _ = finished.send(());
            server.join().unwrap();
        }
    }
}

#[cfg(feature = "standalone-native-transport")]
#[test]
fn peer_closed_game_socket_triggers_lost_con_on_next_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        serve_login_success(&mut s);
        s.shutdown(Shutdown::Both).ok();
        drop(s);
        let (mut s2, _) = listener.accept().unwrap();
        serve_login_reject(&mut s2);
    });
    let mut c = client();
    c.config.host = addr.ip().to_string();
    c.config.port = addr.port();
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert_eq!(c.last_login_reconnect, Some(false));
    let deadline = Instant::now() + Duration::from_millis(200);
    while c.last_login_reconnect != Some(true) && Instant::now() < deadline {
        c.game_loop();
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        c.last_login_reconnect,
        Some(true),
        "EOF on available() must call lost_con without waiting the 15s watchdog"
    );
    assert!(!c.ingame);
    assert_eq!(
        c.take_session_exit_reason(),
        Some(SessionExitReason::ReadEof)
    );
    server.join().unwrap();
}

fn hosted_r289(addr: SocketAddr) -> Client {
    let mut c = Client::new_with_revision(
        ClientConfig {
            host: addr.ip().to_string(),
            port: addr.port(),
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: false,
        },
        ClientRevision::R289,
    );
    c.set_external_reconnect_owner(true);
    c
}

/// DIAG-PROTO-74: the T1 reason carries the frame context the operator
/// log lacked (size, session, frame sequence, the frames before it).
#[test]
fn protocol_error_reason_records_size_session_sequence_and_previous_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (done, wait_done) = mpsc::channel::<()>();
    let energy = ServerProt289::UPDATE_RUNENERGY as u8;
    let server = thread::spawn(move || {
        let (mut game, _) = listener.accept().unwrap();
        for value in 1..=10u8 {
            game.write_all(&[energy, value]).unwrap();
        }
        game.write_all(&[74]).unwrap();
        let _ = wait_done.recv_timeout(Duration::from_secs(2));
    });
    let mut c = hosted_r289(addr);
    c.stream = Some(local_stream::connect(&addr.ip().to_string(), addr.port()).unwrap());
    c.ptype = -1;
    c.ingame = true;
    c.gens.session = 3;
    let deadline = Instant::now() + Duration::from_secs(2);
    while c.ingame {
        assert!(Instant::now() < deadline, "the T1 frame did not arrive");
        c.tcp_in();
        thread::sleep(Duration::from_millis(1));
    }
    let _ = done.send(());
    server.join().unwrap();

    let reason = c.take_session_exit_reason().expect("protocol error reason");
    let SessionExitReason::ProtocolError {
        class,
        last_opcode,
        size,
        session,
        sequence,
        previous,
    } = &reason
    else {
        panic!("expected a protocol error, got {reason:?}");
    };
    assert_eq!((*class, *last_opcode, *size), ("T1", 74, 0));
    assert_eq!((*session, *sequence), (3, 10));
    assert_eq!(
        previous.iter().count(),
        8,
        "the last eight frames before it"
    );
    assert!(previous
        .iter()
        .all(|frame| frame.opcode == energy && frame.size == 1));
    let frames = vec![format!("{energy}/1"); 8].join(" ");
    assert_eq!(
        reason.to_string(),
        format!(
            "protocol_error class=T1 last_opcode=74 size=0 session=3 sequence=10 previous=[{frames}]"
        )
    );
    assert!(!c.session_resumable(), "T1 logs out and clears the map");
}

/// Only an in-world connection loss keeps the map for a reconnect; every
/// path through `logout()` clears it.
#[test]
fn only_an_in_world_connection_loss_is_resumable() {
    let addr: SocketAddr = "127.0.0.1:9".parse().unwrap();

    let mut lost = hosted_r289(addr);
    lost.ingame = true;
    lost.lost_con();
    assert!(lost.session_resumable(), "lost connection");

    let mut silent = hosted_r289(addr);
    silent.ingame = true;
    silent.last_response = Some(Instant::now() - Duration::from_secs(16));
    silent.game_loop();
    assert!(!silent.ingame);
    assert!(silent.session_resumable(), "dead-server deadline");
    assert!(matches!(
        silent.take_session_exit_reason(),
        Some(SessionExitReason::DeadServerDeadline { .. })
    ));

    let mut pending = hosted_r289(addr);
    pending.ingame = true;
    pending.logout_timer = 250;
    pending.last_response = Some(Instant::now() - Duration::from_secs(16));
    pending.game_loop();
    assert!(!pending.ingame);
    assert!(
        !pending.session_resumable(),
        "deadline with a pending logout"
    );

    let mut t1 = hosted_r289(addr);
    t1.ingame = true;
    t1.ptype = 74;
    t1.psize = 0;
    t1.handle_packet(74, &mut client::io::Packet::new(vec![]));
    assert!(!t1.ingame);
    assert!(!t1.session_resumable(), "T1");

    // A resumable session that is then logged out is no longer resumable.
    lost.logout();
    assert!(!lost.session_resumable(), "logout after a lost connection");
}

/// A reconnect grant starts a new session: nothing is left to resume.
#[cfg(feature = "standalone-native-transport")]
#[test]
fn reconnect_grant_ends_the_resumable_state() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        serve_login_reconnect15(&mut s);
        thread::sleep(Duration::from_millis(200));
    });
    let mut c = hosted_r289(addr);
    c.ingame = true;
    c.lost_con();
    assert!(c.session_resumable());
    c.login("bob", "pw", true).unwrap();
    assert!(c.ingame);
    assert!(!c.session_resumable());
    server.join().unwrap();
}
