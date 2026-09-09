//! Generated ownership probes; no cache, account, connection or renderer.
use client::io::Packet;

#[test]
fn packet_probe_preserves_bytes_and_cursor() {
    let mut bytes = Vec::with_capacity(257);
    bytes.extend_from_slice(&[1, 2, 3]);
    let expected_capacity = bytes.capacity();
    let packet = Packet::new(bytes);
    #[cfg(feature = "memory-owner-capture")]
    {
        assert_eq!(packet.owner_data_capacity(), expected_capacity);
        assert!(packet.owner_data_capacity() > packet.data().len());
    }
    #[cfg(not(feature = "memory-owner-capture"))]
    let _ = expected_capacity;
    assert_eq!(packet.data(), &[1, 2, 3]);
    assert_eq!(packet.pos, 0);
}
