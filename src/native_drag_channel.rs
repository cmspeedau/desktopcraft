//! One-shot, in-process delivery of a *verified* native Explorer drag gesture.
//! JSON, a file, a CLI argument or a named shared-memory object cannot
//! authorize a DesktopCraft Workspace write. Only the attached native
//! Explorer hook worker may offer an event on this bounded channel.
//!
//! All checks are repeated on the UI thread, and a failed or stale event is
//! consumed rather than becoming a second chance to move files.
use std::{path::PathBuf, sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
          time::{Duration, Instant}};
use crate::{explorer_native::{self, NativeMultiBoxPlan}, model::State, shell::ViewItem};

pub(crate) const GESTURE_MAX_AGE:Duration=Duration::from_secs(5);
pub(crate) const MAX_GESTURE_PINS:usize=8;

#[derive(Debug,Clone)]
pub(crate) struct NativeGestureEvidence{
    pub(crate) observed_at:Instant,
    pub(crate) explorer_pid:u32,
    pub(crate) listview_hwnd:u64,
    pub(crate) monitor_signature:String,
    pub(crate) workspace_before:Vec<u8>,
    pub(crate) observed_desktop:Vec<(String,(i32,i32))>,
    pub(crate) selected_indices:Vec<usize>,
    pub(crate) source_native_index:usize,
    pub(crate) start_physical:(i32,i32),
    pub(crate) released_physical:(i32,i32),
    pub(crate) observed_moves:u32,
    pub(crate) hook_detached:bool,
    pub(crate) icon_positions_unchanged:bool,
}

#[derive(Debug)]
pub(crate) struct PreparedNativeGesture{
    pub(crate) offered_paths:Vec<PathBuf>,
    pub(crate) target_group:u64,
    pub(crate) target_tab:u64,
    pub(crate) next_state:State,
    pub(crate) next_plan:NativeMultiBoxPlan,
}

impl NativeGestureEvidence {
    /// Pure second-side acceptance. No mouse calls, hook, save or file moves.
    /// The UI must use an independently obtained live Explorer snapshot.
    pub(crate) fn validate(
        &self, now:Instant, explorer_pid:u32, listview_hwnd:u64,
        monitor_signature:&str, saved_workspace_bytes:&[u8],
        state:&State, current_desktop:&[ViewItem],
        max_groups:usize, max_items_per_tab:usize,
    )->Result<PreparedNativeGesture,String>{
        if !self.hook_detached || !self.icon_positions_unchanged {
            return Err("Native gesture hook or Desktop position check not complete".into());
        }
        let age=now.checked_duration_since(self.observed_at)
            .ok_or("Native gesture clock moved backwards")?;
        if age>GESTURE_MAX_AGE{
            return Err("Native gesture expired before UI acceptance".into());
        }
        if self.explorer_pid==0 || self.explorer_pid!=explorer_pid ||
            self.listview_hwnd==0 || self.listview_hwnd!=listview_hwnd ||
            self.monitor_signature.is_empty() ||
            self.monitor_signature!=monitor_signature {
            return Err("Native Explorer PID/HWND or physical monitor topology changed".into());
        }
        if self.workspace_before.is_empty()
            || saved_workspace_bytes!=self.workspace_before {
            return Err("Workspace changed since native Explorer MouseDown".into());
        }
        if serde_json::to_vec(state).map_err(|e|e.to_string())?
            !=serde_json::to_vec(&State::from_saved_bytes(saved_workspace_bytes)?)
                .map_err(|e|e.to_string())? {
            return Err("UI State does not equal the exact saved native Workspace".into());
        }
        if current_desktop.len()!=self.observed_desktop.len() ||
            current_desktop.iter().zip(&self.observed_desktop)
                .any(|(current,(path,position))|
                    !current.path.eq_ignore_ascii_case(path) ||
                    current.pos!=*position) {
            return Err("Explorer native identity order or physical icon position changed".into());
        }
        let count=self.selected_indices.len();
        if count==0 || count>MAX_GESTURE_PINS ||
            self.observed_moves<3 ||
            !self.selected_indices.contains(&self.source_native_index) {
            return Err("Native gesture selection or travel was not verified".into());
        }
        let dx=i64::from(self.released_physical.0)-i64::from(self.start_physical.0);
        let dy=i64::from(self.released_physical.1)-i64::from(self.start_physical.1);
        if dx*dx+dy*dy<400 {
            return Err("Native gesture travel below minimum 20 physical pixels".into());
        }
        if self.selected_indices.iter().any(|&idx|idx>=current_desktop.len()) {
            return Err("Native gesture carries invalid Explorer ListView item index".into());
        }
        let unique=self.selected_indices.iter().collect::<std::collections::BTreeSet<_>>();
        if unique.len()!=count {
            return Err("Native gesture selected indices contain duplicates".into());
        }
        let verified_policy=explorer_native::native_multi_box_plan(
            state,current_desktop,max_groups,max_items_per_tab)?;
        let (next_state,next_plan,target_group,target_tab)=
            explorer_native::plan_native_selected_drag_release(
                state,current_desktop,&verified_policy,&self.selected_indices,
                self.released_physical.0,self.released_physical.1,
                max_groups,max_items_per_tab)?;
        Ok(PreparedNativeGesture{
            offered_paths:self.selected_indices.iter().map(|&index|
                PathBuf::from(&current_desktop[index].path)).collect(),
            next_state,next_plan,target_group,target_tab
        })
    }
}

/// The sender stays private to the worker that owns Explorer hook telemetry.
/// Public mutable report JSON is NOT a sender and never grants permission.
pub(crate) struct NativeGestureInbox{
    receiver:Receiver<NativeGestureEvidence>,
    consumed:bool,
}
pub(crate) fn native_gesture_channel()->(SyncSender<NativeGestureEvidence>,NativeGestureInbox){
    let (sender,receiver)=mpsc::sync_channel(1);
    (sender,NativeGestureInbox{receiver,consumed:false})
}
impl NativeGestureInbox {
    pub(crate) fn try_take(&mut self)->Result<Option<NativeGestureEvidence>,String>{
        if self.consumed {return Ok(None);}
        match self.receiver.try_recv(){
            Ok(item)=>{
                // Even invalid payloads consume the entire one-shot Inbox.
                self.consumed=true;
                Ok(Some(item))
            },
            Err(TryRecvError::Empty)=>Ok(None),
            Err(TryRecvError::Disconnected)=>{
                self.consumed=true;
                Err("Native Explorer hook ended without any trusted gesture".into())
            }
        }
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::model::{Bounds,Item};
    fn pin(path:&str,id:u64)->Item{
        Item{id,name:path.into(),path:path.into(),kind:"lnk".into(),
             size:0,modified:0,label:None}
    }
    fn fixture()->(State,Vec<ViewItem>,NativeGestureEvidence,u64){
        let mut state=State::empty();
        let src=state.add_group("Source",Bounds{x:100.,y:100.,w:320.,h:240.});
        state.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/A.lnk",1));
        state.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/B.lnk",2));
        state.add_tab(src).unwrap();
        state.group_mut(src).unwrap().active=0;
        let dst=state.add_group("Destination",Bounds{x:490.,y:100.,w:320.,h:240.});
        state.group_mut(dst).unwrap().tab_mut().items.push(pin("C:/Desktop/C.lnk",3));
        state.add_tab(dst).unwrap();
        state.group_mut(dst).unwrap().active=0;
        let icons=["A","B","C","Loose"].iter().enumerate().map(|(i,name)|ViewItem{
            path:format!("C:/Desktop/{name}.lnk"),
            label:name.to_string(),
            pos:(i as i32*100+35,40)
        }).collect::<Vec<_>>();
        let before=serde_json::to_vec(&state).unwrap();
        let event=NativeGestureEvidence{
            observed_at:Instant::now(),
            explorer_pid:49101,
            listview_hwnd:0x123456,
            monitor_signature:"DISPLAY1@2560x1440+0+0".into(),
            workspace_before:before,
            observed_desktop:icons.iter().map(|it|(it.path.clone(),it.pos)).collect(),
            selected_indices:vec![0,1],
            source_native_index:0,
            start_physical:(250,180),
            released_physical:(650,220),
            observed_moves:29,
            hook_detached:true,
            icon_positions_unchanged:true
        };
        (state,icons,event,dst)
    }
    fn check(ev:&NativeGestureEvidence,state:&State,items:&[ViewItem])
        ->Result<PreparedNativeGesture,String>{
        ev.validate(Instant::now(),49101,0x123456,"DISPLAY1@2560x1440+0+0",
                    &ev.workspace_before,state,items,4,8)
    }
    #[test]
    fn verified_event_accepts_exact_selected_pins_without_touching_source_state(){
        let (state,icons,event,dst)=fixture();
        let baseline=serde_json::to_vec(&state).unwrap();
        let action=check(&event,&state,&icons).unwrap();
        assert_eq!(action.offered_paths.len(),2);
        assert_eq!(action.target_group,dst);
        assert_eq!(action.next_plan.groups.len(),2);
        assert_eq!(action.next_state.group(dst).unwrap().tab().items.len(),3);
        assert_eq!(state.group(dst).unwrap().tab().items.len(),1);
        assert_eq!(serde_json::to_vec(&state).unwrap(),baseline);
        assert_eq!(action.target_tab,state.group(dst).unwrap().tab().id);
    }
    #[test]
    fn rejected_stale_or_forged_event_never_prepares_state(){
        let (state,icons,event,_)=fixture();
        let mut stale=event.clone();
        stale.observed_at=Instant::now()-Duration::from_secs(6);
        assert!(check(&stale,&state,&icons).is_err());
        let mut changed=event.clone();
        changed.monitor_signature="Another Monitor".into();
        assert!(check(&changed,&state,&icons).is_err());
        let mut replay=event.clone();
        replay.explorer_pid=123;
        assert!(check(&replay,&state,&icons).is_err());
        let mut partial=event.clone();
        partial.selected_indices=vec![0,0];
        assert!(check(&partial,&state,&icons).is_err());
        let mut changed=event.clone();
        changed.hook_detached=false;
        assert!(check(&changed,&state,&icons).is_err());
        let mut moved=icons.clone();
        moved[0].pos=(234,234);
        assert!(check(&event,&state,&moved).is_err());
        let mut altered=state.clone();
        altered.group_mut(1).unwrap().tab_mut().items.clear();
        assert!(check(&event,&altered,&icons).is_err());
        assert!(event.validate(Instant::now(),49101,0x123456,
            "DISPLAY1@2560x1440+0+0",b"{}", &state,&icons,4,8).is_err());
    }
    #[test]
    fn event_channel_is_bounded_single_consumption_and_discard_on_reject(){
        let (_state,_icons,e,_)=fixture();
        let (tx,mut inbox)=native_gesture_channel();
        assert!(inbox.try_take().unwrap().is_none());
        tx.try_send(e.clone()).unwrap();
        assert!(tx.try_send(e.clone()).is_err(),
            "Saturated channel must not buffer multiple gestures");
        assert!(inbox.try_take().unwrap().is_some());
        assert!(inbox.try_take().unwrap().is_none(),
            "Claimed event cannot be replayed");
        let (tx,mut inbox)=native_gesture_channel();
        tx.try_send(e).unwrap();
        drop(tx);
        assert!(inbox.try_take().unwrap().is_some());
        assert!(inbox.try_take().unwrap().is_none());
    }
}