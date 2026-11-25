//! Desktop host: the window that carries groups on the real Windows desktop,
//! monitors, single instance and commands, start-at-login and per-user install.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "DesktopCraft";
const LEGACY_APP_NAME: &str = "Desktop Spaces";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DesktopCraft";
const LEGACY_UNINSTALL_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DesktopSpaces";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VERB_NEW: &str = r"Software\Classes\DesktopBackground\Shell\DesktopCraftNewGroup";
const LEGACY_VERB_NEW: &str = r"Software\Classes\DesktopBackground\Shell\DesktopSpacesNewGroup";
const VERB_SETTINGS: &str = r"Software\Classes\DesktopBackground\Shell\DesktopCraftSettings";
/// The classic Explorer file menu (Windows 11: Show more options).
const VERB_PIN_FILE: &str = r"Software\Classes\*\shell\DesktopCraftPinFile";
const LEGACY_VERB_SETTINGS: &str =
    r"Software\Classes\DesktopBackground\Shell\DesktopSpacesSettings";

mod diagnostics;
pub use diagnostics::*;
mod workspace_paths;
pub use workspace_paths::*;
use workspace_paths::choose_workspace_dir;
mod command_queue;
pub use command_queue::*;
mod installer_transaction;
pub use installer_transaction::*;
#[cfg(windows)]
use installer_transaction::install_executable_staged;
use installer_transaction::rollback_install_stages;

#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Stable device name, e.g. \\.\DISPLAY1.
    pub key: String,
    /// Physical pixels in virtual-screen coordinates: x, y, w, h.
    pub rect: [i32; 4],
    pub work: [i32; 4],
    pub dpi: u32,
    pub primary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    ToggleEnabled,
    OpenSettings,
    Exit,
}

pub fn signature(monitors: &[Monitor]) -> String {
    let mut parts: Vec<String> = monitors
        .iter()
        .map(|m| {
            format!(
                "{}@{}x{}+{}+{}",
                m.key, m.rect[2], m.rect[3], m.rect[0], m.rect[1]
            )
        })
        .collect();
    parts.sort();
    parts.join("|")
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::{mem, ptr};
    use winapi::shared::{
        minwindef::{BOOL, DWORD, HKEY, LPARAM, TRUE},
        windef::{HDC, HMONITOR, HWND, LPRECT, POINT, RECT},
    };
    use winapi::um::{
        wingdi::*,
        winnt::{KEY_SET_VALUE, REG_DWORD, REG_SZ},
        winreg::*,
        winuser::*,
    };

    mod window_and_monitors;
    pub use window_and_monitors::*;
    mod mouse_input;
    pub use mouse_input::*;
    mod tray;
    pub use tray::*;
    mod installer;
    pub use installer::*;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    #[link(name = "shcore")]
    extern "system" {
        fn GetDpiForMonitor(m: HMONITOR, kind: u32, x: *mut u32, y: *mut u32) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(
            attrs: *mut std::ffi::c_void,
            owner: BOOL,
            name: *const u16,
        ) -> *mut std::ffi::c_void;
        fn GetLastError() -> DWORD;
    }

    // Keep the original cross-version mutex name so Desktop Spaces and
    // DesktopCraft never control the Windows desktop simultaneously.
    /// Whether a copy is running (without claiming the instance for this process).
    pub fn instance_running() -> bool {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenMutexW(access: u32, inherit: BOOL, name: *const u16) -> *mut std::ffi::c_void;
            fn CloseHandle(h: *mut std::ffi::c_void) -> BOOL;
        }
        unsafe {
            let h = OpenMutexW(0x0010_0000, 0, wide("Local\\DesktopSpacesHost").as_ptr());
            if h.is_null() {
                false
            } else {
                CloseHandle(h);
                true
            }
        }
    }
    /// True for the first instance; later launches pass their command on and exit.
    pub fn first_instance() -> bool {
        let Some(instance) = claim_desktop_instance() else { return false };
        // The GUI owns the instance for its entire process lifetime.
        std::mem::forget(instance);
        true
    }
    struct DesktopInstance(winapi::um::winnt::HANDLE);
    impl Drop for DesktopInstance {
        fn drop(&mut self) {
            unsafe { winapi::um::handleapi::CloseHandle(self.0); }
        }
    }
    fn claim_desktop_instance() -> Option<DesktopInstance> {
        unsafe {
            let handle = CreateMutexW(
                ptr::null_mut(),
                0,
                wide("Local\\DesktopSpacesHost").as_ptr(),
            );
            let error = GetLastError();
            if handle.is_null() {
                return None;
            }
            let instance = DesktopInstance(handle.cast());
            (error != 183).then_some(instance) // ERROR_ALREADY_EXISTS
        }
    }
    pub fn allow_foreground() {
        unsafe {
            AllowSetForegroundWindow(u32::MAX);
        }
    }

    /// A Yes/No question whose default (Enter) answer is No.
    pub fn ask_default_no(text: &str) -> bool {
        unsafe {
            MessageBoxW(
                ptr::null_mut(),
                wide(text).as_ptr(),
                wide(APP_NAME).as_ptr(),
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2 | MB_SETFOREGROUND,
            ) == IDYES
        }
    }
    pub fn message(text: &str, question: bool) -> bool {
        unsafe {
            let flags = if question {
                MB_YESNO | MB_ICONQUESTION
            } else {
                MB_OK | MB_ICONINFORMATION
            };
            MessageBoxW(
                ptr::null_mut(),
                wide(text).as_ptr(),
                wide(APP_NAME).as_ptr(),
                flags | MB_SETFOREGROUND,
            ) == IDYES
                || !question
        }
    }

}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
mod fallback {
    use super::*;
    pub fn monitors() -> Vec<Monitor> {
        vec![]
    }
    pub fn virtual_rect() -> [i32; 4] {
        [0, 0, 1280, 800]
    }
    pub fn place(_: [i32; 4]) {}
    pub fn set_region(_: &[[i32; 5]]) -> bool { true }
    pub fn set_front(_: bool) {}
    pub fn settings_taskbar(_: bool) {}
    pub fn settings_has_focus() -> bool {
        false
    }
    pub fn settings_deactivate() {}
    pub fn keep_shown() {}
    pub fn begin_color_pick() -> bool {
        false
    }
    pub fn color_pick_shielded() -> bool {
        false
    }
    pub fn finish_color_pick() -> Option<Option<[f32; 3]>> {
        None
    }
    pub fn cleanup_color_pick_hook() {}
    pub fn set_native_quick_hide(_: bool) {}
    pub fn take_native_quick_hide() -> Option<(i32, i32)> {
        None
    }
    pub fn shutdown_mouse_hooks() {}
    pub fn mouse_left_down() -> bool {
        false
    }
    pub fn mouse_right_down() -> bool {
        false
    }
    pub fn explorer_interaction_active() -> bool {
        false
    }
    pub fn capture_pointer(_: bool) {}
    pub fn cursor() -> (i32, i32) {
        (0, 0)
    }
    pub fn over_explorer_desktop(_: i32, _: i32) -> bool {
        false
    }
    pub fn over_other_app(_: i32, _: i32) -> bool {
        false
    }
    pub fn first_instance() -> bool {
        true
    }
    pub fn instance_running() -> bool {
        false
    }
    pub fn allow_foreground() {}
    pub fn sync_tray(_: bool) -> Option<TrayAction> {
        None
    }
    pub fn remove_tray() {}
    pub fn window() -> usize {
        0
    }
    pub fn message(_: &str, _: bool) -> bool {
        false
    }
    pub fn ask_default_no(_: &str) -> bool {
        false
    }
    pub fn autostart(_: Option<bool>) -> bool {
        false
    }
    pub struct InstallCommit {
        exe: PathBuf,
    }
    impl InstallCommit {
        pub fn exe(&self) -> &Path { &self.exe }
        pub fn release_desktop_instance(&mut self) {}
        pub fn commit(self) -> Result<(), String> { Ok(()) }
        pub fn rollback(self) -> Result<(), String> { Ok(()) }
    }
    pub fn install(_: bool) -> Result<InstallCommit, String> {
        Err("Installing is only available on Windows.".into())
    }
    pub fn installed_version() -> Option<String> {
        None
    }
    pub fn uninstall() -> Result<(), String> {
        Err("Uninstalling is only available on Windows.".into())
    }
}
#[cfg(not(windows))]
pub use fallback::*;

#[cfg(test)]
mod desktopcraft_rebrand_tests {
    use super::*;
    #[test]
    fn existing_workspace_has_priority_without_moving_user_data() {
        let root =
            std::env::temp_dir().join(format!("desktopcraft-rebrand-test-{}", std::process::id()));
        let new = root.join(APP_NAME);
        let old = root.join(LEGACY_APP_NAME);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(choose_workspace_dir(&root), new);
        std::fs::create_dir_all(old.join("data")).unwrap();
        let legacy_file = old.join("data/workspace.json");
        std::fs::write(&legacy_file, r#"{"pages":["Legacy layout"]}"#).unwrap();
        assert_eq!(choose_workspace_dir(&root), old);
        assert!(legacy_file.exists());
        std::fs::create_dir_all(new.join("data")).unwrap();
        let current_file = new.join("data/workspace.json");
        std::fs::write(&current_file, r#"{"pages":["New layout"]}"#).unwrap();
        assert_eq!(choose_workspace_dir(&root), new);
        assert_eq!(
            std::fs::read_to_string(&legacy_file).unwrap(),
            r#"{"pages":["Legacy layout"]}"#
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}

/// Whether version `a` is newer than `b` ("0.1.10" > "0.1.9"); parts that
/// aren't numbers count as 0.
pub fn version_newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return p > q;
        }
    }
    false
}
#[cfg(test)]
mod version_tests {
    use super::version_newer;
    #[test]
    fn compares_versions_part_by_part() {
        assert!(version_newer("0.1.1", "0.1.0"));
        assert!(version_newer("0.1.10", "0.1.9"));
        assert!(version_newer("1.0", "0.9.9"));
        assert!(!version_newer("0.1.0", "0.1.0"));
        assert!(!version_newer("0.1.0", "0.1.1"));
        assert!(!version_newer("0.1", "0.1.0"));
        assert!(version_newer("v0.2.0", "0.1.5"));
    }
}
pub fn is_setup_name(exe: &Path) -> bool {
    exe.file_stem()
        .is_some_and(|s| s.to_string_lossy().to_lowercase().contains("setup"))
}
