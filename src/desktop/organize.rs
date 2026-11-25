use super::*;

const CATEGORIES: [(&str, &str); 6] = [
    ("Shortcuts", "lnk,url,appref-ms"),
    (
        "Documents",
        "pdf,doc,docx,txt,rtf,odt,xls,xlsx,csv,ppt,pptx,md",
    ),
    (
        "Pictures",
        "png,jpg,jpeg,gif,bmp,webp,heic,svg,ico,tif,tiff",
    ),
    ("Media", "mp3,wav,flac,m4a,mp4,mov,mkv,avi,wmv"),
    ("Archives & installers", "zip,rar,7z,exe,msi,iso"),
    ("Other", ""),
];


impl App {
    fn build_organize_desktop_target(&self) -> Option<State> {
        let view = self.primary_view();
        let mut buckets: Vec<(String, String, Vec<PathBuf>)> =
            vec![("Folders".into(), String::new(), vec![])];
        buckets.extend(
            CATEGORIES
                .iter()
                .map(|(n, e)| (n.to_string(), e.to_string(), vec![])),
        );
        let grouped = self.regular_tab_paths();
        for path in shell::desktop_entries() {
            if grouped.contains(&path.to_string_lossy().to_lowercase()) {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let i = if path.is_dir() {
                0
            } else {
                buckets
                    .iter()
                    .position(|(_, ext, _)| !ext.is_empty() && extension_matches(&name, ext))
                    .unwrap_or(buckets.len() - 1)
            };
            buckets[i].2.push(path);
        }
        let mut target = self.state.clone();
        let width = (3. * cell_w() + 2. * CONTENT_SIDE).max(260.);
        let mut x = view.x + view.w - width - 24.;
        let mut y = view.y + 24.;
        for (name, ext, paths) in buckets {
            if paths.is_empty() {
                continue;
            }
            let rows = paths.len().div_ceil(3).clamp(1, 4) as f32;
            let height = HEADER + CONTENT_TOP + rows * cell_h() + FOOTER;
            if y + height > view.y + view.h - 12. {
                y = view.y + 24.;
                x -= width + 20.;
            }
            let bx = x.max(view.x + 12.);
            let id = target.add_group(
                &name,
                Bounds {
                    x: bx,
                    y,
                    w: width,
                    h: height,
                },
            );
            for p in &paths {
                let _ = target.import(p, id);
            }
            if !ext.is_empty() && target.rules.len() < MAX_RULES {
                target.rules.push(Rule {
                    mode: 0,
                    pattern: ext,
                    target: id,
                    target_tab: None,
                    enabled: true,
                });
            }
            y += height + 20.;
        }
        target.auto_rules = false; // Rules only run during setup or explicit Apply/Organize.
        target.apply_rules();
        (target != self.state).then_some(target)
    }

    pub(crate) fn plan_native_organize_desktop(&self) -> Option<State> {
        self.build_organize_desktop_target()
    }

    pub(crate) fn request_organize_desktop(&mut self, time: f64) {
        if self.native_env.chrome_only && self.native_env.general_workspace && !self.automation {
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
            let Some(target) = self.plan_native_organize_desktop() else {
                self.notify("All desktop icons are already organized in Boxes.", time);
                return;
            };
            self.native_layout_restore_requested = Some(PendingNativeLayoutRestore {
                target,
                record_undo: true,
                undo_snapshot: Some(self.state.clone()),
                restore_undo_on_fail: None,
                success_toast: "Organized desktop into category groups (files remain on Desktop)"
                    .into(),
            });
            self.native_tx_inflight = true;
            self.classic_file_menu = None;
            self.clear_native_scroll_state();
            return;
        }
        self.auto_organize();
        self.notify(
            "Created desktop category groups (files remain on Desktop)",
            time,
        );
    }

    /// First run: offer to group the desktop by kind (the person can say no).
    pub fn auto_organize(&mut self) {
        if let Some(target) = self.build_organize_desktop_target() {
            self.remember();
            self.state = target;
        } else {
            self.state.auto_rules = false;
        }
        if let Some(h) = self.host.as_mut() {
            h.last_sync = -10.;
        }
    }

    /// Onboarding layout requested by user during install / first run:
    /// - Center Box: Tab 1 = Shortcuts (general desktop icons), Tab 2 = real Windows Downloads Portal in List view.
    /// - Right Drawer Box (`DockEdge::Right`, `rolled = true`): Tab 1 = Documents (Word/Excel/PDF/text), Tab 2 = real Windows Pictures Portal.
    pub fn apply_onboarding_layout(
        &mut self,
        downloads: Option<PathBuf>,
        pictures: Option<PathBuf>,
        desktop_files: Vec<PathBuf>,
    ) {
        let work = self.primary_view();
        self.state.groups.clear();
        self.state.rules.clear();
        self.state.page = 0;
        self.state.auto_rules = false;

        let right_w = 340.0f32.min((work.w * 0.28).max(260.0));
        let right_h = 400.0f32.min((work.h - 120.0).max(240.0));
        let right_x = (work.x + work.w - HEADER - right_w).max(work.x + 24.0);
        let right_y = work.y + ((work.h - right_h) * 0.5).max(32.0);
        let right_bounds = Bounds {
            x: right_x,
            y: right_y,
            w: right_w,
            h: right_h,
        };

        let max_center_right = (right_bounds.x - 24.0).max(work.x + 300.0);
        let center_w = 520.0f32.min((max_center_right - work.x - 40.0).max(280.0));
        let center_h = 360.0f32.min((work.h - 120.0).max(240.0));
        let ideal_center_x = work.x + (work.w - center_w) * 0.5;
        let center_x = ideal_center_x
            .min(max_center_right - center_w)
            .max(work.x + 24.0);
        let center_y = work.y + ((work.h - center_h) * 0.5).max(32.0);
        let center_bounds = Bounds {
            x: center_x,
            y: center_y,
            w: center_w,
            h: center_h,
        };

        let center_id = self.state.add_group("Shortcuts", center_bounds);
        let shortcuts_tab_id = self.state.group(center_id).unwrap().tabs[0].id;
        self.state.rules.push(Rule {
            mode: 0,
            pattern: "lnk,url,appref-ms".into(),
            target: center_id,
            target_tab: Some(shortcuts_tab_id),
            enabled: true,
        });
        if let Some(dl) = downloads {
            let dl_str = dl.to_string_lossy().into_owned();
            if self.state.add_tab(center_id).is_ok() {
                if let Some(g) = self.state.group_mut(center_id) {
                    if let Some(tab) = g.tabs.get_mut(1) {
                        tab.name = "Downloads".into();
                        tab.folder = Some(dl_str);
                        tab.list = true;
                        let _ = tab.refresh();
                    }
                    g.active = 0;
                }
            }
        }

        let right_id = self.state.add_group("Documents", right_bounds);
        let docs_pattern = "pdf,doc,docx,txt,rtf,odt,xls,xlsx,csv,ppt,pptx,md";
        let docs_tab_id = self.state.group(right_id).unwrap().tabs[0].id;
        self.state.rules.push(Rule {
            mode: 0,
            pattern: docs_pattern.into(),
            target: right_id,
            target_tab: Some(docs_tab_id),
            enabled: true,
        });
        if let Some(g) = self.state.group_mut(right_id) {
            g.color = 1;
            g.dock = Some(DockEdge::Right);
            g.rolled = false;
        }
        if let Some(pic) = pictures {
            let pic_str = pic.to_string_lossy().into_owned();
            if self.state.add_tab(right_id).is_ok() {
                if let Some(g) = self.state.group_mut(right_id) {
                    if let Some(tab) = g.tabs.get_mut(1) {
                        tab.name = "Pictures".into();
                        tab.folder = Some(pic_str);
                        tab.list = false;
                        let _ = tab.refresh();
                    }
                    g.active = 0;
                }
            }
        }

        for path in desktop_files {
            if is_recycle_bin_shell_item(&path.to_string_lossy()) {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let is_doc = !path.is_dir()
                && docs_pattern
                    .split(',')
                    .any(|p| p.trim().eq_ignore_ascii_case(&ext));
            let target_group = if is_doc { right_id } else { center_id };
            let _ = self.state.import(&path, target_group);
        }
        self.selected = Some((center_id, 0));
    }

    /// Custom starter layout when user chooses Custom boxes on install / first run.
    pub fn apply_custom_starter_layout(&mut self) {
        let work = self.primary_view();
        self.state.groups.clear();
        self.state.rules.clear();
        self.state.page = 0;
        self.state.auto_rules = false;
        let w = 440.0f32.min((work.w - 80.0).max(280.0));
        let h = 300.0f32.min((work.h - 120.0).max(220.0));
        let bounds = Bounds {
            x: work.x + ((work.w - w) * 0.5).max(24.0),
            y: work.y + ((work.h - h) * 0.5).max(32.0),
            w,
            h,
        };
        let id = self.state.add_group("Shortcuts", bounds);
        self.selected = Some((id, 0));
    }

}
