use crate::*;

/// Sorting rule kinds, in `Rule::mode` order.
pub(crate) const RULE_MODES: [&str; 11] = [
    "File extensions",
    "Name contains",
    "Name starts with",
    "Name ends with (no extension)",
    "Path contains",
    "Name does not contain",
    "Size at least (bytes)",
    "Size below (bytes)",
    "Modified within (days)",
    "Size between (bytes)",
    "Folder (any name)",
];
/// The label above a rule's match field, for its kind.
pub(super) fn rule_field_label(mode: u8) -> &'static str {
    match mode {
        3 => "Ending before the extension",
        6 | 7 => "File size in bytes",
        8 => "Days",
        9 => "Minimum,maximum bytes (inclusive)",
        _ => "Match (example: txt,csv)",
    }
}

impl App {
    /// Every regular tab a rule can send icons to: (box, tab, label).
    pub(crate) fn rule_destinations(&self) -> Vec<(u64, u64, String)> {
        self.state
            .groups
            .iter()
            .flat_map(|g| {
                g.tabs
                    .iter()
                    .filter(|tab| tab.folder.is_none())
                    .map(move |tab| {
                        let label = if g.tabs.len() > 1 {
                            format!("{} / {}", g.tabs[0].name, tab.name)
                        } else {
                            tab.name.clone()
                        };
                        (g.id, tab.id, label)
                    })
            })
            .collect()
    }
    /// The pop-up editor for rule `index`, filled in with its current values.
    pub(crate) fn rule_editor(&self, index: usize) -> Dialog {
        let rule = &self.state.rules[index];
        let dest = self.state.rule_destination(rule);
        let target = self
            .rule_destinations()
            .iter()
            .position(|(g, t, _)| dest == Some((*g, *t)))
            .unwrap_or(0);
        Dialog::EditRule {
            index,
            mode: rule.mode.min(RULE_MODES.len() as u8 - 1),
            pattern: rule.pattern.clone(),
            target,
        }
    }
}
