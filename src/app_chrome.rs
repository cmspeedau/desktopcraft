use crate::*;

impl App {
    pub(super) fn topbar(&mut self, p: &Painter, input: &Input) -> bool {
        draw_rectangle(
            0.,
            0.,
            self.width,
            64.,
            Color::new(0.035, 0.07, 0.115, 0.94),
        );
        draw_line(0., 64., self.width, 64., 1., alpha(WHITE, 0.06));
        for (x, y) in [(22., 22.), (33., 22.), (22., 33.), (33., 33.)] {
            round(Bounds { x, y, w: 8., h: 8. }, 2., ACCENT);
        }
        p.text("DesktopCraft", 52., 18., 20., TEXT);
        let search = Bounds {
            x: 258.,
            y: 15.,
            w: 230.,
            h: 34.,
        };
        round(search, 6., Color::new(0.14, 0.20, 0.28, 0.85));
        draw_circle_lines(273., 30., 5., 1.4, MUTED);
        draw_line(277., 34., 281., 38., 1.4, MUTED);
        p.text(
            &p.fit(
                if self.search.is_empty() {
                    "Find a file…"
                } else {
                    &self.search
                },
                14.,
                181.,
            ),
            290.,
            22.,
            14.,
            if self.search.is_empty() { MUTED } else { TEXT },
        );
        if self.search_focus {
            outline(search, ACCENT);
        }
        if p.enabled && input.pressed && search.contains(input.pos.x, input.pos.y) {
            self.search_focus = true;
            return true;
        }
        let w = self.width;
        if p.button(
            "+ New group",
            Bounds {
                x: w - 652.,
                y: 15.,
                w: 120.,
                h: 34.,
            },
            false,
        ) {
            self.create_group(self.fresh_bounds());
            return true;
        }
        if p.button(
            "Folder view",
            Bounds {
                x: w - 522.,
                y: 15.,
                w: 108.,
                h: 34.,
            },
            false,
        ) {
            self.pick_folder(input.time);
            return true;
        }
        if p.button(
            "Add file",
            Bounds {
                x: w - 404.,
                y: 15.,
                w: 84.,
                h: 34.,
            },
            false,
        ) {
            self.pick_file(input.time);
            return true;
        }
        if p.button(
            if self.peek { "End peek" } else { "Peek" },
            Bounds {
                x: w - 310.,
                y: 15.,
                w: 70.,
                h: 34.,
            },
            false,
        ) {
            self.set_peek(!self.peek);
            return true;
        }
        if p.button(
            "Settings",
            Bounds {
                x: w - 230.,
                y: 15.,
                w: 91.,
                h: 34.,
            },
            false,
        ) {
            self.dialog = Some(Dialog::Settings {
                page: 0,
                pattern: String::new(),
                mode: 0,
                target: 0,
            });
            return true;
        }
        if p.button(
            if self.hidden { "Show all" } else { "Hide all" },
            Bounds {
                x: w - 129.,
                y: 15.,
                w: 106.,
                h: 34.,
            },
            false,
        ) {
            self.hidden = !self.hidden;
            return true;
        }
        input.pressed && input.pos.y < 64.
    }
    /// Desktop page navigator. Stays on the selected DesktopCraft display
    /// (Windows primary by default), regardless of the pointer's location.
    pub(super) fn desktop_pager(&mut self, p: &mut Painter, input: &Input) -> bool {
        let blocking_dialog = self.has_blocking_dialog();
        if !self.state.prefs.pages
            || !self.state.prefs.show_page_bar
            || blocking_dialog
            || self.menu.is_some()
            || self.drag.is_some()
        {
            return false;
        }
        let view = self.primary_view();
        let mut consumed = false;
        self.in_monitor_view(p, input, view, |app, p, input| {
            let bar_y = (app.height - 66. + app.state.prefs.page_bar_offset_y)
                .clamp(6., (app.height - 52.).max(6.));
            let b = Bounds { x: ((app.width - 414.) / 2.).max(5.),
                y: bar_y, w: 414., h: 46. };
            p.push_region(b, 10.);
            round(b, 10., Color::new(0.055, 0.09, 0.14, 0.94));
            let left = Bounds { x: b.x + 8., y: b.y + 7., w: 34., h: 32. };
            let right = Bounds { x: b.x + 227., y: b.y + 7., w: 34., h: 32. };
            let add = Bounds { x: b.x + 268., y: b.y + 7., w: 36., h: 32. };
            let sort = Bounds { x: b.x + 313., y: b.y + 7., w: 92., h: 32. };
            let enabled = !app.has_blocking_dialog() && app.menu.is_none();
            if enabled {
                if app.state.page > 0 {
                    if p.button("‹", left, false) { app.switch_page(-1); }
                } else { p.text("‹", left.x + 12., left.y + 7., 16., MUTED); }
                if app.state.page + 1 < app.state.pages.len() {
                    if p.button("›", right, false) { app.switch_page(1); }
                } else { p.text("›", right.x + 12., right.y + 7., 16., MUTED); }
                if p.button("+", add, false) {
                    app.request_new_page();
                }
                if p.button("Sort icons", sort, false) {
                    app.request_desktop_rule_sort(desktop::RuleSortScope::LooseOnly, input.time);
                }
            } else {
                p.text("‹", left.x + 12., left.y + 7., 16., MUTED);
                p.text("›", right.x + 12., right.y + 7., 16., MUTED);
                p.text("+", add.x + 12., add.y + 7., 16., MUTED);
                p.text("Sort icons", sort.x + 9., sort.y + 7., 14., MUTED);
            }
            let name = &app.state.pages[app.state.page];
            let label = format!("{}  ({}/{})", name, app.state.page + 1, app.state.pages.len());
            p.text(&p.fit(&label, 14., 170.), b.x + 50., b.y + 15., 14., TEXT);
            consumed = input.pressed && b.contains(input.pos.x,input.pos.y);
        });
        consumed
    }
    pub(super) fn footer(&mut self, p: &Painter, input: &Input) -> bool {
        let y = self.height - 48.;
        draw_rectangle(0., y, self.width, 48., Color::new(0.035, 0.07, 0.115, 0.92));
        p.text(
            "Draw to create  ·  Double-click blank space to hide",
            23.,
            y + 17.,
            12.,
            MUTED,
        );
        let page_w = 112.;
        let visible = self.state.pages.len().min(3);
        let first = self
            .state
            .page
            .saturating_sub(1)
            .min(self.state.pages.len().saturating_sub(visible));
        let total = page_w * visible as f32 + 118.;
        let start = (self.width - total) / 2. + 36.;
        let left = Bounds {
            x: start - 36.,
            y: y + 8.,
            w: 28.,
            h: 32.,
        };
        if self.state.page > 0 {
            if p.button("‹", left, false) {
                self.switch_page(-1);
                return true;
            }
        } else {
            p.text("‹", left.x + 9., left.y + 8., 14., MUTED);
        }
        let mut page = None;
        for (i, name) in self
            .state
            .pages
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
        {
            let b = Bounds {
                x: start + (i - first) as f32 * page_w,
                y: y + 8.,
                w: page_w - 6.,
                h: 32.,
            };
            if p.button(name, b, self.state.page == i) {
                page = Some(i)
            }
        }
        if let Some(page) = page {
            self.state.page = page;
            self.selected = None;
            return true;
        }
        let right = Bounds {
            x: start + visible as f32 * page_w,
            y: y + 8.,
            w: 28.,
            h: 32.,
        };
        if self.state.page + 1 < self.state.pages.len() {
            if p.button("›", right, false) {
                self.switch_page(1);
                return true;
            }
        } else {
            p.text("›", right.x + 9., right.y + 8., 14., MUTED);
        }
        if p.button(
            "+",
            Bounds {
                x: start + visible as f32 * page_w + 36.,
                y: y + 8.,
                w: 30.,
                h: 32.,
            },
            false,
        ) {
            self.request_new_page();
            return true;
        }
        draw_circle(
            self.width - 151.,
            y + 24.,
            3.,
            if self.save_ok { COLORS[1] } else { COLORS[4] },
        );
        p.text(
            if self.save_ok {
                "Changes saved"
            } else {
                "Save failed"
            },
            self.width - 140.,
            y + 16.,
            12.,
            MUTED,
        );
        self.stats.status = true;
        input.pressed && input.pos.y >= y
    }
    /// Debounce typed search by 350 ms; clearing it resets immediately.
    pub(super) fn update_search(&mut self, time: f64) {
        if self.search != self.search_seen {
            self.search_seen = self.search.clone();
            self.search_changed = time;
        }
        if self.search.is_empty() {
            self.applied_search.clear();
        } else if self.applied_search != self.search
            && time - self.search_changed >= SEARCH_UPDATE_DELAY
        {
            self.applied_search = self.search.clone();
        }
    }
}
