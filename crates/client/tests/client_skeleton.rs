use std::io::Read;
use std::net::TcpListener;

use client::client::{Client, ClientConfig, LoginError, LoginStage};
use client::render::Renderer;

#[test]
fn config_has_no_rsa_fields() {
    let _r = Renderer::new(false);
    let cfg = ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    };
    let _ = cfg.host;
}

#[test]
fn login_error_carries_code_and_messages() {
    let _r = Renderer::new(false);
    let e = LoginError::response(6, "invalid", "rsa");
    assert_eq!(e.code, 6);
    assert!(
        e.transport.is_none(),
        "a server answer is not a socket failure"
    );
}

fn client_for(port: u16) -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// A refused connection keeps where it failed and the OS error, instead of
/// a bare code -1.
#[test]
fn refused_login_keeps_the_connect_stage_and_os_error() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let mut c = client_for(port);
    let e = c.login("alice", "secret", false).unwrap_err();
    assert_eq!(e.code, -1);
    assert_eq!(e.mes2, "Error connecting to server.");
    let receipt = e.transport.expect("socket failure receipt");
    assert_eq!(receipt.stage, LoginStage::Connect);
    assert_eq!(receipt.kind, std::io::ErrorKind::ConnectionRefused);
    assert!(receipt.errno.is_some(), "the OS error number is kept");
    assert!(!receipt.detail.contains("secret"));
}

/// A server that hangs up before answering is told apart from a refusal.
#[test]
fn server_hangup_before_answer_keeps_the_read_stage() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        // Take the login-server request, then hang up without answering.
        let mut hello = [0u8; 2];
        socket.read_exact(&mut hello).unwrap();
        assert_eq!(hello[0], 14);
        drop(socket);
    });
    let mut c = client_for(port);
    let e = c.login("alice", "secret", false).unwrap_err();
    server.join().unwrap();
    assert_eq!(e.code, -1);
    let receipt = e.transport.expect("hang-up receipt");
    assert_eq!(receipt.stage, LoginStage::ReadHello);
    assert_eq!(receipt.kind, std::io::ErrorKind::UnexpectedEof);
    assert_eq!(receipt.errno, None);
}

#[test]
fn new_client_starts_logged_out() {
    let _r = Renderer::new(false);
    let c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    assert!(!c.ingame);
    assert_eq!(c.loop_cycle, 0);
}

#[test]
fn run_drives_mainloop_and_hook_until_stopped() {
    let mut r = Renderer::new(false);
    let mut c = Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    // This test drives the run-loop mechanics, not the loading screen:
    // skip `maininit` so it stays hermetic (no web fetch into /tmp).
    c.already_started = true;
    let mut calls = 0;
    c.run(&mut r, |c| {
        calls += 1;
        if calls >= 3 {
            c.shell.state = -1;
        }
    });
    // one mainloop pass per frame; a negative state stops the machine
    assert_eq!(calls, 3);
    assert_eq!(c.shell.state, -2);
    assert_eq!(c.loop_cycle, 3);
}
