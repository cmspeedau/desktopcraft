use super::*;

/// Implementation decision (2026-11-12): name and extension rules compare text
/// with simple Unicode lowercase folding, the same on every computer and
/// independent of the system or CRT locale. Scripts without case (Thai,
/// Chinese...) compare exactly. No Unicode normalization is applied.
pub fn fold(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// The text after the final dot of a file name ("" when there is none or the
/// only dot is the first character, as in ".env").
pub fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i + 1..],
        _ => "",
    }
}

/// The name without its final extension.
pub fn stem_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        Some(_) => "",
        None => name,
    }
}

/// Rule modes 1 contains, 2 starts with, 3 stem ends with, 5 does not contain.
pub fn name_rule_matches(name: &str, pattern: &str, mode: u8) -> bool {
    let pattern = fold(pattern);
    match mode {
        1 => fold(name).contains(&pattern),
        2 => fold(name).starts_with(&pattern),
        3 => fold(stem_of(name)).ends_with(&pattern),
        5 => !fold(name).contains(&pattern),
        _ => false,
    }
}

/// Extension rule: comma-separated list, optional leading dots, any case.
pub fn extension_matches(name: &str, list: &str) -> bool {
    let ext = fold(extension_of(name));
    !ext.is_empty()
        && list
            .split(',')
            .map(|p| fold(p.trim().trim_start_matches('.')))
            .any(|p| !p.is_empty() && p == ext)
}

pub(super) fn modified_within(modified: u64, now: u64, days: u64) -> bool {
    // Compare absolute file age against an upper bound in seconds.
    modified.abs_diff(now) < days.saturating_mul(86400)
}

// File size range endpoints are inclusive.
pub fn size_range(pattern: &str) -> Option<(u64, u64)> {
    let (lower, upper) = pattern.split_once(',')?;
    let lower = lower.trim().parse::<u64>().ok()?;
    let upper = upper.trim().parse::<u64>().ok()?;
    (lower <= upper).then_some((lower, upper))
}

/// Whether a file name passes a folder filter ("" shows everything).
pub fn filter_matches(name: &str, filter: &str) -> bool {
    let filter = filter.trim();
    filter.is_empty() || fold(name).contains(&fold(filter))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_and_final_extensions() {
        assert_eq!(extension_of("report.pdf.bak"), "bak");
        assert_eq!(extension_of(".env"), "");
        assert_eq!(stem_of(".env"), "");
        assert_eq!(stem_of("report.pdf.bak"), "report.pdf");
        assert_eq!(stem_of("README"), "README");
        assert!(extension_matches("Report.PDF", " .pdf, DOCX "));
        assert!(!extension_matches("report.pdf.bak", "pdf"));
        assert!(!extension_matches(".env", "env"));
    }

    #[test]
    fn unicode_and_rule_modes() {
        assert!(name_rule_matches("MÉMO.TXT", "mémo", 1));
        assert!(name_rule_matches("รายงาน.pdf", "รายงาน", 2));
        assert!(name_rule_matches("Quarterly.Brief.TXT", "brief", 3));
        assert!(!name_rule_matches("Quarterly.Brief.TXT", "txt", 3));
        assert!(name_rule_matches("report.pdf", "other", 5));
        assert!(!name_rule_matches("report.pdf", "report", 5));
        assert!(!name_rule_matches("report.pdf", "report", 42));
        assert!(extension_matches("ОТЧЁТ.ДОК", "док"));
        assert_eq!(fold("İSTANBUL"), "i̇stanbul");
        assert!(!name_rule_matches("Straße.txt", "STRASSE", 1));
    }

    #[test]
    fn filters_size_and_age_boundaries() {
        assert!(filter_matches("รายงาน.PDF", " PDF "));
        assert!(filter_matches("x", "   "));
        assert!(!filter_matches("x", "y"));
        assert_eq!(size_range(" 0, 10 "), Some((0, 10)));
        assert_eq!(size_range("10,10"), Some((10, 10)));
        assert_eq!(size_range("10,1"), None);
        assert_eq!(size_range("0,1,2"), None);
        assert_eq!(size_range("-1,10"), None);
        assert!(modified_within(1, 86400, 1));
        assert!(!modified_within(0, 86400, 1));
        assert!(modified_within(86400, 1, 1));
        assert!(!modified_within(1, 1, 0));
    }
}
