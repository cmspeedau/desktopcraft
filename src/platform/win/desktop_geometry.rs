use super::*;

#[link(name = "user32")]
extern "system" {
    fn GetDpiForWindow(hwnd: HWND) -> u32;
    fn GetDpiForSystem() -> u32;
}
unsafe extern "system" fn find_desktop_view(hwnd: HWND, out: LPARAM) -> BOOL {
    let view = FindWindowExW(
        hwnd,
        ptr::null_mut(),
        wide("SHELLDLL_DefView").as_ptr(),
        ptr::null(),
    );
    if !view.is_null() {
        let list = FindWindowExW(
            view,
            ptr::null_mut(),
            wide("SysListView32").as_ptr(),
            ptr::null(),
        );
        if !list.is_null() {
            *(out as *mut HWND) = list;
            return 0;
        }
    }
    1
}
/// Use Windows' documented desktop list-view icon spacing when available,
/// otherwise SPI_GETICONMETRICS, returned in 96-DPI logical pixels.
pub fn desktop_icon_spacing() -> Option<(f32, f32)> {
    unsafe {
        let mut list: HWND = ptr::null_mut();
        EnumWindows(Some(find_desktop_view), &mut list as *mut _ as LPARAM);
        if !list.is_null() {
            let mut result = 0usize;
            let ok =
                SendMessageTimeoutW(list, 0x1033, 0, 0, SMTO_ABORTIFHUNG, 300, &mut result);
            let (cx, cy) = ((result & 0xffff) as f32, ((result >> 16) & 0xffff) as f32);
            let dpi = GetDpiForWindow(list).max(1) as f32;
            if ok != 0 && cx > 0.0 && cy > 0.0 {
                return Some((cx * 96.0 / dpi, cy * 96.0 / dpi));
            }
        }
        #[repr(C)]
        #[allow(non_snake_case)]
        struct ICONMETRICSW {
            cbSize: u32,
            iHorzSpacing: i32,
            iVertSpacing: i32,
            iTitleWrap: i32,
            lfFont: [u8; 92],
        }
        let mut metrics: ICONMETRICSW = mem::zeroed();
        metrics.cbSize = mem::size_of::<ICONMETRICSW>() as u32;
        if SystemParametersInfoW(
            SPI_GETICONMETRICS,
            metrics.cbSize,
            &mut metrics as *mut _ as _,
            0,
        ) != 0
            && metrics.iHorzSpacing > 0
            && metrics.iVertSpacing > 0
        {
            let dpi = GetDpiForSystem().max(1) as f32;
            return Some((
                metrics.iHorzSpacing as f32 * 96.0 / dpi,
                metrics.iVertSpacing as f32 * 96.0 / dpi,
            ));
        }
        None
    }
}
pub fn double_click_interval() -> f64 {
    // Use the system double-click interval so the UI matches Windows input.
    unsafe { GetDoubleClickTime() as f64 / 1000.0 }
}
pub fn peek(front: bool, desktop: bool) {
    unsafe {
        let hwnd = window();
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetWindowPos(
                hwnd,
                if front {
                    HWND_TOPMOST
                } else if desktop {
                    HWND_BOTTOM
                } else {
                    HWND_NOTOPMOST
                },
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            if front {
                SetForegroundWindow(hwnd);
            }
        }
    }
}
pub fn desktop(enabled: bool, previous: &mut [i32; 4]) {
    unsafe {
        let hwnd = window();
        if hwnd.is_null() {
            return;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if enabled {
            let mut rect: RECT = mem::zeroed();
            GetWindowRect(hwnd, &mut rect);
            *previous = [
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ];
            SetWindowLongPtrW(
                hwnd,
                GWL_STYLE,
                (style & !(WS_OVERLAPPEDWINDOW as isize)) | WS_POPUP as isize,
            );
            SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rect as *mut _ as _, 0);
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        } else {
            SetWindowLongPtrW(
                hwnd,
                GWL_STYLE,
                (style & !(WS_POPUP as isize)) | WS_OVERLAPPEDWINDOW as isize,
            );
            SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                previous[0],
                previous[1],
                previous[2],
                previous[3],
                SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        }
    }
}
