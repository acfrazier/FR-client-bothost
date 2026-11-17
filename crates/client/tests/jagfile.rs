use client::io::JagFile;

#[test]
fn gen_hash_matches_java() {
    // JagFile.genHash: hash = (hash * 61 + ch - 32) | 0, name uppercased
    assert_eq!(JagFile::gen_hash("p11_full"), JagFile::gen_hash("P11_FULL"));
    let mut hash: i32 = 0;
    for c in "CONFIG".chars() {
        hash = hash
            .wrapping_mul(61)
            .wrapping_add(c as i32)
            .wrapping_sub(32);
    }
    assert_eq!(JagFile::gen_hash("config"), hash);
}

#[test]
fn reads_unpacked_jag_from_engine_pack() {
    let pack = client::engine_dir().join("data/pack/client/config");
    if !pack.is_file() {
        eprintln!("SKIP reads_unpacked_jag_from_engine_pack: local engine config JAG is missing");
        return;
    }
    let bytes = std::fs::read(pack).expect("present engine config JAG must be readable");
    let jag = JagFile::new(bytes);
    assert!(jag.file_count > 0);
    // The config archive must expose the exact `obj.dat`/`obj.idx` pair the
    // client decodes (`ObjType::unpack`). Both files lead with the same g2
    // record count; `obj.idx` then holds one g2 size per record, and
    // `obj.dat` is that count followed by the records back to back. So the
    // exact `obj.dat` size follows from `obj.idx` alone: a wrong entry, a
    // wrong offset or a short decompression breaks the equality.
    let dat = jag.read("obj.dat").expect("config jag has obj.dat");
    let idx = jag.read("obj.idx").expect("config jag has obj.idx");
    let g2 = |bytes: &[u8], at: usize| usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
    let count = g2(&idx, 0);
    assert!(count > 0, "obj.idx declares no records");
    assert_eq!(
        idx.len(),
        2 + 2 * count,
        "obj.idx is a count and one g2 size per record"
    );
    assert_eq!(
        g2(&dat, 0),
        count,
        "obj.dat and obj.idx declare the same count"
    );
    let records: usize = (0..count).map(|record| g2(&idx, 2 + 2 * record)).sum();
    assert_eq!(dat.len(), 2 + records, "obj.dat size follows from obj.idx");
    assert_eq!(
        jag.read("no-such-entry.dat"),
        None,
        "an absent name reads nothing"
    );
}
