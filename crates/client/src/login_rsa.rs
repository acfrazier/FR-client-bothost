//! Login RSA public half: the modulus is baked at compile time
//! (`LOGIN_RSAN`, from `build.rs`' `TARGET` bake) and becomes **mutable**
//! at runtime — login response 6 ("RuneScape has been updated!") refreshes
//! it from the web origin (`/loginkey`, else a `client.js` scrape) via
//! [`set_login_modulus`], exactly like rs2b0t `loginKey.ts`. The exponent
//! stays a baked constant ([`LOGIN_RSAE`]).

include!(concat!(env!("OUT_DIR"), "/login_rsa_gen.rs"));
include!("login_rsa_resolve.rs");

use std::sync::RwLock;

/// Runtime login modulus override; `None` = use the baked [`LOGIN_RSAN`].
static MODULUS: RwLock<Option<String>> = RwLock::new(None);

/// Current login modulus (decimal): the baked `LOGIN_RSAN` until a login-6
/// key refresh replaces it.
pub fn login_modulus() -> String {
    MODULUS
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| LOGIN_RSAN.to_string())
}

/// `/loginkey` response check (`/^\d{250,}$/`, rs2b0t `loginKey.ts`): the
/// body must be a plain decimal modulus of 250+ digits. Also the gate for
/// [`set_login_modulus`] — a refreshed modulus must be that long.
pub fn parse_login_modulus(s: &str) -> Option<String> {
    let t = s.trim();
    (t.len() >= 250 && t.bytes().all(|b| b.is_ascii_digit())).then(|| t.to_string())
}

/// First run of 250+ consecutive digits in `s` (`/\d{250,}/`) — the
/// `client/client.js` scrape `b0t.sh` / `loginKey.ts` use.
pub fn scrape_login_modulus(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i - start >= 250 {
                return Some(s[start..i].to_string());
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Replace the login modulus with a refreshed one (250+ decimal digits, as
/// `/loginkey` serves). Short / non-decimal values are rejected, so a bogus
/// web origin cannot wedge future handshakes.
pub fn set_login_modulus(n: &str) -> Result<(), &'static str> {
    let n = parse_login_modulus(n).ok_or("login modulus must be 250+ decimal digits")?;
    *MODULUS.write().map_err(|_| "login modulus lock poisoned")? = Some(n);
    Ok(())
}

/// Web origin for the login-key refresh: rs2b2t's live hosts serve
/// `https://{host}/client/client.js` (Cloudflare redirects plain HTTP
/// away); everything else — the local engine, test stubs — is plain HTTP
/// on `http_port`.
pub fn login_key_origin(host: &str, http_port: u16) -> (&'static str, u16) {
    if host.ends_with("rs2b2t.com") {
        ("https", 443)
    } else {
        ("http", http_port)
    }
}

/// Fetch a refreshed modulus from the web origin: `/loginkey` first (a
/// plain decimal body), then the `/client/client.js` scrape. `None` when
/// neither yields a 250+ digit modulus.
pub fn fetch_login_modulus(host: &str, port: u16, scheme: &str) -> Option<String> {
    let body = body_get(host, port, scheme, "/loginkey")?;
    if let Some(n) = parse_login_modulus(&body) {
        return Some(n);
    }
    let body = body_get(host, port, scheme, "/client/client.js")?;
    scrape_login_modulus(&body)
}

fn body_get(host: &str, port: u16, scheme: &str, path: &str) -> Option<String> {
    match scheme {
        "https" => https_get(host, path),
        _ => http_get(host, port, path).map(|b| String::from_utf8_lossy(&b).into_owned()),
    }
}

/// HTTP/1.0 `GET {path}` returning the response body, headers split on
/// `\r\n\r\n` (the same wire format `Client::http_get` uses for `/crc`).
fn http_get(host: &str, port: u16, path: &str) -> Option<Vec<u8>> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect((host, port)).ok()?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .ok()?;
    // One `write_all` so the request lands as a single segment (a `write!`
    // can split across `write_str` pieces and race a stub's first read).
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n");
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    Some(buf[split + 4..].to_vec())
}

/// HTTPS `GET` via the system `curl` (rs2b0t's `b0t.sh` fetches
/// `client.js` the same way; the client crate has no TLS dependency).
fn https_get(host: &str, path: &str) -> Option<String> {
    let url = format!("https://{host}{path}");
    let out = std::process::Command::new("curl")
        .args(["-sS", "--max-time", "15", &url])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake env map for [`resolve_rsa`].
    fn env_of<'a>(map: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| map.iter().find(|(key, _)| *key == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn login_key_parse_accepts_250_plus_digits() {
        let n = "9".repeat(300);
        assert_eq!(parse_login_modulus(&n), Some(n.clone()));
        assert_eq!(parse_login_modulus(&format!("{n}\r\n")), Some(n), "trailing CRLF ok");
        assert_eq!(parse_login_modulus(&"1".repeat(249)), None, "short rejected");
        assert_eq!(parse_login_modulus("1234abc5678"), None, "non-digits rejected");
    }

    #[test]
    fn login_key_scrape_extracts_first_250_digit_run() {
        let n = "7".repeat(300);
        let js = format!("var x=1;var KEY=\"{n}\";var y=2;");
        assert_eq!(scrape_login_modulus(&js), Some(n));
        assert_eq!(scrape_login_modulus("var short=12345;"), None);
        // First qualifying run wins, like b0t.sh's `grep | awk length>=250`.
        let first = "5".repeat(251);
        let js = format!("A{first}B{}C", "9".repeat(300));
        assert_eq!(scrape_login_modulus(&js), Some(first));
    }

    #[test]
    fn login_key_set_updates_accessor_and_rejects_short() {
        let n = "3".repeat(300);
        set_login_modulus(&n).unwrap();
        assert_eq!(login_modulus(), n);
        assert!(set_login_modulus(&"1".repeat(249)).is_err(), "short rejected");
        assert_eq!(login_modulus(), n, "failed set must not mutate");
        *MODULUS.write().unwrap() = None; // restore the baked default
    }

    #[test]
    fn login_key_resolve_live_requires_live_rsan() {
        assert!(resolve_rsa("live", env_of(&[])).is_err(), "no LIVE_RSAN must fail");
        assert!(
            resolve_rsa("live", env_of(&[("LIVE_RSAN", "")])).is_err(),
            "empty LIVE_RSAN must fail"
        );
        let n = "9".repeat(300);
        let (got_n, got_e) = resolve_rsa("live", env_of(&[("LIVE_RSAN", n.as_str())])).unwrap();
        assert_eq!(got_n, n);
        assert_eq!(got_e, "65537", "live exponent defaults to 65537");
    }

    #[test]
    fn login_key_resolve_prod_requires_prod_rsan() {
        assert!(resolve_rsa("prod", env_of(&[])).is_err());
        let (n, e) = resolve_rsa("prod", env_of(&[("PROD_RSAN", "12")])).unwrap();
        assert_eq!(n, "12");
        assert_eq!(e, "65537");
    }

    #[test]
    fn login_key_resolve_local_defaults_to_engine_pair() {
        let (n, e) = resolve_rsa("local", env_of(&[])).unwrap();
        assert_eq!(n, JAVA_N);
        assert_eq!(e, JAVA_E);
    }

    #[test]
    fn login_key_resolve_env_overrides() {
        let (n, e) = resolve_rsa("local", env_of(&[("LOCAL_RSAN", "123"), ("LOGIN_RSAE", "17")]))
            .unwrap();
        assert_eq!(n, "123");
        assert_eq!(e, "17");
        let (n, e) = resolve_rsa("live", env_of(&[("LIVE_RSAN", "1"), ("LOGIN_RSAE", "17")]))
            .unwrap();
        assert_eq!(n, "1");
        assert_eq!(e, "17", "LOGIN_RSAE overrides the live 65537 default");
    }
}
