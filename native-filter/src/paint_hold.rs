//! Temporarily retain the ListView's existing frame during layout. Geometry,
//! visibility, and input ownership stay with the Explorer control.
use std::{cell::Cell, ptr};
use winapi::shared::windef::{HWND, RECT};
use winapi::um::winuser::{
    GetUpdateRect, InvalidateRect, KillTimer, SendMessageW, SetTimer, ValidateRect,
    WM_ERASEBKGND, WM_NCDESTROY, WM_PAINT, WM_TIMER,
};

const MAX_HOLD_MS: u32 = 600;
static TIMER_TAG: u8 = 0;

#[derive(Clone, Copy)]
enum Damage { Empty, Bounds([i32; 4]), Full }

thread_local! {
    static HOLD: Cell<Option<(HWND, u64)>> = const { Cell::new(None) };
    static DAMAGE: Cell<Damage> = const { Cell::new(Damage::Empty) };
    static VERIFIED_MOVE: Cell<bool> = const { Cell::new(false) };
}

fn include_bounds(rect: RECT) {
    if rect.left >= rect.right || rect.top >= rect.bottom { return; }
    DAMAGE.with(|damage| {
        let [left, top, right, bottom] = [rect.left, rect.top, rect.right, rect.bottom];
        match damage.get() {
            Damage::Empty => damage.set(Damage::Bounds([left, top, right, bottom])),
            Damage::Bounds([l, t, r, b]) => damage.set(Damage::Bounds([
                l.min(left), t.min(top), r.max(right), b.max(bottom),
            ])),
            Damage::Full => {}
        }
    });
}

pub fn note_unknown_damage(list: HWND) {
    if HOLD.with(Cell::get).is_some_and(|(owner, _)| owner == list) {
        DAMAGE.with(|damage| damage.set(Damage::Full));
    }
}

pub unsafe fn note_item(list: HWND, item: usize) {
    if !HOLD.with(Cell::get).is_some_and(|(owner, _)| owner == list)
        || matches!(DAMAGE.with(Cell::get), Damage::Full) { return; }
    // LVM_GETITEMRECT / LVIR_BOUNDS includes the label and icon.
    let mut rect: RECT = std::mem::zeroed();
    if SendMessageW(list, super::LVM_FIRST + 14, item, &mut rect as *mut _ as isize) != 0 {
        rect.left = rect.left.saturating_sub(4);
        rect.top = rect.top.saturating_sub(4);
        rect.right = rect.right.saturating_add(4);
        rect.bottom = rect.bottom.saturating_add(4);
        include_bounds(rect);
    } else {
        note_unknown_damage(list);
    }
}

#[cfg(feature = "native-fast-tabs")]
pub fn verified_move() { VERIFIED_MOVE.with(|verified| verified.set(true)); }

pub unsafe fn repaint_bounds(list: HWND) -> Option<RECT> {
    if !VERIFIED_MOVE.with(Cell::get) || !HOLD.with(Cell::get).is_some_and(|(owner, deadline)|
        owner == list && super::GetTickCount64() < deadline) { return None; }
    match DAMAGE.with(Cell::get) {
        Damage::Bounds([left, top, right, bottom]) => Some(RECT { left, top, right, bottom }),
        _ => None,
    }
}

fn timer_id() -> usize {
    ptr::addr_of!(TIMER_TAG) as usize
}

#[cfg(feature = "native-fast-tabs")]
pub unsafe fn is_active(list: HWND) -> bool {
    HOLD.with(Cell::get)
        .is_some_and(|(owner, deadline)| owner == list && super::GetTickCount64() < deadline)
}

pub unsafe fn begin(list: HWND) -> bool {
    if list.is_null() || HOLD.with(|hold| hold.get().is_some()) {
        return false;
    }
    // An ordinary window timer avoids leaving a DLL callback address in a
    // queued message when the native filter is detached and unloaded.
    if SetTimer(list, timer_id(), MAX_HOLD_MS, None) == 0 {
        return false;
    }
    HOLD.with(|hold| hold.set(Some((list, super::GetTickCount64() + u64::from(MAX_HOLD_MS)))));
    DAMAGE.with(|damage| damage.set(Damage::Empty));
    VERIFIED_MOVE.with(|verified| verified.set(false));
    true
}

pub unsafe fn finish(list: HWND, repaint: bool) {
    HOLD.with(|hold| {
        if hold.get().is_some_and(|(owner, _)| owner == list) {
            hold.set(None);
            KillTimer(list, timer_id());
            if repaint {
                InvalidateRect(list, ptr::null(), 0);
            }
        }
    });
}

pub unsafe fn handle(list: HWND, message: u32, wparam: usize) -> Option<isize> {
    let (owner, deadline) = HOLD.with(Cell::get)?;
    if owner != list {
        return None;
    }
    if message == WM_NCDESTROY {
        finish(list, false);
        return None;
    }
    let own_timer = message == WM_TIMER && wparam == timer_id();
    if super::GetTickCount64() >= deadline {
        finish(list, true);
        return own_timer.then_some(0);
    }
    match message {
        WM_PAINT if wparam == 0 => {
            let mut rect: RECT = std::mem::zeroed();
            if GetUpdateRect(list, &mut rect, 0) != 0 { include_bounds(rect); }
            (ValidateRect(list, ptr::null()) != 0).then_some(0)
        }
        WM_ERASEBKGND => Some(1),
        WM_TIMER if own_timer => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winapi::shared::windef::POINT;
    use winapi::um::winuser::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW,
        HWND_MESSAGE, MSG, PM_REMOVE, WS_CHILD,
    };

    #[link(name = "comctl32")]
    extern "system" { fn InitCommonControls(); }

    struct TestList { parent: HWND, list: HWND }

    impl TestList {
        unsafe fn new() -> Self {
            fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
            InitCommonControls();
            let create = |class: &str, parent| CreateWindowExW(0, wide(class).as_ptr(), ptr::null(),
                WS_CHILD, 0, 0, 800, 600, parent, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            let parent = create("Static", HWND_MESSAGE);
            assert!(!parent.is_null());
            let list = create("SysListView32", parent);
            assert!(!list.is_null());
            Self { parent, list }
        }
    }

    impl Drop for TestList {
        fn drop(&mut self) {
            unsafe {
                finish(self.list, false);
                DestroyWindow(self.parent);
            }
        }
    }

    #[test]
    fn native_hold_releases_on_commit_or_deadline() {
        unsafe {
            let fixture = TestList::new();
            assert!(begin(fixture.list));
            assert!(!begin(fixture.list));
            assert_eq!(handle(fixture.list, WM_ERASEBKGND, 0), Some(1));
            finish(fixture.list, true);
            assert_eq!(handle(fixture.list, WM_ERASEBKGND, 0), None);
            assert!(begin(fixture.list));
            let timeout = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let mut msg: MSG = std::mem::zeroed();
            loop {
                assert!(std::time::Instant::now() < timeout, "Native paint timer did not fire");
                if PeekMessageW(&mut msg, fixture.list, WM_TIMER, WM_TIMER, PM_REMOVE) != 0 {
                    if msg.wParam == timer_id() {
                        assert_eq!(handle(fixture.list, msg.message, msg.wParam), Some(0));
                        if HOLD.with(|hold| hold.get().is_none()) { break; }
                    } else {
                        DispatchMessageW(&msg);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(handle(fixture.list, WM_ERASEBKGND, 0), None);
        }
    }

    #[test]
    #[cfg(feature = "native-fast-tabs")]
    fn retained_frame_combines_dirty_bounds_and_falls_back_for_unknown_damage() {
        unsafe {
            let fixture = TestList::new();
            assert!(begin(fixture.list));
            include_bounds(RECT { left: 15, top: 20, right: 50, bottom: 60 });
            include_bounds(RECT { left: 30, top: 10, right: 80, bottom: 45 });
            assert!(repaint_bounds(fixture.list).is_none());
            verified_move();
            let rect = repaint_bounds(fixture.list).unwrap();
            assert_eq!((rect.left, rect.top, rect.right, rect.bottom), (15, 10, 80, 60));
            note_unknown_damage(fixture.list);
            assert!(repaint_bounds(fixture.list).is_none());
        }
    }

    #[test]
    #[cfg(feature = "native-fast-tabs")]
    fn native_drag_publishes_only_the_visible_selection_from_its_generation() {
        use crate::{protocol, publish_native_drag, FilterState, GetCurrentProcessId, GetTickCount64,
            LVItem, LVIS_SELECTED, LVM_SETITEMSTATE};
        const LVM_INSERTITEMW: u32 = 0x104d;
        unsafe {
            let fixture = TestList::new();
            let mut item: LVItem = std::mem::zeroed();
            let mut label: Vec<u16> = "Test icon".encode_utf16().chain(Some(0)).collect();
            item.mask = 1; // LVIF_TEXT
            item.text = label.as_mut_ptr();
            for index in 0..3 {
                item.item = index;
                assert_eq!(SendMessageW(fixture.list, LVM_INSERTITEMW, 0, &mut item as *mut _ as isize), index as isize);
            }
            item.state_mask = LVIS_SELECTED;
            item.state = LVIS_SELECTED;
            for index in [0, 2] {
                assert_ne!(SendMessageW(fixture.list, LVM_SETITEMSTATE, index, &item as *const _ as isize), 0);
            }
            let mut memory = Box::<protocol::FilterPolicy>::new_uninit();
            memory.as_mut_ptr().write_bytes(0, 1);
            let mut policy = memory.assume_init();
            policy.magic = protocol::MAGIC;
            policy.abi_version = protocol::ABI_VERSION;
            policy.generation = 2;
            policy.lease_tick_ms = GetTickCount64();
            policy.explorer_pid = GetCurrentProcessId();
            policy.listview_item_count = 3;
            policy.listview_hwnd = fixture.list as u64;
            let mut state = FilterState {
                list: fixture.list as usize,
                parent: fixture.parent as usize,
                mapping_handle: 0,
                mapping_ptr: &mut *policy as *mut _ as usize,
                generation: 2,
                last_paint_hold: 0,
                observed_count: 3,
                validated_labels: 3,
                verified: true,
                hidden: vec![false; 3],
                hidden_labels: std::collections::HashSet::new(),
                count_mismatch_since: std::sync::atomic::AtomicU64::new(0),
                flags: 0,
                drag_capture_active: false,
                drag_capture_index: usize::MAX,
                list_subclass_attached: false,
                parent_subclass_attached: false,
            };
            publish_native_drag(&state, 2);
            assert_ne!(policy.native_drag_tick, 0);
            assert_eq!(policy.native_drag_generation, 2);
            assert_eq!(policy.native_drag_source, 2);
            assert_eq!(policy.native_drag_count, 2);
            assert_eq!(&policy.native_drag_selection[..2], &[0, 2]);
            publish_native_drag(&state, 1);
            assert_eq!(policy.native_drag_source, 2, "A press is published only once");

            policy.native_drag_tick = 0;
            state.hidden[0] = true;
            publish_native_drag(&state, 2);
            assert_eq!(policy.native_drag_tick, 0);
            state.hidden[0] = false;
            publish_native_drag(&state, 1);
            assert_eq!(policy.native_drag_count, 1);
            assert_eq!(policy.native_drag_selection[0], 1);

            policy.native_drag_tick = 0;
            policy.generation = 4;
            publish_native_drag(&state, 2);
            assert_eq!(policy.native_drag_tick, 0);
        }
    }

    #[test]
    fn real_listview_item_follows_in_process_drag_offsets() {
        const LVM_GETITEMPOSITION: u32 = 0x1010;
        const LVM_SETITEMPOSITION32: u32 = 0x1031;
        const LVM_INSERTITEMW: u32 = 0x104d;
        unsafe {
            let fixture = TestList::new();
            let mut item: super::super::LVItem = std::mem::zeroed();
            let mut label: Vec<u16> = "Test icon".encode_utf16().chain(Some(0)).collect();
            item.mask = 1; // LVIF_TEXT
            item.text = label.as_mut_ptr();
            assert_eq!(SendMessageW(fixture.list, LVM_INSERTITEMW, 0, &mut item as *mut _ as isize), 0);
            // The test fixture owns the control and all coordinates. It has
            // no Explorer process, Workspace, or real desktop icons.
            for step in 0..60 {
                let target = POINT { x: 30 + step * 5, y: 40 + step * 2 };
                SendMessageW(fixture.list, LVM_SETITEMPOSITION32, 0, &target as *const _ as isize);
                let mut actual = POINT { x: 0, y: 0 };
                assert_ne!(SendMessageW(fixture.list, LVM_GETITEMPOSITION, 0, &mut actual as *mut _ as isize), 0);
                assert_eq!((actual.x, actual.y), (target.x, target.y));
            }
            #[cfg(feature = "native-fast-tabs")]
            {
                use crate::{protocol::encode_record, tab_moves::read_indexed_positions};
                let record = encode_record(0, "Test icon", false).unwrap();
                assert_eq!(read_indexed_positions(fixture.list, &[record]), Some(vec![[325, 158]]));
                let changed = encode_record(0, "Changed", false).unwrap();
                assert_eq!(read_indexed_positions(fixture.list, &[changed]), None);
            }
        }
    }
}
