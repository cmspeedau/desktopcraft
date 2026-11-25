use super::*;

impl App {
    pub(super) fn settings_page_pages(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32, close: &mut bool, open_modal_from_settings: &mut Option<Dialog>) {
        p.text("Pages", x, y, 23., TEXT);
        p.text(
            "Keep separate screens of groups and switch between them.",
            x,
            y + 32.,
            14.,
            MUTED,
        );
        if p.checkbox(
            "Enable pages",
            Bounds {
                x,
                y: y + 58.,
                w: 220.,
                h: 32.,
            },
            self.state.prefs.pages,
        ) {
            self.state.prefs.pages = !self.state.prefs.pages;
        }
        if p.checkbox(
            "Show page bar on desktop",
            Bounds {
                x: x + 240.,
                y: y + 58.,
                w: w - 240.,
                h: 32.,
            },
            self.state.prefs.show_page_bar,
        ) {
            self.state.prefs.show_page_bar = !self.state.prefs.show_page_bar;
        }
        let view_h = self.primary_view().h.max(400.);
        let min_off = -(view_h - 66.);
        let max_off = 20.0f32;
        p.text("Page bar height", x, y + 105., 14., TEXT);
        if p.button(
            "Move up",
            Bounds {
                x: x + 138.,
                y: y + 98.,
                w: 102.,
                h: 30.,
            },
            false,
        ) {
            self.state.prefs.page_bar_offset_y =
                (self.state.prefs.page_bar_offset_y - 36.0).clamp(min_off, max_off);
        }
        if p.button(
            "Move down",
            Bounds {
                x: x + 248.,
                y: y + 98.,
                w: 112.,
                h: 30.,
            },
            false,
        ) {
            self.state.prefs.page_bar_offset_y =
                (self.state.prefs.page_bar_offset_y + 36.0).clamp(min_off, max_off);
        }
        if p.button(
            "Reset",
            Bounds {
                x: x + 368.,
                y: y + 98.,
                w: 78.,
                h: 30.,
            },
            self.state.prefs.page_bar_offset_y.abs() < 0.5,
        ) {
            self.state.prefs.page_bar_offset_y = 0.0;
        }
        let norm = ((self.state.prefs.page_bar_offset_y - min_off)
            / (max_off - min_off).max(1.0))
            .clamp(0.0, 1.0);
        if let Some(v) = slider(
            p,
            input,
            "Top ↔ Bottom",
            norm,
            x,
            y + 134.,
            w - 18.,
        ) {
            self.state.prefs.page_bar_offset_y =
                (min_off + v * (max_off - min_off)).clamp(min_off, max_off);
        }
        p.text(
            "Switch with the page bar on your selected display or Ctrl / Alt + Left / Right.",
            x,
            y + 168.,
            12.,
            MUTED,
        );
        p.text(
            "Manage pages · rename any page, delete only empty pages",
            x,
            y + 194.,
            14.,
            TEXT,
        );
        let count = self.state.pages.len();
        self.settings_offset = self.settings_offset.min(count.saturating_sub(4));
        let mut deleting = None;
        let mut renaming = None;
        for index in self.settings_offset..count.min(self.settings_offset + 4) {
            let row_y = y + 224. + (index - self.settings_offset) as f32 * 47.;
            let assigned =
                self.state.groups.iter().filter(|g| g.page == index).count();
            let label = format!(
                "{}  {}  ({} boxes)",
                index + 1,
                self.state.pages[index],
                assigned
            );
            if p.button(
                &p.fit(&label, 14., 324.),
                Bounds {
                    x,
                    y: row_y,
                    w: 330.,
                    h: 34.,
                },
                self.state.page == index,
            ) {
                self.state.page = index;
                self.selected = None;
                if let Some(h) = self.host.as_mut() {
                    h.last_sync = -10.;
                }
            }
            if p.button(
                "Rename",
                Bounds {
                    x: x + 338.,
                    y: row_y,
                    w: 83.,
                    h: 34.,
                },
                false,
            ) {
                renaming = Some(index);
            }
            if assigned == 0 && count > 1 {
                if p.button(
                    "Delete",
                    Bounds {
                        x: x + 431.,
                        y: row_y,
                        w: 84.,
                        h: 34.,
                    },
                    false,
                ) {
                    deleting = Some(index);
                }
            } else {
                p.text(
                    if count <= 1 { "Required" } else { "In use" },
                    x + 439.,
                    row_y + 9.,
                    12.,
                    MUTED,
                );
            }
        }
        if let Some(index) = renaming {
            *open_modal_from_settings = Some(Dialog::RenamePage {
                index,
                value: self.state.pages[index].clone(),
            });
            self.edit_focus = 0;
            self.select_all = true;
        }
        if let Some(index) = deleting {
            let page_name =
                self.state.pages.get(index).cloned().unwrap_or_default();
            let approve = !self.automation && host::ask_default_no(&format!(
                "Delete empty page '{}' (page {})?\n\nOnly the page will be removed. Your files remain unchanged.\n\nChoose No to keep it.",
                page_name, index + 1));
            if approve {
                self.remember();
                match self.state.delete_empty_page(index) {
                    Ok(()) => {
                        self.notify(
                            "Empty page deleted; other page numbers adjusted",
                            input.time,
                        );
                        if let Some(h) = self.host.as_mut() {
                            h.last_sync = -10.;
                        }
                    }
                    Err(reason) => self.notify(reason, input.time),
                }
            }
        }
        if p.button(
            "+ New page",
            Bounds {
                x,
                y: y + 430.,
                w: 142.,
                h: 34.,
            },
            false,
        ) {
            self.request_new_page();
            *close = true;
        }
        if p.button(
            "‹",
            Bounds {
                x: x + 166.,
                y: y + 430.,
                w: 36.,
                h: 34.,
            },
            false,
        ) {
            self.settings_offset = self.settings_offset.saturating_sub(4);
        }
        if p.button(
            "›",
            Bounds {
                x: x + 210.,
                y: y + 430.,
                w: 36.,
                h: 34.,
            },
            false,
        ) {
            self.settings_offset = (self.settings_offset + 4)
                .min(self.state.pages.len().saturating_sub(4));
        }
    }
}
