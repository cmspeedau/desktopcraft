use super::*;

/// A chord contains ANY 2-3 distinct supported physical keys; it need not
/// contain modifiers. Key order does not affect matching. This is a global
/// key-state check, not a keyboard hook that records user typing.
pub(super) fn peek_key_codes() -> impl Iterator<Item = i32> {
    [0x11, 0x12, 0x10, 0x09, 0x20, 0x25, 0x26, 0x27, 0x28]
        .into_iter()
        .chain(0x30..=0x39)
        .chain(0x41..=0x5A)
        .chain(0x70..=0x87)
}
fn peek_key_label(key: i32) -> Option<String> {
    match key {
        0x11 => Some("Ctrl".into()),
        0x12 => Some("Alt".into()),
        0x10 => Some("Shift".into()),
        0x09 => Some("Tab".into()),
        0x20 => Some("Space".into()),
        0x25 => Some("Left".into()),
        0x26 => Some("Up".into()),
        0x27 => Some("Right".into()),
        0x28 => Some("Down".into()),
        0x30..=0x39 | 0x41..=0x5A => Some((key as u8 as char).to_string()),
        0x70..=0x87 => Some(format!("F{}", key - 0x70 + 1)),
        _ => None,
    }
}
fn peek_key_code(label: &str) -> Option<i32> {
    let upper = label.trim().to_ascii_uppercase();
    match upper.as_str() {
        "CTRL" | "CONTROL" => Some(0x11),
        "ALT" => Some(0x12),
        "SHIFT" => Some(0x10),
        "TAB" => Some(0x09),
        "SPACE" => Some(0x20),
        "LEFT" => Some(0x25),
        "UP" => Some(0x26),
        "RIGHT" => Some(0x27),
        "DOWN" => Some(0x28),
        _ if upper.len() == 1 && upper.as_bytes()[0].is_ascii_alphanumeric() => {
            Some(upper.as_bytes()[0] as i32)
        }
        _ if upper.starts_with('F')
            && upper[1..]
                .parse::<u8>()
                .is_ok_and(|n| (1..=24).contains(&n)) =>
        {
            Some(0x70 + upper[1..].parse::<i32>().ok()? - 1)
        }
        _ => None,
    }
}
fn normalized_keys(mut keys: Vec<i32>) -> Option<Vec<i32>> {
    let original_len = keys.len();
    if !(2..=3).contains(&original_len) {
        return None;
    }
    if keys.iter().any(|key| peek_key_label(*key).is_none()) {
        return None;
    }
    keys.sort_by_key(|key| {
        peek_key_codes()
            .position(|code| code == *key)
            .unwrap_or(usize::MAX)
    });
    keys.dedup();
    (keys.len() == original_len).then_some(keys)
}
pub fn parse_peek_shortcut(value: &str) -> Option<Vec<i32>> {
    let parts: Vec<_> = value.split('+').collect();
    normalized_keys(
        parts
            .iter()
            .map(|part| peek_key_code(part))
            .collect::<Option<Vec<_>>>()?,
    )
}
pub fn describe_peek_keys(keys: Vec<i32>) -> Option<String> {
    let codes = normalized_keys(keys)?;
    Some(
        codes
            .iter()
            .filter_map(|key| peek_key_label(*key))
            .collect::<Vec<_>>()
            .join("+"),
    )
}
/// Pure chord check used by the Windows poller and tests. Only an exact
/// simultaneous two- or three-key match can keep Peek active.
pub fn peek_chord_held(shortcut: &str, pressed: &[i32]) -> bool {
    match (
        parse_peek_shortcut(shortcut),
        normalized_keys(pressed.to_vec()),
    ) {
        (Some(expected), Some(held)) => expected == held,
        _ => false,
    }
}

#[cfg(test)]
mod shortcut_parse_tests {
    use super::*;
    #[test]
    fn peek_supports_any_two_or_three_distinct_keys() {
        assert_eq!(parse_peek_shortcut("Z+X+C"), Some(vec![0x43, 0x58, 0x5A]));
        assert_eq!(
            parse_peek_shortcut("Ctrl+Shift+Tab"),
            Some(vec![0x11, 0x10, 0x09])
        );
        assert_eq!(
            parse_peek_shortcut("F1+F2+F3"),
            Some(vec![0x70, 0x71, 0x72])
        );
        assert_eq!(
            parse_peek_shortcut("Ctrl+Alt+F10"),
            Some(vec![0x11, 0x12, 0x79])
        );
        assert_eq!(parse_peek_shortcut("Ctrl+Tab"), Some(vec![0x11, 0x09]));
        assert_eq!(parse_peek_shortcut("Shift+P"), Some(vec![0x10, 0x50]));
        assert_eq!(
            describe_peek_keys(vec![0x5A, 0x58, 0x43]).as_deref(),
            Some("C+X+Z")
        );
        assert!(peek_chord_held("Z+X+C", &[0x43, 0x58, 0x5A]));
        assert!(!peek_chord_held("Z+X+C", &[0x43, 0x58])); // key released
        assert!(!peek_chord_held("Z+X+C", &[0x43, 0x58, 0x5A, 0x11]));
        assert!(peek_chord_held("Ctrl+Shift+Tab", &[0x09, 0x10, 0x11]));
        assert!(peek_chord_held("F1+F2+F3", &[0x70, 0x71, 0x72]));
    }
    #[test]
    fn peek_rejects_missing_duplicate_or_oversized_chords() {
        for bad in [
            "P",
            "",
            "Z+Z",
            "F1+F1+F2",
            "A+B+C+D",
            "Ctrl+Shift+",
            "Ctrl+Alt+Shift+Tab",
            "Win+Space",
            "F25+F1",
            "A+?",
        ] {
            assert!(parse_peek_shortcut(bad).is_none(), "accepted {bad}");
        }
        assert!(describe_peek_keys(vec![0x5A, 0x58, 0x43, 0x11]).is_none());
    }
}

