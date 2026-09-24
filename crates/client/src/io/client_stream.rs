//! Game stream: Java TCP locally, WSS (`binary` subprotocol) on Prod.
//!
//! Local reads are blocking on the calling thread with a 30 s soTimeout;
//! writes go through a 5000-byte ring buffer drained by a dedicated writer
//! thread, as in Java. Prod wraps the same byte stream in a WebSocket.
//! After `close` (`dummy`), reads report 0 / EOF and writes are no-ops.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::io::{AsRawFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, RawSocket};

use native_tls::TlsStream;
use tungstenite::protocol::WebSocket;
use tungstenite::Message;

use crate::{uses_secure_transport, BotTarget};

const BUF_SIZE: usize = 5000;
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// `available()` peek buffer; larger than any 274 `psize` (variable-size
/// packets read their length via `g2` and never exceed a few KiB).
const AVAILABLE_BUF: usize = 8192;

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
}

/// Underlying WS transport. Production is TLS; plain TCP is test-only loopback
/// so regressions do not need a production TLS trust bypass.
enum WsConn {
    Tls(WebSocket<TlsStream<TcpStream>>),
    /// Loopback plain WS used by unit tests only (no TLS trust bypass).
    #[cfg(test)]
    Plain(WebSocket<TcpStream>),
}

impl WsConn {
    // tungstenite::Error is large (~136B); boxing would allocate on the hot WS path.
    #[allow(clippy::result_large_err)]
    fn read_message(&mut self) -> Result<Message, tungstenite::Error> {
        match self {
            WsConn::Tls(ws) => ws.read(),
            #[cfg(test)]
            WsConn::Plain(ws) => ws.read(),
        }
    }

    #[allow(clippy::result_large_err)]
    fn send_message(&mut self, msg: Message) -> Result<(), tungstenite::Error> {
        match self {
            WsConn::Tls(ws) => ws.send(msg),
            #[cfg(test)]
            WsConn::Plain(ws) => ws.send(msg),
        }
    }

    #[allow(clippy::result_large_err)]
    fn close(&mut self) -> Result<(), tungstenite::Error> {
        match self {
            WsConn::Tls(ws) => ws.close(None),
            #[cfg(test)]
            WsConn::Plain(ws) => ws.close(None),
        }
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        match self {
            WsConn::Tls(ws) => ws.get_ref().get_ref().set_nonblocking(nonblocking),
            #[cfg(test)]
            WsConn::Plain(ws) => ws.get_ref().set_nonblocking(nonblocking),
        }
    }
}

struct WsInner {
    ws: Mutex<WsConn>,
    leftover: Mutex<VecDeque<u8>>,
    dummy: Mutex<bool>,
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
}

fn io_other(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

impl ClientStream {
    /// Connect. Local is TCP `host:port`. Prod is WSS `wss://host/` with
    /// the `binary` subprotocol (game port 443); `port` is ignored.
    pub fn connect(host: &str, port: u16) -> io::Result<ClientStream> {
        if uses_secure_transport(crate::bot_target()) {
            return Self::connect_wss(host, 443);
        }
        Self::connect_tcp(host, port)
    }

    /// Connect using an explicit frozen transport identity. Unlike the legacy
    /// wrapper, secure sessions honor the supplied port.
    pub fn connect_for(target: BotTarget, host: &str, port: u16) -> io::Result<ClientStream> {
        if uses_secure_transport(target) {
            return Self::connect_wss(host, port);
        }
        Self::connect_tcp(host, port)
    }

    fn connect_tcp(host: &str, port: u16) -> io::Result<ClientStream> {
        let socket = TcpStream::connect((host, port))?;
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
            }),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
        })
    }

    fn connect_wss(host: &str, port: u16) -> io::Result<ClientStream> {
        use tungstenite::client::IntoClientRequest;
        let tcp = TcpStream::connect((host, port))?;
        tcp.set_read_timeout(Some(READ_TIMEOUT))?;
        tcp.set_nodelay(true)?;
        #[cfg(unix)]
        let fd = tcp.as_raw_fd();
        #[cfg(windows)]
        let socket = tcp.as_raw_socket();
        let connector = native_tls::TlsConnector::new().map_err(io_other)?;
        let tls = connector.connect(host, tcp).map_err(io_other)?;
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
        let (ws, _) = tungstenite::client::client(req, tls).map_err(io_other)?;
        Ok(ClientStream {
            inner: Inner::Ws(Box::new(WsInner {
                ws: Mutex::new(WsConn::Tls(ws)),
                leftover: Mutex::new(VecDeque::new()),
                dummy: Mutex::new(false),
                #[cfg(unix)]
                fd,
                #[cfg(windows)]
                socket,
            })),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
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

    fn read_bytes_inner(&mut self, dst: &mut [u8]) -> io::Result<i32> {
        match &mut self.inner {
            Inner::Tcp(t) => {
                if t.shared.lock().unwrap().dummy {
                    return Ok(0);
                }
                match t.reader.read(dst) {
                    Ok(0) => Ok(-1),
                    Ok(n) => {
                        self.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
                        Ok(n as i32)
                    }
                    Err(e) => Err(e),
                }
            }
            Inner::Ws(w) => {
                if *w.dummy.lock().unwrap() {
                    return Ok(0);
                }
                fill_ws_blocking(w)?;
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
                    Ok(0) => 0,
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

    /// Queue `len` bytes for the writer thread (Java `write(count, data, off)`).
    pub fn write(&mut self, buf: &[u8], len: usize) -> io::Result<()> {
        let len = len.min(buf.len());
        match &mut self.inner {
            Inner::Tcp(t) => tcp_write(t, buf, len, &self.bytes_out),
            Inner::Ws(w) => {
                if *w.dummy.lock().unwrap() {
                    return Ok(());
                }
                let payload = buf[..len].to_vec();
                w.ws.lock()
                    .unwrap()
                    .send_message(Message::Binary(payload))
                    .map_err(io_other)?;
                self.bytes_out.fetch_add(len as u64, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    /// Java `close`: mark dummy, shut the socket, stop the writer thread.
    pub fn close(&mut self) {
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
                    let _ = handle.join();
                }
            }
            Inner::Ws(w) => {
                *w.dummy.lock().unwrap() = true;
                let _ = w.ws.lock().unwrap().close();
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
                ws.send_message(Message::Pong(p)).map_err(io_other)?;
            }
            Ok(Message::Pong(_)) | Ok(Message::Frame(_)) | Ok(Message::Text(_)) => {}
            Ok(Message::Close(_))
            | Err(tungstenite::Error::ConnectionClosed)
            | Err(tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(e) => return Err(map_ws_err(e)),
        }
    }
}

/// Non-blocking drain used by `available`: pull every ready binary frame into
/// leftover (capped), including frames already held in tungstenite/TLS when
/// the TCP socket is quiet.
fn fill_ws_nonblocking(w: &WsInner) -> io::Result<()> {
    let mut ws = w.ws.lock().unwrap();
    ws.set_nonblocking(true)?;
    let result = (|| loop {
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
            Ok(Message::Ping(p)) => {
                ws.send_message(Message::Pong(p)).map_err(io_other)?;
            }
            Ok(Message::Pong(_)) | Ok(Message::Frame(_)) | Ok(Message::Text(_)) => {}
            Ok(Message::Close(_))
            | Err(tungstenite::Error::ConnectionClosed)
            | Err(tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                return Ok(());
            }
            Err(e) => return Err(map_ws_err(e)),
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

fn map_ws_err(err: tungstenite::Error) -> io::Error {
    match err {
        tungstenite::Error::Io(e) => e,
        other => io_other(other),
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
    let mut fds = [libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }];
    let rc = unsafe { libc::poll(fds.as_mut_ptr(), 1, 0) };
    rc > 0
}

#[cfg(all(test, windows))]
fn socket_readable_now(socket: RawSocket) -> bool {
    use windows_sys::Win32::Networking::WinSock::{WSAPoll, POLLIN, SOCKET, WSAPOLLFD};
    let mut fds = [WSAPOLLFD {
        fd: socket as SOCKET,
        events: POLLIN,
        revents: 0,
    }];
    let rc = unsafe { WSAPoll(fds.as_mut_ptr(), 1, 0) };
    rc > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use tungstenite::WebSocket as TungsteniteWs;

    /// Plain WS client over loopback (no TLS). Test-only transport.
    fn connect_ws_plain(host: &str, port: u16) -> io::Result<ClientStream> {
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
        let (ws, _) = tungstenite::client::client(req, tcp).map_err(io_other)?;
        Ok(ClientStream {
            inner: Inner::Ws(Box::new(WsInner {
                ws: Mutex::new(WsConn::Plain(ws)),
                leftover: Mutex::new(VecDeque::new()),
                dummy: Mutex::new(false),
                #[cfg(unix)]
                fd,
                #[cfg(windows)]
                socket,
            })),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
        })
    }

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
    fn ws_close_then_read_is_dummy_zero_not_error() {
        let (addr, server) = spawn_ws_server(|mut ws| {
            ws.send(Message::Binary(vec![1])).unwrap();
            let _ = ws.close(None);
            let _ = ws.read();
        });
        let mut stream = connect_ws_plain(&addr.ip().to_string(), addr.port()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while stream.available().unwrap() < 1 {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(stream.read().unwrap(), 1);
        // Peer close: further blocking read reports EOF (-1) once drained.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match stream.read() {
                Ok(-1) | Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        stream.close();
        assert_eq!(stream.read().unwrap(), 0);
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
}
