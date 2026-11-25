use super::*;

impl App {
    pub(super) fn is_desktop_item(&self, path: &str) -> bool {
        self.host
            .as_ref()
            .is_some_and(|h| shell::on_desktop(path, &h.folders))
    }

    // ---------- the real desktop ----------
    pub(super) fn regular_tab_paths(&self) -> HashSet<String> {
        self.state.regular_tab_paths()
    }
}
