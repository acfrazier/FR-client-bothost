// Jagex streams omit the standard "BZh<blocksize>" file header and start
// directly with the first 48-bit block magic. Engine Jagfile compression uses
// 100k blocks, so restore BZh1 rather than allocating libbz2's 900k workspace.
// Headerless streams produced with larger blocks are still accepted below.
use std::io::{self, Read};

/// Fallible Jagex bunzip2. The production [`bunzip2`] wrapper keeps its panic
/// contract; identity and other read-only helpers must not panic on junk.
pub fn try_bunzip2(src: &[u8]) -> io::Result<Vec<u8>> {
    match decode(src, b"BZh1") {
        Err(error) if is_large_block_data_error(&error) => {
            // The removed header cannot tell us that a block exceeds 100k,
            // and libbz2 reports that overflow as the same Data error as CRC
            // or table corruption. Retry once with the 900k hint: large-block
            // streams decode, corrupt streams fail again (at most 2x work).
            decode(src, b"BZh9")
        }
        result => result,
    }
}

fn is_large_block_data_error(error: &io::Error) -> bool {
    matches!(
        error
            .get_ref()
            .and_then(|cause| cause.downcast_ref::<bzip2::Error>()),
        Some(&bzip2::Error::Data)
    )
}

/// Decompresses into Java JagFile's fixed-size output buffer.
pub(crate) fn bunzip2_sized(src: &[u8], unpacked_size: usize) -> Vec<u8> {
    // Java allocates a zero-filled byte array at the declared size and stops
    // emitting when it is full. Keep that bound even if the stream expands
    // further; an early end leaves the untouched suffix zero-filled.
    let mut out = vec![0; unpacked_size];
    let result = match decode_sized(src, b"BZh1", &mut out) {
        Err(error) if is_large_block_data_error(&error) => {
            // Reuse the fixed output allocation when retrying the larger hint.
            out.fill(0);
            decode_sized(src, b"BZh9", &mut out)
        }
        result => result,
    };
    result.unwrap_or_else(|e| panic!("bunzip2: {e}"));
    out
}

fn decode_sized(src: &[u8], header: &[u8; 4], out: &mut [u8]) -> io::Result<()> {
    let mut dec = bzip2::bufread::BzDecoder::new(header.as_slice().chain(src));
    let mut written = 0;
    while written < out.len() {
        match dec.read(&mut out[written..]) {
            Ok(0) => break,
            Ok(count) => written += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn decode(src: &[u8], header: &[u8; 4]) -> io::Result<Vec<u8>> {
    // Chain borrowed slices instead of copying the entire compressed input.
    let mut dec = bzip2::bufread::BzDecoder::new(header.as_slice().chain(src));
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}

pub fn bunzip2(src: &[u8]) -> Vec<u8> {
    try_bunzip2(src).unwrap_or_else(|e| panic!("bunzip2: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{bunzip2_sized, decode, try_bunzip2};
    use bzip2::{write::BzEncoder, Compression};
    use std::io::Write;

    fn payload() -> Vec<u8> {
        (0..300_000)
            .map(|i| ((i * 37 + i / 257) & 255) as u8)
            .collect()
    }

    fn headerless(data: &[u8], level: Compression) -> Vec<u8> {
        let mut encoder = BzEncoder::new(Vec::new(), level);
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()[4..].to_vec()
    }

    #[test]
    fn hundred_k_blocks_decode_a_multi_block_stream() {
        let expected = payload();
        let compressed = headerless(&expected, Compression::fast());
        assert_eq!(decode(&compressed, b"BZh1").unwrap(), expected);
        assert_eq!(try_bunzip2(&compressed).unwrap(), expected);
    }

    #[test]
    fn larger_headerless_blocks_keep_their_existing_support() {
        let expected = payload();
        let compressed = headerless(&expected, Compression::best());
        assert!(decode(&compressed, b"BZh1").is_err());
        assert_eq!(bunzip2_sized(&compressed, expected.len()), expected);
        assert_eq!(try_bunzip2(&compressed).unwrap(), expected);
    }

    #[test]
    fn truncated_streams_return_errors() {
        let compressed = headerless(&payload(), Compression::fast());
        for end in [0, 1, 5, compressed.len() / 2, compressed.len() - 1] {
            assert!(
                try_bunzip2(&compressed[..end]).is_err(),
                "accepted truncated stream ending at {end}"
            );
        }
    }

    #[test]
    fn block_crc_corruption_is_not_hidden_by_the_larger_hint() {
        let mut compressed = headerless(&payload(), Compression::fast());
        compressed[6] ^= 1;
        assert!(try_bunzip2(&compressed).is_err());
    }

    #[test]
    #[should_panic(expected = "bunzip2:")]
    fn malformed_truncated_stream_keeps_decode_panic_contract() {
        // Java BZip2.java:463-477 reads input without an end check and throws
        // for a truncated bitstream; Rust keeps its existing decode panic.
        let data = payload();
        let compressed = headerless(&data, Compression::fast());
        bunzip2_sized(&compressed[..compressed.len() - 1], data.len() + 1);
    }
}
