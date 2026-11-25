use super::*;

pub(super) struct VerifiedRecoverySets {
    pub(super) placed: BTreeSet<String>,
    pub(super) nudged_restore: BTreeSet<String>,
}

/// A crash between moves and the final ACK leaves the previous checkpoint,
/// so automated recovery refuses any unacknowledged coordinates. The original
/// icon baseline remains inside the journal across every generation.
pub(super) fn verified_recovery_plan(
    journal:&Value, icons:&[ViewItem],
)->Result<VerifiedRecoverySets,String>{
    if journal["auto_restore_requires_exact_ack"]!=true
        || journal["last_ack_generation"].as_u64().unwrap_or(0)==0 {
        return Err("No committed exact-position generation; manual review required".into());
    }
    let rows=journal["last_verified_positions"].as_array()
        .ok_or("Missing exact acknowledged Explorer positions")?;
    if rows.len()!=icons.len() {
        return Err("Acknowledged icon count does not match live Explorer".into());
    }
    let mut expected=BTreeMap::new();
    for row in rows {
        let (Some(path),Some(label),Some(x),Some(y))=(
            row["path"].as_str(),row["label"].as_str(),
            row["x"].as_i64(),row["y"].as_i64()
        )else{return Err("Malformed acknowledged icon position".into());};
        let (Ok(x),Ok(y))=(i32::try_from(x),i32::try_from(y))else{
            return Err("Acknowledged icon coordinate overflow".into());
        };
        if expected.insert(path.to_lowercase(),(label.to_string(),(x,y))).is_some(){
            return Err("Duplicate acknowledged icon identity".into());
        }
    }
    let originals=journal["original_icons"].as_array()
        .ok_or("Missing original icon baseline")?;
    if originals.len()!=icons.len() {return Err("Original baseline count mismatch".into());}
    let mut original=BTreeMap::new();
    for row in originals {
        let (Some(path),Some(label),Some(x),Some(y))=(
            row["path"].as_str(),row["label"].as_str(),
            row["x"].as_i64(),row["y"].as_i64()
        )else{return Err("Malformed original icon baseline".into());};
        let (Ok(x),Ok(y))=(i32::try_from(x),i32::try_from(y))else{
            return Err("Original icon coordinate overflow".into());
        };
        if original.insert(path.to_lowercase(),(label.to_string(),(x,y))).is_some(){
            return Err("Duplicate original icon identity".into());
        }
    }
    if expected.keys().ne(original.keys()){
        return Err("Acknowledged identities changed from original baseline".into());
    }
    let mut nudged=BTreeMap::new();
    match journal.get("nudged_loose_positions") {
        None | Some(Value::Null) => {}
        Some(Value::Array(entries)) => {
            for row in entries {
                let (Some(path),Some(x),Some(y))=(
                    row["path"].as_str(),row["x"].as_i64(),row["y"].as_i64()
                )else{return Err("Malformed nudged loose icon position".into());};
                let (Ok(x),Ok(y))=(i32::try_from(x),i32::try_from(y))else{
                    return Err("Nudged loose icon coordinate overflow".into());
                };
                let key=path.to_lowercase();
                if expected.get(&key).is_none_or(|(_,p)|*p!=(x,y)) {
                    return Err("Nudged loose icon differs from exact acknowledged generation".into());
                }
                if nudged.insert(key,(x,y)).is_some() {
                    return Err("Duplicate nudged loose icon identity".into());
                }
            }
        }
        Some(_) => return Err("Malformed nudged loose icon checkpoint".into()),
    }
    let placed = expected.iter()
        .filter(|(path, (_, point))| original[*path].1 != *point && !nudged.contains_key(*path))
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    let mut nudged_restore=BTreeSet::new();
    let mut seen = BTreeSet::new();
    for icon in icons {
        let key=icon.path.to_lowercase();
        if !seen.insert(key.clone()) {
            return Err("Duplicate live Explorer icon identity".into());
        }
        let (label,expected_pos)=expected.get(&key)
            .ok_or("Missing acknowledged icon")?;
        let (old_label,old_pos)=original.get(&key)
            .ok_or("Missing original icon")?;
        if &icon.label!=label || &icon.label!=old_label {
            return Err("Explorer icon label changed since ACK".into());
        }
        if placed.contains(&key) && icon.pos!=*expected_pos && icon.pos!=*old_pos {
            return Err(format!(
                "Icon coordinate differs from original AND exact ACK: {}; do not overwrite possible user drag",
                icon.path));
        }
        if nudged.contains_key(&key) && icon.pos==*expected_pos && icon.pos!=*old_pos {
            nudged_restore.insert(key);
        }
    }
    Ok(VerifiedRecoverySets { placed, nudged_restore })
}

pub(super) fn verified_recovery_positions(
    journal:&Value, icons:&[ViewItem],
)->Result<BTreeSet<String>,String>{
    verified_recovery_plan(journal, icons).map(|sets| sets.placed)
}

pub(super) fn replace_verified_journal(journal:&std::path::Path,bytes:&[u8])
    ->Result<(),String>{
    use std::{io::Write,os::windows::ffi::OsStrExt};
    let stamp=std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?
        .as_nanos();
    let next=journal.with_extension(format!("next-{}-{stamp}.tmp",std::process::id()));
    let backup=journal.with_extension(format!("prev-{}-{stamp}.bak",std::process::id()));
    let mut file=std::fs::OpenOptions::new().create_new(true).write(true)
        .open(&next).map_err(|e|e.to_string())?;
    file.write_all(bytes).and_then(|()|file.sync_all())
        .map_err(|e|e.to_string())?;
    drop(file);
    #[link(name="kernel32")]
    extern "system" {
        fn ReplaceFileW(replaced:*const u16,replacement:*const u16,
            backup:*const u16,flags:u32,
            exclude:*mut std::ffi::c_void,reserved:*mut std::ffi::c_void)->i32;
    }
    let wide=|p:&std::path::Path| p.as_os_str().encode_wide()
        .chain(std::iter::once(0)).collect::<Vec<u16>>();
    let target=wide(journal);let stage=wide(&next);let prior=wide(&backup);
    let replaced=unsafe{ReplaceFileW(target.as_ptr(),stage.as_ptr(),
        prior.as_ptr(),0,std::ptr::null_mut(),std::ptr::null_mut())}!=0;
    if !replaced {
        let reason=std::io::Error::last_os_error();
        if !journal.exists() && backup.exists(){
            std::fs::rename(&backup,journal).map_err(|e|
                format!("Journal replacement failed ({reason}); backup recovery failed ({e}); preserve {}",backup.display()))?;
        }
        if journal.exists(){let _=std::fs::remove_file(&next);}
        return Err(format!("Could not atomically commit exact native positions: {reason}"));
    }
    let _=std::fs::remove_file(&backup);
    Ok(())
}

#[cfg(test)]
mod acknowledged_recovery_tests {
    use super::*;

    fn icon(path:&str,at:(i32,i32))->ViewItem{
        ViewItem{path:path.to_owned(),label:path.to_owned(),pos:at}
    }
    fn acknowledged()->Value{
        json!({
            "auto_restore_requires_exact_ack":true,
            "last_ack_generation":1,
            "original_icons":[
                {"path":"A","label":"A","x":10,"y":10},
                {"path":"B","label":"B","x":20,"y":20}
            ],
            "last_verified_positions":[
                {"path":"A","label":"A","x":100,"y":100},
                {"path":"B","label":"B","x":20,"y":20}
            ]
        })
    }
    #[test]
    fn automatic_recovery_accepts_only_original_or_exact_acknowledged_cells(){
        let prior=acknowledged();
        assert!(verified_recovery_positions(&prior,&[
            icon("A",(100,100)),icon("B",(20,20))]).is_ok());
        assert!(verified_recovery_positions(&prior,&[
            icon("A",(10,10)),icon("B",(20,20))]).is_ok());
        // This coordinate would previously have been accepted merely for
        // lying inside a Box. It must NEVER be auto-overwritten.
        assert!(verified_recovery_positions(&prior,&[
            icon("A",(112,100)),icon("B",(20,20))]).is_err());
    }
    #[test]
    fn automatic_recovery_refuses_legacy_or_incomplete_checkpoint(){
        let mut data=acknowledged();
        data.as_object_mut().unwrap().remove("last_ack_generation");
        assert!(verified_recovery_positions(&data,&[
            icon("A",(100,100)),icon("B",(20,20))]).is_err());
        let mut data=acknowledged();
        data["last_verified_positions"]=json!([
            {"path":"A","label":"A","x":100,"y":100}
        ]);
        assert!(verified_recovery_positions(&data,&[
            icon("A",(100,100)),icon("B",(20,20))]).is_err());
    }

    #[test]
    fn recovery_preserves_a_loose_move_while_restoring_placed_icons() {
        let current = [icon("A", (100, 100)), icon("B", (200, 300))];
        let placed = verified_recovery_positions(&acknowledged(), &current).unwrap();
        assert_eq!(placed, BTreeSet::from(["a".to_string()]));
        let planned = rollback_targets_clear(&current, &[("A".to_string(), (10, 10))]).unwrap();
        assert_eq!(planned[0].pos, (10, 10));
        assert_eq!(planned[1].pos, (200, 300));
    }

    #[test]
    fn recovery_skips_blocked_and_duplicate_targets_and_still_restores_free_cells() {
        let current = [icon("A", (100, 100)), icon("B", (200, 200)),
            icon("C", (300, 300)), icon("D", (400, 400)), icon("E", (500, 500)),
            icon("Stationary", (10, 10))];
        let mut moves = vec![("B".into(), (100, 100)), ("A".into(), (10, 10)),
            ("C".into(), (30, 30)), ("D".into(), (30, 30)), ("E".into(), (50, 50))];
        let retained = retain_icons_with_occupied_restore_targets(&current, &mut moves);
        assert_eq!(retained, BTreeMap::from([("a".into(), (100, 100)),
            ("b".into(), (200, 200)), ("d".into(), (400, 400))]));
        let restored = rollback_targets_clear(&current, &moves).unwrap();
        assert_eq!(restored.iter().map(|i| i.pos).collect::<Vec<_>>(),
            [(100, 100), (200, 200), (30, 30), (400, 400), (50, 50), (10, 10)]);
    }

    #[test]
    fn loose_move_updates_baseline_without_accepting_a_managed_move() {
        let mut original = [icon("A", (10, 10)), icon("B", (20, 20))];
        let mut live_layout = [icon("A", (100, 100)), icon("B", (20, 20))];
        let placed = [("A".to_string(), (100, 100))];
        let loose_move = [icon("A", (100, 100)), icon("B", (200, 300))];
        assert!(adopt_loose_positions(&mut original, &mut live_layout, &placed, &loose_move).unwrap());
        assert_eq!(original[0].pos, (10, 10));
        assert_eq!(original[1].pos, (200, 300));
        assert!(same_icon_positions(&live_layout, &loose_move));
        let managed_move = [icon("A", (99, 100)), icon("B", (300, 400))];
        assert!(adopt_loose_positions(&mut original, &mut live_layout, &placed, &managed_move).is_err());
        assert!(same_icon_positions(&live_layout, &loose_move));
    }
    #[test]
    fn atomic_journal_generation_replacement_preserves_original_content_contract(){
        let dir=std::env::temp_dir().join(format!(
            "desktopcraft-ack-journal-test-{}",std::process::id()));
        let _=std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file=dir.join("native-inbox-recovery.json");
        std::fs::write(&file,b"original checkpoint").unwrap();
        replace_verified_journal(&file,b"new checkpoint with exact positions").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(),b"new checkpoint with exact positions");
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// An old journal is NOT permission to overwrite user icon positions.
/// The safe inspection refuses changed display topology or missing identities.
/// Unrelated Explorer moves are preserved during recovery.
/// Actual repair additionally requires the explicit CLI risk confirmation.
pub fn recovery_status_or_restore(view:&DesktopView,apply:bool,reconcile_grid:bool)->Result<Value,String>{
    recovery_status_or_restore_policy(view,apply,false,reconcile_grid)
}

/// Replacement-Explorer automation has narrower authority than explicit
/// manual recovery. It may only restore *exact* last-acknowledged positions.
pub fn automatic_recovery_status_or_restore(view:&DesktopView,apply:bool)
    ->Result<Value,String>{
    recovery_status_or_restore_policy(view,apply,true,false)
}

pub(super) fn recovery_status_or_restore_policy(
    view:&DesktopView,apply:bool,automatic:bool,reconcile_grid:bool)->Result<Value,String>{
    let journal=host::data_dir().join("native-inbox-recovery.json");
    if !journal.exists(){
        return Ok(json!({"pending":false,"restored":false,"journal_path":journal}));
    }
    let bytes=std::fs::read(&journal).map_err(|e|e.to_string())?;
    let data:Value=serde_json::from_slice(&bytes)
        .map_err(|e|format!("Native recovery checkpoint invalid: {e}"))?;
    if data["format"]!=1 || data["monitor_signature"]!=
        host::signature(&host::monitors()){
        return Err("Native recovery monitor/format mismatch; manual icon review required".into());
    }
    let originals=data["original_icons"].as_array()
        .ok_or("Native journal icon list malformed")?;
    let allowed=data["allowed_box_paths"].as_array()
        .ok_or("Native journal allowed owners missing")?
        .iter().filter_map(|v|v.as_str().map(str::to_lowercase))
        .collect::<BTreeSet<_>>();
    let rectangles=data["allowed_box_bounds"].as_array()
        .ok_or("Native journal Box rectangle list missing")?;
    if originals.is_empty() || originals.len()>crate::native_filter_protocol::MAX_ITEMS
        || rectangles.is_empty() {
        return Err("Native recovery journal bounded identity preflight refused".into());
    }
    let current=view.items();
    let recovered_checkpoint = if reconcile_grid {
        if data.get("snap_to_grid").is_some() || data["last_ack_generation"] != 1
            || data["explorer_pid_before"] != desktop_owner(view)?.pid
            || view.snap_to_grid() != Some(true) {
            return Err("Legacy grid recovery requires the first checkpoint and the same Explorer with Snap to grid active".into());
        }
        let cell = view.spacing().ok_or("Explorer grid spacing unavailable")?;
        Some(reconcile_grid_checkpoint(&data, &current, cell, &current_screens())?)
    } else {
        None
    };
    // A confirmed manual command still cannot establish whether an
    // unacknowledged coordinate belongs to a user drag. Require the exact
    // verified generation before any command can write icon coordinates.
    let verified = (automatic || apply || reconcile_grid || data["last_ack_generation"].is_number())
        .then(|| verified_recovery_plan(recovered_checkpoint.as_ref().unwrap_or(&data), &current))
        .transpose()?;
    let mut old=BTreeMap::new();
    for icon in originals{
        let (Some(path),Some(label),Some(x),Some(y))=(
            icon["path"].as_str(),icon["label"].as_str(),
            icon["x"].as_i64(),icon["y"].as_i64()) else {
            return Err("Native recovery journal icon row invalid".into());
        };
        let (Ok(x),Ok(y))=(i32::try_from(x),i32::try_from(y)) else{
            return Err("Native recovery coordinate out of range".into());
        };
        if old.insert(path.to_lowercase(),(label.to_string(),(x,y))).is_some(){
            return Err("Duplicate native recovery path identity".into());
        }
    }
    if current.len()!=old.len() || current.iter().any(|icon|
        old.get(&icon.path.to_lowercase())
            .is_none_or(|(label,_)|label!=&icon.label)){
        return Err("Native recovery Shell files/labels changed; refusing coordinate overwrite".into());
    }
    let any_moved_from_original = current.iter().any(|item| {
        old.get(&item.path.to_lowercase()).is_some_and(|(_, orig)| item.pos != *orig)
    });
    if verified.is_none() && any_moved_from_original {
        return Err("No committed exact-position generation; manual review required".into());
    }
    let hidden_park = data.get("hidden_park_point").and_then(|p| {
        Some((i32::try_from(p["x"].as_i64()?).ok()?, i32::try_from(p["y"].as_i64()?).ok()?))
    });
    let mut moved=Vec::new();
    let mut loose_moves_preserved = 0;
    for item in &current{
        let key = item.path.to_lowercase();
        let original=old.get(&key)
            .ok_or("Native recovery original not found")?.1;
        if item.pos==original{continue;}
        let Some(ref sets) = verified else { continue; };
        if sets.nudged_restore.contains(&key) {
            moved.push((item.path.clone(), original));
            continue;
        }
        if !sets.placed.contains(&key) {
            loose_moves_preserved += 1;
            continue;
        }
        if !allowed.contains(&key){
            return Err(format!("Native recovery has no authorized Box for {}", item.path));
        }
        let within_native_box=rectangles.iter().any(|b|{
            let coords=(b["x"].as_i64(),b["y"].as_i64(),
                b["w"].as_i64(),b["h"].as_i64());
            if let (Some(x),Some(y),Some(w),Some(h))=coords{
                (item.pos.0 as i64)>=x && (item.pos.0 as i64)<x+w
                && (item.pos.1 as i64)>=y && (item.pos.1 as i64)<y+h
            }else{false}
        });
        let at_hidden_park = hidden_park.is_some_and(|park| item.pos == park);
        if !within_native_box && !at_hidden_park{
            return Err(format!("Possible user-moved native icon outside old Box: {}; manual review required",item.path));
        }
        moved.push((item.path.clone(),original));
    }
    let retained = retain_icons_with_occupied_restore_targets(&current, &mut moved);
    let expected_after = rollback_targets_clear(&current, &moved)?;
    let mut restored=false;
    if apply{
        // Save the immediate pre-restore state in a second collision-safe
        // artifact before any IFolderView mutation.
        // A previous *successful* recovery intentionally leaves its immutable
        // before-image on disk. Never overwrite or reuse that fixed pathname:
        // later sessions need a new collision-safe artifact or they cannot
        // restore their own icons. create_new below is still mandatory.
        let recovery_copy=next_manual_recovery_preimage_path(&journal)?;
        let backup=json!({"prior_journal":data,"current":current.iter()
            .map(|i|json!({"path":i.path,"label":i.label,"position":i.pos}))
            .collect::<Vec<_>>()});
        let out=serde_json::to_vec_pretty(&backup).map_err(|e|e.to_string())?;
        std::fs::OpenOptions::new().write(true).create_new(true)
            .open(&recovery_copy).and_then(|mut file|{
                use std::io::Write;
                file.write_all(&out).and_then(|()|file.sync_all())
            }).map_err(|e|format!("Manual native recovery preimage not saved: {e}"))?;
        // Flush the preimage first, then recheck immediately before the COM
        // write so disk I/O does not widen the window for a user drag.
        if host::signature(&host::monitors()) != data["monitor_signature"]
            || (reconcile_grid && view.snap_to_grid() != Some(true))
            || !same_icon_positions(&view.items(), &current) {
            return Err("Explorer monitor layout or icon positions changed during recovery; no overwrite".into());
        }
        if !moved.is_empty() && view.position(&moved)!=moved.len(){
            return Err("Manual native Explorer recovery positioned fewer icons than requested".into());
        }
        std::thread::sleep(Duration::from_millis(700));
        let after=view.items();
        if !same_icon_positions(&after, &expected_after){
            return Err("Manual recovery was incomplete; original journal retained".into());
        }
        if data["original_snap_to_grid"] == true && view.snap_to_grid() == Some(false) {
            let _ = view.set_snap_to_grid(true);
        }
        std::fs::remove_file(&journal).map_err(|e|e.to_string())?;
        restored=true;
    }
    Ok(json!({
        "pending":!restored,
        "journal_path":journal,
        "changed_box_icon_count":moved.len(),
        "occupied_targets_left_in_place":retained.len(),
        "all_unclaimed_icons_unchanged":true,
        "loose_icon_user_positions_preserved":loose_moves_preserved,
        "monitor_identity_verified":true,
        "manual_restoration_performed":restored,
        "legacy_grid_snap_verified":recovered_checkpoint.is_some(),
        "no_workspace_or_disk_file_changes":true,
    }))
}

/// Leave a managed icon at its verified current position if restoring it
/// would stack it onto an icon that stays in place. A retained icon can in
/// turn block another restore target, so check until the move list is stable.
pub(super) fn retain_icons_with_occupied_restore_targets(
    current: &[ViewItem], moves: &mut Vec<(String, (i32, i32))>,
) -> BTreeMap<String, (i32, i32)> {
    let mut retained = BTreeMap::new();
    let live = current.iter().map(|icon| (icon.path.to_lowercase(), icon.pos))
        .collect::<BTreeMap<_, _>>();
    let moving = moves.iter().map(|(path, _)| path.to_lowercase()).collect::<BTreeSet<_>>();
    let mut occupied = live.iter().filter(|(path, _)| !moving.contains(*path))
        .map(|(_, &pos)| pos).collect::<BTreeSet<_>>();
    loop {
        let before = moves.len();
        let mut claimed = BTreeSet::new();
        moves.retain(|(path, target)| {
            if !occupied.contains(target) && claimed.insert(*target) { return true; }
            let key = path.to_lowercase();
            if let Some(&position) = live.get(&key) {
                occupied.insert(position);
                retained.insert(key, position);
            }
            false
        });
        if moves.len() == before { return retained; }
    }
}

/// Prove that a restore target is still free before asking Explorer to move
/// anything, and record the untouched positions for the postflight check.
pub(super) fn rollback_targets_clear(
    current: &[ViewItem], moves: &[(String, (i32, i32))],
) -> Result<Vec<ViewItem>, String> {
    let targets = moves.iter().map(|(path, point)| (path.to_lowercase(), *point))
        .collect::<BTreeMap<_, _>>();
    let mut expected = current.to_vec();
    for icon in &mut expected {
        if let Some(&position) = targets.get(&icon.path.to_lowercase()) {
            icon.pos = position;
        } else if targets.values().any(|point| *point == icon.pos) {
            return Err("A loose Explorer icon occupies a recovery target".into());
        }
    }
    Ok(expected)
}

/// Only reconcile a whole, exactly predicted legacy snap. All non-placed
/// positions must still be original, and all placed positions must match
/// the nearest cell on the original monitor's independently observed grid.
pub(super) fn reconcile_grid_checkpoint(
    journal: &Value, current: &[ViewItem], cell: (i32, i32), screens: &[Rect],
) -> Result<Value, String> {
    fn point(row: &Value) -> Option<(i32, i32)> {
        Some((i32::try_from(row["x"].as_i64()?).ok()?, i32::try_from(row["y"].as_i64()?).ok()?))
    }
    fn snap(point: i32, origin: i32, step: i32) -> Result<i32, String> {
        let position = i64::from(point);
        let step = i64::from(step);
        let remainder = (position - i64::from(origin)).rem_euclid(step);
        if 2 * remainder == step {
            return Err("Ambiguous nearest Explorer grid cell".into());
        }
        i32::try_from(position - remainder + if 2 * remainder > step { step } else { 0 })
            .map_err(|_| "Explorer grid projection overflow".into())
    }
    let mut originals = BTreeMap::new();
    for row in journal["original_icons"].as_array().ok_or("Original icons unavailable")? {
        let key = row["path"].as_str().ok_or("Original icon path unavailable")?.to_lowercase();
        if originals.insert(key, point(row).ok_or("Original icon position unavailable")?).is_some() {
            return Err("Duplicate original Explorer icon".into());
        }
    }
    let live = current.iter().map(|icon| (icon.path.to_lowercase(), icon.pos))
        .collect::<BTreeMap<_, _>>();
    let mut adjusted = journal.clone();
    let rows = adjusted["last_verified_positions"].as_array_mut().ok_or("Checkpoint positions unavailable")?;
    let mut snapped_count = 0;
    for row in rows {
        let key = row["path"].as_str().ok_or("Checkpoint icon path unavailable")?.to_lowercase();
        let original = *originals.get(&key).ok_or("Checkpoint identity differs from original icon")?;
        let expected = point(row).ok_or("Checkpoint icon position unavailable")?;
        let projected = if expected == original {
            original
        } else {
            let screen = screens.iter().find(|screen| screen.contains(expected)).copied()
                .ok_or("Checkpoint icon outside connected monitors")?;
            let origin = observed_grid_origin(screen, originals.values().copied(), cell)?;
            let projected = (snap(expected.0, origin.0, cell.0)?, snap(expected.1, origin.1, cell.1)?);
            if !screen.contains(projected) {
                return Err("Projected Explorer icon went outside its monitor".into());
            }
            snapped_count += usize::from(projected != expected);
            projected
        };
        if live.get(&key) != Some(&projected) {
            return Err("Live Explorer coordinates differ from the exact legacy grid projection".into());
        }
        row["x"] = json!(projected.0);
        row["y"] = json!(projected.1);
    }
    if snapped_count < 2 {
        return Err("No whole legacy Explorer grid adjustment to recover".into());
    }
    Ok(adjusted)
}
