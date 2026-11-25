use super::*;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Rule {
    pub mode: u8,
    pub pattern: String,
    pub target: u64,
    /// Stable tab ID, preserving rule destination when boxes are combined.
    /// Older saved rules target the first regular tab in their box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_tab: Option<u64>,
    pub enabled: bool,
}
impl Rule {
    pub fn matches(&self, item: &Item) -> bool {
        let pattern = fold(&self.pattern);
        let path = fold(&item.path);
        // Size/date predicates need a real file, not folder metadata or a stale pin.
        if (6..=9).contains(&self.mode) && !Path::new(&item.path).is_file() {
            return false;
        }
        self.enabled
            && !pattern.is_empty()
            && match self.mode {
                // A 'folder' token in legacy extension lists is also valid.
                // New rules should use explicit mode 10 (Folder), instead.
                0 => {
                    (item.kind == "folder"
                        && self
                            .pattern
                            .split(',')
                            .any(|part| part.trim().eq_ignore_ascii_case("folder")))
                        || (item.kind != "folder" && extension_matches(&item.name, &self.pattern))
                }
                1 | 2 | 3 | 5 => name_rule_matches(&item.name, &self.pattern, self.mode),
                10 => item.kind == "folder",
                4 => path.contains(&pattern),
                6 => pattern.parse::<u64>().is_ok_and(|n| item.size >= n),
                7 => pattern.parse::<u64>().is_ok_and(|n| item.size < n),
                8 => pattern
                    .parse::<u64>()
                    .is_ok_and(|days| modified_within(item.modified, now_seconds(), days)),
                9 => size_range(&pattern)
                    .is_some_and(|(lower, upper)| lower <= item.size && item.size <= upper),
                _ => false,
            }
    }
}
/// The rule list is strictly ordered: the topmost enabled rule that matches
/// (and whose destination still exists) wins, whatever kind of rule it is.
/// No rule kind outranks another, so the order shown in Settings alone decides.
pub(super) fn best_destination_from_rules(item: &Item, rules: &[(Rule, (u64, u64))]) -> Option<(u64, u64)> {
    rules
        .iter()
        .find(|(r, _)| r.matches(item))
        .map(|(_, dest)| *dest)
}
