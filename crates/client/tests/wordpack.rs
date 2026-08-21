use client::io::Packet;
use client::wordfilter::WordPack;

#[test]
fn wordpack_roundtrip_hello() {
    let mut p = Packet::new(vec![0; 256]);
    WordPack::pack(&mut p, "hello");
    let len = p.pos;
    p.pos = 0;
    // Oracle (WordPack.ts) byte output for "hello" is 61 bb 40; the trailing
    // carry byte's low nibble is 0, which decodes as a space, so unpack gives
    // "Hello " — verified against the TS codec, not a port artifact.
    assert_eq!(WordPack::unpack(&mut p, len), "Hello ");
}

#[test]
fn wordpack_bytes_match_ts_oracle() {
    let mut p = Packet::new(vec![0; 256]);
    WordPack::pack(&mut p, "hello");
    // Same bytes as running WordPack.ts pack("hello") in node.
    assert_eq!(&p.data()[..p.pos], &[0x61, 0xbb, 0x40]);
}

#[test]
fn wordpack_truncates_to_80() {
    let mut p = Packet::new(vec![0; 256]);
    WordPack::pack(&mut p, &"a".repeat(90));
    let len = p.pos;
    p.pos = 0;
    let out = WordPack::unpack(&mut p, len);
    assert!(out.len() <= 80);
}
