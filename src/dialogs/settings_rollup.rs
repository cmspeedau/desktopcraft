use super::*;

impl App {
    pub(super) fn settings_page_rollup(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32) {
        p.text("Roll-up", x, y, 23., TEXT);
        p.text(
            "Collapse a group into its title to keep icons tucked away.",
            x,
            y + 36.,
            14.,
            MUTED,
        );
        if p.checkbox(
            "Enabled",
            Bounds {
                x,
                y: y + 72.,
                w,
                h: 34.,
            },
            self.state.prefs.rollup,
        ) {
            self.state.prefs.rollup = !self.state.prefs.rollup;
        }
        if p.checkbox(
            "Double-click a group's title to roll it up or down",
            Bounds {
                x,
                y: y + 112.,
                w,
                h: 34.,
            },
            self.state.prefs.rollup_double_click,
        ) {
            self.state.prefs.rollup_double_click =
                !self.state.prefs.rollup_double_click;
        }
        p.text("A rolled-up group opens", x, y + 162., 15., TEXT);
        let modes: Vec<String> = [
            "On mouse over, immediately",
            "On mouse over, after a short delay",
            "When its title is clicked",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(choice) = self.dropdown(
            p,
            2,
            Bounds {
                x,
                y: y + 188.,
                w: 330.,
                h: 32.,
            },
            &modes,
            self.state.prefs.rollup_open.min(2) as usize,
        ) {
            self.state.prefs.rollup_open = choice as u8;
        }
        if p.checkbox(
            "Drawer: roll up groups placed at screen edges",
            Bounds {
                x,
                y: y + 238.,
                w,
                h: 34.,
            },
            self.state.prefs.rollup_dock,
        ) {
            self.state.prefs.rollup_dock = !self.state.prefs.rollup_dock;
        }
        draw_rollup_preview(
            p,
            Bounds {
                x,
                y: y + 292.,
                w: w - 18.,
                h: 244.,
            },
            input.time,
            self.state.prefs.rollup,
            self.state.prefs.rollup_double_click,
            self.state.prefs.rollup_open,
            self.state.prefs.rollup_dock,
        );
    }
}
