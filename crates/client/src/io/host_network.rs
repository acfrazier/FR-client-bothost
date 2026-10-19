//! Host-owned game and asset network operations.
//!
//! The embedding process installs one [`HostNetwork`] before admitting any
//! host transport work. Standalone builds may leave it uninstalled and use
//! the native backend; product builds without an installation fail closed.

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crate::client::client::AssetFetchError;
use crate::io::client_stream::LoginCancel;
use crate::Transport;

/// Opaque adapter identity for one host-owned login attempt.
#[derive(Clone)]
pub struct HostAttemptIdentity(Arc<dyn std::any::Any + Send + Sync>);

impl HostAttemptIdentity {
    /// Create an identity from the host-play-owned attempt payload.
    pub fn new<T: std::any::Any + Send + Sync>(payload: T) -> Self {
        Self(Arc::new(payload))
    }

    /// Borrow the typed adapter payload without exposing it to the client.
    pub fn downcast_ref<T: std::any::Any>(&self) -> Option<&T> {
        self.0.as_ref().downcast_ref()
    }
}

/// Opaque identity for an existing host-owned connection binding.
#[derive(Clone)]
pub struct HostStreamIdentity(Arc<dyn std::any::Any + Send + Sync>);

impl HostStreamIdentity {
    /// Create an identity from the adapter-owned stream binding.
    pub fn new<T: std::any::Any + Send + Sync>(payload: T) -> Self {
        Self(Arc::new(payload))
    }

    /// Borrow the typed adapter payload without exposing it to the client.
    pub fn downcast_ref<T: std::any::Any>(&self) -> Option<&T> {
        self.0.as_ref().downcast_ref()
    }
}

/// Opaque, adapter-minted proof that a login stream was sealed before its
/// deadline.
pub type SealReceipt = Box<dyn std::any::Any + Send>;

/// The process has already selected its host network implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyInstalled;

impl std::fmt::Display for AlreadyInstalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("host network is already installed")
    }
}

impl std::error::Error for AlreadyInstalled {}

/// How a host-owned game stream is closed by its client-side owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCloseDisposition {
    /// Abort a login or other uncommitted attempt immediately.
    AbortAttempt,
    /// Close a stream after the client session has committed.
    GracefulSession,
}

/// Typed protocol exit information preserved by a host-owned game stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStreamExit {
    WebSocketClose { code: Option<u16>, reason: String },
    Tls(String),
    WebSocket(String),
}

/// A game byte stream owned by the embedding host's transport service.
pub trait HostGameStream: Send {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize>;
    fn available(&mut self) -> io::Result<usize>;
    fn wait_available(&mut self, need: usize, until: Instant);
    fn write_record(&mut self, record: &[u8]) -> io::Result<()>;
    fn set_deadline(&mut self, deadline: Option<Instant>) -> io::Result<()>;
    /// Return the opaque identity needed to prepare a Rebind on this stream.
    fn binding_identity(&self) -> HostStreamIdentity;
    /// Attach cancellation to the exact attempt represented by `attempt`.
    fn attach_cancel(
        &mut self,
        cancel: &LoginCancel,
        attempt: &HostAttemptIdentity,
    ) -> io::Result<()>;
    /// Synchronously confirm an adopted-stream Rebind before client I/O.
    fn rebind(
        &mut self,
        attempt: &HostAttemptIdentity,
        deadline: Option<Instant>,
    ) -> io::Result<()>;
    fn close(&mut self, disposition: HostCloseDisposition);
    fn take_exit_reason(&mut self) -> Option<HostStreamExit>;
    fn seal_login(&mut self) -> io::Result<SealReceipt>;
    fn commit_login(&mut self, receipt: SealReceipt) -> io::Result<()>;
}

/// A host-owned HTTP fetch whose body becomes available incrementally.
pub trait HostHttpFetch: Send {
    fn poll(&mut self, until: Instant) -> Result<Option<Vec<u8>>, AssetFetchError>;
}

/// A bounded host-owned resolver result. Keep the address list inline until
/// the OnDemand worker needs its owned copy.
#[derive(Clone, Copy, Debug)]
pub struct HostLookupAddresses {
    addresses: [SocketAddr; 8],
    len: u8,
}

impl HostLookupAddresses {
    /// Copy the adapter's bounded resolver result without a heap allocation.
    pub fn from_slice(addresses: &[SocketAddr]) -> Self {
        assert!(
            addresses.len() <= 8,
            "host lookup exceeds resolver capacity"
        );
        let mut result = Self {
            addresses: [SocketAddr::from(([0, 0, 0, 0], 0)); 8],
            len: addresses.len() as u8,
        };
        result.addresses[..addresses.len()].copy_from_slice(addresses);
        result
    }

    pub fn as_slice(&self) -> &[SocketAddr] {
        &self.addresses[..usize::from(self.len)]
    }
}

/// Distinguishes a failed name lookup from cancellation, deadlines and
/// resolver admission failures when selecting another world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostLookupFailure {
    Lookup,
    TimedOut,
    Cancelled,
    Unavailable,
    Busy,
}

/// A single cold-fill DNS attempt, polled only until the caller's step
/// deadline. Real backend timing is absent for cached resolver outcomes.
pub trait HostLookupTicket: Send {
    fn poll(&mut self, until: Instant) -> Result<Option<HostLookupAddresses>, HostLookupFailure>;
    fn backend_timing(&self) -> Option<(Instant, Instant)>;
}

/// The process-scoped host networking implementation.
pub trait HostNetwork: Send + Sync {
    /// Connect with the exact login attempt authorized by host-play.
    fn connect_game(
        &self,
        mode: Transport,
        host: &str,
        port: u16,
        deadline: Option<Instant>,
        cancel: Option<&LoginCancel>,
        attempt: &HostAttemptIdentity,
    ) -> io::Result<Box<dyn HostGameStream>>;

    fn connect_ondemand(
        &self,
        mode: Transport,
        host: &str,
        port: u16,
        resolved: Option<&[SocketAddr]>,
        cancel: Option<&LoginCancel>,
    ) -> io::Result<Box<dyn HostGameStream>>;

    /// Resolve one cold-fill host without blocking a preparation step beyond
    /// its deadline. The returned addresses are reused for all asset fetches
    /// or passed to OnDemand to avoid a second game-host lookup.
    fn start_cold_fill_lookup(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Box<dyn HostLookupTicket>, HostLookupFailure>;

    fn start_http(
        &self,
        mode: Transport,
        host: &str,
        port: u16,
        path: &str,
    ) -> Result<Box<dyn HostHttpFetch>, AssetFetchError>;

    fn start_http_resolved(
        &self,
        mode: Transport,
        host: &str,
        addrs: &[SocketAddr],
        path: &str,
    ) -> Result<Box<dyn HostHttpFetch>, AssetFetchError>;

    fn fetch_http_blocking(
        &self,
        mode: Transport,
        host: &str,
        port: u16,
        path: &str,
        deadline: Option<Instant>,
        cancel: Option<&LoginCancel>,
    ) -> Result<Vec<u8>, AssetFetchError>;

    fn fetch_http_blocking_resolved(
        &self,
        mode: Transport,
        host: &str,
        addrs: &[SocketAddr],
        path: &str,
        deadline: Option<Instant>,
        cancel: Option<&LoginCancel>,
    ) -> Result<Vec<u8>, AssetFetchError>;
}

static HOST_NETWORK: OnceLock<Arc<dyn HostNetwork>> = OnceLock::new();

/// Install the process-wide host network exactly once.
pub fn install_host_network(network: Arc<dyn HostNetwork>) -> Result<(), AlreadyInstalled> {
    HOST_NETWORK.set(network).map_err(|_| AlreadyInstalled)
}

/// Return the process-wide host network, if installed.
pub fn installed_host_network() -> Option<&'static Arc<dyn HostNetwork>> {
    HOST_NETWORK.get()
}
