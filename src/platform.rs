use std::path::PathBuf;

mod peek_chords;
pub use peek_chords::*;
use peek_chords::peek_key_codes;

#[cfg(windows)]
mod win {
    use super::*;
    use std::{mem, ptr};
    use winapi::{
        shared::{
            minwindef::{BOOL, LPARAM},
            windef::{HWND, RECT},
        },
        um::{
            combaseapi::{CoInitializeEx, CoTaskMemFree, CoUninitialize},
            commdlg::{
                GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
                OPENFILENAMEW,
            },
            processthreadsapi::GetCurrentProcessId,
            shellapi::ShellExecuteW,
            shlobj::SHGetPathFromIDListW,
            winuser::*,
        },
    };
    mod file_dialogs;
    pub use file_dialogs::*;
    mod hotkeys;
    pub use hotkeys::*;
    mod desktop_geometry;
    pub use desktop_geometry::*;

    #[repr(C)]
    struct BrowseInfo {
        owner: HWND,
        root: *const std::ffi::c_void,
        display: *mut u16,
        title: *const u16,
        flags: u32,
        callback: Option<unsafe extern "system" fn(HWND, u32, LPARAM, LPARAM) -> i32>,
        data: LPARAM,
        image: i32,
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHBrowseForFolderW(info: *mut BrowseInfo) -> *mut std::ffi::c_void;
    }
    // Name/extension rules now use model::fold (locale-independent Unicode
    // folding) on every platform; see MECHANICS.md "Implementation decisions".
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    unsafe extern "system" fn find(hwnd: HWND, result: LPARAM) -> BOOL {
        let mut process = 0;
        GetWindowThreadProcessId(hwnd, &mut process);
        if process == GetCurrentProcessId() && IsWindowVisible(hwnd) != 0 {
            *(result as *mut HWND) = hwnd;
            0
        } else {
            1
        }
    }
    fn window() -> HWND {
        unsafe {
            let mut hwnd = ptr::null_mut();
            EnumWindows(Some(find), &mut hwnd as *mut _ as LPARAM);
            hwnd
        }
    }
}
#[cfg(windows)]
pub use win::*;
#[cfg(not(windows))]
pub fn open(path: &str) -> Result<(), String> {
    std::process::Command::new(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    })
    .arg(path)
    .spawn()
    .map(|_| ())
    .map_err(|e| e.to_string())
}
#[cfg(not(windows))]
pub fn choose_file() -> Option<PathBuf> {
    None
}
#[cfg(not(windows))]
pub fn choose_folder() -> Option<PathBuf> {
    None
}
#[cfg(not(windows))]
pub fn peek_pressed_keys() -> Vec<i32> {
    vec![]
}
#[cfg(not(windows))]
pub fn hotkey(_: &str) -> bool {
    false
}
#[cfg(not(windows))]
pub fn page_hotkey() -> i32 {
    0
}
#[cfg(not(windows))]
pub fn undo_hotkey() -> bool {
    false
}
#[cfg(not(windows))]
pub fn desktop_icon_spacing() -> Option<(f32, f32)> {
    None
}
#[cfg(not(windows))]
pub fn double_click_interval() -> f64 {
    0.5 // Compatibility estimate; original target is Windows-only.
}
#[cfg(not(windows))]
pub fn peek(_: bool, _: bool) {}
#[cfg(not(windows))]
pub fn desktop(_: bool, _: &mut [i32; 4]) {}
