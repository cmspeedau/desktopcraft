use super::*;

impl State {
    /// Make legacy page metadata safe without changing any page or group.
    pub(super) fn sync_page_birth(&mut self) {
        let distinct = self
            .page_birth
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        if self.page_birth.len() != self.pages.len() || distinct != self.pages.len() {
            self.page_birth = (0..self.pages.len() as u64).collect();
        }
        self.next_page_birth = self
            .page_birth
            .iter()
            .copied()
            .max()
            .map_or(0, |v| v.saturating_add(1))
            .max(self.next_page_birth);
    }
    /// Sort page tabs without moving files, and remap every box's page index.
    /// newest_first uses persistent creation IDs so repeated sorting is stable.
    pub fn sort_pages(&mut self, newest_first: bool) {
        self.sync_page_birth();
        let mut order: Vec<usize> = (0..self.pages.len()).collect();
        if newest_first {
            order.sort_by_key(|&i| std::cmp::Reverse(self.page_birth[i]));
        } else {
            order.sort_by(|&a, &b| {
                self.pages[a]
                    .to_lowercase()
                    .cmp(&self.pages[b].to_lowercase())
                    .then(a.cmp(&b))
            });
        }
        let mut destination = vec![0; order.len()];
        for (new_index, old_index) in order.iter().enumerate() {
            destination[*old_index] = new_index;
        }
        let current = self.page;
        self.pages = order.iter().map(|&i| self.pages[i].clone()).collect();
        self.page_birth = order.iter().map(|&i| self.page_birth[i]).collect();
        for group in &mut self.groups {
            group.page = destination[group.page];
        }
        self.page = destination[current];
    }
    /// Explicit, confirmed page creation. Navigation must never call this.
    pub fn create_page(&mut self, name: &str) -> Result<usize, &'static str> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err("Enter a page name");
        }
        if clean.chars().count() > 60 {
            return Err("Page name is too long (max 60 characters)");
        }
        self.sync_page_birth();
        let index = self.pages.len();
        self.pages.push(clean.to_owned());
        self.page_birth.push(self.next_page_birth);
        self.next_page_birth = self.next_page_birth.saturating_add(1);
        self.page = index;
        Ok(index)
    }
    /// Rename only the label; keep every group on its original page.
    pub fn rename_page(&mut self, index: usize, name: &str) -> Result<(), &'static str> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err("Enter a page name");
        }
        if clean.chars().count() > 60 {
            return Err("Page name is too long (max 60 characters)");
        }
        let entry = self.pages.get_mut(index).ok_or("Page not found")?;
        *entry = clean.to_owned();
        Ok(())
    }
    /// Delete only an empty page; never discard its groups or disk files.
    /// Renumber remaining groups and keep the active page in bounds.
    pub fn delete_empty_page(&mut self, index: usize) -> Result<(), &'static str> {
        if self.pages.len() <= 1 {
            return Err("At least one page must remain");
        }
        if index >= self.pages.len() {
            return Err("Page not found");
        }
        if self.groups.iter().any(|g| g.page == index) {
            return Err("Move all groups off this page before deleting it");
        }
        self.sync_page_birth();
        self.pages.remove(index);
        self.page_birth.remove(index);
        for g in &mut self.groups {
            if g.page > index {
                g.page -= 1;
            }
        }
        self.page = if self.page > index {
            self.page - 1
        } else if self.page == index {
            index.min(self.pages.len() - 1)
        } else {
            self.page
        };
        Ok(())
    }
}
