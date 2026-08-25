//! Task 5: every `Client` gets its own login uid for the 274 handshake RSA
//! block (Java 274 `loginUid`), so FIFO/slot identity is unambiguous — the
//! host no longer broadcasts a shared `1337` constant to the server. `new`
//! fills it with a random non-zero i32; the host may overwrite it with a
//! profile uid before `login`. The RSA block is encrypted before the wire,
//! so the wrapper check pins the block builder (`write_login_block`) that
//! `login` feeds to `rsaenc`, not the ciphertext.
use client::client::{Client, ClientConfig};

fn cfg() -> ClientConfig {
    ClientConfig {
        host: "127.0.0.1".into(),
        port: 43594,
        cache_dir: "/tmp".into(),
        members: true,
        lowmem: false,
    }
}

/// Every Client carries its own uid: non-zero, never the old shared `1337`,
/// and distinct across two fresh Clients.
#[test]
fn two_clients_get_distinct_login_uids() {
    let a = Client::new(cfg());
    let b = Client::new(cfg());
    assert_ne!(a.login_uid, 0);
    assert_ne!(a.login_uid, 1337);
    assert_ne!(a.login_uid, b.login_uid);
}

/// The uid is written into the 274 login wrapper at its RSA-block slot: the
/// block is p1(10) + 4×p4(seed) + p4(login_uid) + pjstr(user) + pjstr(pass),
/// so the uid occupies bytes 17..=20 of the unencrypted wrapper.
#[test]
fn login_uid_is_present_in_login_wrapper_bytes() {
    let mut c = Client::new(cfg());
    c.login_uid = 0x5151_5253; // host may overwrite before login
    let seed = [0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334];
    c.write_login_block(seed, "bob", "pw");
    assert_eq!(c.out.pos, 28); // 1 + 16 + 4 + (3 + 1) + (2 + 1)
    let data = c.out.data();
    assert_eq!(data[0], 10); // login wrapper opcode
    for (i, s) in seed.iter().enumerate() {
        assert_eq!(
            &data[1 + 4 * i..5 + 4 * i],
            &s.to_be_bytes(),
            "seed[{i}] must land in the wrapper"
        );
    }
    assert_eq!(&data[17..21], &c.login_uid.to_be_bytes());
}
