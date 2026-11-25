use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn plan_layout(
    member:&NativeTwoBoxMembership,state:&State,original:&[ViewItem],
    native_order:&[ViewItem],cell:(i32,i32),screens:&[Rect],monitors:&[host::Monitor],
    coordinates: DesktopCoordinates, snap_to_grid: bool, desktop_grid_origin: Option<(i32, i32)>,
    page_for:impl Fn(u64)->usize,
)->Result<(Vec<NativePagePlan>,Vec<usize>,Option<(i32,i32)>,Vec<Move>),String>{
    if member.desktop_order.len()!=native_order.len()
        || member.desktop_order.iter().zip(native_order).any(|((path,_),item)|
            !path.eq_ignore_ascii_case(&item.path)) {
        return Err("Native Explorer layout index order no longer matches verified membership".into());
    }
    let snapshot=PlacementSnapshot::new(original.iter().map(|item|(item.path.as_str(),item.pos)))?;
    if original.len()!=native_order.len()
        || native_order.iter().any(|item|snapshot.index(&item.path).is_none()) {
        return Err("Native position checkpoint and ListView identities differ".into());
    }
    let mut box_owned_paths = BTreeSet::new();
    for idx in member.hidden_for_current_selection()? {
        if let Some(item) = native_order.get(idx) {
            box_owned_paths.insert(item.path.to_lowercase());
        }
    }
    for &(group_id, tab_index) in &member.selected_tabs {
        for &idx in member.tab_indices(group_id, tab_index)? {
            if let Some(item) = native_order.get(idx) {
                box_owned_paths.insert(item.path.to_lowercase());
            }
        }
    }
    // In two-grid mode, active Boxes/Portals nudge any covered visible loose
    // Desktop icons onto nearest free Desktop grid cells before planning Box
    // cells or hidden parking, so Boxes may move/resize/open freely.
    let (nudged_loose, loose_positions) = if !snap_to_grid {
        plan_nudged_loose_icons(
            original, &box_owned_paths, state, cell, monitors, coordinates, desktop_grid_origin,
        )?
    } else {
        let pos = original.iter()
            .filter(|item| !box_owned_paths.contains(&item.path.to_lowercase()))
            .map(|item| item.pos)
            .collect::<Vec<_>>();
        (Vec::new(), pos)
    };
    let obstacles = (!snap_to_grid).then_some(loose_positions.as_slice());
    let mut plans:Vec<NativePagePlan>=Vec::with_capacity(member.selected_tabs.len());
    let mut hidden=member.hidden_for_current_selection()?;
    let mut visible=Vec::new();
    for &(group_id,tab_index) in &member.selected_tabs {
        let group=state.group(group_id).ok_or("Native Box no longer in Workspace")?;
        if group.rolled || group.page!=state.page {
            return Err("Native Box not active/expanded".into());
        }
        let tab=group.tabs.get(tab_index).ok_or("Native selected Tab no longer valid")?;
        if tab.folder.is_some(){return Err("Native Portal unsupported by live layout".into());}
        let indices=member.tab_indices(group_id,tab_index)?;
        let paths=indices.iter().map(|&index|native_order.get(index)
            .map(|item|item.path.as_str()).ok_or("Native item missing from verified index order"))
            .collect::<Result<Vec<_>,_>>()?;
        let bounds = coordinates.screen_rect(group_native_bounds(group));
        let grid = (!paths.is_empty())
            .then(|| box_grid(bounds, original, monitors, cell, snap_to_grid)).transpose()?;
        let (box_screens, origin, box_cell) = match &grid {
            Some(grid) => (std::slice::from_ref(&grid.screen), grid.origin, grid.cell),
            None => (screens, None, cell),
        };
        let requested_page = page_for(group_id);
        let first_row = if requested_page == 0 { 0 } else {
            let positions = native_box_placement::box_scroll_positions(bounds, box_cell, origin, paths.len())?;
            requested_page.min(positions.saturating_sub(1))
        };
        let page=native_box_placement::plan_indexed_page(
            bounds,&paths,&snapshot,box_cell,box_screens,first_row,origin,obstacles)?;
        for movement in &page.moves {
            if plans.iter().any(|prior| prior.moves.iter().any(|placed|
                !placed.path.eq_ignore_ascii_case(&movement.path)
                && native_box_placement::cells_overlap(placed.to,movement.to,
                    (prior.cell.0.max(box_cell.0),prior.cell.1.max(box_cell.1))))) {
                return Err("Native Boxes propose overlapping Explorer icon cells".into());
            }
        }
        let start=page.start;
        let end=start+page.moves.len();
        visible.extend_from_slice(&indices[start..end]);
        hidden.extend_from_slice(&indices[..start]);
        hidden.extend_from_slice(&indices[end..]);
        plans.push(page);
    }
    hidden.sort_unstable();
    hidden.dedup();
    if visible.iter().any(|index|hidden.binary_search(index).is_ok()) {
        return Err("Native icon cell is marked hidden by the same generation".into());
    }
    let hidden_park = if !snap_to_grid && !hidden.is_empty() {
        let active_boxes = state.groups.iter()
            .filter(|g| g.page == state.page && !g.rolled)
            .map(|g| coordinates.screen_rect(group_native_bounds(g)))
            .collect::<Vec<_>>();
        let ordered_monitors = monitors.iter().filter(|m| m.primary)
            .chain(monitors.iter().filter(|m| !m.primary))
            .map(|m| if m.work[2] > 0 && m.work[3] > 0 {
                Rect { x: m.work[0], y: m.work[1], w: m.work[2], h: m.work[3] }
            } else {
                monitor_rect(m)
            })
            .collect::<Vec<_>>();
        let park_screens = if ordered_monitors.is_empty() { screens } else { &ordered_monitors };
        Some(native_box_placement::choose_hidden_park_point(
            &active_boxes, &loose_positions, cell, park_screens,
        ).ok_or("No free on-screen cell available to park hidden native icons")?)
    } else {
        None
    };
    Ok((plans,hidden,hidden_park,nudged_loose))
}

#[cfg(test)]
mod placement_projection_tests {
    use super::*;
    use crate::model::Item;

    #[test]
    fn logical_box_and_pointer_share_the_negative_physical_monitor_origin() {
        let coordinates = DesktopCoordinates { origin: (-1920, -1080), scale: 1.25 };
        let bounds = Bounds { x: 160., y: 120., w: 320., h: 240. };
        let rect = coordinates.screen_rect(bounds);
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (-1720, -930, 400, 300));
        assert_eq!(coordinates.workspace_point((rect.x, rect.y)), (bounds.x, bounds.y));
    }

    #[test]
    fn box_on_a_125_percent_monitor_plans_that_monitors_scaled_cells() {
        let monitor = |key: &str, x: i32, dpi: u32, primary: bool| host::Monitor {
            key: key.into(), rect: [x, 0, 2560, 1440], work: [x, 0, 2560, 1380], dpi, primary,
        };
        let monitors = [monitor("main", 0, 96, true), monitor("side", 2560, 120, false)];
        let screens = [Rect { x: 0, y: 0, w: 2560, h: 1440 }, Rect { x: 2560, y: 0, w: 2560, h: 1440 }];
        let coordinates = DesktopCoordinates { origin: (0, 0), scale: 1.0 };
        let icons = [(11, 2), (86, 2), (161, 2)].iter().enumerate().map(|(n, &pos)|
            ViewItem { path: format!("C:/Desktop/{n}.lnk"), label: format!("{n}"), pos }).collect::<Vec<_>>();
        let mut state = State::empty();
        let group = state.add_group("Side", Bounds { x: 2860., y: 200., w: 740., h: 416. });
        for (n, icon) in icons.iter().enumerate() {
            state.group_mut(group).unwrap().tab_mut().items.push(Item {
                id: 101 + n as u64, path: icon.path.clone(), name: icon.label.clone(),
                kind: "lnk".into(), size: 16, modified: 1, label: None,
            });
        }
        let member = NativeTwoBoxMembership::prepare_workspace(1, &state, &icons).unwrap();
        let (plans, _, _, _) = plan_layout(&member, &state, &icons, &icons, (75, 107), &screens,
            &monitors, coordinates, true, None, |_| 0).unwrap();
        assert_eq!(plans[0].cell, (94, 134));
        let cells = plans[0].moves.iter().map(|m| m.to).collect::<Vec<_>>();
        // Monitor origin (2560,0) + offset (14,2), step 94: first cell inside x>=2860.
        assert_eq!(cells, vec![(2950, 270), (3044, 270), (3138, 270)]);
    }

    #[test]
    fn missing_shortcut_stays_saved_and_does_not_add_an_explorer_page() {
        let mut state=State::empty();
        let bounds=Bounds{x:300.,y:200.,w:150.,h:180.};
        let group=state.add_group("Notes",bounds);
        let stamp=std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let missing=std::env::temp_dir()
            .join(format!("DesktopCraft-layout-{}-{stamp}",std::process::id()))
            .join("Old Notes.LNK");
        assert!(!missing.try_exists().unwrap());
        let pin=|id,path|Item{
            id,path,name:"Notes".into(),kind:"lnk".into(),size:16,
            modified:1,label:Some("Keep my label".into()),
        };
        let icons=[ViewItem{path:"C:/Desktop/Notes.lnk".into(),label:"Notes".into(),pos:(20,20)}];
        state.group_mut(group).unwrap().tab_mut().items.extend([
            pin(101,icons[0].path.clone()),pin(102,missing.to_str().unwrap().into()),
        ]);
        let saved=serde_json::to_value(&state).unwrap();
        let member=NativeTwoBoxMembership::prepare_workspace(1,&state,&icons).unwrap();
        let screens=[Rect{x:0,y:0,w:800,h:600}];
        let coordinates = DesktopCoordinates { origin: (0, 0), scale: 1.0 };
        let monitors=[host::Monitor{key:"main".into(),rect:[0,0,800,600],work:[0,0,800,560],
            dpi:96,primary:true}];
        let (plans,hidden,_,_)=plan_layout(&member,&state,&icons,&icons,(70,100),&screens,
            &monitors,coordinates, false, None, |_|0).unwrap();
        assert_eq!((plans[0].page_count,plans[0].moves.len()),(1,1));
        assert_eq!(plans[0].moves[0].path,icons[0].path);
        assert!(hidden.is_empty());
        assert_eq!(serde_json::to_value(&state).unwrap(),saved);

        let mut duplicate=icons.to_vec();
        duplicate.push(icons[0].clone());
        assert!(NativeTwoBoxMembership::prepare_workspace(2,&state,&duplicate).is_err());
        state.group_mut(group).unwrap().tab_mut().items[1].path=
            missing.with_extension("txt").to_str().unwrap().into();
        assert!(NativeTwoBoxMembership::prepare_workspace(2,&state,&icons).is_err());
    }

    #[test]
    fn two_grid_mode_allows_dragging_over_hidden_icons_and_parks_them_together() {
        let mut state = State::empty();
        // Box dropped right on top of (400, 400) where a hidden tab icon originally sat.
        let group = state.add_group("Programs", Bounds { x: 400., y: 400., w: 300., h: 260. });
        let g = state.group_mut(group).unwrap();
        g.tabs.push(crate::model::Tab::new(2, "Craft"));
        let pin = |id, path: &str, name: &str| Item {
            id, path: path.into(), name: name.into(), kind: "lnk".into(),
            size: 16, modified: 1, label: None,
        };
        g.tabs[0].items.push(pin(101, "C:/d/Active.lnk", "Active"));
        g.tabs[1].items.push(pin(102, "C:/d/Hidden.lnk", "Hidden"));
        let icons = [
            ViewItem { path: "C:/d/Active.lnk".into(), label: "Active".into(), pos: (100, 100) },
            ViewItem { path: "C:/d/Hidden.lnk".into(), label: "Hidden".into(), pos: (400, 400) },
            ViewItem { path: "C:/d/Loose.lnk".into(), label: "Loose".into(), pos: (11, 2) },
        ];
        let member = NativeTwoBoxMembership::prepare_workspace(1, &state, &icons).unwrap();
        let screens = [Rect { x: 0, y: 0, w: 2560, h: 1440 }];
        let monitors = [host::Monitor {
            key: "main".into(), rect: [0, 0, 2560, 1440], work: [0, 0, 2560, 1380],
            dpi: 96, primary: true,
        }];
        let coordinates = DesktopCoordinates { origin: (0, 0), scale: 1.0 };
        let (plans, hidden, park, _) = plan_layout(
            &member, &state, &icons, &icons, (75, 107), &screens, &monitors, coordinates, false, None, |_| 0,
        ).unwrap();
        // Visible icon aligns strictly to Box grid (400+22, 400+12) — zero Desktop-grid jump.
        assert_eq!(plans[0].moves[0].to, (422, 412));
        assert_eq!(hidden, vec![1]);
        let park = park.expect("hidden icon receives a free on-screen park point");
        assert!(screens[0].contains(park));
        assert_ne!(park, (422, 412));
        assert!(!native_box_placement::cells_overlap(park, (11, 2), (75, 107)));

        // plan_free_moves parks hidden first, then places visible in batch 2.
        let batches = plan_free_moves(
            &[],
            &[],
            &[("C:/d/Active.lnk".into(), (422, 412))],
            &[("C:/d/Hidden.lnk".into(), park)],
            &[],
            &icons,
            |_| None,
        ).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0], vec![("C:/d/Hidden.lnk".into(), park)]);
        assert_eq!(batches[1], vec![("C:/d/Active.lnk".into(), (422, 412))]);
    }

    #[test]
    fn software_loose_snap_aligns_to_desktop_grid_on_main_and_scaled_monitors() {
        let monitors = [
            host::Monitor {
                key: "main".into(), rect: [0, 0, 2560, 1440], work: [0, 0, 2560, 1380],
                dpi: 96, primary: true,
            },
            host::Monitor {
                key: "side".into(), rect: [2560, -1, 2560, 1440], work: [2560, -1, 2560, 1380],
                dpi: 120, primary: false,
            },
        ];
        // Drop near (95, 118) on main monitor (grid (11,2) + 75x107) -> snaps to (86, 109).
        let main_snap = snap_loose_to_desktop_grid(
            (95, 118), (11, 2), (75, 107), &monitors, &[(11, 2)], &[], None,
        );
        assert_eq!(main_snap, (86, 109));
        // If a Box covers (86, 109), snaps to a neighbouring clear Desktop grid cell.
        let avoid_box = snap_loose_to_desktop_grid(
            (95, 118), (11, 2), (75, 107), &monitors, &[(11, 2)],
            &[Rect { x: 80, y: 100, w: 300, h: 260 }], None,
        );
        assert_ne!(avoid_box, (86, 109));
        assert_eq!((avoid_box.0 - 11).rem_euclid(75), 0);
        assert_eq!((avoid_box.1 - 2).rem_euclid(107), 0);
        // On 125% monitor (grid origin (2574, 1) + 94x134), snaps onto the scaled grid.
        let side_snap = snap_loose_to_desktop_grid(
            (2670, 140), (11, 2), (75, 107), &monitors, &[], &[], None,
        );
        assert_eq!((side_snap.0 - 2574).rem_euclid(94), 0);
        assert_eq!((side_snap.1 - 1).rem_euclid(134), 0);
    }
}

/// Strict connected-monitor preflight. A stale icon can be offscreen
/// without using the usual -20000 parking coordinates. Native mode must
/// refuse that entire topology before any Explorer layout mutation.
pub(crate) fn offscreen_icon_count(items:&[ViewItem],screens:&[Rect])->usize{
    items.iter().filter(|item|{
        let (x,y)=item.pos;
        !screens.iter().any(|r|x>=r.x && x<r.x.saturating_add(r.w)
            && y>=r.y && y<r.y.saturating_add(r.h))
    }).count()
}

#[cfg(test)]
#[test]
pub(super) fn native_connected_monitor_guard_rejects_any_offscreen_icon(){
    let screens=[
        Rect{x:-2400,y:0,w:2400,h:1350},
        Rect{x:0,y:0,w:1920,h:1080},
    ];
    let icons=[
        ViewItem{path:"a".into(),label:"a".into(),pos:(-2300,45)},
        ViewItem{path:"b".into(),label:"b".into(),pos:(1830,950)},
        ViewItem{path:"c".into(),label:"c".into(),pos:(2100,1400)},
    ];
    assert_eq!(offscreen_icon_count(&icons[..2],&screens),0);
    assert_eq!(offscreen_icon_count(&icons,&screens),1);
}

pub(crate) fn current_screens()->Vec<Rect>{
    host::monitors().iter().map(|m|Rect{
        x:m.rect[0],y:m.rect[1],w:m.rect[2],h:m.rect[3]
    }).collect()
}
