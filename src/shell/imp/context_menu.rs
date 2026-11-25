use super::*;

/// Classic IContextMenu fallback for a DesktopCraft-drawn item.
pub fn classic_context_menu(path: &str, x: i32, y: i32, can_unpin: bool) -> MenuChoice {
    const FIRST_SHELL_COMMAND: u32 = 1;
    const LAST_SHELL_COMMAND: u32 = 0x7FFF;
    const REVEAL_COMMAND: u32 = 0x8000;
    const UNPIN_COMMAND: u32 = 0x8001;
    unsafe {
        // Keep the parent and PIDL alive through command invocation.
        let item = parent_and_child(path);
        let cm = item.as_ref().map_or_else(Com::null, |(folder, child, _)| {
            let get_ui: extern "system" fn(
                Ptr, HWND, u32, *const Ptr, *const GUID, *mut u32, *mut Ptr,
            ) -> HRESULT = mem::transmute(vfn(folder.0, 10));
            let mut context = ptr::null_mut();
            let result = get_ui(
                folder.0, crate::host::window(), 1, child, &IID_ICONTEXT_MENU,
                ptr::null_mut(), &mut context,
            );
            if result >= 0 { Com(context) } else { Com::null() }
        });
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return MenuChoice::Nothing;
        }
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide("Classic Windows menu").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
        if cm.ok() {
            let query: extern "system" fn(Ptr, HMENU, u32, u32, u32, u32) -> HRESULT =
                mem::transmute(vfn(cm.0, 3));
            query(cm.0, menu, 2, FIRST_SHELL_COMMAND, LAST_SHELL_COMMAND, 0x10 /* CMF_CANRENAME */);
            AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
        }
        AppendMenuW(menu, MF_STRING, REVEAL_COMMAND as usize,
            wide("Show in File Explorer").as_ptr());
        if can_unpin {
            AppendMenuW(menu, MF_STRING, UNPIN_COMMAND as usize,
                wide("Unpin from Box (keep file)").as_ptr());
        }
        let cmd = track(&cm, menu, x, y);
        DestroyMenu(menu);
        match cmd {
            REVEAL_COMMAND => return MenuChoice::Reveal,
            UNPIN_COMMAND if can_unpin => return MenuChoice::Unpin,
            FIRST_SHELL_COMMAND..=LAST_SHELL_COMMAND if cm.ok() => {}
            _ => return MenuChoice::Nothing,
        }
        let ordinal = (cmd - FIRST_SHELL_COMMAND) as usize;
        let owner = menu_owner();
        if Path::new(path).exists() {
            let mut verb = [0u16; 64];
            let get_string: extern "system" fn(
                Ptr,
                usize,
                u32,
                *mut u32,
                *mut u16,
                u32,
            ) -> HRESULT = mem::transmute(vfn(cm.0, 5));
            get_string(
                cm.0,
                ordinal,
                4, /* GCS_VERBW */
                ptr::null_mut(),
                verb.as_mut_ptr(),
                64,
            );
            let verb = String::from_utf16_lossy(
                &verb[..verb.iter().position(|&c| c == 0).unwrap_or(0)],
            );
            match verb.to_ascii_lowercase().as_str() {
                "rename" => return MenuChoice::Rename,
                "delete" => return MenuChoice::Delete,
                "copy" => return MenuChoice::Copy,
                "cut" => return MenuChoice::Cut,
                _ => {}
            }
        }
        let info = InvokeInfo {
            cb: mem::size_of::<InvokeInfo>() as u32,
            mask: 0,
            hwnd: owner,
            verb: ordinal as *const i8,
            parameters: ptr::null(),
            directory: ptr::null(),
            show: SW_SHOWNORMAL,
            hotkey: 0,
            icon: ptr::null_mut(),
        };
        let invoke: extern "system" fn(Ptr, *const InvokeInfo) -> HRESULT =
            mem::transmute(vfn(cm.0, 4));
        if invoke(cm.0, &info) >= 0 { MenuChoice::Invoked } else { MenuChoice::Nothing }
    }
}

// ---- menus: a small owner window that forwards owner-drawn/submenu messages
// to the shell's IContextMenu3/2, so "New", "Send to", "Open with" fill in. ----
const IID_ICONTEXT_MENU2: GUID = guid(0x000214F4, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const IID_ICONTEXT_MENU3: GUID = guid(
    0xBCFCE0A0,
    0xEC17,
    0x11D0,
    [0x8D, 0x10, 0x00, 0xA0, 0xC9, 0x0F, 0x27, 0x19],
);
const IID_ISHELL_VIEW: GUID = guid(0x000214E3, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
static CURRENT_MENU: std::sync::atomic::AtomicPtr<c_void> =
    std::sync::atomic::AtomicPtr::new(ptr::null_mut());
static CURRENT_VERSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
unsafe extern "system" fn owner_proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    let cm = CURRENT_MENU.load(std::sync::atomic::Ordering::Relaxed);
    if !cm.is_null() && matches!(msg, 0x116 | 0x117 | 0x2B | 0x2C | 0x120) {
        if CURRENT_VERSION.load(std::sync::atomic::Ordering::Relaxed) == 3 {
            let f: extern "system" fn(Ptr, u32, usize, isize, *mut isize) -> HRESULT =
                mem::transmute(vfn(cm, 7));
            let mut result = 0isize;
            if f(cm, msg, w, l, &mut result) >= 0 {
                return result;
            }
        } else {
            let f: extern "system" fn(Ptr, u32, usize, isize) -> HRESULT =
                mem::transmute(vfn(cm, 6));
            if f(cm, msg, w, l) >= 0 {
                return 0;
            }
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}
fn menu_owner() -> HWND {
    thread_local! {
        static OWNER: std::cell::Cell<HWND> = const { std::cell::Cell::new(ptr::null_mut()) };
    }
    OWNER.with(|o| {
        if o.get().is_null() {
            unsafe {
                let class = wide("DesktopCraftMenuOwner");
                let mut wc: WNDCLASSW = mem::zeroed();
                wc.lpfnWndProc = Some(owner_proc);
                wc.hInstance = winapi::um::libloaderapi::GetModuleHandleW(ptr::null());
                wc.lpszClassName = class.as_ptr();
                RegisterClassW(&wc);
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    wide("").as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    wc.hInstance,
                    ptr::null_mut(),
                );
                o.set(hwnd);
            }
        }
        o.get()
    })
}
/// Show a popup menu built by a shell IContextMenu; returns the chosen id.
unsafe fn track(cm: &Com, menu: HMENU, x: i32, y: i32) -> u32 {
    let owner = menu_owner();
    let v3 = cm.query(&IID_ICONTEXT_MENU3);
    let v2 = if v3.ok() {
        Com::null()
    } else {
        cm.query(&IID_ICONTEXT_MENU2)
    };
    let (current, version) = if v3.ok() {
        (v3.0, 3)
    } else if v2.ok() {
        (v2.0, 2)
    } else {
        (ptr::null_mut(), 0)
    };
    CURRENT_MENU.store(current, std::sync::atomic::Ordering::Relaxed);
    CURRENT_VERSION.store(version, std::sync::atomic::Ordering::Relaxed);
    SetForegroundWindow(owner);
    let cmd = TrackPopupMenuEx(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON,
        x,
        y,
        owner,
        ptr::null_mut(),
    ) as u32;
    PostMessageW(owner, WM_NULL, 0, 0);
    CURRENT_MENU.store(ptr::null_mut(), std::sync::atomic::Ordering::Relaxed);
    drop(v3);
    drop(v2);
    cmd
}
#[repr(C)]
struct InvokeInfoEx {
    cb: u32,
    mask: u32,
    hwnd: HWND,
    verb: *const i8,
    parameters: *const i8,
    directory: *const i8,
    show: i32,
    hotkey: u32,
    icon: Ptr,
    title: *const i8,
    verb_w: *const u16,
    parameters_w: *const u16,
    directory_w: *const u16,
    title_w: *const u16,
    point: POINT,
}

pub enum DesktopChoice {
    Nothing,
    Windows,
    Ours(usize),
}
/// Windows' own desktop background menu (View, Sort by, Refresh, New,
/// Display settings, Personalize and installed extensions), with the given
/// DesktopCraft items added after it, as the original adds "Configure".
pub fn desktop_menu(x: i32, y: i32, ours: &[&str]) -> DesktopChoice {
    unsafe {
        let menu = CreatePopupMenu();
        let mut cm = Com::null();
        if let Some(view) = DesktopView::acquire() {
            let sv = view.view.query(&IID_ISHELL_VIEW);
            if sv.ok() {
                let mut out = ptr::null_mut();
                let get: extern "system" fn(Ptr, u32, *const GUID, *mut Ptr) -> HRESULT =
                    mem::transmute(vfn(sv.0, 15));
                get(
                    sv.0,
                    0, /* SVGIO_BACKGROUND */
                    &IID_ICONTEXT_MENU,
                    &mut out,
                );
                cm = Com(out);
            }
        }
        if cm.ok() {
            let query: extern "system" fn(Ptr, HMENU, u32, u32, u32, u32) -> HRESULT =
                mem::transmute(vfn(cm.0, 3));
            query(cm.0, menu, 0, 1, 0x6FFF, 0);
            AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());
        }
        for (i, label) in ours.iter().enumerate() {
            AppendMenuW(menu, MF_STRING, 0x7000 + i, wide(label).as_ptr());
        }
        let cmd = if cm.ok() {
            track(&cm, menu, x, y)
        } else {
            let owner = menu_owner();
            SetForegroundWindow(owner);
            TrackPopupMenuEx(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                x,
                y,
                owner,
                ptr::null_mut(),
            ) as u32
        };
        DestroyMenu(menu);
        if cmd >= 0x7000 {
            return DesktopChoice::Ours((cmd - 0x7000) as usize);
        }
        if cmd > 0 && cm.ok() {
            // CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE: "New" items land where
            // the person clicked.
            let info = InvokeInfoEx {
                cb: mem::size_of::<InvokeInfoEx>() as u32,
                mask: 0x4000 | 0x2000_0000,
                hwnd: menu_owner(),
                verb: (cmd - 1) as usize as *const i8,
                parameters: ptr::null(),
                directory: ptr::null(),
                show: SW_SHOWNORMAL,
                hotkey: 0,
                icon: ptr::null_mut(),
                title: ptr::null(),
                verb_w: (cmd - 1) as usize as *const u16,
                parameters_w: ptr::null(),
                directory_w: ptr::null(),
                title_w: ptr::null(),
                point: POINT { x, y },
            };
            let invoke: extern "system" fn(Ptr, *const InvokeInfoEx) -> HRESULT =
                mem::transmute(vfn(cm.0, 4));
            invoke(cm.0, &info);
            return DesktopChoice::Windows;
        }
        DesktopChoice::Nothing
    }
}

