use super::*;

/// A previous recovery may have left its immutable before-image at the
/// legacy fixed filename. Use an event-specific name for every future
/// recovery, with a process-local monotonic suffix. create_new() at the
/// call site remains the final protection against any filesystem collision.
pub(super) fn next_manual_recovery_preimage_path(journal:&std::path::Path)->Result<PathBuf,String>{
    use std::sync::atomic::{AtomicU64,Ordering};
    static SEQUENCE:AtomicU64=AtomicU64::new(0);
    let nonce=std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e|format!("Recovery preimage timestamp unavailable: {e}"))?
        .as_nanos();
    let sequence=SEQUENCE.fetch_add(1,Ordering::Relaxed);
    Ok(journal.with_extension(format!(
        "before-manual-restore-{}-{nonce}-{sequence}.json",std::process::id())))
}

pub(super) fn next_archived_recovery_journal_path(journal:&std::path::Path)->Result<PathBuf,String>{
    use std::sync::atomic::{AtomicU64,Ordering};
    static ARCHIVE_SEQUENCE:AtomicU64=AtomicU64::new(0);
    let nonce=std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e|format!("Recovery archive timestamp unavailable: {e}"))?
        .as_nanos();
    let sequence=ARCHIVE_SEQUENCE.fetch_add(1,Ordering::Relaxed);
    Ok(journal.with_extension(format!(
        "archived-{}-{nonce}-{sequence}.json",std::process::id())))
}

/// Atomically archive an unresolved crash recovery journal to a unique sibling
/// `.archived-*.json` path without deleting its saved icon baseline.
pub fn archive_stale_recovery_journal(journal:&std::path::Path)->Result<PathBuf,String>{
    if !journal.exists(){
        return Err("Recovery journal does not exist".into());
    }
    let archived=next_archived_recovery_journal_path(journal)?;
    if archived.exists(){
        return Err("Archived recovery journal destination already exists".into());
    }
    std::fs::rename(journal,&archived)
        .map_err(|e|format!("Could not archive recovery journal: {e}"))?;
    Ok(archived)
}

/// User-confirmed interactive startup recovery when automatic exact-ACK
/// verification cannot proceed (e.g. Explorer icon spacing shifted before the
/// previous session ended). Restores Box-managed and nudged icons that still
/// exist in Explorer back to their original baseline positions when their
/// destination cells are clear of loose icons, restores original Align-to-Grid,
/// and archives the stale journal to a collision-safe sibling file.
pub fn recover_and_archive_stale_journal(
    view:&DesktopView,
    journal:&std::path::Path,
)->Result<PathBuf,String>{
    if view.icons_visible()==Some(false){
        let _=view.set_icons_visible(true);
        std::thread::sleep(Duration::from_millis(150));
    }
    if let Ok(bytes)=std::fs::read(journal){
        if let Ok(data)=serde_json::from_slice::<Value>(&bytes){
            if data["format"]==1 && data["monitor_signature"]==host::signature(&host::monitors()){
                let allowed=data["allowed_box_paths"].as_array()
                    .into_iter().flatten()
                    .filter_map(|v|v.as_str().map(str::to_lowercase))
                    .collect::<BTreeSet<_>>();
                let nudged=data["nudged_loose_positions"].as_array()
                    .into_iter().flatten()
                    .filter_map(|row|row["path"].as_str().map(str::to_lowercase))
                    .collect::<BTreeSet<_>>();
                let mut originals=BTreeMap::new();
                if let Some(rows)=data["original_icons"].as_array(){
                    for row in rows {
                        if let (Some(path),Some(x),Some(y))=(
                            row["path"].as_str(),row["x"].as_i64(),row["y"].as_i64()
                        ){
                            if let (Ok(x),Ok(y))=(i32::try_from(x),i32::try_from(y)){
                                originals.insert(path.to_lowercase(),(x,y));
                            }
                        }
                    }
                }
                let current=view.items();
                let occupied_loose=current.iter()
                    .filter(|item|{
                        let key=item.path.to_lowercase();
                        !allowed.contains(&key) && !nudged.contains(&key)
                    })
                    .map(|item|item.pos)
                    .collect::<BTreeSet<_>>();
                let restore_moves=current.iter()
                    .filter_map(|item|{
                        let key=item.path.to_lowercase();
                        if !allowed.contains(&key) && !nudged.contains(&key){
                            return None;
                        }
                        let &orig=originals.get(&key)?;
                        (item.pos!=orig && !occupied_loose.contains(&orig))
                            .then(||(item.path.clone(),orig))
                    })
                    .collect::<Vec<_>>();
                if !restore_moves.is_empty(){
                    for batch in order_rollback_batches(&current,&restore_moves){
                        let _=view.position(&batch);
                    }
                    std::thread::sleep(Duration::from_millis(120));
                }
            }
            if data["original_snap_to_grid"]==true && view.snap_to_grid()==Some(false){
                let _=view.set_snap_to_grid(true);
                std::thread::sleep(Duration::from_millis(120));
            }
        }
    }
    archive_stale_recovery_journal(journal)
}

#[cfg(test)]
mod manual_recovery_preimage_tests{
    use super::*;
    #[test]
    fn separate_sessions_never_reuse_legacy_preimage_name(){
        let journal=std::path::Path::new(
            "C:/Test/Desktop Spaces/native-inbox-recovery.json");
        let old=journal.with_extension("before-manual-restore.json");
        let a=next_manual_recovery_preimage_path(journal).unwrap();
        let b=next_manual_recovery_preimage_path(journal).unwrap();
        assert_ne!(a,b);
        assert_ne!(a,old);
        assert_ne!(b,old);
        assert!(a.to_string_lossy().ends_with(".json"));
        assert!(b.to_string_lossy().ends_with(".json"));
    }

    #[test]
    fn archive_stale_recovery_journal_preserves_bytes_and_clears_active_path(){
        let dir=std::env::temp_dir().join(format!(
            "desktopcraft-archive-journal-test-{}",std::process::id()));
        let _=std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let journal=dir.join("native-inbox-recovery.json");
        assert!(archive_stale_recovery_journal(&journal).is_err());
        let payload=b"{\"format\":1,\"original_snap_to_grid\":true}";
        std::fs::write(&journal,payload).unwrap();
        let archived=archive_stale_recovery_journal(&journal).unwrap();
        assert!(!journal.exists());
        assert!(archived.exists());
        assert!(archived.file_name().unwrap().to_string_lossy().starts_with("native-inbox-recovery.archived-"));
        assert_eq!(std::fs::read(&archived).unwrap(),payload);
        let _=std::fs::remove_dir_all(&dir);
    }
}

