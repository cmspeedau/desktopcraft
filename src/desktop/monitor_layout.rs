use super::*;


impl App {
    pub fn host_scale(&self) -> f32 {
        self.host.as_ref().map_or(1.0, |h| h.scale)
    }
    /// Workspace (logical) rectangle -> physical screen rectangle.
    pub fn to_screen(&self, b: Bounds) -> [i32; 4] {
        let Some(h) = &self.host else {
            return [b.x as i32, b.y as i32, b.w as i32, b.h as i32];
        };
        let s = h.scale;
        [
            h.virtual_rect[0] + (b.x * s).round() as i32,
            h.virtual_rect[1] + (b.y * s).round() as i32,
            (b.w * s).round() as i32,
            (b.h * s).round() as i32,
        ]
    }
    pub fn from_screen(&self, x: i32, y: i32) -> Vec2 {
        let Some(h) = &self.host else {
            return vec2(x as f32, y as f32);
        };
        vec2(
            (x - h.virtual_rect[0]) as f32 / h.scale,
            (y - h.virtual_rect[1]) as f32 / h.scale,
        )
    }
    pub(crate) fn monitor_logical(&self, m: &host::Monitor, work: bool) -> Bounds {
        let r = if work { m.work } else { m.rect };
        let p = self.from_screen(r[0], r[1]);
        let s = self.host_scale();
        Bounds {
            x: p.x,
            y: p.y,
            w: r[2] as f32 / s,
            h: r[3] as f32 / s,
        }
    }
    /// The actual Windows primary monitor, regardless of the optional display
    /// chosen for other DesktopCraft tools (used for the page switcher).
    pub fn windows_primary_view(&self) -> Bounds {
        self.host
            .as_ref()
            .and_then(|h| h.monitors.iter().find(|m| m.primary).or(h.monitors.first()))
            .map(|m| self.monitor_logical(m, true))
            .unwrap_or(Bounds {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            })
    }
    /// The selected control monitor's work area, or OS primary by default.
    pub fn primary_view(&self) -> Bounds {
        let Some(h) = &self.host else {
            return Bounds {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            };
        };
        h.monitors
            .iter()
            .find(|m| {
                !self.state.prefs.workspace_monitor.is_empty()
                    && m.key == self.state.prefs.workspace_monitor
            })
            .or_else(|| h.monitors.iter().find(|m| m.primary))
            .or(h.monitors.first())
            .map(|m| self.monitor_logical(m, true))
            .unwrap_or(Bounds {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            })
    }
    pub(crate) fn clamp_bounds_to_monitor_work(&self, bounds: Bounds) -> Bounds {
        let screen = self.to_screen(bounds);
        let work = self
            .monitor_at(screen[0] + screen[2] / 2, screen[1] + screen[3] / 2)
            .map(|m| self.monitor_logical(&m, true))
            .unwrap_or(Bounds {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            });
        Bounds {
            x: bounds.x.clamp(work.x, (work.x + work.w - bounds.w).max(work.x)),
            y: bounds.y.clamp(work.y, (work.y + work.h - bounds.h).max(work.y)),
            ..bounds
        }
    }
    pub(crate) fn menu_monitor_bounds(&self, at: Vec2) -> Bounds {
        let screen = self.to_screen(Bounds {
            x: at.x,
            y: at.y,
            w: 0.,
            h: 0.,
        });
        self.monitor_at(screen[0], screen[1])
            .map(|m| self.monitor_logical(&m, false))
            .unwrap_or(Bounds {
                x: 0.,
                y: 0.,
                w: self.width,
                h: self.height,
            })
    }
    pub(crate) fn monitor_at(&self, x: i32, y: i32) -> Option<host::Monitor> {
        let h = self.host.as_ref()?;
        h.monitors
            .iter()
            .find(|m| {
                x >= m.rect[0]
                    && y >= m.rect[1]
                    && x < m.rect[0] + m.rect[2]
                    && y < m.rect[1] + m.rect[3]
            })
            .or_else(|| h.monitors.iter().find(|m| m.primary))
            .cloned()
    }
    // ---------- per-monitor layouts ----------
    fn placement(&self, g: &Group) -> Option<Placement> {
        let r = self.to_screen(g.bounds);
        let m = self.monitor_at(r[0] + r[2] / 2, r[1] + r[3] / 2)?;
        let s = self.host_scale();
        Some(Placement {
            monitor: m.key.clone(),
            x: (r[0] - m.work[0]) as f32 / s,
            y: (r[1] - m.work[1]) as f32 / s,
            w: g.bounds.w,
            h: g.bounds.h,
        })
    }
    /// Remember where every group is on which monitor, for this arrangement.
    pub fn remember_layout(&mut self) {
        let Some(sig) = self.host.as_ref().map(|h| h.signature.clone()) else {
            return;
        };
        if sig.is_empty() {
            return;
        }
        let mut layout = BTreeMap::new();
        for g in &self.state.groups {
            if let Some(p) = self.placement(g) {
                layout.insert(g.id, p);
            }
        }
        for (id, p) in &layout {
            self.state.placements.insert(*id, p.clone());
        }
        self.state.placements.retain(|id, _| {
            layout.contains_key(id) || self.state.groups.iter().any(|g| g.id == *id)
        });
        self.state.layouts.insert(sig, layout);
    }
    /// Put groups back for the current monitor arrangement: its own saved layout
    /// first, then each group's last place on a monitor that is still connected,
    /// otherwise the primary monitor.
    pub(super) fn apply_layout(&mut self) {
        let Some(h) = &self.host else { return };
        let sig = h.signature.clone();
        let monitors = h.monitors.clone();
        let saved = self.state.layouts.get(&sig).cloned().unwrap_or_default();
        let primary = monitors
            .iter()
            .find(|m| {
                !self.state.prefs.workspace_monitor.is_empty()
                    && m.key == self.state.prefs.workspace_monitor
            })
            .or_else(|| monitors.iter().find(|m| m.primary))
            .or(monitors.first())
            .cloned();
        let ids: Vec<u64> = self.state.groups.iter().map(|g| g.id).collect();
        for (n, id) in ids.into_iter().enumerate() {
            let place = saved
                .get(&id)
                .cloned()
                .or_else(|| self.state.placements.get(&id).cloned());
            let target = place.as_ref().and_then(|p| {
                monitors
                    .iter()
                    .find(|m| m.key == p.monitor)
                    .map(|m| (m.clone(), p.clone()))
            });
            let bounds = match target.as_ref() {
                Some((m, p)) => {
                    let o = self.from_screen(m.work[0], m.work[1]);
                    Some(Bounds {
                        x: o.x + p.x,
                        y: o.y + p.y,
                        w: p.w,
                        h: p.h,
                    })
                }
                None => {
                    // Monitor gone (or never placed): keep it if it is still on
                    // screen, else move it to the primary monitor.
                    let g = self.state.group(id).unwrap();
                    let r = self.to_screen(g.bounds);
                    let on_screen = monitors.iter().any(|m| {
                        let (cx, cy) = (r[0] + r[2] / 2, r[1] + 20);
                        cx >= m.rect[0]
                            && cy >= m.rect[1]
                            && cx < m.rect[0] + m.rect[2]
                            && cy < m.rect[1] + m.rect[3]
                    });
                    match (&primary, on_screen) {
                        (_, true) | (None, _) => None,
                        (Some(m), false) => {
                            let o = self.from_screen(m.work[0], m.work[1]);
                            Some(Bounds {
                                x: o.x + 24. + n as f32 * 28.,
                                y: o.y + 24. + n as f32 * 28.,
                                ..g.bounds
                            })
                        }
                    }
                }
            };
            let g = self.state.group(id).unwrap();
            let mut b = bounds.unwrap_or(g.bounds);
            let dock = g.dock.filter(|_| self.native_env.chrome_only);
            let r = self.to_screen(b);
            // Docked geometry belongs to the saved monitor, even if a smaller
            // work area puts the old center on the next monitor before clamping.
            let monitor = target.filter(|_| dock.is_some()).map(|(m, _)| m)
                .or_else(|| self.monitor_at(r[0] + r[2] / 2, r[1] + 10));
            if let Some(m) = monitor {
                let work = self.monitor_logical(&m, true);
                if let Some(edge) = dock {
                    b = edge.fit_body(b, work);
                } else {
                    b.w = b.w.min(work.w);
                    b.h = b.h.min(work.h);
                    b.x = b.x.clamp(work.x, work.x + work.w - b.w);
                    b.y = b.y.clamp(work.y, work.y + work.h - b.h);
                }
            }
            self.state.group_mut(id).unwrap().bounds = b;
        }
    }
    pub(crate) fn check_monitors(&mut self) {
        let restore = std::env::var_os("DESKTOPCRAFT_NATIVE_CHROME_ONLY").is_none();
        self.sync_monitors(restore);
    }
    /// Native Desktop start: bring Boxes to this monitor arrangement's saved
    /// layout BEFORE the Explorer worker plans real icon cells, so the icons
    /// and the Box chrome use the same bounds for the whole session.
    pub fn prepare_native_monitor_layout(&mut self) {
        self.sync_monitors(true);
    }
    fn sync_monitors(&mut self, restore_layout: bool) {
        let monitors = host::monitors();
        if monitors.is_empty() {
            return;
        }
        let rect = host::virtual_rect();
        let sig = host::signature(&monitors);
        let Some(h) = &self.host else { return };
        let changed = sig != h.signature;
        let moved = rect != h.virtual_rect;
        if !changed && !moved {
            return;
        }
        if changed && !h.signature.is_empty() {
            self.remember_layout();
        }
        host::event(
            "INFO",
            "display.configuration_changed",
            &format!(
                "monitors={} primary_found={} moved={}",
                monitors.len(),
                monitors.iter().any(|m| m.primary),
                moved
            ),
        );
        let primary = monitors
            .iter()
            .find(|m| m.primary)
            .or(monitors.first())
            .unwrap();
        let h = self.host.as_mut().unwrap();
        h.scale = primary.dpi as f32 / 96.0;
        h.monitors = monitors.clone();
        h.virtual_rect = rect;
        h.signature = sig;
        if moved {
            host::place(rect);
        }
        if restore_layout {
            self.apply_layout();
        } else {
            // Explorer's real icons were placed from the Workspace bounds the
            // native worker verified at startup. Restoring a remembered
            // per-monitor layout here would draw the Box chrome somewhere
            // else than the icons. A topology change mid-session already
            // stops the native worker, which restores the original icons.
            host::event("INFO", "native_desktop.layout_kept",
                "Box bounds kept as verified by the Explorer worker");
        }
        if let Some(h) = self.host.as_mut() {
            h.last_sync = -10.;
        }
    }

}
