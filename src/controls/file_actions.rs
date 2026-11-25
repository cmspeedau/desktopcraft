use super::*;

/// Direct file operations shared by shortcuts and classic Shell menu commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileAction {
    Copy,
    Cut,
    #[allow(dead_code)]
    PasteInto,
    Rename,
    Delete,
    Properties,
    Reveal,
}

impl App {
    /// Windows-backed actions on custom-drawn file icons. Unlike the classic
    /// Shell IContextMenu popup this menu works without an Explorer HWND owning
    /// the icon and explicitly routes each action to the Windows API.
    /// Ctrl+V or Box > Paste. Resolve intent from the current surface,
    /// never a stale fallback Box selection.
    pub(crate) fn paste_file_clipboard(&mut self, group: Option<u64>, time: f64) {
        if self.automation {
            self.notify(
                "File clipboard is only available in a Windows session",
                time,
            );
            return;
        }
        match shell::read_file_clipboard() {
            Ok(Some(files)) => self.paste_files(group, files, time),
            Ok(None) => self.notify("Clipboard has no files or folders", time),
            Err(e) => self.notify(e, time),
        }
    }
    /// Regular Box -> virtual pins; linked Folder View or bare Desktop ->
    /// actual Windows disk operation. A Cut must never silently only pin.
    pub(crate) fn paste_files(
        &mut self,
        group: Option<u64>,
        files: shell::FileClipboard,
        time: f64,
    ) {
        if files.paths.is_empty() {
            self.notify("Clipboard has no files", time);
            return;
        }
        let linked = group.and_then(|id| self.state.group(id).map(|g| g.tab().folder.clone()));
        match linked {
            Some(None) => {
                if files.cut {
                    self.notify("Cut files cannot move into a virtual Box. Copy them instead, or paste into a linked Folder View.",time);
                    return;
                }
                let id = group.unwrap();
                if self.native_env.chrome_only
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
                    let Some(tab_id) = self.state.group(id).map(|g| g.tab().id) else {
                        return;
                    };
                    let paths: Vec<String> = files
                        .paths
                        .iter()
                        .map(|p| p.to_string_lossy().into_owned())
                        .collect();
                    self.native_pin_paths_requested = Some((
                        id,
                        tab_id,
                        paths,
                        "Pinned {count} Desktop item(s) to this Box; files stay on Desktop"
                            .to_string(),
                    ));
                    self.native_tx_inflight = true;
                    self.classic_file_menu = None;
                    self.clear_native_scroll_state();
                    return;
                }
                let mut staged = self.state.clone();
                match staged.pin_clipboard_paths(id, &files.paths) {
                    Ok(count) if count > 0 => {
                        self.remember();
                        self.state = staged;
                        self.selected = Some((id, 0));
                        self.multi_selected.clear();
                        if let Some(h) = self.host.as_mut() {
                            h.last_sync = -10.;
                        }
                        self.notify(
                            format!("Pinned {count} file(s) to this Box; files stay on disk"),
                            time,
                        );
                    }
                    Ok(_) => self.notify("Files are already pinned in this Box", time),
                    Err(e) => self.notify(e, time),
                }
            }
            Some(Some(folder)) => {
                if self.automation {
                    return;
                }
                match shell::paste_files_into_folder(&files, &PathBuf::from(&folder)) {
                    Ok(count) => {
                        if files.cut {
                            for path in &files.paths {
                                if !path.exists() {
                                    self.state.unassign(&path.to_string_lossy());
                                }
                            }
                        }
                        if let Some(g) = self.state.group_mut(group.unwrap()) {
                            if let Err(e) = g.tab_mut().refresh() {
                                self.notify(e, time);
                            }
                        }
                        if let Some(h) = self.host.as_mut() {
                            h.last_sync = -10.;
                        }
                        self.notify(
                            format!(
                                "{} {count} file(s) in linked folder",
                                if files.cut { "Moved" } else { "Copied" }
                            ),
                            time,
                        );
                    }
                    Err(e) => self.notify(e, time),
                }
            }
            None if group.is_some() => self.notify("Selected Box no longer exists", time),
            None => {
                if self.host.is_none() {
                    self.notify("Select a Box before pasting files", time);
                    return;
                }
                let folder = shell::desktop_folders().into_iter().next();
                if let Some(folder) = folder {
                    if self.automation {
                        return;
                    }
                    match shell::paste_files_into_folder(&files, &folder) {
                        Ok(count) => {
                            if files.cut {
                                for path in &files.paths {
                                    if !path.exists() {
                                        self.state.unassign(&path.to_string_lossy());
                                    }
                                }
                            }
                            if let Some(h) = self.host.as_mut() {
                                h.last_sync = -10.;
                            }
                            self.notify(
                                format!(
                                    "{} {count} file(s) onto Desktop",
                                    if files.cut { "Moved" } else { "Copied" }
                                ),
                                time,
                            );
                        }
                        Err(e) => self.notify(e, time),
                    }
                } else {
                    self.notify("Windows Desktop folder is unavailable", time);
                }
            }
        }
    }
    pub(super) fn file_action(&mut self, path: &str, action: FileAction, time: f64) {
        // Shell-only operations on virtual namespace items are unsafe. A
        // non-filesystem icon is not a real disk file.
        if !Path::new(path).exists() {
            self.notify("This item is not a regular file or folder", time);
            return;
        }
        let box_target = self.selected.and_then(|(gid, iid)| {
            self.state.group(gid).and_then(|g| {
                g.tab()
                    .items
                    .iter()
                    .any(|it| it.id == iid && it.path.eq_ignore_ascii_case(path))
                    .then_some((gid, iid))
            })
        });
        match action {
            FileAction::PasteInto => {
                if !Path::new(path).is_dir() {
                    self.notify("Paste requires a real folder", time);
                    return;
                }
                if self.automation {
                    return;
                }
                match shell::read_file_clipboard() {
                    Ok(Some(files)) => {
                        match shell::paste_files_into_folder(&files, Path::new(path)) {
                            Ok(n) => {
                                if files.cut {
                                    for item in &files.paths {
                                        if !item.exists() {
                                            self.state.unassign(&item.to_string_lossy());
                                        }
                                    }
                                }
                                if let Some(h) = self.host.as_mut() {
                                    h.last_sync = -10.;
                                }
                                self.notify(format!("Pasted {n} file(s) into folder"), time);
                            }
                            Err(e) => self.notify(e, time),
                        }
                    }
                    Ok(None) => self.notify("Clipboard has no files", time),
                    Err(e) => self.notify(e, time),
                }
            }
            FileAction::Rename => {
                if let Some((gid, iid)) = box_target {
                    self.start_inline_rename(gid, iid);
                } else {
                    self.start_inline_rename_loose(path);
                }
            }
            FileAction::Copy | FileAction::Cut => {
                let paths: Vec<String> = box_target
                    .and_then(|(gid, _)| {
                        self.state.group(gid).map(|g| {
                            let tab_id = g.tab().id;
                            self.multi_selected
                                .iter()
                                .filter(|(g, t, _)| *g == gid && *t == tab_id)
                                .filter_map(|(_, _, iid)| {
                                    g.tab()
                                        .items
                                        .iter()
                                        .find(|i| i.id == *iid)
                                        .map(|i| i.path.clone())
                                })
                                .filter(|p| Path::new(p).exists())
                                .collect()
                        })
                    })
                    .filter(|items: &Vec<String>| items.len() > 1)
                    .or_else(|| {
                        self.host.as_ref().and_then(|h| {
                            let key = path.to_lowercase();
                            if !h.marquee_selected.contains(&key) {
                                return None;
                            }
                            let paths: Vec<String> = h
                                .loose
                                .iter()
                                .filter(|i| h.marquee_selected.contains(&i.key))
                                .map(|i| i.path.clone())
                                .filter(|p| Path::new(p).exists())
                                .collect();
                            (paths.len() > 1).then_some(paths)
                        })
                    })
                    .unwrap_or_else(|| vec![path.to_string()]);
                if !self.automation {
                    match shell::copy_files_to_clipboard(&paths, action == FileAction::Cut) {
                        Ok(()) => self.notify(
                            format!(
                                "{} {} file(s) to Windows clipboard",
                                if action == FileAction::Cut {
                                    "Cut"
                                } else {
                                    "Copied"
                                },
                                paths.len()
                            ),
                            time,
                        ),
                        Err(e) => self.notify(e, time),
                    }
                }
            }
            FileAction::Delete => {
                if !self.automation {
                    match shell::recycle(path) {
                        Ok(()) => {
                            if !Path::new(path).exists() {
                                self.state.unassign(path);
                                self.selected = None;
                                if let Some(h) = self.host.as_mut() {
                                    h.selected_loose = None;
                                    h.last_sync = -10.;
                                }
                            }
                        }
                        Err(e) => self.notify(e, time),
                    }
                }
            }
            FileAction::Properties => {
                self.properties_requested.push(path.to_string());
                if !self.automation {
                    if let Err(e) = shell::properties(path) {
                        self.notify(e, time);
                    }
                }
            }
            FileAction::Reveal => {
                if !self.automation {
                    if let Err(e) = shell::reveal_in_explorer(path) {
                        self.notify(e, time);
                    }
                }
            }
        }
    }
}
