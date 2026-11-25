use crate::*;
use super::preview_common::*;

/// Live vector preview cycling through 1 hover roll-up scene + 4 screen-edge drag-to-snap Drawer scenes.
pub(super) fn draw_rollup_preview(
    p: &Painter,
    b: Bounds,
    time: f64,
    enabled: bool,
    double_click: bool,
    open_mode: u8,
    dock: bool,
) {
    const SCENE_DUR: f64 = 3.8;
    const SCENE_COUNT: usize = 5;
    let scene = ((time / SCENE_DUR).floor() as usize) % SCENE_COUNT;
    let phase = ((time % SCENE_DUR) / SCENE_DUR) as f32;

    let scene_names = [
        "1/5 Hover Roll-up",
        "2/5 Top Drawer",
        "3/5 Right Drawer",
        "4/5 Bottom Drawer",
        "5/5 Left Drawer",
    ];
    let badge = if !enabled {
        "Roll-up disabled"
    } else {
        scene_names[scene]
    };
    let stage = draw_preview_card(p, b, "Roll-up & 4-side edge drawer preview", badge);
    round(stage, 6., Color::new(0.035, 0.060, 0.105, 0.96));

    let desk = Bounds {
        x: stage.x + 8.,
        y: stage.y + 6.,
        w: 326.,
        h: (stage.h - 12.).max(120.),
    };
    round(desk, 5., Color::new(0.048, 0.082, 0.140, 0.98));
    draw_rectangle_lines(desk.x, desk.y, desk.w, desk.h, 1., alpha(WHITE, 0.08));

    if scene == 0 {
        // Scene 1: Hover to unroll a rolled-up box in place.
        let mut unfold = 0.0f32;
        let mut hover_progress = 0.0f32;
        let mut click_ripple = None;
        let mut cursor_t = 0.0f32;

        if !enabled {
            unfold = 1.0;
        } else {
            match open_mode {
                0 => {
                    if phase < 0.22 {
                        cursor_t = 0.5 * preview_smoothstep(0.05, 0.22, phase);
                    } else if phase < 0.44 {
                        unfold = preview_smoothstep(0.22, 0.34, phase);
                        cursor_t = 0.5 + 0.5 * preview_smoothstep(0.32, 0.44, phase);
                    } else if phase < 0.68 {
                        unfold = 1.0;
                        cursor_t = 1.0;
                    } else if phase < 0.86 {
                        let ret = preview_smoothstep(0.68, 0.80, phase);
                        cursor_t = 1.0 - ret;
                        unfold = 1. - preview_smoothstep(0.74, 0.86, phase);
                    }
                }
                1 => {
                    if phase < 0.18 {
                        cursor_t = 0.5 * preview_smoothstep(0.05, 0.18, phase);
                    } else if phase < 0.40 {
                        cursor_t = 0.5;
                        hover_progress = preview_smoothstep(0.18, 0.38, phase);
                        unfold = preview_smoothstep(0.36, 0.46, phase);
                    } else if phase < 0.68 {
                        unfold = 1.0;
                        cursor_t = 0.5 + 0.5 * preview_smoothstep(0.40, 0.54, phase);
                    } else if phase < 0.86 {
                        let ret = preview_smoothstep(0.68, 0.80, phase);
                        cursor_t = 1.0 - ret;
                        unfold = 1. - preview_smoothstep(0.74, 0.86, phase);
                    }
                }
                _ => {
                    if phase < 0.20 {
                        cursor_t = 0.5 * preview_smoothstep(0.05, 0.20, phase);
                    } else if phase < 0.38 {
                        cursor_t = 0.5;
                        if phase >= 0.22 {
                            click_ripple = Some(((phase - 0.22) / 0.14).min(1.));
                        }
                        unfold = preview_smoothstep(0.26, 0.38, phase);
                    } else if phase < 0.68 {
                        unfold = 1.0;
                        cursor_t = 0.5 + 0.5 * preview_smoothstep(0.38, 0.52, phase);
                    } else if phase < 0.86 {
                        cursor_t = 1.0 - preview_smoothstep(0.68, 0.80, phase);
                        if phase >= 0.72 && phase < 0.82 {
                            click_ripple = Some(((phase - 0.72) / 0.10).min(1.));
                        }
                        unfold = 1. - preview_smoothstep(0.74, 0.86, phase);
                    }
                }
            }
        }

        let bw = 208.;
        let fh = 122.;
        let hh = 24.;
        let bx = desk.x + 56.;
        let by = desk.y + 24.;
        let ch = hh + (fh - hh) * unfold;
        let body = Bounds { x: bx, y: by, w: bw, h: ch };
        let hdr = Bounds { x: bx, y: by, w: bw, h: hh };
        let interior = Bounds { x: bx + 14., y: by + hh + 10., w: bw - 28., h: ch - hh - 16. };
        let on_header = vec2(bx + 104., by + 12.);
        let in_body = vec2(bx + 104., by + 76.);
        let away = vec2(desk.x + 26., by + 72.);

        round(body, 6., Color::new(0.11, 0.17, 0.26, 0.96));
        draw_rectangle_lines(body.x, body.y, body.w, body.h, 1., alpha(WHITE, 0.16));
        round(hdr, 5., Color::new(0.15, 0.24, 0.36, 0.98));
        p.text("Project files", hdr.x + 10., hdr.y + 5., 11., TEXT);

        if hover_progress > 0.01 && hover_progress < 0.99 {
            round(
                Bounds {
                    x: hdr.x + 4.,
                    y: hdr.y + hdr.h - 3.,
                    w: (hdr.w - 8.).max(2.) * hover_progress,
                    h: 2.,
                },
                1.,
                ACCENT,
            );
        }
        if unfold > 0.30 {
            let icon_a = ((unfold - 0.30) / 0.70).clamp(0., 1.);
            draw_mini_icon_grid(interior.x, interior.y, 4, 2, ACCENT, icon_a * 0.9);
        }
        if enabled {
            let cursor = if cursor_t <= 0.5 {
                away.lerp(on_header, (cursor_t * 2.).clamp(0., 1.))
            } else {
                on_header.lerp(in_body, ((cursor_t - 0.5) * 2.).clamp(0., 1.))
            };
            draw_preview_cursor(cursor.x, cursor.y, click_ripple);
        }
    } else {
        // Scenes 2..5: Drag a floating box to a screen edge (Top, Right, Bottom, Left) to snap & collapse into a Drawer.
        let drag_t = preview_smoothstep(0.18, 0.54, phase);
        let snap_t = if enabled && dock {
            preview_smoothstep(0.54, 0.68, phase)
        } else {
            0.0
        };
        let unfold = 1.0 - snap_t;
        let edge_pulse = if phase >= 0.42 && phase <= 0.76 {
            0.90
        } else {
            0.32
        };
        let edge_color = alpha(ACCENT, if dock { edge_pulse } else { 0.18 });
        match scene {
            1 => round(Bounds { x: desk.x + 10., y: desk.y + 2., w: desk.w - 20., h: 4. }, 2., edge_color),
            2 => round(Bounds { x: desk.x + desk.w - 6., y: desk.y + 10., w: 4., h: desk.h - 20. }, 2., edge_color),
            3 => round(Bounds { x: desk.x + 10., y: desk.y + desk.h - 6., w: desk.w - 20., h: 4. }, 2., edge_color),
            _ => round(Bounds { x: desk.x + 2., y: desk.y + 10., w: 4., h: desk.h - 20. }, 2., edge_color),
        }

        let (body, hdr, interior, grab_pt) = match scene {
            1 => {
                // Drag upward to Top edge
                let bw = 200.;
                let fh = 112.;
                let hh = 24.;
                let bx = desk.x + (desk.w - bw) / 2.;
                let start_y = desk.y + 52.;
                let end_y = desk.y + 8.;
                let by = start_y + (end_y - start_y) * drag_t;
                let ch = hh + (fh - hh) * unfold;
                (
                    Bounds { x: bx, y: by, w: bw, h: ch },
                    Bounds { x: bx, y: by, w: bw, h: hh },
                    Bounds { x: bx + 14., y: by + hh + 8., w: bw - 28., h: (ch - hh - 14.).max(0.) },
                    vec2(bx + 100., by + 12.),
                )
            }
            2 => {
                // Drag rightward to Right edge
                let hw = 26.;
                let fw = 178.;
                let bh = 124.;
                let by = desk.y + (desk.h - bh) / 2.;
                let start_right = desk.x + 234.;
                let end_right = desk.x + desk.w - 8.;
                let right = start_right + (end_right - start_right) * drag_t;
                let cw = hw + (fw - hw) * unfold;
                let hdr_b = if snap_t > 0.35 {
                    Bounds { x: right - hw, y: by, w: hw, h: bh }
                } else {
                    Bounds { x: right - cw, y: by, w: cw, h: 24. }
                };
                let grab = if snap_t > 0.35 {
                    vec2(right - hw * 0.5, by + bh * 0.5)
                } else {
                    vec2(right - cw * 0.5, by + 12.)
                };
                (
                    Bounds { x: right - cw, y: by, w: cw, h: bh },
                    hdr_b,
                    Bounds { x: right - cw + 12., y: by + 30., w: (cw - 24.).max(0.), h: bh - 38. },
                    grab,
                )
            }
            3 => {
                // Drag downward to Bottom edge
                let bw = 200.;
                let fh = 112.;
                let hh = 24.;
                let bx = desk.x + (desk.w - bw) / 2.;
                let start_bottom = desk.y + desk.h - 46.;
                let end_bottom = desk.y + desk.h - 8.;
                let bottom = start_bottom + (end_bottom - start_bottom) * drag_t;
                let ch = hh + (fh - hh) * unfold;
                let by = bottom - ch;
                let hdr_y = if snap_t > 0.35 { bottom - hh } else { by };
                (
                    Bounds { x: bx, y: by, w: bw, h: ch },
                    Bounds { x: bx, y: hdr_y, w: bw, h: hh },
                    Bounds { x: bx + 14., y: by + hh + 8., w: bw - 28., h: (ch - hh - 14.).max(0.) },
                    vec2(bx + 100., hdr_y + 12.),
                )
            }
            _ => {
                // Drag leftward to Left edge
                let hw = 26.;
                let fw = 178.;
                let bh = 124.;
                let by = desk.y + (desk.h - bh) / 2.;
                let start_left = desk.x + 92.;
                let end_left = desk.x + 8.;
                let left = start_left + (end_left - start_left) * drag_t;
                let cw = hw + (fw - hw) * unfold;
                let hdr_b = if snap_t > 0.35 {
                    Bounds { x: left, y: by, w: hw, h: bh }
                } else {
                    Bounds { x: left, y: by, w: cw, h: 24. }
                };
                let grab = if snap_t > 0.35 {
                    vec2(left + hw * 0.5, by + bh * 0.5)
                } else {
                    vec2(left + cw * 0.5, by + 12.)
                };
                (
                    Bounds { x: left, y: by, w: cw, h: bh },
                    hdr_b,
                    Bounds { x: left + 12., y: by + 30., w: (cw - 24.).max(0.), h: bh - 38. },
                    grab,
                )
            }
        };

        round(body, 6., Color::new(0.11, 0.17, 0.26, 0.96));
        draw_rectangle_lines(body.x, body.y, body.w, body.h, 1., alpha(WHITE, 0.16));
        round(hdr, 5., Color::new(0.15, 0.24, 0.36, 0.98));
        if (scene == 2 || scene == 4) && snap_t > 0.35 {
            p.text("Drawer", hdr.x + 2., hdr.y + 12., 8., TEXT);
        } else {
            p.text("Project files", hdr.x + 10., hdr.y + 5., 11., TEXT);
        }
        if unfold > 0.35 {
            let icon_a = ((unfold - 0.35) / 0.65).clamp(0., 1.);
            draw_mini_icon_grid(interior.x, interior.y, 4, 2, ACCENT, icon_a * 0.9);
        }
        let away = vec2(desk.x + desk.w * 0.5, desk.y + desk.h * 0.5 + 24.);
        let cursor = if phase < 0.18 {
            away.lerp(grab_pt, preview_smoothstep(0.02, 0.18, phase))
        } else if phase < 0.72 {
            grab_pt
        } else {
            grab_pt.lerp(away, preview_smoothstep(0.72, 0.92, phase))
        };
        let ripple = (phase >= 0.14 && phase <= 0.26)
            .then(|| ((phase - 0.14) / 0.12).clamp(0., 1.));
        draw_preview_cursor(cursor.x, cursor.y, ripple);
    }

    // Right status & scene indicator panel (without "5-scene loop" text)
    let info_x = desk.x + desk.w + 18.;
    p.text(scene_names[scene], info_x, stage.y + 18., 15., TEXT);
    for i in 0..SCENE_COUNT {
        let dot_x = info_x + 6. + i as f32 * 18.;
        let dot_y = stage.y + 48.;
        if i == scene {
            draw_circle(dot_x, dot_y, 5., ACCENT);
        } else {
            draw_circle(dot_x, dot_y, 3.5, alpha(WHITE, 0.22));
        }
    }
    p.text(
        if scene > 0 {
            "Drag box to screen edge to snap"
        } else if !enabled {
            "Rolled boxes stay expanded"
        } else if open_mode == 0 {
            "Unfolds immediately on hover"
        } else if open_mode == 1 {
            "Waits briefly before unfolding"
        } else {
            "Stays rolled until header click"
        },
        info_x,
        stage.y + 74.,
        12.,
        TEXT,
    );
    p.text(
        if scene > 0 {
            "Folds into a compact edge Drawer"
        } else if double_click {
            "Titlebar 2x click: Toggles roll-up"
        } else {
            "Titlebar 2x click: Off (use ... menu)"
        },
        info_x,
        stage.y + 100.,
        12.,
        MUTED,
    );
    p.text(
        if dock {
            "4-side screen-edge Drawer: Active"
        } else {
            "4-side screen-edge Drawer: Off above"
        },
        info_x,
        stage.y + 126.,
        12.,
        MUTED,
    );
}
