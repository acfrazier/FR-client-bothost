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

/// 80 `a`s (TABLE index 3) pack to exactly 40 bytes with no trailing
/// carry byte (even nibble count), so the 1:1 unpack restores all 80
/// chars: the sentence-case pass uppercases only the first one.
#[test]
fn wordpack_exact_80_roundtrip() {
    let mut p = Packet::new(vec![0; 256]);
    WordPack::pack(&mut p, &"a".repeat(80));
    let len = p.pos;
    p.pos = 0;
    let out = WordPack::unpack(&mut p, len);
    assert_eq!(len, 40, "80 nibble-pairs fill 40 bytes exactly");
    assert_eq!(out.len(), 80);
    assert_eq!(out, format!("A{}", "a".repeat(79)));
}
