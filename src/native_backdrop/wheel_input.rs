use super::*;

/// Reads Windows Precision Touchpad's ScrollDirection setting
/// (1 = natural/default, 0 = reverse).
pub(super) fn touchpad_natural_scroll() -> bool {
    use winapi::um::winreg::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\PrecisionTouchPad");
    let name = wide("ScrollDirection");
    let mut val: u32 = 1;
    let mut size: u32 = 4;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut val as *mut u32 as _,
            &mut size,
        )
    };
    if status == 0 { val != 0 } else { true }
}

pub(super) unsafe fn query_touchpad_device(h_device: *mut c_void) -> Option<TouchpadDevice> {
    let mut size: u32 = 0;
    if GetRawInputDeviceInfoW(h_device, RIDI_PREPARSEDDATA, std::ptr::null_mut(), &mut size) != 0
        || size == 0
        || size > 256 * 1024
    {
        return None;
    }
    let mut preparsed = vec![0u8; size as usize];
    let got = GetRawInputDeviceInfoW(
        h_device,
        RIDI_PREPARSEDDATA,
        preparsed.as_mut_ptr() as _,
        &mut size,
    );
    if got == u32::MAX {
        return None;
    }
    let mut caps: HidpCaps = std::mem::zeroed();
    if HidP_GetCaps(preparsed.as_ptr() as _, &mut caps) != HIDP_STATUS_SUCCESS {
        return None;
    }
    let mut val_len = caps.number_input_value_caps;
    if val_len == 0 {
        return None;
    }
    let mut value_caps: Vec<HidpValueCaps> = vec![std::mem::zeroed(); val_len as usize];
    if HidP_GetValueCaps(
        HIDP_INPUT,
        value_caps.as_mut_ptr(),
        &mut val_len,
        preparsed.as_ptr() as _,
    ) != HIDP_STATUS_SUCCESS
    {
        return None;
    }
    let mut by_col: BTreeMap<u16, (Option<(i32, i32)>, Option<(i32, i32)>)> = BTreeMap::new();
    for vc in value_caps.iter().take(val_len as usize) {
        if vc.usage_page != HID_USAGE_PAGE_GENERIC || vc.link_collection == 0 {
            continue;
        }
        let usage = vc.usage_min_or_usage;
        let range = (vc.logical_min, vc.logical_max);
        if range.1 <= range.0 {
            continue;
        }
        let entry = by_col.entry(vc.link_collection).or_insert((None, None));
        if usage == HID_USAGE_GENERIC_X {
            entry.0 = Some(range);
        } else if usage == HID_USAGE_GENERIC_Y {
            entry.1 = Some(range);
        }
    }
    let contacts: Vec<ContactAxis> = by_col
        .into_iter()
        .filter_map(|(col, (x, y))| {
            let ((x_min, x_max), (y_min, y_max)) = (x?, y?);
            Some(ContactAxis { link_collection: col, x_min, x_max, y_min, y_max })
        })
        .collect();
    if contacts.is_empty() {
        return None;
    }
    Some(TouchpadDevice { preparsed, contacts })
}
pub(super) unsafe extern "system" fn touchpad_proc(h: Hwnd, m: u32, w: usize, l: isize) -> isize {
    if m == WM_INPUT {
        let mut size: u32 = 0;
        let hdr_size = std::mem::size_of::<RawInputHeader>() as u32;
        if GetRawInputData(l as *mut c_void, RID_INPUT, std::ptr::null_mut(), &mut size, hdr_size) == 0
            && size >= hdr_size + 8
            && size <= 65536
        {
            let mut buf = vec![0u8; size as usize];
            if GetRawInputData(l as *mut c_void, RID_INPUT, buf.as_mut_ptr() as _, &mut size, hdr_size)
                != u32::MAX
            {
                let hdr = &*(buf.as_ptr() as *const RawInputHeader);
                if hdr.dw_type == RIM_TYPEHID {
                    let hid_size = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]) as usize;
                    let hid_count = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]) as usize;
                    let raw_offset = 32usize;
                    if hid_size > 0 && hid_count > 0 && raw_offset + hid_size.saturating_mul(hid_count) <= buf.len() {
                        let now = std::time::Instant::now();
                        let dev_key = hdr.h_device as usize;
                        WHEEL.with(|state| {
                            let mut borrow = state.borrow_mut();
                            let Some(s) = borrow.as_mut() else { return };
                            if now.saturating_duration_since(s.natural_scroll_checked)
                                > std::time::Duration::from_secs(5)
                            {
                                s.natural_scroll = touchpad_natural_scroll();
                                s.natural_scroll_checked = now;
                            }
                            let dev_opt = s
                                .touchpad_devices
                                .entry(dev_key)
                                .or_insert_with(|| query_touchpad_device(hdr.h_device));
                            let Some(dev) = dev_opt.as_ref() else { return };
                            let mut active = Vec::with_capacity(dev.contacts.len());
                            for r_idx in 0..hid_count {
                                let start = raw_offset + r_idx * hid_size;
                                let report = &buf[start..start + hid_size];
                                active.clear();
                                for c in &dev.contacts {
                                    let mut usages = [0u16; 8];
                                    let mut usage_len = usages.len() as u32;
                                    let st = HidP_GetUsages(
                                        HIDP_INPUT,
                                        HID_USAGE_PAGE_DIGITIZER,
                                        c.link_collection,
                                        usages.as_mut_ptr(),
                                        &mut usage_len,
                                        dev.preparsed.as_ptr() as _,
                                        report.as_ptr(),
                                        report.len() as u32,
                                    );
                                    if st != HIDP_STATUS_SUCCESS {
                                        continue;
                                    }
                                    let tip_down = usages[..(usage_len as usize).min(usages.len())]
                                        .contains(&HID_USAGE_DIGITIZER_TIP_SWITCH);
                                    if !tip_down {
                                        continue;
                                    }
                                    let mut rx: u32 = 0;
                                    let mut ry: u32 = 0;
                                    let sx = HidP_GetUsageValue(
                                        HIDP_INPUT,
                                        HID_USAGE_PAGE_GENERIC,
                                        c.link_collection,
                                        HID_USAGE_GENERIC_X,
                                        &mut rx,
                                        dev.preparsed.as_ptr() as _,
                                        report.as_ptr(),
                                        report.len() as u32,
                                    );
                                    let sy = HidP_GetUsageValue(
                                        HIDP_INPUT,
                                        HID_USAGE_PAGE_GENERIC,
                                        c.link_collection,
                                        HID_USAGE_GENERIC_Y,
                                        &mut ry,
                                        dev.preparsed.as_ptr() as _,
                                        report.as_ptr(),
                                        report.len() as u32,
                                    );
                                    if sx == HIDP_STATUS_SUCCESS && sy == HIDP_STATUS_SUCCESS {
                                        let nx = (rx as i32 - c.x_min) as f32 / (c.x_max - c.x_min).max(1) as f32;
                                        let ny = (ry as i32 - c.y_min) as f32 / (c.y_max - c.y_min).max(1) as f32;
                                        active.push((nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0)));
                                    }
                                }
                                if let Some(delta) = s.touchpad_scroll.update(&active, now, s.natural_scroll) {
                                    let recent_wheel = s.last_mouse_wheel.try_lock().ok().and_then(|t| *t).is_some_and(|t| {
                                        now.saturating_duration_since(t) < std::time::Duration::from_millis(100)
                                    });
                                    let modifier = [VK_SHIFT, VK_CONTROL, VK_MENU]
                                        .iter()
                                        .any(|&key| GetAsyncKeyState(key) as u16 & 0x8000 != 0);
                                    if !recent_wheel && !modifier {
                                        let mut pt = Point { x: 0, y: 0 };
                                        if GetCursorPos(&mut pt) != 0 && desktop_under(pt) {
                                            if let Some(id) = panel_at(&s.panels, (pt.x, pt.y)) {
                                                let _ = s.tx.send((id, delta));
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
        return 0;
    }
    DefWindowProcW(h, m, w, l)
}

/// The Box under `pt`, topmost first, when `pt` is inside a Box body.
pub(super) fn panel_at(panels: &[Panel], pt: (i32, i32)) -> Option<u64> {
    panels.iter().rev().find(|p| pt.0 >= p.x && pt.0 < p.x + p.w && pt.1 >= p.y && pt.1 < p.y + p.h)
        .map(|p| p.id)
}

pub(super) unsafe fn desktop_under(pt: Point) -> bool {
    let root = GetAncestor(WindowFromPoint(pt), GA_ROOT);
    let mut name = [0u16; 32];
    let len = GetClassNameW(root, name.as_mut_ptr(), name.len() as i32).max(0) as usize;
    let class = String::from_utf16_lossy(&name[..len]);
    class == "Progman" || class == "WorkerW"
}

/// Explorer owns the Box body, so the wheel goes to Explorer's desktop view.
/// Over a Box body (and only when the desktop is the window there, with no
/// modifier held) the notch is handed to DesktopCraft as a row scroll.
struct WheelHookState {
    panels: std::sync::Arc<std::sync::Mutex<Vec<Panel>>>,
    tx: Sender<(u64, i32)>,
    last: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
}
thread_local! {
    static HOOK_STATE: std::cell::RefCell<Option<WheelHookState>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn start_wheel_hook(
    panels: std::sync::Arc<std::sync::Mutex<Vec<Panel>>>,
    tx: Sender<(u64, i32)>,
    last: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
) -> Option<(u32, JoinHandle<()>)> {
    let (ready, rx) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new().name("desktopcraft-wheel-hook".into()).spawn(move || unsafe {
        SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);
        let mut msg: Msg = std::mem::zeroed();
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 0);
        HOOK_STATE.with(|s| *s.borrow_mut() = Some(WheelHookState { panels, tx, last }));
        let hook = SetWindowsHookExW(WH_MOUSE_LL, wheel_hook, GetModuleHandleW(std::ptr::null()), 0);
        if hook.is_null() { let _ = ready.send(None); return; }
        if ready.send(Some(GetCurrentThreadId())).is_ok() {
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        }
        UnhookWindowsHookEx(hook);
    }).ok()?;
    match rx.recv().ok().flatten() {
        Some(id) => Some((id, thread)),
        None => { let _ = thread.join(); None }
    }
}
unsafe extern "system" fn wheel_hook(code: i32, w: usize, l: isize) -> isize {
    if code >= 0 && w == WM_MOUSEWHEEL {
        let info = &*(l as *const MouseLowLevel);
        let consumed = HOOK_STATE.with(|cell| {
            let Ok(state) = cell.try_borrow() else { return false };
            let Some(s) = state.as_ref() else { return false };
            let target = s.panels.try_lock().ok().and_then(|p| panel_at(&p, (info.pt.x, info.pt.y)));
            let Some(id) = target else { return false };
            if [VK_SHIFT, VK_CONTROL, VK_MENU].iter().any(|&key| GetAsyncKeyState(key) as u16 & 0x8000 != 0)
                || !desktop_under(info.pt) { return false; }
            let delta = (info.mouse_data >> 16) as u16 as i16 as i32;
            if s.tx.send((id, delta)).is_err() { return false; }
            if let Ok(mut last) = s.last.try_lock() { *last = Some(std::time::Instant::now()); }
            true
        });
        if consumed { return 1; }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w, l)
}
