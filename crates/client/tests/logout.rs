//! Task 1: `clientButton` CC_LOGOUT arm (Java `Client.java` 8745-8747).
//! Clicking a control with client code 205 arms `logoutTimer` (250 frames,
//! ~5 s at 20 ms); unported client codes return `true` so the existing
//! unconditional `IF_BUTTON` send is preserved (operator-accepted deferral,
//! 2026-08-20 — the full `clientButton` port is slice 3/5).
use client::client::{Client, ClientConfig};
use client::config::IfType;

fn client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

#[test]
fn cc_logout_arms_logout_timer() {
    let mut c = client();
    let com = IfType {
        client_code: 205,
        ..IfType::default()
    };
    assert!(c.client_button(&com));
    assert_eq!(c.logout_timer, 250);
    let other = IfType {
        client_code: 3,
        ..IfType::default()
    };
    assert!(c.client_button(&other));
    assert_eq!(c.logout_timer, 250); // unchanged for unported codes
}
