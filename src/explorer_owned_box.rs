//! Explorer-owned icons — Phase 2 laboratory: ONE DesktopCraft box holding
//! SEVERAL real Explorer desktop icons
//! (app/docs/EXPLORER_OWNED_ICONS_ARCHITECTURE_PLAN_20261116.md, Gate 2).
//!
//! `--explorer-owned-box <items.txt> <x> <y> <columns> [seconds] [status.json] [title]`
//!
//! * Explorer keeps drawing and owning every icon. DesktopCraft draws only the
//!   box frame (header + border). The whole content area is a window-region
//!   hole, so every click, selection, marquee, F2, drag and the Windows 11
//!   context menu belong to Explorer's own desktop list view.
//! * Icons are laid out on Explorer's own grid (its icon spacing). Explorer may
//!   snap them, so the box is fitted to the rectangles Explorer actually drew,
//!   read through the accessibility interface (MSAA), never to the positions
//!   we asked for.
//! * Membership is model-only: an icon dragged out of the box is unpinned, an
//!   icon dragged in is pinned. Nothing on disk is moved, copied or deleted.
//! * Dragging the header moves the box and its icons. On exit every icon the
//!   box placed goes back to where it was.
//! * Every 0.6 s the lab writes a status file: each member's rectangle,
//!   selected/focused state, icons inside the box that are not members, and
//!   the membership events.

use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Requested positions for `n` icons in `columns` columns from `origin`,
/// one Explorer grid cell (`cell`) apart.
pub fn grid_targets(origin: (i32, i32), cell: (i32, i32), columns: usize, n: usize) -> Vec<(i32, i32)> {
    let columns = columns.max(1);
    (0..n).map(|i| (origin.0 + (i % columns) as i32 * cell.0, origin.1 + (i / columns) as i32 * cell.1)).collect()
}

/// Conservative preflight: deny moving Explorer-owned icons into cells near
/// any other desktop icon. Explorer snaps requested points to its own grid,
/// so a target within one cell width AND height of another icon is unsafe.
/// This preflight is deliberately stricter than the Shell's actual snap rule.
pub fn target_conflicts_with_foreign_icons(
    targets: &[(i32, i32)],
    foreign: &[(i32, i32)],
    cell: (i32, i32),
) -> bool {
    if cell.0 <= 0 || cell.1 <= 0 { return true; }
    targets.iter().any(|target| foreign.iter().any(|other| {
        (i64::from(target.0) - i64::from(other.0)).abs() < i64::from(cell.0)
            && (i64::from(target.1) - i64::from(other.1)).abs() < i64::from(cell.1)
    }))
}

/// Smallest rectangle [x, y, w, h] holding all `rects`, grown by `pad`.
pub fn bounding(rects: &[[i32; 4]], pad: i32) -> Option<[i32; 4]> {
    let first = rects.first()?;
    let (mut l, mut t, mut r, mut b) = (first[0], first[1], first[0] + first[2], first[1] + first[3]);
    for x in rects {
        l = l.min(x[0]); t = t.min(x[1]); r = r.max(x[0] + x[2]); b = b.max(x[1] + x[3]);
    }
    Some([l - pad, t - pad, r - l + 2 * pad, b - t + 2 * pad])
}

/// An icon belongs to the box area when its rectangle's centre is inside it.
pub fn center_inside(rect: [i32; 4], area: [i32; 4]) -> bool {
    let (cx, cy) = (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2);
    cx >= area[0] && cx < area[0] + area[2] && cy >= area[1] && cy < area[1] + area[3]
}

/// Model-only membership changes: (label, "pinned" | "unpinned").
pub fn membership_changes(members: &BTreeSet<String>, inside_now: &BTreeSet<String>) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = members.difference(inside_now).map(|l| (l.clone(), "unpinned")).collect();
    out.extend(inside_now.difference(members).map(|l| (l.clone(), "pinned")));
    out
}

/// Read item paths from a UTF-8 text file produced by PowerShell or other tools.
/// Windows PowerShell 5.1 prepends a UTF-8 BOM; it must not become part of the
/// first item's path.
pub fn parse_icon_path_list(contents: &str) -> Vec<String> {
    contents
        .lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}').trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// The MSAA desktop list exposes names and rectangles, not stable paths.
/// Reject a requested name when more than one real desktop icon has that name:
/// otherwise an unrelated user's icon could be mistaken for our test item.
pub fn ambiguous_requested_labels(all: &[String], requested: &BTreeSet<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ambiguous = BTreeSet::new();
    for label in all {
        if requested.contains(label) && !seen.insert(label.clone()) {
            ambiguous.insert(label.clone());
        }
    }
    ambiguous.into_iter().collect()
}

#[cfg(windows)]
pub use win::{run_box_lab, run_underlay_probe};
#[cfg(not(windows))]
pub fn run_underlay_probe(_: [i32; 4], _: u32) -> Value {
    json!({"started": false, "reason": "Windows only"})
}
#[cfg(not(windows))]
pub fn run_box_lab(_: &[String], _: (i32, i32), _: usize, _: u32, _: Option<std::path::PathBuf>, _: &str) -> Value {
    json!({"started": false, "reason": "Windows only"})
}

#[cfg(windows)]
pub mod win {
    use super::*;
    use crate::explorer_owned::emit;
    use crate::shell::DesktopView;
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::ptr;
    use winapi::shared::guiddef::GUID;
    use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
    use winapi::shared::windef::{HWND, POINT, RECT};
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::wingdi::*;
    use winapi::um::winuser::*;

    fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

    // ---- Minimal MSAA (oleacc) access, declared here: the vendored winapi
    // ---- has no oleacc module. IAccessible derives from IDispatch.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Variant { vt: u16, r1: u16, r2: u16, r3: u16, val: i64 }
    fn child(i: i32) -> Variant { Variant { vt: 3, r1: 0, r2: 0, r3: 0, val: i as i64 } } // VT_I4
    #[link(name = "oleacc")]
    extern "system" { fn AccessibleObjectFromWindow(hwnd: HWND, id: u32, riid: *const GUID, out: *mut *mut c_void) -> i32; }
    #[link(name = "oleaut32")]
    extern "system" { fn SysStringLen(b: *mut u16) -> u32; fn SysFreeString(b: *mut u16); }
    const IID_IACCESSIBLE: GUID = GUID { Data1: 0x618736E0, Data2: 0x3C3D, Data3: 0x11CF, Data4: [0x81, 0x0C, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71] };
    unsafe fn vfn(obj: *mut c_void, index: usize) -> *const c_void { *(*(obj as *const *const *const c_void)).add(index) }

    /// One item of Explorer's desktop list as the accessibility interface
    /// reports it: display name, drawn rectangle and state flags.
    #[derive(Clone, Debug)]
    pub struct AccItem { pub name: String, pub rect: [i32; 4], pub selected: bool, pub focused: bool }

    fn read_named_children<T>(
        list: HWND,
        mut read_item: impl FnMut(*mut c_void, Variant, String) -> Option<T>,
    ) -> Vec<T> {
        let mut out = vec![];
        unsafe {
            let mut acc: *mut c_void = ptr::null_mut();
            if AccessibleObjectFromWindow(list, 0xFFFF_FFFC, &IID_IACCESSIBLE, &mut acc) < 0 || acc.is_null() { return out; }
            let count_fn: extern "system" fn(*mut c_void, *mut i32) -> i32 = std::mem::transmute(vfn(acc, 8));
            let name_fn: extern "system" fn(*mut c_void, Variant, *mut *mut u16) -> i32 = std::mem::transmute(vfn(acc, 10));
            let release: extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(vfn(acc, 2));
            let mut n = 0i32;
            count_fn(acc, &mut n);
            for i in 1..=n.clamp(0, 20000) {
                let id = child(i);
                let mut b: *mut u16 = ptr::null_mut();
                if name_fn(acc, id, &mut b) < 0 || b.is_null() { continue; }
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(b, SysStringLen(b) as usize));
                SysFreeString(b);
                out.extend(read_item(acc, id, name));
            }
            release(acc);
        }
        out
    }

    /// Read MSAA names in child order. Layout verifies coordinates separately in IFolderView.
    pub fn list_item_names(list: HWND) -> Vec<String> {
        read_named_children(list, |_, _, name| Some(name))
    }

    pub fn list_items(list: HWND) -> Vec<AccItem> {
        read_named_children(list, |acc, id, name| unsafe {
            let state_fn: extern "system" fn(*mut c_void, Variant, *mut Variant) -> i32 = std::mem::transmute(vfn(acc, 14));
            let loc_fn: extern "system" fn(*mut c_void, *mut i32, *mut i32, *mut i32, *mut i32, Variant) -> i32 = std::mem::transmute(vfn(acc, 22));
            let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
            if loc_fn(acc, &mut x, &mut y, &mut w, &mut h, id) < 0 { return None; }
            let mut st = Variant { vt: 0, r1: 0, r2: 0, r3: 0, val: 0 };
            let state = if state_fn(acc, id, &mut st) >= 0 && st.vt == 3 { st.val as i32 as u32 } else { 0 };
            Some(AccItem { name, rect: [x, y, w, h], selected: state & 0x2 != 0, focused: state & 0x4 != 0 })
        })
    }

    const MARGIN: i32 = 10;
    const HEADER: i32 = 30;
    const PAD: i32 = 6;
    struct Member { label: String, path: String, original: Option<(i32, i32)> }
    struct BoxLab {
        view: DesktopView,
        list: HWND,
        cell: (i32, i32),
        /// Optional Explorer-parented underlay, stacked below its real ListView.
        /// It paints only the Box background; Explorer still paints all items.
        underlay: HWND,
        members: Vec<Member>,
        inside: BTreeSet<String>,
        content: [i32; 4],
        frame_at: (i32, i32),
        title: String,
        events: Vec<Value>,
        moves: usize,
        status: Option<PathBuf>,
        started: std::time::Instant,
    }
    thread_local! { static BOX: RefCell<Option<BoxLab>> = const { RefCell::new(None) }; }

    fn cell_size(view: &DesktopView) -> (i32, i32) {
        unsafe {
            let mut result = 0usize;
            let ok = SendMessageTimeoutW(view.list_window(), 0x1033, 0, 0, SMTO_ABORTIFHUNG, 300, &mut result);
            let (cx, cy) = ((result & 0xffff) as i32, ((result >> 16) & 0xffff) as i32);
            if ok != 0 && cx > 0 && cy > 0 { (cx, cy) } else { view.spacing().unwrap_or((96, 96)) }
        }
    }

    fn outer_of(content: [i32; 4]) -> [i32; 4] {
        [content[0] - MARGIN, content[1] - MARGIN - HEADER, content[2] + 2 * MARGIN, content[3] + 2 * MARGIN + HEADER]
    }

    /// Paint the Box's body *behind* native Explorer icons rather than over
    /// them. This is an independent, experimental Win32 child of the native
    /// desktop view parent. It does not touch Explorer's memory, icon data,
    /// selection or position. Real appearance must be verified separately.
    unsafe extern "system" fn underlay_proc(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_NCHITTEST => HTTRANSPARENT as LRESULT,
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                let brush = CreateSolidBrush(RGB(0x22, 0x2D, 0x3B));
                FillRect(dc, &rect, brush);
                DeleteObject(brush as _);
                EndPaint(hwnd, &ps);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    /// Coordinates in Explorer's parent client space, *not* DesktopCraft's.
    /// The parent is discovered afresh when Explorer restarts.
    unsafe fn position_underlay(list: HWND, underlay: HWND, content: [i32; 4]) -> bool {
        if underlay.is_null() || IsWindow(list) == 0 { return false; }
        let parent = GetParent(list);
        if parent.is_null() || GetParent(underlay) != parent { return false; }
        let mut position = POINT { x: content[0], y: content[1] };
        if ScreenToClient(parent, &mut position) == 0 { return false; }
        SetWindowPos(underlay, HWND_BOTTOM, position.x, position.y,
            content[2], content[3], SWP_NOACTIVATE | SWP_SHOWWINDOW) != 0
    }

    unsafe fn create_underlay(list: HWND, content: [i32; 4]) -> HWND {
        let parent = GetParent(list);
        if parent.is_null() { return ptr::null_mut(); }
        let class = wide("DesktopCraftExplorerUnderlay");
        let instance = GetModuleHandleW(ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(underlay_proc),
            hInstance: instance,
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class.as_ptr(), wide("DesktopCraft native icon background").as_ptr(),
            WS_CHILD | WS_VISIBLE, 0, 0, 1, 1, parent, ptr::null_mut(), instance, ptr::null_mut());
        if !hwnd.is_null() && !position_underlay(list, hwnd, content) {
            DestroyWindow(hwnd);
            return ptr::null_mut();
        }
        hwnd
    }

    /// Fit the frame to `content`: move/resize it and cut the content hole.
    fn fit(hwnd: HWND, content: [i32; 4]) {
        let o = outer_of(content);
        unsafe {
            SetWindowPos(hwnd, ptr::null_mut(), o[0], o[1], o[2], o[3], SWP_NOZORDER | SWP_NOACTIVATE);
            let all = CreateRectRgn(0, 0, o[2], o[3]);
            let gap = CreateRectRgn(MARGIN, MARGIN + HEADER, MARGIN + content[2], MARGIN + HEADER + content[3]);
            CombineRgn(all, all, gap, RGN_DIFF);
            DeleteObject(gap as _);
            SetWindowRgn(hwnd, all, 1);
        }
    }

    /// Re-measure Explorer's drawn rectangles, update membership (model only)
    /// and write the status file.
    fn update(lab: &mut BoxLab) {
        let items = list_items(lab.list);
        let inside_now: BTreeSet<String> = items.iter().filter(|i| center_inside(i.rect, lab.content)).map(|i| i.name.clone()).collect();
        let t = lab.started.elapsed().as_secs_f64();
        for (label, change) in membership_changes(&lab.inside, &inside_now) {
            lab.events.push(json!({"t": (t * 10.0).round() / 10.0, "item": label, "change": change, "files_touched": 0}));
        }
        lab.inside = inside_now;
        if let Some(path) = &lab.status {
            let rows: Vec<Value> = items.iter().filter(|i| lab.inside.contains(&i.name)).map(|i| json!({
                "name": i.name, "rect": i.rect, "selected": i.selected, "focused": i.focused,
                "original_member": lab.members.iter().any(|m| m.label == i.name)})).collect();
            let status = json!({"t": t, "content": lab.content, "frame": outer_of(lab.content), "title": lab.title,
                "inside": rows, "inside_count": lab.inside.len(),
                "native_background_underlay_created": !lab.underlay.is_null(),
                "selected": items.iter().filter(|i| i.selected).map(|i| i.name.clone()).collect::<Vec<_>>(),
                "focused": items.iter().find(|i| i.focused).map(|i| i.name.clone()),
                "events": lab.events, "frame_moves": lab.moves});
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, serde_json::to_vec_pretty(&status).unwrap_or_default()).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_NCHITTEST => {
                let mut pt = POINT { x: (lp & 0xffff) as i16 as i32, y: ((lp >> 16) & 0xffff) as i16 as i32 };
                ScreenToClient(hwnd, &mut pt);
                if pt.y < HEADER { HTCAPTION as LRESULT } else { HTCLIENT as LRESULT }
            }
            WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
            WM_EXITSIZEMOVE => {
                // Header drag: move the box's icons by the same amount, then
                // fit the frame to where Explorer actually put them.
                let mut r: RECT = std::mem::zeroed();
                GetWindowRect(hwnd, &mut r);
                BOX.with(|b| if let Some(lab) = b.borrow_mut().as_mut() {
                    let (dx, dy) = (r.left - lab.frame_at.0, r.top - lab.frame_at.1);
                    let members: Vec<String> = lab.inside.iter().filter(|label| lab.members.iter().any(|m| &m.label == *label)).cloned().collect();
                    let member_paths: BTreeSet<String> = lab.members.iter()
                        .filter(|m| members.contains(&m.label))
                        .map(|m| m.path.to_lowercase()).collect();
                    let all = lab.view.items();
                    let by_label: Vec<(String, (i32, i32))> = all.iter()
                        .filter(|i| member_paths.contains(&i.path.to_lowercase()))
                        .map(|i| (i.path.clone(), (i.pos.0.saturating_add(dx), i.pos.1.saturating_add(dy)))).collect();
                    let occupied: Vec<(i32, i32)> = all.iter()
                        .filter(|i| !member_paths.contains(&i.path.to_lowercase())).map(|i| i.pos).collect();
                    let targets: Vec<(i32, i32)> = by_label.iter().map(|(_, pos)| *pos).collect();
                    if target_conflicts_with_foreign_icons(&targets, &occupied, lab.cell) {
                        lab.events.push(json!({"change": "header_move_rejected", "reason": "occupied Explorer grid cell", "files_touched": 0}));
                    } else if !by_label.is_empty() && lab.view.position(&by_label) > 0 {
                        lab.moves += 1;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(350));
                    let rects: Vec<[i32; 4]> = list_items(lab.list).into_iter().filter(|i| members.contains(&i.name)).map(|i| i.rect).collect();
                    if let Some(c) = bounding(&rects, PAD) { lab.content = c; }
                    fit(hwnd, lab.content);
                    if !lab.underlay.is_null() {
                        position_underlay(lab.list, lab.underlay, lab.content);
                    }
                    let o = outer_of(lab.content);
                    lab.frame_at = (o[0], o[1]);
                    update(lab);
                });
                InvalidateRect(hwnd, ptr::null(), 1);
                0
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let mut r: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut r);
                let body = CreateSolidBrush(RGB(0x24, 0x30, 0x40));
                FillRect(dc, &r, body);
                DeleteObject(body as _);
                let head = RECT { left: 0, top: 0, right: r.right, bottom: HEADER };
                let hb = CreateSolidBrush(RGB(0x3A, 0x4A, 0x60));
                FillRect(dc, &head, hb);
                DeleteObject(hb as _);
                SetBkMode(dc, TRANSPARENT as i32);
                SetTextColor(dc, RGB(0xF0, 0xF4, 0xF8));
                let title = BOX.with(|b| b.borrow().as_ref().map(|l| format!("{}  ·  {}", l.title, l.inside.len()))).unwrap_or_default();
                let text = wide(&title);
                let mut tr = head;
                DrawTextW(dc, text.as_ptr(), -1, &mut tr, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                EndPaint(hwnd, &ps);
                0
            }
            WM_TIMER if wp == 1 => {
                let before = BOX.with(|b| b.borrow().as_ref().map(|l| l.inside.len()));
                BOX.with(|b| if let Some(lab) = b.borrow_mut().as_mut() { update(lab); });
                let after = BOX.with(|b| b.borrow().as_ref().map(|l| l.inside.len()));
                if before != after { InvalidateRect(hwnd, ptr::null(), 1); }
                0
            }
            WM_TIMER | WM_CLOSE => { DestroyWindow(hwnd); 0 }
            WM_DESTROY => {
                BOX.with(|b| if let Some(lab) = b.borrow_mut().as_mut() {
                    if !lab.underlay.is_null() {
                        DestroyWindow(lab.underlay);
                        lab.underlay = ptr::null_mut();
                    }
                });
                PostQuitMessage(0); 0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    /// Non-destructive visual proof: render only a background below Explorer's
    /// native desktop ListView. No icons are repositioned, hidden or selected.
    /// A successful HWND proves only child stacking, NOT visible compositing.
    pub fn run_underlay_probe(content: [i32; 4], seconds: u32) -> Value {
        if content[2] < 40 || content[3] < 40 || content[2] > 4000 || content[3] > 4000 {
            return json!({"started": false, "reason": "invalid preview size"});
        }
        let Some(view) = DesktopView::acquire() else {
            return json!({"started": false, "reason": "Explorer desktop unavailable"});
        };
        if view.icons_visible() != Some(true) {
            return json!({"started": false, "reason": "native Explorer icons must be visible; no settings changed"});
        }
        let before = view.items();
        let list = view.list_window();
        let parent = unsafe { GetParent(list) };
        let hwnd = unsafe { create_underlay(list, content) };
        if hwnd.is_null() {
            return json!({"started": false, "reason": "could not create a native Explorer sibling underlay",
                "last_error": std::io::Error::last_os_error().to_string()});
        }
        let mut explorer_pid = 0u32;
        unsafe { GetWindowThreadProcessId(list, &mut explorer_pid); }
        let started = std::time::Instant::now();
        let deadline = std::time::Duration::from_secs(u64::from(seconds.clamp(2, 60)));
        while started.elapsed() < deadline {
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&mut msg);
                    DispatchMessageW(&msg);
                }
                if IsWindow(hwnd) == 0 { break; }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let sibling = unsafe { GetParent(hwnd) == parent };
        if unsafe { IsWindow(hwnd) } != 0 { unsafe { DestroyWindow(hwnd); } }
        let after = view.items();
        let same_positions = before.len() == after.len() && before.iter().all(|a| {
            after.iter().any(|b| a.path.eq_ignore_ascii_case(&b.path) && a.pos == b.pos)
        });
        json!({"started": true, "explorer_pid": explorer_pid, "shared_native_parent": sibling,
            "icons_before": before.len(), "icons_after": after.len(), "all_icon_positions_unchanged": same_positions,
            "desktop_files_touched": 0, "preview_area": content, "underlay_closed": true,
            "native_icon_paint_above_background": "UNVERIFIED: requires visible screen inspection"})
    }

    pub fn run_box_lab(paths: &[String], origin: (i32, i32), columns: usize, seconds: u32, status: Option<PathBuf>, title: &str) -> Value {
        let Some(view) = DesktopView::acquire() else {
            return json!({"started": false, "reason": "Explorer desktop view unavailable"});
        };
        if view.icons_visible() != Some(true) {
            return json!({"started": false, "reason": "Explorer desktop icons are hidden; run --recover-desktop or close the program hiding them (DesktopCraft)"});
        }
        let all = view.items();
        let members: Vec<Member> = paths.iter().filter_map(|p| all.iter().find(|i| i.path.eq_ignore_ascii_case(p))
            .map(|i| Member { label: i.label.clone(), path: i.path.clone(), original: Some(i.pos) })).collect();
        let missing: Vec<&String> = paths.iter().filter(|p| !all.iter().any(|i| i.path.eq_ignore_ascii_case(p))).collect();
        if members.is_empty() {
            return json!({"started": false, "reason": "none of the paths is an icon in Explorer's desktop view", "missing": missing});
        }
        let requested_labels: BTreeSet<String> = members.iter().map(|m| m.label.clone()).collect();
        let desktop_labels: Vec<String> = all.iter().map(|item| item.label.clone()).collect();
        let ambiguous = ambiguous_requested_labels(&desktop_labels, &requested_labels);
        if !ambiguous.is_empty() {
            return json!({"started": false, "reason": "duplicate Explorer display names cannot be identified safely by MSAA; no icons moved", "ambiguous": ambiguous});
        }
        let unique_paths: BTreeSet<String> = members.iter().map(|m| m.path.to_lowercase()).collect();
        if unique_paths.len() != members.len() {
            return json!({"started": false, "reason": "duplicate item paths in requested box; no icons moved"});
        }
        let cell = cell_size(&view);
        if view.auto_arrange() != Some(false) {
            return json!({"started": false, "reason": "Explorer Auto arrange must be OFF before testing; DesktopCraft did not change the setting"});
        }
        let targets = grid_targets(origin, cell, columns, members.len());
        let foreign_positions: Vec<(i32, i32)> = all.iter()
            .filter(|i| !unique_paths.contains(&i.path.to_lowercase()))
            .map(|i| i.pos).collect();
        if target_conflicts_with_foreign_icons(&targets, &foreign_positions, cell) {
            return json!({"started": false, "reason": "requested Explorer grid cells are occupied or too close to other desktop icons; no icons moved", "origin": [origin.0, origin.1], "cell": [cell.0, cell.1]});
        }
        let moves: Vec<(String, (i32, i32))> = members.iter().zip(&targets).map(|(m, t)| (m.path.clone(), *t)).collect();
        let placed = view.position(&moves);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let list = view.list_window();
        let labels: BTreeSet<String> = members.iter().map(|m| m.label.clone()).collect();
        let drawn: Vec<AccItem> = list_items(list).into_iter().filter(|i| labels.contains(&i.name)).collect();
        let Some(content) = bounding(&drawn.iter().map(|i| i.rect).collect::<Vec<_>>(), PAD) else {
            let _ = view.position(&members.iter().filter_map(|m| m.original.map(|o| (m.path.clone(), o))).collect::<Vec<_>>());
            return json!({"started": false, "reason": "Explorer's accessibility interface reported no rectangles for the box icons"});
        };
        let outer = outer_of(content);
        let mut explorer_pid = 0u32;
        unsafe { GetWindowThreadProcessId(list, &mut explorer_pid) };
        let foreign_inside: Vec<String> = list_items(list).into_iter()
            .filter(|i| !labels.contains(&i.name) && center_inside(i.rect, content)).map(|i| i.name).collect();
        // Abort before creating a Box frame if snapping pulled any unrelated
        // Explorer icon into its area. Never hide an unexpected native icon.
        if !foreign_inside.is_empty() || placed != members.len() {
            let restore: Vec<(String, (i32, i32))> = members.iter().filter_map(|m| m.original.map(|o| (m.path.clone(), o))).collect();
            let restored = view.position(&restore);
            return json!({"started": false, "reason": "Explorer did not place all items safely; requested items restored", "placed": placed, "expected": members.len(), "foreign_inside": foreign_inside, "restored": restored});
        }
        BOX.with(|b| *b.borrow_mut() = Some(BoxLab {
            view, list, cell, underlay: ptr::null_mut(), inside: labels.clone(), content, frame_at: (outer[0], outer[1]), title: title.to_string(),
            events: vec![], moves: 1, status: status.clone(), started: std::time::Instant::now(), members }));
        let underlay = unsafe { create_underlay(list, content) };
        BOX.with(|b| if let Some(lab) = b.borrow_mut().as_mut() { lab.underlay = underlay; });
        let hwnd = unsafe {
            let class = wide("DesktopCraftExplorerOwnedBox");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: GetModuleHandleW(ptr::null()),
                hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class.as_ptr(),
                wide("DesktopCraft explorer-owned box").as_ptr(), WS_POPUP,
                outer[0], outer[1], outer[2], outer[3], GetShellWindow(), ptr::null_mut(), wc.hInstance, ptr::null_mut());
            if !hwnd.is_null() {
                fit(hwnd, content);
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                SetTimer(hwnd, 1, 600, None);
                SetTimer(hwnd, 2, seconds.clamp(5, 3600) * 1000, None);
            }
            hwnd
        };
        if hwnd.is_null() {
            restore_all();
            return json!({"started": false, "reason": "could not create the box window"});
        }
        BOX.with(|b| if let Some(lab) = b.borrow_mut().as_mut() { update(lab); });
        emit(&json!({"started": true, "box_hwnd": format!("0x{:X}", hwnd as usize), "list_hwnd": format!("0x{:X}", list as usize),
            "explorer_pid": explorer_pid, "cell": [cell.0, cell.1], "content": content, "frame": outer,
            "items": drawn.iter().map(|i| json!({"name": i.name, "rect": i.rect})).collect::<Vec<_>>(),
            "native_background_underlay_created": !underlay.is_null(),
            "placed": placed, "missing": missing, "foreign_inside_at_start": foreign_inside}));
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let summary = BOX.with(|b| b.borrow().as_ref().map(|l| json!({"events": l.events, "frame_moves": l.moves, "inside_at_end": l.inside})));
        let restored = restore_all();
        json!({"started": true, "explorer_pid": explorer_pid, "summary": summary, "restored": restored,
            "files_touched": 0, "explorer_injection": false, "hooks": false})
    }

    /// Put every icon the box placed back where it was before the lab.
    fn restore_all() -> Value {
        BOX.with(|b| {
            let Some(lab) = b.borrow_mut().take() else { return json!(null) };
            let moves: Vec<(String, (i32, i32))> = lab.members.iter().filter_map(|m| m.original.map(|o| (m.path.clone(), o))).collect();
            let asked = moves.len();
            let done = lab.view.position(&moves);
            std::thread::sleep(std::time::Duration::from_millis(300));
            let back = lab.members.iter().filter(|m| m.original.is_some() && lab.view.position_of(&m.path) == m.original).count();
            json!({"asked": asked, "accepted": done, "back_at_original_position": back})
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conservative_grid_conflict_preflight() {
        let foreign = [(525, 647), (900, 430)];
        assert!(target_conflicts_with_foreign_icons(&[(500, 650)], &foreign, (75, 107)));
        assert!(target_conflicts_with_foreign_icons(&[(550, 650)], &foreign, (75, 107)));
        assert!(!target_conflicts_with_foreign_icons(&[(450, 647)], &foreign, (75, 107)), "adjacent Explorer grid cell allowed");
        assert!(!target_conflicts_with_foreign_icons(&[(100, 100)], &foreign, (75, 107)));
        assert!(target_conflicts_with_foreign_icons(&[(100, 100)], &foreign, (0, 107)), "invalid cell fails closed");
    }
    #[test]
    fn refuses_ambiguous_msaa_display_names() {
        let all = vec!["One".into(), "Same".into(), "Two".into(), "Same".into()];
        let requested: BTreeSet<String> = ["Same".to_string()].into_iter().collect();
        assert_eq!(ambiguous_requested_labels(&all, &requested), vec!["Same"]);
        let safe: BTreeSet<String> = ["One".to_string()].into_iter().collect();
        assert!(ambiguous_requested_labels(&all, &safe).is_empty());
    }
    #[test]
    fn icon_path_list_accepts_utf8_bom_and_blank_lines() {
        let paths = parse_icon_path_list("\u{feff}C:\\Users\\Test\\Desktop\\One.txt\r\n \r\nC:\\Users\\Test\\Desktop\\Two.txt\n");
        assert_eq!(paths, vec![
            r"C:\Users\Test\Desktop\One.txt",
            r"C:\Users\Test\Desktop\Two.txt",
        ]);
    }
    #[test]
    fn grid_targets_fill_rows_on_explorer_cells() {
        let t = grid_targets((100, 200), (75, 107), 4, 6);
        assert_eq!(t, vec![(100, 200), (175, 200), (250, 200), (325, 200), (100, 307), (175, 307)]);
        assert_eq!(grid_targets((0, 0), (10, 10), 0, 2), vec![(0, 0), (0, 10)], "zero columns behaves as one");
    }
    #[test]
    fn bounding_box_covers_all_drawn_rectangles() {
        assert_eq!(bounding(&[[375, 647, 75, 75], [525, 647, 75, 75], [450, 754, 75, 75]], 6), Some([369, 641, 237, 194]));
        assert_eq!(bounding(&[], 6), None);
    }
    #[test]
    fn membership_is_by_icon_centre_and_reports_pin_changes() {
        let area = [100, 100, 200, 200];
        assert!(center_inside([150, 150, 75, 75], area));
        assert!(!center_inside([270, 150, 75, 75], area), "centre at x=307 is outside");
        let before: BTreeSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let now: BTreeSet<String> = ["b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(membership_changes(&before, &now), vec![("a".to_string(), "unpinned"), ("c".to_string(), "pinned")]);
        assert!(membership_changes(&before, &before).is_empty());
    }
}
