use super::*;

impl State {
    pub fn snapshot(&mut self, name: &str) {
        self.snapshots.push(Snapshot {
            name: name.into(),
            groups: self.groups.clone(),
            pages: self.pages.clone(),
            page_birth: self.page_birth.clone(),
            time: now_seconds(),
        });
    }
    pub fn restore(&mut self, i: usize) -> bool {
        if let Some(s) = self.snapshots.get(i).cloned() {
            self.groups = s.groups;
            self.pages = s.pages;
            self.page_birth = s.page_birth;
            self.sync_page_birth();
            self.page = self.page.min(self.pages.len() - 1);
            true
        } else {
            false
        }
    }
}
