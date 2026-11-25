use super::*;

/// ACK-backed, debug-only real Explorer two-Box UI bridge. The controller
/// never accepts a click as committed until the Explorer policy publishes.
/// Geometry and visibility updates use the same checked placement and
/// recovery journal. Pin membership still requires a verified drag permit.
pub(super) fn validate_native_workspace_edit(old:&State,next:&State)
    ->Result<(),String>{
    crate::desktop::validate_native_structural_edit(old, next, true)
}

pub(super) fn is_native_box_paint_transition(old: &State, next: &State) -> bool {
    let regular = |group: &crate::model::Group| {
        group.tabs.get(group.active).is_some_and(|tab| tab.folder.is_none())
    };
    old.page == next.page && next.groups.iter().any(|group| {
        group.page == next.page && !group.rolled
            && old.group(group.id).is_some_and(|prior| {
                prior.page == old.page && !prior.rolled
                    && ((prior.active != group.active && (regular(prior) || regular(group)))
                        || (prior.bounds != group.bounds && regular(prior) && regular(group)))
            })
    })
}

pub(super) fn is_native_icon_tab_switch(old: &State, next: &State) -> bool {
    if old.page != next.page || old.groups.len() != next.groups.len() {
        return false;
    }
    let mut switched = false;
    for (prior, current) in old.groups.iter().zip(&next.groups) {
        if prior == current {
            continue;
        }
        let (Some(from), Some(to)) = (prior.tabs.get(prior.active), current.tabs.get(current.active)) else {
            return false;
        };
        if switched || prior.active == current.active || current.rolled
            || current.page != next.page || (from.folder.is_some() && to.folder.is_some()) {
            return false;
        }
        let mut normalized = current.clone();
        normalized.active = prior.active;
        if from.folder.is_some() || to.folder.is_some() {
            for (tab, old_tab) in normalized.tabs.iter_mut().zip(&prior.tabs) {
                // Refreshing Portal contents does not change Desktop placement.
                if tab.id == old_tab.id && tab.folder.is_some() && tab.folder == old_tab.folder {
                    tab.clone_from(old_tab);
                }
            }
        }
        if &normalized != prior {
            return false;
        }
        switched = true;
    }
    switched
}

pub(super) fn is_regular_box_drop(old: &State, next: &State, group: u64) -> bool {
    let (Some(prior), Some(current)) = (old.group(group), next.group(group)) else {
        return false;
    };
    if old.page != next.page || current.page != next.page || current.rolled
        || current.bounds == prior.bounds
        || !current.tabs.get(current.active).is_some_and(|tab| tab.folder.is_none()) {
        return false;
    }
    let mut normalized = current.clone();
    normalized.bounds = prior.bounds;
    &normalized == prior && old.groups.len() == next.groups.len()
        && old.groups.iter().zip(&next.groups).all(|(old, new)| {
            old.id == new.id && (old.id == group || old == new)
        })
}

pub(super) fn is_regular_rollup_transition(old: &State, next: &State) -> bool {
    if old.page != next.page || old.groups.len() != next.groups.len() {
        return false;
    }
    let mut rolled_changed = false;
    for (prior, current) in old.groups.iter().zip(&next.groups) {
        if prior == current {
            continue;
        }
        if current.page != next.page || prior.rolled == current.rolled {
            return false;
        }
        let mut normalized = current.clone();
        normalized.rolled = prior.rolled;
        if &normalized != prior {
            return false;
        }
        rolled_changed = true;
    }
    rolled_changed
}

pub(crate) fn state_with_rolled_overrides(
    effective: &State,
    overrides: &[(u64, bool)],
) -> State {
    let mut restored = effective.clone();
    for &(id, rolled) in overrides {
        if let Some(group) = restored.group_mut(id) {
            group.rolled = rolled;
        }
    }
    restored
}

pub(super) fn pin_drop_bridge_hide_indices(
    old: &State,
    next: &State,
    drop_paths: &[String],
    native_order: &[crate::shell::ViewItem],
) -> Vec<usize> {
    let mut affected_paths = drop_paths
        .iter()
        .map(|p| p.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    for (prior, current) in old.groups.iter().zip(&next.groups) {
        if prior.id != current.id || current.rolled || current.page != next.page {
            continue;
        }
        let (Some(old_tab), Some(new_tab)) =
            (prior.tabs.get(prior.active), current.tabs.get(current.active))
        else {
            continue;
        };
        if old_tab.folder.is_some() || new_tab.folder.is_some() {
            continue;
        }
        let items_changed = old_tab.items.len() != new_tab.items.len()
            || old_tab.items.iter().zip(&new_tab.items).any(|(a, b)| {
                a.id != b.id || !a.path.eq_ignore_ascii_case(&b.path)
            });
        if items_changed {
            for item in old_tab.items.iter().chain(&new_tab.items) {
                affected_paths.insert(item.path.to_lowercase());
            }
        }
    }
    let mut indices = native_order
        .iter()
        .enumerate()
        .filter(|(_, item)| affected_paths.contains(&item.path.to_lowercase()))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    indices
}

#[cfg(test)]
#[test]
pub(super) fn native_paint_hold_targets_regular_tab_and_geometry_changes() {
    use crate::model::Bounds;
    let mut before = State::empty();
    let id = before.add_group("Test", Bounds { x: 10., y: 10., w: 300., h: 300. });
    before.add_tab(id).unwrap();
    let other = before.add_group("Under", Bounds { x: 10., y: 50., w: 300., h: 260. });
    let mut after = before.clone();
    assert!(!is_native_box_paint_transition(&before, &after));
    assert!(!is_regular_rollup_transition(&before, &after));
    after.group_mut(id).unwrap().active = 0;
    assert!(is_native_box_paint_transition(&before, &after));
    assert!(!is_regular_rollup_transition(&before, &after));
    after = before.clone();
    after.group_mut(id).unwrap().bounds.x += 20.;
    assert!(is_native_box_paint_transition(&before, &after));
    assert!(!is_regular_rollup_transition(&before, &after));
    after = before.clone();
    after.group_mut(id).unwrap().rolled = true;
    assert!(!is_native_box_paint_transition(&before, &after));
    assert!(is_regular_rollup_transition(&before, &after));
    after.group_mut(other).unwrap().rolled = true;
    assert!(is_regular_rollup_transition(&before, &after));
    after.group_mut(other).unwrap().active = 1;
    assert!(!is_regular_rollup_transition(&before, &after));
}

#[cfg(test)]
#[test]
pub(super) fn native_paint_hold_and_fast_batch_cover_both_portal_tab_directions() {
    use crate::model::Bounds;
    let mut regular = State::empty();
    let id = regular.add_group("Test", Bounds { x: 10., y: 10., w: 300., h: 300. });
    regular.add_tab(id).unwrap();
    regular.add_tab(id).unwrap();
    let group = regular.group_mut(id).unwrap();
    group.tabs[1].folder = Some("D:/Folder1".into());
    group.tabs[2].folder = Some("D:/Folder2".into());
    group.active = 0;

    let mut portal = regular.clone();
    portal.group_mut(id).unwrap().active = 1;
    portal.group_mut(id).unwrap().tabs[1].scroll = 2;
    for (from, to) in [(&regular, &portal), (&portal, &regular)] {
        assert!(is_native_box_paint_transition(from, to));
        assert!(is_native_icon_tab_switch(from, to));
    }

    let mut other_portal = portal.clone();
    other_portal.group_mut(id).unwrap().active = 2;
    assert!(!is_native_box_paint_transition(&portal, &other_portal));
    assert!(!is_native_icon_tab_switch(&portal, &other_portal));
}
