use super::*;

/// The REAL Windows Desktop keeps a stable Explorer callback generation
/// through mouse capture/context menu. Only an actual Tab/Page/layout
/// transition publishes a new generation and schedules a repaint.
/// While icons are hidden, the Explorer filter blanks their ListView label
/// (LVN_GETDISPINFO). Accessibility then reports an empty name, or skips the
/// item. Every visible index must still carry its exact verified name.
pub(super) fn redacted_names_match(current:&[String],verified:&[String],hidden:&[usize])->bool{
    if current.len()==verified.len() {
        return current.iter().zip(verified).enumerate().all(|(index,(now,known))|
            now==known || (now.is_empty() && hidden.contains(&index)));
    }
    let visible=verified.iter().enumerate()
        .filter(|(index,_)|!hidden.contains(index)).map(|(_,name)|name);
    current.len()+hidden.len()==verified.len() && current.iter().eq(visible)
}

#[cfg(test)]
mod redacted_name_tests {
    use super::redacted_names_match;
    fn names(list:&[&str])->Vec<String>{list.iter().map(|s|s.to_string()).collect()}

    #[test]
    fn hidden_labels_may_be_blank_or_skipped_but_visible_ones_must_match(){
        let verified=names(&["A","B","C"]);
        assert!(redacted_names_match(&names(&["A","","C"]),&verified,&[1]));
        assert!(redacted_names_match(&names(&["A","C"]),&verified,&[1]));
        assert!(redacted_names_match(&verified,&verified,&[1]));
        assert!(!redacted_names_match(&names(&["A","","C"]),&verified,&[0]));
        assert!(!redacted_names_match(&names(&["C","A"]),&verified,&[1]));
        assert!(!redacted_names_match(&names(&["A","X","C"]),&verified,&[1]));
    }
}

pub(super) fn reuse_native_generation(real_desktop:bool,current:Option<u32>,
                           published:Option<u32>)->bool{
    real_desktop && current.is_some() && current==published
}

pub(super) fn inventory_refresh_publish_hidden(
    planned_hidden: &[usize],
    mapped: &[ViewItem],
    is_parked: impl Fn(&str) -> bool,
) -> Vec<usize> {
    let mut hidden = planned_hidden.to_vec();
    for (index, item) in mapped.iter().enumerate() {
        if is_parked(&item.path) {
            hidden.push(index);
        }
    }
    hidden.sort_unstable();
    hidden.dedup();
    hidden
}

pub(super) fn inventory_refresh_removed_affects_active_box(
    state: &State,
    removed: &[String],
    mapped: &[ViewItem],
    planned_hidden: &[usize],
    is_parked: impl Fn(&str) -> bool,
) -> (bool, Vec<usize>) {
    let removed_keys: HashSet<String> = removed.iter().map(|p| p.to_lowercase()).collect();
    let mut unhidden_parked = Vec::new();
    for (index, item) in mapped.iter().enumerate() {
        if is_parked(&item.path) && planned_hidden.binary_search(&index).is_err() {
            unhidden_parked.push(index);
        }
    }
    let mut bridge_paths = HashSet::new();
    let mut hit_active_box = false;
    for group in state.groups.iter().filter(|g| g.page == state.page && !g.rolled) {
        let Some(tab) = group.tabs.get(group.active) else { continue };
        if tab.folder.is_some() {
            continue;
        }
        if tab.items.iter().any(|item| removed_keys.contains(&item.path.to_lowercase())) {
            hit_active_box = true;
            for item in &tab.items {
                bridge_paths.insert(item.path.to_lowercase());
            }
        }
    }
    if !hit_active_box && unhidden_parked.is_empty() {
        return (false, Vec::new());
    }
    let mut bridge_indices = unhidden_parked;
    for (index, item) in mapped.iter().enumerate() {
        if bridge_paths.contains(&item.path.to_lowercase()) {
            bridge_indices.push(index);
        }
    }
    bridge_indices.sort_unstable();
    bridge_indices.dedup();
    (true, bridge_indices)
}

pub(super) fn merge_inventory_deltas(
    pending_added: &mut Vec<ViewItem>,
    pending_removed: &mut Vec<String>,
    outcome: &crate::native_box_live_layout::InventoryReconcileOutcome,
) {
    for path in &outcome.removed {
        let key = path.to_lowercase();
        let before = pending_added.len();
        pending_added.retain(|item| item.path.to_lowercase() != key);
        if pending_added.len() == before
            && !pending_removed.iter().any(|existing| existing.to_lowercase() == key)
        {
            pending_removed.push(path.clone());
        }
    }
    for item in &outcome.added {
        let key = item.path.to_lowercase();
        pending_removed.retain(|existing| existing.to_lowercase() != key);
        if let Some(slot) = pending_added.iter_mut().find(|existing| existing.path.to_lowercase() == key) {
            *slot = item.clone();
        } else {
            pending_added.push(item.clone());
        }
    }
}

pub(super) fn cancel_pending_background_reflow(
    staged_hide: &mut Option<Vec<Vec<usize>>>,
    staged_tab_selections: &mut Option<Vec<Vec<(u64, usize)>>>,
    current_stage: Option<u32>,
    ui_next_stage: &mut usize,
    hold_workspace_paint: &mut bool,
    native_move_event: &mut Option<&'static str>,
    extra_bridge_hide: &mut Vec<usize>,
) {
    if let (Some(stages), Some(tab_sels), Some(cur)) =
        (staged_hide.as_mut(), staged_tab_selections.as_mut(), current_stage)
    {
        let cur = cur as usize;
        if *ui_next_stage != cur && cur < stages.len() {
            stages.truncate(cur + 1);
            tab_sels.truncate(cur + 1);
            *ui_next_stage = cur;
            *hold_workspace_paint = false;
            *native_move_event = None;
            extra_bridge_hide.clear();
        }
    }
}

pub(super) fn should_defer_idle_poll(
    real_native_desktop: bool,
    current_stage_ready: bool,
    no_pending_ack: bool,
    has_queued_commands: bool,
    inventory_retrying: bool,
    recent_external_drop: bool,
) -> bool {
    real_native_desktop
        && current_stage_ready
        && no_pending_ack
        && has_queued_commands
        && !inventory_retrying
        && !recent_external_drop
}

#[cfg(test)]
mod native_generation_lease_tests{
    use super::*;
    #[test]
    fn stable_real_desktop_stage_renews_lease_without_republish(){
        assert!(reuse_native_generation(true,Some(1),Some(1)));
        assert!(reuse_native_generation(true,Some(12),Some(12)));
        assert!(!reuse_native_generation(true,Some(2),Some(1)));
        assert!(!reuse_native_generation(true,Some(1),None));
        assert!(!reuse_native_generation(true,None,None));
        assert!(!reuse_native_generation(false,Some(1),Some(1)));
    }

    #[test]
    fn should_defer_idle_poll_prioritizes_queued_ui_commands_unless_inventory_busy() {
        assert!(should_defer_idle_poll(true, true, true, true, false, false));
        assert!(!should_defer_idle_poll(false, true, true, true, false, false));
        assert!(!should_defer_idle_poll(true, false, true, true, false, false));
        assert!(!should_defer_idle_poll(true, true, false, true, false, false));
        assert!(!should_defer_idle_poll(true, true, true, false, false, false));
        assert!(!should_defer_idle_poll(true, true, true, true, true, false));
        assert!(!should_defer_idle_poll(true, true, true, true, false, true));
    }

    #[test]
    fn inventory_refresh_keeps_parked_overflow_masked_until_reflow() {
        let view = |path: &str, x, y| ViewItem {
            path: path.into(),
            label: path.into(),
            pos: (x, y),
        };
        let mapped = vec![
            view("C:/Desktop/Visible.lnk", 100, 100),
            view("C:/Desktop/ParkedNowVisible.lnk", 2000, 1200),
            view("C:/Desktop/StillHidden.lnk", 2000, 1200),
        ];
        let planned_hidden = vec![2usize];
        let publish = inventory_refresh_publish_hidden(&planned_hidden, &mapped, |path| {
            path.eq_ignore_ascii_case("C:/Desktop/ParkedNowVisible.lnk")
                || path.eq_ignore_ascii_case("C:/Desktop/StillHidden.lnk")
        });
        assert_eq!(publish, vec![1, 2]);
    }

    #[test]
    fn inventory_refresh_detects_active_box_removals_and_unhidden_parked_icons() {
        use crate::model::{Bounds, Item};
        let mut state = State::empty();
        let active_id = state.add_group("BoxA", Bounds { x: 20., y: 20., w: 300., h: 240. });
        state.add_tab(active_id).unwrap();
        let rolled_id = state.add_group("BoxB", Bounds { x: 360., y: 20., w: 300., h: 240. });
        state.group_mut(rolled_id).unwrap().rolled = true;
        let item = |id, path: &str| Item {
            id,
            name: path.into(),
            path: path.into(),
            kind: "lnk".into(),
            size: 0,
            modified: 0,
            label: None,
        };
        state.group_mut(active_id).unwrap().active = 0;
        state.group_mut(active_id).unwrap().tabs[0].items = vec![
            item(1, "C:/Desktop/Keep.lnk"),
            item(2, "C:/Desktop/DeletedInBox.lnk"),
        ];
        state.group_mut(active_id).unwrap().tabs[1].items = vec![
            item(3, "C:/Desktop/InactiveTab.lnk"),
        ];
        state.group_mut(rolled_id).unwrap().tabs[0].items = vec![
            item(4, "C:/Desktop/InRolledBox.lnk"),
        ];
        let view = |path: &str| ViewItem { path: path.into(), label: path.into(), pos: (0, 0) };
        let mapped = vec![
            view("C:/Desktop/Keep.lnk"),
            view("C:/Desktop/InactiveTab.lnk"),
            view("C:/Desktop/InRolledBox.lnk"),
            view("C:/Desktop/OverflowUnhidden.lnk"),
        ];

        assert_eq!(
            inventory_refresh_removed_affects_active_box(
                &state,
                &["C:/Desktop/DeletedInBox.lnk".into()],
                &mapped,
                &[1, 2],
                |p| p.eq_ignore_ascii_case("C:/Desktop/OverflowUnhidden.lnk"),
            ),
            (true, vec![0, 3])
        );
        assert_eq!(
            inventory_refresh_removed_affects_active_box(
                &state,
                &["C:/Desktop/LooseOrOtherTab.lnk".into()],
                &mapped,
                &[1, 2, 3],
                |_| false,
            ),
            (false, Vec::new())
        );
        assert_eq!(
            inventory_refresh_removed_affects_active_box(
                &state,
                &["C:/Desktop/LooseOrOtherTab.lnk".into()],
                &mapped,
                &[1, 2],
                |p| p.eq_ignore_ascii_case("C:/Desktop/OverflowUnhidden.lnk"),
            ),
            (true, vec![3])
        );
    }

    #[test]
    fn merge_inventory_deltas_preserves_and_cancels_transient_add_remove_bursts() {
        use crate::native_box_live_layout::InventoryReconcileOutcome;
        let view = |path: &str, x, y| ViewItem {
            path: path.into(),
            label: path.into(),
            pos: (x, y),
        };
        let mut added = Vec::new();
        let mut removed = Vec::new();
        merge_inventory_deltas(
            &mut added,
            &mut removed,
            &InventoryReconcileOutcome {
                added: vec![view("C:/Desktop/NewA.pdf", 10, 10)],
                removed: vec!["C:/Desktop/OldPin.lnk".into()],
                moved_loose: Vec::new(),
            },
        );
        merge_inventory_deltas(
            &mut added,
            &mut removed,
            &InventoryReconcileOutcome {
                added: vec![view("C:/Desktop/NewB.pdf", 20, 20)],
                removed: vec!["C:/Desktop/NewA.pdf".into()],
                moved_loose: Vec::new(),
            },
        );
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].path, "C:/Desktop/NewB.pdf");
        assert_eq!(removed, vec!["C:/Desktop/OldPin.lnk"]);
    }

    #[test]
    fn cancel_pending_background_reflow_resets_staged_slot_only_when_queued() {
        let mut stages = Some(vec![vec![1, 2], vec![2]]);
        let mut selections = Some(vec![vec![(10, 0)], vec![(10, 1)]]);
        let mut ui_next = 1usize;
        let mut hold = true;
        let mut move_event = Some("native_desktop.delete_reflow_move_batch");
        let mut bridge = vec![0usize, 3usize];

        cancel_pending_background_reflow(
            &mut stages,
            &mut selections,
            Some(0),
            &mut ui_next,
            &mut hold,
            &mut move_event,
            &mut bridge,
        );
        assert_eq!(stages.as_ref().unwrap(), &vec![vec![1, 2]]);
        assert_eq!(selections.as_ref().unwrap(), &vec![vec![(10, 0)]]);
        assert_eq!(ui_next, 0);
        assert!(!hold);
        assert!(move_event.is_none());
        assert!(bridge.is_empty());

        // No-op when ui_next_stage already equals current_stage.
        let mut stable_bridge = vec![9usize];
        cancel_pending_background_reflow(
            &mut stages,
            &mut selections,
            Some(0),
            &mut ui_next,
            &mut hold,
            &mut move_event,
            &mut stable_bridge,
        );
        assert_eq!(stable_bridge, vec![9]);
    }
}
