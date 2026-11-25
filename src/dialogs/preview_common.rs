use crate::*;

pub(super) fn preview_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return if x >= edge1 { 1. } else { 0. };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

pub(super) fn draw_preview_card(p: &Painter, b: Bounds, title: &str, badge: &str) -> Bounds {
    round(b, 8., Color::new(0.055, 0.088, 0.142, 0.96));
    draw_rectangle_lines(b.x, b.y, b.w, b.h, 1., alpha(WHITE, 0.10));
    p.text(title, b.x + 12., b.y + 8., 12., TEXT);
    if !badge.is_empty() {
        let bw = p.measure(badge, 11.) + 14.;
        let pill = Bounds {
            x: b.x + b.w - bw - 10.,
            y: b.y + 6.,
            w: bw,
            h: 18.,
        };
        round(pill, 4., alpha(ACCENT, 0.16));
        p.text(badge, pill.x + 7., pill.y + 3., 11., ACCENT);
    }
    Bounds {
        x: b.x + 12.,
        y: b.y + 28.,
        w: b.w - 24.,
        h: (b.h - 36.).max(20.),
    }
}

pub(super) fn draw_preview_cursor(x: f32, y: f32, ripple: Option<f32>) {
    if let Some(r) = ripple {
        let radius = 4. + r * 14.;
        draw_circle_lines(x, y, radius, 1.5, alpha(ACCENT, (1. - r).max(0.) * 0.9));
    }
    draw_triangle(
        vec2(x, y),
        vec2(x + 11., y + 6.),
        vec2(x + 4., y + 12.),
        Color::new(0.05, 0.08, 0.12, 0.95),
    );
    draw_triangle(
        vec2(x + 1., y + 1.5),
        vec2(x + 9.5, y + 6.),
        vec2(x + 4., y + 10.5),
        TEXT,
    );
}

pub(super) fn draw_mini_icon_grid(x: f32, y: f32, cols: usize, rows: usize, tint: Color, a: f32) {
    for row in 0..rows {
        for col in 0..cols {
            let ix = x + col as f32 * 26.;
            let iy = y + row as f32 * 22.;
            round(Bounds { x: ix, y: iy, w: 16., h: 12. }, 3., alpha(tint, a));
            round(
                Bounds {
                    x: ix + 2.,
                    y: iy + 14.,
                    w: 12.,
                    h: 2.,
                },
                1.,
                alpha(TEXT, a * 0.45),
            );
        }
    }
}
