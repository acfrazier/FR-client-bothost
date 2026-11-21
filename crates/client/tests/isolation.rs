#[path = "common/isolation.rs"]
mod isolation;

use client::client::ClientRevision;
use isolation::IsolatedClient;
use std::time::{Duration, Instant};

#[test]
fn constructor_fast_closes_the_isolated_crc_origin() {
    let started = Instant::now();
    let client = IsolatedClient::new(ClientRevision::R274, None);
    let elapsed = started.elapsed();

    assert!(
        client.accepted_http_connections() > 0,
        "the client must request /crc from the isolated origin"
    );
    eprintln!("isolated client constructor elapsed: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(1),
        "isolated /crc origin did not close promptly: {elapsed:?}"
    );
}
