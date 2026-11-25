//! Windows Shell integration through documented, supported interfaces only:
//! known folders, IShellWindows -> IFolderView (Explorer's desktop icon view),
//! IContextMenu (the classic Shell menu), OLE DoDragDrop with the shell's data
//! object, SHFileOperation (undoable file operations) and IShellItemImageFactory
//! (Explorer's icons and thumbnails). Nothing is injected into Explorer.
#![allow(non_snake_case, clippy::missing_safety_doc, dead_code)]

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ViewItem {
    pub path: String,
    /// The label Explorer shows (e.g. "Recycle Bin", shortcuts without ".lnk").
    pub label: String,
    /// Screen position (physical pixels) of the icon's top-left corner.
    pub pos: (i32, i32),
}

pub enum MenuChoice {
    Nothing,
    Invoked,
    Rename,
    Delete,
    Copy,
    Cut,
    Reveal,
    Unpin,
}

/// Real files copied/cut in Explorer (CF_HDROP + Preferred DropEffect).
/// Cut requires a physical move only when pasting into a real directory.
#[derive(Clone, Debug)]
pub struct FileClipboard {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::{ffi::c_void, mem, ptr};
    use winapi::shared::{
        guiddef::GUID,
        windef::{HBITMAP, HMENU, HWND, POINT, SIZE},
    };
    use winapi::um::{shellapi::*, wingdi::*, winuser::*};

    mod desktop_view;
    mod context_menu;
    pub use context_menu::*;
    mod ole_drag;
    pub use ole_drag::*;
    mod file_operations;
    pub use file_operations::*;
    mod icon_image;
    pub use icon_image::*;

    type HRESULT = i32;
    type Ptr = *mut c_void;

    const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> GUID {
        GUID {
            Data1: d1,
            Data2: d2,
            Data3: d3,
            Data4: d4,
        }
    }
    const CLSID_SHELL_WINDOWS: GUID = guid(
        0x9BA05972,
        0xF6A8,
        0x11CF,
        [0xA4, 0x42, 0x00, 0xA0, 0xC9, 0x0A, 0x8F, 0x39],
    );
    const IID_ISHELL_WINDOWS: GUID = guid(
        0x85CB6900,
        0x4D95,
        0x11CF,
        [0x96, 0x0C, 0x00, 0x80, 0xC7, 0xF4, 0xEE, 0x85],
    );
    const IID_ISERVICE_PROVIDER: GUID = guid(
        0x6D5140C1,
        0x7436,
        0x11CE,
        [0x80, 0x34, 0x00, 0xAA, 0x00, 0x60, 0x09, 0xFA],
    );
    const SID_STOP_LEVEL_BROWSER: GUID = guid(
        0x4C96BE40,
        0x915C,
        0x11CF,
        [0x99, 0xD3, 0x00, 0xAA, 0x00, 0x4A, 0xE8, 0x37],
    );
    const IID_ISHELL_BROWSER: GUID = guid(0x000214E2, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IFOLDER_VIEW: GUID = guid(
        0xCDE725B0,
        0xCCC9,
        0x4519,
        [0x91, 0x7E, 0x32, 0x5D, 0x72, 0xFA, 0xB4, 0xCE],
    );
    const IID_IFOLDER_VIEW2: GUID = guid(
        0x1AF3A467,
        0x214F,
        0x4298,
        [0x90, 0x8E, 0x06, 0xB0, 0x3E, 0x0B, 0x39, 0xF9],
    );
    const IID_ISHELL_FOLDER: GUID = guid(0x000214E6, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IENUM_IDLIST: GUID = guid(0x000214F2, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_ICONTEXT_MENU: GUID = guid(0x000214E4, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IDATA_OBJECT: GUID = guid(0x0000010E, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IDROP_SOURCE: GUID = guid(0x00000121, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IUNKNOWN: GUID = guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IIMAGE_FACTORY: GUID = guid(
        0xBCC18B79,
        0xBA16,
        0x442F,
        [0x80, 0xC4, 0x8A, 0x59, 0xC3, 0x0C, 0x46, 0x3B],
    );
    const FOLDERID_DESKTOP: GUID = guid(
        0xB4BFCC3A,
        0xDB2C,
        0x424C,
        [0xB0, 0x29, 0x7F, 0xE9, 0x9A, 0x87, 0xC6, 0x41],
    );
    const FOLDERID_PUBLIC_DESKTOP: GUID = guid(
        0xC4AA340D,
        0xF20F,
        0x4863,
        [0xAF, 0xEF, 0xF8, 0x7E, 0xF2, 0xE6, 0xBA, 0x25],
    );
    const FOLDERID_DOWNLOADS: GUID = guid(
        0x374DE290,
        0x123F,
        0x4565,
        [0x91, 0x64, 0x39, 0xC4, 0x92, 0x5E, 0x46, 0x7B],
    );
    const FOLDERID_PICTURES: GUID = guid(
        0x33E28130,
        0x4E1E,
        0x4676,
        [0x83, 0x5A, 0x98, 0x39, 0x5C, 0x3B, 0xC3, 0xBB],
    );
    const FOLDERID_DOCUMENTS: GUID = guid(
        0xFDD39AD0,
        0x238F,
        0x46AF,
        [0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7],
    );

    #[link(name = "shell32")]
    extern "system" {
        fn SHParseDisplayName(
            name: *const u16,
            bc: Ptr,
            pidl: *mut Ptr,
            sfgao_in: u32,
            sfgao_out: *mut u32,
        ) -> HRESULT;
        fn SHBindToParent(pidl: Ptr, riid: *const GUID, ppv: *mut Ptr, last: *mut Ptr) -> HRESULT;
        fn SHGetKnownFolderPath(
            id: *const GUID,
            flags: u32,
            token: Ptr,
            path: *mut *mut u16,
        ) -> HRESULT;
        fn SHCreateItemFromParsingName(
            path: *const u16,
            bc: Ptr,
            riid: *const GUID,
            ppv: *mut Ptr,
        ) -> HRESULT;
        fn ILFree(pidl: Ptr);
    }
    #[link(name = "shlwapi")]
    extern "system" {
        fn StrRetToStrW(strret: *mut StrRet, pidl: Ptr, out: *mut *mut u16) -> HRESULT;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn OleInitialize(reserved: Ptr) -> HRESULT;
        fn CoCreateInstance(
            clsid: *const GUID,
            outer: Ptr,
            ctx: u32,
            iid: *const GUID,
            ppv: *mut Ptr,
        ) -> HRESULT;
        fn CoTaskMemFree(p: Ptr);
        fn DoDragDrop(data: Ptr, source: Ptr, ok: u32, effect: *mut u32) -> HRESULT;
    }

    #[repr(C, align(8))]
    struct StrRet([u8; 272]);
    #[repr(C)]
    #[derive(Default)]
    struct Variant {
        vt: u16,
        r1: u16,
        r2: u16,
        r3: u16,
        val: [u64; 2],
    }
    #[repr(C)]
    struct InvokeInfo {
        cb: u32,
        mask: u32,
        hwnd: HWND,
        verb: *const i8,
        parameters: *const i8,
        directory: *const i8,
        show: i32,
        hotkey: u32,
        icon: Ptr,
    }

    unsafe fn vfn(obj: Ptr, index: usize) -> *const c_void {
        *(*(obj as *mut *const *const c_void)).add(index)
    }
    /// An owned COM interface pointer, released on drop.
    struct Com(Ptr);
    impl Com {
        fn null() -> Self {
            Com(ptr::null_mut())
        }
        fn ok(&self) -> bool {
            !self.0.is_null()
        }
        unsafe fn query(&self, iid: &GUID) -> Com {
            let mut out = ptr::null_mut();
            if self.ok() {
                let f: extern "system" fn(Ptr, *const GUID, *mut Ptr) -> HRESULT =
                    mem::transmute(vfn(self.0, 0));
                f(self.0, iid, &mut out);
            }
            Com(out)
        }
    }
    impl Drop for Com {
        fn drop(&mut self) {
            if self.ok() {
                unsafe {
                    let f: extern "system" fn(Ptr) -> u32 = mem::transmute(vfn(self.0, 2));
                    f(self.0);
                }
            }
        }
    }
    struct Pidl(Ptr);
    impl Pidl {
        fn bytes(&self) -> Option<&[u8]> {
            if self.0.is_null() {
                return None;
            }
            #[link(name = "shell32")]
            unsafe extern "system" {
                fn ILGetSize(pidl: *const c_void) -> u32;
            }
            let size = unsafe { ILGetSize(self.0) } as usize;
            (size >= 2).then(|| unsafe { std::slice::from_raw_parts(self.0.cast(), size) })
        }
    }
    impl Drop for Pidl {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CoTaskMemFree(self.0) }
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    unsafe fn from_wide(p: *const u16) -> String {
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }

    pub fn init() {
        unsafe {
            OleInitialize(ptr::null_mut());
        }
    }

    fn known_folder(id: &GUID) -> Option<PathBuf> {
        unsafe {
            let mut out = ptr::null_mut();
            let hr = SHGetKnownFolderPath(id, 0, ptr::null_mut(), &mut out);
            let result = (hr >= 0 && !out.is_null()).then(|| PathBuf::from(from_wide(out)));
            if !out.is_null() {
                CoTaskMemFree(out as Ptr);
            }
            result
        }
    }
    /// The signed-in user's Desktop folder, then the shared Public Desktop.
    pub fn desktop_folders() -> Vec<PathBuf> {
        [&FOLDERID_DESKTOP, &FOLDERID_PUBLIC_DESKTOP]
            .into_iter()
            .filter_map(known_folder)
            .collect()
    }
    pub fn downloads_folder() -> Option<PathBuf> {
        known_folder(&FOLDERID_DOWNLOADS)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads")))
            .filter(|p| p.is_dir())
    }
    pub fn pictures_folder() -> Option<PathBuf> {
        known_folder(&FOLDERID_PICTURES)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Pictures")))
            .filter(|p| p.is_dir())
    }
    #[allow(dead_code)]
    pub fn documents_folder() -> Option<PathBuf> {
        known_folder(&FOLDERID_DOCUMENTS)
            .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")))
            .filter(|p| p.is_dir())
    }

    thread_local! {
        static LAST_POSITION: std::cell::Cell<(i32, usize, usize)> = const { std::cell::Cell::new((0, 0, 0)) };
    }
    /// Explorer's desktop icon view, reached through IShellWindows (SWC_DESKTOP).
    /// Exact Shell item IDs captured alongside the worker's initial audit.
    /// Reusing labels requires a bijection with the current IDs of this view.
    pub(crate) struct DropIdentityCache {
        items: std::collections::HashMap<Vec<u8>, ViewItem>,
    }

    pub struct DesktopView {
        view: Com,
        folder: Com,
        list: HWND,
    }
    fn native_icon_menu_point(x:i32,y:i32)->Option<isize>{
        let cx=x.checked_add(24)?;
        let cy=y.checked_add(24)?;
        if !(i16::MIN as i32..=i16::MAX as i32).contains(&cx)
            || !(i16::MIN as i32..=i16::MAX as i32).contains(&cy)
            || cx<=-10000 || cy<=-10000 {
            return None;
        }
        Some(((cx as u16 as u32) | ((cy as u16 as u32)<<16)) as isize)
    }
    #[cfg(test)]
    mod native_menu_point_tests {
        use super::*;
        #[test]
        fn encodes_native_screen_point_and_rejects_parked_icon() {
            let point=native_icon_menu_point(1816,142).unwrap();
            assert_eq!((point as u32 & 0xffff) as i16,1840);
            assert_eq!(((point as u32 >> 16) & 0xffff) as i16,166);
            let left=native_icon_menu_point(-1920,300).unwrap();
            assert_eq!((left as u32 & 0xffff) as i16,-1896);
            assert!(native_icon_menu_point(-20000,-20001).is_none());
            assert!(native_icon_menu_point(i32::MAX,10).is_none());
            assert!(native_icon_menu_point(10,i32::MAX).is_none());
        }
    }


    /// Diagnostic: Explorer icon enumerations per second, per thread. Each one
    /// is ~180 cross-process calls into Explorer's desktop thread.
    fn explorer_enumeration_diagnostics() {
        use std::cell::Cell;
        thread_local! { static WINDOW: Cell<(u64, u32)> = const { Cell::new((0, 0)) }; }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs()).unwrap_or(0);
        WINDOW.with(|w| {
            let (second, count) = w.get();
            if second == now {
                w.set((second, count + 1));
            } else {
                if second != 0 && count > 2 {
                    let thread = std::thread::current();
                    crate::host::event("INFO", "shell.explorer_enumerations", &format!(
                        "thread={} per_second={count}", thread.name().unwrap_or("unnamed")));
                }
                w.set((now, 1));
            }
        });
    }

    unsafe fn parent_and_child(path: &str) -> Option<(Com, Ptr, Pidl)> {
        let mut pidl = ptr::null_mut();
        if SHParseDisplayName(
            wide(&path.replace('/', "\\")).as_ptr(),
            ptr::null_mut(),
            &mut pidl,
            0,
            ptr::null_mut(),
        ) < 0
            || pidl.is_null()
        {
            return None;
        }
        let pidl = Pidl(pidl);
        let mut folder = ptr::null_mut();
        let mut child = ptr::null_mut();
        if SHBindToParent(pidl.0, &IID_ISHELL_FOLDER, &mut folder, &mut child) < 0 {
            return None;
        }
        Some((Com(folder), child, pidl))
    }

}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
mod fallback {
    use super::*;
    pub fn init() {}
    pub fn desktop_folders() -> Vec<PathBuf> {
        vec![]
    }
    pub fn downloads_folder() -> Option<PathBuf> {
        None
    }
    pub fn pictures_folder() -> Option<PathBuf> {
        None
    }
    #[allow(dead_code)]
    pub fn documents_folder() -> Option<PathBuf> {
        None
    }
    pub struct DesktopView;
    impl DesktopView {
        pub fn acquire() -> Option<Self> {
            None
        }
        pub fn items(&self) -> Vec<ViewItem> {
            vec![]
        }
        pub fn select_explorer_icon(&self, _: &str) -> Result<(), String> {
            Err("Explorer Desktop View is only available on Windows".into())
        }
        pub fn request_explorer_context_menu(&self, _: &str) -> Result<(), String> {
            Err("Explorer Desktop View is only available on Windows".into())
        }
        pub fn explorer_bridge_probe(&self, _: Option<&str>) -> serde_json::Value {
            serde_json::json!({"bridge":"explorer-desktop-view-lab","available":false})
        }
        pub fn position(&self, _: &[(String, (i32, i32))]) -> usize {
            0
        }
        pub fn disable_auto_arrange(&self) -> bool {
            false
        }
        pub fn set_icons_visible(&self, _: bool) -> bool {
            false
        }
        pub fn icons_visible(&self) -> Option<bool> {
            None
        }
        pub fn auto_arrange(&self) -> Option<bool> {
            None
        }
        pub fn snap_to_grid(&self) -> Option<bool> {
            None
        }
        pub fn set_snap_to_grid(&self, _: bool) -> bool {
            false
        }
        pub fn spacing(&self) -> Option<(i32, i32)> {
            None
        }
        pub fn icon_size(&self) -> Option<i32> {
            None
        }
    }
    pub fn image(_: &str, _: i32) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
    pub fn classic_context_menu(_: &str, _: i32, _: i32, _: bool) -> MenuChoice {
        MenuChoice::Nothing
    }
    pub enum DesktopChoice {
        Nothing,
        Windows,
        Ours(usize),
    }
    pub fn desktop_menu(_: i32, _: i32, _: &[&str]) -> DesktopChoice {
        DesktopChoice::Nothing
    }
    pub fn drag(_: &str) -> u32 {
        0
    }
    pub fn move_into_folder(_: &Path, _: &Path) -> Result<PathBuf, String> {
        Err("Only available on Windows".into())
    }
    pub fn properties(_: &str) -> Result<(), String> {
        Err("Only available on Windows".into())
    }
    pub fn reveal_in_explorer(_: &str) -> Result<(), String> {
        Err("Only available on Windows".into())
    }
    pub fn copy_files_to_clipboard(_: &[String], _: bool) -> Result<(), String> {
        Err("Only available on Windows".into())
    }
    pub fn read_file_clipboard() -> Result<Option<FileClipboard>, String> {
        Err("Only available on Windows".into())
    }
    pub fn preflight_folder_paste(_: &[PathBuf], _: &Path, _: bool) -> Result<(), String> {
        Err("Only available on Windows".into())
    }
    pub fn paste_files_into_folder(_: &FileClipboard, _: &Path) -> Result<usize, String> {
        Err("Only available on Windows".into())
    }
    pub fn rename(_: &str, _: &str) -> Result<PathBuf, String> {
        Err("Only available on Windows".into())
    }
    pub fn recycle(_: &str) -> Result<(), String> {
        Err("Only available on Windows".into())
    }
}
#[cfg(not(windows))]
pub use fallback::*;

/// Every visible item directly on the desktop (user and Public Desktop),
/// skipping hidden/system files such as desktop.ini.
pub fn desktop_entries() -> Vec<PathBuf> {
    let mut out = vec![];
    for folder in desktop_folders() {
        let Ok(read) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case("desktop.ini") || name.starts_with('~') {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if let Ok(meta) = entry.metadata() {
                    if meta.file_attributes() & 0x6 != 0 {
                        continue;
                    }
                }
            }
            out.push(path);
        }
    }
    out
}

/// Whether a path sits directly in one of the desktop folders.
pub fn on_desktop(path: &str, folders: &[PathBuf]) -> bool {
    let p = std::path::Path::new(path);
    p.parent().is_some_and(|parent| {
        folders
            .iter()
            .any(|f| f.as_os_str().eq_ignore_ascii_case(parent.as_os_str()))
    })
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore = "requires the interactive Explorer Desktop"]
    fn live_drop_identity_cache_matches_full_enumeration() {
        super::init();
        let view = super::DesktopView::acquire().expect("interactive Explorer Desktop required");
        let (_, cache) = view.items_with_drop_identities();
        let cache = cache.expect("unique Shell identities");
        let order = |mut items: Vec<super::ViewItem>| {
            items.sort_by(|a, b| a.path.cmp(&b.path));
            items.into_iter().map(|item| (item.path, item.label, item.pos)).collect::<Vec<_>>()
        };
        let mut full_times = [0; 3];
        let mut cached_times = [0; 3];
        let mut item_count = 0;
        for index in 0..full_times.len() {
            let started = std::time::Instant::now();
            let full = view.items();
            full_times[index] = started.elapsed().as_micros();
            let started = std::time::Instant::now();
            let cached = view.drop_positions(&cache).expect("Desktop inventory changed during audit");
            cached_times[index] = started.elapsed().as_micros();
            item_count = cached.len();
            assert!(item_count > 0);
            assert_eq!(order(full), order(cached));
        }
        full_times.sort_unstable();
        cached_times.sort_unstable();
        let full = view.items();
        let anchor = &full[0].path;
        let mut positions: Vec<_> = full.iter().map(|item| item.pos).collect();
        let (batched, _, source) = view.items_for_drop(anchor, &full, Some((&cache, &full, &positions)), None);
        assert_eq!(source, "listview");
        assert_eq!(order(full.clone()), order(batched));
        positions[0].0 += 1;
        let (fallback, _, source) = view.items_for_drop(anchor, &full, Some((&cache, &full, &positions)), None);
        assert_eq!(source, "shell");
        assert_eq!(order(full), order(fallback));
        println!("items={item_count} full_median_us={} cached_median_us={}", full_times[1], cached_times[1]);
    }

    #[test]
    fn physical_move_changes_disk_location_without_overwriting_collisions() {
        super::init();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "desktop-spaces-safe-move-{}-{nanos}",
            std::process::id()
        ));
        let from = root.join("source");
        let to = root.join("destination");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        let original = from.join("demo.txt");
        std::fs::write(&original, b"test only").unwrap();
        let moved = super::move_into_folder(&original, &to).unwrap();
        assert!(!original.exists());
        assert_eq!(std::fs::read(&moved).unwrap(), b"test only");
        std::fs::write(&original, b"new file").unwrap();
        assert!(super::move_into_folder(&original, &to).is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"new file");
        assert_eq!(std::fs::read(&moved).unwrap(), b"test only");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn explorer_returns_an_icon_for_a_file() {
        super::init();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), r"\Cargo.toml");
        let image = super::image(path, 64);
        assert!(image.is_some(), "no icon for {path}");
        let (w, h, px) = image.unwrap();
        assert!(w > 0 && h > 0 && px.len() == (w * h * 4) as usize);
        let slashes = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
        assert!(
            super::image(slashes, 64).is_some(),
            "forward slashes rejected"
        );
    }
}
