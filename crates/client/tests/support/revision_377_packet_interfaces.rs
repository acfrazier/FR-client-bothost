//! Root-boundary tests for revision-377 interface/modal/tutorial packets.
//! Values and ordering are anchored to Client.java:7268-8570. The engine codec
//! comparisons are documented beside the decoder; these cases pin Java's
//! signed TUT_OPEN/IF_OPENOVERLAY reads and opcode-18 packed rotation word.

use crate::packet_377::interface_packet_377::{
    InterfaceOperation377, ALL_OPCODES_377 as INTERFACE_OPCODES_377,
};
use crate::packet_377::{
    decode_packet_377, Operation377, PacketContext377, PacketErrorKind377, ReferenceKind377,
};

/// Explicitly test-only resource context: it has no client/cache handle and
/// answers resource-presence queries from a single denial rule.
struct TestPacketContext377 {
    deny: DeniedResource,
    id: i32,
}

#[derive(Clone, Copy)]
enum DeniedResource {
    None,
    Component,
    Object,
    Npc,
    Sequence,
    Model,
}

impl TestPacketContext377 {
    const fn all_present() -> Self {
        Self {
            deny: DeniedResource::None,
            id: -1,
        }
    }

    const fn denying(deny: DeniedResource, id: i32) -> Self {
        Self { deny, id }
    }
}

impl PacketContext377 for TestPacketContext377 {
    fn contains(&self, kind: ReferenceKind377, id: i32) -> bool {
        let denied = match (self.deny, kind) {
            (DeniedResource::Component, ReferenceKind377::Component)
            | (DeniedResource::Object, ReferenceKind377::Object)
            | (DeniedResource::Npc, ReferenceKind377::Npc)
            | (DeniedResource::Sequence, ReferenceKind377::Sequence)
            | (DeniedResource::Model, ReferenceKind377::Model) => id == self.id,
            _ => false,
        };
        !denied
    }

    fn inventory_capacity(&self, _component: i32) -> Option<usize> {
        None
    }
}

fn assert_packet<'a>(opcode: u8, payload: &'a [u8], expected: InterfaceOperation377<'a>) {
    let decoded = decode_packet_377(opcode, payload).expect("decode complete Java frame");

    // A validated wrapper, not the initial decoded packet, is the handoff.
    let validated = decoded
        .validate(&TestPacketContext377::all_present())
        .expect("test context contains referenced resources");
    assert_eq!(validated.operation(), &Operation377::Interface(expected));
}

fn put_g1(bytes: &mut Vec<u8>, value: i32) {
    bytes.push(value as u8);
}

fn put_g1_alt2(bytes: &mut Vec<u8>, value: i32) {
    bytes.push((0u8).wrapping_sub(value as u8));
}

fn put_g1_alt3(bytes: &mut Vec<u8>, value: i32) {
    bytes.push((128i32 - value) as u8);
}

fn put_g2(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&(value as u16).to_be_bytes());
}

fn put_g2b(bytes: &mut Vec<u8>, value: i32) {
    put_g2(bytes, value);
}

fn put_g2_alt1(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&(value as u16).to_le_bytes());
}

fn put_g2_alt2(bytes: &mut Vec<u8>, value: i32) {
    let value = value as u16;
    bytes.push((value >> 8) as u8);
    bytes.push((value as u8).wrapping_add(128));
}

fn put_g2_alt3(bytes: &mut Vec<u8>, value: i32) {
    let value = value as u16;
    bytes.push((value as u8).wrapping_add(128));
    bytes.push((value >> 8) as u8);
}

fn put_g2b_alt1(bytes: &mut Vec<u8>, value: i32) {
    put_g2_alt1(bytes, value);
}

fn put_g2b_alt2(bytes: &mut Vec<u8>, value: i32) {
    put_g2_alt2(bytes, value);
}

fn sample_payload(opcode: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    match opcode {
        166 => {
            put_g2b_alt1(&mut bytes, -1234); // y
            put_g2b_alt1(&mut bytes, 2345); // x
            put_g2(&mut bytes, 0x1234); // component
        }
        186 => {
            put_g2_alt2(&mut bytes, 0x1234); // xan
            put_g2_alt3(&mut bytes, 0x3456); // component
            put_g2_alt2(&mut bytes, 0x5678); // zoom
            put_g2_alt1(&mut bytes, 0x789A); // yan
        }
        216 => {
            put_g2_alt3(&mut bytes, 0x2345); // model
            put_g2_alt3(&mut bytes, 0x3456); // component
        }
        162 => {
            put_g2_alt2(&mut bytes, 0x2345); // NPC
            put_g2_alt1(&mut bytes, 0x3456); // component
        }
        109 => put_g2(&mut bytes, 0x1234),
        158 => put_g2b_alt1(&mut bytes, -1),
        218 => {
            put_g2(&mut bytes, 0x1234); // component
            put_g2_alt2(&mut bytes, 0x4567); // RGB555
        }
        6 | 29 | 58 => {}
        50 => put_g2b(&mut bytes, -1),
        82 => {
            put_g1(&mut bytes, 1);
            put_g2(&mut bytes, 0x1234);
        }
        128 => {
            put_g2_alt2(&mut bytes, 0x2345); // main
            put_g2_alt3(&mut bytes, 0x3456); // side
        }
        252 => put_g1_alt2(&mut bytes, 14),
        255 | 159 | 246 => put_g2_alt3(&mut bytes, 0x2345),
        21 => {
            put_g2(&mut bytes, 100); // divisor
            put_g2_alt1(&mut bytes, 0x2345); // object
            put_g2_alt3(&mut bytes, 0x3456); // component
        }
        2 => {
            put_g2_alt3(&mut bytes, 0x2345); // component
            put_g2b_alt2(&mut bytes, 0x1234); // sequence
        }
        10 => {
            put_g1_alt3(&mut bytes, 14); // final Java tab-array index
            put_g2_alt2(&mut bytes, 0xFFFF); // clear tab component
        }
        238 => put_g1(&mut bytes, 255), // Java preserves out-of-range/no-op values
        253 => {
            put_g2_alt1(&mut bytes, 0x9234); // unsigned overlay
            put_g2_alt2(&mut bytes, 0x4567); // main
        }
        18 => {
            put_g2(&mut bytes, 0xFEDC); // packed high word
            put_g2_alt2(&mut bytes, 0x1234); // component
            put_g2_alt1(&mut bytes, 0xBA98); // packed low word
        }
        232 => {
            put_g2_alt3(&mut bytes, 0x2345);
            bytes.extend_from_slice(b"text\x80\xff");
            put_g1(&mut bytes, 10); // Java LF terminator
        }
        200 => {
            put_g2(&mut bytes, 0x1234);
            put_g2_alt3(&mut bytes, 0x5678);
        }
        _ => panic!("no sample payload for opcode {opcode}"),
    }
    bytes
}

#[test]
fn every_interface_packet_decodes_to_its_java_meaning() {
    assert_eq!(INTERFACE_OPCODES_377.len(), 25);

    let payload = sample_payload(166);
    assert_packet(
        166,
        &payload,
        InterfaceOperation377::SetPosition {
            component: 0x1234,
            x: 2345,
            y: -1234,
        },
    );

    let payload = sample_payload(186);
    assert_packet(
        186,
        &payload,
        InterfaceOperation377::SetAngle {
            component: 0x3456,
            xan: 0x1234,
            yan: 0x789A,
            zoom: 0x5678,
        },
    );

    let payload = sample_payload(216);
    assert_packet(
        216,
        &payload,
        InterfaceOperation377::SetModel {
            component: 0x3456,
            model: 0x2345,
        },
    );

    let payload = sample_payload(162);
    assert_packet(
        162,
        &payload,
        InterfaceOperation377::SetNpcHead {
            component: 0x3456,
            npc: 0x2345,
        },
    );

    let payload = sample_payload(109);
    assert_packet(
        109,
        &payload,
        InterfaceOperation377::OpenChat { component: 0x1234 },
    );

    let payload = sample_payload(158);
    assert_packet(
        158,
        &payload,
        InterfaceOperation377::TutOpen { component: -1 },
    );

    let payload = sample_payload(218);
    assert_packet(
        218,
        &payload,
        InterfaceOperation377::SetColour {
            component: 0x1234,
            rgb555: 0x4567,
        },
    );

    assert_packet(6, &sample_payload(6), InterfaceOperation377::NameDialog);
    assert_packet(29, &sample_payload(29), InterfaceOperation377::Close);

    let payload = sample_payload(50);
    assert_packet(
        50,
        &payload,
        InterfaceOperation377::OpenOverlay { component: -1 },
    );

    let payload = sample_payload(82);
    assert_packet(
        82,
        &payload,
        InterfaceOperation377::SetHide {
            component: 0x1234,
            hidden: true,
        },
    );

    let payload = sample_payload(128);
    assert_packet(
        128,
        &payload,
        InterfaceOperation377::OpenMainSide {
            main: 0x2345,
            side: 0x3456,
        },
    );

    assert_packet(58, &sample_payload(58), InterfaceOperation377::CountDialog);

    let payload = sample_payload(252);
    assert_packet(
        252,
        &payload,
        InterfaceOperation377::SetTabActive { tab: 14 },
    );

    let payload = sample_payload(255);
    assert_packet(
        255,
        &payload,
        InterfaceOperation377::SetPlayerHead { component: 0x2345 },
    );

    let payload = sample_payload(159);
    assert_packet(
        159,
        &payload,
        InterfaceOperation377::OpenMain { component: 0x2345 },
    );

    let payload = sample_payload(246);
    assert_packet(
        246,
        &payload,
        InterfaceOperation377::OpenSide { component: 0x2345 },
    );

    let payload = sample_payload(21);
    assert_packet(
        21,
        &payload,
        InterfaceOperation377::SetObject {
            component: 0x3456,
            object: 0x2345,
            divisor: 100,
        },
    );

    let payload = sample_payload(2);
    assert_packet(
        2,
        &payload,
        InterfaceOperation377::SetAnim {
            component: 0x2345,
            sequence: 0x1234,
        },
    );

    let payload = sample_payload(10);
    assert_packet(
        10,
        &payload,
        InterfaceOperation377::SetTab {
            tab: 14,
            component: -1,
        },
    );

    let payload = sample_payload(238);
    assert_packet(238, &payload, InterfaceOperation377::TutFlash { tab: 255 });

    let payload = sample_payload(253);
    assert_packet(
        253,
        &payload,
        InterfaceOperation377::OpenFull {
            overlay: 0x9234,
            main: 0x4567,
        },
    );

    let payload = sample_payload(18);
    assert_packet(
        18,
        &payload,
        InterfaceOperation377::SetRotation {
            component: 0x1234,
            packed: 0xFEDC_BA98u32 as i32,
        },
    );

    let payload = sample_payload(232);
    assert_packet(
        232,
        &payload,
        InterfaceOperation377::SetText {
            component: 0x2345,
            text: b"text\x80\xff",
        },
    );

    let payload = sample_payload(200);
    assert_packet(
        200,
        &payload,
        InterfaceOperation377::SetScrollPos {
            component: 0x1234,
            position: 0x5678,
        },
    );
}

#[test]
fn root_rejects_every_truncated_and_trailing_interface_frame() {
    for &opcode in INTERFACE_OPCODES_377 {
        let payload = sample_payload(opcode);
        for end in 0..payload.len() {
            assert!(
                decode_packet_377(opcode, &payload[..end]).is_err(),
                "opcode {opcode} accepted a {end}-byte prefix"
            );
        }
        let mut trailing = payload;
        trailing.push(0xA5);
        assert!(
            decode_packet_377(opcode, &trailing).is_err(),
            "opcode {opcode} accepted trailing bytes"
        );
    }
}

#[test]
fn java_sentinels_and_noop_shapes_are_not_reinterpreted() {
    // IF_SETOBJECT's object 65535 bypasses Java's divisor calculation.
    let mut object = Vec::new();
    put_g2(&mut object, 0);
    put_g2_alt1(&mut object, 0xFFFF);
    put_g2_alt3(&mut object, 0x1234);
    assert_packet(
        21,
        &object,
        InterfaceOperation377::SetObject {
            component: 0x1234,
            object: 0xFFFF,
            divisor: 0,
        },
    );

    decode_packet_377(21, &object)
        .unwrap()
        .validate(&TestPacketContext377::denying(
            DeniedResource::Object,
            0xFFFF,
        ))
        .expect("the sentinel branch does not resolve an object");

    // IF_OPENFULL reads an unsigned overlay. Do not convert 65535 to -1.
    let mut full = Vec::new();
    put_g2_alt1(&mut full, 0xFFFF);
    put_g2_alt2(&mut full, 0x1234);
    let decoded = decode_packet_377(253, &full)
        .unwrap()
        .validate(&TestPacketContext377::all_present())
        .unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation377::Interface(InterfaceOperation377::OpenFull {
            overlay: 0xFFFF,
            main: 0x1234,
        })
    );

    // Sequence -1 is the Java IF_SETANIM no-animation sentinel.
    let mut anim = Vec::new();
    put_g2_alt3(&mut anim, 0x1234);
    put_g2b_alt2(&mut anim, -1);
    assert_packet(
        2,
        &anim,
        InterfaceOperation377::SetAnim {
            component: 0x1234,
            sequence: -1,
        },
    );

    // Java compares TUT_FLASH values but does not index an array here; retain
    // the byte value even when it cannot flash a visible tab.
    let mut flash = Vec::new();
    put_g1(&mut flash, 255);
    assert_packet(238, &flash, InterfaceOperation377::TutFlash { tab: 255 });

    // IF_SETHIDE is exactly g1() == 1, not a general nonzero boolean.
    let mut hide = Vec::new();
    put_g1(&mut hide, 2);
    put_g2(&mut hide, 0x1234);
    assert_packet(
        82,
        &hide,
        InterfaceOperation377::SetHide {
            component: 0x1234,
            hidden: false,
        },
    );
}

#[test]
fn signed_component_reads_keep_engine_high_bits_as_java_negative_ids() {
    let mut tut_open = Vec::new();
    put_g2b_alt1(&mut tut_open, -2345);
    let decoded = decode_packet_377(158, &tut_open).unwrap();
    assert_eq!(
        decoded
            .validate(&TestPacketContext377::all_present())
            .unwrap_err()
            .kind,
        PacketErrorKind377::InvalidReference {
            kind: ReferenceKind377::Component,
            id: -2345,
        }
    );

    let mut overlay = Vec::new();
    put_g2b(&mut overlay, -1234);
    let decoded = decode_packet_377(50, &overlay).unwrap();
    assert_eq!(
        decoded
            .validate(&TestPacketContext377::all_present())
            .unwrap_err()
            .kind,
        PacketErrorKind377::InvalidReference {
            kind: ReferenceKind377::Component,
            id: -1234,
        }
    );
}

#[test]
fn intrinsic_divisor_and_tab_index_checks_match_java_array_use() {
    let mut object = Vec::new();
    put_g2(&mut object, 0);
    put_g2_alt1(&mut object, 0x1234);
    put_g2_alt3(&mut object, 0x2345);
    assert_eq!(
        decode_packet_377(21, &object).unwrap_err().kind,
        PacketErrorKind377::InvalidValue("object_divisor")
    );

    let mut set_tab = Vec::new();
    put_g1_alt3(&mut set_tab, 15);
    put_g2_alt2(&mut set_tab, 0x1234);
    assert_eq!(
        decode_packet_377(10, &set_tab).unwrap_err().kind,
        PacketErrorKind377::InvalidValue("tab_index")
    );

    let mut active_tab = Vec::new();
    put_g1_alt2(&mut active_tab, 15);
    assert_eq!(
        decode_packet_377(252, &active_tab).unwrap_err().kind,
        PacketErrorKind377::InvalidValue("tab_index")
    );
}

#[test]
fn resource_validation_fails_before_a_validated_handoff() {
    let payload = sample_payload(166);
    let decoded = decode_packet_377(166, &payload).unwrap();
    assert!(decoded
        .validate(&TestPacketContext377::denying(
            DeniedResource::Component,
            0x1234,
        ))
        .is_err());

    let payload = sample_payload(21);
    let decoded = decode_packet_377(21, &payload).unwrap();
    assert!(decoded
        .validate(&TestPacketContext377::denying(
            DeniedResource::Object,
            0x2345,
        ))
        .is_err());

    let payload = sample_payload(162);
    let decoded = decode_packet_377(162, &payload).unwrap();
    assert!(decoded
        .validate(&TestPacketContext377::denying(DeniedResource::Npc, 0x2345,))
        .is_err());

    let payload = sample_payload(2);
    let decoded = decode_packet_377(2, &payload).unwrap();
    assert!(decoded
        .validate(&TestPacketContext377::denying(
            DeniedResource::Sequence,
            0x1234,
        ))
        .is_err());

    let payload = sample_payload(216);
    let decoded = decode_packet_377(216, &payload).unwrap();
    assert!(decoded
        .validate(&TestPacketContext377::denying(
            DeniedResource::Model,
            0x2345,
        ))
        .is_err());
}
