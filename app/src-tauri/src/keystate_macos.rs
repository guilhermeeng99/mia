//! macOS physical key-state probes for the hotkey layer (hotkey.rs) — the macOS
//! counterpart of Windows' `GetAsyncKeyState`. Used by modifier-only chord polling,
//! the missing-release watchdog (hotkeys.md Rule 11), and the Hub's chord recorder.
//!
//! WHY CoreGraphics `CGEventSourceKeyState` / `CGEventSourceFlagsState`: they read
//! the combined session's key state without an event tap, so no Input Monitoring
//! permission is needed. Key codes are the layout-independent `kVK_*` virtual codes.

use crate::hotkey::Mods;

/// `kCGEventSourceStateCombinedSessionState`.
const COMBINED_SESSION: i32 = 0;

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_OPTION: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

/// `kVK_ANSI_*` codes for A..Z (not contiguous on the ANSI layout).
const LETTERS: [(char, u16); 26] = [
    ('A', 0), ('B', 11), ('C', 8), ('D', 2), ('E', 14), ('F', 3), ('G', 5),
    ('H', 4), ('I', 34), ('J', 38), ('K', 40), ('L', 37), ('M', 46), ('N', 45),
    ('O', 31), ('P', 35), ('Q', 12), ('R', 15), ('S', 1), ('T', 17), ('U', 32),
    ('V', 9), ('W', 13), ('X', 7), ('Y', 16), ('Z', 6),
];

/// `kVK_ANSI_0..9`.
const DIGITS: [(char, u16); 10] = [
    ('0', 29), ('1', 18), ('2', 19), ('3', 20), ('4', 21),
    ('5', 23), ('6', 22), ('7', 26), ('8', 28), ('9', 25),
];

/// Named keys in the canonical spelling hotkey.rs uses. `Delete` is forward-delete,
/// matching Windows' `VK_DELETE`.
const NAMED: [(&str, u16); 9] = [
    ("Space", 49), ("Tab", 48), ("Enter", 36), ("Escape", 53), ("Delete", 117),
    ("Up", 126), ("Down", 125), ("Left", 123), ("Right", 124),
];

/// `kVK_F1..F20` (macOS has no F21-F24).
const FUNCTION_KEYS: [u16; 20] = [
    122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90,
];

/// Backspace, Home, End, PageUp, PageDown — non-modifiers that should block a
/// modifier-only chord but are never recorded as its main key.
const OTHER_NON_MODIFIERS: [u16; 5] = [51, 115, 119, 116, 121];

fn code_pressed(code: u16) -> bool {
    unsafe { CGEventSourceKeyState(COMBINED_SESSION, code) }
}

/// Map a canonical key name to its virtual key code. Pure, unit-tested.
fn key_code(key: &str) -> Option<u16> {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let c = c.to_ascii_uppercase();
        return LETTERS
            .iter()
            .chain(DIGITS.iter())
            .find(|(k, _)| *k == c)
            .map(|(_, code)| *code);
    }
    if let Some((_, code)) = NAMED.iter().find(|(name, _)| *name == key) {
        return Some(*code);
    }
    let n: usize = key.strip_prefix('F')?.parse().ok()?;
    FUNCTION_KEYS.get(n.checked_sub(1)?).copied()
}

/// Which modifiers are physically held right now (Cmd maps to `sup`).
pub fn current_mods_pressed() -> Mods {
    let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION) };
    Mods {
        ctrl: flags & FLAG_CONTROL != 0,
        alt: flags & FLAG_OPTION != 0,
        shift: flags & FLAG_SHIFT != 0,
        sup: flags & FLAG_COMMAND != 0,
    }
}

pub fn key_pressed(key: &str) -> bool {
    key_code(key).is_some_and(code_pressed)
}

/// First held non-modifier key, in its canonical name — feeds the chord recorder.
pub fn first_pressed_record_key() -> Option<String> {
    if let Some((name, _)) = NAMED.iter().find(|(_, code)| code_pressed(*code)) {
        return Some(name.to_string());
    }
    if let Some((c, _)) = LETTERS.iter().chain(DIGITS.iter()).find(|(_, code)| code_pressed(*code)) {
        return Some(c.to_string());
    }
    FUNCTION_KEYS
        .iter()
        .position(|code| code_pressed(*code))
        .map(|i| format!("F{}", i + 1))
}

pub fn any_non_modifier_pressed() -> bool {
    first_pressed_record_key().is_some() || OTHER_NON_MODIFIERS.iter().copied().any(code_pressed)
}

#[cfg(test)]
mod tests {
    use super::key_code;

    #[test]
    fn maps_canonical_keys_to_virtual_codes() {
        assert_eq!(key_code("A"), Some(0));
        assert_eq!(key_code("v"), Some(9));
        assert_eq!(key_code("0"), Some(29));
        assert_eq!(key_code("Space"), Some(49));
        assert_eq!(key_code("F1"), Some(122));
        assert_eq!(key_code("F20"), Some(90));
        assert_eq!(key_code("F21"), None);
        assert_eq!(key_code("F0"), None);
        assert_eq!(key_code("Nope"), None);
    }
}
