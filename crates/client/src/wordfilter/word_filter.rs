// Port of `~/experiments/Server/webclient/src/wordfilter/WordFilter.ts` (~938 lines).
// Tables live in a process-wide `OnceLock` (campaign statics rule): `unpack` is
// idempotent and `filter` returns the input unchanged until the tables exist.
// Each private method is ported 1:1, including the TS control flow that is
// unreachable (the webclient reimplementation kept the Java structure with
// mangled state); those branches are marked so they are not "fixed" later.
// Keep the TS comparison/branch text, so the port-fidelity lints are allowed
// at module level (same as dash3d/mod.rs).
#![allow(clippy::if_same_then_else)]
#![allow(clippy::manual_range_contains)]
use std::sync::OnceLock;

use crate::io::{JagFile, Packet};

pub struct WordFilter;

#[derive(Default)]
struct Tables {
    tld_types: Vec<i32>,
    tlds: Vec<Vec<i32>>,
    bad_words: Vec<Vec<i32>>,
    bad_combinations: Vec<Option<Vec<[i32; 2]>>>,
    domains: Vec<Vec<i32>>,
    fragments: Vec<i32>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

impl WordFilter {
    // prettier-ignore
    const PERIOD: [i32; 3] = ['d' as i32, 'o' as i32, 't' as i32];
    // prettier-ignore
    const AMPERSAT: [i32; 3] = ['(' as i32, 'a' as i32, ')' as i32];
    // prettier-ignore
    const SLASH: [i32; 5] = ['s' as i32, 'l' as i32, 'a' as i32, 's' as i32, 'h' as i32];
    const WHITELIST: [&str; 8] = [
        "cook", "cook's", "cooks", "seeks", "sheet", "woop", "woops", "faq",
    ];

    pub fn unpack(wordenc: &JagFile) {
        let _ = TABLES.get_or_init(|| {
            let mut tables = Tables::default();
            // TS assumes all four files exist and would throw on a missing
            // read; skip the table instead so a partial pack stays usable.
            if let Some(data) = wordenc.read("fragmentsenc.txt") {
                Self::read_fragments(&mut tables, &mut Packet::new(data));
            }
            if let Some(data) = wordenc.read("badenc.txt") {
                Self::read_bad_words(&mut tables, &mut Packet::new(data));
            }
            if let Some(data) = wordenc.read("domainenc.txt") {
                Self::read_domains(&mut tables, &mut Packet::new(data));
            }
            if let Some(data) = wordenc.read("tldlist.txt") {
                Self::read_tld(&mut tables, &mut Packet::new(data));
            }
            tables
        });
    }

    pub fn filter(input: &str) -> String {
        let Some(tables) = TABLES.get() else {
            return input.to_string();
        };
        let mut characters: Vec<char> = input.chars().collect();
        Self::format(&mut characters);
        let trimmed: String = characters.iter().collect::<String>().trim().to_string();
        let lowercase: String = trimmed.to_lowercase();
        let mut filtered: Vec<char> = lowercase.chars().collect();
        Self::filter_tlds(tables, &mut filtered);
        Self::filter_bad_words(tables, &mut filtered);
        Self::filter_domains(tables, &mut filtered);
        Self::filter_fragments(&mut filtered);
        let lowercase_chars: Vec<char> = lowercase.chars().collect();
        for index in 0..Self::WHITELIST.len() {
            let whitelisted: Vec<char> = Self::WHITELIST[index].chars().collect();
            let mut offset: i32 = -1;
            while let Some(found) =
                Self::index_of(&lowercase_chars, &whitelisted, (offset + 1) as usize)
            {
                offset = found as i32;
                for (char_index, char) in whitelisted.iter().enumerate() {
                    filtered[char_index + offset as usize] = *char;
                }
            }
        }
        let trimmed_chars: Vec<char> = trimmed.chars().collect();
        Self::replace_uppercases(&mut filtered, &trimmed_chars);
        Self::format_uppercases(&mut filtered);
        filtered.iter().collect::<String>().trim().to_string()
    }

    fn read_tld(tables: &mut Tables, packet: &mut Packet) {
        let count: i32 = packet.g4();
        for _ in 0..count {
            tables.tld_types.push(packet.g1());
            let length: i32 = packet.g1();
            let mut chars: Vec<i32> = Vec::with_capacity(length as usize);
            for _ in 0..length {
                chars.push(packet.g1());
            }
            tables.tlds.push(chars);
        }
    }

    fn read_bad_words(tables: &mut Tables, packet: &mut Packet) {
        let count: i32 = packet.g4();
        for _ in 0..count {
            let length: i32 = packet.g1();
            let mut chars: Vec<i32> = Vec::with_capacity(length as usize);
            for _ in 0..length {
                chars.push(packet.g1());
            }
            tables.bad_words.push(chars);
            let combo_count: i32 = packet.g1();
            let mut combos: Vec<[i32; 2]> = Vec::with_capacity(combo_count as usize);
            for _ in 0..combo_count {
                combos.push([packet.g1b(), packet.g1b()]);
            }
            if combos.is_empty() {
                tables.bad_combinations.push(None);
            } else {
                tables.bad_combinations.push(Some(combos));
            }
        }
    }

    fn read_domains(tables: &mut Tables, packet: &mut Packet) {
        let count: i32 = packet.g4();
        for _ in 0..count {
            let length: i32 = packet.g1();
            let mut chars: Vec<i32> = Vec::with_capacity(length as usize);
            for _ in 0..length {
                chars.push(packet.g1());
            }
            tables.domains.push(chars);
        }
    }

    fn read_fragments(tables: &mut Tables, packet: &mut Packet) {
        let count: i32 = packet.g4();
        tables.fragments = Vec::with_capacity(count as usize);
        for _ in 0..count {
            tables.fragments.push(packet.g2());
        }
    }

    fn filter_tlds(tables: &Tables, chars: &mut [char]) {
        let mut period: Vec<char> = chars.to_vec();
        let mut slash: Vec<char> = chars.to_vec();
        Self::filter_bad_combinations(tables, None, &mut period, &Self::PERIOD);
        Self::filter_bad_combinations(tables, None, &mut slash, &Self::SLASH);
        for index in 0..tables.tlds.len() {
            Self::filter_tld(
                &slash,
                tables.tld_types[index],
                chars,
                &tables.tlds[index],
                &period,
            );
        }
    }

    fn filter_bad_words(tables: &Tables, chars: &mut [char]) {
        for _combo_index in 0..2 {
            for index in (0..tables.bad_words.len()).rev() {
                Self::filter_bad_combinations(
                    tables,
                    tables.bad_combinations[index].as_deref(),
                    chars,
                    &tables.bad_words[index],
                );
            }
        }
    }

    fn filter_domains(tables: &Tables, chars: &mut [char]) {
        let mut ampersat: Vec<char> = chars.to_vec();
        let mut period: Vec<char> = chars.to_vec();
        Self::filter_bad_combinations(tables, None, &mut ampersat, &Self::AMPERSAT);
        Self::filter_bad_combinations(tables, None, &mut period, &Self::PERIOD);
        for index in (0..tables.domains.len()).rev() {
            Self::filter_domain(&period, &ampersat, &tables.domains[index], chars);
        }
    }

    // TS quirk: `startIndex` is recomputed every iteration (the Java `var3`
    // run counter was lost in the reimplementation), so this never masks and
    // only advances `currentIndex` past the number runs. Ported as-is,
    // including the dead `startIndex = 0` write TS keeps after the no-op if.
    #[allow(unused_assignments)]
    fn filter_fragments(chars: &mut [char]) {
        let mut current_index: i32 = 0;
        while (current_index as usize) < chars.len() {
            let number_index: i32 = Self::index_of_number(chars, current_index);
            if number_index == -1 {
                return;
            }
            let mut is_symbol_or_not_lowercase_alpha: bool = false;
            for index in current_index..number_index {
                if !Self::is_symbol(chars[index as usize])
                    && !Self::is_not_lowercase_alpha(chars[index as usize])
                {
                    is_symbol_or_not_lowercase_alpha = true;
                    break;
                }
            }
            let mut start_index: i32 = 0;
            if is_symbol_or_not_lowercase_alpha {
                start_index = 0;
            }
            if start_index == 0 {
                start_index = 1;
                current_index = number_index;
            }
            let mut value: i32 = 0;
            for &char in &chars[number_index as usize..current_index as usize] {
                value = value * 10 + char as i32 - 48;
            }
            if value <= 255 && current_index - number_index <= 8 {
                start_index += 1;
            } else {
                start_index = 0;
            }
            if start_index == 4 {
                Self::mask_chars(number_index, current_index, chars);
                start_index = 0;
            }
            current_index = Self::index_of_non_number(current_index, chars);
        }
    }

    fn is_bad_fragment(tables: &Tables, chars: &[char]) -> bool {
        if Self::is_numerical_chars(chars) {
            return true;
        }
        let value: i32 = Self::get_integer(chars);
        let fragments: &[i32] = &tables.fragments;
        let fragments_length: usize = fragments.len();
        if fragments_length == 0 {
            return false;
        }
        if value == fragments[0] || value == fragments[fragments_length - 1] {
            return true;
        }
        let mut start: i32 = 0;
        let mut end: i32 = (fragments_length - 1) as i32;
        while start <= end {
            let mid: i32 = (start + end) / 2;
            if value == fragments[mid as usize] {
                return true;
            } else if value < fragments[mid as usize] {
                end = mid - 1;
            } else {
                start = mid + 1;
            }
        }
        false
    }

    fn get_integer(chars: &[char]) -> i32 {
        if chars.len() > 6 {
            return 0;
        }
        let mut value: i32 = 0;
        for index in 0..chars.len() {
            let char: char = chars[chars.len() - index - 1];
            if Self::is_lowercase_alpha(char) {
                value = value * 38 + char as i32 + 1 - 'a' as i32;
            } else if char == '\'' {
                value = value * 38 + 27;
            } else if Self::is_numerical(char) {
                value = value * 38 + char as i32 + 28 - '0' as i32;
            } else if char != '\u{0}' {
                return 0;
            }
        }
        value
    }

    fn index_of_number(chars: &[char], offset: i32) -> i32 {
        let mut index: i32 = offset;
        while index >= 0 && (index as usize) < chars.len() {
            if Self::is_numerical(chars[index as usize]) {
                return index;
            }
            index += 1;
        }
        -1
    }

    fn index_of_non_number(offset: i32, chars: &[char]) -> i32 {
        let mut index: i32 = offset;
        while index >= 0 && (index as usize) < chars.len() {
            if !Self::is_numerical(chars[index as usize]) {
                return index;
            }
            index += 1;
        }
        chars.len() as i32
    }

    fn get_emulated_domain_char_len(next_char: char, domain_char: char, current_char: char) -> i32 {
        if domain_char == current_char {
            return 1;
        } else if domain_char == 'o' && current_char == '0' {
            return 1;
        } else if domain_char == 'o' && current_char == '(' && next_char == ')' {
            return 2;
        } else if domain_char == 'c'
            && (current_char == '(' || current_char == '<' || current_char == '[')
        {
            return 1;
        } else if domain_char == 'e' && current_char == '€' {
            return 1;
        } else if domain_char == 's' && current_char == '$' {
            return 1;
        } else if domain_char == 'l' && current_char == 'i' {
            return 1;
        }
        0
    }

    fn filter_domain(period: &[char], ampersat: &[char], domain: &[i32], chars: &mut [char]) {
        let domain_length: usize = domain.len();
        let chars_length: usize = chars.len();
        // TS relies on a negative loop bound when the domain is longer; guard
        // the usize subtraction instead.
        if domain_length > chars_length {
            return;
        }
        let mut index: i32 = 0;
        while index as usize <= chars_length - domain_length {
            let (matched, current_index) = Self::find_matching_domain(index, domain, chars);
            if matched {
                let ampersat_status: i32 =
                    Self::prefix_symbol_status(index, chars, 3, ampersat, &['@']);
                let period_status: i32 =
                    Self::suffix_symbol_status(current_index - 1, chars, 3, period, &['.', ',']);
                if ampersat_status > 2 || period_status > 2 {
                    Self::mask_chars(index, current_index, chars);
                }
            }
            index += 1;
        }
    }

    fn find_matching_domain(start_index: i32, domain: &[i32], chars: &[char]) -> (bool, i32) {
        let domain_length: usize = domain.len();
        let mut current_index: i32 = start_index;
        let mut domain_index: i32 = 0;
        while (current_index as usize) < chars.len() && (domain_index as usize) < domain_length {
            let current_char: char = chars[current_index as usize];
            let next_char: char = if (current_index + 1) < chars.len() as i32 {
                chars[(current_index + 1) as usize]
            } else {
                '\u{0}'
            };
            let current_length: i32 = Self::get_emulated_domain_char_len(
                next_char,
                Self::from_code(domain[domain_index as usize]),
                current_char,
            );
            if current_length > 0 {
                current_index += current_length;
                domain_index += 1;
            } else {
                if domain_index == 0 {
                    break;
                }
                let previous_length: i32 = Self::get_emulated_domain_char_len(
                    next_char,
                    Self::from_code(domain[(domain_index - 1) as usize]),
                    current_char,
                );
                if previous_length > 0 {
                    // TS also does `startIndex++` here; its local `startIndex`
                    // is never read again, so it is dropped.
                    current_index += previous_length;
                } else {
                    if (domain_index as usize) >= domain_length || !Self::is_symbol(current_char) {
                        break;
                    }
                    current_index += 1;
                }
            }
        }
        ((domain_index as usize) >= domain_length, current_index)
    }

    fn filter_bad_combinations(
        tables: &Tables,
        combos: Option<&[[i32; 2]]>,
        chars: &mut [char],
        bads: &[i32],
    ) {
        if bads.len() > chars.len() {
            return;
        }
        let mut start_index: i32 = 0;
        while start_index as usize <= chars.len() - bads.len() {
            let (current_index, bad_index, has_symbol, has_number, has_digit) =
                Self::process_bad_characters(chars, bads, start_index);
            // TS first reads `chars[currentIndex]`/`chars[currentIndex + 1]`
            // here but the values are reassigned before either branch uses
            // them, so the reads are dropped.
            if !(bad_index >= bads.len() as i32 && (!has_number || !has_digit)) {
                start_index += 1;
                continue;
            }
            let mut should_filter: bool = true;
            if has_symbol {
                let mut is_before_symbol: bool = false;
                let mut is_after_symbol: bool = false;
                if start_index - 1 < 0
                    || (Self::is_symbol(chars[(start_index - 1) as usize])
                        && chars[(start_index - 1) as usize] != '\'')
                {
                    is_before_symbol = true;
                }
                if current_index >= chars.len() as i32
                    || (Self::is_symbol(chars[current_index as usize])
                        && chars[current_index as usize] != '\'')
                {
                    is_after_symbol = true;
                }
                if !is_before_symbol || !is_after_symbol {
                    let mut is_substring_valid: bool = false;
                    let mut local_index: i32;
                    if is_before_symbol {
                        local_index = start_index;
                    } else {
                        local_index = start_index - 2;
                    }
                    while !is_substring_valid && local_index < current_index {
                        if local_index >= 0
                            && (!Self::is_symbol(chars[local_index as usize])
                                || chars[local_index as usize] == '\'')
                        {
                            let mut local_sub_string: Vec<char> = Vec::new();
                            let mut local_sub_string_index: i32 = 0;
                            while local_sub_string_index < 3
                                && (local_index + local_sub_string_index) < chars.len() as i32
                                && (!Self::is_symbol(
                                    chars[(local_index + local_sub_string_index) as usize],
                                ) || chars[(local_index + local_sub_string_index) as usize]
                                    == '\'')
                            {
                                local_sub_string
                                    .push(chars[(local_index + local_sub_string_index) as usize]);
                                local_sub_string_index += 1;
                            }
                            let mut is_sub_string_valid_condition: bool = true;
                            if local_sub_string_index == 0 {
                                is_sub_string_valid_condition = false;
                            }
                            if local_sub_string_index < 3
                                && local_index > 0
                                && (!Self::is_symbol(chars[(local_index - 1) as usize])
                                    || chars[(local_index - 1) as usize] == '\'')
                            {
                                is_sub_string_valid_condition = false;
                            }
                            if is_sub_string_valid_condition
                                && !Self::is_bad_fragment(tables, &local_sub_string)
                            {
                                is_substring_valid = true;
                            }
                        }
                        local_index += 1;
                    }
                    if !is_substring_valid {
                        should_filter = false;
                    }
                }
            } else {
                let mut current_char: char = ' ';
                if start_index > 0 {
                    current_char = chars[(start_index - 1) as usize];
                }
                let mut next_char: char = ' ';
                if current_index < chars.len() as i32 {
                    next_char = chars[current_index as usize];
                }
                let current: i32 = Self::get_index(current_char);
                let next: i32 = Self::get_index(next_char);
                if let Some(combos) = combos {
                    if Self::combo_matches(current, combos, next) {
                        should_filter = false;
                    }
                }
            }
            if should_filter {
                let mut numeral_count: i32 = 0;
                let mut alpha_count: i32 = 0;
                for &char in &chars[start_index as usize..current_index as usize] {
                    if Self::is_numerical(char) {
                        numeral_count += 1;
                    } else if Self::is_alpha(char) {
                        alpha_count += 1;
                    }
                }
                if numeral_count <= alpha_count {
                    Self::mask_chars(start_index, current_index, chars);
                }
            }
            start_index += 1;
        }
    }

    fn process_bad_characters(
        chars: &[char],
        bads: &[i32],
        start_index: i32,
    ) -> (i32, i32, bool, bool, bool) {
        let mut index: i32 = start_index;
        let mut bad_index: i32 = 0;
        let mut count: i32 = 0;
        let mut has_symbol: bool = false;
        let mut has_number: bool = false;
        let mut has_digit: bool = false;
        while index < chars.len() as i32 && !(has_number && has_digit) {
            let current_char: char = chars[index as usize];
            let next_char: char = if index + 1 < chars.len() as i32 {
                chars[(index + 1) as usize]
            } else {
                '\u{0}'
            };
            let current_length: i32 = if bad_index < bads.len() as i32 {
                Self::get_emulated_bad_char_len(
                    next_char,
                    Self::from_code(bads[bad_index as usize]),
                    current_char,
                )
            } else {
                0
            };
            if current_length > 0 {
                if current_length == 1 && Self::is_numerical(current_char) {
                    has_number = true;
                }
                if current_length == 2
                    && (Self::is_numerical(current_char) || Self::is_numerical(next_char))
                {
                    has_number = true;
                }
                index += current_length;
                bad_index += 1;
            } else {
                if bad_index == 0 {
                    break;
                }
                let previous_length: i32 = Self::get_emulated_bad_char_len(
                    next_char,
                    Self::from_code(bads[(bad_index - 1) as usize]),
                    current_char,
                );
                if previous_length > 0 {
                    index += previous_length;
                } else {
                    if bad_index >= bads.len() as i32 || !Self::is_not_lowercase_alpha(current_char)
                    {
                        break;
                    }
                    if Self::is_symbol(current_char) && current_char != '\'' {
                        has_symbol = true;
                    }
                    if Self::is_numerical(current_char) {
                        has_digit = true;
                    }
                    index += 1;
                    count += 1;
                    if ((count * 100) / (index - start_index)) > 90 {
                        break;
                    }
                }
            }
        }
        (index, bad_index, has_symbol, has_number, has_digit)
    }

    fn get_emulated_bad_char_len(next_char: char, bad_char: char, current_char: char) -> i32 {
        if bad_char == current_char {
            return 1;
        }
        if bad_char >= 'a' && bad_char <= 'm' {
            if bad_char == 'a' {
                if current_char != '4' && current_char != '@' && current_char != '^' {
                    if current_char == '/' && next_char == '\\' {
                        return 2;
                    }
                    return 0;
                }
                return 1;
            }
            if bad_char == 'b' {
                if current_char != '6' && current_char != '8' {
                    if current_char == '1' && next_char == '3' {
                        return 2;
                    }
                    return 0;
                }
                return 1;
            }
            if bad_char == 'c' {
                if current_char != '('
                    && current_char != '<'
                    && current_char != '{'
                    && current_char != '['
                {
                    return 0;
                }
                return 1;
            }
            if bad_char == 'd' {
                if current_char == '[' && next_char == ')' {
                    return 2;
                }
                return 0;
            }
            if bad_char == 'e' {
                if current_char != '3' && current_char != '€' {
                    return 0;
                }
                return 1;
            }
            if bad_char == 'f' {
                if current_char == 'p' && next_char == 'h' {
                    return 2;
                }
                if current_char == '£' {
                    return 1;
                }
                return 0;
            }
            if bad_char == 'g' {
                if current_char != '9' && current_char != '6' {
                    return 0;
                }
                return 1;
            }
            if bad_char == 'h' {
                if current_char == '#' {
                    return 1;
                }
                return 0;
            }
            if bad_char == 'i' {
                if current_char != 'y'
                    && current_char != 'l'
                    && current_char != 'j'
                    && current_char != '1'
                    && current_char != '!'
                    && current_char != ':'
                    && current_char != ';'
                    && current_char != '|'
                {
                    return 0;
                }
                return 1;
            }
            if bad_char == 'j' {
                return 0;
            }
            if bad_char == 'k' {
                return 0;
            }
            if bad_char == 'l' {
                if current_char != '1' && current_char != '|' && current_char != 'i' {
                    return 0;
                }
                return 1;
            }
            if bad_char == 'm' {
                return 0;
            }
        }
        if bad_char >= 'n' && bad_char <= 'z' {
            if bad_char == 'n' {
                return 0;
            }
            if bad_char == 'o' {
                if current_char != '0' && current_char != '*' {
                    if (current_char != '(' || next_char != ')')
                        && (current_char != '[' || next_char != ']')
                        && (current_char != '{' || next_char != '}')
                        && (current_char != '<' || next_char != '>')
                    {
                        return 0;
                    }
                    return 2;
                }
                return 1;
            }
            if bad_char == 'p' {
                return 0;
            }
            if bad_char == 'q' {
                return 0;
            }
            if bad_char == 'r' {
                return 0;
            }
            if bad_char == 's' {
                if current_char != '5'
                    && current_char != 'z'
                    && current_char != '$'
                    && current_char != '2'
                {
                    return 0;
                }
                return 1;
            }
            if bad_char == 't' {
                if current_char != '7' && current_char != '+' {
                    return 0;
                }
                return 1;
            }
            if bad_char == 'u' {
                if current_char == 'v' {
                    return 1;
                }
                if (current_char != '\\' || next_char != '/')
                    && (current_char != '\\' || next_char != '|')
                    && (current_char != '|' || next_char != '/')
                {
                    return 0;
                }
                return 2;
            }
            if bad_char == 'v' {
                if (current_char != '\\' || next_char != '/')
                    && (current_char != '\\' || next_char != '|')
                    && (current_char != '|' || next_char != '/')
                {
                    return 0;
                }
                return 2;
            }
            if bad_char == 'w' {
                if current_char == 'v' && next_char == 'v' {
                    return 2;
                }
                return 0;
            }
            if bad_char == 'x' {
                if (current_char != ')' || next_char != '(')
                    && (current_char != '}' || next_char != '{')
                    && (current_char != ']' || next_char != '[')
                    && (current_char != '>' || next_char != '<')
                {
                    return 0;
                }
                return 2;
            }
            if bad_char == 'y' {
                return 0;
            }
            if bad_char == 'z' {
                return 0;
            }
        }
        if bad_char >= '0' && bad_char <= '9' {
            if bad_char == '0' {
                if current_char == 'o' || current_char == 'O' {
                    return 1;
                } else if (current_char != '(' || next_char != ')')
                    && (current_char != '{' || next_char != '}')
                    && (current_char != '[' || next_char != ']')
                {
                    return 0;
                } else {
                    return 2;
                }
            } else if bad_char == '1' {
                return if current_char == 'l' { 1 } else { 0 };
            } else {
                return 0;
            }
        } else if bad_char == ',' {
            return if current_char == '.' { 1 } else { 0 };
        } else if bad_char == '.' {
            return if current_char == ',' { 1 } else { 0 };
        } else if bad_char == '!' {
            return if current_char == 'i' { 1 } else { 0 };
        }
        0
    }

    fn combo_matches(current_index: i32, combos: &[[i32; 2]], next_index: i32) -> bool {
        let mut start: i32 = 0;
        let mut end: i32 = (combos.len() - 1) as i32;
        while start <= end {
            let mid: i32 = (start + end) / 2;
            if combos[mid as usize][0] == current_index && combos[mid as usize][1] == next_index {
                return true;
            } else if current_index < combos[mid as usize][0]
                || (current_index == combos[mid as usize][0]
                    && next_index < combos[mid as usize][1])
            {
                end = mid - 1;
            } else {
                start = mid + 1;
            }
        }
        false
    }

    fn get_index(char: char) -> i32 {
        if Self::is_lowercase_alpha(char) {
            return char as i32 + 1 - 'a' as i32;
        } else if char == '\'' {
            return 28;
        } else if Self::is_numerical(char) {
            return char as i32 + 29 - '0' as i32;
        }
        27
    }

    // TS re-declares `foundPeriod = false` between the two extension scans;
    // the write is dead (kept for 1:1 structure).
    #[allow(unused_assignments)]
    fn filter_tld(slash: &[char], tld_type: i32, chars: &mut [char], tld: &[i32], period: &[char]) {
        if tld.len() > chars.len() {
            return;
        }
        let mut index: i32 = 0;
        while index as usize <= chars.len() - tld.len() {
            let (current_index, tld_index) = Self::process_tlds(chars, tld, index);
            if tld_index < tld.len() as i32 {
                index += 1;
                continue;
            }
            let mut should_filter: bool = false;
            let period_filter_status: i32 =
                Self::prefix_symbol_status(index, chars, 3, period, &[',', '.']);
            let slash_filter_status: i32 =
                Self::suffix_symbol_status(current_index - 1, chars, 5, slash, &['\\', '/']);
            if tld_type == 1 && period_filter_status > 0 && slash_filter_status > 0 {
                should_filter = true;
            }
            if tld_type == 2
                && ((period_filter_status > 2 && slash_filter_status > 0)
                    || (period_filter_status > 0 && slash_filter_status > 2))
            {
                should_filter = true;
            }
            if tld_type == 3 && period_filter_status > 0 && slash_filter_status > 2 {
                should_filter = true;
            }
            if should_filter {
                let mut start_filter_index: i32 = index;
                let mut end_filter_index: i32 = current_index - 1;
                let mut found_period: bool = false;
                if period_filter_status > 2 {
                    if period_filter_status == 4 {
                        found_period = false;
                        let mut period_index: i32 = index - 1;
                        while period_index >= 0 {
                            if found_period {
                                if period[period_index as usize] != '*' {
                                    break;
                                }
                                start_filter_index = period_index;
                            } else if period[period_index as usize] == '*' {
                                start_filter_index = period_index;
                                found_period = true;
                            }
                            period_index -= 1;
                        }
                    }
                    found_period = false;
                    let mut period_index: i32 = start_filter_index - 1;
                    while period_index >= 0 {
                        if found_period {
                            if Self::is_symbol(chars[period_index as usize]) {
                                break;
                            }
                            start_filter_index = period_index;
                        } else if !Self::is_symbol(chars[period_index as usize]) {
                            found_period = true;
                            start_filter_index = period_index;
                        }
                        period_index -= 1;
                    }
                }
                if slash_filter_status > 2 {
                    if slash_filter_status == 4 {
                        found_period = false;
                        let mut period_index: i32 = end_filter_index + 1;
                        while period_index < chars.len() as i32 {
                            if found_period {
                                if slash[period_index as usize] != '*' {
                                    break;
                                }
                                end_filter_index = period_index;
                            } else if slash[period_index as usize] == '*' {
                                end_filter_index = period_index;
                                found_period = true;
                            }
                            period_index += 1;
                        }
                    }
                    found_period = false;
                    let mut period_index: i32 = end_filter_index + 1;
                    while period_index < chars.len() as i32 {
                        if found_period {
                            if Self::is_symbol(chars[period_index as usize]) {
                                break;
                            }
                            end_filter_index = period_index;
                        } else if !Self::is_symbol(chars[period_index as usize]) {
                            found_period = true;
                            end_filter_index = period_index;
                        }
                        period_index += 1;
                    }
                }
                Self::mask_chars(start_filter_index, end_filter_index + 1, chars);
            }
            index += 1;
        }
    }

    fn process_tlds(chars: &[char], tld: &[i32], mut current_index: i32) -> (i32, i32) {
        let mut tld_index: i32 = 0;
        while (current_index as usize) < chars.len() && (tld_index as usize) < tld.len() {
            let current_char: char = chars[current_index as usize];
            let next_char: char = if (current_index + 1) < chars.len() as i32 {
                chars[(current_index + 1) as usize]
            } else {
                '\u{0}'
            };
            let current_length: i32 = Self::get_emulated_domain_char_len(
                next_char,
                Self::from_code(tld[tld_index as usize]),
                current_char,
            );
            if current_length > 0 {
                current_index += current_length;
                tld_index += 1;
            } else {
                if tld_index == 0 {
                    break;
                }
                let previous_length: i32 = Self::get_emulated_domain_char_len(
                    next_char,
                    Self::from_code(tld[(tld_index - 1) as usize]),
                    current_char,
                );
                if previous_length > 0 {
                    current_index += previous_length;
                } else {
                    if !Self::is_symbol(current_char) {
                        break;
                    }
                    current_index += 1;
                }
            }
        }
        (current_index, tld_index)
    }

    fn is_symbol(char: char) -> bool {
        !Self::is_alpha(char) && !Self::is_numerical(char)
    }

    fn is_not_lowercase_alpha(char: char) -> bool {
        if Self::is_lowercase_alpha(char) {
            char == 'v' || char == 'x' || char == 'j' || char == 'q' || char == 'z'
        } else {
            true
        }
    }

    fn is_alpha(char: char) -> bool {
        Self::is_lowercase_alpha(char) || Self::is_uppercase_alpha(char)
    }

    fn is_numerical(char: char) -> bool {
        char >= '0' && char <= '9'
    }

    fn is_lowercase_alpha(char: char) -> bool {
        char >= 'a' && char <= 'z'
    }

    fn is_uppercase_alpha(char: char) -> bool {
        char >= 'A' && char <= 'Z'
    }

    fn is_numerical_chars(chars: &[char]) -> bool {
        for char in chars {
            if !Self::is_numerical(*char) && *char != '\u{0}' {
                return false;
            }
        }
        true
    }

    fn mask_chars(offset: i32, length: i32, chars: &mut [char]) {
        for index in offset..length {
            chars[index as usize] = '*';
        }
    }

    fn masked_count_backwards(chars: &[char], offset: i32) -> i32 {
        let mut count: i32 = 0;
        let mut index: i32 = offset - 1;
        while index >= 0 && Self::is_symbol(chars[index as usize]) {
            if chars[index as usize] == '*' {
                count += 1;
            }
            index -= 1;
        }
        count
    }

    fn masked_count_forwards(chars: &[char], offset: i32) -> i32 {
        let mut count: i32 = 0;
        let mut index: i32 = offset + 1;
        while index < chars.len() as i32 && Self::is_symbol(chars[index as usize]) {
            if chars[index as usize] == '*' {
                count += 1;
            }
            index += 1;
        }
        count
    }

    fn masked_chars_status(
        chars: &[char],
        filtered: &[char],
        offset: i32,
        length: i32,
        prefix: bool,
    ) -> i32 {
        let count: i32 = if prefix {
            Self::masked_count_backwards(filtered, offset)
        } else {
            Self::masked_count_forwards(filtered, offset)
        };
        if count >= length {
            return 4;
        } else if Self::is_symbol(if prefix {
            Self::get(chars, offset - 1)
        } else {
            Self::get(chars, offset + 1)
        }) {
            return 1;
        }
        0
    }

    fn prefix_symbol_status(
        offset: i32,
        chars: &[char],
        length: i32,
        symbol_chars: &[char],
        symbols: &[char],
    ) -> i32 {
        if offset == 0 {
            return 2;
        }
        let mut index: i32 = offset - 1;
        while index >= 0 && Self::is_symbol(chars[index as usize]) {
            if symbols.contains(&chars[index as usize]) {
                return 3;
            }
            index -= 1;
        }
        Self::masked_chars_status(chars, symbol_chars, offset, length, true)
    }

    fn suffix_symbol_status(
        offset: i32,
        chars: &[char],
        length: i32,
        symbol_chars: &[char],
        symbols: &[char],
    ) -> i32 {
        if offset + 1 == chars.len() as i32 {
            return 2;
        }
        let mut index: i32 = offset + 1;
        while index < chars.len() as i32 && Self::is_symbol(chars[index as usize]) {
            if symbols.contains(&chars[index as usize]) {
                return 3;
            }
            index += 1;
        }
        Self::masked_chars_status(chars, symbol_chars, offset, length, false)
    }

    fn format(chars: &mut [char]) {
        let mut pos: i32 = 0;
        for index in 0..chars.len() {
            if Self::is_character_allowed(chars[index]) {
                chars[pos as usize] = chars[index];
            } else {
                chars[pos as usize] = ' ';
            }
            if pos == 0 || chars[pos as usize] != ' ' || chars[(pos - 1) as usize] != ' ' {
                pos += 1;
            }
        }
        chars[pos as usize..].fill(' ');
    }

    fn is_character_allowed(char: char) -> bool {
        (char >= ' ' && char <= '\u{7f}')
            || char == ' '
            || char == '\n'
            || char == '\t'
            || char == '£'
            || char == '€'
    }

    fn replace_uppercases(chars: &mut [char], comparison: &[char]) {
        for index in 0..comparison.len() {
            if chars[index] != '*' && Self::is_uppercase_alpha(comparison[index]) {
                chars[index] = comparison[index];
            }
        }
    }

    fn format_uppercases(chars: &mut [char]) {
        let mut flagged: bool = true;
        for char in chars {
            let value: char = *char;
            if !Self::is_alpha(value) {
                flagged = true;
            } else if flagged {
                if Self::is_lowercase_alpha(value) {
                    flagged = false;
                }
            } else if Self::is_uppercase_alpha(value) {
                *char = char::from_u32(value as u32 + 'a' as u32 - 65).unwrap();
            }
        }
    }

    // TS reads `chars[index]` past the end as `undefined`; the predicates
    // treat it like any other symbol, so a '\u0000' sentinel is equivalent.
    fn get(chars: &[char], index: i32) -> char {
        if index >= 0 && (index as usize) < chars.len() {
            chars[index as usize]
        } else {
            '\u{0}'
        }
    }

    // TS `String.fromCharCode` for table values (all single-byte).
    fn from_code(code: i32) -> char {
        char::from_u32(code as u32).unwrap_or('\u{0}')
    }

    // TS `String.indexOf` equivalent over a char slice; returns the char index.
    fn index_of(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
        if needle.is_empty() || needle.len() > haystack.len().saturating_sub(from) {
            return None;
        }
        for index in from..=haystack.len() - needle.len() {
            if haystack[index..index + needle.len()] == *needle {
                return Some(index);
            }
        }
        None
    }
}
