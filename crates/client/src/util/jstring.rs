//! Port of `~/experiments/Server/webclient/src/datastruct/JString.ts`:
//! `toUserhash` (login handshake) plus the name decode used by
//! `ClientPlayer.setAppearance`.

pub struct JString;

// prettier-ignore
const USERHASH_CHAR: [char; 37] = [
    '_',
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
    's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
];

impl JString {
    /// `JString.toUserhash`: hash a name into base-37, trimming and taking at
    /// most 12 chars. Letters A-Z/a-z map to 1-26, digits 0-9 to 27-36;
    /// anything else contributes 0. Trailing 37-factors are stripped. The
    /// value fits in u64 (max 37^12 - 1).
    pub fn to_userhash(string: &str) -> u64 {
        let mut hash: u64 = 0;
        for c in string.trim().chars().take(12) {
            hash *= 37;
            let c = c as u32;
            if (0x41..=0x5a).contains(&c) {
                // A-Z
                hash += (c + 1 - 0x41) as u64;
            } else if (0x61..=0x7a).contains(&c) {
                // a-z
                hash += (c + 1 - 0x61) as u64;
            } else if (0x30..=0x39).contains(&c) {
                // 0-9
                hash += (c + 27 - 0x30) as u64;
            }
        }

        while hash.is_multiple_of(37) && hash != 0 {
            hash /= 37;
        }

        hash
    }

    /// `JString.toRawUsername(value)`: decode a base-37 user hash.
    pub fn to_raw_username(value: i64) -> String {
        // >= 37 to the 12th power
        if value < 0 || value as u64 >= 6582952005840035281 {
            return "invalid_name".into();
        }

        if value % 37 == 0 {
            return "invalid_name".into();
        }

        let mut value = value as u64;
        let mut len = 0usize;
        let mut chars = ['_'; 12];
        while value != 0 {
            let l1 = value;
            value /= 37;
            let index = (l1 - value * 37) as usize;
            chars[11 - len] = USERHASH_CHAR[index];
            len += 1;
        }

        chars[12 - len..].iter().collect()
    }

    /// `JString.toScreenName(str)`: sentence-case a raw username.
    pub fn to_screen_name(input: &str) -> String {
        if input.is_empty() {
            return input.to_string();
        }

        let mut chars: Vec<char> = input.chars().collect();
        for i in 0..chars.len() {
            if chars[i] == '_' {
                chars[i] = ' ';
                if i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase() {
                    chars[i + 1] = chars[i + 1].to_ascii_uppercase();
                }
            }
        }

        if chars[0].is_ascii_lowercase() {
            chars[0] = chars[0].to_ascii_uppercase();
        }

        chars.into_iter().collect()
    }

    /// `JString.getRepeatedCharacter(str)`: one `*` per character (the
    /// password field mask).
    pub fn get_repeated_character(str: &str) -> String {
        "*".repeat(str.chars().count())
    }

    /// `JString.toSentenceCase(input)`: lowercase, then uppercase each
    /// letter that follows the string start or a `.`/`!`.
    pub fn to_sentence_case(input: &str) -> String {
        let mut chars: Vec<char> = input.to_lowercase().chars().collect();
        let mut punctuation = true;
        for char in chars.iter_mut() {
            if punctuation && char.is_ascii_lowercase() {
                *char = char.to_ascii_uppercase();
                punctuation = false;
            }
            if *char == '.' || *char == '!' {
                punctuation = true;
            }
        }
        chars.into_iter().collect()
    }
}
