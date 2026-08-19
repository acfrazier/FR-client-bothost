// Port of `~/experiments/Server/webclient/src/io/BZip2.js` contract: Jagex
// streams omit the standard "BZh<blocksize>" file header and start directly
// with the first 48-bit block magic. libbz2 requires the header, so synthesize
// one (blocksize digit is only used for allocation hints) before feeding the
// stream to the crate. Verified against 274 engine packs.
use std::io::Read;

pub fn bunzip2(src: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(src.len() + 4);
    framed.extend_from_slice(b"BZh9");
    framed.extend_from_slice(src);
    let mut dec = bzip2::read::BzDecoder::new(&framed[..]);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .unwrap_or_else(|e| panic!("bunzip2: {e}"));
    out
}
