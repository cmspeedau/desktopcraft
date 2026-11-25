use super::*;

pub fn open(path: &str) -> Result<(), String> {
    unsafe {
        let result = ShellExecuteW(
            window(),
            wide("open").as_ptr(),
            wide(path).as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        ) as isize;
        if result > 32 {
            Ok(())
        } else {
            Err(format!(
                "Windows could not open this item (error {result})."
            ))
        }
    }
}
pub fn choose_file() -> Option<PathBuf> {
    unsafe {
        let mut buffer = [0u16; 32768];
        let mut ofn: OPENFILENAMEW = mem::zeroed();
        let title = wide("Add a file or shortcut");
        let filter = wide("All files\0*.*\0");
        ofn.lStructSize = mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = window();
        ofn.lpstrFile = buffer.as_mut_ptr();
        ofn.nMaxFile = buffer.len() as u32;
        ofn.lpstrTitle = title.as_ptr();
        ofn.lpstrFilter = filter.as_ptr();
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut ofn) != 0 {
            Some(PathBuf::from(String::from_utf16_lossy(
                &buffer[..buffer.iter().position(|&x| x == 0).unwrap_or(0)],
            )))
        } else {
            None
        }
    }
}
pub fn choose_folder() -> Option<PathBuf> {
    unsafe {
        let hr = CoInitializeEx(ptr::null_mut(), 2);
        let mut info: BrowseInfo = mem::zeroed();
        let title = wide("Choose a folder to show in your workspace");
        info.owner = window();
        info.title = title.as_ptr();
        info.flags = 0x41;
        let item = SHBrowseForFolderW(&mut info);
        let mut output = None;
        if !item.is_null() {
            let mut path = [0u16; 260];
            if SHGetPathFromIDListW(item as _, path.as_mut_ptr()) != 0 {
                output = Some(PathBuf::from(String::from_utf16_lossy(
                    &path[..path.iter().position(|&x| x == 0).unwrap_or(0)],
                )))
            }
            CoTaskMemFree(item as _);
        }
        if hr >= 0 {
            CoUninitialize()
        }
        output
    }
}
