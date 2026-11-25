use crate::*;
use super::preview_common::*;

/// Large vector preview for moving files across Box tabs (`switch_tab_after_drop`).
pub(super) fn draw_tab_drop_preview(p: &Painter, b: Bounds, time: f64, switch_tab: bool) {
    let stage = draw_preview_card(
        p,
        b,
        "Drag-across-tabs preview",
        if switch_tab {
            "Switch to destination"
        } else {
            "Stay on source tab"
        },
    );
    round(stage, 6., Color::new(0.035, 0.060, 0.105, 0.96));

    let phase = ((time % 4.0) / 4.0) as f32;
    let target_box = Bounds {
        x: stage.x + 140.,
        y: stage.y + 8.,
        w: (stage.w - 154.).max(220.),
        h: stage.h - 32.,
    };
    round(target_box, 6., Color::new(0.11, 0.17, 0.26, 0.96));
    draw_rectangle_lines(
        target_box.x,
        target_box.y,
        target_box.w,
        target_box.h,
        1.,
        alpha(WHITE, 0.10),
    );
    let drop_done = preview_smoothstep(0.46, 0.64, phase) > 0.72 && phase < 0.90;
    let tab_b_active = switch_tab && drop_done;
    let tab_hover = phase >= 0.34 && phase <= 0.64;

    let tab_a_r = Bounds { x: target_box.x + 8., y: target_box.y + 5., w: 96., h: 18. };
    let tab_b_r = Bounds { x: target_box.x + 110., y: target_box.y + 5., w: 96., h: 18. };
    let tab_c_r = Bounds { x: target_box.x + 212., y: target_box.y + 5., w: 86., h: 18. };
    round(
        tab_a_r,
        4.,
        if !tab_b_active { alpha(ACCENT, 0.45) } else { alpha(WHITE, 0.08) },
    );
    round(
        tab_b_r,
        4.,
        if tab_b_active {
            alpha(Color::new(0.48, 0.78, 0.68, 1.), 0.55)
        } else if tab_hover {
            alpha(ACCENT, 0.28)
        } else {
            alpha(WHITE, 0.08)
        },
    );
    round(tab_c_r, 4., alpha(WHITE, 0.08));
    p.text("Work", tab_a_r.x + 14., tab_a_r.y + 3., 11., TEXT);
    p.text("Projects", tab_b_r.x + 14., tab_b_r.y + 3., 11., TEXT);
    p.text("Notes", tab_c_r.x + 14., tab_c_r.y + 3., 11., TEXT);

    let (grid_tint, cols) = if tab_b_active {
        (Color::new(0.48, 0.78, 0.68, 1.), 5)
    } else {
        (ACCENT, 4)
    };
    draw_mini_icon_grid(target_box.x + 16., target_box.y + 30., cols, 2, grid_tint, 0.94);

    // Source desktop chip picked up and dragged onto `Projects` tab.
    let src_x = stage.x + 36.;
    let src_y = stage.y + 42.;
    if phase < 0.20 || phase > 0.76 {
        let chip = Bounds { x: src_x, y: src_y, w: 72., h: 24. };
        round(chip, 4., alpha(ACCENT, 0.22));
        draw_rectangle_lines(chip.x, chip.y, chip.w, chip.h, 1., alpha(ACCENT, 0.45));
        p.text("Brief.pdf", chip.x + 10., chip.y + 6., 10., TEXT);
    }
    let drag_t = preview_smoothstep(0.18, 0.56, phase);
    if phase >= 0.14 && phase <= 0.66 {
        let ex = tab_b_r.x + 38.;
        let ey = tab_b_r.y + 9.;
        let cx = (src_x + 28.) + (ex - (src_x + 28.)) * drag_t;
        let cy = (src_y + 10.) + (ey - (src_y + 10.)) * drag_t;
        round(
            Bounds { x: cx - 18., y: cy - 8., w: 36., h: 16. },
            3.,
            alpha(ACCENT, 0.92),
        );
        draw_preview_cursor(cx + 6., cy + 4., None);
    }
    p.text(
        if switch_tab {
            "Moving onto Projects automatically switches the Box view to Projects"
        } else {
            "Moving onto Projects sends the file there while keeping Work active"
        },
        stage.x + 12.,
        stage.y + stage.h - 19.,
        11.,
        MUTED,
    );
}
