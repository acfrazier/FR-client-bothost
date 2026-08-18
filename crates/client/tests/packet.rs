use client::io::Packet;

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
