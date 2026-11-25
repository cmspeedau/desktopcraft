use super::*;

impl App {
    pub(crate) fn shortcuts(&mut self, input: &Input) {
        if self.inline_rename_typing(input) {
            return;
        }
        if self.filter_typing(input) {
            return;
        }
        if input.key(KeyCode::Escape) {
            self.folder_sort_menu = None;
            self.menu = None;
            self.search_focus = false;
            if matches!(self.dialog, Some(Dialog::EditRule { .. })) {
                // Escape leaves the rule pop-up for the rules page it came from.
                self.dialog = Some(Dialog::Settings {
                    page: 2,
                    pattern: String::new(),
                    mode: 0,
                    target: 0,
                });
                self.settings_bring_to_front = true;
            } else if self.has_blocking_dialog() {
                self.dialog = None;
                if let Some(ret) = self.settings_return.take() {
                    self.dialog = Some(ret);
                    self.settings_bring_to_front = true;
                }
            } else if self.dialog.is_some() && !self.settings_in_own_window() {
                self.dialog = None;
            } else if self.peek {
                self.set_peek(false)
            } else if self.desktop {
                self.desktop = false;
                if !self.automation {
                    platform::desktop(false, &mut self.previous);
                }
            } else {
                self.search.clear();
            }
            self.drag = None;
            return;
        }
        // An unmodified-letter Peek chord (e.g. Z+X+C) must not also
        // trigger type-ahead, search or other box keyboard shortcuts.
        if self.has_blocking_dialog() || self.peek {
            return;
        }
        // Type-ahead selection behaves like Explorer. A focused box reacts to
        // ordinary letters (Latin or Thai) without opening global search.
        if !input.ctrl && !input.alt && !self.search_focus && !input.typed.is_empty() {
            if let Some(id) = self.group_id().filter(|id| {
                self.state
                    .group(*id)
                    .is_some_and(|g| g.page == self.state.page)
            }) {
                if self.type_group != Some(id) || input.time - self.typed_at > 1.2 {
                    self.typed_prefix.clear();
                }
                self.type_group = Some(id);
                self.typed_at = input.time;
                self.typed_prefix.push_str(&input.typed.to_lowercase());
                let matching = |prefix: &str| {
                    self.state.group(id).and_then(|g| {
                        g.tab()
                            .items
                            .iter()
                            .enumerate()
                            .find(|(_, item)| item.name.to_lowercase().starts_with(prefix))
                            .map(|(index, item)| (index, item.id))
                    })
                };
                let mut found = matching(&self.typed_prefix);
                if found.is_none() {
                    self.typed_prefix = input.typed.to_lowercase();
                    found = matching(&self.typed_prefix);
                }
                if let Some((index, item_id)) = found {
                    self.selected = Some((id, item_id));
                    self.scroll_item_into_view(id, index);
                }
                return;
            }
        }
        if input.ctrl && input.key(KeyCode::F) {
            self.search_focus = true;
            self.select_all = true;
            return;
        }
        if self.search_focus {
            let mut s = std::mem::take(&mut self.search);
            self.text_edit(&mut s, input);
            self.search = s;
            if input.key(KeyCode::Enter) {
                self.search_focus = false
            }
            return;
        }
        if input.ctrl && input.key(KeyCode::N) {
            self.create_group(self.fresh_bounds());
        }
        if input.ctrl && input.key(KeyCode::O) {
            if input.shift {
                self.pick_folder(input.time)
            } else {
                self.pick_file(input.time)
            }
        }
        if input.ctrl && input.key(KeyCode::S) {
            self.open_modal_preserving_settings(Dialog::Snapshot {
                value: format!("Layout {}", self.state.snapshots.len() + 1),
            });
            self.select_all = true;
            self.edit_focus = 0;
        }
        if input.ctrl && input.key(KeyCode::Z) {
            self.request_undo(input.time);
        }
        // Only bottom page selector + Ctrl/Alt + arrows switch pages.
        // On the Windows desktop, keyboard input is polled only when its own
        // shell/our app has focus (no interference with other apps).
        if self.state.prefs.pages {
            if self.host.is_some() && !self.automation {
                match platform::page_hotkey() {
                    1 => self.switch_page(1),
                    -1 => self.switch_page(-1),
                    _ => {}
                }
            } else if (input.ctrl || input.alt) && !input.shift {
                if input.key(KeyCode::Right) {
                    self.switch_page(1);
                }
                if input.key(KeyCode::Left) {
                    self.switch_page(-1);
                }
            }
        }
        // Windows-standard Enter and Alt+Enter for loose icons and Box pins.
        // Settings and text fields retain their own keyboard handling.
        if !self.has_blocking_dialog() && !self.search_focus && self.filter_focus.is_none() {
            let loose = self.host.as_ref().and_then(|h| h.selected_loose.clone());
            let pinned = self.selected.and_then(|(gid, iid)| {
                self.state.group(gid).and_then(|g| {
                    g.tab()
                        .items
                        .iter()
                        .find(|it| it.id == iid)
                        .map(|it| it.path.clone())
                })
            });
            // Alt+Enter opens the Windows Properties sheet, as on the desktop.
            if input.alt
                && !input.ctrl
                && (input.key(KeyCode::Enter) || input.key(KeyCode::KpEnter))
            {
                if let Some(path) = loose.clone().or(pinned.clone()) {
                    self.file_action(&path, FileAction::Properties, input.time);
                    return;
                }
            }
            if !input.ctrl && !input.alt && input.key(KeyCode::Enter) {
                if let Some(path) = loose {
                    self.open(&path, input.time);
                    return;
                }
            }
        }
        // Ctrl+V: selected Box -> its active Tab, selected loose Desktop icon
        // or no selected Box -> the signed-in user's real Desktop folder.
        if !self.has_blocking_dialog()
            && !self.search_focus
            && self.filter_focus.is_none()
            && input.ctrl
            && input.key(KeyCode::V)
        {
            let on_desktop = self
                .host
                .as_ref()
                .is_some_and(|h| h.selected_loose.is_some());
            let group = if on_desktop {
                None
            } else {
                self.selected.map(|(g, _)| g)
            };
            self.paste_file_clipboard(group, input.time);
            return;
        }
        // Real Windows file clipboard from both Box pins and loose icons.
        // Do not intercept Ctrl+C/X from Settings or an active text editor.
        if !self.has_blocking_dialog()
            && !self.search_focus
            && self.filter_focus.is_none()
            && input.ctrl
            && (input.key(KeyCode::C) || input.key(KeyCode::X))
        {
            let loose = self.host.as_ref().and_then(|h| h.selected_loose.clone());
            let pinned = self.selected.and_then(|(gid, iid)| {
                self.state.group(gid).and_then(|g| {
                    g.tab()
                        .items
                        .iter()
                        .find(|i| i.id == iid)
                        .map(|i| i.path.clone())
                })
            });
            if let Some(path) = loose.or(pinned) {
                self.file_action(
                    &path,
                    if input.key(KeyCode::X) {
                        FileAction::Cut
                    } else {
                        FileAction::Copy
                    },
                    input.time,
                );
                return;
            }
        }
        // F2 on a selected icon renames it in place (as on the Windows desktop);
        // F2 on a box renames the box.
        let selected_icon = self.selected.filter(|(g, i)| {
            *i != 0
                && self
                    .state
                    .group(*g)
                    .is_some_and(|g| g.tab().items.iter().any(|it| it.id == *i))
        });
        if input.key(KeyCode::F2)
            && self.host.as_ref().is_some_and(|h| {
                h.selected_loose
                    .as_deref()
                    .is_some_and(|p| Path::new(p).exists())
            })
        {
            let path = self
                .host
                .as_ref()
                .and_then(|h| h.selected_loose.clone())
                .unwrap();
            self.start_inline_rename_loose(&path);
        } else if input.key(KeyCode::F2) && selected_icon.is_some() {
            let (group, item) = selected_icon.unwrap();
            self.start_inline_rename(group, item);
        } else if input.key(KeyCode::F2)
            && self
                .host
                .as_ref()
                .is_some_and(|h| h.selected_loose.is_some())
        {
            // Virtual shell item: do not accidentally rename a Box.
        } else if input.key(KeyCode::F2) {
            if let Some(id) = self.group_id() {
                self.open_modal_preserving_settings(Dialog::Rename {
                    group: id,
                    value: self.state.group(id).unwrap().tab().name.clone(),
                });
                self.select_all = true;
                self.edit_focus = 0;
            }
        }
        if input.key(KeyCode::F1) {
            self.dialog = Some(Dialog::Settings {
                page: 4,
                pattern: String::new(),
                mode: 0,
                target: 0,
            });
            self.settings_bring_to_front = true;
        }
        // Desktop mode: Delete on a selected desktop icon sends it to the Recycle Bin.
        if input.key(KeyCode::Delete) {
            if let Some(path) = self.host.as_ref().and_then(|h| h.selected_loose.clone()) {
                if let Err(e) = shell::recycle(&path) {
                    self.notify(e, input.time);
                }
                if let Some(h) = self.host.as_mut() {
                    h.last_sync = -10.;
                    h.selected_loose = None;
                }
                return;
            }
        }
        if let Some((group, item)) = self.selected {
            if input.key(KeyCode::Delete) && self.host.is_some() {
                // Desktop mode: Delete sends the real item to the Recycle Bin,
                // exactly like on the desktop (Windows confirms as usual).
                let path = self
                    .state
                    .group(group)
                    .and_then(|g| g.tab().items.iter().find(|i| i.id == item))
                    .map(|i| i.path.clone());
                if let Some(path) = path {
                    if let Err(e) = shell::recycle(&path) {
                        self.notify(e, input.time);
                    }
                    if let Some(h) = self.host.as_mut() {
                        h.last_sync = -10.;
                    }
                }
            } else if input.key(KeyCode::Delete) {
                if self
                    .state
                    .group(group)
                    .is_some_and(|g| g.tab().folder.is_none())
                {
                    self.remember();
                    self.state
                        .group_mut(group)
                        .unwrap()
                        .tab_mut()
                        .items
                        .retain(|i| i.id != item);
                    self.notify("Pin removed from group", input.time);
                }
            }
            if input.key(KeyCode::Enter) {
                self.activate_item(group, item, input.time);
            }
            if input.shift
                && [KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down]
                    .iter()
                    .any(|&k| input.key(k))
            {
                self.remember();
                if let Some(g) = self.state.group_mut(group) {
                    if !g.locked {
                        let d = if input.ctrl { 20. } else { 8. };
                        g.bounds.x += (if input.key(KeyCode::Right) { d } else { 0. })
                            - (if input.key(KeyCode::Left) { d } else { 0. });
                        g.bounds.y += (if input.key(KeyCode::Down) { d } else { 0. })
                            - (if input.key(KeyCode::Up) { d } else { 0. });
                    }
                }
            }
        }
        if input.key(KeyCode::Tab) {
            let groups: Vec<u64> = self
                .state
                .groups
                .iter()
                .filter(|g| g.page == self.state.page)
                .map(|g| g.id)
                .collect();
            if !groups.is_empty() {
                let idx = self
                    .group_id()
                    .and_then(|id| groups.iter().position(|x| *x == id))
                    .map_or(0, |i| (i + 1) % groups.len());
                self.selected = Some((groups[idx], 0));
            }
        }
        if !input.ctrl
            && !input.shift
            && [KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down]
                .iter()
                .any(|&k| input.key(k))
        {
            if let Some(id) = self.group_id() {
                let g = self.state.group(id).unwrap();
                if !g.tab().items.is_empty() {
                    let tab = g.tab();
                    let cols = if tab.list {
                        1
                    } else {
                        ((g.bounds.w - 20.) / (cell_w() * tab.icon_scale))
                            .floor()
                            .max(1.) as usize
                    };
                    let index = self
                        .selected
                        .and_then(|(_, item)| tab.items.iter().position(|x| x.id == item))
                        .unwrap_or(0);
                    let delta = if input.key(KeyCode::Right) {
                        1
                    } else if input.key(KeyCode::Left) {
                        -1
                    } else if input.key(KeyCode::Down) {
                        cols as i32
                    } else {
                        -(cols as i32)
                    };
                    let next = (index as i32 + delta).clamp(0, tab.items.len() as i32 - 1) as usize;
                    self.selected = Some((id, tab.items[next].id));
                    let available =
                        g.bounds.h - HEADER - FOOTER - if tab.folder.is_some() { 38. } else { 12. };
                    let rows = (available
                        / if tab.list {
                            36.
                        } else {
                            cell_h() * tab.icon_scale
                        })
                    .floor()
                    .max(1.) as usize;
                    let count = rows * cols;
                    let tab = self.state.group_mut(id).unwrap().tab_mut();
                    if next < tab.scroll {
                        tab.scroll = next / cols * cols;
                    } else if next >= tab.scroll + count {
                        tab.scroll = (next / cols + 1).saturating_sub(rows) * cols;
                    }
                }
            }
        }
    }
}
