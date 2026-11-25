use super::*;

impl App {
    pub(super) fn settings_page_portal(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32, open_modal_from_settings: &mut Option<Dialog>) {
        p.text("Portal", x, y - 4., 23., TEXT);
        p.text(
            "View and update the folders linked to your boxes.",
            x,
            y + 32.,
            14.,
            MUTED,
        );
        let entries: Vec<(u64, usize, String, String)> = self
            .state
            .groups
            .iter()
            .flat_map(|g| {
                g.tabs.iter().enumerate().filter_map(move |(ti, tab)| {
                    tab.folder
                        .as_ref()
                        .map(|path| (g.id, ti, tab.name.clone(), path.clone()))
                })
            })
            .collect();
        p.text(
            &format!("Linked folders ({})", entries.len()),
            x,
            y + 90.,
            16.,
            TEXT,
        );
        let visible = 5usize;
        let max_offset = entries.len().saturating_sub(visible);
        self.settings_offset = self.settings_offset.min(max_offset);
        let mut unlink = None;
        let mut open_path = None;
        let mut change = None;
        for (offset, (gid, ti, name, path)) in entries
            .iter()
            .enumerate()
            .skip(self.settings_offset)
            .take(visible)
        {
            let yy = y + 125. + (offset - self.settings_offset) as f32 * 67.;
            p.text(&p.fit(name, 14., w - 242.), x, yy, 14., TEXT);
            p.text(&p.fit(path, 11., w - 242.), x, yy + 22., 11., MUTED);
            if p.button(
                "Open",
                Bounds {
                    x: x + w - 232.,
                    y: yy,
                    w: 64.,
                    h: 32.,
                },
                false,
            ) {
                open_path = Some(path.clone());
            }
            if p.button(
                "Change",
                Bounds {
                    x: x + w - 160.,
                    y: yy,
                    w: 80.,
                    h: 32.,
                },
                false,
            ) {
                change = Some((*gid, *ti, path.clone()));
            }
            if p.button(
                "Unlink",
                Bounds {
                    x: x + w - 72.,
                    y: yy,
                    w: 72.,
                    h: 32.,
                },
                false,
            ) {
                unlink = Some((*gid, *ti));
            }
        }
        if let Some(path) = open_path {
            if let Err(e) = platform::open(&path) {
                self.notify(e, input.time);
            }
        }
        if let Some((gid, ti, path)) = change {
            if let Some(g) = self.state.group_mut(gid) {
                g.active = ti;
            }
            *open_modal_from_settings = Some(Dialog::BindFolder {
                group: gid,
                value: path,
            });
            self.edit_focus = 0;
            self.select_all = true;
        }
        if let Some((gid, ti)) = unlink {
            self.remember();
            if let Some(tab) =
                self.state.group_mut(gid).and_then(|g| g.tabs.get_mut(ti))
            {
                tab.folder = None;
                tab.history.clear();
                tab.items.clear();
                tab.scroll = 0;
            }
            self.selected = None;
            self.notify("Folder link removed; files remain on disk", input.time);
        }
        if entries.is_empty() {
            p.text(
                "No linked folders. Connect one through a box menu.",
                x,
                y + 146.,
                14.,
                MUTED,
            );
        }
        if p.button(
            "Previous",
            Bounds {
                x,
                y: y + 480.,
                w: 118.,
                h: 34.,
            },
            false,
        ) {
            self.settings_offset = self.settings_offset.saturating_sub(visible);
        }
        if p.button(
            "Next",
            Bounds {
                x: x + 132.,
                y: y + 480.,
                w: 118.,
                h: 34.,
            },
            false,
        ) {
            self.settings_offset = (self.settings_offset + visible).min(max_offset);
        }
        p.text(
            &format!(
                "{}–{} of {}",
                if entries.is_empty() {
                    0
                } else {
                    self.settings_offset + 1
                },
                (self.settings_offset + visible).min(entries.len()),
                entries.len()
            ),
            x + 275.,
            y + 490.,
            12.,
            MUTED,
        );
    }
}
