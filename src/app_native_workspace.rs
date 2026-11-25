use crate::*;

pub(super) fn should_hold_new_rollup_reveal(
    chrome_native_live: bool,
    pointer_button_down: bool,
    now: f64,
    hold_until: &mut f64,
) -> bool {
    if !chrome_native_live {
        return false;
    }
    if pointer_button_down {
        *hold_until = now + 0.35;
        return true;
    }
    now < *hold_until
}

pub(crate) fn rollup_header_bounds(g: &Group, chrome_native: bool) -> Bounds {
    if chrome_native {
        paint::native_group_header(g)
    } else {
        Bounds {
            h: HEADER,
            ..g.bounds
        }
    }
}

pub(crate) fn rollup_revealed_bounds(g: &Group, chrome_native: bool) -> Bounds {
    if chrome_native {
        paint::native_group_outer(g)
    } else {
        g.bounds
    }
}

pub(super) fn bounds_intersect(a: Bounds, b: Bounds) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y
}

pub(super) fn native_revealed_regular_group(
    state: &State,
    revealed: Option<u64>,
    hidden_active: bool,
) -> Option<&Group> {
    if !state.prefs.rollup {
        return None;
    }
    let id = revealed?;
    state.group(id).filter(|g| {
        g.page == state.page
            && g.rolled
            && g.tab().folder.is_none()
            && (!hidden_active || g.keep_visible)
    })
}

pub(super) fn native_revealed_occludes(state: &State, revealed: Option<u64>, other: &Group) -> bool {
    let Some(rev) = native_revealed_regular_group(state, revealed, false) else {
        return false;
    };
    if other.id == rev.id || other.page != state.page {
        return false;
    }
    let other_visual = if other.rolled {
        rollup_header_bounds(other, true)
    } else {
        rollup_revealed_bounds(other, true)
    };
    bounds_intersect(rollup_revealed_bounds(rev, true), other_visual)
}

pub(super) fn native_effective_state(
    state: &State,
    revealed: Option<u64>,
    hidden_active: bool,
) -> (std::borrow::Cow<'_, State>, Vec<(u64, bool)>) {
    let effective_revealed = native_revealed_regular_group(state, revealed, hidden_active)
        .map(|g| g.id);
    let mut changed_indices = Vec::new();
    for (idx, g) in state.groups.iter().enumerate() {
        if g.page != state.page {
            continue;
        }
        let orig = g.rolled;
        let desired = if hidden_active && !g.keep_visible {
            true
        } else if !state.prefs.rollup {
            false
        } else if effective_revealed == Some(g.id) {
            false
        } else if !orig
            && effective_revealed.is_some()
            && native_revealed_occludes(state, effective_revealed, g)
        {
            true
        } else {
            orig
        };
        if desired != orig {
            changed_indices.push((idx, g.id, orig, desired));
        }
    }
    if changed_indices.is_empty() {
        return (std::borrow::Cow::Borrowed(state), Vec::new());
    }
    let mut effective = state.clone();
    let mut overrides = Vec::with_capacity(changed_indices.len());
    for (idx, id, orig, desired) in changed_indices {
        effective.groups[idx].rolled = desired;
        overrides.push((id, orig));
    }
    (std::borrow::Cow::Owned(effective), overrides)
}

/// Verifies that a worker-captured native Explorer drop (`proposal_before`)
/// starts from the exact last worker-acknowledged UI state (including its
/// acknowledged hover-reveal / quick-hide overrides), and that `current_state`
/// has no un-acknowledged layout/tab/page edits. Transient hover-reveal flips
/// in `app.revealed` are ignored and re-aligned until the drop ACK completes.
#[cfg(windows)]
pub(super) fn native_drop_matches_acknowledged_ui(
    accepted_state: &State,
    accepted_revealed: Option<u64>,
    accepted_hidden: bool,
    current_state: &State,
    current_hidden: bool,
    proposal_before: &State,
) -> bool {
    let (accepted_eff, _) =
        native_effective_state(accepted_state, accepted_revealed, accepted_hidden);
    !native_desktop_requires_worker_sync(proposal_before, &accepted_eff)
        && accepted_hidden == current_hidden
        && !native_desktop_requires_worker_sync(accepted_state, current_state)
}

/// Returns true whenever either the effective Explorer layout OR the
/// hover-reveal / quick-hide `rolled_overrides` table differs between two UI
/// snapshots. Both must stay in lockstep with the native Explorer worker
/// because the worker reconstructs disk state via `state_with_rolled_overrides`.
#[cfg(windows)]
pub(super) fn native_effective_sync_needed(
    before: &State,
    before_revealed: Option<u64>,
    before_hidden: bool,
    now: &State,
    now_revealed: Option<u64>,
    now_hidden: bool,
) -> bool {
    let (before_eff, before_overrides) =
        native_effective_state(before, before_revealed, before_hidden);
    let (now_eff, now_overrides) = native_effective_state(now, now_revealed, now_hidden);
    native_desktop_requires_worker_sync(&before_eff, &now_eff) || before_overrides != now_overrides
}

#[cfg(windows)]
pub(super) fn clamp_native_tx_frame_mutation(
    app: &mut App,
    baseline: (&State, Option<u64>, bool),
    undo_before_frame: &[State],
) -> bool {
    let (base_state, base_rev, base_hidden) = baseline;
    let cur_hidden = app.hidden;
    let undo_changed = app.undo.as_slice() != undo_before_frame;
    if native_effective_sync_needed(
        base_state,
        base_rev,
        base_hidden,
        &app.state,
        app.revealed,
        cur_hidden,
    ) || native_desktop_requires_worker_sync(base_state, &app.state)
        || app.drag.is_some()
        || app.menu_press.is_some()
        || app.classic_file_menu.is_some()
        || app.native_scroll_target.is_some()
        || app.native_page_request.is_some()
        || undo_changed
    {
        if app.drag.is_some() && app.host.is_some() {
            host::capture_pointer(false);
        }
        app.state = base_state.clone();
        app.clear_orphaned_rename_dialog();
        app.revealed = base_rev;
        app.hidden = base_hidden;
        app.native_visual_revealed = base_rev;
        app.drag = None;
        app.menu_press = None;
        app.classic_file_menu = None;
        app.clear_native_scroll_state();
        if undo_changed {
            app.undo = undo_before_frame.to_vec();
        }
        true
    } else {
        false
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub(super) fn native_page_change_allowed(
    rules_sort_reply_pending: bool,
    pending_sort_followup: bool,
    layout_restore_reply_pending: bool,
    pending_layout_followup: bool,
    undo_inflight: bool,
    record_undo_on_sync: bool,
    drag_active: bool,
    accepted: Option<(&State, Option<u64>, bool)>,
    current: (&State, Option<u64>, bool),
) -> bool {
    !rules_sort_reply_pending
        && !pending_sort_followup
        && !layout_restore_reply_pending
        && !pending_layout_followup
        && !undo_inflight
        && !record_undo_on_sync
        && !drag_active
        && accepted.is_some_and(|(acc_state, acc_rev, acc_hidden)| {
            !native_effective_sync_needed(
                acc_state,
                acc_rev,
                acc_hidden,
                current.0,
                current.1,
                current.2,
            )
        })
}

#[cfg(windows)]
pub(super) fn resolve_native_filter_dll() -> Result<PathBuf, String> {
    if cfg!(feature = "native-release") {
        native_bundle::extract_native_filter()
    } else if cfg!(all(debug_assertions, feature = "native-lab")) {
        Ok(std::env::current_exe()
            .ok()
            .and_then(|exe| {
                exe.ancestors()
                    .nth(3)
                    .map(|root| root.join("native-filter-build/debug/desktopcraft_native_filter.dll"))
            })
            .unwrap_or_default())
    } else {
        Err("A verified embedded native DLL is required".into())
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub(super) fn try_enable_native_desktop(
    app: &mut App,
    native_save_permitted: bool,
    native_disk_state: &mut Option<State>,
    native_last_accepted_state: &mut Option<State>,
    native_last_accepted_revealed: &mut Option<u64>,
    native_last_accepted_hidden: &mut bool,
    last_saved_state: &mut Vec<u8>,
    last_seen_state: &mut Vec<u8>,
    native_display_baseline: &mut Option<String>,
    time: f64,
) -> Result<native_filter_host::NativeTwoBoxBackgroundService, String> {
    if !enable_native_audit_physical_dpi() {
        return Err("Physical DPI-aware thread context unavailable".into());
    }
    let view = shell::DesktopView::acquire()
        .ok_or_else(|| "Actual Windows Explorer Desktop unavailable".to_string())?;
    if view.icons_visible() == Some(false) && view.set_icons_visible(true) {
        host::event(
            "INFO",
            "native_desktop.restored_hidden_icons",
            "Restored Explorer desktop icon layer visibility before enabling Native Desktop",
        );
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let journal = host::data_dir().join("native-inbox-recovery.json");
    let policy = native_filter_host::native_policy_status();
    if journal.exists() && policy["mapping_present"] != true {
        native_box_live_layout::automatic_recovery_status_or_restore(&view, true).map_err(
            |err| format!("Interrupted Explorer checkpoint could not be restored: {err}"),
        )?;
    }
    let bounds_before = app
        .state
        .groups
        .iter()
        .map(|g| (g.id, g.bounds))
        .collect::<Vec<_>>();
    app.prepare_native_monitor_layout();
    let bounds_after = app
        .state
        .groups
        .iter()
        .map(|g| (g.id, g.bounds))
        .collect::<Vec<_>>();
    if bounds_after != bounds_before {
        app.remember_layout();
    }
    if !native_general_workspace_eligible(&app.state) {
        return Err(
            "Keep at least one regular non-rolled Box on the current page before enabling."
                .into(),
        );
    }
    native_box_live_layout::native_desktop_preflight(&app.state, &view)?;
    let dll = resolve_native_filter_dll()?;
    app.state.prefs.desktop_enabled = true;
    if native_save_permitted {
        persist_native_workspace(
            app,
            native_disk_state,
            native_last_accepted_state,
            last_saved_state,
            last_seen_state,
            time,
        );
        if !app.save_ok {
            app.state.prefs.desktop_enabled = false;
            return Err("Could not save enabled state to Workspace".into());
        }
    } else if let Err(e) = app.state.save(&app.save_path) {
        app.state.prefs.desktop_enabled = false;
        return Err(format!("Could not save enabled state: {e}"));
    }
    let mut worker = native_filter_host::NativeTwoBoxBackgroundService::start(
        app.root.join("data/workspace.json"),
        dll,
        host::window() as usize,
    );
    if !worker.await_ready(std::time::Duration::from_secs(6)) {
        let _ = worker.shutdown();
        app.state.prefs.desktop_enabled = false;
        if native_save_permitted {
            persist_native_workspace(
                app,
                native_disk_state,
                native_last_accepted_state,
                last_saved_state,
                last_seen_state,
                time,
            );
        }
        *native_last_accepted_state = None;
        return Err("Explorer did not accept native Boxes before timeout".into());
    }
    *native_last_accepted_state = Some(app.state.clone());
    *native_last_accepted_revealed = None;
    *native_last_accepted_hidden = false;
    *native_display_baseline = Some(host::signature(&host::monitors()));
    app.revealed = None;
    app.native_visual_revealed = None;
    app.hover_rolled = None;
    app.hidden = false;
    host::event(
        "INFO",
        "native_desktop.enabled_live",
        "DesktopCraft re-enabled live from Settings",
    );
    Ok(worker)
}

#[cfg(windows)]
pub(super) fn persist_native_workspace(
    app: &mut App,
    native_disk_state: &mut Option<State>,
    native_last_accepted_state: &mut Option<State>,
    last_saved_state: &mut Vec<u8>,
    last_seen_state: &mut Vec<u8>,
    time: f64,
) {
    app.remember_layout();
    let save_res = if let Some(expected) = crate::ai_tools::expected_ai_disk_bytes(app).map(|b| b.to_vec()) {
        app.state
            .save_returning_bytes(&app.save_path, &expected)
            .map(Some)
    } else {
        app.state.save(&app.save_path).map(|()| None)
    };
    match save_res {
        Ok(ai_bytes) => {
            app.save_ok = true;
            if let Some((predecessor, written_bytes)) = ai_bytes {
                crate::ai_tools::record_ai_disk_bytes(app, predecessor, written_bytes);
            }
            *native_disk_state = State::load(&app.save_path)
                .ok()
                .or_else(|| Some(app.state.clone()));
            *last_saved_state = serde_json::to_vec(&app.state).unwrap_or_default();
            *last_seen_state = last_saved_state.clone();
            *native_last_accepted_state = Some(app.state.clone());
            host::event(
                "INFO",
                "native_desktop.workspace_saved",
                "Saved acknowledged Native Desktop workspace layout",
            );
        }
        Err(e) => {
            app.save_ok = false;
            host::event("WARN", "native_desktop.workspace_save_failed", &e.to_string());
            app.notify(format!("Could not save changes: {e}"), time);
        }
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub(super) fn ensure_native_disk_matches(
    native_save_permitted: bool,
    app: &mut App,
    native_disk_state: &mut Option<State>,
    native_last_accepted_state: &mut Option<State>,
    last_saved_state: &mut Vec<u8>,
    last_seen_state: &mut Vec<u8>,
    time: f64,
) -> bool {
    let matches = |disk: &State, state: &State| {
        disk.next_id == state.next_id
            && desktop::native_layout_structure_matches(disk, state)
            && !native_desktop_requires_worker_sync(disk, state)
    };
    if native_save_permitted
        && !native_disk_state
            .as_ref()
            .is_some_and(|disk| matches(disk, &app.state))
    {
        persist_native_workspace(
            app,
            native_disk_state,
            native_last_accepted_state,
            last_saved_state,
            last_seen_state,
            time,
        );
    }
    native_save_permitted
        && native_disk_state
            .as_ref()
            .is_some_and(|disk| matches(disk, &app.state))
}

