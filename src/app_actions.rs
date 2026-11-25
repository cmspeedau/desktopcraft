use crate::*;

impl App {
    pub(super) fn create_group(&mut self, bounds: Bounds) {
        self.remember();
        let id = self.state.add_group("New group", bounds);
        self.selected = Some((id, 0));
        self.open_modal_preserving_settings(Dialog::Rename {
            group: id,
            value: "New group".into(),
        });
        self.select_all = true;
        self.edit_focus = 0;
    }
    pub(super) fn clear_orphaned_rename_dialog(&mut self) {
        if matches!(
            self.dialog,
            Some(Dialog::Rename { group, .. }) if self.state.group(group).is_none()
        ) {
            self.dialog = None;
        }
    }
    pub(super) fn native_point_hits_box_or_loose_icon(&self, sx: i32, sy: i32) -> bool {
        if self.native_visible_loose_rects.iter().any(|&[x, y, w, h]| {
            sx >= x && sy >= y && sx < x + w && sy < y + h
        }) {
            return true;
        }
        let chrome_native = self.host.is_some() && self.native_env.chrome_only;
        let visual_revealed = self.visual_revealed(chrome_native);
        let pos = self.from_screen(sx, sy);
        self.state.groups.iter().any(|g| {
            if g.page != self.state.page || (self.hidden && !g.keep_visible) {
                return false;
            }
            let rolled = g.rolled && self.state.prefs.rollup;
            let expanded = !rolled || visual_revealed == Some(g.id);
            let area = if expanded {
                paint::native_group_outer(g)
            } else {
                paint::native_group_header(g)
            };
            area.contains(pos.x, pos.y)
        })
    }
    pub(super) fn open(&mut self, path: &str, time: f64) {
        if !Path::new(path).exists() {
            self.notify("This item is no longer at its saved location.", time);
            return;
        }
        if self.automation {
            self.opened.push(path.into());
            self.notify("Opened sample item", time)
        } else {
            match platform::open(path) {
                Ok(()) => self.notify("Opened in its default application", time),
                Err(e) => self.notify(e, time),
            }
        }
    }
    pub(super) fn pick_file(&mut self, time: f64) {
        if self.group_id().is_none() {
            if self.native_env.chrome_only && self.native_env.general_workspace && !self.automation
            {
                if let Some(id) = self
                    .state
                    .groups
                    .iter()
                    .find(|g| g.page == self.state.page && g.tab().folder.is_none())
                    .or_else(|| self.state.groups.iter().find(|g| g.tab().folder.is_none()))
                    .map(|g| g.id)
                {
                    self.selected = Some((id, 0));
                } else {
                    self.notify("Create a regular Box first.", time);
                    return;
                }
            } else {
                let bounds = self.fresh_bounds();
                let id = self.state.add_group("My files", bounds);
                self.selected = Some((id, 0));
            }
        }
        if self.automation {
            self.open_modal_preserving_settings(Dialog::Path {
                portal: false,
                value: String::new(),
            });
            return;
        }
        if let Some(path) = platform::choose_file() {
            self.import_path(&path, false, time)
        }
    }
    pub(super) fn pick_folder(&mut self, time: f64) {
        if self.automation {
            return;
        }
        if let Some(path) = platform::choose_folder() {
            self.import_path(&path, true, time)
        }
    }
    pub(super) fn import_path(&mut self, path: &Path, portal: bool, time: f64) {
        // Explicit file import follows the destination's semantics too:
        // normal box = pin only, linked Folder View = physical move.
        if !portal && self.host.is_some() {
            if let Some(id) = self.group_id() {
                if let Err(e) = self.drop_path_into_box(&path.to_string_lossy(), id, time) {
                    self.notify(e, time);
                }
                return;
            }
        }
        self.remember();
        let result = if portal {
            self.state.add_portal(path, self.fresh_bounds()).map(|id| {
                self.selected = Some((id, 0));
            })
        } else if let Some(id) = self.group_id() {
            self.state.unassign(&path.to_string_lossy());
            self.state.import(path, id)
        } else {
            Err("Create a group first.".into())
        };
        match result {
            Ok(()) => self.notify(
                if portal {
                    "Folder connected. Double-click a subfolder to browse."
                } else {
                    "File pinned. Double-click to open."
                },
                time,
            ),
            Err(e) => self.notify(e, time),
        }
    }
    pub(super) fn switch_page(&mut self, direction: i32) {
        let previous = self.state.page;
        if direction > 0 && self.state.page + 1 < self.state.pages.len() {
            self.state.page += 1;
        } else if direction < 0 && self.state.page > 0 {
            self.state.page -= 1;
        }
        self.selected = None;
        self.multi_selected.clear();
        self.menu = None;
        if self.host.is_some() && self.state.page != previous {
            host::event(
                "INFO",
                "pages.switched",
                &format!(
                    "from={} to={} total={}",
                    previous + 1,
                    self.state.page + 1,
                    self.state.pages.len()
                ),
            );
        }
        if self.state.page != previous {
            if let Some(h) = self.host.as_mut() {
                h.last_sync = -10.;
            }
        }
    }
    pub(super) fn request_new_page(&mut self) {
        self.open_modal_preserving_settings(Dialog::NewPage {
            value: format!("Space {}", self.state.pages.len() + 1),
        });
        self.edit_focus = 0;
        self.select_all = true;
    }
    pub(super) fn set_peek(&mut self, on: bool) {
        self.peek = on;
        if self.hidden {
            self.set_hidden(false);
        }
        // Desktop mode raises and lowers the window in host_after_frame.
        if !self.automation && self.host.is_none() {
            platform::peek(on, self.desktop)
        }
    }
    pub(super) fn activate_item(&mut self, group: u64, item: u64, time: f64) {
        let Some(g) = self.state.group(group) else {
            return;
        };
        let Some(item) = g.tab().items.iter().find(|i| i.id == item).cloned() else {
            return;
        };
        if item.kind == "folder" && g.tab().folder.is_some() {
            if let Err(e) = self
                .state
                .group_mut(group)
                .unwrap()
                .tab_mut()
                .navigate(&item.path)
            {
                self.notify(e, time)
            }
        } else {
            self.open(&item.path, time)
        }
    }
}
