use super::*;

fn double_null(paths: &[&str]) -> Vec<u16> {
    let mut v = vec![];
    for p in paths {
        v.extend(p.encode_utf16());
        v.push(0);
    }
    v.push(0);
    v
}
unsafe fn file_op(owner: HWND, func: u16, from: &str, to: Option<&str>) -> Result<(), String> {
    let from = double_null(&[from]);
    let to = to.map(|t| double_null(&[t]));
    let mut op: SHFILEOPSTRUCTW = mem::zeroed();
    op.hwnd = owner;
    op.wFunc = func as u32;
    op.pFrom = from.as_ptr();
    op.pTo = to.as_ref().map_or(ptr::null(), |t| t.as_ptr());
    // A drop on the drawn Recycle Bin must never silently become a
    // permanent deletion on a volume where recycling is unavailable.
    let warning = if func == FO_DELETE {
        FOF_WANTNUKEWARNING
    } else {
        0
    };
    op.fFlags = (FOF_ALLOWUNDO | FOF_NOCONFIRMMKDIR | warning) as u16;
    let r = SHFileOperationW(&mut op);
    if r != 0 {
        Err(format!(
            "Windows could not complete the file operation (code {r})."
        ))
    } else if op.fAnyOperationsAborted != 0 {
        Err("The file operation was cancelled.".into())
    } else {
        Ok(())
    }
}
/// Windows shell move (including between drives), with Explorer's undo
/// support. Refuse to overwrite or auto-rename any existing destination.
pub fn move_into_folder(path: &Path, folder: &Path) -> Result<PathBuf, String> {
    let name = path.file_name().ok_or("Invalid file or folder name")?;
    let target = folder.join(name);
    if path == target {
        return Ok(target);
    }
    if !path.exists() {
        return Err("The source file no longer exists".into());
    }
    if path.is_dir() && folder.starts_with(path) {
        return Err("Cannot move a folder into itself".into());
    }
    if target.exists() {
        return Err(format!(
            "Destination already contains '{}'; no file was replaced",
            name.to_string_lossy()
        ));
    }
    if !folder.is_dir() {
        return Err("Destination folder no longer exists".into());
    }
    unsafe {
        file_op(
            crate::host::window(),
            FO_MOVE,
            &path.to_string_lossy(),
            Some(&folder.to_string_lossy()),
        )?;
    }
    if !target.exists() {
        return Err("Windows reported a move, but the destination could not be verified; check both folders".into());
    }
    Ok(target)
}

#[link(name = "shell32")]
extern "system" {
    fn SHObjectProperties(
        hwnd: HWND,
        object_type: u32,
        object_name: *const u16,
        property_page: *const u16,
    ) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> Ptr;
    fn GlobalLock(handle: Ptr) -> Ptr;
    fn GlobalUnlock(handle: Ptr) -> i32;
    fn GlobalFree(handle: Ptr) -> Ptr;
}
pub fn properties(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err("File no longer exists".into());
    }
    // std::fs accepts mixed separators, but SHObjectProperties can reject
    // paths with forward slashes (seen in demo and imported Workspace).
    let normalized = path.replace('/', "\\");
    let path = wide(&normalized);
    // SHOP_FILEPATH, the real Windows file/folder Properties dialog.
    let ok =
        unsafe { SHObjectProperties(crate::host::window(), 2, path.as_ptr(), ptr::null()) };
    if ok != 0 {
        Ok(())
    } else {
        Err("Windows could not show Properties for this item".into())
    }
}
pub fn reveal_in_explorer(path: &str) -> Result<(), String> {
    if !Path::new(path).exists() {
        return Err("File no longer exists".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{path}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Explorer could not open: {e}"))
}
/// CF_HDROP uses a DROPFILES header and double-NUL UTF-16 paths.
fn encode_file_drop(paths: &[String]) -> Vec<u8> {
    let mut payload = vec![0u8; 20];
    payload[0..4].copy_from_slice(&20u32.to_le_bytes());
    payload[16..20].copy_from_slice(&1u32.to_le_bytes()); // fWide TRUE
    for path in paths {
        for c in path
            .replace('/', std::path::MAIN_SEPARATOR_STR)
            .encode_utf16()
        {
            payload.extend_from_slice(&c.to_le_bytes());
        }
        payload.extend_from_slice(&0u16.to_le_bytes());
    }
    payload.extend_from_slice(&0u16.to_le_bytes());
    payload
}
#[test]
fn windows_file_clipboard_payload_is_unicode_and_double_nul_terminated() {
    let files = vec![
        "C:/Users/Test/Desktop/งานไทย.txt".to_string(),
        "C:/Users/Test/Desktop/backup.zip".to_string(),
    ];
    let payload = encode_file_drop(&files);
    assert_eq!(&payload[0..4], &20u32.to_le_bytes());
    assert_eq!(&payload[16..20], &1u32.to_le_bytes());
    let data: Vec<u16> = payload[20..]
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    assert_eq!(data[data.len() - 2..], [0, 0]);
    let joined = String::from_utf16_lossy(&data);
    assert!(joined.contains("งานไทย.txt"));
    assert!(joined.contains("backup.zip"));
}
/// Read only file objects from Explorer's current clipboard; never treat
/// text or a shell PIDL as a path. Own no clipboard storage.
pub fn read_file_clipboard() -> Result<Option<FileClipboard>, String> {
    unsafe {
        if OpenClipboard(crate::host::window()) == 0 {
            return Err("The Windows clipboard is busy; retry Paste".into());
        }
        let result = (|| -> Result<Option<FileClipboard>, String> {
            let drop = GetClipboardData(15); // CF_HDROP
            if drop.is_null() {
                return Ok(None);
            }
            let files = drop as HDROP;
            let count = DragQueryFileW(files, u32::MAX, ptr::null_mut(), 0);
            if count == 0 {
                return Ok(None);
            }
            if count > 4096 {
                return Err("Too many clipboard files".into());
            }
            let mut paths = Vec::with_capacity(count as usize);
            for i in 0..count {
                let n = DragQueryFileW(files, i, ptr::null_mut(), 0);
                if n == 0 || n > 32767 {
                    return Err("Invalid clipboard file path".into());
                }
                let mut buf = vec![0u16; n as usize + 1];
                if DragQueryFileW(files, i, buf.as_mut_ptr(), buf.len() as u32) != n {
                    return Err("Could not read a clipboard file path".into());
                }
                paths.push(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])));
            }
            let mut cut = false;
            let cf = RegisterClipboardFormatW(wide("Preferred DropEffect").as_ptr());
            if cf != 0 {
                let effect = GetClipboardData(cf);
                if !effect.is_null() {
                    let data = GlobalLock(effect as Ptr);
                    if !data.is_null() {
                        cut = ptr::read_unaligned(data as *const u32) & 2 != 0;
                        GlobalUnlock(effect as Ptr);
                    }
                }
            }
            Ok(Some(FileClipboard { paths, cut }))
        })();
        CloseClipboard();
        result
    }
}

/// Validate every source and destination before Windows Shell starts any
/// file operation. No overwrite, recursive folder move or partial batch.
pub fn preflight_folder_paste(
    paths: &[PathBuf],
    folder: &Path,
    cut: bool,
) -> Result<(), String> {
    if !folder.is_dir() {
        return Err("Target folder does not exist".into());
    }
    if paths.is_empty() {
        return Err("Clipboard has no files".into());
    }
    let mut names = std::collections::HashSet::new();
    for path in paths {
        if !path.exists() {
            return Err(format!("Clipboard source is missing: {}", path.display()));
        }
        let name = path
            .file_name()
            .ok_or("Clipboard source has no file name")?;
        if !names.insert(name.to_string_lossy().to_lowercase()) {
            return Err("Clipboard contains multiple items with the same name".into());
        }
        let target = folder.join(name);
        if path.as_path() == target.as_path() {
            return Err("The file is already in this folder".into());
        }
        if target.exists() {
            return Err(format!(
                "Destination already has '{}'; nothing was replaced",
                name.to_string_lossy()
            ));
        }
        if path.is_dir()
            && (folder.starts_with(path)
                || folder
                    .canonicalize()
                    .ok()
                    .zip(path.canonicalize().ok())
                    .is_some_and(|(f, p)| f.starts_with(p)))
        {
            return Err("Cannot paste a folder into itself or its subfolder".into());
        }
        if cut && path.parent().is_some_and(|p| p == folder) {
            return Err("Cannot move a file into its current folder".into());
        }
    }
    Ok(())
}
pub fn paste_files_into_folder(
    clipboard: &FileClipboard,
    folder: &Path,
) -> Result<usize, String> {
    preflight_folder_paste(&clipboard.paths, folder, clipboard.cut)?;
    let names: Vec<String> = clipboard
        .paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let from = double_null(&refs);
    let dst = double_null(&[&folder.to_string_lossy()]);
    unsafe {
        let mut op: SHFILEOPSTRUCTW = mem::zeroed();
        op.hwnd = crate::host::window();
        op.wFunc = if clipboard.cut {
            FO_MOVE as u32
        } else {
            FO_COPY as u32
        };
        op.pFrom = from.as_ptr();
        op.pTo = dst.as_ptr();
        op.fFlags = FOF_ALLOWUNDO as u16; // Explorer handles confirmation
        let result = SHFileOperationW(&mut op);
        if result != 0 {
            return Err(format!(
                "Windows file {} failed (code {result})",
                if clipboard.cut { "move" } else { "copy" }
            ));
        }
        if op.fAnyOperationsAborted != 0 {
            return Err("Windows file operation was cancelled".into());
        }
    }
    let success = clipboard
        .paths
        .iter()
        .filter(|p| p.file_name().is_some_and(|name| folder.join(name).exists()))
        .count();
    if success != clipboard.paths.len() {
        return Err(format!(
            "Windows completed {success}/{} file operations; check both folders",
            clipboard.paths.len()
        ));
    }
    Ok(success)
}

#[test]
fn folder_paste_preflight_rejects_conflicts_cycles_and_missing_sources() {
    let root = std::env::temp_dir().join(format!(
        "desktopcraft-paste-validation-{}",
        std::process::id()
    ));
    let from = root.join("from");
    let dst = root.join("to");
    std::fs::create_dir_all(&from).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    let source = from.join("ภาษาไทย.txt");
    std::fs::write(&source, b"safe").unwrap();
    assert!(preflight_folder_paste(&[source.clone()], &dst, false).is_ok());
    std::fs::write(dst.join("ภาษาไทย.txt"), b"existing").unwrap();
    assert!(preflight_folder_paste(&[source.clone()], &dst, false).is_err());
    assert_eq!(std::fs::read(dst.join("ภาษาไทย.txt")).unwrap(), b"existing");
    assert!(preflight_folder_paste(&[from.join("missing.zip")], &dst, true).is_err());
    let inner = from.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    assert!(preflight_folder_paste(&[from.clone()], &inner, true).is_err());
    assert!(preflight_folder_paste(&[source.clone(), source.clone()], &dst, false).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
/// Actual Explorer-compatible Windows file clipboard. No disk mutation:
/// Explorer performs moves only when the user pastes a cut file.
pub fn copy_files_to_clipboard(paths: &[String], cut: bool) -> Result<(), String> {
    if paths.is_empty() || paths.iter().any(|p| !Path::new(p).exists()) {
        return Err("Select existing files or folders first".into());
    }
    let payload = encode_file_drop(paths);
    unsafe {
        if OpenClipboard(crate::host::window()) == 0 {
            return Err("Windows clipboard is currently busy".into());
        }
        let result = (|| -> Result<(), String> {
            if EmptyClipboard() == 0 {
                return Err("Could not clear clipboard".into());
            }
            let mem = GlobalAlloc(0x2, payload.len()); // GMEM_MOVEABLE
            if mem.is_null() {
                return Err("Clipboard allocation failed".into());
            }
            let data = GlobalLock(mem);
            if data.is_null() {
                GlobalFree(mem);
                return Err("Clipboard could not lock data".into());
            }
            ptr::copy_nonoverlapping(payload.as_ptr(), data as *mut u8, payload.len());
            GlobalUnlock(mem);
            if SetClipboardData(15, mem as _).is_null() {
                GlobalFree(mem);
                return Err("Windows rejected the file list".into());
            }
            // Standard Shell Preferred DropEffect: 2=move, 1=copy.
            let cf = RegisterClipboardFormatW(wide("Preferred DropEffect").as_ptr());
            if cf != 0 {
                let mem = GlobalAlloc(0x2, 4);
                if !mem.is_null() {
                    let data = GlobalLock(mem);
                    if !data.is_null() {
                        *(data as *mut u32) = if cut { 2 } else { 1 };
                        GlobalUnlock(mem);
                        if SetClipboardData(cf, mem as _).is_null() {
                            GlobalFree(mem);
                        }
                    } else {
                        GlobalFree(mem);
                    }
                }
            }
            Ok(())
        })();
        CloseClipboard();
        result
    }
}
pub fn rename(path: &str, new_name: &str) -> Result<PathBuf, String> {
    let source = PathBuf::from(path);
    let target = source.with_file_name(new_name);
    unsafe {
        file_op(
            crate::host::window(),
            FO_RENAME,
            path,
            Some(&target.to_string_lossy()),
        )?
    };
    Ok(target)
}
/// Send to the Recycle Bin (Windows asks for confirmation as usual).
pub fn recycle(path: &str) -> Result<(), String> {
    unsafe { file_op(crate::host::window(), FO_DELETE, path, None) }
}

