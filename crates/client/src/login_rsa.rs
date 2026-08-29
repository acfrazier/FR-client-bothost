//! Login RSA. Prod public half is **baked** (rs2b2t; it does not rotate).
//! Local engine keys are read at runtime from `$ENGINE_DIR` / `LOGIN_RSAN`
//! — no compile-time bake, no `BOT_TARGET` rebuild.

use std::fs;
use std::path::Path;
use std::str::FromStr;

use num_bigint::BigUint;

use crate::bot_target::{bot_target, private_pem, BotTarget};

/// Java Client-TS default pair. Unit tests and a cache-less `/tmp` client
/// use this when no engine pem is present.
pub const JAVA_LOGIN_RSAN: &str = "7162900525229798032761816791230527296329313291232324290237849263501208207972894053929065636522363163621000728841182238772712427862772219676577293600221789";
pub const JAVA_LOGIN_RSAE: &str = "58778699976184461502525193738213253649000149147835990136706041084440742975821";

/// rs2b2t public modulus (scraped from `/client/client.js`, 250+ digit run).
/// Exponent 65537. Baked so a prod bin does not rebuild when flipping worlds.
pub const PROD_LOGIN_RSAN: &str = "117420683091599437363781545043460293895633275635653353309906159820872703885723869096825270694383466833728011835587324760936150761784279979493634580041806369762348843902867397790796219798581737432768036489623686153294697841819355248591000037921789209503314465546289565662596345179694574470836552536702466642733";
pub const PROD_LOGIN_RSAE: &str = "65537";

/// Active modulus for this process (prod baked, or local env/pem/Java).
pub fn login_rsan() -> String {
    active_pair().0
}

/// Active exponent for this process.
pub fn login_rsae() -> String {
    active_pair().1
}

/// `(n, e)` used by `Client::login`.
pub fn active_pair() -> (String, String) {
    match bot_target() {
        BotTarget::Prod => (PROD_LOGIN_RSAN.to_string(), PROD_LOGIN_RSAE.to_string()),
        BotTarget::Local => local_pair(),
    }
}

/// Bigints for `rsaenc`.
pub fn active_biguints() -> (BigUint, BigUint) {
    let (n, e) = active_pair();
    (
        BigUint::from_str(&n).expect("login RSA n"),
        BigUint::from_str(&e).expect("login RSA e"),
    )
}

/// Local engine RSA. Stock Lost City Server ships the Java default pair —
/// that is the usual local-dev case and needs no bake. Overrides, in order:
/// `LOGIN_RSAN` / `LOGIN_RSAE`, then `$ENGINE_DIR/data/config/private.pem`
/// if you rotated the engine key.
fn local_pair() -> (String, String) {
    if let Ok(n) = std::env::var("LOGIN_RSAN") {
        if !n.is_empty() {
            let e = std::env::var("LOGIN_RSAE").unwrap_or_else(|_| JAVA_LOGIN_RSAE.to_string());
            return (n, e);
        }
    }
    let pem = private_pem();
    if pem.is_file() {
        match rsa_from_pkcs1_pem_file(&pem) {
            Ok(pair) => return pair,
            Err(e) => eprintln!("login RSA: {}: {e}; using Lost City defaults", pem.display()),
        }
    }
    (JAVA_LOGIN_RSAN.to_string(), JAVA_LOGIN_RSAE.to_string())
}

/// Public `(n, e)` from PKCS#1 (`BEGIN RSA PRIVATE KEY`) or PKCS#8
/// (`BEGIN PRIVATE KEY`) PEM — OpenSSL 3 writes PKCS#8 by default.
pub fn rsa_from_pkcs1_pem_file(path: &Path) -> Result<(String, String), String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    rsa_from_pem(&text)
}

fn rsa_from_pem(text: &str) -> Result<(String, String), String> {
    if let Some(der) = pem_body(text, "RSA PRIVATE KEY") {
        return n_e_from_pkcs1(&decode_base64(&der)?);
    }
    if let Some(der) = pem_body(text, "PRIVATE KEY") {
        let inner = pkcs8_unwrap_rsa(&decode_base64(&der)?)?;
        return n_e_from_pkcs1(&inner);
    }
    Err("PEM: expected RSA PRIVATE KEY or PRIVATE KEY".into())
}

fn pem_body(text: &str, kind: &str) -> Option<String> {
    let begin = format!("-----BEGIN {kind}-----");
    let end = format!("-----END {kind}-----");
    let start = text.find(&begin)? + begin.len();
    let rest = text.get(start..)?;
    let stop = rest.find(&end)?;
    Some(rest[..stop].into())
}

fn n_e_from_pkcs1(der: &[u8]) -> Result<(String, String), String> {
    let ints = der_sequence_integers(der)?;
    if ints.len() < 3 {
        return Err(format!("PKCS#1 expected n,e; got {} integers", ints.len()));
    }
    Ok((ints[1].to_str_radix(10), ints[2].to_str_radix(10)))
}

fn pkcs8_unwrap_rsa(der: &[u8]) -> Result<Vec<u8>, String> {
    if der.first() != Some(&0x30) {
        return Err("PKCS#8: expected SEQUENCE".into());
    }
    let (hdr, len) = der_len(&der[1..])?;
    let mut i = 1 + hdr;
    let end = i + len;
    // version INTEGER
    i = der_skip(der, i, 0x02)?;
    // algorithm SEQUENCE
    i = der_skip(der, i, 0x30)?;
    // privateKey OCTET STRING
    if der.get(i) != Some(&0x04) {
        return Err("PKCS#8: expected OCTET STRING".into());
    }
    i += 1;
    let (oh, olen) = der_len(&der[i..])?;
    i += oh;
    if i + olen > end {
        return Err("PKCS#8: truncated".into());
    }
    Ok(der[i..i + olen].to_vec())
}

fn der_skip(der: &[u8], i: usize, tag: u8) -> Result<usize, String> {
    if der.get(i) != Some(&tag) {
        return Err("DER: unexpected tag".into());
    }
    let (hdr, len) = der_len(&der[i + 1..])?;
    Ok(i + 1 + hdr + len)
}

fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let clean: String = input.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if clean.len() % 4 != 0 {
        return Err("base64 length".into());
    }
    let table = |c: u8| -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => Some(0),
            _ => None,
        }
    };
    let bytes = clean.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i < bytes.len() {
        let (a, b, c, d) = (
            table(bytes[i]).ok_or("base64")?,
            table(bytes[i + 1]).ok_or("base64")?,
            table(bytes[i + 2]).ok_or("base64")?,
            table(bytes[i + 3]).ok_or("base64")?,
        );
        out.push((a << 2) | (b >> 4));
        if bytes[i + 2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if bytes[i + 3] != b'=' {
            out.push((c << 6) | d);
        }
        i += 4;
    }
    Ok(out)
}

fn der_sequence_integers(der: &[u8]) -> Result<Vec<BigUint>, String> {
    if der.first() != Some(&0x30) {
        return Err("DER: expected SEQUENCE".into());
    }
    let (hdr, len) = der_len(&der[1..])?;
    let seq = &der[1 + hdr..1 + hdr + len];
    let mut i = 0;
    let mut out = Vec::new();
    while i < seq.len() {
        if seq[i] != 0x02 {
            // skip remaining fields we do not need
            break;
        }
        i += 1;
        let (hdr, len) = der_len(&seq[i..])?;
        i += hdr;
        if i + len > seq.len() {
            return Err("DER: truncated INTEGER".into());
        }
        out.push(BigUint::from_bytes_be(&seq[i..i + len]));
        i += len;
    }
    Ok(out)
}

/// Returns (header bytes consumed, value length).
fn der_len(rest: &[u8]) -> Result<(usize, usize), String> {
    let b0 = *rest.first().ok_or("DER: empty length")?;
    if b0 < 0x80 {
        return Ok((1, b0 as usize));
    }
    let n = (b0 & 0x7f) as usize;
    if n == 0 || n > 4 || rest.len() < 1 + n {
        return Err("DER: bad length".into());
    }
    let mut len = 0usize;
    for b in &rest[1..1 + n] {
        len = (len << 8) | *b as usize;
    }
    Ok((1 + n, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prod_modulus_is_decimal_and_long() {
        assert!(PROD_LOGIN_RSAN.chars().all(|c| c.is_ascii_digit()));
        assert!(PROD_LOGIN_RSAN.len() >= 250);
        assert_eq!(PROD_LOGIN_RSAE, "65537");
    }

    #[test]
    fn java_defaults_are_the_lost_city_pair() {
        assert_eq!(JAVA_LOGIN_RSAN.len(), 154);
        assert!(JAVA_LOGIN_RSAE.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn openssl_pkcs1_pem_roundtrip_n_e() {
        let dir = std::env::temp_dir().join(format!("274bot-rsa-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let pem = dir.join("key.pem");
        let status = std::process::Command::new("openssl")
            .args(["genrsa", "-out"])
            .arg(&pem)
            .arg("512")
            .status();
        let Ok(st) = status else {
            return;
        };
        if !st.success() || !pem.is_file() {
            let _ = fs::remove_dir_all(&dir);
            return;
        }
        let (n, e) = rsa_from_pkcs1_pem_file(&pem).expect("parse pem");
        assert!(n.chars().all(|c| c.is_ascii_digit()), "{n}");
        assert!(n.len() > 20);
        assert!(e == "65537" || e == "3" || e.chars().all(|c| c.is_ascii_digit()));
        let _ = fs::remove_dir_all(&dir);
    }
}
