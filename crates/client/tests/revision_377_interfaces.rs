//! Tests for the registered revision-377 interface decoder modules.
//! They cover component fields, sparse storage, lazy reload, and a pinned real
//! interface archive. The real cache gate follows `revision_377_stage1`.

use client::config::{config_377, if_type_377};
use client::io;
use if_type_377::{
    parse_script_377, IfType377, InterfaceArchive377, ScriptOpcode377, ScriptParseError377,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;

#[path = "support/revision_377_interface_oracle.rs"]
mod java_oracle;

use config_377::{DecodeError377, DecodeErrorKind377, Reader377};

fn put_g2(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&(value as u16).to_be_bytes());
}

fn put_g4(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn put_jstr(bytes: &mut Vec<u8>, text: &str) {
    assert!(!text.as_bytes().contains(&10));
    bytes.extend_from_slice(text.as_bytes());
    bytes.push(10);
}

fn component_header(
    component_type: i32,
    button_type: i32,
    client_code: i32,
    (width, height): (i32, i32),
    trans: i32,
    overlayer: Option<(i32, i32)>,
    scripts: &[&[i32]],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.push(component_type as u8);
    bytes.push(button_type as u8);
    put_g2(&mut bytes, client_code);
    put_g2(&mut bytes, width);
    put_g2(&mut bytes, height);
    bytes.push(trans as u8);
    match overlayer {
        None => bytes.push(0),
        Some((high, low)) => {
            bytes.push(high as u8);
            bytes.push(low as u8);
        }
    }
    bytes.push(0); // no script comparators
    bytes.push(scripts.len() as u8);
    for words in scripts {
        put_g2(&mut bytes, words.len() as i32);
        for word in *words {
            put_g2(&mut bytes, *word);
        }
    }
    bytes
}

fn simple_component(component_type: i32, button_type: i32) -> Vec<u8> {
    component_header(component_type, button_type, 0, (1, 1), 0, None, &[])
}

fn append_record(bytes: &mut Vec<u8>, id: i32, layer: Option<i32>, body: &[u8]) {
    if let Some(layer) = layer {
        put_g2(bytes, 65_535);
        put_g2(bytes, layer);
    }
    put_g2(bytes, id);
    bytes.extend_from_slice(body);
}

fn decode_body(id: i32, layer: i32, body: &[u8]) -> Result<IfType377, DecodeError377> {
    let mut reader = Reader377::new(body, "interface", id as usize);
    let component = IfType377::decode(id, layer, &mut reader)?;
    reader.finish()?;
    Ok(component)
}

fn inventory_component() -> Vec<u8> {
    let mut body = simple_component(2, 0);
    body.extend_from_slice(&[1, 0, 1, 1, 3, 4]);
    for slot in 0..20 {
        if slot == 0 {
            body.push(1);
            put_g2(&mut body, -2);
            put_g2(&mut body, -32_768);
            put_jstr(&mut body, "slot,sprite,12");
        } else {
            body.push(0);
        }
    }
    for text in ["Wear", "", "Use", "", ""] {
        put_jstr(&mut body, text);
    }
    // Component.java:528-531 requires these for type 2 even without a button.
    put_jstr(&mut body, "Use");
    put_jstr(&mut body, "inventory");
    put_g2(&mut body, 0x1F);
    body
}

#[test]
fn type8_preserves_text_and_every_script_word() {
    let words = [20, 0xABCD, 0, 0x9876];
    let mut body = component_header(8, 0, 0, (80, 20), 0xFE, None, &[&words]);
    put_jstr(&mut body, "tooltip text");

    let component = decode_body(31, -1, &body).unwrap();
    assert_eq!(component.r#type, 8);
    assert_eq!(component.text.as_deref(), Some("tooltip text"));
    assert_eq!(component.trans, -2);
    assert_eq!(component.scripts, Some(vec![words.into_iter().collect()]));
    let instructions = parse_script_377(component.scripts.as_ref().unwrap()[0].as_slice()).unwrap();
    assert_eq!(instructions.len(), 2);
    assert_eq!(instructions[0].word_index, 0);
    assert_eq!(instructions[1].word_index, 2);
    assert_eq!(instructions[0].opcode, ScriptOpcode377::Literal);
    assert_eq!(instructions[0].operands, 1..2);
    assert_eq!(instructions[1].opcode, ScriptOpcode377::Terminate);
    // Java preserves the post-terminator word in the decoded script array.
    assert_eq!(component.scripts.as_ref().unwrap()[0][3], 0x9876);
}

#[test]
fn type1_fields_signed_values_and_sentinel_conversions_match_java() {
    let mut body = component_header(1, 2, 600, (30, 14), 0xFE, Some((2, 7)), &[]);
    put_g2(&mut body, 0x1234); // field707
    body.push(1); // field715
    body.push(1); // center
    body.push(17); // font-array index
    body.push(1); // shadowed
    put_g4(&mut body, 0x89AB_CDEF_u32 as i32);
    put_jstr(&mut body, "Cast");
    put_jstr(&mut body, "spell target");
    put_g2(&mut body, 0x8FFF);

    let component = decode_body(17, 0x3456, &body).unwrap();
    assert_eq!(component.id, 17);
    assert_eq!(component.layer, 0x3456);
    assert_eq!(component.field_707, 0x1234);
    assert!(component.field_715);
    assert!(component.center);
    assert_eq!(component.font_index, 17);
    assert!(component.shadowed);
    assert_eq!(component.colour, 0x89AB_CDEF_u32 as i32);
    assert_eq!(component.trans, -2);
    assert_eq!(component.overlayer, 263);
    assert_eq!(component.target_verb.as_deref(), Some("Cast"));
    assert_eq!(component.target_text.as_deref(), Some("spell target"));
    assert_eq!(component.target_mask, 0x8FFF);
    assert_eq!(component.option, None);
    assert_eq!(component.active_text, None);
}

#[test]
fn all_component_types_zero_through_eight_consume_their_java_fields() {
    for component_type in 0..=8 {
        let mut body = simple_component(component_type, 0);
        match component_type {
            0 => {
                put_g2(&mut body, 100);
                body.push(1);
                put_g2(&mut body, 1);
                put_g2(&mut body, 12);
                put_g2(&mut body, -1);
                put_g2(&mut body, -32_768);
            }
            1 => {
                put_g2(&mut body, 0x77);
                body.push(1);
                body.push(1);
                body.push(5);
                body.push(1);
                put_g4(&mut body, 0x1234_5678);
            }
            2 => body.extend_from_slice(&inventory_component()[12..]),
            3 => {
                body.push(1);
                put_g4(&mut body, 1);
                put_g4(&mut body, 2);
                put_g4(&mut body, 3);
                put_g4(&mut body, 4);
            }
            4 => {
                body.push(1);
                body.push(6);
                body.push(1);
                put_jstr(&mut body, "text");
                put_jstr(&mut body, "active");
                put_g4(&mut body, 5);
                put_g4(&mut body, 6);
                put_g4(&mut body, 7);
                put_g4(&mut body, 8);
            }
            5 => {
                put_jstr(&mut body, "sprite,4");
                put_jstr(&mut body, "active,9");
            }
            6 => {
                body.extend_from_slice(&[2, 3, 4, 5, 0, 6, 1]);
                put_g2(&mut body, 900);
                put_g2(&mut body, 100);
                put_g2(&mut body, 200);
            }
            7 => {
                body.push(1);
                body.push(8);
                body.push(1);
                put_g4(&mut body, 0x1020_3040);
                put_g2(&mut body, -3);
                put_g2(&mut body, 4);
                body.push(1);
                for action in ["One", "", "", "", ""] {
                    put_jstr(&mut body, action);
                }
            }
            8 => put_jstr(&mut body, "type eight"),
            _ => unreachable!(),
        }
        let component = decode_body(component_type, -1, &body).unwrap();
        assert_eq!(component.r#type, component_type);
        match component_type {
            0 => {
                assert_eq!(component.scroll, 100);
                assert!(component.hide);
                assert_eq!(component.children.as_deref(), Some(&[12][..]));
                assert_eq!(component.child_x.as_deref(), Some(&[-1][..]));
                assert_eq!(component.child_y.as_deref(), Some(&[-32_768][..]));
            }
            1 => {
                assert_eq!(component.field_707, 0x77);
                assert!(component.field_715);
                assert_eq!(component.font_index, 5);
                assert_eq!(component.colour, 0x1234_5678);
            }
            3 => {
                assert!(component.fill);
                assert_eq!(component.colour, 1);
                assert_eq!(component.active_colour, 2);
                assert_eq!(component.over_colour, 3);
                assert_eq!(component.active_over_colour, 4);
            }
            4 => {
                assert_eq!(component.text.as_deref(), Some("text"));
                assert_eq!(component.active_text.as_deref(), Some("active"));
                assert_eq!(component.font_index, 6);
                assert_eq!(component.active_over_colour, 8);
            }
            5 => {
                assert_eq!(component.graphic.as_ref().unwrap().name, "sprite");
                assert_eq!(component.graphic.as_ref().unwrap().index, 4);
                assert_eq!(component.active_graphic.as_ref().unwrap().index, 9);
            }
            6 => {
                assert_eq!(component.model_type, 1);
                assert_eq!(component.model, 259);
                assert_eq!(component.active_model_type, 1);
                assert_eq!(component.active_model, 773);
                assert_eq!(component.anim, -1);
                assert_eq!(component.active_anim, 1281);
                assert_eq!(component.zoom, 900);
                assert_eq!(component.xan, 100);
                assert_eq!(component.yan, 200);
            }
            7 => {
                assert_eq!(component.margin_x, -3);
                assert_eq!(component.margin_y, 4);
                assert!(component.interactable);
                assert_eq!(component.iop.as_ref().unwrap()[0].as_deref(), Some("One"));
            }
            8 => assert_eq!(component.text.as_deref(), Some("type eight")),
            _ => {}
        }
    }
}

#[test]
fn type2_arrays_sprite_names_iop_and_button_defaults_are_preserved() {
    let mut body = component_header(2, 2, 0, (1, 2), 0, None, &[]);
    body.extend_from_slice(&[1, 0, 1, 1, 3, 4]);
    for slot in 0..20 {
        if slot == 0 {
            body.push(1);
            put_g2(&mut body, -2);
            put_g2(&mut body, -32_768);
            put_jstr(&mut body, "slot,sprite,12");
        } else {
            body.push(0);
        }
    }
    for text in ["Wear", "", "Use", "", ""] {
        put_jstr(&mut body, text);
    }
    put_jstr(&mut body, "Cast");
    put_jstr(&mut body, "target");
    put_g2(&mut body, 0x1234);

    let component = decode_body(8, 0x4444, &body).unwrap();
    assert_eq!(component.inv_slot_obj_id, Some(vec![0, 0]));
    assert_eq!(component.inv_slot_obj_count, Some(vec![0, 0]));
    assert!(component.draggable);
    assert!(!component.interactable);
    assert!(component.usable);
    assert!(component.swappable);
    assert_eq!(component.margin_x, 3);
    assert_eq!(component.margin_y, 4);
    assert_eq!(component.inv_slot_offset_x.as_ref().unwrap()[0], -2);
    assert_eq!(component.inv_slot_offset_y.as_ref().unwrap()[0], -32_768);
    let sprite = component.inv_slot_graphic.as_ref().unwrap()[0]
        .as_ref()
        .unwrap();
    assert_eq!(sprite.name, "slot,sprite");
    assert_eq!(sprite.index, 12);
    assert_eq!(component.iop.as_ref().unwrap()[0].as_deref(), Some("Wear"));
    assert_eq!(component.iop.as_ref().unwrap()[1], None);
    assert_eq!(component.target_verb.as_deref(), Some("Cast"));
    assert_eq!(component.target_text.as_deref(), Some("target"));
    assert_eq!(component.target_mask, 0x1234);
    assert_eq!(component.option, None);

    for (button_type, expected) in [(1, "Ok"), (4, "Select"), (5, "Select"), (6, "Continue")] {
        let mut button = simple_component(9, button_type);
        put_jstr(&mut button, "");
        let component = decode_body(0, -1, &button).unwrap();
        assert_eq!(component.option.as_deref(), Some(expected));
    }
    let button = decode_body(0, -1, &simple_component(9, 0)).unwrap();
    assert_eq!(button.option, None);
    assert_eq!(button.target_mask, 0);
}

#[test]
fn sparse_archive_lazy_get_unload_and_saved_layer_bytes_match_java() {
    let mut data = Vec::new();
    put_g2(&mut data, 14); // sparse declared total

    let inv_body = inventory_component();
    append_record(&mut data, 2, None, &inv_body); // Java layer starts at -1

    let mut layer_component = component_header(0, 0, 600, (1, 1), 0, None, &[]);
    put_g2(&mut layer_component, 0);
    layer_component.push(0);
    put_g2(&mut layer_component, 0);
    append_record(&mut data, 3, None, &layer_component);

    let layer = 0x1234;
    append_record(&mut data, 4, Some(layer), &layer_component);
    append_record(&mut data, 5, None, &simple_component(9, 0));

    let inv_body = inventory_component();
    append_record(&mut data, 7, None, &inv_body);

    let mut code_component = component_header(8, 0, 650, (10, 10), 0, None, &[]);
    put_jstr(&mut code_component, "later layer");
    append_record(&mut data, 10, None, &code_component);

    let layer = 0x2345;
    let older_code = component_header(9, 0, 600, (1, 1), 0, None, &[]);
    append_record(&mut data, 11, Some(layer), &older_code);
    let later_code = component_header(9, 0, 650, (1, 1), 0, None, &[]);
    append_record(&mut data, 12, None, &later_code);
    let final_code = component_header(9, 0, 655, (1, 1), 0, None, &[]);
    append_record(&mut data, 13, None, &final_code);

    let archive = InterfaceArchive377::scan(&data).unwrap();
    assert_eq!(archive.declared_total(), 14);
    assert_eq!(archive.occupied_slots(), 9);
    assert_eq!(archive.stats().record_count, 9);
    assert_eq!(archive.stats().input_bytes, data.len());
    assert_eq!(archive.stats().component_type_counts.get(&2), Some(&2));
    assert_eq!(archive.stats().component_type_counts.get(&8), Some(&1));
    let layers = archive.client_code_layers();
    assert_eq!(layers.field_728, 0x2345);
    assert_eq!(layers.field_737, 0x2345);
    assert_eq!(layers.field_759, 0x2345);

    let saved_before = archive.saved_record(7).unwrap().as_java_bytes();
    assert_eq!(&saved_before[..2], &[0x12, 0x34]);
    assert_eq!(&saved_before[2..], inventory_component().as_slice());
    assert_eq!(archive.saved_record(2).unwrap().layer, -1);
    assert_eq!(
        &archive.saved_record(2).unwrap().as_java_bytes()[..2],
        &[0xFF, 0xFF]
    );
    let saved_unprefixed = archive.saved_record(3).unwrap();
    assert_eq!(saved_unprefixed.layer, -1);
    assert_eq!(&saved_unprefixed.as_java_bytes()[..2], &[0xFF, 0xFF]);

    let first_unprefixed = archive.get(3).unwrap().unwrap();
    assert_eq!(first_unprefixed.layer, 65_535);
    assert_eq!(archive.client_code_layers().field_728, 65_535);
    let prefix_inventory = archive.get(2).unwrap().unwrap();
    assert_eq!(prefix_inventory.layer, 65_535);
    assert_eq!(archive.unload_layer(-1), 0);
    assert!(Arc::ptr_eq(
        &first_unprefixed,
        &archive.get(3).unwrap().unwrap()
    ));
    assert_eq!(archive.unload_layer(65_535), 1);
    let unprefixed_after = archive.get(3).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&first_unprefixed, &unprefixed_after));
    assert_eq!(unprefixed_after.layer, 65_535);
    assert!(Arc::ptr_eq(
        &prefix_inventory,
        &archive.get(2).unwrap().unwrap()
    ));

    let older_layer = archive.get(4).unwrap().unwrap();
    assert_eq!(older_layer.layer, 0x1234);
    assert_eq!(archive.client_code_layers().field_728, 0x1234);
    let persistent = archive.get(7).unwrap().unwrap();
    let disposable = archive.get(10).unwrap().unwrap();
    assert_eq!(disposable.text.as_deref(), Some("later layer"));
    assert_eq!(archive.unload_layer(0x1234), 2);
    let persistent_after = archive.get(7).unwrap().unwrap();
    assert!(Arc::ptr_eq(&persistent, &persistent_after));
    assert_eq!(
        archive.saved_record(7).unwrap().as_java_bytes(),
        saved_before
    );

    let reloaded = archive.get(4).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&older_layer, &reloaded));
    assert_eq!(reloaded.layer, 0x1234);
    let reloaded_later = archive.get(10).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&disposable, &reloaded_later));
    assert_eq!(reloaded_later.text.as_deref(), Some("later layer"));
    assert_eq!(archive.get(5).unwrap().unwrap().layer, 0x1234);
    let layers = archive.client_code_layers();
    assert_eq!(layers.field_728, 0x1234);
    assert_eq!(layers.field_737, 0x1234);
    assert_eq!(layers.field_759, 0x2345);
}

#[test]
fn scan_reports_truncation_and_decoder_finish_reports_exact_trailing_bytes() {
    let mut valid = simple_component(8, 0);
    put_jstr(&mut valid, "ok");
    let mut with_trailing = valid.clone();
    with_trailing.push(0xAA);
    let mut reader = Reader377::new(&with_trailing, "interface", 3);
    IfType377::decode(3, -1, &mut reader).unwrap();
    let trailing = reader.finish().unwrap_err();
    assert_eq!(trailing.offset, valid.len());
    assert_eq!(trailing.kind, DecodeErrorKind377::TrailingBytes(1));

    let mut truncated = simple_component(8, 0);
    truncated.extend_from_slice(b"unterminated");
    let error = decode_body(3, -1, &truncated).unwrap_err();
    assert_eq!(error.kind, DecodeErrorKind377::UnexpectedEof);

    let mut member = Vec::new();
    put_g2(&mut member, 4);
    append_record(&mut member, 2, None, &valid);
    member.push(0x7F); // a partial next component ID cannot be ignored
    let error = InterfaceArchive377::scan(&member).err().unwrap();
    assert_eq!(error.kind, DecodeErrorKind377::UnexpectedEof);
}

#[test]
fn revision377_script_opcodes_are_explicit_and_unknown_words_do_not_fallback() {
    for id in 0..=20 {
        assert!(ScriptOpcode377::from_id(id).is_some(), "opcode {id}");
    }
    assert_eq!(ScriptOpcode377::from_id(21), None);
    assert_eq!(
        parse_script_377(&[21]),
        Err(ScriptParseError377::UnknownOpcode {
            word_index: 0,
            opcode: 21,
        })
    );
    assert_eq!(
        parse_script_377(&[4, 100]),
        Err(ScriptParseError377::TruncatedOperands {
            word_index: 0,
            opcode: ScriptOpcode377::ComponentInventoryCount,
            required: 2,
            available: 1,
        })
    );
}

#[test]
fn real_377_interface_archive_matches_pinned_bytes_and_scans_every_record() {
    let Some(dir) = real_cache_dir() else { return };
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/revision_377/cache.json")).unwrap();
    let row = fixture["archives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "interface")
        .unwrap();
    let packed = std::fs::read(dir.join("interface")).unwrap();
    assert_eq!(packed.len() as u64, row["length"].as_u64().unwrap());
    assert_eq!(
        format!("{:x}", Sha256::digest(&packed)),
        row["sha256"].as_str().unwrap()
    );

    let jag = io::jagfile::JagFile::new(packed);
    let member: Arc<[u8]> = Arc::from(jag.read("data").expect("interface JAG must contain data"));
    let member_oracle = &row["members"][0];
    assert_eq!(
        member.len() as u64,
        member_oracle["length"].as_u64().unwrap()
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(member.as_ref())),
        member_oracle["sha256"].as_str().unwrap()
    );

    let archive = InterfaceArchive377::scan_shared(Arc::clone(&member)).unwrap();
    java_oracle::assert_archive(&archive, member.as_ref());
    let stats = archive.stats();
    assert_eq!(stats.input_bytes, member.len());
    assert_eq!(stats.declared_total, 19_269);
    assert_eq!(stats.record_count, 19_131);
    assert!(
        stats.unknown_script_opcode_counts.is_empty(),
        "unknown revision-377 script opcodes: {:?}",
        stats.unknown_script_opcode_counts
    );
    assert_eq!(
        stats.malformed_script_count, 0,
        "malformed revision-377 scripts"
    );
    assert_eq!(stats.script_count, 3_153);
    assert_eq!(stats.script_word_count, 18_198);
    assert!(stats.record_count > 0);
    assert!(stats.occupied_slots > 0);
    assert!(stats.occupied_slots <= stats.declared_total);
    let mut loaded = 0;
    for id in 0..archive.declared_total() {
        if archive.saved_record(id as i32).is_some() {
            let component = archive
                .get(id as i32)
                .unwrap()
                .expect("every saved interface slot must lazily decode");
            assert_eq!(component.id, id as i32);
            let saved = archive.saved_record(id as i32).unwrap();
            let mut reader = Reader377::new(saved.payload(), "interface", id);
            let reloaded =
                IfType377::decode(id as i32, saved.layer as u16 as i32, &mut reader).unwrap();
            reader.finish().unwrap();
            assert_eq!(*component, reloaded);
            loaded += 1;
        }
    }
    assert_eq!(loaded, stats.occupied_slots);

    println!(
        "377 interface slots: declared={} occupied={} holes={} records={} input_bytes={} saved_record_bytes={}",
        stats.declared_total,
        stats.occupied_slots,
        stats.declared_total - stats.occupied_slots,
        stats.record_count,
        stats.input_bytes,
        stats.saved_record_bytes,
    );
    for (component_type, count) in &stats.component_type_counts {
        println!("377 interface type {component_type}: {count} records");
    }
    println!(
        "377 interface scripts: arrays={} words={} known_opcodes={} unknown_opcodes={} malformed={}",
        stats.script_count,
        stats.script_word_count,
        stats.script_opcode_counts.len(),
        stats.unknown_script_opcode_counts.len(),
        stats.malformed_script_count,
    );
    for (opcode, count) in &stats.script_opcode_counts {
        println!("377 interface script opcode {opcode}: {count}");
    }
    for (opcode, count) in &stats.unknown_script_opcode_counts {
        println!("377 interface unknown script opcode {opcode}: {count}");
    }
}

fn real_cache_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLIENT_377_CACHE_DIR") {
        return Some(PathBuf::from(path));
    }
    if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        eprintln!("SKIP real 377 interface archive: CLIENT_377_CACHE_DIR is not set on hosted CI");
        return None;
    }
    panic!("CLIENT_377_CACHE_DIR must name the pinned real revision-377 cache fixture");
}
