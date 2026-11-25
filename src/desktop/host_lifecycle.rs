use super::*;

/// Windows' canonical Recycle Bin shell folder (language-independent).
/// Matching the shell namespace GUID is safer than a localized display name
/// or a user-controlled ordinary file whose name happens to be "Recycle Bin".
/// Diagnostic: how often the host window's clip region changes. Each change
/// makes Windows repaint whatever was under the old region.
fn region_update_diagnostics() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    static WINDOW: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    let count = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let started = WINDOW.load(Ordering::Relaxed);
    if now != started {
        WINDOW.store(now, Ordering::Relaxed);
        COUNT.store(0, Ordering::Relaxed);
        if started != 0 && count > 1 {
            host::event("INFO", "host.region_updates", &format!("per_second={}", count - 1));
        }
    }
}

/// Switch Explorer's desktop icons back on (exit, crash, Windows sign-out).
pub fn restore_explorer_icons() {
    if let Some(view) = shell::DesktopView::acquire() {
        view.set_icons_visible(true);
    }
}


impl App {
    /// Quick-hide is owned by Native Desktop; never change Explorer visibility here.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
    }

    /// Once per frame after drawing: clip the window to what was drawn and keep it
    /// on the desktop layer unless a dialog, menu or Peek needs it in front.
    pub fn host_after_frame(&mut self, p: &Painter) {
        if self.host.is_none() {
            return;
        }
        if host::color_pick_shielded() {
            let h = self.host.as_mut().unwrap();
            if !h.region.is_empty() {
                host::set_region(&[]);
                h.region.clear();
            }
            if h.front {
                host::set_front(false);
                h.front = false;
            }
            return;
        }
        let s = self.host_scale();
        let mut region: Vec<[i32; 5]> = p
            .regions
            .borrow()
            .iter()
            .map(|(b, r)| {
                let left = (b.x * s).floor() as i32;
                let top = (b.y * s).floor() as i32;
                [
                    left,
                    top,
                    ((b.x + b.w) * s).ceil() as i32 - left,
                    ((b.y + b.h) * s).ceil() as i32 - top,
                    (r * s * 2.).round() as i32,
                ]
            })
            .collect();
        region.sort();
        // Native Desktop: every clip-region change makes Explorer repaint the
        // desktop under it. A Box drag changed it ~25 times a second and left
        // Explorer so busy afterwards that the icon reflow for the drop timed
        // out. While a Box is dragged or resized, clip to the whole window
        // once instead: cleared pixels stay transparent (DWM blur-behind),
        // and the drag holds mouse capture anyway.
        // The same while the dragged Box's icon copies are drawn (until the
        // drop's reflow has placed the real icons), so the copies are visible.
        #[cfg(windows)]
        let ghost = self.native_ghost.is_some();
        #[cfg(not(windows))]
        let ghost = false;
        let own_settings = self.settings_in_own_window();
        let front = self.has_blocking_dialog() || self.peek;
        if !front && self.native_env.chrome_only && (ghost || self.is_active_native_box_drag()) {
            if let Some(h) = self.host.as_ref() {
                region = vec![[0, 0, h.virtual_rect[2], h.virtual_rect[3], 0]];
            }
        }
        let h = self.host.as_mut().unwrap();
        if !front && h.front {
            host::set_front(false);
            h.front = false;
        }
        if region != h.region {
            let whole_desktop = [[0, 0, h.virtual_rect[2], h.virtual_rect[3], 0]];
            let restore_native_clip = self.native_env.chrome_only
                && h.region.as_slice() == whole_desktop && region.as_slice() != whole_desktop;
            if !host::set_region(&region) {
                host::event("ERROR", "desktop.input_region_failed",
                    "Could not update the Box input region; desktop host hidden");
                h.quit = true;
                return;
            }
            h.region = region;
            if restore_native_clip {
                host::event("INFO", "native_desktop.drag_input_clip_restored", "");
            }
            region_update_diagnostics();
        }
        // The normal desktop overlay does not need a taskbar button, but
        // Settings should be discoverable and focusable from the taskbar.
        let settings = (matches!(self.dialog, Some(Dialog::Settings { .. })) && !own_settings)
            || self.minimized_settings.is_some();
        if settings != h.taskbar_visible {
            h.taskbar_visible = settings;
            host::settings_taskbar(settings);
        }
        if front && !h.front {
            host::set_front(true);
            h.front = true;
        }
    }

    /// On exit, restore native desktop icon visibility without rearranging
    /// any Explorer icon. The legacy tuck-on-exit moved users' original icon
    /// positions (38+ moves in earlier live tests). The future Explorer-owned
    /// architecture must never mutate native layout merely by quitting.
    pub fn host_exit(&mut self) {
        host::remove_tray();
        if self.state.explorer_icons_hidden {
            restore_explorer_icons();
            self.state.explorer_icons_hidden = false;
        }
        self.remember_layout();
        host::log("exit: Explorer icons shown again");
    }
    /// Native Explorer-owned preview: perform only the display geometry
    /// initialization shared with legacy mode. Never disable Explorer icons,
    /// install legacy quick-hide hooks, sync/reflow native icon positions or
    /// write layout data just because this preview opened.
    pub fn host_start_native_chrome(&mut self) {
        self.check_monitors();
        self.state.explorer_icons_hidden = false;
        host::log("native-chrome: monitor geometry initialized without hiding native icons");
    }

}
