//! Game stream: Java TCP locally, WSS (`binary` subprotocol) on Prod.
//!
//! Local reads are blocking on the calling thread with a 30 s soTimeout;
//! writes go through a 5000-byte ring buffer drained by a dedicated writer
//! thread, as in Java. Prod wraps the same byte stream in a WebSocket. A peer
//! Close or TLS/WebSocket protocol failure is an IOException-equivalent
//! transport loss: an in-game standalone client enters `lostCon` immediately,
//! while a close during login reports the ordinary connection error. After
//! local `close` (`dummy`), reads report 0 / EOF and writes are no-ops.
//!
//! A login handshake can carry a deadline ([`ClientStream::connect_until`],
//! [`ClientStream::set_deadline`]): the connect, the TLS and WebSocket setup
//! and every read and write stop at it with `TimedOut`, however much
//! WebSocket control traffic keeps arriving and whatever data is already
//! buffered. While the deadline is set, TCP writes go straight to the socket
//! on the caller's thread, each armed with the time left, not through the
//! writer thread. A [`LoginCancel`] lets another thread shut the handshake's
//! socket, so a peer sees it close at once.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
#[cfg(test)]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::io::{AsRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, RawSocket};

use native_tls::TlsStream;
use tungstenite::protocol::WebSocket;
use tungstenite::Message;

use crate::Transport;

const BUF_SIZE: usize = 5000;
/// Socket read timeout: the original client's `setSoTimeout(30000)`
/// (`jagex2/io/ClientStream.java:47`).
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// `available()` peek buffer; larger than any 274 `psize` (variable-size
/// packets read their length via `g2` and never exceed a few KiB).
const AVAILABLE_BUF: usize = 8192;

/// Transport-level session exit captured before `io::Error` erases protocol
/// details. The client takes this once when it classifies a disconnected
/// session for its embedding host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StreamExitReason {
    WebSocketClose { code: Option<u16>, reason: String },
    Tls(String),
    WebSocket(String),
}

struct WriterState {
    buf: Box<[u8; BUF_SIZE]>,
    tcycl: usize,
    tnum: usize,
    writer: bool,
    ioerror: bool,
    dummy: bool,
}

impl WriterState {
    fn new() -> Self {
        WriterState {
            buf: Box::new([0; BUF_SIZE]),
            tcycl: 0,
            tnum: 0,
            writer: false,
            ioerror: false,
            dummy: false,
        }
    }
}

struct TcpInner {
    reader: TcpStream,
    writer_sock: TcpStream,
    shared: Arc<Mutex<WriterState>>,
    condvar: Arc<Condvar>,
    writer_thread: Option<JoinHandle<()>>,
    /// Login handshake deadline. While it is set, reads and writes run on
    /// the caller's thread, each socket call armed with the time left
    /// ([`tcp_write_until`]).
    deadline: Option<Instant>,
}

/// Error for socket I/O attempted at or after a login deadline.
fn deadline_passed() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "login handshake deadline passed")
}

/// Socket timeout for the next call before `deadline`, never above one
/// read timeout; an error once the deadline has passed.
fn time_left(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(deadline_passed());
    }
    Ok(left.min(READ_TIMEOUT))
}

/// How the embedding host runs a socket call that can block its thread;
/// see [`set_transport_wait_hook`].
static TRANSPORT_WAIT: OnceLock<fn(&mut dyn FnMut())> = OnceLock::new();

/// Run every socket call that can block the calling thread for a long time
/// (a send waiting for the peer to take the bytes, a read waiting for them
/// to arrive, a close) through `hook`, which calls its argument once. A
/// host uses it to tell time its thread waited on the network from time
/// the OS kept it off the CPU. Set once per process; later calls do
/// nothing.
pub fn set_transport_wait_hook(hook: fn(&mut dyn FnMut())) {
    let _ = TRANSPORT_WAIT.set(hook);
}

/// `wait`, run through the [`set_transport_wait_hook`] hook if there is one.
fn transport_wait<R>(wait: impl FnOnce() -> R) -> R {
    let Some(hook) = TRANSPORT_WAIT.get() else {
        return wait();
    };
    let mut wait = Some(wait);
    let mut out = None;
    hook(&mut || {
        if let Some(wait) = wait.take() {
            out = Some(wait());
        }
    });
    match (out, wait) {
        (Some(out), _) => out,
        // The hook didn't call it: the call still has to happen.
        (None, Some(wait)) => wait(),
        (None, None) => unreachable!("the wait ran, so it returned"),
    }
}

/// A socket timeout that fired at the deadline reports the deadline
/// (Linux reports an expired `SO_RCVTIMEO` as `WouldBlock`).
fn timeout_at_deadline(deadline: Option<Instant>, error: io::Error) -> io::Error {
    let timed_out = matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    );
    match deadline {
        Some(deadline) if timed_out && Instant::now() >= deadline => deadline_passed(),
        _ => error,
    }
}

/// A TLS or WebSocket setup step that failed once the deadline had passed
/// failed because of it (the libraries flatten the socket error to text).
fn setup_error(deadline: Option<Instant>, error: io::Error) -> io::Error {
    match deadline {
        Some(deadline) if Instant::now() >= deadline => deadline_passed(),
        _ => error,
    }
}

/// Error for handshake I/O after its [`LoginCancel`] fired.
fn login_cancelled() -> io::Error {
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "login cancelled by its host",
    )
}

/// Stops one login handshake's transport from another thread. An embedding
/// host that admits one handshake at a time cancels an attempt whose turn
/// ran out, so a hung attempt cannot keep a connection to the server open
/// once the next login starts. The stream holds the handle only during its
/// handshake: a session that completed its login is never touched. The
/// bot-host cold-fill OnDemand worker uses it the same way for its update
/// server handshake ([`ClientStream::connect_cancellable`]).
#[derive(Debug, Clone, Default)]
pub struct LoginCancel(Arc<Mutex<CancelState>>);

#[derive(Debug, Default)]
struct CancelState {
    cancelled: bool,
    /// A second handle on the handshake's socket, while the handshake runs.
    socket: Option<TcpStream>,
}

impl LoginCancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shut the handshake's socket in both directions now: the server reads
    /// end of stream, and a read or write blocked on the socket returns. A
    /// socket the handshake connects later is shut as it connects. The
    /// handshake's I/O then fails with "login cancelled by its host".
    pub fn cancel(&self) {
        let mut state = self.state();
        state.cancelled = true;
        if let Some(socket) = state.socket.take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.state().cancelled
    }

    fn state(&self) -> std::sync::MutexGuard<'_, CancelState> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Hold a handle on `socket` until [`Self::detach`]; an already
    /// cancelled handshake shuts it and fails.
    fn attach(&self, socket: &TcpStream) -> io::Result<()> {
        let mut state = self.state();
        if state.cancelled {
            let _ = socket.shutdown(Shutdown::Both);
            return Err(login_cancelled());
        }
        state.socket = Some(socket.try_clone()?);
        Ok(())
    }

    /// The handshake is over: a later cancel must not touch its socket.
    fn detach(&self) {
        self.state().socket = None;
    }
}

/// The TCP socket under a WebSocket. With a deadline, each read and write
/// is armed with the time left, so no single call, and no run of
/// WebSocket control frames, outlasts it.
#[derive(Debug)]
struct DeadlineSocket {
    sock: TcpStream,
    deadline: Option<Instant>,
}

impl DeadlineSocket {
    fn set_deadline(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        self.deadline = deadline;
        if deadline.is_none() {
            self.sock.set_read_timeout(Some(READ_TIMEOUT))?;
            self.sock.set_write_timeout(None)?;
        }
        Ok(())
    }
}

impl Read for DeadlineSocket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            self.sock.set_read_timeout(Some(time_left(deadline)?))?;
        }
        self.sock
            .read(buf)
            .map_err(|error| timeout_at_deadline(self.deadline, error))
    }
}

impl Write for DeadlineSocket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            self.sock.set_write_timeout(Some(time_left(deadline)?))?;
        }
        self.sock
            .write(buf)
            .map_err(|error| timeout_at_deadline(self.deadline, error))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sock.flush()
    }
}

#[cfg(test)]
struct WriteOnceWouldBlock {
    socket: TcpStream,
    block_next_write: Arc<AtomicBool>,
    blocked_writes: Arc<AtomicU64>,
}

#[cfg(test)]
impl Read for WriteOnceWouldBlock {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.read(buf)
    }
}

#[cfg(test)]
impl Write for WriteOnceWouldBlock {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.block_next_write.swap(false, Ordering::Relaxed) {
            self.blocked_writes.fetch_add(1, Ordering::Relaxed);
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.socket.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

/// Underlying WS transport. Production is TLS; plain TCP is test-only loopback
/// so regressions do not need a production TLS trust bypass.
// The test-only `Plain` variant makes the enum two-variant, and Windows'
// TLS stream is far larger than the plain one. Boxing `Tls` would add an
// allocation to the production path for a test-build-only lint.
#[cfg_attr(any(test, feature = "test-support"), allow(clippy::large_enum_variant))]
enum WsConn {
    Tls(WebSocket<TlsStream<DeadlineSocket>>),
    #[cfg(test)]
    BlockedWriter(WebSocket<WriteOnceWouldBlock>),
    /// Loopback plain WS used by unit tests only (no TLS trust bypass).
    #[cfg(any(test, feature = "test-support"))]
    Plain(WebSocket<DeadlineSocket>),
}

impl WsConn {
    // tungstenite::Error is large (~136B); boxing would allocate on the hot WS path.
    #[allow(clippy::result_large_err)]
    fn read_message(&mut self) -> Result<Message, tungstenite::Error> {
        match self {
            WsConn::Tls(ws) => ws.read(),
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => ws.read(),
            #[cfg(test)]
            WsConn::BlockedWriter(ws) => ws.read(),
        }
    }

    #[allow(clippy::result_large_err)]
    fn send_message(&mut self, msg: Message) -> Result<(), tungstenite::Error> {
        match self {
            WsConn::Tls(ws) => ws.send(msg),
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => ws.send(msg),
            #[cfg(test)]
            WsConn::BlockedWriter(ws) => ws.send(msg),
        }
    }

    #[allow(clippy::result_large_err)]
    fn flush(&mut self) -> Result<(), tungstenite::Error> {
        match self {
            WsConn::Tls(ws) => ws.flush(),
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => ws.flush(),
            #[cfg(test)]
            WsConn::BlockedWriter(ws) => ws.flush(),
        }
    }

    #[allow(clippy::result_large_err)]
    fn close(&mut self) -> Result<(), tungstenite::Error> {
        match self {
            WsConn::Tls(ws) => ws.close(None),
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => ws.close(None),
            #[cfg(test)]
            WsConn::BlockedWriter(ws) => ws.close(None),
        }
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        match self {
            WsConn::Tls(ws) => ws.get_ref().get_ref().sock.set_nonblocking(nonblocking),
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => ws.get_ref().sock.set_nonblocking(nonblocking),
            #[cfg(test)]
            WsConn::BlockedWriter(ws) => ws.get_ref().socket.set_nonblocking(nonblocking),
        }
    }

    fn deadline(&self) -> Option<Instant> {
        match self {
            WsConn::Tls(ws) => ws.get_ref().get_ref().deadline,
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => ws.get_ref().deadline,
            #[cfg(test)]
            WsConn::BlockedWriter(_) => None,
        }
    }

    /// The TCP socket under the WebSocket.
    fn socket(&self) -> &TcpStream {
        match self {
            WsConn::Tls(ws) => &ws.get_ref().get_ref().sock,
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => &ws.get_ref().sock,
            #[cfg(test)]
            WsConn::BlockedWriter(ws) => &ws.get_ref().socket,
        }
    }

    fn set_deadline(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        match self {
            WsConn::Tls(ws) => ws.get_mut().get_mut().set_deadline(deadline),
            #[cfg(any(test, feature = "test-support"))]
            WsConn::Plain(ws) => ws.get_mut().set_deadline(deadline),
            #[cfg(test)]
            WsConn::BlockedWriter(_) => Ok(()),
        }
    }
}

struct WsInner {
    ws: Mutex<WsConn>,
    leftover: Mutex<VecDeque<u8>>,
    dummy: Mutex<bool>,
    pending_exit: Mutex<Option<StreamExitReason>>,
    /// Underlying TCP handle for zero-time readability probes (WSS).
    #[cfg(unix)]
    fd: RawFd,
    #[cfg(windows)]
    socket: RawSocket,
}

enum Inner {
    Tcp(TcpInner),
    Ws(Box<WsInner>),
}

pub struct ClientStream {
    inner: Inner,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    /// The handshake's [`LoginCancel`], until the handshake ends.
    cancel: Option<LoginCancel>,
}

fn io_other(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

impl ClientStream {
    /// Connect an unbound standalone client over plain TCP.
    pub fn connect(host: &str, port: u16) -> io::Result<ClientStream> {
        Self::connect_tcp(host, port)
    }

    /// Connect using the explicit frozen session transport.
    pub fn connect_for(transport: Transport, host: &str, port: u16) -> io::Result<ClientStream> {
        if transport.uses_tls() {
            return Self::connect_wss(host, port);
        }
        Self::connect_tcp(host, port)
    }

    /// Connect for one login handshake that must stop at `deadline`: the
    /// TCP connect, and for WSS the TLS and WebSocket setup, are bounded by
    /// it, and so is every later read and write until [`Self::set_deadline`]
    /// clears it. Name resolution is the system resolver's and cannot be
    /// interrupted, but no connect starts after the deadline. `cancel`
    /// shuts the socket from another thread from the moment it connects
    /// until the deadline is cleared.
    pub fn connect_until(
        transport: Transport,
        host: &str,
        port: u16,
        deadline: Instant,
        cancel: Option<&LoginCancel>,
    ) -> io::Result<ClientStream> {
        let socket = connect_socket_until(host, port, deadline)?;
        Self::open_handshake(transport, socket, host, port, Some(deadline), cancel)
    }

    /// Connect for a handshake that `cancel` can stop from another thread at
    /// every point until [`Self::set_deadline`]`(None)` ends it. The TCP
    /// connect waits in slices of at most 10 ms and stops between them once
    /// `cancel` fired. After the connect, `cancel` shuts the socket, which
    /// ends a blocked TLS or WebSocket setup or handshake read. There is no
    /// deadline: otherwise every wait is that of [`Self::connect_for`]. Name
    /// resolution is the system resolver's and cannot be interrupted.
    pub fn connect_cancellable(
        transport: Transport,
        host: &str,
        port: u16,
        cancel: &LoginCancel,
    ) -> io::Result<ClientStream> {
        const SLICE: Duration = Duration::from_millis(10);
        let mut connect = PendingConnect::start((host, port).to_socket_addrs()?.collect());
        let socket = loop {
            if cancel.is_cancelled() {
                return Err(login_cancelled());
            }
            if let Some(socket) = connect.poll(Instant::now() + SLICE)? {
                break socket;
            }
        };
        Self::open_handshake(transport, socket, host, port, None, Some(cancel))
    }

    /// Set up the stream over a connected `socket`, with `cancel` attached
    /// from before the TLS or WebSocket setup until the handshake ends.
    fn open_handshake(
        transport: Transport,
        socket: TcpStream,
        host: &str,
        port: u16,
        deadline: Option<Instant>,
        cancel: Option<&LoginCancel>,
    ) -> io::Result<ClientStream> {
        if let Some(cancel) = cancel {
            cancel.attach(&socket)?;
        }
        let opened = if transport.uses_tls() {
            Self::wss_over(socket, host, port, deadline)
        } else {
            Self::tcp_over(socket, deadline)
        };
        match (opened, cancel) {
            (Ok(mut stream), Some(cancel)) => {
                stream.cancel = Some(cancel.clone());
                Ok(stream)
            }
            (Ok(stream), None) => Ok(stream),
            (Err(error), Some(cancel)) => {
                cancel.detach();
                Err(if cancel.is_cancelled() {
                    login_cancelled()
                } else {
                    error
                })
            }
            (Err(error), None) => Err(error),
        }
    }

    /// Let `cancel` shut this stream's socket from another thread until the
    /// deadline is cleared (a login over an adopted stream).
    pub fn attach_cancel(&mut self, cancel: &LoginCancel) -> io::Result<()> {
        match &self.inner {
            Inner::Tcp(t) => cancel.attach(&t.reader)?,
            Inner::Ws(w) => cancel.attach(w.ws.lock().unwrap().socket())?,
        }
        self.cancel = Some(cancel.clone());
        Ok(())
    }

    /// Set or clear (`None`) the deadline for this stream's reads and
    /// writes. Clearing ends the handshake: it restores the plain 30 s read
    /// timeout and unblocked writes, and lets go of the [`LoginCancel`].
    pub fn set_deadline(&mut self, deadline: Option<Instant>) -> io::Result<()> {
        if deadline.is_none() {
            if let Some(cancel) = self.cancel.take() {
                cancel.detach();
            }
        }
        match &mut self.inner {
            Inner::Tcp(t) => {
                t.deadline = deadline;
                if deadline.is_none() {
                    t.reader.set_read_timeout(Some(READ_TIMEOUT))?;
                    t.reader.set_write_timeout(None)?;
                }
                Ok(())
            }
            Inner::Ws(w) => w.ws.lock().unwrap().set_deadline(deadline),
        }
    }

    fn connect_tcp(host: &str, port: u16) -> io::Result<ClientStream> {
        Self::tcp_over(TcpStream::connect((host, port))?, None)
    }

    fn tcp_over(socket: TcpStream, deadline: Option<Instant>) -> io::Result<ClientStream> {
        socket.set_read_timeout(Some(READ_TIMEOUT))?;
        socket.set_nodelay(true)?;
        let writer_sock = socket.try_clone()?;
        Ok(ClientStream {
            inner: Inner::Tcp(TcpInner {
                reader: socket,
                writer_sock,
                shared: Arc::new(Mutex::new(WriterState::new())),
                condvar: Arc::new(Condvar::new()),
                writer_thread: None,
                deadline,
            }),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            cancel: None,
        })
    }

    fn connect_wss(host: &str, port: u16) -> io::Result<ClientStream> {
        Self::wss_over(TcpStream::connect((host, port))?, host, port, None)
    }

    fn wss_over(
        tcp: TcpStream,
        host: &str,
        port: u16,
        deadline: Option<Instant>,
    ) -> io::Result<ClientStream> {
        use tungstenite::client::IntoClientRequest;
        tcp.set_read_timeout(Some(READ_TIMEOUT))?;
        tcp.set_nodelay(true)?;
        #[cfg(unix)]
        let fd = tcp.as_raw_fd();
        #[cfg(windows)]
        let socket = tcp.as_raw_socket();
        let tcp = DeadlineSocket {
            sock: tcp,
            deadline,
        };
        let connector = native_tls::TlsConnector::new().map_err(io_other)?;
        let tls = connector
            .connect(host, tcp)
            .map_err(|error| setup_error(deadline, io_other(error)))?;
        let authority = if port == 443 {
            host.to_string()
        } else {
            format!("{host}:{port}")
        };
        let mut req = format!("wss://{authority}/")
            .into_client_request()
            .map_err(io_other)?;
        req.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            tungstenite::http::HeaderValue::from_static("binary"),
        );
        let (ws, _) = tungstenite::client::client(req, tls)
            .map_err(|error| setup_error(deadline, io_other(error)))?;
        Ok(ClientStream {
            inner: Inner::Ws(Box::new(WsInner {
                ws: Mutex::new(WsConn::Tls(ws)),
                leftover: Mutex::new(VecDeque::new()),
                dummy: Mutex::new(false),
                pending_exit: Mutex::new(None),
                #[cfg(unix)]
                fd,
                #[cfg(windows)]
                socket,
            })),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            cancel: None,
        })
    }

    /// Payload bytes read so far (headers excluded); wraps at `u64`.
    pub fn bytes_in(&self) -> u64 {
        self.bytes_in.load(Ordering::Relaxed)
    }

    /// Payload bytes queued for writing so far (headers excluded); wraps at
    /// `u64`.
    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Relaxed)
    }

    pub(crate) fn take_exit_reason(&mut self) -> Option<StreamExitReason> {
        match &self.inner {
            Inner::Tcp(_) => None,
            Inner::Ws(w) => w.pending_exit.lock().unwrap().take(),
        }
    }

    /// The reader socket's raw fd, for `poll(2)` readability waits by the
    /// host's idle-slot scheduler. The writer thread runs on a clone of the
    /// socket, so polling this fd cannot race the writer; `close` shuts
    /// both ends down, which wakes a parked poll with EOF.
    #[cfg(unix)]
    pub fn fd(&self) -> RawFd {
        match &self.inner {
            Inner::Tcp(t) => t.reader.as_raw_fd(),
            Inner::Ws(w) => w.fd,
        }
    }

    /// Reader socket as a Windows `SOCKET` (pointer-width), for `WSAPoll`
    /// readability waits by the host's idle-slot scheduler. Same ownership
    /// notes as [`Self::fd`]: writer uses a clone; `close` wakes waiters.
    #[cfg(windows)]
    pub fn raw_socket(&self) -> RawSocket {
        match &self.inner {
            Inner::Tcp(t) => t.reader.as_raw_socket(),
            Inner::Ws(w) => w.socket,
        }
    }

    /// Read one byte: 0 after `close`, -1 at EOF, else the byte value.
    pub fn read(&mut self) -> io::Result<i32> {
        let mut b = [0u8; 1];
        match self.read_bytes_inner(&mut b)? {
            0 => Ok(0),
            n if n < 0 => Ok(-1),
            _ => Ok(b[0] as i32),
        }
    }

    /// Read into `dst`. A handshake whose [`LoginCancel`] fired reports
    /// that instead of the end of stream or error its shut socket gives.
    fn read_bytes_inner(&mut self, dst: &mut [u8]) -> io::Result<i32> {
        let cancelled = |stream: &Self| {
            stream
                .cancel
                .as_ref()
                .is_some_and(LoginCancel::is_cancelled)
        };
        if cancelled(self) {
            return Err(login_cancelled());
        }
        let result = self.read_socket(dst);
        match result {
            Ok(n) if n > 0 => Ok(n),
            _ if cancelled(self) => Err(login_cancelled()),
            other => other,
        }
    }

    fn read_socket(&mut self, dst: &mut [u8]) -> io::Result<i32> {
        match &mut self.inner {
            Inner::Tcp(t) => {
                if t.shared.lock().unwrap().dummy {
                    return Ok(0);
                }
                if let Some(deadline) = t.deadline {
                    t.reader.set_read_timeout(Some(time_left(deadline)?))?;
                }
                match transport_wait(|| t.reader.read(dst)) {
                    Ok(0) => Ok(-1),
                    Ok(n) => {
                        self.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
                        Ok(n as i32)
                    }
                    Err(e) => Err(timeout_at_deadline(t.deadline, e)),
                }
            }
            Inner::Ws(w) => {
                if *w.dummy.lock().unwrap() {
                    return Ok(0);
                }
                // Buffered payload is no excuse to go on past the deadline.
                if let Some(deadline) = w.ws.lock().unwrap().deadline() {
                    time_left(deadline)?;
                }
                transport_wait(|| fill_ws_blocking(w))?;
                let mut leftover = w.leftover.lock().unwrap();
                if leftover.is_empty() {
                    return Ok(-1);
                }
                let n = leftover.len().min(dst.len());
                for (i, b) in leftover.drain(..n).enumerate() {
                    dst[i] = b;
                }
                self.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
                Ok(n as i32)
            }
        }
    }

    /// Read exactly `len` bytes into `buf[off..off + len]`; error on EOF.
    /// After `close`, returns without touching the buffer (as Java).
    pub fn read_bytes(&mut self, buf: &mut [u8], off: usize, len: usize) -> io::Result<()> {
        if self.is_dummy() {
            return Ok(());
        }
        let dst = &mut buf[off..off + len];
        let mut filled = 0;
        while filled < dst.len() {
            let n = self.read_bytes_inner(&mut dst[filled..])?;
            if n <= 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
            }
            filled += n as usize;
        }
        Ok(())
    }

    fn is_dummy(&self) -> bool {
        match &self.inner {
            Inner::Tcp(t) => t.shared.lock().unwrap().dummy,
            Inner::Ws(w) => *w.dummy.lock().unwrap(),
        }
    }

    /// Bytes readable without blocking — the kernel receive-buffer count,
    /// capped at `AVAILABLE_BUF` (Java `SocketInputStream.available`
    /// estimate). `Client::tcp_in` (Task 16) relies on the exact count for its
    /// `available < psize` back-pressure check, so this is a full peek, not a
    /// 0/1 probe.
    ///
    /// For WSS this also drains binary frames already held in tungstenite/TLS
    /// buffers even when the TCP socket is quiet (`poll` would return 0).
    pub fn available(&mut self) -> io::Result<i32> {
        match &mut self.inner {
            Inner::Tcp(t) => {
                if t.shared.lock().unwrap().dummy {
                    return Ok(0);
                }
                t.reader.set_nonblocking(true)?;
                let mut b = [0u8; AVAILABLE_BUF];
                let n = match t.reader.peek(&mut b) {
                    Ok(0) => {
                        t.reader.set_nonblocking(false)?;
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
                    }
                    Ok(n) => n as i32,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => 0,
                    Err(e) => {
                        t.reader.set_nonblocking(false)?;
                        return Err(e);
                    }
                };
                t.reader.set_nonblocking(false)?;
                Ok(n)
            }
            Inner::Ws(w) => {
                if *w.dummy.lock().unwrap() {
                    return Ok(0);
                }
                // Always attempt a non-blocking drain: leftover alone is not
                // the full picture (further frames may already sit in
                // tungstenite/TLS), and a quiet TCP fd does not mean no app
                // data remains buffered above the kernel.
                match fill_ws_nonblocking(w) {
                    Ok(()) => {}
                    Err(e) => {
                        let n = w.leftover.lock().unwrap().len();
                        if n == 0 {
                            return Err(e);
                        }
                        // Prefer already-buffered payload over losing it to a
                        // transient drain error; next call can surface the err.
                    }
                }
                let n = w.leftover.lock().unwrap().len().min(AVAILABLE_BUF);
                Ok(n as i32)
            }
        }
    }

    /// Wait until [`Self::available`] reports at least `need` bytes, a read
    /// error is pending, or `limit` passes. A quiet socket parks in `poll`
    /// (`WSAPoll` on Windows), so the caller wakes as soon as the first byte
    /// arrives. Bytes that are already buffered but short of `need` are
    /// re-checked every millisecond: a readable socket would not park.
    pub(crate) fn wait_available(&mut self, need: i32, limit: Duration) {
        let deadline = Instant::now() + limit;
        loop {
            let available = match self.available() {
                Ok(available) => available,
                Err(_) => return,
            };
            if available >= need || self.is_dummy() {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return;
            }
            if available > 0 {
                thread::sleep(remaining.min(Duration::from_millis(1)));
            } else {
                self.park_readable(remaining);
            }
        }
    }

    #[cfg(unix)]
    fn park_readable(&self, limit: Duration) {
        poll_socket(self.fd(), Ready::Read, limit);
    }

    #[cfg(windows)]
    fn park_readable(&self, limit: Duration) {
        poll_socket(self.raw_socket(), Ready::Read, limit);
    }

    #[cfg(not(any(unix, windows)))]
    fn park_readable(&self, limit: Duration) {
        thread::sleep(limit.min(Duration::from_millis(1)));
    }

    /// Queue `len` bytes for the writer thread (Java `write(count, data, off)`).
    /// During a TCP login handshake (a deadline is set) the bytes are written
    /// before this returns, bounded by the deadline.
    pub fn write(&mut self, buf: &[u8], len: usize) -> io::Result<()> {
        let len = len.min(buf.len());
        if let Some(cancel) = &self.cancel {
            if cancel.is_cancelled() {
                return Err(login_cancelled());
            }
        }
        match &mut self.inner {
            Inner::Tcp(t) => match t.deadline {
                Some(deadline) => {
                    let written = transport_wait(|| {
                        tcp_write_until(t, &buf[..len], deadline, &self.bytes_out)
                    });
                    match (&self.cancel, written) {
                        (Some(cancel), Err(_)) if cancel.is_cancelled() => Err(login_cancelled()),
                        (_, written) => written,
                    }
                }
                None => tcp_write(t, buf, len, &self.bytes_out),
            },
            Inner::Ws(w) => {
                if *w.dummy.lock().unwrap() {
                    return Ok(());
                }
                let payload = buf[..len].to_vec();
                transport_wait(|| {
                    w.ws.lock()
                        .unwrap()
                        .send_message(Message::Binary(payload))
                        .map_err(|error| map_ws_err(w, error))
                })?;
                self.bytes_out.fetch_add(len as u64, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    /// Java `close`: mark dummy, shut the socket, stop the writer thread.
    pub fn close(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.detach();
        }
        match &mut self.inner {
            Inner::Tcp(t) => {
                {
                    let mut st = t.shared.lock().unwrap();
                    if st.dummy {
                        return;
                    }
                    st.dummy = true;
                    st.writer = false;
                }
                t.condvar.notify_all();
                let _ = t.reader.shutdown(Shutdown::Both);
                let _ = t.writer_sock.shutdown(Shutdown::Both);
                if let Some(handle) = t.writer_thread.take() {
                    let _ = transport_wait(|| handle.join());
                }
            }
            Inner::Ws(w) => {
                *w.dummy.lock().unwrap() = true;
                transport_wait(|| {
                    let _ = w.ws.lock().unwrap().close();
                });
            }
        }
    }
}

/// Blocking fill used by reads: wait for the next binary payload (or close).
/// Control frames are not payload and must not be treated as EOF.
fn fill_ws_blocking(w: &WsInner) -> io::Result<()> {
    if !w.leftover.lock().unwrap().is_empty() {
        return Ok(());
    }
    let mut ws = w.ws.lock().unwrap();
    loop {
        match ws.read_message() {
            Ok(Message::Binary(b)) => {
                if !b.is_empty() {
                    w.leftover.lock().unwrap().extend(b);
                }
                // Empty binary is a no-op frame; keep waiting for payload/close.
                if !w.leftover.lock().unwrap().is_empty() {
                    return Ok(());
                }
            }
            Ok(Message::Ping(p)) => {
                ws.send_message(Message::Pong(p))
                    .map_err(|error| map_ws_err(w, error))?;
            }
            Ok(Message::Pong(_)) | Ok(Message::Frame(_)) | Ok(Message::Text(_)) => {}
            Ok(Message::Close(frame)) => return Err(record_ws_close(w, frame)),
            Err(tungstenite::Error::ConnectionClosed) | Err(tungstenite::Error::AlreadyClosed) => {
                return Err(record_ws_close(w, None));
            }
            Err(e) => return Err(map_ws_err(w, e)),
        }
    }
}

/// Non-blocking drain used by `available`: pull every ready binary frame into
/// leftover (capped), including frames already held in tungstenite/TLS when
/// the TCP socket is quiet.
fn fill_ws_nonblocking(w: &WsInner) -> io::Result<()> {
    let mut ws = w.ws.lock().unwrap();
    ws.set_nonblocking(true)?;
    let result = (|| {
        // Tungstenite retains a queued control frame when a write returns
        // WouldBlock. Retry it before reading more frames on the next probe.
        match ws.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(());
            }
            Err(e) => return Err(map_ws_err(w, e)),
        }
        loop {
            {
                let n = w.leftover.lock().unwrap().len();
                if n >= AVAILABLE_BUF {
                    return Ok(());
                }
            }
            match ws.read_message() {
                Ok(Message::Binary(b)) => {
                    if !b.is_empty() {
                        w.leftover.lock().unwrap().extend(b);
                    }
                }
                Ok(Message::Ping(p)) => match ws.send_message(Message::Pong(p)) {
                    Ok(()) => {}
                    Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                        return Ok(());
                    }
                    Err(e) => return Err(map_ws_err(w, e)),
                },
                Ok(Message::Pong(_)) | Ok(Message::Frame(_)) | Ok(Message::Text(_)) => {}
                Ok(Message::Close(frame)) => return Err(record_ws_close(w, frame)),
                Err(tungstenite::Error::ConnectionClosed)
                | Err(tungstenite::Error::AlreadyClosed) => {
                    return Err(record_ws_close(w, None));
                }
                Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    return Ok(());
                }
                Err(e) => return Err(map_ws_err(w, e)),
            }
        }
    })();
    // Always restore blocking mode for subsequent blocking reads.
    let restore = ws.set_nonblocking(false);
    match (result, restore) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), _) => Err(e),
        (Ok(()), Err(e)) => Err(e),
    }
}

/// `TcpStream::connect` over every resolved address, each connect bounded
/// by the time left before `deadline`.
fn connect_socket_until(host: &str, port: u16, deadline: Instant) -> io::Result<TcpStream> {
    let mut last = None;
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, time_left(deadline)?) {
            Ok(socket) => return Ok(socket),
            Err(error) => last = Some(timeout_at_deadline(Some(deadline), error)),
        }
    }
    Err(last.unwrap_or_else(no_addresses))
}

fn no_addresses() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "could not resolve to any addresses",
    )
}

/// A TCP connect in flight on a non-blocking socket, tried over each
/// address in turn as `TcpStream::connect` does. Its caller waits for it in
/// slices ([`Self::poll`]), so a peer that never answers the connect cannot
/// hold the caller past its own deadline or cancel check. Without a
/// deadline of its own the connect ends as a blocking one does: connected,
/// or with the system's error for every address.
pub(crate) struct PendingConnect {
    socket: Option<socket2::Socket>,
    addrs: std::vec::IntoIter<SocketAddr>,
    last: Option<io::Error>,
}

impl PendingConnect {
    pub(crate) fn start(addrs: Vec<SocketAddr>) -> Self {
        Self {
            socket: None,
            addrs: addrs.into_iter(),
            last: None,
        }
    }

    /// Wait for the connect until `until`. `Ok(Some)` is the connected
    /// stream, in blocking mode; `Ok(None)` means the connect is still in
    /// flight at `until`; `Err` is the last address's error once every
    /// address failed.
    pub(crate) fn poll(&mut self, until: Instant) -> io::Result<Option<TcpStream>> {
        loop {
            let Some(socket) = &self.socket else {
                let addr = self
                    .addrs
                    .next()
                    .ok_or_else(|| self.last.take().unwrap_or_else(no_addresses))?;
                match begin_connect(addr) {
                    Ok(socket) => self.socket = Some(socket),
                    Err(error) => self.last = Some(error),
                }
                continue;
            };
            let left = until.saturating_duration_since(Instant::now());
            if !wait_ready(socket, Ready::Write, left) {
                return Ok(None);
            }
            let socket = self.socket.take().expect("checked above");
            match socket.take_error() {
                Ok(None) => {}
                Ok(Some(error)) | Err(error) => {
                    self.last = Some(error);
                    continue;
                }
            }
            socket.set_nonblocking(false)?;
            return Ok(Some(socket.into()));
        }
    }
}

fn begin_connect(addr: SocketAddr) -> io::Result<socket2::Socket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    socket.set_nonblocking(true)?;
    match socket.connect(&addr.into()) {
        Ok(()) => Ok(socket),
        Err(error) if connect_in_progress(&error) => Ok(socket),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn connect_in_progress(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::EINPROGRESS)
}

#[cfg(not(unix))]
fn connect_in_progress(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
}

fn record_ws_close(
    w: &WsInner,
    frame: Option<tungstenite::protocol::CloseFrame<'static>>,
) -> io::Error {
    let (code, reason) = frame.map_or((None, String::new()), |frame| {
        (Some(frame.code.into()), frame.reason.into_owned())
    });
    let mut pending = w.pending_exit.lock().unwrap();
    if pending.is_none() {
        *pending = Some(StreamExitReason::WebSocketClose { code, reason });
    }
    io::Error::new(io::ErrorKind::UnexpectedEof, "websocket closed")
}

fn map_ws_err(w: &WsInner, err: tungstenite::Error) -> io::Error {
    match err {
        tungstenite::Error::Io(error) => error,
        tungstenite::Error::Tls(error) => {
            let message = error.to_string();
            let mut pending = w.pending_exit.lock().unwrap();
            if pending.is_none() {
                *pending = Some(StreamExitReason::Tls(message.clone()));
            }
            io::Error::other(message)
        }
        other => {
            let message = other.to_string();
            let mut pending = w.pending_exit.lock().unwrap();
            if pending.is_none() {
                *pending = Some(StreamExitReason::WebSocket(message.clone()));
            }
            io::Error::other(message)
        }
    }
}

fn tcp_write(t: &mut TcpInner, buf: &[u8], len: usize, bytes_out: &AtomicU64) -> io::Result<()> {
    let sock = t.writer_sock.try_clone()?;
    let shared = t.shared.clone();
    let condvar = t.condvar.clone();
    let mut st = t.shared.lock().unwrap();
    if st.dummy {
        return Ok(());
    }
    if st.ioerror {
        st.ioerror = false;
        return Err(io::Error::other("Error in writer thread"));
    }
    let mut queued = 0u64;
    for &b in &buf[..len] {
        let tnum = st.tnum;
        st.buf[tnum] = b;
        st.tnum = (st.tnum + 1) % BUF_SIZE;
        queued += 1;
        if st.tnum == (st.tcycl + BUF_SIZE - 100) % BUF_SIZE {
            bytes_out.fetch_add(queued, Ordering::Relaxed);
            return Err(io::Error::other("buffer overflow"));
        }
    }
    bytes_out.fetch_add(queued, Ordering::Relaxed);
    if !st.writer {
        st.writer = true;
        t.writer_thread = Some(thread::spawn(move || writer_loop(shared, condvar, sock)));
    }
    t.condvar.notify_one();
    Ok(())
}

/// A login handshake write: the bytes go to the socket before this returns,
/// on the caller's thread, with each socket write armed with the time left
/// before `deadline`. A peer that stops reading therefore cannot hold the
/// handshake past it. Game bytes the writer thread still holds (an adopted
/// stream) are sent first, so the order on the wire is kept.
fn tcp_write_until(
    t: &mut TcpInner,
    buf: &[u8],
    deadline: Instant,
    bytes_out: &AtomicU64,
) -> io::Result<()> {
    loop {
        {
            let mut st = t.shared.lock().unwrap();
            if st.dummy {
                return Ok(());
            }
            if st.ioerror {
                st.ioerror = false;
                return Err(io::Error::other("Error in writer thread"));
            }
            if st.tnum == st.tcycl {
                break;
            }
        }
        time_left(deadline)?;
        thread::sleep(Duration::from_millis(1));
    }
    let mut rest = buf;
    while !rest.is_empty() {
        t.reader.set_write_timeout(Some(time_left(deadline)?))?;
        match (&t.reader).write(rest) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                rest = &rest[n..];
                bytes_out.fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // A send timeout that fired a little early: go on with the time left.
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {}
            Err(error) => return Err(timeout_at_deadline(Some(deadline), error)),
        }
    }
    Ok(())
}

impl Drop for ClientStream {
    fn drop(&mut self) {
        self.close();
    }
}

fn writer_loop(shared: Arc<Mutex<WriterState>>, condvar: Arc<Condvar>, mut sock: TcpStream) {
    loop {
        let (tcycl, var3) = {
            let mut st = shared.lock().unwrap();
            while st.tnum == st.tcycl && st.writer {
                st = condvar.wait(st).unwrap();
            }
            if !st.writer {
                return;
            }
            let tcycl = st.tcycl;
            let var3 = if st.tnum >= tcycl {
                st.tnum - tcycl
            } else {
                BUF_SIZE - tcycl
            };
            (tcycl, var3)
        };
        if var3 == 0 {
            continue;
        }
        let mut chunk = vec![0u8; var3];
        {
            let st = shared.lock().unwrap();
            let mut idx = tcycl;
            for b in chunk.iter_mut() {
                *b = st.buf[idx];
                idx = (idx + 1) % BUF_SIZE;
            }
        }
        let wr = sock.write_all(&chunk);
        let mut st = shared.lock().unwrap();
        st.tcycl = (st.tcycl + var3) % BUF_SIZE;
        if wr.is_err() {
            st.ioerror = true;
        }
        if st.tnum == st.tcycl && sock.flush().is_err() {
            st.ioerror = true;
        }
    }
}

/// Zero-time readability probe handle for WSS diagnostics/tests.
#[cfg(all(test, unix))]
fn wss_wait_handle(w: &WsInner) -> RawFd {
    w.fd
}

#[cfg(all(test, windows))]
fn wss_wait_handle(w: &WsInner) -> RawSocket {
    w.socket
}

#[cfg(all(test, unix))]
fn socket_readable_now(fd: RawFd) -> bool {
    poll_socket(fd, Ready::Read, Duration::ZERO)
}

#[cfg(all(test, windows))]
fn socket_readable_now(socket: RawSocket) -> bool {
    poll_socket(socket, Ready::Read, Duration::ZERO)
}

/// What [`wait_ready`] waits for on a socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ready {
    Read,
    Write,
}

/// Park until `socket` is ready for `ready` (or reports an error or
/// hang-up) or `limit` passes, rounded up to whole milliseconds. True when
/// it became ready.
#[cfg(unix)]
pub(crate) fn wait_ready(socket: &impl AsRawFd, ready: Ready, limit: Duration) -> bool {
    poll_socket(socket.as_raw_fd(), ready, limit)
}

#[cfg(windows)]
pub(crate) fn wait_ready(socket: &impl AsRawSocket, ready: Ready, limit: Duration) -> bool {
    poll_socket(socket.as_raw_socket(), ready, limit)
}

/// [`wait_ready`] on a raw fd.
#[cfg(unix)]
fn poll_socket(fd: RawFd, ready: Ready, limit: Duration) -> bool {
    let events = match ready {
        Ready::Read => libc::POLLIN,
        Ready::Write => libc::POLLOUT,
    };
    let mut fds = [libc::pollfd {
        fd,
        events,
        revents: 0,
    }];
    let timeout = limit.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32;
    // SAFETY: `fds` is a live one-element array for the whole call and its
    // length is passed as 1; poll only writes `revents` inside it.
    let rc = unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout) };
    rc > 0
}

/// [`wait_ready`] on a raw `SOCKET`.
#[cfg(windows)]
fn poll_socket(socket: RawSocket, ready: Ready, limit: Duration) -> bool {
    use windows_sys::Win32::Networking::WinSock::{WSAPoll, POLLIN, POLLOUT, SOCKET, WSAPOLLFD};
    let events = match ready {
        Ready::Read => POLLIN,
        Ready::Write => POLLOUT,
    };
    let mut fds = [WSAPOLLFD {
        fd: socket as SOCKET,
        events,
        revents: 0,
    }];
    let timeout = limit.as_micros().div_ceil(1000).min(i32::MAX as u128) as i32;
    // SAFETY: `fds` is a live one-element array for the whole call and its
    // length is passed as 1; WSAPoll only writes `revents` inside it.
    let rc = unsafe { WSAPoll(fds.as_mut_ptr(), 1, timeout) };
    rc > 0
}

/// Plain loopback WebSocket connector for host transport regressions. This
/// cannot bypass TLS trust in product builds: `test-support` is dev-only.
#[cfg(any(test, feature = "test-support"))]
pub fn connect_ws_plain(host: &str, port: u16) -> io::Result<ClientStream> {
    use tungstenite::client::IntoClientRequest;
    let tcp = TcpStream::connect((host, port))?;
    tcp.set_read_timeout(Some(READ_TIMEOUT))?;
    tcp.set_nodelay(true)?;
    #[cfg(unix)]
    let fd = tcp.as_raw_fd();
    #[cfg(windows)]
    let socket = tcp.as_raw_socket();
    let req = format!("ws://{host}:{port}/")
        .into_client_request()
        .map_err(io_other)?;
    let tcp = DeadlineSocket {
        sock: tcp,
        deadline: None,
    };
    let (ws, _) = tungstenite::client::client(req, tcp).map_err(io_other)?;
    Ok(ClientStream {
        inner: Inner::Ws(Box::new(WsInner {
            ws: Mutex::new(WsConn::Plain(ws)),
            leftover: Mutex::new(VecDeque::new()),
            dummy: Mutex::new(false),
            pending_exit: Mutex::new(None),
            #[cfg(unix)]
            fd,
            #[cfg(windows)]
            socket,
        })),
        bytes_in: AtomicU64::new(0),
        bytes_out: AtomicU64::new(0),
        cancel: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::client::{SessionExitReason, SERVER_TIMEOUT};
    use crate::client::{Client, ClientConfig, ClientRevision};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use tungstenite::WebSocket as TungsteniteWs;

    fn spawn_ws_server(
        on_ready: impl FnOnce(TungsteniteWs<TcpStream>) + Send + 'static,
    ) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let ws = tungstenite::accept(stream).unwrap();
            on_ready(ws);
        });
        (addr, handle)
    }

    fn connect_ws_plain_with_write_backpressure(
        host: &str,
        port: u16,
    ) -> (ClientStream, Arc<AtomicBool>, Arc<AtomicU64>) {
        use tungstenite::client::IntoClientRequest;

        let tcp = TcpStream::connect((host, port)).unwrap();
        tcp.set_read_timeout(Some(READ_TIMEOUT)).unwrap();
        tcp.set_nodelay(true).unwrap();
        #[cfg(unix)]
        let fd = tcp.as_raw_fd();
        #[cfg(windows)]
        let socket = tcp.as_raw_socket();
        let block_next_write = Arc::new(AtomicBool::new(false));
        let blocked_writes = Arc::new(AtomicU64::new(0));
        let writer = WriteOnceWouldBlock {
            socket: tcp,
            block_next_write: Arc::clone(&block_next_write),
            blocked_writes: Arc::clone(&blocked_writes),
        };
        let req = format!("ws://{host}:{port}/")
            .into_client_request()
            .unwrap();
        let (ws, _) = tungstenite::client::client(req, writer).unwrap();
        let stream = ClientStream {
            inner: Inner::Ws(Box::new(WsInner {
                ws: Mutex::new(WsConn::BlockedWriter(ws)),
                leftover: Mutex::new(VecDeque::new()),
                dummy: Mutex::new(false),
                pending_exit: Mutex::new(None),
                #[cfg(unix)]
                fd,
                #[cfg(windows)]
                socket,
            })),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            cancel: None,
        };
        (stream, block_next_write, blocked_writes)
    }

    #[test]
    fn reader_handle_tracks_the_socket_and_wakes_on_data() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut stream = ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        #[cfg(unix)]
        assert!(stream.fd() >= 0, "fd must be a valid socket descriptor");
        #[cfg(windows)]
        assert!(
            stream.raw_socket()
                != windows_sys::Win32::Networking::WinSock::INVALID_SOCKET as RawSocket,
            "raw_socket must be a valid SOCKET"
        );
        // Idle: nothing readable yet.
        assert_eq!(stream.available().unwrap(), 0);
        server.write_all(&[1, 2, 3]).unwrap();
        // The handle sits on the same socket `available` peeks; wait for the
        // bytes to land (loopback delivery is not synchronous with write).
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if stream.available().unwrap() == 3 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "bytes never became readable on the socket"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        #[cfg(unix)]
        assert!(stream.fd() >= 0);
        #[cfg(windows)]
        assert!(
            stream.raw_socket()
                != windows_sys::Win32::Networking::WinSock::INVALID_SOCKET as RawSocket
        );
    }

    /// Coalesced WS binary frames with no further TCP traffic: `available`
    /// must report the full application payload, not only the first frame
    /// that a single `ws.read()` pulled (remainder may sit only in
    /// tungstenite's buffer while the kernel fd is quiet).
    #[test]
    fn ws_available_drains_coalesced_frames_when_socket_quiet() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            ws.send(Message::Binary(vec![1, 2, 3])).unwrap();
            ws.send(Message::Binary(vec![4, 5, 6])).unwrap();
            // Hold the connection open but send nothing else so the client
            // side goes quiet after the frames are absorbed above TCP.
            let _ = ws.read(); // wait for client close
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut n = 0;
        while std::time::Instant::now() < deadline {
            n = stream.available().unwrap();
            if n == 6 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            n, 6,
            "available must coalesce both binary frames without more TCP sends"
        );

        // After first fill, fd may be quiet; a second available must not drop
        // to a partial leftover-only view.
        assert_eq!(stream.available().unwrap(), 6);

        let mut buf = [0u8; 6];
        stream.read_bytes(&mut buf, 0, 6).unwrap();
        assert_eq!(&buf, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(stream.available().unwrap(), 0);
        stream.close();
        let _ = server.join();
    }

    /// Split protocol payload across two WS frames: partial leftover must not
    /// freeze available below the total buffered application bytes.
    #[test]
    fn ws_available_appends_while_partial_leftover_exists() {
        let (tx, rx) = mpsc::channel::<()>();
        let (addr, server) = spawn_ws_server(move |mut ws| {
            ws.send(Message::Binary(vec![0xaa])).unwrap();
            // Pause until the client has observed the first byte so leftover
            // is non-empty before the second frame is considered.
            let _ = rx.recv_timeout(Duration::from_secs(2));
            ws.send(Message::Binary(vec![0xbb, 0xcc])).unwrap();
            let _ = ws.read();
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while stream.available().unwrap() < 1 {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(stream.available().unwrap(), 1);
        tx.send(()).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let mut n = 1;
        while std::time::Instant::now() < deadline {
            n = stream.available().unwrap();
            if n == 3 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(n, 3, "second frame must extend leftover, not stall at 1");

        let mut buf = [0u8; 3];
        stream.read_bytes(&mut buf, 0, 3).unwrap();
        assert_eq!(&buf, &[0xaa, 0xbb, 0xcc]);
        stream.close();
        let _ = server.join();
    }

    /// Control frames are not payload and must not surface as EOF on read.
    #[test]
    fn ws_read_skips_ping_and_continues_to_binary() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            ws.send(Message::Ping(vec![9])).unwrap();
            ws.send(Message::Binary(vec![7, 8])).unwrap();
            // Consume the automatic pong if any, then wait for close.
            let _ = ws.read();
            let _ = ws.read();
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();
        assert_eq!(stream.read().unwrap(), 7);
        assert_eq!(stream.read().unwrap(), 8);
        stream.close();
        let _ = server.join();
    }

    #[test]
    fn ws_available_retries_pong_after_would_block_in_order() {
        let (observed_tx, observed_rx) = mpsc::channel();
        let (addr, server) = spawn_ws_server(move |mut ws| {
            ws.send(Message::Ping(vec![9, 8, 7])).unwrap();
            loop {
                match ws.read() {
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(message) => observed_tx.send(message).unwrap(),
                }
            }
        });
        let (mut stream, block_next_write, blocked_writes) =
            connect_ws_plain_with_write_backpressure(&addr.ip().to_string(), addr.port());
        block_next_write.store(true, Ordering::Relaxed);

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while blocked_writes.load(Ordering::Relaxed) == 0 {
            assert_eq!(
                stream
                    .available()
                    .expect("a backpressured Pong is temporary, not a read error"),
                0,
                "the control Pong is not inbound game data"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "the client did not attempt to send the Ping response"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(blocked_writes.load(Ordering::Relaxed), 1);
        assert_eq!(
            stream
                .available()
                .expect("the second availability probe must retry the queued Pong"),
            0
        );
        assert_eq!(
            observed_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("server did not observe the Pong after the second availability probe"),
            Message::Pong(vec![9, 8, 7]),
            "the Pong must reach the server before any client write"
        );

        stream.write(&[0x12, 0x34], 2).unwrap();
        assert_eq!(
            observed_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("server did not observe the following game frame"),
            Message::Binary(vec![0x12, 0x34]),
            "the game frame must follow the Pong"
        );
        stream.close();
        server.join().unwrap();
        assert!(
            observed_rx.try_recv().is_err(),
            "server observed an unexpected extra client message"
        );
    }

    #[test]
    fn ws_close_preserves_code_and_reason_then_local_close_is_dummy() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            ws.send(Message::Binary(vec![1])).unwrap();
            let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Away,
                reason: std::borrow::Cow::Borrowed("maintenance"),
            }));
            let _ = ws.read();
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while stream.available().unwrap() < 1 {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(stream.read().unwrap(), 1);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let reason = loop {
            let _ = stream.available();
            if let Some(reason) = stream.take_exit_reason() {
                break reason;
            }
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            reason,
            StreamExitReason::WebSocketClose {
                code: Some(1001),
                reason: "maintenance".into(),
            }
        );
        stream.close();
        assert_eq!(stream.read().unwrap(), 0);
        let _ = server.join();
    }

    /// Drive `tcp_in` until the client enters the `lostCon` reconnect path.
    /// The fatal pass runs `login` synchronously against a dead port, so the
    /// wait covers loopback delivery *and* the OS connect-refused latency;
    /// bound it by the product's own dead-server horizon, not a tuned guess
    /// (2 s flakes on Windows, where refused loopback connects are slower).
    fn drive_to_reconnect(client: &mut Client) {
        let deadline = std::time::Instant::now() + SERVER_TIMEOUT;
        while client.last_login_reconnect != Some(true) {
            client.tcp_in();
            assert!(
                std::time::Instant::now() < deadline,
                "reconnect path not entered within SERVER_TIMEOUT"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn standalone_ws_close_enters_lost_con_reconnect_path() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            let _ = ws.close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::Away,
                reason: std::borrow::Cow::Borrowed("maintenance"),
            }));
            let _ = ws.read();
        });
        let closed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_addr = closed_listener.local_addr().unwrap();
        drop(closed_listener);
        let mut client = Client::new_with_revision(
            ClientConfig {
                host: closed_addr.ip().to_string(),
                port: closed_addr.port(),
                cache_dir: std::env::temp_dir()
                    .join(format!("ws-close-{}", std::process::id()))
                    .to_string_lossy()
                    .into_owned(),
                members: true,
                lowmem: true,
            },
            ClientRevision::R289,
        );
        client.stream = Some(connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap());
        client.ingame = true;
        client.ptype = -1;

        drive_to_reconnect(&mut client);

        assert!(!client.ingame);
        assert_eq!(
            client.take_session_exit_reason(),
            Some(SessionExitReason::WebSocketClose {
                code: Some(1001),
                reason: "maintenance".into(),
            })
        );
        let _ = server.join();
    }

    /// A non-I/O WebSocket failure is transport loss, not the client's T2
    /// packet-decoder path: an in-game standalone client reconnects at once.
    #[test]
    fn standalone_ws_protocol_error_enters_lost_con_reconnect_path() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            // FIN plus reserved opcode 3 is not a legal WebSocket frame.
            ws.get_mut().write_all(&[0x83, 0x00]).unwrap();
        });
        let closed_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_addr = closed_listener.local_addr().unwrap();
        drop(closed_listener);
        let mut client = Client::new_with_revision(
            ClientConfig {
                host: closed_addr.ip().to_string(),
                port: closed_addr.port(),
                cache_dir: std::env::temp_dir()
                    .join(format!("ws-protocol-{}", std::process::id()))
                    .to_string_lossy()
                    .into_owned(),
                members: true,
                lowmem: true,
            },
            ClientRevision::R289,
        );
        client.stream = Some(connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap());
        client.ingame = true;
        client.ptype = -1;

        drive_to_reconnect(&mut client);

        assert!(!client.ingame);
        assert!(matches!(
            client.take_session_exit_reason(),
            Some(SessionExitReason::ReadError {
                kind: io::ErrorKind::Other,
                ..
            })
        ));
        let _ = server.join();
    }

    #[test]
    fn ws_dummy_write_is_noop_and_available_zero() {
        let (addr, server) = spawn_ws_server(|ws| {
            let _ = ws;
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();
        stream.close();
        assert_eq!(stream.available().unwrap(), 0);
        assert_eq!(stream.read().unwrap(), 0);
        stream.write(&[1, 2, 3], 3).unwrap();
        assert_eq!(stream.bytes_out(), 0);
        let _ = server.join();
    }

    /// Quiet nonblocking readiness: after frames are fully absorbed above TCP,
    /// available still reports leftover without requiring POLLIN.
    #[test]
    fn ws_available_with_leftover_when_fd_not_readable() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            ws.send(Message::Binary(vec![10, 11, 12, 13])).unwrap();
            let _ = ws.read();
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while stream.available().unwrap() < 4 {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        // Drain one byte so leftover is partial, then ensure fd can go quiet.
        assert_eq!(stream.read().unwrap(), 10);
        // Spin until poll says not readable (data held only in leftover).
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let readable = match &stream.inner {
                Inner::Ws(w) => socket_readable_now(wss_wait_handle(w)),
                _ => unreachable!(),
            };
            if !readable {
                break;
            }
            // Nudge any residual kernel byte without consuming leftover via available.
            assert!(std::time::Instant::now() < deadline, "fd never went quiet");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            stream.available().unwrap(),
            3,
            "leftover must remain visible when TCP poll is quiet"
        );
        let mut buf = [0u8; 3];
        stream.read_bytes(&mut buf, 0, 3).unwrap();
        assert_eq!(&buf, &[11, 12, 13]);
        stream.close();
        let _ = server.join();
    }

    /// The cold-fill wait parks on a quiet WebSocket, wakes when a frame
    /// arrives (long before its limit), and keeps waiting while only part of
    /// the needed bytes is buffered.
    #[test]
    fn ws_wait_available_wakes_on_arrival_and_waits_for_the_whole_need() {
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (addr, server) = spawn_ws_server(move |mut ws| {
            release_rx.recv().unwrap();
            ws.send(Message::Binary(vec![1, 2, 3])).unwrap();
            release_rx.recv().unwrap();
            ws.send(Message::Binary(vec![4, 5, 6])).unwrap();
            let _ = ws.read();
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();

        let started = std::time::Instant::now();
        stream.wait_available(6, Duration::from_millis(80));
        let quiet = started.elapsed();
        assert!(quiet >= Duration::from_millis(80), "{quiet:?}");
        assert_eq!(stream.available().unwrap(), 0);

        let release = release_tx.clone();
        let sender = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            release.send(()).unwrap();
            thread::sleep(Duration::from_millis(40));
            release.send(()).unwrap();
        });
        let started = std::time::Instant::now();
        stream.wait_available(3, Duration::from_secs(5));
        let first = started.elapsed();
        assert!(first < Duration::from_secs(1), "woke after {first:?}");
        assert_eq!(stream.available().unwrap(), 3);
        stream.wait_available(6, Duration::from_secs(5));
        let whole = started.elapsed();
        assert!(whole < Duration::from_secs(2), "woke after {whole:?}");
        assert_eq!(stream.available().unwrap(), 6);
        sender.join().unwrap();
        stream.close();
        let _ = server.join();
    }

    /// The same wait over plain TCP.
    #[test]
    fn tcp_wait_available_wakes_on_arrival() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(50));
            socket.write_all(&[7; 6]).unwrap();
            thread::sleep(Duration::from_millis(200));
        });
        let mut stream = ClientStream::connect("127.0.0.1", port).unwrap();
        let started = std::time::Instant::now();
        stream.wait_available(6, Duration::from_secs(5));
        let woke = started.elapsed();
        assert!(woke < Duration::from_secs(1), "woke after {woke:?}");
        assert_eq!(stream.available().unwrap(), 6);
        stream.close();
        server.join().unwrap();
    }

    /// A WebSocket peer that keeps sending control frames but never answers
    /// the login still ends that login at its deadline: control frames are
    /// not progress, and no read or Pong reply outlasts the deadline.
    #[test]
    fn login_deadline_ends_a_handshake_kept_alive_by_ws_control_frames() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            let until = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < until && ws.send(Message::Ping(Vec::new())).is_ok() {
                thread::sleep(Duration::from_millis(20));
            }
        });
        let mut client = Client::new(ClientConfig {
            host: "127.0.0.1".into(),
            port: 1,
            cache_dir: "/tmp".into(),
            members: true,
            lowmem: true,
        });
        client.stream = Some(connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap());
        client.baton = true;
        let started = std::time::Instant::now();
        client.set_login_deadline(started + Duration::from_millis(500));
        let error = client.login("alice", "secret", true).unwrap_err();
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(500) && took < Duration::from_millis(1000),
            "the login ended {took:?} after it started, not at its 500 ms deadline"
        );
        let receipt = error.transport.expect("deadline receipt");
        assert_eq!(
            receipt.stage,
            crate::client::login_error::LoginStage::ReadHello
        );
        assert_eq!(receipt.kind, io::ErrorKind::TimedOut);
        assert_eq!(receipt.detail, "login handshake deadline passed");
        drop(client);
        server.join().unwrap();
    }

    /// A server that accepts the connection and never reads: the handshake's
    /// writes fill both socket buffers and then block, and the write that
    /// blocks still returns at the deadline.
    #[test]
    fn login_deadline_ends_a_handshake_write_to_a_peer_that_never_reads() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let started = Instant::now();
        let deadline = started + Duration::from_millis(500);
        let mut stream = ClientStream::connect_until(
            Transport::Tcp,
            &addr.ip().to_string(),
            addr.port(),
            deadline,
            None,
        )
        .unwrap();
        let (_held, _) = listener.accept().unwrap();
        let chunk = vec![0u8; 64 * 1024];
        let error = loop {
            if let Err(error) = stream.write(&chunk, chunk.len()) {
                break error;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the writes never blocked"
            );
        };
        let took = started.elapsed();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "login handshake deadline passed");
        assert!(
            took >= Duration::from_millis(500) && took < Duration::from_millis(1000),
            "the blocked write returned {took:?} after the start, not at its 500 ms deadline"
        );
        assert!(
            stream.bytes_out() > 0,
            "the handshake wrote before it blocked"
        );
    }

    thread_local! {
        /// When this thread's last call through the transport wait hook
        /// went in and came out.
        static HOOKED: std::cell::Cell<Option<(Instant, Instant)>> =
            const { std::cell::Cell::new(None) };
    }

    fn record_transport_wait(wait: &mut dyn FnMut()) {
        let entered = Instant::now();
        wait();
        HOOKED.with(|span| span.set(Some((entered, Instant::now()))));
    }

    /// A host sees each socket call that blocks its thread through the
    /// hook: the call is still waiting for the peer inside the hook. A game
    /// send to a peer that stopped reading, a login handshake write, and a
    /// read waiting for the peer's next byte.
    #[test]
    fn blocking_socket_calls_run_through_the_transport_wait_hook() {
        set_transport_wait_hook(record_transport_wait);
        let stall = Duration::from_millis(500);
        let blocked_inside = |peer_moved: Instant, what: &str| {
            let (entered, left) = HOOKED
                .with(std::cell::Cell::take)
                .unwrap_or_else(|| panic!("{what} never went through the hook"));
            assert!(
                entered <= peer_moved && peer_moved <= left,
                "{what} was not waiting for the peer inside the hook: in {:?} before, out {:?} after",
                peer_moved.saturating_duration_since(entered),
                left.saturating_duration_since(peer_moved),
            );
        };

        // A send larger than both socket buffers to a peer that has
        // stopped reading.
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (resumed_tx, resumed_rx) = mpsc::channel();
        let (addr, server) = spawn_ws_server(move |mut ws| {
            stop_rx.recv().unwrap();
            thread::sleep(stall);
            resumed_tx.send(Instant::now()).unwrap();
            let _ = io::copy(ws.get_mut(), &mut io::sink());
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();
        let payload = vec![0u8; 16 << 20];
        stop_tx.send(()).unwrap();
        stream.write(&payload, payload.len()).unwrap();
        blocked_inside(resumed_rx.recv().unwrap(), "the WebSocket send");
        drop(stream);
        server.join().unwrap();

        // A login handshake write to a peer that never reads, which blocks
        // until its deadline.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let deadline = Instant::now() + stall;
        let mut stream = ClientStream::connect_until(
            Transport::Tcp,
            &addr.ip().to_string(),
            addr.port(),
            deadline,
            None,
        )
        .unwrap();
        let (_held, _) = listener.accept().unwrap();
        let chunk = vec![0u8; 64 * 1024];
        while stream.write(&chunk, chunk.len()).is_ok() {}
        blocked_inside(deadline, "the handshake write");

        // A read waiting for the peer's next byte.
        let mut stream = ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        let peer = thread::spawn(move || {
            thread::sleep(stall);
            let sent = Instant::now();
            peer.write_all(&[7]).unwrap();
            (peer, sent)
        });
        assert_eq!(stream.read().unwrap(), 7);
        let (_peer, sent) = peer.join().unwrap();
        blocked_inside(sent, "the read");
    }

    /// A cancel shuts the handshake's socket from another thread: the server
    /// reads end of stream at once, and the read blocked on that socket
    /// returns with the cancel. A handshake cancelled before it connects
    /// fails its connect; a cancel after the handshake leaves the session.
    #[test]
    fn login_cancel_shuts_only_the_handshake_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let host = addr.ip().to_string();
        let far = || Instant::now() + Duration::from_secs(30);

        let cancel = LoginCancel::new();
        let mut stream =
            ClientStream::connect_until(Transport::Tcp, &host, addr.port(), far(), Some(&cancel))
                .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        stream.write(&[14, 0], 2).unwrap();
        let mut hello = [0u8; 2];
        server.read_exact(&mut hello).unwrap();
        // The server says nothing more, so this read blocks until the cancel.
        let reader = thread::spawn(move || {
            let result = stream.read();
            (result, Instant::now())
        });
        thread::sleep(Duration::from_millis(100));
        let cancelled_at = Instant::now();
        cancel.cancel();
        server
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut byte = [0u8; 1];
        assert_eq!(
            server.read(&mut byte).unwrap(),
            0,
            "the server reads end of stream"
        );
        assert!(cancelled_at.elapsed() < Duration::from_secs(1));
        let (result, returned) = reader.join().unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        assert_eq!(error.to_string(), "login cancelled by its host");
        assert!(returned.duration_since(cancelled_at) < Duration::from_secs(1));

        let error =
            ClientStream::connect_until(Transport::Tcp, &host, addr.port(), far(), Some(&cancel))
                .err()
                .expect("a cancelled handshake does not connect");
        assert_eq!(error.to_string(), "login cancelled by its host");
        let (mut refused, _) = listener.accept().unwrap();
        refused
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        assert_eq!(refused.read(&mut byte).unwrap(), 0);

        let cancel = LoginCancel::new();
        let mut stream =
            ClientStream::connect_until(Transport::Tcp, &host, addr.port(), far(), Some(&cancel))
                .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        stream.set_deadline(None).unwrap();
        cancel.cancel();
        stream.write(&[7], 1).unwrap();
        server.read_exact(&mut byte).unwrap();
        assert_eq!(
            byte,
            [7],
            "the session outlives a cancel after its handshake"
        );
        stream.close();
    }

    /// Payload already buffered from an earlier frame is not read past the
    /// deadline either; clearing the deadline hands it to the session.
    #[test]
    fn ws_buffered_payload_is_not_read_past_the_deadline() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            ws.send(Message::Binary(vec![1, 2])).unwrap();
            // Until the client's close.
            let _ = ws.read();
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();
        let deadline = Instant::now() + Duration::from_millis(300);
        stream.set_deadline(Some(deadline)).unwrap();
        assert_eq!(stream.read().unwrap(), 1);
        thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(20),
        );
        let error = stream.read().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "login handshake deadline passed");
        stream.set_deadline(None).unwrap();
        assert_eq!(stream.read().unwrap(), 2);
        stream.close();
        server.join().unwrap();
    }

    /// A cancellable connect to a server that takes the TCP connection but
    /// never starts TLS ends at the cancel, not at the 30 s read timeout,
    /// and the server sees the connection close.
    #[test]
    fn cancel_ends_a_stalled_tls_setup_of_a_cancellable_connect() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            // Read the ClientHello, never answer it, and wait for the close.
            let mut buf = [0u8; 4096];
            let started = Instant::now();
            while socket.read(&mut buf).is_ok_and(|n| n > 0) {}
            started.elapsed()
        });
        let cancel = LoginCancel::new();
        let canceller = {
            let cancel = cancel.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(200));
                cancel.cancel();
                Instant::now()
            })
        };
        let error = ClientStream::connect_cancellable(Transport::Wss, "127.0.0.1", port, &cancel)
            .err()
            .expect("a cancelled setup fails");
        let ended = Instant::now();
        let cancelled_at = canceller.join().unwrap();
        let server_open = server.join().unwrap();
        assert_eq!(error.to_string(), "login cancelled by its host");
        let late = ended.saturating_duration_since(cancelled_at);
        assert!(
            late < Duration::from_millis(100),
            "returned {late:?} after the cancel"
        );
        assert!(
            server_open < Duration::from_secs(1),
            "server saw the close after {server_open:?}"
        );
    }

    /// A cancellable connect that completes is an ordinary stream: the
    /// cancel shuts it during the handshake and leaves it alone once
    /// `set_deadline(None)` has ended the handshake.
    #[test]
    fn a_cancellable_connect_is_released_when_its_handshake_ends() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut byte = [0u8; 1];
            socket.read_exact(&mut byte).unwrap();
            socket.write_all(&[byte[0] + 1]).unwrap();
            socket.read_exact(&mut byte).unwrap();
            socket.write_all(&[byte[0] + 1]).unwrap();
        });
        let cancel = LoginCancel::new();
        let mut stream =
            ClientStream::connect_cancellable(Transport::Tcp, "127.0.0.1", port, &cancel).unwrap();
        stream.write(&[15], 1).unwrap();
        assert_eq!(stream.read().unwrap(), 16);
        stream.set_deadline(None).unwrap();
        cancel.cancel();
        stream.write(&[20], 1).unwrap();
        assert_eq!(stream.read().unwrap(), 21);
        stream.close();
        server.join().unwrap();
    }
}
