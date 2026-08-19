//! Blocking TCP stream, 1:1 port of javaclient `ClientStream` (not the TS WebSocket).
//!
//! Reads are blocking on the calling thread with a 30 s soTimeout; writes go
//! through a 5000-byte ring buffer drained by a dedicated writer thread, as in
//! Java. After `close` (`dummy`), reads report 0 / EOF and writes are no-ops.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const BUF_SIZE: usize = 5000;
const READ_TIMEOUT: Duration = Duration::from_secs(30);

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
        })
    }

    /// Read one byte: 0 after `close`, -1 at EOF, else the byte value.
    pub fn read(&mut self) -> io::Result<i32> {
        if self.shared.lock().unwrap().dummy {
            return Ok(0);
        }
        let mut b = [0u8; 1];
        match self.reader.read(&mut b) {
            Ok(0) => Ok(-1),
            Ok(_) => Ok(b[0] as i32),
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
        Ok(())
    }

    /// Bytes available without blocking: 0 or 1 (Java clients only test > 0).
    pub fn available(&mut self) -> io::Result<i32> {
        if self.shared.lock().unwrap().dummy {
            return Ok(0);
        }
        self.reader.set_nonblocking(true)?;
        let mut b = [0u8; 1];
        let n = match self.reader.peek(&mut b) {
            Ok(0) => 0,
            Ok(_) => 1,
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
        for &b in &buf[..len] {
            let tnum = st.tnum;
            st.buf[tnum] = b;
            st.tnum = (st.tnum + 1) % BUF_SIZE;
            if st.tnum == (st.tcycl + BUF_SIZE - 100) % BUF_SIZE {
                return Err(io::Error::other("buffer overflow"));
            }
        }
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
