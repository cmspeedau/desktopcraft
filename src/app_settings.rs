use crate::*;

impl App {
    pub(crate) fn settings_in_own_window(&self) -> bool {
        #[cfg(windows)]
        {
            self.host.is_some()
                && !self.automation
                && matches!(self.dialog, Some(Dialog::Settings { .. }))
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    pub(crate) fn has_blocking_dialog(&self) -> bool {
        self.dialog.is_some() && !self.settings_in_own_window()
    }

    pub(crate) fn open_modal_preserving_settings(&mut self, modal: Dialog) {
        if matches!(self.dialog, Some(Dialog::Settings { .. })) {
            self.settings_return = self.dialog.take();
        }
        self.dialog = Some(modal);
    }

    #[cfg(windows)]
    pub(crate) fn settings_window_focused(&self) -> bool {
        self.settings_window
            .as_ref()
            .is_some_and(settings_window::SettingsWindow::has_focus)
    }

    #[cfg(windows)]
    pub(crate) fn settings_window_active_on_page(&self, page: usize) -> bool {
        matches!(self.dialog, Some(Dialog::Settings { page: p, .. }) if p == page)
            && self.settings_window_focused()
    }

    /// Draw something centered on the primary monitor's work area (dialogs and
    /// messages in desktop mode, where the workspace spans every monitor).
    pub(super) fn in_primary_view(
        &mut self,
        p: &mut Painter,
        input: &Input,
        f: impl FnOnce(&mut Self, &mut Painter, &Input),
    ) {
        let view = self.primary_view();
        self.in_monitor_view(p, input, view, f);
    }
    pub(super) fn in_monitor_view(
        &mut self,
        p: &mut Painter,
        input: &Input,
        v: Bounds,
        f: impl FnOnce(&mut Self, &mut Painter, &Input),
    ) {
        let (w, h) = (self.width, self.height);
        set_camera(&Camera2D {
            target: vec2(w / 2. - v.x, h / 2. - v.y),
            zoom: vec2(2. / w, 2. / h),
            ..Default::default()
        });
        self.width = v.w;
        self.height = v.h;
        let mut local = input.clone();
        local.pos -= vec2(v.x, v.y);
        let mouse = p.mouse;
        p.mouse = local.pos;
        p.offset = vec2(v.x, v.y);
        f(self, p, &local);
        p.mouse = mouse;
        p.offset = Vec2::ZERO;
        self.width = w;
        self.height = h;
        set_camera(&workspace_camera(w, h));
    }
    /// Settings uses virtual-desktop coordinates in desktop mode, not the
    /// selected monitor's local coordinates. A floating Settings window can
    /// therefore cross from the primary display onto another monitor.
    pub(crate) fn settings_bounds_local(&self) -> Bounds {
        if self.host.is_some() {
            let h = self.host.as_ref().unwrap();
            return floating_settings_bounds(
                Bounds {
                    x: 0.,
                    y: 0.,
                    w: self.width.max(h.virtual_rect[2] as f32 / h.scale.max(0.1)),
                    h: self.height.max(h.virtual_rect[3] as f32 / h.scale.max(0.1)),
                },
                self.primary_view(),
                self.settings_origin,
            );
        }
        self.settings_bounds_local_for_view(Bounds {
            x: 0.,
            y: 0.,
            w: self.width,
            h: self.height,
        })
    }
    pub(crate) fn settings_bounds_local_for_view(&self, view: Bounds) -> Bounds {
        let w = 870.0f32;
        let h = 650.0f32;
        let default = vec2(((view.w - w) / 2.).max(0.), ((view.h - h) / 2.).max(0.));
        let origin = self.settings_origin.unwrap_or(default);
        Bounds {
            x: origin.x.clamp(0., (view.w - w).max(0.)),
            y: origin.y.clamp(0., (view.h - h).max(0.)),
            w,
            h,
        }
    }
}

impl App {
    pub(super) fn host_lifted_for_popup(&self) -> bool {
        self.host.is_some() && !self.peek && self.has_blocking_dialog()
    }
}
