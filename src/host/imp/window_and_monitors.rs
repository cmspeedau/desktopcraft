use super::*;

unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: LPRECT, out: LPARAM) -> BOOL {
    let list = &mut *(out as *mut Vec<Monitor>);
    let mut info: MONITORINFOEXW = mem::zeroed();
    info.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(m, &mut info as *mut _ as *mut MONITORINFO) != 0 {
        let r = info.rcMonitor;
        let w = info.rcWork;
        let (mut dx, mut dy) = (96u32, 96u32);
        GetDpiForMonitor(m, 0, &mut dx, &mut dy);
        let name = String::from_utf16_lossy(
            &info.szDevice[..info.szDevice.iter().position(|&c| c == 0).unwrap_or(0)],
        );
        list.push(Monitor {
            key: name,
            rect: [r.left, r.top, r.right - r.left, r.bottom - r.top],
            work: [w.left, w.top, w.right - w.left, w.bottom - w.top],
            dpi: dx.max(48),
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    TRUE
}
pub fn monitors() -> Vec<Monitor> {
    let mut list: Vec<Monitor> = vec![];
    unsafe {
        EnumDisplayMonitors(
            ptr::null_mut(),
            ptr::null(),
            Some(collect),
            &mut list as *mut _ as LPARAM,
        );
    }
    list
}
pub fn virtual_rect() -> [i32; 4] {
    unsafe {
        [
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        ]
    }
}

unsafe extern "system" fn find(hwnd: HWND, out: LPARAM) -> BOOL {
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let mut class = [0u16; 32];
    let n = GetClassNameW(hwnd, class.as_mut_ptr(), 32);
    if pid == std::process::id()
        && String::from_utf16_lossy(&class[..n as usize]) == "MINIQUADAPP"
    {
        *(out as *mut HWND) = hwnd;
        0
    } else {
        1
    }
}
pub fn window() -> HWND {
    unsafe {
        let mut hwnd: HWND = ptr::null_mut();
        EnumWindows(Some(find), &mut hwnd as *mut _ as LPARAM);
        hwnd
    }
}
/// Keep the window covering every monitor.
pub fn place(rect: [i32; 4]) {
    unsafe {
        let hwnd = window();
        if !hwnd.is_null() {
            SetWindowPos(
                hwnd,
                ptr::null_mut(),
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}
/// Clip the window to the groups (client physical pixels). Everything else is
/// the real desktop: Explorer keeps its icons, wallpaper, menus and drops.
/// Each entry: x, y, w, h, corner diameter (client physical pixels).
pub fn set_region(rects: &[[i32; 5]]) -> bool {
    unsafe {
        let hwnd = window();
        if hwnd.is_null() {
            return false;
        }
        if color_pick_shielded() && !rects.is_empty() {
            return false;
        }
        let region = CreateRectRgn(0, 0, 0, 0);
        if region.is_null() {
            ShowWindow(hwnd, SW_HIDE);
            return false;
        }
        for r in rects {
            let part = if r[4] > 0 {
                CreateRoundRectRgn(r[0], r[1], r[0] + r[2], r[1] + r[3], r[4], r[4])
            } else {
                CreateRectRgn(r[0], r[1], r[0] + r[2], r[1] + r[3])
            };
            let ok = !part.is_null() && CombineRgn(region, region, part, RGN_OR) != 0;
            DeleteObject(part as _);
            if !ok {
                DeleteObject(region as _);
                ShowWindow(hwnd, SW_HIDE);
                return false;
            }
        }
        // On success Windows owns the region.
        if SetWindowRgn(hwnd, region, TRUE) != 0 {
            return true;
        }
        DeleteObject(region as _);
        ShowWindow(hwnd, SW_HIDE);
        false
    }
}
pub fn settings_has_focus() -> bool {
    unsafe {
        let hwnd = window();
        !hwnd.is_null() && GetForegroundWindow() == hwnd
    }
}
/// A click in a Portal search field gives the overlay keyboard input.
pub fn focus_portal_filter() {
    unsafe {
        let hwnd = window();
        if !hwnd.is_null() && GetForegroundWindow() != hwnd {
            SetForegroundWindow(hwnd);
        }
    }
}
/// Minimizing just the floating Settings panel should leave desktop
/// groups usable. Return foreground focus to the Windows shell.
pub fn settings_deactivate() {
    unsafe {
        let hwnd = window();
        if hwnd.is_null() {
            return;
        }
        SetWindowPos(
            hwnd,
            HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let shell = GetShellWindow();
        if !shell.is_null() {
            SetForegroundWindow(shell);
        }
    }
}
/// Temporarily expose Settings in the Windows taskbar and Alt+Tab.
/// The window itself stays the same clipped desktop host; no second
/// process, window or desktop manager instance is launched.
pub fn settings_taskbar(visible: bool) {
    unsafe {
        let hwnd = window();
        if hwnd.is_null() {
            return;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let desired = if visible {
            (style | WS_EX_APPWINDOW as isize) & !(WS_EX_TOOLWINDOW as isize)
        } else {
            (style | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize)
        };
        if desired == style {
            return;
        }
        // Windows creates/removes the taskbar button after a visibility
        // transition. Keep the current region and restore Z-order later.
        ShowWindow(hwnd, SW_HIDE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired);
        let title = if visible {
            "DesktopCraft Settings"
        } else {
            "DesktopCraft"
        };
        SetWindowTextW(hwnd, wide(title).as_ptr());
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(
            hwnd,
            ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
}
/// Front: above other windows (dialogs, menus, Peek). Otherwise the desktop layer.
pub fn set_front(front: bool) {
    macroquad::miniquad::DESKTOP_HOST_FRONT.store(front, std::sync::atomic::Ordering::Relaxed);
    unsafe {
        let hwnd = window();
        if hwnd.is_null() {
            return;
        }
        if front {
            // Peek must be ABOVE ordinary foreground apps while held;
            // HWND_TOP only raises within the non-topmost tier.
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        } else {
            // Important: remove TOPMOST as soon as the chord is released,
            // then return the clipped host to Explorer's desktop layer.
            SetWindowPos(
                hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}
/// After "Show desktop" (Win+D) or a minimize, show the groups again.
pub fn keep_shown() {
    if color_pick_shielded() {
        return;
    }
    unsafe {
        let hwnd = window();
        if !hwnd.is_null() && (IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0) {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}
/// Read the physical mouse button while moving across region-clipped boxes.
pub fn mouse_left_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000 != 0 }
}
pub fn mouse_right_down() -> bool {
    unsafe { GetAsyncKeyState(VK_RBUTTON) as u16 & 0x8000 != 0 }
}
fn window_class_name(hwnd: HWND) -> String {
    unsafe {
        let mut class = [0u16; 96];
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&class[..len])
    }
}
/// True while the user holds a mouse button or Explorer owns an active
/// context menu, drag capture, or inline rename inside the desktop view.
pub fn explorer_interaction_active() -> bool {
    if mouse_left_down() || mouse_right_down() {
        return true;
    }
    unsafe {
        let shell = GetShellWindow();
        if shell.is_null() {
            return false;
        }
        let mut shell_pid = 0;
        let shell_tid = GetWindowThreadProcessId(shell, &mut shell_pid);
        let mut guiti: GUITHREADINFO = mem::zeroed();
        guiti.cbSize = mem::size_of::<GUITHREADINFO>() as u32;
        if shell_tid != 0 && GetGUIThreadInfo(shell_tid, &mut guiti) != 0 {
            if (guiti.flags & (GUI_INMENUMODE | GUI_POPUPMENUMODE | GUI_SYSTEMMENUMODE)) != 0
                || !guiti.hwndCapture.is_null()
            {
                return true;
            }
            if !guiti.hwndFocus.is_null() && window_class_name(guiti.hwndFocus) == "Edit" {
                return true;
            }
        }
        let mut fg_pid = 0;
        let fg = GetForegroundWindow();
        if !fg.is_null() {
            GetWindowThreadProcessId(fg, &mut fg_pid);
        }
        let mut cursor_pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor_pt);
        struct PopupSearch {
            shell_pid: DWORD,
            fg_pid: DWORD,
            cursor: POINT,
            found: bool,
        }
        unsafe extern "system" fn check_popup(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let state = &mut *(lparam as *mut PopupSearch);
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != state.shell_pid {
                return 1;
            }
            let mut r: RECT = mem::zeroed();
            if GetWindowRect(hwnd, &mut r) == 0 || r.right <= r.left || r.bottom <= r.top {
                return 1;
            }
            let class = window_class_name(hwnd);
            let is_popup = class == "#32768"
                || class == "Xaml_WindowedPopupClass"
                || class.contains("PopupWindowSiteBridge");
            if !is_popup {
                return 1;
            }
            let cursor_inside = state.cursor.x >= r.left
                && state.cursor.x < r.right
                && state.cursor.y >= r.top
                && state.cursor.y < r.bottom;
            if state.fg_pid == state.shell_pid || cursor_inside {
                state.found = true;
                return 0;
            }
            1
        }
        let mut search = PopupSearch {
            shell_pid,
            fg_pid,
            cursor: cursor_pt,
            found: false,
        };
        EnumWindows(Some(check_popup), &mut search as *mut _ as LPARAM);
        search.found
    }
}
/// The DesktopCraft host is clipped to icon shapes by SetWindowRgn.
/// Capture the pointer while dragging a loose icon so MouseMove/MouseUp
/// still reaches us over blank wallpaper, outside the initial icon region.
pub fn capture_pointer(enable: bool) {
    unsafe {
        let hwnd = window();
        if hwnd.is_null() {
            return;
        }
        if enable {
            SetCapture(hwnd);
        } else if GetCapture() == hwnd {
            ReleaseCapture();
        }
    }
}
pub fn cursor() -> (i32, i32) {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        (p.x, p.y)
    }
}
/// True only for Explorer's actual desktop. Explorer folder windows can
/// also contain SysListView32/SHELLDLL_DefView, but their root is CabinetWClass.
pub fn over_explorer_desktop(x: i32, y: i32) -> bool {
    unsafe {
        let mut hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.is_null() {
            return false;
        }
        let mut hit_pid = 0;
        GetWindowThreadProcessId(hwnd, &mut hit_pid);
        if hit_pid == std::process::id() {
            return false;
        }
        while !GetParent(hwnd).is_null() {
            hwnd = GetParent(hwnd);
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), 64);
        matches!(
            String::from_utf16_lossy(&class[..n as usize]).as_str(),
            "Progman" | "WorkerW"
        )
    }
}
/// Whether a drag is over a different application (not a group or the real
/// desktop), so an external Windows OLE drag is actually needed.
pub fn over_other_app(x: i32, y: i32) -> bool {
    if over_explorer_desktop(x, y) {
        return false;
    }
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid != 0 && pid != std::process::id()
    }
}
