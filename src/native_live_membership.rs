//! Native Explorer multi-Box / per-Tab identity controller.
//! Pure, bounded, non-mutating model: the Explorer ListView remains the
//! sole owner of actual desktop files, icon locations, context menus and
//! selection. This engine computes a full, future per-Box visibility policy
//! and refuses any stale identity or ambiguous Tab ownership. No Windows
//! hook is installed here.
use crate::{explorer_native::{self,NativeMultiBoxPlan},model::State,
            shell::ViewItem};
use std::collections::{BTreeMap,BTreeSet};

pub(crate) const MAX_SAFE_NATIVE_HIDE_ON_REAL_DESKTOP:usize=8;
/// Research-only cap, never used by safe_to_publish or normal UI.
pub(crate) const MAX_EXPERIMENTAL_STAGED_HIDE:usize=34;
/// Explicitly opted-in Windows v0.2 Native Desktop capacity, under the
/// Explorer ownership and position checkpoints with per-item rollback.
/// The older eight-icon safe publish policy is NOT relaxed.
pub(crate) const MAX_USER_NATIVE_DESKTOP_HIDE:usize=crate::native_filter_protocol::MAX_ITEMS;
pub(crate) fn active_native_hide_cap()->usize {
    if cfg!(feature="native-release") &&
        std::env::var("DESKTOPCRAFT_NATIVE_REAL_DESKTOP")
            .ok().as_deref()==Some("1"){
        MAX_USER_NATIVE_DESKTOP_HIDE
    }else{MAX_EXPERIMENTAL_STAGED_HIDE}
}

/// Bounded diagnostic-only visibility ramp. Each step is a nested subset of
/// the same fully validated inactive-Tab icons, never the active items.
/// Production policy publication continues to use safe_to_publish and is
/// deliberately NOT bypassed by this staging helper.
pub(crate) fn native_hide_stages(
    inactive:&[usize],requested_max:usize
)->Result<Vec<Vec<usize>>,String>{
    if inactive.is_empty() || requested_max==0 ||
        requested_max>MAX_EXPERIMENTAL_STAGED_HIDE ||
        requested_max>inactive.len(){
        return Err("Native Debug hide ramp requires 1..=34 verified inactive icons".into());
    }
    if inactive.windows(2).any(|pair|pair[0]>=pair[1]){
        return Err("Native hide ramp requires sorted, duplicate-free indexes".into());
    }
    let mut limits=vec![1usize,2,4,8,12,16,24,34].into_iter()
        .filter(|size|*size<=requested_max).collect::<Vec<_>>();
    if limits.last().copied()!=Some(requested_max){limits.push(requested_max);}
    Ok(limits.into_iter().map(|limit|inactive[..limit].to_vec()).collect())
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub(crate) struct NativeTwoBoxMembership {
    pub(crate) generation:u64,
    pub(crate) source_group:u64,
    pub(crate) target_group:u64,
    pub(crate) desktop_order:Vec<(String,(i32,i32))>,
    pub(crate) plan:NativeMultiBoxPlan,
    pub(crate) selected_tabs:Vec<(u64,usize)>,
    /// Inactive Pages, rolled-up Boxes and native regular siblings of a
    /// currently active Portal must be invisible even though none are moved.
    pub(crate) extra_hidden_indices:Vec<usize>,
    pub(crate) general_workspace:bool,
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub(crate) struct NativeTwoBoxView {
    pub(crate) generation:u64,
    pub(crate) source_index_count:usize,
    pub(crate) target_index_count:usize,
    pub(crate) hidden_indices:Vec<usize>,
    pub(crate) shown_source_indices:Vec<usize>,
    pub(crate) shown_target_indices:Vec<usize>,
    pub(crate) unclaimed_native_count:usize,
}
impl NativeTwoBoxMembership{
    pub(crate) fn prepare(
        generation:u64,state:&State,items:&[ViewItem],
        source_group:u64,target_group:u64,
    )->Result<Self,String>{
        if generation==0 || source_group==0 || target_group==0 ||
            source_group==target_group {
            return Err("Native two-Box generation or distinct Box IDs invalid".into());
        }
        let mut names=BTreeSet::new();
        for item in items {
            if !names.insert(item.path.to_lowercase())
                || item.label.trim().is_empty() {
                return Err("Explorer Desktop icon fullpath collision or blank name".into());
            }
        }
        let plan=explorer_native::native_multi_box_plan(state,items,16,128)?;
        if plan.groups.len()!=2 ||
            plan.groups.iter().map(|g|g.group_id).collect::<BTreeSet<_>>()
                != BTreeSet::from([source_group,target_group]) {
            return Err("Native controller requires EXACTLY two independently eligible regular Boxes".into());
        }
        let selected_tabs=plan.groups.iter()
            .map(|group|(group.group_id,group.active)).collect::<Vec<_>>();
        let controller=Self{
            generation,source_group,target_group,plan,selected_tabs,
            desktop_order:items.iter().map(|it|(it.path.to_lowercase(),it.pos)).collect(),
            extra_hidden_indices:Vec::new(),general_workspace:false
        };
        controller.snapshot(items)?;
        Ok(controller)
    }
    /// Generalized membership across all currently visible regular Desktop
    /// Boxes, no fixed Box IDs and no assumption of exactly two Tabs.
    /// Full native item ownership is unique throughout the entire Workspace.
    /// Linked Folder Portals are deliberately excluded from the desktop
    /// ListView, but their inactive regular sibling icons are suppressed.
    pub(crate) fn prepare_workspace(
        generation:u64,state:&State,items:&[ViewItem],
    )->Result<Self,String>{
        if generation==0{return Err("Invalid native workspace generation".into());}
        let plan=explorer_native::user_native_workspace_plan(state,items)?;
        let decision=explorer_native::NativePresentation::decide(state,items);
        if decision.conflicting_active_references!=0{
            return Err("Ambiguous active Explorer Desktop native ownership".into());
        }
        let unseen=decision.items.iter().filter(|item|item.total_references>0
            && item.visibility!=explorer_native::NativeVisibility::Active)
            .map(|item|item.path.to_lowercase())
            .collect::<std::collections::BTreeSet<_>>();
        let eligible_indices=plan.groups.iter().flat_map(|group|
            group.native_indices.iter().flat_map(|list|list.iter().copied()))
            .collect::<std::collections::BTreeSet<_>>();
        let mut extra_hidden_indices=items.iter().enumerate()
            .filter(|(index,item)|!eligible_indices.contains(index)
                && unseen.contains(&item.path.to_lowercase()))
            .map(|(index,_)|index).collect::<Vec<_>>();
        extra_hidden_indices.sort_unstable();
        let selected_tabs=plan.groups.iter().map(|group|
            (group.group_id,group.active)).collect::<Vec<_>>();
        let source_group=selected_tabs.first().map_or(0,|(id,_)|*id);
        let target_group=selected_tabs.get(1).map_or(0,|(id,_)|*id);
        let member=Self{
            generation,source_group,target_group,plan,selected_tabs,
            desktop_order:items.iter().map(|it|(it.path.to_lowercase(),it.pos))
                .collect(),
            extra_hidden_indices,general_workspace:true
        };
        member.snapshot(items)?;
        Ok(member)
    }
    fn current<'a>(&self,items:&'a [ViewItem])->Result<&'a [ViewItem],String>{
        if items.len()!=self.desktop_order.len() ||
            items.iter().zip(&self.desktop_order).any(|(item,(path,position))|
                !item.path.eq_ignore_ascii_case(path) || item.pos!=*position) {
            return Err("Explorer native identities or absolute positions changed; reject visibility generation".into());
        }
        Ok(items)
    }
    pub(crate) fn tab_indices(&self, group_id: u64, tab: usize) -> Result<&[usize], String> {
        self.plan.groups.iter().find(|group| group.group_id == group_id)
            .and_then(|group| group.native_indices.get(tab))
            .map(Vec::as_slice)
            .ok_or_else(|| "Native Box or Tab is missing from the verified index plan".into())
    }

    pub(crate) fn hidden_for_current_selection(&self)->Result<Vec<usize>,String>{
        let mut hidden=self.plan.hidden_for(&self.selected_tabs)?;
        if self.general_workspace {
            hidden.extend_from_slice(&self.extra_hidden_indices);
            hidden.sort_unstable();hidden.dedup();
        }
        Ok(hidden)
    }
    pub(crate) fn snapshot(&self,items:&[ViewItem])->Result<NativeTwoBoxView,String>{
        self.current(items)?;
        let overrides=&self.selected_tabs;
        let hidden=self.hidden_for_current_selection()?;
        let mut shown_source=Vec::new();
        let mut shown_target=Vec::new();
        for group in &self.plan.groups {
            let tab=overrides.iter().find(|(id,_)|*id==group.group_id)
                .map(|(_,tab)|*tab).ok_or("Missing selected native Tab")?;
            let list=group.native_indices.get(tab)
                .ok_or("Native Tab selection no longer exists")?;
            if group.group_id==self.source_group {shown_source=list.clone();}
            if group.group_id==self.target_group {shown_target=list.clone();}
        }
        if shown_source.iter().chain(&shown_target)
            .any(|idx|hidden.binary_search(idx).is_ok()) {
            return Err("Active real Explorer icon was marked hidden".into());
        }
        Ok(NativeTwoBoxView{
            generation:self.generation,
            source_index_count:shown_source.len(),
            target_index_count:shown_target.len(),
            shown_source_indices:shown_source,
            shown_target_indices:shown_target,
            hidden_indices:hidden,
            unclaimed_native_count:self.plan.unclaimed_native_items
        })
    }
    /// Pure policy is not permission to inject an Explorer paint/input Hook.
    /// An earlier Explorer reflow incident affected many icon
    /// positions; do not publish a large hide policy on the real Desktop
    /// until incrementally stress-tested with recovery checkpoints.
    pub(crate) fn safe_to_publish(&self,items:&[ViewItem])->Result<NativeTwoBoxView,String>{
        let view=self.snapshot(items)?;
        if view.hidden_indices.len()>MAX_SAFE_NATIVE_HIDE_ON_REAL_DESKTOP {
            return Err(format!(
                "Native Explorer real-Desktop visibility refused: would suppress {} icons, above tested safety limit {}",
                view.hidden_indices.len(),MAX_SAFE_NATIVE_HIDE_ON_REAL_DESKTOP));
        }
        Ok(view)
    }
    /// Single named Box Tab change creates a new generation. Caller decides
    /// whether and when to publish it to any real native Explorer filter.
    /// A rejected transition never changes the active controller.
    pub(crate) fn switch_tab(
        &self,state:&State,items:&[ViewItem],
        group_id:u64,target_tab:usize
    )->Result<Self,String>{
        self.current(items)?;
        if !self.plan.groups.iter().any(|group|group.group_id==group_id){
            return Err("Native Tab switch attempted on unsupported Box".into());
        }
        let group=state.group(group_id).ok_or("Selected native Box absent")?;
        if group.page!=state.page || group.rolled ||
            target_tab>=group.tabs.len() || group.tabs[target_tab].folder.is_some(){
            return Err("Native Tab switch requires visible regular Tab".into());
        }
        if self.generation==u64::MAX{
            return Err("Native policy generation exhausted".into());
        }
        let mut next=self.clone();
        next.generation+=1;
        if let Some(tab)=next.selected_tabs.iter_mut().find(|(id,_)|*id==group_id){
            tab.1=target_tab;
        }
        next.snapshot(items)?;
        Ok(next)
    }
    pub(crate) fn rebase_after_pins(
        &self,new_state:&State,items:&[ViewItem],
    )->Result<Self,String>{
        self.current(items)?;
        if self.generation==u64::MAX {return Err("Native membership generation exhausted".into());}
        if self.general_workspace{
            Self::prepare_workspace(self.generation+1,new_state,items)
        }else{
            Self::prepare(self.generation+1,new_state,items,
                self.source_group,self.target_group)
        }
    }
}
/// Developer-only real-Explorer A/B policy cycle. Both Boxes retain independent
/// tab choices; do not publish any step until EVERY candidate fits the
/// experimental cap. The production eight-item limit is unchanged.
pub(crate) fn native_two_box_tab_cycle(
    state:&State,items:&[ViewItem],controller:&NativeTwoBoxMembership,
)->Result<Vec<(Vec<(u64,usize)>,NativeTwoBoxView)>,String>{
    let source=controller.plan.groups.iter()
        .find(|group|group.group_id==controller.source_group)
        .ok_or("Cycle source Box is missing")?;
    let target=controller.plan.groups.iter()
        .find(|group|group.group_id==controller.target_group)
        .ok_or("Cycle destination Box is missing")?;
    if source.native_indices.len()<2 || target.native_indices.len()<2 {
        return Err("Native Tab cycle needs at least two regular Tabs in both Boxes".into());
    }
    let from_source=source.active;
    let from_target=target.active;
    let most_visible_source=(0..source.native_indices.len())
        .max_by_key(|&tab|source.native_indices[tab].len())
        .ok_or("Cycle has no eligible source Tab")?;
    let alternate_target=(0..target.native_indices.len())
        .find(|&tab|tab!=from_target)
        .ok_or("Cycle has no independent destination Tab")?;
    let mut choices=vec![(from_source,from_target)];
    for tab in 0..source.native_indices.len() {
        if tab!=from_source {choices.push((tab,from_target));}
    }
    // Change destination ONLY while source is maximally visible, minimizing
    // inactive-icon suppression in the other Box during a real Desktop test.
    choices.extend([
        (most_visible_source,from_target),
        (most_visible_source,alternate_target),
        (most_visible_source,from_target),
        (from_source,from_target),
    ]);
    choices.dedup();
    let mut stages=Vec::new();
    for (source_tab,target_tab) in choices {
        let after_source=controller.switch_tab(state,items,
            controller.source_group,source_tab)?;
        let after_target=after_source.switch_tab(state,items,
            controller.target_group,target_tab)?;
        let view=after_target.snapshot(items)?;
        if view.hidden_indices.len()>MAX_EXPERIMENTAL_STAGED_HIDE {
            return Err(format!(
                "Native two-Box Tab cycle would hide {} icons, beyond isolated 34-icon cap",
                view.hidden_indices.len()));
        }
        stages.push((vec![(controller.source_group,source_tab),
                          (controller.target_group,target_tab)],view));
    }
    if stages.len()<5 || stages.first().map(|x|&x.0)!=stages.last().map(|x|&x.0){
        return Err("Native cycle cannot safely return to initial tab selections".into());
    }
    Ok(stages)
}

#[cfg(test)]
mod generalized_workspace_tests{
    use super::*;
    use crate::model::{Bounds,Item};
    fn pin(id:u64,name:&str)->Item{
        Item{id,name:name.into(),path:format!("C:/Desktop/{name}.lnk"),
            kind:"lnk".into(),size:0,modified:0,label:None}
    }
    fn make()->(State,Vec<ViewItem>,Vec<u64>){
        let mut s=State::empty();
        let mut ids=Vec::new();
        for index in 0..3 {
            let id=s.add_group(&format!("Box{index}"),Bounds{
                x:120.+index as f32*420.,y:200.,w:390.,h:300.});
            s.group_mut(id).unwrap().tab_mut().items.push(pin(index+100,&format!("A{index}")));
            if index!=2 {
                s.add_tab(id).unwrap();
                s.group_mut(id).unwrap().tabs[1].items.push(
                    pin(index+200,&format!("B{index}")));
                // State::add_tab intentionally activates the new Tab.
                // This synthetic visibility fixture explicitly starts on A.
                s.group_mut(id).unwrap().active=0;
            }
            ids.push(id);
        }
        let item_names=["A0","A1","A2","B0","B1","Loose"];
        let icons=item_names.iter().enumerate().map(|(i,name)|ViewItem{
            label:name.to_string(),path:format!("C:/Desktop/{name}.lnk"),
            pos:(i as i32*95+50,700)
        }).collect();
        (s,icons,ids)
    }
    #[test]
    fn generalized_member_tracks_three_boxes_and_single_tab(){
        let (s,icons,ids)=make();
        let model=NativeTwoBoxMembership::prepare_workspace(1,&s,&icons).unwrap();
        assert_eq!(model.plan.groups.len(),3);
        assert_eq!(model.hidden_for_current_selection().unwrap(),vec![3,4]);
        let next=model.switch_tab(&s,&icons,ids[1],1).unwrap();
        assert_eq!(next.hidden_for_current_selection().unwrap(),vec![1,3]);
        assert!(next.switch_tab(&s,&icons,ids[2],1).is_err());
        assert!(next.switch_tab(&s,&icons,u64::MAX,0).is_err());
    }
    #[test]
    fn general_workspace_hides_other_page_and_rolled_icons(){
        let (mut s,icons,ids)=make();
        s.group_mut(ids[1]).unwrap().rolled=true;
        s.group_mut(ids[2]).unwrap().page=1;
        let model=NativeTwoBoxMembership::prepare_workspace(1,&s,&icons).unwrap();
        assert_eq!(model.plan.groups.len(),1);
        assert_eq!(model.hidden_for_current_selection().unwrap(),vec![1,2,3,4]);
        assert_eq!(model.source_group,ids[0]);
        assert_eq!(model.target_group,0);
    }
    #[test]
    fn active_portal_tab_is_not_a_desktop_icon_view(){
        let (mut s,icons,ids)=make();
        let source=s.group_mut(ids[1]).unwrap();
        source.tabs[1].folder=Some("D:/ForeignNamespace".into());
        source.active=1;
        let model=NativeTwoBoxMembership::prepare_workspace(1,&s,&icons).unwrap();
        assert_eq!(model.plan.groups.len(),2);
        assert!(model.plan.skipped.iter().any(|(id,_)|*id==ids[1]));
        assert!(model.hidden_for_current_selection().unwrap().contains(&1));
        let dup=s.group_mut(ids[2]).unwrap();
        dup.tabs[0].items.push(pin(999,"A0"));
        assert!(NativeTwoBoxMembership::prepare_workspace(1,&s,&icons).is_err());
    }

    #[test]
    fn general_workspace_allows_rolling_up_every_box_and_hides_all_their_icons(){
        let (mut s,icons,ids)=make();
        for id in ids {
            s.group_mut(id).unwrap().rolled=true;
        }
        let model=NativeTwoBoxMembership::prepare_workspace(1,&s,&icons).unwrap();
        assert!(model.plan.groups.is_empty());
        assert!(model.selected_tabs.is_empty());
        assert_eq!(model.source_group,0);
        assert_eq!(model.target_group,0);
        assert_eq!(model.hidden_for_current_selection().unwrap(),vec![0,1,2,3,4]);
        let view=model.snapshot(&icons).unwrap();
        assert_eq!(view.hidden_indices,vec![0,1,2,3,4]);
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
    fn fixture()->(State,Vec<ViewItem>,u64,u64){
        let mut state=State::empty();
        let source=state.add_group("Source",Bounds{x:100.,y:100.,w:300.,h:230.});
        state.group_mut(source).unwrap().tab_mut().items
            .push(pin("C:/Desktop/A.lnk",100));
        state.group_mut(source).unwrap().tab_mut().items
            .push(pin("C:/Desktop/B.lnk",101));
        state.add_tab(source).unwrap();
        state.group_mut(source).unwrap().tabs[1].items
            .push(pin("C:/Desktop/C.lnk",102));
        state.group_mut(source).unwrap().active=0;
        let target=state.add_group("Target",Bounds{x:500.,y:100.,w:300.,h:230.});
        state.add_tab(target).unwrap();
        state.group_mut(target).unwrap().active=0;
        let icons=["A","B","C","Loose"].iter().enumerate().map(|(i,s)|ViewItem{
            path:format!("C:/Desktop/{s}.lnk"),label:s.to_string(),
            pos:(20+i as i32*100,50)
        }).collect::<Vec<_>>();
        (state,icons,source,target)
    }
    #[test]
    fn two_box_cycle_is_independent_bounded_and_restores_initial_tabs(){
        let (mut state,icons,source,target)=fixture();
        state.add_tab(source).unwrap();
        state.group_mut(source).unwrap().active=1;
        let controller=NativeTwoBoxMembership::prepare(1,&state,&icons,source,target).unwrap();
        let original=serde_json::to_vec(&state).unwrap();
        let steps=native_two_box_tab_cycle(&state,&icons,&controller).unwrap();
        assert!(steps.len()>=6);
        assert_eq!(steps.first().unwrap().0,steps.last().unwrap().0);
        assert!(steps.iter().any(|(tabs,_)|tabs[0].1==0));
        assert!(steps.iter().any(|(tabs,_)|tabs[0].1==2));
        assert!(steps.iter().any(|(tabs,_)|tabs[1].1==1));
        assert!(steps.iter().all(|(_,view)|
            view.hidden_indices.len()<=MAX_EXPERIMENTAL_STAGED_HIDE));
        assert_eq!(serde_json::to_vec(&state).unwrap(),original);
        assert_eq!(controller.selected_tabs,vec![(source,1),(target,0)]);
        let mut drift=icons.clone();
        drift[0].pos=(1234,4444);
        assert!(native_two_box_tab_cycle(&state,&drift,&controller).is_err());
    }
    #[test]
    fn real_tab_cycle_refuses_more_than_thirty_four_hidden_icons(){
        let (mut state,mut icons,source,target)=fixture();
        for i in 0..35 {
            let path=format!("C:/Desktop/CycleExtra{i}.lnk");
            state.group_mut(source).unwrap().tabs[1].items
                .push(pin(&path,900+i));
            icons.push(ViewItem{
                path,label:format!("CycleExtra{i}"),pos:(900+i as i32*8,450)
            });
        }
        let planner=NativeTwoBoxMembership::prepare(1,&state,&icons,source,target).unwrap();
        let error=native_two_box_tab_cycle(&state,&icons,&planner).unwrap_err();
        assert!(error.contains("beyond isolated 34-icon cap"),"{error}");
    }
    #[test]
    fn staged_hide_is_nested_bounded_and_preserves_original_identity_order(){
        let hidden=vec![1,4,8,15,19,23,40,52,57,60];
        let original=hidden.clone();
        let stages=native_hide_stages(&hidden,8).unwrap();
        assert_eq!(stages.iter().map(Vec::len).collect::<Vec<_>>(),vec![1,2,4,8]);
        assert_eq!(stages[0],vec![1]);
        assert_eq!(stages[3],vec![1,4,8,15,19,23,40,52]);
        assert_eq!(hidden,original);
        assert_eq!(native_hide_stages(&hidden,3).unwrap().iter()
            .map(Vec::len).collect::<Vec<_>>(),vec![1,2,3]);
        assert_eq!(native_hide_stages(&hidden,9).unwrap().last().unwrap().len(),9);
        assert!(native_hide_stages(&hidden,35).is_err());
        assert!(native_hide_stages(&hidden,0).is_err());
        assert!(native_hide_stages(&[1,1,2],2).is_err());
        assert!(native_hide_stages(&[2,1],2).is_err());
    }
    #[test]
    fn independent_tab_switch_and_generation_does_not_touch_source_layout(){
        let (state,icons,source,target)=fixture();
        let bytes=serde_json::to_vec(&state).unwrap();
        let start=NativeTwoBoxMembership::prepare(1,&state,&icons,source,target).unwrap();
        let v=start.safe_to_publish(&icons).unwrap();
        assert_eq!((v.source_index_count,v.target_index_count),(2,0));
        assert_eq!(v.hidden_indices,vec![2]);
        let next=start.switch_tab(&state,&icons,source,1).unwrap();
        let switched=next.snapshot(&icons).unwrap();
        assert_eq!((switched.source_index_count,switched.target_index_count),(1,0));
        assert_eq!(switched.hidden_indices,vec![0,1]);
        assert_eq!(start.snapshot(&icons).unwrap(),v);
        assert_eq!(serde_json::to_vec(&state).unwrap(),bytes);
        assert!(next.switch_tab(&state,&icons,999,0).is_err());
        assert!(next.switch_tab(&state,&icons,target,99).is_err());
    }
    #[test]
    fn rebase_after_atomic_multi_pin_updates_both_groups_without_icon_move(){
        let (state,icons,source,target)=fixture();
        let base=NativeTwoBoxMembership::prepare(2,&state,&icons,source,target).unwrap();
        let mut moved=state.clone();
        let pins=moved.group_mut(source).unwrap().tab_mut().items.drain(..2)
            .collect::<Vec<_>>();
        moved.group_mut(target).unwrap().tab_mut().items.extend(pins);
        let revised=base.rebase_after_pins(&moved,&icons).unwrap();
        let view=revised.snapshot(&icons).unwrap();
        assert_eq!((view.source_index_count,view.target_index_count),(0,2));
        assert_eq!(view.generation,3);
        assert_eq!(view.hidden_indices,vec![2]);
        assert_eq!(base.snapshot(&icons).unwrap().source_index_count,2);
        let mut drift=icons.clone();
        drift[0].pos=(999,999);
        assert!(base.rebase_after_pins(&moved,&drift).is_err());
        assert!(revised.snapshot(&drift).is_err());
    }
    #[test]
    fn more_than_eight_native_icons_cannot_be_hidden_on_live_desktop(){
        let (mut state,mut items,source,target)=fixture();
        for i in 0..10 {
            let name=format!("C:/Desktop/Extra{i}.lnk");
            state.group_mut(source).unwrap().tabs[1].items
                .push(pin(&name,300+i));
            items.push(ViewItem{
                path:name,label:format!("Extra{i}"),pos:(900+i as i32*100,50)
            });
        }
        let planner=NativeTwoBoxMembership::prepare(1,&state,&items,source,target).unwrap();
        let view=planner.snapshot(&items).unwrap();
        assert!(view.hidden_indices.len()>MAX_SAFE_NATIVE_HIDE_ON_REAL_DESKTOP);
        assert!(planner.safe_to_publish(&items).is_err());
        assert_eq!(items[0].pos,(20,50));
    }
    #[test]
    fn rejects_portals_ambiguous_icons_and_wrong_page_before_any_policy(){
        let (state,icons,source,target)=fixture();
        let mut portal=state.clone();
        portal.group_mut(target).unwrap().tabs[0].folder=Some("D:/Portal".into());
        assert!(NativeTwoBoxMembership::prepare(1,&portal,&icons,source,target).is_err());
        let mut dup=icons.clone();dup.push(icons[0].clone());
        assert!(NativeTwoBoxMembership::prepare(1,&state,&dup,source,target).is_err());
        assert!(NativeTwoBoxMembership::prepare(0,&state,&icons,source,target).is_err());
        assert!(NativeTwoBoxMembership::prepare(1,&state,&icons,source,source).is_err());
    }
}
