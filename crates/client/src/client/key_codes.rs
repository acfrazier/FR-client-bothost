//! DOM key name to Java keycode map, ported from client-ts
//! `src/client/KeyCodes.ts`. `code` is the DOM/Java keycode; `ch` is the
//! remapped value `GameShell::apply_key` uses to index `key_held` and
//! enqueue into `key_queue`.

/// One KeyCodes.ts entry: the Java keycode and the char to queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JavaKeyCode {
    pub code: i32,
    pub ch: i32,
}

/// Look up a DOM `KeyboardEvent.key` string (e.g. `"ArrowLeft"`, `"a"`).
/// Punctuation values are KeyCodes.ts 48–80 (`:` ch 58, `~` ch 126).
pub fn lookup(name: &str) -> Option<JavaKeyCode> {
    let (code, ch) = match name {
        "ArrowLeft" => (37, 1),
        "ArrowRight" => (39, 2),
        "ArrowUp" => (38, 3),
        "ArrowDown" => (40, 4),
        "Control" => (17, 5),
        "Shift" => (16, 6),
        "Alt" => (18, 7),
        "Backspace" => (8, 8),
        "Tab" => (9, 9),
        "Enter" | "\r" | "\n" => (10, 10),
        "Escape" => (27, 27),
        " " => (32, 32),
        "Delete" => (127, 8),
        "`" => (192, 96),
        "~" => (192, 126),
        "!" => (49, 33),
        "@" => (50, 64),
        "#" => (51, 35),
        "£" => (51, 163),
        "$" => (52, 36),
        "%" => (53, 37),
        "^" => (54, 94),
        "&" => (55, 38),
        "*" => (56, 42),
        "(" => (57, 40),
        ")" => (48, 41),
        "-" => (45, 45),
        "_" => (45, 95),
        "=" => (61, 61),
        "+" => (61, 43),
        "[" => (91, 91),
        "{" => (91, 123),
        "]" => (93, 93),
        "}" => (93, 125),
        "\\" => (92, 92),
        "|" => (92, 124),
        ";" => (59, 59),
        ":" => (59, 58),
        "'" => (222, 39),
        "\"" => (222, 34),
        "," => (44, 44),
        "<" => (44, 60),
        "." => (46, 46),
        ">" => (46, 62),
        "/" => (47, 47),
        "?" => (47, 63),
        _ => {
            let bytes = name.as_bytes();
            if bytes.len() != 1 {
                return None;
            }
            let b = bytes[0];
            let (code, ch) = match b {
                b'0'..=b'9' => (b as i32, b as i32),
                b'a'..=b'z' => (b as i32 - 32, b as i32),
                b'A'..=b'Z' => (b as i32, b as i32),
                _ => return None,
            };
            return Some(JavaKeyCode { code, ch });
        }
    };
    Some(JavaKeyCode { code, ch })
}
