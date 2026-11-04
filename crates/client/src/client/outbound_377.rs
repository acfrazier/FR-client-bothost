//! Revision-377 outbound packets needed for the first login/scene/logout path.
//!
//! Packet ids and field layouts are taken from Client-Java 377. Keep these
//! helpers independent of the 274/289 `map_client_prot` translation: each
//! in-game emitter uses the generated revision-377 packet descriptor, while
//! the login wrapper is a raw, unencrypted handshake frame.

use crate::io::{ClientProt377, Packet};

/// Build the RSA plaintext login block with the caller's revision-377 UID.
///
/// The block layout is the same as the existing `Client::write_login_block`;
/// the explicit UID lets the caller supply the Java `signlink.uid` value.
/// Apply `Packet::rsaenc` after writing this block.
///
/// Java: `Client.java:2701-2710`.
pub(crate) fn write_login_block(
    block: &mut Packet,
    seed: [i32; 4],
    uid: i32,
    username: &str,
    password: &str,
) {
    block.pos = 0;
    block.p1(10);
    for seed_word in seed {
        block.p4(seed_word);
    }
    block.p4(uid);
    block.pjstr(username);
    block.pjstr(password);
}

/// Build the 377 login wrapper around the already RSA-encoded login block.
///
/// `rsa_block` is the source of the appended ciphertext and must have had
/// `rsaenc` applied already. The wrapper packet is an ordinary, unobserved
/// `Packet`; it is not an ISAAC-framed game packet.
///
/// Java: `Client.java:2711-2724`.
pub(crate) fn write_login_wrapper(
    wrapper: &mut Packet,
    reconnect: bool,
    low_mem: bool,
    checksums: &[i32; 9],
    rsa_block: &Packet,
) {
    wrapper.pos = 0;
    wrapper.p1(if reconnect { 18 } else { 16 });
    wrapper.p1((rsa_block.pos + 40) as i32);
    wrapper.p1(255);
    wrapper.p2(377);
    wrapper.p1(i32::from(low_mem));
    for &checksum in checksums {
        wrapper.p4(checksum);
    }
    wrapper.pdata(rsa_block.data(), 0, rsa_block.pos);
}

/// Emit the revision-377 `NO_TIMEOUT` packet (opcode 40, empty body).
/// Java: `Client.java:3407-3410`; during `buildScene`, `3527-3547`,
/// `3569-3570`, and `3623-3631`.
pub(crate) fn no_timeout(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::NO_TIMEOUT.id);
}

/// Emit `MAP_BUILD_COMPLETE` after a successful scene build (opcode 6).
/// Java: `Client.java:3496-3501`.
pub(crate) fn map_build_complete(packet: &mut Packet) {
    packet.p1_enc(ClientProt377::MAP_BUILD_COMPLETE.id);
}

/// Emit the `IF_BUTTON` frame for the selected interface component.
/// The caller must preserve Java's `clientButton` gate: this is used for the
/// logout button only when the client-code handler arms `logoutTimer` and
/// allows the click to be sent.
///
/// Java emits the interface click at `Client.java:9890-9893`.
pub(crate) fn if_button(packet: &mut Packet, component_id: i32) {
    packet.p1_enc(ClientProt377::IF_BUTTON.id);
    packet.p2(component_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_block_matches_java_377_literal() {
        // J:2701-2710 writes opcode 10, four seed words, signlink.uid,
        // username and password as newline-terminated strings.
        let mut block = Packet::alloc(0);
        write_login_block(
            &mut block,
            [0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334],
            0x4142_4344,
            "u",
            "p",
        );

        assert_eq!(
            &block.data()[..block.pos],
            &[
                0x0a, 0x01, 0x02, 0x03, 0x04, 0x11, 0x12, 0x13, 0x14, 0x21, 0x22, 0x23, 0x24, 0x31,
                0x32, 0x33, 0x34, 0x41, 0x42, 0x43, 0x44, b'u', 0x0a, b'p', 0x0a,
            ]
        );
    }

    #[test]
    fn login_wrapper_matches_java_377_literal() {
        // J:2711-2724: 16, length=(RSA block+40), 255, revision 377,
        // low-memory flag, nine big-endian JAG CRCs, then the RSA bytes.
        let mut rsa_block = Packet::alloc(0);
        rsa_block.p1(0xaa);
        rsa_block.p1(0xbb);
        let checksums = [
            0x0102_0304,
            0x1112_1314,
            0x2122_2324,
            0x3132_3334,
            0x4142_4344,
            0x5152_5354,
            0x6162_6364,
            0x7172_7374,
            0x8182_8384u32 as i32,
        ];
        let mut wrapper = Packet::alloc(1);

        write_login_wrapper(&mut wrapper, false, true, &checksums, &rsa_block);

        assert_eq!(
            &wrapper.data()[..wrapper.pos],
            &[
                0x10, 0x2a, 0xff, 0x01, 0x79, 0x01, 0x01, 0x02, 0x03, 0x04, 0x11, 0x12, 0x13, 0x14,
                0x21, 0x22, 0x23, 0x24, 0x31, 0x32, 0x33, 0x34, 0x41, 0x42, 0x43, 0x44, 0x51, 0x52,
                0x53, 0x54, 0x61, 0x62, 0x63, 0x64, 0x71, 0x72, 0x73, 0x74, 0x81, 0x82, 0x83, 0x84,
                0xaa, 0xbb,
            ]
        );
    }

    #[test]
    fn reconnect_wrapper_uses_java_opcode_18() {
        // J:2712-2716 selects 18 for reconnect, with the same 377 wrapper.
        let mut rsa_block = Packet::alloc(0);
        rsa_block.p1(0xaa);
        let mut wrapper = Packet::alloc(1);

        write_login_wrapper(&mut wrapper, true, false, &[0; 9], &rsa_block);

        assert_eq!(&wrapper.data()[..2], &[0x12, 0x29]);
        assert_eq!(&wrapper.data()[2..6], &[0xff, 0x01, 0x79, 0x00]);
        assert_eq!(wrapper.data()[wrapper.pos - 1], 0xaa);
    }

    #[test]
    fn minimum_377_game_packets_match_java_literals() {
        // J:3407-3410 (keepalive), 3496-3501 (scene completion), and
        // 9890-9893 (IF_BUTTON; the logout clientButton path allows it).
        let mut packet = Packet::alloc_outbound(1);
        no_timeout(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[40]);

        packet.pos = 0;
        map_build_complete(&mut packet);
        assert_eq!(&packet.data()[..packet.pos], &[6]);

        packet.pos = 0;
        if_button(&mut packet, 2458);
        assert_eq!(&packet.data()[..packet.pos], &[79, 0x09, 0x9a]);

        assert_eq!(ClientProt377::NO_TIMEOUT.length, 0);
        assert_eq!(ClientProt377::MAP_BUILD_COMPLETE.length, 0);
        assert_eq!(ClientProt377::IF_BUTTON.length, 2);
    }
}
