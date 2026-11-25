use super::*;

static ON_DEMAND_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static ON_DEMAND_MOUSE_HOOK: std::sync::Mutex<Option<(u32, std::thread::JoinHandle<()>)>> = std::sync::Mutex::new(None);
static NATIVE_QUICK_HIDE_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static NATIVE_QUICK_HIDE_CLICK: std::sync::Mutex<Option<(i32, i32)>> =
    std::sync::Mutex::new(None);
static LAST_CLICK: std::sync::Mutex<(u32, i32, i32)> = std::sync::Mutex::new((0, 0, 0));
// Screen-wide color picker: the hook captures exactly one user click;
// no screenshot or pixel content is written to disk or sent elsewhere.
static COLOR_PICK_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static COLOR_PICK_SHIELDED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static COLOR_PICK_LEFT_UP: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static COLOR_PICK_RIGHT_UP: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static COLOR_PICK_RESULT: std::sync::Mutex<Option<Option<(i32, i32)>>> =
    std::sync::Mutex::new(None);

fn ensure_mouse_hook() -> bool {
    let Ok(mut slot) = ON_DEMAND_MOUSE_HOOK.lock() else {
        return false;
    };
    if let Some((_, thread)) = slot.as_ref() {
        if !thread.is_finished() {
            return ON_DEMAND_ACTIVE.load(std::sync::atomic::Ordering::Acquire);
        }
        if let Some((_, completed)) = slot.take() { let _ = completed.join(); }
        ON_DEMAND_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    }
    let (ready, rx) = std::sync::mpsc::channel();
    let Ok(thread) = std::thread::Builder::new().name("desktopcraft-quick-hide-hook".into()).spawn(move || unsafe {
        use winapi::um::winuser::*;
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, 0);
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook),
            winapi::um::libloaderapi::GetModuleHandleW(ptr::null()), 0);
        if hook.is_null() { let _ = ready.send(None); return; }
        if ready.send(Some(winapi::um::processthreadsapi::GetCurrentThreadId())).is_ok() {
            while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {}
        }
        UnhookWindowsHookEx(hook);
    }) else { return false; };
    if let Ok(Some(id)) = rx.recv_timeout(std::time::Duration::from_secs(2)) {
        *slot = Some((id, thread));
        ON_DEMAND_ACTIVE.store(true, std::sync::atomic::Ordering::Release);
        true
    } else {
        // A timed-out startup will unhook when its ready receiver is gone.
        drop(rx);
        if thread.is_finished() { let _ = thread.join(); }
        else { *slot = Some((0, thread)); }
        false
    }
}

fn release_on_demand_mouse_hook(force: bool) {
    if !force {
        // Keep the dedicated pump alive across transient UI states.
        return;
    }
    if let Ok(mut slot) = ON_DEMAND_MOUSE_HOOK.lock() {
        if let Some((id, thread)) = slot.take() {
            let posted = id != 0 && unsafe { winapi::um::winuser::PostThreadMessageW(id, winapi::um::winuser::WM_QUIT, 0, 0) } != 0;
            if posted {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
                while !thread.is_finished() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            if thread.is_finished() { let _ = thread.join(); }
            else {
                *slot = Some((id, thread));
                crate::host::log("Mouse hook shutdown not acknowledged; callbacks disabled");
            }
        }
        ON_DEMAND_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
    }
}

pub fn begin_color_pick() -> bool {
    if let Ok(mut result) = COLOR_PICK_RESULT.lock() {
        *result = None;
    }
    COLOR_PICK_LEFT_UP.store(false, std::sync::atomic::Ordering::SeqCst);
    COLOR_PICK_RIGHT_UP.store(false, std::sync::atomic::Ordering::SeqCst);
    if !ensure_mouse_hook() {
        COLOR_PICK_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        COLOR_PICK_SHIELDED.store(false, std::sync::atomic::Ordering::SeqCst);
        return false;
    }
    COLOR_PICK_SHIELDED.store(true, std::sync::atomic::Ordering::SeqCst);
    COLOR_PICK_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
    // Make the entire DesktopCraft window click-through temporarily,
    // revealing pixels of other apps, browsers and photographs beneath it.
    set_region(&[]);
    true
}
pub fn color_pick_shielded() -> bool {
    COLOR_PICK_SHIELDED.load(std::sync::atomic::Ordering::SeqCst)
}
pub fn finish_color_pick() -> Option<Option<[f32; 3]>> {
    if COLOR_PICK_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
        && unsafe { (GetAsyncKeyState(VK_ESCAPE) as u16 & 0x8000) != 0 }
    {
        COLOR_PICK_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        COLOR_PICK_SHIELDED.store(false, std::sync::atomic::Ordering::SeqCst);
        COLOR_PICK_LEFT_UP.store(false, std::sync::atomic::Ordering::SeqCst);
        COLOR_PICK_RIGHT_UP.store(false, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut result) = COLOR_PICK_RESULT.lock() {
            *result = None;
        }
        return Some(None);
    }
    let event = COLOR_PICK_RESULT.lock().ok()?.take()?;
    let color = event.and_then(|(x, y)| unsafe {
        let dc = GetDC(ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let pixel = GetPixel(dc, x, y) as u32;
        ReleaseDC(ptr::null_mut(), dc);
        if pixel == 0xFFFF_FFFF {
            None
        } else {
            Some([
                (pixel & 0xff) as f32 / 255.,
                ((pixel >> 8) & 0xff) as f32 / 255.,
                ((pixel >> 16) & 0xff) as f32 / 255.,
            ])
        }
    });
    COLOR_PICK_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    COLOR_PICK_SHIELDED.store(false, std::sync::atomic::Ordering::SeqCst);
    Some(color)
}
pub fn cleanup_color_pick_hook() {
    release_on_demand_mouse_hook(false);
}
pub fn set_native_quick_hide(enabled: bool) {
    if enabled {
        NATIVE_QUICK_HIDE_ENABLED.store(ensure_mouse_hook(), std::sync::atomic::Ordering::SeqCst);
    } else {
        if NATIVE_QUICK_HIDE_ENABLED.swap(false, std::sync::atomic::Ordering::SeqCst) {
            if let Ok(mut last) = LAST_CLICK.lock() { *last = (0, 0, 0); }
            if let Ok(mut click) = NATIVE_QUICK_HIDE_CLICK.lock() {
                *click = None;
            }
        }
        release_on_demand_mouse_hook(false);
    }
}
pub fn take_native_quick_hide() -> Option<(i32, i32)> {
    NATIVE_QUICK_HIDE_CLICK
        .lock()
        .ok()
        .and_then(|mut click| click.take())
}
pub fn shutdown_mouse_hooks() {
    NATIVE_QUICK_HIDE_ENABLED.store(false, std::sync::atomic::Ordering::SeqCst);
    COLOR_PICK_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    COLOR_PICK_SHIELDED.store(false, std::sync::atomic::Ordering::SeqCst);
    release_on_demand_mouse_hook(true);
}
unsafe extern "system" fn mouse_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    if code < 0 || lparam == 0 || !matches!(wparam as u32, WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP) || (*(lparam as *const MSLLHOOKSTRUCT)).flags & 1 != 0 {
        return CallNextHookEx(ptr::null_mut(), code, wparam, lparam);
    }
    if code >= 0 {
        let event = wparam as u32;
        if event == WM_LBUTTONUP
            && COLOR_PICK_LEFT_UP.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return 1;
        }
        if event == WM_RBUTTONUP
            && COLOR_PICK_RIGHT_UP.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return 1;
        }
        if COLOR_PICK_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
            if event == WM_LBUTTONDOWN || event == WM_RBUTTONDOWN {
                let pos = (*(lparam as *const MSLLHOOKSTRUCT)).pt;
                if let Ok(mut result) = COLOR_PICK_RESULT.try_lock() {
                    *result = Some(if event == WM_LBUTTONDOWN {
                        Some((pos.x, pos.y))
                    } else {
                        None
                    });
                } else {
                    return CallNextHookEx(ptr::null_mut(), code, wparam, lparam);
                }
                COLOR_PICK_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
                if event == WM_LBUTTONDOWN {
                    COLOR_PICK_LEFT_UP.store(true, std::sync::atomic::Ordering::SeqCst);
                    return 1; // Do not activate the sampled app window.
                }
                COLOR_PICK_RIGHT_UP.store(true, std::sync::atomic::Ordering::SeqCst);
                return 1; // Do not leak right-click menu to the underlying app.
            }
        }
    }
    if code >= 0 && wparam == WM_LBUTTONDOWN as usize {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        let native_qh = NATIVE_QUICK_HIDE_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
        let blank_desktop = native_qh && over_explorer_desktop(x, y);
        if let Ok(mut last) = LAST_CLICK.try_lock() {
            if blank_desktop {
                let (t, lx, ly) = *last;
                let near = (lx - x).abs() <= GetSystemMetrics(SM_CXDOUBLECLK) / 2 + 1
                    && (ly - y).abs() <= GetSystemMetrics(SM_CYDOUBLECLK) / 2 + 1;
                if t != 0 && info.time.wrapping_sub(t) <= GetDoubleClickTime() && near {
                    if let Ok(mut click) = NATIVE_QUICK_HIDE_CLICK.try_lock() {
                        *click = Some((x, y));
                    }
                    *last = (0, 0, 0);
                } else {
                    *last = (info.time, x, y);
                }
            } else {
                // Do not pair a click on a folder/window/group with one on
                // the desktop. Only two consecutive blank desktop clicks count.
                *last = (0, 0, 0);
            }
        }
    }
    CallNextHookEx(ptr::null_mut(), code, wparam, lparam)
}
