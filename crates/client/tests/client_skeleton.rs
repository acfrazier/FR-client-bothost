use client::client::{Client, ClientConfig, LoginError};

#[test]
fn config_has_no_rsa_fields() {
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
    let e = LoginError { code: 6, mes1: "invalid".into(), mes2: "rsa".into() };
    assert_eq!(e.code, 6);
}

#[test]
fn new_client_starts_logged_out() {
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
