use super::*;

/// A worker-produced native drag permit is one-shot: no guessed Box ID,
/// UI-only state change or duplicate mouse event may authorize a different
/// membership. Both in-memory States must match the worker's permit.
pub(super) fn validate_native_drop_permit(
    outstanding:Option<&NativeVerifiedDrop>,
    sequence:u64,
    previous:&State,
    proposed:&State,
)->Result<(),String>{
    let granted=outstanding.filter(|event|event.sequence==sequence)
        .ok_or("Native Pin gesture permit is missing, expired or already consumed")?;
    if &granted.before != previous || &granted.next != proposed {
        return Err("Native Pin action changed since actual Explorer mouse release".into());
    }
    Ok(())
}

pub(crate) fn native_explorer_layout_matches(a: &State, b: &State) -> bool {
    if a.page != b.page || a.groups.len() != b.groups.len() {
        return false;
    }
    for (ga, gb) in a.groups.iter().zip(&b.groups) {
        if ga.id != gb.id
            || ga.page != gb.page
            || ga.bounds != gb.bounds
            || ga.rolled != gb.rolled
            || ga.active != gb.active
            || ga.tabs.len() != gb.tabs.len()
        {
            return false;
        }
        for (ta, tb) in ga.tabs.iter().zip(&gb.tabs) {
            if ta.id != tb.id || ta.folder.is_some() != tb.folder.is_some() {
                return false;
            }
            if ta.folder.is_some() {
                continue;
            }
            if ta.items.len() != tb.items.len()
                || ta.items.iter().zip(&tb.items).any(|(x, y)| {
                    x.id != y.id || !x.path.eq_ignore_ascii_case(&y.path)
                })
            {
                return false;
            }
        }
    }
    true
}

pub(super) fn accept_expected_workspace_disk_change(
    latest: &[u8],
    workspace_before: &[u8],
    expected_disk_after_drop: Option<&State>,
    current_state: &State,
    rolled_overrides: &[(u64, bool)],
) -> Result<Option<(State, bool)>, String> {
    if latest == workspace_before {
        return Ok(None);
    }
    let disk = State::from_saved_bytes(latest).map_err(|_| {
        "Unexpected Workspace disk change while native Explorer is active; stopping fail-open"
            .to_string()
    })?;
    let disk_vec = serde_json::to_vec(&disk).ok();
    if expected_disk_after_drop.is_some_and(|exp| {
        serde_json::to_vec(exp).ok() == disk_vec || native_explorer_layout_matches(exp, &disk)
    }) {
        return Ok(Some((disk, true)));
    }
    let real_current = if rolled_overrides.is_empty() {
        current_state.clone()
    } else {
        state_with_rolled_overrides(current_state, rolled_overrides)
    };
    if native_explorer_layout_matches(&real_current, &disk) {
        return Ok(Some((disk, false)));
    }
    Err("Unexpected Workspace disk change while native Explorer is active; stopping fail-open".into())
}

pub(super) fn sync_expected_workspace_disk(
    workspace: &std::path::Path,
    workspace_before: &mut Vec<u8>,
    persisted_disk_state: &mut State,
    expected_disk_after_drop: &mut Option<State>,
    current_state: &State,
    rolled_overrides: &[(u64, bool)],
) -> Result<(), String> {
    let latest = crate::native_ole_persist::read_existing(workspace)?;
    if let Some((disk, matched_drop_patch)) = accept_expected_workspace_disk_change(
        &latest,
        workspace_before,
        expected_disk_after_drop.as_ref(),
        current_state,
        rolled_overrides,
    )? {
        *workspace_before = latest;
        *persisted_disk_state = disk;
        if matched_drop_patch || expected_disk_after_drop.is_some() {
            *expected_disk_after_drop = None;
        }
    }
    Ok(())
}
