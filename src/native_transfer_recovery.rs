//! Explicit per-gesture recovery evidence for a REAL Workspace Pin move.
//! Construction is read-only except for a NEW expected-post image. The
//! original checkpoint remains immutable, and a later rollback may proceed
//! only when the active Workspace EXACTLY matches this expected image.
//!
//! This unit never authorizes a native gesture: authorization comes from the
//! in-process verified one-shot Explorer hook on the original Main UI thread.
use std::{fs::{self,OpenOptions},io::Write,path::{Path,PathBuf}};
use crate::{model::State,native_drag_channel::PreparedNativeGesture,shell::ViewItem,
            explorer_native};

#[derive(Debug)]
pub(crate) struct VerifiedPinTransferRecovery {
    pub(crate) backup:PathBuf,
    pub(crate) expected_post:PathBuf,
    pub(crate) previous:Vec<u8>,
    pub(crate) next:Vec<u8>,
    pub(crate) source_group:u64,
    pub(crate) target_group:u64,
}
impl VerifiedPinTransferRecovery {
    pub(crate) fn prepare(
        workspace:&Path,backup:&Path,expected_post:&Path,
        before:&State,
        offered:&PreparedNativeGesture,
        native_desktop:&[ViewItem],
        allowed_source:u64,allowed_dest:u64
    )->Result<Self,String>{
        if !workspace.is_absolute() || !backup.is_absolute() || !expected_post.is_absolute() {
            return Err("Verified native transfer requires absolute Workspace/backup/post-image paths".into());
        }
        let ws=workspace.canonicalize().map_err(|e|e.to_string())?;
        let base=backup.canonicalize().map_err(|e|format!("Missing verified native Pin backup: {e}"))?;
        let dir=expected_post.parent().ok_or("Missing post-image archive directory")?
            .canonicalize().map_err(|e|e.to_string())?;
        let output=dir.join(expected_post.file_name().ok_or("Post-image filename missing")?);
        if ws==base || ws==output || base==output {
            return Err("Original, backup and expected image must all be distinct".into());
        }
        if allowed_source==allowed_dest || allowed_source==0 || allowed_dest==0 ||
            offered.target_group!=allowed_dest {
            return Err("Native recovery attempt targets an unapproved Box".into());
        }
        let original=fs::read(&ws).map_err(|e|format!("Cannot read Workspace: {e}"))?;
        let checkpoint=fs::read(&base).map_err(|e|format!("Cannot read verified backup: {e}"))?;
        if original!=checkpoint {
            return Err("Native transfer checkpoint does not match CURRENT saved Workspace".into());
        }
        let from_disk=State::from_saved_bytes(&original)?;
        if serde_json::to_vec(&from_disk).map_err(|e|e.to_string())?
            !=serde_json::to_vec(before).map_err(|e|e.to_string())? {
            return Err("Native UI no longer equals saved checkpoint".into());
        }
        let after=&offered.next_state;
        if before.page!=after.page || before.groups.len()!=after.groups.len() ||
            before.pages!=after.pages {
            return Err("Native Pin transfer changed Page/Box topology".into());
        }
        // Exact original Group order and every non-participating Box must
        // remain byte-equivalent, including inactive Tabs, Portals and Rules.
        let mut prior=serde_json::to_value(before).map_err(|e|e.to_string())?;
        let mut future=serde_json::to_value(after).map_err(|e|e.to_string())?;
        for data in [&mut prior,&mut future] {
            if let Some(groups)=data.get_mut("groups").and_then(|v|v.as_array_mut()) {
                for group in groups {
                    if group.get("id")==Some(&serde_json::json!(allowed_source))
                        || group.get("id")==Some(&serde_json::json!(allowed_dest)) {
                        if let Some(active)=group.get("active").and_then(|v|v.as_u64()) {
                            if let Some(tabs)=group.get_mut("tabs").and_then(|v|v.as_array_mut()){
                                if let Some(tab)=tabs.get_mut(active as usize) {
                                    if let Some(obj)=tab.as_object_mut(){
                                        obj.remove("items");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if prior!=future {
            return Err("Native Pin transfer changed unrelated Box/Tab or preference data".into());
        }
        let old_source=before.group(allowed_source)
            .ok_or("Approved source Group no longer exists")?.tab();
        let new_source=after.group(allowed_source)
            .ok_or("Approved source Group disappeared")?.tab();
        let old_target=before.group(allowed_dest)
            .ok_or("Approved destination Group no longer exists")?.tab();
        let new_target=after.group(allowed_dest)
            .ok_or("Approved destination Group disappeared")?.tab();
        if old_source.folder.is_some() || new_source.folder.is_some() ||
            old_target.folder.is_some() || new_target.folder.is_some() ||
            old_target.items.len()!=0 || old_source.items.len()<offered.offered_paths.len() ||
            new_source.items.len()+offered.offered_paths.len()!=old_source.items.len() ||
            new_target.items.len()!=offered.offered_paths.len() {
            return Err("Approved Pins or regular active Tab ownership changed".into());
        }
        let mut remaining=old_source.items.iter()
            .map(|pin|pin.path.to_lowercase()).collect::<Vec<_>>();
        for moved in &offered.offered_paths {
            let needle=moved.to_string_lossy().to_lowercase();
            let pos=remaining.iter().position(|path|path==&needle)
                .ok_or("Native move offered a shortcut outside approved Source")?;
            remaining.remove(pos);
        }
        let new_remaining=new_source.items.iter()
            .map(|pin|pin.path.to_lowercase()).collect::<Vec<_>>();
        let actual_target=new_target.items.iter()
            .map(|pin|pin.path.to_lowercase()).collect::<Vec<_>>();
        let offered_target=offered.offered_paths.iter()
            .map(|pin|pin.to_string_lossy().to_lowercase()).collect::<Vec<_>>();
        if remaining!=new_remaining || actual_target!=offered_target {
            return Err("Native moved Pin path identity/order mismatch".into());
        }
        let revalidated=explorer_native::native_multi_box_plan(after,native_desktop,16,128)?;
        if !revalidated.groups.iter().any(|g|g.group_id==allowed_source)
            || !revalidated.groups.iter().any(|g|g.group_id==allowed_dest) {
            return Err("After-transfer source/dest no longer hold verified native policy".into());
        }
        let post=serde_json::to_vec_pretty(after).map_err(|e|e.to_string())?;
        if post.len()>32*1024*1024 {return Err("Expected post transfer Workspace exceeds safe size".into());}
        let mut file=OpenOptions::new().write(true).create_new(true)
            .open(&output).map_err(|e|format!("Expected Pin post-image MUST be a new file: {e}"))?;
        let outcome=(||->Result<(),String>{
            file.write_all(&post).map_err(|e|e.to_string())?;
            file.sync_all().map_err(|e|e.to_string())?;
            if fs::read(&output).map_err(|e|e.to_string())?!=post {
                return Err("Expected post-transfer image not byte-for-byte verified".into());
            }
            if fs::read(&ws).map_err(|e|e.to_string())?!=original ||
                fs::read(&base).map_err(|e|e.to_string())?!=original {
                return Err("Native recovery original or live Workspace changed during staging".into());
            }
            Ok(())
        })();
        if outcome.is_err(){let _=fs::remove_file(&output);}
        outcome?;
        Ok(Self{backup:base,expected_post:output,previous:original,
            next:post,source_group:allowed_source,target_group:allowed_dest})
    }
    /// Confirm the live State matches the exact expected file after COM or
    /// debug-only real MouseDown transaction; never silently restore.
    pub(crate) fn confirm(&self,workspace:&Path)->Result<(),String>{
        if fs::read(workspace).map_err(|e|e.to_string())?!=self.next
            || fs::read(&self.expected_post).map_err(|e|e.to_string())?!=self.next
            || fs::read(&self.backup).map_err(|e|e.to_string())?!=self.previous {
            return Err("Native Pin transfer postflight or original checkpoint differs; manual review required".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::model::{Bounds,Item};
    struct Tmp{root:PathBuf,workspace:PathBuf,backup:PathBuf,expected:PathBuf,
               before:State,icons:Vec<ViewItem>,prepared:PreparedNativeGesture}
    impl Drop for Tmp{fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}
    impl Tmp{
        fn new()->Self{
            let root=std::env::temp_dir().join(format!("dc-native-pin-{}-{}",
                std::process::id(),std::time::SystemTime::now().duration_since(
                    std::time::UNIX_EPOCH).unwrap().as_nanos()));
            fs::create_dir_all(&root).unwrap();
            let workspace=root.join("workspace.json");
            let backup=root.join("before.json");
            let expected=root.join("expected.json");
            let mut before=State::empty();
            let source=before.add_group("Source",Bounds{x:100.,y:100.,w:320.,h:240.});
            for (i,path) in ["C:/Desktop/A.lnk","C:/Desktop/B.lnk"].iter().enumerate(){
                before.group_mut(source).unwrap().tab_mut().items.push(Item{
                    id:100+i as u64,name:path.to_string(),path:path.to_string(),
                    kind:"lnk".into(),size:0,modified:0,label:None});
            }
            before.add_tab(source).unwrap();
            before.group_mut(source).unwrap().active=0;
            let dest=before.add_group("Dest",Bounds{x:500.,y:100.,w:320.,h:240.});
            before.add_tab(dest).unwrap();
            before.group_mut(dest).unwrap().active=0;
            before.save(&workspace).unwrap();
            fs::copy(&workspace,&backup).unwrap();
            let icons=["A","B","Loose"].iter().enumerate().map(|(i,label)|ViewItem{
                path:format!("C:/Desktop/{label}.lnk"),label:label.to_string(),
                pos:(50+i as i32*100,40)}).collect::<Vec<_>>();
            let policy=explorer_native::native_multi_box_plan(&before,&icons,16,128).unwrap();
            let (next_state,next_plan,target_group,target_tab)=
                explorer_native::plan_native_selected_drag_release(
                    &before,&icons,&policy,&[0,1],650,220,16,128).unwrap();
            assert_eq!((source,dest),(1,4));
            let prepared=PreparedNativeGesture{
                offered_paths:vec!["C:/Desktop/A.lnk".into(),"C:/Desktop/B.lnk".into()],
                target_group,target_tab,next_state,next_plan
            };
            Self{root,workspace,backup,expected,before,icons,prepared}
        }
        fn prepare(&self)->Result<VerifiedPinTransferRecovery,String>{
            VerifiedPinTransferRecovery::prepare(&self.workspace,&self.backup,
                &self.expected,&self.before,&self.prepared,&self.icons,1,4)
        }
    }
    #[test]
    fn immutable_pre_and_expected_after_image_require_exact_commit(){
        let case=Tmp::new();
        let old=fs::read(&case.workspace).unwrap();
        let permit=case.prepare().unwrap();
        assert_eq!(fs::read(&case.workspace).unwrap(),old);
        assert_eq!(fs::read(&case.backup).unwrap(),old);
        assert_eq!(fs::read(&case.expected).unwrap(),permit.next);
        assert!(case.prepare().is_err(),"Existing expected-post must never be overwritten");
        assert!(permit.confirm(&case.workspace).is_err());
        case.prepared.next_state.save(&case.workspace).unwrap();
        permit.confirm(&case.workspace).unwrap();
        assert_eq!(fs::read(&case.backup).unwrap(),old);
    }
    #[test]
    fn reject_stale_backup_wrong_target_and_unrelated_box_settings(){
        let mut case=Tmp::new();
        assert!(VerifiedPinTransferRecovery::prepare(&case.workspace,&case.backup,
            &case.expected,&case.before,&case.prepared,&case.icons,1,900).is_err());
        case.prepared.next_state.snap=!case.before.snap;
        assert!(case.prepare().is_err());
        case.prepared.next_state.snap=case.before.snap;
        case.prepared.offered_paths[0]="C:/Desktop/Bad.lnk".into();
        assert!(case.prepare().is_err());
        case.prepared.offered_paths[0]="C:/Desktop/A.lnk".into();
        let mut external=case.before.clone();
        external.add_group("External Edit",Bounds{x:1000.,y:100.,w:200.,h:200.});
        external.save(&case.workspace).unwrap();
        assert!(case.prepare().is_err());
        assert!(!case.expected.exists());
    }
}
