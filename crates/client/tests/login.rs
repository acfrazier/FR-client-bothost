use client::client::{Client, ClientConfig};
use client::util::JString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

#[test]
fn to_userhash_matches_client_ts() {
    // values generated with webclient JString.ts toUserhash
    assert_eq!(JString::to_userhash("bob"), 3295);
    assert_eq!(JString::to_userhash("admin"), 2094917);
    assert_eq!(JString::to_userhash("Zz0_9"), 50082163);
    assert_eq!(JString::to_userhash("runescape"), 65254502242866);
    assert_eq!(JString::to_userhash("RuneScape"), 65254502242866);
    assert_eq!(JString::to_userhash("  bob  "), 3295);
    assert_eq!(
        JString::to_userhash("aaaaaaaaaaaaaaaaaaaa"),
        182859777940000980
    );
    assert_eq!(((JString::to_userhash("bob") >> 16) & 0x1f), 0); // loginServer byte
}

#[test]
fn cold_login_opcode_16_success() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        assert_eq!(hdr[0], 14); // login server probe
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap(); // response 0 → send seed
        s.write_all(&[0, 0, 0, 0, 0, 0, 0, 1]).unwrap(); // g8 seed
                                                         // read client loginout (variable); then grant
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf[0], 16); // cold login
        let size = buf[1] as usize;
        assert_eq!(size, n - 2);
        assert_eq!(buf[2], 255); // rev marker
        assert_eq!((buf[3] as usize) << 8 | buf[4] as usize, 274); // client version
        assert_eq!(buf[5], 0); // info: lowmem off
        assert_eq!(n, 2 + size);
        if client::LOGIN_RSAN.starts_with("7162900525229798032761816791230527296329313291") {
            // baked Java 274 key is 512-bit: rsa len byte + 64-byte ciphertext
            assert_eq!(buf[42], 64);
            assert_eq!(n, 2 + 40 + 1 + 64);
        }
        s.write_all(&[2, 0, 0]).unwrap(); // response 2, staff=0, mouseTrack=0
    });

    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.login("bob", "pw", false).unwrap();
    assert!(c.ingame);
    assert!(c.local_player.is_some());
    assert!(c.players[2047].is_some());
    assert_eq!(c.login_user, "bob");
    assert_eq!(c.login_pass, "pw");
    server.join().unwrap();
}

#[test]
fn reconnect_uses_opcode_18() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        s.read_exact(&mut hdr).unwrap();
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        s.write_all(&[0]).unwrap();
        s.write_all(&[0u8; 8]).unwrap();
        let mut buf = [0u8; 512];
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0);
        assert_eq!(buf[0], 18);
        s.write_all(&[2, 0, 0]).unwrap();
    });
    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    c.login("bob", "pw", true).unwrap();
    server.join().unwrap();
}

#[test]
fn login_code_6_is_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut hdr = [0u8; 2];
        let _ = s.read_exact(&mut hdr);
        for _ in 0..8 {
            let _ = s.write_all(&[0]);
        }
        let _ = s.write_all(&[6]);
    });
    let mut c = Client::new(ClientConfig {
        host: addr.ip().to_string(),
        port: addr.port(),
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    });
    let e = c.login("bob", "pw", false).unwrap_err();
    assert_eq!(e.code, 6);
}
