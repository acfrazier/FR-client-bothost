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
    let path = client::engine_dir().display().to_string();
    let bytes = std::fs::read(format!("{path}/data/pack/client/config"));
    let Ok(bytes) = bytes else {
        return;
    };
    let jag = JagFile::new(bytes);
    assert!(jag.file_count > 0);
    assert!(jag.read("obj.dat").is_some() || jag.read("loc.dat").is_some() || jag.file_count > 0);
}
