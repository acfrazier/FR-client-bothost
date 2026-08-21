// Port of `~/experiments/Server/webclient/src/wordfilter/WordPack.ts` (89 lines).
// Nibble codec for chat words: each byte holds two nibbles; a nibble < 13 is a
// direct TABLE index, a nibble >= 13 carries the high half of a two-nibble
// value (index = (carry << 4) + nibble - 195).
use crate::io::Packet;

pub struct WordPack;

impl WordPack {
    // prettier-ignore
    const TABLE: [char; 61] = [
        ' ', 'e', 't', 'a', 'o', 'i', 'h', 'n', 's', 'r', 'd', 'l', 'u',
        'm', 'w', 'c', 'y', 'f', 'g', 'p', 'b', 'v', 'k', 'x', 'j', 'q', 'z',
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        ' ', '!', '?', '.', ',', ':', ';', '(', ')', '-', '&', '*', '\\', '\'', '@', '#', '+', '=', '£', '$', '%', '"', '[', ']'
    ];

    pub fn unpack(word: &mut Packet, length: usize) -> String {
        let mut builder: Vec<char> = Vec::new();
        let mut carry: i32 = -1;
        for _index in 0..length {
            if builder.len() >= 100 {
                break;
            }
            let value: i32 = word.g1();
            let mut nibble: i32 = (value >> 4) & 0xf;
            if carry != -1 {
                builder.push(Self::TABLE[((carry << 4) + nibble - 195) as usize]);
                carry = -1;
            } else if nibble < 13 {
                builder.push(Self::TABLE[nibble as usize]);
            } else {
                carry = nibble;
            }
            nibble = value & 0xf;
            if carry != -1 {
                builder.push(Self::TABLE[((carry << 4) + nibble - 195) as usize]);
                carry = -1;
            } else if nibble < 13 {
                builder.push(Self::TABLE[nibble as usize]);
            } else {
                carry = nibble;
            }
        }
        let mut uppercase: bool = true;
        for c in &mut builder {
            let char: char = *c;
            if uppercase && char.is_ascii_lowercase() {
                *c = char.to_ascii_uppercase();
                uppercase = false;
            }
            if char == '.' || char == '!' {
                uppercase = true;
            }
        }
        builder.iter().collect()
    }

    pub fn pack(word: &mut Packet, str: &str) {
        let str: String = str.chars().take(80).collect::<String>().to_lowercase();
        let mut carry: i32 = -1;
        for char in str.chars() {
            let mut current_char: i32 = 0;
            for (lookup_index, table_char) in Self::TABLE.iter().enumerate() {
                if char == *table_char {
                    current_char = lookup_index as i32;
                    break;
                }
            }
            if current_char > 12 {
                current_char += 195;
            }
            if carry == -1 {
                if current_char < 13 {
                    carry = current_char;
                } else {
                    word.p1(current_char);
                }
            } else if current_char < 13 {
                word.p1((carry << 4) + current_char);
                carry = -1;
            } else {
                word.p1((carry << 4) + (current_char >> 4));
                carry = current_char & 0xf;
            }
        }
        if carry != -1 {
            word.p1(carry << 4);
        }
    }
}
