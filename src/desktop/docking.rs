use super::*;

/// Use the unsnapped position so the next mouse movement can leave the edge.
const DOCK_SNAP_DISTANCE: f32 = 14.;

fn native_dock_edge(body: Bounds, work: Bounds, from_bottom: bool) -> Option<DockEdge> {
    let right = work.x + work.w;
    let bottom = work.y + work.h;
    let bottom_edge = if from_bottom { body.y } else { body.y + body.h };
    [
        (DockEdge::Bottom, (bottom_edge - bottom).abs()),
        (DockEdge::Top, (body.y - HEADER - work.y).abs()),
        (DockEdge::Left, (body.x - work.x).abs()),
        (DockEdge::Right, (body.x + body.w - right).abs()),
    ].into_iter().filter(|(_, distance)| *distance <= DOCK_SNAP_DISTANCE)
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(edge, _)| edge)
}


impl App {
    /// Roll-up docking (manager "Automatically roll up groups placed near the
    /// edge of the screen"): a group dropped against the top or bottom of its
    /// monitor's work area rolls up.
    pub fn dock_if_at_edge(&mut self, id: u64) {
        if self.host.is_none() || !self.state.prefs.rollup || !self.state.prefs.rollup_dock {
            return;
        }
        let Some(b) = self.state.group(id).map(|g| g.bounds) else {
            return;
        };
        let r = self.to_screen(b);
        let Some(m) = self.monitor_at(r[0] + r[2] / 2, r[1] + 10) else {
            return;
        };
        let work = self.monitor_logical(&m, true);
        let near = 6.;
        // Top edge only: a group rolled up at the bottom would open off-screen
        // (the original opens it upwards; not built yet).
        if b.y <= work.y + near {
            if let Some(g) = self.state.group_mut(id) {
                g.bounds.y = work.y;
                g.rolled = true;
            }
            self.notify("Drawer: the group rolled up. Point at it to open; double-click its title to unroll.", get_time());
        }
    }

    pub(crate) fn native_dock_target(
        &self, pointer: Vec2, body: Bounds, from_bottom: bool, moved_side: Option<(DockEdge, Bounds)>,
    ) -> Option<(DockEdge, Bounds)> {
        if !self.native_env.chrome_only || !self.state.prefs.rollup || !self.state.prefs.rollup_dock {
            return None;
        }
        let [x, y, _, _] = self.to_screen(Bounds { x: pointer.x, y: pointer.y, w: 0., h: 0. });
        let monitor = self.monitor_at(x, y)?;
        let work = self.monitor_logical(&monitor, true);
        if work.h <= HEADER || work.w <= HEADER {
            return None;
        }
        if let Some((edge, header)) = moved_side {
            let distance = if edge == DockEdge::Left {
                header.x - work.x
            } else {
                header.x + header.w - work.x - work.w
            };
            if distance.abs() <= DOCK_SNAP_DISTANCE {
                return Some((edge, work));
            }
        }
        native_dock_edge(body, work, from_bottom).map(|edge| (edge, work))
    }

    pub(crate) fn docked_resize_target(&self, id: u64, before: Bounds) -> Option<(DockEdge, Bounds)> {
        if !self.native_env.chrome_only {
            return None;
        }
        let edge = self.state.group(id)?.dock?;
        let [x, y, w, h] = self.to_screen(before);
        let monitor = self.monitor_at(x + w / 2, y + h / 2)?;
        Some((edge, self.monitor_logical(&monitor, true)))
    }

}
