use super::*;

impl LiveLayoutTransaction {
    pub fn restore(&mut self,view:&DesktopView)->Result<(),String>{
        if self.restored{return Ok(());}
        if host::signature(&host::monitors())!=self.initial_monitor_signature{
            return Err("Monitor disconnected or changed; never restore offscreen native icon positions automatically. Preserve journal for manual recovery".into());
        }
        self.pending_scaled_recheck_at = None;
        self.scaled_targets.clear();
        let current_live = view.items();
        self.absorb_pending_scaled_shifts(&current_live);
        let (displaced_parked, reconciled_live) = split_displaced_hidden_park(
            &self.expected,
            &current_live,
            &self.parked_paths,
            self.hidden_park,
            None,
        );
        reconcile_desktop_inventory(
            &mut self.original,
            &mut self.expected,
            &mut self.moved,
            &mut self.nudged_loose,
            &reconciled_live,
        )?;
        if !displaced_parked.is_empty() {
            let live_by_key = current_live
                .iter()
                .map(|i| (i.path.to_lowercase(), i.pos))
                .collect::<BTreeMap<_, _>>();
            let displaced_keys = displaced_parked
                .iter()
                .map(|p| p.to_lowercase())
                .collect::<BTreeSet<_>>();
            for icon in &mut self.expected {
                let key = icon.path.to_lowercase();
                if displaced_keys.contains(&key) {
                    if let Some(&pos) = live_by_key.get(&key) {
                        icon.pos = pos;
                    }
                }
            }
        }
        let pending_drop = std::mem::take(&mut self.pending_drop_pre_positions);
        if self.moved.is_empty() && self.nudged_loose.is_empty() && pending_drop.is_empty(){
            self.expected=self.original.clone();
            if !same_icon_positions(&current_live,&self.original){
                return Err("Native Explorer rollback positions differ; recovery journal preserved".into());
            }
        }else{
            let mut rollback = Vec::new();
            for (path, _) in &self.moved {
                let orig = self.original.iter().find(|i| i.path.eq_ignore_ascii_case(path))
                    .ok_or("Native original icon checkpoint incomplete; manual recovery required")?;
                rollback.push((path.clone(), orig.pos));
            }
            let moving_keys = self.moved.iter().chain(self.nudged_loose.iter())
                .map(|(p, _)| p.to_lowercase())
                .collect::<BTreeSet<_>>();
            for (path, _) in &self.nudged_loose {
                let orig = self.original.iter().find(|i| i.path.eq_ignore_ascii_case(path))
                    .ok_or("Native original icon checkpoint incomplete; manual recovery required")?;
                let occupied_by_user_loose = self.expected.iter().any(|item| {
                    !moving_keys.contains(&item.path.to_lowercase()) && item.pos == orig.pos
                });
                if occupied_by_user_loose {
                    if let Some(live_now) = self.expected.iter()
                        .find(|i| i.path.eq_ignore_ascii_case(path))
                        .map(|i| i.pos)
                    {
                        if let Some(o) = self.original.iter_mut()
                            .find(|i| i.path.eq_ignore_ascii_case(path))
                        {
                            o.pos = live_now;
                        }
                    }
                } else {
                    rollback.push((path.clone(), orig.pos));
                }
            }
            for (path, pre_pos) in pending_drop {
                let key = path.to_lowercase();
                if !moving_keys.contains(&key)
                    && self.original.iter().any(|i| i.path.eq_ignore_ascii_case(&path))
                {
                    rollback.push((path, pre_pos));
                }
            }
            let retained = retain_icons_with_occupied_restore_targets(&self.expected, &mut rollback);
            let mut restored_positions = self.original.clone();
            for icon in &mut restored_positions {
                if let Some(&position) = retained.get(&icon.path.to_lowercase()) {
                    icon.pos = position;
                }
            }
            let expected_after = rollback_targets_clear(&self.expected, &rollback)?;
            if !same_icon_positions(&expected_after, &restored_positions) {
                return Err("Native Explorer rollback plan differs; recovery journal preserved".into());
            }
            if !rollback.is_empty() {
                for batch in order_rollback_batches(&self.expected, &rollback) {
                    let moved = view.position(&batch);
                    if moved != batch.len() {
                        return Err("Native Explorer rollback API did not accept every original icon".into());
                    }
                }
                let settled = wait_until_settled(false, &[40, 60, 70, 70], || {
                    let items = view.items();
                    (same_icon_positions(&items, &restored_positions), items)
                });
                if !same_icon_positions(&settled, &restored_positions) {
                    return Err("Native Explorer rollback positions differ; recovery journal preserved".into());
                }
                std::thread::sleep(Duration::from_millis(200));
                if !same_icon_positions(&view.items(), &restored_positions) {
                    return Err("Native Explorer delayed rollback drift; recovery journal preserved".into());
                }
            }
            if !retained.is_empty() {
                host::event("INFO", "native_desktop.rollback_occupied_targets",
                    &format!("Kept {} icons at their current positions because their original positions were occupied", retained.len()));
            }
            self.original = restored_positions;
        }
        if self.original_snap_to_grid && !self.snap_to_grid {
            let _ = view.set_snap_to_grid(true);
            self.snap_to_grid = view.snap_to_grid().unwrap_or(true);
        }
        self.moved.clear();
        self.nudged_loose.clear();
        self.hidden_park=None;
        self.parked_paths.clear();
        self.pending_drop_pre_positions.clear();
        self.expected=self.original.clone();
        self.restored=true;
        std::fs::remove_file(&self.journal).map_err(|e|
            format!("Native rollback positions restored but cannot remove journal: {e}"))?;
        Ok(())
    }
    pub fn recovery_journal(&self)->&std::path::Path{&self.journal}
    pub fn layout_generation_count(&self)->usize{self.count_changes}
}
