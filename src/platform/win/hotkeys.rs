use super::*;

/// Read a small allowlist of physical keys only while recording or
/// checking the chosen Peek chord; no key history is retained.
pub fn peek_pressed_keys() -> Vec<i32> {
    unsafe {
        peek_key_codes()
            .filter(|key| GetAsyncKeyState(*key) as u16 & 0x8000 != 0)
            .collect()
    }
}
pub fn hotkey(shortcut: &str) -> bool {
    // Any held chord can be seen even when another application has focus.
    // This does not intercept keys from that application.
    peek_chord_held(shortcut, &peek_pressed_keys())
}

/// Global polling for Ctrl/Alt + arrow, limited to DesktopCraft or the
/// Windows desktop foreground. Never hijack keys inside other programs.
/// Return one transition per keypress; holding a key does not create pages.
pub fn page_hotkey() -> i32 {
    use std::sync::atomic::{AtomicI32, Ordering};
    static HELD: AtomicI32 = AtomicI32::new(0);
    unsafe {
        let active = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(active, &mut pid);
        let allowed = if pid == std::process::id() {
            true
        } else {
            let mut class = [0u16; 64];
            let n = GetClassNameW(active, class.as_mut_ptr(), class.len() as i32);
            matches!(
                String::from_utf16_lossy(&class[..n as usize]).as_str(),
                "Progman" | "WorkerW"
            )
        };
        let down = |k: i32| (GetAsyncKeyState(k) as u16 & 0x8000) != 0;
        let state = if allowed && (down(0x11) || down(0x12)) && !down(0x5B) && !down(0x5C) {
            if down(0x27) && !down(0x25) {
                1
            } else if down(0x25) && !down(0x27) {
                -1
            } else {
                0
            }
        } else {
            0
        };
        let previous = HELD.swap(state, Ordering::Relaxed);
        if state != previous {
            state
        } else {
            0
        }
    }
}

/// Global polling for Ctrl+Z, limited to DesktopCraft or the Windows
/// desktop foreground (excluding inline Explorer icon rename "Edit" boxes).
/// Returns true once per physical keypress (edge-triggered).
pub fn undo_hotkey() -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static HELD: AtomicBool = AtomicBool::new(false);
    unsafe {
        let down = |k: i32| (GetAsyncKeyState(k) as u16 & 0x8000) != 0;
        let chord = down(0x11)
            && down(0x5A)
            && !down(0x12)
            && !down(0x10)
            && !down(0x5B)
            && !down(0x5C);
        let previous = HELD.swap(chord, Ordering::Relaxed);
        if !chord || previous {
            return false;
        }
        let active = GetForegroundWindow();
        if active.is_null() {
            return false;
        }
        let mut pid = 0;
        let thread_id = GetWindowThreadProcessId(active, &mut pid);
        if pid == std::process::id() {
            return true;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(active, class.as_mut_ptr(), class.len() as i32);
        let is_desktop = n > 0
            && matches!(
                String::from_utf16_lossy(&class[..n as usize]).as_str(),
                "Progman" | "WorkerW"
            );
        if !is_desktop {
            return false;
        }
        let mut gui: GUITHREADINFO = mem::zeroed();
        gui.cbSize = mem::size_of::<GUITHREADINFO>() as u32;
        if thread_id != 0
            && GetGUIThreadInfo(thread_id, &mut gui) != 0
            && !gui.hwndFocus.is_null()
        {
            let mut focus_class = [0u16; 64];
            let fn_len = GetClassNameW(
                gui.hwndFocus,
                focus_class.as_mut_ptr(),
                focus_class.len() as i32,
            );
            if fn_len > 0
                && String::from_utf16_lossy(&focus_class[..fn_len as usize])
                    .eq_ignore_ascii_case("Edit")
            {
                return false;
            }
        }
        true
    }
}
