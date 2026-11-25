use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleSortScope {
    LooseOnly,
    AllIncludingPins,
}

pub(crate) fn format_pin_destinations(before: &State, after: &State) -> String {
    let mut prior: HashMap<String, (u64, u64)> = HashMap::new();
    for group in &before.groups {
        for tab in group.tabs.iter().filter(|t| t.folder.is_none()) {
            for item in &tab.items {
                prior.insert(item.path.to_lowercase(), (group.id, tab.id));
            }
        }
    }
    let mut labels = Vec::new();
    for group in &after.groups {
        for tab in group.tabs.iter().filter(|t| t.folder.is_none()) {
            let changed_here = tab.items.iter().any(|item| {
                prior.get(&item.path.to_lowercase()) != Some(&(group.id, tab.id))
            });
            if changed_here {
                let label = if group.tabs.len() > 1 {
                    format!("{} / {}", group.tabs[0].name, tab.name)
                } else {
                    tab.name.clone()
                };
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }
    }
    if labels.len() > 3 {
        let head = labels[..3].join(", ");
        format!("{head}, ...")
    } else {
        labels.join(", ")
    }
}

pub(crate) fn format_rule_sort_toast(count: usize, scope: RuleSortScope, dests: &str) -> String {
    let noun = if scope == RuleSortScope::LooseOnly {
        "loose icon(s)"
    } else {
        "icon(s)"
    };
    if dests.is_empty() {
        format!("Organized {count} {noun} by Rules (files unchanged). Ctrl+Z to undo.")
    } else {
        format!("Organized {count} {noun} into {dests} (files unchanged). Ctrl+Z to undo.")
    }
}


impl App {
    /// Assign only currently loose desktop files to matching regular tabs.
    /// Manually placed pins stay where they are, and no disk file moves.
    pub fn sort_loose_desktop_now(&mut self) -> usize {
        let count = self.sort_paths_by_rules(shell::desktop_entries());
        if count > 0 {
            // Update the overlay immediately while Explorer reconciliation
            // happens on its normal background cadence.
            let grouped = self.regular_tab_paths();
            if let Some(host) = self.host.as_mut() {
                host.loose.retain(|icon| !grouped.contains(&icon.key));
                host.last_sync = -10.;
            }
        }
        count
    }
    pub(super) fn sort_paths_by_rules(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        self.state.sort_loose_paths_by_rules(paths).len()
    }
    /// Settings' explicit Apply rules reassigns existing pins too.
    pub fn sort_desktop_now(&mut self) -> usize {
        self.sort_loose_desktop_now() + self.state.apply_rules()
    }

    pub(crate) fn request_desktop_rule_sort(&mut self, scope: RuleSortScope, time: f64) {
        if self.native_env.chrome_only && self.native_env.general_workspace && !self.automation
            && self.native_tx_inflight
        {
            self.notify("Explorer is still applying the previous change; please wait a moment.", time);
            return;
        }
        if !self.state.rules.iter().any(|rule| rule.enabled && self.state.rule_destination(rule).is_some()) {
            self.notify("No enabled Rules. Add sorting rules in Settings first.", time);
            return;
        }
        if self.native_env.chrome_only && self.native_env.general_workspace && !self.automation {
            self.native_rules_sort_requested = Some(scope);
            self.native_tx_inflight = true;
            self.classic_file_menu = None;
            self.clear_native_scroll_state();
            return;
        }
        let before = self.state.clone();
        self.remember();
        self.multi_selected.clear();
        let count = match scope {
            RuleSortScope::LooseOnly => self.sort_loose_desktop_now(),
            RuleSortScope::AllIncludingPins => self.sort_desktop_now(),
        };
        if count == 0 {
            self.undo.pop();
            self.notify(
                if scope == RuleSortScope::LooseOnly {
                    "No loose desktop icons match the enabled Rules (icons already in Boxes stay where they are)."
                } else {
                    "No icons match the enabled Rules."
                },
                time,
            );
        } else {
            let dests = format_pin_destinations(&before, &self.state);
            self.notify(format_rule_sort_toast(count, scope, &dests), time);
        }
    }

}
