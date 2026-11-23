//! A local update server for the tests of how an HTTP response ends, and the
//! table of ends that every reader of an update-server response must pass:
//! the blocking readers in `client.rs` and the stepped `unpack::http::HttpFetch`.

use std::fmt::Debug;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::client::AssetFetchError;
use crate::Transport;

/// How long the server waits after the last byte before it ends the
/// connection: the client has read the whole response by then and waits in
/// `read`, as it does for a real server that ends a connection a while after
/// its response.
const PAUSE: Duration = Duration::from_millis(150);

/// How the server ends the connection after the response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum End {
    /// Orderly: a TCP FIN, over TLS after `close_notify`.
    Clean,
    /// A TCP FIN with no TLS `close_notify` (over plain TCP the same as
    /// [`End::Clean`]): OpenSSL 4 reports "unexpected eof".
    NoCloseNotify,
    /// A TCP reset.
    Reset,
}

/// A response with `Content-Length: {declared}` and `body`.
pub(crate) fn framed(body: &[u8], declared: usize) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.0 200 OK\r\nContent-Length: {declared}\r\n\r\n").into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// A response with no `Content-Length`, which the end of the connection frames.
pub(crate) fn unframed(body: &[u8]) -> Vec<u8> {
    let mut bytes = b"HTTP/1.0 200 OK\r\nConnection: close\r\n\r\n".to_vec();
    bytes.extend_from_slice(body);
    bytes
}

/// The TCP stream of a TLS connection that can be muted: a muted wire takes
/// bytes and sends none, so that dropping the TLS stream cannot send a
/// `close_notify`, whichever TLS library the build uses.
#[derive(Debug)]
struct Wire {
    tcp: TcpStream,
    muted: Arc<AtomicBool>,
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.tcp).read(buf)
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.muted.load(Ordering::SeqCst) {
            return Ok(buf.len());
        }
        (&self.tcp).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.muted.load(Ordering::SeqCst) {
            return Ok(());
        }
        (&self.tcp).flush()
    }
}

/// A server that answers one connection with `bytes` and ends it as `end`.
pub(crate) struct ReplyServer {
    port: u16,
    thread: JoinHandle<Vec<u8>>,
}

impl ReplyServer {
    pub(crate) fn start(transport: Transport, bytes: Vec<u8>, end: End) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let tls = transport.uses_tls().then(|| {
            let cert = include_bytes!("../../tests/fixtures/tls/localhost.pem");
            let key = include_bytes!("../../tests/fixtures/tls/localhost.key");
            let identity = native_tls::Identity::from_pkcs8(cert, key).unwrap();
            native_tls::TlsAcceptor::new(identity).unwrap()
        });
        let thread = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let muted = Arc::new(AtomicBool::new(false));
            let wire = Wire {
                tcp: tcp.try_clone().unwrap(),
                muted: Arc::clone(&muted),
            };
            let head = match tls {
                Some(acceptor) => {
                    let mut stream = acceptor.accept(wire).unwrap();
                    let head = read_head(&mut stream);
                    stream.write_all(&bytes).unwrap();
                    stream.flush().unwrap();
                    thread::sleep(PAUSE);
                    if end == End::Clean {
                        stream.shutdown().unwrap();
                    } else {
                        muted.store(true, Ordering::SeqCst);
                    }
                    head
                }
                None => {
                    let mut wire = wire;
                    let head = read_head(&mut wire);
                    wire.write_all(&bytes).unwrap();
                    thread::sleep(PAUSE);
                    head
                }
            };
            // Every owner of the socket is gone when the stream above is
            // dropped, and `tcp` is the last: the socket closes here.
            if end == End::Reset {
                socket2::SockRef::from(&tcp)
                    .set_linger(Some(Duration::ZERO))
                    .unwrap();
            }
            drop(tcp);
            head
        });
        Self { port, thread }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Wait for the server to finish; the request head it read.
    pub(crate) fn finish(self) -> Vec<u8> {
        self.thread.join().unwrap()
    }
}

fn read_head(stream: &mut impl Read) -> Vec<u8> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    head
}

/// One row of the table: a response, how it ends, and what the reader
/// returns for it.
pub(crate) struct Case<T> {
    pub(crate) name: &'static str,
    pub(crate) bytes: Vec<u8>,
    pub(crate) end: End,
    pub(crate) expect: Result<T, AssetFetchError>,
    /// The row holds only where the TLS library of the build reports `end`
    /// as a read error. A library that reports it as an orderly end (OpenSSL
    /// 3 for a close without `close_notify`; Security.framework for that and
    /// for a reset) leaves the reader no way to tell it from the clean
    /// close, and the row is skipped over TLS.
    pub(crate) needs_reported_end: bool,
}

impl<T> Case<T> {
    fn new(name: &'static str, bytes: &[u8], end: End, expect: Result<T, AssetFetchError>) -> Self {
        Self {
            name,
            bytes: bytes.to_vec(),
            end,
            expect,
            needs_reported_end: false,
        }
    }

    fn needing_reported_end(mut self) -> Self {
        self.needs_reported_end = true;
        self
    }
}

/// The ends of a response that every reader of an update-server body must
/// pass, over plain TCP and over TLS. `good` is a payload that verifies;
/// `cut` is a strict prefix of it that does not, and that the reader's own
/// check refuses (for the login key it is longer than the 250 digits that
/// the old reader accepted). `payload` is what the reader returns for `good`.
pub(crate) fn end_cases<T: Clone>(good: &[u8], cut: &[u8], payload: T) -> Vec<Case<T>> {
    let whole = framed(good, good.len());
    let short = framed(cut, good.len());
    let ok = || Ok(payload.clone());
    let incomplete = || Err(AssetFetchError::Incomplete);
    vec![
        Case::new("clean control", &whole, End::Clean, ok()),
        Case::new(
            "complete, no close_notify",
            &whole,
            End::NoCloseNotify,
            ok(),
        ),
        Case::new("complete, reset", &whole, End::Reset, ok()),
        Case::new("truncated, clean", &short, End::Clean, incomplete()),
        Case::new(
            "truncated, no close_notify",
            &short,
            End::NoCloseNotify,
            incomplete(),
        ),
        Case::new("truncated, reset", &short, End::Reset, incomplete()),
        Case::new("no length, clean", &unframed(good), End::Clean, ok()),
        Case::new(
            "no length, no close_notify, payload checks",
            &unframed(good),
            End::NoCloseNotify,
            ok(),
        ),
        Case::new(
            "no length, reset, payload checks",
            &unframed(good),
            End::Reset,
            ok(),
        ),
        Case::new(
            "no length, no close_notify, payload cut",
            &unframed(cut),
            End::NoCloseNotify,
            incomplete(),
        )
        .needing_reported_end(),
        Case::new(
            "no length, reset, payload cut",
            &unframed(cut),
            End::Reset,
            incomplete(),
        )
        .needing_reported_end(),
    ]
}

/// Whether the TLS library of this build reports a connection that the
/// server ended as `end` as a read error: a client of the update-server
/// connector reads a short response to its end.
fn tls_reports(end: End) -> bool {
    let server = ReplyServer::start(Transport::Wss, framed(b"x", 1), end);
    let tcp = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    let mut stream = super::http_reply::tls_connector()
        .unwrap()
        .connect("localhost", tcp)
        .unwrap();
    stream.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    let ended = stream.read_to_end(&mut Vec::new());
    server.finish();
    ended.is_err()
}

/// Run `reader` against a server for each case, over plain TCP and TLS, and
/// check what it returns. `reader` gets the transport and the server's port.
/// Every case runs; the failure lists each one that read wrongly.
pub(crate) fn check_reader<T: Debug + PartialEq>(
    cases: &[Case<T>],
    reader: impl Fn(Transport, u16) -> Result<T, AssetFetchError>,
) {
    fn shown<T>(result: &Result<T, AssetFetchError>) -> String {
        match result {
            Ok(_) => "the payload".to_string(),
            Err(error) => format!("{error:?}"),
        }
    }
    let mut wrong = Vec::new();
    for transport in [Transport::Tcp, Transport::Wss] {
        for case in cases {
            if transport == Transport::Tcp && case.end == End::NoCloseNotify {
                continue;
            }
            if transport == Transport::Wss && case.needs_reported_end && !tls_reports(case.end) {
                eprintln!(
                    "skipped over TLS: {} (this TLS library reports no error for it)",
                    case.name
                );
                continue;
            }
            let server = ReplyServer::start(transport, case.bytes.clone(), case.end);
            let got = reader(transport, server.port());
            server.finish();
            if got != case.expect {
                wrong.push(format!(
                    "{transport:?}, {}: got {}, expected {}",
                    case.name,
                    shown(&got),
                    shown(&case.expect)
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "read wrongly:\n{}", wrong.join("\n"));
}
