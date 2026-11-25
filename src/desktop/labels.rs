use super::*;

/// Thai vowel/tone marks must remain attached to their preceding base glyph.
pub fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        | 0x0300..=0x036F | 0x1AB0..=0x1AFF)
}

/// Up to two lines, wrap long Thai names without requiring spaces.
pub fn wrap_label(p: &Painter, label: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::<String>::new();
    let mut current = String::new();
    // On long unspaced Thai names, wrap by codepoint clusters while retaining
    // combining marks; on English names, spaces stay attached naturally.
    for c in label.chars() {
        let trial = format!("{current}{c}");
        if !current.is_empty() && !is_combining_mark(c) && p.measure(&trial, size) > width {
            lines.push(std::mem::take(&mut current));
        }
        current.push(c);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    let mut out: Vec<String> = lines
        .iter()
        .take(2)
        .map(|line| p.fit(line.trim_start(), size, width))
        .collect();
    if lines.len() > 2 {
        if let Some(last) = out.last_mut() {
            *last = p.fit(&format!("{}…", last.trim_end_matches('…')), size, width);
        }
    }
    out
}

