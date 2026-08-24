use client::client::{Client, ClientConfig};
use client::render::Renderer;

fn new_client() -> Client {
    Client::new(ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    })
}

/// The window close path sets `shell.state = -1` (Present::poll false → the
/// run loop's close contract). `run` must stop the machine on the next
/// frame: the while condition fails and the `state == -1` arm calls `stop()`
/// (state → -2), exactly like GameShell.run.
#[test]
fn run_stops_when_state_negative() {
let mut r = Renderer::new(false);
    let mut c = new_client();
    // This test drives the run-loop stop contract, not the loading screen:
    // skip `maininit` so it stays hermetic (no web fetch into /tmp).
    c.already_started = true;
    let mut on_loop_calls = 0;
    c.run(&mut r, |client| {
        on_loop_calls += 1;
        client.shell.state = -1;
    });
    assert!(on_loop_calls >= 1);
    assert_eq!(c.shell.state, -2); // stop() → shutdown
}
