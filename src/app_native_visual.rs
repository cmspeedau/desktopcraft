use crate::*;

impl App {
    /// Visual rollup reveal for Chrome rendering & WorkerW backdrop:
    /// regular Native Boxes wait for the worker ACK so they never show an empty
    /// dark box while Explorer is still moving icons, whereas Folder Portals
    /// draw directly inside Macroquad without waiting.
    pub(super) fn visual_revealed(&self, chrome_native: bool) -> Option<u64> {
        if !chrome_native || self.automation || !self.native_env.in_box_layout {
            return self.revealed;
        }
        if self.revealed.is_some_and(|id| {
            self.state.group(id).is_some_and(|g| {
                g.page == self.state.page && g.rolled && g.tab().folder.is_some()
            })
        }) {
            return self.revealed;
        }
        self.native_visual_revealed
    }

    pub(super) fn visual_revealed_with_ack(
        &self,
        chrome_native: bool,
        native_acknowledged: Option<&State>,
    ) -> Option<u64> {
        if let Some(id) = self.visual_revealed(chrome_native) {
            return Some(id);
        }
        if !chrome_native || self.automation || !self.native_env.in_box_layout {
            return None;
        }
        // When switching a regular Native Box from fixed (unrolled) to rolled up
        // for the first time, keep its backdrop/frame aligned with Explorer's
        // icons until the worker ACKs hiding them so icons never hang naked.
        let ack = native_acknowledged?;
        if ack.page != self.state.page {
            return None;
        }
        self.state
            .groups
            .iter()
            .find(|g| {
                g.page == self.state.page
                    && g.rolled
                    && g.tab().folder.is_none()
                    && ack
                        .group(g.id)
                        .is_some_and(|a| a.page == g.page && !a.rolled && a.tab().folder.is_none())
            })
            .map(|g| g.id)
    }
}
#[cfg(windows)]
impl App {
    /// Renders Settings into its independent Win32 floating host window so
    /// desktop Boxes keep crisp Macroquad chrome and stay interactive behind it.
    pub(super) fn render_own_settings_window(&mut self, p: &mut Painter, time: f64) {
        if !self.settings_in_own_window() {
            if let Some(sw) = self.settings_window.as_mut() {
                sw.hide();
            }
            self.settings_cursor_was_over = false;
            return;
        }
        let [sx, sy, pw, ph] = self.to_screen(self.settings_bounds_local());
        if self.settings_window.is_none() {
            self.settings_window = settings_window::SettingsWindow::create(sx, sy, pw, ph);
        }
        let Some(sw) = self.settings_window.as_mut() else {
            return;
        };
        if self.settings_bring_to_front && sw.is_iconic() {
            sw.show(true);
        }
        if sw.is_iconic() && !self.settings_bring_to_front {
            let _ = sw.drain_input(time, false);
            return;
        }
        sw.ensure_size(pw, ph);
        sw.move_to(sx, sy);

        let (mut sw_input, close_requested) = sw.drain_input(time, self.settings_drag_offset.is_some());
        let sw_has_focus = sw.has_focus();
        let sw_rendered_once = sw.has_rendered_once();
        if sw_input.key(KeyCode::Escape) && self.shortcut_recording {
            self.shortcut_recording = false;
            self.shortcut_candidate = None;
            self.shortcut_recording_rejected = false;
            sw_input.keys.retain(|k| *k != KeyCode::Escape);
        } else if close_requested || sw_input.key(KeyCode::Escape) {
            self.dialog = None;
            self.settings_return = None;
            self.settings_drag_offset = None;
            self.settings_mouse_was_down = false;
            self.shortcut_recording = false;
            self.shortcut_candidate = None;
            self.shortcut_recording_rejected = false;
            self.dropdown_open = None;
            self.dropdown_list = None;
            self.dropdown_hit = None;
            self.dropdown_pick = None;
            self.settings_bring_to_front = false;
            sw.hide();
            self.settings_cursor_was_over = false;
            return;
        }
        if sw_has_focus
            && !matches!(self.dialog, Some(Dialog::Settings { page: 2, .. }))
            && !self.shortcut_recording
            && self.dropdown_open.is_none()
            && self.state.prefs.desktop_enabled
            && sw_input.ctrl
            && !sw_input.alt
            && !sw_input.shift
            && sw_input.key(KeyCode::Z)
            && self.native_undo_shortcut_allowed()
        {
            self.request_undo(time);
        }

        let cursor_over = sw_input.pos.x >= 0.0
            && sw_input.pos.y >= 0.0
            && sw_input.pos.x <= settings_window::SETTINGS_LOGICAL_W
            && sw_input.pos.y <= settings_window::SETTINGS_LOGICAL_H;
        let toast_active = !self.toast.is_empty() && time < self.toast_until;
        let animated_page = matches!(self.dialog, Some(Dialog::Settings { page: 1 | 6 | 8, .. }));
        let should_render = !sw_rendered_once
            || self.settings_bring_to_front
            || sw_input.pressed
            || sw_input.released
            || sw_input.right_pressed
            || sw_input.middle_pressed
            || sw_input.scroll != 0.0
            || !sw_input.keys.is_empty()
            || !sw_input.repeats.is_empty()
            || !sw_input.typed.is_empty()
            || cursor_over
            || cursor_over != self.settings_cursor_was_over
            || self.settings_drag_offset.is_some()
            || self.shortcut_recording
            || animated_page
            || toast_active
            || time - self.settings_last_paint >= 0.25;
        self.settings_cursor_was_over = cursor_over;
        if !should_render {
            return;
        }

        let Some(mut sw_owned) = self.settings_window.take() else {
            return;
        };
        let rt = sw_owned.render_target();
        let dwm_rounded = sw_owned.dwm_rounded();

        let saved_width = p.width;
        let saved_height = p.height;
        let saved_mouse = p.mouse;
        let saved_click = p.click;
        let saved_enabled = p.enabled;
        let saved_offset = p.offset;
        let saved_regions = std::mem::take(&mut *p.regions.borrow_mut());

        p.width = settings_window::SETTINGS_LOGICAL_W;
        p.height = settings_window::SETTINGS_LOGICAL_H;
        p.mouse = sw_input.pos;
        p.click = sw_input.pressed;
        p.enabled = true;
        p.offset = Vec2::ZERO;

        push_camera_state();
        let mut cam = Camera2D::from_display_rect(Rect::new(
            0.,
            0.,
            settings_window::SETTINGS_LOGICAL_W,
            settings_window::SETTINGS_LOGICAL_H,
        ));
        cam.render_target = Some(rt);
        set_camera(&cam);
        clear_background(SURFACE);

        self.rendering_settings_window = true;
        self.modal(p, &sw_input);
        self.rendering_settings_window = false;

        draw_rectangle_lines(
            0.5,
            0.5,
            settings_window::SETTINGS_LOGICAL_W - 1.0,
            settings_window::SETTINGS_LOGICAL_H - 1.0,
            1.0,
            alpha(WHITE, if dwm_rounded { 0.08 } else { 0.16 }),
        );
        if !self.toast.is_empty() && time < self.toast_until {
            let tw = (p.measure(&self.toast, 14.) + 36.).min(settings_window::SETTINGS_LOGICAL_W - 64.);
            let tb = Bounds {
                x: (settings_window::SETTINGS_LOGICAL_W - tw) / 2.,
                y: settings_window::SETTINGS_LOGICAL_H - 52.,
                w: tw,
                h: 38.,
            };
            round(tb, 7., Color::new(0.09, 0.14, 0.21, 0.98));
            draw_rectangle_lines(tb.x, tb.y, tb.w, tb.h, 1., alpha(WHITE, 0.16));
            p.label(&self.toast, tb, 14., TEXT);
        }

        pop_camera_state();
        set_camera(&workspace_camera(self.width, self.height));

        p.width = saved_width;
        p.height = saved_height;
        p.mouse = saved_mouse;
        p.click = saved_click;
        p.enabled = saved_enabled;
        p.offset = saved_offset;
        *p.regions.borrow_mut() = saved_regions;

        let [nx, ny, _, _] = self.to_screen(self.settings_bounds_local());
        sw_owned.move_to(nx, ny);

        if self.settings_minimize_requested {
            self.settings_minimize_requested = false;
            self.settings_bring_to_front = false;
            sw_owned.minimize();
        } else if !self.settings_in_own_window() {
            self.settings_bring_to_front = false;
            sw_owned.hide();
        } else {
            sw_owned.present_from_render_target();
            self.settings_last_paint = time;
            let activate = std::mem::take(&mut self.settings_bring_to_front) || !sw_owned.is_visible();
            sw_owned.show(activate);
        }
        self.settings_window = Some(sw_owned);
    }

    /// Native in-Box layout: Explorer draws the icons, so the dark Box body
    /// must sit under them in the wallpaper layer, not in DesktopCraft's
    /// window (whose region only keeps the header and edges). While floating
    /// popups lift `host`, mirror Box headers onto the wallpaper layer too.
    pub(super) fn sync_native_backdrop(
        &mut self,
        enabled: bool,
        native_acknowledged: Option<&State>,
        mirror_headers: bool,
    ) {
        // DESKTOPCRAFT_NATIVE_BACKDROP=0 turns Box bodies behind icons off.
        let enabled = enabled && !self.native_env.backdrop_disabled;
        if !enabled {
            if let Some(backdrop) = self.native_backdrop.as_mut() {
                backdrop.sync(Vec::new(), Vec::new(), Vec::new());
            }
            return;
        }
        let coordinates = if let Some(ref host_state) = self.host {
            native_box_live_layout::DesktopCoordinates {
                origin: (host_state.virtual_rect[0], host_state.virtual_rect[1]),
                scale: host_state.scale,
            }
        } else if let Ok(coords) = native_box_live_layout::DesktopCoordinates::current() {
            coords
        } else {
            return;
        };
        let radius = (GROUP_CORNER_RADIUS * coordinates.scale).round() as i32;
        let visual_revealed = self.visual_revealed_with_ack(true, native_acknowledged);
        let top_dock_preview = self.previewing_top_dock();
        let mouse = if self.host.is_some() && !self.automation {
            let (mx, my) = host::cursor();
            if self.settings_window.as_ref().is_some_and(|sw| sw.is_point_over_window(mx, my)) {
                None
            } else {
                Some(self.from_screen(mx, my))
            }
        } else {
            None
        };
        let menu_open_group = self.menu.map(|(id, _)| id);
        let active_drag_group = match &self.drag {
            Some(Drag::Group { id, .. } | Drag::Resize { id, .. }) => Some(*id),
            _ => None,
        };
        // Keep the same WorkerW panel throughout a move and its release.
        let panels = paint_order(&self.state.groups, self.state.page, visual_revealed)
            .filter(|g| {
                mirror_headers
                    || g.tab().folder.is_none()
                    || portal_waiting_for_explorer(g, native_acknowledged)
            })
            .filter(|g| !(self.hidden && !g.keep_visible && !self.peek))
            .filter(|g| !(g.rolled && self.state.prefs.rollup && visual_revealed != Some(g.id)))
            .filter(|g| top_dock_preview != Some(g.id))
            .filter(|g| !native_revealed_occludes(&self.state, visual_revealed, g))
            .filter_map(|g| {
                let hovered = mouse.is_some_and(|m| paint::native_group_outer(g).contains(m.x, m.y));
                let rect = coordinates.screen_rect(paint::native_group_body(g));
                let mut rgba = paint::panel_rgba(g);
                let (eff_alpha, _) = paint::effective_panel_alpha_and_fade(
                    rgba[3],
                    self.state.blend,
                    self.state.prefs.blend_opacity,
                    hovered,
                    self.peek,
                );
                if eff_alpha <= 0.004 {
                    return None;
                }
                rgba[3] = eff_alpha;
                let (rgb, alpha) = native_backdrop::panel_color(rgba);
                Some(native_backdrop::Panel {
                    id: g.id, x: rect.x, y: rect.y, w: rect.w, h: rect.h, rgb, alpha, radius,
                })
            })
            .filter(|panel| panel.w > 0 && panel.h > 0)
            .collect::<Vec<_>>();
        let headers = if mirror_headers {
            paint_order(&self.state.groups, self.state.page, visual_revealed)
                .filter(|g| !(self.hidden && !g.keep_visible && !self.peek))
                .filter(|g| top_dock_preview != Some(g.id))
                .filter(|g| !native_revealed_occludes(&self.state, visual_revealed, g))
                .filter_map(|g| {
                    let hdr_rect = coordinates.screen_rect(paint::native_group_header(g));
                    if hdr_rect.w <= 0 || hdr_rect.h <= 0 {
                        return None;
                    }
                    let rolled = g.rolled && self.state.prefs.rollup && visual_revealed != Some(g.id);
                    let hovered = mouse.is_some_and(|m| {
                        let area = if rolled {
                            paint::native_group_header(g)
                        } else {
                            paint::native_group_outer(g)
                        };
                        area.contains(m.x, m.y)
                    });
                    let show_title = paint::should_show_titlebar(
                        self.state.prefs.titlebar,
                        hovered,
                        menu_open_group == Some(g.id),
                        active_drag_group == Some(g.id),
                        rolled,
                    );
                    if !show_title {
                        return None;
                    }
                    let rgba = paint::panel_rgba(g);
                    let (_, fade) = paint::effective_panel_alpha_and_fade(
                        rgba[3],
                        self.state.blend,
                        self.state.prefs.blend_opacity,
                        hovered,
                        self.peek,
                    );
                    let vertical = g.dock.is_some_and(model::DockEdge::is_side);
                    let corners = if rolled || vertical {
                        native_backdrop::HeaderCorners::AllRounded
                    } else if g.dock == Some(model::DockEdge::Bottom) {
                        native_backdrop::HeaderCorners::BottomRounded
                    } else {
                        native_backdrop::HeaderCorners::TopRounded
                    };
                    let (rgb, alpha) = native_backdrop::header_color(rgba, fade);
                    let font_px = ((if g.tabs.len() > 1 { 14.0 } else { 15.0 }) * coordinates.scale).round() as i32;
                    let (title, tabs) = if !show_title {
                        (String::new(), Vec::new())
                    } else if g.tabs.len() > 1 {
                        let tabs = paint::native_header_tabs(g)
                            .map(|(i, tb)| {
                                let tr = coordinates.screen_rect(tb);
                                native_backdrop::HeaderTab {
                                    x: tr.x - hdr_rect.x,
                                    y: tr.y - hdr_rect.y,
                                    w: tr.w,
                                    h: tr.h,
                                    name: g.tabs[i].name.clone(),
                                    active: i == g.active,
                                }
                            })
                            .collect::<Vec<_>>();
                        (String::new(), tabs)
                    } else {
                        (g.tab().name.clone(), Vec::new())
                    };
                    Some(native_backdrop::HeaderPanel {
                        id: g.id,
                        x: hdr_rect.x,
                        y: hdr_rect.y,
                        w: hdr_rect.w,
                        h: hdr_rect.h,
                        rgb,
                        alpha,
                        radius,
                        corners,
                        vertical,
                        font_px,
                        title,
                        tabs,
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let tints = if self.state.tint && !self.peek {
            paint_order(&self.state.groups, self.state.page, visual_revealed)
                .filter(|g| g.tab().folder.is_none() && !portal_waiting_for_explorer(g, native_acknowledged))
                .filter(|g| !(self.hidden && !g.keep_visible && !self.peek))
                .filter(|g| !(g.rolled && self.state.prefs.rollup && visual_revealed != Some(g.id)))
                .filter(|g| top_dock_preview != Some(g.id))
                .filter(|g| !native_revealed_occludes(&self.state, visual_revealed, g))
                .filter_map(|g| {
                    let hovered = mouse.is_some_and(|m| paint::native_group_outer(g).contains(m.x, m.y));
                    let rect = coordinates.screen_rect(paint::native_group_body(g));
                    if rect.w <= 0 || rect.h <= 0 {
                        return None;
                    }
                    let rgba = paint::panel_rgba(g);
                    let (_, fade) = paint::effective_panel_alpha_and_fade(
                        rgba[3],
                        self.state.blend,
                        self.state.prefs.blend_opacity,
                        hovered,
                        self.peek,
                    );
                    let tint = paint::tint_color(g);
                    let (rgb, _) = native_backdrop::panel_color([tint.r, tint.g, tint.b, 1.0]);
                    let alpha = (56.0 * fade).round().clamp(0.0, 96.0) as u8;
                    if alpha == 0 {
                        return None;
                    }
                    Some(native_backdrop::Panel {
                        id: g.id,
                        x: rect.x,
                        y: rect.y,
                        w: rect.w,
                        h: rect.h,
                        rgb,
                        alpha,
                        radius,
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if self.native_backdrop.is_none() {
            if panels.is_empty() && headers.is_empty() && tints.is_empty() {
                return;
            }
            self.native_backdrop = native_backdrop::Backdrop::start();
        }
        if let Some(backdrop) = self.native_backdrop.as_mut() {
            backdrop.sync(panels, headers, tints);
        }
        let wheel = self.native_backdrop.as_ref().map(|b| b.take_wheel()).unwrap_or_default();
        for (group, delta) in wheel {
            let (id, rest) = self.native_wheel_remainder;
            let total = if id == group { rest + delta } else { delta };
            // One notch (120) = one icon row; wheel up (positive) scrolls up.
            let rows = total / 120;
            self.native_wheel_remainder = (group, total - rows * 120);
            if rows != 0 {
                self.queue_native_scroll(group, -rows);
            }
        }
    }
}

impl App {
    /// Native Box row scroll. Notches are merged into one
    /// pending request and clamped to rows that exist, so scrolling past
    /// the top or bottom asks Explorer for nothing.
    pub(super) fn queue_native_scroll(&mut self, group: u64, rows: i32) {
        self.native_scroll_target = None;
        let Some(&(_, first, positions, _)) =
            self.native_pages.iter().find(|status| status.0 == group) else { return };
        let pending = match self.native_page_request {
            Some((id, delta)) if id == group => i32::from(delta),
            _ => 0,
        };
        let limit = i32::from(native_scroll_limit());
        let target = (first as i32 + pending + rows).clamp(0, positions as i32 - 1);
        let delta = (target - first as i32).clamp(-limit, limit);
        self.native_page_request = (delta != 0).then_some((group, delta as i8));
    }

    /// Scroll a native Box to a specific first visible row (from dragging its
    /// scrollbar thumb). Jumps larger than one bounded Explorer reflow step
    /// keep stepping via `advance_native_scroll_target` after each ACK.
    pub(super) fn queue_native_scroll_to(&mut self, group: u64, row: usize) {
        let Some(&(_, first, positions, _)) =
            self.native_pages.iter().find(|status| status.0 == group) else { return };
        let target = row.min(positions.saturating_sub(1));
        if target == first {
            self.native_scroll_target = None;
            if self.native_page_request.is_some_and(|(id, _)| id == group) {
                self.native_page_request = None;
            }
            return;
        }
        self.native_scroll_target = Some((group, target));
        let limit = i32::from(native_scroll_limit());
        let delta = (target as i32 - first as i32).clamp(-limit, limit);
        self.native_page_request = (delta != 0).then_some((group, delta as i8));
    }

    /// Queue the next bounded scroll step for a long scrollbar thumb drag,
    /// or clear the target once Explorer has reached it.
    pub(super) fn advance_native_scroll_target(&mut self) {
        let Some((group, target)) = self.native_scroll_target else { return };
        let Some(&(_, first, positions, _)) =
            self.native_pages.iter().find(|status| status.0 == group)
        else {
            self.native_scroll_target = None;
            return;
        };
        let clamped = target.min(positions.saturating_sub(1));
        if first == clamped {
            self.native_scroll_target = None;
            return;
        }
        if self.native_page_request.is_none() {
            let limit = i32::from(native_scroll_limit());
            let delta = (clamped as i32 - first as i32).clamp(-limit, limit);
            self.native_page_request = (delta != 0).then_some((group, delta as i8));
        }
    }
}
