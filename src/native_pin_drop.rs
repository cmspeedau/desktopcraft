//! Actual Explorer-native icon drag -> safe virtual Pin membership planning.
//!
//! This module deliberately does NOT implement an Explorer hook, move a file,
//! alter an IFolderView icon position, or write the user Workspace. A caller
//! must supply an independently verified, one-icon physical Explorer gesture.
//! The planning output is usable only after the native worker revalidates its
//! live Explorer identity/order and ACKs a new layout generation.
use crate::explorer_native::{native_workspace_plan,user_native_workspace_plan,NativeMultiBoxPlan};
use crate::model::{Item,State};
use crate::shell::ViewItem;
use std::collections::{HashMap,HashSet};
use std::path::Path;

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum NativeDropKind {
    BoxToBox { from:u64,from_tab:u64,to:u64,to_tab:u64,pin_id:u64 },
    DesktopToBox { to:u64,to_tab:u64,pin_id:u64 },
    BoxToDesktop { from:u64,from_tab:u64,pin_id:u64 },
    SameBox { group:u64 },
}

pub const MAX_MULTI_DRAG_ICONS: usize = 8;
pub(crate) const LOOSE_DESKTOP_DROP: &str = "Loose Desktop icons did not enter a native Box";

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum NativeMultiDropKind {
    BoxToBox { from:u64,from_tab:u64,to:u64,to_tab:u64,pin_ids:Vec<u64> },
    DesktopToBox { to:u64,to_tab:u64,pin_ids:Vec<u64> },
    BoxToDesktop { from:u64,from_tab:u64,pin_ids:Vec<u64> },
    SameBox { group:u64 },
    SortRules,
    RestoreLayout,
}

pub struct NativeMultiDropPrepared {
    pub next:State,
    pub native_policy:NativeMultiBoxPlan,
    pub kind:NativeMultiDropKind,
    pub source_paths:Vec<String>,
}

pub(crate) type NativeDropSlot = Result<Option<(u64, u64, usize)>, String>;

fn reorder_pins(items: &mut Vec<Item>, selected: &HashSet<u64>, slot: usize) {
    let index = items[..slot.min(items.len())].iter()
        .filter(|item| !selected.contains(&item.id)).count();
    let (moving, remaining): (Vec<_>, Vec<_>) = std::mem::take(items).into_iter()
        .partition(|item| selected.contains(&item.id));
    *items = remaining;
    items.splice(index..index, moving);
}

/// Speculation only. Actual Shell movement must still produce the same
/// next state before a prepared placement can be consumed.
pub(crate) fn predict_same_box_reorder(
    state: &State,
    (group_id, tab_id, slot): (u64, u64, usize),
    selected: &[String],
    grabbed: &str,
    eligible: &HashSet<String>,
) -> Option<State> {
    let group = state.group(group_id)?;
    let tab = group.tabs.get(group.active)?;
    if group.rolled || group.page != state.page || tab.id != tab_id || tab.folder.is_some() {
        return None;
    }
    let paths: HashSet<_> = selected.iter().map(|p| p.to_lowercase()).collect();
    if !paths.contains(&grabbed.to_lowercase()) || !paths.is_subset(eligible)
        || paths.len() != selected.len()
    {
        return None;
    }
    let pins: HashSet<_> = tab.items.iter().filter(|pin| paths.contains(&pin.path.to_lowercase()))
        .map(|pin| pin.id).collect();
    if pins.len() != paths.len() {
        return None;
    }
    let mut next = state.clone();
    reorder_pins(&mut next.group_mut(group_id)?.tab_mut().items, &pins, slot);
    Some(next)
}

pub(crate) fn native_drop_slot(
    state: &State,
    plan: &NativeMultiBoxPlan,
    before: &[ViewItem],
    eligible_paths: &HashSet<String>,
    point: (i32, i32),
    spacing: (i32, i32),
    coordinates: crate::native_box_live_layout::DesktopCoordinates,
    monitors: &[crate::host::Monitor],
) -> NativeDropSlot {
    let Some((id, tab)) = eligible_box_target_at(state, plan, point, coordinates, 8.)? else {
        return Ok(None);
    };
    let group = state.group(id).ok_or("Destination Box disappeared")?;
    let items = &group.tabs.iter().find(|target| target.id == tab)
        .ok_or("Destination Tab disappeared")?.items;
    let (x, y) = coordinates.workspace_point(point);
    if native_tab_header_at(state, (x, y)) == Some((id, tab)) {
        return Ok(Some((id, tab, items.len())));
    }
    // The first visible Pin gives both the Explorer anchor and the offset
    // of the current scroll row in the full Tab.
    let first_visible = items.iter().enumerate().find_map(|(index, item)| {
        let key = item.path.to_lowercase();
        if !eligible_paths.contains(&key) {
            return None;
        }
        before.iter().find(|icon| icon.path.to_lowercase() == key)
            .map(|icon| (index, icon.pos))
    });
    let Some((first, anchor)) = first_visible else { return Ok(Some((id, tab, items.len()))); };
    let cell = crate::native_box_live_layout::cell_for_point(anchor, spacing, monitors);
    let bounds = coordinates.screen_rect(crate::paint::native_group_body(group));
    let (_, cols, _) = crate::native_box_placement::cell_layout(bounds, cell, None)?;
    let row = (point.1 - anchor.1).max(0) / cell.1;
    let col = ((point.0 - anchor.0 + cell.0 / 2).max(0) / cell.0) as usize;
    let slot = (first + row as usize * cols + col.min(cols)).min(items.len());
    Ok(Some((id, tab, slot)))
}

#[derive(Debug,Clone)]
pub struct NativePhysicalDrop {
    pub source_path:String,
    pub source_native_index:usize,
    pub mouse_down:(i32,i32),
    pub mouse_up:(i32,i32),
    /// Explorer's actual icon positions BEFORE and AFTER the physical
    /// mouse gesture; independent of DesktopCraft's virtual Pin model.
    pub before:Vec<ViewItem>,
    pub after:Vec<ViewItem>,
    pub icon_spacing:(i32,i32),
}

pub struct NativeDropPrepared {
    pub next:State,
    pub native_policy:NativeMultiBoxPlan,
    pub kind:NativeDropKind,
    pub source_path:String,
}

fn in_icon(point:(i32,i32),position:(i32,i32),spacing:(i32,i32))->bool{
    point.0>=position.0 && point.0<position.0.saturating_add(spacing.0)
        && point.1>=position.1 && point.1<position.1.saturating_add(spacing.1)
}
fn in_icon_margin(point:(i32,i32),position:(i32,i32),spacing:(i32,i32),margin:i32)->bool{
    point.0>=position.0.saturating_sub(margin)
        && point.0<position.0.saturating_add(spacing.0).saturating_add(margin)
        && point.1>=position.1.saturating_sub(margin)
        && point.1<position.1.saturating_add(spacing.1).saturating_add(margin)
}
fn hit_icon_index<'a>(
    pointer: (i32, i32),
    expected: &'a [ViewItem],
    eligible: &HashSet<String>,
    cell_for: &impl Fn((i32, i32)) -> (i32, i32),
) -> Option<(usize, &'a ViewItem)> {
    let mut exact = expected.iter().enumerate()
        .filter(|(_, item)| in_icon(pointer, item.pos, cell_for(item.pos))
            && eligible.contains(&item.path.to_lowercase()));
    if let Some((index, item)) = exact.next() {
        return exact.next().is_none().then_some((index, item));
    }
    let mut padded = expected.iter().enumerate()
        .filter(|(_, item)| in_icon_margin(pointer, item.pos, cell_for(item.pos), 6)
            && eligible.contains(&item.path.to_lowercase()));
    let (index, item) = padded.next()?;
    padded.next().is_none().then_some((index, item))
}
fn in_box(point:(f32,f32),state:&State)->Vec<u64>{
    state.groups.iter().filter(|group|group.page==state.page && {
        let b=group.bounds;
        b.x.is_finite() && b.y.is_finite() && b.w.is_finite() && b.h.is_finite()
            && point.0>=b.x && point.0<b.x+b.w
            && point.1>=b.y && point.1<b.y+b.h
    }).map(|g|g.id).collect()
}

/// Resolve ambiguous stacked Box hit-testing when a rolled-up Box sits over an
/// expanded Box (or vice versa during native hover-reveal): if exactly one of
/// the containing Boxes is active/expanded, it owns that screen footprint.
fn resolve_drop_box(containing: &[u64], state: &State) -> Result<Option<u64>, String> {
    if containing.len() <= 1 {
        return Ok(containing.first().copied());
    }
    let expanded: Vec<u64> = containing
        .iter()
        .copied()
        .filter(|id| state.group(*id).is_some_and(|g| !g.rolled))
        .collect();
    match expanded.as_slice() {
        [single] => Ok(Some(*single)),
        _ => Err("Native drop overlaps more than one Box".into()),
    }
}

fn native_header_box_at(state: &State, point: (f32, f32)) -> Option<&crate::model::Group> {
    state.groups.iter().rev().filter(|group| group.page == state.page).find(|group| {
        let header = crate::paint::native_group_header(group);
        header.contains(point.0, point.1)
    })
}

pub(crate) fn native_tab_header_at(state: &State, point: (f32, f32)) -> Option<(u64, u64)> {
    let group = native_header_box_at(state, point)?;
    let index = crate::paint::native_header_tab_at(group, point).unwrap_or(group.active);
    let tab = group.tabs.get(index)?;
    tab.folder.is_none().then_some((group.id, tab.id))
}

pub(crate) fn visible_regular_pin_owner(state: &State, path: &str) -> Option<(u64, u64, u64)> {
    state.groups.iter().find_map(|group| {
        let tab = group.tabs.get(group.active)?;
        if group.page != state.page || group.rolled || tab.folder.is_some() {
            return None;
        }
        let pin = tab.items.iter().find(|pin| pin.path.eq_ignore_ascii_case(path))?;
        Some((group.id, tab.id, pin.id))
    })
}

fn eligible_box_target_at(
    state: &State,
    plan: &NativeMultiBoxPlan,
    point: (i32, i32),
    coordinates: crate::native_box_live_layout::DesktopCoordinates,
    top_inset: f32,
) -> Result<Option<(u64, u64)>, String> {
    let (x, y) = coordinates.workspace_point(point);
    if let Some(target) = native_tab_header_at(state, (x, y)) {
        return Ok(Some(target));
    }
    if native_header_box_at(state, (x, y)).is_some() {
        return Err("Native drag landed outside a regular Tab on the Box header".into());
    }
    let containing = in_box((x, y), state);
    let Some(id) = resolve_drop_box(&containing, state)? else {
        return Ok(None);
    };
    let group = state.group(id).ok_or("Native destination Box disappeared")?;
    if group.rolled
        || group.active >= group.tabs.len()
        || group.tabs[group.active].folder.is_some()
        || !plan.groups.iter().any(|p| p.group_id == id)
    {
        return Err("Dragged icon ended on a rolled/Portal/ineligible native Box".into());
    }
    let b = crate::paint::native_group_body(group);
    if x < b.x + 8. || x >= b.x + b.w - 8. || y < b.y + top_inset || y >= b.y + b.h - 8. {
        return Err("Native drag landed on Box header, edge or resize handle".into());
    }
    Ok(Some((group.id, group.tabs[group.active].id)))
}

fn append_new_loose_pins_to_tab(
    next: &mut State,
    to: u64,
    to_tab: u64,
    source_paths: &[String],
    insert_at: Option<usize>,
) -> Result<Vec<u64>, String> {
    let dest = next.group(to).ok_or("Destination Box disappeared")?;
    let current_len = dest.tabs.iter()
        .find(|t| t.id == to_tab)
        .ok_or("Destination Tab disappeared")?
        .items.len();
    if current_len + source_paths.len() > crate::native_filter_protocol::MAX_ITEMS {
        return Err("Destination Tab exceeds the number of icons supported by the Explorer policy".into());
    }
    let mut new_pins = Vec::with_capacity(source_paths.len());
    for path in source_paths {
        let pin_id = next.id();
        let pin = Item::from_path(Path::new(path), pin_id)
            .map_err(|e| format!("Cannot pin real Desktop item: {e}"))?;
        new_pins.push(pin);
    }
    let pin_ids = new_pins.iter().map(|p| p.id).collect::<Vec<_>>();
    let items = &mut next.group_mut(to).ok_or("Target disappeared")?
        .tabs.iter_mut().find(|t| t.id == to_tab)
        .ok_or("Target Tab disappeared")?.items;
    let index = insert_at.unwrap_or(items.len()).min(items.len());
    items.splice(index..index, new_pins);
    Ok(pin_ids)
}

/// Accept only an actual drag that moved EXACTLY ONE Windows Explorer icon,
/// after the left mouse button was released and the source identity was
/// observed at the physical MouseDown. This rejects bulk Explorer
/// reflows, auto arrange, source file renames/deletes and label changes.
pub fn physical_drop_identity(gesture:&NativePhysicalDrop)->Result<(),String>{
    let spacing=gesture.icon_spacing;
    if spacing.0<24 || spacing.1<24 || spacing.0>512 || spacing.1>512 {
        return Err("Explorer icon spacing outside verified bounds".into());
    }
    if gesture.before.len()<1 || gesture.before.len()>2048
        || gesture.before.len()!=gesture.after.len() {
        return Err("Native drag changed Explorer icon inventory".into());
    }
    let begin=gesture.mouse_down;
    let end=gesture.mouse_up;
    let dx=i64::from(end.0)-i64::from(begin.0);
    let dy=i64::from(end.1)-i64::from(begin.1);
    if dx*dx+dy*dy<400 {
        return Err("Native gesture did not exceed the physical drag threshold".into());
    }
    let mut original=HashMap::new();
    for (index,item) in gesture.before.iter().enumerate(){
        if item.path.trim().is_empty() ||
            original.insert(item.path.to_lowercase(),(index,item)).is_some(){
            return Err("Native drag source identities are not unique".into());
        }
    }
    let item=gesture.before.get(gesture.source_native_index)
        .ok_or("Native gesture source index outside actual Explorer item order")?;
    if !item.path.eq_ignore_ascii_case(&gesture.source_path)
        || !in_icon(begin,item.pos,spacing) {
        return Err("Native physical MouseDown did not hit the verified source icon".into());
    }
    let mut moved=None;
    let mut seen=HashSet::new();
    for item in &gesture.after {
        let key=item.path.to_lowercase();
        let (_,before)=original.get(&key)
            .ok_or("Explorer added or renamed an icon during native drag")?;
        if !seen.insert(key) || item.label!=before.label{
            return Err("Explorer icon label/order identities changed during drag".into());
        }
        if item.pos!=before.pos {
            if moved.replace(item.path.clone()).is_some(){
                return Err("Several Explorer icons moved; abort native virtual drop".into());
            }
            // Explorer can snap the icon to its own physical grid. Its final
            // top-left must nevertheless be within one cell of the release.
            let ux=i64::from(end.0)-i64::from(item.pos.0);
            let uy=i64::from(end.1)-i64::from(item.pos.1);
            if ux.abs()>i64::from(spacing.0) || uy.abs()>i64::from(spacing.1){
                return Err("Moved native icon landed outside the verified pointer cell".into());
            }
        }
    }
    if moved.as_deref().is_none_or(|path|!path.eq_ignore_ascii_case(&gesture.source_path)){
        return Err("The verified source icon was not the only native item moved".into());
    }
    Ok(())
}

/// Pure membership transfer for one validated Explorer icon drop. The active
/// regular Tab only, not a Folder Portal, can receive virtual membership.
/// A drop onto a header, rolled/Portal Box or overlapped region is refused:
/// it must NEVER be misinterpreted as dropping onto the loose desktop.
pub fn prepare_native_drop(
    state:&State,gesture:&NativePhysicalDrop,
    coordinates:crate::native_box_live_layout::DesktopCoordinates,
)->Result<NativeDropPrepared,String>{
    physical_drop_identity(gesture)?;
    let before_plan=native_workspace_plan(state,&gesture.before,16,128)?;
    let icon=gesture.before.get(gesture.source_native_index)
        .ok_or("Verified native icon missing")?;
    let mut current_owner=None;
    for group in &state.groups {
        for (tab_index,tab) in group.tabs.iter().enumerate(){
            if tab.folder.is_some(){continue;}
            for pin in &tab.items {
                if pin.path.eq_ignore_ascii_case(&icon.path){
                    if current_owner.replace((group.id,tab_index,tab.id,pin.id)).is_some(){
                        return Err("Native pin appears in more than one regular Tab".into());
                    }
                }
            }
        }
    }
    if let Some((group_id,tab_index,_,_))=current_owner {
        let group=state.group(group_id).ok_or("Native source Box disappeared")?;
        if group.page!=state.page || group.rolled || group.active!=tab_index
            || !before_plan.groups.iter().any(|p|p.group_id==group_id){
            return Err("Dragged source belongs to a hidden/rolled/Portal/non-active Box".into());
        }
    }
    let (x,y)=coordinates.workspace_point(gesture.mouse_up);
    let containing=in_box((x,y),state);
    let mut target=None;
    if let Some(id)=resolve_drop_box(&containing, state)? {
        let group=state.group(id).ok_or("Native destination Box disappeared")?;
        if group.rolled || group.active>=group.tabs.len()
            || group.tabs[group.active].folder.is_some() ||
            !before_plan.groups.iter().any(|p|p.group_id==id) {
            return Err("Dragged icon ended on a rolled/Portal/ineligible native Box".into());
        }
        // Keep the header/tab bar and resize handles free for real Windows
        // UI commands. A physical file icon cannot pin on the title bar.
        let b=group.bounds;
        if x<b.x+8. || x>=b.x+b.w-8.
            || y<b.y+46. || y>=b.y+b.h-8. {
            return Err("Native drag landed on Box header, edge or resize handle".into());
        }
        target=Some((group.id,group.tabs[group.active].id));
    }
    let mut next=state.clone();
    let kind=match (current_owner,target){
        (Some((from,_,src_tab,pin)),Some((to,to_tab))) if from==to=>
            NativeDropKind::SameBox{group:from},
        (Some((from,_,src_tab,pin)),Some((to,to_tab)))=>{
            let dst=next.group(to).ok_or("Destination disappeared")?
                .tabs.iter().find(|t|t.id==to_tab)
                .ok_or("Destination Tab disappeared")?;
            if dst.items.len()>=128 {return Err("Target Tab is full".into());}
            let count=dst.items.len();
            next.transfer_between_tabs(from,src_tab,pin,to,to_tab,count)?;
            NativeDropKind::BoxToBox{from,from_tab:src_tab,to,to_tab,pin_id:pin}
        }
        (Some((from,_,src_tab,pin)),None)=>{
            next.unassign(&icon.path);
            NativeDropKind::BoxToDesktop{from,from_tab:src_tab,pin_id:pin}
        }
        (None,Some((to,to_tab)))=>{
            let dest=next.group(to).ok_or("Destination Box disappeared")?;
            if dest.tab().items.len()>=128{
                return Err("Destination Tab has reached its 128-pin limit".into());
            }
            // A loose native Explorer identity is not necessarily a normal
            // filesystem file (virtual Shell items exist). Only physical
            // files for which metadata can be obtained may be pinned.
            if !Path::new(&icon.path).is_file() && !Path::new(&icon.path).is_dir(){
                return Err("Loose native source has no accessible file identity".into());
            }
            let pin_id=next.id();
            let pin=Item::from_path(Path::new(&icon.path),pin_id)
                .map_err(|e|format!("Cannot pin real Desktop item: {e}"))?;
            next.group_mut(to).ok_or("Target disappeared")?
                .tabs.iter_mut().find(|t|t.id==to_tab)
                .ok_or("Target Tab disappeared")?.items.push(pin);
            NativeDropKind::DesktopToBox{to,to_tab,pin_id}
        }
        (None,None)=>return Err("Loose Desktop icon did not enter a native Box".into()),
    };
    let after_plan=native_workspace_plan(&next,&gesture.before,16,128)?;
    if before_plan.groups.len()!=after_plan.groups.len() ||
        before_plan.groups.iter().any(|g|!after_plan.groups.iter().any(|now|now.group_id==g.group_id)){
        return Err("Native drop invalidated an active Desktop Box".into());
    }
    Ok(NativeDropPrepared{next,native_policy:after_plan,kind,
        source_path:icon.path.clone()})
}

/// Identify the moved Explorer icons with
/// identical before/after icon inventory and labels, sorted deterministically
/// by `(before_pos.1, before_pos.0, path)`. Rejects bulk Explorer reflows.
pub fn physical_multi_drop_identity(
    before: &[ViewItem],
    after: &[ViewItem],
    limit: usize,
) -> Result<Vec<String>, String> {
    if before.is_empty() || before.len() > 2048 || before.len() != after.len() {
        return Err("Native drag changed Explorer icon inventory".into());
    }
    let mut original = HashMap::new();
    for item in before {
        if item.path.trim().is_empty() || original.insert(item.path.to_lowercase(), item).is_some() {
            return Err("Native drag source identities are not unique".into());
        }
    }
    let mut moved = Vec::new();
    let mut seen = HashSet::new();
    for item in after {
        let key = item.path.to_lowercase();
        let before_item = original.get(&key)
            .ok_or("Explorer added or renamed an icon during native drag")?;
        if !seen.insert(key) || item.label != before_item.label {
            return Err("Explorer icon label/order identities changed during drag".into());
        }
        if item.pos != before_item.pos {
            moved.push((before_item.pos, before_item.path.clone()));
        }
    }
    if moved.is_empty() {
        return Err("Explorer did not move any native icon".into());
    }
    if moved.len() > limit {
        return Err(format!(
            "Too many Explorer icons moved together ({} > {}); abort native virtual drop",
            moved.len(),
            limit
        ));
    }
    moved.sort_by(|a, b| (a.0.1, a.0.0, a.1.to_lowercase()).cmp(&(b.0.1, b.0.0, b.1.to_lowercase())));
    Ok(moved.into_iter().map(|(_, path)| path).collect())
}

/// Atomic membership or order change for a verified physical Explorer drag.
#[allow(clippy::too_many_arguments)]
pub fn prepare_native_multi_drop(
    state: &State,
    before: &[ViewItem],
    after: &[ViewItem],
    eligible_paths: &HashSet<String>,
    mouse_down: (i32, i32),
    mouse_up: (i32, i32),
    icon_spacing: (i32, i32),
    coordinates: crate::native_box_live_layout::DesktopCoordinates,
) -> Result<NativeMultiDropPrepared, String> {
    prepare_native_multi_drop_with_monitors(
        state, before, after, eligible_paths, mouse_down, mouse_up, icon_spacing, coordinates, &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn prepare_native_multi_drop_with_monitors(
    state: &State,
    before: &[ViewItem],
    after: &[ViewItem],
    eligible_paths: &HashSet<String>,
    mouse_down: (i32, i32),
    mouse_up: (i32, i32),
    icon_spacing: (i32, i32),
    coordinates: crate::native_box_live_layout::DesktopCoordinates,
    monitors: &[crate::host::Monitor],
) -> Result<NativeMultiDropPrepared, String> {
    prepare_native_multi_drop_with_slot(
        state, before, after, eligible_paths, mouse_down, mouse_up, icon_spacing,
        coordinates, monitors, None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_native_multi_drop_with_slot(
    state: &State,
    before: &[ViewItem],
    after: &[ViewItem],
    eligible_paths: &HashSet<String>,
    mouse_down: (i32, i32),
    mouse_up: (i32, i32),
    icon_spacing: (i32, i32),
    coordinates: crate::native_box_live_layout::DesktopCoordinates,
    monitors: &[crate::host::Monitor],
    cached: Option<(&NativeMultiBoxPlan, Option<&NativeDropSlot>)>,
) -> Result<NativeMultiDropPrepared, String> {
    if icon_spacing.0 < 24 || icon_spacing.1 < 24 || icon_spacing.0 > 512 || icon_spacing.1 > 512 {
        return Err("Explorer icon spacing outside verified bounds".into());
    }
    let dx = i64::from(mouse_up.0) - i64::from(mouse_down.0);
    let dy = i64::from(mouse_up.1) - i64::from(mouse_down.1);
    if dx * dx + dy * dy < 400 {
        return Err("Native gesture did not exceed the physical drag threshold".into());
    }
    let source_paths = physical_multi_drop_identity(before, after, crate::native_ole_persist::MAX_NATIVE_PIN_PATHS)?;
    let before_map = before.iter()
        .map(|i| (i.path.to_lowercase(), i))
        .collect::<HashMap<_, _>>();
    let after_map = after.iter()
        .map(|i| (i.path.to_lowercase(), i))
        .collect::<HashMap<_, _>>();
    if source_paths.iter().any(|p| !eligible_paths.contains(&p.to_lowercase())) {
        return Err("Native drag included a hidden or ineligible icon".into());
    }
    let anchor = source_paths.iter().find_map(|p| {
        let key = p.to_lowercase();
        let (b, a) = (*before_map.get(&key)?, *after_map.get(&key)?);
        let down_cell = crate::native_box_live_layout::cell_for_point(b.pos, icon_spacing, monitors);
        let up_cell = crate::native_box_live_layout::cell_for_point(a.pos, icon_spacing, monitors);
        let down_hit = in_icon(mouse_down, b.pos, down_cell)
            || in_icon_margin(mouse_down, b.pos, down_cell, 6);
        let tol_x = i64::from(down_cell.0.max(up_cell.0)) * 3 / 2;
        let tol_y = i64::from(down_cell.1.max(up_cell.1)) * 3 / 2;
        (down_hit
            && (i64::from(mouse_up.0) - i64::from(a.pos.0)).abs() <= tol_x
            && (i64::from(mouse_up.1) - i64::from(a.pos.1)).abs() <= tol_y)
            .then_some((b.pos, a.pos, down_cell, up_cell))
    });
    let Some((anchor_from, anchor_to, from_cell, to_cell)) = anchor else {
        return Err("Native physical MouseDown/MouseUp did not match the dragged selection".into());
    };
    let point = coordinates.workspace_point(mouse_up);
    if native_header_box_at(state, point).is_none() && in_box(point, state).is_empty() {
        let dropped: HashSet<_> = source_paths.iter().map(|path| path.to_lowercase()).collect();
        let any_pinned = state.groups.iter().flat_map(|group| &group.tabs)
            .filter(|tab| tab.folder.is_none()).flat_map(|tab| &tab.items)
            .any(|item| dropped.contains(&item.path.to_lowercase()));
        if !any_pinned {
            // The physical drag is already verified. Moving loose icons
            // on bare Desktop has no Workspace membership to plan.
            return Err(LOOSE_DESKTOP_DROP.into());
        }
    }
    let uncached_plan;
    let before_plan = if let Some((plan, _)) = cached {
        plan
    } else {
        uncached_plan = user_native_workspace_plan(state, before)?;
        &uncached_plan
    };
    let mut box_owner: Option<(u64, u64, Vec<u64>)> = None;
    let mut saw_loose = false;
    for path in &source_paths {
        let mut found = None;
        for group in &state.groups {
            for (tab_index, tab) in group.tabs.iter().enumerate() {
                if tab.folder.is_some() {
                    continue;
                }
                for pin in &tab.items {
                    if pin.path.eq_ignore_ascii_case(path) {
                        if found.replace((group.id, tab_index, tab.id, pin.id)).is_some() {
                            return Err("Native pin appears in more than one regular Tab".into());
                        }
                    }
                }
            }
        }
        match found {
            Some((gid, tab_index, tid, pin_id)) => {
                let group = state.group(gid).ok_or("Native source Box disappeared")?;
                if group.page != state.page
                    || group.rolled
                    || group.active != tab_index
                    || !before_plan.groups.iter().any(|p| p.group_id == gid)
                {
                    return Err("Dragged source belongs to a hidden/rolled/Portal/non-active Box".into());
                }
                if saw_loose {
                    return Err("Native multi-selection mixes Box and loose Desktop icons".into());
                }
                match &mut box_owner {
                    None => box_owner = Some((gid, tid, vec![pin_id])),
                    Some((prev_g, prev_t, ids)) => {
                        if *prev_g != gid || *prev_t != tid {
                            return Err("Native multi-selection mixes icons from different Boxes".into());
                        }
                        ids.push(pin_id);
                    }
                }
            }
            None => {
                if box_owner.is_some() {
                    return Err("Native multi-selection mixes Box and loose Desktop icons".into());
                }
                saw_loose = true;
            }
        }
    }
    let destination = if let Some((_, Some(slot))) = cached {
        slot.clone()?
    } else {
        native_drop_slot(
            state, before_plan, before, eligible_paths, mouse_up, icon_spacing, coordinates, monitors,
        )?
    };
    let target = destination.map(|(group, tab, _)| (group, tab));
    if source_paths.len() > MAX_MULTI_DRAG_ICONS && (box_owner.is_some() || target.is_some()) {
        // Explorer may retain physical spacing or scale it on a different
        // DPI monitor. Reject unrelated icon movement in either case.
        let follows_anchor = |source_cell: (i32, i32), target_cell: (i32, i32)| {
            let relative_delta = |from: i32, to: i32, scale_from: i32, scale_to: i32| {
                (i64::from(to) - i64::from(from)) * i64::from(scale_to) / i64::from(scale_from)
            };
            source_paths.iter().all(|path| {
                let key = path.to_lowercase();
                let from = before_map[&key].pos;
                let to = after_map[&key].pos;
                let x = i64::from(anchor_to.0) + relative_delta(anchor_from.0, from.0, source_cell.0, target_cell.0);
                let y = i64::from(anchor_to.1) + relative_delta(anchor_from.1, from.1, source_cell.1, target_cell.1);
                (i64::from(to.0) - x).abs() <= i64::from((to_cell.0 / 3).clamp(8, 32))
                    && (i64::from(to.1) - y).abs() <= i64::from((to_cell.1 / 3).clamp(8, 32))
            })
        };
        if !follows_anchor(from_cell, from_cell) && !follows_anchor(from_cell, to_cell) {
            return Err("Large Explorer drag included icons that did not move with the selection".into());
        }
    }
    let insert_at = destination.map(|(_, _, slot)| slot);
    let mut next = state.clone();
    let kind = match (box_owner, target) {
        (Some((from, from_tab, pin_ids)), Some((to, to_tab))) if (from, from_tab) == (to, to_tab) => {
            let items = &mut next.group_mut(from).ok_or("Source Box disappeared")?.tab_mut().items;
            let slot = insert_at.unwrap_or(items.len());
            reorder_pins(items, &pin_ids.into_iter().collect(), slot);
            NativeMultiDropKind::SameBox { group: from }
        }
        (Some((from, src_tab, pin_ids)), Some((to, to_tab))) => {
            let dst = next.group(to).ok_or("Destination disappeared")?
                .tabs.iter().find(|t| t.id == to_tab)
                .ok_or("Destination Tab disappeared")?;
            if dst.items.len() + pin_ids.len() > crate::native_filter_protocol::MAX_ITEMS {
                return Err("Target Tab is full".into());
            }
            let mut insert_at = insert_at.unwrap_or(dst.items.len());
            for &pin_id in &pin_ids {
                next.transfer_between_tabs(from, src_tab, pin_id, to, to_tab, insert_at)?;
                insert_at += 1;
            }
            NativeMultiDropKind::BoxToBox { from, from_tab: src_tab, to, to_tab, pin_ids }
        }
        (Some((from, src_tab, pin_ids)), None) => {
            for path in &source_paths {
                next.unassign(path);
            }
            NativeMultiDropKind::BoxToDesktop { from, from_tab: src_tab, pin_ids }
        }
        (None, Some((to, to_tab))) => {
            let pin_ids = append_new_loose_pins_to_tab(&mut next, to, to_tab, &source_paths, insert_at)?;
            NativeMultiDropKind::DesktopToBox { to, to_tab, pin_ids }
        }
        (None, None) => return Err(LOOSE_DESKTOP_DROP.into()),
    };
    let after_plan = user_native_workspace_plan(&next, before)?;
    if before_plan.groups.len() != after_plan.groups.len()
        || before_plan.groups.iter().any(|g| !after_plan.groups.iter().any(|now| now.group_id == g.group_id))
    {
        return Err("Native drop invalidated an active Desktop Box".into());
    }
    Ok(NativeMultiDropPrepared { next, native_policy: after_plan, kind, source_paths })
}

/// A completed Shell drop onto a tab header carries verified Desktop paths
/// even when Explorer does not move the icons underneath the app window.
pub(crate) fn prepare_native_header_drop(
    state: &State,
    explorer: &[ViewItem],
    eligible: &HashSet<String>,
    point: (f32, f32),
    destination: (u64, u64),
    paths: &[String],
) -> Result<Option<NativeMultiDropPrepared>, String> {
    if native_tab_header_at(state, point) != Some(destination)
        || paths.is_empty() || paths.len() > crate::native_ole_persist::MAX_NATIVE_PIN_PATHS
    {
        return Err("Explorer file drop no longer matches a visible regular Tab".into());
    }
    let (to, to_tab) = destination;
    let target = state.group(to).and_then(|group| group.tabs.iter().find(|tab| tab.id == to_tab))
        .ok_or("Destination Tab disappeared")?;
    // Native drag observation and the OLE target can both report one
    // gesture. A repeat aimed at the already committed destination is inert.
    if paths.iter().all(|path| target.items.iter().any(|pin| pin.path.eq_ignore_ascii_case(path))) {
        return Ok(None);
    }
    let mut source_paths = Vec::with_capacity(paths.len());
    let mut seen = HashSet::new();
    let mut source = None;
    let mut pin_ids = Vec::new();
    let mut loose = false;
    for path in paths {
        let key = path.to_lowercase();
        if !eligible.contains(&key) || !seen.insert(key) {
            return Err("Drop requires distinct visible Desktop icons".into());
        }
        let actual = explorer.iter().find(|item| item.path.eq_ignore_ascii_case(path))
            .ok_or("Dropped icon is no longer on the Desktop")?;
        source_paths.push(actual.path.clone());
        let owner = visible_regular_pin_owner(state, path);
        if let Some((group, tab, pin)) = owner {
            let owner = (group, tab);
            if loose || source.is_some_and(|previous| previous != owner) {
                return Err("Select icons from one Box or the Desktop before dropping onto a Tab".into());
            }
            source = Some(owner);
            pin_ids.push(pin);
        } else {
            if source.is_some() {
                return Err("Select icons from one Box or the Desktop before dropping onto a Tab".into());
            }
            loose = true;
        }
    }
    if source == Some(destination) {
        return Ok(None);
    }
    let mut next = state.clone();
    let count = target.items.len();
    if count + paths.len() > crate::native_filter_protocol::MAX_ITEMS {
        return Err("Target Tab is full".into());
    }
    let kind = if let Some((from, from_tab)) = source {
        let selected: HashSet<_> = pin_ids.into_iter().collect();
        let pin_ids: Vec<_> = state.group(from).ok_or("Source Box disappeared")?.tab().items.iter()
            .filter_map(|pin| selected.contains(&pin.id).then_some(pin.id)).collect();
        for (offset, &pin) in pin_ids.iter().enumerate() {
            next.transfer_between_tabs(from, from_tab, pin, to, to_tab, count + offset)?;
        }
        NativeMultiDropKind::BoxToBox { from, from_tab, to, to_tab, pin_ids }
    } else {
        let pin_ids = append_new_loose_pins_to_tab(&mut next, to, to_tab, &source_paths, Some(count))?;
        NativeMultiDropKind::DesktopToBox { to, to_tab, pin_ids }
    };
    let native_policy = user_native_workspace_plan(&next, explorer)?;
    Ok(Some(NativeMultiDropPrepared { next, native_policy, kind, source_paths }))
}

/// Auto-pin newly added Desktop items at the last verified external drop.
/// Files created programmatically or dropped outside Boxes stay loose.
pub fn prepare_external_desktop_add_drop(
    state: &State,
    desktop_items: &[ViewItem],
    added: &[ViewItem],
    drop_point: Option<(i32, i32)>,
    coordinates: crate::native_box_live_layout::DesktopCoordinates,
) -> Result<Option<NativeMultiDropPrepared>, String> {
    if added.is_empty() || added.len() > crate::native_filter_protocol::MAX_ITEMS {
        return Ok(None);
    }
    let Some(up_pt) = drop_point else {
        return Ok(None);
    };
    let before_plan = user_native_workspace_plan(state, desktop_items)?;
    let Some((to, to_tab)) = eligible_box_target_at(state, &before_plan, up_pt, coordinates, 8.)? else {
        return Ok(None);
    };
    let mut source_paths = Vec::with_capacity(added.len());
    let mut seen = HashSet::new();
    for item in added {
        let key = item.path.to_lowercase();
        if key.trim().is_empty() || !seen.insert(key.clone()) {
            return Err("External desktop additions contain duplicate or empty paths".into());
        }
        let already_pinned = state.groups.iter()
            .flat_map(|g| g.tabs.iter().filter(|t| t.folder.is_none()))
            .flat_map(|t| t.items.iter())
            .any(|pin| pin.path.eq_ignore_ascii_case(&item.path));
        if already_pinned {
            return Err("External added Desktop file is already pinned to a regular Tab".into());
        }
        source_paths.push(item.path.clone());
    }
    let mut next = state.clone();
    let pin_ids = append_new_loose_pins_to_tab(&mut next, to, to_tab, &source_paths, None)?;
    let after_plan = user_native_workspace_plan(&next, desktop_items)?;
    if before_plan.groups.len() != after_plan.groups.len()
        || before_plan.groups.iter().any(|g| !after_plan.groups.iter().any(|now| now.group_id == g.group_id))
    {
        return Err("External Desktop drop invalidated an active Desktop Box".into());
    }
    Ok(Some(NativeMultiDropPrepared {
        next,
        native_policy: after_plan,
        kind: NativeMultiDropKind::DesktopToBox { to, to_tab, pin_ids },
        source_paths,
    }))
}


/// Actual user's physical left-button pointer trail, observed without
/// capturing the mouse or blocking Explorer's own native drag/drop.
/// The controller does not synthesize or replay Windows mouse messages.
#[derive(Debug,Clone)]
pub struct NativeReleasedPointer {
    pub path:String,
    pub index:usize,
    pub down:(i32,i32),
    pub up:(i32,i32),
}
#[derive(Default)]
pub struct NativePointerObserver {
    was_down:bool,
    press:Option<(String,usize,(i32,i32))>,
}
impl NativePointerObserver {
    pub fn is_down(&self) -> bool { self.was_down }

    pub(crate) fn pressed_path(&self) -> Option<&str> {
        self.press.as_ref().map(|(path, _, _)| path.as_str())
    }

    pub(crate) fn is_dragging(&self, point: (i32, i32)) -> bool {
        self.press.as_ref().is_some_and(|(_, _, down)| {
            let dx = i64::from(point.0) - i64::from(down.0);
            let dy = i64::from(point.1) - i64::from(down.1);
            dx * dx + dy * dy >= 400
        })
    }

    /// The pointer_is_real_listview boolean must mean WindowFromPoint
    /// returned the exact Explorer desktop SysListView32 HWND.
    /// Eligible paths exclude hidden/rolled/inactive Tab items.
    pub fn sample(&mut self,button_down:bool,
        pointer:(i32,i32),pointer_is_real_listview:bool,
        expected:&[ViewItem],
        press_context:impl FnOnce()->(HashSet<String>,(i32,i32)),
    )->Option<NativeReleasedPointer>{
        self.sample_with_monitors(
            button_down, pointer, pointer_is_real_listview, expected, &[], press_context,
        )
    }

    pub fn sample_with_monitors(&mut self,button_down:bool,
        pointer:(i32,i32),pointer_is_real_listview:bool,
        expected:&[ViewItem],
        monitors:&[crate::host::Monitor],
        press_context:impl FnOnce()->(HashSet<String>,(i32,i32)),
    )->Option<NativeReleasedPointer>{
        let pressed=button_down && !self.was_down;
        let released=!button_down && self.was_down;
        self.was_down=button_down;
        if pressed {
            self.press=None;
            if pointer_is_real_listview {
                let (eligible,spacing)=press_context();
                if let Some((index,item))=hit_icon_index(
                    pointer,
                    expected,
                    &eligible,
                    &|pos| crate::native_box_live_layout::cell_for_point(pos, spacing, monitors),
                ) {
                    self.press=Some((item.path.clone(),index,pointer));
                }
            }
        }
        if released {
            return self.press.take().and_then(|(path,index,down)|{
                let dx=i64::from(pointer.0)-i64::from(down.0);
                let dy=i64::from(pointer.1)-i64::from(down.1);
                (dx*dx+dy*dy>=400).then_some(NativeReleasedPointer{
                    path,index,down,up:pointer})
            });
        }
        None
    }
    pub fn reset(&mut self){self.was_down=false;self.press=None;}
}

#[cfg(test)]
mod native_pointer_observer_tests {
    use super::*;
    #[test]
    fn reads_eligibility_and_spacing_only_on_a_real_listview_press(){
        let mut observer=NativePointerObserver::default();
        let calls=std::cell::Cell::new(0);
        for (down,on_list) in [(false,true),(true,false),(true,true),
            (false,true),(true,true),(true,true),(false,true)] {
            observer.sample(down,(0,0),on_list,&[],||{
                calls.set(calls.get()+1);
                (HashSet::new(),(80,90))
            });
        }
        assert_eq!(calls.get(),1);
    }

    #[test]
    fn physical_release_requires_actual_unique_visible_icon_and_travel(){
        let icons=vec![
            ViewItem{path:"C:/A".into(),label:"A".into(),pos:(100,100)},
            ViewItem{path:"C:/B".into(),label:"B".into(),pos:(400,100)},
        ];
        let visible=HashSet::from(["c:/a".into(),"c:/b".into()]);
        let mut observe=NativePointerObserver::default();
        assert!(observe.sample(true,(120,120),false,&icons,||(visible.clone(),(80,90))).is_none());
        assert!(observe.sample(false,(490,200),true,&icons,||(visible.clone(),(80,90))).is_none());
        assert!(observe.sample(true,(120,130),true,&icons,||(visible.clone(),(80,90))).is_none());
        let event=observe.sample(false,(480,240),true,&icons,||(visible.clone(),(80,90))).unwrap();
        assert_eq!(event.path,"C:/A");
        assert_eq!(event.index,0);
        assert_eq!(event.down,(120,130));
        assert_eq!(event.up,(480,240));
        let visible=HashSet::from(["c:/b".into()]);
        assert!(observe.sample(true,(120,130),true,&icons,||(visible.clone(),(80,90))).is_none());
        assert!(observe.sample(false,(480,240),true,&icons,||(visible.clone(),(80,90))).is_none());
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::native_box_live_layout::DesktopCoordinates;

    const SCREEN: DesktopCoordinates = DesktopCoordinates { origin: (0, 0), scale: 1.0 };
    use crate::model::{Bounds,Item};
    fn icon(name:&str,pos:(i32,i32))->ViewItem{
        ViewItem{path:format!("C:/Desktop/{name}.lnk"),label:name.into(),pos}
    }
    fn pin(name:&str,id:u64)->Item{
        Item{id,name:name.into(),path:format!("C:/Desktop/{name}.lnk"),
            kind:"lnk".into(),size:0,modified:0,label:None}
    }
    fn layout()->(State,Vec<ViewItem>,u64,u64) {
        let mut s=State::empty();
        let a=s.add_group("Source",Bounds{x:100.,y:80.,w:300.,h:330.});
        let b=s.add_group("Target",Bounds{x:500.,y:80.,w:300.,h:330.});
        s.group_mut(a).unwrap().tab_mut().items.push(pin("A",31));
        s.group_mut(b).unwrap().tab_mut().items.push(pin("B",32));
        let items=vec![icon("A",(160,160)),icon("B",(560,160)),icon("Loose",(860,160))];
        (s,items,a,b)
    }
    fn moved(before:&[ViewItem],item:usize,to:(i32,i32),end:(i32,i32))->NativePhysicalDrop{
        let mut after=before.to_vec();
        after[item].pos=to;
        NativePhysicalDrop{source_path:before[item].path.clone(),source_native_index:item,
            mouse_down:(before[item].pos.0+26,before[item].pos.1+30),
            mouse_up:end,before:before.to_vec(),after,icon_spacing:(90,108)}
    }
    #[test]
    fn genuine_box_to_box_preserves_pin_id_and_original_input(){
        let (state,icons,a,b)=layout();
        let candidate=moved(&icons,0,(570,220),(596,260));
        let base=serde_json::to_vec(&state).unwrap();
        let planned=prepare_native_drop(&state,&candidate,SCREEN).unwrap();
        assert_eq!(planned.kind,NativeDropKind::BoxToBox{
            from:a,from_tab:state.group(a).unwrap().tab().id,to:b,
            to_tab:state.group(b).unwrap().tab().id,pin_id:31});
        assert!(planned.next.group(a).unwrap().tab().items.is_empty());
        assert_eq!(planned.next.group(b).unwrap().tab().items.len(),2);
        assert_eq!(planned.next.group(b).unwrap().tab().items[1].id,31);
        assert_eq!(serde_json::to_vec(&state).unwrap(),base);
    }
    #[test]
    fn box_to_loose_unpins_without_touching_source_file(){
        let (state,icons,a,_)=layout();
        let event=moved(&icons,0,(860,300),(888,333));
        let result=prepare_native_drop(&state,&event,SCREEN).unwrap();
        assert_eq!(result.kind,NativeDropKind::BoxToDesktop{
            from:a,from_tab:state.group(a).unwrap().tab().id,pin_id:31});
        assert!(result.next.group(a).unwrap().tab().items.is_empty());
    }
    #[test]
    fn refuses_ambiguous_hidden_portal_and_multi_icon_moves(){
        let (mut state,icons,a,b)=layout();
        let event=moved(&icons,0,(570,220),(600,260));
        let mut altered=event.clone();
        altered.after[1].pos=(600,310);
        assert!(physical_drop_identity(&altered).is_err());
        altered=event.clone();
        altered.mouse_down=(1,1);
        assert!(physical_drop_identity(&altered).is_err());
        altered=event.clone();
        altered.mouse_up=(2,2);
        assert!(physical_drop_identity(&altered).is_err());
        state.group_mut(b).unwrap().tab_mut().folder=Some("D:/Portal".into());
        assert!(prepare_native_drop(&state,&event,SCREEN).is_err());
        state.group_mut(b).unwrap().tab_mut().folder=None;
        state.group_mut(a).unwrap().rolled=true;
        assert!(prepare_native_drop(&state,&event,SCREEN).is_err());
    }
    #[test]
    fn refuses_header_overlap_and_unverified_source(){
        let (mut state,icons,_,b)=layout();
        let event=moved(&icons,0,(570,100),(593,120));
        assert!(prepare_native_drop(&state,&event,SCREEN).is_err());
        let new=state.add_group("Overlap",Bounds{x:540.,y:110.,w:300.,h:300.});
        assert_ne!(b,new);
        let event=moved(&icons,0,(580,220),(600,250));
        assert!(prepare_native_drop(&state,&event,SCREEN).is_err());
    }

    #[test]
    fn native_tab_header_drop_keeps_pin_identity_inside_the_same_box() {
        let (mut state, icons, group, _) = layout();
        let from_tab = state.group(group).unwrap().tab().id;
        let to_tab = state.id();
        state.group_mut(group).unwrap().tabs.push(crate::model::Tab::new(to_tab, "Later"));
        let bounds = state.group(group).unwrap().header_tabs().0[1].1;
        let point = (bounds.x + 12., bounds.y + 12. - crate::model::HEADER);
        assert!(point.1 < state.group(group).unwrap().bounds.y);
        let eligible = icons.iter().map(|item| item.path.to_lowercase()).collect();
        let destination = (group, to_tab);
        let result = prepare_native_header_drop(
            &state, &icons, &eligible, point, destination, &[icons[0].path.clone()],
        ).unwrap().unwrap();
        assert_eq!(result.kind, NativeMultiDropKind::BoxToBox {
            from: group, from_tab, to: group, to_tab, pin_ids: vec![31],
        });
        let after = result.next.group(group).unwrap();
        assert_eq!(after.tab().id, from_tab);
        assert!(after.tab().items.is_empty());
        assert_eq!(after.tabs[1].items[0].id, 31);
        assert_eq!(result.native_policy.hidden_for(&[]).unwrap(), vec![0]);
        assert!(prepare_native_header_drop(
            &result.next, &icons, &HashSet::new(), point, destination, &[icons[0].path.clone()],
        ).unwrap().is_none());
        assert!(prepare_native_header_drop(
            &state, &icons, &eligible, point, (group, from_tab), &[icons[0].path.clone()],
        ).is_err());

        let screen = (point.0 as i32, point.1 as i32);
        let native = moved(&icons, 0, (screen.0 - 20, screen.1 - 24), screen);
        let physical = prepare_native_multi_drop(
            &state, &icons, &native.after, &eligible, native.mouse_down, native.mouse_up, (90, 108), SCREEN,
        ).unwrap();
        assert_eq!(physical.next, result.next);

        state.group_mut(group).unwrap().tabs[1].folder = Some("D:/Folder".into());
        assert!(prepare_native_header_drop(
            &state, &icons, &eligible, point, destination, &[icons[0].path.clone()],
        ).is_err());
    }

    #[test]
    fn side_docked_box_routes_drops_to_tabs_along_the_vertical_header() {
        use crate::model::DockEdge;
        let (mut state, _, group, _) = layout();
        let tab_id = state.id();
        let source = state.group_mut(group).unwrap();
        source.tabs.push(crate::model::Tab::new(tab_id, "Second"));
        source.dock = Some(DockEdge::Left);
        let header = crate::paint::native_group_header(source);
        let tabs = crate::paint::native_header_tabs(source).collect::<Vec<_>>();
        assert!(tabs[0].1.y + tabs[0].1.h < tabs[1].1.y);
        for (_, b) in &tabs {
            assert!(header.contains(b.x, b.y));
            assert!(header.contains(b.x + b.w - 1., b.y + b.h - 1.));
        }
        let b = tabs[1].1;
        let point = (b.x + b.w / 2., b.y + b.h / 2.);
        assert_eq!(native_tab_header_at(&state, point), Some((group, tab_id)));
        state.group_mut(group).unwrap().dock = Some(DockEdge::Right);
        assert!(native_tab_header_at(&state, point).is_none());
    }

    #[test]
    fn visible_source_can_drop_on_the_folded_destination_without_unrolling_the_saved_boxes() {
        let (mut real, icons, source, destination) = layout();
        real.group_mut(source).unwrap().rolled = true;
        real.group_mut(destination).unwrap().rolled = true;
        let from_tab = real.group(source).unwrap().tab().id;
        let to_tab = real.group(destination).unwrap().tab().id;
        let header = crate::paint::native_group_header(real.group(destination).unwrap());
        let point = (header.x + 35., header.y + 18.);
        let (effective, _) = crate::native_effective_state(&real, Some(source), false);
        let eligible = HashSet::from([icons[0].path.to_lowercase()]);

        let drop = prepare_native_header_drop(
            &effective, &icons, &eligible, point, (destination, to_tab), &[icons[0].path.clone()],
        ).unwrap().unwrap();
        assert_eq!(drop.kind, NativeMultiDropKind::BoxToBox {
            from: source, from_tab, to: destination, to_tab, pin_ids: vec![31],
        });
        let saved = crate::native_ole_persist::pins_change_onto(&real, &drop.next, &drop.source_paths).unwrap();
        assert!(saved.group(source).unwrap().rolled && saved.group(destination).unwrap().rolled);
        assert!(saved.group(source).unwrap().tab().items.is_empty());
        assert_eq!(saved.group(destination).unwrap().tab().items.last().unwrap().id, 31);
        assert!(prepare_native_header_drop(
            &effective, &icons, &eligible, (point.0, header.y + header.h + 10.),
            (destination, to_tab), &[icons[0].path.clone()],
        ).is_err());
    }

    #[test]
    fn same_box_drop_inserts_before_an_icon_or_appends_at_the_end() {
        let mut state = State::empty();
        let group = state.add_group("Source", Bounds { x: 100., y: 80., w: 450., h: 340. });
        let names = ["First", "Middle", "Last"];
        let before = names.iter().enumerate().map(|(index, name)| {
            state.group_mut(group).unwrap().tab_mut().items.push(pin(name, 100 + index as u64));
            icon(name, (120 + index as i32 * 90, 140))
        }).collect::<Vec<_>>();
        let eligible = before.iter().map(|item| item.path.to_lowercase()).collect();
        for (up, expected) in [((266, 163), [101, 100, 102]), ((451, 255), [101, 102, 100])] {
            let mut after = before.clone();
            after[0].pos = (up.0 - 15, up.1 - 19);
            let drop = prepare_native_multi_drop(
                &state, &before, &after, &eligible, (135, 159), up, (90, 108), SCREEN,
            ).unwrap();
            let items = &drop.next.group(group).unwrap().tab().items;
            let actual = items.iter().map(|item| item.id).collect::<Vec<_>>();
            assert_eq!(actual, expected);
            let plan = user_native_workspace_plan(&state, &before).unwrap();
            let slot = native_drop_slot(&state, &plan, &before, &eligible, up, (90, 108), SCREEN, &[]);
            let target = slot.as_ref().unwrap().unwrap();
            let prediction = predict_same_box_reorder(
                &state, target, &[before[0].path.clone()], &before[0].path, &eligible,
            ).unwrap();
            assert_eq!(prediction, drop.next);
            assert!(predict_same_box_reorder(
                &state, target, &[before[1].path.clone()], &before[0].path, &eligible,
            ).is_none());
            let cached = prepare_native_multi_drop_with_slot(
                &state, &before, &after, &eligible, (135, 159), up, (90, 108), SCREEN, &[],
                Some((&plan, Some(&slot))),
            ).unwrap();
            assert_eq!(cached.next, drop.next);
            assert!(prepare_native_multi_drop_with_slot(
                &state, &before, &before, &eligible, (135, 159), up, (90, 108), SCREEN, &[],
                Some((&plan, Some(&slot))),
            ).is_err());
        }
    }

    #[test]
    fn desktop_drop_into_box_accepts_seventeen_files_and_inserts_at_the_release_slot() {
        let dir = std::env::temp_dir().join(format!("dc-bulk-desktop-drop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut state = State::empty();
        let group = state.add_group("Target", Bounds { x: 500., y: 80., w: 410., h: 340. });
        state.group_mut(group).unwrap().tab_mut().items.extend([pin("First", 91), pin("Second", 92)]);
        let mut before = vec![icon("First", (520, 140)), icon("Second", (610, 140))];
        for n in 0..17 {
            let path = dir.join(format!("File{n}.txt"));
            std::fs::write(&path, []).unwrap();
            before.push(ViewItem { path: path.to_string_lossy().into_owned(), label: format!("File{n}"),
                pos: (40 + (n % 3) * 90, 540 + (n / 3) * 108) });
        }
        let eligible = before.iter().map(|item| item.path.to_lowercase()).collect();
        let mut after = before.clone();
        for item in &mut after[2..] {
            item.pos.0 += 545;
            item.pos.1 -= 400;
        }
        let drop = prepare_native_multi_drop(
            &state, &before, &after, &eligible, (57, 562), (602, 162), (90, 108), SCREEN,
        ).unwrap();
        let items = &drop.next.group(group).unwrap().tab().items;
        assert_eq!(drop.source_paths.len(), 17);
        assert_eq!(items.first().unwrap().id, 91);
        assert_eq!(items.last().unwrap().id, 92);
        assert_eq!(items.len(), 19);
        for n in 0..17 { std::fs::remove_file(dir.join(format!("File{n}.txt"))).unwrap(); }
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn full_box_drag_unpins_seventeen_icons_only_when_they_move_together() {
        let mut state = State::empty();
        let group = state.add_group("Source", Bounds { x: 100., y: 80., w: 750., h: 360. });
        let mut before = Vec::new();
        for n in 0..17 {
            let name = format!("App{n}");
            state.group_mut(group).unwrap().tab_mut().items.push(pin(&name, 100 + n as u64));
            before.push(icon(&name, (122 + (n % 8) * 86, 148 + (n / 8) * 94)));
        }
        let eligible = before.iter().map(|item| item.path.to_lowercase()).collect();
        let mut after = before.clone();
        for item in &mut after {
            item.pos.0 += 450;
            item.pos.1 += 420;
        }
        let drop = prepare_native_multi_drop(
            &state, &before, &after, &eligible, (140, 168), (590, 588), (86, 94), SCREEN,
        ).unwrap();
        assert!(matches!(drop.kind, NativeMultiDropKind::BoxToDesktop { from, .. } if from == group));
        assert_eq!(drop.source_paths.len(), 17);
        assert!(drop.next.group(group).unwrap().tab().items.is_empty());
        after[16].pos.1 += 93;
        assert!(prepare_native_multi_drop(
            &state, &before, &after, &eligible, (140, 168), (590, 588), (86, 94), SCREEN,
        ).is_err());
    }

    #[test]
    fn multi_icon_box_to_box_desktop_and_same_box_preserve_order_and_reject_mixed_or_oversized() {
        let mut s = State::empty();
        let a = s.add_group("Source", Bounds { x: 100., y: 80., w: 300., h: 330. });
        let b = s.add_group("Target", Bounds { x: 500., y: 80., w: 300., h: 330. });
        s.group_mut(a).unwrap().tab_mut().items.extend([pin("A1", 41), pin("A2", 42)]);
        s.group_mut(b).unwrap().tab_mut().items.push(pin("B1", 43));

        let mut before = vec![
            icon("A1", (120, 140)),
            icon("A2", (210, 140)),
            icon("B1", (560, 160)),
            icon("Loose1", (860, 160)),
            icon("Loose2", (950, 160)),
        ];
        // Extend up to 11 icons for >8 bulk test.
        for n in 0..6 {
            before.push(icon(&format!("Extra{n}"), (860 + n * 90, 300)));
        }
        let eligible: HashSet<String> = before.iter().map(|i| i.path.to_lowercase()).collect();

        // 2-icon BoxToBox (moved out of order in Vec, sorted deterministically by before pos).
        let mut after = before.clone();
        after[0].pos = (570, 220);
        after[1].pos = (660, 220);
        let prepared = prepare_native_multi_drop(
            &s, &before, &after, &eligible, (140, 160), (590, 245), (90, 108), SCREEN,
        ).unwrap();
        assert_eq!(prepared.kind, NativeMultiDropKind::BoxToBox {
            from: a,
            from_tab: s.group(a).unwrap().tab().id,
            to: b,
            to_tab: s.group(b).unwrap().tab().id,
            pin_ids: vec![41, 42],
        });
        assert_eq!(prepared.source_paths, vec!["C:/Desktop/A1.lnk", "C:/Desktop/A2.lnk"]);
        assert!(prepared.next.group(a).unwrap().tab().items.is_empty());
        assert_eq!(
            prepared.next.group(b).unwrap().tab().items.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![41, 42, 43]
        );

        // 2-icon BoxToDesktop.
        let unpin = prepare_native_multi_drop(
            &s, &before, &after, &eligible, (140, 160), (900, 500), (90, 108), SCREEN,
        );
        // After[0] pos (570,220) is too far from mouse_up=(900,500), so move after[0] near (880,480).
        assert!(unpin.is_err());
        let mut after_desk = before.clone();
        after_desk[0].pos = (880, 480);
        after_desk[1].pos = (970, 480);
        let unpin = prepare_native_multi_drop(
            &s, &before, &after_desk, &eligible, (140, 160), (900, 500), (90, 108), SCREEN,
        ).unwrap();
        assert_eq!(unpin.kind, NativeMultiDropKind::BoxToDesktop {
            from: a,
            from_tab: s.group(a).unwrap().tab().id,
            pin_ids: vec![41, 42],
        });
        assert!(unpin.next.group(a).unwrap().tab().items.is_empty());

        // 2-icon SameBox.
        let mut after_same = before.clone();
        after_same[0].pos = (200, 240);
        after_same[1].pos = (290, 240);
        let same = prepare_native_multi_drop(
            &s, &before, &after_same, &eligible, (140, 160), (220, 260), (90, 108), SCREEN,
        ).unwrap();
        assert_eq!(same.kind, NativeMultiDropKind::SameBox { group: a });

        // Mixed-owner (Box A1 + Loose1) refused.
        let mut after_mixed = before.clone();
        after_mixed[0].pos = (570, 220);
        after_mixed[3].pos = (660, 220);
        assert!(prepare_native_multi_drop(
            &s, &before, &after_mixed, &eligible, (140, 160), (590, 245), (90, 108), SCREEN,
        ).is_err());

        // >8 icons moved refused.
        let mut after_bulk = before.clone();
        for item in after_bulk.iter_mut().take(9) {
            item.pos.1 += 20;
        }
        assert!(physical_multi_drop_identity(&before, &after_bulk, MAX_MULTI_DRAG_ICONS).is_err());

        // 2-icon DesktopToBox using existing temp files so Item::from_path succeeds.
        let dir = std::env::temp_dir().join(format!("dc-multi-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("D1.txt");
        let f2 = dir.join("D2.txt");
        std::fs::write(&f1, b"1").unwrap();
        std::fs::write(&f2, b"2").unwrap();
        before[3].path = f1.to_string_lossy().into_owned();
        before[4].path = f2.to_string_lossy().into_owned();
        let eligible_real: HashSet<String> = before.iter().map(|i| i.path.to_lowercase()).collect();
        let mut after_d2b = before.clone();
        after_d2b[3].pos = (570, 220);
        after_d2b[4].pos = (660, 220);
        let d2b = prepare_native_multi_drop(
            &s, &before, &after_d2b, &eligible_real, (880, 180), (590, 245), (90, 108), SCREEN,
        ).unwrap();
        assert!(matches!(d2b.kind, NativeMultiDropKind::DesktopToBox { to, .. } if to == b));
        assert_eq!(d2b.next.group(b).unwrap().tab().items.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mixed_dpi_scaled_monitor_hitbox_and_drop_tolerance_accept_valid_drags() {
        let monitors = [
            crate::host::Monitor {
                key: "main".into(), rect: [0, 0, 2560, 1440], work: [0, 0, 2560, 1380],
                dpi: 96, primary: true,
            },
            crate::host::Monitor {
                key: "side".into(), rect: [2560, -1, 2560, 1440], work: [2560, -1, 2560, 1380],
                dpi: 120, primary: false,
            },
        ];
        let dir = std::env::temp_dir().join(format!("dc-scaled-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let loose_file = dir.join("ScaledLoose.txt");
        let box_file = dir.join("ScaledInBox.txt");
        std::fs::write(&loose_file, b"1").unwrap();
        std::fs::write(&box_file, b"2").unwrap();

        let mut s = State::empty();
        let b_id = s.add_group("ScaledBox", Bounds { x: 2860., y: 200., w: 500., h: 400. });
        let b_item = Item {
            id: 51,
            name: "ScaledInBox".into(),
            path: box_file.to_string_lossy().into_owned(),
            kind: "txt".into(),
            size: 1,
            modified: 1,
            label: None,
        };
        s.group_mut(b_id).unwrap().tab_mut().items.push(b_item);

        // Box icon at (2860, 270), Loose icon at (2668, 135) on 125% monitor (cell 94x134).
        let before = vec![
            ViewItem { path: box_file.to_string_lossy().into_owned(), label: "ScaledInBox".into(), pos: (2860, 270) },
            ViewItem { path: loose_file.to_string_lossy().into_owned(), label: "ScaledLoose".into(), pos: (2668, 135) },
        ];
        let eligible: HashSet<String> = before.iter().map(|i| i.path.to_lowercase()).collect();

        // Grab bottom-right of 125% cell: (+84, +122) exceeds primary (75, 107) but fits (94, 134).
        let down = (2668 + 84, 135 + 122);
        let up = (2960, 350);
        let mut observer = NativePointerObserver::default();
        assert!(observer.sample_with_monitors(
            true, down, true, &before, &monitors, || (eligible.clone(), (75, 107)),
        ).is_none());
        let released = observer.sample_with_monitors(
            false, up, true, &before, &monitors, || (eligible.clone(), (75, 107)),
        ).unwrap();
        assert_eq!(released.path, before[1].path);

        // Explorer settled icon 1.2 cells away due to occupied neighbor cell -> still accepted.
        let mut after_in = before.clone();
        after_in[1].pos = (3060, 390);
        let d2b = prepare_native_multi_drop_with_monitors(
            &s, &before, &after_in, &eligible, down, up, (75, 107), SCREEN, &monitors,
        ).unwrap();
        assert!(matches!(d2b.kind, NativeMultiDropKind::DesktopToBox { to, .. } if to == b_id));

        // Too far (> 1.5x scaled cell) must still reject.
        let mut after_far = before.clone();
        after_far[1].pos = (3200, 390);
        assert!(prepare_native_multi_drop_with_monitors(
            &s, &before, &after_far, &eligible, down, up, (75, 107), SCREEN, &monitors,
        ).is_err());

        // Box -> Desktop drag from 125% monitor bottom-right of icon cell (+84, +122).
        let box_down = (2860 + 84, 270 + 122);
        let box_up = (2600, 500);
        let mut after_out = before.clone();
        after_out[0].pos = (2574, 410);
        let b2d = prepare_native_multi_drop_with_monitors(
            &s, &before, &after_out, &eligible, box_down, box_up, (75, 107), SCREEN, &monitors,
        ).unwrap();
        assert!(matches!(b2d.kind, NativeMultiDropKind::BoxToDesktop { from, .. } if from == b_id));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn external_desktop_addition_auto_pins_only_when_dropped_into_active_regular_box() {
        let dir = std::env::temp_dir().join(format!("dc-ext-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let existing_path = dir.join("InBox.txt");
        std::fs::write(&existing_path, b"0").unwrap();
        let mut s = State::empty();
        let box_id = s.add_group("BoxA", Bounds { x: 100., y: 80., w: 300., h: 330. });
        let portal_id = s.add_group("Portal", Bounds { x: 500., y: 80., w: 300., h: 330. });
        s.group_mut(box_id).unwrap().tab_mut().items.push(Item {
            id: 201,
            name: "InBox".into(),
            path: existing_path.to_string_lossy().into_owned(),
            kind: "txt".into(),
            size: 1,
            modified: 1,
            label: None,
        });
        s.group_mut(portal_id).unwrap().tab_mut().folder = Some("C:/Downloads".into());

        let f1 = dir.join("DroppedFromExplorer1.txt");
        let f2 = dir.join("DroppedFromExplorer2.txt");
        std::fs::write(&f1, b"1").unwrap();
        std::fs::write(&f2, b"2").unwrap();
        let added = vec![
            ViewItem { path: f1.to_string_lossy().into_owned(), label: "DroppedFromExplorer1".into(), pos: (140, 160) },
            ViewItem { path: f2.to_string_lossy().into_owned(), label: "DroppedFromExplorer2".into(), pos: (220, 160) },
        ];
        let mut desktop = vec![
            ViewItem { path: existing_path.to_string_lossy().into_owned(), label: "InBox".into(), pos: (100, 120) },
        ];
        desktop.extend(added.clone());

        // 1. Background file creation (no external drop point) -> stays loose (`Ok(None)`).
        assert!(prepare_external_desktop_add_drop(&s, &desktop, &added, None, SCREEN).unwrap().is_none());

        // 2. Dropped outside any Box -> stays loose (`Ok(None)`).
        assert!(prepare_external_desktop_add_drop(&s, &desktop, &added, Some((920, 500)), SCREEN).unwrap().is_none());

        // 3. Dropped in the frame margin or Portal body -> Err (caller keeps loose cleanly).
        assert!(prepare_external_desktop_add_drop(&s, &desktop, &added, Some((150, 84)), SCREEN).is_err());
        assert!(prepare_external_desktop_add_drop(&s, &desktop, &added, Some((580, 200)), SCREEN).is_err());

        // 4. Dropped into active regular BoxA body -> auto-pinned in order.
        let planned = prepare_external_desktop_add_drop(
            &s, &desktop, &added, Some((200, 220)), SCREEN,
        ).unwrap().unwrap();
        assert!(matches!(planned.kind, NativeMultiDropKind::DesktopToBox { to, .. } if to == box_id));
        assert_eq!(planned.next.group(box_id).unwrap().tab().items.len(), 3);

        // 5. A verified batch is assigned together even beyond eight files.
        let bulk = (0..9)
            .map(|n| {
                let path = dir.join(format!("B{n}.txt"));
                std::fs::write(&path, b"test").unwrap();
                ViewItem { path: path.to_string_lossy().into_owned(), label: format!("B{n}"), pos: (10 + n * 80, 500) }
            })
            .collect::<Vec<_>>();
        desktop.extend(bulk.clone());
        let planned = prepare_external_desktop_add_drop(&s, &desktop, &bulk, Some((200, 220)), SCREEN).unwrap().unwrap();
        assert_eq!(planned.next.group(box_id).unwrap().tab().items.len(), 10);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolves_single_expanded_box_over_overlapping_rolled_box_and_rejects_two_expanded() {
        let mut s = State::empty();
        let top = s.add_group("Games", Bounds { x: 100., y: 1., w: 460., h: 630. });
        let under = s.add_group("Docs", Bounds { x: 100., y: 69., w: 460., h: 562. });
        s.group_mut(top).unwrap().rolled = true;
        assert_eq!(resolve_drop_box(&[top, under], &s).unwrap(), Some(under));

        // Swapped during hover-reveal: `top` is unrolled, `under` is occluded/rolled.
        s.group_mut(top).unwrap().rolled = false;
        s.group_mut(under).unwrap().rolled = true;
        assert_eq!(resolve_drop_box(&[top, under], &s).unwrap(), Some(top));

        // Two expanded overlapping Boxes must still fail closed.
        s.group_mut(under).unwrap().rolled = false;
        assert!(resolve_drop_box(&[top, under], &s).unwrap_err().contains("overlaps"));
    }
}
