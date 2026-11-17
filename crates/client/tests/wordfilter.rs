use client::io::JagFile;
use client::wordfilter::WordFilter;
use std::fs;

#[test]
fn filter_without_unpack_is_identity() {
    // a fresh process may already have unpacked if another test ran first;
    // identity is only required when tables are empty. A harmless sentence
    // must come back byte for byte either way: no table entry matches it,
    // and its case already satisfies `format_uppercases`.
    assert_eq!(WordFilter::filter("Hello world."), "Hello world.");
}

/// The first `badenc.txt` entry the filter masks without exceptions: no
/// combination list, plain lowercase ASCII letters, and no whitelist overlap.
/// Decoded here with the `read_bad_words` layout (g4 count; per entry g1
/// length, that many g1 chars, g1 combination count, two g1b per combination)
/// so the expected word comes from the data, not from the filter under test.
fn plain_bad_word(badenc: &[u8]) -> Option<String> {
    let count = u32::from_be_bytes(badenc.get(0..4)?.try_into().ok()?);
    let mut pos = 4usize;
    for _ in 0..count {
        let len = usize::from(*badenc.get(pos)?);
        let word = badenc.get(pos + 1..pos + 1 + len)?;
        pos += 1 + len;
        let combos = usize::from(*badenc.get(pos)?);
        pos += 1 + 2 * combos;
        let whitelisted = ["cook", "seeks", "sheet", "woop", "faq"]
            .iter()
            .any(|white| word.windows(white.len()).any(|w| w == white.as_bytes()));
        if combos == 0 && len >= 3 && word.iter().all(u8::is_ascii_lowercase) && !whitelisted {
            return String::from_utf8(word.to_vec()).ok();
        }
    }
    None
}

#[test]
fn unpack_wordenc_and_whitelist_cook() {
    let path = client::cache_dir().join("wordenc").display().to_string();
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    let jag = JagFile::new(bytes);
    WordFilter::unpack(&jag);
    assert_eq!(WordFilter::filter("cook"), "cook");

    let badenc = jag.read("badenc.txt").expect("wordenc has badenc.txt");
    let word = plain_bad_word(&badenc).expect("badenc.txt has a plain lowercase entry");
    let stars = "*".repeat(word.len());
    assert_eq!(
        WordFilter::filter(&word),
        stars,
        "the bad word {word:?} must be masked whole"
    );
    // The webclient TS filter (the port's source) gives the same two results
    // on this archive. The neighbours matter there: after "Say" it shifts
    // the mask ("Sa*****le now."), so this sentence keeps short words that
    // the reference masks plainly.
    assert_eq!(
        WordFilter::filter(&format!("Go {word} now.")),
        format!("Go {stars} now."),
        "only the bad word {word:?} is masked; the rest keeps its text and case"
    );
}
