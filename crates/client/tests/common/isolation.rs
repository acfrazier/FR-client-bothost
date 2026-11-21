use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use client::client::{Client, ClientConfig, ClientRevision};

pub struct IsolatedClient {
    client: Client,
    _cache_dir: EmptyCacheDir,
}

impl IsolatedClient {
    /// Build a client over a newly created empty cache and a closed web origin.
    /// When a test supplies a game server, its endpoint stays open only for the
    /// client protocol; `/crc` still targets a separately allocated closed port.
    pub fn new(revision: ClientRevision, game_server: Option<SocketAddr>) -> Self {
        let cache_dir = EmptyCacheDir::new();
        let (host, port, http_port) = if let Some(address) = game_server {
            (
                address.ip().to_string(),
                address.port(),
                closed_loopback_port(),
            )
        } else {
            let port = closed_loopback_port();
            ("127.0.0.1".into(), port, port)
        };
        let config = ClientConfig {
            host,
            port,
            cache_dir: cache_dir.0.to_string_lossy().into_owned(),
            members: true,
            lowmem: false,
        };
        let client = Client::new_with_revision_and_http_port(config, revision, http_port);
        Self {
            client,
            _cache_dir: cache_dir,
        }
    }
}

impl Deref for IsolatedClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl DerefMut for IsolatedClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.client
    }
}

struct EmptyCacheDir(PathBuf);

impl EmptyCacheDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir();
        for _ in 0..32 {
            let path = base.join(format!(
                "fr-client-test-cache-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated client cache {}: {error}", path.display()),
            }
        }
        panic!("could not allocate a unique isolated client cache directory");
    }
}

impl Drop for EmptyCacheDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn closed_loopback_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a free loopback port");
    listener
        .local_addr()
        .expect("read the free loopback port")
        .port()
}
