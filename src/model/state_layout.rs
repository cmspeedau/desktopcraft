use super::*;

impl State {
    pub fn snap_position(&self, id: u64, proposed: Bounds) -> Bounds {
        let Some(group) = self.group(id) else {
            return proposed;
        };
        let mut dx = None;
        let mut dy = None;
        let consider = |best: &mut Option<f32>, distance: f32, limit: f32, inclusive: bool| {
            if (distance.abs() < limit || (inclusive && distance.abs() == limit))
                && best.is_none_or(|old| distance.abs() < old.abs())
            {
                *best = Some(distance);
            }
        };
        for other in self
            .groups
            .iter()
            .filter(|g| g.id != id && g.page == group.page)
        {
            let b = other.bounds;
            for distance in [b.x - proposed.x, b.x + b.w - proposed.x - proposed.w] {
                consider(&mut dx, distance, SNAP_ALIGNMENT, true);
            }
            for distance in [b.y - proposed.y, b.y + b.h - proposed.y - proposed.h] {
                consider(&mut dy, distance, SNAP_ALIGNMENT, true);
            }
            // A box beside its neighbor should snap even if there is still a
            // small positive gap. Require overlap on the orthogonal axis so
            // distant unrelated boxes do not attract each other.
            let row_overlap = proposed.y < b.y + b.h + SNAP_PADDING
                && proposed.y + proposed.h > b.y - SNAP_PADDING;
            let column_overlap = proposed.x < b.x + b.w + SNAP_PADDING
                && proposed.x + proposed.w > b.x - SNAP_PADDING;
            if row_overlap {
                let right_gap = b.x - (proposed.x + proposed.w);
                let left_gap = proposed.x - (b.x + b.w);
                // A separated box attracts only within 20 px; a slightly
                // overlapping pair can be resolved within the original 40 px.
                consider(
                    &mut dx,
                    right_gap,
                    if right_gap >= 0. {
                        SNAP_ALIGNMENT
                    } else {
                        SNAP_OVERLAP
                    },
                    false,
                );
                consider(
                    &mut dx,
                    -left_gap,
                    if left_gap >= 0. {
                        SNAP_ALIGNMENT
                    } else {
                        SNAP_OVERLAP
                    },
                    false,
                );
            }
            if column_overlap {
                let bottom_gap = b.y - (proposed.y + proposed.h);
                let top_gap = proposed.y - (b.y + b.h);
                consider(
                    &mut dy,
                    bottom_gap,
                    if bottom_gap >= 0. {
                        SNAP_ALIGNMENT
                    } else {
                        SNAP_OVERLAP
                    },
                    false,
                );
                consider(
                    &mut dy,
                    -top_gap,
                    if top_gap >= 0. {
                        SNAP_ALIGNMENT
                    } else {
                        SNAP_OVERLAP
                    },
                    false,
                );
            }
        }
        // Match the clear spacing between two already-neighboring boxes.
        // This works on rows or columns and only when the proposed box sits
        // along the same band; unrelated boxes never pull it across the page.
        let peers: Vec<&Group> = self
            .groups
            .iter()
            .filter(|g| g.id != id && g.page == group.page)
            .collect();
        for (i, first) in peers.iter().enumerate() {
            for second in peers.iter().skip(i + 1) {
                let (a, b) = (first.bounds, second.bounds);
                let overlap_y = proposed.y < a.y + a.h
                    && proposed.y + proposed.h > a.y
                    && proposed.y < b.y + b.h
                    && proposed.y + proposed.h > b.y;
                let overlap_x = proposed.x < a.x + a.w
                    && proposed.x + proposed.w > a.x
                    && proposed.x < b.x + b.w
                    && proposed.x + proposed.w > b.x;
                let (left, right) = if a.x <= b.x { (a, b) } else { (b, a) };
                let gap_x = right.x - (left.x + left.w);
                if overlap_y {
                    if gap_x >= proposed.w + 8. {
                        // Center a box in the space between two neighbors:
                        // both gaps become equal without any overlap.
                        let middle = (left.x + left.w + right.x - proposed.w) / 2.;
                        consider(&mut dx, middle - proposed.x, SNAP_ALIGNMENT, true);
                    }
                    if (4.0..=100.0).contains(&gap_x) {
                        // Repeat the established gutter on either outside edge.
                        for target_x in [right.x + right.w + gap_x, left.x - gap_x - proposed.w] {
                            consider(&mut dx, target_x - proposed.x, SNAP_ALIGNMENT, true);
                        }
                    }
                }
                let (top, bottom) = if a.y <= b.y { (a, b) } else { (b, a) };
                let gap_y = bottom.y - (top.y + top.h);
                if overlap_x {
                    if gap_y >= proposed.h + 8. {
                        let middle = (top.y + top.h + bottom.y - proposed.h) / 2.;
                        consider(&mut dy, middle - proposed.y, SNAP_ALIGNMENT, true);
                    }
                    if (4.0..=100.0).contains(&gap_y) {
                        for target_y in [bottom.y + bottom.h + gap_y, top.y - gap_y - proposed.h] {
                            consider(&mut dy, target_y - proposed.y, SNAP_ALIGNMENT, true);
                        }
                    }
                }
            }
        }
        Bounds {
            x: proposed.x + dx.unwrap_or(0.0),
            y: proposed.y + dy.unwrap_or(0.0),
            ..proposed
        }
    }
    /// Snap the dragged resize edge (not the opposite fixed edge) to a
    /// neighbor's left/right or top/bottom boundary. No resizing across pages.
    pub fn snap_resize(
        &self,
        id: u64,
        proposed: Bounds,
        edges: u8,
        min: (f32, f32),
        area: Bounds,
    ) -> Bounds {
        let Some(group) = self.group(id) else {
            return proposed;
        };
        let mut result = proposed;
        let (mut l, mut r) = (proposed.x, proposed.x + proposed.w);
        let (mut t, mut b) = (proposed.y, proposed.y + proposed.h);
        let limit = SNAP_ALIGNMENT.min(16.0);
        let mut best_x = limit + 1.0;
        let mut best_y = limit + 1.0;
        for other in self
            .groups
            .iter()
            .filter(|g| g.id != id && g.page == group.page)
        {
            let peer = other.bounds;
            for target in [peer.x, peer.x + peer.w] {
                if edges & 1 != 0
                    && (l - target).abs() < best_x
                    && target >= area.x
                    && r - target >= min.0
                {
                    best_x = (l - target).abs();
                    result.x = target;
                    result.w = r - target;
                } else if edges & 2 != 0
                    && (r - target).abs() < best_x
                    && target <= area.x + area.w
                    && target - l >= min.0
                {
                    best_x = (r - target).abs();
                    result.w = target - l;
                }
            }
            for target in [peer.y, peer.y + peer.h] {
                if edges & 4 != 0
                    && (t - target).abs() < best_y
                    && target >= area.y
                    && b - target >= min.1
                {
                    best_y = (t - target).abs();
                    result.y = target;
                    result.h = b - target;
                } else if edges & 8 != 0
                    && (b - target).abs() < best_y
                    && target <= area.y + area.h
                    && target - t >= min.1
                {
                    best_y = (b - target).abs();
                    result.h = target - t;
                }
            }
        }
        // No coordinates or sizes are committed until after all candidate
        // checks, preserving the fixed far edges and min dimensions.
        l = result.x;
        r = result.x + result.w;
        t = result.y;
        b = result.y + result.h;
        debug_assert!(r - l >= min.0 - 0.01 && b - t >= min.1 - 0.01);
        result
    }
}
