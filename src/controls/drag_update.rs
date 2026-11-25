use super::*;

impl App {
    pub(super) fn move_group(&mut self, id: u64, start: Vec2, before: Bounds, before_dock: Option<DockEdge>, input: &Input, finish: bool) -> bool {
        let delta = input.pos - start;
        let old_dock = before_dock.filter(|_| self.native_env.chrome_only);
        let from_bottom = old_dock == Some(DockEdge::Bottom);
        let translated = Bounds {
            x: before.x + delta.x,
            y: before.y + delta.y,
            ..before
        };
        let moved_side = old_dock.and_then(|edge| edge.side_header(translated).map(|header| (edge, header)));
        let raw = if let Some((edge, header)) = moved_side {
            // Keep a grip on the corresponding end of the horizontal titlebar when undocking.
            let x = if edge == DockEdge::Left { header.x } else { header.x + HEADER - before.w };
            Bounds { x, y: input.pos.y + HEADER / 2., ..translated }
        } else if from_bottom {
            Bounds { y: translated.y + before.h + HEADER, ..translated }
        } else {
            translated
        };
        let target = (!input.shift)
            .then(|| self.native_dock_target(input.pos, raw, from_bottom, moved_side)).flatten();
        self.top_dock_preview = (!finish && target.is_some_and(|(edge, _)| edge == DockEdge::Top)).then_some(id);
        let (bounds, dock) = if let Some((edge, work)) = target {
            let body = if moved_side.is_some_and(|(side, _)| side == edge) { translated } else { raw };
            let mut bounds = edge.fit_body(body, work);
            if !finish && edge == DockEdge::Bottom && !from_bottom {
                bounds.y += HEADER; // Let the visible bottom touch the taskbar during the drag.
            }
            let dock = (finish || edge.is_side() || (from_bottom && edge == DockEdge::Bottom)).then_some(edge);
            (bounds, dock)
        } else {
            let (left, top, right, bottom) = if self.host.is_some() {
                (0., 0., self.width - before.w, self.height - HEADER)
            } else {
                (8., 124., self.width - before.w - 8., self.height - HEADER - 54.)
            };
            let bounds = Bounds {
                x: raw.x.clamp(left, right.max(left)),
                y: raw.y.clamp(top, bottom.max(top)),
                ..before
            };
            let bounds = if self.state.snap && !input.shift {
                self.state.snap_position(id, bounds)
            } else {
                bounds
            };
            (bounds, None)
        };
        let g = self.state.group_mut(id).unwrap();
        g.bounds = bounds;
        g.dock = dock;
        if finish && (dock.is_some() || old_dock.is_some()) {
            g.rolled = dock.is_some();
            self.revealed = None;
            self.hover_rolled = None;
            self.dock_reveal_block = dock.map(|_| id);
        }
        finish && dock.is_some()
    }

    pub(super) fn pointer_drag_update(&mut self, input: &Input) {
        if input.down || input.right_down || input.middle_down {
            match self.drag.clone() {
                Some(Drag::Group { id, start, before, before_dock }) => 'moving: {
                    let delta = input.pos - start;
                    let g = self.state.group(id).unwrap();
                    // A press only becomes a move after DRAG_THRESHOLD px, so a
                    // click (or a slightly shaky double-click) never nudges a group.
                    if g.bounds == before && delta.length() <= DRAG_THRESHOLD {
                        break 'moving;
                    }
                    self.move_group(id, start, before, before_dock, input, false);
                }
                Some(Drag::Scroll {
                    group,
                    track_y,
                    track_h,
                    total,
                    page,
                    cols,
                    grab,
                }) => {
                    let row = scroll_row_at(input.pos.y, track_y, track_h, total, page, grab);
                    if let Some(g) = self.state.group_mut(group) {
                        g.tab_mut().scroll = row * cols;
                    }
                }
                Some(Drag::NativeScroll {
                    group,
                    track_y,
                    track_h,
                    thumb_h,
                    positions,
                    grab,
                    row: previous,
                }) => {
                    let row = paint::native_scroll_row_at(
                        input.pos.y, track_y, track_h, thumb_h, positions, grab,
                    );
                    if let Some(Drag::NativeScroll { row: current, .. }) = self.drag.as_mut() {
                        *current = row;
                    }
                    if row != previous {
                        self.queue_native_scroll_to(group, row);
                    }
                }
                Some(Drag::Item { group, item, start }) if self.host.is_some() => {
                    // Keep Box -> Box and Box -> Desktop inside DesktopCraft.
                    // Those are changes to icon placement, NOT filesystem moves.
                    // Only start Windows OLE dragging when entering another app.
                    let over_group = self.state.groups.iter().any(|g| {
                        g.page == self.state.page && g.bounds.contains(input.pos.x, input.pos.y)
                    });
                    let source_tab = self.state.group(group).map(|g| g.tab().id);
                    let multi_count = self
                        .multi_selected
                        .iter()
                        .filter(|(gid, tid, _)| *gid == group && Some(*tid) == source_tab)
                        .count();
                    if multi_count <= 1 && !over_group && start.distance(input.pos) > DRAG_THRESHOLD
                    {
                        let (x, y) = host::cursor();
                        if host::over_other_app(x, y) {
                            let path = self
                                .state
                                .group(group)
                                .and_then(|g| g.tab().items.iter().find(|i| i.id == item))
                                .map(|i| i.path.clone());
                            if let (Some(path), Some(h)) = (path, self.host.as_mut()) {
                                h.pending = Some(desktop::Pending::Drag { path });
                            }
                            host::capture_pointer(false);
                            self.drag = None;
                        }
                    }
                }
                Some(Drag::Resize {
                    id,
                    start,
                    before,
                    edges,
                }) => {
                    let host = self.host.is_some();
                    let dock = self.docked_resize_target(id, before);
                    let area = if let Some((edge, work)) = dock {
                        edge.body_area(work)
                    } else if host {
                        Bounds {
                            x: 0.,
                            y: 0.,
                            w: self.width,
                            h: self.height,
                        }
                    } else {
                        Bounds {
                            x: 8.,
                            y: 124.,
                            w: self.width - 16.,
                            h: self.height - 124. - 58.,
                        }
                    };
                    let folder = self.state.group(id).unwrap().tab().folder.is_some();
                    let min = self.resize_min_size(before, input.pos - start, edges, area, folder);
                    let resized = resize_bounds(before, input.pos - start, edges, min, area);
                    let result = if self.state.snap && !input.shift {
                        self.state.snap_resize(id, resized, edges, min, area)
                    } else {
                        resized
                    };
                    self.state.group_mut(id).unwrap().bounds = dock.map_or(result, |(edge, work)| edge.fit_body(result, work));
                }
                _ => {}
            }
        }
    }
}
