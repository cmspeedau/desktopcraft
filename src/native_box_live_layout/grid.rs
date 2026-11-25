use super::*;

/// DesktopCraft saves coordinates relative to its virtual-desktop HWND.
/// Explorer placement and pointer samples use physical screen coordinates.
#[derive(Clone, Copy)]
pub(crate) struct DesktopCoordinates {
    pub origin: (i32, i32),
    pub scale: f32,
}
impl DesktopCoordinates {
    pub(crate) fn current() -> Result<Self, String> {
        let monitors = host::monitors();
        Self::from_monitors(&monitors)
    }

    pub(crate) fn from_monitors(monitors: &[host::Monitor]) -> Result<Self, String> {
        let primary = monitors.iter().find(|monitor| monitor.primary)
            .or_else(|| monitors.first()).ok_or("Desktop monitor geometry unavailable")?;
        let [x, y, _, _] = host::virtual_rect();
        Ok(Self { origin: (x, y), scale: primary.dpi as f32 / 96. })
    }

    pub(crate) fn screen_rect(self, bounds: Bounds) -> Rect {
        Rect {
            x: self.origin.0 + (bounds.x * self.scale).round() as i32,
            y: self.origin.1 + (bounds.y * self.scale).round() as i32,
            w: (bounds.w * self.scale).round() as i32,
            h: (bounds.h * self.scale).round() as i32,
        }
    }

    pub(crate) fn workspace_point(self, point: (i32, i32)) -> (f32, f32) {
        (
            (point.0 as f32 - self.origin.0 as f32) / self.scale,
            (point.1 as f32 - self.origin.1 as f32) / self.scale,
        )
    }

    pub(crate) fn active_box_rects(self, state: &State) -> Vec<Rect> {
        state.groups.iter().filter(|g| g.page == state.page && !g.rolled)
            .map(|group| self.screen_rect(group_native_bounds(group)))
            .collect()
    }
}

pub(super) fn observed_grid_origin(
    screen: Rect, positions: impl Iterator<Item = (i32, i32)>, cell: (i32, i32),
) -> Result<(i32, i32), String> {
    if cell.0 <= 0 || cell.1 <= 0 {
        return Err("Invalid Explorer grid spacing".into());
    }
    let mut cells = positions.filter(|point| screen.contains(*point))
        .map(|point| (point.0.rem_euclid(cell.0), point.1.rem_euclid(cell.1)));
    let origin = cells.next().ok_or("No Explorer icons to verify the grid on this monitor")?;
    if cells.next() != Some(origin) || cells.any(|cell| cell != origin) {
        return Err("Explorer icon positions do not verify one grid on this monitor".into());
    }
    Ok(origin)
}

/// Explorer snaps every desktop icon inside ONE desktop ListView that spans
/// all monitors. A monitor holding no icons uses the grid that the icons on
/// the other monitors verify. If Explorer snaps differently there, the exact
/// position check after the move refuses the generation and restores icons.
pub(super) fn grid_origin_for_screen(
    screen: Rect, positions: &[(i32, i32)], cell: (i32, i32),
) -> Result<(i32, i32), String> {
    if positions.iter().any(|point| screen.contains(*point)) {
        return observed_grid_origin(screen, positions.iter().copied(), cell);
    }
    let all = positions.iter().fold(None::<Rect>, |area, &(x, y)| Some(match area {
        None => Rect { x, y, w: 1, h: 1 },
        Some(r) => {
            let (left, top) = (r.x.min(x), r.y.min(y));
            let (right, bottom) = ((r.x + r.w).max(x + 1), (r.y + r.h).max(y + 1));
            Rect { x: left, y: top, w: right - left, h: bottom - top }
        }
    })).ok_or("No Explorer icons to verify the desktop grid")?;
    observed_grid_origin(all, positions.iter().copied(), cell)
        .map_err(|_| "Explorer icon positions do not verify one desktop grid".to_string())
}

/// The icon grid a Box lays its Explorer icons on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BoxGrid {
    pub(super) screen: Rect,
    pub(super) cell: (i32, i32),
    /// A point on the grid; `None` when Explorer's snap to grid is off.
    pub(super) origin: Option<(i32, i32)>,
}

pub(super) fn monitor_rect(m: &host::Monitor) -> Rect {
    Rect { x: m.rect[0], y: m.rect[1], w: m.rect[2], h: m.rect[3] }
}

/// Explorer keeps one icon grid per monitor. On a monitor whose DPI differs
/// from the main monitor's, both the cell and the grid's offset from the
/// monitor's top-left corner are the main monitor's scaled by the DPI ratio.
/// Explorer re-snaps icons placed off that grid about a second later and
/// pushes colliding ones down (2026-11-19, 125% monitor: cells 94×134 from
/// (2574, 1) while the 100% main grid was 75×107 from (11, 2)).
pub(super) fn scaled_grid(main_offset: (i32, i32), cell: (i32, i32), monitor: Rect, ratio: f64)
    -> ((i32, i32), (i32, i32)) {
    let scale = |v: i32| (f64::from(v) * ratio).round_ties_even() as i32;
    let cell = (scale(cell.0), scale(cell.1));
    let origin = (monitor.x + scale(main_offset.0), monitor.y + scale(main_offset.1));
    (cell, origin)
}

/// Explorer scales the effective Desktop icon cell on monitors whose DPI
/// differs from the primary monitor (e.g. `(75, 107)` -> `(94, 134)` at 125%).
pub(crate) fn cell_for_point(
    point: (i32, i32),
    main_cell: (i32, i32),
    monitors: &[host::Monitor],
) -> (i32, i32) {
    let Some(monitor) = monitor_for_point(point, monitors) else {
        return main_cell;
    };
    let main = monitors.iter().find(|m| m.primary).unwrap_or(monitor);
    if monitor.dpi == main.dpi || main.dpi == 0 || monitor.dpi == 0 {
        return main_cell;
    }
    let ratio = f64::from(monitor.dpi) / f64::from(main.dpi);
    let scale = |v: i32| ((f64::from(v) * ratio).round_ties_even() as i32).max(1);
    (scale(main_cell.0), scale(main_cell.1))
}

pub(super) fn box_grid(
    bounds: Rect, original: &[ViewItem], monitors: &[host::Monitor], cell: (i32,i32),
    snap_to_grid: bool,
) -> Result<BoxGrid, String> {
    let anchor = (bounds.x.saturating_add(bounds.w / 2), bounds.y.saturating_add(bounds.h / 2));
    let monitor = monitors.iter().find(|m| monitor_rect(m).contains(anchor))
        .ok_or("Native Box grid anchor outside connected monitors")?;
    let main = monitors.iter().find(|m| m.primary).unwrap_or(monitor);
    let screen = monitor_rect(monitor);
    let positions = original.iter().map(|item| item.pos).collect::<Vec<_>>();
    if monitor.dpi == main.dpi || main.dpi == 0 {
        let origin = snap_to_grid.then(|| grid_origin_for_screen(screen, &positions, cell))
            .transpose()?;
        return Ok(BoxGrid { screen, cell, origin });
    }
    let ratio = f64::from(monitor.dpi) / f64::from(main.dpi);
    let main_screen = monitor_rect(main);
    let main_offset = if snap_to_grid {
        let origin = grid_origin_for_screen(main_screen, &positions, cell)?;
        ((origin.0 - main_screen.x).rem_euclid(cell.0), (origin.1 - main_screen.y).rem_euclid(cell.1))
    } else { (0, 0) };
    let (scaled_cell, derived) = scaled_grid(main_offset, cell, screen, ratio);
    let origin = if !snap_to_grid {
        None
    } else if let Some(first) = positions.iter().find(|p| screen.contains(**p)) {
        // Icons already on that monitor show its real grid; one is enough,
        // but every icon there must agree.
        let residue = |p: &(i32, i32)| (p.0.rem_euclid(scaled_cell.0), p.1.rem_euclid(scaled_cell.1));
        if positions.iter().filter(|p| screen.contains(**p)).any(|p| residue(p) != residue(first)) {
            return Err("Explorer icon positions do not verify one grid on this monitor".into());
        }
        Some(*first)
    } else {
        Some(derived)
    };
    Ok(BoxGrid { screen, cell: scaled_cell, origin })
}

#[cfg(test)]
mod scaled_monitor_grid_tests {
    use super::*;
    fn monitor(key: &str, x: i32, y: i32, dpi: u32, primary: bool) -> host::Monitor {
        host::Monitor { key: key.into(), rect: [x, y, 2560, 1440], work: [x, y, 2560, 1380], dpi, primary }
    }
    fn icons() -> Vec<ViewItem> {
        [(11, 2), (86, 109), (161, 216)].iter().enumerate().map(|(n, &pos)|
            ViewItem { path: format!("C:/d/{n}.lnk"), label: format!("{n}"), pos }).collect()
    }

    #[test]
    fn grid_on_a_125_percent_monitor_matches_where_explorer_settled_icons() {
        let monitors = [monitor("main", 0, 0, 96, true), monitor("side", 2560, -1, 120, false)];
        let bounds = Rect { x: 2860, y: 199, w: 740, h: 416 };
        let grid = box_grid(bounds, &icons(), &monitors, (75, 107), true).unwrap();
        assert_eq!(grid.cell, (94, 134));
        let origin = grid.origin.unwrap();
        // Columns Explorer settled on: 2856 + 94k; rows: 269 + 134k.
        assert_eq!((2856 - origin.0).rem_euclid(94), 0);
        assert_eq!((269 - origin.1).rem_euclid(134), 0);
    }

    #[test]
    fn one_icon_already_on_the_scaled_monitor_confirms_its_grid() {
        let monitors = [monitor("main", 0, 0, 96, true), monitor("side", 2560, -1, 120, false)];
        let bounds = Rect { x: 2860, y: 199, w: 740, h: 416 };
        let mut with_one = icons();
        // Live 2026-11-19: an icon the user left on the 125% monitor sat at (3232,135).
        with_one.push(ViewItem { path: "C:/d/unity.lnk".into(), label: "unity".into(), pos: (3232, 135) });
        let derived = box_grid(bounds, &icons(), &monitors, (75, 107), true).unwrap();
        let observed = box_grid(bounds, &with_one, &monitors, (75, 107), true).unwrap();
        let on_grid = |o: (i32, i32), p: (i32, i32)| (p.0 - o.0).rem_euclid(94) == 0 && (p.1 - o.1).rem_euclid(134) == 0;
        assert!(on_grid(derived.origin.unwrap(), (3232, 135)));
        assert!(on_grid(observed.origin.unwrap(), derived.origin.unwrap()));
        with_one.push(ViewItem { path: "C:/d/off.lnk".into(), label: "off".into(), pos: (3240, 135) });
        assert!(box_grid(bounds, &with_one, &monitors, (75, 107), true).is_err());
    }

    #[test]
    fn main_scale_monitors_keep_the_observed_grid() {
        let monitors = [monitor("main", 0, 0, 96, true), monitor("side", 2560, -1, 96, false)];
        let main = box_grid(Rect { x: 1788, y: 30, w: 740, h: 416 }, &icons(), &monitors, (75, 107), true).unwrap();
        assert_eq!((main.cell, main.origin), ((75, 107), Some((11, 2))));
        let side = box_grid(Rect { x: 2860, y: 199, w: 740, h: 416 }, &icons(), &monitors, (75, 107), true).unwrap();
        assert_eq!((side.cell, side.origin), ((75, 107), Some((11, 2))));
    }

    #[test]
    fn snap_off_scales_the_cell_without_a_grid() {
        let monitors = [monitor("main", 0, 0, 96, true), monitor("side", 2560, -1, 120, false)];
        let grid = box_grid(Rect { x: 2860, y: 199, w: 740, h: 416 }, &[], &monitors, (75, 107), false).unwrap();
        assert_eq!((grid.cell, grid.origin), ((94, 134), None));
    }

    #[test]
    fn cell_for_point_scales_per_monitor_dpi() {
        let monitors = [monitor("main", 0, 0, 96, true), monitor("side", 2560, -1, 120, false)];
        assert_eq!(cell_for_point((500, 300), (75, 107), &monitors), (75, 107));
        assert_eq!(cell_for_point((2860, 200), (75, 107), &monitors), (94, 134));
        assert_eq!(cell_for_point((2860, 200), (75, 107), &[]), (75, 107));
    }
}

#[cfg(test)]
mod empty_monitor_grid_tests {
    use super::*;
    const CELL: (i32, i32) = (75, 107);
    fn primary() -> Rect { Rect { x: 0, y: 0, w: 2560, h: 1440 } }
    fn secondary() -> Rect { Rect { x: 2560, y: -1, w: 2560, h: 1440 } }

    #[test]
    fn monitor_without_icons_uses_the_desktop_wide_grid() {
        let icons = [(11, 2), (86, 109), (161, 216)];
        assert_eq!(grid_origin_for_screen(secondary(), &icons, CELL), Ok((11, 2)));
        assert_eq!(grid_origin_for_screen(primary(), &icons, CELL), Ok((11, 2)));
    }

    #[test]
    fn disagreeing_or_missing_icons_still_refuse() {
        assert!(grid_origin_for_screen(secondary(), &[(11, 2), (12, 2)], CELL).is_err());
        assert!(grid_origin_for_screen(secondary(), &[], CELL).is_err());
        // A monitor WITH icons still verifies its own grid only.
        assert!(grid_origin_for_screen(secondary(), &[(2571, 2)], CELL).is_err());
    }
}

