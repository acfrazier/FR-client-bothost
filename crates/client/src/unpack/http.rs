//! One update-server HTTP GET for the stepped runtime cache fill, waited for
//! in slices so that no fill step waits for the server past its deadline.
//!
//! The request bytes and the result are those of the client's blocking fetch
//! (`Client::http_get_for`): `GET {path} HTTP/1.0` with a `Host` header over
//! TCP, and also `Connection: close` over TLS; the body after the first blank
//! line of a 200 response. A connection that stays quiet fails after the same
//! read timeout, 2 s over TCP and 8 s over TLS, counted from the last bytes
//! received. The caller looks up the host's addresses: the fetch never
//! calls the resolver, which cannot be interrupted.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::client::client::AssetFetchError;
use crate::client::Client;
use crate::io::client_stream::{park_until_ready, PendingConnect, Readiness};
use crate::Transport;

/// Read timeout of `Client::http_get_plain`.
const TCP_QUIET_LIMIT: Duration = Duration::from_secs(2);
/// Read timeout of `Client::https_get` without a login-key timeout.
const TLS_QUIET_LIMIT: Duration = Duration::from_secs(8);

pub(super) struct HttpFetch {
    host: String,
    request: Vec<u8>,
    /// The TLS connector for a fetch over TLS; `None` over plain TCP.
    tls: Option<native_tls::TlsConnector>,
    quiet_limit: Duration,
    /// When the connection last made progress; `None` while connecting,
    /// which waits as long as the system's connect does.
    progress_at: Option<Instant>,
    state: State,
}

enum State {
    Connecting(PendingConnect),
    Handshaking(native_tls::MidHandshakeTlsStream<TcpStream>),
    Sending { conn: Conn, sent: usize },
    Receiving { conn: Conn, response: Vec<u8> },
    Failed,
}

enum Conn {
    Tcp(TcpStream),
    Tls(Box<native_tls::TlsStream<TcpStream>>),
}

impl Conn {
    fn socket(&self) -> &TcpStream {
        match self {
            Self::Tcp(socket) => socket,
            Self::Tls(stream) => stream.get_ref(),
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(socket) => socket.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(socket) => socket.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }
}

/// What one non-blocking pass of [`HttpFetch::advance`] got to.
enum Advance {
    Done(Vec<u8>),
    /// The socket must become ready for this before the fetch can go on.
    Blocked(Readiness),
}

const FAILED: AssetFetchError = AssetFetchError::Connection;

impl HttpFetch {
    /// Start the connect to `addrs`, the addresses of `host` that the
    /// caller looked up. No other wait happens here. `host` names the
    /// server in the request and for TLS.
    pub(super) fn start(
        transport: Transport,
        host: &str,
        addrs: &[SocketAddr],
        path: &str,
    ) -> Result<Self, AssetFetchError> {
        let tls = if transport.uses_tls() {
            Some(native_tls::TlsConnector::new().map_err(|_| FAILED)?)
        } else {
            None
        };
        Ok(Self::start_with(tls, host, addrs, path))
    }

    fn start_with(
        tls: Option<native_tls::TlsConnector>,
        host: &str,
        addrs: &[SocketAddr],
        path: &str,
    ) -> Self {
        let request = if tls.is_some() {
            format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n")
        } else {
            format!("GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n")
        };
        Self {
            host: host.to_string(),
            request: request.into_bytes(),
            quiet_limit: if tls.is_some() {
                TLS_QUIET_LIMIT
            } else {
                TCP_QUIET_LIMIT
            },
            tls,
            progress_at: None,
            state: State::Connecting(PendingConnect::start(addrs.to_vec())),
        }
    }

    /// Go on with the fetch until `until`. `Ok(Some)` is the response body;
    /// `Ok(None)` means the fetch is still in flight at `until`. After an
    /// error the fetch is over.
    pub(super) fn poll(&mut self, until: Instant) -> Result<Option<Vec<u8>>, AssetFetchError> {
        loop {
            if let State::Connecting(connect) = &mut self.state {
                match connect.poll(until) {
                    Ok(Some(socket)) => self.connected(socket)?,
                    Ok(None) => return Ok(None),
                    Err(_) => return Err(self.fail()),
                }
                continue;
            }
            let ready = match self.advance()? {
                Advance::Done(body) => return Ok(Some(body)),
                Advance::Blocked(ready) => ready,
            };
            let now = Instant::now();
            let quiet_at = self.progress_at.map(|at| at + self.quiet_limit);
            if quiet_at.is_some_and(|quiet_at| now >= quiet_at) {
                return Err(self.fail());
            }
            if now >= until {
                return Ok(None);
            }
            let wake = quiet_at.map_or(until, |quiet_at| quiet_at.min(until));
            if let Some(socket) = self.socket() {
                if park_until_ready(socket, ready, wake.saturating_duration_since(now)) {
                    self.progress_at = Some(Instant::now());
                }
            }
        }
    }

    fn fail(&mut self) -> AssetFetchError {
        self.state = State::Failed;
        FAILED
    }

    fn socket(&self) -> Option<&TcpStream> {
        match &self.state {
            State::Handshaking(mid) => Some(mid.get_ref()),
            State::Sending { conn, .. } | State::Receiving { conn, .. } => Some(conn.socket()),
            State::Connecting(_) | State::Failed => None,
        }
    }

    fn connected(&mut self, socket: TcpStream) -> Result<(), AssetFetchError> {
        if socket.set_nonblocking(true).is_err() {
            return Err(self.fail());
        }
        self.progress_at = Some(Instant::now());
        let Some(connector) = &self.tls else {
            self.state = State::Sending {
                conn: Conn::Tcp(socket),
                sent: 0,
            };
            return Ok(());
        };
        let handshake = connector.connect(&self.host, socket);
        self.handshake(handshake)
    }

    fn handshake(
        &mut self,
        result: Result<native_tls::TlsStream<TcpStream>, native_tls::HandshakeError<TcpStream>>,
    ) -> Result<(), AssetFetchError> {
        self.state = match result {
            Ok(stream) => State::Sending {
                conn: Conn::Tls(Box::new(stream)),
                sent: 0,
            },
            Err(native_tls::HandshakeError::WouldBlock(mid)) => State::Handshaking(mid),
            Err(native_tls::HandshakeError::Failure(_)) => return Err(self.fail()),
        };
        Ok(())
    }

    /// Do every part of the fetch that needs no wait.
    fn advance(&mut self) -> Result<Advance, AssetFetchError> {
        loop {
            match std::mem::replace(&mut self.state, State::Failed) {
                State::Handshaking(mid) => {
                    self.handshake(mid.handshake())?;
                    if matches!(self.state, State::Handshaking(_)) {
                        return Ok(Advance::Blocked(Readiness::Read));
                    }
                }
                State::Sending { mut conn, mut sent } => match conn.write(&self.request[sent..]) {
                    Ok(0) => return Err(FAILED),
                    Ok(n) => {
                        sent += n;
                        self.state = if sent == self.request.len() {
                            State::Receiving {
                                conn,
                                response: Vec::new(),
                            }
                        } else {
                            State::Sending { conn, sent }
                        };
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        self.state = State::Sending { conn, sent };
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        self.state = State::Sending { conn, sent };
                        return Ok(Advance::Blocked(Readiness::Write));
                    }
                    Err(_) => return Err(FAILED),
                },
                State::Receiving {
                    mut conn,
                    mut response,
                } => {
                    let mut chunk = [0u8; 16 * 1024];
                    match conn.read(&mut chunk) {
                        Ok(0) => {
                            return Client::split_http_body(&response)
                                .map(Advance::Done)
                                .ok_or(FAILED);
                        }
                        Ok(n) => {
                            response.extend_from_slice(&chunk[..n]);
                            self.progress_at = Some(Instant::now());
                            self.state = State::Receiving { conn, response };
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                            self.state = State::Receiving { conn, response };
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            self.state = State::Receiving { conn, response };
                            return Ok(Advance::Blocked(Readiness::Read));
                        }
                        Err(_) => return Err(FAILED),
                    }
                }
                State::Failed => return Err(FAILED),
                State::Connecting(_) => unreachable!("poll connects before it advances"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    const STEP: Duration = Duration::from_millis(8);
    /// Generous for a debug build: far below the server delays.
    const STEP_BOUND: Duration = Duration::from_millis(60);

    /// Poll `fetch` with `STEP` deadlines until it ends; the result, the
    /// number of polls and the longest poll.
    fn poll_steps(fetch: &mut HttpFetch) -> (Result<Vec<u8>, AssetFetchError>, usize, Duration) {
        let mut polls = 0;
        let mut longest = Duration::ZERO;
        loop {
            let started = Instant::now();
            let result = fetch.poll(started + STEP);
            longest = longest.max(started.elapsed());
            polls += 1;
            match result {
                Ok(None) => {}
                Ok(Some(body)) => return (Ok(body), polls, longest),
                Err(error) => return (Err(error), polls, longest),
            }
        }
    }

    /// Read one request head from `socket`.
    fn read_head(socket: &mut impl Read) -> Vec<u8> {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
        }
        head
    }

    fn response(body: &[u8]) -> Vec<u8> {
        let mut response =
            format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn loopback(port: u16) -> [SocketAddr; 1] {
        [SocketAddr::from(([127, 0, 0, 1], port))]
    }

    /// A plain update server that answers 200 ms late: the fetch waits
    /// in short polls, sends the blocking fetch's exact request and
    /// returns the body.
    #[test]
    fn tcp_fetch_waits_in_steps_and_returns_the_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (head_tx, head_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            head_tx.send(read_head(&mut socket)).unwrap();
            thread::sleep(Duration::from_millis(200));
            socket.write_all(&response(b"crc-body")).unwrap();
        });
        let mut fetch =
            HttpFetch::start(Transport::Tcp, "127.0.0.1", &loopback(port), "/crc").unwrap();
        let (body, polls, longest) = poll_steps(&mut fetch);
        server.join().unwrap();
        assert_eq!(body.unwrap(), b"crc-body");
        assert_eq!(
            head_rx.recv().unwrap(),
            b"GET /crc HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n"
        );
        assert!(polls > 10, "{polls} polls");
        assert!(longest < STEP_BOUND, "longest poll {longest:?}");
    }

    /// A server that never answers fails the fetch after the blocking
    /// fetch's 2 s read timeout, not sooner and not never.
    #[test]
    fn a_quiet_server_fails_the_fetch_after_the_read_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            read_head(&mut socket);
            let _ = done_rx.recv_timeout(Duration::from_secs(10));
        });
        let started = Instant::now();
        let mut fetch =
            HttpFetch::start(Transport::Tcp, "127.0.0.1", &loopback(port), "/crc").unwrap();
        let (body, _, longest) = poll_steps(&mut fetch);
        let failed = started.elapsed();
        done_tx.send(()).unwrap();
        server.join().unwrap();
        assert_eq!(body, Err(AssetFetchError::Connection));
        assert!(
            failed >= TCP_QUIET_LIMIT && failed < TCP_QUIET_LIMIT + Duration::from_millis(500),
            "failed after {failed:?}"
        );
        assert!(longest < STEP_BOUND, "longest poll {longest:?}");
    }

    /// The HTTPS fetch of a TLS asset server: the TLS handshake and the
    /// response wait in short polls, the request is the blocking fetch's,
    /// and the body arrives intact. The fixture CA signs the server
    /// certificate. This test adds the CA to its TLS connector.
    #[test]
    fn tls_fetch_waits_in_steps_and_returns_the_body() {
        let cert = include_bytes!("../../tests/fixtures/tls/localhost.pem");
        let key = include_bytes!("../../tests/fixtures/tls/localhost.key");
        let identity = native_tls::Identity::from_pkcs8(cert, key).unwrap();
        let acceptor = native_tls::TlsAcceptor::new(identity).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body: Vec<u8> = (0..40_000u32).map(|n| (n % 251) as u8).collect();
        let served = body.clone();
        let (head_tx, head_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            // Hold the TLS handshake, then the response.
            thread::sleep(Duration::from_millis(100));
            let mut tls = acceptor.accept(socket).unwrap();
            head_tx.send(read_head(&mut tls)).unwrap();
            thread::sleep(Duration::from_millis(100));
            tls.write_all(&response(&served)).unwrap();
            tls.shutdown().unwrap();
        });
        let ca = include_bytes!("../../tests/fixtures/tls/ca.pem");
        let connector = native_tls::TlsConnector::builder()
            .add_root_certificate(native_tls::Certificate::from_pem(ca).unwrap())
            .build()
            .unwrap();
        let mut fetch =
            HttpFetch::start_with(Some(connector), "localhost", &loopback(port), "/config1234");
        let (fetched, polls, longest) = poll_steps(&mut fetch);
        server.join().unwrap();
        assert_eq!(fetched.unwrap(), body);
        assert_eq!(
            head_rx.recv().unwrap(),
            b"GET /config1234 HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        );
        assert!(polls > 10, "{polls} polls");
        assert!(longest < STEP_BOUND, "longest poll {longest:?}");
    }
}
