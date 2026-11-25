use super::*;

impl App {
    pub(super) fn settings_page_displays(&mut self, p: &mut Painter, x: f32, y: f32, w: f32) {
        p.text("Displays", x, y, 23., TEXT);
        p.text(
            "Choose which monitor hosts DesktopCraft' panels and controls.",
            x,
            y + 36.,
            14.,
            MUTED,
        );
        p.text(
            "This does not change Windows' primary monitor or move your files.",
            x,
            y + 60.,
            13.,
            MUTED,
        );
        if let Some(h) = &self.host {
            let monitors = h.monitors.clone();
            let selected_key = self.state.prefs.workspace_monitor.clone();
            let actual_primary = monitors
                .iter()
                .find(|m| m.primary)
                .map(|m| m.key.as_str())
                .unwrap_or("");
            if p.button(
                "Auto-detect Windows primary monitor",
                Bounds {
                    x,
                    y: y + 99.,
                    w: 400.,
                    h: 38.,
                },
                selected_key.is_empty(),
            ) {
                self.state.prefs.workspace_monitor.clear();
                self.settings_origin = None;
            }
            for (index, monitor) in monitors.iter().take(7).enumerate() {
                let display = format!(
                    "Display {}  |  {} × {}  |  {}{}",
                    index + 1,
                    monitor.rect[2],
                    monitor.rect[3],
                    monitor.key,
                    if monitor.primary {
                        "  (Windows primary)"
                    } else {
                        ""
                    }
                );
                if p.button(
                    &p.fit(&display, 14., w - 18.),
                    Bounds {
                        x,
                        y: y + 150. + index as f32 * 43.,
                        w: w - 18.,
                        h: 36.,
                    },
                    selected_key == monitor.key,
                ) {
                    self.state.prefs.workspace_monitor = monitor.key.clone();
                    self.settings_origin = None;
                }
            }
            let list_bottom = y + 150. + monitors.len().min(7) as f32 * 43.;
            if selected_key.is_empty() {
                p.text(
                    &format!("Currently following {}", actual_primary),
                    x,
                    list_bottom + 4.,
                    12.,
                    MUTED,
                );
            } else if !monitors.iter().any(|m| m.key == selected_key) {
                p.text(
                    "Saved display is disconnected; using Windows primary until it returns.",
                    x,
                    list_bottom + 4.,
                    12.,
                    MUTED,
                );
            }
            if monitors.len() <= 4 {
                let preview_y = list_bottom + 28.;
                let preview_h = (y + 528. - preview_y).clamp(112., 156.);
                draw_displays_preview(
                    p,
                    Bounds {
                        x,
                        y: preview_y,
                        w: w - 18.,
                        h: preview_h,
                    },
                    &monitors,
                    &selected_key,
                );
            }
        } else {
            p.text("Available in Windows desktop mode. Display selection is automatic here.", x, y + 115., 14., MUTED);
        }
    }
}
