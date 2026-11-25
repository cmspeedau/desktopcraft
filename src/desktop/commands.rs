use super::*;


impl App {
    pub(crate) fn native_chrome_shortcuts(&mut self, input: &Input) -> bool {
        if self.filter_typing(input) {
            return false;
        }
        if input.key(KeyCode::Escape) {
            if matches!(self.dialog, Some(Dialog::EditRule { .. })) {
                self.dialog = Some(Dialog::Settings {
                    page: 2,
                    pattern: String::new(),
                    mode: 0,
                    target: 0,
                });
                self.settings_bring_to_front = true;
            } else if self.has_blocking_dialog() {
                self.dialog = None;
                self.settings_drag_offset = None;
                self.shortcut_recording = false;
                if let Some(ret) = self.settings_return.take() {
                    self.dialog = Some(ret);
                    self.settings_bring_to_front = true;
                }
            } else if self.dialog.is_some() && !self.settings_in_own_window() {
                self.dialog = None;
                self.settings_drag_offset = None;
                self.shortcut_recording = false;
            } else if self.menu.is_some() || self.folder_sort_menu.is_some() {
                self.close_popups();
            } else if self.peek {
                self.set_peek(false);
            }
            self.drag = None;
            return false;
        }
        if !self.native_env.general_workspace {
            return false;
        }
        #[cfg(windows)]
        let settings_blocks_hotkeys = self.settings_window_active_on_page(2);
        #[cfg(not(windows))]
        let settings_blocks_hotkeys = false;
        if self.state.prefs.pages
            && self.state.prefs.desktop_enabled
            && !self.has_blocking_dialog()
            && !settings_blocks_hotkeys
            && self.menu.is_none()
            && self.folder_sort_menu.is_none()
            && self.drag.is_none()
            && !self.peek
            && !self.shortcut_recording
            && !self.native_tx_inflight
        {
            if self.host.is_some() && !self.automation {
                match platform::page_hotkey() {
                    1 => self.switch_page(1),
                    -1 => self.switch_page(-1),
                    _ => {}
                }
            } else if (input.ctrl || input.alt) && !input.shift {
                if input.key(KeyCode::Right) {
                    self.switch_page(1);
                }
                if input.key(KeyCode::Left) {
                    self.switch_page(-1);
                }
            }
        }
        let undo_pressed = if self.host.is_some() && !self.automation {
            platform::undo_hotkey()
        } else {
            input.ctrl && !input.alt && !input.shift && input.key(KeyCode::Z)
        };
        if self.state.prefs.desktop_enabled && undo_pressed && self.native_undo_shortcut_allowed() {
            let before_tx = self.native_tx_inflight;
            self.request_undo(input.time);
            return !before_tx && self.native_tx_inflight;
        }
        false
    }
    pub(crate) fn run_command(&mut self, command: &str, time: f64) {
        let is_native = self.native_env.chrome_only
            && self.native_env.general_workspace
            && !self.automation;
        let workspace_command = command == "new-group"
            || command == "organize"
            || command.starts_with("pin-file ")
            || command.starts_with("remove-group ");
        if workspace_command && !self.state.prefs.desktop_enabled {
            self.notify(
                "DesktopCraft is disabled. Enable it in Settings -> Help.",
                time,
            );
            self.dialog = Some(Dialog::Settings {
                page: 4,
                pattern: String::new(),
                mode: 0,
                target: 0,
            });
            self.settings_bring_to_front = true;
            return;
        }
        if is_native
            && workspace_command
            && (self.native_tx_inflight
                || self.drag.is_some()
                || self.native_layout_restore_requested.is_some()
                || self.native_rules_sort_requested.is_some()
                || self.native_pin_paths_requested.is_some())
        {
            self.notify("Please wait for the current desktop update to finish.", time);
            return;
        }
        match command {
            "new-group" => {
                let (cx, cy) = host::cursor();
                let p = self.from_screen(cx, cy);
                self.create_group(Bounds {
                    x: p.x,
                    y: p.y,
                    w: 330.,
                    h: 260.,
                });
                if let Some(id) = self.selected.map(|s| s.0) {
                    let b = self.state.group(id).unwrap().bounds;
                    let r = self.to_screen(b);
                    if let Some(m) = self.monitor_at(cx, cy) {
                        let w = self.monitor_logical(&m, true);
                        let g = self.state.group_mut(id).unwrap();
                        g.bounds.x = g.bounds.x.clamp(w.x, (w.x + w.w - b.w).max(w.x));
                        g.bounds.y = g.bounds.y.clamp(w.y, (w.y + w.h - b.h).max(w.y));
                    }
                    let _ = r;
                }
            }
            c if c.starts_with("ai-tool ") => {
                crate::ai_tools::process_request(
                    self,
                    c.trim_start_matches("ai-tool ").trim(),
                    time,
                );
            }
            "settings" | "show" => {
                if !matches!(self.dialog, Some(Dialog::Settings { .. })) {
                    self.dialog = Some(Dialog::Settings {
                        page: if self.state.prefs.desktop_enabled { 0 } else { 4 },
                        pattern: String::new(),
                        mode: 0,
                        target: 0,
                    });
                }
                self.settings_bring_to_front = true;
            }
            "organize" => self.request_organize_desktop(time),
            c if c.starts_with("pin-file ") => {
                let path = c["pin-file ".len()..].trim();
                let path = Path::new(path);
                if !path.is_file() {
                    self.notify("Cannot pin: the selected file is not available.", time);
                    return;
                }
                // Context-menu pinning is *always virtual*. Never target a
                // linked Folder View, which would physically move the file.
                let regular_group = self
                    .selected
                    .and_then(|(id, _)| self.state.group(id))
                    .filter(|g| g.tab().folder.is_none())
                    .map(|g| g.id)
                    .or_else(|| {
                        self.state
                            .groups
                            .iter()
                            .find(|g| g.page == self.state.page && g.tab().folder.is_none())
                            .map(|g| g.id)
                    })
                    .or_else(|| {
                        self.state
                            .groups
                            .iter()
                            .find(|g| g.tab().folder.is_none())
                            .map(|g| g.id)
                    });
                let group = if is_native {
                    let Some(id) = regular_group else {
                        self.notify("Create a regular Box first before pinning files.", time);
                        return;
                    };
                    id
                } else {
                    regular_group.unwrap_or_else(|| {
                        self.state.add_group("Pinned files", self.fresh_bounds())
                    })
                };
                if let Err(error) = self.drop_path_into_box(&path.to_string_lossy(), group, time) {
                    self.notify(error, time);
                } else {
                    self.selected = Some((group, 0));
                }
            }
            c if c.starts_with("remove-group ") => {
                // Remove a group by id (its files stay on the desktop).
                if let Ok(id) = c["remove-group ".len()..].trim().parse::<u64>() {
                    self.request_remove_group(id, time);
                }
            }
            "exit" => {
                if let Some(h) = self.host.as_mut() {
                    h.quit = true;
                }
            }
            // Sent by the setup program after it replaced an installed copy.
            "updated" => self.notify(
                format!("DesktopCraft updated to {}", env!("CARGO_PKG_VERSION")),
                time,
            ),
            _ => self.notify(format!("Unknown command: {command}"), time),
        }
    }

}
