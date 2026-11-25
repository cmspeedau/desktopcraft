//! Pure native Explorer-owned icon placement planner, NO Windows IO.
//! One icon cell at a time, entire batch planned before any IFolderView move.
//! Does not invent scroll/overflow behavior: rejects capacity overflow.
use std::collections::HashMap;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Rect{pub x:i32,pub y:i32,pub w:i32,pub h:i32}
impl Rect {
    pub(crate) fn contains(self, point: (i32, i32)) -> bool {
        point.0 >= self.x && i64::from(point.0) < i64::from(self.x) + i64::from(self.w)
            && point.1 >= self.y && i64::from(point.1) < i64::from(self.y) + i64::from(self.h)
    }
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct NativePlace{pub path:String,pub from:(i32,i32),pub to:(i32,i32)}

/// Indexed positions from one Explorer snapshot, reused across Box plans.
pub(crate) struct PlacementSnapshot<'a> {
    icons: Vec<(&'a str, (i32, i32))>,
    by_path: HashMap<String, usize>,
}

impl<'a> PlacementSnapshot<'a> {
    pub(crate) fn new(
        icons: impl ExactSizeIterator<Item = (&'a str, (i32, i32))>,
    ) -> Result<Self, String> {
        if icons.len()>2048 {
            return Err("Explorer native item count exceeds placement limit".into());
        }
        let mut by_path=HashMap::with_capacity(icons.len());
        let mut positions=Vec::with_capacity(icons.len());
        for (path,position) in icons {
            if by_path.insert(path.to_lowercase(),positions.len()).is_some() {
                return Err("Explorer native item identities not unique".into());
            }
            positions.push((path,position));
        }
        Ok(Self{icons:positions,by_path})
    }

    pub(crate) fn index(&self,path:&str)->Option<usize>{
        let index=*self.by_path.get(&path.to_lowercase())?;
        self.icons[index].0.eq_ignore_ascii_case(path).then_some(index)
    }
}

/// Target cells must lie within the Box and the connected desktop, and
/// stay clear of every unassigned icon in the original Explorer snapshot.
pub fn plan_box_cells(
    bounds:Rect,selected:&[String],all:&[(String,(i32,i32))],
    cell:(i32,i32),screen_rects:&[Rect],
)->Result<Vec<NativePlace>,String>{
    let snapshot=PlacementSnapshot::new(all.iter().map(|(path,pos)|(path.as_str(),*pos)))?;
    let paths=selected.iter().map(String::as_str).collect::<Vec<_>>();
    plan_indexed_cells(bounds,&paths,&snapshot,cell,screen_rects,None,None)
}

fn plan_indexed_cells(
    bounds:Rect,selected:&[&str],all:&PlacementSnapshot<'_>,
    cell:(i32,i32),screen_rects:&[Rect],grid_origin:Option<(i32,i32)>,
    obstacles:Option<&[(i32,i32)]>,
)->Result<Vec<NativePlace>,String>{
    if bounds.w<64 || bounds.h<64
        || cell.0<32 || cell.1<32 || cell.0>512 || cell.1>512
        || selected.is_empty() || selected.len()>crate::native_filter_protocol::MAX_ITEMS
        || all.icons.len()<selected.len() {
        return Err("Invalid native Box sizing, item or cell limits".into());
    }
    let mut assigned=vec![false;all.icons.len()];
    let mut origins=Vec::with_capacity(selected.len());
    for &path in selected {
        let index=all.index(path).ok_or("Some pinned Box items missing from Explorer Desktop")?;
        if path.is_empty() || std::mem::replace(&mut assigned[index],true) {
            return Err("Native Box item identity duplicate/blank".into());
        }
        origins.push(all.icons[index].1);
    }
    let unassigned_owned: Vec<(i32, i32)>;
    let blocking: &[(i32, i32)] = match obstacles {
        Some(loose_only) => loose_only,
        None => {
            unassigned_owned = all.icons.iter().zip(assigned)
                .filter_map(|((_, position), assigned)| (!assigned).then_some(*position))
                .collect();
            &unassigned_owned
        }
    };
    let (anchor, cols, rows) = cell_layout(bounds, cell, grid_origin)?;
    if cols==0 || rows==0 || selected.len()>cols.saturating_mul(rows){
        return Err(format!(
            "Native Box needs {} icons but has capacity {} (scroll/overflow not implemented)",
            selected.len(),cols.saturating_mul(rows)));
    }
    let mut moves=Vec::with_capacity(selected.len());
    for (i,path) in selected.iter().enumerate(){
        let x=anchor.0+(i%cols) as i32*cell.0;
        let y=anchor.1+(i/cols) as i32*cell.1;
        let destination=(x,y);
        if !screen_rects.iter().any(|r|
            x>=r.x && y>=r.y && i64::from(x)+i64::from(cell.0)<i64::from(r.x)+i64::from(r.w)
                && i64::from(y)+i64::from(cell.1)<i64::from(r.y)+i64::from(r.h)){
            return Err("Native Box icon cell outside connected physical monitors".into());
        }
        if blocking.iter().any(|&origin|cells_overlap(origin,destination,cell)){
            return Err("Native Box proposed cell overlaps unrelated Explorer icon".into());
        }
        moves.push(NativePlace{path:(*path).to_owned(),from:origins[i],to:destination});
    }
    Ok(moves)
}
pub(crate) fn cells_overlap(a:(i32,i32),b:(i32,i32),cell:(i32,i32))->bool{
    (i64::from(a.0)-i64::from(b.0)).abs()<i64::from(cell.0)
        && (i64::from(a.1)-i64::from(b.1)).abs()<i64::from(cell.1)
}

pub(crate) fn cell_overlaps_rect(pos: (i32, i32), cell: (i32, i32), rect: Rect) -> bool {
    i64::from(pos.0) < i64::from(rect.x) + i64::from(rect.w)
        && i64::from(pos.0) + i64::from(cell.0) > i64::from(rect.x)
        && i64::from(pos.1) < i64::from(rect.y) + i64::from(rect.h)
        && i64::from(pos.1) + i64::from(cell.1) > i64::from(rect.y)
}

/// Pick one on-screen parking point for hidden Box icons (inactive Tabs,
/// rolled-up Boxes, scrolled-out rows) outside every active Box and visible
/// loose Desktop icon. With Align-to-Grid disabled, all hidden icons stack
/// cleanly at this single invisible point without blocking Box drags.
pub(crate) fn choose_hidden_park_point(
    boxes: &[Rect],
    loose: &[(i32, i32)],
    cell: (i32, i32),
    monitors: &[Rect],
) -> Option<(i32, i32)> {
    if cell.0 < 32 || cell.1 < 32 {
        return None;
    }
    let free = |pt: (i32, i32)| {
        !boxes.iter().any(|&b| cell_overlaps_rect(pt, cell, b))
            && !loose.iter().any(|&l| cells_overlap(l, pt, cell))
    };
    for &screen in monitors {
        if screen.w <= cell.0 + 24 || screen.h <= cell.1 + 16 {
            continue;
        }
        let bottom_pad = if screen.h > cell.1 + 64 {
            (cell.1 / 2).clamp(48, 72)
        } else {
            8
        };
        let max_x = screen.x + screen.w - cell.0 - 16;
        let max_y = screen.y + screen.h - cell.1 - bottom_pad;
        let min_x = screen.x + 8;
        let min_y = screen.y + 8;
        // Prefer quiet corners first (bottom-right, top-right, bottom-left, top-left).
        for pt in [(max_x, max_y), (max_x, min_y), (min_x, max_y), (min_x, min_y)] {
            if free(pt) {
                return Some(pt);
            }
        }
        // Fallback: scan monitor from bottom-right upward in cell-sized steps.
        let mut y = max_y;
        while y >= min_y {
            let mut x = max_x;
            while x >= min_x {
                if free((x, y)) {
                    return Some((x, y));
                }
                x -= cell.0;
            }
            y -= cell.1;
        }
    }
    None
}

/// A page of Explorer-owned native item identities. Off-page items remain
/// at their ORIGINAL physical positions, and must be hidden by the matching
/// native paint/input policy until their page is selected. Never park icons
/// at (-20000,-20001) or another out-of-screen coordinate.
#[derive(Clone,Debug,PartialEq,Eq)]
/// One Box viewport. The viewport scrolls by whole icon rows:
/// `page` is the first visible row and `page_count` the number of scroll
/// positions (1 when everything fits).
pub struct NativePagePlan{
    pub moves:Vec<NativePlace>,
    pub page:usize,
    pub page_count:usize,
    pub page_capacity:usize,
    pub offpage_paths:Vec<String>,
    /// Index of the first visible icon (`page` × columns).
    pub start:usize,
    pub visible_rows:usize,
    /// Explorer icon cell on this Box's monitor (scaled on a monitor whose
    /// DPI differs from the main monitor's).
    pub cell:(i32,i32),
}
const CELL_INSET: i32 = 0;

/// Two-grid inner padding between the Box border and the first icon bitmap's
/// top-left coordinate so centered Explorer text labels remain inside the Box
/// instead of protruding past its left or top edges.
pub(crate) fn box_grid_padding(bounds: Rect, cell: (i32, i32)) -> (i32, i32) {
    let base_x = (cell.0 * 22 / 75).clamp(14, 36);
    let base_y = (cell.1 * 12 / 107).clamp(10, 24);
    let avail_x = (i64::from(bounds.w) - 14 - i64::from(cell.0)).max(0) as i32;
    let avail_y = (i64::from(bounds.h) - 14 - i64::from(cell.1)).max(0) as i32;
    (base_x.min(avail_x), base_y.min(avail_y))
}

/// Minimum physical body `(w, h)` that fits one full Explorer icon column
/// and one full icon row with standard `box_grid_padding` and edge margin,
/// matching `cell_layout` across every icon size and monitor DPI.
pub(crate) fn native_box_min_body_phys(cell: (i32, i32)) -> (i32, i32) {
    let cw = cell.0.clamp(32, 512);
    let ch = cell.1.clamp(32, 512);
    let base_x = (cw * 22 / 75).clamp(14, 36);
    let base_y = (ch * 12 / 107).clamp(10, 24);
    ((cw + base_x + 14).max(64), (ch + base_y + 14).max(64))
}

pub(crate) fn cell_layout(bounds: Rect, cell: (i32, i32), grid_origin: Option<(i32, i32)>)
    -> Result<((i32, i32), usize, usize), String> {
    if bounds.w<64 || bounds.h<64
        || !(32..=512).contains(&cell.0) || !(32..=512).contains(&cell.1) {
        return Err("Invalid native Box or Explorer icon cell size".into());
    }
    let right = bounds.x.checked_add(bounds.w).ok_or("Box right bound overflow")?;
    let bottom = bounds.y.checked_add(bounds.h).ok_or("Box bottom bound overflow")?;
    let anchor = if let Some(origin) = grid_origin {
        let base = (bounds.x + CELL_INSET, bounds.y + CELL_INSET);
        let align = |point: i32, origin: i32, step: i32| {
            let offset = (i64::from(origin) - i64::from(point)).rem_euclid(i64::from(step));
            point.saturating_add(offset as i32)
        };
        (align(base.0, origin.0, cell.0), align(base.1, origin.1, cell.1))
    } else {
        let (pad_x, pad_y) = box_grid_padding(bounds, cell);
        (bounds.x.saturating_add(pad_x), bounds.y.saturating_add(pad_y))
    };
    let cols = ((i64::from(right) - 14 - i64::from(anchor.0)) / i64::from(cell.0)).max(0) as usize;
    let rows = ((i64::from(bottom) - 14 - i64::from(anchor.1)) / i64::from(cell.1)).max(0) as usize;
    Ok((anchor, cols, rows))
}
/// Visible columns and rows of a Box viewport.
fn viewport(bounds:Rect,cell:(i32,i32),grid_origin:Option<(i32,i32)>)->Result<(usize,usize),String>{
    let (_, cols, rows) = cell_layout(bounds, cell, grid_origin)?;
    Ok((cols,rows))
}
/// Scroll positions for `count` icons: one per hidden row, plus the top.
fn scroll_positions(count:usize,cols:usize,rows:usize)->usize{
    if cols == 0 || rows == 0 { return 1; }
    count.div_ceil(cols).saturating_sub(rows)+1
}
/// Number of row scroll positions a Box offers for `count` icons.
pub fn box_scroll_positions(bounds:Rect,cell:(i32,i32),grid_origin:Option<(i32,i32)>,count:usize)
    ->Result<usize,String>{
    let (cols,rows)=viewport(bounds,cell,grid_origin)?;
    Ok(scroll_positions(count,cols,rows))
}
#[cfg(test)]
pub fn plan_box_page(
    bounds:Rect,selected:&[String],all:&[(String,(i32,i32))],
    cell:(i32,i32),screen_rects:&[Rect],page:usize,
)->Result<NativePagePlan,String>{
    let snapshot=PlacementSnapshot::new(all.iter().map(|(path,pos)|(path.as_str(),*pos)))?;
    let paths=selected.iter().map(String::as_str).collect::<Vec<_>>();
    plan_indexed_page(bounds,&paths,&snapshot,cell,screen_rects,page,None,None)
}

pub(crate) fn plan_indexed_page(
    bounds:Rect,selected:&[&str],all:&PlacementSnapshot<'_>,
    cell:(i32,i32),screen_rects:&[Rect],page:usize,grid_origin:Option<(i32,i32)>,
    obstacles:Option<&[(i32,i32)]>,
)->Result<NativePagePlan,String>{
    let (cols,rows)=viewport(bounds,cell,grid_origin)?;
    let capacity=cols.saturating_mul(rows);
    if selected.is_empty(){
        return Ok(NativePagePlan{moves:Vec::new(),page:0,
            page_count:1,page_capacity:capacity,offpage_paths:Vec::new(),
            start:0,visible_rows:rows,cell});
    }
    if selected.len()>crate::native_filter_protocol::MAX_ITEMS{return Err("Native Box contains more items than Explorer's policy can address".into());}
    if capacity==0{
        if page!=0{return Err("Native Box requested scroll row outside available range".into());}
        return Ok(NativePagePlan{
            moves:Vec::new(),page:0,page_count:1,page_capacity:0,
            offpage_paths:selected.iter().map(|s|(*s).to_owned()).collect(),
            start:0,visible_rows:0,cell,
        });
    }
    let page_count=scroll_positions(selected.len(),cols,rows);
    if page>=page_count{return Err("Native Box requested scroll row outside available range".into());}
    let start=page*cols;
    let stop=(start+capacity).min(selected.len());
    let visible=&selected[start..stop];
    let moves=plan_indexed_cells(bounds,visible,all,cell,screen_rects,grid_origin,obstacles)?;
    let offpage_paths=selected.iter().enumerate().filter(|(i,_)|
        *i<start || *i>=stop).map(|(_,s)|(*s).to_owned()).collect();
    Ok(NativePagePlan{moves,page,page_count,page_capacity:capacity,offpage_paths,
        start,visible_rows:rows,cell})
}

#[cfg(test)]
mod native_page_tests {
    use super::*;
    #[test]
    fn large_viewport_can_place_few_icons() {
        let bounds = Rect { x: 80, y: 80, w: 1300, h: 1100 };
        let paths = vec!["A".into(), "B".into()];
        let items = vec![("A".into(), (10, 10)), ("B".into(), (40, 10))];
        let screen = Rect { x: 0, y: 0, w: 1920, h: 1440 };
        let plan = plan_box_page(bounds, &paths, &items, (75, 107), &[screen], 0).unwrap();
        assert_eq!(plan.page_capacity, 160);
        assert_eq!(plan.moves.len(), 2);
    }
    #[test]
    fn native_page_uses_observed_grid_and_keeps_cells_inside_box() {
        let bounds = Rect { x:1788, y:30, w:740, h:416 };
        let icons = (0..24).map(|index| (format!("icon-{index}"), (11, 2 + 107 * index)))
            .collect::<Vec<_>>();
        let snapshot = PlacementSnapshot::new(icons.iter().map(|(path, pos)| (path.as_str(), *pos))).unwrap();
        let paths = icons.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>();
        let screens = [Rect { x:0, y:0, w:2560, h:1440 }];
        let page = plan_indexed_page(bounds, &paths, &snapshot, (75, 107), &screens, 0, Some((11, 2)), None).unwrap();
        assert_eq!(page.page_capacity, 27);
        assert_eq!(page.moves[0].to, (1811, 109));
        assert_eq!(page.moves[23].to, (2186, 323));
    }
    #[test]
    fn thirty_one_icons_scroll_one_row_at_a_time(){
        let bounds=Rect{x:1788,y:31,w:740,h:416};
        let icons=(0..31).map(|n|(format!("C:/desk/{n}.lnk"),(n*50,900)))
            .collect::<Vec<_>>();
        let paths=icons.iter().map(|(path,_)|path.clone()).collect::<Vec<_>>();
        let monitors=[Rect{x:0,y:0,w:2560,h:1440}];
        let first=plan_box_page(bounds,&paths,&icons,(75,107),&monitors,0).unwrap();
        let second=plan_box_page(bounds,&paths,&icons,(75,107),&monitors,1).unwrap();
        // 9 columns x 3 rows visible; 31 icons are 4 rows, so 2 scroll positions.
        assert_eq!((first.page_capacity,first.page_count,first.moves.len(),first.offpage_paths.len()),
            (27,2,27,4));
        assert_eq!((second.start,second.visible_rows,second.moves.len(),second.offpage_paths.len()),
            (9,3,22,9));
        // Scrolling one row: icon 9 moves up into the first cell.
        assert_eq!(second.moves[0].path,"C:/desk/9.lnk");
        assert_eq!(second.moves[0].to,first.moves[0].to);
        assert!(plan_box_page(bounds,&paths,&icons,(75,107),&monitors,2).is_err());
        assert!(first.moves.iter().all(|p|p.to.0>=bounds.x && p.to.1>=bounds.y));
    }
    #[test]
    fn scroll_positions_count_hidden_rows(){
        assert_eq!(scroll_positions(27,9,3),1);
        assert_eq!(scroll_positions(28,9,3),2);
        assert_eq!(scroll_positions(46,9,3),4);
        assert_eq!(scroll_positions(0,9,3),1);
    }
    #[test]
    fn pages_do_not_wrap_silently_and_empty_tab_has_single_empty_page(){
        let b=Rect{x:1976,y:840,w:360,h:270};
        assert!(plan_box_page(b,&["x".into()],&[("x".into(),(1,1))],
            (75,107),&[Rect{x:0,y:0,w:2560,h:1440}],1).is_err());
        assert_eq!(plan_box_page(b,&[],&[],(75,107),&[],0).unwrap().page_count,1);
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    fn rect()->Rect{Rect{x:1976,y:840,w:360,h:270}}
    fn monitor()->Vec<Rect>{vec![Rect{x:0,y:0,w:2560,h:1440}]}
    fn icons()->Vec<(String,(i32,i32))>{vec![
        ("C:/ExampleApp.lnk".into(),(311,321)),
        ("C:/ExampleApp Launcher.lnk".into(),(161,1284)),
        ("C:/Else.lnk".into(),(10,10))]}
    #[test]
    fn two_real_native_icons_get_distinct_cells_within_box(){
        let plan=plan_box_cells(rect(),&["C:/ExampleApp.lnk".into(),"C:/ExampleApp Launcher.lnk".into()],
            &icons(),(75,107),&monitor()).unwrap();
        assert_eq!(plan[0].to,(1998,852));
        assert_eq!(plan[1].to,(2073,852));
        assert_eq!(plan[0].from,(311,321));
    }
    #[test]
    fn offscreen_parked_or_conflicting_icon_blocks_proposed_cell(){
        let mut all=icons();all[2].1=(2045,905);
        assert!(plan_box_cells(rect(),&["C:/ExampleApp.lnk".into()],
            &all,(75,107),&monitor()).unwrap_err().contains("overlaps"));
    }
    #[test]
    fn capacity_overflow_refuses_without_partial_plan(){
        let all=(0..9).map(|i|(format!("C:/icon{i}.lnk"),(i*100,300))).collect::<Vec<_>>();
        let selected=all.iter().map(|(p,_)|p.clone()).collect::<Vec<_>>();
        assert!(plan_box_cells(rect(),&selected,&all,(75,107),&monitor()).unwrap_err()
            .contains("scroll/overflow not implemented"));
    }
    #[test]
    fn missing_or_duplicate_identity_never_plans(){
        let items=icons();
        assert!(plan_box_cells(rect(),&["X".into()],&items,(75,107),&monitor()).is_err());
        assert!(plan_box_cells(rect(),&["C:/ExampleApp.lnk".into(),"c:/ExampleApp.lnk".into()],
            &items,(75,107),&monitor()).is_err());
    }
    #[test]
    fn secondary_physical_monitor_with_negative_origin_accepted(){
        let all=vec![("C:/a".into(),(55,10))];
        let b=Rect{x:-2420,y:250,w:400,h:290};
        let p=plan_box_cells(b,&["C:/a".into()],&all,(75,107),
            &[Rect{x:-2560,y:0,w:2560,h:1440}]).unwrap();
        assert_eq!(p[0].to,(-2398,262));
    }
    #[test]
    fn loose_only_obstacles_ignore_hidden_icons_while_protecting_visible_loose_icons(){
        // Hidden icon sits at (1998,852) — right where Box drops — while visible loose icon is at (11,2).
        let all: Vec<(String, (i32, i32))> = vec![
            ("C:/Visible.lnk".into(),(311,321)),
            ("C:/HiddenTab.lnk".into(),(1998,852)),
            ("C:/Loose.lnk".into(),(11,2)),
        ];
        let snap=PlacementSnapshot::new(all.iter().map(|(p,pos)|(p.as_str(),*pos))).unwrap();
        let loose=[(11,2)];
        let page=plan_indexed_page(rect(),&["C:/Visible.lnk"],&snap,(75,107),
            &monitor(),0,None,Some(&loose)).unwrap();
        assert_eq!(page.moves[0].to,(1998,852));
        // Dropping on top of the visible loose icon still refuses.
        let over_loose=Rect{x:0,y:0,w:360,h:270};
        assert!(plan_indexed_page(over_loose,&["C:/Visible.lnk"],&snap,(75,107),
            &monitor(),0,None,Some(&loose)).unwrap_err().contains("overlaps"));
    }
    #[test]
    fn hidden_park_point_avoids_boxes_and_loose_icons_and_relocates_when_covered(){
        let mon=[Rect{x:0,y:0,w:2560,h:1440}];
        let loose=[(11,2)];
        let p1=choose_hidden_park_point(&[rect()],&loose,(75,107),&mon).unwrap();
        assert!(mon[0].contains(p1));
        assert!(!cell_overlaps_rect(p1,(75,107),rect()));
        // Drag a Box right over p1: park point relocates cleanly to another free spot.
        let cover=Rect{x:p1.0-20,y:p1.1-20,w:300,h:300};
        let p2=choose_hidden_park_point(&[rect(),cover],&loose,(75,107),&mon).unwrap();
        assert_ne!(p1,p2);
        assert!(mon[0].contains(p2));
        assert!(!cell_overlaps_rect(p2,(75,107),cover));
        assert!(!cells_overlap(p2,loose[0],(75,107)));

        // Work area with taskbar bottom reserve keeps 2-line icon labels clear of taskbar clamp.
        let work = [Rect { x: 0, y: 0, w: 2560, h: 1392 }];
        let p3 = choose_hidden_park_point(&[rect()], &loose, (75, 97), &work).unwrap();
        assert!(p3.1 <= 1392 - 97 - 48, "park y={} too close to taskbar bottom", p3.1);
    }
    #[test]
    fn box_grid_padding_scales_with_dpi_and_shrinks_for_tight_boxes(){
        let standard=Rect{x:100,y:100,w:400,h:300};
        assert_eq!(box_grid_padding(standard,(75,107)),(22,12));
        assert_eq!(box_grid_padding(standard,(94,134)),(27,15));
        let tight=Rect{x:100,y:100,w:200,h:200};
        assert_eq!(box_grid_padding(tight,(180,190)),(6,0));
        assert_eq!(box_grid_padding(Rect{x:100,y:100,w:180,h:200},(320,340)),(0,0));
    }
    #[test]
    fn zoomed_large_and_extra_large_icons_reflow_and_handle_tiny_box_safely(){
        let mons=monitor();
        let all=(0..4).map(|i|(format!("C:/zoom{i}.lnk"),(11+i*90,2))).collect::<Vec<_>>();
        let paths=all.iter().map(|(p,_)|p.clone()).collect::<Vec<_>>();
        let large_plan=plan_box_page(Rect{x:700,y:500,w:420,h:500},&paths,&all,(154,194),&mons,0).unwrap();
        assert_eq!(large_plan.page_capacity,4);
        assert_eq!(large_plan.moves.len(),4);
        assert_eq!(large_plan.moves[0].to,(736,521));

        let xlarge=plan_box_page(Rect{x:700,y:500,w:760,h:460},&paths,&all,(354,394),&mons,0).unwrap();
        assert_eq!(xlarge.page_capacity,2);
        assert_eq!(xlarge.page_count,2);
        assert_eq!(xlarge.moves.len(),2);
        assert_eq!(xlarge.offpage_paths.len(),2);

        // Box smaller than a zoomed Extra-Large icon degrades cleanly to an offpage-only page.
        let tiny=plan_box_page(Rect{x:700,y:500,w:260,h:260},&paths,&all,(354,394),&mons,0).unwrap();
        assert!(tiny.moves.is_empty());
        assert_eq!(tiny.page_capacity,0);
        assert_eq!(tiny.page_count,1);
        assert_eq!(tiny.visible_rows,0);
        assert_eq!(tiny.offpage_paths.len(),4);
    }

    #[test]
    fn single_row_and_single_column_compact_boxes_place_and_scroll_cleanly() {
        let mons = monitor();
        let all = (0..10)
            .map(|i| (format!("C:/compact{i}.lnk"), (11 + i * 75, 2)))
            .collect::<Vec<_>>();
        let paths = all.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();

        let row_97 = plan_box_page(
            Rect { x: 200, y: 200, w: 400, h: 125 },
            &paths,
            &all,
            (75, 97),
            &mons,
            0,
        ).unwrap();
        assert_eq!(row_97.visible_rows, 1);
        assert_eq!(row_97.moves.len(), 4);
        assert_eq!(row_97.page_count, 3);
        let page1 = plan_box_page(
            Rect { x: 200, y: 200, w: 400, h: 125 },
            &paths,
            &all,
            (75, 97),
            &mons,
            1,
        ).unwrap();
        assert_eq!(page1.moves.len(), 4);
        assert_eq!(page1.moves[0].path, "C:/compact4.lnk");

        let row_107 = plan_box_page(
            Rect { x: 200, y: 200, w: 400, h: 163 },
            &paths,
            &all,
            (75, 107),
            &mons,
            0,
        ).unwrap();
        assert_eq!(row_107.visible_rows, 1);
        assert_eq!(row_107.moves.len(), 4);

        let single_cell = plan_box_page(
            Rect { x: 200, y: 200, w: 95, h: 125 },
            &paths,
            &all,
            (75, 97),
            &mons,
            0,
        ).unwrap();
        assert_eq!(single_cell.page_capacity, 1);
        assert_eq!(single_cell.visible_rows, 1);
        assert_eq!(single_cell.moves.len(), 1);

        assert!(plan_box_page(
            Rect { x: 200, y: 200, w: 60, h: 125 },
            &paths,
            &all,
            (75, 97),
            &mons,
            0,
        ).is_err());
        assert!(plan_box_page(
            Rect { x: 200, y: 200, w: 200, h: 60 },
            &paths,
            &all,
            (75, 97),
            &mons,
            0,
        ).is_err());
    }

    #[test]
    fn native_box_min_body_fits_exactly_one_cell_for_every_icon_size() {
        for cell in [(48, 60), (56, 68), (75, 97), (75, 107), (94, 134), (154, 194), (354, 394)] {
            let (w, h) = native_box_min_body_phys(cell);
            let rect = Rect { x: 200, y: 200, w, h };
            let (_, cols, rows) = cell_layout(rect, cell, None).unwrap();
            assert_eq!((cols, rows), (1, 1), "min body {:?} failed for cell {:?}", (w, h), cell);
            let base_x = (cell.0.clamp(32, 512) * 22 / 75).clamp(14, 36);
            let base_y = (cell.1.clamp(32, 512) * 12 / 107).clamp(10, 24);
            assert_eq!(box_grid_padding(rect, cell), (base_x, base_y));
        }
    }
}
