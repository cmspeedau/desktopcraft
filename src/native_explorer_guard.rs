//! Read-only guards for native operations on the COM-acquired Desktop view.
use crate::shell::{DesktopView, ViewItem};
use winapi::shared::windef::HWND;
use winapi::um::winuser::{GetClassNameW, GetShellWindow, GetWindowThreadProcessId, IsWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExplorerOwner {
    pub pid: u32,
    pub thread: u32,
}

fn verified_owner(list: ExplorerOwner, shell: ExplorerOwner) -> Option<ExplorerOwner> {
    (list.thread != 0 && list.pid != 0 && shell.thread != 0 && list.pid == shell.pid)
        .then_some(list)
}

fn window_owner(window: HWND) -> ExplorerOwner {
    let mut pid = 0;
    let thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
    ExplorerOwner { pid, thread }
}

pub(crate) fn desktop_owner(view: &DesktopView) -> Result<ExplorerOwner, String> {
    let list = view.list_window();
    if list.is_null() || unsafe { IsWindow(list) } == 0 {
        return Err("Explorer Desktop ListView window is unavailable".into());
    }
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(list, class.as_mut_ptr(), class.len() as i32) };
    if len <= 0 || !class[..len as usize].iter().copied().eq("SysListView32".encode_utf16()) {
        return Err("Explorer Desktop handle is not a verified SysListView32".into());
    }
    let shell = unsafe { GetShellWindow() };
    verified_owner(window_owner(list), window_owner(shell))
        .ok_or_else(|| "Could not verify the Desktop ListView thread and Shell owner PID".into())
}

pub(crate) fn parked_icon_count(items: &[ViewItem]) -> usize {
    // Ordinary negative virtual-desktop coordinates are valid.
    items.iter().filter(|item| item.pos.0 <= -10000 || item.pos.1 <= -10000).count()
}

pub(crate) fn validate_layout_snapshot(items: &[ViewItem]) -> Result<(), String> {
    if !(1..=crate::native_filter_protocol::MAX_ITEMS).contains(&items.len()) || parked_icon_count(items) != 0 {
        return Err("Explorer Desktop identity count or parked icon positions unsafe".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_or_mismatched_shell_and_list_owners() {
        let list = ExplorerOwner { pid: 12, thread: 34 };
        let shell = ExplorerOwner { pid: 12, thread: 56 };
        let missing = ExplorerOwner { pid: 0, thread: 0 };
        assert_eq!(verified_owner(list, shell), Some(list));
        assert_eq!(verified_owner(list, missing), None);
        assert_eq!(verified_owner(missing, shell), None);
        assert_eq!(verified_owner(ExplorerOwner { thread: 0, ..list }, shell), None);
        assert_eq!(verified_owner(list, ExplorerOwner { pid: 78, ..shell }), None);
    }

    #[test]
    fn layout_guard_accepts_negative_monitor_origins_and_refuses_parking() {
        let mut icons: Vec<_> = (0..10).map(|index| ViewItem {
            path: format!("C:/Desktop/{index}.lnk"),
            label: index.to_string(),
            pos: (-2520 + index * 80, 120),
        }).collect();
        assert!(validate_layout_snapshot(&icons).is_ok());
        assert!(validate_layout_snapshot(&icons[..1]).is_ok());
        assert!(validate_layout_snapshot(&[]).is_err());
        for pos in [(-10000, 120), (100, -20001)] {
            icons[9].pos = pos;
            assert_eq!(parked_icon_count(&icons), 1);
            assert!(validate_layout_snapshot(&icons).is_err());
        }
    }
}
