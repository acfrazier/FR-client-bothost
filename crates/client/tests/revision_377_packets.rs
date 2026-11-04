//! Executes the actual additive modules without changing shared registrations.
//! Golden frames run original engine encoders and untouched Java Packet readers.
//! These tests prove decoding and validation, not runtime apply or a live login.

#[path = "support/revision_377_packet_interfaces.rs"]
mod interfaces;
#[path = "../src/client/packet_377.rs"]
mod packet_377;
#[path = "support/revision_377_packet_zones.rs"]
mod zones;

use packet_377::{
    decode_packet_377, CameraTarget377, HintArrow377, InventoryEntry377, InventorySlot377,
    Operation377 as O, PacketContext377, PacketErrorKind377 as E, PacketReader377,
    ReferenceKind377 as R, ALL_OPCODES_377, DEFERRED_OPCODES_377,
};
use serde_json::{json, Value};
use std::cell::Cell;

/// Deliberately synthetic index view. Production must supply the real cache and
/// session view. Missing-resource tests override one reference, not the decoder.
struct TestContext {
    deny: Option<(R, i32)>,
    capacity: Option<usize>,
    reads: Cell<usize>,
}
impl Default for TestContext {
    fn default() -> Self {
        Self {
            deny: None,
            capacity: Some(256),
            reads: Cell::new(0),
        }
    }
}
impl PacketContext377 for TestContext {
    fn contains(&self, kind: R, id: i32) -> bool {
        self.reads.set(self.reads.get() + 1);
        id >= 0 && self.deny != Some((kind, id))
    }
    fn inventory_capacity(&self, _component: i32) -> Option<usize> {
        self.capacity
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
fn w2(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&(value as u16).to_be_bytes());
}
fn w2le(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&(value as u16).to_le_bytes());
}
fn w2a(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&[(value >> 8) as u8, (value as u8).wrapping_add(128)]);
}
fn w2lea(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&[(value as u8).wrapping_add(128), (value >> 8) as u8]);
}

#[test]
fn original_engine_frames_match_original_java_primitives_and_complete_dispatch() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/revision_377/packets.json")).unwrap();
    assert_eq!(fixture["covered_opcodes"], json!(ALL_OPCODES_377));
    assert_eq!(fixture["deferred_opcodes"], json!(DEFERRED_OPCODES_377));
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 71);
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["origin"] == "original-engine-encoder")
            .count(),
        69
    );
    for case in cases {
        let opcode = case["opcode"].as_u64().unwrap() as u8;
        let data = unhex(case["payload_hex"].as_str().unwrap());
        let mut reader = PacketReader377::new(opcode, &data);
        let mut values = Vec::new();
        for method in case["reads"].as_array().unwrap() {
            let value = match method.as_str().unwrap() {
                "g1" => json!(reader.g1().unwrap()),
                "g1b" => json!(reader.g1b().unwrap()),
                "g1_alt1" => json!(reader.g1_alt1().unwrap()),
                "g1_alt2" => json!(reader.g1_alt2().unwrap()),
                "g1_alt3" => json!(reader.g1_alt3().unwrap()),
                "g1b_alt1" => json!(reader.g1b_alt1().unwrap()),
                "g1b_alt2" => json!(reader.g1b_alt2().unwrap()),
                "g1b_alt3" => json!(reader.g1b_alt3().unwrap()),
                "g2" => json!(reader.g2().unwrap()),
                "g2b" => json!(reader.g2b().unwrap()),
                "g2_alt1" => json!(reader.g2_alt1().unwrap()),
                "g2_alt2" => json!(reader.g2_alt2().unwrap()),
                "g2_alt3" => json!(reader.g2_alt3().unwrap()),
                "g2b_alt1" => json!(reader.g2b_alt1().unwrap()),
                "g2b_alt2" => json!(reader.g2b_alt2().unwrap()),
                "g3_alt3" => json!(reader.g3_alt3().unwrap()),
                "g4" => json!(reader.g4().unwrap()),
                "g4_alt1" => json!(reader.g4_alt1().unwrap()),
                "g4_alt3" => json!(reader.g4_alt3().unwrap()),
                "g8" => json!(reader.g8().unwrap().to_string()),
                "gsmarts" => json!(reader.gsmarts().unwrap()),
                "gjstr" => json!(std::str::from_utf8(reader.gjstr().unwrap()).unwrap()),
                "rest" => {
                    let mut rest = Vec::new();
                    while reader.remaining() != 0 {
                        rest.push(reader.g1().unwrap() as u8);
                    }
                    json!(hex(&rest))
                }
                other => panic!("unhandled Java primitive {other}"),
            };
            values.push(value);
        }
        assert_eq!(
            json!(values),
            case["java_values"],
            "{} primitive trace",
            case["name"]
        );
        assert_eq!(
            reader.position(),
            case["consumed"].as_u64().unwrap() as usize
        );
        assert_eq!(reader.remaining(), 0);
        let decoded = decode_packet_377(opcode, &data)
            .unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
        decoded
            .validate(&TestContext::default())
            .unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
        // Every strict prefix is safe to inspect, even where a list prefix is a
        // complete shorter valid packet. No read can escape its supplied slice.
        for end in 0..data.len() {
            let _ = decode_packet_377(opcode, &data[..end]);
        }
        let mut extended = data.clone();
        extended.push(0xff);
        if opcode != 135 {
            assert!(
                decode_packet_377(opcode, &extended).is_err(),
                "{} accepted a trailing byte",
                case["name"]
            );
        }
    }
    println!("377 inbound: 71/75 Java handlers; 69 executed engine encoders; 71 complete Java primitive traces; 4 deferred handlers");
}

#[test]
fn status_social_audio_and_camera_table_has_java_field_order() {
    let target = || CameraTarget377 {
        x: 2,
        z: 3,
        height: 0x456,
        rate: 7,
        acceleration: 101,
    };
    let cases: Vec<(u8, Vec<u8>, O<'_>)> = vec![
        (
            26,
            vec![1, 2, 3, 4, 5],
            O::SynthSound {
                sound: 258,
                loops: 3,
                delay: 1029,
            },
        ),
        (
            182,
            vec![1, 0x83, 165],
            O::Varp {
                id: 259,
                value: -37,
            },
        ),
        (
            115,
            vec![0x34, 0x12, 0x78, 0x56, 0x23, 0x01],
            O::Varp {
                id: 0x123,
                value: 0x12345678,
            },
        ),
        (113, vec![], O::VarpSync),
        (13, vec![], O::ResetAnims),
        (156, vec![2], O::MinimapToggle(2)),
        (220, vec![0x7f, 0xff], O::MidiSong(-1)),
        (
            249,
            vec![0x34, 0x12, 0x34, 0x12, 0x56],
            O::MidiJingle {
                song: 0x1234,
                delay: 0x123456,
            },
        ),
        (
            157,
            b"\xfcTrade\n\0".to_vec(),
            O::PlayerOption {
                index: 4,
                text: b"Trade",
                priority: 0,
            },
        ),
        (
            201,
            vec![1, 2, 3],
            O::ChatFilter {
                public: 1,
                private: 2,
                trade: 3,
            },
        ),
        (3, vec![2, 3, 4, 0x56, 7, 101], O::CameraMoveTo(target())),
        (167, vec![2, 3, 4, 0x56, 7, 101], O::CameraLookAt(target())),
        (5, vec![], O::Logout),
        (63, b"hello\n".to_vec(), O::MessageGame(b"hello")),
        (174, vec![0xfe, 0xbf], O::RunWeight(-321)),
        (233, vec![1], O::Multiway(1)),
        (61, vec![], O::UnsetMapFlag),
        (
            67,
            vec![4, 17, 19, 23],
            O::CameraShake {
                axis: 4,
                jitter: 17,
                amplitude: 19,
                rate: 23,
            },
        ),
        (
            78,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 42],
            O::Friend {
                name: 0x0102030405060708,
                world: 42,
            },
        ),
        (
            49,
            vec![253, 67, 0, 1, 0xe2, 0x40],
            O::UpdateStat {
                stat: 3,
                level: 67,
                experience: 123456,
            },
        ),
        (190, vec![123, 0], O::RebootTimer { ticks: 123 }),
        (125, vec![73], O::RunEnergy(73)),
        (
            219,
            vec![0x34, 0x12],
            O::InventoryStopTransmit { component: 0x1234 },
        ),
        (148, vec![], O::CameraReset),
        (
            126,
            vec![1, 0x56, 4],
            O::UpdatePid {
                members: 1,
                slot: 0x456,
            },
        ),
        (251, vec![2], O::FriendListLoaded(2)),
    ];
    for (opcode, bytes, expected) in cases {
        let result = decode_packet_377(opcode, &bytes)
            .unwrap()
            .validate(&TestContext::default())
            .unwrap();
        assert_eq!(result.operation(), &expected, "opcode {opcode}");
        assert_eq!(result.into_operation(), expected);
    }
}

#[test]
fn inventories_preserve_id_plus_one_and_distinct_full_partial_transforms() {
    let mut full = vec![];
    w2(&mut full, 0x1234);
    w2(&mut full, 2);
    w2lea(&mut full, 124);
    full.push(254); // count 2, not literal 254.
    w2lea(&mut full, 235);
    full.push(1);
    full.extend_from_slice(&65537_i32.to_le_bytes());
    assert_eq!(
        decode_packet_377(206, &full).unwrap().operation(),
        &O::InventoryFull {
            component: 0x1234,
            entries: vec![
                InventoryEntry377 {
                    object_plus_one: 124,
                    count: 2
                },
                InventoryEntry377 {
                    object_plus_one: 235,
                    count: 65537
                }
            ]
        }
    );
    let mut partial = vec![];
    w2(&mut partial, 0x1234);
    partial.push(127);
    w2(&mut partial, 124);
    partial.push(254);
    w2(&mut partial, 0x8080);
    w2(&mut partial, 235);
    partial.push(255);
    partial.extend_from_slice(&(-123_i32).to_be_bytes());
    assert_eq!(
        decode_packet_377(134, &partial).unwrap().operation(),
        &O::InventoryPartial {
            component: 0x1234,
            entries: vec![
                InventorySlot377 {
                    slot: 127,
                    entry: InventoryEntry377 {
                        object_plus_one: 124,
                        count: 254
                    }
                },
                InventorySlot377 {
                    slot: 128,
                    entry: InventoryEntry377 {
                        object_plus_one: 235,
                        count: -123
                    }
                }
            ]
        }
    );
    let short_capacity = TestContext {
        capacity: Some(1),
        ..Default::default()
    };
    assert!(matches!(
        decode_packet_377(206, &full)
            .unwrap()
            .validate(&short_capacity)
            .unwrap_err()
            .kind,
        E::InvalidValue("full inventory capacity")
    ));
    // Out-of-range partial updates are consumed and ignored by Java, not a
    // protocol failure even when their object would otherwise be unavailable.
    let ignored = TestContext {
        deny: Some((R::Object, 234)),
        capacity: Some(128),
        ..Default::default()
    };
    decode_packet_377(134, &partial)
        .unwrap()
        .validate(&ignored)
        .unwrap();
    full.pop();
    assert!(decode_packet_377(206, &full).is_err());
    assert!(decode_packet_377(206, &[0, 1, 0xff, 0xff]).is_err());
}

#[test]
fn original_engine_inventory_loops_match_all_java_slots_and_extended_counts() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/revision_377/packets.json")).unwrap();
    for opcode in [134, 206] {
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["opcode"] == opcode)
            .unwrap();
        let bytes = unhex(case["payload_hex"].as_str().unwrap());
        let mut values = case["java_values"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_i64().unwrap() as i32);
        let component = values.next().unwrap();
        let operation = decode_packet_377(opcode, &bytes)
            .unwrap()
            .validate(&TestContext::default())
            .unwrap()
            .into_operation();
        if opcode == 206 {
            let count = values.next().unwrap() as usize;
            let expected: Vec<_> = (0..count)
                .map(|_| {
                    let object_plus_one = values.next().unwrap();
                    let count = values.next().unwrap();
                    let count = if count == 255 {
                        values.next().unwrap()
                    } else {
                        count
                    };
                    InventoryEntry377 {
                        object_plus_one,
                        count,
                    }
                })
                .collect();
            assert_eq!(count, 129);
            assert_eq!(
                operation,
                O::InventoryFull {
                    component,
                    entries: expected
                }
            );
        } else {
            let mut expected = Vec::new();
            while let Some(slot) = values.next() {
                let object_plus_one = values.next().unwrap();
                let count = values.next().unwrap();
                let count = if count == 255 {
                    values.next().unwrap()
                } else {
                    count
                };
                expected.push(InventorySlot377 {
                    slot,
                    entry: InventoryEntry377 {
                        object_plus_one,
                        count,
                    },
                });
            }
            assert_eq!(
                expected.iter().map(|entry| entry.slot).collect::<Vec<_>>(),
                [0, 1, 128]
            );
            assert_eq!(
                operation,
                O::InventoryPartial {
                    component,
                    entries: expected
                }
            );
        }
        assert_eq!(values.next(), None);
    }
}

#[test]
fn last_login_preserves_all_fields_and_reserved_values() {
    let mut bytes = vec![];
    w2le(&mut bytes, 1);
    w2lea(&mut bytes, 2);
    w2(&mut bytes, 3);
    w2(&mut bytes, 4);
    w2le(&mut bytes, 5);
    w2a(&mut bytes, 6);
    w2a(&mut bytes, 7);
    w2(&mut bytes, 8);
    bytes.extend_from_slice(&0x7f000001_i32.to_le_bytes());
    w2lea(&mut bytes, 9);
    bytes.push(138);
    let O::LastLogin(info) = decode_packet_377(76, &bytes)
        .unwrap()
        .validate(&TestContext::default())
        .unwrap()
        .into_operation()
    else {
        panic!()
    };
    assert_eq!(
        [
            info.days_since_password_change,
            info.field371,
            info.unused_word,
            info.field504,
            info.current_day,
            info.unread_messages,
            info.previous_login_day,
            info.members_days,
            info.last_address,
            info.recovery_change_day,
            info.unused_byte
        ],
        [1, 2, 3, 4, 5, 6, 7, 8, 0x7f000001, 9, 10]
    );
}

#[test]
fn hints_consume_ignored_fixed_tails_without_inventing_zero_constraints() {
    for kind in [0, 1, 2, 3, 4, 5, 6, 10, 255] {
        let data = [kind, 1, 2, 3, 4, 5];
        let decoded = decode_packet_377(199, &data).unwrap();
        match decoded.operation() {
            O::Hint(HintArrow377::Npc { slot, padding })
            | O::Hint(HintArrow377::Player { slot, padding }) => {
                assert_eq!(*slot, 258);
                assert_eq!(*padding, [3, 4, 5]);
            }
            O::Hint(HintArrow377::Tile {
                kind: k,
                x,
                z,
                height,
            }) => {
                assert_eq!(*k, kind as i32);
                assert_eq!((*x, *z, *height), (258, 772, 5));
            }
            O::Hint(HintArrow377::Other { kind: k, padding }) => {
                assert_eq!(*k, kind as i32);
                assert_eq!(*padding, [1, 2, 3, 4, 5]);
            }
            _ => panic!(),
        }
    }
}

#[test]
fn borrowed_text_private_body_and_ignore_names_do_not_copy_frame_storage() {
    let text = b"hello\n";
    let packet = decode_packet_377(63, text).unwrap();
    let O::MessageGame(value) = packet.operation() else {
        panic!()
    };
    assert_eq!(value.as_ptr(), text.as_ptr());
    let bytes = [
        1, 2, 3, 4, 5, 6, 7, 8, 0x12, 0x34, 0x56, 0x78, 3, 0xab, 0xcd,
    ];
    let packet = decode_packet_377(135, &bytes).unwrap();
    assert!(
        matches!(packet.operation(),O::PrivateMessage {from:0x0102030405060708,message_id:0x12345678,staff_level:3,packed_body} if packed_body.as_ptr()==bytes[13..].as_ptr())
    );
    let names = [0xff; 8];
    let packet = decode_packet_377(226, &names).unwrap();
    let O::IgnoreList(list) = packet.operation() else {
        panic!()
    };
    assert_eq!(list.iter().collect::<Vec<_>>(), vec![-1]);
    assert!(decode_packet_377(226, &[0; 9]).is_err());
    assert!(decode_packet_377(226, &[0; 808]).is_err());
}

#[test]
fn validation_and_framing_fail_before_an_applicable_handoff() {
    let context = TestContext {
        deny: Some((R::Varp, 259)),
        ..Default::default()
    };
    let error = decode_packet_377(182, &[1, 0x83, 165])
        .unwrap()
        .validate(&context)
        .unwrap_err();
    assert_eq!(
        error.kind,
        E::InvalidReference {
            kind: R::Varp,
            id: 259
        }
    );
    assert_eq!(error.offset, 3);
    assert_eq!(context.reads.get(), 1);
    assert!(decode_packet_377(182, &[1, 0x83, 165, 0]).is_err());
    assert_eq!(context.reads.get(), 1); // framing failed without any state query.
    assert!(matches!(
        decode_packet_377(67, &[5, 1, 2, 3]).unwrap_err().kind,
        E::InvalidValue(_)
    ));
    assert_eq!(
        decode_packet_377(63, b"x").unwrap_err().kind,
        E::UnterminatedString
    );
    assert_eq!(
        decode_packet_377(63, b"x\ny").unwrap_err().kind,
        E::TrailingBytes(1)
    );
    assert!(matches!(
        decode_packet_377(63, &[0; 256]).unwrap_err().kind,
        E::FrameTooLarge { maximum: 255, .. }
    ));
    assert!(matches!(
        decode_packet_377(226, &[0; 65536]).unwrap_err().kind,
        E::FrameTooLarge { maximum: 65535, .. }
    ));
    for opcode in 0..=255 {
        if DEFERRED_OPCODES_377.contains(&opcode) {
            assert_eq!(
                decode_packet_377(opcode, &[]).unwrap_err().kind,
                E::DeferredOpcode
            );
        } else if !ALL_OPCODES_377.contains(&opcode) {
            assert_eq!(
                decode_packet_377(opcode, &[]).unwrap_err().kind,
                E::UnknownOpcode
            );
        }
    }
}

#[test]
fn resource_checks_distinguish_assets_actor_slots_and_java_sound_sentinels() {
    let cases: &[(u8, &[u8], R, i32)] = &[
        (26, &[1, 200, 3, 0, 7], R::Sound, 456),
        (220, &[0x6a, 0], R::Midi, 234),
        (249, &[0xea, 0, 0x34, 0x12, 0x56], R::Midi, 234),
        (49, &[253, 67, 0, 1, 0xe2, 0x40], R::Stat, 3),
        (126, &[1, 0x56, 4], R::Player, 0x456),
        (199, &[1, 1, 2, 3, 4, 5], R::NpcSlot, 258),
        (199, &[10, 1, 2, 3, 4, 5], R::Player, 258),
    ];
    for &(opcode, bytes, kind, id) in cases {
        let context = TestContext {
            deny: Some((kind, id)),
            ..Default::default()
        };
        let error = decode_packet_377(opcode, bytes)
            .unwrap()
            .validate(&context)
            .unwrap_err();
        assert_eq!(error.kind, E::InvalidReference { kind, id });
    }
    // Java 7308-7313 never indexes the delay table on this special path.
    let context = TestContext {
        deny: Some((R::Sound, 65535)),
        ..Default::default()
    };
    decode_packet_377(26, &[255, 255, 3, 255, 255])
        .unwrap()
        .validate(&context)
        .unwrap();
    assert_eq!(context.reads.get(), 0);
    let context = TestContext {
        deny: Some((R::Midi, 65535)),
        ..Default::default()
    };
    decode_packet_377(220, &[0x7f, 0xff])
        .unwrap()
        .validate(&context)
        .unwrap();
    assert_eq!(context.reads.get(), 0);
}

#[test]
fn signed_and_mixed_endian_reader_edges_match_java_arithmetic() {
    let mut reader = PacketReader377::new(0, &[0xff, 0xff, 0, 0x80, 0x7f, 0xff, 0x80, 0xff]);
    assert_eq!(reader.g1b().unwrap(), -1);
    assert_eq!(reader.g1b_alt1().unwrap(), 127);
    assert_eq!(reader.g1b_alt2().unwrap(), 0);
    assert_eq!(reader.g1b_alt3().unwrap(), 0);
    assert_eq!(reader.g2b_alt1().unwrap(), -129);
    assert_eq!(reader.g2b_alt2().unwrap(), -32641);
    let mut reader = PacketReader377::new(0, &[0x34, 0x12, 0x78, 0x56]);
    assert_eq!(reader.g4_alt3().unwrap(), 0x12345678);
    assert!(matches!(reader.g8().unwrap_err().kind, E::Truncated { .. }));
}
