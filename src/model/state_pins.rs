use super::*;

impl State {
    /// Atomic virtual pin paste into a Box. Never moves/copies disk objects.
    /// A real file may belong to one regular Tab at a time; preserve existing
    /// pin IDs when reassigning, and prepare all items before mutating state.
    pub fn pin_clipboard_paths(&mut self, group: u64, paths: &[PathBuf]) -> Result<usize, String> {
        let destination = self.group(group).ok_or("Box not found")?;
        if destination.tab().folder.is_some() {
            return Err("Use a folder paste for a linked Folder View".into());
        }
        if paths.is_empty() {
            return Err("No files to pin".into());
        }
        if paths.len() > 4096 {
            return Err("Too many files selected".into());
        }
        let dest_id = destination.tab().id;
        let mut unique = std::collections::HashSet::new();
        let mut staged = Vec::new();
        for path in paths {
            if !path.exists() {
                return Err(format!("File no longer exists: {}", path.display()));
            }
            let key = path.to_string_lossy().to_lowercase();
            if !unique.insert(key) {
                continue;
            }
            let old = self
                .groups
                .iter()
                .flat_map(|g| &g.tabs)
                .filter(|tab| tab.folder.is_none())
                .flat_map(|tab| &tab.items)
                .find(|item| item.path.eq_ignore_ascii_case(&path.to_string_lossy()))
                .cloned();
            let fresh = Item::from_path(path, 0).map_err(|e| e.to_string())?;
            // Do not overwrite an explicit manually assigned display label,
            // and retain a stable pin id for files previously in another Box.
            let item = old.unwrap_or(fresh);
            staged.push(item);
        }
        if staged.is_empty() {
            return Ok(0);
        }
        let dst_keys: std::collections::HashSet<_> = self
            .group(group)
            .unwrap()
            .tab()
            .items
            .iter()
            .map(|i| i.path.to_lowercase())
            .collect();
        let mut moves = Vec::new();
        for mut item in staged {
            if dst_keys.contains(&item.path.to_lowercase()) {
                continue;
            }
            if item.id == 0 {
                item.id = self.id();
            }
            moves.push(item);
        }
        if moves.is_empty() {
            return Ok(0);
        }
        let keys: std::collections::HashSet<String> =
            moves.iter().map(|i| i.path.to_lowercase()).collect();
        for g in &mut self.groups {
            for tab in &mut g.tabs {
                if tab.folder.is_none() {
                    tab.items.retain(|i| !keys.contains(&i.path.to_lowercase()));
                }
            }
        }
        let total = moves.len();
        let dest = self
            .group_mut(group)
            .and_then(|g| g.tabs.iter_mut().find(|t| t.id == dest_id))
            .ok_or("Destination Tab disappeared")?;
        dest.items.extend(moves);
        Ok(total)
    }
    pub fn import(&mut self, path: &Path, target: u64) -> Result<(), String> {
        let id = self.id();
        let item = Item::from_path(path, id).map_err(|e| e.to_string())?;
        let g = self.group_mut(target).ok_or("Choose a group first.")?;
        if g.tab().folder.is_some() {
            return Err(
                "Folder views mirror their folder. Add this item to a regular group instead."
                    .into(),
            );
        }
        if !g.tab().items.iter().any(|x| x.path == item.path) {
            g.tab_mut().items.push(item)
        }
        Ok(())
    }
    pub fn transfer(
        &mut self,
        source: u64,
        item_id: u64,
        target: u64,
        index: usize,
    ) -> Result<(), String> {
        let src_tab = self.group(source).ok_or("Source box missing")?.tab().id;
        let dst_tab = self
            .group(target)
            .ok_or("Destination box missing")?
            .tab()
            .id;
        self.transfer_between_tabs(source, src_tab, item_id, target, dst_tab, index)
    }
    /// Move a virtual pin between identified tabs, including two tabs in one box.
    /// No filesystem rename or move is performed.
    /// Move selected virtual pins as one validated operation. Linked Folder
    /// Views cannot be batch targets because they represent disk folders.
    pub fn transfer_many_between_tabs(
        &mut self,
        source: u64,
        src_tab: u64,
        ids: &[u64],
        target: u64,
        dst_tab: u64,
    ) -> Result<usize, String> {
        if ids.is_empty() {
            return Err("No pins selected".into());
        }
        if source == target && src_tab == dst_tab {
            return Ok(0);
        }
        let source_items = self
            .group(source)
            .and_then(|g| {
                g.tabs
                    .iter()
                    .find(|t| t.id == src_tab && t.folder.is_none())
            })
            .ok_or("Source must be a regular tab")?;
        let destination = self
            .group(target)
            .and_then(|g| {
                g.tabs
                    .iter()
                    .find(|t| t.id == dst_tab && t.folder.is_none())
            })
            .ok_or("Destination must be a regular tab")?;
        let distinct: std::collections::HashSet<_> = ids.iter().copied().collect();
        if distinct.len() != ids.len() {
            return Err("Duplicate selected pin IDs".into());
        }
        let ordered: Vec<_> = source_items
            .items
            .iter()
            .filter(|item| distinct.contains(&item.id))
            .cloned()
            .collect();
        if ordered.len() != ids.len() {
            return Err("Some pins are no longer in the source tab".into());
        }
        if ordered.iter().any(|item| {
            destination
                .items
                .iter()
                .any(|existing| existing.path.eq_ignore_ascii_case(&item.path))
        }) {
            return Err("A selected pin is already in the destination tab".into());
        }
        self.group_mut(source)
            .unwrap()
            .tabs
            .iter_mut()
            .find(|t| t.id == src_tab)
            .unwrap()
            .items
            .retain(|item| !distinct.contains(&item.id));
        let paths: std::collections::HashSet<_> = ordered
            .iter()
            .map(|item| item.path.to_lowercase())
            .collect();
        for group in &mut self.groups {
            for tab in &mut group.tabs {
                if tab.folder.is_none() && !(group.id == target && tab.id == dst_tab) {
                    tab.items
                        .retain(|pin| !paths.contains(&pin.path.to_lowercase()));
                }
            }
        }
        let count = ordered.len();
        self.group_mut(target)
            .unwrap()
            .tabs
            .iter_mut()
            .find(|t| t.id == dst_tab)
            .unwrap()
            .items
            .extend(ordered);
        Ok(count)
    }
    pub fn transfer_between_tabs(
        &mut self,
        source: u64,
        src_tab: u64,
        item_id: u64,
        target: u64,
        dst_tab: u64,
        index: usize,
    ) -> Result<(), String> {
        if source == target && src_tab == dst_tab {
            return Ok(());
        }
        let find_tab = |gid, tid| {
            self.group(gid)
                .and_then(|g| g.tabs.iter().find(|t| t.id == tid))
        };
        let src = find_tab(source, src_tab).ok_or("Source tab missing")?;
        if src.folder.is_some() {
            return Err("Linked folders retain their on-disk files".into());
        }
        let position = src
            .items
            .iter()
            .position(|item| item.id == item_id)
            .ok_or("Item missing")?;
        let dest = find_tab(target, dst_tab).ok_or("Destination tab missing")?;
        if dest.folder.is_some() {
            return Err("Drop into a linked folder to move its file on disk".into());
        }
        let item = self
            .group_mut(source)
            .unwrap()
            .tabs
            .iter_mut()
            .find(|t| t.id == src_tab)
            .unwrap()
            .items
            .remove(position);
        for group in &mut self.groups {
            for tab in &mut group.tabs {
                if tab.folder.is_none() {
                    tab.items
                        .retain(|pin| !pin.path.eq_ignore_ascii_case(&item.path));
                }
            }
        }
        let dest = self
            .group_mut(target)
            .unwrap()
            .tabs
            .iter_mut()
            .find(|t| t.id == dst_tab)
            .unwrap();
        dest.items.insert(index.min(dest.items.len()), item);
        Ok(())
    }
}
