use super::*;

impl Layer {
    pub(super) unsafe fn clear(&mut self) {
        for (_, (hwnd, _)) in std::mem::take(&mut self.windows) {
            if IsWindow(hwnd) != 0 { DestroyWindow(hwnd); }
        }
        for (_, (hwnd, _)) in std::mem::take(&mut self.headers) {
            if IsWindow(hwnd) != 0 { DestroyWindow(hwnd); }
        }
        for (_, (hwnd, _)) in std::mem::take(&mut self.tints) {
            if IsWindow(hwnd) != 0 { DestroyWindow(hwnd); }
        }
    }

    pub(super) unsafe fn keep_tints_shown(&self) {
        for (_, (hwnd, _)) in &self.tints {
            if IsWindow(*hwnd) != 0 && (IsIconic(*hwnd) != 0 || IsWindowVisible(*hwnd) == 0) {
                ShowWindow(*hwnd, SW_SHOWNOACTIVATE);
                SetWindowPos(*hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    pub(super) unsafe fn shape(hwnd: Hwnd, p: &Panel) {
        let color = p.rgb.0 as u32 | (p.rgb.1 as u32) << 8 | (p.rgb.2 as u32) << 16;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, color as isize);
        SetLayeredWindowAttributes(hwnd, 0, p.alpha, LWA_ALPHA);
        // Square top edge: DesktopCraft's header sits directly above the body.
        let r = p.radius.max(0) * 2;
        let rgn = CreateRoundRectRgn(0, -r, p.w + 1, p.h + 1, r, r);
        if SetWindowRgn(hwnd, rgn, 1) == 0 { DeleteObject(rgn); }
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }

    /// Updates a top-level click-through tint overlay window using `UpdateLayeredWindow`
    /// with a 32-bpp premultiplied ARGB buffer so Windows 11 DWM never flashes a
    /// white GDI region frame or border when tinting is toggled.
    pub(super) unsafe fn paint_tint_ulw(hwnd: Hwnd, p: &Panel) {
        let w = p.w.max(1);
        let h = p.h.max(1);
        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return;
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return;
        }
        let bmi = BitmapInfo {
            bmi_header: BitmapInfoHeader {
                bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: w,
                bi_height: -h, // top-down DIB
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0, // BI_RGB
                bi_size_image: 0,
                bi_x_pels_per_meter: 0,
                bi_y_pels_per_meter: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            bmi_colors: [0],
        };
        let mut bits: *mut u8 = std::ptr::null_mut();
        let dib = CreateDIBSection(screen_dc, &bmi, 0, &mut bits, std::ptr::null_mut(), 0);
        if !dib.is_null() && !bits.is_null() {
            let old_bmp = SelectObject(mem_dc, dib);
            let a = u32::from(p.alpha);
            let r_pre = ((u32::from(p.rgb.0) * a + 127) / 255) as u8;
            let g_pre = ((u32::from(p.rgb.1) * a + 127) / 255) as u8;
            let b_pre = ((u32::from(p.rgb.2) * a + 127) / 255) as u8;
            let fill_px: u32 = u32::from(b_pre)
                | (u32::from(g_pre) << 8)
                | (u32::from(r_pre) << 16)
                | (a << 24);
            let rad = p.radius.max(0).min(w / 2).min(h / 2);
            let rad_sq = rad * rad;
            let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (w as usize) * (h as usize));
            let straight_rows = (h - rad).max(0) as usize;
            let w_usize = w as usize;
            pixels[..straight_rows * w_usize].fill(fill_px);
            for y in (h - rad).max(0)..h {
                let dy = y - (h - rad - 1);
                let dy_sq = dy * dy;
                let row = &mut pixels[(y as usize) * w_usize..(y as usize + 1) * w_usize];
                for x in 0..w {
                    let inside = if x < rad {
                        let dx = rad - x;
                        dx * dx + dy_sq <= rad_sq
                    } else if x >= w - rad {
                        let dx = x - (w - rad - 1);
                        dx * dx + dy_sq <= rad_sq
                    } else {
                        true
                    };
                    row[x as usize] = if inside { fill_px } else { 0 };
                }
            }
            let dst_pt = Point { x: p.x, y: p.y };
            let size = Size { cx: w, cy: h };
            let src_pt = Point { x: 0, y: 0 };
            let blend = BlendFunction {
                blend_op: AC_SRC_OVER,
                blend_flags: 0,
                source_constant_alpha: 255,
                alpha_format: AC_SRC_ALPHA,
            };
            UpdateLayeredWindow(
                hwnd,
                screen_dc,
                &dst_pt,
                &size,
                mem_dc,
                &src_pt,
                0,
                &blend,
                ULW_ALPHA,
            );
            SelectObject(mem_dc, old_bmp);
            DeleteObject(dib);
        }
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);
    }

    pub(super) unsafe fn shape_header(hwnd: Hwnd, h: &HeaderPanel) {
        let r = h.radius.max(0) * 2;
        let rgn = match h.corners {
            HeaderCorners::TopRounded => CreateRoundRectRgn(0, 0, h.w + 1, h.h + r + 1, r, r),
            HeaderCorners::BottomRounded => CreateRoundRectRgn(0, -r, h.w + 1, h.h + 1, r, r),
            HeaderCorners::AllRounded => CreateRoundRectRgn(0, 0, h.w + 1, h.h + 1, r, r),
        };
        if SetWindowRgn(hwnd, rgn, 1) == 0 { DeleteObject(rgn); }
        set_header_payload(hwnd, h.clone());
        SetLayeredWindowAttributes(hwnd, 0, h.alpha, LWA_ALPHA);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }

    pub(super) unsafe fn apply(&mut self, frame: &BackdropFrame) {
        let any_windows = !frame.panels.is_empty() || !frame.headers.is_empty() || !frame.tints.is_empty();
        if self.parent.is_null() || IsWindow(self.parent) == 0 {
            // Explorer restarted, or the layer was never created.
            self.clear();
            self.parent = if !any_windows { std::ptr::null_mut() } else { wallpaper_layer() };
            if self.parent.is_null() {
                if any_windows {
                    crate::host::event("WARN", "native_backdrop.layer_missing",
                        "Explorer wallpaper layer not found; Box bodies not drawn behind icons");
                }
                return;
            }
            crate::host::event("INFO", "native_backdrop.layer_ready",
                &format!("parent=0x{:X} panels={} headers={}", self.parent as usize, frame.panels.len(), frame.headers.len()));
        }
        let keep: Vec<u64> = frame.panels.iter().map(|p| p.id).collect();
        let gone: Vec<u64> = self.windows.keys().copied().filter(|id| !keep.contains(id)).collect();
        for id in gone {
            if let Some((hwnd, _)) = self.windows.remove(&id) { DestroyWindow(hwnd); }
        }
        for p in &frame.panels {
            let mut origin = Point { x: p.x, y: p.y };
            ScreenToClient(self.parent, &mut origin);
            match self.windows.get_mut(&p.id) {
                Some((hwnd, last)) if IsWindow(*hwnd) != 0 => {
                    if (last.x, last.y, last.w, last.h) != (p.x, p.y, p.w, p.h) {
                        SetWindowPos(*hwnd, std::ptr::null_mut(), origin.x, origin.y, p.w, p.h,
                            SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    if (last.w, last.h, last.rgb, last.alpha, last.radius)
                        != (p.w, p.h, p.rgb, p.alpha, p.radius) {
                        Self::shape(*hwnd, p);
                    }
                    *last = *p;
                }
                _ => {
                    let hwnd = CreateWindowExW(
                        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                        self.class.as_ptr(), std::ptr::null(), WS_CHILD | WS_VISIBLE,
                        origin.x, origin.y, p.w, p.h, self.parent, std::ptr::null_mut(),
                        self.instance, std::ptr::null_mut());
                    if hwnd.is_null() {
                        if !self.create_failure_logged {
                            self.create_failure_logged = true;
                            crate::host::event("WARN", "native_backdrop.create_failed",
                                &format!("CreateWindowEx error {}", GetLastError()));
                        }
                        continue;
                    }
                    Self::shape(hwnd, p);
                    self.windows.insert(p.id, (hwnd, *p));
                }
            }
        }
        let keep_headers: Vec<u64> = frame.headers.iter().map(|h| h.id).collect();
        let gone_headers: Vec<u64> = self.headers.keys().copied().filter(|id| !keep_headers.contains(id)).collect();
        for id in gone_headers {
            if let Some((hwnd, _)) = self.headers.remove(&id) { DestroyWindow(hwnd); }
        }
        for h in &frame.headers {
            let mut origin = Point { x: h.x, y: h.y };
            ScreenToClient(self.parent, &mut origin);
            match self.headers.get_mut(&h.id) {
                Some((hwnd, last)) if IsWindow(*hwnd) != 0 => {
                    let resized = (last.x, last.y, last.w, last.h) != (h.x, h.y, h.w, h.h);
                    if resized {
                        SetWindowPos(*hwnd, std::ptr::null_mut(), origin.x, origin.y, h.w, h.h,
                            SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                    if last != h {
                        Self::shape_header(*hwnd, h);
                    }
                    *last = h.clone();
                }
                _ => {
                    let hwnd = CreateWindowExW(
                        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
                        self.header_class.as_ptr(), std::ptr::null(), WS_CHILD | WS_VISIBLE,
                        origin.x, origin.y, h.w, h.h, self.parent, std::ptr::null_mut(),
                        self.instance, std::ptr::null_mut());
                    if hwnd.is_null() {
                        if !self.create_failure_logged {
                            self.create_failure_logged = true;
                            crate::host::event("WARN", "native_backdrop.create_failed",
                                &format!("CreateWindowEx header error {}", GetLastError()));
                        }
                        continue;
                    }
                    Self::shape_header(hwnd, h);
                    self.headers.insert(h.id, (hwnd, h.clone()));
                }
            }
        }
        let keep_tints: Vec<u64> = frame.tints.iter().map(|t| t.id).collect();
        let gone_tints: Vec<u64> = self.tints.keys().copied().filter(|id| !keep_tints.contains(id)).collect();
        for id in gone_tints {
            if let Some((hwnd, _)) = self.tints.remove(&id) { DestroyWindow(hwnd); }
        }
        let shell_owner = GetShellWindow();
        for t in &frame.tints {
            match self.tints.get_mut(&t.id) {
                Some((hwnd, last)) if IsWindow(*hwnd) != 0 => {
                    if *last != *t {
                        Self::paint_tint_ulw(*hwnd, t);
                        SetWindowPos(*hwnd, HWND_BOTTOM, t.x, t.y, t.w, t.h, SWP_NOACTIVATE);
                    }
                    *last = *t;
                }
                _ => {
                    let hwnd = CreateWindowExW(
                        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                        self.class.as_ptr(), std::ptr::null(), WS_POPUP,
                        t.x, t.y, t.w, t.h, shell_owner, std::ptr::null_mut(),
                        self.instance, std::ptr::null_mut());
                    if hwnd.is_null() {
                        continue;
                    }
                    let ncrp: u32 = DWMNCRP_DISABLED;
                    let disable_transitions: u32 = 1;
                    let no_round: u32 = DWMWCP_DONOTROUND;
                    let border_none: u32 = DWMWA_COLOR_NONE;
                    DwmSetWindowAttribute(hwnd, DWMWA_NCRENDERING_POLICY, &ncrp as *const u32 as _, 4);
                    DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, &disable_transitions as *const u32 as _, 4);
                    DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &no_round as *const u32 as _, 4);
                    DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border_none as *const u32 as _, 4);
                    Self::paint_tint_ulw(hwnd, t);
                    SetWindowPos(
                        hwnd,
                        HWND_BOTTOM,
                        t.x,
                        t.y,
                        t.w,
                        t.h,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    self.tints.insert(t.id, (hwnd, *t));
                }
            }
        }
    }
}
