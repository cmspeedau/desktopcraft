use super::*;

pub(super) fn is_recycle_bin_shell_item(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    (p.starts_with("::") || p.starts_with("shell:"))
        && p.contains("645ff040-5081-101b-9f08-00aa002f954e")
}

/// Snap the top-left of a DesktopCraft-rendered loose icon to the nearest
/// Explorer-sized desktop cell, searching outward for a free, unobscured slot.
/// This is independent of Explorer's FWF_SNAPTOGRID preference (which we never
/// silently disable). All coordinates are physical screen pixels.
#[cfg(test)]
pub(super) fn snap_loose_icon(
    desired: (i32, i32),
    work: [i32; 4],
    spacing: (i32, i32),
    occupied: &[(i32, i32)],
    blocked: &[[i32; 4]],
) -> (i32, i32) {
    snap_loose_icon_phase(desired, work, spacing, occupied, blocked, (0, 0))
}
/// Explorer's icon grid is not always aligned to the work-area origin,
/// especially at nonstandard scaling on Windows Server. Calibrate the
/// phase from observed Explorer icon coordinates instead of forcing x=0.
pub(super) fn grid_phase(work: [i32; 4], spacing: (i32, i32), positions: &[(i32, i32)]) -> (i32, i32) {
    let axis = |origin: i32, step: i32, coords: Vec<i32>| -> i32 {
        let step = step.max(1);
        let residues: Vec<i32> = coords
            .iter()
            .map(|v| (v - origin).rem_euclid(step))
            .collect();
        if residues.is_empty() {
            return 0;
        }
        residues
            .iter()
            .copied()
            .max_by_key(|candidate| {
                residues
                    .iter()
                    .filter(|r| {
                        let d = (**r - *candidate).abs();
                        d.min(step - d) <= 5
                    })
                    .count()
            })
            .unwrap_or(0)
    };
    (
        axis(work[0], spacing.0, positions.iter().map(|p| p.0).collect()),
        axis(work[1], spacing.1, positions.iter().map(|p| p.1).collect()),
    )
}
pub(super) fn snap_loose_icon_phase(
    desired: (i32, i32),
    work: [i32; 4],
    spacing: (i32, i32),
    occupied: &[(i32, i32)],
    blocked: &[[i32; 4]],
    phase: (i32, i32),
) -> (i32, i32) {
    let (sx, sy) = (spacing.0.max(1), spacing.1.max(1));
    let max_col = ((work[2] - sx - phase.0).max(0) / sx).max(0);
    let max_row = ((work[3] - sy - phase.1).max(0) / sy).max(0);
    let base_col = ((desired.0 - work[0] - phase.0) as f64 / sx as f64).round() as i32;
    let base_row = ((desired.1 - work[1] - phase.1) as f64 / sy as f64).round() as i32;
    let base = (base_col.clamp(0, max_col), base_row.clamp(0, max_row));
    let point = |col: i32, row: i32| (work[0] + phase.0 + col * sx, work[1] + phase.1 + row * sy);
    let clear = |at: (i32, i32)| {
        !occupied
            .iter()
            .any(|p| (p.0 - at.0).abs() < sx / 2 && (p.1 - at.1).abs() < sy / 2)
            && !blocked.iter().any(|b| {
                at.0 < b[0] + b[2] && at.0 + sx > b[0] && at.1 < b[1] + b[3] && at.1 + sy > b[1]
            })
    };
    // A bounded ring search is cheap even on an icon-heavy desktop.
    for radius in 0i32..=12 {
        let mut candidate = None;
        let mut score = i64::MAX;
        for row in (base.1 - radius).max(0)..=(base.1 + radius).min(max_row) {
            for col in (base.0 - radius).max(0)..=(base.0 + radius).min(max_col) {
                if (col - base.0).abs().max((row - base.1).abs()) != radius {
                    continue;
                }
                let at = point(col, row);
                if !clear(at) {
                    continue;
                }
                let dx = (at.0 - desired.0) as i64;
                let dy = (at.1 - desired.1) as i64;
                let distance = dx * dx + dy * dy;
                if distance < score {
                    candidate = Some(at);
                    score = distance;
                }
            }
        }
        if let Some(at) = candidate {
            return at;
        }
    }
    point(base.0, base.1)
}


impl App {
    /// User released a file over DesktopCraft's own drawn Recycle Bin.
    /// Shell delete is the only intentional physical file operation here;
    /// a cancelled or failed deletion leaves both the file and its pin intact.
    pub fn try_recycle_drop(&mut self, source: &str, screen: (i32, i32), time: f64) -> bool {
        let on_bin = self.host.as_ref().is_some_and(|h| {
            let (sx, sy) = h.spacing;
            h.loose.iter().any(|l| {
                is_recycle_bin_shell_item(&l.path)
                    && !l.path.eq_ignore_ascii_case(source)
                    && screen.0 >= l.pos.0
                    && screen.0 < l.pos.0 + sx
                    && screen.1 >= l.pos.1
                    && screen.1 < l.pos.1 + sy
            })
        });
        if !on_bin {
            return false;
        }
        if !Path::new(source).exists() {
            self.notify(
                "Only existing files or folders can be sent to Recycle Bin.",
                time,
            );
            return true;
        }
        match shell::recycle(source) {
            Ok(()) => {
                // Windows owns restore/undo for disk files; layout undo cannot
                // undo a Shell recycle operation.
                self.undo.clear();
                self.state.unassign(source);
                // A linked Folder View mirrors the disk, so refresh affected
                // folders after Windows moves an item into the Recycle Bin.
                for group in &mut self.state.groups {
                    for tab in &mut group.tabs {
                        if tab.folder.as_deref().is_some_and(|folder| {
                            Path::new(source).parent() == Some(Path::new(folder))
                        }) {
                            let _ = tab.refresh();
                        }
                    }
                }
                if let Some(h) = self.host.as_mut() {
                    h.loose
                        .retain(|icon| !icon.path.eq_ignore_ascii_case(source));
                    h.selected_loose = None;
                    h.marquee_selected.clear();
                    h.last_sync = -10.;
                }
                host::event(
                    "INFO",
                    "drag.recycled",
                    "Windows Shell recycled dropped item",
                );
                self.notify(
                    "Sent to Recycle Bin (restore using Windows if needed)",
                    time,
                );
            }
            Err(error) => {
                host::event(
                    "WARN",
                    "drag.recycle_failed",
                    "Windows Shell cancelled or rejected operation",
                );
                self.notify(error, time);
            }
        }
        true
    }

}
