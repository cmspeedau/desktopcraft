//! Dedicated Win32 top-level floating host window for DesktopCraft Settings.
//! Keeps Settings independent from the desktop-layer `host` window so open
//! Settings never lifts desktop Boxes above apps or degrades desktop chrome.

mod backing_dib;
mod input;

pub(crate) use backing_dib::rgba_to_bgrx;
use backing_dib::BackingDib;
use input::settings_wnd_proc;

use crate::Input;
use macroquad::prelude::*;
use std::ptr;
use winapi::ctypes::c_void;
use winapi::shared::minwindef::{DWORD, HINSTANCE, LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBITMAP, HDC, HICON, HWND, POINT, RECT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, CreateRoundRectRgn, DeleteDC, DeleteObject,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use winapi::um::winuser::{
    BeginPaint, CreateWindowExW, DefWindowProcW, DestroyWindow, EndPaint, GetAsyncKeyState,
    GetCapture, GetClientRect, GetCursorPos, GetDC, GetForegroundWindow, GetKeyState,
    GetWindowLongPtrW, IsIconic, IsWindow, IsWindowVisible, LoadCursorW, LoadIconW,
    RegisterClassExW, ReleaseCapture, ReleaseDC, ScreenToClient, SetCapture, SetCursor, SetFocus,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowRgn, ShowWindow,
    WindowFromPoint, GWLP_USERDATA, HTCLIENT, IDC_ARROW, IDI_APPLICATION, PAINTSTRUCT,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_MINIMIZE, SW_RESTORE,
    SW_SHOWNORMAL, SW_SHOWNOACTIVATE, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_F1, VK_F10, VK_F12, VK_F2, VK_F4, VK_HOME, VK_LEFT, VK_LBUTTON, VK_MENU,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, WM_CAPTURECHANGED, WM_CHAR,
    WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEWHEEL, WM_NCDESTROY, WM_PAINT, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN, WNDCLASSEXW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WS_EX_APPWINDOW, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
};

pub(crate) const SETTINGS_LOGICAL_W: f32 = 870.0;
pub(crate) const SETTINGS_LOGICAL_H: f32 = 650.0;

const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_ROUND: u32 = 2;
const DWMWA_BORDER_COLOR: u32 = 34;
const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attr: u32, value: *const c_void, size: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> DWORD;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}



struct WindowInputState {
    left_pressed: bool,
    left_released: bool,
    left_down: bool,
    right_pressed: bool,
    right_released: bool,
    right_down: bool,
    middle_pressed: bool,
    middle_released: bool,
    middle_down: bool,
    scroll: f32,
    keys: Vec<KeyCode>,
    repeats: Vec<KeyCode>,
    typed: String,
    high_surrogate: Option<u16>,
    close_requested: bool,
    paint_dc: HDC,
    paint_w: i32,
    paint_h: i32,
    paint_ready: bool,
}

impl WindowInputState {
    fn new() -> Self {
        Self {
            left_pressed: false,
            left_released: false,
            left_down: false,
            right_pressed: false,
            right_released: false,
            right_down: false,
            middle_pressed: false,
            middle_released: false,
            middle_down: false,
            scroll: 0.0,
            keys: Vec::new(),
            repeats: Vec::new(),
            typed: String::new(),
            high_surrogate: None,
            close_requested: false,
            paint_dc: ptr::null_mut(),
            paint_w: 0,
            paint_h: 0,
            paint_ready: false,
        }
    }

    fn clear_queued(&mut self) {
        self.left_pressed = false;
        self.left_released = false;
        self.left_down = false;
        self.right_pressed = false;
        self.right_released = false;
        self.right_down = false;
        self.middle_pressed = false;
        self.middle_released = false;
        self.middle_down = false;
        self.scroll = 0.0;
        self.keys.clear();
        self.repeats.clear();
        self.typed.clear();
        self.high_surrogate = None;
        self.close_requested = false;
    }

    fn push_utf16_unit(&mut self, unit: u16) {
        if (0xD800..=0xDBFF).contains(&unit) {
            self.high_surrogate = Some(unit);
            return;
        }
        let ch = if (0xDC00..=0xDFFF).contains(&unit) {
            self.high_surrogate.take().and_then(|hi| {
                let cp = 0x10000 + (((u32::from(hi) - 0xD800) << 10) | (u32::from(unit) - 0xDC00));
                char::from_u32(cp)
            })
        } else {
            self.high_surrogate = None;
            char::from_u32(u32::from(unit))
        };
        if let Some(c) = ch.filter(|c| !c.is_control()) {
            self.typed.push(c);
        }
    }
}



pub(crate) struct SettingsWindow {
    hwnd: HWND,
    dib: BackingDib,
    rt: RenderTarget,
    rgba_buf: Vec<u8>,
    dwm_rounded: bool,
    last_pos: (i32, i32),
    last_size: (i32, i32),
    rendered_once: bool,
    visible: bool,
}

impl SettingsWindow {
    unsafe fn state_mut(&mut self) -> Option<&mut WindowInputState> {
        if self.hwnd.is_null() || IsWindow(self.hwnd) == 0 {
            return None;
        }
        let ptr = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut WindowInputState;
        ptr.as_mut()
    }

    pub fn create(x: i32, y: i32, pw: i32, ph: i32) -> Option<Self> {
        let pw = pw.max(320);
        let ph = ph.max(240);
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(ptr::null());
            let class_name = wide("DesktopCraftSettingsWindow");
            let icon: HICON = {
                let embedded = LoadIconW(instance, 1usize as *const u16);
                if embedded.is_null() {
                    LoadIconW(ptr::null_mut(), IDI_APPLICATION)
                } else {
                    embedded
                }
            };
            let cursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(settings_wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: icon,
                hCursor: cursor,
                hbrBackground: ptr::null_mut(),
                lpszMenuName: ptr::null(),
                lpszClassName: class_name.as_ptr(),
                hIconSm: icon,
            };
            if RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return None;
            }
            let title = wide("DesktopCraft Settings");
            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                x,
                y,
                pw,
                ph,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null_mut(),
            );
            if hwnd.is_null() {
                return None;
            }
            let state_box = Box::new(WindowInputState::new());
            let state_ptr = Box::into_raw(state_box);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            let corner_pref: u32 = DWMWCP_ROUND;
            let dwm_rounded = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            ) == 0;
            let border_color: u32 = 0x004A3828; // BGR COLORREF dark slate border
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &border_color as *const u32 as *const c_void,
                std::mem::size_of::<u32>() as u32,
            );
            if !dwm_rounded {
                apply_fallback_round_rgn(hwnd, pw, ph);
            }
            let Some(dib) = BackingDib::create(hwnd, pw, ph) else {
                DestroyWindow(hwnd);
                return None;
            };
            (*state_ptr).paint_dc = dib.mem_dc;
            (*state_ptr).paint_w = pw;
            (*state_ptr).paint_h = ph;
            let rt = render_target(pw as u32, ph as u32);
            let byte_len = pw as usize * ph as usize * 4;
            Some(Self {
                hwnd,
                dib,
                rt,
                rgba_buf: vec![0u8; byte_len],
                dwm_rounded,
                last_pos: (x, y),
                last_size: (pw, ph),
                rendered_once: false,
                visible: false,
            })
        }
    }

    pub fn dwm_rounded(&self) -> bool {
        self.dwm_rounded
    }

    pub fn has_rendered_once(&self) -> bool {
        self.rendered_once
    }

    pub fn ensure_size(&mut self, pw: i32, ph: i32) {
        let pw = pw.max(320);
        let ph = ph.max(240);
        if self.last_size == (pw, ph) {
            return;
        }
        unsafe {
            if self.hwnd.is_null() || IsWindow(self.hwnd) == 0 {
                return;
            }
            let Some(new_dib) = BackingDib::create(self.hwnd, pw, ph) else {
                return;
            };
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                0,
                0,
                pw,
                ph,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            if !self.dwm_rounded {
                apply_fallback_round_rgn(self.hwnd, pw, ph);
            }
            self.dib = new_dib;
            let mem_dc = self.dib.mem_dc;
            if let Some(s) = self.state_mut() {
                s.paint_dc = mem_dc;
                s.paint_w = pw;
                s.paint_h = ph;
                s.paint_ready = false;
            }
            self.rt = render_target(pw as u32, ph as u32);
            self.rgba_buf.resize(pw as usize * ph as usize * 4, 0);
            self.last_size = (pw, ph);
            self.rendered_once = false;
        }
    }

    pub fn move_to(&mut self, x: i32, y: i32) {
        if self.last_pos == (x, y) {
            return;
        }
        self.last_pos = (x, y);
        unsafe {
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    pub fn show(&mut self, activate: bool) {
        unsafe {
            if IsIconic(self.hwnd) != 0 {
                if activate {
                    ShowWindow(self.hwnd, SW_RESTORE);
                    SetForegroundWindow(self.hwnd);
                }
                self.visible = IsWindowVisible(self.hwnd) != 0;
                return;
            }
            if !self.visible || IsWindowVisible(self.hwnd) == 0 {
                ShowWindow(
                    self.hwnd,
                    if activate { SW_SHOWNORMAL } else { SW_SHOWNOACTIVATE },
                );
                self.visible = true;
            }
            if activate {
                SetForegroundWindow(self.hwnd);
            }
        }
    }

    pub fn hide(&mut self) {
        unsafe {
            if self.hwnd.is_null() || IsWindow(self.hwnd) == 0 {
                self.visible = false;
                return;
            }
            if GetCapture() == self.hwnd {
                ReleaseCapture();
            }
            if self.visible || IsWindowVisible(self.hwnd) != 0 {
                ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
            if let Some(s) = self.state_mut() {
                s.clear_queued();
            }
        }
    }

    pub fn minimize(&mut self) {
        unsafe {
            if GetCapture() == self.hwnd {
                ReleaseCapture();
            }
            ShowWindow(self.hwnd, SW_MINIMIZE);
            self.visible = IsWindowVisible(self.hwnd) != 0;
        }
    }

    pub fn is_visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd) != 0 && IsIconic(self.hwnd) == 0 }
    }

    pub fn is_iconic(&self) -> bool {
        unsafe { IsIconic(self.hwnd) != 0 }
    }

    pub fn has_focus(&self) -> bool {
        unsafe { !self.hwnd.is_null() && GetForegroundWindow() == self.hwnd }
    }

    pub fn is_point_over_window(&self, sx: i32, sy: i32) -> bool {
        unsafe {
            !self.hwnd.is_null()
                && IsWindow(self.hwnd) != 0
                && IsWindowVisible(self.hwnd) != 0
                && IsIconic(self.hwnd) == 0
                && WindowFromPoint(POINT { x: sx, y: sy }) == self.hwnd
        }
    }


    pub fn render_target(&self) -> RenderTarget {
        self.rt.clone()
    }

    pub fn present_from_render_target(&mut self) {
        let byte_len = self.dib.pw as usize * self.dib.ph as usize * 4;
        if self.rgba_buf.len() != byte_len {
            self.rgba_buf.resize(byte_len, 0);
        }
        unsafe {
            if self.hwnd.is_null() || IsWindow(self.hwnd) == 0 {
                return;
            }
            get_internal_gl()
                .quad_context
                .texture_read_pixels(self.rt.texture.raw_miniquad_id(), &mut self.rgba_buf);
            let dst = std::slice::from_raw_parts_mut(self.dib.bits, byte_len);
            rgba_to_bgrx(&self.rgba_buf, dst);
            let pw = self.dib.pw;
            let ph = self.dib.ph;
            let mem_dc = self.dib.mem_dc;
            let hwnd = self.hwnd;
            if let Some(s) = self.state_mut() {
                s.paint_ready = true;
            }
            let dc = GetDC(hwnd);
            if !dc.is_null() {
                BitBlt(dc, 0, 0, pw, ph, mem_dc, 0, 0, SRCCOPY);
                ReleaseDC(hwnd, dc);
            }
        }
        self.rendered_once = true;
    }
}

unsafe fn apply_fallback_round_rgn(hwnd: HWND, pw: i32, ph: i32) {
    let r = (20.0 * (pw as f32 / SETTINGS_LOGICAL_W)).round().max(12.0) as i32;
    let rgn = CreateRoundRectRgn(0, 0, pw + 1, ph + 1, r, r);
    if !rgn.is_null() && SetWindowRgn(hwnd, rgn, 1) == 0 {
        DeleteObject(rgn as *mut c_void);
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.is_null() && IsWindow(self.hwnd) != 0 {
                if GetCapture() == self.hwnd {
                    ReleaseCapture();
                }
                if let Some(s) = self.state_mut() {
                    s.paint_dc = ptr::null_mut();
                    s.paint_ready = false;
                }
                DestroyWindow(self.hwnd);
            }
            self.hwnd = ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_bgrx_swaps_red_blue_and_forces_opaque_x_byte() {
        let rgba = [12u8, 34, 56, 78, 250, 10, 20, 0];
        let mut out = [0u8; 8];
        rgba_to_bgrx(&rgba, &mut out);
        assert_eq!(out, [56, 34, 12, 255, 20, 10, 250, 255]);
    }

    #[test]
    fn surrogate_pairs_and_thai_chars_assemble_without_control_leak() {
        let mut state = WindowInputState::new();
        state.push_utf16_unit('ก' as u16);
        state.push_utf16_unit('ิ' as u16);
        state.push_utf16_unit(0x0008); // backspace control ignored in typed text
        state.push_utf16_unit(0xD83D);
        state.push_utf16_unit(0xDE00);
        assert_eq!(state.typed, "กิ😀");
    }
}
