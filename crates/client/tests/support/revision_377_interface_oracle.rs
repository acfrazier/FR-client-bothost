//! Compare every defined interface against fields emitted by untouched Java.
//! The six unencoded runtime counters are Java's zero-initialized fields, not
//! interface data. Font and image objects are resolved by rendering, so Java's
//! primitive/string/array reflection intentionally does not include them.

use super::config_377::Reader377;
use super::if_type_377::{IfType377, InterfaceArchive377};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn fields(com: &IfType377) -> Value {
    Value::Object(
        [
            ("activeAnim", json!(com.active_anim)),
            ("activeColour", json!(com.active_colour)),
            ("activeModel", json!(com.active_model)),
            ("activeModelType", json!(com.active_model_type)),
            ("activeOverColour", json!(com.active_over_colour)),
            ("activeText", json!(com.active_text)),
            ("anim", json!(com.anim)),
            ("buttonType", json!(com.button_type)),
            ("center", json!(com.center)),
            ("childX", json!(com.child_x)),
            ("childY", json!(com.child_y)),
            ("children", json!(com.children)),
            ("clientCode", json!(com.client_code)),
            ("colour", json!(com.colour)),
            ("draggable", json!(com.draggable)),
            ("field700", json!(com.field_700)),
            ("field707", json!(com.field_707)),
            ("field709", json!(com.field_709)),
            ("field710", json!(com.field_710)),
            ("field713", json!(com.field_713)),
            ("field715", json!(com.field_715)),
            ("field717", json!(com.field_717)),
            ("field741", json!(com.field_741)),
            ("fill", json!(com.fill)),
            ("height", json!(com.height)),
            ("hide", json!(com.hide)),
            ("id", json!(com.id)),
            ("interactable", json!(com.interactable)),
            ("invSlotObjCount", json!(com.inv_slot_obj_count)),
            ("invSlotObjId", json!(com.inv_slot_obj_id)),
            ("invSlotOffsetX", json!(com.inv_slot_offset_x)),
            ("invSlotOffsetY", json!(com.inv_slot_offset_y)),
            ("iop", json!(com.iop)),
            ("layer", json!(com.layer)),
            ("marginX", json!(com.margin_x)),
            ("marginY", json!(com.margin_y)),
            ("model", json!(com.model)),
            ("modelType", json!(com.model_type)),
            ("option", json!(com.option)),
            ("overColour", json!(com.over_colour)),
            ("overlayer", json!(com.overlayer)),
            ("scriptComparator", json!(com.script_comparator)),
            ("scriptOperand", json!(com.script_operand)),
            ("scripts", json!(com.scripts)),
            ("scroll", json!(com.scroll)),
            ("shadowed", json!(com.shadowed)),
            ("swappable", json!(com.swappable)),
            ("targetMask", json!(com.target_mask)),
            ("targetText", json!(com.target_text)),
            ("targetVerb", json!(com.target_verb)),
            ("text", json!(com.text)),
            ("trans", json!(com.trans)),
            ("type", json!(com.r#type)),
            ("usable", json!(com.usable)),
            ("width", json!(com.width)),
            ("xan", json!(com.xan)),
            ("yan", json!(com.yan)),
            ("zoom", json!(com.zoom)),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect(),
    )
}

pub fn assert_archive(archive: &InterfaceArchive377, data: &[u8]) {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/revision_377/decoders.json")).unwrap();
    let expected = &fixture["tables"]["interface"];
    assert_eq!(
        archive.declared_total() as u64,
        expected["count"].as_u64().unwrap()
    );
    assert_eq!(
        archive.occupied_slots() as u64,
        expected["present"].as_u64().unwrap()
    );
    let layers = archive.client_code_layers();
    assert_eq!(
        json!([layers.field_728, layers.field_737, layers.field_759]),
        fixture["interface_layers"]
    );
    let rows: Vec<Value> = (0..archive.declared_total())
        .map(|id| {
            archive
                .get(id as i32)
                .unwrap()
                .map_or(Value::Null, |com| fields(&com))
        })
        .collect();
    let first = rows.iter().find_map(Value::as_object).unwrap();
    let expected_fields = expected["fields"].as_object().unwrap();
    assert_eq!(
        first.keys().collect::<Vec<_>>(),
        expected_fields.keys().collect::<Vec<_>>()
    );
    for (field, digest) in expected_fields {
        let values: Vec<&Value> = rows.iter().map(|row| &row[field]).collect();
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&values).unwrap()));
        assert_eq!(
            hash,
            digest.as_str().unwrap(),
            "interface.{field}, all populated and absent slots"
        );
    }
    for (id, sample) in expected["samples"].as_object().unwrap() {
        assert_eq!(
            &rows[id.parse::<usize>().unwrap()],
            sample,
            "interface sample {id}"
        );
    }

    let mut reader = Reader377::new(data, "interface", 0);
    assert_eq!(reader.g2().unwrap() as usize, archive.declared_total());
    let mut layer = -1;
    let mut records = Vec::new();
    while reader.pos < reader.data.len() {
        let mut id = reader.g2().unwrap();
        if id == 65535 {
            layer = reader.g2().unwrap();
            id = reader.g2().unwrap();
        }
        let start = reader.pos;
        IfType377::decode(id, layer, &mut reader).unwrap();
        let saved = archive.saved_record(id).unwrap();
        assert_eq!(saved.layer, layer);
        assert_eq!(saved.payload(), &data[start..reader.pos]);
        records.push(json!({"id": id, "layer": layer, "start": start, "end": reader.pos}));
    }
    reader.finish().unwrap();
    assert_eq!(
        records.len() as u64,
        fixture["interface_records"]["count"].as_u64().unwrap()
    );
    let hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&records).unwrap())
    );
    assert_eq!(
        hash,
        fixture["interface_records"]["sha256"].as_str().unwrap(),
        "all Java component record boundaries"
    );
    println!("377 interface: {} definitions, {} slots, {} deterministic Java fields and all record boundaries matched", archive.occupied_slots(), archive.declared_total(), expected_fields.len());
}
