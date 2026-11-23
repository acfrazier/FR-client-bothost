//! When the bytes of an update-server HTTP response are a whole body.
//!
//! The blocking readers in `client.rs` and the stepped `unpack::http::HttpFetch`
//! read a response until the connection ends, and then ask [`reply_body`]
//! what the bytes are worth. The connection can end two ways: [`ReplyEnd::Closed`]
//! is the peer's orderly end (a TCP FIN, or a TLS `close_notify`), and
//! [`ReplyEnd::Broken`] is every read error (a TLS close without
//! `close_notify`, which OpenSSL 4 reports as "unexpected eof", a reset, a
//! timeout). A reader keeps the bytes that arrived before a read error and
//! hands them over; it never throws them away with the error.
//!
//! * A `Content-Length` frames the body. A body of at least that length is
//!   whole, however the connection ended. A shorter one is
//!   [`AssetFetchError::Incomplete`], and so is never used or cached.
//! * With no `Content-Length`, the orderly end of the connection frames the
//!   body, as it always did. A broken end proves nothing by itself: the body
//!   counts only when its own check passes ([`BodyProof`]).

use super::client::{AssetFetchError, Client};
use crate::io::Packet;

/// The number of decimal digits of the public login modulus the update
/// server serves: that of the baked fallback key.
pub(crate) const LOGIN_MODULUS_DIGITS: usize = crate::PROD_LOGIN_RSAN.len();

/// How a response ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplyEnd {
    /// The peer ended the connection in order: `read` returned 0.
    Closed,
    /// A read failed: a TLS close without `close_notify`, a reset or a timeout.
    Broken,
}

/// What shows that a body with no `Content-Length` is whole when the
/// connection broke at its end.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BodyProof {
    /// `/crc`: nine checksums and the hash over them.
    Checksums,
    /// A JAG archive: its CRC is the one `/crc` named for it.
    Crc(i32),
    /// `/client/client.js`: a decimal run of exactly the login modulus length.
    LoginKey,
}

impl BodyProof {
    fn proves(self, body: &[u8]) -> bool {
        match self {
            Self::Checksums => Client::parse_jag_checksums(body).is_ok(),
            Self::Crc(crc) => Packet::getcrc(body, 0, body.len()) == crc,
            Self::LoginKey => Client::login_modulus_from_client_js(body)
                .is_some_and(|key| key.len() == LOGIN_MODULUS_DIGITS),
        }
    }
}

/// The TLS connector of the update-server fetches. A test build also trusts
/// the CA of the test fixtures, so that a test can serve TLS from a local
/// server with no change to the machine's trust.
pub(crate) fn tls_connector() -> Result<native_tls::TlsConnector, native_tls::Error> {
    #[cfg(test)]
    {
        let ca =
            native_tls::Certificate::from_pem(include_bytes!("../../tests/fixtures/tls/ca.pem"))?;
        native_tls::TlsConnector::builder()
            .add_root_certificate(ca)
            .build()
    }
    #[cfg(not(test))]
    {
        native_tls::TlsConnector::new()
    }
}

/// The body of the 200 response in `received`, which a read of the
/// connection gathered before it ended as `end` says. `Connection` when the
/// bytes are not a 200 response with a complete head.
pub(crate) fn reply_body(
    received: &[u8],
    end: ReplyEnd,
    proof: BodyProof,
) -> Result<Vec<u8>, AssetFetchError> {
    if !received.starts_with(b"HTTP/1.0 200 ") && !received.starts_with(b"HTTP/1.1 200 ") {
        return Err(AssetFetchError::Connection);
    }
    let split = received
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(AssetFetchError::Connection)?;
    let (head, body) = (&received[..split], &received[split + 4..]);
    match (content_length(head), end) {
        (Some(length), _) if body.len() >= length => Ok(body.to_vec()),
        (Some(_), _) => Err(AssetFetchError::Incomplete),
        (None, ReplyEnd::Closed) => Ok(body.to_vec()),
        (None, ReplyEnd::Broken) if proof.proves(body) => Ok(body.to_vec()),
        (None, ReplyEnd::Broken) => Err(AssetFetchError::Incomplete),
    }
}

/// The `Content-Length` of a response head. `None` when there is none, or
/// when the head names no single number: the body is then unframed.
fn content_length(head: &[u8]) -> Option<usize> {
    let head = std::str::from_utf8(head).ok()?;
    let mut length = None;
    for line in head.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("content-length") {
            continue;
        }
        let value = value.trim().parse::<usize>().ok()?;
        if length.is_some_and(|seen| seen != value) {
            return None;
        }
        length = Some(value);
    }
    length
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(headers: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes = format!("HTTP/1.0 200 OK\r\n{headers}\r\n").into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn a_framed_body_is_whole_however_the_connection_ended() {
        let bytes = reply("Content-Length: 4\r\n", b"abcd");
        for end in [ReplyEnd::Closed, ReplyEnd::Broken] {
            assert_eq!(
                reply_body(&bytes, end, BodyProof::Crc(0)),
                Ok(b"abcd".to_vec())
            );
        }
    }

    #[test]
    fn a_body_shorter_than_its_length_is_incomplete_however_the_connection_ended() {
        let bytes = reply("Content-Length: 5\r\n", b"abcd");
        for end in [ReplyEnd::Closed, ReplyEnd::Broken] {
            assert_eq!(
                reply_body(&bytes, end, BodyProof::Crc(Packet::getcrc(b"abcd", 0, 4))),
                Err(AssetFetchError::Incomplete)
            );
        }
    }

    #[test]
    fn the_length_header_matches_in_any_case_and_a_value_that_is_not_one_number_unframes() {
        let lower = reply("content-length: 5\r\n", b"abcd");
        assert_eq!(
            reply_body(&lower, ReplyEnd::Closed, BodyProof::LoginKey),
            Err(AssetFetchError::Incomplete)
        );
        let same_twice = reply("Content-Length: 5\r\ncontent-length: 5\r\n", b"abcd");
        assert_eq!(
            reply_body(&same_twice, ReplyEnd::Closed, BodyProof::LoginKey),
            Err(AssetFetchError::Incomplete)
        );
        for headers in [
            "Content-Length: 5\r\nContent-Length: 9\r\n",
            "Content-Length: five\r\n",
        ] {
            assert_eq!(
                reply_body(
                    &reply(headers, b"abcd"),
                    ReplyEnd::Closed,
                    BodyProof::LoginKey
                ),
                Ok(b"abcd".to_vec()),
                "{headers:?}"
            );
        }
    }

    #[test]
    fn an_unframed_body_is_whole_at_an_orderly_end_and_needs_its_proof_at_a_broken_one() {
        let crc = Packet::getcrc(b"abcd", 0, 4);
        let bytes = reply("", b"abcd");
        assert_eq!(
            reply_body(&bytes, ReplyEnd::Closed, BodyProof::Crc(crc ^ 1)),
            Ok(b"abcd".to_vec())
        );
        assert_eq!(
            reply_body(&bytes, ReplyEnd::Broken, BodyProof::Crc(crc)),
            Ok(b"abcd".to_vec())
        );
        assert_eq!(
            reply_body(&bytes, ReplyEnd::Broken, BodyProof::Crc(crc ^ 1)),
            Err(AssetFetchError::Incomplete)
        );
    }

    #[test]
    fn a_login_key_with_no_length_must_have_the_exact_modulus_length_at_a_broken_end() {
        let js = |digits: usize| {
            reply(
                "",
                format!("var n=\"{}\";", &crate::PROD_LOGIN_RSAN[..digits]).as_bytes(),
            )
        };
        let full = js(LOGIN_MODULUS_DIGITS);
        assert!(reply_body(&full, ReplyEnd::Broken, BodyProof::LoginKey).is_ok());
        let cut = js(LOGIN_MODULUS_DIGITS - 29);
        assert_eq!(
            reply_body(&cut, ReplyEnd::Broken, BodyProof::LoginKey),
            Err(AssetFetchError::Incomplete)
        );
        assert!(reply_body(&cut, ReplyEnd::Closed, BodyProof::LoginKey).is_ok());
    }

    #[test]
    fn anything_but_a_200_with_a_whole_head_is_a_connection_failure() {
        for bytes in [
            &b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n"[..],
            b"HTTP/1.0 200 OK\r\nContent-Length: 4\r\n",
            b"",
        ] {
            for end in [ReplyEnd::Closed, ReplyEnd::Broken] {
                assert_eq!(
                    reply_body(bytes, end, BodyProof::Checksums),
                    Err(AssetFetchError::Connection)
                );
            }
        }
    }
}
