use super::*;


impl App {
    /// Regular Box -> Desktop unpins without changing disk files.
    /// Linked Folder View -> Desktop physically moves the file out of its folder.
    pub fn ungroup_to_desktop(&mut self, group: u64, item: u64, cursor: (i32, i32), time: f64) {
        let Some(source) = self
            .state
            .group(group)
            .and_then(|g| g.tab().items.iter().find(|i| i.id == item))
            .map(|i| i.path.clone())
        else {
            return;
        };
        let from_folder_view = self
            .state
            .group(group)
            .is_some_and(|g| g.tab().folder.is_some());
        let mut path = source.clone();
        if from_folder_view && !self.is_desktop_item(&source) {
            // Leaving a linked Folder View is a real move, just as leaving
            // that folder in Explorer. Virtual boxes never enter this branch.
            let Some(desktop) = self.host.as_ref().and_then(|h| h.folders.first()) else {
                self.notify("Windows Desktop folder is unavailable", time);
                return;
            };
            match shell::move_into_folder(Path::new(&source), desktop) {
                Ok(destination) => path = destination.to_string_lossy().to_string(),
                Err(e) => {
                    self.notify(e, time);
                    return;
                }
            }
            self.undo.clear(); // Disk I/O cannot be undone by layout undo.
            let original_parent = Path::new(&source).parent();
            for g in &mut self.state.groups {
                for tab in &mut g.tabs {
                    if tab
                        .folder
                        .as_deref()
                        .is_some_and(|f| Some(Path::new(f)) == original_parent)
                    {
                        let _ = tab.refresh();
                    }
                }
            }
            self.notify("File moved from linked folder to Desktop", time);
        } else {
            self.remember();
        }
        self.state.unassign(&source);
        if self.is_desktop_item(&path) {
            let (sx, sy) = self.host.as_ref().map_or((75, 98), |h| h.spacing);
            if let Some(view) = shell::DesktopView::acquire() {
                view.position(&[(path, (cursor.0 - sx / 2, cursor.1 - sy / 4))]);
            }
            if !from_folder_view {
                self.notify("Icon placed on Desktop (file unchanged)", time);
            }
        } else {
            self.notify("Pin removed; original file remains in its folder", time);
        }
        if let Some(h) = self.host.as_mut() {
            h.last_sync = -10.;
        }
        self.selected = None;
    }

    /// Ordinary boxes are virtual icon groups. A Folder View is the *only*
    /// destination that performs a physical Windows Shell move.
    /// Normal destinations change the pin assignment without touching disks.
    pub fn drop_path_into_box(&mut self, path: &str, target: u64, time: f64) -> Result<(), String> {
        let tab_id = self
            .state
            .group(target)
            .ok_or("Target box not found")?
            .tab()
            .id;
        self.drop_path_into_tab(path, target, tab_id, time)
    }
    /// A drop destination is an explicit tab ID, whether active or not.
    /// Regular tabs only change virtual pins. Linked folder views retain
    /// their existing deliberate Windows Shell move semantics.
    pub fn drop_path_into_tab(
        &mut self,
        path: &str,
        target: u64,
        tab_id: u64,
        time: f64,
    ) -> Result<(), String> {
        let tab = self
            .state
            .group(target)
            .and_then(|g| g.tabs.iter().find(|t| t.id == tab_id))
            .ok_or("Target tab not found")?;
        if let Some(folder) = tab.folder.clone() {
            let folder = PathBuf::from(folder);
            let source = Path::new(path);
            if source.parent() == Some(folder.as_path()) {
                return Ok(());
            }
            let destination = shell::move_into_folder(source, &folder).map_err(|e| {
                host::event(
                    "ERROR",
                    "drag.linked_folder_move_failed",
                    "disk file not reassigned",
                );
                e
            })?;
            self.undo.clear();
            self.state.unassign(path);
            for group in &mut self.state.groups {
                for tab in &mut group.tabs {
                    if tab.folder.as_deref().is_some_and(|f| {
                        Path::new(f) == folder || source.parent() == Some(Path::new(f))
                    }) {
                        let _ = tab.refresh();
                    }
                }
            }
            if let Some(h) = self.host.as_mut() {
                h.last_sync = -10.;
            }
            self.selected = None;
            self.notify(
                format!("Moved to linked folder: {}", destination.display()),
                time,
            );
            return Ok(());
        }
        if self.native_env.chrome_only
            && self.native_env.general_workspace
            && !self.automation
        {
            if self.native_tx_inflight
                || self.drag.is_some()
                || self.native_layout_restore_requested.is_some()
                || self.native_pin_paths_requested.is_some()
            {
                return Err(
                    "Explorer is still applying the previous change; please wait a moment.".into(),
                );
            }
            self.native_pin_paths_requested = Some((
                target,
                tab_id,
                vec![path.to_string()],
                "Pinned {count} Desktop item(s) to Box; files stay on Desktop".into(),
            ));
            self.native_tx_inflight = true;
            self.classic_file_menu = None;
            self.clear_native_scroll_state();
            return Ok(());
        }
        // Refuse stale paths before modifying any pin; deduplicate across
        // all normal tabs, including hidden tabs in combined boxes.
        let already = tab.items.iter().any(|i| i.path.eq_ignore_ascii_case(path));
        if already {
            return Ok(());
        }
        let id = self.state.id();
        let item = Item::from_path(Path::new(path), id).map_err(|e| e.to_string())?;
        self.remember();
        self.state.unassign(path);
        let show = self.state.prefs.switch_tab_after_drop;
        let group = self.state.group_mut(target).ok_or("Target box missing")?;
        let index = group
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .ok_or("Target tab missing")?;
        group.tabs[index].items.push(item);
        if show {
            group.active = index;
        }
        self.selected = Some((target, if show { id } else { 0 }));
        if let Some(h) = self.host.as_mut() {
            h.last_sync = -10.;
        }
        self.notify("File assigned to tab (file unchanged)", time);
        Ok(())
    }

    /// Explorer drag/drop: regular boxes pin; linked Folder Views move files.
    /// No virtual box ever creates a Documents directory or moves a file.
    pub fn drop_files(&mut self, paths: Vec<PathBuf>, time: f64, drop_at: Option<(i32, i32)>) {
        let (cx, cy) = drop_at.unwrap_or_else(host::cursor);
        let pos = self.from_screen(cx, cy);
        let target = self
            .state
            .groups
            .iter()
            .rev()
            .find(|g| g.page == self.state.page && g.bounds.contains(pos.x, pos.y))
            .map(|g| g.id);
        host::log(&format!(
            "explorer-drop: files={} target={target:?} screen=({cx},{cy})",
            paths.len()
        ));
        let Some(target) = target else {
            self.notify(
                "Drop onto a box to add a pin or into a Folder View to move the file",
                time,
            );
            return;
        };
        let target_tab = self
            .state
            .group(target)
            .and_then(|g| {
                g.drop_tab_at(pos.x, pos.y)
                    .and_then(|index| g.tabs.get(index))
                    .or_else(|| Some(g.tab()))
                    .map(|t| t.id)
            })
            .unwrap();
        let mut accepted = 0;
        let total = paths.len();
        let linked = self.state.group(target).is_some_and(|g| {
            g.tabs
                .iter()
                .any(|t| t.id == target_tab && t.folder.is_some())
        });
        if !linked
            && self.native_env.chrome_only
            && self.native_env.general_workspace
            && !self.automation
        {
            if self.native_tx_inflight
                || self.drag.is_some()
                || self.native_layout_restore_requested.is_some()
                || self.native_pin_paths_requested.is_some()
            {
                self.notify(
                    "Explorer is still applying the previous change; please wait a moment.",
                    time,
                );
                return;
            }
            let path_strings: Vec<String> = paths
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            self.native_pin_paths_requested = Some((
                target,
                target_tab,
                path_strings,
                "Pinned {count} Desktop item(s) to Box; files stay on Desktop".into(),
            ));
            self.native_tx_inflight = true;
            self.classic_file_menu = None;
            self.clear_native_scroll_state();
            return;
        }
        for path in paths {
            match self.drop_path_into_tab(&path.to_string_lossy(), target, target_tab, time) {
                Ok(()) => accepted += 1,
                Err(e) => self.notify(e, time),
            }
        }
        host::event(
            if accepted == total { "INFO" } else { "WARN" },
            "drag.explorer_drop_completed",
            &format!("target={target} linked={linked} accepted={accepted}"),
        );
        if accepted > 1 {
            self.notify(
                format!(
                    "{} {accepted} item(s)",
                    if linked {
                        "Moved into linked folder:"
                    } else {
                        "Pinned without moving:"
                    }
                ),
                time,
            );
        }
        if let Some(h) = self.host.as_mut() {
            h.last_sync = -10.;
        }
    }

    /// Shell actions that run their own modal loops (Explorer's menu, a Windows
    /// drag), started after the frame so drawing state is consistent.
    pub fn run_pending(&mut self, time: f64, _p: &Painter) {
        let Some(pending) = self.host.as_mut().and_then(|h| h.pending.take()) else {
            return;
        };
        match pending {
            Pending::Drag { path } => {
                // External app drops use OLE; internal Desktop/Box placement is
                // handled before reaching here. A cancelled OLE drag must NEVER
                // unassign its icon. A successful move removes a stale pin only
                // when the original path actually disappeared.
                let effect = shell::drag(&path);
                if effect == 0 {
                    self.notify("Drag cancelled", time);
                } else if !Path::new(&path).exists() {
                    self.remember();
                    self.state.unassign(&path);
                    self.selected = None;
                }
            }
        }
        if let Some(h) = self.host.as_mut() {
            h.last_sync = -10.;
        }
    }

    /// Rename a real item through Windows (undoable in Explorer).
    pub fn rename_item(&mut self, path: &str, name: &str, time: f64) -> bool {
        match shell::rename(path, name) {
            Ok(new) if new.exists() => {
                let new = new.to_string_lossy().to_string();
                for g in &mut self.state.groups {
                    for tab in &mut g.tabs {
                        for item in &mut tab.items {
                            if item.path.eq_ignore_ascii_case(path) {
                                item.path = new.clone();
                                item.name = name.to_string();
                            }
                        }
                    }
                }
                // A loose Desktop icon must remain selected after its path
                // changes, and Explorer must reconcile the renamed shell item.
                if let Some(h) = self.host.as_mut() {
                    if h.selected_loose
                        .as_deref()
                        .is_some_and(|p| p.eq_ignore_ascii_case(path))
                    {
                        h.selected_loose = Some(new);
                    }
                    h.last_sync = -10.;
                }
                true
            }
            Ok(_) => {
                self.notify("Windows did not confirm the new file name", time);
                false
            }
            Err(e) => {
                self.notify(e, time);
                false
            }
        }
    }

}
