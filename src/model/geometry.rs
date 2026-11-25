use super::*;

/// Minimum content is one icon cell with a two-pixel height allowance.
pub fn min_content(cell: (f32, f32)) -> (f32, f32) {
    (cell.0, cell.1 + (2.0f32 * 1.0 + 0.5).trunc())
}
/// Scrollbar drag: the first visible row for a pointer at `y`, where `grab` is
/// where on the thumb it was taken (inverse of `scroll_thumb`).
pub fn scroll_row_at(
    y: f32,
    track_y: f32,
    track_h: f32,
    total: usize,
    page: usize,
    grab: f32,
) -> usize {
    let total = total.max(1);
    let page = page.clamp(1, total);
    let span = total - page;
    if span == 0 {
        return 0;
    }
    let (_, len) = scroll_thumb(total, page, 0, track_h);
    let room = (track_h - len).max(1.0);
    let t = ((y - grab - track_y) / room).clamp(0.0, 1.0);
    (t * span as f32).round() as usize
}

/// Scroll by whole *rows*, never by a pageful of icons for a single wheel tick.
/// The last row is reachable while the first visible item stays row-aligned.
pub fn scroll_rows(
    current: usize,
    item_count: usize,
    visible: usize,
    cols: usize,
    notches: i32,
) -> usize {
    let cols = cols.max(1);
    let max_row = item_count
        .div_ceil(cols)
        .saturating_sub(visible.div_ceil(cols));
    ((current / cols) as i32 - notches).clamp(0, max_row as i32) as usize * cols
}

/// Resize a group by dragging one or more edges (1 left, 2 right, 4 top, 8
/// bottom). The opposite edge stays put, the size never goes below `min`, and
/// the group stays inside `area`.
pub fn resize_bounds(
    before: Bounds,
    delta: glam_vec::Vec2,
    edges: u8,
    min: (f32, f32),
    area: Bounds,
) -> Bounds {
    let (mut l, mut t, mut r, mut b) =
        (before.x, before.y, before.x + before.w, before.y + before.h);
    if edges & 1 != 0 {
        l = (l + delta.x).clamp(area.x, r - min.0);
    }
    if edges & 2 != 0 {
        r = (r + delta.x).clamp(l + min.0, (area.x + area.w).max(l + min.0));
    }
    if edges & 4 != 0 {
        t = (t + delta.y).clamp(area.y, b - min.1);
    }
    if edges & 8 != 0 {
        b = (b + delta.y).clamp(t + min.1, (area.y + area.h).max(t + min.1));
    }
    Bounds {
        x: l,
        y: t,
        w: r - l,
        h: b - t,
    }
}
mod glam_vec {
    pub use macroquad::math::Vec2;
}

/// Scrollbar thumb occupies at least eight pixels, with its position
/// normalized between the first and last visible rows.
pub fn scroll_thumb(max: usize, page: usize, pos: usize, height: f32) -> (f32, f32) {
    let max = max.max(1);
    let page = page.clamp(1, max);
    let pos = pos.min(max - page);
    let len = ((page as f32 / max as f32) * height)
        .round()
        .max(SCROLL_MIN_THUMB);
    let span = (max - page).max(1);
    let start = (pos as f32 / span as f32 * (height - len)).round();
    (start, len)
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Bounds {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macroquad::math::vec2;

    #[test]
    fn scroll_limits_and_row_alignment() {
        assert_eq!(scroll_rows(0, 40, 9, 3, -1), 3);
        assert_eq!(scroll_rows(3, 40, 9, 3, 1), 0);
        assert_eq!(scroll_rows(33, 40, 9, 3, -1), 33);
        assert_eq!(scroll_rows(0, 0, 9, 0, -1), 0);
        assert_eq!(scroll_row_at(500., 10., 100., 20, 20, 0.), 0);
    }

    #[test]
    fn thumb_and_drag_round_trip() {
        for pos in 0..=8 {
            let (start, _) = scroll_thumb(10, 2, pos, 100.);
            assert_eq!(scroll_row_at(10. + start + 3., 10., 100., 10, 2, 3.), pos);
        }
        assert_eq!(scroll_thumb(1000, 1, 999, 100.), (92., 8.));
        assert_eq!(min_content((75., 86.)), (75., 88.));
    }

    #[test]
    fn resize_preserves_opposite_edges_and_clamps() {
        let b = Bounds { x: 100., y: 100., w: 300., h: 200. };
        let area = Bounds { x: 0., y: 0., w: 1000., h: 800. };
        let min = (100., 80.);
        let r = resize_bounds(b, vec2(500., -500.), 1 | 4, min, area);
        assert_eq!((r.x, r.y, r.w, r.h), (300., 0., 100., 300.));
        let r = resize_bounds(b, vec2(1000., 1000.), 2 | 8, min, area);
        assert_eq!((r.x, r.y, r.w, r.h), (100., 100., 900., 700.));
        assert_eq!(resize_bounds(b, vec2(50., 50.), 0, min, area), b);
    }
}
