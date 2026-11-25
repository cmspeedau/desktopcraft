use crate::*;

impl App {
    pub fn frame(&mut self, p: &mut Painter, input: &Input, native_acknowledged: Option<&State>) {
        // Opening Settings again replaces any previously minimized panel.
        if matches!(self.dialog, Some(Dialog::Settings { .. })) && self.minimized_settings.is_some()
        {
            self.minimized_settings = None;
            self.settings_restore_armed = false;
        }
        if self.host.is_some() {
            clear_background(Color::new(0., 0., 0., 0.));
        } else {
            p.wallpaper(self.state.wallpaper);
        }
        let undo_started_in_frame = if self.native_env.chrome_only {
            // Desktop icons take keyboard input in Explorer; Portal search
            // and explicit Native Desktop shortcuts (Ctrl+Z undo) run here.
            self.native_chrome_shortcuts(input)
        } else {
            self.shortcuts(input); // no destructive legacy keyboard actions in lab
            false
        };
        self.update_search(input.time);
        self.refresh_portals(input.time);
        let host = self.host.is_some();
        if host && !self.automation && !self.state.prefs.desktop_enabled {
            #[cfg(windows)]
            self.sync_native_backdrop(false, None, false);
            self.hit_buffer.clear();
            self.popup_rects.clear();
            if self.has_blocking_dialog() {
                self.in_primary_view(p, input, |app, p, input| {
                    p.push_region(
                        Bounds {
                            x: 0.,
                            y: 0.,
                            w: app.width,
                            h: app.height,
                        },
                        0.,
                    );
                    app.modal(p, input)
                });
            }
            if !self.toast.is_empty() && input.time < self.toast_until && !self.settings_in_own_window() {
                self.in_primary_view(p, input, |app, p, _| {
                    let w = (p.measure(&app.toast, 14.) + 40.).min(app.width - 80.);
                    let b = Bounds {
                        x: (app.width - w) / 2.,
                        y: app.height - 70.,
                        w,
                        h: 43.,
                    };
                    p.push_region(b, 7.);
                    round(b, 7., SURFACE);
                    p.label(&app.toast, b, 14., TEXT);
                });
            }
            return;
        }
        let own_settings = self.settings_in_own_window();
        let floating_settings = matches!(self.dialog, Some(Dialog::Settings { .. }));
        let settings_hit = floating_settings
            && !own_settings
            && self
                .settings_bounds_local()
                .contains(input.pos.x, input.pos.y);
        let popup_open = self.menu.is_some() || self.folder_sort_menu.is_some();
        let desktop_interactive = self.dialog.is_none()
            || own_settings
            || (!host && floating_settings && !settings_hit);
        p.enabled = desktop_interactive && !popup_open && !undo_started_in_frame;
        p.click = input.pressed && !settings_hit && !undo_started_in_frame;
        self.stats = DrawStats::default();
        // Desktop mode draws only groups, menus, dialogs and messages: the rest is
        // Explorer's real desktop (wallpaper, icons and its own right-click menu).
        let top_consumed = if host { false } else { self.topbar(p, input) };
        if !host {
            p.text(
                if self.peek {
                    "Your groups are in front"
                } else {
                    &self.state.pages[self.state.page]
                },
                52.,
                80.,
                17.,
                TEXT,
            );
            p.text(
                if self.hidden {
                    "Double-click to bring everything back"
                } else {
                    "A place for everything you’re working on"
                },
                52.,
                103.,
                12.,
                MUTED,
            );
        }
        if !undo_started_in_frame {
            self.update_rollup_reveal(input);
        }
        let prefs = &self.state.prefs;
        let chrome_native = self.host.is_some() && self.native_env.chrome_only;
        let visual_revealed = self.visual_revealed_with_ack(chrome_native, native_acknowledged);
        let native_workspace_groups: std::sync::Arc<[u64]> =
            if chrome_native && self.native_env.general_workspace {
                self.state.groups.iter()
                    .filter(|g| g.page == self.state.page)
                    .map(|g| g.id)
                    .collect::<Vec<_>>()
                    .into()
            } else {
                std::sync::Arc::from([])
            };
        #[cfg(windows)]
        let native_drag_fill = if self.native_backdrop.is_some() && !self.native_env.backdrop_disabled {
            None
        } else {
            self.native_ghost.as_ref().map(|g| g.group)
                .or_else(|| self.active_native_drag_group())
        };
        #[cfg(not(windows))]
        let native_drag_fill = self.active_native_drag_group();
        let style = paint::GroupStyle {
            titlebar: prefs.titlebar,
            hide_scrollbars: prefs.hide_scrollbars,
            blend_opacity: prefs.blend_opacity,
            rollup: prefs.rollup,
            lock_layout: prefs.lock_layout,
            revealed: visual_revealed,
            menu_open_group: self.menu.map(|(id, _)| id),
            active_drag_group: match &self.drag {
                Some(Drag::Group { id, .. } | Drag::Resize { id, .. }) => Some(*id),
                _ => None,
            },
            top_dock_preview: self.previewing_top_dock(),
            filter_focus: self.filter_focus,
            portal_loading: false,
            native_chrome_only: chrome_native,
            native_portal_frame: false,
            native_target_group: if chrome_native && !self.native_env.general_workspace {
                self.native_env.target_group
            } else {
                None
            },
            native_tab_pair: if chrome_native { self.native_env.tab_pair } else { None },
            native_two_box_ui: chrome_native && self.native_env.two_box_ui,
            native_secondary_group: if chrome_native
                && !self.native_env.general_workspace
                && self.native_env.two_box_ui
            {
                Some(168)
            } else {
                None
            },
            native_workspace_groups,
            native_in_box_layout: chrome_native && self.native_env.in_box_layout,
            native_ole_dest: if chrome_native { self.native_env.ole_dest } else { None },
            native_pages: self.native_pages.clone(),
            native_scroll_drag: match &self.drag {
                Some(Drag::NativeScroll { group, row, .. }) => Some((*group, *row)),
                _ => self.native_scroll_target,
            },
            native_drag_fill,
        };
        let mut hits = std::mem::take(&mut self.hit_buffer);
        hits.clear();
        // Draw the expanded rolled-up box last so it is physically on top
        // of stacked neighbors, and its hits win pointer dispatch too.
        for g in paint_order(&self.state.groups, self.state.page, visual_revealed) {
            if chrome_native && native_revealed_occludes(&self.state, visual_revealed, g) {
                continue;
            }
            let mut group_style = style.clone();
            group_style.portal_loading = g.tab().folder.is_some() && g.tab().items.is_empty()
                && self.portal_refresh_inflight.contains(&(g.id, g.tab().id));
            if chrome_native
                && (self.peek
                    || (g.tab().folder.is_some()
                        && !portal_waiting_for_explorer(g, native_acknowledged)))
            {
                // Peek and Folder Portals render inside Macroquad's own interactive overlay.
                group_style.native_chrome_only = false;
                group_style.native_in_box_layout = false;
                group_style.native_portal_frame = true;
            }
            paint::group(
                p,
                g,
                self.selected,
                &self.multi_selected,
                self.hidden,
                &self.applied_search,
                self.peek,
                self.state.blend,
                self.state.tint,
                &group_style,
                &mut hits,
                &mut self.stats,
            );
        }
        if undo_started_in_frame {
            self.clear_native_scroll_state();
        }
        #[cfg(windows)]
        self.draw_native_ghost(p, input.released);
        // An icon being renamed in place gets its edit box over its label.
        self.draw_inline_rename(p, &hits, input.time);
        if let Some(Drag::BoxSelect { group, start, .. }) = &self.drag {
            if start.distance(input.pos) > DRAG_THRESHOLD
                && self
                    .state
                    .group(*group)
                    .is_some_and(|g| g.page == self.state.page)
            {
                let b = Bounds {
                    x: start.x.min(input.pos.x),
                    y: start.y.min(input.pos.y),
                    w: (start.x - input.pos.x).abs(),
                    h: (start.y - input.pos.y).abs(),
                };
                draw_rectangle(b.x, b.y, b.w, b.h, Color::new(0.25, 0.50, 0.95, 0.16));
                draw_rectangle_lines(b.x, b.y, b.w, b.h, 1.5, Color::new(0.50, 0.75, 1.0, 0.85));
            }
        }
        let pager_consumed = if host {
            self.desktop_pager(p, input)
        } else {
            false
        };

        if !host && self.state.groups.iter().all(|g| g.page != self.state.page) && !self.hidden {
            p.label(
                "Room to focus.",
                Bounds {
                    x: self.width / 2. - 200.,
                    y: self.height / 2. - 65.,
                    w: 400.,
                    h: 50.,
                },
                32.,
                TEXT,
            );
            p.label(
                "Draw a group, add a folder, or bring a group from another page.",
                Bounds {
                    x: self.width / 2. - 300.,
                    y: self.height / 2. - 6.,
                    w: 600.,
                    h: 32.,
                },
                15.,
                MUTED,
            );
        }
        if let Some(Drag::Create { start }) = &self.drag {
            let b = Bounds {
                x: start.x.min(input.pos.x),
                y: start.y.min(input.pos.y),
                w: (start.x - input.pos.x).abs(),
                h: (start.y - input.pos.y).abs(),
            };
            draw_rectangle(b.x, b.y, b.w, b.h, alpha(ACCENT, 0.13));
            outline(b, ACCENT);
        }
        if let Some(Drag::Item { group, item, start }) = &self.drag {
            if start.distance(input.pos) > DRAG_THRESHOLD {
                if let Some(item) = self
                    .state
                    .group(*group)
                    .and_then(|g| g.tab().items.iter().find(|x| x.id == *item))
                {
                    p.icon(item, input.pos.x - 20., input.pos.y - 20., 40., None);
                    p.push_region(
                        Bounds {
                            x: input.pos.x - 22.,
                            y: input.pos.y - 22.,
                            w: 44.,
                            h: 44.,
                        },
                        0.,
                    );
                }
            }
        }
        let footer_consumed = if host { false } else { self.footer(p, input) };
        let had_menu = self.menu.is_some();
        let had_sort_list = self.folder_sort_menu.is_some();
        // Resize cursor over group edges and corners (and while resizing).
        let edges = match &self.drag {
            Some(Drag::Resize { edges, .. }) => Some(*edges),
            _ => hits
                .iter()
                .rev()
                .find(|h| h.bounds.contains(input.pos.x, input.pos.y))
                .and_then(|h| {
                    if let Target::Resize(_, e) = h.target {
                        Some(e)
                    } else {
                        None
                    }
                }),
        };
        if edges != self.cursor_edges {
            self.cursor_edges = edges;
            if !self.automation {
                macroquad::miniquad::window::set_mouse_cursor(
                    edges
                        .map(paint::resize_cursor)
                        .unwrap_or(macroquad::miniquad::CursorIcon::Default),
                );
            }
        }
        if style.native_two_box_ui && input.pressed{
            let target_tabs=hits.iter().filter(|hit|
                hit.bounds.contains(input.pos.x,input.pos.y))
                .filter(|hit|matches!(hit.target,paint::Target::Tab(_, _)))
                .count();
            host::event("INFO","native_two_box.pointer_hit",
                &format!("local=({:.1},{:.1}) native_tab_hits={target_tabs} total_hits={}",
                    input.pos.x,input.pos.y,hits.len()));
        }
        let keyboard_menu = if undo_started_in_frame || self.native_tx_inflight {
            false
        } else {
            self.keyboard_file_menu(input, &hits)
        };
        #[cfg(windows)]
        let drag_handoff_pending = self.native_ghost.as_ref().is_some_and(|ghost| ghost.dropped);
        #[cfg(not(windows))]
        let drag_handoff_pending = false;
        if !keyboard_menu
            && !drag_handoff_pending
            && !undo_started_in_frame
            && desktop_interactive
            && self.menu.is_none()
            && self.folder_sort_menu.is_none()
            && !top_consumed
            && !footer_consumed
            && !pager_consumed
        {
            self.pointer(input, &hits);
        }
        if chrome_native && !self.automation
            && (self.menu.is_some() || self.folder_sort_menu.is_some() || self.filter_focus.is_some())
        {
            // Once the host is clipped to the popup, outside clicks reach
            // Windows directly. Dismiss on a fresh press using its screen point.
            let down = host::mouse_left_down() || host::mouse_right_down();
            let h = self.host.as_mut().unwrap();
            let pressed = down && !h.popup_mouse_down;
            h.popup_mouse_down = down;
            if pressed && (had_menu || had_sort_list || self.filter_focus.is_some()) {
                let (x, y) = host::cursor();
                let point = self.from_screen(x, y);
                if (had_menu || had_sort_list)
                    && !self.popup_rects.iter().any(|b| b.contains(point.x, point.y))
                {
                    self.menu = None;
                    self.menu_page_open = false;
                    self.menu_sort_open = false;
                    self.folder_sort_menu = None;
                }
                if let Some(focused_id) = self.filter_focus {
                    let in_focused = self
                        .state
                        .group(focused_id)
                        .is_some_and(|g| paint::native_group_outer(g).contains(point.x, point.y));
                    if !in_focused {
                        self.filter_focus = None;
                    }
                }
            }
        }
        // Rebuilt below by whichever pop-ups are drawn this frame.
        self.popup_rects.clear();
        let host_lifted = self.host_lifted_for_popup();
        #[cfg(windows)]
        self.sync_native_backdrop(
            chrome_native && style.native_in_box_layout && !host::color_pick_shielded(),
            native_acknowledged,
            host_lifted,
        );
        if host_lifted {
            // The clipped host has one z-order. Only floating UI may travel
            // with it over other apps; the desktop Box pixels and hits go away.
            clear_background(Color::new(0., 0., 0., 0.));
            p.regions.borrow_mut().clear();
            hits.clear();
        }
        if self.menu.is_some() {
            if had_menu {
                self.context_menu(p, input);
            } else {
                let mut opening = input.clone();
                opening.pressed = false;
                self.context_menu(p, &opening);
            }
        }
        if self.folder_sort_menu.is_some() {
            if had_sort_list {
                self.folder_sort_list(p, input);
            } else {
                // The press that opened it must not also pick a row.
                let mut opening = input.clone();
                opening.pressed = false;
                self.folder_sort_list(p, &opening);
            }
        }
        if self.has_blocking_dialog() {
            let mut dialog_input;
            let dialog_in = if undo_started_in_frame {
                dialog_input = input.clone();
                dialog_input.pressed = false;
                dialog_input.released = false;
                dialog_input.down = false;
                dialog_input.right_pressed = false;
                dialog_input.right_released = false;
                dialog_input.right_down = false;
                dialog_input.middle_pressed = false;
                dialog_input.middle_released = false;
                dialog_input.middle_down = false;
                dialog_input.scroll = 0.;
                dialog_input.keys.clear();
                dialog_input.repeats.clear();
                dialog_input.typed.clear();
                &dialog_input
            } else {
                input
            };
            let floating_settings = matches!(self.dialog, Some(Dialog::Settings { .. }));
            if host && !floating_settings {
                self.in_primary_view(p, dialog_in, |app, p, input| {
                    p.push_region(
                        Bounds {
                            x: 0.,
                            y: 0.,
                            w: app.width,
                            h: app.height,
                        },
                        0.,
                    );
                    app.modal(p, input)
                });
            } else {
                // Draw floating Settings in the virtual-screen camera so both
                // its rendering and its mouse hit regions cross monitor edges.
                if host {
                    p.push_region(self.settings_bounds_local(), 10.);
                }
                self.modal(p, dialog_in);
            }
        }
        #[cfg(windows)]
        let settings_iconic = self
            .settings_window
            .as_ref()
            .is_some_and(settings_window::SettingsWindow::is_iconic);
        #[cfg(not(windows))]
        let settings_iconic = false;
        if !self.toast.is_empty() && input.time < self.toast_until && (!own_settings || settings_iconic) {
            let draw_toast = |app: &mut App, p: &mut Painter, _: &Input| {
                let w = (p.measure(&app.toast, 14.) + 40.).min(app.width - 80.);
                let b = Bounds {
                    x: (app.width - w) / 2.,
                    y: app.height - if app.host.is_some() { 70. } else { 108. },
                    w,
                    h: 43.,
                };
                p.push_region(b, 7.);
                round(b, 7., SURFACE);
                p.label(&app.toast, b, 14., TEXT);
            };
            if host {
                self.in_primary_view(p, input, draw_toast);
            } else {
                draw_toast(self, p, input);
            }
        }
        self.hit_buffer = hits;
    }

}
