use crate::*;
use super::preview_common::*;

/// Real Windows monitor topology map preview for Displays settings.
pub(super) fn draw_displays_preview(
    p: &Painter,
    b: Bounds,
    monitors: &[host::Monitor],
    selected_key: &str,
) {
    let stage = draw_preview_card(p, b, "Monitor layout", "Click a display above to host controls");
    round(stage, 6., Color::new(0.035, 0.060, 0.105, 0.96));
    if monitors.is_empty() {
        return;
    }
    let min_x = monitors.iter().map(|m| m.rect[0]).min().unwrap_or(0) as f32;
    let min_y = monitors.iter().map(|m| m.rect[1]).min().unwrap_or(0) as f32;
    let max_x = monitors
        .iter()
        .map(|m| m.rect[0] + m.rect[2])
        .max()
        .unwrap_or(1920) as f32;
    let max_y = monitors
        .iter()
        .map(|m| m.rect[1] + m.rect[3])
        .max()
        .unwrap_or(1080) as f32;
    let span_w = (max_x - min_x).max(1.);
    let span_h = (max_y - min_y).max(1.);
    let pad = 12.;
    let scale = ((stage.w - pad * 2.) / span_w).min((stage.h - pad * 2.) / span_h);
    let off_x = stage.x + (stage.w - span_w * scale) / 2.;
    let off_y = stage.y + (stage.h - span_h * scale) / 2.;

    for (idx, m) in monitors.iter().enumerate() {
        let active = (!selected_key.is_empty() && m.key == selected_key)
            || (selected_key.is_empty() && m.primary)
            || (!selected_key.is_empty()
                && !monitors.iter().any(|cand| cand.key == selected_key)
                && m.primary);
        let mr = Bounds {
            x: off_x + (m.rect[0] as f32 - min_x) * scale,
            y: off_y + (m.rect[1] as f32 - min_y) * scale,
            w: (m.rect[2] as f32 * scale).max(24.),
            h: (m.rect[3] as f32 * scale).max(18.),
        };
        let wr = Bounds {
            x: off_x + (m.work[0] as f32 - min_x) * scale,
            y: off_y + (m.work[1] as f32 - min_y) * scale,
            w: (m.work[2] as f32 * scale).max(20.),
            h: (m.work[3] as f32 * scale).max(14.),
        };
        round(
            mr,
            4.,
            if active {
                Color::new(0.14, 0.25, 0.39, 0.96)
            } else {
                Color::new(0.10, 0.14, 0.21, 0.92)
            },
        );
        round(wr, 3., Color::new(0.08, 0.13, 0.20, 0.45));
        draw_rectangle_lines(
            mr.x,
            mr.y,
            mr.w,
            mr.h,
            if active { 2. } else { 1. },
            if active { ACCENT } else { alpha(WHITE, 0.22) },
        );
        let tag = if m.primary {
            format!("{}*", idx + 1)
        } else {
            format!("{}", idx + 1)
        };
        p.text(&tag, mr.x + 8., mr.y + 6., 13., if active { ACCENT } else { TEXT });
        if mr.w >= 76. && mr.h >= 42. {
            p.text(
                &format!("{}x{}", m.rect[2], m.rect[3]),
                mr.x + 8.,
                mr.y + 23.,
                10.,
                MUTED,
            );
        }
    }
}
