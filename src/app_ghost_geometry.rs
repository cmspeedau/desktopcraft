use crate::*;

/// Scale Explorer's main-monitor icon cell for a monitor at `monitor_dpi`.
/// Matches `native_box_live_layout::scaled_grid` (ties-to-even rounding).
pub(super) fn ghost_cell_for_dpi(main_cell: (i32, i32), monitor_dpi: u32, primary_dpi: u32) -> (i32, i32) {
    if monitor_dpi == 0 || primary_dpi == 0 || monitor_dpi == primary_dpi {
        return main_cell;
    }
    let ratio = f64::from(monitor_dpi) / f64::from(primary_dpi);
    (
        (f64::from(main_cell.0) * ratio).round_ties_even() as i32,
        (f64::from(main_cell.1) * ratio).round_ties_even() as i32,
    )
}

/// Logical minimum Box `(w, h)` for a regular Native Desktop Box at the
/// live Explorer icon `cell` on its monitor and workspace `scale`.
/// Native headers float above `bounds.y` (clamped to `0..HEADER`), so
/// normal Boxes do not add `HEADER` to body height; top-edge Boxes
/// (`bounds_y < HEADER`) add just the clearance needed below the header.
pub(crate) fn native_box_min_size(cell: (i32, i32), scale: f32, bounds_y: f32) -> (f32, f32) {
    let scale = scale.max(0.5);
    let (phys_w, phys_h) = native_box_placement::native_box_min_body_phys(cell);
    let min_w = (phys_w as f32 / scale).ceil().max(95.0);
    let top_clearance = (HEADER - bounds_y).max(0.0);
    let min_h = (phys_h as f32 / scale).ceil() + top_clearance;
    (min_w, min_h)
}

pub(super) fn snap_native_row_height(bounds: Bounds, edges: u8, desktop_height: f32, scale: f32, cell: (i32, i32)) -> Bounds {
    if cell.1 <= 0 || scale <= 0. || edges & 12 == 0 { return bounds; }
    let bottom = bounds.y + bounds.h;
    let body = paint::native_body_bounds(bounds);
    let top_clearance = body.y - bounds.y;
    if edges & 4 != 0 && top_clearance > 0. { return bounds; }
    let minimum = native_box_placement::native_box_min_body_phys(cell).1 as f32;
    let available = if edges & 4 != 0 { bottom - HEADER } else { desktop_height - body.y };
    let pitch = cell.1 as f32;
    let max_extra = ((available * scale - minimum) / pitch).floor();
    if max_extra < 0. { return bounds; }
    let extra = ((body.h * scale - minimum) / pitch).round().clamp(0., max_extra);
    let h = (minimum + extra * pitch) / scale + top_clearance;
    let y = if edges & 4 != 0 { bottom - h } else { bounds.y };
    Bounds { y, h, ..bounds }
}

/// Physical icon size (px) on a monitor at `monitor_dpi` from Explorer's
/// logical icon size (e.g. 48 px at 96 DPI -> 60 px at 120 DPI; 96 px -> 120 px).
pub(super) fn ghost_icon_px(base_icon_size: i32, monitor_dpi: u32) -> f32 {
    (base_icon_size.clamp(16, 256) as f32 * monitor_dpi.max(96) as f32 / 96.0).round()
}

/// Whether ending a drag preview should ask Explorer to restore the pre-drag
/// hidden set (cancelled or refused drag, not an applied move or edge rollup).
pub(super) fn ghost_should_end_preview(current: Option<(Bounds, bool)>, origin: Bounds) -> bool {
    current.is_none_or(|(bounds, rolled)| bounds == origin && !rolled)
}

/// Physical shift that puts a dragged Box's icons on the Explorer grid cells
/// the in-Box planner gives them at the drop: the first cell is the first grid
/// point at or after the Box's top-left (native_box_placement::cell_layout).
pub(super) fn ghost_drop_shift(icons: &[impl GhostCell], cell: (i32, i32), new_box: [i32; 4])
    -> Option<(i32, i32)> {
    if cell.0 <= 0 || cell.1 <= 0 { return None; }
    let first = icons.iter().map(GhostCell::position)
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)))?;
    let align = |edge: i32, grid: i32, step: i32| {
        edge.saturating_add((i64::from(grid) - i64::from(edge)).rem_euclid(i64::from(step)) as i32)
    };
    let anchor = (align(new_box[0], first.0, cell.0), align(new_box[1], first.1, cell.1));
    Some((anchor.0 - first.0, anchor.1 - first.1))
}

/// Reflow row-major icon copies inside `new_box` during a Resize drag (or a
/// cross-DPI drag), matching `native_box_placement::cell_layout`.
pub(super) fn ghost_resize_positions(
    count: usize,
    grid_origin: Option<(i32, i32)>,
    cell: (i32, i32),
    new_box: [i32; 4],
    first_row: usize,
    half_icon: Option<i32>,
) -> Vec<(usize, (i32, i32))> {
    if count == 0 || cell.0 <= 0 || cell.1 <= 0 || new_box[2] <= 14 || new_box[3] <= 14 {
        return Vec::new();
    }
    let align = |edge: i32, origin: i32, step: i32| {
        edge.saturating_add((i64::from(origin) - i64::from(edge)).rem_euclid(i64::from(step)) as i32)
    };
    let anchor = match grid_origin {
        Some((ox, oy)) => (align(new_box[0], ox, cell.0), align(new_box[1], oy, cell.1)),
        None => {
            let (pad_x, pad_y) = native_box_placement::box_grid_padding(
                native_box_placement::Rect {
                    x: new_box[0],
                    y: new_box[1],
                    w: new_box[2],
                    h: new_box[3],
                },
                cell,
            );
            (new_box[0].saturating_add(pad_x), new_box[1].saturating_add(pad_y))
        }
    };
    let cols = ((i64::from(new_box[0]) + i64::from(new_box[2]) - 14 - i64::from(anchor.0))
        / i64::from(cell.0))
        .max(0) as usize;
    let rows = ((i64::from(new_box[1]) + i64::from(new_box[3]) - 14 - i64::from(anchor.1))
        / i64::from(cell.1))
        .max(0) as usize;
    if cols == 0 { return Vec::new(); }
    let first_row = first_row.min(count.div_ceil(cols).saturating_sub(rows.max(1)));
    let first = first_row * cols;
    let next_top = i64::from(anchor.1) + rows as i64 * i64::from(cell.1);
    let bottom = i64::from(new_box[1]) + i64::from(new_box[3]);
    let partial = half_icon.is_some_and(|half| bottom - next_top >= i64::from(half));
    let visible = (count - first).min(cols.saturating_mul(rows + usize::from(partial)));
    (0..visible)
        .map(|i| (first + i, (anchor.0 + (i % cols) as i32 * cell.0,
            anchor.1 + (i / cols) as i32 * cell.1)))
        .collect()
}

/// Compute screen pixel positions for each visible ghost icon during Move
/// (live drag or snapped drop, same-DPI or cross-DPI) and Resize drags.
#[allow(clippy::too_many_arguments)]
pub(super) fn ghost_icon_positions(
    icons: &[impl GhostCell],
    resizing: bool,
    dropped: bool,
    snap_to_grid: bool,
    origin_cell: (i32, i32),
    target_cell: (i32, i32),
    drag_delta_phys: (i32, i32),
    new_box: [i32; 4],
) -> Vec<(usize, (i32, i32))> {
    if icons.is_empty() {
        return Vec::new();
    }
    let first = icons.iter().map(GhostCell::position)
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1)));
    let cross_dpi = origin_cell != target_cell;
    if resizing || cross_dpi {
        let origin = if cross_dpi || !snap_to_grid { None } else { first };
        return ghost_resize_positions(icons.len(), origin, target_cell, new_box, 0, None);
    }
    let (dx, dy) = if dropped && snap_to_grid {
        ghost_drop_shift(icons, target_cell, new_box).unwrap_or(drag_delta_phys)
    } else {
        drag_delta_phys
    };
    icons
        .iter()
        .enumerate()
        .map(|(i, icon)| {
            let p = icon.position();
            (i, (p.0.saturating_add(dx), p.1.saturating_add(dy)))
        })
        .collect()
}

