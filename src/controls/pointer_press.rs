use super::*;

impl App {
    /// True means the original pointer dispatcher must return immediately.
    pub(super) fn pointer_press(&mut self, input: &Input, hit: Option<Target>) -> bool {
        if input.pressed {
            self.search_focus = false;
            // Clicking anywhere but the filter itself stops typing into it
            // (the filter stays applied until it is cleared).
            if !matches!(
                hit,
                Some(Target::FilterField(_)) | Some(Target::FolderFilter(_))
            ) {
                self.filter_focus = None;
            }
            let double_click = platform::double_click_interval();
            let double = self.last_click.as_ref().is_some_and(|(target, time, pos)| {
                Some(target) == hit.as_ref()
                    && input.time - time <= double_click
                    && pos.distance(input.pos) < DRAG_THRESHOLD
            });
            if let Some(ref h) = hit {
                self.last_click = if double {
                    None
                } else {
                    Some((h.clone(), input.time, input.pos))
                };
            } else {
                let blank = Target::Body(0);
                let dbl = self.last_click.as_ref().is_some_and(|(t, time, pos)| {
                    *t == blank
                        && input.time - time <= double_click
                        && pos.distance(input.pos) < DRAG_THRESHOLD
                });
                self.last_click = Some((blank, input.time, input.pos));
                // Window mode has its own blank canvas. In desktop mode only
                // the OS hook may toggle quick-hide, after checking Explorer's
                // actual desktop (never arbitrary application windows).
                if dbl && self.state.quick_hide && self.host.is_none() {
                    self.last_click = None;
                    self.hidden = !self.hidden;
                    self.drag = None;
                    return true;
                }
            }
            match hit.clone() {
                Some(Target::Header(id)) => {
                    self.selected = Some((id, 0));
                    if double {
                        if input.ctrl {
                            if let Some(path) =
                                self.state.group(id).and_then(|g| g.tab().folder.clone())
                            {
                                self.open(&path, input.time)
                            }
                        } else if self.state.prefs.rollup && self.state.prefs.rollup_double_click {
                            self.remember();
                            let rolled_now = {
                                let g = self.state.group_mut(id).unwrap();
                                g.rolled = !g.rolled;
                                g.rolled
                            };
                            if rolled_now {
                                self.dock_reveal_block = Some(id);
                                if self.revealed == Some(id) {
                                    self.revealed = None;
                                }
                                self.hover_rolled = None;
                            }
                        }
                        return true;
                    }
                    // Roll-up "open on click": a click on a rolled group's title opens it.
                    if self.state.prefs.rollup_open == 2
                        && self.state.group(id).is_some_and(|g| g.rolled)
                    {
                        self.revealed = Some(id);
                    }
                    let g = self.state.group(id).unwrap();
                    if !g.locked && !self.state.prefs.lock_layout {
                        self.drag = Some(Drag::Group {
                            id,
                            start: input.pos,
                            before: g.bounds,
                            before_dock: g.dock,
                        });
                        self.remember();
                    }
                }
                // Native scrollbar track above / below its thumb: one row.
                Some(Target::NativePagePrev(id)) => {
                    if std::env::var("DESKTOPCRAFT_NATIVE_INBOX_LAYOUT").ok()
                        .as_deref()==Some("1") {
                        self.queue_native_scroll(id,-1);
                    }
                }
                Some(Target::NativePageNext(id)) => {
                    if std::env::var("DESKTOPCRAFT_NATIVE_INBOX_LAYOUT").ok()
                        .as_deref()==Some("1") {
                        self.queue_native_scroll(id,1);
                    }
                }
                Some(Target::NativeScroll {
                    group,
                    track_y,
                    track_h,
                    thumb_y,
                    thumb_h,
                    positions,
                }) => {
                    if std::env::var("DESKTOPCRAFT_NATIVE_INBOX_LAYOUT").ok()
                        .as_deref()==Some("1")
                        || self.native_pages.iter().any(|s| s.0 == group)
                    {
                        let grab = (input.pos.y - thumb_y).clamp(0., thumb_h);
                        let row = paint::native_scroll_row_at(
                            input.pos.y, track_y, track_h, thumb_h, positions, grab,
                        );
                        self.drag = Some(Drag::NativeScroll {
                            group, track_y, track_h, thumb_h, positions, grab, row,
                        });
                        if self.host.is_some() {
                            host::capture_pointer(true);
                        }
                    }
                }
                Some(Target::TabNext(id)) => {
                    if let Some(g) = self.state.group_mut(id) {
                        g.active = (g.active + 1) % g.tabs.len();
                        self.selected = Some((id, 0));
                    }
                }
                Some(Target::Tab(id, index)) => {
                    let tab_id = self.state.group(id).unwrap().tabs[index].id;
                    let is_rolled = self.state.group(id).is_some_and(|g| g.rolled);
                    self.state.group_mut(id).unwrap().active = index;
                    self.selected = Some((id, 0));
                    if self.state.prefs.rollup && self.state.prefs.rollup_open == 2 && is_rolled {
                        self.revealed = Some(id);
                        if self.host.is_some() && self.native_env.chrome_only {
                            self.rollup_interact_until = input.time + 0.50;
                        }
                    }
                    if input.shift {
                        if !self.state.prefs.lock_layout
                            && !self.state.group(id).is_some_and(|g| g.locked)
                        {
                            let bounds = self.clamp_bounds_to_monitor_work(
                                Bounds {
                                    x: self.state.group(id).unwrap().bounds.x + 35.,
                                    y: self.state.group(id).unwrap().bounds.y + 55.,
                                    ..self.state.group(id).unwrap().bounds
                                },
                            );
                            let can_detach = self.state.group(id).is_some_and(|g| g.tabs.len() > 1);
                            if can_detach {
                                self.remember();
                                if let Some(new) = self.state.detach(id, index, bounds) {
                                    self.drag = Some(Drag::Group {
                                        id: new,
                                        start: input.pos,
                                        before: self.state.group(new).unwrap().bounds,
                                        before_dock: None,
                                    });
                                } else {
                                    self.undo.pop();
                                }
                            }
                        }
                    } else if self.state.prefs.drag_tab_to_rearrange {
                        self.drag = Some(Drag::Tab {
                            group: id,
                            tab_id,
                            start: input.pos,
                        });
                    }
                }
                Some(Target::Roll(id)) => {
                    self.remember();
                    let rolled_now = {
                        let g = self.state.group_mut(id).unwrap();
                        g.rolled = !g.rolled;
                        g.rolled
                    };
                    if rolled_now {
                        self.dock_reveal_block = Some(id);
                        if self.revealed == Some(id) {
                            self.revealed = None;
                        }
                        self.hover_rolled = None;
                    }
                }
                Some(Target::Menu(id)) => {
                    // Hold and drag the "…" button to move the box: when tabs
                    // fill the title bar it's the one handle always left. A
                    // plain click still opens the menu, on release.
                    self.selected = Some((id, 0));
                    let g = self.state.group(id).unwrap();
                    let movable = !g.locked && !self.state.prefs.lock_layout;
                    if movable {
                        self.drag = Some(Drag::Group {
                            id,
                            start: input.pos,
                            before: g.bounds,
                            before_dock: g.dock,
                        });
                        self.remember();
                    }
                    self.menu_press = Some((id, input.pos, movable));
                }
                Some(Target::FolderFilter(id)) => {
                    let active = self.filter_focus == Some(id)
                        || self
                            .state
                            .group(id)
                            .is_some_and(|g| !g.tab().filter.is_empty());
                    if active {
                        // A second click puts the filter away and shows every file.
                        if let Some(g) = self.state.group_mut(id) {
                            g.tab_mut().filter.clear();
                            g.tab_mut().scroll = 0;
                        }
                        self.filter_focus = None;
                    } else {
                        self.filter_focus = Some(id);
                        self.select_all = false;
                        self.selected = Some((id, 0));
                        #[cfg(windows)]
                        if self.host.is_some() && self.native_env.chrome_only {
                            host::focus_portal_filter();
                        }
                    }
                }
                Some(Target::FilterField(id)) => {
                    self.filter_focus = Some(id);
                    self.select_all = false;
                    self.selected = Some((id, 0));
                    #[cfg(windows)]
                    if self.host.is_some() && self.native_env.chrome_only {
                        host::focus_portal_filter();
                    }
                }
                Some(Target::FolderSort(id)) => {
                    self.selected = Some((id, 0));
                    self.folder_sort_menu = Some((id, input.pos));
                }
                Some(Target::Resize(id, edges)) => {
                    if !self.state.prefs.lock_layout
                        && !self.state.group(id).is_some_and(|g| g.locked)
                    {
                        self.remember();
                        self.drag = Some(Drag::Resize {
                            id,
                            start: input.pos,
                            before: self.state.group(id).unwrap().bounds,
                            edges,
                        });
                    }
                }
                Some(Target::Item(group, item)) => {
                    let tab_id = self.state.group(group).map(|g| g.tab().id).unwrap_or(0);
                    let key = (group, tab_id, item);
                    if input.ctrl {
                        // Ctrl-click toggles individual pins in the selection.
                        if self.multi_selected.is_empty() {
                            if let Some((gid, iid)) = self.selected {
                                if gid == group && iid != 0 && iid != item {
                                    self.multi_selected.push((gid, tab_id, iid));
                                }
                            }
                        }
                        if self.multi_selected.contains(&key) {
                            self.multi_selected.retain(|entry| *entry != key);
                        } else {
                            self.multi_selected.push(key);
                        }
                    } else if input.shift {
                        // Shift-click selects a contiguous range in this tab.
                        let previous = self
                            .selected
                            .and_then(|(gid, iid)| (gid == group).then_some(iid));
                        let positions = self.state.group(group).map(|g| &g.tab().items);
                        if let Some(items) = positions {
                            let end = items.iter().position(|i| i.id == item);
                            let begin =
                                previous.and_then(|iid| items.iter().position(|i| i.id == iid));
                            if let (Some(a), Some(b)) = (begin, end) {
                                self.multi_selected = (a.min(b)..=a.max(b))
                                    .map(|idx| (group, tab_id, items[idx].id))
                                    .collect();
                            } else {
                                self.multi_selected = vec![key];
                            }
                        }
                    } else if !self.multi_selected.contains(&key) {
                        self.multi_selected.clear();
                    }
                    self.selected = if input.ctrl && self.multi_selected.is_empty() {
                        Some((group, 0))
                    } else {
                        Some((group, item))
                    };
                    let native_regular_overlay = self.native_env.chrome_only
                        && self.state.group(group).is_some_and(|g| g.tab().folder.is_none());
                    if double && !input.ctrl && !input.shift {
                        self.multi_selected.clear();
                        self.activate_item(group, item, input.time);
                        self.drag = None;
                    } else if !input.ctrl && !input.shift && !native_regular_overlay {
                        self.drag = Some(Drag::Item {
                            group,
                            item,
                            start: input.pos,
                        });
                        if self.host.is_some() {
                            host::capture_pointer(true);
                        }
                    } else {
                        self.drag = None;
                    }
                }
                Some(Target::Back(id)) => {
                    let tab = self.state.group_mut(id).unwrap().tab_mut();
                    if tab.history.is_empty() {
                        if let Some(parent) = tab
                            .folder
                            .as_ref()
                            .and_then(|x| Path::new(x).parent())
                            .map(|x| x.to_string_lossy().to_string())
                        {
                            if let Err(e) = tab.navigate(&parent) {
                                self.notify(e, input.time)
                            }
                        }
                    } else if let Err(e) = tab.back() {
                        self.notify(e, input.time)
                    }
                }
                Some(Target::Open(id)) => {
                    if let Some(path) = self.state.group(id).and_then(|g| g.tab().folder.clone()) {
                        self.open(&path, input.time)
                    }
                }
                Some(Target::Scroll {
                    group,
                    track_y,
                    track_h,
                    thumb_y,
                    thumb_h,
                    total,
                    page,
                    cols,
                }) => {
                    // Grab the thumb where it was pressed; a press on the track
                    // centres the thumb there (and keeps following the pointer).
                    let grab = if input.pos.y >= thumb_y && input.pos.y <= thumb_y + thumb_h {
                        input.pos.y - thumb_y
                    } else {
                        thumb_h / 2.
                    };
                    let row = scroll_row_at(input.pos.y, track_y, track_h, total, page, grab);
                    if let Some(g) = self.state.group_mut(group) {
                        g.tab_mut().scroll = row * cols;
                    }
                    self.drag = Some(Drag::Scroll {
                        group,
                        track_y,
                        track_h,
                        total,
                        page,
                        cols,
                        grab,
                    });
                }
                Some(Target::Loose(_)) => {}
                Some(Target::Body(id)) => {
                    self.selected = Some((id, 0));
                    if double {
                        if let Some(path) =
                            self.state.group(id).and_then(|g| g.tab().folder.clone())
                        {
                            self.open(&path, input.time);
                        }
                    } else if let Some(tab) = self.state.group(id).map(|g| g.tab()) {
                        let native_regular_overlay = self.native_env.chrome_only && tab.folder.is_none();
                        if tab.folder.is_none() && !native_regular_overlay {
                            self.multi_selected.clear();
                            self.drag = Some(Drag::BoxSelect {
                                group: id,
                                tab_id: tab.id,
                                start: input.pos,
                            });
                            if self.host.is_some() {
                                host::capture_pointer(true);
                            }
                        }
                    }
                }
                None => {
                    if !self.state.prefs.lock_layout && input.pos.y > 124. && input.pos.y < self.height - 60. {
                        self.drag = Some(Drag::Create { start: input.pos });
                        self.selected = None;
                    }
                }
            }
        }
        false
    }
}
