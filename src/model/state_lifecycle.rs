use super::*;

impl State {
    /// Desktop mode's first start: no groups yet (the first-run prompt offers to
    /// organize the desktop).
    pub fn empty() -> Self {
        Self {
            version: 1,
            groups: vec![],
            pages: vec!["Desktop".into()],
            page_birth: vec![0],
            next_page_birth: 1,
            page: 0,
            next_id: 1,
            rules: vec![],
            snapshots: vec![],
            blend: false,
            tint: false,
            snap: true,
            quick_hide: true,
            auto_rules: false,
            wallpaper: 0,
            layouts: Default::default(),
            placements: Default::default(),
            explorer_icons_hidden: false,
            prefs: Default::default(),
        }
    }
    /// Take an item (by path) out of every regular group.
    pub fn unassign(&mut self, path: &str) {
        for g in &mut self.groups {
            for tab in &mut g.tabs {
                if tab.folder.is_none() {
                    tab.items.retain(|i| !i.path.eq_ignore_ascii_case(path));
                }
            }
        }
    }
    pub fn demo(root: &Path) -> Self {
        let mut s = Self {
            version: 1,
            groups: vec![],
            pages: vec!["Everyday".into(), "Focus".into()],
            page_birth: vec![0, 1],
            next_page_birth: 2,
            page: 0,
            next_id: 1,
            rules: vec![],
            snapshots: vec![],
            blend: false,
            tint: false,
            snap: true,
            quick_hide: true,
            auto_rules: false,
            wallpaper: 0,
            layouts: Default::default(),
            placements: Default::default(),
            explorer_icons_hidden: false,
            prefs: Default::default(),
        };
        let projects = s.add_group(
            "Projects",
            Bounds {
                x: 52.,
                y: 125.,
                w: 376.,
                h: 294.,
            },
        );
        let daily = s.add_group(
            "Quick access",
            Bounds {
                x: 456.,
                y: 125.,
                w: 332.,
                h: 294.,
            },
        );
        let _ = s.add_portal(
            &root.join("demo/Library"),
            Bounds {
                x: 816.,
                y: 125.,
                w: 410.,
                h: 410.,
            },
        );
        let notes = s.add_group(
            "Notes & ideas",
            Bounds {
                x: 52.,
                y: 451.,
                w: 376.,
                h: 220.,
            },
        );
        for (target, paths) in [
            (
                projects,
                vec![
                    "demo/Projects/Studio",
                    "demo/Projects/Website",
                    "demo/Projects/Research",
                    "demo/Projects/Project brief.txt",
                    "demo/Projects/Timeline.csv",
                    "demo/Projects/Checklist.txt",
                ],
            ),
            (
                daily,
                vec![
                    "demo/Library",
                    "demo/Projects",
                    "demo/Notes",
                    "demo/Welcome.html",
                ],
            ),
            (
                notes,
                vec![
                    "demo/Notes/Ideas.txt",
                    "demo/Notes/Reading list.txt",
                    "demo/Notes/This week.txt",
                ],
            ),
        ] {
            for path in paths {
                let _ = s.import(&root.join(path), target);
            }
        }
        for (i, g) in s.groups.iter_mut().enumerate() {
            g.color = [0, 1, 2, 3][i];
        }
        s.snapshot("Starting layout");
        s
    }
}
