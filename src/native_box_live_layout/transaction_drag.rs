use super::*;

impl LiveLayoutTransaction {
    /// Read-only pointer-free baseline for actual Explorer position changes.
    /// This is the expected physical layout AFTER the last ACK, not the
    /// original checkpoint; the original remains immutable for rollback.
    pub fn expected_icons(&self)->&[ViewItem]{&self.expected}
    pub(crate) fn original_icons(&self)->&[ViewItem]{&self.original}

    /// Explorer has already performed a genuine one-icon desktop drag.
    /// After checking the exact one-icon move and native monitor identity,
    /// undo ONLY that physical move to the last acknowledged icon cell.
    /// Then the standard ACK-backed reflow can safely place the icon under
    /// its newly authorized virtual Box membership. Do not hide, rename,
    /// delete or copy the file itself. If this restore fails, keep the
    /// recovery journal and detach/fail-open.
    pub fn restore_one_verified_native_drag(
        &self,view:&DesktopView,
        evidence:&crate::native_pin_drop::NativePhysicalDrop,
    )->Result<(),String>{
        crate::native_pin_drop::physical_drop_identity(evidence)?;
        if host::signature(&host::monitors())!=self.initial_monitor_signature{
            return Err("Display geometry changed during Explorer physical drag".into());
        }
        if !same_icon_positions(&evidence.before,&self.expected)
            || !same_icon_positions(&view.items(),&evidence.after){
            return Err("Native Explorer moved another item after gesture evidence was captured".into());
        }
        let expected=self.expected.iter().find(|icon|
            icon.path.eq_ignore_ascii_case(&evidence.source_path))
            .ok_or("Native recovery baseline has no source item")?;
        if view.position(&[(expected.path.clone(),expected.pos)])!=1{
            return Err("Could not restore physically dragged icon to its acknowledged cell".into());
        }
        std::thread::sleep(Duration::from_millis(120));
        if !same_icon_positions(&view.items(),&self.expected){
            return Err("Explorer native icon source cell was not restored after drag".into());
        }
        Ok(())
    }

    pub fn restore_verified_multi_drag(
        &self,
        view: &DesktopView,
        before: &[ViewItem],
        after: &[ViewItem],
        moved_paths: &[String],
    ) -> Result<(), String> {
        if moved_paths.is_empty() {
            return Err("Native multi-drag restore requires at least one moved path".into());
        }
        if host::signature(&host::monitors()) != self.initial_monitor_signature {
            return Err("Display geometry changed during Explorer physical drag".into());
        }
        if !same_icon_positions(before, &self.expected)
            || !same_icon_positions(&view.items(), after)
        {
            return Err("Native Explorer moved another item after gesture evidence was captured".into());
        }
        let mut moves = Vec::with_capacity(moved_paths.len());
        for path in moved_paths {
            let expected = self.expected.iter()
                .find(|icon| icon.path.eq_ignore_ascii_case(path))
                .ok_or("Native recovery baseline has no source item")?;
            moves.push((expected.path.clone(), expected.pos));
        }
        if view.position(&moves) != moves.len() {
            return Err("Could not restore physically dragged icons to their acknowledged cells".into());
        }
        let settled = wait_until_settled(false, &[0, 25, 35, 50, 50], || {
            let items = view.items();
            (same_icon_positions(&items, &self.expected), items)
        });
        if !same_icon_positions(&settled, &self.expected) {
            return Err("Explorer native icon source cells were not restored after drag".into());
        }
        Ok(())
    }

    /// Record pre-drop coordinates for verified `DesktopToBox` / `BoxToBox` /
    /// `BoxToDesktop` icons and adopt their dropped `observed` coordinates in
    /// `expected` (never mutating `original`), avoiding a visible bounce-back
    /// before the direct reflow places them into their target cells.
    pub(crate) fn accept_inflight_drop_positions(
        &mut self,
        observed: &[ViewItem],
        source_paths: &[String],
    ) -> Result<(), String> {
        if source_paths.is_empty() {
            return Err("In-flight native drop requires at least one source path".into());
        }
        let observed_by_key = observed
            .iter()
            .map(|item| (item.path.to_lowercase(), item))
            .collect::<BTreeMap<_, _>>();
        let mut updates = Vec::with_capacity(source_paths.len());
        let mut seen = BTreeSet::new();
        for path in source_paths {
            let key = path.to_lowercase();
            if key.trim().is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            let live = observed_by_key
                .get(&key)
                .ok_or("Dropped native icon disappeared from Explorer observation")?;
            let exp = self
                .expected
                .iter()
                .find(|item| item.path.eq_ignore_ascii_case(path))
                .ok_or("Native recovery baseline has no source item")?;
            if exp.label != live.label {
                return Err("Explorer icon label changed during native drop".into());
            }
            updates.push((exp.path.clone(), exp.pos, live.pos));
        }
        if !self.pending_drop_pre_positions.is_empty() {
            return Err("Previous in-flight native drop positions were not settled".into());
        }
        self.pending_drop_pre_positions = updates
            .iter()
            .map(|(path, pre_pos, _)| (path.clone(), *pre_pos))
            .collect();
        for (path, _, live_pos) in updates {
            if let Some(exp) = self
                .expected
                .iter_mut()
                .find(|item| item.path.eq_ignore_ascii_case(&path))
            {
                exp.pos = live_pos;
            }
            for (moved_path, moved_pos) in &mut self.moved {
                if moved_path.eq_ignore_ascii_case(&path) {
                    *moved_pos = live_pos;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn revert_inflight_drop_positions(&mut self, view: &DesktopView) -> Result<(), String> {
        let prior = std::mem::take(&mut self.pending_drop_pre_positions);
        if prior.is_empty() {
            return Ok(());
        }
        if view.position(&prior) != prior.len() {
            return Err("Could not revert in-flight native drop icons".into());
        }
        let targets = prior
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
            return Err("Explorer did not settle reverted in-flight native drop icons".into());
        }
        for (path, pre_pos) in prior {
            if let Some(exp) = self
                .expected
                .iter_mut()
                .find(|item| item.path.eq_ignore_ascii_case(&path))
            {
                exp.pos = pre_pos;
            }
            for (moved_path, moved_pos) in &mut self.moved {
                if moved_path.eq_ignore_ascii_case(&path) {
                    *moved_pos = pre_pos;
                }
            }
        }
        Ok(())
    }
}
