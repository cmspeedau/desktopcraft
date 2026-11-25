//! Box bodies painted in the desktop wallpaper layer, BEHIND Explorer's real
//! icons. On Windows 11 24H2+ the layer is a `WorkerW` child of `Progman`
//! below `SHELLDLL_DefView`; older builds use the top-level `WorkerW` after the
//! one that hosts the desktop view. Explorer keeps drawing and owning every
//! icon, so selection, drag and the Windows 11 file menu stay native.
//!
//! The panels live on their own thread: a child window of an Explorer window
//! shares that thread's input queue, and DesktopCraft's UI thread must never
//! do that. No Explorer code, icon, setting or file is changed. Lab evidence:
//! docs/BEHIND_ICONS_LAYER_LAB_20261119.md.

mod wheel_input;
use wheel_input::*;
mod header_rendering;
use header_rendering::*;
mod layer;
mod thread_runtime;
use thread_runtime::*;
mod colors;
use colors::*;
pub use colors::panel_color;
pub use colors::header_color;

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    pub id: u64,
    /// Physical screen pixels.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub rgb: (u8, u8, u8),
    pub alpha: u8,
    pub radius: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderCorners {
    TopRounded,
    BottomRounded,
    AllRounded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderTab {
    /// Header-local physical pixel rect.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub name: String,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderPanel {
    pub id: u64,
    /// Physical screen pixels.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub rgb: (u8, u8, u8),
    pub alpha: u8,
    pub radius: i32,
    pub corners: HeaderCorners,
    pub vertical: bool,
    pub font_px: i32,
    pub title: String,
    pub tabs: Vec<HeaderTab>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackdropFrame {
    pub panels: Vec<Panel>,
    pub headers: Vec<HeaderPanel>,
    pub tints: Vec<Panel>,
}

type Hwnd = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point { x: i32, y: i32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct Size { cx: i32, cy: i32 }
#[repr(C)]
#[derive(Clone, Copy)]
struct BlendFunction {
    blend_op: u8,
    blend_flags: u8,
    source_constant_alpha: u8,
    alpha_format: u8,
}
#[repr(C)]
struct BitmapInfoHeader {
    bi_size: u32,
    bi_width: i32,
    bi_height: i32,
    bi_planes: u16,
    bi_bit_count: u16,
    bi_compression: u32,
    bi_size_image: u32,
    bi_x_pels_per_meter: i32,
    bi_y_pels_per_meter: i32,
    bi_clr_used: u32,
    bi_clr_important: u32,
}
#[repr(C)]
struct BitmapInfo {
    bmi_header: BitmapInfoHeader,
    bmi_colors: [u32; 1],
}
#[repr(C)]
struct MouseLowLevel { pt: Point, mouse_data: u32, flags: u32, time: u32, extra: usize }
#[repr(C)]
#[derive(Default)]
struct Rect { l: i32, t: i32, r: i32, b: i32 }
#[repr(C)]
struct Msg { hwnd: Hwnd, message: u32, wparam: usize, lparam: isize, time: u32, pt: Point }
#[repr(C)]
struct PaintStruct { hdc: *mut c_void, erase: i32, rc: Rect, restore: i32, inc: i32, reserved: [u8; 32] }
#[repr(C)]
struct WndClassExW {
    cb_size: u32, style: u32,
    wnd_proc: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
    cls_extra: i32, wnd_extra: i32, instance: *mut c_void, icon: *mut c_void,
    cursor: *mut c_void, background: *mut c_void, menu_name: *const u16,
    class_name: *const u16, icon_small: *mut c_void,
}
type EnumProc = unsafe extern "system" fn(Hwnd, isize) -> i32;

#[allow(non_snake_case)]
unsafe fn SendMessageTimeoutW(h: Hwnd, m: u32, w: usize, l: isize, flags: u32, ms: u32, out: *mut usize) -> isize { winapi::um::winuser::SendMessageTimeoutW(h.cast(),m,w,l,flags,ms,out) }
#[allow(non_snake_case)]
unsafe fn SetWindowsHookExW(id: i32, f: unsafe extern "system" fn(i32, usize, isize) -> isize, module: *mut c_void, thread: u32) -> *mut c_void { winapi::um::winuser::SetWindowsHookExW(id,Some(f),module.cast(),thread).cast() }
#[allow(non_snake_case)]
unsafe fn WindowFromPoint(p: Point) -> Hwnd { winapi::um::winuser::WindowFromPoint(winapi::shared::windef::POINT{x:p.x,y:p.y}).cast() }
#[allow(non_snake_case)]
unsafe fn InvalidateRect(h:Hwnd,r:*const Rect,erase:i32)->i32 { winapi::um::winuser::InvalidateRect(h.cast(),r.cast(),erase) }
#[allow(non_snake_case)]
unsafe fn GetClientRect(h:Hwnd,r:*mut Rect)->i32 { winapi::um::winuser::GetClientRect(h.cast(),r.cast()) }
#[allow(non_snake_case)]
unsafe fn GetClassNameW(h:Hwnd,s:*mut u16,n:i32)->i32 { winapi::um::winuser::GetClassNameW(h.cast(),s,n) }
#[allow(non_snake_case)]
unsafe fn CreateDIBSection(dc:*mut c_void,bmi:*const BitmapInfo,usage:u32,bits:*mut *mut u8,section:*mut c_void,offset:u32)->*mut c_void { winapi::um::wingdi::CreateDIBSection(dc.cast(),bmi.cast(),usage,bits.cast(),section.cast(),offset).cast() }
#[allow(non_snake_case)]
unsafe fn DwmSetWindowAttribute(hwnd:Hwnd,attr:u32,value:*const c_void,size:u32)->i32 { winapi::um::dwmapi::DwmSetWindowAttribute(hwnd.cast(),attr,value.cast(),size) }

#[link(name = "user32")]
extern "system" {
    fn FindWindowW(class: *const u16, name: *const u16) -> Hwnd;
    fn FindWindowExW(parent: Hwnd, after: Hwnd, class: *const u16, name: *const u16) -> Hwnd;
    fn EnumWindows(f: EnumProc, l: isize) -> i32;

    fn RegisterClassExW(c: *const WndClassExW) -> u16;
    fn CreateWindowExW(ex: u32, class: *const u16, name: *const u16, style: u32, x: i32, y: i32,
        w: i32, h: i32, parent: Hwnd, menu: *mut c_void, inst: *mut c_void, p: *mut c_void) -> Hwnd;
    fn DestroyWindow(h: Hwnd) -> i32;
    fn DefWindowProcW(h: Hwnd, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(m: *mut Msg, h: Hwnd, a: u32, b: u32) -> i32;
    fn TranslateMessage(m: *const Msg) -> i32;
    fn DispatchMessageW(m: *const Msg) -> isize;
    fn PeekMessageW(m: *mut Msg, h: Hwnd, a: u32, b: u32, remove: u32) -> i32;
    fn PostThreadMessageW(thread: u32, m: u32, w: usize, l: isize) -> i32;
    fn SetTimer(h: Hwnd, id: usize, ms: u32, f: *mut c_void) -> usize;
    fn SetLayeredWindowAttributes(h: Hwnd, key: u32, alpha: u8, flags: u32) -> i32;
    fn UpdateLayeredWindow(
        h: Hwnd, dst_dc: *mut c_void, dst_pt: *const Point, size: *const Size,
        src_dc: *mut c_void, src_pt: *const Point, key: u32,
        blend: *const BlendFunction, flags: u32,
    ) -> i32;
    fn GetDC(h: Hwnd) -> *mut c_void;
    fn ReleaseDC(h: Hwnd, dc: *mut c_void) -> i32;
    fn SetWindowPos(h: Hwnd, after: Hwnd, x: i32, y: i32, w: i32, h2: i32, flags: u32) -> i32;
    fn SetWindowRgn(h: Hwnd, rgn: *mut c_void, redraw: i32) -> i32;
    fn SetWindowLongPtrW(h: Hwnd, i: i32, v: isize) -> isize;
    fn GetWindowLongPtrW(h: Hwnd, i: i32) -> isize;

    fn BeginPaint(h: Hwnd, p: *mut PaintStruct) -> *mut c_void;
    fn EndPaint(h: Hwnd, p: *const PaintStruct) -> i32;

    fn FillRect(dc: *mut c_void, r: *const Rect, brush: *mut c_void) -> i32;
    fn FrameRect(dc: *mut c_void, r: *const Rect, brush: *mut c_void) -> i32;
    fn DrawTextW(dc: *mut c_void, text: *const u16, count: i32, r: *mut Rect, format: u32) -> i32;
    fn ScreenToClient(h: Hwnd, p: *mut Point) -> i32;
    fn IsWindow(h: Hwnd) -> i32;
    fn IsIconic(h: Hwnd) -> i32;
    fn IsWindowVisible(h: Hwnd) -> i32;
    fn ShowWindow(h: Hwnd, cmd: i32) -> i32;
    fn GetShellWindow() -> Hwnd;
    fn SetThreadDpiAwarenessContext(ctx: isize) -> isize;

    fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
    fn CallNextHookEx(hook: *mut c_void, code: i32, w: usize, l: isize) -> isize;

    fn GetAncestor(h: Hwnd, flags: u32) -> Hwnd;

    fn GetAsyncKeyState(key: i32) -> i16;
    fn GetCursorPos(pt: *mut Point) -> i32;
    fn RegisterRawInputDevices(devices: *const RawInputDevice, num: u32, size: u32) -> i32;
    fn GetRawInputData(
        hrawinput: *mut c_void, cmd: u32, data: *mut c_void, size: *mut u32, header_size: u32,
    ) -> u32;
    fn GetRawInputDeviceInfoW(device: *mut c_void, cmd: u32, data: *mut c_void, size: *mut u32) -> u32;
}
#[link(name = "hid")]
extern "system" {
    fn HidP_GetCaps(preparsed: *const c_void, caps: *mut HidpCaps) -> i32;
    fn HidP_GetValueCaps(
        report_type: u32, value_caps: *mut HidpValueCaps, length: *mut u16, preparsed: *const c_void,
    ) -> i32;
    fn HidP_GetUsages(
        report_type: u32, usage_page: u16, link_collection: u16,
        usage_list: *mut u16, usage_length: *mut u32,
        preparsed: *const c_void, report: *const u8, report_len: u32,
    ) -> i32;
    fn HidP_GetUsageValue(
        report_type: u32, usage_page: u16, link_collection: u16, usage: u16,
        usage_value: *mut u32, preparsed: *const c_void, report: *const u8, report_len: u32,
    ) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn CreateSolidBrush(color: u32) -> *mut c_void;
    fn CreateRoundRectRgn(l: i32, t: i32, r: i32, b: i32, w: i32, h: i32) -> *mut c_void;
    fn DeleteObject(o: *mut c_void) -> i32;
    fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
    fn DeleteDC(dc: *mut c_void) -> i32;

    fn SetBkMode(dc: *mut c_void, mode: i32) -> i32;
    fn SetTextColor(dc: *mut c_void, color: u32) -> u32;
    fn SetTextAlign(dc: *mut c_void, align: u32) -> u32;
    fn SelectObject(dc: *mut c_void, obj: *mut c_void) -> *mut c_void;
    fn ExtTextOutW(
        dc: *mut c_void, x: i32, y: i32, options: u32, r: *const Rect,
        text: *const u16, count: u32, dx: *const i32,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn CreateFontW(
        height: i32, width: i32, escapement: i32, orientation: i32, weight: i32,
        italic: u32, underline: u32, strikeout: u32, charset: u32,
        out_prec: u32, clip_prec: u32, quality: u32, pitch: u32, face: *const u16,
    ) -> *mut c_void;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetCurrentThreadId() -> u32;
    fn GetLastError() -> u32;
}

const WM_QUIT: u32 = 0x0012;
const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_NCDESTROY: u32 = 0x0082;
const WM_NCHITTEST: u32 = 0x0084;
const WM_TIMER: u32 = 0x0113;
const WM_APP_PANELS: u32 = 0x8000 + 0x2C1;
const GWLP_USERDATA: i32 = -21;
const HTTRANSPARENT: isize = -1;
const TRANSPARENT: i32 = 1;
const DT_CENTER: u32 = 0x0001;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_NOPREFIX: u32 = 0x0800;
const DT_END_ELLIPSIS: u32 = 0x8000;
const FW_SEMIBOLD: i32 = 600;
const DEFAULT_CHARSET: u32 = 1;
const ANTIALIASED_QUALITY: u32 = 4;
const TA_CENTER: u32 = 0x0006;
const TA_BASELINE: u32 = 0x0018;
const ETO_CLIPPED: u32 = 0x0004;
const WS_CHILD: u32 = 0x4000_0000;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const LWA_ALPHA: u32 = 0x2;
const ULW_ALPHA: u32 = 0x0000_0002;
const AC_SRC_OVER: u8 = 0x00;
const AC_SRC_ALPHA: u8 = 0x01;
const SW_SHOWNOACTIVATE: i32 = 4;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_SHOWWINDOW: u32 = 0x0040;
const DWMWA_NCRENDERING_POLICY: u32 = 2;
const DWMNCRP_DISABLED: u32 = 1;
const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWCP_DONOTROUND: u32 = 1;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
const HWND_BOTTOM: Hwnd = 1 as Hwnd;
const SMTO_NORMAL: u32 = 0;
const PER_MONITOR_AWARE_V2: isize = -4;
const CLASS: &str = "DesktopCraftBoxBackdrop";
const HEADER_CLASS: &str = "DesktopCraftBoxHeaderMirror";

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn colorref(rgb: (u8, u8, u8)) -> u32 {
    rgb.0 as u32 | (rgb.1 as u32) << 8 | (rgb.2 as u32) << 16
}

const WH_MOUSE_LL: i32 = 14;
const WM_MOUSEWHEEL: usize = 0x020A;
const WM_INPUT: u32 = 0x00FF;
const GA_ROOT: u32 = 2;
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;
const HWND_MESSAGE: Hwnd = (-3isize) as Hwnd;
const RIDEV_INPUTSINK: u32 = 0x0000_0100;
const RID_INPUT: u32 = 0x1000_0003;
const RIDI_PREPARSEDDATA: u32 = 0x2000_0005;
const RIM_TYPEHID: u32 = 2;
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
const HID_USAGE_GENERIC_X: u16 = 0x30;
const HID_USAGE_GENERIC_Y: u16 = 0x31;
const HID_USAGE_PAGE_DIGITIZER: u16 = 0x0D;
const HID_USAGE_DIGITIZER_TOUCH_PAD: u16 = 0x05;
const HID_USAGE_DIGITIZER_TIP_SWITCH: u16 = 0x42;
const HIDP_INPUT: u32 = 0;
const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;
const TOUCHPAD_CLASS: &str = "DesktopCraftTouchpadSink";

#[repr(C)]
struct RawInputDevice {
    usage_page: u16,
    usage: u16,
    flags: u32,
    hwnd_target: Hwnd,
}
#[repr(C)]
struct RawInputHeader {
    dw_type: u32,
    dw_size: u32,
    h_device: *mut c_void,
    w_param: usize,
}
#[repr(C)]
struct HidpCaps {
    usage: u16,
    usage_page: u16,
    input_report_byte_length: u16,
    output_report_byte_length: u16,
    feature_report_byte_length: u16,
    reserved: [u16; 17],
    number_link_collection_nodes: u16,
    number_input_button_caps: u16,
    number_input_value_caps: u16,
    number_input_data_indices: u16,
    number_output_button_caps: u16,
    number_output_value_caps: u16,
    number_output_data_indices: u16,
    number_feature_button_caps: u16,
    number_feature_value_caps: u16,
    number_feature_data_indices: u16,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct HidpValueCaps {
    usage_page: u16,
    report_id: u8,
    is_alias: u8,
    bit_field: u16,
    link_collection: u16,
    link_usage: u16,
    link_usage_page: u16,
    is_range: u8,
    is_string_range: u8,
    is_designator_range: u8,
    is_absolute: u8,
    has_null: u8,
    reserved: u8,
    bit_size: u16,
    report_count: u16,
    reserved2: [u16; 5],
    units_exp: u32,
    units: u32,
    logical_min: i32,
    logical_max: i32,
    physical_min: i32,
    physical_max: i32,
    usage_min_or_usage: u16,
    usage_max_or_reserved1: u16,
    string_min_or_string_index: u16,
    string_max_or_reserved2: u16,
    designator_min_or_designator_index: u16,
    designator_max_or_reserved3: u16,
    data_index_min_or_data_index: u16,
    data_index_max_or_reserved4: u16,
}

#[derive(Clone, Copy)]
struct ContactAxis {
    link_collection: u16,
    x_min: i32,
    x_max: i32,
    y_min: i32,
    y_max: i32,
}

struct TouchpadDevice {
    preparsed: Vec<u8>,
    contacts: Vec<ContactAxis>,
}

/// Tracks 2-finger vertical scrolling on a Windows Precision Touchpad.
#[derive(Default)]
struct TouchpadScrollState {
    last_two_finger: Option<(f32, f32, std::time::Instant)>,
}

impl TouchpadScrollState {
    fn update(
        &mut self,
        active_contacts: &[(f32, f32)],
        now: std::time::Instant,
        natural_scroll: bool,
    ) -> Option<i32> {
        if active_contacts.len() != 2 {
            self.last_two_finger = None;
            return None;
        }
        let avg_x = (active_contacts[0].0 + active_contacts[1].0) * 0.5;
        let avg_y = (active_contacts[0].1 + active_contacts[1].1) * 0.5;
        let prev = self.last_two_finger.replace((avg_x, avg_y, now));
        let (px, py, pt) = prev?;
        if now.saturating_duration_since(pt) > std::time::Duration::from_millis(250) {
            return None;
        }
        let dx = avg_x - px;
        let dy = avg_y - py;
        // Require vertical dominance and reject sub-pixel sensor jitter.
        if dy.abs() < 0.0005 || dy.abs() < dx.abs() * 0.75 {
            return None;
        }
        // ~3.5% of touchpad height = 1 wheel notch (120 units).
        let raw_delta = (dy / 0.035 * 120.0).round() as i32;
        if raw_delta == 0 {
            return None;
        }
        Some(if natural_scroll { raw_delta } else { -raw_delta })
    }
}


struct WheelState {
    panels: Vec<Panel>,
    tx: Sender<(u64, i32)>,
    last_mouse_wheel: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    touchpad_devices: BTreeMap<usize, Option<TouchpadDevice>>,
    touchpad_scroll: TouchpadScrollState,
    natural_scroll: bool,
    natural_scroll_checked: std::time::Instant,
}
std::thread_local! {
    static WHEEL: std::cell::RefCell<Option<WheelState>> = const { std::cell::RefCell::new(None) };
}



/// The window Box panels are parented to, or null if Explorer has none.
unsafe fn wallpaper_layer() -> Hwnd {
    let progman = FindWindowW(wide("Progman").as_ptr(), std::ptr::null());
    if progman.is_null() { return progman; }
    let mut result = 0usize;
    // Ask Explorer for the wallpaper WorkerW, as wallpaper programs do.
    SendMessageTimeoutW(progman, 0x052C, 0xD, 0x1, SMTO_NORMAL, 1000, &mut result);
    SendMessageTimeoutW(progman, 0x052C, 0, 0, SMTO_NORMAL, 1000, &mut result);
    let worker = wide("WorkerW");
    let child = FindWindowExW(progman, std::ptr::null_mut(), worker.as_ptr(), std::ptr::null());
    if !child.is_null() && !FindWindowExW(progman, std::ptr::null_mut(),
        wide("SHELLDLL_DefView").as_ptr(), std::ptr::null()).is_null() {
        return child; // 24H2+: Progman > [SHELLDLL_DefView, WorkerW]
    }
    unsafe extern "system" fn find(h: Hwnd, out: isize) -> i32 {
        let view = FindWindowExW(h, std::ptr::null_mut(),
            wide("SHELLDLL_DefView").as_ptr(), std::ptr::null());
        if view.is_null() { return 1; }
        *(out as *mut Hwnd) = FindWindowExW(std::ptr::null_mut(), h,
            wide("WorkerW").as_ptr(), std::ptr::null());
        0
    }
    let mut legacy: Hwnd = std::ptr::null_mut();
    EnumWindows(find, &mut legacy as *mut Hwnd as isize);
    legacy
}

struct Layer {
    parent: Hwnd,
    windows: BTreeMap<u64, (Hwnd, Panel)>,
    headers: BTreeMap<u64, (Hwnd, HeaderPanel)>,
    tints: BTreeMap<u64, (Hwnd, Panel)>,
    instance: *mut c_void,
    class: Vec<u16>,
    header_class: Vec<u16>,
    create_failure_logged: bool,
}



/// Owner of the backdrop thread. Dropping it removes every panel.
pub struct Backdrop {
    tx: Sender<BackdropFrame>,
    wheel: Receiver<(u64, i32)>,
    thread: u32,
    join: Option<JoinHandle<()>>,
    last: BackdropFrame,
}

impl Backdrop {
    pub fn start() -> Option<Self> {
        let (tx, rx) = channel();
        let (ready_tx, ready_rx) = channel();
        let (wheel_tx, wheel) = channel();
        let join = std::thread::Builder::new().name("desktopcraft-backdrop".into())
            .spawn(move || run(rx, ready_tx, wheel_tx)).ok()?;
        let thread = ready_rx.recv_timeout(std::time::Duration::from_secs(2)).ok()?;
        Some(Self { tx, wheel, thread, join: Some(join), last: BackdropFrame::default() })
    }

    /// Wheel notches over Box bodies since the last call: (Box id, delta).
    pub fn take_wheel(&self) -> Vec<(u64, i32)> {
        self.wheel.try_iter().collect()
    }

    /// Sends the panel, mirrored header, and click-through tint lists only when changed.
    pub fn sync(&mut self, panels: Vec<Panel>, headers: Vec<HeaderPanel>, tints: Vec<Panel>) {
        let frame = BackdropFrame { panels, headers, tints };
        if frame == self.last { return; }
        self.last = frame.clone();
        if self.tx.send(frame).is_ok() {
            unsafe { PostThreadMessageW(self.thread, WM_APP_PANELS, 0, 0); }
        }
    }
}

impl Drop for Backdrop {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.thread, WM_QUIT, 0, 0); }
        if let Some(join) = self.join.take() { let _ = join.join(); }
    }
}


#[cfg(test)]
mod tests;
