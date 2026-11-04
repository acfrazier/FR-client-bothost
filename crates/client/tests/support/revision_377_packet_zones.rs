//! Packet-boundary tests for revision-377 zone frames and child operations.
//! The fixtures below encode the Java/engine wire transforms independently of
//! the decoder; the test context is deliberately a test-only resource view.

use crate::packet_377::zone_377::{
    ZoneFrame377, ZoneOperation377, ZoneOperations377, ZoneTileOffset377, ALL_OPCODES_377,
};
use crate::packet_377::{
    decode_packet_377, Operation377, PacketContext377, PacketErrorKind377, ReferenceKind377,
    ValidatedPacket377,
};

struct TestPacketContext377 {
    missing: Option<(ReferenceKind377, i32)>,
}

impl TestPacketContext377 {
    fn complete() -> Self {
        Self { missing: None }
    }

    fn missing(kind: ReferenceKind377, id: i32) -> Self {
        Self {
            missing: Some((kind, id)),
        }
    }
}

impl PacketContext377 for TestPacketContext377 {
    fn contains(&self, kind: ReferenceKind377, id: i32) -> bool {
        self.missing != Some((kind, id))
    }

    fn inventory_capacity(&self, _component: i32) -> Option<usize> {
        None
    }
}

fn p1_alt1(bytes: &mut Vec<u8>, value: i32) {
    bytes.push(value.wrapping_add(128) as u8);
}

fn p1_alt2(bytes: &mut Vec<u8>, value: i32) {
    bytes.push(value.wrapping_neg() as u8);
}

fn p1_alt3(bytes: &mut Vec<u8>, value: i32) {
    bytes.push(128i32.wrapping_sub(value) as u8);
}

fn p2(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&(value as u16).to_be_bytes());
}

fn p2_alt1(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&(value as u16).to_le_bytes());
}

fn p2_alt2(bytes: &mut Vec<u8>, value: i32) {
    let value = value as u16;
    bytes.push((value >> 8) as u8);
    bytes.push((value as u8).wrapping_add(128));
}

fn p2_alt3(bytes: &mut Vec<u8>, value: i32) {
    let value = value as u16;
    bytes.push((value as u8).wrapping_add(128));
    bytes.push((value >> 8) as u8);
}

struct ZoneCase {
    opcode: u8,
    body: Vec<u8>,
    expected: ZoneOperation377,
}

fn cases() -> Vec<ZoneCase> {
    let mut body = Vec::new();
    p2(&mut body, 77);
    body.push((10 << 2) | 3);
    p1_alt2(&mut body, -3);
    p1_alt1(&mut body, 0x52);
    p1_alt1(&mut body, 7);
    p2_alt2(&mut body, 0x1234);
    p2_alt1(&mut body, 13);
    body.push((-7i32) as u8);
    p1_alt1(&mut body, 2);
    p2(&mut body, 0x4567);
    let loc_merge = ZoneCase {
        opcode: 203,
        body,
        expected: ZoneOperation377::LocMerge {
            tile: ZoneTileOffset377 { x: 5, z: 2 },
            shape: 10,
            angle: 3,
            location: 77,
            min_x_offset: -3,
            max_x_offset: 7,
            min_z_offset: -7,
            max_z_offset: 2,
            player_slot: 13,
            start_cycle: 0x4567,
            end_cycle: 0x1234,
        },
    };

    let mut body = Vec::new();
    p1_alt1(&mut body, 0xa6);
    p2_alt3(&mut body, 0xabcd);
    p2_alt2(&mut body, 321);
    p2_alt2(&mut body, 18);
    let object_reveal = ZoneCase {
        opcode: 106,
        body,
        expected: ZoneOperation377::ObjectReveal {
            tile: ZoneTileOffset377 { x: 2, z: 6 },
            count: 0xabcd,
            object: 321,
            receiver_id: 18,
        },
    };

    let mut body = Vec::new();
    p2(&mut body, 0x1234);
    p1_alt1(&mut body, (12 << 2) | 2);
    body.push(0x3e);
    let loc_anim = ZoneCase {
        opcode: 142,
        body,
        expected: ZoneOperation377::LocAnim {
            tile: ZoneTileOffset377 { x: 3, z: 6 },
            shape: 12,
            angle: 2,
            sequence: 0x1234,
        },
    };

    let mut body = Vec::new();
    p2(&mut body, 0x1234);
    p1_alt2(&mut body, 0x5a);
    p2_alt2(&mut body, 0xabcd);
    let obj_add = ZoneCase {
        opcode: 107,
        body,
        expected: ZoneOperation377::ObjAdd {
            tile: ZoneTileOffset377 { x: 5, z: 2 },
            object: 0x1234,
            count: 0xabcd,
        },
    };

    let mut body = vec![0x6f];
    p2(&mut body, 0x9abc);
    p2(&mut body, 0x3456);
    p2(&mut body, 0x789a);
    let obj_count = ZoneCase {
        opcode: 121,
        body,
        expected: ZoneOperation377::ObjCount {
            tile: ZoneTileOffset377 { x: 6, z: 7 },
            object: 0x1abc,
            old_count: 0x3456,
            new_count: 0x789a,
        },
    };

    let mut body = vec![0x72, (-3i32) as u8, 5];
    p2(&mut body, -9);
    p2(&mut body, 0x1234);
    body.push(13);
    body.push(17);
    p2(&mut body, 0x2345);
    p2(&mut body, 0x3456);
    body.extend_from_slice(&[29, 31]);
    let map_proj_anim = ZoneCase {
        opcode: 181,
        body,
        expected: ZoneOperation377::MapProjAnim {
            tile: ZoneTileOffset377 { x: 7, z: 2 },
            destination_x_offset: -3,
            destination_z_offset: 5,
            target: -9,
            spotanim: Some(0x1234),
            source_height: 52,
            destination_height: 68,
            start_delay: 0x2345,
            end_delay: 0x3456,
            peak: 29,
            arc: 31,
        },
    };

    let mut body = vec![0x83];
    p2(&mut body, 0x4321);
    body.push(0xa5);
    let sound_area = ZoneCase {
        opcode: 41,
        body,
        expected: ZoneOperation377::SoundArea {
            tile: ZoneTileOffset377 { x: 0, z: 3 }, // Java masks the high x bit.
            sound: 0x4321,
            radius: 10,
            loops: 5,
        },
    };

    let mut body = vec![0x94];
    p2(&mut body, 0x5678);
    body.push(0xab);
    p2(&mut body, 0x1234);
    let map_anim = ZoneCase {
        opcode: 59,
        body,
        expected: ZoneOperation377::MapAnim {
            tile: ZoneTileOffset377 { x: 1, z: 4 },
            spotanim: 0x5678,
            height: 0xab,
            delay: 0x1234,
        },
    };

    let mut body = Vec::new();
    p1_alt2(&mut body, (21 << 2) | 1);
    p2_alt3(&mut body, 0x2345);
    p1_alt1(&mut body, 0xa7);
    let loc_add_change = ZoneCase {
        opcode: 152,
        body,
        expected: ZoneOperation377::LocAddChange {
            tile: ZoneTileOffset377 { x: 2, z: 7 },
            shape: 21,
            angle: 1,
            location: 0x2345,
        },
    };

    let mut body = Vec::new();
    p2_alt2(&mut body, 0x8007);
    p1_alt1(&mut body, 0xb1);
    let obj_del = ZoneCase {
        opcode: 208,
        body,
        expected: ZoneOperation377::ObjDel {
            tile: ZoneTileOffset377 { x: 3, z: 1 },
            object: 7,
        },
    };

    let mut body = Vec::new();
    p1_alt3(&mut body, 0xc6);
    p1_alt3(&mut body, (22 << 2) | 3);
    let loc_del = ZoneCase {
        opcode: 88,
        body,
        expected: ZoneOperation377::LocDel {
            tile: ZoneTileOffset377 { x: 4, z: 6 },
            shape: 22,
            angle: 3,
        },
    };

    vec![
        loc_merge,
        object_reveal,
        loc_anim,
        obj_add,
        obj_count,
        map_proj_anim,
        sound_area,
        map_anim,
        loc_add_change,
        obj_del,
        loc_del,
    ]
}

fn zone_frame<'a>(packet: &'a ValidatedPacket377<'_>) -> &'a ZoneFrame377 {
    match packet.operation() {
        Operation377::Zone(frame) => frame,
        other => panic!("expected zone operation, got {other:?}"),
    }
}

fn sole_operation<'a>(packet: &'a ValidatedPacket377<'_>) -> &'a ZoneOperation377 {
    let operations = zone_frame(packet).operations();
    assert_eq!(operations.len(), 1);
    &operations[0]
}

fn decoded_zone(opcode: u8, payload: &[u8]) -> ValidatedPacket377<'_> {
    let packet = decode_packet_377(opcode, payload)
        .unwrap()
        .validate(&TestPacketContext377::complete())
        .unwrap();
    assert!(matches!(packet.operation(), Operation377::Zone(_)));
    packet
}

fn required_references(operation: &ZoneOperation377) -> Vec<(ReferenceKind377, i32)> {
    use ReferenceKind377 as R;
    match operation {
        ZoneOperation377::LocMerge {
            location,
            player_slot,
            ..
        } => {
            vec![(R::Location, *location), (R::Player, *player_slot)]
        }
        ZoneOperation377::ObjectReveal {
            object,
            receiver_id,
            ..
        } => {
            let mut references = vec![(R::Object, *object)];
            if *receiver_id != 65_535 {
                references.push((R::Player, *receiver_id));
            }
            references
        }
        ZoneOperation377::LocAnim { sequence, .. } => vec![(R::Sequence, *sequence)],
        ZoneOperation377::ObjAdd { object, .. }
        | ZoneOperation377::ObjCount { object, .. }
        | ZoneOperation377::ObjDel { object, .. } => vec![(R::Object, *object)],
        ZoneOperation377::MapProjAnim {
            target, spotanim, ..
        } => {
            let mut references = Vec::new();
            if let Some(spotanim) = spotanim {
                references.push((R::Spotanim, *spotanim));
                if *target > 0 {
                    references.push((R::NpcSlot, *target - 1));
                } else if *target < 0 {
                    references.push((R::Player, -(*target) - 1));
                }
            }
            references
        }
        ZoneOperation377::SoundArea { sound, .. } => vec![(R::Sound, *sound)],
        ZoneOperation377::MapAnim { spotanim, .. } => vec![(R::Spotanim, *spotanim)],
        ZoneOperation377::LocAddChange { location, .. } => vec![(R::Location, *location)],
        ZoneOperation377::LocDel { .. } => Vec::new(),
    }
}

#[test]
fn all_zone_opcodes_and_frame_forms_decode_with_exact_origin_and_inline_singleton() {
    assert_eq!(
        ALL_OPCODES_377,
        &[40, 75, 183, 203, 106, 142, 107, 121, 181, 41, 59, 152, 208, 88]
    );

    let mut full = Vec::new();
    p1_alt3(&mut full, 9);
    p1_alt2(&mut full, 17);
    let full = decoded_zone(40, &full);
    assert_eq!(zone_frame(&full).origin(), Some((17, 9)));
    assert!(zone_frame(&full).clear());
    assert!(zone_frame(&full).operations().is_empty());
    assert!(matches!(
        zone_frame(&full).operation_storage(),
        ZoneOperations377::Empty
    ));

    let mut partial = Vec::new();
    p1_alt2(&mut partial, 18);
    p1_alt1(&mut partial, 23);
    let partial = decoded_zone(75, &partial);
    assert_eq!(zone_frame(&partial).origin(), Some((18, 23)));
    assert!(!zone_frame(&partial).clear());
    assert!(zone_frame(&partial).operations().is_empty());

    let child = cases().into_iter().find(|case| case.opcode == 107).unwrap();
    let mut enclosed = vec![12];
    p1_alt1(&mut enclosed, 20);
    enclosed.push(child.opcode);
    enclosed.extend_from_slice(&child.body);
    let packet = decoded_zone(183, &enclosed);
    assert_eq!(zone_frame(&packet).origin(), Some((12, 20)));
    assert!(!zone_frame(&packet).clear());
    assert_eq!(sole_operation(&packet), &child.expected);
    assert!(matches!(
        zone_frame(&packet).operation_storage(),
        ZoneOperations377::Enclosed(_)
    ));
}

#[test]
fn every_zone_subopcode_uses_the_same_field_parser_standalone_and_enclosed() {
    for case in cases() {
        let opcode = case.opcode;
        let standalone = decoded_zone(case.opcode, &case.body);
        let standalone_frame = zone_frame(&standalone);
        assert_eq!(standalone_frame.origin(), None, "opcode {opcode}");
        assert!(!standalone_frame.clear(), "opcode {opcode}");
        assert_eq!(
            sole_operation(&standalone),
            &case.expected,
            "opcode {opcode}"
        );
        assert!(matches!(
            standalone_frame.operation_storage(),
            ZoneOperations377::Single(_)
        ));

        let mut enclosed = vec![12];
        p1_alt1(&mut enclosed, 20);
        enclosed.push(case.opcode);
        enclosed.extend_from_slice(&case.body);
        let packet = decoded_zone(183, &enclosed);
        let frame = zone_frame(&packet);
        assert_eq!(frame.origin(), Some((12, 20)), "opcode {opcode}");
        assert_eq!(sole_operation(&packet), &case.expected, "opcode {opcode}");
    }
}

#[test]
fn loc_anim_preserves_java_shape_eleven_to_ten_normalization() {
    let mut body = Vec::new();
    p2(&mut body, 0x1234);
    p1_alt1(&mut body, (11 << 2) | 3);
    body.push(0x27);
    let packet = decoded_zone(142, &body);
    assert_eq!(
        sole_operation(&packet),
        &ZoneOperation377::LocAnim {
            tile: ZoneTileOffset377 { x: 2, z: 7 },
            shape: 10,
            angle: 3,
            sequence: 0x1234,
        }
    );
}

#[test]
fn java_signed_edges_and_zone_specific_sentinels_are_preserved() {
    let mut merge = Vec::new();
    p2(&mut merge, 1);
    merge.push(0);
    p1_alt2(&mut merge, -128);
    p1_alt1(&mut merge, 0x77);
    p1_alt1(&mut merge, 127);
    p2_alt2(&mut merge, 2);
    p2_alt1(&mut merge, 3);
    merge.push((-128i32) as u8);
    p1_alt1(&mut merge, 127);
    p2(&mut merge, 4);
    let decoded = decoded_zone(203, &merge);
    assert_eq!(
        sole_operation(&decoded),
        &ZoneOperation377::LocMerge {
            tile: ZoneTileOffset377 { x: 7, z: 7 },
            shape: 0,
            angle: 0,
            location: 1,
            min_x_offset: -128,
            max_x_offset: 127,
            min_z_offset: -128,
            max_z_offset: 127,
            player_slot: 3,
            start_cycle: 4,
            end_cycle: 2,
        }
    );

    let mut projectile = vec![0x11, 0x80, 0x7f];
    p2(&mut projectile, -32_768);
    p2(&mut projectile, 65_535);
    projectile.extend_from_slice(&[0, 255]);
    p2(&mut projectile, 1);
    p2(&mut projectile, 2);
    projectile.extend_from_slice(&[3, 4]);
    let decoded_projectile = decoded_zone(181, &projectile);
    assert!(matches!(
        sole_operation(&decoded_projectile),
        ZoneOperation377::MapProjAnim {
            tile: ZoneTileOffset377 { x: 1, z: 1 },
            destination_x_offset: -128,
            destination_z_offset: 127,
            target: -32_768,
            spotanim: None,
            source_height: 0,
            destination_height: 1020,
            ..
        }
    ));
    // The no-projectile sentinel suppresses both asset and actor lookups.
    decode_packet_377(181, &projectile)
        .unwrap()
        .validate(&TestPacketContext377::missing(
            ReferenceKind377::Player,
            2047,
        ))
        .unwrap();

    let mut reveal = Vec::new();
    p1_alt1(&mut reveal, 0x22);
    p2_alt3(&mut reveal, 1);
    p2_alt2(&mut reveal, 2);
    p2_alt2(&mut reveal, 65_535);
    let reveal = decode_packet_377(106, &reveal).unwrap();
    reveal
        .validate(&TestPacketContext377::missing(
            ReferenceKind377::Player,
            65_535,
        ))
        .unwrap();
}

#[test]
fn each_resource_and_actor_reference_is_checked_before_handoff() {
    for case in cases() {
        let opcode = case.opcode;
        for (kind, id) in required_references(&case.expected) {
            let packet = decode_packet_377(case.opcode, &case.body).unwrap();
            let error = packet
                .validate(&TestPacketContext377::missing(kind, id))
                .unwrap_err();
            assert_eq!(
                error.kind,
                PacketErrorKind377::InvalidReference { kind, id },
                "opcode {opcode} resource {kind:?} {id}",
            );
        }
    }

    let mut npc_projectile = vec![0x11, 0, 0];
    p2(&mut npc_projectile, 16_384);
    p2(&mut npc_projectile, 5);
    npc_projectile.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    let npc_projectile = decode_packet_377(181, &npc_projectile).unwrap();
    let error = npc_projectile
        .validate(&TestPacketContext377::missing(
            ReferenceKind377::NpcSlot,
            16_383,
        ))
        .unwrap_err();
    assert_eq!(
        error.kind,
        PacketErrorKind377::InvalidReference {
            kind: ReferenceKind377::NpcSlot,
            id: 16_383
        }
    );
}

#[test]
fn malformed_second_enclosed_operation_fails_before_validated_handoff() {
    let first = cases().into_iter().find(|case| case.opcode == 107).unwrap();
    let second = cases().into_iter().find(|case| case.opcode == 142).unwrap();
    let mut enclosed = vec![12];
    p1_alt1(&mut enclosed, 20);
    enclosed.push(first.opcode);
    enclosed.extend_from_slice(&first.body);
    enclosed.push(second.opcode);
    enclosed.push(0x12); // sequence word is incomplete
    let error = decode_packet_377(183, &enclosed).unwrap_err();
    assert!(matches!(error.kind, PacketErrorKind377::Truncated { .. }));

    let mut enclosed = vec![12];
    p1_alt1(&mut enclosed, 20);
    enclosed.push(first.opcode);
    enclosed.extend_from_slice(&first.body);
    enclosed.push(second.opcode);
    enclosed.extend_from_slice(&second.body);
    let decoded = decode_packet_377(183, &enclosed).unwrap();
    let error = decoded
        .validate(&TestPacketContext377::missing(
            ReferenceKind377::Sequence,
            0x1234,
        ))
        .unwrap_err();
    assert_eq!(
        error.kind,
        PacketErrorKind377::InvalidReference {
            kind: ReferenceKind377::Sequence,
            id: 0x1234
        }
    );
}

#[test]
fn truncation_trailing_unknown_children_and_invalid_shapes_are_rejected() {
    let object_add = cases().into_iter().find(|case| case.opcode == 107).unwrap();
    let short = &object_add.body[..object_add.body.len() - 1];
    assert!(matches!(
        decode_packet_377(object_add.opcode, short)
            .unwrap_err()
            .kind,
        PacketErrorKind377::WrongSize { .. }
    ));
    let mut trailing = object_add.body;
    trailing.push(0);
    assert!(matches!(
        decode_packet_377(object_add.opcode, &trailing)
            .unwrap_err()
            .kind,
        PacketErrorKind377::WrongSize { .. }
    ));

    let first = cases().into_iter().find(|case| case.opcode == 107).unwrap();
    let mut unknown_child = vec![12];
    p1_alt1(&mut unknown_child, 20);
    unknown_child.push(first.opcode);
    unknown_child.extend_from_slice(&first.body);
    unknown_child.push(255);
    assert_eq!(
        decode_packet_377(183, &unknown_child).unwrap_err().kind,
        PacketErrorKind377::UnknownOpcode
    );

    let mut invalid_shape = Vec::new();
    p1_alt3(&mut invalid_shape, 0x11);
    p1_alt3(&mut invalid_shape, (23 << 2) | 1);
    assert_eq!(
        decode_packet_377(88, &invalid_shape).unwrap_err().kind,
        PacketErrorKind377::InvalidValue("zone location shape")
    );

    let mut invalid_target = vec![0x11, 0, 0];
    p2(&mut invalid_target, -2049);
    p2(&mut invalid_target, 5);
    invalid_target.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        decode_packet_377(181, &invalid_target).unwrap_err().kind,
        PacketErrorKind377::InvalidValue("zone projectile target")
    );
    let mut invalid_player = Vec::new();
    p2(&mut invalid_player, 1);
    invalid_player.push(0);
    p1_alt2(&mut invalid_player, 0);
    p1_alt1(&mut invalid_player, 0);
    p1_alt1(&mut invalid_player, 0);
    p2_alt2(&mut invalid_player, 0);
    p2_alt1(&mut invalid_player, 2048);
    invalid_player.extend_from_slice(&[0, 128]);
    p2(&mut invalid_player, 0);
    assert_eq!(
        decode_packet_377(203, &invalid_player).unwrap_err().kind,
        PacketErrorKind377::InvalidValue("zone merge player slot")
    );
}
