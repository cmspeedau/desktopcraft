//! Read-only placement and native policy preflight for adding a new empty
//! regular two-Tab Box to a DesktopCraft Workspace. Never reposition the
//! Windows Desktop icons and never mutate the input State.
use crate::explorer_native::{self,NativeMultiBoxPlan};
use crate::host::Monitor;
use crate::model::{Bounds,State};
use crate::shell::ViewItem;

pub(crate) struct PlannedNativeDestination{
    pub state:State,
    pub policy:NativeMultiBoxPlan,
    pub monitor:String,
    pub source_id:u64,
    pub destination_id:u64,
    pub destination_tab:u64,
    pub bounds:Bounds,
    pub icon_clearance_px:i32,
}

fn intersects(a:Bounds,b:Bounds,pad:f32)->bool{
    a.x<a.x+a.w && b.x<b.x+b.w &&
    a.x < b.x+b.w+pad && a.x+a.w+pad > b.x &&
    a.y < b.y+b.h+pad && a.y+a.h+pad > b.y
}

fn finite_positive(rect:Bounds)->bool{
    rect.x.is_finite() && rect.y.is_finite() && rect.w.is_finite() &&
    rect.h.is_finite() && rect.w>0. && rect.h>0.
}

fn candidate_clear(state:&State,icons:&[ViewItem],bounds:Bounds,clear:i32)->bool{
    if !finite_positive(bounds) {return false;}
    if state.groups.iter().any(|group|
        group.page==state.page &&
        (!finite_positive(group.bounds) ||
         intersects(bounds,group.bounds,clear as f32))) {
        return false;
    }
    // Explorer icon positions are top-left in physical desktop coordinates.
    // Do not cover even unclaimed native icons, allowing space for labels.
    icons.iter().all(|item|{
        let b=Bounds{x:item.pos.0 as f32-16.,y:item.pos.1 as f32-16.,
            w:116.,h:132.};
        !intersects(bounds,b,clear as f32)
    })
}

/// Plans a fully validated 2-Tab target on a *single* physical monitor work
/// area. Never crosses work bounds / taskbar or touches any existing Box or
/// native icon footprint. Prefers the monitor containing the source Box.
pub(crate) fn plan_new_native_destination(
    state:&State,icons:&[ViewItem],monitors:&[Monitor],source_id:u64,
)->Result<PlannedNativeDestination,String>{
    let source=state.group(source_id)
        .ok_or("Native target source Box missing")?;
    if source.page!=state.page || source.rolled ||
        source.active>=source.tabs.len() || source.tabs.len()<2 ||
        source.tab().folder.is_some() {
        return Err("Native target source must be a visible regular multi-Tab Box".into());
    }
    if icons.is_empty() || monitors.is_empty() || monitors.len()>8 {
        return Err("Native target requires a verified Explorer Desktop and monitor work area".into());
    }
    explorer_native::native_group_tab_plan(state,icons,source_id,128)?;
    let baseline=serde_json::to_vec(state).map_err(|e|e.to_string())?;
    let source_center=(source.bounds.x+source.bounds.w/2.,
                       source.bounds.y+source.bounds.h/2.);
    if !finite_positive(source.bounds) {
        return Err("Native target source Box bounds are invalid".into());
    }
    let mut candidates=Vec::<(i64,Bounds,String)>::new();
    for monitor in monitors {
        let [mx,my,mw,mh]=monitor.work;
        if mw<480 || mh<410 || monitor.dpi<48 || monitor.dpi>768 {
            continue;
        }
        let is_source=source_center.0>=mx as f32 &&
            source_center.0<(mx+mw) as f32 &&
            source_center.1>=my as f32 &&
            source_center.1<(my+mh) as f32;
        let x_min=mx.saturating_add(24);
        let y_min=my.saturating_add(72);
        let max_x=mx.saturating_add(mw).saturating_sub(360+24);
        let max_y=my.saturating_add(mh).saturating_sub(270+24);
        if max_x<x_min || max_y<y_min {continue;}
        let mut y=y_min;
        while y<=max_y {
            let mut x=x_min;
            while x<=max_x {
                let area=Bounds{x:x as f32,y:y as f32,w:360.,h:270.};
                if candidate_clear(state,icons,area,32) {
                    let cx=area.x+area.w/2.;
                    let cy=area.y+area.h/2.;
                    let dx=(cx-source_center.0) as f64;
                    let dy=(cy-source_center.1) as f64;
                    let cost=(dx*dx+dy*dy) as i64 +
                        if is_source{0}else{1_000_000_000};
                    candidates.push((cost,area,monitor.key.clone()));
                }
                x=x.saturating_add(32);
            }
            y=y.saturating_add(32);
        }
    }
    candidates.sort_by(|a,b|a.0.cmp(&b.0).then_with(||
        a.1.x.total_cmp(&b.1.x)).then_with(||a.1.y.total_cmp(&b.1.y)));
    // If the best placement fails the compositional native policy, try
    // another. Never change the input State while searching.
    for (_,bounds,monitor) in candidates {
        let mut copy=state.clone();
        let destination_id=copy.add_group("Native Desktop",bounds);
        copy.add_tab(destination_id)?;
        copy.group_mut(destination_id).ok_or("Native destination lost after creation")?
            .active=0;
        let destination_tab=copy.group(destination_id)
            .ok_or("Native destination tab missing")?.tab().id;
        let policy=explorer_native::native_multi_box_plan(&copy,icons,16,128)?;
        if policy.groups.iter().any(|g|g.group_id==source_id)
            && policy.groups.iter().any(|g|
                g.group_id==destination_id && g.active==0 &&
                g.native_indices.iter().all(|v|v.is_empty())) {
            if serde_json::to_vec(state).map_err(|e|e.to_string())?!=baseline {
                return Err("Native destination preflight unexpectedly altered the source State".into());
            }
            return Ok(PlannedNativeDestination{
                state:copy,policy,monitor,source_id,destination_id,
                destination_tab,bounds,icon_clearance_px:32
            });
        }
    }
    Err("No non-overlapping regular Native Box position on current monitor work areas".into())
}

#[cfg(test)]
mod tests{
    use super::*;
    use crate::model::Item;
    fn pin(path:&str,id:u64)->Item {
        Item{id,name:path.into(),path:path.into(),kind:"lnk".into(),
            size:0,modified:0,label:None}
    }
    fn screen()->Monitor{
        Monitor{key:"DISPLAY1".into(),rect:[0,0,1920,1080],
            work:[0,0,1920,1040],dpi:96,primary:true}
    }
    fn setup()->(State,Vec<ViewItem>,u64){
        let mut state=State::empty();
        let src=state.add_group("Source",Bounds{x:60.,y:80.,w:440.,h:300.});
        state.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/A.lnk",1));
        state.group_mut(src).unwrap().tab_mut().items.push(pin("C:/Desktop/B.lnk",2));
        state.add_tab(src).unwrap();
        state.group_mut(src).unwrap().active=0;
        let native=vec![
            ViewItem{path:"C:/Desktop/A.lnk".into(),label:"A".into(),pos:(100,120)},
            ViewItem{path:"C:/Desktop/B.lnk".into(),label:"B".into(),pos:(200,120)},
            ViewItem{path:"C:/Desktop/Loose.lnk".into(),label:"Loose".into(),pos:(580,130)}
        ];
        (state,native,src)
    }
    #[test]
    fn selects_empty_two_tab_target_without_modifying_any_box_or_explorer_icon(){
        let (state,native,source)=setup();
        let old=serde_json::to_vec(&state).unwrap();
        let selected=plan_new_native_destination(&state,&native,&[screen()],source).unwrap();
        assert_eq!(serde_json::to_vec(&state).unwrap(),old);
        assert_eq!(selected.policy.groups.len(),2);
        assert_eq!(selected.state.groups.len(),state.groups.len()+1);
        assert_eq!(selected.state.group(selected.destination_id).unwrap().tabs.len(),2);
        assert_eq!(selected.state.group(selected.destination_id).unwrap().active,0);
        assert!(candidate_clear(&state,&native,selected.bounds,32));
        assert_eq!(selected.destination_tab,selected.state.group(selected.destination_id).unwrap().tab().id);
        assert!(selected.bounds.y+selected.bounds.h<=screen().work[3] as f32);
        assert_eq!(native[0].pos,(100,120));
    }
    #[test]
    fn rejects_rolled_portal_or_missing_source_without_modifying_state(){
        let (state,items,source)=setup();
        let mut rolled=state.clone();
        rolled.group_mut(source).unwrap().rolled=true;
        assert!(plan_new_native_destination(&rolled,&items,&[screen()],source).is_err());
        let mut portal=state.clone();
        portal.group_mut(source).unwrap().tabs[0].folder=Some("D:/Portal".into());
        assert!(plan_new_native_destination(&portal,&items,&[screen()],source).is_err());
        assert!(plan_new_native_destination(&state,&items,&[screen()],u64::MAX).is_err());
    }
    #[test]
    fn rejects_when_visible_boxes_or_native_icons_occupy_every_valid_space(){
        let (state,icons,source)=setup();
        let mut blocked=state.clone();
        blocked.add_group("Fills Work Area",Bounds{x:0.,y:0.,w:1920.,h:1040.});
        assert!(plan_new_native_destination(&blocked,&icons,&[screen()],source).is_err());
        assert!(plan_new_native_destination(&state,&icons,&[],source).is_err());
        assert!(plan_new_native_destination(&state,&icons,&[Monitor{
            work:[0,0,360,270],..screen()
        }],source).is_err());
    }
    #[test]
    fn placement_rejects_unsafe_overlap_and_nan_coordinate(){
        let (state,icons,source)=setup();
        let proposed=Bounds{x:80.,y:120.,w:360.,h:270.};
        assert!(!candidate_clear(&state,&icons,proposed,32));
        let icon=Bounds{x:550.,y:150.,w:360.,h:270.};
        assert!(!candidate_clear(&state,&icons,icon,32));
        assert!(!candidate_clear(&state,&icons,
            Bounds{x:f32::NAN,y:0.,w:360.,h:270.},32));
        let ok=plan_new_native_destination(&state,&icons,&[screen()],source).unwrap();
        assert!(ok.bounds.w>=320.);
    }
}
