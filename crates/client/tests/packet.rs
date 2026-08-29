use client::io::{Isaac, Packet};
use num_bigint::BigUint;
use std::str::FromStr;

#[test]
fn p1_enc_xors_isaac() {
    let mut p = Packet::alloc(0);
    p.random = Some(Isaac::new(&[1, 2, 3, 4]));
    p.p1_enc(120);
    assert_eq!(p.data()[0], 182); // (120 + -621246914) & 0xff
}

#[test]
fn rsaenc_java_key_hi_1337() {
    let mut p = Packet::alloc(1);
    p.pjstr("hi");
    p.p4(1337);
    let n = BigUint::from_str(client::JAVA_LOGIN_RSAN).unwrap();
    let e = BigUint::from_str(client::JAVA_LOGIN_RSAE).unwrap();
    p.rsaenc(&n, &e);
    assert!(p.pos >= 2);
    assert_eq!(p.data()[0] as usize, p.pos - 1);
    assert_eq!(
        &p.data()[..p.pos],
        &[
            64, 86, 83, 87, 172, 26, 221, 24, 175, 132, 67, 99, 115, 197, 146, 98, 155, 170, 193,
            109, 58, 8, 193, 175, 2, 236, 115, 37, 114, 49, 206, 94, 10, 55, 29, 205, 170, 223, 128,
            245, 135, 72, 178, 150, 234, 153, 197, 241, 204, 145, 159, 190, 42, 207, 8, 85, 247,
            113, 125, 158, 157, 214, 15, 100, 176
        ]
    );
}

#[test]
fn p1_p2_p4_big_endian_layout() {
    let mut p = Packet::alloc(0);
    p.p1(1);
    p.p2(0x0203);
    p.p4(0x04050607);
    assert_eq!(&p.data()[..p.pos], &[1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn g_roundtrip_signed_and_smart() {
    let mut p = Packet::alloc(0);
    p.p1(0xff);
    p.p2(0xfffe);
    p.pos = 0;
    assert_eq!(p.g1(), 255);
    p.pos = 0;
    assert_eq!(p.g1b(), -1);
    // p2 wrote data[1..3] = [0xff, 0xfe]; rewind to 1, not 0
    p.pos = 1;
    assert_eq!(p.g2(), 0xfffe);
    p.pos = 1;
    assert_eq!(p.g2b(), -2);
}

#[test]
fn crc32_matches_client_ts() {
    let src: Vec<u8> = (0..10).collect();
    assert_eq!(Packet::getcrc(&src, 0, 10), 1164760902);
}

#[test]
fn pjstr_newline_terminated() {
    let mut p = Packet::alloc(0);
    p.pjstr("hi");
    assert_eq!(&p.data()[..p.pos], b"hi\n");
    p.pos = 0;
    assert_eq!(p.gjstr(), "hi");
}
