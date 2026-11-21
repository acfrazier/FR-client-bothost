// Shared by unit and integration test targets, which use different subsets.
#![allow(dead_code)]

use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct IsolatedEnvironment {
    cache_dir: EmptyCacheDir,
    loopback: FastCloseLoopback,
    game_host: String,
    game_port: u16,
}

impl IsolatedEnvironment {
    pub fn new(game_server: Option<SocketAddr>) -> Self {
        let cache_dir = EmptyCacheDir::new();
        let loopback = FastCloseLoopback::new();
        let (game_host, game_port) = game_server
            .map(|address| (address.ip().to_string(), address.port()))
            .unwrap_or_else(|| ("127.0.0.1".into(), loopback.port()));

        Self {
            cache_dir,
            loopback,
            game_host,
            game_port,
        }
    }

    pub fn cache_dir(&self) -> String {
        self.cache_dir.0.to_string_lossy().into_owned()
    }

    pub fn game_host(&self) -> &str {
        &self.game_host
    }

    pub fn game_port(&self) -> u16 {
        self.game_port
    }

    pub fn http_port(&self) -> u16 {
        self.loopback.port()
    }

    pub fn accepted_http_connections(&self) -> usize {
        self.loopback.accepted_connections()
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

struct FastCloseLoopback {
    address: SocketAddr,
    accepted: Arc<AtomicUsize>,
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl FastCloseLoopback {
    fn new() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind isolated loopback origin");
        let address = listener
            .local_addr()
            .expect("read isolated loopback origin address");
        let (stop, stop_receiver) = mpsc::channel();
        let accepted = Arc::new(AtomicUsize::new(0));
        let worker_accepted = Arc::clone(&accepted);
        let worker = thread::spawn(move || {
            while let Ok((stream, _)) = listener.accept() {
                worker_accepted.fetch_add(1, Ordering::Relaxed);
                drop(stream);
                match stop_receiver.try_recv() {
                    Ok(()) | Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                }
            }
        });

        Self {
            address,
            accepted,
            stop,
            worker: Some(worker),
        }
    }

    fn port(&self) -> u16 {
        self.address.port()
    }

    fn accepted_connections(&self) -> usize {
        self.accepted.load(Ordering::Relaxed)
    }
}

impl Drop for FastCloseLoopback {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
