use crate::*;
use super::preview_common::*;

/// Live vector preview for Peek (hotkey bring-groups-to-front animation).
pub(super) fn draw_peek_preview(
    p: &Painter,
    b: Bounds,
    time: f64,
    hotkeys_enabled: bool,
    shortcut: &str,
    active_now: bool,
) {
    let stage = draw_preview_card(
        p,
        b,
        "Peek above windows preview",
        if active_now {
            "Peeking now"
        } else if hotkeys_enabled {
            "Hotkey armed"
        } else {
            "Hotkey off"
        },
    );
    round(stage, 6., Color::new(0.035, 0.060, 0.105, 0.96));

    let phase = ((time % 4.0) / 4.0) as f32;
    let peek_amt = if active_now {
        1.0
    } else if !hotkeys_enabled {
        0.0
    } else if phase < 0.25 {
        0.0
    } else if phase < 0.38 {
        preview_smoothstep(0.25, 0.38, phase)
    } else if phase < 0.72 {
        1.0
    } else if phase < 0.85 {
        1. - preview_smoothstep(0.72, 0.85, phase)
    } else {
        0.0
    };

    // Draw open application windows behind/in front depending on `peek_amt`.
    let draw_apps = |dim: f32| {
        let win1 = Bounds {
            x: stage.x + 24.,
            y: stage.y + 18.,
            w: 220.,
            h: 112.,
        };
        let win2 = Bounds {
            x: stage.x + 165.,
            y: stage.y + 34.,
            w: 225.,
            h: 114.,
        };
        for (win, title) in [(win1, "Browser / Docs"), (win2, "Code Editor")] {
            round(win, 6., Color::new(0.14, 0.16, 0.21, 0.94 * dim));
            round(
                Bounds { h: 20., ..win },
                5.,
                Color::new(0.20, 0.23, 0.29, 0.96 * dim),
            );
            draw_rectangle_lines(win.x, win.y, win.w, win.h, 1., alpha(WHITE, 0.12 * dim));
            p.text(title, win.x + 10., win.y + 4., 10., alpha(TEXT, dim));
        }
    };
    let draw_groups = |elevated: bool, glow: f32| {
        let lift = glow * 4.;
        let b1 = Bounds {
            x: stage.x + 42. - lift * 0.5,
            y: stage.y + 44. - lift,
            w: 145.,
            h: 92.,
        };
        let b2 = Bounds {
            x: stage.x + 215. - lift * 0.5,
            y: stage.y + 24. - lift,
            w: 155.,
            h: 92.,
        };
        for (grp, name) in [(b1, "Pinned Work"), (b2, "Shortcuts")] {
            if elevated {
                round(
                    Bounds {
                        x: grp.x + 4.,
                        y: grp.y + 6.,
                        ..grp
                    },
                    7.,
                    alpha(BLACK, 0.42 * glow),
                );
            }
            round(grp, 6., Color::new(0.10, 0.16, 0.25, 0.96));
            round(Bounds { h: 22., ..grp }, 5., Color::new(0.15, 0.25, 0.38, 0.98));
            draw_rectangle_lines(
                grp.x,
                grp.y,
                grp.w,
                grp.h,
                if elevated { 1.5 } else { 1. },
                if elevated { alpha(ACCENT, 0.85) } else { alpha(WHITE, 0.14) },
            );
            p.text(name, grp.x + 9., grp.y + 4., 11., TEXT);
            draw_mini_icon_grid(grp.x + 12., grp.y + 30., 4, 2, ACCENT, 0.95);
        }
    };

    if peek_amt >= 0.45 {
        draw_apps(1.0 - peek_amt * 0.58);
        draw_groups(true, peek_amt);
    } else {
        draw_groups(false, 0.0);
        draw_apps(1.0 - peek_amt * 0.58);
    }

    // Keycap indicator on right side
    let rx = stage.x + 408.;
    p.text("Hold shortcut:", rx, stage.y + 24., 12., MUTED);
    let mut kx = rx;
    let ky = stage.y + 46. + if peek_amt > 0.4 { 2. } else { 0. };
    for part in shortcut.split('+') {
        let key = part.trim();
        let kw = (p.measure(key, 12.) + 14.).max(32.);
        if kx + kw > stage.x + stage.w - 8. {
            break;
        }
        round(
            Bounds {
                x: kx,
                y: ky,
                w: kw,
                h: 26.,
            },
            4.,
            if peek_amt > 0.4 {
                alpha(ACCENT, 0.32)
            } else {
                Color::new(0.14, 0.19, 0.27, 1.)
            },
        );
        draw_rectangle_lines(
            kx,
            ky,
            kw,
            26.,
            1.,
            if peek_amt > 0.4 { ACCENT } else { alpha(WHITE, 0.18) },
        );
        p.text(key, kx + 7., ky + 6., 12., TEXT);
        kx += kw + 6.;
    }
    p.text(
        if peek_amt > 0.4 {
            "Groups & icons brought in front"
        } else if hotkeys_enabled {
            "Other windows cover desktop"
        } else {
            "Enable hotkeys to preview Peek"
        },
        rx,
        stage.y + 92.,
        12.,
        if peek_amt > 0.4 { ACCENT } else { MUTED },
    );
}
