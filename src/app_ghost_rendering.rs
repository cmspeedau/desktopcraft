use crate::*;

/// Where Explorer draws a desktop icon inside its cell (physical pixels from
/// the item position at 96 DPI) and the gap to its label; tuned against live
/// captures and scaled by monitor DPI.
#[cfg(windows)]
pub(super) const GHOST_ICON_TOP: f32 = 4.;
#[cfg(windows)]
pub(super) const GHOST_LABEL_GAP: f32 = 3.;

/// Label width inside an Explorer icon cell (physical pixels).
#[cfg(windows)]
pub(super) fn ghost_label_width(cell: (i32, i32)) -> i32 { (cell.0 - 4).max(16) }

#[cfg(windows)]
impl App {
    /// Monitor DPI where `bounds` sits, plus the primary monitor's DPI.
    pub(super) fn box_monitor_dpi(&self, bounds: Bounds) -> (u32, u32) {
        let primary_dpi = (self.host_scale() * 96.0).round().max(96.0) as u32;
        let r = self.to_screen(bounds);
        let dpi = self.monitor_at(r[0] + r[2] / 2, r[1] + r[3] / 2)
            .map(|m| m.dpi)
            .filter(|&d| d > 0)
            .unwrap_or(primary_dpi);
        (dpi, primary_dpi)
    }

    pub(crate) fn group_min_size(&self, bounds: Bounds, folder: bool) -> (f32, f32) {
        if !folder
            && self.host.is_some()
            && self.native_env.chrome_only
            && self.native_env.in_box_layout
        {
            let (dpi, primary_dpi) = self.box_monitor_dpi(bounds);
            let cell = ghost_cell_for_dpi(self.native_cell, dpi, primary_dpi);
            return native_box_min_size(cell, self.host_scale(), bounds.y);
        }
        min_size(folder)
    }

    pub(crate) fn resize_min_size(
        &self,
        before: Bounds,
        delta: Vec2,
        edges: u8,
        area: Bounds,
        folder: bool,
    ) -> (f32, f32) {
        if !folder
            && self.host.is_some()
            && self.native_env.chrome_only
            && self.native_env.in_box_layout
        {
            let (dpi, primary_dpi) = self.box_monitor_dpi(before);
            let cell = ghost_cell_for_dpi(self.native_cell, dpi, primary_dpi);
            let scale = self.host_scale();
            let (min_w, min_body_h) = native_box_min_size(cell, scale, HEADER);
            let effective_y = if edges & 4 != 0 {
                let bottom = before.y + before.h;
                (before.y + delta.y).clamp(area.y, (bottom - min_body_h).max(area.y))
            } else {
                before.y
            };
            let (_, min_h) = native_box_min_size(cell, scale, effective_y);
            return (min_w, min_h);
        }
        min_size(folder)
    }

    pub(crate) fn snap_box_resize_rows(
        &self,
        bounds: Bounds,
        edges: u8,
        folder: bool,
        list: bool,
        icon_scale: f32,
    ) -> Bounds {
        if edges & 12 == 0 || self.host.is_none() || !self.native_env.chrome_only {
            return bounds;
        }
        let screen = self.to_screen(bounds);
        let max_bottom = self
            .monitor_at(screen[0] + screen[2] / 2, screen[1] + screen[3] / 2)
            .map(|m| {
                let w = self.monitor_logical(&m, true);
                w.y + w.h
            })
            .unwrap_or(self.height);
        if folder {
            let overhead = HEADER + FOOTER + 38.0;
            let pitch = if list { 36.0 } else { (cell_h() * icon_scale).max(24.0) };
            let bottom = bounds.y + bounds.h;
            let available = if edges & 4 != 0 {
                bottom - overhead
            } else {
                max_bottom - bounds.y - overhead
            };
            let max_rows = (available / pitch).floor().max(1.0);
            let rows = ((bounds.h - overhead) / pitch).round().clamp(1.0, max_rows);
            let h = overhead + rows * pitch;
            let y = if edges & 4 != 0 { bottom - h } else { bounds.y };
            return Bounds { y, h, ..bounds };
        }
        if !self.native_env.in_box_layout {
            return bounds;
        }
        let (dpi, primary_dpi) = self.box_monitor_dpi(bounds);
        let cell = ghost_cell_for_dpi(self.native_cell, dpi, primary_dpi);
        snap_native_row_height(bounds, edges, max_bottom, self.host_scale(), cell)
    }
}

#[cfg(not(windows))]
impl App {
    pub(crate) fn group_min_size(&self, _bounds: Bounds, folder: bool) -> (f32, f32) {
        min_size(folder)
    }

    pub(crate) fn resize_min_size(
        &self,
        _before: Bounds,
        _delta: Vec2,
        _edges: u8,
        _area: Bounds,
        folder: bool,
    ) -> (f32, f32) {
        min_size(folder)
    }

    pub(crate) fn snap_box_resize_rows(
        &self,
        bounds: Bounds,
        _: u8,
        _: bool,
        _: bool,
        _: f32,
    ) -> Bounds {
        bounds
    }
}

#[cfg(windows)]
impl App {

    /// Called once per frame in Native Desktop mode. `sync_pending` is true
    /// while the worker has not yet acknowledged a layout change.
    pub(super) fn update_native_ghost(
        &mut self, worker: Option<&native_filter_host::NativeTwoBoxBackgroundService>,
        sync_pending: bool,
    ) {
        if self.ghost_art.is_none() {
            if let Some(art_worker) = explorer_art::ArtWorker::start() {
                let mut art = GhostArt {
                    worker: art_worker,
                    icons: Default::default(),
                    labels: Default::default(),
                    pending_icons: Default::default(),
                    pending_labels: Default::default(),
                    snapshot_renderer: native_drag_surface::Renderer::new(),
                };
                let primary_dpi = (self.host_scale() * 96.0).round().max(96.0) as u32;
                let label_width = ghost_label_width(self.native_cell);
                // Warm the icon and primary-DPI label caches off the UI thread so drags stay responsive.
                for tab in self.state.groups.iter().flat_map(|g| &g.tabs).filter(|t| t.folder.is_none()) {
                    for pin in &tab.items {
                        art.request_icon(&pin.path);
                        art.request_label(pin.label.as_deref().unwrap_or(&pin.name), label_width, primary_dpi);
                    }
                }
                self.ghost_art = Some(art);
            }
        }
        if let Some(art) = self.ghost_art.as_mut() { art.collect(); }
        let visual_revealed = self.visual_revealed(true);
        let dragging = match &self.drag {
            Some(Drag::Group { id, before, before_dock, .. }) => Some((*id, *before, *before_dock, false)),
            Some(Drag::Resize { id, before, .. }) => {
                self.state.group(*id).map(|group| (*id, *before, group.dock, true))
            }
            _ => None,
        };
        if self.native_ghost.is_none() {
            let (Some((group, origin, dock, resizing)), Some(worker)) = (dragging, worker) else { return };
            // Start hiding Explorer icons only once the Box actually moves or
            // resizes, so a plain header or ellipsis click never flashes icons.
            let active_box = self.state.group(group).is_some_and(|g| {
                let effectively_rolled =
                    g.rolled && !(self.state.prefs.rollup && visual_revealed == Some(g.id));
                g.bounds != origin
                    && !effectively_rolled
                    && g.tab().folder.is_none()
                    && !g.tab().items.is_empty()
            });
            if !active_box { return; }
            let (origin_dpi, primary_dpi) = self.box_monitor_dpi(origin);
            let origin_cell = ghost_cell_for_dpi(self.native_cell, origin_dpi, primary_dpi);
            let origin_body = paint::native_body_for_dock(origin, dock);
            let origin_screen = self.to_screen(origin_body);
            let first_row = self.native_pages.iter().find(|s| s.0 == group).map_or(0, |s| s.1);
            let cached = self.state.group(group)
                .and_then(|g| self.native_box_ghosts.get(&(group, g.active)))
                .cloned();
            let cached_positions = cached.is_some();
            let instant_icons = cached.unwrap_or_else(|| {
                self.state.group(group).map(|g| {
                    build_instant_ghost_icons(
                        g,
                        native_box_placement::Rect {
                            x: origin_screen[0],
                            y: origin_screen[1],
                            w: origin_screen[2],
                            h: origin_screen[3],
                        },
                        origin_cell,
                        first_row,
                        self.native_snap_to_grid,
                    )
                }).unwrap_or_default()
            });
            if instant_icons.is_empty() { return; }
            if let Ok(reply) = worker.begin_drag_preview(group, resizing) {
                let started = std::time::Instant::now();
                host::event("INFO", "native_desktop.drag_started", &format!(
                    "group={group} icons={} resize={resizing} dpi={origin_dpi}", instant_icons.len()));
                self.native_ghost = Some(NativeGhost {
                    group,
                    origin,
                    origin_body,
                    resizing,
                    first_row,
                    origin_dpi,
                    icons: instant_icons,
                    main_cell: self.native_cell,
                    base_icon_size: self.native_icon_size,
                    snap_to_grid: self.native_snap_to_grid,
                    reply: Some(reply),
                    cached_positions,
                    dropped: false,
                    restoring: None,
                    snapshot: None,
                    snapshot_attempted: false,
                    trace_started: started,
                    trace_last_move: started,
                    trace_bounds: origin,
                    trace_reported_pause: false,
                });
            }
            return;
        }
        let ghost_bounds = self.native_ghost.as_ref()
            .and_then(|g| self.state.group(g.group).map(|b| b.bounds));
        let (target_dpi, primary_dpi) = ghost_bounds
            .map(|b| self.box_monitor_dpi(b))
            .unwrap_or((96, 96));
        let Some(ghost) = self.native_ghost.as_mut() else { return };
        if let Some((icons, main_cell, base_icon_size, snap_to_grid)) =
            ghost.reply.as_ref().and_then(|r| r.try_recv().ok())
        {
            let changed = ghost.icons != icons || ghost.main_cell != main_cell
                || ghost.base_icon_size != base_icon_size || ghost.snap_to_grid != snap_to_grid;
            host::event("INFO", "native_desktop.drag_native_hidden", &format!(
                "group={} elapsed_ms={} icons={} preview_reseeded={changed}",
                ghost.group, ghost.trace_started.elapsed().as_millis(), icons.len()));
            if changed {
                ghost.snapshot = None;
                ghost.snapshot_attempted = false;
            }
            ghost.icons = icons;
            ghost.main_cell = main_cell;
            ghost.base_icon_size = base_icon_size;
            ghost.snap_to_grid = snap_to_grid;
            ghost.reply = None;
            self.native_cell = main_cell;
            self.native_icon_size = base_icon_size;
            self.native_snap_to_grid = snap_to_grid;
        }
        if !ghost.icons.is_empty() {
            let cell = ghost_cell_for_dpi(ghost.main_cell, target_dpi, primary_dpi);
            let label_width = ghost_label_width(cell);
            if let Some(art) = self.ghost_art.as_mut() {
                for icon in &ghost.icons {
                    art.request_icon(&icon.path);
                    art.request_label(&icon.label, label_width, target_dpi);
                }
            }
        }
        if !ghost.dropped && dragging.is_some_and(|(group, ..)| group == ghost.group) {
            if let Some(bounds) = ghost_bounds {
                if bounds != ghost.trace_bounds {
                    ghost.trace_bounds = bounds;
                    ghost.trace_last_move = std::time::Instant::now();
                } else if !ghost.trace_reported_pause && ghost.trace_last_move.elapsed().as_millis() >= 180 {
                    ghost.trace_reported_pause = true;
                    host::event("INFO", "native_desktop.drag_held_still", &format!(
                        "group={} elapsed_ms={} snapshot={}", ghost.group,
                        ghost.trace_started.elapsed().as_millis(), ghost.snapshot.is_some()));
                }
            }
            return;
        }
        if !ghost.dropped {
            host::event("INFO", "native_desktop.drag_released", &format!(
                "group={} elapsed_ms={} waiting_for_layout={} snapshot={}", ghost.group,
                ghost.trace_started.elapsed().as_millis(), sync_pending, ghost.snapshot.is_some()));
        }
        ghost.dropped = true;
        // The drop's workspace ACK now includes Explorer painting the final
        // generation. A refused or cancelled drop has no such final ACK.
        if sync_pending { return; }
        let current = self.state.group(ghost.group).map(|g| {
            let effectively_rolled =
                g.rolled && !(self.state.prefs.rollup && visual_revealed == Some(g.id));
            (g.bounds, effectively_rolled)
        });
        if ghost_should_end_preview(current, ghost.origin) {
            use native_filter_host::NativeWorkspaceReply;
            let result = worker.ok_or_else(|| "Native drag preview worker unavailable".to_string())
                .and_then(|worker| match ghost.restoring.as_ref() {
                    Some(ticket) => worker.poll_workspace_sync(ticket),
                    None => {
                        ghost.restoring = Some(worker.end_drag_preview(ghost.group)?);
                        Ok(NativeWorkspaceReply::Pending)
                    }
                });
            match result {
                Ok(NativeWorkspaceReply::Pending) => return,
                Ok(NativeWorkspaceReply::Applied(_)) => {}
                Err(reason) | Ok(NativeWorkspaceReply::Rejected(reason)) => {
                    host::event("WARN", "native_desktop.drag_preview_restore_failed", &reason);
                    self.save_allowed = false;
                    if let Some(host) = self.host.as_mut() { host.quit = true; }
                }
            }
        }
        let _ = self.retire_native_ghost();
    }

    pub(super) fn retire_native_ghost(&mut self) -> Option<NativeGhost> {
        let ghost = self.native_ghost.take()?;
        host::event("INFO", "native_desktop.drag_preview_retired", &format!(
            "group={} elapsed_ms={}", ghost.group, ghost.trace_started.elapsed().as_millis()));
        Some(ghost)
    }

    /// Draw the dragged/resized Box's icon copies at their scaled cells.
    /// `releasing` is true on the mouse-up frame (before `pointer()` clears
    /// `self.drag`), so copies snap to the target Explorer grid on frame 0.
    pub(super) fn draw_native_ghost(&mut self, p: &Painter, releasing: bool) {
        let Some(mut ghost) = self.native_ghost.take() else { return };
        self.draw_ghost_preview(p, &mut ghost, releasing);
        self.native_ghost = Some(ghost);
    }

    pub(super) fn draw_ghost_preview(&self, p: &Painter, ghost: &mut NativeGhost, releasing: bool) {
        let Some(group) = self.state.group(ghost.group) else { return };
        if ghost.icons.is_empty() || self.previewing_top_dock() == Some(group.id)
            || (group.rolled && self.revealed != Some(group.id)) {
            return;
        }
        let s = self.host_scale();
        let (target_dpi, primary_dpi) = self.box_monitor_dpi(group.bounds);
        let origin_cell = ghost_cell_for_dpi(ghost.main_cell, ghost.origin_dpi, primary_dpi);
        let target_cell = ghost_cell_for_dpi(ghost.main_cell, target_dpi, primary_dpi);
        let dragging_this = matches!(
            &self.drag,
            Some(Drag::Group { id, .. }) | Some(Drag::Resize { id, .. }) if *id == ghost.group
        );
        let dropped = ghost.dropped || releasing || !dragging_this;
        let origin_screen = self.to_screen(ghost.origin_body);
        let target_screen = self.to_screen(paint::native_group_body(group));
        let dx_phys = target_screen[0] - origin_screen[0];
        let dy_phys = target_screen[1] - origin_screen[1];
        if !ghost.resizing && target_dpi == ghost.origin_dpi
            && (ghost.cached_positions || ghost.reply.is_none()) {
            if let Some(art) = self.ghost_art.as_ref() {
                if let Some(renderer) = art.snapshot_renderer.as_ref() {
                    if !dropped && !ghost.snapshot_attempted
                        && ghost.icons.iter().all(|icon| art.icons.contains_key(&icon.path)
                            && art.labels.contains_key(&(icon.label.clone(), ghost_label_width(origin_cell), target_dpi)))
                    {
                        ghost.snapshot_attempted = true;
                        ghost.snapshot = self.capture_ghost_snapshot(p, renderer, ghost, origin_screen, origin_cell);
                    }
                    if let Some(snapshot) = ghost.snapshot.as_ref() {
                        let (dx, dy) = if dropped && ghost.snap_to_grid {
                            ghost_drop_shift(&ghost.icons, target_cell, target_screen).unwrap_or((dx_phys, dy_phys))
                        } else {
                            (dx_phys, dy_phys)
                        };
                        renderer.draw(&snapshot.target, snapshot.at + vec2(dx as f32, dy as f32) / s, snapshot.size);
                        return;
                    }
                }
            }
        }
        let clip_partial = ghost.resizing && !dropped && !ghost.snap_to_grid;
        let positions = if ghost.resizing && !ghost.snap_to_grid {
            let first_row = if ghost.reply.is_none() { ghost.first_row } else { 0 };
            let half_icon = clip_partial.then(|| {
                let top_gap = (GHOST_ICON_TOP * target_dpi.max(96) as f32 / 96.).round();
                (top_gap + ghost_icon_px(ghost.base_icon_size, target_dpi) / 2.).ceil() as i32
            });
            ghost_resize_positions(ghost.icons.len(), None, target_cell, target_screen, first_row, half_icon)
        } else {
            ghost_icon_positions(
                &ghost.icons, ghost.resizing, dropped, ghost.snap_to_grid,
                origin_cell, target_cell, (dx_phys, dy_phys), target_screen,
            )
        };
        let clip = clip_partial.then(|| self.host.as_ref()).flatten().map(|host| (
            target_screen[0] - host.virtual_rect[0], target_screen[1] - host.virtual_rect[1],
            target_screen[2], target_screen[3],
        ));
        if let Some(clip) = clip {
            unsafe { get_internal_gl().quad_gl.scissor(Some(clip)); }
        }
        self.draw_ghost_icons(p, ghost, positions, target_cell, target_dpi);
        if clip.is_some() {
            unsafe { get_internal_gl().quad_gl.scissor(None); }
        }
    }

    pub(super) fn capture_ghost_snapshot(
        &self,
        p: &Painter,
        renderer: &native_drag_surface::Renderer,
        ghost: &NativeGhost,
        origin: [i32; 4],
        cell: (i32, i32),
    ) -> Option<NativeGhostSnapshot> {
        // A label is allowed to extend slightly beyond the Box body.
        let (pad_x, pad_y) = (cell.0.max(16), cell.1.max(16));
        let w = origin[2].saturating_add(pad_x.saturating_mul(2));
        let h = origin[3].saturating_add(pad_y.saturating_mul(2));
        if !(1..=3072).contains(&w) || !(1..=3072).contains(&h) { return None; }
        let at = self.from_screen(origin[0].saturating_sub(pad_x), origin[1].saturating_sub(pad_y));
        let size = vec2(w as f32, h as f32) / self.host_scale();
        let target = renderer.capture(Rect::new(at.x, at.y, size.x, size.y), (w as u32, h as u32), || {
            self.draw_ghost_icons(p, ghost, ghost.icons.iter().map(|icon| icon.pos).enumerate(),
                cell, ghost.origin_dpi);
        });
        host::event("INFO", "native_desktop.drag_snapshot_captured", &format!(
            "group={} elapsed_ms={} icons={} pixels={}x{}", ghost.group,
            ghost.trace_started.elapsed().as_millis(), ghost.icons.len(), w, h));
        Some(NativeGhostSnapshot { target, at, size })
    }

    pub(super) fn draw_ghost_icons(
        &self,
        p: &Painter,
        ghost: &NativeGhost,
        positions: impl IntoIterator<Item = (usize, (i32, i32))>,
        target_cell: (i32, i32),
        target_dpi: u32,
    ) {
        let s = self.host_scale();
        let sx = target_cell.0 as f32 / s;
        let dpi_scale = target_dpi.max(96) as f32 / 96.0;
        let icon = ghost_icon_px(ghost.base_icon_size, target_dpi) / s;
        let icon_top = (GHOST_ICON_TOP * dpi_scale).round() / s;
        let label_gap = (GHOST_LABEL_GAP * dpi_scale).round() / s;
        let label_width = ghost_label_width(target_cell);
        let art = self.ghost_art.as_ref();
        let tint = self.state.tint.then(|| self.state.group(ghost.group).map(paint::tint_color)).flatten();
        for (idx, pos) in positions {
            let item = &ghost.icons[idx];
            let at = self.from_screen(pos.0, pos.1);
            // IFolderView position is the icon bitmap's top-left corner;
            // labels are centered under that bitmap, matching Explorer pixels.
            let (ix, iy) = (at.x, at.y + icon_top);
            match art.and_then(|a| a.icons.get(&item.path).cloned().flatten()) {
                Some(texture) => draw_texture_ex(&texture, ix, iy, tint.unwrap_or(WHITE),
                    DrawTextureParams { dest_size: Some(vec2(icon, icon)), ..Default::default() }),
                None => p.icon_path(&item.path, extension_of(&item.path), ix, iy, icon, tint),
            }
            let ly = iy + icon + label_gap;
            match art.and_then(|a| a.labels.get(&(item.label.clone(), label_width, target_dpi)).cloned().flatten()) {
                Some(texture) => {
                    let (w, h) = (texture.width() / s, texture.height() / s);
                    // The label bitmap is already rasterized in screen pixels.
                    let lx = at.x + ((icon - w) * s / 2.).round() / s;
                    let params = || DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() };
                    // The desktop's dark text shadow, then the white label.
                    draw_texture_ex(&texture, lx, ly + 1. / s, Color::new(0., 0., 0., 0.75), params());
                    draw_texture_ex(&texture, lx, ly, WHITE, params());
                }
                None => {
                    for (n, line) in p.wrap_label_cached(&item.label, 12. * dpi_scale / s, sx - 6.).iter().take(2).enumerate() {
                        let w = p.measure(line, 12. * dpi_scale / s);
                        let (lx, lyy) = (at.x + (icon - w) / 2., ly + n as f32 * (15. * dpi_scale / s));
                        p.text(line, lx + 1., lyy + 1., 12. * dpi_scale / s, Color::new(0., 0., 0., 0.85));
                        p.text(line, lx, lyy, 12. * dpi_scale / s, WHITE);
                    }
                }
            }
        }
    }
}
