use super::*;

impl App {
    pub(super) fn settings_page_saved_layouts(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32) {
        p.text("Keep a layout you like", x, y, 23., TEXT);
        p.text(
            "Restore groups, tabs and pages from an earlier layout.",
            x,
            y + 39.,
            14.,
            MUTED,
        );
        if p.button(
            "Save current layout",
            Bounds {
                x,
                y: y + 78.,
                w: 190.,
                h: 38.,
            },
            true,
        ) {
            let name = format!("Layout {}", self.state.snapshots.len() + 1);
            self.state.snapshot(&name);
            self.notify("Layout saved", input.time);
        }
        let mut restore = None;
        let mut remove = None;
        self.settings_offset = self
            .settings_offset
            .min(self.state.snapshots.len().saturating_sub(5));
        for (i, s) in self
            .state
            .snapshots
            .iter()
            .enumerate()
            .rev()
            .skip(self.settings_offset)
            .take(5)
        {
            let row = self.state.snapshots.len() - 1 - i - self.settings_offset;
            let yy = y + 145. + row as f32 * 64.;
            round(
                Bounds {
                    x,
                    y: yy,
                    w,
                    h: 54.,
                },
                6.,
                Color::new(0.11, 0.17, 0.24, 1.),
            );
            p.text(&p.fit(&s.name, 15., w - 210.), x + 13., yy + 8., 15., TEXT);
            p.text(
                &format!("{} groups  ·  {} pages", s.groups.len(), s.pages.len()),
                x + 13.,
                yy + 29.,
                11.,
                MUTED,
            );
            if p.button(
                "Restore",
                Bounds {
                    x: x + w - 150.,
                    y: yy + 10.,
                    w: 90.,
                    h: 32.,
                },
                false,
            ) {
                restore = Some(i)
            }
            if p.button(
                "×",
                Bounds {
                    x: x + w - 46.,
                    y: yy + 10.,
                    w: 32.,
                    h: 32.,
                },
                false,
            ) {
                remove = Some(i)
            }
        }
        if let Some(i) = restore {
            self.request_restore_snapshot(i, input.time);
        }
        if let Some(i) = remove {
            self.state.snapshots.remove(i);
        }
        if p.button(
            "‹ Newer",
            Bounds {
                x,
                y: y + 488.,
                w: 100.,
                h: 34.,
            },
            false,
        ) {
            self.settings_offset = self.settings_offset.saturating_sub(5);
        }
        if p.button(
            "Older ›",
            Bounds {
                x: x + 112.,
                y: y + 488.,
                w: 100.,
                h: 34.,
            },
            false,
        ) {
            self.settings_offset = (self.settings_offset + 5)
                .min(self.state.snapshots.len().saturating_sub(5));
        }
    }
}
