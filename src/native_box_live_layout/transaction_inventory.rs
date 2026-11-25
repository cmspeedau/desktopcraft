use super::*;

impl LiveLayoutTransaction {
    pub(super) fn absorb_pending_scaled_shifts(&mut self, current: &[ViewItem]) {
        if self.pending_scaled_recheck_at.is_none() || self.scaled_targets.is_empty()
            || current.len() != self.expected.len()
        {
            return;
        }
        let now_by_key = current.iter()
            .map(|i| (i.path.to_lowercase(), i.pos))
            .collect::<BTreeMap<_, _>>();
        if now_by_key.len() != current.len() {
            return;
        }
        // Only absorb minor DPI-rounding settle shifts (<=3 px from scaled target)
        // when all icons keep distinct coordinates.
        let mut updates = Vec::new();
        for icon in &self.expected {
            let key = icon.path.to_lowercase();
            let Some(&live_pos) = now_by_key.get(&key) else { return; };
            if live_pos == icon.pos {
                continue;
            }
            let Some(&target) = self.scaled_targets.get(&key) else { continue; };
            if within_settle_tolerance(live_pos, target) {
                updates.push((key, live_pos));
            }
        }
        if updates.is_empty() {
            return;
        }
        let scaled_cells = current.iter()
            .filter(|i| self.scaled_targets.contains_key(&i.path.to_lowercase()))
            .map(|i| i.pos)
            .collect::<Vec<_>>();
        if scaled_cells.iter().enumerate().any(|(n, a)| scaled_cells[n + 1..].contains(a)) {
            return;
        }
        for (key, live_pos) in updates {
            if let Some(exp) = self.expected.iter_mut().find(|i| i.path.eq_ignore_ascii_case(&key)) {
                exp.pos = live_pos;
            }
            for (_, pos) in self.moved.iter_mut().chain(self.nudged_loose.iter_mut())
                .filter(|(p, _)| p.eq_ignore_ascii_case(&key))
            {
                *pos = live_pos;
            }
        }
    }

    pub(super) fn snap_loose_targets_on_view(
        &mut self,
        view: &DesktopView,
        targets: Vec<(String, (i32, i32))>,
        active_boxes: &[Rect],
        current: &mut Vec<ViewItem>,
        geometry: Option<((i32, i32), &[host::Monitor])>,
        place: impl FnOnce(&[Move]) -> bool,
    ) {
        if targets.is_empty()
            || !self.original_snap_to_grid
            || self.snap_to_grid
            || host::mouse_left_down()
        {
            return;
        }
        let Some(desktop_origin) = self.desktop_grid_origin else {
            return;
        };
        let fallback_monitors;
        let (main_cell, monitors) = if let Some(geometry) = geometry {
            geometry
        } else {
            let Some(cell) = view.spacing() else { return; };
            fallback_monitors = host::monitors();
            (cell, fallback_monitors.as_slice())
        };
        let resolved = compute_loose_drop_targets(
            &targets, self.original_snap_to_grid, self.snap_to_grid, Some(desktop_origin),
            main_cell, monitors, &self.expected, &self.moved, active_boxes, self.hidden_park,
        );
        let snap_moves: Vec<_> = resolved.into_iter().filter(|(path, snapped)| {
            self.expected.iter().any(|item| item.path.eq_ignore_ascii_case(path) && item.pos != *snapped)
        }).collect();
        if !snap_moves.is_empty() {
            if place(&snap_moves) {
                for (path, snapped) in &snap_moves {
                    if let Some(item) = current.iter_mut().find(|item| item.path.eq_ignore_ascii_case(path)) {
                        item.pos = *snapped;
                    }
                }
            } else {
                *current = view.items();
            }
            // Read back Explorer's settled coordinates so monitor-edge
            // clamping or DPI rounding cannot trigger a repeat snap.
            let _ = adopt_loose_positions(
                &mut self.original, &mut self.expected, &self.moved, current,
            );
        }
    }

    pub fn positions_match(&self, current: &[ViewItem]) -> bool {
        same_icon_positions(&self.expected, current)
    }

    pub(crate) fn is_parked(&self, path: &str) -> bool {
        self.parked_paths.contains(&path.to_lowercase())
    }

    pub fn reconcile_loose_positions(&mut self, view: &DesktopView, active_boxes: &[Rect]) -> Result<(bool, Vec<ViewItem>), String> {
        self.reconcile_observed_loose_positions(view, active_boxes, view.items())
    }

    pub(crate) fn reconcile_observed_loose_positions(
        &mut self, view: &DesktopView, active_boxes: &[Rect], current: Vec<ViewItem>,
    ) -> Result<(bool, Vec<ViewItem>), String> {
        self.reconcile_loose_drop(view, active_boxes, current, None, |moves| {
            view.position(moves);
            false
        })
    }

    pub(crate) fn reconcile_loose_drop(
        &mut self, view: &DesktopView, active_boxes: &[Rect], mut current: Vec<ViewItem>,
        geometry: Option<((i32, i32), &[host::Monitor])>,
        place: impl FnOnce(&[Move]) -> bool,
    ) -> Result<(bool, Vec<ViewItem>), String> {
        self.absorb_pending_scaled_shifts(&current);
        if !host::mouse_left_down() {
            let (displaced_parked, _) = split_displaced_hidden_park(
                &self.expected,
                &current,
                &self.parked_paths,
                self.hidden_park,
                None,
            );
            if !displaced_parked.is_empty() {
                current = self.repark_displaced_hidden_icons(view, &displaced_parked)?;
            }
            let drift = nearby_box_cell_repairs(
                &self.expected, &current, &self.moved, &self.parked_paths, active_boxes,
            );
            if !drift.is_empty() && !host::mouse_left_down() {
                current = self.restore_expected_subset(view, &drift)?;
                host::event("INFO", "native_desktop.box_cells_reseated", &format!("count={}", drift.len()));
            }
        }
        let moved_loose = adopt_loose_positions_tracking(
            &mut self.original, &mut self.expected, &self.moved, &current,
        )?;
        if moved_loose.is_empty() {
            return Ok((false, current));
        }
        for (path, _) in &moved_loose {
            self.nudged_loose.retain(|(nudged_path, _)| !nudged_path.eq_ignore_ascii_case(path));
        }
        // Software Align-to-Grid for loose Desktop icons dropped outside Boxes
        // when the user originally had Windows Align-to-Grid enabled.
        self.snap_loose_targets_on_view(view, moved_loose, active_boxes, &mut current, geometry, place);
        Ok((true, current))
    }

    pub(crate) fn reconcile_inventory(
        &mut self,
        view: &DesktopView,
        active_boxes: &[Rect],
        state: &State,
        skip_snap_added_in_boxes: bool,
    ) -> Result<InventoryReconcileOutcome, String> {
        let mut current = view.items();
        self.absorb_pending_scaled_shifts(&current);
        if !host::mouse_left_down() {
            let (displaced_parked, _) = split_displaced_hidden_park(
                &self.expected,
                &current,
                &self.parked_paths,
                self.hidden_park,
                None,
            );
            if !displaced_parked.is_empty() {
                current = self.repark_displaced_hidden_icons(view, &displaced_parked)?;
            }
        }
        let outcome = reconcile_desktop_inventory(
            &mut self.original,
            &mut self.expected,
            &mut self.moved,
            &mut self.nudged_loose,
            &current,
        )?;
        for path in &outcome.removed {
            let key = path.to_lowercase();
            self.scaled_targets.remove(&key);
            self.parked_paths.remove(&key);
        }
        let mut snap_targets = outcome.moved_loose.clone();
        for item in &outcome.added {
            let inside_box = active_boxes.iter().any(|r| r.contains(item.pos));
            if !skip_snap_added_in_boxes || !inside_box {
                snap_targets.push((item.path.clone(), item.pos));
            }
        }
        self.snap_loose_targets_on_view(view, snap_targets, active_boxes, &mut current, None,
            |moves| {
                view.position(moves);
                false
            });
        if !outcome.added.is_empty() || !outcome.removed.is_empty() {
            self.native_batch_shell_verified = false;
            self.persist_verified_generation(&current, state, true)?;
        }
        Ok(outcome)
    }

    /// Update tracked layout positions after Explorer completes a Desktop icon
    /// size/spacing change (e.g. Ctrl + Mouse Wheel), resetting Box scroll rows.
    pub(crate) fn adopt_explorer_spacing_change(
        &mut self,
        view: &DesktopView,
        new_cell: (i32, i32),
    ) -> Result<Vec<ViewItem>, String> {
        let monitors = host::monitors();
        if host::signature(&monitors) != self.initial_monitor_signature {
            return Err("Display topology changed during Explorer icon size update".into());
        }
        if !view.native_placement_settings(true) {
            return Err("Explorer arrangement settings incompatible with Native Desktop after zoom".into());
        }
        let current = view.items();
        let drop_origins = self.original.iter().filter(|item| self.pending_drop_pre_positions.iter()
            .any(|(path, _)| path.eq_ignore_ascii_case(&item.path)))
            .map(|item| (item.path.clone(), item.pos)).collect::<Vec<_>>();
        adopt_spacing_positions(
            &mut self.original,
            &mut self.expected,
            &mut self.moved,
            &mut self.nudged_loose,
            &current,
        )?;
        self.reset_all_pages();
        self.hidden_park = None;
        self.parked_paths.clear();
        self.pending_scaled_recheck_at = None;
        self.scaled_targets.clear();
        self.native_batch_shell_verified = false;
        self.rollback_loose_drop_targets(&drop_origins);
        if self.original_snap_to_grid {
            let placed_keys = self
                .moved
                .iter()
                .map(|(p, _)| p.to_lowercase())
                .collect::<BTreeSet<_>>();
            let loose_positions = current
                .iter()
                .filter(|item| !placed_keys.contains(&item.path.to_lowercase()))
                .map(|item| item.pos)
                .collect::<Vec<_>>();
            if let Some(main) = monitors.iter().find(|m| m.primary).or_else(|| monitors.first()) {
                if let Ok(origin) =
                    grid_origin_for_screen(monitor_rect(main), &loose_positions, new_cell)
                {
                    self.desktop_grid_origin = Some(origin);
                }
            }
        }
        Ok(current)
    }

    pub(crate) fn persist_current_checkpoint(&self, state: &State) -> Result<(), String> {
        self.persist_verified_generation(&self.expected, state, false)
    }

    pub(crate) fn apply_loose_drop_targets(
        &mut self,
        targets: &[(String, (i32, i32))],
        state: &State,
        view: &DesktopView,
    ) -> Vec<(String, (i32, i32))> {
        if targets.is_empty() {
            return Vec::new();
        }
        let monitors = host::monitors();
        let active_boxes = DesktopCoordinates::from_monitors(&monitors)
            .map(|coords| coords.active_box_rects(state))
            .unwrap_or_default();
        let main_cell = view.spacing().unwrap_or((75, 107));
        let resolved = compute_loose_drop_targets(
            targets,
            self.original_snap_to_grid,
            self.snap_to_grid,
            self.desktop_grid_origin,
            main_cell,
            &monitors,
            &self.expected,
            &self.moved,
            &active_boxes,
            self.hidden_park,
        );
        let mut prior = Vec::with_capacity(resolved.len());
        for (path, dest) in resolved {
            if let Some(orig) = self.original.iter_mut()
                .find(|icon| icon.path.eq_ignore_ascii_case(&path))
            {
                prior.push((orig.path.clone(), orig.pos));
                orig.pos = dest;
            }
            self.nudged_loose.retain(|(p, _)| !p.eq_ignore_ascii_case(&path));
        }
        prior
    }

    pub(crate) fn rollback_loose_drop_targets(&mut self, prior: &[(String, (i32, i32))]) {
        for (path, pos) in prior {
            if let Some(orig) = self.original.iter_mut()
                .find(|icon| icon.path.eq_ignore_ascii_case(path))
            {
                orig.pos = *pos;
            }
        }
    }

    pub fn placed_by_app(&self, path: &str) -> bool {
        self.moved.iter().any(|(placed, _)| placed.eq_ignore_ascii_case(path))
    }

    pub(crate) fn filter_displaced_hidden_park(
        &self,
        observed: &[ViewItem],
        eligible: &std::collections::HashSet<String>,
    ) -> (Vec<String>, Vec<ViewItem>) {
        split_displaced_hidden_park(
            &self.expected,
            observed,
            &self.parked_paths,
            self.hidden_park,
            Some(eligible),
        )
    }

    pub(crate) fn restore_expected_subset(
        &self,
        view: &DesktopView,
        paths: &[String],
    ) -> Result<Vec<ViewItem>, String> {
        if paths.is_empty() {
            return Ok(view.items());
        }
        if host::signature(&host::monitors()) != self.initial_monitor_signature {
            return Err("Display geometry changed before restoring Explorer icons".into());
        }
        let mut seen = BTreeSet::new();
        let mut moves = Vec::with_capacity(paths.len());
        for path in paths {
            let key = path.to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            if !self.placed_by_app(path) && !self.parked_paths.contains(&key) {
                return Err("Refusing to restore non-managed Explorer icon to expected cell".into());
            }
            let expected = self
                .expected
                .iter()
                .find(|icon| icon.path.eq_ignore_ascii_case(path))
                .ok_or("Native recovery baseline has no source item")?;
            moves.push((expected.path.clone(), expected.pos));
        }
        if moves.is_empty() {
            return Ok(view.items());
        }
        if view.position(&moves) != moves.len() {
            return Err("Could not restore managed Explorer icons to their acknowledged cells".into());
        }
        let targets = moves
            .iter()
            .map(|(p, pos)| (p.to_lowercase(), *pos))
            .collect::<BTreeMap<_, _>>();
        let settled = wait_until_settled(false, &[0, 25, 35, 50, 50], || {
            let items = view.items();
            let now = items
                .iter()
                .map(|i| (i.path.to_lowercase(), i.pos))
                .collect::<BTreeMap<_, _>>();
            let ok = targets.iter().all(|(k, pos)| now.get(k) == Some(pos));
            (ok, items)
        });
        let settled_now = settled
            .iter()
            .map(|i| (i.path.to_lowercase(), i.pos))
            .collect::<BTreeMap<_, _>>();
        if !targets.iter().all(|(k, pos)| settled_now.get(k) == Some(pos)) {
            return Err("Explorer did not settle restored managed icons to their verified cells".into());
        }
        Ok(settled)
    }

    pub(crate) fn repark_displaced_hidden_icons(
        &self,
        view: &DesktopView,
        displaced: &[String],
    ) -> Result<Vec<ViewItem>, String> {
        let settled = self.restore_expected_subset(view, displaced)?;
        if !displaced.is_empty() {
            host::event(
                "INFO",
                "native_desktop.hidden_park_reseated",
                &format!(
                    "count={} park={:?}",
                    displaced.len(),
                    self.hidden_park
                ),
            );
        }
        Ok(settled)
    }
}
