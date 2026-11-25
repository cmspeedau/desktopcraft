use super::*;

pub(super) fn run(rx: Receiver<BackdropFrame>, ready: Sender<u32>, wheel: Sender<(u64, i32)>) {
    unsafe {
        SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS);
        let header_class = wide(HEADER_CLASS);
        let wc = WndClassExW {
            cb_size: std::mem::size_of::<WndClassExW>() as u32, style: 0, wnd_proc: proc,
            cls_extra: 0, wnd_extra: 0, instance, icon: std::ptr::null_mut(),
            cursor: std::ptr::null_mut(), background: std::ptr::null_mut(),
            menu_name: std::ptr::null(), class_name: class.as_ptr(), icon_small: std::ptr::null_mut(),
        };
        RegisterClassExW(&wc);
        let hwc = WndClassExW {
            cb_size: std::mem::size_of::<WndClassExW>() as u32, style: 0, wnd_proc: header_proc,
            cls_extra: 0, wnd_extra: 0, instance, icon: std::ptr::null_mut(),
            cursor: std::ptr::null_mut(), background: std::ptr::null_mut(),
            menu_name: std::ptr::null(), class_name: header_class.as_ptr(), icon_small: std::ptr::null_mut(),
        };
        RegisterClassExW(&hwc);
        let mut layer = Layer {
            parent: std::ptr::null_mut(),
            windows: BTreeMap::new(),
            headers: BTreeMap::new(),
            tints: BTreeMap::new(),
            instance, class, header_class, create_failure_logged: false,
        };
        let mut msg: Msg = std::mem::zeroed();
        // Create this thread's message queue before announcing it.
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 0);
        SetTimer(std::ptr::null_mut(), 1, 1500, std::ptr::null_mut());
        let hook_panels = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let last_wheel = std::sync::Arc::new(std::sync::Mutex::new(None));
        let hook = start_wheel_hook(hook_panels.clone(), wheel.clone(), last_wheel.clone());
        if hook.is_none() {
            crate::host::event("WARN", "native_backdrop.wheel_hook_failed", "Dedicated wheel hook unavailable");
        }
        let tp_class = wide(TOUCHPAD_CLASS);
        let twc = WndClassExW {
            cb_size: std::mem::size_of::<WndClassExW>() as u32,
            style: 0,
            wnd_proc: touchpad_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: std::ptr::null_mut(),
            cursor: std::ptr::null_mut(),
            background: std::ptr::null_mut(),
            menu_name: std::ptr::null(),
            class_name: tp_class.as_ptr(),
            icon_small: std::ptr::null_mut(),
        };
        RegisterClassExW(&twc);
        let tp_hwnd = CreateWindowExW(
            0,
            tp_class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null_mut(),
        );
        if !tp_hwnd.is_null() {
            let rid = RawInputDevice {
                usage_page: HID_USAGE_PAGE_DIGITIZER,
                usage: HID_USAGE_DIGITIZER_TOUCH_PAD,
                flags: RIDEV_INPUTSINK,
                hwnd_target: tp_hwnd,
            };
            let _ = RegisterRawInputDevices(&rid, 1, std::mem::size_of::<RawInputDevice>() as u32);
        }
        WHEEL.with(|state| {
            *state.borrow_mut() = Some(WheelState {
                panels: Vec::new(),
                tx: wheel,
                last_mouse_wheel: last_wheel,
                touchpad_devices: BTreeMap::new(),
                touchpad_scroll: TouchpadScrollState::default(),
                natural_scroll: touchpad_natural_scroll(),
                natural_scroll_checked: std::time::Instant::now(),
            });
        });
        let _ = ready.send(GetCurrentThreadId());
        let mut current = BackdropFrame::default();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            match msg.message {
                WM_APP_PANELS => {
                    if let Some(latest) = rx.try_iter().last() {
                        current = latest;
                        layer.apply(&current);
                        if let Ok(mut p) = hook_panels.lock() { *p = current.panels.clone(); }
                        WHEEL.with(|state| {
                            if let Some(s) = state.borrow_mut().as_mut() { s.panels = current.panels.clone(); }
                        });
                    }
                }
                // Explorer restart check: recreate the layer and panels.
                WM_TIMER if msg.hwnd.is_null() => {
                    if (!current.panels.is_empty() || !current.headers.is_empty())
                        && (layer.parent.is_null() || IsWindow(layer.parent) == 0)
                    {
                        layer.apply(&current);
                    }
                    layer.keep_tints_shown();
                }
                _ => {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        if !tp_hwnd.is_null() && IsWindow(tp_hwnd) != 0 { DestroyWindow(tp_hwnd); }
        if let Some((id, thread)) = hook { PostThreadMessageW(id, WM_QUIT, 0, 0); let _ = thread.join(); }
        WHEEL.with(|state| *state.borrow_mut() = None);
        layer.clear();
    }
}
