//! DOM key name to Java keycode map, ported from client-ts
//! `src/client/KeyCodes.ts`. `code` indexes `GameShell::key_held` and
//! `GameShell::key_queue`; `ch` is the char queued for text entry.

/// One KeyCodes.ts entry: the Java keycode and the char to queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JavaKeyCode {
    pub code: i32,
    pub ch: i32,
}

/// Look up a DOM `KeyboardEvent.key` string (e.g. `"ArrowLeft"`, `"a"`).
pub fn lookup(name: &str) -> Option<JavaKeyCode> {
    let (code, ch) = match name {
        "ArrowLeft" => (37, 1),
        "ArrowRight" => (39, 2),
        "ArrowUp" => (38, 3),
        "ArrowDown" => (40, 4),
        "Enter" => (10, 10),
        "Backspace" => (8, 8),
        " " => (32, 32),
        _ => {
            // single-char entries from KeyCodes.ts: 0-9 and a-z/A-Z
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
