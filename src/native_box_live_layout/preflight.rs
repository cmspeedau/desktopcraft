use super::*;

/// Read-only preflight for the ACTUAL Windows DesktopCraft UI, without
/// creating a Lab overlay, allocating a Shell hook or moving ANY native icon.
/// This is called before the installed/native app is permitted to start.
pub fn native_desktop_preflight(state:&State,view:&DesktopView)
    ->Result<Value,String>{
    // Read-only named policy inspection: another live DesktopCraft instance,
    // or an expired DLL left mapped by Explorer after an interrupted session,
    // must NEVER be overwritten with a new native hook. Only the guarded
    // emergency-unhook CLI may detach an expired, ownerless policy.
    let policy=crate::native_filter_host::native_policy_status();
    if policy["mapping_present"]==true {
        if policy["native_policy_currently_valid"]==true {
            return Err("Another real Explorer native policy is active. Close that DesktopCraft instance before starting a second one".into());
        }
        return Err("Explorer retains a stale Native Desktop policy mapping. No icons were moved; close any Native Desktop instances and review --native-filter-policy-status before using the guarded --native-filter-emergency-off command".into());
    }
    if host::data_dir().join("native-inbox-recovery.json").exists(){
        return Err("An unresolved Explorer icon recovery journal exists from an interrupted Native Desktop session.".into());
    }
    if !view.native_placement_settings(true) {
        return Err("Explorer icons must be visible, with Auto arrange disabled".into());
    }
    let snap_to_grid = view.snap_to_grid().ok_or("Explorer grid setting unavailable")?;
    let pid=desktop_owner(view)?.pid;
    let cell=view.spacing().ok_or("Explorer icon spacing unavailable")?;
    let icons=view.items();
    validate_layout_snapshot(&icons)?;
    if offscreen_icon_count(&icons,&current_screens())!=0 {
        return Err("Explorer Desktop contains icons outside connected monitors; Native layout refused".into());
    }
    // Folder Portals retain their own view. Native Boxes project only verified
    // Desktop icons, leaving saved Pins and other Workspace data intact.
    // Live Native Desktop pauses Explorer's global Align-to-Grid so Boxes align
    // to Box grids and hidden icons park at one free spot (decision 0001).
    let membership=NativeTwoBoxMembership::prepare_workspace(1,state,&icons)?;
    let monitors = host::monitors();
    let desktop_origin = snap_to_grid.then(|| {
        let positions = icons.iter().map(|i| i.pos).collect::<Vec<_>>();
        monitors.iter().find(|m| m.primary).or_else(|| monitors.first())
            .and_then(|main| grid_origin_for_screen(monitor_rect(main), &positions, cell).ok())
    }).flatten();
    let (plans,hidden,_,nudged_loose)=plan_layout(&membership,state,&icons,&icons,cell,&current_screens(),
        &monitors,DesktopCoordinates::current()?, false, desktop_origin, |_|0)?;
    let mut pages=Vec::with_capacity(plans.len());
    for ((id,selected),plan) in membership.selected_tabs.iter().zip(&plans) {
        pages.push(json!({
            "group_id":id,"active_tab":selected,
            "native_page_count":plan.page_count,
            "native_page_capacity":plan.page_capacity,
            "initially_displayed":plan.moves.len(),
            "offpage_count":plan.offpage_paths.len()
        }));
    }
    let cap=if cfg!(feature="native-release"){
        crate::native_live_membership::MAX_USER_NATIVE_DESKTOP_HIDE
    }else{crate::native_live_membership::MAX_EXPERIMENTAL_STAGED_HIDE};
    if hidden.len()>cap{
        return Err(format!(
            "Native layout requires hiding {} icons, beyond this build's {}-item safety ceiling",
            hidden.len(),cap));
    }
    Ok(json!({
        "ready":true,"real_desktop_explorer_pid":pid,
        "monitor_signature":host::signature(&host::monitors()),
        "actual_explorer_icons":icons.len(),
        "explorer_snap_to_grid":snap_to_grid,
        "active_native_boxes":membership.plan.groups.len(),
        "real_explorer_icon_moves_planned":plans.iter().map(|plan|plan.moves.len()).sum::<usize>(),
        "hidden_by_active_tab_and_overflow":hidden.len(),
        "native_pages":pages,
        "loose_icons_nudged_planned":nudged_loose.len(),
        "separate_folder_portals_preserved":state.groups.iter().filter(|g|
            g.page==state.page && !g.rolled &&
            g.tabs.get(g.active).is_some_and(|t|t.folder.is_some())).count(),
        "rolled_box_headers_preserved":state.groups.iter().filter(|g|
            g.page==state.page && g.rolled).count(),
        "source_files_modified":false,
        "workspace_modified":false,"native_hook_installed":false,
    }))
}
