//! Game stream: Java TCP locally, WSS (`binary` subprotocol) on Prod.
//!
//! Local reads are blocking on the calling thread with a 30 s soTimeout;
//! writes go through a 5000-byte ring buffer drained by a dedicated writer
//! thread, as in Java. Prod wraps the same byte stream in a WebSocket.
//! After `close` (`dummy`), reads report 0 / EOF and writes are no-ops.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use native_tls::TlsStream;
use tungstenite::protocol::WebSocket;
use tungstenite::Message;

use crate::uses_secure_transport;

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

struct WsInner {
    ws: Mutex<WebSocket<TlsStream<TcpStream>>>,
    leftover: Mutex<VecDeque<u8>>,
    dummy: Mutex<bool>,
    fd: i32,
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
            return Self::connect_wss(host);
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

    fn connect_wss(host: &str) -> io::Result<ClientStream> {
        use tungstenite::client::IntoClientRequest;
        let tcp = TcpStream::connect((host, 443))?;
        tcp.set_read_timeout(Some(READ_TIMEOUT))?;
        tcp.set_nodelay(true)?;
        let fd = tcp.as_raw_fd();
        let connector = native_tls::TlsConnector::new().map_err(io_other)?;
        let tls = connector.connect(host, tcp).map_err(io_other)?;
        let mut req = format!("wss://{host}/")
            .into_client_request()
            .map_err(io_other)?;
        req.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            tungstenite::http::HeaderValue::from_static("binary"),
        );
        let (ws, _) = tungstenite::client::client(req, tls).map_err(io_other)?;
        Ok(ClientStream {
            inner: Inner::Ws(Box::new(WsInner {
                ws: Mutex::new(ws),
                leftover: Mutex::new(VecDeque::new()),
                dummy: Mutex::new(false),
                fd,
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
    pub fn fd(&self) -> i32 {
        match &self.inner {
            Inner::Tcp(t) => t.reader.as_raw_fd(),
            Inner::Ws(w) => w.fd,
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
                fill_ws(w)?;
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
                {
                    let n = w.leftover.lock().unwrap().len();
                    if n > 0 {
                        return Ok(n as i32);
                    }
                }
                let mut fds = [libc::pollfd {
                    fd: w.fd,
                    events: libc::POLLIN,
                    revents: 0,
                }];
                let rc = unsafe { libc::poll(fds.as_mut_ptr(), 1, 0) };
                if rc <= 0 {
                    return Ok(0);
                }
                let _ = fill_ws(w);
                Ok(w.leftover.lock().unwrap().len() as i32)
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
                    .send(Message::Binary(payload))
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
                let _ = w.ws.lock().unwrap().close(None);
            }
        }
    }
}

fn fill_ws(w: &WsInner) -> io::Result<()> {
    if !w.leftover.lock().unwrap().is_empty() {
        return Ok(());
    }
    let mut ws = w.ws.lock().unwrap();
    match ws.read() {
        Ok(Message::Binary(b)) => {
            w.leftover.lock().unwrap().extend(b);
            Ok(())
        }
        Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => Ok(()),
        Ok(_) => Ok(()),
        Err(e) => Err(io_other(e)),
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn fd_tracks_the_reader_socket_and_wakes_on_data() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut stream = ClientStream::connect(&addr.ip().to_string(), addr.port()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        assert!(stream.fd() >= 0, "fd must be a valid socket descriptor");
        // Idle: nothing readable yet.
        assert_eq!(stream.available().unwrap(), 0);
        server.write_all(&[1, 2, 3]).unwrap();
        // The fd sits on the same socket `available` peeks; wait for the
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
        assert!(stream.fd() >= 0);
    }
}
