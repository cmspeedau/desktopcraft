use super::*;

pub(super) unsafe extern "system" fn proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    match m {
        WM_NCHITTEST => HTTRANSPARENT,
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps: PaintStruct = std::mem::zeroed();
            let dc = BeginPaint(h, &mut ps);
            let mut rc = Rect::default();
            GetClientRect(h, &mut rc);
            let brush = CreateSolidBrush(GetWindowLongPtrW(h, GWLP_USERDATA) as u32);
            FillRect(dc, &rc, brush);
            DeleteObject(brush);
            EndPaint(h, &ps);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}

pub(super) unsafe fn draw_header_label(dc: *mut c_void, text: &str, mut rect: Rect, vertical: bool, font_px: i32, color: u32) {
    if text.is_empty() || rect.r <= rect.l || rect.b <= rect.t {
        return;
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.is_empty() {
        return;
    }
    SetTextColor(dc, color);
    if vertical {
        let prev_align = SetTextAlign(dc, TA_CENTER | TA_BASELINE);
        let cx = (rect.l + rect.r) / 2 - font_px / 4;
        let cy = (rect.t + rect.b) / 2;
        ExtTextOutW(
            dc,
            cx,
            cy,
            ETO_CLIPPED,
            &rect,
            units.as_ptr(),
            units.len().min(u32::MAX as usize) as u32,
            std::ptr::null(),
        );
        SetTextAlign(dc, prev_align);
    } else {
        DrawTextW(
            dc,
            units.as_ptr(),
            units.len().min(i32::MAX as usize) as i32,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}

pub(super) unsafe fn set_header_payload(hwnd: Hwnd, panel: HeaderPanel) {
    let next = Box::into_raw(Box::new(panel)) as isize;
    let prev = SetWindowLongPtrW(hwnd, GWLP_USERDATA, next) as *mut HeaderPanel;
    if !prev.is_null() {
        drop(Box::from_raw(prev));
    }
}

pub(super) unsafe extern "system" fn header_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    match m {
        WM_NCHITTEST => HTTRANSPARENT,
        WM_ERASEBKGND => 1,
        WM_NCDESTROY => {
            let prev = SetWindowLongPtrW(h, GWLP_USERDATA, 0) as *mut HeaderPanel;
            if !prev.is_null() {
                drop(Box::from_raw(prev));
            }
            DefWindowProcW(h, m, w, l)
        }
        WM_PAINT => {
            let mut ps: PaintStruct = std::mem::zeroed();
            let dc = BeginPaint(h, &mut ps);
            let mut rc = Rect::default();
            GetClientRect(h, &mut rc);
            let payload = (GetWindowLongPtrW(h, GWLP_USERDATA) as *const HeaderPanel).as_ref();
            let rgb = payload.map_or((24, 34, 48), |p| p.rgb);
            let brush = CreateSolidBrush(colorref(rgb));
            FillRect(dc, &rc, brush);
            DeleteObject(brush);
            let border = CreateSolidBrush(colorref((
                rgb.0.saturating_add(24),
                rgb.1.saturating_add(24),
                rgb.2.saturating_add(24),
            )));
            FrameRect(dc, &rc, border);
            DeleteObject(border);
            if let Some(panel) = payload {
                let esc = if panel.vertical { -900 } else { 0 };
                let face = wide("Leelawadee UI");
                let font = CreateFontW(
                    -((panel.font_px * 92 / 100).max(10)),
                    0,
                    esc,
                    esc,
                    400, // FW_NORMAL matches Macroquad LeelawUI weight
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    0,
                    0,
                    ANTIALIASED_QUALITY,
                    0,
                    face.as_ptr(),
                );
                let old_font = if !font.is_null() {
                    SelectObject(dc, font)
                } else {
                    std::ptr::null_mut()
                };
                SetBkMode(dc, TRANSPARENT);
                if !panel.tabs.is_empty() {
                    for tab in &panel.tabs {
                        let tr = Rect {
                            l: tab.x,
                            t: tab.y,
                            r: tab.x + tab.w,
                            b: tab.y + tab.h,
                        };
                        if tab.active {
                            let active_brush = CreateSolidBrush(colorref((
                                rgb.0.saturating_add(28),
                                rgb.1.saturating_add(28),
                                rgb.2.saturating_add(28),
                            )));
                            FillRect(dc, &tr, active_brush);
                            DeleteObject(active_brush);
                        }
                        let text_rect = Rect {
                            l: tr.l + 4,
                            t: tr.t,
                            r: (tr.r - 4).max(tr.l + 4),
                            b: tr.b,
                        };
                        draw_header_label(
                            dc,
                            &tab.name,
                            text_rect,
                            panel.vertical,
                            panel.font_px.max(10),
                            colorref((238, 243, 248)),
                        );
                    }
                } else if !panel.title.is_empty() {
                    let pad = if panel.vertical {
                        6
                    } else if panel.w > 120 {
                        (panel.w / 8).min(88)
                    } else {
                        8
                    };
                    let shadow = Rect {
                        l: rc.l + pad + 1,
                        t: rc.t + 1,
                        r: (rc.r - pad + 1).max(rc.l + pad + 1),
                        b: rc.b + 1,
                    };
                    draw_header_label(
                        dc,
                        &panel.title,
                        shadow,
                        panel.vertical,
                        panel.font_px.max(10),
                        0,
                    );
                    let fg = Rect {
                        l: rc.l + pad,
                        t: rc.t,
                        r: (rc.r - pad).max(rc.l + pad),
                        b: rc.b,
                    };
                    draw_header_label(
                        dc,
                        &panel.title,
                        fg,
                        panel.vertical,
                        panel.font_px.max(10),
                        colorref((255, 255, 255)),
                    );
                }
                if !font.is_null() {
                    SelectObject(dc, old_font);
                    DeleteObject(font);
                }
            }
            EndPaint(h, &ps);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}
