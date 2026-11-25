use crate::*;
use super::preview_common::*;

/// Large vector preview for double-clicking blank desktop space (`quick_hide`).
pub(super) fn draw_quick_hide_preview(p: &Painter, b: Bounds, time: f64, quick_hide: bool) {
    let stage = draw_preview_card(
        p,
        b,
        "Quick-hide preview",
        if quick_hide { "2x click ON" } else { "2x click OFF" },
    );
    round(stage, 6., Color::new(0.035, 0.060, 0.105, 0.96));

    let phase = ((time % 4.0) / 4.0) as f32;
    let box_alpha = if !quick_hide {
        0.95
    } else if phase < 0.22 {
        0.95
    } else if phase < 0.36 {
        0.95 * (1. - preview_smoothstep(0.22, 0.36, phase))
    } else if phase < 0.72 {
        0.08
    } else if phase < 0.86 {
        0.08 + 0.87 * preview_smoothstep(0.72, 0.86, phase)
    } else {
        0.95
    };

    let draw_mini_box = |bx: f32, by: f32, bw: f32, bh: f32, title: &str, cols: usize, tint: Color| {
        let box_r = Bounds { x: bx, y: by, w: bw, h: bh };
        round(box_r, 5., Color::new(0.11, 0.17, 0.26, box_alpha));
        round(Bounds { h: 16., ..box_r }, 4., alpha(tint, 0.32 * box_alpha));
        p.text(title, bx + 8., by + 2., 10., alpha(TEXT, box_alpha));
        draw_mini_icon_grid(bx + 10., by + 22., cols, 2, tint, box_alpha * 0.9);
    };

    draw_mini_box(stage.x + 14., stage.y + 10., 118., 72., "Shortcuts", 3, ACCENT);
    draw_mini_box(stage.x + 146., stage.y + 10., 118., 72., "Work", 3, Color::new(0.48, 0.78, 0.68, 1.));
    draw_mini_box(stage.x + 278., stage.y + 10., 118., 72., "Notes", 3, Color::new(0.65, 0.58, 0.87, 1.));

    let click_pt = vec2(stage.x + stage.w - 92., stage.y + 44.);
    let ripple1 = (phase >= 0.16 && phase < 0.32).then(|| (phase - 0.16) / 0.16);
    let ripple2 = (phase >= 0.66 && phase < 0.82).then(|| (phase - 0.66) / 0.16);
    if quick_hide {
        p.text(
            "2x click blank area",
            stage.x + stage.w - 152.,
            stage.y + 14.,
            11.,
            ACCENT,
        );
    }
    draw_preview_cursor(click_pt.x, click_pt.y, ripple1.or(ripple2));
    p.text(
        if quick_hide {
            "Double-clicking open desktop toggles all groups (pinned groups stay visible)"
        } else {
            "Quick-hide disabled: double-clicking blank space leaves all groups visible"
        },
        stage.x + 12.,
        stage.y + stage.h - 19.,
        11.,
        MUTED,
    );
}
