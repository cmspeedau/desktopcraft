//! The DesktopCraft side of the optional native Explorer visibility channel.
//! This preview ONLY publishes a bounded, expiring policy to shared memory;
//! it does not load a DLL, install hooks, move icons, change native icon
//! visibility or intercept mouse/keyboard events.
mod workspace_transitions;
use workspace_transitions::*;
mod pin_planning;
use pin_planning::*;
mod workspace_sync;
use workspace_sync::*;
mod inventory;
use inventory::*;
mod background_service;
mod shared_policy;
mod worker_trial;
use worker_trial::*;
pub(crate) use workspace_transitions::state_with_rolled_overrides;
pub(crate) use pin_planning::plan_native_pin_paths;
pub(crate) use workspace_sync::native_explorer_layout_matches;

use crate::{explorer_native::{self, NativePresentation, NativeVisibility},
            explorer_owned_box::win::{list_item_names, list_items},
            model::State, native_filter_protocol as proto, shell::DesktopView};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet, VecDeque}, ffi::c_void, mem, ptr, sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}}, time::Duration};
use winapi::um::winuser::{GetWindowThreadProcessId, ClientToScreen, SendMessageTimeoutW};
use winapi::shared::windef::{HWND, POINT};
use std::sync::mpsc::{self,Receiver,Sender,SyncSender};
use std::thread::JoinHandle;
use crate::native_drag_channel::{NativeGestureEvidence,native_gesture_channel};
use crate::native_box_live_layout::{DesktopCoordinates, LiveLayoutTransaction, NativePlacementPlan};
use crate::native_explorer_guard::desktop_owner;
use crate::shell::ViewItem;

/// Bound layout changes per diagnostic session after real Explorer icon reflow.
const MAX_NATIVE_TWO_BOX_UI_CHANGES_PER_SESSION:usize=4;

#[cfg(test)]
#[test]
fn diagnostic_session_keeps_four_change_safety_cap(){
    assert_eq!(MAX_NATIVE_TWO_BOX_UI_CHANGES_PER_SESSION,4);
}
/// A verified Explorer Pin change. Only the worker can authorize its layout.
#[derive(Clone)]
pub struct NativeVerifiedDrop {
    pub sequence:u64,
    pub before:State,
    pub next:State,
    pub kind:crate::native_pin_drop::NativeMultiDropKind,
    pub source_paths:Vec<String>,
    pub desktop_drop_targets:Vec<(String,(i32,i32))>,
}


struct PreparedSameBoxDrop {
    next: State,
    member: crate::native_live_membership::NativeTwoBoxMembership,
    placement: NativePlacementPlan,
    spacing: (i32, i32),
}

struct NativeDragLanding {
    generation: u64,
    before: Vec<ViewItem>,
    plan: explorer_native::NativeMultiBoxPlan,
    monitors: Vec<crate::host::Monitor>,
    spacing: (i32, i32),
    coordinates: DesktopCoordinates,
    eligible: HashSet<String>,
    active_box_rects: Vec<crate::native_box_placement::Rect>,
    slot_sample: Option<((i32, i32), crate::native_pin_drop::NativeDropSlot)>,
    planned_slot: Option<(u64, u64, usize)>,
    reorder_selection: Option<Vec<String>>,
    same_box: Option<PreparedSameBoxDrop>,
}

impl NativeDragLanding {
    fn new(
        generation: u64, state: &State, before: &[ViewItem], spacing: (i32, i32),
        eligible: HashSet<String>,
    ) -> Result<Self, String> {
        let monitors = crate::host::monitors();
        let coordinates = DesktopCoordinates::from_monitors(&monitors)?;
        Ok(Self {
            generation,
            before: before.to_vec(),
            plan: explorer_native::user_native_workspace_plan(state, before)?,
            monitors,
            spacing,
            coordinates,
            eligible,
            active_box_rects: coordinates.active_box_rects(state),
            slot_sample: None,
            planned_slot: None,
            reorder_selection: None,
            same_box: None,
        })
    }

    fn sample(&mut self, state: &State, point: (i32, i32)) {
        if self.slot_sample.as_ref().is_none_or(|(last, _)| *last != point) {
            self.slot_sample = Some((point, self.slot_at(state, point)));
        }
    }

    fn slot_at(&self, state: &State, point: (i32, i32)) -> crate::native_pin_drop::NativeDropSlot {
        if let Some((sample, slot)) = &self.slot_sample {
            if *sample == point {
                return slot.clone();
            }
        }
        crate::native_pin_drop::native_drop_slot(
            state, &self.plan, &self.before, &self.eligible, point, self.spacing,
            self.coordinates, &self.monitors,
        )
    }

    fn matches(
        &self, generation: u64, before: &[ViewItem], spacing: (i32, i32),
        monitors: &[crate::host::Monitor],
    ) -> bool {
        self.generation == generation
            && self.spacing == spacing
            && self.monitors == monitors
            && self.before.len() == before.len()
            && self.before.iter().zip(before).all(|(old, live)| {
                old.path == live.path && old.label == live.label && old.pos == live.pos
            })
    }

    fn prepare_same_box(
        &mut self,
        state: &State,
        grabbed: &str,
        membership_generation: u64,
        layout: &LiveLayoutTransaction,
        native_shell: &[ViewItem],
        native_original_order: &[ViewItem],
        read_selection: impl FnOnce() -> Option<Vec<String>>,
    ) {
        let slot = self.slot_sample.as_ref().and_then(|(_, slot)| slot.as_ref().ok())
            .and_then(|slot| *slot);
        if self.planned_slot == slot {
            return;
        }
        self.planned_slot = slot;
        self.same_box = (|| {
            let slot = slot?;
            if !state.group(slot.0)?.tab().items.iter()
                .any(|pin| pin.path.eq_ignore_ascii_case(grabbed))
            {
                return None;
            }
            if self.reorder_selection.is_none() {
                self.reorder_selection = read_selection();
            }
            let selected = self.reorder_selection.as_ref()?;
            let next = crate::native_pin_drop::predict_same_box_reorder(
                state, slot, selected, grabbed, &self.eligible,
            )?;
            let member = crate::native_live_membership::NativeTwoBoxMembership::prepare_workspace(
                membership_generation, &next, native_shell,
            ).ok()?;
            let placement = layout.planned_visibility(
                &member, &next, native_original_order, self.spacing,
            ).ok()?;
            Some(PreparedSameBoxDrop { next, member, placement, spacing: self.spacing })
        })();
    }
}

/// Passive sample from the worker's own physical-DPI-aware thread. This
/// never captures the mouse or sends messages into the Explorer process.
fn physical_left_button_and_owner(list:HWND)->Option<(bool,(i32,i32),bool)>{
    #[link(name="user32")]
    unsafe extern "system"{
        fn GetAsyncKeyState(virtual_key:i32)->i16;
        fn GetCursorPos(point:*mut POINT)->i32;
        fn WindowFromPoint(point:POINT)->HWND;
    }
    let mut point=POINT{x:0,y:0};
    if unsafe{GetCursorPos(&mut point)}==0{return None;}
    let held=unsafe{GetAsyncKeyState(0x01)}<0;
    let owner=unsafe{WindowFromPoint(point)}==list;
    Some((held,(point.x,point.y),owner))
}

#[derive(Clone, Copy)]
struct NativePointerEdge {
    pressed: bool,
    point: (i32, i32),
    window: usize,
    on_list: bool,
    tick: u32,
    allow_tab_drop: bool,
}

fn drag_tick_is_current(pressed: u32, started: u32, released: u32) -> bool {
    let elapsed = released.wrapping_sub(pressed);
    elapsed < (1 << 31) && started.wrapping_sub(pressed) <= elapsed
}

#[cfg(test)]
#[test]
fn explorer_drag_start_must_belong_to_the_current_physical_press() {
    assert!(drag_tick_is_current(100, 130, 170));
    assert!(!drag_tick_is_current(100, 99, 170));
    assert!(!drag_tick_is_current(100, 171, 170));
    assert!(drag_tick_is_current(u32::MAX - 3, 0, 4));
}

#[cfg(any(test, feature = "native-fast-tabs"))]
fn drag_generation_is_compatible(
    drag_generation: u64,
    inventory_generation: u64,
    current_generation: u64,
) -> bool {
    drag_generation != 0
        && drag_generation % 2 == 0
        && inventory_generation != 0
        && inventory_generation % 2 == 0
        && current_generation != 0
        && current_generation % 2 == 0
        && drag_generation >= inventory_generation
        && drag_generation <= current_generation
}

#[cfg(test)]
#[test]
fn explorer_drag_generation_survives_same_inventory_policy_updates() {
    assert!(drag_generation_is_compatible(28, 4, 28));
    assert!(drag_generation_is_compatible(28, 4, 32));
    assert!(!drag_generation_is_compatible(28, 30, 32));
    assert!(!drag_generation_is_compatible(34, 4, 32));
    assert!(!drag_generation_is_compatible(0, 4, 32));
    assert!(!drag_generation_is_compatible(29, 4, 32));
    assert!(!drag_generation_is_compatible(28, 5, 32));
    assert!(!drag_generation_is_compatible(28, 4, 31));
}

struct EdgeTapThreadState {
    hook: winapi::shared::windef::HHOOK,
    target_list: usize,
    queue: Arc<std::sync::Mutex<VecDeque<NativePointerEdge>>>,
    pressed: Arc<AtomicBool>,
    has_pending: Arc<AtomicBool>,
    wake: Sender<NativeWorkerMessage>,
    escape_hook: winapi::shared::windef::HHOOK,
    cancelled: std::cell::Cell<bool>,
}

unsafe extern "system" fn escape_edge_hook(code: i32, wparam: usize, lparam: isize) -> isize {
    use winapi::um::winuser::{CallNextHookEx, KBDLLHOOKSTRUCT, VK_ESCAPE, WM_KEYDOWN, WM_SYSKEYDOWN};
    if code >= 0 && lparam != 0 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN)
        && (*(lparam as *const KBDLLHOOKSTRUCT)).vkCode == VK_ESCAPE as u32
    {
        EDGE_TAP_STATE.with(|cell| {
            if let Some(st) = cell.borrow().as_ref().filter(|state| state.pressed.load(Ordering::Acquire)) {
                st.cancelled.set(true);
            }
        });
    }
    CallNextHookEx(ptr::null_mut(), code, wparam, lparam)
}

std::thread_local! {
    static EDGE_TAP_STATE: std::cell::RefCell<Option<EdgeTapThreadState>> =
        const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn pointer_edge_hook(
    code: i32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if code < 0 || lparam == 0 || !matches!(wparam as u32, winapi::um::winuser::WM_LBUTTONDOWN | winapi::um::winuser::WM_LBUTTONUP | winapi::um::winuser::WM_RBUTTONDOWN) {
        return winapi::um::winuser::CallNextHookEx(ptr::null_mut(), code, wparam, lparam);
    }
    let hook = EDGE_TAP_STATE.with(|cell| {
        cell.borrow().as_ref().map_or(ptr::null_mut(), |s| s.hook)
    });
    if code >= 0 && lparam != 0 {
        let wp = wparam as u32;
        if wp == winapi::um::winuser::WM_RBUTTONDOWN {
            EDGE_TAP_STATE.with(|cell| {
                if let Some(st) = cell.borrow().as_ref().filter(|state| state.pressed.load(Ordering::Acquire)) {
                    st.cancelled.set(true);
                }
            });
        }
        if wp == winapi::um::winuser::WM_LBUTTONDOWN || wp == winapi::um::winuser::WM_LBUTTONUP {
            let info = &*(lparam as *const winapi::um::winuser::MSLLHOOKSTRUCT);
            let is_down = wp == winapi::um::winuser::WM_LBUTTONDOWN;
            let pt = (info.pt.x, info.pt.y);
            EDGE_TAP_STATE.with(|cell| {
                if let Some(st) = cell.borrow().as_ref() {
                    let window = winapi::um::winuser::WindowFromPoint(info.pt) as usize;
                    if is_down { st.cancelled.set(false); }
                    st.pressed.store(is_down, Ordering::Release);
                    for _ in 0..8 {
                        if let Ok(mut q) = st.queue.try_lock() {
                            if q.len() >= 64 {
                                q.pop_front();
                            }
                            q.push_back(NativePointerEdge {
                                pressed: is_down,
                                point: pt,
                                window,
                                on_list: window == st.target_list,
                                tick: info.time,
                                allow_tab_drop: !st.escape_hook.is_null() && !st.cancelled.get(),
                            });
                            if !st.has_pending.swap(true, Ordering::AcqRel) {
                                let _ = st.wake.send(NativeWorkerMessage::PointerEdge);
                            }
                            break;
                        }
                        std::hint::spin_loop();
                    }
                }
            });
        }
    }
    winapi::um::winuser::CallNextHookEx(hook, code, wparam, lparam)
}

/// Passive global `WH_MOUSE_LL` listener that captures left-button press and
/// release coordinates cleanly (never consuming events or blocking Explorer),
/// preventing fast `< 200 ms` drags or external File Explorer drops from being missed.
struct NativePointerEdgeTap {
    queue: Arc<std::sync::Mutex<VecDeque<NativePointerEdge>>>,
    pressed: Arc<AtomicBool>,
    has_pending: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl NativePointerEdgeTap {
    fn start(list: HWND, wake: Sender<NativeWorkerMessage>) -> Option<Self> {
        let queue = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        let pressed = Arc::new(AtomicBool::new(false));
        let has_pending = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<u32, ()>>(0);
        let t_queue = Arc::clone(&queue);
        let t_pressed = Arc::clone(&pressed);
        let t_has_pending = Arc::clone(&has_pending);
        let target_list = list as usize;
        let thread = std::thread::Builder::new()
            .name("desktopcraft-mouse-edge-tap".into())
            .spawn(move || {
                unsafe extern "system" {
                    fn SetThreadDpiAwarenessContext(context: isize) -> isize;
                }
                if unsafe { SetThreadDpiAwarenessContext(-4) } == 0 {
                    let _ = ready_tx.send(Err(()));
                    return;
                }
                unsafe {
                    let module = winapi::um::libloaderapi::GetModuleHandleW(ptr::null());
                    let hook = winapi::um::winuser::SetWindowsHookExW(
                        winapi::um::winuser::WH_MOUSE_LL,
                        Some(pointer_edge_hook),
                        module,
                        0,
                    );
                    if hook.is_null() {
                        let _ = ready_tx.send(Err(()));
                        return;
                    }
                    let tid = winapi::um::processthreadsapi::GetCurrentThreadId();
                    let escape_hook = winapi::um::winuser::SetWindowsHookExW(
                        winapi::um::winuser::WH_KEYBOARD_LL, Some(escape_edge_hook), module, 0,
                    );
                    EDGE_TAP_STATE.with(|cell| {
                        *cell.borrow_mut() = Some(EdgeTapThreadState {
                            hook,
                            target_list,
                            queue: t_queue,
                            pressed: t_pressed,
                            has_pending: t_has_pending,
                            wake,
                            escape_hook,
                            cancelled: std::cell::Cell::new(false),
                        });
                    });
                    let mut msg: winapi::um::winuser::MSG = mem::zeroed();
                    use winapi::um::winuser::{GetMessageW, KillTimer, PeekMessageW, SetTimer, PM_NOREMOVE};
                    PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_NOREMOVE);
                    if ready_tx.send(Ok(tid)).is_ok() {
                        let timer = SetTimer(ptr::null_mut(), 0, 200, None);
                        while !thread_stop.load(Ordering::Acquire)
                            && GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {}
                        if timer != 0 { KillTimer(ptr::null_mut(), timer); }
                    }
                    if let Some(installed) = EDGE_TAP_STATE.with(|cell| cell.borrow_mut().take()) {
                        winapi::um::winuser::UnhookWindowsHookEx(installed.hook);
                        if !installed.escape_hook.is_null() {
                            winapi::um::winuser::UnhookWindowsHookEx(installed.escape_hook);
                        }
                    }
                }
            })
            .ok()?;
        match ready_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(thread_id)) => Some(Self {
                queue,
                pressed,
                has_pending,
                stop,
                thread_id,
                thread: Some(thread),
            }),
            _ => None,
        }
    }

    fn has_pending_or_pressed(&self) -> bool {
        self.pressed.load(Ordering::Acquire) || self.has_pending.load(Ordering::Acquire)
    }

    fn drain(&self) -> Vec<NativePointerEdge> {
        let Ok(mut q) = self.queue.lock() else {
            return Vec::new();
        };
        let drained = mem::take(&mut *q);
        self.has_pending.store(false, Ordering::Release);
        drained.into_iter().collect()
    }
}

impl Drop for NativePointerEdgeTap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe {
            winapi::um::winuser::PostThreadMessageW(
                self.thread_id,
                winapi::um::winuser::WM_QUIT,
                0,
                0,
            );
        }
        if let Some(thread) = self.thread.take() {
            let deadline = std::time::Instant::now() + Duration::from_millis(250);
            while !thread.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if thread.is_finished() { let _ = thread.join(); }
        }
    }
}


#[cfg(test)]
mod native_real_drop_permit_tests;

enum NativePinBatchCommand {
    RuleSort {
        rules: Vec<crate::model::Rule>,
        next_id: u64,
        include_existing_pins: bool,
    },
    LayoutRestore {
        target: State,
        next_id: u64,
    },
    PinPaths {
        group_id: u64,
        tab_id: u64,
        paths: Vec<String>,
        next_id: u64,
    },
}

struct NativeTwoBoxCommand{
    group_id:u64,
    tab:usize,
    page_delta:i8,
    /// New virtual DesktopCraft Page; never an Explorer file move on its own.
    desktop_page:Option<usize>,
    /// Fully validated in-memory Workspace update from actual main UI.
    /// No source file move and no persisted Workspace write inside worker.
    workspace_state:Option<State>,
    rolled_overrides:Vec<(u64,bool)>,
    validated_drop_sequence:Option<u64>,
    pin_batch: Option<NativePinBatchCommand>,
    result:Sender<Result<usize,String>>,
    /// Box drag preview: hide that Box's Explorer icons (no icon moves) and
    /// report where they are, or restore the current view when it ends.
    drag_preview:Option<DragPreviewRequest>,
}

struct DragPreviewRequest{
    group_id:u64,
    active:bool,
    resizing: bool,
    reply:Option<Sender<NativeGhostIcons>>,
}

/// One Explorer icon shown as a copy while its Box is dragged.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct NativeGhostIcon{pub path:String,pub label:String,pub pos:(i32,i32)}
pub(crate) type NativeBoxGhosts = HashMap<(u64, usize), Vec<NativeGhostIcon>>;

fn visible_box_ghosts(
    member: &crate::native_live_membership::NativeTwoBoxMembership,
    group: u64,
    tab: usize,
    records: &[proto::FilterRecord],
    items: &[crate::shell::ViewItem],
) -> Vec<NativeGhostIcon> {
    let mut icons = Vec::new();
    for &index in member.tab_indices(group, tab).unwrap_or_default() {
        if records.get(index).is_some_and(|record| record.state == proto::STATE_SHOW) {
            if let Some(item) = items.get(index) {
                icons.push(NativeGhostIcon {
                    path: item.path.clone(), label: item.label.clone(), pos: item.pos,
                });
            }
        }
    }
    icons.sort_by_key(|icon| (icon.pos.1, icon.pos.0));
    icons
}

fn resize_box_ghosts(
    member: &crate::native_live_membership::NativeTwoBoxMembership,
    group: u64,
    tab: usize,
    items: &[crate::shell::ViewItem],
) -> Vec<NativeGhostIcon> {
    member.tab_indices(group, tab).unwrap_or_default().iter()
        .filter_map(|&index| items.get(index))
        .map(|item| NativeGhostIcon { path: item.path.clone(), label: item.label.clone(), pos: item.pos })
        .collect()
}
/// The dragged Box's icons, Explorer's main-monitor icon cell, Explorer's
/// logical icon size (px), and whether Explorer's global Align-to-Grid is
/// still active (false in two-grid mode).
pub type NativeGhostIcons=(Vec<NativeGhostIcon>,(i32,i32),i32,bool);
/// One unacknowledged REAL Native Desktop change. Never waits on the GUI
/// thread: the caller polls this receiver in normal frame updates. Every
/// pending command has a finite deadline; at expiry the Explorer filter is
/// told to stop and its original icon recovery checkpoint remains intact.
pub struct PendingNativeWorkspaceSync {
    ack:mpsc::Receiver<Result<usize,String>>,
    issued_at:std::time::Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NativeWorkspaceReply {
    Pending,
    Applied(usize),
    Rejected(String),
}

impl PendingNativeWorkspaceSync {
    const MAX_ACK_WAIT:Duration=Duration::from_secs(12);
    fn poll(&self)->Result<NativeWorkspaceReply,String>{
        // Check the deadline FIRST: even a late reply already queued before
        // the GUI gets another frame is not permission to commit an expired
        // native Explorer generation. The worker's five-second policy lease
        // and recovery journal remain independent of UI frame scheduling.
        if self.issued_at.elapsed()>=Self::MAX_ACK_WAIT {
            return Err("Native Explorer Workspace ACK deadline expired; stop filter, preserve icon recovery journal".into());
        }
        match self.ack.try_recv() {
            Ok(Ok(page))=>Ok(NativeWorkspaceReply::Applied(page)),
            Ok(Err(reason))=>Ok(NativeWorkspaceReply::Rejected(reason)),
            Err(mpsc::TryRecvError::Disconnected)=>
                Err("Native Explorer worker dropped Workspace ACK; refusing UI commit".into()),
            Err(mpsc::TryRecvError::Empty)=>Ok(NativeWorkspaceReply::Pending),
        }
    }
}

#[cfg(test)]
mod native_async_ack_tests {
    use super::*;
    #[test]
    fn pending_does_not_block_gui_frame() {
        let (_sender,receiver)=mpsc::channel();
        let request=PendingNativeWorkspaceSync{
            ack:receiver,issued_at:std::time::Instant::now()
        };
        let start=std::time::Instant::now();
        for _ in 0..128 {
            assert_eq!(request.poll().unwrap(),NativeWorkspaceReply::Pending);
        }
        assert!(start.elapsed()<Duration::from_millis(300));
    }
    #[test]
    fn accepted_generation_requires_real_worker_ack() {
        let (sender,receiver)=mpsc::channel();
        let request=PendingNativeWorkspaceSync{
            ack:receiver,issued_at:std::time::Instant::now()
        };
        assert_eq!(request.poll().unwrap(),NativeWorkspaceReply::Pending);
        sender.send(Ok(7)).unwrap();
        assert_eq!(request.poll().unwrap(),NativeWorkspaceReply::Applied(7));
    }
    #[test]
    fn worker_rejection_never_commits_state() {
        let (sender,receiver)=mpsc::channel();
        let request=PendingNativeWorkspaceSync{
            ack:receiver,issued_at:std::time::Instant::now()
        };
        sender.send(Err("Explorer native item ownership changed".into())).unwrap();
        assert_eq!(request.poll().unwrap(), NativeWorkspaceReply::Rejected(
            "Explorer native item ownership changed".into()));
    }
    #[test]
    fn expired_generation_stays_rejected_even_with_queued_success() {
        let (sender,receiver)=mpsc::channel();
        let request=PendingNativeWorkspaceSync{
            ack:receiver,
            issued_at:std::time::Instant::now()
                - PendingNativeWorkspaceSync::MAX_ACK_WAIT
                - Duration::from_millis(1),
        };
        sender.send(Ok(5)).unwrap();
        assert!(request.poll().unwrap_err().contains("deadline expired"));
    }
    #[test]
    fn disconnected_worker_never_commits() {
        let (sender,receiver)=mpsc::channel::<Result<usize,String>>();
        let request=PendingNativeWorkspaceSync{
            ack:receiver,issued_at:std::time::Instant::now(),
        };
        drop(sender);
        assert!(request.poll().unwrap_err().contains("dropped Workspace ACK"));
    }
}


/// (Box id, first visible row, scroll positions, visible rows).
type NativePageStatus = Arc<[(u64,usize,usize,usize)]>;
/// Live Explorer placement metrics and page scrollbars pushed from worker to UI.
#[derive(Clone, Debug)]
pub struct NativeLayoutSync {
    pub pages: Option<NativePageStatus>,
    pub ghost_icons: Option<NativeBoxGhosts>,
    pub visible_loose_rects: Option<std::sync::Arc<[[i32; 4]]>>,
    pub main_cell: (i32, i32),
    pub icon_size: i32,
    pub snap_to_grid: bool,
}

fn compute_visible_loose_rects(
    expected: &[ViewItem],
    state: &State,
    cell: (i32, i32),
) -> std::sync::Arc<[[i32; 4]]> {
    let owned: HashSet<String> = state
        .groups
        .iter()
        .flat_map(|g| &g.tabs)
        .filter(|t| t.folder.is_none())
        .flat_map(|t| &t.items)
        .map(|item| item.path.to_lowercase())
        .collect();
    let w = cell.0.max(1);
    let h = cell.1.max(1);
    expected
        .iter()
        .filter(|item| !owned.contains(&item.path.to_lowercase()))
        .map(|item| [item.pos.0, item.pos.1, w, h])
        .collect::<Vec<_>>()
        .into()
}
/// Upper bound for queued wheel notches sent as one Explorer reflow.
pub(crate) const MAX_SCROLL_ROWS_PER_CHANGE: u8 = 8;

enum NativeWorkerMessage {
    Command(NativeTwoBoxCommand),
    PointerEdge,
}

impl NativeWorkerMessage {
    fn command(self) -> Option<NativeTwoBoxCommand> {
        match self {
            Self::Command(command) => Some(command),
            Self::PointerEdge => None,
        }
    }
}

struct NativeUiChannel {
    commands: Receiver<NativeWorkerMessage>,
    pointer_wake: Sender<NativeWorkerMessage>,
    pages: Sender<NativeLayoutSync>,
    header_drops: Receiver<NativeHeaderDrop>,
    overlay_hwnd: usize,
}

struct NativeHeaderDrop {
    destination: (u64, u64),
    screen: (i32, i32),
    paths: Vec<String>,
    result: Option<Sender<Result<(), String>>>,
}

fn rollup_header_drag_selection(
    state: &State,
    overrides: &[(u64, bool)],
    target: u64,
    grabbed: &str,
    selection: impl FnOnce() -> Option<Vec<String>>,
) -> Option<Vec<String>> {
    let (source, _, _) = crate::native_pin_drop::visible_regular_pin_owner(state, grabbed)?;
    let involves_rollup = state.group(target).is_some_and(|g| g.rolled)
        || overrides.iter().any(|&(id, rolled)| rolled && (id == source || id == target));
    if !involves_rollup { return None; }
    let selected = selection()?;
    selected.iter().any(|path| path.eq_ignore_ascii_case(grabbed)).then_some(selected)
}

#[cfg(test)]
#[test]
fn rollup_selection_fallback_requires_the_grabbed_visible_pin() {
    use crate::model::{Bounds, Item};
    let mut state = State::empty();
    let bounds = Bounds { x: 100., y: 100., w: 300., h: 300. };
    let source = state.add_group("Source", bounds);
    let target = state.add_group("Target", Bounds { x: 500., ..bounds });
    let grabbed = "C:/Desktop/Grabbed.lnk";
    state.group_mut(source).unwrap().tab_mut().items.push(Item {
        id: 42, name: "Grabbed".into(), path: grabbed.into(), kind: "lnk".into(),
        size: 0, modified: 0, label: None,
    });
    assert!(rollup_header_drag_selection(&state, &[], target, grabbed, || panic!()).is_none());
    let rolled = [(source, true)];
    assert!(rollup_header_drag_selection(&state, &rolled, target, grabbed, || {
        Some(vec!["C:/Desktop/Other.lnk".into()])
    }).is_none());
    let paths = vec![grabbed.to_string(), "C:/Desktop/Other.lnk".into()];
    assert_eq!(rollup_header_drag_selection(&state, &rolled, target, grabbed, || Some(paths.clone())), Some(paths));
    state.group_mut(source).unwrap().rolled = true;
    assert!(rollup_header_drag_selection(&state, &rolled, target, grabbed, || panic!()).is_none());
}

fn queue_verified_drop(
    sender: &SyncSender<(NativeVerifiedDrop, PendingNativeWorkspaceSync)>,
    commands: &mut VecDeque<NativeTwoBoxCommand>,
    proposal: &NativeVerifiedDrop,
) -> Result<(), String> {
    let (result, ack) = mpsc::channel();
    let ticket = PendingNativeWorkspaceSync { ack, issued_at: std::time::Instant::now() };
    sender.try_send((proposal.clone(), ticket)).map_err(|_| {
        "Real native drag UI notification unavailable; keeping original Workspace".to_string()
    })?;
    commands.push_front(NativeTwoBoxCommand {
        drag_preview: None,
        group_id: 0,
        tab: 0,
        page_delta: 0,
        desktop_page: None,
        workspace_state: Some(proposal.next.clone()),
        rolled_overrides: Vec::new(),
        validated_drop_sequence: Some(proposal.sequence),
        pin_batch: None,
        result,
    });
    Ok(())
}

pub struct NativeTwoBoxBackgroundService{
    commands:Sender<NativeWorkerMessage>,
    stop:Arc<AtomicBool>,
    ready:Arc<AtomicBool>,
    worker:Option<JoinHandle<Value>>,
    real_drop_events:Receiver<(NativeVerifiedDrop, PendingNativeWorkspaceSync)>,
    pages:Receiver<NativeLayoutSync>,
    header_drops: SyncSender<NativeHeaderDrop>,
}

pub struct NativeTabBackgroundService {
    commands: Sender<usize>,
    ready: Arc<AtomicBool>,
    worker: Option<JoinHandle<Value>>,
}
impl NativeTabBackgroundService {
    /// Launch a single independently reversible Explorer-owned filter session.
    /// Only a fully verified pair of actual DesktopCraft regular Tabs is
    /// affected; every other native icon remains Explorer-owned and visible.
    pub fn start(workspace:std::path::PathBuf,dll:std::path::PathBuf,
                 group_id:u64,to_tab:usize)->Self {
        let (sender,receiver)=mpsc::channel();
        let ready=Arc::new(AtomicBool::new(false));
        let signal=Arc::clone(&ready);
        let worker=std::thread::spawn(move||{
            crate::shell::init(); // Explorer COM apartment belongs to worker
            run_native_hook_trial(3600,&workspace,&dll,None,
                Some((group_id,Some(to_tab),true)),None,Some(receiver),None,0,Some(signal),false)
        });
        Self{commands:sender,ready,worker:Some(worker)}
    }
    pub fn is_ready(&self)->bool {
        self.ready.load(Ordering::Acquire)
            && self.worker.as_ref().is_some_and(|worker|!worker.is_finished())
    }
    /// Prevent the UI from accepting Tab clicks before Explorer has actually
    /// installed and validated the native DLL subclass. Bounded wait only in
    /// explicitly opted-in native preview startup.
    pub fn await_ready(&self,limit:Duration)->bool {
        let started=std::time::Instant::now();
        while started.elapsed()<limit {
            if self.is_ready(){return true;}
            if self.worker.as_ref().is_none_or(|w|w.is_finished()){return false;}
            std::thread::sleep(Duration::from_millis(25));
        }
        self.is_ready()
    }
    pub fn selected(&self,index:usize)->bool{
        self.is_ready() && self.commands.send(index).is_ok()
    }
    pub fn shutdown(&mut self)->Value {
        if let Some(worker)=self.worker.take(){
            let _=self.commands.send(usize::MAX);
            match worker.join(){
                Ok(report)=>report,
                Err(_)=>json!({"started":false,"reason":"native worker crashed; shared policy expires automatically"}),
            }
        }else{json!({"reason":"already detached"})}
    }
}
impl Drop for NativeTabBackgroundService {
    fn drop(&mut self){let _=self.shutdown();}
}

/// Debug-only bridge from the currently attached Explorer native MouseDown
/// hook to the ORIGINAL DesktopCraft GUI thread. The receiver never accepts
/// a JSON report or CLI path as a gesture authority.
#[cfg(feature="native-unsafe-drag-lab")]
pub struct NativeGestureBackgroundService{
    inbox:crate::native_drag_channel::NativeGestureInbox,
    ready:Arc<AtomicBool>,
    stop:Arc<AtomicBool>,
    worker:Option<JoinHandle<Value>>,
}
#[cfg(feature="native-unsafe-drag-lab")]
impl NativeGestureBackgroundService{
    pub fn start(workspace:std::path::PathBuf,dll:std::path::PathBuf,
                 allow_real_workspace:bool)->Self{
        let (sender,inbox)=native_gesture_channel();
        let ready=Arc::new(AtomicBool::new(false));
        let signal=Arc::clone(&ready);
        let stop=Arc::new(AtomicBool::new(false));
        let worker_stop=Arc::clone(&stop);
        let worker=std::thread::spawn(move||{
            crate::shell::init();
            run_native_hook_trial_with_event(
                30,&workspace,&dll,None,None,None,None,None,0,
                Some(signal),true,Some(sender),Some(worker_stop),
                allow_real_workspace,None,None,None)
        });
        Self{inbox,ready,stop,worker:Some(worker)}
    }
    pub fn await_ready(&self,limit:Duration)->bool{
        let at=std::time::Instant::now();
        while at.elapsed()<limit {
            if self.ready.load(Ordering::Acquire)
                && self.worker.as_ref().is_some_and(|w|!w.is_finished()){
                return true;
            }
            if self.worker.as_ref().is_none_or(|w|w.is_finished()){return false;}
            std::thread::sleep(Duration::from_millis(25));
        }
        self.ready.load(Ordering::Acquire)
            && self.worker.as_ref().is_some_and(|w|!w.is_finished())
    }
    pub fn try_take(&mut self)->Result<Option<NativeGestureEvidence>,String>{
        self.inbox.try_take()
    }
    pub fn shutdown(&mut self)->Value{
        self.stop.store(true,Ordering::Release);
        match self.worker.take(){
            Some(worker)=>match worker.join(){
                Ok(report)=>report,
                Err(_)=>json!({"reason":"Native gesture hook worker panicked"})
            },
            None=>json!({"reason":"already joined"})
        }
    }
}
#[cfg(all(test,feature="native-unsafe-drag-lab"))]
mod native_gesture_worker_stop_tests {
    use super::*;
    #[test]
    fn original_ui_can_request_immediate_clean_worker_join_without_waiting_for_full_trial(){
        let (sender,inbox)=native_gesture_channel();
        drop(sender);
        let ready=Arc::new(AtomicBool::new(true));
        let stop=Arc::new(AtomicBool::new(false));
        let watcher=Arc::clone(&stop);
        let worker=std::thread::spawn(move||{
            while !watcher.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(5));
            }
            json!({"stopped_after_explicit_request":true})
        });
        let mut service=NativeGestureBackgroundService{
            inbox,ready,stop,worker:Some(worker)
        };
        let started=std::time::Instant::now();
        let report=service.shutdown();
        assert_eq!(report["stopped_after_explicit_request"],true);
        assert!(started.elapsed()<Duration::from_secs(1));
        assert_eq!(service.shutdown()["reason"],"already joined");
    }
}

#[cfg(feature="native-unsafe-drag-lab")]
impl Drop for NativeGestureBackgroundService{
    fn drop(&mut self){let _=self.shutdown();}
}


#[link(name="kernel32")]
extern "system" {
    fn CreateFileMappingW(file: *mut c_void, attributes: *mut c_void, flags: u32,
                          high: u32, low: u32, name: *const u16) -> *mut c_void;
    fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
    fn MapViewOfFile(mapping: *mut c_void, access: u32, high: u32, low: u32,
                     len: usize) -> *mut c_void;
    fn UnmapViewOfFile(base: *const c_void) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetTickCount64() -> u64;
    fn GetLastError() -> u32;
}
const PAGE_READWRITE: u32 = 4;
const FILE_MAP_ALL_ACCESS: u32 = 0xF001F;
const FILE_MAP_READ: u32 = 0x0004;
const ERROR_ALREADY_EXISTS: u32 = 183;

struct SharedPolicy {
    handle: *mut c_void,
    mapped: *mut proto::FilterPolicy,
    #[cfg(feature = "native-fast-tabs")]
    position_ticket: std::cell::Cell<usize>,
}


#[link(name="kernel32")]
extern "system" {
    fn LoadLibraryW(path: *const u16) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetProcAddress(module: *mut c_void, name: *const i8) -> *const c_void;
}
#[link(name="user32")]
extern "system" {
    fn SetWindowsHookExW(kind: i32, function: *const c_void, dll: *mut c_void, thread: u32) -> *mut c_void;
    fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
    fn PostMessageW(hwnd: HWND, message: u32, wp: usize, lp: isize) -> i32;
    fn InvalidateRect(hwnd: HWND, rect: *const c_void, erase: i32) -> i32;

}

fn live_item_count(list: HWND) -> Option<usize> {
    const LVM_GETITEMCOUNT: u32 = 0x1004;
    if list.is_null() {
        return None;
    }
    let mut count = 0usize;
    let ok = unsafe {
        SendMessageTimeoutW(
            list,
            LVM_GETITEMCOUNT,
            0,
            0,
            0x0002,
            50,
            &mut count,
        )
    };
    (ok != 0 && count <= proto::MAX_ITEMS).then_some(count)
}

struct HookGuard {
    hook: *mut c_void,
    module: *mut c_void,
    list: HWND,
    mapping: *mut proto::FilterPolicy,
}
impl Drop for HookGuard {
    fn drop(&mut self) {
        unsafe {
            // ListView handles this removal even if GetMessage hooks are gone.
            let mut output=0usize;
            SendMessageTimeoutW(self.list,proto::APP_HOOK_REMOVE,proto::HOOK_COOKIE,0,
                0x0002,1800,&mut output);
            for _ in 0..12 {
                if (*self.mapping).hook_state.load(Ordering::Acquire)
                    == proto::HOOK_STATE_DETACHED { break; }
                std::thread::sleep(Duration::from_millis(50));
            }
            let detached = (*self.mapping).hook_state.load(Ordering::Acquire)
                == proto::HOOK_STATE_DETACHED;
            let hook_removed = self.hook.is_null() || UnhookWindowsHookEx(self.hook) != 0;
            // Explorer also pins its own DLL reference until both subclasses
            // are removed. Retain the controller reference if either step is
            // uncertain, so a failed detach cannot explicitly unload it.
            if detached && hook_removed && !self.module.is_null() {
                FreeLibrary(self.module);
            }
            if !self.list.is_null() { InvalidateRect(self.list,ptr::null(),0); }
        }
    }
}

/// Source compatibility wrapper for the existing read-only / one-item
/// experiments. Native Tab cycling is opt-in through its own command.
pub fn native_hook_observe(seconds: u32, workspace: &std::path::Path,
                           dll_path: &std::path::Path,
                           experimental_single_index: Option<usize>) -> Value {
    run_native_hook_trial(seconds, workspace, dll_path, experimental_single_index, None, None, None, None,0,None,false)
}

/// A -> B -> A visual state transition with at most two known Explorer icons.
/// The current DesktopCraft workspace is READ-ONLY: changing the target
/// group is a transient native visibility experiment, not a saved Tab change.
pub fn native_tab_switch_probe(seconds: u32, workspace: &std::path::Path,
                               dll_path: &std::path::Path,
                               group_id: u64, to_tab: Option<usize>) -> Value {
    run_native_hook_trial(seconds, workspace, dll_path, None, Some((group_id,to_tab,false)), None, None, None,0,None,false)
}

/// Test a complete small pair of regular DesktopCraft Tabs rather than
/// only two representative items. A strict three-items-per-Tab ceiling keeps
/// this opt-in prototype from accidentally hiding an entire user desktop.
pub fn native_tab_full_probe(seconds: u32, workspace: &std::path::Path,
                             dll_path: &std::path::Path,
                             group_id: u64, to_tab: Option<usize>) -> Value {
    run_native_hook_trial(seconds, workspace, dll_path, None, Some((group_id,to_tab,true)), None, None, None,0,None,false)
}

/// In developer UI mode, true = synthetic button events for integration tests.
pub fn native_tab_click_probe(seconds: u32, workspace:&std::path::Path,
                              dll_path:&std::path::Path,
                              group_id:u64,to_tab:Option<usize>,synthetic:bool)->Value {
    run_native_hook_trial(seconds,workspace,dll_path,None,
                          Some((group_id,to_tab,true)),Some(synthetic),None,None,0,None,false)
}

/// Read-only preflight for native input diagnostics. This does not
/// allocate shared memory, install any hook, send pointer-bearing ListView
/// messages, touch files, or change Explorer state.
pub fn native_input_preflight(workspace: &std::path::Path, requested_index: usize) -> Value {
    let refuse = |reason: String| json!({
        "preflight_passed": false, "reason": reason,
        "injected_dll": false, "installed_hook": false,
        "input_messages_sent": 0, "icons_repositioned": 0,
        "desktop_files_touched": 0,
    });
    if requested_index >= proto::MAX_ITEMS {
        return refuse("Index exceeds native filter protocol limit".into());
    }
    let Some(view)=DesktopView::acquire() else {
        return refuse("Explorer Desktop View is not available".into());
    };
    if view.icons_visible()!=Some(true) {
        return refuse("Explorer's genuine Desktop icons are not visible".into());
    }
    let owner=match desktop_owner(&view) {
        Ok(owner)=>owner,
        Err(reason)=>return refuse(reason),
    };
    let hwnd=view.list_window();
    let (pid,thread)=(owner.pid,owner.thread);
    let observed=view.items();
    let names:Vec<String>=list_items(hwnd).into_iter().map(|i|i.name).collect();
    let audit=explorer_native::audit_native_indices(&observed,&names);
    let reordered=match explorer_native::native_view_in_msaa_order(&observed,&names) {
        Ok(mapped)=>mapped,
        Err(reason)=>return refuse(format!("Cannot uniquely reconcile Explorer item names: {reason}")),
    };
    let Some(item)=reordered.get(requested_index) else {
        return refuse("Requested item index exceeds the current native ListView".into());
    };
    if item.label.trim().is_empty() {
        return refuse("Native target has no display label".into());
    }
    let state=match State::load(workspace) {
        Ok(state)=>state,
        Err(reason)=>return refuse(format!("DesktopCraft workspace cannot be loaded: {reason}")),
    };
    let decisions=NativePresentation::decide(&state,&reordered);
    if decisions.conflicting_active_references>0 {
        return refuse("DesktopCraft has multiple active references for one native icon".into());
    }
    let all=proto::verified_records(reordered.iter().enumerate()
        .map(|(i,it)|(i,it.label.as_str(),false)),reordered.len());
    if all.is_none() {
        return refuse("Native policy records could not be uniquely validated".into());
    }
    json!({
        "preflight_passed":true,
        "explorer_pid":pid,
        "native_thread_id":thread,
        "native_listview_class":"SysListView32",
        "native_listview_item_count":reordered.len(),
        "native_listview_item_index":requested_index,
        "native_target_label":item.label,
        "native_unique_names_reconciled":audit.reconciled_names,
        "native_shell_order_match_count":audit.same_index_labels,
        "native_shell_order_different":!audit.matching_order,
        "active_workspace_reference_conflicts":0,
        "policy_abi":proto::ABI_VERSION,
        "injected_dll":false, "installed_hook":false,
        "input_messages_sent":0, "icons_repositioned":0,
        "desktop_files_touched":0,
        "diagnostic_readiness":"Read-only identity preflight only; not evidence the in-process LVM_GETITEMRECT/LVM_HITTEST probe or Explorer stability has passed",
    })
}

/// Purely observational pointer-free Win32 probe: the host sends an integer
/// and the native DLL performs all pointer-bearing ListView calls on the
/// Explorer desktop UI thread. No paint or input suppression flags are set.
pub fn native_input_observe(seconds:u32,workspace:&std::path::Path,
                            dll_path:&std::path::Path,index:usize)->Value {
    run_native_hook_trial(seconds,workspace,dll_path,None,None,None,None,Some(index),0,None,false)
}

/// Temporary bounded synthetic hover: the target's geometry is resolved
/// inside Explorer and the DLL posts only WM_MOUSEMOVE to its own ListView.
/// No real mouse pointer movement, file open, drag or context menu.
pub fn native_input_hover_probe(seconds:u32,workspace:&std::path::Path,
                                dll_path:&std::path::Path,index:usize)->Value {
    run_native_hook_trial(seconds,workspace,dll_path,None,None,None,None,Some(index),1,None,false)
}

/// In-process, opt-in bounded synthetic right-button + context-menu messages.
/// Every message is addressed to the native Explorer ListView and must be
/// suppressed before Explorer can invoke any native file context menu.
pub fn native_input_rightclick_probe(seconds:u32,workspace:&std::path::Path,
                                     dll_path:&std::path::Path,index:usize)->Value {
    run_native_hook_trial(seconds,workspace,dll_path,None,None,None,None,Some(index),2,None,false)
}

/// Explicit developer-only staged paint/input visibility on the real
/// Explorer Desktop. Scope is restricted to a verified subset (max 8) of
/// inactive Tab items; the real user Workspace is never modified.
pub fn native_staged_hide_probe(
    seconds:u32,workspace:&std::path::Path,dll:&std::path::Path,max_hide:usize,
)->Value{
    if !cfg!(debug_assertions) || !cfg!(feature="native-lab"){
        return json!({"started":false,"reason":"Staged Explorer hide tests require Debug native-lab build"});
    }
    run_native_hook_trial_with_event(
        seconds,workspace,dll,None,None,None,None,None,0,
        None,false,None,None,false,Some(max_hide),None,None)
}

/// Separate Debug-only verification of independent Tab choices in two real
/// user Boxes. Zero workspace writes and a fail-open policy at every change.
pub fn native_two_box_tab_cycle_probe(
    seconds:u32,workspace:&std::path::Path,dll:&std::path::Path,
    inspect_hidden_input:bool,
)->Value{
    if !cfg!(debug_assertions) || !cfg!(feature="native-lab"){
        return json!({"started":false,"reason":"Two-Box Tab cycle requires Debug native-lab build"});
    }
    // 0 = Tab policy cycle; usize::MAX = additionally inspect and verify
    // suppressed hidden-item input. Both require the same triple CLI gates.
    let mode=if inspect_hidden_input {usize::MAX}else{0};
    run_native_hook_trial_with_event(
        seconds,workspace,dll,None,None,None,None,None,0,
        None,false,None,None,false,Some(mode),None,None)
}

pub fn native_multibox_fixture_probe(seconds:u32,workspace:&std::path::Path,
                                     dll_path:&std::path::Path)->Value {
    // This call runs on DesktopCraft's in-process controller thread. A
    // bounded, private channel receives native Explorer event evidence
    // directly from the verified running hook, not via any JSON log file.
    let (tx,mut inbox)=native_gesture_channel();
    let mut report=run_native_hook_trial_with_event(
        seconds,workspace,dll_path,None,None,None,None,None,0,None,true,Some(tx),None,false,None,None,None);
    let bridge=(||->Result<Value,String>{
        let Some(event)=inbox.try_take()? else {
            return Err("Native hook did not produce a one-shot physical drag".into());
        };
        let view=DesktopView::acquire()
            .ok_or("Explorer Desktop vanished before UI channel validation")?;
        let native_names=list_items(view.list_window()).into_iter()
            .map(|it|it.name).collect::<Vec<_>>();
        let current=explorer_native::native_view_in_msaa_order(
            &view.items(),&native_names)?;
        let mut pid=0u32;
        let hwnd=view.list_window();
        let thread=unsafe{GetWindowThreadProcessId(hwnd,&mut pid)};
        if thread==0 {
            return Err("Explorer ListView is not attached to a process".into());
        }
        let snapshot=std::fs::read(workspace).map_err(|e|e.to_string())?;
        let state=State::load(workspace)?;
        let action=event.validate(
            std::time::Instant::now(),pid,hwnd as usize as u64,
            &crate::host::signature(&crate::host::monitors()),
            &snapshot,&state,&current,2,4)?;
        // Ensure consuming the exact channel event is irreversible.
        if inbox.try_take()?.is_some() {
            return Err("One-shot native gesture unexpectedly replayed".into());
        }
        Ok(json!({
            "validated":true,
            "transport":"process-private-bounded-sync-channel",
            "native_source_file_count":action.offered_paths.len(),
            "native_source_paths":action.offered_paths,
            "target_group_id":action.target_group,
            "target_tab_id":action.target_tab,
            "source_groups_after":action.next_plan.groups.len(),
            "no_files_moved":true,
            "user_workspace_written":false,
            "shell_file_operations":false,
            "replay_prevented":true,
            "physical_explorer_capture":true,
            "original_ui_state_modified":false,
        }))
    })();
    report["native_inprocess_gesture_bridge"]=bridge.unwrap_or_else(|reason|
        json!({"validated":false,"reason":reason,
            "user_workspace_written":false,"shell_file_operations":false}));
    report
}

/// Pure acceptance predicate: one hidden hover, both right-button edges,
/// and a context-menu message must all have been intercepted. This is a
/// necessary condition for the experimental probe, not a visual Win11 menu test.
fn hidden_input_counters_pass(before:(u32,u32,u32),after:(u32,u32,u32))->bool{
    after.0>before.0 && before.1.checked_add(2)
        .is_some_and(|required|after.1>=required)
        && after.2>before.2
}

/// LVM_GETITEMSTATE takes ONLY integer item index and state-mask values;
/// unlike LVM_HITTEST/GETITEMRECT it contains NO cross-process pointer.
/// A hidden native Explorer item must not remain Selected or Focused.
fn native_selected_or_focused_indices(list:HWND,indices:&[usize])
    ->Result<Vec<(usize,u32)>,String>{
    let mut chosen=Vec::new();
    for &index in indices{
        let mut value=0usize;
        let ack=unsafe{SendMessageTimeoutW(list,0x1000+44,index,0x3,
            0x0002,500,&mut value)};
        if ack==0 {
            return Err(format!("Explorer native selection-state query timed out for index {index}"));
        }
        let flags=(value as u32)&0x3;
        if flags!=0 {chosen.push((index,flags));}
    }
    Ok(chosen)
}

/// A genuine Windows desktop background context request is not the same as
/// an invisible FILE context menu. A hidden icon must NOT be hit-testable,
/// remain selected or focused, or be the target of the menu. This is
/// diagnostic-only: it does NOT verify a correct Windows 11 Modern popup.
fn hidden_context_routed_to_background(
    received_before:u32,parent_before:u32,
    received_after:u32,parent_after:u32,
    hit_index:i32,hidden_at_hit:u32,policy_active:u32,matching_client_point:bool,
    hidden_selection_clear:bool,
)->bool{
    received_after>received_before && parent_after>parent_before
        && hit_index==-1 && hidden_at_hit==0 && policy_active==1
        && matching_client_point && hidden_selection_clear
}

#[cfg(test)]
mod hidden_input_acceptance_tests {
    use super::hidden_input_counters_pass;
    #[test]
    fn hidden_hover_both_right_button_edges_and_context_menu_are_required(){
        assert!(hidden_input_counters_pass((1,2,3),(2,4,4)));
        assert!(!hidden_input_counters_pass((1,2,3),(1,4,4)));
        assert!(!hidden_input_counters_pass((1,2,3),(2,3,4)));
        assert!(!hidden_input_counters_pass((1,2,3),(2,4,3)));
    }
    #[test]
    fn background_context_must_be_targetless_with_no_hidden_selection(){
        assert!(super::hidden_context_routed_to_background(
            0,0,2,2,-1,0,1,true,true));
        assert!(!super::hidden_context_routed_to_background(
            0,0,2,2,1,1,1,true,true));
        assert!(!super::hidden_context_routed_to_background(
            0,0,2,2,-1,0,1,true,false));
        assert!(!super::hidden_context_routed_to_background(
            0,0,0,2,-1,0,1,true,true));
        assert!(!super::hidden_context_routed_to_background(
            0,0,2,0,-1,0,1,true,true));
        assert!(!super::hidden_context_routed_to_background(
            0,0,2,2,-1,0,1,false,true));
        assert!(!super::hidden_context_routed_to_background(
            0,0,2,2,-1,0,0,true,true));
    }
    #[test]
    fn counter_overflow_and_incomplete_callbacks_never_pass(){
        assert!(!hidden_input_counters_pass((u32::MAX,2,3),
            (u32::MAX,4,4)));
        assert!(!hidden_input_counters_pass((1,u32::MAX,3),
            (2,u32::MAX,4)));
    }
}

/// The native Explorer Desktop View is tied to a particular display
/// topology. Changing monitors invalidates icon positions and hit targets,
/// even if the shell item count and labels remain unchanged. Never keep a
/// selective visibility policy active across a display change.
fn monitor_topology_changed(initial:&str,current:&str)->bool{
    !initial.is_empty() && (current.is_empty() || initial!=current)
}

#[cfg(test)]
mod display_guard_tests {
    use super::*;
    #[test]
    fn abort_on_monitor_disconnect_reconnect_or_resolution_change() {
        let dual="DISPLAY1@2560x1440+0+0|DISPLAY2@2560x1440+2560+0";
        let single="DISPLAY1@2048x1152+0+0";
        assert!(!monitor_topology_changed(single,single));
        assert!(monitor_topology_changed(dual,single));
        assert!(monitor_topology_changed(single,dual));
        assert!(monitor_topology_changed(single,""));
        assert!(monitor_topology_changed("DISPLAY1@2048x1152+0+0","DISPLAY1@1920x1080+0+0"));
    }
}


/// Read-only inspection of the exact Explorer-owned native policy mapping.
/// Does NOT create, renew, modify, unhook, unload DLLs or touch icon positions.
/// A stale mapping is an object kept alive by another process, not evidence
/// that its original Explorer callback is still actively hiding anything.
pub fn native_policy_status()->Value {
    let Some(view)=DesktopView::acquire() else {
        return json!({"mapping_present":false,"reason":"Explorer Desktop unavailable",
            "read_only":true});
    };
    let list=view.list_window();
    let mut pid=0u32;
    unsafe{GetWindowThreadProcessId(list,&mut pid);}
    if pid==0 {
        return json!({"mapping_present":false,"reason":"Native Explorer owner unavailable",
            "read_only":true});
    }
    let label=proto::shared_memory_name(pid);
    let handle=unsafe{OpenFileMappingW(FILE_MAP_READ,0,label.as_ptr())};
    if handle.is_null() {
        // Access denied or another inspection error is distinct from a
        // confirmed missing mapping. Installation must fail closed.
        let error = unsafe { GetLastError() };
        return json!({"mapping_present":error!=2,"mapping_absence_confirmed":error==2,
            "explorer_pid":pid,"win32_error":error,
            "native_hook_installed_by_inspector":false,"read_only":true});
    }
    // The header prefix has the same layout in older fast-filter mappings.
    // Inspect only that prefix so an older, smaller live mapping is visible.
    let header_len = std::mem::offset_of!(proto::FilterPolicy, hook_error);
    let ptr=unsafe{MapViewOfFile(handle,FILE_MAP_READ,0,0,header_len)} as *const proto::FilterPolicy;
    if ptr.is_null(){
        unsafe{CloseHandle(handle);}
        return json!({"mapping_present":true,"explorer_pid":pid,
            "read_failed":true,"read_only":true});
    }
    // The host publishes generation (odd=in progress, even=complete) and
    // leases atomically. Snapshot only minimal safe scalar metadata.
    let details=unsafe{
        let generation=&*(std::ptr::addr_of!((*ptr).generation)
            as *const std::sync::atomic::AtomicU64);
        let lease=&*(std::ptr::addr_of!((*ptr).lease_tick_ms)
            as *const std::sync::atomic::AtomicU64);
        let before=generation.load(Ordering::Acquire);
        let issued=lease.load(Ordering::Acquire);
        let now=GetTickCount64();
        let age=now.saturating_sub(issued);
        let magic=std::ptr::read_volatile(std::ptr::addr_of!((*ptr).magic));
        let abi=std::ptr::read_volatile(std::ptr::addr_of!((*ptr).abi_version));
        let mapping_pid=std::ptr::read_volatile(std::ptr::addr_of!((*ptr).explorer_pid));
        let mapping_hwnd=std::ptr::read_volatile(std::ptr::addr_of!((*ptr).listview_hwnd));
        let hook=(&*std::ptr::addr_of!((*ptr).hook_state)).load(Ordering::Acquire);
        let flags=std::ptr::read_volatile(std::ptr::addr_of!((*ptr).policy_flags));
        let after=generation.load(Ordering::Acquire);
        let consistent=before>0 && before==after && before%2==0;
        json!({
            "mapping_present":true,"explorer_pid":pid,
            "mapping_matches_live_Explorer":mapping_pid==pid
                && mapping_hwnd==list as usize as u64,
            "protocol_magic_valid":magic==proto::MAGIC,
            "protocol_abi_valid":abi==proto::ABI_VERSION,
            "stable_generation":consistent,
            "generation":after,
            "hook_state":hook,
            "policy_flags":flags,
            "lease_age_ms":age,
            "lease_expired":age>=proto::HEARTBEAT_TIMEOUT_MS,
            "native_policy_currently_valid":consistent && age<proto::HEARTBEAT_TIMEOUT_MS
                && magic==proto::MAGIC
                && (abi==proto::ABI_VERSION || (cfg!(feature = "native-fast-tabs") && matches!(abi, 16 | 17 | 18)))
                && mapping_pid==pid && mapping_hwnd==list as usize as u64,
            "read_only":true,
            "native_hook_installed_by_inspector":false,
            "native_icon_positions_changed":false,
        })
    };
    unsafe{
        UnmapViewOfFile(ptr as *const c_void);
        CloseHandle(handle);
    }
    details
}

pub fn emergency_unhook() -> Value {
    let Some(view)=DesktopView::acquire() else {
        return json!({"command_sent":false,"reason":"Explorer Desktop unavailable"});
    };
    let mut response=0usize;
    let result=unsafe {
        SendMessageTimeoutW(view.list_window(),proto::APP_HOOK_REMOVE,
            proto::HOOK_COOKIE,0,0x0002,1500,&mut response)
    };
    json!({
        "command_sent":result!=0,
        "native_filter_detach_requested":true,
        "native_icon_positions_changed_by_command":false,
        "warning":"Request is safe and idempotent; if a previous hook survived its controller, the in-process ListView subclass handles this emergency detach."
    })
}

pub fn policy_preview(seconds: u32, workspace: &std::path::Path) -> Value {
    let reject = |why: String| json!({"started":false, "reason":why,
        "explorer_hook_installed": false, "files_touched":0, "icons_moved":0});
    let Some(view) = DesktopView::acquire() else { return reject("Explorer Desktop unavailable".into()); };
    if view.icons_visible() != Some(true) {
        return reject("Explorer native icons must be visible; no settings were changed".into());
    }
    let shell = view.items();
    let list = view.list_window();
    let acc: Vec<String> = list_items(list).into_iter().map(|it| it.name).collect();
    let alignment = explorer_native::audit_native_indices(&shell, &acc);
    if !alignment.safe_to_address_by_index {
        return reject(format!("Index/name validation failed, {}/{} same-index names; refusing to publish a policy", alignment.same_index_labels, shell.len()));
    }
    let state = match State::load(workspace) {
        Ok(s) => s,
        Err(_) => return reject("DesktopCraft workspace unavailable".into()),
    };
    let decision = NativePresentation::decide(&state, &shell);
    if decision.conflicting_active_references != 0 {
        return reject("Simultaneous duplicate active native pins cannot share one position".into());
    }
    let by_path: HashMap<String, NativeVisibility> = decision.items.into_iter()
        .map(|i| (i.path.to_lowercase(), i.visibility)).collect();
    let Some(records) = proto::verified_records(shell.iter().enumerate().map(|(idx,item)| {
        let visibility = by_path.get(&item.path.to_lowercase()).copied().unwrap_or(NativeVisibility::Loose);
        (idx, item.label.as_str(), matches!(visibility,
            NativeVisibility::InactiveTab | NativeVisibility::InactivePage | NativeVisibility::RolledUp))
    }), shell.len()) else {
        return reject("Could not validate a complete, uniquely named native icon mapping".into());
    };
    let hidden = records.iter().filter(|rec| rec.state == proto::STATE_HIDE).count();
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(list, &mut pid); }
    if pid == 0 { return reject("No validated Explorer window process ID".into()); }
    let memory = match unsafe { SharedPolicy::create(pid) } {
        Ok(memory) => memory,
        Err(reason) => return reject(reason),
    };
    let hwnd = list as usize as u64;
    let mut valid_reads = 0u32;
    let cycles = seconds.clamp(1, 15) * 5;
    for i in 0..cycles {
        unsafe {
            memory.publish(pid, hwnd, shell.len(), &records, (i as u64 + 1)*2,0);
            if memory.verify(pid, hwnd, shell.len()) { valid_reads += 1; }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    drop(memory);
    json!({
        "started":true,
        "policy_mapping_published_then_released":true,
        "protocol_version":proto::ABI_VERSION,
        "native_desktop_item_count":shell.len(),
        "matching_native_item_labels":alignment.same_index_labels,
        "hidden_items_in_policy":hidden,
        "visible_items_in_policy":records.len() - hidden,
        "mapping_self_checks_passed":valid_reads,
        "mapping_self_checks_expected":cycles,
        "explorer_hook_installed":false,
        "icons_moved":0,
        "files_touched":0,
        "warning":"Publishing a read-only preview policy does not hide items. No DLL injected or installed."
    })
}
