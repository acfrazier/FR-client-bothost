//! Port of `~/experiments/Server/webclient/src/datastruct/JString.ts`
//! `toUserhash` (the only method the login handshake needs).

pub struct JString;

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
}
