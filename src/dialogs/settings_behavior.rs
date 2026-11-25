use super::*;

impl App {
    pub(super) fn settings_page_behavior(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32, close: &mut bool) {
        p.text("Behavior", x, y - 4., 23., TEXT);
        if p.checkbox(
            "Align and snap box edges to nearby boxes (hold Shift to move freely)",
            Bounds {
                x,
                y: y + 28.,
                w,
                h: 28.,
            },
            self.state.snap,
        ) {
            self.state.snap = !self.state.snap
        }
        if p.checkbox(
            "Lock layout (prevent moving or resizing boxes)",
            Bounds {
                x,
                y: y + 58.,
                w,
                h: 28.,
            },
            self.state.prefs.lock_layout,
        ) {
            self.state.prefs.lock_layout = !self.state.prefs.lock_layout;
        }
        if p.checkbox(
            "Enable drag tab to rearrange",
            Bounds {
                x,
                y: y + 88.,
                w,
                h: 28.,
            },
            self.state.prefs.drag_tab_to_rearrange,
        ) {
            self.state.prefs.drag_tab_to_rearrange =
                !self.state.prefs.drag_tab_to_rearrange;
        }
        if p.checkbox(
            "Double-click blank space to hide groups",
            Bounds {
                x,
                y: y + 118.,
                w,
                h: 28.,
            },
            self.state.quick_hide,
        ) {
            self.state.quick_hide = !self.state.quick_hide
        }
        draw_quick_hide_preview(
            p,
            Bounds {
                x,
                y: y + 150.,
                w: w - 18.,
                h: 120.,
            },
            input.time,
            self.state.quick_hide,
        );
        p.text("After moving a file between tabs", x, y + 278., 14., TEXT);
        if p.button(
            "Stay on source tab",
            Bounds {
                x,
                y: y + 298.,
                w: 226.,
                h: 30.,
            },
            !self.state.prefs.switch_tab_after_drop,
        ) {
            self.state.prefs.switch_tab_after_drop = false;
        }
        if p.button(
            "Switch to destination",
            Bounds {
                x: x + 238.,
                y: y + 298.,
                w: 226.,
                h: 30.,
            },
            self.state.prefs.switch_tab_after_drop,
        ) {
            self.state.prefs.switch_tab_after_drop = true;
        }
        draw_tab_drop_preview(
            p,
            Bounds {
                x,
                y: y + 334.,
                w: w - 18.,
                h: 124.,
            },
            input.time,
            self.state.prefs.switch_tab_after_drop,
        );
        if self.host.is_some() {
            p.text("On your Windows desktop", x, y + 468., 15., TEXT);
            let on = host::autostart(None);
            if p.checkbox(
                "Start DesktopCraft when I sign in to Windows",
                Bounds {
                    x,
                    y: y + 490.,
                    w,
                    h: 30.,
                },
                on,
            ) {
                host::autostart(Some(!on));
            }
        } else {
            p.text("Desktop window", x, y + 468., 15., TEXT);
            if p.button(
                if self.desktop {
                    "Return to normal window"
                } else {
                    "Use desktop window"
                },
                Bounds {
                    x,
                    y: y + 490.,
                    w: 224.,
                    h: 30.,
                },
                false,
            ) {
                self.desktop = !self.desktop;
                if !self.automation {
                    platform::desktop(self.desktop, &mut self.previous)
                }
                *close = true;
            }
            p.text(
                "Press Escape to return to a normal window.",
                x + 238.,
                y + 497.,
                13.,
                MUTED,
            );
        }
    }
}
