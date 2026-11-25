use super::*;

impl App {
    /// Typing into a Folder View's filter: Enter keeps it, Escape clears it.
    pub(crate) fn filter_typing(&mut self, input: &Input) -> bool {
        let Some(id) = self.filter_focus else {
            return false;
        };
        let usable = !self.has_blocking_dialog()
            && self
                .state
                .group(id)
                .is_some_and(|g| g.tab().folder.is_some());
        if !usable {
            self.filter_focus = None;
            return false;
        }
        let tab = self.state.group_mut(id).unwrap().tab_mut();
        if input.key(KeyCode::Escape) {
            tab.filter.clear();
            tab.scroll = 0;
            self.filter_focus = None;
            return true;
        }
        if input.key(KeyCode::Enter) || input.key(KeyCode::KpEnter) {
            self.filter_focus = None;
            return true;
        }
        let mut text = std::mem::take(&mut tab.filter);
        let before = text.clone();
        self.text_edit(&mut text, input);
        text.truncate(text.char_indices().nth(80).map_or(text.len(), |(i, _)| i));
        let tab = self.state.group_mut(id).unwrap().tab_mut();
        if text != before {
            tab.scroll = 0;
        }
        tab.filter = text;
        true
    }
    /// The Folder View sort list opened from the sort button in its folder bar.
    pub(crate) fn folder_sort_list(&mut self, p: &mut Painter, input: &Input) {
        let Some((id, at)) = self.folder_sort_menu else {
            return;
        };
        let Some(current) = self.state.group(id).map(|g| g.tab().sort) else {
            self.folder_sort_menu = None;
            return;
        };
        let screen = self.menu_monitor_bounds(at);
        let h = 30. + 4. * 33. + 6.;
        let b = Bounds {
            x: (at.x - 180.).clamp(
                screen.x + 5.,
                (screen.x + screen.w - 213.).max(screen.x + 5.),
            ),
            y: (at.y + 16.).clamp(
                screen.y + 5.,
                (screen.y + screen.h - h - 5.).max(screen.y + 5.),
            ),
            w: 208.,
            h,
        };
        self.popup_rects.push(b);
        round(
            Bounds {
                x: b.x + 4.,
                y: b.y + 7.,
                ..b
            },
            8.,
            alpha(BLACK, 0.22),
        );
        round(b, 7., SURFACE);
        p.push_region(b, 7.);
        p.enabled = true;
        p.text("Sort files by", b.x + 13., b.y + 9., 12., MUTED);
        let mut choice = None;
        for (i, label) in ["Name (A–Z)", "File type", "Date (newest)", "Size (largest)"]
            .iter()
            .enumerate()
        {
            let row = Bounds {
                x: b.x + 5.,
                y: b.y + 30. + i as f32 * 33.,
                w: b.w - 10.,
                h: 32.,
            };
            if p.hover(row) {
                round(row, 4., Color::new(0.18, 0.26, 0.35, 1.));
                if input.pressed {
                    choice = Some(i as u8);
                }
            }
            let on = current == i as u8;
            if on {
                draw_line(
                    row.x + 9.,
                    row.y + 17.,
                    row.x + 14.,
                    row.y + 22.,
                    2.,
                    ACCENT,
                );
                draw_line(
                    row.x + 14.,
                    row.y + 22.,
                    row.x + 23.,
                    row.y + 10.,
                    2.,
                    ACCENT,
                );
            }
            p.text(
                label,
                row.x + 32.,
                row.y + 7.,
                14.,
                if on { ACCENT } else { TEXT },
            );
        }
        if let Some(sort) = choice {
            self.remember();
            if let Some(g) = self.state.group_mut(id) {
                g.tab_mut().sort = sort;
                g.tab_mut().sort_items();
                g.tab_mut().scroll = 0;
            }
            self.folder_sort_menu = None;
            self.notify(
                [
                    "Sorted by name",
                    "Sorted by type",
                    "Sorted by newest date",
                    "Sorted by largest size",
                ][sort as usize],
                input.time,
            );
        } else if input.pressed && !b.contains(input.pos.x, input.pos.y) {
            self.folder_sort_menu = None;
        }
    }
}
