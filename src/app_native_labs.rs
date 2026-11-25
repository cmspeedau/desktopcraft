use crate::*;

#[cfg(all(windows,feature="native-lab"))]
pub(super) struct IsolatedNativeOleTempRoot(pub(super) PathBuf);
#[cfg(all(windows,feature="native-lab"))]
impl Drop for IsolatedNativeOleTempRoot {
    fn drop(&mut self) {
        // Cleanup only THIS run's disposable lab directory; never touch a
        // user workspace. Absence of an exact prefix fails closed.
        let root=&self.0;
        if root.parent()==Some(std::env::temp_dir().as_path()) &&
            root.file_name().is_some_and(|name|
                name.to_string_lossy().starts_with("DesktopCraftIsolatedMainOle-")
                || name.to_string_lossy().starts_with("DesktopCraftExplorerChromeOLE-")){
            let _=std::fs::remove_dir_all(root);
        }
    }
}

/// The normal Main-HWND OLE callback may persist an atomic move while an
/// Explorer ListView paint/input policy still remembers the previous Tab.
/// Re-attach that policy only after a byte-stable, EXACT ownership check.
/// This is deliberately pure: validation cannot reposition Explorer icons,
/// touch the user's Workspace or install a native DLL.
#[cfg(all(windows,feature="native-lab"))]
pub(super) fn validate_native_ole_rehook_after_move(
    before:&State,
    after:&State,
    explorer:&[shell::ViewItem],
    source_id:u64,
    destination_id:u64,
    selected_count:usize,
)->Result<explorer_native::NativeMultiBoxPlan,String>{
    if selected_count==0 || selected_count>8 || source_id==destination_id
        || before.page!=after.page || before.groups.len()!=after.groups.len()
        || before.pages!=after.pages {
        return Err("Native rehook refused: Workspace topology/selection changed".into());
    }
    for old in &before.groups {
        let new=after.group(old.id)
            .ok_or("Native rehook refused: a saved Box disappeared")?;
        if old.id!=source_id && old.id!=destination_id &&
            serde_json::to_vec(old).map_err(|e|e.to_string())?
                !=serde_json::to_vec(new).map_err(|e|e.to_string())?{
            return Err("Native rehook refused: an unrelated Box changed".into());
        }
        if old.id==source_id || old.id==destination_id {
            if old.tabs.len()!=new.tabs.len()
                || old.active!=new.active
                || old.page!=new.page || old.rolled!=new.rolled
                || old.bounds!=new.bounds {
                return Err("Native rehook refused: source or target geometry/Tab changed".into());
            }
            for (index,tab) in old.tabs.iter().enumerate(){
                if index!=old.active &&
                    serde_json::to_vec(tab).map_err(|e|e.to_string())?
                        !=serde_json::to_vec(&new.tabs[index]).map_err(|e|e.to_string())? {
                    return Err("Native rehook refused: inactive Tab or Portal mutated".into());
                }
            }
        }
    }
    let old_source=before.group(source_id)
        .ok_or("Native rehook source missing before Drop")?.tab();
    let new_source=after.group(source_id)
        .ok_or("Native rehook source missing after Drop")?.tab();
    let old_target=before.group(destination_id)
        .ok_or("Native rehook target missing before Drop")?.tab();
    let new_target=after.group(destination_id)
        .ok_or("Native rehook target missing after Drop")?.tab();
    if old_source.folder.is_some() || new_source.folder.is_some()
        || old_target.folder.is_some() || new_target.folder.is_some()
        || old_source.items.len()!=new_source.items.len()+selected_count
        || new_target.items.len()!=old_target.items.len()+selected_count {
        return Err("Native rehook refused: source/target Pin count or Portal mismatch".into());
    }
    let moved=new_target.items.iter().skip(old_target.items.len())
        .map(|pin|pin.path.to_lowercase()).collect::<Vec<_>>();
    let distinct=moved.iter().collect::<std::collections::BTreeSet<_>>();
    if moved.len()!=selected_count || distinct.len()!=selected_count
        || moved.iter().any(|path|
            !old_source.items.iter().any(|pin|pin.path.eq_ignore_ascii_case(path))
                || new_source.items.iter().any(|pin|pin.path.eq_ignore_ascii_case(path))) {
        return Err("Native rehook refused: transferred Explorer identity mismatch".into());
    }
    if old_target.items.iter().zip(&new_target.items)
        .any(|(a,b)|serde_json::to_vec(a).ok()!=serde_json::to_vec(b).ok()){
        return Err("Native rehook refused: pre-existing destination Pins were altered".into());
    }
    let plan=explorer_native::native_multi_box_plan(after,explorer,16,128)?;
    if !plan.groups.iter().any(|g|g.group_id==source_id)
        || !plan.groups.iter().any(|g|g.group_id==destination_id) {
        return Err("Native rehook refused: source or destination lacks native policy".into());
    }
    Ok(plan)
}

/// A pure, fail-closed model fixture for an EXISTING user-layout clone.
/// Real native icons remain at their original Explorer coordinates. The new
/// empty Box is created ONLY in the isolated clone, NEVER the user Workspace.
#[cfg(all(windows,feature="native-lab"))]
pub(super) fn native_chrome_ole_clone_plan(
    original:&State,
    explorer:&[shell::ViewItem],
    source_id:u64,
)->Result<(State,Vec<u64>),String>{
    let source=original.group(source_id)
        .filter(|g|g.page==original.page && !g.rolled
            && g.active<g.tabs.len() && g.tabs[g.active].folder.is_none())
        .ok_or("Native Chrome OLE lab requires a visible regular source Box Tab")?;
    let selected=source.tab().items.iter().take(2).collect::<Vec<_>>();
    if selected.len()!=2 || selected.iter().any(|pin|explorer.iter()
        .filter(|item|item.path.eq_ignore_ascii_case(&pin.path)).count()!=1){
        return Err("Native Chrome lab source must own two unique genuine Desktop shortcuts".into());
    }
    // If the REAL user layout already contains exactly one compatible
    // native Desktop destination, use THAT Box in the isolated clone.
    // Do not invent a third Box or silently route a gesture to a different
    // place than the user will see after launching DesktopCraft normally.
    let current=explorer_native::native_multi_box_plan(
        original,explorer,16,128)?;
    let existing=current.groups.iter().filter_map(|native|{
        let group=original.group(native.group_id)?;
        (group.id!=source_id && group.active<native.native_indices.len()
            && native.native_indices[group.active].is_empty()
            && group.tabs.iter().all(|tab|
                tab.folder.is_none() && tab.items.is_empty()))
            .then_some(group.id)
    }).collect::<Vec<_>>();
    if existing.len()>1 {
        return Err("Native Chrome lab has ambiguous existing empty destinations".into());
    }
    if let Some(&target)=existing.first(){
        return Ok((original.clone(),vec![source_id,target]));
    }
    let b=source.bounds;
    let dest=Bounds{
        x:if b.x>=550. {b.x-420.}else{b.x+b.w+45.},
        y:if b.y<120. {300.}else{b.y+80.},
        w:360.,h:270.
    };
    let mut next=original.clone();
    let target=next.add_group("Native OLE — Temporary Destination",dest);
    next.add_tab(target)?;
    next.group_mut(target).ok_or("Disposable destination Box missing")?.active=0;
    let plan=explorer_native::native_multi_box_plan(&next,explorer,16,128)?;
    let source_ok=plan.groups.iter().any(|g|g.group_id==source_id && g.active==source.active);
    let destination_ok=plan.groups.iter().any(|g|g.group_id==target && g.active==0);
    if !source_ok || !destination_ok {
        return Err("Native Chrome OLE source and empty destination must both pass native policy".into());
    }
    Ok((next,vec![source_id,target]))
}

#[cfg(all(test,windows,feature="native-lab"))]
mod native_chrome_original_layout_lab_tests {
    use super::*;
    fn pin(path:&str,id:u64)->Item{
        Item{id,name:path.into(),path:path.into(),kind:"lnk".into(),
            size:0,modified:0,label:None}
    }
    fn mock(path:&str,index:i32)->shell::ViewItem{
        shell::ViewItem{path:path.into(),label:path.into(),pos:(index*110,40)}
    }
    fn fixture()->(State,Vec<shell::ViewItem>,u64){
        let mut s=State::empty();
        let src=s.add_group("Craft",Bounds{x:1788.,y:31.,w:740.,h:416.});
        s.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/A.lnk",1));
        s.add_tab(src).unwrap();
        s.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/B.lnk",2));
        s.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/C.lnk",2));
        s.add_tab(src).unwrap();
        s.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/D.lnk",4));
        s.group_mut(src).unwrap().active=1;
        let portal=s.add_group("Other View",Bounds{x:100.,y:100.,w:390.,h:300.});
        s.add_tab(portal).unwrap();
        s.group_mut(portal).unwrap().tabs[0].folder=Some("D:/LinkedFolder".into());
        s.group_mut(portal).unwrap().active=0;
        let icons=["A","B","C","D","Loose"].iter().enumerate()
            .map(|(i,letter)|mock(&format!("C:/Desktop/{letter}.lnk"),i as i32))
            .collect();
        (s,icons,src)
    }

    #[test]
    fn native_filter_can_rehook_only_after_exact_two_pin_ownership_change() {
        let (original,icons,source)=fixture();
        let (pre,ids)=native_chrome_ole_clone_plan(&original,&icons,source).unwrap();
        let target=ids[1];
        let tab=pre.group(target).unwrap().tab().id;
        let offered=[
            PathBuf::from("C:/Desktop/B.lnk"),
            PathBuf::from("C:/Desktop/C.lnk")
        ];
        let policy=explorer_native::native_multi_box_plan(&pre,&icons,16,128).unwrap();
        let (post,_)=explorer_native::plan_native_ole_drop(
            &pre,&icons,&policy,&offered,target,tab,16,128).unwrap();
        let refreshed=validate_native_ole_rehook_after_move(
            &pre,&post,&icons,source,target,2).unwrap();
        assert!(refreshed.groups.iter().any(|g|g.group_id==source));
        assert!(refreshed.groups.iter().any(|g|g.group_id==target));
        assert_eq!(post.group(source).unwrap().tab().items.len(),0);
        assert_eq!(post.group(target).unwrap().tab().items.len(),2);
        assert_eq!(original.group(source).unwrap().tab().items.len(),2);
        assert!(validate_native_ole_rehook_after_move(
            &pre,&post,&icons,source,target,1).is_err());
        assert!(validate_native_ole_rehook_after_move(
            &pre,&post,&icons,source,source,2).is_err());
    }
    #[test]
    fn native_rehook_refuses_unrelated_portal_mutation_inactive_tab_and_stale_geometry() {
        let (original,icons,source)=fixture();
        let (pre,ids)=native_chrome_ole_clone_plan(&original,&icons,source).unwrap();
        let target=ids[1];
        let destination_tab=pre.group(target).unwrap().tab().id;
        let policy=explorer_native::native_multi_box_plan(&pre,&icons,16,128).unwrap();
        let (valid,_)=explorer_native::plan_native_ole_drop(
            &pre,&icons,&policy,
            &[PathBuf::from("C:/Desktop/B.lnk"),PathBuf::from("C:/Desktop/C.lnk")],
            target,destination_tab,16,128).unwrap();
        let mut external=valid.clone();
        external.group_mut(original.groups[1].id).unwrap().tabs[0].name.push_str(" changed");
        assert!(validate_native_ole_rehook_after_move(
            &pre,&external,&icons,source,target,2).is_err());
        let mut tabs=valid.clone();
        tabs.group_mut(source).unwrap().tabs[2].name.push_str(" modified");
        assert!(validate_native_ole_rehook_after_move(
            &pre,&tabs,&icons,source,target,2).is_err());
        let mut shifted=valid.clone();
        shifted.group_mut(target).unwrap().bounds.x+=10.;
        assert!(validate_native_ole_rehook_after_move(
            &pre,&shifted,&icons,source,target,2).is_err());
        let mut substituted=valid.clone();
        substituted.group_mut(target).unwrap().tab_mut().items[0].path=
            "C:/Desktop/External.lnk".into();
        assert!(validate_native_ole_rehook_after_move(
            &pre,&substituted,&icons,source,target,2).is_err());
        let mut changed_native=icons.clone();
        changed_native.retain(|item|!item.path.ends_with("C.lnk"));
        assert!(validate_native_ole_rehook_after_move(
            &pre,&valid,&changed_native,source,target,2).is_err());
    }
    #[test]
    fn copied_real_layout_preserves_original_and_adds_only_one_native_receiver_box() {
        let (s,icons,source)=fixture();
        let original=serde_json::to_vec(&s).unwrap();
        let (copy,visible)=native_chrome_ole_clone_plan(&s,&icons,source).unwrap();
        assert_eq!(serde_json::to_vec(&s).unwrap(),original);
        assert_eq!(copy.groups.len(),s.groups.len()+1);
        assert_eq!(visible[0],source);
        let target=visible[1];
        let target_group=copy.group(target).unwrap();
        assert_eq!(target_group.tabs.len(),2);
        assert!(target_group.tabs.iter().all(|tab|tab.items.is_empty() && tab.folder.is_none()));
        assert_eq!(copy.group(source).unwrap().active,1);
        assert_eq!(copy.group(source).unwrap().tab().items.len(),2);
        assert_eq!(copy.group(source).unwrap().tab().items[1].id,2);
        let policy=explorer_native::native_multi_box_plan(&copy,&icons,16,128).unwrap();
        assert!(policy.groups.iter().any(|g|g.group_id==source));
        assert!(policy.groups.iter().any(|g|g.group_id==target));
        // An unrelated Box using a Folder Portal was not rewritten.
        assert_eq!(copy.groups[1].tabs[0].folder.as_deref(),Some("D:/LinkedFolder"));
    }

    #[test]
    fn real_user_existing_native_destination_is_reused_in_temp_clone_without_extra_box(){
        let (mut state,icons,source)=fixture();
        let destination=state.add_group("Native Desktop",
            Bounds{x:1976.,y:840.,w:360.,h:270.});
        state.add_tab(destination).unwrap();
        state.group_mut(destination).unwrap().active=0;
        let original=serde_json::to_vec(&state).unwrap();
        let (copy,ids)=native_chrome_ole_clone_plan(&state,&icons,source).unwrap();
        assert_eq!(ids,[source,destination]);
        assert_eq!(copy.groups.len(),state.groups.len());
        assert_eq!(serde_json::to_vec(&copy).unwrap(),original,
            "Lab must not fabricate a third group or move the real target Box");
        assert_eq!(copy.group(destination).unwrap().bounds,
            Bounds{x:1976.,y:840.,w:360.,h:270.});
        let policy=explorer_native::native_multi_box_plan(&copy,&icons,16,128).unwrap();
        assert_eq!(policy.groups.len(),2);
    }
    #[test]
    fn real_user_multiple_empty_native_destinations_are_ambiguous_and_refused(){
        let (mut state,icons,source)=fixture();
        for x in [1000.,1500.] {
            let id=state.add_group("Native Desktop",Bounds{x,y:600.,w:360.,h:270.});
            state.add_tab(id).unwrap();
            state.group_mut(id).unwrap().active=0;
        }
        let old=serde_json::to_vec(&state).unwrap();
        assert!(native_chrome_ole_clone_plan(&state,&icons,source).is_err());
        assert_eq!(serde_json::to_vec(&state).unwrap(),old);
    }
    #[test]
    fn copied_real_layout_refuses_portal_and_missing_source_identities() {
        let (s,icons,source)=fixture();
        let baseline=serde_json::to_vec(&s).unwrap();
        let mut portal=s.clone();
        portal.group_mut(source).unwrap().tabs[1].folder=Some("D:/Portal".into());
        assert!(native_chrome_ole_clone_plan(&portal,&icons,source).is_err());
        assert_eq!(serde_json::to_vec(&s).unwrap(),baseline);
        let mut missing=icons.clone();
        missing.retain(|item|!item.path.ends_with("C.lnk"));
        assert!(native_chrome_ole_clone_plan(&s,&missing,source).is_err());
        let mut duplicated=icons.clone();
        duplicated.push(icons[1].clone());
        assert!(native_chrome_ole_clone_plan(&s,&duplicated,source).is_err());
        assert!(native_chrome_ole_clone_plan(&s,&icons,u64::MAX).is_err());
        let mut hidden=s.clone();
        hidden.group_mut(source).unwrap().rolled=true;
        assert!(native_chrome_ole_clone_plan(&hidden,&icons,source).is_err());
    }
}

/// Only the ORIGINAL Main GUI thread may apply a one-shot physical Explorer
/// gesture, and ONLY inside a unique isolated OS TEMP copy of user layout.
/// The normal product never activates this code path. The worker has already
/// detached its native Explorer subclass before the event can be received.
#[cfg(all(windows,feature="native-lab",feature="native-unsafe-drag-lab"))]
pub(super) fn apply_genuine_native_event_to_ui(
    app:&mut App,event:native_drag_channel::NativeGestureEvidence,
    live_recovery:Option<(&std::path::Path,&std::path::Path)>,
)->Result<serde_json::Value,String>{
    let root=app.root.canonicalize().map_err(|e|e.to_string())?;
    let temp=std::env::temp_dir().canonicalize().map_err(|e|e.to_string())?;
    let workspace=app.save_path.canonicalize().map_err(|e|e.to_string())?;
    let is_live=live_recovery.is_some();
    if is_live{
        if root!=host::data_dir().canonicalize().map_err(|e|e.to_string())?
            || workspace!=root.join("data/workspace.json") {
            return Err("Real native gesture cannot target an arbitrary Workspace".into());
        }
    }else if root.parent()!=Some(temp.as_path())
        || !root.file_name().is_some_and(|n|n.to_string_lossy()
            .starts_with("DesktopCraftExplorerChromeOLE-"))
        || workspace!=root.join("data/workspace.json") {
        return Err("Genuine native gesture may only persist to a unique disposable TEMP Workspace".into());
    }
    let view=shell::DesktopView::acquire()
        .ok_or("Original GUI could not reacquire real Explorer Desktop")?;
    if view.icons_visible()!=Some(true){
        return Err("Explorer native Desktop icons are not visible".into());
    }
    let names=explorer_owned_box::win::list_items(view.list_window())
        .into_iter().map(|item|item.name).collect::<Vec<_>>();
    let current=explorer_native::native_view_in_msaa_order(&view.items(),&names)?;
    let mut pid=0u32;
    let hwnd=view.list_window();
    if unsafe{winapi::um::winuser::GetWindowThreadProcessId(hwnd,&mut pid)}==0 {
        return Err("Original GUI native Explorer ListView no longer owned".into());
    }
    let signature=host::signature(&host::monitors());
    let saved_bytes=std::fs::read(&app.save_path)
        .map_err(|e|format!("Could not reread isolated Workspace: {e}"))?;
    let prepared=event.validate(std::time::Instant::now(),
        pid,hwnd as usize as u64,&signature,&saved_bytes,
        &app.state,&current,16,128)?;
    let bounds=host::virtual_rect();
    if bounds[2]<=0 || bounds[3]<=0 {
        return Err("Virtual physical screen geometry is invalid".into());
    }
    let viewport=native_ole_gate::NativeOleViewport{
        screen_origin_px:(bounds[0],bounds[1]),
        client_size_px:(bounds[2] as u32,bounds[3] as u32),
        logical_size:(bounds[2] as f64,bounds[3] as f64),
    };
    let visible=prepared.next_plan.groups.iter()
        .map(|g|g.group_id).collect::<Vec<_>>();
    if visible.len()!=2 || !visible.contains(&prepared.target_group) {
        return Err("Only two uniquely verified original GUI native Boxes may participate".into());
    }
    if is_live { return Err("Live owner workspace experiment is unavailable.".into()); }
    let recovery=if let Some((checkpoint,expected))=live_recovery{
        Some(native_transfer_recovery::VerifiedPinTransferRecovery::prepare(
            &app.save_path,checkpoint,expected,
            &app.state,&prepared,&current,144,168)?)
    }else{None};
    let source_fingerprints=prepared.offered_paths.iter()
        .map(|path|std::fs::read(path)
            .map_err(|e|format!("Native source shortcut not readable before virtual Pin transfer: {e}")))
        .collect::<Result<Vec<_>,_>>()?;
    let before=app.state.clone();
    let live_membership_before=native_live_membership::NativeTwoBoxMembership::prepare(
        1,&before,&current,visible[0],visible[1])?;
    let transaction=native_ole_persist::NativeOlePersistedGesture::begin(
        &app.save_path,&app.state,&current,&signature,
        viewport,&prepared.offered_paths,&visible,16,128)?;
    let mut committed=app.state.clone();
    // Explicit native MouseDown/MouseUp is NOT genuine OLE IDataObject
    // LINK negotiation. Never fabricate a Shell effect to authorize it.
    transaction.commit_verified_native_gesture_lab(
        &mut committed,&current,&signature,viewport,
        &visible,&prepared.offered_paths,event.released_physical,&prepared)?;
    if serde_json::to_vec(&committed).map_err(|e|e.to_string())?
        !=serde_json::to_vec(&prepared.next_state).map_err(|e|e.to_string())?
        || serde_json::to_vec(&State::load(&app.save_path)?).map_err(|e|e.to_string())?
            !=serde_json::to_vec(&committed).map_err(|e|e.to_string())? {
        // Never overwrite a committed disk image on failure. Leave its OLE
        // recovery backup to manual review if the disk differs from UI.
        app.save_allowed=false;
        return Err("Native real-gesture committed disk disagrees with planned UI state; saves suspended".into());
    }
    if let Some(ref recovery)=recovery{
        if let Err(reason)=recovery.confirm(&app.save_path){
            app.save_allowed=false;
            return Err(format!("REAL user Workspace postflight requires manual recovery: {reason}"));
        }
    }
    let live_membership_after=match live_membership_before.rebase_after_pins(
        &committed,&current).and_then(|generation|generation.snapshot(&current)){
        Ok(snapshot)=>snapshot,
        Err(reason)=>{
            app.save_allowed=false;
            return Err(format!("Committed Native membership refresh needs review: {reason}"));
        }
    };
    if prepared.offered_paths.iter().zip(&source_fingerprints).any(|(path,data)|
        std::fs::read(path).ok().as_deref()!=Some(data.as_slice())) {
        app.save_allowed=false;
        return Err("Native source shortcut changed during virtual-only gesture; saves suspended".into());
    }
    app.undo.push(before);
    if app.undo.len()>32{app.undo.remove(0);}
    app.state=committed;
    app.drag=None;
    app.selected=None;
    app.multi_selected.clear();
    Ok(serde_json::json!({
        "validated":true,
        "transport":"real-explorer-hook-to-main-gui-one-shot-channel",
        "source_files_count":prepared.offered_paths.len(),
        "destination_group_id":prepared.target_group,
        "destination_tab_id":prepared.target_tab,
        "destination_tab_items":app.state.group(prepared.target_group)
            .map(|g|g.tab().items.len()),
        "native_policy_revalidated_generation":live_membership_after.generation,
        "native_policy_source_active_count":live_membership_after.source_index_count,
        "native_policy_destination_active_count":live_membership_after.target_index_count,
        "native_policy_hidden_index_count":live_membership_after.hidden_indices.len(),
        "native_policy_recomputed_from_committed_state":true,
        "source_tab_items":app.state.group(visible.into_iter()
            .find(|id|*id!=prepared.target_group)
            .ok_or("Verified source group absent")?)
            .map(|g|g.tab().items.len()),
        "real_user_workspace_modified":is_live,
        "workspace_is_unique_temp_clone":!is_live,
        "independent_pre_transfer_checkpoint":recovery.as_ref()
            .map(|permit|permit.backup.clone()),
        "independent_expected_post_snapshot":recovery.as_ref()
            .map(|permit|permit.expected_post.clone()),
        "disk_state_matches_app_state":true,
        "native_hook_detached_before_commit":true,
        "source_shortcut_bytes_unchanged":true,
        "replayed_event_rejected":true,
        "genuine_explorer_ole_link_effect_checked":false
    }))
}

#[cfg(all(test,windows,feature="native-lab",feature="native-unsafe-drag-lab"))]
mod native_original_gui_clip_startup_tests {
    use super::*;
    #[test]
    fn original_gui_explorer_hook_requires_both_clip_and_real_native_ownership(){
        // A host window can look visually transparent yet still own all
        // mouse input before SetWindowRgn. Never rely on elapsed time alone.
        assert!(!native_original_gui_hook_clip_authorized(false,true,false));
        assert!(!native_original_gui_hook_clip_authorized(true,false,false));
        assert!(!native_original_gui_hook_clip_authorized(true,true,true));
        assert!(native_original_gui_hook_clip_authorized(true,true,false));
    }
}
#[cfg(all(windows,feature="native-lab",feature="native-unsafe-drag-lab"))]
pub(super) fn native_original_gui_hook_clip_authorized(
    clipped:bool,explorer_owns_source:bool,app_owns_source:bool,
)->bool{
    clipped && explorer_owns_source && !app_owns_source
}

/// Gate the first native Explorer subclass install on the *already painted*
/// original Main HWND region. A visible DesktopCraft host may initially own
/// the entire monitor; until host_after_frame(SetWindowRgn) runs, the icon
/// under the GUI is not a real clickable Explorer source. Never install an
/// invasive MouseDown hook while that native source is occluded.
#[cfg(all(windows,feature="native-lab",feature="native-unsafe-drag-lab"))]
pub(super) fn native_original_gui_source_is_explorer_exposed(
    app:&App, source_id:u64,
)->Result<bool,String>{
    if !app.host.as_ref().is_some_and(|h|!h.region.is_empty()){
        return Ok(false);
    }
    let source=app.state.group(source_id)
        .filter(|g|g.page==app.state.page && !g.rolled
            && g.active<g.tabs.len() && g.tab().folder.is_none())
        .ok_or("Real native source Box must be an active regular Tab")?;
    let source_path=&source.tab().items.first()
        .ok_or("Original UI has no genuine Explorer source icon")?.path;
    let view=shell::DesktopView::acquire()
        .ok_or("Explorer Desktop disappeared while gating original UI")?;
    if view.icons_visible()!=Some(true) {
        return Err("Native Explorer icons are no longer visible".into());
    }
    let icons=view.items();
    let mut sources=icons.iter().filter(|item|
        item.path.eq_ignore_ascii_case(source_path));
    let item=sources.next()
        .ok_or("Original GUI source icon not found on real Explorer Desktop")?;
    if sources.next().is_some(){
        return Err("Original GUI source path matched multiple native icons".into());
    }
    let x=item.pos.0.checked_add(24)
        .ok_or("Explorer native icon hit-test X overflow")?;
    let y=item.pos.1.checked_add(24)
        .ok_or("Explorer native icon hit-test Y overflow")?;
    let at=winapi::shared::windef::POINT{x,y};
    let under=unsafe{winapi::um::winuser::WindowFromPoint(at)};
    Ok(native_original_gui_hook_clip_authorized(
        app.host.as_ref().is_some_and(|h|!h.region.is_empty()),
        !under.is_null() && under==view.list_window(),
        under==host::window()))
}

/// Owner-specific live experiment excluded from distribution.
#[cfg(all(windows,feature="native-lab",feature="native-unsafe-drag-lab"))]
pub(super) fn prepare_original_live_native_chrome(
    _app:&App,_source:u64,_backup:&std::path::Path,_expected_post:&std::path::Path,
)->Result<(Vec<shell::ViewItem>,Vec<u64>),String>{
    Err("Owner-specific live workspace experiment is not included in this distribution.".into())
}
/// Called only after a unique OS TEMP copy of the user's Workspace was
/// opened. The exact original layout is never rewritten; this function
/// persists and returns a verified clone with one new empty regular Box.
#[cfg(all(windows,feature="native-lab"))]
pub(super) fn prepare_native_chrome_ole_copy(
    app:&mut App,source_id:u64,
)->Result<(Vec<shell::ViewItem>,Vec<u64>),String>{
    let temp=std::env::temp_dir().canonicalize().map_err(|e|e.to_string())?;
    let root=app.root.canonicalize().map_err(|e|e.to_string())?;
    if root.parent()!=Some(temp.as_path()) ||
        !root.file_name().is_some_and(|n|n.to_string_lossy()
            .starts_with("DesktopCraftExplorerChromeOLE-"))
        || app.save_path!=app.root.join("data/workspace.json") {
        return Err("Native Chrome OLE lab refuses any non-disposable Workspace".into());
    }
    let view=shell::DesktopView::acquire()
        .ok_or("Native Explorer Desktop view unavailable")?;
    if view.icons_visible()!=Some(true){
        return Err("Native Chrome OLE lab requires visible Explorer Desktop icons".into());
    }
    let shell=view.items();
    let names=explorer_owned_box::win::list_items(view.list_window())
        .into_iter().map(|item|item.name).collect::<Vec<_>>();
    let explorer=explorer_native::native_view_in_msaa_order(&shell,&names)?;
    let original=std::fs::read(&app.save_path).map_err(|e|e.to_string())?;
    if serde_json::to_vec(&State::load(&app.save_path)?).map_err(|e|e.to_string())?
        !=serde_json::to_vec(&app.state).map_err(|e|e.to_string())? {
        return Err("Native Chrome OLE original State diverged from cloned disk".into());
    }
    let (candidate,visible)=native_chrome_ole_clone_plan(&app.state,&explorer,source_id)?;
    candidate.save(&app.save_path)?;
    if serde_json::to_vec(&State::load(&app.save_path)?).map_err(|e|e.to_string())?
        !=serde_json::to_vec(&candidate).map_err(|e|e.to_string())?{
        return Err("Native Chrome OLE copy could not be verified after Save".into());
    }
    // A second original snapshot remains *inside the disposable TEMP tree*,
    // so the experiment can inspect the prior clone without touching real data.
    std::fs::write(root.join("native-original-workspace.snapshot"),original)
        .map_err(|e|e.to_string())?;
    app.state=candidate;
    Ok((explorer,visible))
}

/// Prepare the *normal* DesktopCraft Box/Tab UI with only disposable sample
/// files. The caller supplies a freshly allocated temporary root. Never use
/// this helper on a real Explorer Desktop or the user's persisted Workspace.
#[cfg(all(windows,feature="native-lab"))]
pub(super) fn prepare_isolated_native_main_hwnd_ole_fixture(
    app:&mut App,
    multi:bool,
)->Result<(Vec<shell::ViewItem>,Vec<u64>),String>{
    if app.desktop || app.host.is_some() || app.automation
        || !app.root.to_string_lossy().contains("DesktopCraftIsolatedMainOle-") {
        return Err("Main-HWND OLE lab must run in disposable window mode".into());
    }
    let paths=[
        "demo/Projects/Project brief.txt",
        "demo/Projects/Timeline.csv",
        "demo/Notes/Ideas.txt",
        "demo/Notes/Reading list.txt",
    ].map(|part|app.root.join(part));
    if paths.iter().any(|p|!p.is_file() || !p.starts_with(&app.root)) {
        return Err("Isolated OLE lab sample files were not created".into());
    }
    let mut model=State::empty();
    let from=model.add_group("Source — Lab",Bounds{
        x:80.,y:130.,w:330.,h:295.
    });
    let item_id=model.id();
    model.group_mut(from).unwrap().tab_mut().items.push(
        Item::from_path(&paths[0],item_id).map_err(|e|e.to_string())?);
    model.add_tab(from)?;
    let item_id=model.id();
    model.group_mut(from).unwrap().tab_mut().items.push(
        Item::from_path(&paths[1],item_id).map_err(|e|e.to_string())?);
    model.group_mut(from).unwrap().active=0;
    if multi {
        // Keep both distinct real temp files in the same active source Tab.
        // The inactive Tab stays present so native multi-Tab policy holds.
        let source=model.group_mut(from).unwrap();
        let second=source.tabs[1].items.remove(0);
        source.tabs[0].items.push(second);
    }
    let to=model.add_group("Destination — Lab",Bounds{
        x:510.,y:130.,w:330.,h:295.
    });
    let item_id=model.id();
    model.group_mut(to).unwrap().tab_mut().items.push(
        Item::from_path(&paths[2],item_id).map_err(|e|e.to_string())?);
    model.add_tab(to)?;
    let item_id=model.id();
    model.group_mut(to).unwrap().tab_mut().items.push(
        Item::from_path(&paths[3],item_id).map_err(|e|e.to_string())?);
    model.group_mut(to).unwrap().active=0;
    model.save(&app.save_path)?;
    app.state=model;
    let explorer=paths.iter().enumerate().map(|(index,path)|
        shell::ViewItem{
            path:path.to_string_lossy().into_owned(),
            label:path.file_name().unwrap().to_string_lossy().into_owned(),
            pos:(index as i32*100,0),
        }).collect::<Vec<_>>();
    Ok((explorer,vec![from,to]))
}


#[cfg(all(test,windows,feature="native-lab"))]
mod isolated_native_ole_main_window_tests {
    use super::*;
    #[test]
    fn existing_app_state_and_original_box_ui_are_valid_ole_targets() {
        use std::sync::atomic::{AtomicU64,Ordering};
        static SEQ:AtomicU64=AtomicU64::new(1);
        let root=std::env::temp_dir().join(format!(
            "DesktopCraftIsolatedMainOle-{}-{}",
            std::process::id(),SEQ.fetch_add(1,Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        create_demo(&root).unwrap();
        let mut app=App::new(root.clone(),false,false);
        assert!(app.host.is_none());
        let (icons,ids)=prepare_isolated_native_main_hwnd_ole_fixture(&mut app,false).unwrap();
        assert_eq!(ids.len(),2);
        assert_eq!(icons.len(),4);
        assert!(icons.iter().all(|it|std::path::Path::new(&it.path).starts_with(&root)));
        let before=State::load(&app.save_path).unwrap();
        assert_eq!(serde_json::to_vec(&before).unwrap(),
            serde_json::to_vec(&app.state).unwrap());
        assert!(app.state.groups.iter().all(|g|
            g.tabs.len()==2 && g.active==0 && !g.rolled && g.page==app.state.page));
        let native=explorer_native::native_multi_box_plan(&app.state,&icons,4,8).unwrap();
        assert_eq!(native.groups.len(),2);
        let target=app.state.group(ids[1]).unwrap().tab().id;
        let virtual_drop=explorer_native::plan_native_ole_drop(
            &app.state,&icons,&native,&[PathBuf::from(&icons[0].path)],
            ids[1],target,4,8);
        assert!(virtual_drop.is_ok());
        // Pure model plan never writes the saved sample Workspace.
        assert_eq!(serde_json::to_vec(&State::load(&app.save_path).unwrap()).unwrap(),
            serde_json::to_vec(&before).unwrap());
        assert!(prepare_isolated_native_main_hwnd_ole_fixture(&mut App::new(
            root.join("NotIsolated"),false,false),false).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
