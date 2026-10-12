//! Login handshake error, mirroring the response code and the two
//! `loginMes` lines the 274 client would show on the title screen.

use std::fmt;
use std::io;

#[derive(Debug)]
pub struct LoginError {
    pub code: i32,
    pub mes1: String,
    pub mes2: String,
    /// Server-directed delay before retrying the same endpoint. Present for
    /// response 21 (world-hop profile transfer cooldown).
    pub retry_after: Option<std::time::Duration>,
    /// Why the connection failed before the server sent a response code
    /// (code -1). `None` when the server answered.
    pub transport: Option<LoginTransportError>,
}

impl LoginError {
    /// A login the server answered with response `code`.
    pub fn response(code: i32, mes1: impl Into<String>, mes2: impl Into<String>) -> Self {
        Self {
            code,
            mes1: mes1.into(),
            mes2: mes2.into(),
            retry_after: None,
            transport: None,
        }
    }
}

/// The handshake step a transport failure interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStage {
    /// Opening the socket (DNS, TCP, and for WSS the TLS and WebSocket
    /// upgrade).
    Connect,
    /// Sending the login-server request (opcode 14).
    SendHello,
    /// Reading the server's reply to that request.
    ReadHello,
    /// Reading the eight-byte login seed.
    ReadSeed,
    /// Sending the encrypted login block.
    SendLogin,
    /// Reading the login response code.
    ReadResponse,
    /// Reading the seconds of a response-21 transfer wait.
    ReadTransferDelay,
    /// Reading the accepted login's trailing bytes.
    ReadAccept,
}

impl LoginStage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::SendHello => "send_hello",
            Self::ReadHello => "read_hello",
            Self::ReadSeed => "read_seed",
            Self::SendLogin => "send_login",
            Self::ReadResponse => "read_response",
            Self::ReadTransferDelay => "read_transfer_delay",
            Self::ReadAccept => "read_accept",
        }
    }
}

impl fmt::Display for LoginStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Longest error text kept in a receipt.
const DETAIL_CAP: usize = 160;

/// A socket failure during the login handshake: where, the I/O error kind,
/// the OS error number when there is one, and the error text (bounded).
/// Holds no credentials or login payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginTransportError {
    pub stage: LoginStage,
    pub kind: io::ErrorKind,
    pub errno: Option<i32>,
    pub detail: String,
}

impl LoginTransportError {
    pub fn from_io(stage: LoginStage, error: &io::Error) -> Self {
        let mut detail = error.to_string();
        if detail.len() > DETAIL_CAP {
            let mut end = DETAIL_CAP;
            while !detail.is_char_boundary(end) {
                end -= 1;
            }
            detail.truncate(end);
        }
        Self {
            stage,
            kind: error.kind(),
            errno: error.raw_os_error(),
            detail,
        }
    }

    /// The server closed the connection before it sent a response code.
    pub fn closed(stage: LoginStage) -> Self {
        Self {
            stage,
            kind: io::ErrorKind::UnexpectedEof,
            errno: None,
            detail: "connection closed before the server answered".into(),
        }
    }
}

impl fmt::Display for LoginTransportError {
    /// `connect: Connection refused (os error 111)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.stage, self.detail)
    }
}
