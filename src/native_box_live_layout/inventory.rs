use super::*;

pub(super) fn monitor_for_point(point: (i32, i32), monitors: &[host::Monitor]) -> Option<&host::Monitor> {
    monitors.iter().find(|m| monitor_rect(m).contains(point))
        .or_else(|| monitors.iter().find(|m| m.primary))
        .or_else(|| monitors.first())
}

pub(super) fn monitor_loose_grid(
    monitor: &host::Monitor,
    main: &host::Monitor,
    desktop_origin: (i32, i32),
    main_cell: (i32, i32),
) -> ((i32, i32), (i32, i32), Rect) {
    let screen = monitor_rect(monitor);
    let main_screen = monitor_rect(main);
    let (cell, origin) = if monitor.dpi != main.dpi && main.dpi > 0 && monitor.dpi > 0 {
        let ratio = f64::from(monitor.dpi) / f64::from(main.dpi);
        let main_offset = (
            (desktop_origin.0 - main_screen.x).rem_euclid(main_cell.0.max(1)),
            (desktop_origin.1 - main_screen.y).rem_euclid(main_cell.1.max(1)),
        );
        scaled_grid(main_offset, main_cell, screen, ratio)
    } else {
        (main_cell, desktop_origin)
    };
    let cell = (cell.0.max(1), cell.1.max(1));
    let work = if monitor.work[2] > 0 && monitor.work[3] > 0 {
        Rect { x: monitor.work[0], y: monitor.work[1], w: monitor.work[2], h: monitor.work[3] }
    } else {
        screen
    };
    (cell, origin, work)
}

pub(super) fn find_clear_desktop_cell_on_monitor(
    desired: (i32, i32),
    monitor: &host::Monitor,
    main: &host::Monitor,
    desktop_origin: (i32, i32),
    main_cell: (i32, i32),
    occupied: &[((i32, i32), (i32, i32))],
    active_boxes: &[(Rect, (i32, i32))],
    hidden_park: Option<(i32, i32)>,
    max_radius_cap: Option<i32>,
) -> Option<((i32, i32), (i32, i32), i64, (i32, i32))> {
    let ((sx, sy), origin, work) = monitor_loose_grid(monitor, main, desktop_origin, main_cell);
    let phase_x = (origin.0 - work.x).rem_euclid(sx);
    let phase_y = (origin.1 - work.y).rem_euclid(sy);
    let max_col = ((work.w - sx - phase_x).max(0) / sx).max(0);
    let max_row = ((work.h - sy - phase_y).max(0) / sy).max(0);
    let base_col = ((f64::from(desired.0 - work.x - phase_x) / f64::from(sx)).round() as i32)
        .clamp(0, max_col);
    let base_row = ((f64::from(desired.1 - work.y - phase_y) / f64::from(sy)).round() as i32)
        .clamp(0, max_row);
    let fallback = (work.x + phase_x + base_col * sx, work.y + phase_y + base_row * sy);
    let point = |col: i32, row: i32| (work.x + phase_x + col * sx, work.y + phase_y + row * sy);
    let clear = |at: (i32, i32)| {
        !occupied.iter().any(|&(p, c)| {
            native_box_placement::cells_overlap(p, at, (sx.max(c.0), sy.max(c.1)))
        })
            && !hidden_park.is_some_and(|p| {
                native_box_placement::cells_overlap(p, at, (sx.max(main_cell.0), sy.max(main_cell.1)))
            })
            && !active_boxes.iter().any(|&(b, bc)| {
                native_box_placement::cell_overlaps_rect(at, (sx.max(bc.0), sy.max(bc.1)), b)
            })
    };
    let full_radius = base_col
        .max(max_col - base_col)
        .max(base_row)
        .max(max_row - base_row);
    let limit = max_radius_cap.map_or(full_radius, |cap| cap.min(full_radius));
    for radius in 0i32..=limit {
        let mut best = None;
        let mut best_dist = i64::MAX;
        for row in (base_row - radius).max(0)..=(base_row + radius).min(max_row) {
            for col in (base_col - radius).max(0)..=(base_col + radius).min(max_col) {
                if (col - base_col).abs().max((row - base_row).abs()) != radius {
                    continue;
                }
                let at = point(col, row);
                if !clear(at) {
                    continue;
                }
                let dx = i64::from(at.0 - desired.0);
                let dy = i64::from(at.1 - desired.1);
                let d2 = dx * dx + dy * dy;
                if d2 < best_dist {
                    best_dist = d2;
                    best = Some(at);
                }
            }
        }
        if let Some(at) = best {
            return Some((at, (sx, sy), best_dist, fallback));
        }
    }
    None
}

pub(super) fn find_clear_desktop_cell_across_monitors(
    desired: (i32, i32),
    desktop_origin: (i32, i32),
    main_cell: (i32, i32),
    monitors: &[host::Monitor],
    occupied: &[((i32, i32), (i32, i32))],
    active_boxes: &[(Rect, (i32, i32))],
) -> Result<((i32, i32), (i32, i32)), String> {
    let Some(home) = monitor_for_point(desired, monitors) else {
        return Err("No connected monitors available to nudge covered loose icon".into());
    };
    let main = monitors.iter().find(|m| m.primary).unwrap_or(home);
    if let Some((at, cell, _, _)) = find_clear_desktop_cell_on_monitor(
        desired, home, main, desktop_origin, main_cell, occupied, active_boxes, None, None,
    ) {
        return Ok((at, cell));
    }
    let mut best_other: Option<((i32, i32), (i32, i32), i64)> = None;
    for monitor in monitors {
        if monitor.key == home.key {
            continue;
        }
        if let Some((at, cell, d2, _)) = find_clear_desktop_cell_on_monitor(
            desired, monitor, main, desktop_origin, main_cell, occupied, active_boxes, None, None,
        ) {
            if best_other.is_none_or(|(_, _, best_d2)| d2 < best_d2) {
                best_other = Some((at, cell, d2));
            }
        }
    }
    best_other
        .map(|(at, cell, _)| (at, cell))
        .ok_or_else(|| "No free Desktop grid cell available to nudge covered loose icon".to_string())
}

pub(super) fn group_native_bounds(group: &crate::model::Group) -> crate::model::Bounds {
    if group.tabs.get(group.active).is_some_and(|t| t.folder.is_none()) {
        crate::paint::native_group_body(group)
    } else {
        crate::paint::native_group_outer(group)
    }
}

pub(super) fn active_boxes_with_cells(
    state: &State,
    coordinates: DesktopCoordinates,
    monitors: &[host::Monitor],
    main_cell: (i32, i32),
    desktop_origin: (i32, i32),
) -> Vec<(Rect, (i32, i32))> {
    let main = monitors.iter().find(|m| m.primary).or_else(|| monitors.first());
    state.groups.iter()
        .filter(|g| g.page == state.page && !g.rolled)
        .map(|g| {
            let rect = coordinates.screen_rect(group_native_bounds(g));
            let anchor = (rect.x.saturating_add(rect.w / 2), rect.y.saturating_add(rect.h / 2));
            let cell = monitor_for_point(anchor, monitors)
                .zip(main)
                .map_or(main_cell, |(m, main_m)| {
                    monitor_loose_grid(m, main_m, desktop_origin, main_cell).0
                });
            (rect, cell)
        })
        .collect()
}

/// Auto-nudge Desktop placement: when an active Native Box (or expanded
/// Folder Portal) covers visible loose Desktop icons, automatically nudge them
/// onto the nearest free Desktop grid cells instead of blocking Box placement.
#[allow(clippy::too_many_arguments)]
pub(super) fn plan_nudged_loose_icons(
    original: &[ViewItem],
    box_owned_paths: &BTreeSet<String>,
    state: &State,
    main_cell: (i32, i32),
    monitors: &[host::Monitor],
    coordinates: DesktopCoordinates,
    desktop_grid_origin: Option<(i32, i32)>,
) -> Result<(Vec<Move>, Vec<(i32, i32)>), String> {
    let Some(main) = monitors.iter().find(|m| m.primary).or_else(|| monitors.first()) else {
        return Err("No connected monitors available for loose icon layout".into());
    };
    let origin = desktop_grid_origin
        .or_else(|| {
            let positions = original.iter().map(|i| i.pos).collect::<Vec<_>>();
            grid_origin_for_screen(monitor_rect(main), &positions, main_cell).ok()
        })
        .unwrap_or_else(|| {
            if main.work[2] > 0 && main.work[3] > 0 {
                (main.work[0] + 11, main.work[1] + 2)
            } else {
                (main.rect[0] + 11, main.rect[1] + 2)
            }
        });
    let active_boxes = active_boxes_with_cells(state, coordinates, monitors, main_cell, origin);
    let cell_at = |pos: (i32, i32)| {
        monitor_for_point(pos, monitors)
            .map_or(main_cell, |m| monitor_loose_grid(m, main, origin, main_cell).0)
    };
    let mut loose_items = original.iter()
        .filter(|item| !box_owned_paths.contains(&item.path.to_lowercase()))
        .collect::<Vec<_>>();
    loose_items.sort_by(|a, b| {
        (a.pos.1, a.pos.0, a.path.to_lowercase()).cmp(&(b.pos.1, b.pos.0, b.path.to_lowercase()))
    });

    let mut occupied_cells = Vec::new();
    let mut final_loose_positions = Vec::with_capacity(loose_items.len());
    let mut to_nudge = Vec::new();
    for item in loose_items {
        let l_cell = cell_at(item.pos);
        let covered_by_box = active_boxes.iter().any(|&(rect, b_cell)| {
            native_box_placement::cell_overlaps_rect(
                item.pos,
                (l_cell.0.max(b_cell.0), l_cell.1.max(b_cell.1)),
                rect,
            )
        });
        let collides_uncovered = occupied_cells.iter().any(|&(occ_pos, occ_cell): &((i32, i32), (i32, i32))| {
            native_box_placement::cells_overlap(
                item.pos,
                occ_pos,
                (l_cell.0.max(occ_cell.0), l_cell.1.max(occ_cell.1)),
            )
        });
        if covered_by_box || collides_uncovered {
            to_nudge.push(item);
        } else {
            occupied_cells.push((item.pos, l_cell));
            final_loose_positions.push(item.pos);
        }
    }
    if to_nudge.is_empty() {
        return Ok((Vec::new(), final_loose_positions));
    }

    let mut occupied_with_reserved = occupied_cells.clone();
    for item in original {
        if box_owned_paths.contains(&item.path.to_lowercase()) {
            occupied_with_reserved.push((item.pos, cell_at(item.pos)));
        }
    }
    let mut nudged_moves = Vec::with_capacity(to_nudge.len());
    for item in to_nudge {
        let (dest, dest_cell) = find_clear_desktop_cell_across_monitors(
            item.pos, origin, main_cell, monitors, &occupied_with_reserved, &active_boxes,
        ).or_else(|_| {
            find_clear_desktop_cell_across_monitors(
                item.pos, origin, main_cell, monitors, &occupied_cells, &active_boxes,
            )
        })?;
        occupied_cells.push((dest, dest_cell));
        occupied_with_reserved.push((dest, dest_cell));
        final_loose_positions.push(dest);
        if dest != item.pos {
            nudged_moves.push((item.path.clone(), dest));
        }
    }
    Ok((nudged_moves, final_loose_positions))
}

/// Software Align-to-Grid for a loose Desktop icon dropped outside Boxes while
/// Explorer's global Align-to-Grid is paused for Box-grid freedom. Matches the
/// monitor's Desktop grid (scaled by DPI ratio on secondary scaled monitors)
/// and avoids active Boxes, other loose icons, and the hidden parking cell.
pub(super) fn snap_loose_to_desktop_grid(
    desired: (i32, i32),
    desktop_origin: (i32, i32),
    main_cell: (i32, i32),
    monitors: &[host::Monitor],
    occupied_loose: &[(i32, i32)],
    active_boxes: &[Rect],
    hidden_park: Option<(i32, i32)>,
) -> (i32, i32) {
    let Some(monitor) = monitor_for_point(desired, monitors) else { return desired };
    let main = monitors.iter().find(|m| m.primary).unwrap_or(monitor);
    let (cell, origin, work) = monitor_loose_grid(monitor, main, desktop_origin, main_cell);
    let occupied = occupied_loose.iter().map(|&p| (p, cell)).collect::<Vec<_>>();
    let boxes = active_boxes.iter().map(|&r| (r, cell)).collect::<Vec<_>>();
    if let Some((at, _, _, _)) = find_clear_desktop_cell_on_monitor(
        desired, monitor, main, desktop_origin, main_cell, &occupied, &boxes, hidden_park, Some(12),
    ) {
        return at;
    }
    let (sx, sy) = cell;
    let phase_x = (origin.0 - work.x).rem_euclid(sx);
    let phase_y = (origin.1 - work.y).rem_euclid(sy);
    let max_col = ((work.w - sx - phase_x).max(0) / sx).max(0);
    let max_row = ((work.h - sy - phase_y).max(0) / sy).max(0);
    let base_col = ((f64::from(desired.0 - work.x - phase_x) / f64::from(sx)).round() as i32)
        .clamp(0, max_col);
    let base_row = ((f64::from(desired.1 - work.y - phase_y) / f64::from(sy)).round() as i32)
        .clamp(0, max_row);
    (work.x + phase_x + base_col * sx, work.y + phase_y + base_row * sy)
}

/// Record an Explorer-owned position change without changing any app-placed
/// icon. Keep the latest loose position as its restore point if later pinned.
pub(super) fn adopt_loose_positions(
    original: &mut [ViewItem], expected: &mut [ViewItem],
    placed: &[(String, (i32, i32))], current: &[ViewItem],
) -> Result<bool, String> {
    adopt_loose_positions_tracking(original, expected, placed, current).map(|v| !v.is_empty())
}

#[derive(Debug, Clone)]
pub(crate) struct InventoryReconcileOutcome {
    pub added: Vec<ViewItem>,
    pub removed: Vec<String>,
    pub moved_loose: Vec<(String, (i32, i32))>,
}

/// Reconcile live Explorer Desktop inventory changes (added or deleted files/links
/// plus user-dragged loose icons) while keeping strict protection on every
/// surviving app-placed Box icon.
pub(crate) fn reconcile_desktop_inventory(
    original: &mut Vec<ViewItem>,
    expected: &mut Vec<ViewItem>,
    placed: &mut Vec<(String, (i32, i32))>,
    nudged_loose: &mut Vec<(String, (i32, i32))>,
    current: &[ViewItem],
) -> Result<InventoryReconcileOutcome, String> {
    if same_icon_positions(expected, current) {
        return Ok(InventoryReconcileOutcome {
            added: Vec::new(),
            removed: Vec::new(),
            moved_loose: Vec::new(),
        });
    }
    if current.is_empty() || current.len() > 2048 {
        return Err("Explorer icon inventory outside verified bounds".into());
    }
    let mut current_by_path = BTreeMap::new();
    for item in current {
        if item.path.trim().is_empty() || item.label.trim().is_empty()
            || current_by_path.insert(item.path.to_lowercase(), item).is_some()
        {
            return Err("Explorer icon identities invalid or duplicated during live reconcile".into());
        }
    }
    let placed_paths = placed.iter()
        .map(|(path, _)| path.to_lowercase())
        .collect::<BTreeSet<_>>();
    let mut expected_keys = BTreeSet::new();
    let mut removed = Vec::new();
    let mut moved_loose = Vec::new();
    for icon in expected.iter() {
        let key = icon.path.to_lowercase();
        expected_keys.insert(key.clone());
        match current_by_path.get(&key) {
            Some(live) => {
                if live.label != icon.label {
                    return Err("Explorer icon label changed".into());
                }
                if placed_paths.contains(&key) && live.pos != icon.pos {
                    return Err("An app-placed Explorer icon moved outside the layout transaction".into());
                }
                if !placed_paths.contains(&key) && live.pos != icon.pos {
                    moved_loose.push((icon.path.clone(), live.pos));
                }
            }
            None => removed.push(icon.path.clone()),
        }
    }
    if removed.len() > 16 || (expected.len() >= 4 && removed.len() * 2 >= expected.len()) {
        return Err("Too many Explorer icons disappeared at once".into());
    }
    let mut added = current.iter()
        .filter(|item| !expected_keys.contains(&item.path.to_lowercase()))
        .cloned()
        .collect::<Vec<_>>();
    added.sort_by(|a, b| (a.pos.1, a.pos.0, a.path.to_lowercase()).cmp(&(b.pos.1, b.pos.0, b.path.to_lowercase())));

    if !removed.is_empty() {
        let removed_keys = removed.iter().map(|p| p.to_lowercase()).collect::<BTreeSet<_>>();
        placed.retain(|(p, _)| !removed_keys.contains(&p.to_lowercase()));
        nudged_loose.retain(|(p, _)| !removed_keys.contains(&p.to_lowercase()));
        original.retain(|i| !removed_keys.contains(&i.path.to_lowercase()));
        expected.retain(|i| !removed_keys.contains(&i.path.to_lowercase()));
    }
    let remaining_placed = placed.iter()
        .map(|(path, _)| path.to_lowercase())
        .collect::<BTreeSet<_>>();
    for icon in original.iter_mut() {
        let key = icon.path.to_lowercase();
        if !remaining_placed.contains(&key) {
            if let Some(live) = current_by_path.get(&key) {
                icon.pos = live.pos;
            }
        }
    }
    for icon in expected.iter_mut() {
        if let Some(live) = current_by_path.get(&icon.path.to_lowercase()) {
            icon.pos = live.pos;
        }
    }
    for (path, _) in &moved_loose {
        nudged_loose.retain(|(nudged_path, _)| !nudged_path.eq_ignore_ascii_case(path));
    }
    original.extend(added.iter().cloned());
    expected.extend(added.iter().cloned());
    Ok(InventoryReconcileOutcome { added, removed, moved_loose })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compute_loose_drop_targets(
    targets: &[(String, (i32, i32))],
    original_snap_to_grid: bool,
    snap_to_grid: bool,
    desktop_origin: Option<(i32, i32)>,
    main_cell: (i32, i32),
    monitors: &[host::Monitor],
    expected: &[ViewItem],
    placed: &[(String, (i32, i32))],
    active_boxes: &[Rect],
    hidden_park: Option<(i32, i32)>,
) -> Vec<(String, (i32, i32))> {
    if targets.is_empty() {
        return Vec::new();
    }
    let placed_keys = placed.iter()
        .map(|(p, _)| p.to_lowercase())
        .collect::<BTreeSet<_>>();
    let target_keys = targets.iter()
        .map(|(p, _)| p.to_lowercase())
        .collect::<BTreeSet<_>>();
    let mut occupied_loose = expected.iter()
        .filter(|icon| {
            let key = icon.path.to_lowercase();
            !placed_keys.contains(&key) && !target_keys.contains(&key)
        })
        .map(|icon| icon.pos)
        .collect::<Vec<_>>();
    let mut out = Vec::with_capacity(targets.len());
    for (path, raw_pos) in targets {
        if !monitors.iter().any(|m| monitor_rect(m).contains(*raw_pos)) {
            continue;
        }
        let chosen = if original_snap_to_grid && !snap_to_grid {
            if let Some(origin) = desktop_origin {
                snap_loose_to_desktop_grid(
                    *raw_pos,
                    origin,
                    main_cell,
                    monitors,
                    &occupied_loose,
                    active_boxes,
                    hidden_park,
                )
            } else {
                *raw_pos
            }
        } else {
            *raw_pos
        };
        occupied_loose.push(chosen);
        out.push((path.clone(), chosen));
    }
    out
}

pub(super) fn adopt_loose_positions_tracking(
    original: &mut [ViewItem], expected: &mut [ViewItem],
    placed: &[(String, (i32, i32))], current: &[ViewItem],
) -> Result<Vec<(String, (i32, i32))>, String> {
    if same_icon_positions(expected, current) {
        return Ok(Vec::new());
    }
    let current_by_path = current.iter().map(|item| (item.path.to_lowercase(), item))
        .collect::<BTreeMap<_, _>>();
    if current_by_path.len() != current.len() || current.len() != expected.len() {
        return Err("Explorer icon inventory changed".into());
    }
    let placed_paths = placed.iter().map(|(path, _)| path.to_lowercase()).collect::<BTreeSet<_>>();
    let mut moved_loose = Vec::new();
    for icon in expected.iter() {
        let key = icon.path.to_lowercase();
        let live = current_by_path.get(&key).ok_or("Explorer icon identity changed")?;
        if live.label != icon.label {
            return Err("Explorer icon label changed".into());
        }
        if placed_paths.contains(&key) && live.pos != icon.pos {
            return Err("An app-placed Explorer icon moved outside the layout transaction".into());
        }
        if !placed_paths.contains(&key) && live.pos != icon.pos {
            moved_loose.push((icon.path.clone(), live.pos));
        }
    }
    for icon in original.iter_mut() {
        let key = icon.path.to_lowercase();
        if !placed_paths.contains(&key) {
            icon.pos = current_by_path[&key].pos;
        }
    }
    for icon in expected.iter_mut() {
        icon.pos = current_by_path[&icon.path.to_lowercase()].pos;
    }
    Ok(moved_loose)
}

pub(super) fn nearby_box_cell_repairs(
    expected: &[ViewItem],
    current: &[ViewItem],
    placed: &[Move],
    parked: &BTreeSet<String>,
    active_boxes: &[Rect],
) -> Vec<String> {
    // A partial-shell move within the same cell can follow a short drag or
    // a redraw. Restore only that cell, never adopt a different assignment.
    const MAX_CELL_DRIFT_PX: u32 = 8;
    let live: BTreeMap<_, _> = current.iter()
        .map(|item| (item.path.to_lowercase(), item)).collect();
    if current.len() != expected.len() || live.len() != current.len()
        || expected.iter().any(|item| {
            live.get(&item.path.to_lowercase()).is_none_or(|now| now.label != item.label)
        }) {
        return Vec::new();
    }
    placed.iter().filter_map(|(path, target)| {
        let key = path.to_lowercase();
        let from = live.get(&key)?.pos;
        if parked.contains(&key) || from == *target
            || !expected.iter().any(|item| item.path.eq_ignore_ascii_case(path) && item.pos == *target)
            || from.0.abs_diff(target.0) > MAX_CELL_DRIFT_PX
            || from.1.abs_diff(target.1) > MAX_CELL_DRIFT_PX
            || !active_boxes.iter().any(|bounds| bounds.contains(from) && bounds.contains(*target))
            || current.iter().any(|item| item.pos == *target && !item.path.eq_ignore_ascii_case(path)) {
            return None;
        }
        Some(path.clone())
    }).collect()
}

/// Adopt Explorer's post-zoom coordinates across all icons when the desktop
/// icon size or spacing changes (e.g. Ctrl + Mouse Wheel), preserving original
/// pre-session baselines for icons already owned by Boxes or nudged aside.
pub(crate) fn adopt_spacing_positions(
    original: &mut [ViewItem],
    expected: &mut [ViewItem],
    moved: &mut [(String, (i32, i32))],
    nudged_loose: &mut [(String, (i32, i32))],
    current: &[ViewItem],
) -> Result<(), String> {
    if current.is_empty() || current.len() > 2048 || current.len() != expected.len() {
        return Err("Explorer icon count changed during icon spacing transition".into());
    }
    let mut current_by_path = BTreeMap::new();
    for item in current {
        if item.path.trim().is_empty()
            || item.label.trim().is_empty()
            || current_by_path.insert(item.path.to_lowercase(), item).is_some()
        {
            return Err("Explorer icon identities invalid or duplicated during spacing update".into());
        }
    }
    for icon in expected.iter() {
        let live = current_by_path
            .get(&icon.path.to_lowercase())
            .ok_or("Explorer icon identity missing during spacing update")?;
        if live.label != icon.label {
            return Err("Explorer icon label changed during spacing update".into());
        }
    }
    let managed_keys = moved
        .iter()
        .chain(nudged_loose.iter())
        .map(|(path, _)| path.to_lowercase())
        .collect::<BTreeSet<_>>();
    for icon in expected.iter_mut() {
        icon.pos = current_by_path[&icon.path.to_lowercase()].pos;
    }
    for (path, pos) in moved.iter_mut().chain(nudged_loose.iter_mut()) {
        if let Some(live) = current_by_path.get(&path.to_lowercase()) {
            *pos = live.pos;
        }
    }
    for icon in original.iter_mut() {
        let key = icon.path.to_lowercase();
        if !managed_keys.contains(&key) {
            if let Some(live) = current_by_path.get(&key) {
                icon.pos = live.pos;
            }
        }
    }
    Ok(())
}
