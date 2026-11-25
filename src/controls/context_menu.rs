use super::*;

/// Return only currently usable actions for the box ellipsis menu.
/// IDs deliberately match the action dispatcher; hidden items consume no
/// menu height or input hitbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoxMenuEntry {
    Action(usize),
    Separator,
}

fn visible_box_menu_entries(
    tab_count: usize,
    linked_folder: bool,
    selected_pin: bool,
    native_regular_box: bool,
) -> Vec<BoxMenuEntry> {
    let groups: [&[usize]; 5] = [
        &[1, 16, 2, 3, 9, 10],
        &[0, 15, 8, 7],
        &[13, 14],
        &[4, 5, 6],
        &[12, 11],
    ];
    let mut entries = Vec::new();
    for group in groups {
        let items: Vec<usize> = group
            .iter()
            .copied()
            .filter(|i| match i {
                3 => !native_regular_box,
                8 => tab_count > 1,
                9 => linked_folder || selected_pin,
                10 => !linked_folder && !native_regular_box && selected_pin,
                13 => linked_folder,
                _ => true,
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        if !entries.is_empty() {
            entries.push(BoxMenuEntry::Separator);
        }
        entries.extend(items.into_iter().map(BoxMenuEntry::Action));
    }
    entries
}

#[cfg(test)]
fn visible_box_menu_indices(
    tab_count: usize,
    linked_folder: bool,
    selected_pin: bool,
    native_regular_box: bool,
) -> Vec<usize> {
    visible_box_menu_entries(tab_count, linked_folder, selected_pin, native_regular_box)
        .into_iter()
        .filter_map(|entry| match entry {
            BoxMenuEntry::Action(id) => Some(id),
            BoxMenuEntry::Separator => None,
        })
        .collect()
}

#[cfg(test)]
mod menu_visibility_tests {
    use super::*;
    #[test]
    fn empty_regular_box_shows_only_actionable_items() {
        let ids = visible_box_menu_indices(1, false, false, false);
        assert!(ids.contains(&14)); // bind a folder
        assert_eq!(ids.first(), Some(&1)); // Add file in Content group first
        assert!(ids.contains(&15) && ids.contains(&16));
        assert_eq!(ids.last(), Some(&11)); // Remove group... always last
        for i in [8, 9, 10, 13] {
            assert!(!ids.contains(&i));
        }
    }
    #[test]
    fn native_regular_box_hides_list_view_toggle_and_keeps_five_groups() {
        let entries = visible_box_menu_entries(2, false, true, true);
        let sep_count = entries
            .iter()
            .filter(|e| matches!(e, BoxMenuEntry::Separator))
            .count();
        assert_eq!(sep_count, 4);
        assert_ne!(entries.first(), Some(&BoxMenuEntry::Separator));
        assert_ne!(entries.last(), Some(&BoxMenuEntry::Separator));
        assert!(!visible_box_menu_indices(2, false, true, true).contains(&3));
        assert!(!visible_box_menu_indices(2, false, true, true).contains(&10));
        assert!(visible_box_menu_indices(2, false, true, false).contains(&3));
    }
    #[test]
    fn linked_folder_hides_unpin_but_shows_open_and_change_folder() {
        let ids = visible_box_menu_indices(1, true, false, false);
        assert!(ids.contains(&9) && ids.contains(&13) && ids.contains(&14));
        assert!(!ids.contains(&10));
        assert_eq!(ids.last(), Some(&11));
    }
    #[test]
    fn selected_virtual_pin_enables_copy_and_unpin() {
        let ids = visible_box_menu_indices(2, false, true, false);
        assert!(ids.contains(&8) && ids.contains(&9) && ids.contains(&10));
        assert!(!ids.contains(&13));
        assert_eq!(ids.last(), Some(&11));
    }
}

impl App {
    /// Close every pop-up (the "…" menu with its sub-lists, the folder sort
    /// list) and stop typing into a folder filter.
    pub(crate) fn close_popups(&mut self) {
        self.menu = None;
        self.menu_page_open = false;
        self.menu_sort_open = false;
        self.menu_press = None;
        self.folder_sort_menu = None;
        self.filter_focus = None;
        self.popup_rects.clear();
    }
    /// Only the current frame's DesktopCraft file hits are eligible. With
    /// Explorer focus, Windows handles Shift+F10 on its own selection.
    pub(crate) fn keyboard_file_menu(&mut self, input: &Input, hits: &[Hit]) -> bool {
        if !input.shift || input.ctrl || input.alt || !input.key(KeyCode::F10)
            || self.has_blocking_dialog() || self.search_focus || self.filter_focus.is_some()
            || self.inline_rename.is_some() || self.drag.is_some()
        {
            return false;
        }
        let loose = self.host.as_ref().and_then(|h| h.selected_loose.as_deref());
        let hit = hits.iter().rev().find(|hit| match hit.target {
            Target::Loose(index) => self.host.as_ref().and_then(|h| h.loose.get(index))
                .is_some_and(|icon| loose.is_some_and(|path| icon.path.eq_ignore_ascii_case(path))),
            Target::Item(group, item) => loose.is_none() && self.selected == Some((group, item)),
            _ => false,
        });
        hit.is_some_and(|hit| self.queue_classic_file_menu(
            &hit.target,
            vec2(hit.bounds.x + hit.bounds.w / 2., hit.bounds.y + hit.bounds.h / 2.),
        ))
    }

    pub(super) fn queue_classic_file_menu(&mut self, target: &Target, pos: Vec2) -> bool {
        let (path, pin) = match *target {
            Target::Loose(index) => {
                let Some(host) = self.host.as_mut() else { return false };
                let Some(icon) = host.loose.get(index) else { return false };
                host.selected_loose = Some(icon.path.clone());
                self.selected = None;
                (icon.path.clone(), None)
            }
            Target::Item(group, item) => {
                let Some(group_state) = self.state.group(group) else { return false };
                let tab = group_state.tab();
                let Some(file) = tab.items.iter().find(|file| file.id == item) else { return false };
                if let Some(host) = self.host.as_mut() {
                    host.selected_loose = None;
                }
                self.selected = Some((group, item));
                let is_native_regular = self.host.is_some()
                    && self.native_env.chrome_only
                    && !self.automation
                    && tab.folder.is_none();
                (
                    file.path.clone(),
                    (!is_native_regular && tab.folder.is_none()).then_some((group, tab.id, item)),
                )
            }
            _ => return false,
        };
        self.close_popups();
        let anchor = self.host.as_ref().map(|_| {
            let [x, y, ..] = self.to_screen(Bounds { x: pos.x, y: pos.y, w: 0., h: 0. });
            (x, y)
        });
        self.classic_file_menu = Some(desktop::ClassicFileMenu { path, anchor, pin });
        true
    }

    pub(crate) fn run_classic_file_menu(&mut self, time: f64) {
        let Some(menu) = self.classic_file_menu.take() else { return };
        if self.automation {
            return;
        }
        let (x, y) = menu.anchor.unwrap_or_else(host::cursor);
        let choice = shell::classic_context_menu(&menu.path, x, y, menu.pin.is_some());
        self.apply_classic_file_menu_choice(menu, choice, time);
    }

    pub(super) fn apply_classic_file_menu_choice(
        &mut self, menu: desktop::ClassicFileMenu, choice: shell::MenuChoice, time: f64,
    ) {
        match choice {
            shell::MenuChoice::Rename => self.file_action(&menu.path, FileAction::Rename, time),
            shell::MenuChoice::Delete => self.file_action(&menu.path, FileAction::Delete, time),
            shell::MenuChoice::Copy => self.file_action(&menu.path, FileAction::Copy, time),
            shell::MenuChoice::Cut => self.file_action(&menu.path, FileAction::Cut, time),
            shell::MenuChoice::Reveal => self.file_action(&menu.path, FileAction::Reveal, time),
            shell::MenuChoice::Unpin => {
                if self.host.is_some() && self.native_env.chrome_only && !self.automation {
                    self.notify("Drag the icon out to Desktop to unpin.", time);
                    return;
                }
                let Some((group, tab, item)) = menu.pin else { return };
                let matches_pin = |entry: &Item| entry.id == item
                    && entry.path.eq_ignore_ascii_case(&menu.path);
                let Some(index) = self.state.group(group).and_then(|g| g.tabs.iter().position(|t|
                    t.id == tab && t.folder.is_none() && t.items.iter().any(matches_pin)))
                else { return };
                self.remember();
                self.state.group_mut(group).unwrap().tabs[index].items.retain(|entry| !matches_pin(entry));
                self.selected = None;
                self.multi_selected.retain(|&selection| selection != (group, tab, item));
                if let Some(host) = self.host.as_mut() {
                    host.last_sync = -10.;
                }
                self.notify("Pin removed from Box", time);
            }
            shell::MenuChoice::Invoked => {
                if !Path::new(&menu.path).exists() {
                    self.state.unassign(&menu.path);
                    self.selected = None;
                    if let Some(host) = self.host.as_mut() {
                        if host.selected_loose.as_deref().is_some_and(|path|
                            path.eq_ignore_ascii_case(&menu.path))
                        {
                            host.selected_loose = None;
                        }
                    }
                }
            }
            shell::MenuChoice::Nothing => {}
        }
    }

    pub(crate) fn context_menu(&mut self, p: &mut Painter, input: &Input) {
        let Some((id, pos)) = self.menu else { return };
        let Some(g) = self.state.group(id) else {
            self.menu = None;
            return;
        };
        let rows = [
            "Rename group",
            "Add file…",
            "Sort by  ›",
            if g.tab().list {
                "Switch to icon view"
            } else {
                "Switch to list view"
            },
            "Change appearance…",
            if g.locked {
                "Unlock position"
            } else {
                "Lock position"
            },
            if g.keep_visible {
                "Include in quick-hide"
            } else {
                "Keep visible when hidden"
            },
            "Move to page  ›",
            "Separate active tab",
            if g.tab().folder.is_some() {
                "Copy this folder path"
            } else {
                "Copy selected file path"
            },
            "Unpin selected file (keep on disk)",
            "Remove group…",
            "DesktopCraft settings…",
            "Open linked folder",
            if g.tab().folder.is_some() {
                "Change linked folder path…"
            } else {
                "Open portal"
            },
            "Add tab",
            "Paste files",
        ];
        let linked_folder = g.tab().folder.is_some();
        let selected_pin = self.selected.is_some_and(|(gid, item)| {
            gid == id
                && item != 0
                && !linked_folder
                && g.tab().items.iter().any(|x| x.id == item)
        });
        let native_regular_box =
            self.host.is_some() && self.native_env.chrome_only && !linked_folder;
        // Only render actions that can actually be used by this box, grouped cleanly.
        // Stable action IDs are retained for the command dispatcher.
        let entries = visible_box_menu_entries(
            g.tabs.len(),
            linked_folder,
            selected_pin,
            native_regular_box,
        );
        // Clamp to the monitor containing the ellipsis, not the virtual
        // multi-monitor desktop; popup menus must not leak onto the next panel.
        let screen = self.menu_monitor_bounds(pos);
        let content_h: f32 = entries
            .iter()
            .map(|entry| match entry {
                BoxMenuEntry::Action(_) => 33.,
                BoxMenuEntry::Separator => 9.,
            })
            .sum();
        let menu_h = content_h + 14.;
        let b = Bounds {
            x: pos.x.clamp(
                screen.x + 5.,
                (screen.x + screen.w - 327.).max(screen.x + 5.),
            ),
            y: pos.y.clamp(
                screen.y + 5.,
                (screen.y + screen.h - menu_h - 5.).max(screen.y + 5.),
            ),
            w: 322.,
            h: menu_h,
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
        let mut action = None;
        let mut row_y = b.y + 7.;
        let mut sort_row_y = b.y + 7.;
        let mut page_row_y = b.y + 7.;
        for entry in &entries {
            match *entry {
                BoxMenuEntry::Separator => {
                    draw_line(
                        b.x + 12.,
                        row_y + 4.,
                        b.x + b.w - 12.,
                        row_y + 4.,
                        1.,
                        alpha(WHITE, 0.09),
                    );
                    row_y += 9.;
                }
                BoxMenuEntry::Action(i) => {
                    if i == 2 {
                        sort_row_y = row_y;
                    }
                    if i == 7 {
                        page_row_y = row_y;
                    }
                    let row = Bounds {
                        x: b.x + 5.,
                        y: row_y,
                        w: b.w - 10.,
                        h: 32.,
                    };
                    if p.hover(row) {
                        round(
                            row,
                            4.,
                            if i == 11 {
                                Color::new(0.32, 0.14, 0.15, 0.92)
                            } else {
                                Color::new(0.18, 0.26, 0.35, 1.)
                            },
                        );
                        if i == 7 {
                            self.menu_page_open = true;
                            self.menu_sort_open = false;
                        }
                        if i == 2 {
                            self.menu_sort_open = true;
                            self.menu_page_open = false;
                        }
                        if input.pressed && i != 7 && i != 2 {
                            action = Some(i);
                        }
                    }
                    p.text(
                        &p.fit(rows[i], 14., row.w - 24.),
                        row.x + 11.,
                        row.y + 6.,
                        14.,
                        if i == 11 {
                            Color::new(0.97, 0.52, 0.50, 1.0)
                        } else {
                            TEXT
                        },
                    );
                    row_y += 33.;
                }
            }
        }
        // Same side-aware submenu pattern for sorting and moving between pages.
        let mut sort_bounds = None;
        let mut sort_choice = None;
        if self.menu_sort_open {
            let sb = Bounds {
                x: if b.x + b.w + 212. <= screen.x + screen.w {
                    b.x + b.w + 4.
                } else {
                    (b.x - 212.).max(screen.x + 5.)
                },
                y: sort_row_y.clamp(
                    screen.y + 5.,
                    (screen.y + screen.h - 4. * 33. - 16.).max(screen.y + 5.),
                ),
                w: 208.,
                h: 4. * 33. + 10.,
            };
            sort_bounds = Some(sb);
            self.popup_rects.push(sb);
            round(sb, 7., SURFACE);
            p.push_region(sb, 7.);
            for (i, label) in ["Name (A–Z)", "File type", "Date (newest)", "Size (largest)"]
                .iter()
                .enumerate()
            {
                let row = Bounds {
                    x: sb.x + 5.,
                    y: sb.y + 5. + i as f32 * 33.,
                    w: sb.w - 10.,
                    h: 32.,
                };
                if p.hover(row) {
                    round(row, 4., Color::new(0.18, 0.26, 0.35, 1.));
                    if input.pressed {
                        sort_choice = Some(i as u8);
                    }
                }
                let current = self
                    .state
                    .group(id)
                    .is_some_and(|g| g.tab().sort == i as u8);
                // The old checkmark glyph was missing on some Windows fonts.
                // Draw a real vector tick instead of a Unicode fallback square.
                if current {
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
                    if current { ACCENT } else { TEXT },
                );
            }
        }
        // Move-to-page lists existing destinations only. Page creation is
        // intentionally restricted to the confirmed + action in the pager.
        let mut submenu_bounds = None;
        let mut page_choice = None;
        if self.menu_page_open {
            let total = self.state.pages.len();
            let visible = total.min(((self.height - 120.) / 33.).floor().max(3.) as usize);
            self.menu_page_scroll = self.menu_page_scroll.min(total.saturating_sub(visible));
            let sb = Bounds {
                x: if b.x + b.w + 208. <= screen.x + screen.w {
                    b.x + b.w + 4.
                } else {
                    (b.x - 208.).max(screen.x + 5.)
                },
                y: page_row_y.clamp(
                    screen.y + 5.,
                    (screen.y + screen.h - visible as f32 * 33. - 14.).max(screen.y + 5.),
                ),
                w: 204.,
                h: visible as f32 * 33. + 10.,
            };
            submenu_bounds = Some(sb);
            self.popup_rects.push(sb);
            if sb.contains(input.pos.x, input.pos.y) && input.scroll != 0. {
                if input.scroll < 0. {
                    self.menu_page_scroll =
                        (self.menu_page_scroll + 1).min(total.saturating_sub(visible));
                } else {
                    self.menu_page_scroll = self.menu_page_scroll.saturating_sub(1);
                }
            }
            round(sb, 7., SURFACE);
            p.push_region(sb, 7.);
            for offset in 0..visible {
                let i = self.menu_page_scroll + offset;
                let row = Bounds {
                    x: sb.x + 5.,
                    y: sb.y + 5. + offset as f32 * 33.,
                    w: sb.w - 10.,
                    h: 32.,
                };
                if p.hover(row) {
                    round(row, 4., Color::new(0.18, 0.26, 0.35, 1.));
                    if input.pressed {
                        page_choice = Some(i);
                    }
                }
                let label = format!("{}  {}", i + 1, self.state.pages[i]);
                p.text(
                    &p.fit(&label, 14., row.w - 18.),
                    row.x + 9.,
                    row.y + 7.,
                    14.,
                    if i == self.state.page { ACCENT } else { TEXT },
                );
            }
        }
        if input.pressed
            && !b.contains(input.pos.x, input.pos.y)
            && !submenu_bounds.is_some_and(|sb| sb.contains(input.pos.x, input.pos.y))
            && !sort_bounds.is_some_and(|sb| sb.contains(input.pos.x, input.pos.y))
        {
            self.menu = None;
            self.menu_page_open = false;
            self.menu_sort_open = false;
            return;
        }
        if let Some(sort) = sort_choice {
            self.remember();
            if let Some(g) = self.state.group_mut(id) {
                g.tab_mut().sort = sort;
                g.tab_mut().sort_items();
                g.tab_mut().scroll = 0;
            }
            self.menu = None;
            self.menu_sort_open = false;
            self.notify(
                [
                    "Sorted by name",
                    "Sorted by type",
                    "Sorted by newest date",
                    "Sorted by largest size",
                ][sort as usize],
                input.time,
            );
            return;
        }
        if let Some(page) = page_choice {
            self.remember();
            if page >= self.state.pages.len() {
                return;
            }
            if let Some(g) = self.state.group_mut(id) {
                g.page = page;
            }
            if let Some(h) = self.host.as_mut() {
                h.last_sync = -10.;
            }
            self.menu = None;
            self.menu_page_open = false;
            self.selected = None;
            self.notify("Group moved to selected page", input.time);
            return;
        }
        if let Some(action) = action {
            if matches!(action, 2 | 3 | 5 | 6 | 8 | 15) {
                self.remember();
            }
            self.menu = None;
            match action {
                16 => self.paste_file_clipboard(Some(id), input.time),
                15 => match self.state.add_tab(id) {
                    Ok(_) => {
                        self.selected = Some((id, 0));
                        self.multi_selected.clear();
                        if let Some(h) = self.host.as_mut() {
                            h.last_sync = -10.;
                        }
                        self.notify("Tab added to this Box", input.time);
                    }
                    Err(e) => {
                        self.undo.pop();
                        self.notify(e, input.time);
                    }
                },
                0 => {
                    self.open_modal_preserving_settings(Dialog::Rename {
                        group: id,
                        value: self.state.group(id).unwrap().tab().name.clone(),
                    });
                    self.select_all = true;
                    self.edit_focus = 0;
                }
                1 => {
                    self.selected = Some((id, 0));
                    self.pick_file(input.time);
                }
                2 => {
                    let tab = self.state.group_mut(id).unwrap().tab_mut();
                    tab.sort = (tab.sort + 1) % 4;
                    tab.sort_items();
                    self.notify(
                        [
                            "Sorted by name",
                            "Sorted by type",
                            "Sorted by newest",
                            "Sorted by largest",
                        ][self.state.group(id).unwrap().tab().sort as usize],
                        input.time,
                    );
                }
                3 => {
                    let tab = self.state.group_mut(id).unwrap().tab_mut();
                    tab.list = !tab.list;
                    tab.scroll = 0;
                }
                4 => {
                    // Opens Appearance with this group already chosen.
                    let index = self
                        .state
                        .groups
                        .iter()
                        .position(|g| g.id == id)
                        .map_or(0, |i| i + 1);
                    self.dialog = Some(Dialog::Settings {
                        page: 0,
                        pattern: String::new(),
                        mode: 0,
                        target: index,
                    });
                    self.settings_bring_to_front = true;
                }
                5 => {
                    let g = self.state.group_mut(id).unwrap();
                    g.locked = !g.locked;
                }
                6 => {
                    let g = self.state.group_mut(id).unwrap();
                    g.keep_visible = !g.keep_visible;
                }
                7 => {
                    if self.state.page + 1 >= self.state.pages.len() {
                        self.state
                            .pages
                            .push(format!("Space {}", self.state.pages.len() + 1));
                    }
                    let next = self.state.page + 1;
                    self.state.group_mut(id).unwrap().page = next;
                    self.notify("Group moved to the next page", input.time);
                }
                8 => {
                    let g = self.state.group(id).unwrap();
                    let b = self.clamp_bounds_to_monitor_work(Bounds {
                        x: g.bounds.x + 40.,
                        y: g.bounds.y + 60.,
                        ..g.bounds
                    });
                    let active = g.active;
                    if self
                        .state
                        .detach(id, active, b)
                        .is_none()
                    {
                        self.undo.pop();
                        self.notify(
                            "Combine two groups first by dragging one header onto another.",
                            input.time,
                        );
                    }
                }
                9 => {
                    let path = self
                        .state
                        .group(id)
                        .unwrap()
                        .tab()
                        .folder
                        .clone()
                        .or_else(|| {
                            self.selected.and_then(|(_, item)| {
                                self.state
                                    .group(id)
                                    .unwrap()
                                    .tab()
                                    .items
                                    .iter()
                                    .find(|i| i.id == item)
                                    .map(|i| i.path.clone())
                            })
                        });
                    if let Some(path) = path {
                        macroquad::miniquad::window::clipboard_set(&path);
                        self.notify("Path copied", input.time)
                    }
                }
                10 => {
                    if self.host.is_some() && self.native_env.chrome_only && !self.automation {
                        self.notify("Drag the icon out to Desktop to unpin.", input.time);
                    } else if let Some((_, item)) = self.selected {
                        if self.state.group(id).is_some_and(|g| g.tab().folder.is_none()) {
                            self.remember();
                            let tab = self.state.group_mut(id).unwrap().tab_mut();
                            tab.items.retain(|i| i.id != item);
                            self.selected = None;
                            self.notify(
                                "Pin removed from this group; the file was not deleted",
                                input.time,
                            );
                        } else {
                            self.notify("Folder view items reflect the folder on disk.", input.time)
                        }
                    }
                }
                11 => self.open_modal_preserving_settings(Dialog::Remove { group: id }),
                12 => {
                    self.dialog = Some(Dialog::Settings {
                        page: 1,
                        pattern: String::new(),
                        mode: 0,
                        target: 0,
                    });
                    self.settings_bring_to_front = true;
                }
                13 => {
                    let location = self.state.group(id).and_then(|g| g.tab().folder.clone());
                    if let Some(path) = location {
                        if let Err(e) = platform::open(&path) {
                            self.notify(e, input.time);
                        }
                    }
                }
                14 => {
                    self.open_modal_preserving_settings(Dialog::BindFolder {
                        group: id,
                        value: self
                            .state
                            .group(id)
                            .and_then(|g| g.tab().folder.clone())
                            .unwrap_or_default(),
                    })
                }
                _ => {}
            }
        }
    }
}
