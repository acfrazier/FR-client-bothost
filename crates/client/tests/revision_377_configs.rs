//! Executes the actual additive 377 modules without changing shared mod lists.
//! The frozen oracle comes from compiling and calling untouched Client-Java 377.

use client::{io, util};
use io::JagFile;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;

#[path = "../src/config/config_377.rs"]
mod config_377;
use config_377::*;
#[path = "support/revision_377_oracle.rs"]
mod oracle;

fn read_record<T>(
    bytes: &[u8],
    decode: impl FnOnce(&mut Reader377<'_>) -> Result<T, DecodeError377>,
) -> T {
    let mut dat = Reader377::new(bytes, "synthetic", 7);
    let result = decode(&mut dat).unwrap();
    dat.finish().unwrap();
    result
}

#[test]
fn sequence_unsigned_words_sentinels_merge_defaults_and_opcode12() {
    let defaults = read_record(&[0], SeqType377::decode);
    assert_eq!(defaults.frames, [-1]);
    assert_eq!(defaults.iframes, [-1]);
    assert_eq!(defaults.delay, [-1]);
    assert_eq!(
        (
            defaults.preanim_move,
            defaults.postanim_move,
            defaults.duplicatebehaviour
        ),
        (0, 0, 2)
    );
    let seq = read_record(
        &[
            1, 1, 255, 255, 255, 255, 128, 0, 2, 0, 5, 3, 0, 4, 5, 8, 6, 255, 255, 7, 128, 0, 8, 9,
            9, 1, 10, 2, 11, 0, 12, 128, 0, 0, 1, 0,
        ],
        SeqType377::decode,
    );
    assert_eq!(seq.frames, [65535]);
    assert_eq!(seq.iframes, [-1]);
    assert_eq!(seq.delay, [32768]);
    assert_eq!(seq.walkmerge.as_deref(), Some([9_999_999].as_slice()));
    assert_eq!(
        (
            seq.loops,
            seq.priority,
            seq.replaceheldleft,
            seq.replaceheldright,
            seq.maxloops
        ),
        (5, 8, 65535, 32768, 9)
    );
    assert_eq!(
        (
            seq.preanim_move,
            seq.postanim_move,
            seq.duplicatebehaviour,
            seq.reserved
        ),
        (1, 2, 0, i32::MIN + 1)
    );
    assert!(seq.reachforward);
    let merge = read_record(&[3, 1, 42, 0], SeqType377::decode);
    assert_eq!((merge.preanim_move, merge.postanim_move), (2, 2));
}

#[test]
fn identity_and_spot_arrays_have_java_sizes_and_unsigned_values() {
    let kit = read_record(
        &[
            1, 255, 2, 1, 255, 255, 3, 40, 128, 0, 55, 255, 255, 64, 128, 1, 0,
        ],
        IdkType377::decode,
    );
    assert_eq!(kit.r#type, 255);
    assert_eq!(kit.models.as_deref(), Some([65535].as_slice()));
    assert!(kit.disable);
    assert_eq!(kit.recol_s[0], 32768);
    assert_eq!(kit.recol_d[5], 65535);
    assert_eq!(kit.heads, [-1, -1, -1, -1, 32769]);
    let spot = read_record(
        &[
            1, 255, 255, 2, 0, 0, 4, 0, 90, 5, 0, 91, 6, 1, 14, 7, 255, 8, 128, 45, 128, 0, 50,
            255, 255, 0,
        ],
        SpotType377::decode,
    );
    assert_eq!((spot.index, spot.model, spot.anim), (7, 65535, 0));
    assert_eq!(
        (
            spot.resizeh,
            spot.resizev,
            spot.orientation,
            spot.ambient,
            spot.contrast
        ),
        (90, 91, 270, 255, 128)
    );
    assert_eq!(spot.recol_s[5], 32768);
    assert_eq!(spot.recol_d[0], 65535);
    assert!(matches!(
        IdkType377::decode(&mut Reader377::new(&[46, 0, 0, 0], "idk.dat", 0))
            .unwrap_err()
            .kind,
        DecodeErrorKind377::InvalidValue("idk recolour slot")
    ));
    assert!(matches!(
        SpotType377::decode(&mut Reader377::new(&[56, 0, 0, 0], "spotanim.dat", 0))
            .unwrap_err()
            .kind,
        DecodeErrorKind377::InvalidValue("spotanim recolour slot")
    ));
}

#[test]
fn varp_and_varbit_preserve_all_fields_and_repeated_tracking() {
    let mut tracked = Vec::new();
    let varp = read_record(
        &[
            1, 255, 2, 128, 3, 3, 4, 5, 255, 255, 6, 7, 128, 0, 0, 0, 8, 10, b'x', 10, 11, 12, 255,
            255, 255, 254, 13, 14, 0,
        ],
        |dat| VarpType377::decode(dat, &mut tracked),
    );
    assert_eq!(tracked, [7, 7]);
    assert_eq!(
        (
            varp.code1,
            varp.code2,
            varp.clientcode,
            varp.code7,
            varp.code12,
            varp.scope
        ),
        (255, 128, 65535, i32::MIN, -2, 2)
    );
    assert!(varp.tracked && varp.code6 && varp.transmit);
    assert!(!varp.code4 && !varp.code14);
    assert_eq!(varp.debugname.as_deref(), Some("x"));
    let bit = read_record(
        &[
            1, 255, 255, 128, 255, 2, 3, 128, 0, 0, 0, 4, 255, 255, 255, 255, 5, 10, b'y', 10, 0,
        ],
        VarbitType377::decode,
    );
    assert_eq!(
        (bit.basevar, bit.startbit, bit.endbit, bit.code3, bit.code4),
        (65535, 128, 255, i32::MIN, -1)
    );
    assert!(bit.transmit && !bit.code5);
    assert_eq!(bit.debugname.as_deref(), Some("y"));
}

#[test]
fn floor_magenta_and_opcode7_keep_the_java_restore_behavior() {
    let magenta = read_record(&[1, 255, 0, 255, 0], |dat| {
        FloType377::decode(dat, &mut || 0.5)
    });
    assert_eq!(
        (
            magenta.colour,
            magenta.hue,
            magenta.saturation,
            magenta.lightness,
            magenta.chroma
        ),
        (0xff00ff, 0, 0, 0, 1)
    );
    let mut draws = 0;
    let flo = read_record(
        &[1, 255, 0, 0, 2, 255, 3, 5, 6, b'z', 10, 7, 0, 0, 255, 0],
        |dat| {
            FloType377::decode(dat, &mut || {
                draws += 1;
                0.5
            })
        },
    );
    assert_eq!(draws, 6);
    assert_eq!(
        (
            flo.hue,
            flo.saturation,
            flo.lightness,
            flo.underlay_hue,
            flo.chroma
        ),
        (0, 255, 127, 0, 0)
    );
    assert_eq!(flo.overlay_hsl, 43967);
    assert_eq!(flo.texture, 255);
    assert!(flo.overlay && !flo.occlude);
    assert_eq!(flo.debugname.as_deref(), Some("z"));
}

fn jag(members: &[(String, Vec<u8>)]) -> JagFile {
    let mut payload = Vec::new();
    payload.extend_from_slice(&(members.len() as u16).to_be_bytes());
    for (name, data) in members {
        payload.extend_from_slice(&JagFile::gen_hash(name).to_be_bytes());
        for _ in 0..2 {
            payload.extend_from_slice(&(data.len() as u32).to_be_bytes()[1..]);
        }
    }
    for (_, bytes) in members {
        payload.extend_from_slice(bytes);
    }
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
    encoder.write_all(&payload).unwrap();
    let compressed = encoder.finish().unwrap();
    let compressed = &compressed[4..];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
    bytes.extend_from_slice(&(compressed.len() as u32).to_be_bytes()[1..]);
    bytes.extend_from_slice(compressed);
    JagFile::new(bytes)
}

fn minimal_members() -> Vec<(String, Vec<u8>)> {
    [
        "seq.dat",
        "loc.dat",
        "loc.idx",
        "flo.dat",
        "obj.dat",
        "obj.idx",
        "npc.dat",
        "npc.idx",
        "idk.dat",
        "spotanim.dat",
        "varp.dat",
        "varbit.dat",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), vec![0, 0]))
    .collect()
}

fn replace(members: &mut [(String, Vec<u8>)], name: &str, bytes: &[u8]) {
    members
        .iter_mut()
        .find(|(member, _)| member == name)
        .unwrap()
        .1 = bytes.to_vec();
}

#[test]
fn archive_driver_links_sequences_and_propagates_varbit_transmit() {
    let mut members = minimal_members();
    replace(&mut members, "seq.dat", &[0, 1, 0]);
    replace(&mut members, "spotanim.dat", &[0, 1, 2, 0, 0, 0]);
    replace(&mut members, "varp.dat", &[0, 1, 0]);
    replace(&mut members, "varbit.dat", &[0, 1, 1, 0, 0, 1, 3, 2, 0]);
    let cache = Cache377::unpack(&jag(&members)).unwrap();
    assert_eq!(cache.spots[0].seq, Some(0));
    assert!(cache.varps[0].transmit);
    assert_eq!(cache.coverage.len(), 12);
    replace(&mut members, "spotanim.dat", &[0, 1, 2, 0, 1, 0]);
    assert!(matches!(
        Cache377::unpack(&jag(&members)).unwrap_err().kind,
        DecodeErrorKind377::InvalidValue("sequence reference")
    ));
    replace(&mut members, "spotanim.dat", &[0, 0]);
    replace(&mut members, "varbit.dat", &[0, 1, 1, 0, 1, 1, 3, 2, 0]);
    assert!(matches!(
        Cache377::unpack(&jag(&members)).unwrap_err().kind,
        DecodeErrorKind377::InvalidValue("varp reference")
    ));
}

#[test]
fn malformed_records_cannot_hide_unknown_opcodes_or_trailing_bytes() {
    let mut members = minimal_members();
    replace(&mut members, "loc.idx", &[0, 1, 0, 2]);
    replace(&mut members, "loc.dat", &[0, 1, 0, 0]);
    let error = Cache377::unpack(&jag(&members)).unwrap_err();
    assert_eq!((error.table, error.record, error.offset), ("loc.dat", 0, 3));
    assert_eq!(error.kind, DecodeErrorKind377::TrailingBytes(1));
    replace(&mut members, "loc.idx", &[0, 1, 0, 1]);
    replace(&mut members, "loc.dat", &[0, 1, 255]);
    assert_eq!(
        Cache377::unpack(&jag(&members)).unwrap_err().kind,
        DecodeErrorKind377::UnknownOpcode(255)
    );
    replace(&mut members, "loc.dat", &[0, 1, 1]);
    assert_eq!(
        Cache377::unpack(&jag(&members)).unwrap_err().kind,
        DecodeErrorKind377::UnexpectedEof
    );
    assert_eq!(
        Cache377::unpack(&jag(&[])).unwrap_err().kind,
        DecodeErrorKind377::MissingMember
    );
    let mut reader = Reader377::new(b"x", "text", 2);
    assert_eq!(
        reader.gjstr().unwrap_err().kind,
        DecodeErrorKind377::UnexpectedEof
    );
}

fn real_cache_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLIENT_377_CACHE_DIR") {
        return Some(PathBuf::from(path));
    }
    if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        eprintln!("SKIP real 377 decoder cache: CLIENT_377_CACHE_DIR is not set on hosted CI");
        return None;
    }
    panic!("CLIENT_377_CACHE_DIR must name the real eight-JAG revision-377 cache fixture");
}

#[test]
fn indexed_defaults_morphs_signed_fields_and_lowmem_model_override() {
    let loc = read_record(
        &[
            1, 1, 255, 255, 10, 5, 1, 0, 2, 19, 2, 29, 255, 39, 128, 70, 255, 255, 71, 128, 0, 72,
            127, 255, 74, 77, 255, 255, 0, 3, 1, 255, 255, 0, 4, 0,
        ],
        LocType377::decode,
    );
    assert_eq!(loc.model.as_deref(), Some([65535].as_slice()));
    assert_eq!(loc.shape.as_deref(), Some([10].as_slice()));
    assert!(!loc.active && !loc.blockwalk && !loc.blockrange);
    assert_eq!(
        (
            loc.ambient,
            loc.contrast,
            loc.offsetx,
            loc.offsety,
            loc.offsetz
        ),
        (-1, -128, -1, -32768, 32767)
    );
    assert_eq!(
        (loc.morph_varbit, loc.morph_varp, loc.raiseobject),
        (-1, 3, 0)
    );
    assert_eq!(loc.morphs.as_deref(), Some([-1, 4].as_slice()));
    let low = read_record(&[1, 1, 0, 1, 2, 5, 1, 0, 2, 0], |dat| {
        LocType377::decode_with_lowmem(dat, true)
    });
    assert_eq!(low.model.as_deref(), Some([2].as_slice()));
    assert_eq!(low.shape, None);
    assert!(low.active);
    let hidden = read_record(
        &[30, b'h', b'i', b'd', b'd', b'e', b'n', 10, 0],
        LocType377::decode,
    );
    assert!(hidden.active);
    assert_eq!(hidden.op, Some(Default::default()));
    // LocType.java:444-448 sets true only; a later opcode 19 cannot clear it.
    let repeated_active = read_record(&[19, 1, 19, 0, 0], LocType377::decode);
    assert!(repeated_active.active);
    let npc = read_record(
        &[
            12, 255, 13, 255, 255, 100, 128, 101, 255, 106, 255, 255, 0, 3, 0, 255, 255, 107, 0,
        ],
        NpcType377::decode,
    );
    assert_eq!(
        (npc.size, npc.readyanim, npc.ambient, npc.contrast),
        (-1, 65535, -128, -5)
    );
    assert_eq!((npc.morph_varbit, npc.morph_varp), (-1, 3));
    assert_eq!(npc.morphs.as_deref(), Some([-1].as_slice()));
    assert!(!npc.field_1434);
}

#[test]
fn object_certificate_article_and_nonmember_filter_preserve_cost() {
    let template = read_record(
        &[1, 0, 42, 4, 0, 99, 40, 1, 255, 255, 128, 0, 0],
        ObjType377::decode,
    );
    let link = read_record(
        &[
            2, b'A', b'p', b'p', b'l', b'e', 10, 12, 0, 0, 0, 17, 16, 115, 9, 0,
        ],
        ObjType377::decode,
    );
    let mut note = read_record(&[97, 0, 1, 98, 0, 2, 0], ObjType377::decode);
    note.apply_certificate(&template, &link).unwrap();
    assert_eq!((note.model, note.zoom2d, note.cost), (42, 99, 17));
    assert_eq!(note.name.as_deref(), Some("Apple"));
    assert_eq!(
        note.desc.as_deref(),
        Some(b"Swap this note at any bank for an Apple.".as_slice())
    );
    assert_eq!(note.recol_s, template.recol_s);
    assert!(note.members && note.stackable);
    note.sanitize_membership(false);
    assert_eq!(note.name.as_deref(), Some("Members Object"));
    assert_eq!((note.cost, note.team), (17, 0));
    let mut ordinary = read_record(
        &[2, b'a', b'p', b'p', b'l', b'e', 10, 0],
        ObjType377::decode,
    );
    let mut second = ObjType377::default();
    second.apply_certificate(&template, &ordinary).unwrap();
    assert_eq!(
        second.desc.as_deref(),
        Some(b"Swap this note at any bank for a apple.".as_slice())
    );
    ordinary.name = None;
    assert!(second.apply_certificate(&template, &ordinary).is_err());
}

fn assert_java_table(name: &str, rows: Vec<Value>, fixture: &Value) {
    let expected = &fixture["tables"][name];
    assert_eq!(
        rows.len() as u64,
        expected["count"].as_u64().unwrap(),
        "{name} count"
    );
    let first = rows.iter().find_map(Value::as_object).unwrap();
    let fields = expected["fields"].as_object().unwrap();
    assert_eq!(
        first.keys().collect::<Vec<_>>(),
        fields.keys().collect::<Vec<_>>(),
        "{name} Java field coverage"
    );
    for (field, digest) in fields {
        let values: Vec<&Value> = rows.iter().map(|row| &row[field]).collect();
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&values).unwrap()));
        assert_eq!(
            hash,
            digest.as_str().unwrap(),
            "{name}.{field}, all {} entries",
            rows.len()
        );
    }
    for (id, sample) in expected["samples"].as_object().unwrap() {
        assert_eq!(
            &rows[id.parse::<usize>().unwrap()],
            sample,
            "{name} sample {id}"
        );
    }
    println!(
        "377 {name}: {} entries, {} deterministic Java fields matched",
        rows.len(),
        fields.len()
    );
}

fn object_for_world(data: &[u8], index: &[u8], id: usize, members: bool) -> ObjType377 {
    let start = 2 + index[2..2 + id * 2]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|entry| u16::from_be_bytes(*entry) as usize)
        .sum::<usize>();
    let length = u16::from_be_bytes([index[2 + id * 2], index[3 + id * 2]]) as usize;
    let mut record = Reader377::new(&data[start..start + length], "obj.dat", id);
    let mut object = ObjType377::decode(&mut record).unwrap();
    record.finish().unwrap();
    if object.certtemplate != -1 {
        let template = object_for_world(data, index, object.certtemplate as usize, members);
        let link = object_for_world(data, index, object.certlink as usize, members);
        object.apply_certificate(&template, &link).unwrap();
    }
    object.sanitize_membership(members);
    object
}

#[test]
fn every_real_config_record_matches_the_original_java_decoder() {
    let Some(dir) = real_cache_dir() else { return };
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/revision_377/decoders.json")).unwrap();
    let packed = std::fs::read(dir.join("config")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&packed)),
        fixture["cache_inputs"]["config"]["sha256"]
            .as_str()
            .unwrap()
    );
    let jag = JagFile::new(packed);
    let cache = Cache377::unpack_with_random(&jag, &mut || 0.5).unwrap();
    for coverage in &cache.coverage {
        assert_eq!(coverage.bytes, jag.read(coverage.member).unwrap().len());
        println!(
            "377 {}: {} records, {} bytes consumed, zero trailing bytes",
            coverage.member, coverage.records, coverage.bytes
        );
    }
    assert_java_table(
        "seq",
        cache.seqs.iter().map(oracle::seq).collect(),
        &fixture,
    );
    assert_java_table(
        "loc",
        cache.locs.iter().map(oracle::loc).collect(),
        &fixture,
    );
    assert_java_table(
        "flo",
        cache.flos.iter().map(oracle::flo).collect(),
        &fixture,
    );
    assert_java_table(
        "obj",
        cache.objs.iter().map(oracle::obj).collect(),
        &fixture,
    );
    assert_java_table(
        "npc",
        cache.npcs.iter().map(oracle::npc).collect(),
        &fixture,
    );
    assert_java_table(
        "idk",
        cache.idks.iter().map(oracle::idk).collect(),
        &fixture,
    );
    assert_java_table(
        "spotanim",
        cache.spots.iter().map(oracle::spot).collect(),
        &fixture,
    );
    assert_java_table(
        "varp",
        cache.varps.iter().map(oracle::varp).collect(),
        &fixture,
    );
    assert_java_table(
        "varbit",
        cache.varbits.iter().map(oracle::varbit).collect(),
        &fixture,
    );
    assert_eq!(json!(cache.tracked_varps), fixture["tracked_varps"]);
    for spot in &cache.spots {
        assert_eq!(spot.seq, (spot.anim >= 0).then_some(spot.anim as usize));
    }
    let data = jag.read("obj.dat").unwrap();
    let index = jag.read("obj.idx").unwrap();
    for (key, members) in [("obj_members", true), ("obj_nonmembers", false)] {
        for (id, expected) in fixture[key].as_object().unwrap() {
            let id = id.parse::<usize>().unwrap();
            let object = object_for_world(&data, &index, id, members);
            assert_eq!(&oracle::obj(&object), expected, "{key} id {id}");
        }
    }
}
