use super::*;

impl App {
    pub(super) fn pointer_drag_release(&mut self, input: &Input, hits: &[Hit]) {
        if input.released || input.right_released || input.middle_released {
            if self.host.is_some() {
                host::capture_pointer(false);
            }
            // A "…" press that never moved its box is a click: open the menu.
            let menu_press = self.menu_press.take();
            let menu_moved = match (&self.drag, &menu_press) {
                (Some(Drag::Group { id, start, .. }), Some((pressed, _, _))) if id == pressed => {
                    start.distance(input.pos) > DRAG_THRESHOLD
                }
                _ => false,
            };
            if let Some((id, at, added_undo)) = menu_press {
                if !menu_moved && input.released && self.state.group(id).is_some() {
                    if added_undo {
                        self.undo.pop();
                    } // nothing changed
                    self.drag = None;
                    self.menu = Some((id, at));
                    self.menu_page_open = false;
                    self.menu_page_scroll = 0;
                    self.menu_sort_open = false;
                    return;
                }
            }
            match self.drag.take() {
                Some(Drag::BoxSelect {
                    group,
                    tab_id,
                    start,
                }) => {
                    if start.distance(input.pos) > DRAG_THRESHOLD {
                        let area = Bounds {
                            x: start.x.min(input.pos.x),
                            y: start.y.min(input.pos.y),
                            w: (start.x - input.pos.x).abs(),
                            h: (start.y - input.pos.y).abs(),
                        };
                        let still_active = self
                            .state
                            .group(group)
                            .is_some_and(|g| g.tab().id == tab_id && g.tab().folder.is_none());
                        if still_active {
                            self.multi_selected = hits
                                .iter()
                                .filter_map(|h| match h.target {
                                    Target::Item(gid, iid)
                                        if gid == group
                                            && area.x < h.bounds.x + h.bounds.w
                                            && area.x + area.w > h.bounds.x
                                            && area.y < h.bounds.y + h.bounds.h
                                            && area.y + area.h > h.bounds.y =>
                                    {
                                        Some((group, tab_id, iid))
                                    }
                                    _ => None,
                                })
                                .collect();
                            self.multi_selected.sort_unstable();
                            self.multi_selected.dedup();
                            self.selected = self
                                .multi_selected
                                .last()
                                .map(|(gid, _, iid)| (*gid, *iid))
                                .or(Some((group, 0)));
                        }
                    }
                }
                Some(Drag::Tab {
                    group,
                    tab_id,
                    start,
                }) => {
                    if self.state.prefs.drag_tab_to_rearrange
                        && start.distance(input.pos) > DRAG_THRESHOLD
                    {
                        let target = self
                            .state
                            .group(group)
                            .and_then(|g| {
                                if self.native_env.chrome_only {
                                    paint::native_header_tab_at(g, (input.pos.x, input.pos.y))
                                } else {
                                    g.drop_tab_at(input.pos.x, input.pos.y)
                                }
                            });
                        if let Some(target) = target {
                            let from = self
                                .state
                                .group(group)
                                .and_then(|g| g.tabs.iter().position(|tab| tab.id == tab_id));
                            if from != Some(target) {
                                self.remember();
                                if self.state.reorder_tab(group, tab_id, target) {
                                    self.selected = Some((group, 0));
                                    self.notify("Tab order updated", input.time);
                                }
                            }
                        }
                    }
                }
                Some(Drag::Group { id, start, before, before_dock }) => {
                    if start.distance(input.pos) > DRAG_THRESHOLD {
                        let docked = self.move_group(id, start, before, before_dock, input, true);
                        if !self.native_env.chrome_only {
                            self.dock_if_at_edge(id);
                        }
                        if let Some(h) = self.host.as_mut() {
                            h.last_sync = -10.;
                        }
                        self.last_click = None;
                        if docked { return; }
                        let target = self
                            .state
                            .groups
                            .iter()
                            .rev()
                            .find(|g| {
                                g.id != id
                                    && g.page == self.state.page
                                    && rollup_header_bounds(g, self.native_env.chrome_only)
                                        .contains(input.pos.x, input.pos.y)
                            })
                            .map(|g| g.id);
                        if let Some(target) = target {
                            if self.state.merge(id, target) {
                                self.selected = Some((target, 0));
                                self.notify(
                                    "Groups combined as tabs. Shift-click a tab to separate it.",
                                    input.time,
                                )
                            }
                        }
                    } else if let Some(group) = self.state.group_mut(id) {
                        group.bounds = before;
                        group.dock = before_dock;
                    }
                }
                Some(Drag::Item { group, item, start }) => {
                    if start.distance(input.pos) > DRAG_THRESHOLD {
                        self.last_click = None;
                        // A tab strip is a drop target even when its tab is inactive.
                        let tab_target = hits.iter().rev().find_map(|h| {
                            if !h.bounds.contains(input.pos.x, input.pos.y) {
                                return None;
                            }
                            if let Target::Tab(gid, index) = &h.target {
                                Some((*gid, *index))
                            } else {
                                None
                            }
                        });
                        let target = self
                            .state
                            .groups
                            .iter()
                            .rev()
                            .find(|g| {
                                g.page == self.state.page
                                    && g.bounds.contains(input.pos.x, input.pos.y)
                            })
                            .map(|g| g.id);
                        if self.host.is_some() {
                            host::log(&format!(
                                "drag-release: src={group} item={item} target={target:?} at={:?}",
                                input.pos
                            ));
                        }
                        let source_tab = self.state.group(group).map(|g| g.tab().id);
                        let batch_ids: Vec<_> = self
                            .multi_selected
                            .iter()
                            .filter_map(|(gid, tid, iid)| {
                                (Some(*gid) == Some(group) && Some(*tid) == source_tab)
                                    .then_some(*iid)
                            })
                            .collect();
                        let batch = batch_ids.len() > 1 && batch_ids.contains(&item);
                        if let Some(target) = target {
                            if batch {
                                let destination_tab = self.state.group(target).and_then(|g| {
                                    tab_target
                                        .filter(|(gid, _)| *gid == target)
                                        .and_then(|(_, idx)| g.tabs.get(idx))
                                        .or_else(|| g.tabs.get(g.active))
                                        .map(|t| t.id)
                                });
                                if let (Some(src), Some(dst)) = (source_tab, destination_tab) {
                                    if src == dst && target == group {
                                        self.notify(
                                            "Selection unchanged: choose another tab or Box",
                                            input.time,
                                        );
                                    } else {
                                        self.remember();
                                        match self.state.transfer_many_between_tabs(
                                            group, src, &batch_ids, target, dst,
                                        ) {
                                            Ok(count) => {
                                                if self.state.prefs.switch_tab_after_drop {
                                                    if let Some(g) = self.state.group_mut(target) {
                                                        if let Some(i) =
                                                            g.tabs.iter().position(|t| t.id == dst)
                                                        {
                                                            g.active = i;
                                                        }
                                                    }
                                                    self.multi_selected = batch_ids
                                                        .iter()
                                                        .map(|id| (target, dst, *id))
                                                        .collect();
                                                    self.selected = Some((target, item));
                                                } else {
                                                    self.multi_selected.clear();
                                                    self.selected = Some((group, 0));
                                                }
                                                if let Some(h) = self.host.as_mut() {
                                                    h.last_sync = -10.;
                                                }
                                                self.notify(
                                                    format!(
                                                        "Moved {count} pins (disk files unchanged)"
                                                    ),
                                                    input.time,
                                                );
                                            }
                                            Err(e) => {
                                                self.undo.pop();
                                                self.notify(e, input.time);
                                            }
                                        }
                                    }
                                }
                            } else if self.host.is_some()
                                && self
                                    .state
                                    .group(target)
                                    .is_some_and(|g| g.tab().folder.is_some())
                            {
                                let path = self.state.group(group).and_then(|g| {
                                    g.tab()
                                        .items
                                        .iter()
                                        .find(|i| i.id == item)
                                        .map(|i| i.path.clone())
                                });
                                if let Some(path) = path {
                                    if let Err(e) =
                                        self.drop_path_into_box(&path, target, input.time)
                                    {
                                        self.notify(e, input.time);
                                    }
                                }
                            } else if self.host.is_some()
                                && self
                                    .state
                                    .group(group)
                                    .is_some_and(|g| g.tab().folder.is_some())
                            {
                                // A Folder View remains a view of the real folder.
                                // Pin its file to the normal box without moving it.
                                let path = self.state.group(group).and_then(|g| {
                                    g.tab()
                                        .items
                                        .iter()
                                        .find(|i| i.id == item)
                                        .map(|i| i.path.clone())
                                });
                                if let Some(path) = path {
                                    if let Err(e) =
                                        self.drop_path_into_box(&path, target, input.time)
                                    {
                                        self.notify(e, input.time);
                                    }
                                }
                            } else {
                                // Regular Box -> Box: only the virtual assignment
                                // changes; original file stays at the same path.
                                self.remember();
                                let source_tab = self.state.group(group).map(|g| g.tab().id);
                                let destination_tab = self.state.group(target).and_then(|g| {
                                    tab_target
                                        .filter(|(gid, _)| *gid == target)
                                        .and_then(|(_, idx)| g.tabs.get(idx))
                                        .or_else(|| g.tabs.get(g.active))
                                        .map(|t| t.id)
                                });
                                if let (Some(source_tab), Some(destination_tab)) =
                                    (source_tab, destination_tab)
                                {
                                    let index = self
                                        .state
                                        .group(target)
                                        .and_then(|g| {
                                            g.tabs.iter().find(|t| t.id == destination_tab)
                                        })
                                        .map_or(0, |t| t.items.len());
                                    match self.state.transfer_between_tabs(
                                        group,
                                        source_tab,
                                        item,
                                        target,
                                        destination_tab,
                                        index,
                                    ) {
                                        Ok(()) => {
                                            if self.state.prefs.switch_tab_after_drop {
                                                if let Some(g) = self.state.group_mut(target) {
                                                    if let Some(i) = g
                                                        .tabs
                                                        .iter()
                                                        .position(|t| t.id == destination_tab)
                                                    {
                                                        g.active = i;
                                                    }
                                                }
                                            }
                                            self.selected =
                                                if self.state.prefs.switch_tab_after_drop {
                                                    Some((target, item))
                                                } else {
                                                    Some((group, 0))
                                                };
                                            self.notify(
                                                "Pin moved to tab (file unchanged)",
                                                input.time,
                                            );
                                        }
                                        Err(e) => self.notify(e, input.time),
                                    }
                                }
                            }
                        } else if batch {
                            self.notify(
                                "Select another Box or Tab to move the selected pins",
                                input.time,
                            );
                        } else if self.host.is_some() {
                            let (x, y) = host::cursor();
                            if !host::over_other_app(x, y) {
                                let path = self.state.group(group).and_then(|g| {
                                    g.tab()
                                        .items
                                        .iter()
                                        .find(|i| i.id == item)
                                        .map(|i| i.path.clone())
                                });
                                let recycled = path.as_deref().is_some_and(|source| {
                                    self.try_recycle_drop(source, (x, y), input.time)
                                });
                                if !recycled {
                                    self.ungroup_to_desktop(group, item, (x, y), input.time);
                                }
                            }
                        }
                    }
                }
                Some(Drag::Create { start }) => {
                    if start.distance(input.pos) > 45. {
                        let raw = Bounds {
                            x: start.x.min(input.pos.x),
                            y: start.y.min(input.pos.y),
                            w: (start.x - input.pos.x).abs(),
                            h: (start.y - input.pos.y).abs(),
                        };
                        let min = self.group_min_size(raw, false);
                        let b = Bounds {
                            w: raw.w.max(min.0),
                            h: raw.h.max(min.1),
                            ..raw
                        };
                        self.create_group(b)
                    } else if input.right_released {
                        self.create_group(self.fresh_bounds());
                    }
                }
                Some(Drag::NativeScroll { group, row, .. }) => {
                    self.queue_native_scroll_to(group, row);
                }
                Some(Drag::Resize { id, before, edges, .. }) => {
                    if let Some(group) = self.state.group(id).filter(|g| g.bounds != before) {
                        let mut bounds = if input.shift {
                            group.bounds
                        } else {
                            let tab = group.tab();
                            self.snap_box_resize_rows(
                                group.bounds,
                                edges,
                                tab.folder.is_some(),
                                tab.list,
                                tab.icon_scale,
                            )
                        };
                        if let Some((edge, work)) = self.docked_resize_target(id, before) {
                            bounds = edge.fit_body(bounds, work);
                        }
                        if let Some(group) = self.state.group_mut(id) { group.bounds = bounds; }
                    }
                }
                _ => {}
            }
        }
    }
}
