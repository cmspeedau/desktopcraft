//! Native desktop item visibility planning.
//!
//! One Explorer SysListView32 owns the real Desktop items. DesktopCraft's
//! groups/tabs/pages must decide which of those stable native identities can
//! be painted and interacted with. This module computes the decision only;
//! a native integration backend must enforce BOTH drawing and hit testing.
//! Never claim that this policy alone hides Explorer icons.
use crate::{model::{Group, State}, shell::ViewItem};
use serde_json::{json, Value};
use std::{cell::OnceCell, collections::BTreeMap, path::{Path, PathBuf}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeVisibility {
    /// Explorer owns it, and it is not assigned to a Box.
    Loose,
    /// Active Page, active Tab, Box not collapsed.
    Active,
    /// Native desktop item is pinned only in an inactive Tab.
    InactiveTab,
    /// It belongs to another DesktopCraft Page.
    InactivePage,
    /// All currently applicable Box presentations are rolled up.
    RolledUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeItem {
    pub path: String,
    pub visibility: NativeVisibility,
    pub active_references: usize,
    pub total_references: usize,
}

#[derive(Debug, Default, Clone)]
struct Claim {
    active: usize,
    tab: usize,
    page: usize,
    rolled: usize,
}

#[derive(Debug, Clone)]
pub struct NativePresentation {
    /// Exact existing Explorer item identities; never fabricated proxies.
    pub items: Vec<NativeItem>,
    pub external_pin_references: usize,
    pub conflicting_active_references: usize,
}

impl NativePresentation {
    pub fn decide(state: &State, explorer: &[ViewItem]) -> Self {
        let native: BTreeMap<String, String> = explorer.iter()
            .map(|item| (item.path.to_lowercase(), item.path.clone())).collect();
        let mut claims = BTreeMap::<String, Claim>::new();
        let mut external_pin_references = 0usize;
        for group in &state.groups {
            for (tab_index, tab) in group.tabs.iter().enumerate() {
                // A folder portal is a DIFFERENT shell namespace/view. Never
                // project its files into the existing desktop ListView.
                if tab.folder.is_some() { continue; }
                for item in &tab.items {
                    let identity = item.path.to_lowercase();
                    if !native.contains_key(&identity) {
                        external_pin_references += 1;
                        continue;
                    }
                    let claim = claims.entry(identity).or_default();
                    if group.page != state.page {
                        claim.page += 1;
                    } else if group.rolled {
                        claim.rolled += 1;
                    } else if tab_index != group.active {
                        claim.tab += 1;
                    } else {
                        claim.active += 1;
                    }
                }
            }
        }

        let conflicting_active_references = claims.values()
            .map(|claim| claim.active.saturating_sub(1)).sum();

        let items = native.into_iter().map(|(key, path)| {
            let claim = claims.get(&key).cloned().unwrap_or_default();
            let visibility = if claim.active > 0 {
                NativeVisibility::Active
            } else if claim.tab > 0 {
                NativeVisibility::InactiveTab
            } else if claim.page > 0 {
                NativeVisibility::InactivePage
            } else if claim.rolled > 0 {
                NativeVisibility::RolledUp
            } else {
                NativeVisibility::Loose
            };
            NativeItem {
                path,
                visibility,
                active_references: claim.active,
                total_references: claim.active + claim.tab + claim.page + claim.rolled,
            }
        }).collect();

        Self { items, external_pin_references, conflicting_active_references }
    }

    pub fn summary(&self) -> Value {
        let count = |which| self.items.iter().filter(|item| item.visibility == which).count();
        json!({
            "native_explorer_items": self.items.len(),
            "active_tab_items": count(NativeVisibility::Active),
            "inactive_tab_items_needing_native_suppression": count(NativeVisibility::InactiveTab),
            "other_page_items_needing_native_suppression": count(NativeVisibility::InactivePage),
            "rolled_up_items_needing_native_suppression": count(NativeVisibility::RolledUp),
            "loose_explorer_items": count(NativeVisibility::Loose),
            "pins_not_in_desktop_namespace": self.external_pin_references,
            "conflicting_simultaneously_visible_references": self.conflicting_active_references,
            "native_visibility_filter_installed": false,
            "warning": "Planning only: native hooks/painting/input filtering are not installed. DesktopCraft must not pretend inactive icons are hidden."
        })
    }

    /// Identity-based delta; tab switches must never rely on unstable Explorer
    /// ListView indices. The return includes only items with changed treatment.
    pub fn changes_since(&self, previous: &NativePresentation) -> Vec<(String, NativeVisibility, NativeVisibility)> {
        let former: BTreeMap<String, NativeVisibility> = previous.items.iter()
            .map(|item| (item.path.to_lowercase(), item.visibility)).collect();
        self.items.iter().filter_map(|item| {
            let old = former.get(&item.path.to_lowercase()).copied()?;
            (old != item.visibility).then(|| (item.path.clone(), old, item.visibility))
        }).collect()
    }
}

/// Pick a bounded two-item native tab-switch trial from one real
/// DesktopCraft Box. No source item is created, moved, or unpinned: the
/// two real Explorer paths must already appear in *different* normal Tabs.
///
/// A probe with one icon from each Tab lets us test A -> B -> A without
/// suddenly hiding all inactive items in the user's desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTabPair {
    pub group_id: u64,
    pub from_tab: usize,
    pub to_tab: usize,
    /// A regular Tab may legitimately contain ZERO native items after a
    /// persisted virtual-Pin move. No fake Explorer index is ever invented.
    pub from_index: Option<usize>,
    pub to_index: Option<usize>,
    pub from_label: String,
    pub to_label: String,
}

pub fn choose_native_tab_pair(state: &State, explorer: &[ViewItem],
                              group_id: u64, requested_to_tab: Option<usize>)
                              -> Result<NativeTabPair, String> {
    let group = state.groups.iter().find(|g| g.id == group_id)
        .ok_or("DesktopCraft Box ID does not exist")?;
    if group.page != state.page || group.rolled {
        return Err("Selected Box is on another Page or rolled up".into());
    }
    if group.tabs.len() < 2 || group.active >= group.tabs.len() {
        return Err("Selected Box has no usable alternate native Tab".into());
    }
    if group.tabs[group.active].folder.is_some() {
        return Err("Native desktop trial cannot use a linked Folder Portal Tab".into());
    }
    let actual: BTreeMap<String, Vec<usize>> = {
        let mut data: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, item) in explorer.iter().enumerate() {
            data.entry(item.path.to_lowercase()).or_default().push(index);
        }
        data
    };
    let candidates: Vec<usize> = if let Some(to) = requested_to_tab {
        if to == group.active || to >= group.tabs.len() {
            return Err("Alternate Tab must exist and differ from Active Tab".into());
        }
        vec![to]
    } else {
        (0..group.tabs.len()).filter(|index| *index != group.active).collect()
    };
    let from_tab = &group.tabs[group.active];
    for to in candidates {
        let to_tab = &group.tabs[to];
        if to_tab.folder.is_some() { continue; }
        for a in &from_tab.items {
            let from = a.path.to_lowercase();
            let Some(from_indices) = actual.get(&from).filter(|v|v.len()==1) else {continue};
            for b in &to_tab.items {
                let target = b.path.to_lowercase();
                if from == target { continue; }
                let Some(to_indices) = actual.get(&target).filter(|v|v.len()==1) else {continue};
                let (fi,ti)=(from_indices[0],to_indices[0]);
                if explorer[fi].label.trim().is_empty() ||
                    explorer[ti].label.trim().is_empty() ||
                    explorer[fi].label.eq_ignore_ascii_case(&explorer[ti].label) {
                    continue;
                }
                return Ok(NativeTabPair {
                    group_id,from_tab:group.active,to_tab:to,
                    from_index:Some(fi),to_index:Some(ti),
                    from_label:explorer[fi].label.clone(),
                    to_label:explorer[ti].label.clone(),
                });
            }
        }
    }
    Err("No two distinct native Explorer icons exist in the chosen Tabs".into())
}

/// Native multi-item visibility trial. All icons in both selected Tabs
/// must exist in the single real Explorer Desktop namespace. No partial
/// success is allowed: a Folder Portal/virtual pin needs another backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTabSet {
    pub pair: NativeTabPair,
    pub from_indices: Vec<usize>,
    pub to_indices: Vec<usize>,
    pub from_labels: Vec<String>,
    pub to_labels: Vec<String>,
}

impl NativeTabSet {
    pub fn from_pair(pair: NativeTabPair) -> Self {
        Self {
            from_indices: pair.from_index.into_iter().collect(),
            to_indices: pair.to_index.into_iter().collect(),
            from_labels: if pair.from_index.is_some() {
                vec![pair.from_label.clone()]
            }else{Vec::new()},
            to_labels: if pair.to_index.is_some() {
                vec![pair.to_label.clone()]
            }else{Vec::new()},
            pair,
        }
    }
    pub fn hidden_indices(&self, stage: u32) -> &[usize] {
        if stage == 1 { &self.from_indices } else { &self.to_indices }
    }
    pub fn hidden_labels(&self, stage: u32) -> &[String] {
        if stage == 1 { &self.from_labels } else { &self.to_labels }
    }
}

pub fn choose_native_tab_set(
    state:&State,
    explorer:&[ViewItem],
    group_id:u64,
    to_tab:Option<usize>,
    max_each:usize,
)->Result<NativeTabSet,String>{
    if max_each==0 || max_each>8 {
        return Err("Native Tab sample limit is outside safe bounds".into());
    }
    // Keep the stricter legacy pair selection when both Tabs have real
    // icons. A completely EMPTY regular Tab is valid after the last Pin is
    // moved, however, and must not invent an Explorer source index.
    let pair=match choose_native_tab_pair(state,explorer,group_id,to_tab) {
        Ok(pair)=>pair,
        Err(reason)=>{
            let group=state.group(group_id).ok_or(reason.clone())?;
            let Some(target_index)=to_tab else{return Err(reason)};
            if group.page!=state.page || group.rolled ||
                group.active>=group.tabs.len() ||
                target_index>=group.tabs.len() ||
                target_index==group.active {
                return Err(reason);
            }
            let from=&group.tabs[group.active];
            let target=&group.tabs[target_index];
            if from.folder.is_some() || target.folder.is_some() ||
                from.items.is_empty()==target.items.is_empty() {
                // Both empty: no policy to install. Both nonempty: the
                // stricter identity validation already rejected the pair.
                return Err(reason);
            }
            NativeTabPair{
                group_id,from_tab:group.active,to_tab:target_index,
                from_index:None,to_index:None,
                from_label:String::new(),to_label:String::new(),
            }
        }
    };
    let group=state.group(group_id)
        .ok_or("Box disappeared while selecting native Tab sample")?;
    let mut map=BTreeMap::<String,Vec<usize>>::new();
    for (index,item) in explorer.iter().enumerate() {
        map.entry(item.path.to_lowercase()).or_default().push(index);
    }
    let mut used=std::collections::HashSet::new();
    let mut collect=|tab_index:usize| -> Result<(Vec<usize>,Vec<String>),String> {
        let tab=&group.tabs[tab_index];
        if tab.folder.is_some() || tab.items.len()>max_each {
            return Err("Native Tab contains a Portal or exceeds bounded sample limit".into());
        }
        let mut indices=Vec::with_capacity(tab.items.len());
        let mut names=Vec::with_capacity(tab.items.len());
        for item in &tab.items {
            let Some(matches)=map.get(&item.path.to_lowercase()).filter(|v|v.len()==1)
                else{return Err("Native Tab has non-Desktop or ambiguous Explorer identity".into())};
            let index=matches[0];
            if explorer[index].label.trim().is_empty() || !used.insert(index) {
                return Err("Native Tab has missing or duplicate Explorer item identity".into());
            }
            names.push(explorer[index].label.clone());
            indices.push(index);
        }
        Ok((indices,names))
    };
    let (from_indices,from_labels)=collect(pair.from_tab)?;
    let (to_indices,to_labels)=collect(pair.to_tab)?;
    if from_indices.is_empty() && to_indices.is_empty() {
        return Err("No real Explorer native icons in the selected Tab pair".into());
    }
    // For fully populated Tabs preserve representative strict-pair indices;
    // for an empty side use None. No LVM_* call receives a fake index.
    let pair=NativeTabPair{
        from_index:if from_indices.is_empty(){None}else{pair.from_index.or_else(||from_indices.first().copied())},
        to_index:if to_indices.is_empty(){None}else{pair.to_index.or_else(||to_indices.first().copied())},
        from_label:if from_indices.is_empty(){String::new()}
            else if pair.from_label.is_empty(){from_labels[0].clone()}else{pair.from_label},
        to_label:if to_indices.is_empty(){String::new()}
            else if pair.to_label.is_empty(){to_labels[0].clone()}else{pair.to_label},
        ..pair
    };
    Ok(NativeTabSet{pair,from_indices,to_indices,from_labels,to_labels})
}

/// General native DesktopCraft Tab model, independent of the temporary
/// two-button lab. On real UI clicks, derive the target from this one source
/// of truth rather than teaching each button to invent its own icon indexes.
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct NativeGroupTabPlan {
    pub group_id:u64,
    pub active:usize,
    pub names:Vec<String>,
    pub native_indices:Vec<Vec<usize>>,
    /// Portal tabs are represented by NO native Desktop icons. They belong
    /// to a different Shell View and may never be selected by this policy.
    pub portal_tabs:Vec<usize>,
}
impl NativeGroupTabPlan {
    /// Only the selected tab's real items remain visible. All other desktop
    /// icons outside this group are never mentioned by this scoped plan.
    pub fn hidden_for(&self,selected:usize)->Result<Vec<usize>,String>{
        if selected>=self.native_indices.len(){return Err("No such native tab".into());}
        if self.portal_tabs.contains(&selected) {
            return Err("Linked Folder Portal requires its own Explorer Shell View".into());
        }
        Ok(self.native_indices.iter().enumerate()
            .filter(|(index,_)|*index!=selected)
            .flat_map(|(_,items)|items.iter().copied())
            .collect())
    }
    /// Exact group state transition taken from the real DesktopCraft model.
    pub fn event(&self,new_selected:usize)->Result<Option<NativeTabChange>,String>{
        if new_selected>=self.native_indices.len(){return Err("Native tab index outside group".into());}
        if self.portal_tabs.contains(&new_selected){
            return Err("Linked Folder Portal must use its own Shell View".into());
        }
        Ok((new_selected!=self.active).then_some(NativeTabChange{
            group_id:self.group_id,from:self.active,to:new_selected,
        }))
    }
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub struct NativeTabChange {pub group_id:u64,pub from:usize,pub to:usize}

pub fn native_tab_changes(before:&State,after:&State)->Vec<NativeTabChange>{
    after.groups.iter().filter_map(|new|{
        let previous=before.groups.iter().find(|g|g.id==new.id)?;
        (previous.active!=new.active
            && previous.page==new.page
            && !new.rolled
            && new.page==after.page).then_some(NativeTabChange{
                group_id:new.id,from:previous.active,to:new.active,
            })
    }).collect()
}

/// Each normalized path records a single native index or an ambiguity.
type NativePathIndex = BTreeMap<String, Option<usize>>;

fn native_path_index(explorer: &[ViewItem]) -> NativePathIndex {
    let mut paths = BTreeMap::new();
    for (index, item) in explorer.iter().enumerate() {
        paths.entry(item.path.to_lowercase())
            .and_modify(|match_index| *match_index = None)
            .or_insert(Some(index));
    }
    paths
}

fn confirmed_missing_pin(path: &str, desktop_folders: &OnceCell<Vec<PathBuf>>) -> bool {
    let path = Path::new(path);
    if !path.is_absolute() || !matches!(path.try_exists(), Ok(false)) {
        return false;
    }
    if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("lnk")) {
        return true;
    }
    // Native Pins can also be ordinary files, apps, or folders. Only
    // direct children of the actual user/shared Desktop are eligible here.
    let Some(parent) = path.parent() else { return false; };
    desktop_folders.get_or_init(crate::shell::desktop_folders).iter().any(|desktop| {
        let parent = parent.components();
        let desktop = desktop.components();
        parent.clone().count() == desktop.clone().count()
            && parent.zip(desktop).all(|(left, right)| {
                left.as_os_str().eq_ignore_ascii_case(right.as_os_str())
            })
    })
}

pub fn native_group_tab_plan(state:&State, explorer:&[ViewItem],
                             group_id:u64, max_items_per_tab:usize)
                             ->Result<NativeGroupTabPlan,String>{
    if max_items_per_tab==0 || max_items_per_tab>128 {
        return Err("Native Tab plan limit is outside supported bounds".into());
    }
    let group=state.group(group_id).ok_or("Native group not found")?;
    group_tab_plan(group,state.page,explorer,&native_path_index(explorer),
        max_items_per_tab,None)
}

fn group_tab_plan(
    group: &Group, page: usize, explorer: &[ViewItem], by_path: &NativePathIndex,
    max_items_per_tab: usize, desktop_folders: Option<&OnceCell<Vec<PathBuf>>>,
) -> Result<NativeGroupTabPlan,String> {
    let group_id=group.id;
    if group.rolled || group.page!=page || group.tabs.is_empty()
        || group.active>=group.tabs.len(){
        return Err("Native group not active on current page".into());
    }
    if group.tabs[group.active].folder.is_some(){
        return Err("Active Folder Portal requires its own Explorer Shell View".into());
    }
    let mut assigned=std::collections::HashSet::<usize>::new();
    let mut names=Vec::with_capacity(group.tabs.len());
    let mut native_indices=Vec::with_capacity(group.tabs.len());
    let mut portal_tabs=Vec::new();
    for (tab_index,tab) in group.tabs.iter().enumerate() {
        if tab.folder.is_some(){
            // A separate Folder Portal must neither disqualify a regular
            // active native Tab nor accidentally claim/hide any of its
            // Explorer source items. No native destination can be a Portal.
            portal_tabs.push(tab_index);
            names.push(tab.name.clone());
            native_indices.push(Vec::new());
            continue;
        }
        if tab.items.len()>max_items_per_tab{
            return Err("Native tab item count exceeds verified limit".into());
        }
        let mut mapped=Vec::with_capacity(tab.items.len());
        for item in &tab.items {
            let index=match by_path.get(&item.path.to_lowercase()) {
                Some(Some(index))
                    if desktop_folders.is_none() || explorer[*index].path.eq_ignore_ascii_case(&item.path)=>*index,
                None if desktop_folders.is_some_and(|folders| confirmed_missing_pin(&item.path, folders))=>continue,
                _=>return Err("Tab has non-desktop or duplicate native identity".into()),
            };
            if explorer[index].label.trim().is_empty() || !assigned.insert(index) {
                return Err("Native item shared by tabs or missing label".into());
            }
            mapped.push(index);
        }
        names.push(tab.name.clone());
        native_indices.push(mapped);
    }
    Ok(NativeGroupTabPlan{group_id,active:group.active,
        names,native_indices,portal_tabs})
}

/// Composable, non-mutating native visibility policy for multiple
/// independent DesktopCraft Boxes. Only exact real Desktop item identities
/// from fully validated regular Tabs may be hidden. Ineligible Boxes (linked
/// Folder Portals, rolled-up/other Page, single Tab, missing native items)
/// are *never* partially suppressed by this plan.
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct NativeMultiBoxPlan {
    pub groups:Vec<NativeGroupTabPlan>,
    pub skipped:Vec<(u64,String)>,
    pub unclaimed_native_items:usize,
}
impl NativeMultiBoxPlan {
    /// Return the union of native indexes hidden by all supported Boxes.
    /// Every group not included in overrides retains its recorded active
    /// Tab. Reject unknown groups, duplicate overrides and invalid Tabs
    /// instead of silently redirecting another Box's icons.
    pub fn hidden_for(&self,overrides:&[(u64,usize)])
        -> Result<Vec<usize>,String> {
        let mut selections=BTreeMap::new();
        for &(id,tab) in overrides {
            if selections.insert(id,tab).is_some() {
                return Err("Multiple Tab selections supplied for one Box".into());
            }
        }
        if selections.keys().any(|id|!self.groups.iter().any(|g|g.group_id==*id)){
            return Err("Selection attempts to change an unsupported native Box".into());
        }
        let mut indices=std::collections::BTreeSet::new();
        for group in &self.groups {
            let selected=selections.get(&group.group_id).copied()
                .unwrap_or(group.active);
            for index in group.hidden_for(selected)? {
                if !indices.insert(index){
                    return Err("Multiple native Boxes attempted to hide the same Explorer item".into());
                }
            }
        }
        Ok(indices.into_iter().collect())
    }
}

/// Build a fail-open multi-Box plan without changing State, Explorer items,
/// icon positions or the desktop namespace. A scoped item must not be
/// claimed anywhere else in State, even in an unsupported Box. Otherwise
/// hiding it could remove a real icon still needed by that other Box.
pub fn native_multi_box_plan(state:&State,explorer:&[ViewItem],
                             max_groups:usize,max_items_per_tab:usize)
    -> Result<NativeMultiBoxPlan,String> {
    if max_groups==0 || max_groups>16 || max_items_per_tab==0
        || max_items_per_tab>128 {
        return Err("Native multi-Box limits exceed verified bounds".into());
    }
    let by_path=native_path_index(explorer);
    let mut groups=Vec::new();
    let mut skipped=Vec::new();
    let mut seen_group_ids=std::collections::HashSet::new();
    for group in &state.groups {
        if !seen_group_ids.insert(group.id){
            return Err("DesktopCraft State contains duplicate Box IDs".into());
        }
        if group.page!=state.page || group.rolled || group.tabs.len()<2 {
            skipped.push((group.id,"Other Page, rolled-up or single-Tab Box".into()));
            continue;
        }
        match group_tab_plan(group,state.page,explorer,&by_path,max_items_per_tab,None) {
            Ok(plan)=>groups.push(plan),
            Err(reason)=>skipped.push((group.id,reason)),
        }
    }
    if groups.is_empty(){
        return Err("No fully verifiable native multi-Tab Desktop Boxes".into());
    }
    if groups.len()>max_groups {
        return Err("Native multi-Box plan exceeds configured group limit".into());
    }
    let mut all_references=BTreeMap::<String,usize>::new();
    for group in &state.groups {
        for tab in &group.tabs {
            if tab.folder.is_some(){continue;}
            for item in &tab.items {
                let key=item.path.to_lowercase();
                if by_path.contains_key(&key){
                    *all_references.entry(key).or_default()+=1;
                }
            }
        }
    }
    let mut claimed=std::collections::HashSet::new();
    for group in &groups {
        for indices in &group.native_indices {
            for &index in indices {
                let item=explorer.get(index)
                    .ok_or("Native plan index no longer exists")?;
                let key=item.path.to_lowercase();
                if by_path.get(&key)!=Some(&Some(index))
                    || all_references.get(&key)!=Some(&1)
                    || !claimed.insert(index) {
                    return Err("Native Desktop icon is claimed by multiple Boxes or Tabs".into());
                }
            }
        }
    }
    let plan=NativeMultiBoxPlan{
        groups,skipped,
        unclaimed_native_items:explorer.len().saturating_sub(claimed.len()),
    };
    let _=plan.hidden_for(&[])?; // Validate composed initial policy.
    Ok(plan)
}

/// General DesktopCraft native workspace plan. Supports any bounded number of
/// *visible regular* Boxes (including 0 when all Boxes are rolled up, on another
/// Page, or on Folder Portals, provided the workspace defines at least one regular
/// Desktop-backed tab), including single-Tab Boxes, without assuming particular
/// user IDs. Folder Portals are separate Shell namespaces: only their regular
/// Desktop sibling tabs can participate, and the Portal itself must be handled
/// by the existing linked-folder renderer.
///
/// Keep saved Pins intact. A Desktop file or shortcut confirmed missing
/// from both Explorer and disk contributes no index to the native layout.
/// Other unverifiable or ambiguous identities still refuse the native layout.
pub(crate) fn user_native_workspace_plan(state: &State, explorer: &[ViewItem]) -> Result<NativeMultiBoxPlan, String> {
    native_workspace_plan(state, explorer, usize::MAX, crate::native_filter_protocol::MAX_ITEMS)
}

pub fn native_workspace_plan(
    state:&State,explorer:&[ViewItem],max_groups:usize,
    max_items_per_tab:usize,
)->Result<NativeMultiBoxPlan,String>{
    if max_groups==0 || max_items_per_tab==0
        || max_items_per_tab>crate::native_filter_protocol::MAX_ITEMS {
        return Err("Native workspace limits exceed verified bounds".into());
    }
    if explorer.is_empty() || explorer.len()>2048{
        return Err("Native workspace requires bounded real Explorer identities".into());
    }
    let by_path=native_path_index(explorer);
    if by_path.len()!=explorer.len() || explorer.iter().any(|item|
        item.path.is_empty() || item.label.trim().is_empty()) {
        return Err("Explorer native workspace identity duplicated or missing".into());
    }
    let mut ownership=vec![false;explorer.len()];
    let mut group_ids=std::collections::BTreeSet::new();
    for group in &state.groups {
        if !group_ids.insert(group.id){
            return Err("Native Workspace has duplicate Box IDs".into());
        }
        for tab in &group.tabs {
            if tab.folder.is_some(){continue;}
            for item in &tab.items {
                if let Some(Some(index))=by_path.get(&item.path.to_lowercase()) {
                    if std::mem::replace(&mut ownership[*index],true) {
                        return Err("Explorer desktop item is multiply pinned across Boxes/Tabs/Pages".into());
                    }
                }
            }
        }
    }
    let mut groups=Vec::new();
    let mut skipped=Vec::new();
    let desktop_folders = OnceCell::new();
    for group in &state.groups {
        if group.page!=state.page || group.rolled{
            skipped.push((group.id,"Inactive Page or rolled-up Box".into()));
            continue;
        }
        if group.tabs.is_empty() || group.active>=group.tabs.len(){
            return Err("Visible Native Box contains no valid selected Tab".into());
        }
        if group.tabs[group.active].folder.is_some(){
            skipped.push((group.id,"Linked Folder Portal owns its own Shell View".into()));
            continue;
        }
        let plan=group_tab_plan(group,state.page,explorer,&by_path,max_items_per_tab,Some(&desktop_folders))
            .map_err(|e|format!("Native Box {} is not safe to place: {e}",group.id))?;
        groups.push(plan);
        if groups.len()>max_groups{
            return Err("Native workspace has more visible regular Boxes than approved".into());
        }
    }
    if groups.is_empty()
        && !state
            .groups
            .iter()
            .any(|group| group.tabs.iter().any(|tab| tab.folder.is_none()))
    {
        return Err("No active regular Desktop-backed Boxes available for native layout".into());
    }
    let plan=NativeMultiBoxPlan{
        groups,skipped,
        unclaimed_native_items:ownership.iter().filter(|claimed|!**claimed).count(),
    };
    let _=plan.hidden_for(&[])?;
    Ok(plan)
}

/// Validated logical drop of an existing *virtual* Desktop pin between
/// two Explorer-backed Boxes. No Windows file is ever renamed, copied, moved
/// or deleted. This produces an uncommitted cloned State and an updated
/// visibility plan; real mouse/drop routing must call this separately.
pub fn plan_native_pin_transfer(
    original:&State,
    explorer:&[ViewItem],
    source_group:u64,
    source_tab_id:u64,
    path:&str,
    destination_group:u64,
    destination_tab_id:u64,
    max_groups:usize,
    max_items_per_tab:usize,
) -> Result<(State,NativeMultiBoxPlan),String> {
    let before=native_multi_box_plan(original,explorer,max_groups,max_items_per_tab)?;
    if !before.groups.iter().any(|group|group.group_id==source_group)
        || !before.groups.iter().any(|group|group.group_id==destination_group){
        return Err("Native pin transfer requires two verified regular Desktop Boxes".into());
    }
    let path_key=path.to_lowercase();
    if explorer.iter().filter(|item|item.path.to_lowercase()==path_key).count()!=1 {
        return Err("Dragged pin has no unique native Explorer identity".into());
    }
    let source=original.group(source_group)
        .and_then(|group|group.tabs.iter().find(|tab|
            tab.id==source_tab_id && tab.folder.is_none()))
        .ok_or("Native drop source Tab no longer exists")?;
    let matched=source.items.iter().filter(|item|
        item.path.eq_ignore_ascii_case(path)).collect::<Vec<_>>();
    if matched.len()!=1 {
        return Err("Dragged native pin was not uniquely present in source Tab".into());
    }
    let item_id=matched[0].id;
    let destination=original.group(destination_group)
        .and_then(|group|group.tabs.iter().find(|tab|
            tab.id==destination_tab_id && tab.folder.is_none()))
        .ok_or("Native drop destination Tab is not a regular native Tab")?;
    if destination.items.iter().any(|item|item.path.eq_ignore_ascii_case(path)){
        return Err("Native drop destination already contains this pin".into());
    }
    let mut next=original.clone();
    next.transfer_between_tabs(
        source_group,source_tab_id,item_id,destination_group,
        destination_tab_id,destination.items.len())?;
    let after=native_multi_box_plan(&next,explorer,max_groups,max_items_per_tab)?;
    // This is an all-or-nothing native operation. If the move makes the
    // destination exceed the scoped item limit, the planner may skip it;
    // NEVER accept that partial policy or the moved icon would appear loose.
    if !after.groups.iter().any(|group|group.group_id==source_group)
        || !after.groups.iter().any(|group|group.group_id==destination_group) {
        return Err("Native drop would invalidate the source or destination Box policy".into());
    }
    Ok((next,after))
}

/// Generalized virtual Pin transfer for Explorer-backed regular Tabs in
/// an arbitrary (bounded) workspace. No on-disk rename/copy/move occurs.
/// Uses the same stable pin IDs and State transfer method as the existing
/// proven two-Box path, but validates every native Box and Page rather than
/// depending on the legacy multi-Tab-only eligibility check.
pub fn plan_native_workspace_pin_transfer(
    original:&State,explorer:&[ViewItem],source_group:u64,
    source_tab_id:u64,path:&str,destination_group:u64,
    destination_tab_id:u64,
)->Result<(State,NativeMultiBoxPlan),String>{
    let before=native_workspace_plan(original,explorer,16,128)?;
    if source_group==destination_group
        || !before.groups.iter().any(|g|g.group_id==source_group)
        || !before.groups.iter().any(|g|g.group_id==destination_group){
        return Err("Native transfer must cross two distinct eligible regular Boxes".into());
    }
    if explorer.iter().filter(|item|item.path.eq_ignore_ascii_case(path)).count()!=1{
        return Err("Native drag source has no unique Explorer identity".into());
    }
    let source=original.group(source_group).ok_or("Source Box missing")?;
    let active_source=source.tabs.get(source.active)
        .filter(|tab|tab.id==source_tab_id && tab.folder.is_none())
        .ok_or("Source is not the currently active regular Tab")?;
    let matching=active_source.items.iter()
        .filter(|pin|pin.path.eq_ignore_ascii_case(path)).collect::<Vec<_>>();
    if matching.len()!=1{
        return Err("Source virtual Pin not unique in active native Tab".into());
    }
    let id=matching[0].id;
    let target=original.group(destination_group).ok_or("Target Box missing")?;
    let active_target=target.tabs.get(target.active)
        .filter(|tab|tab.id==destination_tab_id && tab.folder.is_none())
        .ok_or("Destination is not an active regular native Tab")?;
    if active_target.items.len()>=128
        || active_target.items.iter().any(|pin|pin.path.eq_ignore_ascii_case(path)) {
        return Err("Native transfer destination full or already contains source".into());
    }
    let mut next=original.clone();
    next.transfer_between_tabs(source_group,source_tab_id,id,
        destination_group,destination_tab_id,active_target.items.len())?;
    let after=native_workspace_plan(&next,explorer,16,128)?;
    if !after.groups.iter().any(|g|g.group_id==source_group)
        || !after.groups.iter().any(|g|g.group_id==destination_group){
        return Err("Native transfer invalidated a required Box membership".into());
    }
    Ok((next,after))
}

/// Hit-test the *physical screen* drop coordinates against verified native
/// DesktopCraft Box bodies. Only a regular currently visible tab is eligible;
/// overlapping candidates are ambiguous and reject the entire transaction.
/// Header controls are not drop targets (safe no-op instead of wrong Tab).
pub fn native_drop_target_for_screen_point(
    state:&State,plan:&NativeMultiBoxPlan,source_group:u64,
    screen_x:i32,screen_y:i32,
) -> Result<(u64,u64),String>{
    let x=screen_x as f64;
    let y=screen_y as f64;
    let mut matches=Vec::new();
    for verified in &plan.groups {
        if verified.group_id==source_group {continue;}
        let Some(group)=state.group(verified.group_id) else {
            return Err("Verified Box no longer exists in State".into());
        };
        if group.rolled || group.page!=state.page || group.active>=group.tabs.len(){
            continue;
        }
        let rect=group.bounds;
        let (left,top,width,height)=(
            rect.x as f64,rect.y as f64,rect.w as f64,rect.h as f64);
        if ![left,top,width,height].iter().all(|v|v.is_finite())
            || width<80.0 || height<85.0 {
            continue;
        }
        // Titlebar/tab-strip and resize handles are not drop targets.
        let inset=8.0;
        let header=46.0;
        if x>=left+inset && x<left+width-inset &&
            y>=top+header && y<top+height-inset &&
            group.tabs[group.active].folder.is_none(){
            matches.push((group.id,group.tabs[group.active].id));
        }
    }
    match matches.as_slice(){
        [target]=>Ok(*target),
        []=>Err("Drop position is outside every verified native Box body".into()),
        _=>Err("Overlapping native Box drop zones are ambiguous".into()),
    }
}

pub fn plan_native_ole_drop(
    original: &State,
    explorer: &[ViewItem],
    verified: &NativeMultiBoxPlan,
    ole_paths: &[std::path::PathBuf],
    target_group: u64,
    target_tab: u64,
    max_groups: usize,
    max_items_per_tab: usize,
) -> Result<(State, NativeMultiBoxPlan), String> {
    // A bounded selection is one indivisible gesture. Never partially
    // transfer a CF_HDROP list or infer a source from the current cursor.
    if ole_paths.is_empty() || ole_paths.len() > 8 {
        return Err("Native OLE Box drop supports 1..=8 distinct source items".into());
    }
    let now=native_multi_box_plan(original,explorer,max_groups,max_items_per_tab)?;
    if &now!=verified {
        return Err("Native OLE Desktop/Box policy changed during drag".into());
    }
    let destination=verified.groups.iter().find(|g|g.group_id==target_group)
        .ok_or("Native OLE target Box is not verified")?;
    let dest_group=original.group(target_group)
        .ok_or("Native OLE target Box vanished")?;
    if dest_group.page!=original.page || dest_group.rolled ||
       dest_group.active!=destination.active ||
       dest_group.tabs.get(dest_group.active).is_none_or(|tab|
           tab.id!=target_tab || tab.folder.is_some()) {
        return Err("Native OLE target is not the visible regular Tab".into());
    }
    let mut unique=std::collections::BTreeSet::new();
    let mut source_claim:Option<(u64,usize)>=None;
    let mut transfers=Vec::with_capacity(ole_paths.len());
    for offered in ole_paths {
        let path=offered.to_str()
            .filter(|v| !v.trim().is_empty())
            .ok_or("OLE source path is not valid Unicode")?;
        if !unique.insert(path.to_lowercase()) {
            return Err("Native OLE selection has duplicate source paths".into());
        }
        let indices=explorer.iter().enumerate()
            .filter(|(_,item)|item.path.eq_ignore_ascii_case(path))
            .map(|(i,_)|i).collect::<Vec<_>>();
        let [native_index]=indices.as_slice() else {
            return Err("Native OLE selected path has no unique Explorer Desktop identity".into());
        };
        let claims=verified.groups.iter().filter_map(|g|
            g.native_indices.get(g.active)
                .filter(|items|items.contains(native_index))
                .map(|_|(g.group_id,g.active))
        ).collect::<Vec<_>>();
        let [claim]=claims.as_slice() else {
            return Err("Every OLE selected item must belong to one visible Box Tab".into());
        };
        if let Some(previous)=source_claim {
            if previous!=*claim {
                return Err("All OLE selected items must come from the same visible Tab".into());
            }
        }else{
            source_claim=Some(*claim);
        }
        let from_group=original.group(claim.0)
            .ok_or("Native OLE source Box disappeared")?;
        let from_tab=from_group.tabs.get(claim.1)
            .filter(|tab|tab.folder.is_none())
            .ok_or("Native OLE source Tab not regular")?;
        let matched=from_tab.items.iter()
            .filter(|item|item.path.eq_ignore_ascii_case(path))
            .collect::<Vec<_>>();
        let [source_pin]=matched.as_slice() else {
            return Err("Selected OLE native Pin not unique in its source Tab".into());
        };
        if dest_group.tabs[dest_group.active].items.iter()
            .any(|item|item.path.eq_ignore_ascii_case(path)) {
            return Err("Destination already contains a selected native Pin".into());
        }
        transfers.push((source_pin.id,path.to_owned()));
    }
    let (src_group,src_tab_index)=source_claim
        .ok_or("OLE selection has no source Tab")?;
    if src_group==target_group {
        return Err("Native OLE source and destination Boxes must differ".into());
    }
    let src_tab_id=original.group(src_group)
        .and_then(|group|group.tabs.get(src_tab_index))
        .ok_or("Native OLE source Tab disappeared")?.id;
    let dest_count=dest_group.tabs[dest_group.active].items.len();
    if dest_count.checked_add(transfers.len()).is_none_or(|n|n>max_items_per_tab) {
        return Err("OLE selection exceeds the destination native Tab capacity".into());
    }
    let mut next=original.clone();
    for (offset,(_,path)) in transfers.iter().enumerate() {
        // Native Explorer identity is a FULL PATH, not an Item ID. An
        // older imported Workspace may contain duplicate item IDs across
        // different shortcuts. Move only the exact previously validated Pin.
        let moved={
            let from=next.group_mut(src_group)
                .and_then(|g|g.tabs.iter_mut().find(|t|t.id==src_tab_id))
                .ok_or("Native OLE source Tab removed while staging")?;
            let matches=from.items.iter().enumerate()
                .filter(|(_,pin)|pin.path.eq_ignore_ascii_case(path))
                .map(|(i,_)|i).collect::<Vec<_>>();
            let [position]=matches.as_slice() else {
                return Err("Native OLE source identity changed while staging".into());
            };
            from.items.remove(*position)
        };
        let target=next.group_mut(target_group)
            .and_then(|g|g.tabs.iter_mut().find(|t|t.id==target_tab))
            .ok_or("Native OLE destination Tab removed while staging")?;
        target.items.insert((dest_count+offset).min(target.items.len()),moved);
    }
    let after=native_multi_box_plan(&next,explorer,max_groups,max_items_per_tab)?;
    if !after.groups.iter().any(|g|g.group_id==src_group && g.active==src_tab_index)
        || !after.groups.iter().any(|g|g.group_id==target_group &&
            g.active==destination.active) {
        return Err("Native multi-Pin drop invalidated source or destination policy".into());
    }
    let destination_after=next.group(target_group)
        .and_then(|g|g.tabs.iter().find(|t|t.id==target_tab))
        .ok_or("Native multi-Pin destination Tab disappeared")?;
    if transfers.iter().any(|(_,path)|
        destination_after.items.iter()
            .filter(|pin|pin.path.eq_ignore_ascii_case(path)).count()!=1) {
        return Err("Native OLE multi-Pin postflight source identities invalid".into());
    }
    Ok((next,after))
}

/// The event itself comes from Explorer's verified in-process ListView, not
/// a fake cursor/source icon. Host commits nothing until the source identity,
/// drop position, and post-transfer native policy ALL validate.
pub fn plan_native_drag_release(
    state:&State,explorer:&[ViewItem],plan:&NativeMultiBoxPlan,
    source_index:usize,end_x:i32,end_y:i32,
    max_groups:usize,max_items_per_tab:usize,
)->Result<(State,NativeMultiBoxPlan,u64,u64),String>{
    let Some(source)=explorer.get(source_index) else {
        return Err("Native drag source index is no longer present".into());
    };
    let mut claims=Vec::new();
    for group in &plan.groups {
        for (index,items) in group.native_indices.iter().enumerate(){
            if index==group.active && items.contains(&source_index){
                let actual=state.group(group.group_id)
                    .ok_or("Source Box removed during native gesture")?;
                claims.push((group.group_id,actual.tabs[index].id));
            }
        }
    }
    let [(source_group,source_tab)]=claims.as_slice() else {
        return Err("Native dragged icon must belong to exactly one visible Tab".into());
    };
    let (destination_group,destination_tab)=native_drop_target_for_screen_point(
        state,plan,*source_group,end_x,end_y)?;
    let (next,after)=plan_native_pin_transfer(state,explorer,
        *source_group,*source_tab,&source.path,destination_group,destination_tab,
        max_groups,max_items_per_tab)?;
    Ok((next,after,destination_group,destination_tab))
}

/// Atomic model-only release of multiple REAL selected Explorer native icons.
/// Source indices are read from the live in-process ListView at MouseDown,
/// not guessed from file names. All must belong to exactly one active Tab.
/// Never sends file operations, alters icon positions, or saves Workspace.
pub fn plan_native_selected_drag_release(
    state:&State,
    explorer:&[ViewItem],
    plan:&NativeMultiBoxPlan,
    source_indices:&[usize],
    end_x:i32,
    end_y:i32,
    max_groups:usize,
    max_items_per_tab:usize,
)->Result<(State,NativeMultiBoxPlan,u64,u64),String>{
    if source_indices.is_empty() || source_indices.len()>8 {
        return Err("Native selected drag needs 1..=8 source icons".into());
    }
    let mut seen=std::collections::BTreeSet::new();
    let mut paths=Vec::with_capacity(source_indices.len());
    let mut source:Option<u64>=None;
    for &index in source_indices {
        if !seen.insert(index) {
            return Err("Explorer native drag selection has duplicate index".into());
        }
        let item=explorer.get(index)
            .ok_or("Explorer drag selected native index is invalid")?;
        let claims=plan.groups.iter().filter_map(|g|
            g.native_indices.get(g.active)
                .filter(|indices|indices.contains(&index))
                .map(|_|g.group_id)
        ).collect::<Vec<_>>();
        let [owner]=claims.as_slice() else {
            return Err("Every selected Explorer native item must belong to one visible Tab".into());
        };
        if let Some(previous)=source {
            if previous!=*owner {
                return Err("Explorer selection spans multiple source Boxes".into());
            }
        }else {source=Some(*owner);}
        paths.push(std::path::PathBuf::from(&item.path));
    }
    let source_group=source.ok_or("No verified Explorer drag source Box")?;
    let (destination_group,destination_tab)=native_drop_target_for_screen_point(
        state,plan,source_group,end_x,end_y)?;
    let (next,after)=plan_native_ole_drop(
        state,explorer,plan,&paths,destination_group,destination_tab,
        max_groups,max_items_per_tab)?;
    Ok((next,after,destination_group,destination_tab))
}

/// Read-only safety gate for a future in-Explorer hook. Never send an item
/// index computed from IFolderView to the native ListView unless *all* labels
/// match its live MSAA order and no display label is ambiguous. An index is
/// still an ephemeral handle and must be revalidated after every view change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeIndexAudit {
    pub shell_count: usize,
    pub accessibility_count: usize,
    pub same_index_labels: usize,
    pub ambiguous_shell_names: usize,
    pub ambiguous_accessibility_names: usize,
    pub empty_names: usize,
    pub matching_order: bool,
    pub safe_to_address_by_index: bool,
    /// Safe to construct a full path->name bijection without trusting index.
    /// DLL still validates actual native ListView index before acting.
    pub safe_to_reconcile_by_name: bool,
    pub reconciled_names: usize,
}

pub fn audit_native_indices(shell: &[ViewItem], accessible_names: &[String]) -> NativeIndexAudit {
    use std::collections::HashSet;
    let mut shell_labels = HashSet::new();
    let mut acc_labels = HashSet::new();
    let mut ambiguous_shell_names = 0;
    let mut ambiguous_accessibility_names = 0;
    let mut empty_names = 0;
    let mut same_index_labels = 0;
    for item in shell {
        let label = item.label.trim().to_lowercase();
        if label.is_empty() { empty_names += 1; }
        else if !shell_labels.insert(label) { ambiguous_shell_names += 1; }
    }
    for label in accessible_names {
        let label = label.trim().to_lowercase();
        if label.is_empty() { empty_names += 1; }
        else if !acc_labels.insert(label) { ambiguous_accessibility_names += 1; }
    }
    for (item, acc) in shell.iter().zip(accessible_names) {
        if item.label == *acc { same_index_labels += 1; }
    }
    let matching_order = shell.len() == accessible_names.len()
        && same_index_labels == shell.len() && !shell.is_empty();
    let reconciled_names=shell_labels.intersection(&acc_labels).count();
    let safe_to_reconcile_by_name=shell.len()>0
        && shell.len()==accessible_names.len()
        && reconciled_names==shell.len() && empty_names==0
        && ambiguous_shell_names==0 && ambiguous_accessibility_names==0;
    let safe_to_address_by_index=matching_order && safe_to_reconcile_by_name;
    NativeIndexAudit {
        shell_count: shell.len(), accessibility_count: accessible_names.len(),
        same_index_labels, ambiguous_shell_names, ambiguous_accessibility_names,
        empty_names, matching_order, safe_to_address_by_index,
        safe_to_reconcile_by_name, reconciled_names,
    }
}

/// Resolve native Explorer paths in the *current* accessibility ListView order
/// through a complete unique display-name bijection. Unlike Shell enumeration
/// order, MSAA/ListView ordering can change after Explorer restarts. This
/// mapping grants NO permission to filter by itself: the DLL independently
/// checks each index through in-process LVM_GETITEMTEXTW first.
pub fn native_view_in_msaa_order(shell:&[ViewItem], names:&[String])
    -> Result<Vec<ViewItem>,String> {
    let audit=audit_native_indices(shell,names);
    if !audit.safe_to_reconcile_by_name {
        return Err(format!("Native names/paths cannot be matched bijectively: {}/{}",
            audit.reconciled_names,shell.len()));
    }
    let mut by_name=std::collections::HashMap::with_capacity(shell.len());
    for item in shell {by_name.insert(item.label.trim().to_lowercase(),item);}
    names.iter().map(|name|{
        let found=by_name.get(&name.trim().to_lowercase())
            .ok_or_else(||format!("Missing native Shell path for ListView label"))?;
        Ok(ViewItem{path:found.path.clone(),label:name.clone(),pos:found.pos})
    }).collect()
}

impl NativeIndexAudit {
    pub fn to_json(&self) -> Value {
        json!({
            "shell_items": self.shell_count,
            "accessible_items": self.accessibility_count,
            "same_index_labels": self.same_index_labels,
            "ambiguous_shell_labels": self.ambiguous_shell_names,
            "ambiguous_accessibility_labels": self.ambiguous_accessibility_names,
            "empty_labels": self.empty_names,
            "ordered_labels_match": self.matching_order,
            "safe_to_address_by_index": self.safe_to_address_by_index,
            "safe_to_reconcile_by_name": self.safe_to_reconcile_by_name,
            "reconciled_names": self.reconciled_names,
            "not_a_persistent_identity": true,
            "warning": "Read-only structural check. If false, do not use native ListView indexes for visibility or selection. Even if true, indexes must be revalidated after native reflow/re-enumeration."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bounds, Item};

    fn item(path: &str) -> Item {
        Item { id: 10, name: path.into(), path: path.into(),
            kind: "lnk".into(), size: 0, modified: 0, label: None }
    }
    fn native(path: &str) -> ViewItem {
        ViewItem { path: path.into(), label: path.into(), pos: (0, 0) }
    }
    fn box_bounds() -> Bounds { Bounds { x: 0., y: 0., w: 300., h: 220. } }
    fn status(plan: &NativePresentation, path: &str) -> NativeVisibility {
        plan.items.iter().find(|i| i.path.eq_ignore_ascii_case(path)).unwrap().visibility
    }

    #[test]
    fn real_desktop_items_change_visibility_when_tab_changes() {
        let mut state = State::empty();
        let g = state.add_group("Work", box_bounds());
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/active.lnk"));
        state.add_tab(g).unwrap();
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/inactive.lnk"));
        state.group_mut(g).unwrap().active = 0;
        let actual = [native("c:/desktop/ACTIVE.lnk"), native("C:/Desktop/inactive.lnk"), native("C:/Desktop/loose.txt")];
        let first = NativePresentation::decide(&state, &actual);
        assert_eq!(status(&first, "C:/Desktop/active.lnk"), NativeVisibility::Active);
        assert_eq!(status(&first, "C:/Desktop/inactive.lnk"), NativeVisibility::InactiveTab);
        assert_eq!(status(&first, "C:/Desktop/loose.txt"), NativeVisibility::Loose);
        state.group_mut(g).unwrap().active = 1;
        let second = NativePresentation::decide(&state, &actual);
        assert_eq!(status(&second, "C:/Desktop/active.lnk"), NativeVisibility::InactiveTab);
        assert_eq!(status(&second, "C:/Desktop/inactive.lnk"), NativeVisibility::Active);
        assert_eq!(second.changes_since(&first).len(), 2);
        state.group_mut(g).unwrap().active = 0;
        assert!(NativePresentation::decide(&state, &actual).changes_since(&first).is_empty());
    }

    #[test]
    fn never_create_native_items_for_external_pins() {
        let mut state = State::empty();
        let g = state.add_group("Tools", box_bounds());
        state.group_mut(g).unwrap().tab_mut().items.push(item("D:/Apps/tool.exe"));
        let result = NativePresentation::decide(&state, &[native("C:/Desktop/loose.txt")]);
        assert_eq!(result.external_pin_references, 1);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].visibility, NativeVisibility::Loose);
    }

    #[test]
    fn duplicate_native_identity_is_counted_not_drawn_twice() {
        let mut state = State::empty();
        let a = state.add_group("A", box_bounds());
        let b = state.add_group("B", box_bounds());
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/shared.txt"));
        state.group_mut(b).unwrap().tab_mut().items.push(item("c:/desktop/SHARED.TXT"));
        let result = NativePresentation::decide(&state, &[native("C:/Desktop/Shared.txt")]);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.conflicting_active_references, 1);
        assert_eq!(result.items[0].active_references, 2);
    }

    #[test]
    fn visible_claim_wins_over_inactive_duplicate_without_losing_identity() {
        let mut state = State::empty();
        let g = state.add_group("Work", box_bounds());
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/a.lnk"));
        state.add_tab(g).unwrap();
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/a.lnk"));
        state.group_mut(g).unwrap().active = 0;
        let result = NativePresentation::decide(&state, &[native("C:/Desktop/a.lnk")]);
        assert_eq!(result.items[0].visibility, NativeVisibility::Active);
        assert_eq!(result.items[0].total_references, 2);
        assert_eq!(result.conflicting_active_references, 0);
    }

    #[test]
    fn other_pages_and_collapsed_boxes_are_not_active() {
        let mut state = State::empty();
        let g = state.add_group("Page 1", box_bounds());
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/rolled.txt"));
        state.group_mut(g).unwrap().rolled = true;
        let other = state.add_group("Page 2", box_bounds());
        state.group_mut(other).unwrap().tab_mut().items.push(item("C:/Desktop/other.txt"));
        state.group_mut(other).unwrap().page = 1;
        let result = NativePresentation::decide(&state, &[native("C:/Desktop/rolled.txt"), native("C:/Desktop/other.txt")]);
        assert_eq!(status(&result, "C:/Desktop/rolled.txt"), NativeVisibility::RolledUp);
        assert_eq!(status(&result, "C:/Desktop/other.txt"), NativeVisibility::InactivePage);
    }

    #[test]
    fn tab_pair_chooses_native_items_from_real_distinct_tabs() {
        let mut state=State::empty();
        let group=state.add_group("Tools",box_bounds());
        state.group_mut(group).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(group).unwrap();
        state.group_mut(group).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(group).unwrap().active=0;
        let real=[native("C:/Desktop/A.lnk"),native("C:/Desktop/B.lnk"),native("C:/Desktop/C.lnk")];
        let result=choose_native_tab_pair(&state,&real,group,Some(1)).unwrap();
        assert_eq!(result.from_index,Some(0));
        assert_eq!(result.to_index,Some(1));
        assert_eq!(result.from_tab,0);
        assert_eq!(result.to_tab,1);
    }

    #[test]
    fn full_native_tab_policy_supports_empty_source_after_last_pin_transfer() {
        let mut state=State::empty();
        let group=state.add_group("Craft",box_bounds());
        state.add_tab(group).unwrap();
        state.group_mut(group).unwrap().tab_mut().items
            .push(item("C:/Desktop/B.lnk"));
        state.group_mut(group).unwrap().tab_mut().items
            .push(item("C:/Desktop/C.lnk"));
        state.group_mut(group).unwrap().active=0;
        let native_items=[
            native("C:/Desktop/B.lnk"),
            native("C:/Desktop/C.lnk"),
            native("C:/Desktop/Loose.lnk")
        ];
        // Old representative two-nonempty icon probe stays strict.
        assert!(choose_native_tab_pair(&state,&native_items,group,Some(1)).is_err());
        let full=choose_native_tab_set(&state,&native_items,group,Some(1),3).unwrap();
        assert_eq!(full.pair.from_index,None);
        assert_eq!(full.from_indices,Vec::<usize>::new());
        assert_eq!(full.to_indices,vec![0,1]);
        assert_eq!(full.hidden_indices(0),vec![0,1]);
        assert_eq!(full.hidden_indices(1),Vec::<usize>::new());
        let reverse=state.group_mut(group).unwrap();
        reverse.active=1;
        let reversed=choose_native_tab_set(&state,&native_items,group,Some(0),3).unwrap();
        assert_eq!(reversed.to_indices,Vec::<usize>::new());
        assert_eq!(reversed.pair.to_index,None);
        assert_eq!(reversed.hidden_indices(0),Vec::<usize>::new());
        assert_eq!(reversed.hidden_indices(1),vec![0,1]);
    }
    #[test]
    fn native_empty_tab_policy_rejects_both_empty_portal_and_duplicate_identity() {
        let mut state=State::empty();
        let group=state.add_group("Craft",box_bounds());
        state.add_tab(group).unwrap();
        state.group_mut(group).unwrap().active=0;
        let icons=[native("C:/Desktop/B.lnk")];
        assert!(choose_native_tab_set(&state,&icons,group,Some(1),3).is_err());
        state.group_mut(group).unwrap().tabs[1].folder=Some("D:/Portal".into());
        assert!(choose_native_tab_set(&state,&icons,group,Some(1),3).is_err());
        state.group_mut(group).unwrap().tabs[1].folder=None;
        state.group_mut(group).unwrap().tabs[1].items
            .push(item("C:/Desktop/B.lnk"));
        let duplicated=[icons[0].clone(),icons[0].clone()];
        assert!(choose_native_tab_set(&state,&duplicated,group,Some(1),3).is_err());
        assert!(choose_native_tab_set(&state,&icons,group,Some(1),0).is_err());
    }

    #[test]
    fn tab_pair_never_guesses_unknown_or_same_native_identity() {
        let mut state=State::empty();
        let group=state.add_group("Tools",box_bounds());
        state.group_mut(group).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(group).unwrap();
        state.group_mut(group).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.group_mut(group).unwrap().active=0;
        assert!(choose_native_tab_pair(&state,&[native("C:/Desktop/A.lnk")],group,None).is_err());
        state.group_mut(group).unwrap().tabs[1].items.clear();
        state.group_mut(group).unwrap().tabs[1].items.push(item("D:/Outside.exe"));
        assert!(choose_native_tab_pair(&state,&[native("C:/Desktop/A.lnk")],group,None).is_err());
        state.group_mut(group).unwrap().tabs[1].items.clear();
        state.group_mut(group).unwrap().tabs[1].items.push(item("C:/Desktop/B.lnk"));
        assert!(choose_native_tab_pair(&state,&[native("C:/Desktop/A.lnk"),native("C:/Desktop/A.lnk"),native("C:/Desktop/B.lnk")],group,None).is_err());
    }

    #[test]
    fn complete_tab_set_uses_all_real_members_without_duplicates() {
        let mut state=State::empty();
        let group=state.add_group("Craft/Work",box_bounds());
        for name in ["A","B","C"] {
            state.group_mut(group).unwrap().tab_mut().items.push(item(
                &format!("C:/Desktop/{name}.lnk")));
        }
        state.add_tab(group).unwrap();
        for name in ["D","E"] {
            state.group_mut(group).unwrap().tab_mut().items.push(item(
                &format!("C:/Desktop/{name}.lnk")));
        }
        state.group_mut(group).unwrap().active=0;
        let explorer=["A","B","C","D","E"].iter().map(|s|
            native(&format!("C:/Desktop/{s}.lnk"))).collect::<Vec<_>>();
        let all=choose_native_tab_set(&state,&explorer,group,Some(1),3).unwrap();
        assert_eq!(all.from_indices,vec![0,1,2]);
        assert_eq!(all.to_indices,vec![3,4]);
        assert_eq!(all.hidden_indices(0),&[3,4]);
        assert_eq!(all.hidden_indices(1),&[0,1,2]);
        assert_eq!(all.hidden_indices(2),&[3,4]);
        assert!(choose_native_tab_set(&state,&explorer,group,Some(1),2).is_err());
        state.group_mut(group).unwrap().tabs[1].items.push(item("D:/Apps/not-native.exe"));
        assert!(choose_native_tab_set(&state,&explorer,group,Some(1),3).is_err());
    }

    #[test]
    fn native_pin_drop_rejects_folder_portal_and_duplicate_claims() {
        let mut state=State::empty();
        let a=state.add_group("A",box_bounds());
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(a).unwrap();
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(a).unwrap().active=0;
        let b=state.add_group("B",box_bounds());
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.add_tab(b).unwrap();
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/D.lnk"));
        state.group_mut(b).unwrap().active=0;
        let from=state.group(a).unwrap().tabs[0].id;
        let to=state.group(b).unwrap().tabs[0].id;
        let native_items=["A","B","C","D"].iter().map(|v|
            native(&format!("C:/Desktop/{v}.lnk"))).collect::<Vec<_>>();
        state.group_mut(b).unwrap().tabs[1].folder=Some("D:/Portal".into());
        let (allowed,_) = plan_native_pin_transfer(&state,&native_items,
            a,from,"C:/Desktop/A.lnk",b,to,4,3).unwrap();
        assert_eq!(allowed.group(b).unwrap().tabs[1].folder.as_deref(),
            Some("D:/Portal"));
        // An inactive Portal is harmless; an active Portal cannot be the
        // receiver of native Desktop Pins and must use its own Shell View.
        state.group_mut(b).unwrap().active=1;
        assert!(plan_native_pin_transfer(&state,&native_items,
            a,from,"C:/Desktop/A.lnk",b,to,4,3).is_err());
        state.group_mut(b).unwrap().active=0;
        state.group_mut(b).unwrap().tabs[1].folder=None;
        state.group_mut(b).unwrap().tabs[1].items.push(item("C:/Desktop/A.lnk"));
        assert!(plan_native_pin_transfer(&state,&native_items,
            a,from,"C:/Desktop/A.lnk",b,to,4,3).is_err());
        assert_eq!(state.group(a).unwrap().tabs[0].items.len(),1);
    }

    #[test]
    fn native_drop_hit_targets_only_unique_box_body_and_valid_source() {
        let mut state=State::empty();
        let a=state.add_group("A",Bounds{x:100.,y:50.,w:300.,h:260.});
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(a).unwrap();
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(a).unwrap().active=0;
        let b=state.add_group("B",Bounds{x:500.,y:100.,w:300.,h:260.});
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.add_tab(b).unwrap();
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/D.lnk"));
        state.group_mut(b).unwrap().active=0;
        let native_items=["A","B","C","D","Loose"].iter()
            .map(|v|native(&format!("C:/Desktop/{v}.lnk"))).collect::<Vec<_>>();
        let plan=native_multi_box_plan(&state,&native_items,4,3).unwrap();
        assert_eq!(native_drop_target_for_screen_point(&state,&plan,a,600,180).unwrap().0,b);
        assert!(native_drop_target_for_screen_point(&state,&plan,a,600,120).is_err()); // header
        assert!(native_drop_target_for_screen_point(&state,&plan,a,200,200).is_err()); // same box
        assert!(native_drop_target_for_screen_point(&state,&plan,a,1500,180).is_err());
        let (next,_,gid,_)=plan_native_drag_release(&state,&native_items,&plan,0,600,180,4,3).unwrap();
        assert_eq!(gid,b);
        assert!(next.group(a).unwrap().tabs[0].items.is_empty());
        assert_eq!(next.group(b).unwrap().tabs[0].items.len(),2);
        assert_eq!(state.group(a).unwrap().tabs[0].items.len(),1); // no mutation
        assert!(plan_native_drag_release(&state,&native_items,&plan,4,600,180,4,3).is_err()); // loose
        assert!(plan_native_drag_release(&state,&native_items,&plan,1,600,180,4,3).is_err()); // hidden Tab
        assert!(plan_native_drag_release(&state,&native_items,&plan,999,600,180,4,3).is_err());
        let both=state.add_group("C",Bounds{x:500.,y:100.,w:300.,h:260.});
        state.group_mut(both).unwrap().tab_mut().items.push(item("C:/Desktop/E.lnk"));
        state.add_tab(both).unwrap();
        state.group_mut(both).unwrap().tab_mut().items.push(item("C:/Desktop/F.lnk"));
        state.group_mut(both).unwrap().active=0;
        let extended=["A","B","C","D","E","F"].iter().map(|v|
            native(&format!("C:/Desktop/{v}.lnk"))).collect::<Vec<_>>();
        let ambiguous=native_multi_box_plan(&state,&extended,4,3).unwrap();
        assert!(native_drop_target_for_screen_point(&state,&ambiguous,a,600,180).is_err());
    }

    #[test]
    fn native_ole_single_item_drop_is_model_only_and_fail_closed() {
        let mut state=State::empty();
        let a=state.add_group("A",Bounds{x:100.,y:50.,w:320.,h:260.});
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(a).unwrap();
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(a).unwrap().active=0;
        let b=state.add_group("B",Bounds{x:500.,y:50.,w:320.,h:260.});
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.add_tab(b).unwrap();
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/D.lnk"));
        state.group_mut(b).unwrap().active=0;
        let target_tab=state.group(b).unwrap().tabs[0].id;
        let hidden_tab=state.group(b).unwrap().tabs[1].id;
        let explorer=["A","B","C","D","Loose"].iter().map(|v|
            native(&format!("C:/Desktop/{v}.lnk"))).collect::<Vec<_>>();
        let before=native_multi_box_plan(&state,&explorer,4,3).unwrap();
        let path=std::path::PathBuf::from("C:/Desktop/A.lnk");
        let (after,policy)=plan_native_ole_drop(&state,&explorer,&before,
            &[path.clone()],b,target_tab,4,3).unwrap();
        assert_eq!(state.group(a).unwrap().tabs[0].items.len(),1);
        assert!(after.group(a).unwrap().tabs[0].items.is_empty());
        assert_eq!(after.group(b).unwrap().tabs[0].items.len(),2);
        assert_eq!(policy.hidden_for(&[]).unwrap(),vec![1,3]);
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[],b,target_tab,4,3).is_err());
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[path.clone(),path.clone()],b,target_tab,4,3).is_err());
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[std::path::PathBuf::from("C:/Desktop/Loose.lnk")],b,target_tab,4,3).is_err());
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[std::path::PathBuf::from("C:/Desktop/B.lnk")],b,target_tab,4,3).is_err());
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[path.clone()],b,hidden_tab,4,3).is_err());
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[path.clone()],a,state.group(a).unwrap().tabs[0].id,4,3).is_err());
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[std::path::PathBuf::from("C:/Desktop/Outside.lnk")],b,target_tab,4,3).is_err());
        let stale_plan=before.clone();
        let mut changed=state.clone();
        changed.group_mut(b).unwrap().active=1;
        assert!(plan_native_ole_drop(&changed,&explorer,&stale_plan,
            &[path.clone()],b,target_tab,4,3).is_err());
        assert!(plan_native_ole_drop(&state,&explorer,&before,
            &[path],b,target_tab,4,1).is_err());
        assert_eq!(state.group(b).unwrap().tabs[0].items.len(),1);
    }

    #[test]
    fn physical_native_multiselect_uses_two_verified_indices_or_rejects_whole_drag() {
        let mut state=State::empty();
        let a=state.add_group("Craft",Bounds{x:100.,y:80.,w:320.,h:260.});
        state.group_mut(a).unwrap().tabs[0].items.push(item("C:/Desktop/ExampleApp.lnk"));
        state.group_mut(a).unwrap().tabs[0].items.push(item("C:/Desktop/ExampleApp Launcher.lnk"));
        state.add_tab(a).unwrap();
        state.group_mut(a).unwrap().active=0;
        let b=state.add_group("Work",Bounds{x:460.,y:80.,w:320.,h:260.});
        state.group_mut(b).unwrap().tabs[0].items.push(item("C:/Desktop/RustDesk.lnk"));
        state.group_mut(b).unwrap().tabs[0].items.push(item("C:/Desktop/AnyDesk.lnk"));
        state.add_tab(b).unwrap();
        state.group_mut(b).unwrap().active=0;
        let icons=["ExampleApp","ExampleApp Launcher","RustDesk","AnyDesk"].iter()
            .map(|n|native(&format!("C:/Desktop/{n}.lnk")))
            .collect::<Vec<_>>();
        let policy=native_multi_box_plan(&state,&icons,2,4).unwrap();
        let (next,after,group,tab)=plan_native_selected_drag_release(
            &state,&icons,&policy,&[0,1],620,210,2,4).unwrap();
        assert_eq!(group,b);
        assert_eq!(tab,state.group(b).unwrap().tab().id);
        assert_eq!(next.group(a).unwrap().tab().items.len(),0);
        assert_eq!(next.group(b).unwrap().tab().items.len(),4);
        assert_eq!(next.group(b).unwrap().tab().items[2].path,"C:/Desktop/ExampleApp.lnk");
        assert_eq!(next.group(b).unwrap().tab().items[3].path,"C:/Desktop/ExampleApp Launcher.lnk");
        assert_eq!(after.groups.len(),2);
        assert_eq!(state.group(a).unwrap().tab().items.len(),2);
        for invalid in [&[0usize,2][..],&[0,0][..],&[1,99][..],&[][..]] {
            assert!(plan_native_selected_drag_release(
                &state,&icons,&policy,invalid,620,210,2,4).is_err());
        }
        assert!(plan_native_selected_drag_release(
            &state,&icons,&policy,&[0,1],620,210,2,3).is_err(),
            "Destination cannot silently lose an item when capacity is exceeded");
        assert_eq!(state.group(b).unwrap().tab().items.len(),2);
    }

    #[test]
    fn native_pin_drop_updates_only_a_cloned_group_model() {
        let mut state=State::empty();
        let a=state.add_group("A",box_bounds());
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(a).unwrap();
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(a).unwrap().active=0;
        let b=state.add_group("B",box_bounds());
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.add_tab(b).unwrap();
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/D.lnk"));
        state.group_mut(b).unwrap().active=0;
        let from=state.group(a).unwrap().tabs[0].id;
        let to=state.group(b).unwrap().tabs[0].id;
        let explorer=["A","B","C","D"].iter().map(|v|
            native(&format!("C:/Desktop/{v}.lnk"))).collect::<Vec<_>>();
        let (updated,plan)=plan_native_pin_transfer(&state,&explorer,
            a,from,"C:/Desktop/A.lnk",b,to,4,3).unwrap();
        assert_eq!(state.group(a).unwrap().tabs[0].items.len(),1);
        assert!(updated.group(a).unwrap().tabs[0].items.is_empty());
        assert_eq!(updated.group(b).unwrap().tabs[0].items.len(),2);
        assert_eq!(plan.hidden_for(&[]).unwrap(),vec![1,3]);
        assert!(plan_native_pin_transfer(&state,&explorer,
            a,from,"C:/Desktop/Missing.lnk",b,to,4,3).is_err());
        assert!(plan_native_pin_transfer(&state,&explorer,
            a,from,"C:/Desktop/A.lnk",b,to,4,1).is_err());
    }

    #[test]
    fn multi_box_visibility_is_composable_and_never_hides_loose_items() {
        let mut state=State::empty();
        let a=state.add_group("A",box_bounds());
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(a).unwrap();
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(a).unwrap().active=0;
        let b=state.add_group("B",box_bounds());
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.add_tab(b).unwrap();
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/D.lnk"));
        state.group_mut(b).unwrap().active=0;
        let native_items=["A","B","C","D","Loose"].iter().map(|n|
            native(&format!("C:/Desktop/{n}.lnk"))).collect::<Vec<_>>();
        let plan=native_multi_box_plan(&state,&native_items,4,4).unwrap();
        assert_eq!(plan.groups.len(),2);
        assert_eq!(plan.unclaimed_native_items,1);
        assert_eq!(plan.hidden_for(&[]).unwrap(),vec![1,3]);
        assert_eq!(plan.hidden_for(&[(a,1)]).unwrap(),vec![0,3]);
        assert_eq!(plan.hidden_for(&[(b,1)]).unwrap(),vec![1,2]);
        assert_eq!(plan.hidden_for(&[(a,1),(b,1)]).unwrap(),vec![0,2]);
        assert!(plan.hidden_for(&[(a,2)]).is_err());
        assert!(plan.hidden_for(&[(a,1),(a,0)]).is_err());
        assert!(plan.hidden_for(&[(999999,0)]).is_err());
        assert!(native_multi_box_plan(&state,&native_items,1,4).is_err());
    }

    #[test]
    fn multi_box_refuses_cross_group_duplicate_even_when_other_box_skipped() {
        let mut state=State::empty();
        let a=state.add_group("Native",box_bounds());
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(a).unwrap();
        state.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(a).unwrap().active=0;
        let b=state.add_group("Unsupported",box_bounds());
        state.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.add_tab(b).unwrap();
        state.group_mut(b).unwrap().tab_mut().folder=Some("D:/External".into());
        let actual=[native("C:/Desktop/A.lnk"),native("C:/Desktop/B.lnk")];
        assert!(native_multi_box_plan(&state,&actual,4,4).is_err());
        state.group_mut(b).unwrap().tabs[0].items.clear();
        let plan=native_multi_box_plan(&state,&actual,4,4).unwrap();
        assert_eq!(plan.groups.len(),1);
        assert_eq!(plan.skipped.len(),1);
        assert_eq!(plan.hidden_for(&[]).unwrap(),vec![1]);
    }

    #[test]
    fn multi_box_skips_rollup_single_tab_portal_and_external_pins() {
        let mut state=State::empty();
        let active=state.add_group("Supported",box_bounds());
        state.group_mut(active).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(active).unwrap();
        state.group_mut(active).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(active).unwrap().active=0;
        let rolled=state.add_group("Rolled",box_bounds());
        state.group_mut(rolled).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.group_mut(rolled).unwrap().rolled=true;
        let portal=state.add_group("Portal",box_bounds());
        state.group_mut(portal).unwrap().tab_mut().items.push(item("C:/Desktop/D.lnk"));
        state.add_tab(portal).unwrap();
        state.group_mut(portal).unwrap().tab_mut().folder=Some("D:/Portal".into());
        let actual=["A","B","C","D","Loose"].iter().map(|n|
            native(&format!("C:/Desktop/{n}.lnk"))).collect::<Vec<_>>();
        let plan=native_multi_box_plan(&state,&actual,4,4).unwrap();
        assert_eq!(plan.groups.len(),1);
        assert_eq!(plan.skipped.len(),2);
        assert_eq!(plan.hidden_for(&[]).unwrap(),vec![1]);
        assert!(plan.hidden_for(&[(portal,1)]).is_err());
        assert!(native_multi_box_plan(&state,&actual,0,4).is_err());
        assert!(native_multi_box_plan(&state,&actual,4,0).is_err());
    }

    #[test]
    fn general_group_plan_supports_three_tabs_and_scoped_native_visibility() {
        let mut state=State::empty();
        let g=state.add_group("Tools",box_bounds());
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(g).unwrap();
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.add_tab(g).unwrap();
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.group_mut(g).unwrap().active=0;
        let actual=["A","B","C","Loose"].iter().map(|n|
            native(&format!("C:/Desktop/{n}.lnk"))).collect::<Vec<_>>();
        let plan=native_group_tab_plan(&state,&actual,g,3).unwrap();
        assert_eq!(plan.native_indices,vec![vec![0],vec![1],vec![2]]);
        assert_eq!(plan.hidden_for(1).unwrap(),vec![0,2]);
        assert_eq!(plan.hidden_for(0).unwrap(),vec![1,2]);
        assert!(plan.hidden_for(4).is_err());
        assert_eq!(plan.event(2).unwrap(),Some(NativeTabChange{group_id:g,from:0,to:2}));
        assert_eq!(plan.event(0).unwrap(),None);
        let original=state.clone();
        state.group_mut(g).unwrap().active=2;
        assert_eq!(native_tab_changes(&original,&state),
            vec![NativeTabChange{group_id:g,from:0,to:2}]);
    }

    #[test]
    fn general_group_plan_refuses_duplicate_identity_or_portal() {
        let mut state=State::empty();
        let g=state.add_group("Tools",box_bounds());
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(g).unwrap();
        state.group_mut(g).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.group_mut(g).unwrap().active=0;
        let actual=[native("C:/Desktop/A.lnk"),native("C:/Desktop/B.lnk")];
        assert!(native_group_tab_plan(&state,&actual,g,4).is_err());
        state.group_mut(g).unwrap().tabs[1].items.clear();
        state.group_mut(g).unwrap().tabs[1].folder=Some("D:/Folder".into());
        let portal_safe=native_group_tab_plan(&state,&actual,g,4).unwrap();
        assert_eq!(portal_safe.portal_tabs,vec![1]);
        assert_eq!(portal_safe.native_indices,vec![vec![0],vec![]]);
        assert!(portal_safe.hidden_for(1).is_err(),
            "Portal tabs may NEVER be selected for native Desktop hiding");
        assert!(portal_safe.event(1).is_err());
        state.group_mut(g).unwrap().active=1;
        assert!(native_group_tab_plan(&state,&actual,g,4).is_err(),
            "Active Portal requires an independent Shell View");
        state.group_mut(g).unwrap().active=0;
        state.group_mut(g).unwrap().tabs[1].folder=None;
        state.group_mut(g).unwrap().tabs[1].items.push(item("C:/Desktop/B.lnk"));
        let previous=state.clone();
        state.group_mut(g).unwrap().active=1;
        assert_eq!(native_tab_changes(&previous,&state).len(),1);
        state.group_mut(g).unwrap().page=1;
        assert!(native_tab_changes(&previous,&state).is_empty());
    }

    #[test]
    fn inactive_folder_portal_cannot_block_atomic_native_regular_tab_drop() {
        let mut state=State::empty();
        let source=state.add_group("Native Source",Bounds{x:100.,y:60.,w:320.,h:260.});
        state.group_mut(source).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(source).unwrap();
        state.group_mut(source).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        state.group_mut(source).unwrap().active=0;
        let target=state.add_group("Native Destination",Bounds{x:500.,y:60.,w:320.,h:260.});
        state.group_mut(target).unwrap().tab_mut().items.push(item("C:/Desktop/C.lnk"));
        state.add_tab(target).unwrap();
        state.group_mut(target).unwrap().tab_mut().items.push(item("C:/Desktop/D.lnk"));
        state.add_tab(target).unwrap();
        state.group_mut(target).unwrap().tabs[2].folder=Some("D:/Projects".into());
        state.group_mut(target).unwrap().active=0;
        let native_items=["A","B","C","D","Loose"].iter()
            .map(|name|native(&format!("C:/Desktop/{name}.lnk")))
            .collect::<Vec<_>>();
        let baseline=serde_json::to_vec(&state).unwrap();
        let checked=native_multi_box_plan(&state,&native_items,4,4).unwrap();
        assert_eq!(checked.groups.len(),2);
        assert_eq!(checked.groups[1].portal_tabs,vec![2]);
        assert!(checked.hidden_for(&[(target,2)]).is_err());
        let destination_tab=state.group(target).unwrap().tabs[0].id;
        let (after,policy)=plan_native_ole_drop(
            &state,&native_items,&checked,
            &[std::path::PathBuf::from("C:/Desktop/A.lnk")],
            target,destination_tab,4,4).unwrap();
        assert_eq!(after.group(source).unwrap().tabs[0].items.len(),0);
        assert_eq!(after.group(target).unwrap().tabs[0].items.len(),2);
        assert_eq!(after.group(target).unwrap().tabs[2].folder.as_deref(),
            Some("D:/Projects"));
        assert_eq!(policy.groups[1].portal_tabs,vec![2]);
        assert_eq!(serde_json::to_vec(&state).unwrap(),baseline);
        let mut active_portal=state.clone();
        active_portal.group_mut(target).unwrap().active=2;
        assert!(plan_native_ole_drop(
            &active_portal,&native_items,&checked,
            &[std::path::PathBuf::from("C:/Desktop/A.lnk")],
            target,destination_tab,4,4).is_err(),
            "OLE Drop into a Folder Portal is NEVER treated as virtual Pin");
    }

    #[test]
    fn tab_pair_rejects_rolled_group_and_missing_target() {
        let mut state=State::empty();
        let group=state.add_group("Tools",box_bounds());
        state.group_mut(group).unwrap().tab_mut().items.push(item("C:/Desktop/A.lnk"));
        state.add_tab(group).unwrap();
        state.group_mut(group).unwrap().tab_mut().items.push(item("C:/Desktop/B.lnk"));
        let real=[native("C:/Desktop/A.lnk"),native("C:/Desktop/B.lnk")];
        assert!(choose_native_tab_pair(&state,&real,group,Some(4)).is_err());
        state.group_mut(group).unwrap().rolled=true;
        assert!(choose_native_tab_pair(&state,&real,group,Some(1)).is_err());
    }

    #[test]
    fn index_alignment_requires_matching_order_and_unique_names() {
        let real = vec![native("A"), native("B")];
        let good = audit_native_indices(&real, &["A".into(), "B".into()]);
        assert!(good.safe_to_address_by_index);
        assert_eq!(good.same_index_labels, 2);
        let reversed = audit_native_indices(&real, &["B".into(), "A".into()]);
        assert!(!reversed.safe_to_address_by_index);
        assert!(reversed.safe_to_reconcile_by_name);
        assert_eq!(reversed.reconciled_names,2);
        let mapped=native_view_in_msaa_order(&real,&["B".into(),"A".into()]).unwrap();
        assert_eq!(mapped[0].path,real[1].path);
        assert_eq!(mapped[1].path,real[0].path);
        assert_eq!(reversed.same_index_labels, 0);
        let partial = audit_native_indices(&real, &["A".into()]);
        assert!(!partial.safe_to_address_by_index);
        assert_eq!(partial.accessibility_count, 1);
        assert!(!partial.safe_to_reconcile_by_name);
        assert!(native_view_in_msaa_order(&real,&["A".into(),"C".into()]).is_err());
    }

    #[test]
    fn duplicate_label_and_empty_name_fail_closed() {
        let actual = vec![native("same"), native("same")];
        let duplicate = audit_native_indices(&actual, &["same".into(), "same".into()]);
        assert!(!duplicate.safe_to_address_by_index);
        assert_eq!(duplicate.ambiguous_accessibility_names, 1);
        assert!(!duplicate.safe_to_reconcile_by_name);
        assert!(native_view_in_msaa_order(&actual,&["same".into(),"same".into()]).is_err());
        let blank = audit_native_indices(&[native("")], &["".into()]);
        assert_eq!(blank.empty_names, 2);
        assert!(!blank.safe_to_address_by_index);
    }

    #[test]
    fn missing_desktop_executable_keeps_its_pin_and_layout_uses_the_remaining_icons() {
        let desktop = std::env::temp_dir().join(format!("DesktopCraft-plan-{}", std::process::id()));
        let folders = OnceCell::from(vec![desktop.clone()]);
        let missing = desktop.join("desktopcraft.exe");
        let missing_path = missing.to_str().unwrap();
        let remaining = desktop.join("Tool.lnk");
        let remaining_path = remaining.to_str().unwrap();
        let mut state = State::empty();
        let id = state.add_group("Programs", box_bounds());
        let group = state.group_mut(id).unwrap();
        group.tab_mut().items.extend([item(missing_path), item(remaining_path)]);
        let saved = serde_json::to_value(&state).unwrap();

        let icons = [native(remaining_path)];
        let plan = group_tab_plan(state.group(id).unwrap(), 0, &icons, &native_path_index(&icons),
            128, Some(&folders)).unwrap();
        assert_eq!(plan.native_indices, vec![vec![0]]);
        assert_eq!(serde_json::to_value(&state).unwrap(), saved);

        let restored = [native(remaining_path), native(missing_path)];
        let plan = group_tab_plan(state.group(id).unwrap(), 0, &restored, &native_path_index(&restored),
            128, Some(&folders)).unwrap();
        assert_eq!(plan.native_indices, vec![vec![1, 0]]);

        let nested = desktop.join("subfolder").join("desktopcraft.exe");
        assert!(!confirmed_missing_pin(nested.to_str().unwrap(), &folders));
    }

    #[test]
    fn native_workspace_plan_allows_all_boxes_rolled_up_or_on_other_page() {
        let mut s = State::empty();
        let a = s.add_group("One", box_bounds());
        let b = s.add_group("Two", Bounds { x: 340., y: 0., w: 300., h: 220. });
        let c = s.add_group("Three", Bounds { x: 680., y: 0., w: 300., h: 220. });
        s.group_mut(a).unwrap().tab_mut().items.push(item("C:/Desktop/One.lnk"));
        s.group_mut(b).unwrap().tab_mut().items.push(item("C:/Desktop/Two.lnk"));
        s.group_mut(c).unwrap().tab_mut().items.push(item("C:/Desktop/Three.lnk"));
        s.group_mut(a).unwrap().rolled = true;
        s.group_mut(b).unwrap().rolled = true;
        s.group_mut(c).unwrap().rolled = true;
        let icons = vec![
            native("C:/Desktop/One.lnk"),
            native("C:/Desktop/Two.lnk"),
            native("C:/Desktop/Three.lnk"),
            native("C:/Desktop/Loose.lnk"),
        ];
        let plan = native_workspace_plan(&s, &icons, 16, 128).unwrap();
        assert!(plan.groups.is_empty());
        assert_eq!(plan.skipped.len(), 3);
        assert_eq!(plan.unclaimed_native_items, 1);
        assert_eq!(plan.hidden_for(&[]).unwrap(), Vec::<usize>::new());

        // A workspace containing only Folder Portals still refuses native desktop layout.
        let mut portal_only = State::empty();
        let p = portal_only.add_group("Cloud", box_bounds());
        portal_only.group_mut(p).unwrap().tab_mut().folder = Some("D:/Cloud".into());
        assert!(native_workspace_plan(&portal_only, &icons, 16, 128).is_err());
    }
}
