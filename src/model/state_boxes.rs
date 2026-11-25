use super::*;

impl State {
    /// Add an empty virtual tab directly to an existing Box. Does not create
    /// a directory, move any file, or change stable IDs of existing tabs.
    pub fn add_tab(&mut self, group: u64) -> Result<u64, String> {
        let g = self.group(group).ok_or("Box no longer exists")?;
        if g.tabs.len() >= 128 {
            return Err("Too many tabs in this Box".into());
        }
        let mut number = 2usize;
        let name = loop {
            let candidate = format!("Tab {number}");
            if !g
                .tabs
                .iter()
                .any(|tab| tab.name.eq_ignore_ascii_case(&candidate))
            {
                break candidate;
            }
            number += 1;
        };
        let tab_id = self.id();
        let g = self.group_mut(group).unwrap();
        g.tabs.push(Tab::new(tab_id, &name));
        g.active = g.tabs.len() - 1;
        Ok(tab_id)
    }
    pub fn add_group(&mut self, name: &str, bounds: Bounds) -> u64 {
        let id = self.id();
        let tab_id = self.id();
        self.groups.push(Group {
            id,
            bounds,
            page: self.page,
            tabs: vec![Tab::new(tab_id, name)],
            active: 0,
            color: (id as usize) % 6,
            opacity: 0.78,
            rolled: false,
            dock: None,
            locked: false,
            keep_visible: false,
            custom_color: None,
        });
        id
    }
    /// Link an existing box to a real folder, without moving disk files.
    /// Refuse to discard a regular box's virtual pins implicitly.
    pub fn bind_group_folder(&mut self, group: u64, path: &Path) -> Result<(), String> {
        let box_ref = self.group_mut(group).ok_or("Box does not exist")?;
        let tab = box_ref.tab_mut();
        if tab.folder.is_none() && !tab.items.is_empty() {
            return Err("Move or unpin the box's existing icons before binding a folder.".into());
        }
        tab.navigate(&path.to_string_lossy())
    }
    pub fn add_portal(&mut self, path: &Path, bounds: Bounds) -> Result<u64, String> {
        if !path.is_dir() {
            return Err("Choose an existing folder.".into());
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let id = self.add_group(&name, bounds);
        if let Err(e) = self
            .group_mut(id)
            .unwrap()
            .tab_mut()
            .navigate(&path.to_string_lossy())
        {
            self.groups.retain(|g| g.id != id);
            return Err(e);
        }
        Ok(id)
    }
    /// Reorder tab headers without changing tab IDs or sorting-rule targets.
    pub fn reorder_tab(&mut self, group: u64, tab_id: u64, to: usize) -> bool {
        let Some(g) = self.group_mut(group) else {
            return false;
        };
        let Some(from) = g.tabs.iter().position(|tab| tab.id == tab_id) else {
            return false;
        };
        if to >= g.tabs.len() {
            return false;
        }
        if from == to {
            return true;
        }
        let active_id = g.tab().id;
        let tab = g.tabs.remove(from);
        g.tabs.insert(to, tab);
        g.active = g
            .tabs
            .iter()
            .position(|tab| tab.id == active_id)
            .unwrap_or(0);
        true
    }
    pub fn merge(&mut self, source: u64, target: u64) -> bool {
        if source == target || self.group(source).is_none() || self.group(target).is_none() {
            return false;
        }
        if self.group(source).unwrap().locked || self.group(target).unwrap().locked {
            return false;
        }
        let i = self.groups.iter().position(|g| g.id == source).unwrap();
        let g = self.groups.remove(i);
        // Rules keep following the same stable tab after groups are combined.
        let default_tab = g.tabs.iter().find(|t| t.folder.is_none()).map(|t| t.id);
        for rule in &mut self.rules {
            if rule.target == source {
                rule.target = target;
                if rule.target_tab.is_none() {
                    rule.target_tab = default_tab;
                }
            }
        }
        let dst = self.group_mut(target).unwrap();
        dst.tabs.extend(g.tabs);
        dst.active = dst.tabs.len() - 1;
        true
    }
    pub fn detach(&mut self, group: u64, tab: usize, bounds: Bounds) -> Option<u64> {
        if self.group(group)?.tabs.len() < 2 {
            return None;
        }
        let color = self.group(group)?.color;
        let g = self.group_mut(group)?;
        let tab = g.tabs.remove(tab);
        g.active = g.active.min(g.tabs.len() - 1);
        let id = self.id();
        for rule in &mut self.rules {
            if rule.target == group && rule.target_tab == Some(tab.id) {
                rule.target = id;
            }
        }
        self.groups.push(Group {
            id,
            bounds,
            page: self.page,
            tabs: vec![tab],
            active: 0,
            color,
            opacity: 0.78,
            rolled: false,
            dock: None,
            locked: false,
            keep_visible: false,
            custom_color: None,
        });
        Some(id)
    }
}
