//! Blocking TCP stream, 1:1 port of javaclient `ClientStream` (not the TS WebSocket).
//!
//! Reads are blocking on the calling thread with a 30 s soTimeout; writes go
//! through a 5000-byte ring buffer drained by a dedicated writer thread, as in
//! Java. After `close` (`dummy`), reads report 0 / EOF and writes are no-ops.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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

pub struct ClientStream {
    reader: TcpStream,
    writer_sock: TcpStream,
    shared: Arc<Mutex<WriterState>>,
    condvar: Arc<Condvar>,
    writer_thread: Option<JoinHandle<()>>,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
}

impl ClientStream {
    /// Connect to `host:port` with a 30 s read timeout and TCP_NODELAY.
    pub fn connect(host: &str, port: u16) -> io::Result<ClientStream> {
        let socket = TcpStream::connect((host, port))?;
        socket.set_read_timeout(Some(READ_TIMEOUT))?;
        socket.set_nodelay(true)?;
        let writer_sock = socket.try_clone()?;
        Ok(ClientStream {
            reader: socket,
            writer_sock,
            shared: Arc::new(Mutex::new(WriterState::new())),
            condvar: Arc::new(Condvar::new()),
            writer_thread: None,
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
        self.reader.as_raw_fd()
    }

    /// Read one byte: 0 after `close`, -1 at EOF, else the byte value.
    pub fn read(&mut self) -> io::Result<i32> {
        if self.shared.lock().unwrap().dummy {
            return Ok(0);
        }
        let mut b = [0u8; 1];
        match self.reader.read(&mut b) {
            Ok(0) => Ok(-1),
            Ok(_) => {
                self.bytes_in.fetch_add(1, Ordering::Relaxed);
                Ok(b[0] as i32)
            }
            Err(e) => Err(e),
        }
    }

    /// Read exactly `len` bytes into `buf[off..off + len]`; error on EOF.
    /// After `close`, returns without touching the buffer (as Java).
    pub fn read_bytes(&mut self, buf: &mut [u8], off: usize, len: usize) -> io::Result<()> {
        if self.shared.lock().unwrap().dummy {
            return Ok(());
        }
        let dst = &mut buf[off..off + len];
        let mut filled = 0;
        while filled < dst.len() {
            let n = self.reader.read(&mut dst[filled..])?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "EOF"));
            }
            filled += n;
        }
        self.bytes_in.fetch_add(len as u64, Ordering::Relaxed);
        Ok(())
    }

    /// Bytes readable without blocking — the kernel receive-buffer count,
    /// capped at `AVAILABLE_BUF` (Java `SocketInputStream.available`
    /// estimate). `Client::tcp_in` (Task 16) relies on the exact count for its
    /// `available < psize` back-pressure check, so this is a full peek, not a
    /// 0/1 probe.
    pub fn available(&mut self) -> io::Result<i32> {
        if self.shared.lock().unwrap().dummy {
            return Ok(0);
        }
        self.reader.set_nonblocking(true)?;
        let mut b = [0u8; AVAILABLE_BUF];
        let n = match self.reader.peek(&mut b) {
            Ok(0) => 0,
            Ok(n) => n as i32,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => 0,
            Err(e) => {
                self.reader.set_nonblocking(false)?;
                return Err(e);
            }
        };
        self.reader.set_nonblocking(false)?;
        Ok(n)
    }

    /// Queue `len` bytes for the writer thread (Java `write(count, data, off)`).
    pub fn write(&mut self, buf: &[u8], len: usize) -> io::Result<()> {
        let len = len.min(buf.len());
        let sock = self.writer_sock.try_clone()?;
        let shared = self.shared.clone();
        let condvar = self.condvar.clone();
        let mut st = self.shared.lock().unwrap();
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
                self.bytes_out.fetch_add(queued, Ordering::Relaxed);
                return Err(io::Error::other("buffer overflow"));
            }
        }
        self.bytes_out.fetch_add(queued, Ordering::Relaxed);
        if !st.writer {
            st.writer = true;
            self.writer_thread = Some(thread::spawn(move || writer_loop(shared, condvar, sock)));
        }
        self.condvar.notify_one();
        Ok(())
    }

    /// Java `close`: mark dummy, shut the socket, stop the writer thread.
    pub fn close(&mut self) {
        {
            let mut st = self.shared.lock().unwrap();
            if st.dummy {
                return;
            }
            st.dummy = true;
            st.writer = false;
        }
        self.condvar.notify_all();
        let _ = self.reader.shutdown(Shutdown::Both);
        let _ = self.writer_sock.shutdown(Shutdown::Both);
        if let Some(handle) = self.writer_thread.take() {
            let _ = handle.join();
        }
    }
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
