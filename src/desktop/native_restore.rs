use super::*;

#[derive(Clone)]
pub(crate) struct PendingNativeLayoutRestore {
    pub(crate) target: State,
    pub(crate) record_undo: bool,
    pub(crate) undo_snapshot: Option<State>,
    pub(crate) restore_undo_on_fail: Option<State>,
    pub(crate) success_toast: String,
}

pub(crate) fn validate_native_structural_edit(
    old: &State,
    next: &State,
    check_geometry_and_group_page: bool,
) -> Result<(), String> {
    if next.pages.is_empty() || next.page >= next.pages.len() {
        return Err("Native Desktop Workspace page selection is invalid".into());
    }
    let collect_pin_tabs = |state: &State| -> Result<HashMap<u64, Vec<(u64, String)>>, String> {
        let mut seen_groups = HashSet::with_capacity(state.groups.len());
        let mut seen_tabs = HashSet::new();
        let mut pin_tabs = HashMap::new();
        for group in &state.groups {
            if !seen_groups.insert(group.id) {
                return Err("Native Desktop Box or Tab structure changed".into());
            }
            for tab in &group.tabs {
                if !seen_tabs.insert(tab.id) {
                    return Err("Native Desktop Box or Tab structure changed".into());
                }
                if tab.folder.is_none() && !tab.items.is_empty() {
                    let mut pins: Vec<(u64, String)> = tab
                        .items
                        .iter()
                        .map(|item| (item.id, item.path.to_lowercase()))
                        .collect();
                    pins.sort();
                    pin_tabs.insert(tab.id, pins);
                }
            }
        }
        Ok(pin_tabs)
    };
    let old_pin_tabs = collect_pin_tabs(old)?;
    let next_pin_tabs = collect_pin_tabs(next)?;
    for current in &next.groups {
        if current.tabs.is_empty() || current.active >= current.tabs.len() {
            return Err("Native Desktop Box or Tab structure changed".into());
        }
        if check_geometry_and_group_page {
            if current.page >= next.pages.len() {
                return Err("Native Desktop Box or Tab structure changed".into());
            }
            let b = current.bounds;
            if ![b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite()) || b.w <= 0. || b.h <= 0. {
                return Err("Native Desktop Box geometry is invalid".into());
            }
        }
    }
    if old_pin_tabs != next_pin_tabs {
        return Err(
            "Native Desktop Box Pin ownership or membership transfer requires a verified Shell transaction"
                .into(),
        );
    }
    let old_has_regular = old
        .groups
        .iter()
        .any(|g| g.tabs.iter().any(|t| t.folder.is_none()));
    let next_has_regular = next
        .groups
        .iter()
        .any(|g| g.tabs.iter().any(|t| t.folder.is_none()));
    if old_has_regular && !next_has_regular {
        return Err(
            "Keep at least one regular Desktop Box on your Native Desktop".into(),
        );
    }
    Ok(())
}

pub(crate) fn native_pins_differ(a: &State, b: &State) -> bool {
    validate_native_structural_edit(a, b, false).is_err()
}

pub(crate) fn apply_non_pin_layout_from(current: &mut State, target: &State) {
    if !current.pages.is_empty() {
        current.page = target.page.min(current.pages.len() - 1);
    }
    for (group, goal) in current.groups.iter_mut().zip(&target.groups) {
        if group.id != goal.id || group.tabs.len() != goal.tabs.len() {
            continue;
        }
        group.bounds = goal.bounds;
        group.page = goal.page;
        group.active = goal.active.min(group.tabs.len().saturating_sub(1));
        group.color = goal.color;
        group.opacity = goal.opacity;
        group.rolled = goal.rolled;
        group.dock = goal.dock;
        group.locked = goal.locked;
        group.keep_visible = goal.keep_visible;
        group.custom_color = goal.custom_color;
        for (tab, goal_tab) in group.tabs.iter_mut().zip(&goal.tabs) {
            if tab.id != goal_tab.id {
                continue;
            }
            tab.name = goal_tab.name.clone();
            tab.sort = goal_tab.sort;
            tab.list = goal_tab.list;
            tab.icon_scale = goal_tab.icon_scale;
            if tab.folder.is_some() && goal_tab.folder.is_some() {
                tab.folder = goal_tab.folder.clone();
                tab.items = goal_tab.items.clone();
                tab.history = goal_tab.history.clone();
                tab.scroll = goal_tab.scroll;
                tab.filter = goal_tab.filter.clone();
            }
        }
    }
}

pub(crate) fn native_layout_structure_matches(a: &State, b: &State) -> bool {
    if a.pages != b.pages || a.groups.len() != b.groups.len() {
        return false;
    }
    for (ga, gb) in a.groups.iter().zip(&b.groups) {
        if ga.id != gb.id || ga.page != gb.page || ga.tabs.len() != gb.tabs.len() {
            return false;
        }
        for (ta, tb) in ga.tabs.iter().zip(&gb.tabs) {
            if ta.id != tb.id || ta.folder.is_some() != tb.folder.is_some() {
                return false;
            }
        }
    }
    true
}

pub(crate) fn native_regular_pins_match(a: &State, b: &State) -> bool {
    if !native_layout_structure_matches(a, b) {
        return false;
    }
    for (ga, gb) in a.groups.iter().zip(&b.groups) {
        for (ta, tb) in ga.tabs.iter().zip(&gb.tabs) {
            if ta.folder.is_some() {
                continue;
            }
            if ta.items.len() != tb.items.len()
                || ta.items.iter().zip(&tb.items).any(|(x, y)| {
                    x.id != y.id || x.path.to_lowercase() != y.path.to_lowercase()
                })
            {
                return false;
            }
        }
    }
    true
}

pub(crate) fn stage1_unpin_incompatible_tabs(current: &State, target: &State) -> Option<State> {
    let target_regular_tabs: HashSet<u64> = target
        .groups
        .iter()
        .flat_map(|g| &g.tabs)
        .filter(|t| t.folder.is_none())
        .map(|t| t.id)
        .collect();
    let mut cleared = false;
    let mut stage1 = current.clone();
    for group in &mut stage1.groups {
        for tab in &mut group.tabs {
            if tab.folder.is_none()
                && !tab.items.is_empty()
                && !target_regular_tabs.contains(&tab.id)
            {
                tab.items.clear();
                cleared = true;
            }
        }
    }
    cleared.then_some(stage1)
}

pub(crate) fn stage2_apply_target_structure_preserving_pins(
    current: &State,
    target: &State,
) -> State {
    let mut curr_pins: HashMap<u64, Vec<Item>> = HashMap::new();
    for group in &current.groups {
        for tab in &group.tabs {
            if tab.folder.is_none() && !tab.items.is_empty() {
                curr_pins.insert(tab.id, tab.items.clone());
            }
        }
    }
    let mut stage2 = target.clone();
    stage2.next_id = current.next_id.max(target.next_id);
    if !stage2.pages.is_empty() {
        stage2.page = stage2.page.min(stage2.pages.len() - 1);
    }
    for group in &mut stage2.groups {
        for tab in &mut group.tabs {
            if tab.folder.is_none() {
                tab.items = curr_pins.remove(&tab.id).unwrap_or_default();
            }
        }
    }
    stage2
}

impl App {
    pub(crate) fn clear_native_scroll_state(&mut self) {
        self.native_scroll_target = None;
        self.native_page_request = None;
        self.native_wheel_remainder = (0, 0);
    }

    pub(crate) fn request_restore_snapshot(&mut self, index: usize, time: f64) {
        if self.native_env.chrome_only && self.native_env.general_workspace && !self.automation {
            if self.native_tx_inflight
                || self.native_layout_restore_requested.is_some()
                || self.native_pin_paths_requested.is_some()
            {
                self.notify("Explorer is still applying the previous change; please wait a moment.", time);
                return;
            }
            let mut target = self.state.clone();
            if !target.restore(index) {
                return;
            }
            if !target
                .groups
                .iter()
                .any(|g| g.tabs.iter().any(|t| t.folder.is_none()))
            {
                self.notify(
                    "Cannot restore a layout without any regular Desktop Box.",
                    time,
                );
                return;
            }
            self.native_layout_restore_requested = Some(PendingNativeLayoutRestore {
                target,
                record_undo: true,
                undo_snapshot: Some(self.state.clone()),
                restore_undo_on_fail: None,
                success_toast: "Layout restored. Ctrl + Z undoes this change.".into(),
            });
            self.native_tx_inflight = true;
            self.classic_file_menu = None;
            self.clear_native_scroll_state();
            return;
        }
        self.remember();
        if self.state.restore(index) {
            self.selected = None;
            self.notify("Layout restored. Ctrl + Z undoes this change.", time);
        } else {
            self.undo.pop();
        }
    }

    pub(crate) fn request_undo(&mut self, time: f64) {
        let is_native = self.native_env.chrome_only
            && self.native_env.general_workspace
            && !self.automation;
        if is_native
            && (self.native_tx_inflight
                || self.drag.is_some()
                || self.native_layout_restore_requested.is_some()
                || self.native_pin_paths_requested.is_some())
        {
            return;
        }
        let Some(mut target) = self.undo.pop() else {
            return;
        };
        if is_native {
            target.next_id = target.next_id.max(self.state.next_id);
            if !native_pins_differ(&self.state, &target) {
                self.native_undo_inflight = Some(target.clone());
                self.native_tx_inflight = true;
                self.state = target;
                self.revealed = None;
                self.hover_rolled = None;
                self.selected = None;
                self.multi_selected.clear();
                self.classic_file_menu = None;
                self.clear_native_scroll_state();
                self.notify("Last change undone", time);
            } else {
                self.native_layout_restore_requested = Some(PendingNativeLayoutRestore {
                    target: target.clone(),
                    record_undo: false,
                    undo_snapshot: None,
                    restore_undo_on_fail: Some(target),
                    success_toast: "Last change undone".into(),
                });
                self.native_tx_inflight = true;
                self.selected = None;
                self.multi_selected.clear();
                self.classic_file_menu = None;
                self.clear_native_scroll_state();
            }
            return;
        }
        self.state = target;
        self.selected = None;
        self.multi_selected.clear();
        self.notify("Last change undone", time);
    }

    pub(crate) fn native_undo_shortcut_allowed(&self) -> bool {
        !self.undo.is_empty()
            && !self.peek
            && !self.shortcut_recording
            && self.drag.is_none()
            && self.inline_rename.is_none()
            && self.filter_focus.is_none()
            && !self.search_focus
            && self.menu.is_none()
            && self.folder_sort_menu.is_none()
            && (self.dialog.is_none()
                || matches!(
                    &self.dialog,
                    Some(Dialog::Settings { page, pattern, .. }) if *page != 2 || pattern.is_empty()
                ))
    }

    pub(crate) fn request_remove_group(&mut self, group_id: u64, time: f64) {
        if !self
            .state
            .groups
            .iter()
            .any(|g| g.id == group_id)
        {
            return;
        }
        if self.native_env.chrome_only && self.native_env.general_workspace && !self.automation {
            if self.native_tx_inflight
                || self.drag.is_some()
                || self.native_layout_restore_requested.is_some()
                || self.native_pin_paths_requested.is_some()
            {
                self.notify(
                    "Explorer is still applying the previous change; please wait a moment.",
                    time,
                );
                return;
            }
            let remaining_regular = self
                .state
                .groups
                .iter()
                .filter(|g| g.id != group_id)
                .flat_map(|g| &g.tabs)
                .filter(|t| t.folder.is_none())
                .count();
            if remaining_regular == 0 {
                self.notify("Keep at least one regular Desktop Box.", time);
                return;
            }
            let mut target = self.state.clone();
            target.groups.retain(|g| g.id != group_id);
            if validate_native_structural_edit(&self.state, &target, true).is_ok() {
                self.remember();
                self.state = target;
                self.selected = None;
                self.multi_selected.clear();
                self.notify("Group removed; its files are kept", time);
            } else {
                self.native_layout_restore_requested = Some(PendingNativeLayoutRestore {
                    target,
                    record_undo: true,
                    undo_snapshot: Some(self.state.clone()),
                    restore_undo_on_fail: None,
                    success_toast: "Group removed; its files remain on Desktop".into(),
                });
                self.native_tx_inflight = true;
                self.selected = None;
                self.multi_selected.clear();
                self.classic_file_menu = None;
                self.clear_native_scroll_state();
            }
            return;
        }
        self.remember();
        self.state.groups.retain(|g| g.id != group_id);
        self.selected = None;
        self.notify("Group removed; its files are kept", time);
    }

}
