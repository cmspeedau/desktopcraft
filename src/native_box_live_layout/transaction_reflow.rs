use super::*;

impl LiveLayoutTransaction {
    pub fn reflow(&mut self,view:&DesktopView,
                  member:&NativeTwoBoxMembership,state:&State,
                  native_original:&[ViewItem],
                  mut prepared: Option<((i32, i32), NativePlacementPlan)>,
                  try_native_batch: impl FnOnce(&[Vec<Move>], &[ViewItem]) -> Result<bool, String>,
                  )->Result<LiveLayoutReport,String>{
        let monitors = host::monitors();
        if host::signature(&monitors)!=self.initial_monitor_signature{
            return Err("Display topology changed before native layout mutation".into());
        }
        let mut timing=StepTimer::new();
        if !view.native_placement_settings(true)
            || view.snap_to_grid() != Some(self.snap_to_grid) {
            return Err("Explorer arrangement, identities or positions changed outside Native layout transaction".into());
        }
        timing.mark("settings");
        let mut try_native_batch = Some(try_native_batch);
        let main_monitor = monitors.iter().find(|m| m.primary).or_else(|| monitors.first());
        let is_scaled_point = |pos: (i32, i32)| {
            let Some(main) = main_monitor else { return false; };
            monitor_for_point(pos, &monitors)
                .is_some_and(|m| m.dpi != main.dpi && main.dpi > 0 && m.dpi > 0)
        };
        if self.native_batch_shell_verified
            && !self.snap_to_grid
            && self.pending_scaled_recheck_at.is_none()
        {
            let fast_cell = view.spacing().ok_or("Native Explorer icon spacing unavailable")?;
            let (plans, hidden_indices, hidden_park, raw_nudged) = match prepared.take() {
                Some((cell, plan)) if cell == fast_cell => plan,
                _ => self.planned_visibility(member, state, native_original, fast_cell)?,
            };
            timing.mark("fast_plan");
            let has_scaled_targets = plans.iter().any(|plan| plan.cell != fast_cell)
                || raw_nudged.iter().any(|(_, dest)| is_scaled_point(*dest));
            let nudged_unchanged = same_move_set(&self.nudged_loose, &raw_nudged);
            if !has_scaled_targets && nudged_unchanged {
                let visible_placement = plans
                    .iter()
                    .flat_map(|plan| plan.moves.iter().map(|p| (p.path.clone(), p.to)))
                    .collect::<Vec<_>>();
                let parked_placement = match hidden_park {
                    Some(park) => hidden_indices
                        .iter()
                        .filter_map(|&idx| native_original.get(idx).map(|item| (item.path.clone(), park)))
                        .collect::<Vec<_>>(),
                    None => Vec::new(),
                };
                let mut placement = visible_placement.clone();
                placement.extend(parked_placement.iter().cloned());
                let original_pos = |path: &str| {
                    self.original
                        .iter()
                        .find(|icon| icon.path.eq_ignore_ascii_case(path))
                        .map(|icon| icon.pos)
                };
                let steps = plan_free_moves(
                    &self.moved,
                    &self.nudged_loose,
                    &visible_placement,
                    &parked_placement,
                    &raw_nudged,
                    &self.expected,
                    original_pos,
                )?;
                let steps_hit_scaled = steps
                    .iter()
                    .flatten()
                    .any(|(_, dest)| is_scaled_point(*dest));
                if can_attempt_fast_native_batch(
                    self.native_batch_shell_verified,
                    self.snap_to_grid,
                    self.pending_scaled_recheck_at.is_some(),
                    has_scaled_targets || steps_hit_scaled,
                    !steps.is_empty(),
                    nudged_unchanged,
                ) {
                    let applied = match try_native_batch.take() {
                        Some(f) => f(&steps, &self.expected)?,
                        None => false,
                    };
                    if applied {
                        let mut next_expected = self.expected.clone();
                        for batch in &steps {
                            for (path, to) in batch {
                                if let Some(icon) = next_expected
                                    .iter_mut()
                                    .find(|icon| icon.path.eq_ignore_ascii_case(path))
                                {
                                    icon.pos = *to;
                                }
                            }
                        }
                        let mut rollback = placement.clone();
                        rollback.extend(
                            self.moved
                                .iter()
                                .filter(|(path, _)| !placement.iter().any(|(next, _)| next.eq_ignore_ascii_case(path)))
                                .cloned(),
                        );
                        if next_expected.len() != self.original.len()
                            || !positions_settled(
                                &next_expected,
                                &self.original,
                                &placement,
                                &raw_nudged,
                                &BTreeSet::new(),
                            )
                        {
                            self.moved = rollback;
                            let _ = self.restore(view);
                            return Err("Fast native batch layout postcondition failed".into());
                        }
                        self.moved = rollback;
                        self.expected = next_expected;
                        self.hidden_park = hidden_park;
                        self.parked_paths = parked_placement
                            .iter()
                            .map(|(p, _)| p.to_lowercase())
                            .collect();
                        self.pending_scaled_recheck_at = None;
                        self.scaled_targets.clear();
                        timing.mark("fast_moves");
                        if let Err(error) = self.persist_verified_generation(&self.expected, state, false) {
                            let _ = self.restore(view);
                            return Err(format!("Native position journal ACK not durable: {error}"));
                        }
                        self.moved = placement;
                        self.nudged_loose = raw_nudged;
                        self.pending_drop_pre_positions.clear();
                        timing.mark("journal");
                        self.count_changes += 1;
                        for ((group, _), plan) in member.selected_tabs.iter().zip(&plans) {
                            self.set_page(*group, plan.page);
                        }
                        let native_order = native_original
                            .iter()
                            .map(|prior| {
                                self.expected
                                    .iter()
                                    .find(|now| now.path.eq_ignore_ascii_case(&prior.path))
                                    .cloned()
                                    .expect("native index identities already verified")
                            })
                            .collect::<Vec<_>>();
                        return Ok(LiveLayoutReport {
                            hidden_indices,
                            expected_native_order: native_order,
                            page_status: member
                                .selected_tabs
                                .iter()
                                .zip(plans.iter())
                                .map(|((gid, _), plan)| (*gid, plan.page, plan.page_count, plan.visible_rows))
                                .collect(),
                            visible_paths: visible_placement.iter().map(|(path, _)| path.clone()).collect(),
                            observed_after: self.expected.clone(),
                            timing: format!(
                                "{} moves={} fast=1",
                                timing.summary(),
                                steps.iter().map(Vec::len).sum::<usize>()
                            ),
                            fast_batch_applied: true,
                        });
                    }
                }
            }
        }
        let active_boxes = DesktopCoordinates::current().map(|coords| {
            state.groups.iter()
                .filter(|g| g.page == state.page && !g.rolled)
                .map(|g| coords.screen_rect(group_native_bounds(g)))
                .collect::<Vec<_>>()
        }).unwrap_or_default();
        let (loose_changed, before) = self.reconcile_loose_positions(view, &active_boxes)?;
        timing.mark("reconcile");
        let cell=view.spacing().ok_or("Native Explorer icon spacing unavailable")?;
        let (plans,hidden_indices,hidden_park,raw_nudged) = match prepared {
            Some((cached_cell, plan)) if cached_cell == cell && !loose_changed => plan,
            _ => self.planned_visibility(member, state, native_original, cell)?,
        };
        timing.mark("plan");
        // Plan ENTIRE next generation before moving any item, to prevent a
        // partial Tab or viewport change from losing native icons.
        let old=std::mem::take(&mut self.moved);
        let old_nudged=std::mem::take(&mut self.nudged_loose);
        timing.mark("items");
        if !same_icon_positions(&before,&self.expected){
            self.moved=old;
            self.nudged_loose=old_nudged;
            return Err("Native icons moved outside the last acknowledged layout; keep recovery journal".into());
        }
        let mut scaled_targets = BTreeMap::new();
        for plan in &plans {
            if plan.cell != cell {
                for p in &plan.moves {
                    scaled_targets.insert(p.path.to_lowercase(), p.to);
                }
            }
        }
        for (path, dest) in &raw_nudged {
            if is_scaled_point(*dest) {
                scaled_targets.insert(path.to_lowercase(), *dest);
            }
        }
        // On a monitor scaled differently from the main one, Explorer may
        // settle an icon a pixel or two from the computed cell; an icon
        // already there keeps its settled position instead of moving again.
        let visible_placement=plans.iter().flat_map(|plan|plan.moves.iter().map(|p|{
            let settled=(plan.cell!=cell).then(||before.iter()
                .find(|now|now.path.eq_ignore_ascii_case(&p.path)
                    && within_settle_tolerance(now.pos,p.to)).map(|now|now.pos)).flatten();
            (p.path.clone(),settled.unwrap_or(p.to))
        })).collect::<Vec<_>>();
        let nudged_placement=raw_nudged.iter().map(|(path, dest)|{
            let settled = scaled_targets.contains_key(&path.to_lowercase()).then(||
                before.iter().find(|now| now.path.eq_ignore_ascii_case(path)
                    && within_settle_tolerance(now.pos, *dest)).map(|now| now.pos)
            ).flatten();
            (path.clone(), settled.unwrap_or(*dest))
        }).collect::<Vec<_>>();
        let parked_placement=match hidden_park {
            Some(park) => hidden_indices.iter()
                .filter_map(|&idx| native_original.get(idx).map(|item| (item.path.clone(), park)))
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };
        let mut placement=visible_placement.clone();
        placement.extend(parked_placement.iter().cloned());
        let move_cell=plans.iter().fold(cell,|c,plan|(c.0.max(plan.cell.0),c.1.max(plan.cell.1)));
        let original_pos=|path:&str|self.original.iter()
            .find(|icon|icon.path.eq_ignore_ascii_case(path)).map(|icon|icon.pos);
        // Move only what changes. With Align-to-Grid off, hidden icons and
        // nudged loose icons move in batch 1 and visible Box icons in batch 2.
        let steps=if !self.snap_to_grid {
            match plan_free_moves(
                &old, &old_nudged, &visible_placement, &parked_placement, &nudged_placement, &before, original_pos,
            ){
                Ok(steps)=>steps,
                Err(error)=>{self.moved=old;self.nudged_loose=old_nudged;return Err(error);}
            }
        } else {
            match plan_incremental_moves(&old,&placement,&before,move_cell,original_pos){
                Ok(steps)=>steps,
                Err(error)=>{self.moved=old;self.nudged_loose=old_nudged;return Err(error);}
            }
        };
        // Capture rollback intent before the policy thread or IFolderView moves anything: every
        // previous and next app-placed or app-nudged icon goes back to original.
        let mut rollback=placement.clone();
        rollback.extend(old.into_iter().filter(|(path,_)|
            !placement.iter().any(|(next,_)|next.eq_ignore_ascii_case(path))));
        let mut nudged_rollback=nudged_placement.clone();
        nudged_rollback.extend(old_nudged.into_iter().filter(|(path,_)|
            !nudged_placement.iter().any(|(next,_)|next.eq_ignore_ascii_case(path))));
        self.moved=rollback;
        self.nudged_loose=nudged_rollback;
        self.hidden_park=hidden_park;
        self.parked_paths=parked_placement
            .iter()
            .map(|(p,_)|p.to_lowercase())
            .collect();
        let can_move_batch = !steps.is_empty() && !self.snap_to_grid && !loose_changed
            && self.pending_scaled_recheck_at.is_none() && scaled_targets.is_empty();
        self.pending_scaled_recheck_at=None;
        self.scaled_targets.clear();
        let moved_as_batch = can_move_batch
            && match try_native_batch.take() {
                Some(f) => f(&steps, &before)?,
                None => false,
            };
        let mut batch_observed = moved_as_batch.then(|| view.items());
        if batch_observed.as_ref().is_some_and(|items| same_icon_positions(&before, items)) {
            // A native ListView move is insufficient if Explorer's Shell view
            // still reports the unchanged layout. Use the PIDL-based path.
            batch_observed = None;
        }
        for batch in &steps {
            if batch_observed.is_none() {
                let placed=view.position(batch);
                if placed!=batch.len(){
                    let _=self.restore(view);
                    return Err(format!("Native Explorer moved {placed}/{} icons; generation refused",batch.len()));
                }
            }
            for (path,to) in batch {
                if let Some(icon)=self.expected.iter_mut()
                    .find(|icon|icon.path.eq_ignore_ascii_case(path)) {icon.pos=*to;}
            }
        }
        self.moved=placement.clone();
        self.nudged_loose=nudged_placement.clone();
        timing.mark("moves");
        let scaled_paths=scaled_targets.keys().cloned().collect::<BTreeSet<_>>();
        let moved_scaled=steps.iter().flatten()
            .any(|(path,_)|scaled_paths.contains(&path.to_lowercase()));
        // On the free-placement path, exact same-DPI positions may be ready
        // as soon as the move returns. Keep the delayed settle windows for
        // grid snapping and the deferred mixed-DPI recheck below.
        let delays: &[u64] = if moved_scaled {
            &[0, 10, 25, 45, 60, 80, 80]
        } else if self.snap_to_grid {
            &[25, 35, 50, 50]
        } else {
            &[0, 25, 35, 50, 50]
        };
        let mut batch_settled_immediately = false;
        let current = wait_until_settled(steps.is_empty(), delays, || {
            let from_batch = batch_observed.is_some();
            let items = batch_observed.take().unwrap_or_else(|| view.items());
            let ready = positions_settled(
                &items, &self.original, &placement, &nudged_placement, &scaled_paths,
            );
            if from_batch && ready {
                batch_settled_immediately = true;
            }
            (ready, items)
        });
        timing.mark("settle");
        let landed=|path:&str,dest:(i32,i32)|current.iter().any(|now|
            now.path.eq_ignore_ascii_case(path) && (now.pos==dest
                || (scaled_paths.contains(&path.to_lowercase())
                    && within_settle_tolerance(now.pos,dest))));
        let settled_cells=current.iter()
            .filter(|now|scaled_paths.contains(&now.path.to_lowercase()))
            .map(|now|now.pos).collect::<Vec<_>>();
        let collided=settled_cells.iter().enumerate()
            .any(|(n,a)|settled_cells[n+1..].contains(a));
        let all_placed = placement.iter().chain(nudged_placement.iter()).collect::<Vec<_>>();
        if current.len()!=self.original.len() || collided ||
            all_placed.iter().any(|&&(ref path,dest)|!landed(path,dest)) ||
            self.original.iter().filter(|icon|!all_placed.iter()
                .any(|&&(ref path,_)|path.eq_ignore_ascii_case(&icon.path)))
                .any(|icon|!current.iter().any(|now|
                    now.path.eq_ignore_ascii_case(&icon.path) && now.pos==icon.pos)){
            if view.spacing() != Some(cell) {
                self.expected = current;
                return Err("EXPLORER_SPACING_CHANGED".into());
            }
            if moved_scaled {
                // Explorer itself re-snapped icons on the scaled monitor. Those
                // are this generation's icons, not a user drag: restore from
                // where they are now rather than refusing the rollback.
                self.expected=current.clone();
            }
            let _=self.restore(view);
            return Err("Explorer real icon positions did not match bounded in-Box layout generation".into());
        }
        if batch_settled_immediately {
            self.native_batch_shell_verified = true;
        }
        // Keep where Explorer settled each icon as this generation's cell.
        self.moved=placement.iter().map(|(path,dest)|(path.clone(),current.iter()
            .find(|now|now.path.eq_ignore_ascii_case(path)).map_or(*dest,|now|now.pos)))
            .collect();
        self.nudged_loose=nudged_placement.iter().map(|(path,dest)|(path.clone(),current.iter()
            .find(|now|now.path.eq_ignore_ascii_case(path)).map_or(*dest,|now|now.pos)))
            .collect();
        self.expected=current.clone();
        if moved_scaled {
            self.pending_scaled_recheck_at = Some(
                std::time::Instant::now() + Duration::from_millis(SCALED_SETTLE_MS),
            );
            self.scaled_targets = scaled_targets;
        }
        if self.count_changes == 0 || !steps.is_empty() || loose_changed {
            if let Err(error)=self.persist_verified_generation(&current, state, false) {
                // This move was not durably acknowledged. A crash may require
                // manual recovery, but automated recovery cannot assume it.
                let _=self.restore(view);
                return Err(format!("Native position journal ACK not durable: {error}"));
            }
        }
        timing.mark("journal");
        self.pending_drop_pre_positions.clear();
        self.count_changes+=1;
        for ((group, _), plan) in member.selected_tabs.iter().zip(&plans) {
            self.set_page(*group, plan.page);
        }
        let native_order=native_original.iter().map(|prior|
            current.iter().find(|now|now.path.eq_ignore_ascii_case(&prior.path))
                .cloned().expect("native index identities already verified"))
            .collect::<Vec<_>>();
        Ok(LiveLayoutReport{
            hidden_indices,
            expected_native_order:native_order,
            page_status:member.selected_tabs.iter().zip(plans.iter())
                .map(|((gid,_),plan)|(*gid,plan.page,plan.page_count,plan.visible_rows)).collect(),
            visible_paths:visible_placement.iter().map(|(path,_)|path.clone()).collect(),
            observed_after:current,
            timing:format!("{} moves={}",timing.summary(),steps.iter().map(Vec::len).sum::<usize>()),
            fast_batch_applied:false,
        })
    }

    pub fn scaled_recheck_due_in(&self) -> Option<Duration> {
        self.pending_scaled_recheck_at
            .map(|due| due.saturating_duration_since(std::time::Instant::now()))
    }

    pub fn verify_delayed_scaled_settle(
        &mut self,
        view: &DesktopView,
        state: &State,
    ) -> Result<(), String> {
        let Some(due) = self.pending_scaled_recheck_at else {
            return Ok(());
        };
        if std::time::Instant::now() < due {
            return Ok(());
        }
        self.pending_scaled_recheck_at = None;
        let targets = std::mem::take(&mut self.scaled_targets);
        if self.restored || targets.is_empty() {
            return Ok(());
        }
        if host::signature(&host::monitors()) != self.initial_monitor_signature {
            return Err("Display topology changed during scaled native settle check".into());
        }
        let current = view.items();
        if current.len() != self.original.len() {
            let _ = self.restore(view);
            return Err("Explorer icon inventory changed during delayed scaled settle check".into());
        }
        let now_by_key = current.iter()
            .map(|item| (item.path.to_lowercase(), item.pos))
            .collect::<BTreeMap<_, _>>();
        if now_by_key.len() != current.len() {
            let _ = self.restore(view);
            return Err("Duplicate Explorer identities during delayed scaled settle check".into());
        }
        let placed_or_nudged = self.moved.iter().chain(self.nudged_loose.iter())
            .map(|(p, _)| p.to_lowercase())
            .collect::<BTreeSet<_>>();
        let scaled_cells = current.iter()
            .filter(|now| targets.contains_key(&now.path.to_lowercase()))
            .map(|now| now.pos)
            .collect::<Vec<_>>();
        let collided = scaled_cells.iter().enumerate()
            .any(|(n, a)| scaled_cells[n + 1..].contains(a));
        let ok = !collided
            && self.moved.iter().chain(self.nudged_loose.iter()).all(|(path, prior_pos)| {
                let key = path.to_lowercase();
                let Some(&live_pos) = now_by_key.get(&key) else { return false; };
                match targets.get(&key) {
                    Some(&target) => within_settle_tolerance(live_pos, target),
                    None => live_pos == *prior_pos,
                }
            })
            && self.original.iter()
                .filter(|i| !placed_or_nudged.contains(&i.path.to_lowercase()))
                .all(|i| now_by_key.get(&i.path.to_lowercase()) == Some(&i.pos));
        if !ok {
            self.expected = current;
            let _ = self.restore(view);
            return Err("Explorer delayed scaled icon positions drifted outside settle tolerance".into());
        }
        let mut changed = false;
        for (path, pos) in self.moved.iter_mut().chain(self.nudged_loose.iter_mut()) {
            if let Some(&live_pos) = now_by_key.get(&path.to_lowercase()) {
                if *pos != live_pos {
                    *pos = live_pos;
                    changed = true;
                }
            }
        }
        if changed {
            self.expected = current.clone();
            if let Err(error) = self.persist_verified_generation(&current, state, false) {
                let _ = self.restore(view);
                return Err(format!("Native scaled settle journal ACK failed: {error}"));
            }
        }
        Ok(())
    }
}
