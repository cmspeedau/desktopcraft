use super::*;

impl LiveLayoutTransaction {
    pub fn new(view:&DesktopView,worker_pid:u32,state:&State)->Result<Self,String>{
        if desktop_owner(view)?.pid!=worker_pid || !view.native_placement_settings(true) {
            return Err("Explorer owner or arrangement changed before native layout checkpoint".into());
        }
        let original_snap_to_grid = view.snap_to_grid().ok_or("Explorer grid setting unavailable")?;
        let original=view.items();
        validate_layout_snapshot(&original)?;
        if offscreen_icon_count(&original,&current_screens())!=0 {
            return Err("Native icon checkpoint contains offscreen Explorer icons".into());
        }
        let monitors = host::monitors();
        let monitor=host::signature(&monitors);
        if monitor.is_empty(){return Err("Native monitor signature unavailable".into());}
        let desktop_grid_origin = if original_snap_to_grid {
            let positions = original.iter().map(|i| i.pos).collect::<Vec<_>>();
            monitors.iter().find(|m| m.primary).or_else(|| monitors.first())
                .zip(view.spacing())
                .and_then(|(main, cell)| grid_origin_for_screen(monitor_rect(main), &positions, cell).ok())
        } else {
            None
        };
        let journal=host::data_dir().join("native-inbox-recovery.json");
        // A pending journal is evidence of a previously interrupted native
        // icon mutation. Never overwrite it; never silently restore stale
        // positions, particularly when monitors/workspace have changed.
        if journal.exists(){
            return Err(format!("An unresolved native layout recovery checkpoint already exists at {}. Manual recovery review required; no icon moved",journal.display()));
        }
        let allowed_paths=state.groups.iter()
            .flat_map(|g|g.tabs.iter().flat_map(|t|t.items.iter()
                .map(|pin|pin.path.to_lowercase())))
            .collect::<BTreeSet<_>>();
        if allowed_paths.is_empty(){
            return Err("Native recovery journal lacks known Box item identities".into());
        }
        let coordinates = DesktopCoordinates::current()?;
        let box_bounds=state.groups.iter().map(|g| {
            let rect = coordinates.screen_rect(g.bounds);
            json!({"id":g.id,"x":rect.x,"y":rect.y,"w":rect.w,"h":rect.h})
        }).collect::<Vec<_>>();
        let data=json!({
            "format":1,"explorer_pid_before":worker_pid,
            "allowed_box_paths":allowed_paths,
            "allowed_box_bounds":box_bounds,
            "monitor_signature":monitor,
            "mode":"reversible_original_explorer_positions",
            "snap_to_grid":false,
            "original_snap_to_grid":original_snap_to_grid,
            "hidden_park_point":null,
            "nudged_loose_positions":[],
            "original_icons":original.iter().map(|item|json!({
                "path":item.path,"label":item.label,"x":item.pos.0,"y":item.pos.1
            })).collect::<Vec<Value>>(),
            "automatic_restore_on_next_launch":false,
            "no_saved_workspace_changes":true
        });
        let json=serde_json::to_vec_pretty(&data).map_err(|e|e.to_string())?;
        // Create-new is an atomic guard against concurrent Explorer writers.
        let mut file=std::fs::OpenOptions::new().create_new(true).write(true)
            .open(&journal).map_err(|e|format!(
                "Native icon recovery journal refused before any move: {e}"))?;
        use std::io::Write;
        if let Err(error)=file.write_all(&json).and_then(|()|file.sync_all()){
            let _=std::fs::remove_file(&journal);
            return Err(format!("Cannot persist native layout recovery journal: {error}"));
        }
        // Pause Explorer's global Align-to-Grid while Boxes are active so Box
        // icons align to Box grids and hidden icons can stack at one free point
        // (docs/decisions/0001-native-two-grid-placement.md).
        let snap_to_grid = if original_snap_to_grid {
            if view.set_snap_to_grid(false) {
                host::event("INFO", "native_desktop.two_grid_enabled",
                    "Explorer Align-to-Grid paused for Box-grid placement; loose icons snap to Desktop grid and original setting restores on exit");
                false
            } else {
                host::event("WARN", "native_desktop.two_grid_unavailable",
                    "Explorer refused Align-to-Grid toggle; continuing with Desktop-grid snap");
                true
            }
        } else {
            false
        };
        Ok(Self{
            expected:original.clone(),original,moved:Vec::new(),nudged_loose:Vec::new(),
            pages:BTreeMap::new(),journal,initial_monitor_signature:monitor,snap_to_grid,
            original_snap_to_grid,desktop_grid_origin,hidden_park:None,
            parked_paths:BTreeSet::new(),
            pending_scaled_recheck_at:None,scaled_targets:BTreeMap::new(),
            native_batch_shell_verified:false,
            pending_drop_pre_positions:Vec::new(),
            count_changes:0,restored:false
        })
    }
    pub fn snap_to_grid(&self)->bool{self.snap_to_grid}
    pub fn current_page(&self,group:u64)->usize{
        self.pages.get(&group).copied().unwrap_or(0)
    }
    pub fn set_page(&mut self,group:u64,page:usize){
        self.pages.insert(group,page);
    }
    pub fn reset_page(&mut self,group:u64){self.pages.insert(group,0);}
    pub fn reset_all_pages(&mut self){self.pages.clear();}
    pub fn page_limit(&self,member:&NativeTwoBoxMembership,state:&State,group:u64,tab:usize,
                      cell:(i32,i32))->Result<usize,String>{
        let g=state.group(group).ok_or("Native Box for paging missing")?;
        let t=g.tabs.get(tab).ok_or("Native tab for paging missing")?;
        if t.folder.is_some(){return Err("Native Folder Portal is not paged".into());}
        let count=member.tab_indices(group,tab)?.len();
        let bounds = DesktopCoordinates::current()?.screen_rect(group_native_bounds(g));
        let grid = (count > 0)
            .then(|| box_grid(bounds, &self.original, &host::monitors(), cell, self.snap_to_grid))
            .transpose()?;
        native_box_placement::box_scroll_positions(bounds,grid.map_or(cell,|g|g.cell),
            grid.and_then(|g|g.origin),count)
    }
    pub fn planned_visibility(
        &self,member:&NativeTwoBoxMembership,state:&State,
        native_original:&[ViewItem],cell:(i32,i32),
    )->Result<NativePlacementPlan,String>{
        let screens=current_screens();
        let (plans,hidden,park,nudged)=plan_layout(member,state,&self.original,native_original,
            cell,&screens,&host::monitors(),DesktopCoordinates::current()?, self.snap_to_grid,
            self.desktop_grid_origin, |id|self.current_page(id))?;
        let cap=crate::native_live_membership::active_native_hide_cap();
        if hidden.len()>cap{
            return Err(format!("Native viewport would hide {} icons, above current {}-item safety cap",hidden.len(),cap));
        }
        Ok((plans,hidden,park,nudged))
    }
}
