use super::*;

impl State {
    /// Repair omitted starter rules without overwriting a user's chosen
    /// destination, disabled rule, or linked Folder View.
    pub fn ensure_builtin_sort_rules(&mut self) -> usize {
        let mut additions = Vec::new();
        for group in &self.groups {
            for tab in &group.tabs {
                if tab.folder.is_some() {
                    continue;
                }
                let name = tab.name.trim().to_ascii_lowercase();
                let template = if name == "folders" || name == "folder" {
                    Some((10, "folder"))
                } else if ["archives", "archive", "archives & installers"].contains(&name.as_str())
                {
                    Some((0, "zip,rar,7z,exe,msi,iso"))
                } else {
                    None
                };
                let Some((mode, pattern)) = template else {
                    continue;
                };
                let has_rule_for_tab = self.rules.iter().any(|rule| {
                    rule.target == group.id
                        && (rule.target_tab == Some(tab.id)
                            || rule.target_tab.is_none()
                                && group
                                    .tabs
                                    .iter()
                                    .find(|t| t.folder.is_none())
                                    .is_some_and(|t| t.id == tab.id))
                });
                let has_other_matching_rule = self.rules.iter().any(|rule| {
                    if mode == 10 {
                        rule.mode == 10
                            || (rule.mode == 0
                                && rule
                                    .pattern
                                    .split(',')
                                    .any(|s| s.trim().eq_ignore_ascii_case("folder")))
                    } else {
                        rule.mode == 0 && extension_matches("example.zip", &rule.pattern)
                    }
                });
                if !has_rule_for_tab && !has_other_matching_rule {
                    additions.push(Rule {
                        mode,
                        pattern: pattern.into(),
                        target: group.id,
                        target_tab: Some(tab.id),
                        enabled: true,
                    });
                }
            }
        }
        let available = MAX_RULES.saturating_sub(self.rules.len());
        let count = additions.len().min(available);
        self.rules.extend(additions.into_iter().take(available));
        count
    }
    /// The topmost enabled matching rule wins, exactly in the order shown in
    /// Settings (use Up to raise a rule). A rule whose box or tab no longer
    /// exists is skipped so the next matching rule can still apply.
    pub fn best_rule_destination(&self, item: &Item) -> Option<(u64, u64)> {
        self.rules
            .iter()
            .take(MAX_RULES)
            .filter(|rule| rule.matches(item))
            .find_map(|rule| self.rule_destination(rule))
    }
    /// Resolve a rule to a real regular tab (not merely the active box tab).
    pub fn rule_destination(&self, rule: &Rule) -> Option<(u64, u64)> {
        let group = self.group(rule.target)?;
        let tab = match rule.target_tab {
            Some(id) => group
                .tabs
                .iter()
                .find(|t| t.id == id && t.folder.is_none())?,
            None => group.tabs.iter().find(|t| t.folder.is_none())?,
        };
        Some((group.id, tab.id))
    }
    pub(crate) fn regular_tab_paths(&self) -> std::collections::HashSet<String> {
        self.groups.iter().flat_map(|group| &group.tabs)
            .filter(|tab| tab.folder.is_none())
            .flat_map(|tab| tab.items.iter().map(|item| item.path.to_lowercase()))
            .collect()
    }

    pub(crate) fn sort_loose_paths_by_rules(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Vec<String> {
        let mut grouped = self.regular_tab_paths();
        let mut added = Vec::new();
        for path in paths {
            let key = path.to_string_lossy().to_lowercase();
            if grouped.contains(&key) {
                continue;
            }
            let Ok(mut item) = Item::from_path(&path, 0) else { continue };
            let Some((group, tab_id)) = self.best_rule_destination(&item) else { continue };
            item.id = self.id();
            if let Some(tab) = self.group_mut(group).and_then(|g| {
                g.tabs.iter_mut().find(|tab| tab.id == tab_id && tab.folder.is_none())
            }) {
                added.push(item.path.clone());
                tab.items.push(item);
                grouped.insert(key);
            }
        }
        added
    }

    pub fn apply_rules(&mut self) -> usize {
        self.apply_rules_to_pins(|_| true).len()
    }

    pub(crate) fn apply_rules_to_pins(&mut self, include: impl Fn(&Item) -> bool) -> Vec<String> {
        let rules: Vec<_> = self
            .rules
            .iter()
            .take(MAX_RULES)
            .filter_map(|r| self.rule_destination(r).map(|dest| (r.clone(), dest)))
            .collect();
        let mut transfers = vec![];
        for group in &mut self.groups {
            for tab in &mut group.tabs {
                if tab.folder.is_none() {
                    tab.items.retain(|item| {
                        if !include(item) {
                            return true;
                        }
                        if let Some((dest_group, dest_tab)) =
                            best_destination_from_rules(item, &rules)
                        {
                            if dest_group != group.id || dest_tab != tab.id {
                                transfers.push((dest_group, dest_tab, item.clone()));
                                return false;
                            }
                        }
                        true
                    });
                }
            }
        }
        let mut moved = Vec::with_capacity(transfers.len());
        for (gid, tid, item) in transfers {
            if let Some(group) = self.group_mut(gid) {
                if let Some(tab) = group
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == tid && t.folder.is_none())
                {
                    moved.push(item.path.clone());
                    tab.items.push(item);
                }
            }
        }
        moved
    }
}
