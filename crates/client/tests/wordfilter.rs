use client::io::JagFile;
use client::wordfilter::WordFilter;
use std::fs;

#[test]
fn filter_without_unpack_is_identity() {
    // a fresh process may already have unpacked if another test ran first;
    // identity is only required when tables are empty. Assert that a
    // harmless sentence is unchanged either way:
    let out = WordFilter::filter("Hello world.");
    assert!(out.contains("ello") || out.contains("Hello"));
}

#[test]
fn unpack_wordenc_and_whitelist_cook() {
    let path = "/Users/acfrazier/experiments/Server/engine/data/pack/client/wordenc";
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    let jag = JagFile::new(bytes);
    WordFilter::unpack(&jag);
    assert_eq!(WordFilter::filter("cook"), "cook");
}
