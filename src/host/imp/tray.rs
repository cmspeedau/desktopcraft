use super::*;

// ---- Windows System Tray notification icon ----
const WM_APP_TRAY: u32 = WM_APP + 0x55;
const TRAY_ICON_ID: u32 = 1;
const NIM_ADD: u32 = 0x0000_0000;
const NIM_MODIFY: u32 = 0x0000_0001;
const NIM_DELETE: u32 = 0x0000_0002;
const NIF_MESSAGE: u32 = 0x0000_0001;
const NIF_ICON: u32 = 0x0000_0002;
const NIF_TIP: u32 = 0x0000_0004;

#[repr(C)]
struct NotifyIconDataW {
    cb_size: u32,
    h_wnd: HWND,
    u_id: u32,
    u_flags: u32,
    u_callback_message: u32,
    h_icon: winapi::shared::windef::HICON,
    sz_tip: [u16; 128],
    dw_state: u32,
    dw_state_mask: u32,
    sz_info: [u16; 256],
    u_timeout_or_version: u32,
    sz_info_title: [u16; 64],
    dw_info_flags: u32,
    guid_item: winapi::shared::guiddef::GUID,
    h_balloon_icon: winapi::shared::windef::HICON,
}

#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(message: u32, data: *mut NotifyIconDataW) -> BOOL;
}

static TRAY_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static TRAY_ICON: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static TRAY_ADDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static TRAY_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static TRAY_LAST_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static TRAY_ACTION: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static TRAY_TASKBAR_MSG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe fn load_app_tray_icon() -> winapi::shared::windef::HICON {
    let instance = winapi::um::libloaderapi::GetModuleHandleW(ptr::null());
    let cx = GetSystemMetrics(SM_CXSMICON).max(16);
    let cy = GetSystemMetrics(SM_CYSMICON).max(16);
    let small = LoadImageW(
        instance,
        1usize as *const u16,
        IMAGE_ICON,
        cx,
        cy,
        LR_DEFAULTCOLOR | LR_SHARED,
    ) as winapi::shared::windef::HICON;
    if !small.is_null() {
        return small;
    }
    let embedded = LoadIconW(instance, 1usize as *const u16);
    if !embedded.is_null() {
        return embedded;
    }
    LoadIconW(ptr::null_mut(), IDI_APPLICATION)
}

unsafe fn fill_tray_nid(hwnd: HWND, enabled: bool) -> NotifyIconDataW {
    let mut nid: NotifyIconDataW = mem::zeroed();
    nid.cb_size = mem::size_of::<NotifyIconDataW>() as u32;
    nid.h_wnd = hwnd;
    nid.u_id = TRAY_ICON_ID;
    nid.u_flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.u_callback_message = WM_APP_TRAY;
    let mut icon = TRAY_ICON.load(std::sync::atomic::Ordering::SeqCst) as winapi::shared::windef::HICON;
    if icon.is_null() {
        icon = load_app_tray_icon();
        TRAY_ICON.store(icon as usize, std::sync::atomic::Ordering::SeqCst);
    }
    nid.h_icon = icon;
    let tip = if enabled {
        "DesktopCraft — Enabled (Right-click for options)"
    } else {
        "DesktopCraft — Disabled (Right-click to enable)"
    };
    for (dst, src) in nid.sz_tip.iter_mut().take(127).zip(tip.encode_utf16()) {
        *dst = src;
    }
    nid
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if msg == WM_APP_TRAY {
        let event = (lparam as u32) & 0xFFFF;
        if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
            TRAY_ACTION.store(2, std::sync::atomic::Ordering::SeqCst);
        } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            let menu = CreatePopupMenu();
            if !menu.is_null() {
                let enabled = TRAY_ENABLED.load(std::sync::atomic::Ordering::SeqCst);
                let toggle_label = if enabled {
                    "Disable DesktopCraft"
                } else {
                    "Enable DesktopCraft"
                };
                AppendMenuW(menu, MF_STRING, 1, wide(toggle_label).as_ptr());
                AppendMenuW(menu, MF_STRING, 2, wide("DesktopCraft settings…").as_ptr());
                AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
                AppendMenuW(menu, MF_STRING, 3, wide("Exit DesktopCraft").as_ptr());
                SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    ptr::null(),
                );
                PostMessageW(hwnd, WM_NULL, 0, 0);
                DestroyMenu(menu);
                if matches!(cmd, 1..=3) {
                    TRAY_ACTION.store(cmd as u8, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
        return 0;
    }
    let taskbar_msg = TRAY_TASKBAR_MSG.load(std::sync::atomic::Ordering::Relaxed);
    if taskbar_msg != 0 && msg == taskbar_msg {
        let enabled = TRAY_ENABLED.load(std::sync::atomic::Ordering::SeqCst);
        let mut nid = fill_tray_nid(hwnd, enabled);
        Shell_NotifyIconW(NIM_ADD, &mut nid);
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn sync_tray(enabled: bool) -> Option<TrayAction> {
    TRAY_ENABLED.store(enabled, std::sync::atomic::Ordering::SeqCst);
    unsafe {
        let mut hwnd = TRAY_HWND.load(std::sync::atomic::Ordering::SeqCst) as HWND;
        if hwnd.is_null() || IsWindow(hwnd) == 0 {
            let instance = winapi::um::libloaderapi::GetModuleHandleW(ptr::null());
            let class = wide("DesktopCraftTrayHost");
            let mut wc: WNDCLASSEXW = mem::zeroed();
            wc.cbSize = mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(tray_wnd_proc);
            wc.hInstance = instance;
            wc.lpszClassName = class.as_ptr();
            RegisterClassExW(&wc);
            let tb_msg = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
            TRAY_TASKBAR_MSG.store(tb_msg, std::sync::atomic::Ordering::Relaxed);
            hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                wide("DesktopCraftTray").as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null_mut(),
            );
            if !hwnd.is_null() {
                TRAY_HWND.store(hwnd as usize, std::sync::atomic::Ordering::SeqCst);
                TRAY_ADDED.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        if !hwnd.is_null() {
            if !TRAY_ADDED.load(std::sync::atomic::Ordering::SeqCst) {
                let mut nid = fill_tray_nid(hwnd, enabled);
                if Shell_NotifyIconW(NIM_ADD, &mut nid) != 0 {
                    TRAY_ADDED.store(true, std::sync::atomic::Ordering::SeqCst);
                    TRAY_LAST_ENABLED.store(enabled, std::sync::atomic::Ordering::SeqCst);
                }
            } else if TRAY_LAST_ENABLED.swap(enabled, std::sync::atomic::Ordering::SeqCst) != enabled {
                let mut nid = fill_tray_nid(hwnd, enabled);
                Shell_NotifyIconW(NIM_MODIFY, &mut nid);
            }
        }
    }
    match TRAY_ACTION.swap(0, std::sync::atomic::Ordering::SeqCst) {
        1 => Some(TrayAction::ToggleEnabled),
        2 => Some(TrayAction::OpenSettings),
        3 => Some(TrayAction::Exit),
        _ => None,
    }
}

pub fn remove_tray() {
    unsafe {
        let hwnd = TRAY_HWND.swap(0, std::sync::atomic::Ordering::SeqCst) as HWND;
        if TRAY_ADDED.swap(false, std::sync::atomic::Ordering::SeqCst) && !hwnd.is_null() {
            let mut nid: NotifyIconDataW = mem::zeroed();
            nid.cb_size = mem::size_of::<NotifyIconDataW>() as u32;
            nid.h_wnd = hwnd;
            nid.u_id = TRAY_ICON_ID;
            Shell_NotifyIconW(NIM_DELETE, &mut nid);
        }
        if !hwnd.is_null() && IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
    }
}
