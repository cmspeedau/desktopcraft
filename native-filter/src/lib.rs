//! Experimental independent shell integration DLL for DesktopCraft v0.2.0.
//!
//! The EXPORTED hook is opt-in and must be installed only by a guarded
//! DesktopCraft controller, never on ordinary app startup. Its default is
//! OBSERVE/PASS THROUGH. Selective paint and mouse suppression are disabled
//! unless a fresh fully verified shared-memory policy explicitly sets flags.
//!
//! We retain a process-local module reference while native subclasses are
//! active. If the controller dies unexpectedly, callbacks remain in a loaded
//! module and fail open when the heartbeat expires. After explicit detach,
//! a delayed FreeLibraryAndExitThread releases that reference only after
//! subclass callbacks have returned (otherwise unload could crash Explorer).
//!
//! Do not substitute this for live Shell verification: the precise Explorer
//! custom-draw notification contract is not yet empirically established.
#![cfg(windows)]
#[path = "../../src/native_filter_protocol.rs"]
mod protocol;
mod paint_hold;
#[cfg(feature = "native-fast-tabs")]
mod tab_moves;

use std::{
    cell::Cell, collections::HashSet, ffi::c_void, mem, ptr,
    sync::{Mutex, OnceLock, atomic::{AtomicU32, AtomicU64, Ordering}},
};
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HWND, POINT};
use winapi::um::winuser::{
    CallNextHookEx, GetClassNameW, GetParent, GetWindowThreadProcessId,
    SendMessageW, PostMessageW, ScreenToClient, ClientToScreen, MSG, WM_NOTIFY, WM_NULL, WM_LBUTTONDOWN,
    WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONDBLCLK,
    WM_RBUTTONUP, WM_CONTEXTMENU, WM_MOUSEMOVE,
    GetCapture, SetCapture, ReleaseCapture, GetAsyncKeyState, IsWindow, WM_CANCELMODE, WM_CAPTURECHANGED,
    RedrawWindow, RDW_INVALIDATE, RDW_UPDATENOW,
};

#[link(name="kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetTickCount64() -> u64;
    fn OpenFileMappingW(access: u32, inherit: i32, name: *const u16) -> *mut c_void;
    fn MapViewOfFile(mapping: *mut c_void, access: u32, high: u32, low: u32, len: usize) -> *mut c_void;
    fn UnmapViewOfFile(base: *const c_void) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn GetModuleHandleExW(flags: u32, name: *const u16, handle: *mut *mut c_void) -> i32;
    fn CreateThread(attributes:*mut c_void, stack:usize,
        start:Option<unsafe extern "system" fn(*mut c_void)->u32>,
        parameter:*mut c_void, flags:u32, thread_id:*mut u32) -> *mut c_void;
    fn Sleep(milliseconds:u32);
    fn FreeLibraryAndExitThread(module:*mut c_void, code:u32) -> !;
}
type SubclassCallback = Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM, usize, usize) -> LRESULT>;
#[link(name="comctl32")]
extern "system" {
    fn SetWindowSubclass(hwnd: HWND, proc: SubclassCallback, id: usize, data: usize) -> i32;
    fn RemoveWindowSubclass(hwnd: HWND, proc: SubclassCallback, id: usize) -> i32;
    fn DefSubclassProc(hwnd: HWND, msg: UINT, wp: WPARAM, lp: LPARAM) -> LRESULT;
}
const FILE_MAP_ALL_ACCESS: u32 = 0xF001F;
const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x4;
const SUBCLASS_PARENT: usize = 0xDCA7_0001;
const SUBCLASS_LIST: usize = 0xDCA7_0002;
const NM_CUSTOMDRAW: i32 = -12;
const LVN_ITEMCHANGING: i32 = -100;
const LVN_ITEMCHANGED: i32 = -101;
#[cfg(feature = "native-fast-tabs")]
const LVN_BEGINDRAG: i32 = -109;
const LVN_GETDISPINFOW: i32 = -177;
const LVN_GETDISPINFOA: i32 = -150;
const LVIF_TEXT: u32 = 0x1;
const LVIF_IMAGE: u32 = 0x2;
const LVIF_DI_SETITEM: u32 = 0x1000;
static EMPTY_NATIVE_TEXT: [u16; 1] = [0];
const LVN_ODCACHEHINT: i32 = -113;
const WM_DRAWITEM_LOCAL: u32 = 0x002B;
const WM_PRINTCLIENT_LOCAL: u32 = 0x0318;
const WM_PAINT_LOCAL: u32 = 0x000F;
const CDDS_PREPAINT: u32 = 0x1;
const CDDS_ITEMPREPAINT: u32 = 0x10001;
const CDRF_NOTIFYITEMDRAW: LRESULT = 0x20;
const CDRF_SKIPDEFAULT: LRESULT = 0x4;
const LVM_FIRST: u32 = 0x1000;
const LVM_GETITEMCOUNT: u32 = LVM_FIRST + 4;
const LVM_GETITEMTEXTW: u32 = LVM_FIRST + 115;
const LVM_HITTEST: u32 = LVM_FIRST + 18;
const LVM_GETITEMRECT: u32 = LVM_FIRST + 14;
const LVIR_ICON: i32 = 1;
const WM_INPUT_DIAG_OK: LRESULT = 1;
const LVM_GETITEMSTATE: u32 = LVM_FIRST + 44;
const LVM_SETITEMSTATE: u32 = LVM_FIRST + 43;
const LVIS_FOCUSED: u32 = 0x1;
const LVIS_SELECTED: u32 = 0x2;
const FILTERED_NATIVE_STATE: u32 = LVIS_FOCUSED | LVIS_SELECTED;
const COUNT_TRANSITION_GRACE_MS: u64 = 2000;

thread_local! {
    static IN_LIST_PAINT: Cell<u32> = const { Cell::new(0) };
    static IN_LABEL_LOOKUP: Cell<bool> = const { Cell::new(false) };
}

struct PaintScope;
impl PaintScope {
    fn enter() -> Self {
        IN_LIST_PAINT.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self
    }
}
impl Drop for PaintScope {
    fn drop(&mut self) {
        IN_LIST_PAINT.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

struct LabelLookupScope(bool);
impl LabelLookupScope {
    fn enter() -> Self {
        let prev = IN_LABEL_LOOKUP.with(|flag| {
            let was = flag.get();
            flag.set(true);
            was
        });
        Self(prev)
    }
}
impl Drop for LabelLookupScope {
    fn drop(&mut self) {
        IN_LABEL_LOOKUP.with(|flag| flag.set(self.0));
    }
}

#[repr(C)]
struct NMHdr { hwnd_from: HWND, id_from: usize, code: i32 }
#[repr(C)]
struct Rect { left:i32, top:i32, right:i32, bottom:i32 }
#[repr(C)]
struct NMCustomDraw {
    hdr: NMHdr, stage: u32, hdc: *mut c_void, rect: Rect,
    item: usize, state: u32, lparam: isize,
}
#[repr(C)]
struct NMListView {
    hdr: NMHdr,
    item: i32,
    subitem: i32,
    new_state: u32,
    old_state: u32,
    changed: u32,
    action_x: i32,
    action_y: i32,
    param: isize,
}
fn newly_selected_or_focused(new_state:u32, old_state:u32) -> bool {
    (new_state & FILTERED_NATIVE_STATE) & !(old_state & FILTERED_NATIVE_STATE) != 0
}
#[repr(C)]
struct LVItem {
    mask: u32, item: i32, subitem: i32, state: u32, state_mask: u32,
    text: *mut u16, max_chars: i32, image:i32, param:isize,
    indent:i32, group_id:i32, column_count:u32, columns:*mut u32,
    col_format:*mut i32, group: i32,
}
#[repr(C)]
struct LVHitTest { x:i32, y:i32, flags:u32, item:i32, subitem:i32, group:i32 }
#[repr(C)]
struct NMLVDispInfoW { hdr: NMHdr, item: LVItem }

/// Speculative output filter for LVN_GETDISPINFOW (measured 1456 calls in
/// three seconds). It cannot hide icons already cached by Explorer; real pixel
/// parity still requires a user-verified visual test. Never set DI_SETITEM for
/// hidden data: the original icon text must not be cached as permanently blank.
fn suppress_requested_display_info(item: &mut LVItem) -> bool {
    let requested=item.mask & (LVIF_TEXT | LVIF_IMAGE);
    if requested & LVIF_TEXT != 0 {
        item.text=EMPTY_NATIVE_TEXT.as_ptr() as *mut u16;
        item.max_chars=1;
    }
    if requested & LVIF_IMAGE != 0 { item.image=-1; }
    item.mask &= !LVIF_DI_SETITEM;
    requested != 0
}

struct FilterState {
    list: usize,
    parent: usize,
    mapping_handle: usize,
    mapping_ptr: usize,
    generation: u64,
    last_paint_hold: u64,
    observed_count: usize,
    validated_labels: usize,
    verified: bool,
    hidden: Vec<bool>,
    hidden_labels: HashSet<Vec<u16>>,
    count_mismatch_since: AtomicU64,
    flags: u32,
    drag_capture_active: bool,
    drag_capture_index: usize,
    list_subclass_attached: bool,
    parent_subclass_attached: bool,
}
// All window callbacks execute on the target UI thread. A Mutex also provides
// fail-open try_lock on reentrant list-view notifications.
unsafe impl Send for FilterState {}
static FILTER: OnceLock<Mutex<Option<FilterState>>> = OnceLock::new();
static PINNED: OnceLock<usize> = OnceLock::new();
fn state() -> &'static Mutex<Option<FilterState>> { FILTER.get_or_init(|| Mutex::new(None)) }

unsafe fn atomic_generation(p: *const protocol::FilterPolicy) -> &'static AtomicU64 {
    &*(ptr::addr_of!((*p).generation) as *const AtomicU64)
}
unsafe fn atomic_lease_tick(p:*const protocol::FilterPolicy)->u64{
    (*(ptr::addr_of!((*p).lease_tick_ms) as *const AtomicU64))
        .load(Ordering::Acquire)
}
unsafe fn counter(slot: *mut u32) -> &'static AtomicU32 {
    &*(slot as *const AtomicU32)
}
unsafe fn increment(slot: *mut u32) {
    counter(slot).fetch_add(1, Ordering::Relaxed);
}

#[allow(clippy::too_many_arguments)]
fn count_transition_header_valid(
    magic: u32,
    version: u32,
    policy_count: u32,
    live_count: u32,
    target_pid: u32,
    current_pid: u32,
    target_hwnd: u64,
    current_hwnd: u64,
    lease_tick: u64,
    now_tick: u64,
    generation: u64,
    expected_generation: u64,
    mismatch_since: u64,
) -> bool {
    magic == protocol::MAGIC
        && version == protocol::ABI_VERSION
        && policy_count > 0
        && policy_count as usize <= protocol::MAX_ITEMS
        && live_count > 0
        && live_count as usize <= protocol::MAX_ITEMS
        && policy_count != live_count
        && target_pid == current_pid
        && current_pid != 0
        && target_hwnd == current_hwnd
        && current_hwnd != 0
        && generation > 0
        && generation % 2 == 0
        && generation == expected_generation
        && now_tick >= lease_tick
        && now_tick - lease_tick <= protocol::HEARTBEAT_TIMEOUT_MS
        && mismatch_since > 0
        && now_tick >= mismatch_since
        && now_tick - mismatch_since <= COUNT_TRANSITION_GRACE_MS
}

unsafe fn policy_is_live(st: &FilterState) -> bool {
    if st.mapping_ptr == 0 { return false; }
    let live_count = SendMessageW(st.list as HWND, LVM_GETITEMCOUNT, 0, 0) as usize;
    if live_count != st.observed_count { return false; }
    let p=st.mapping_ptr as *const protocol::FilterPolicy;
    let generation=atomic_generation(p).load(Ordering::Acquire);
    let ok = generation == st.generation && generation % 2 == 0 &&
        protocol::valid_header((*p).magic,(*p).abi_version,(*p).listview_item_count,
        live_count as u32,(*p).explorer_pid,GetCurrentProcessId(),
        (*p).listview_hwnd,st.list as u64,atomic_lease_tick(p),
        GetTickCount64(),generation);
    if ok {
        st.count_mismatch_since.store(0, Ordering::Relaxed);
    }
    ok
}

unsafe fn policy_count_transition_live(st: &FilterState) -> bool {
    if !st.verified || st.mapping_ptr == 0 || st.hidden_labels.is_empty() {
        return false;
    }
    let live_count = SendMessageW(st.list as HWND, LVM_GETITEMCOUNT, 0, 0) as usize;
    if live_count == st.observed_count {
        st.count_mismatch_since.store(0, Ordering::Relaxed);
        return false;
    }
    let p = st.mapping_ptr as *const protocol::FilterPolicy;
    let generation = atomic_generation(p).load(Ordering::Acquire);
    let now = GetTickCount64().max(1);
    let since = match st.count_mismatch_since.compare_exchange(
        0, now, Ordering::AcqRel, Ordering::Acquire,
    ) {
        Ok(_) => now,
        Err(existing) => existing,
    };
    count_transition_header_valid(
        (*p).magic,
        (*p).abi_version,
        (*p).listview_item_count,
        live_count as u32,
        (*p).explorer_pid,
        GetCurrentProcessId(),
        (*p).listview_hwnd,
        st.list as u64,
        atomic_lease_tick(p),
        now,
        generation,
        st.generation,
        since,
    )
}

unsafe fn is_hidden(st: &FilterState, index: usize, flag:u32) -> bool {
    st.verified && (st.flags & flag) != 0 && policy_is_live(st)
        && st.hidden.get(index).copied().unwrap_or(false)
}
unsafe fn name_from_list(list: HWND, index: usize) -> Option<Vec<u16>> {
    let _lookup = LabelLookupScope::enter();
    let mut text=[0u16; protocol::MAX_LABEL_U16];
    let mut lvi: LVItem=mem::zeroed();
    lvi.item=index as i32;
    lvi.subitem=0;
    lvi.text=text.as_mut_ptr();
    lvi.max_chars=text.len() as i32;
    let n=SendMessageW(list,LVM_GETITEMTEXTW,index,
        &mut lvi as *mut LVItem as LPARAM) as usize;
    if n == 0 || n >= text.len() { return None; }
    Some(text[..n].to_vec())
}

#[cfg(feature = "native-fast-tabs")]
unsafe fn publish_native_drag(st: &FilterState, source: usize) {
    const LVM_GETNEXTITEM: u32 = LVM_FIRST + 12;
    const LVNI_SELECTED: LPARAM = 2;
    if !st.verified || source >= st.observed_count || st.hidden.get(source) != Some(&false)
        || !policy_is_live(st)
    {
        return;
    }
    let policy = st.mapping_ptr as *mut protocol::FilterPolicy;
    let tick = &*ptr::addr_of!((*policy).native_drag_tick).cast::<AtomicU64>();
    // The first begin notification for this press is the authoritative one.
    if tick.load(Ordering::Acquire) != 0 {
        return;
    }
    let mut selection = Vec::new();
    let mut next = usize::MAX;
    loop {
        let index = SendMessageW(st.list as HWND, LVM_GETNEXTITEM, next, LVNI_SELECTED);
        if index < 0 {
            break;
        }
        let index = index as usize;
        if index >= st.observed_count || (next != usize::MAX && index <= next)
            || st.hidden.get(index) != Some(&false)
        {
            return;
        }
        selection.push(index as u32);
        next = index;
    }
    // Some Shell views notify before a newly grabbed item is selected.
    // The notification itself identifies that real drag source.
    if !selection.contains(&(source as u32)) {
        selection.clear();
        selection.push(source as u32);
    }
    (*policy).native_drag_generation = st.generation;
    (*policy).native_drag_source = source as u32;
    (*policy).native_drag_count = selection.len() as u32;
    (&mut (*policy).native_drag_selection)[..selection.len()].copy_from_slice(&selection);
    tick.store(GetTickCount64().max(1), Ordering::Release);
}

unsafe fn load_validated_snapshot(st: &mut FilterState) {
    let p=st.mapping_ptr as *const protocol::FilterPolicy;
    if p.is_null() {
        st.verified=false;
        st.flags=0;
        st.hidden.clear();
        st.hidden_labels.clear();
        st.count_mismatch_since.store(0, Ordering::Relaxed);
        return;
    }
    let live_count = SendMessageW(st.list as HWND, LVM_GETITEMCOUNT, 0, 0) as usize;
    let generation=atomic_generation(p).load(Ordering::Acquire);
    if st.verified
        && generation == st.generation
        && live_count != st.observed_count
        && policy_count_transition_live(st)
    {
        // Keep verified label masking active across brief add/delete count deltas
        // until the host publishes the updated generation or grace expires.
        return;
    }
    let previous_verified = st.verified;
    let previous_hidden = std::mem::take(&mut st.hidden);
    st.verified=false;
    st.flags=0;
    st.hidden_labels.clear();
    st.count_mismatch_since.store(0, Ordering::Relaxed);
    if live_count == 0 || live_count > protocol::MAX_ITEMS {
        increment(ptr::addr_of_mut!((*(st.mapping_ptr as *mut protocol::FilterPolicy)).hook_rejected_stale));
        return;
    }
    if !protocol::valid_header((*p).magic,(*p).abi_version,(*p).listview_item_count,
        live_count as u32,(*p).explorer_pid,GetCurrentProcessId(),
        (*p).listview_hwnd,st.list as u64,atomic_lease_tick(p),
        GetTickCount64(),generation) || (*p).record_count as usize != live_count {
        increment(ptr::addr_of_mut!((*(st.mapping_ptr as *mut protocol::FilterPolicy)).hook_rejected_stale));
        return;
    }
    let mut hidden=vec![false; live_count];
    let mut hidden_labels=HashSet::new();
    let mut matched=0usize;
    for index in 0..live_count {
        let record=&(*p).records[index];
        if record.index as usize != index
            || record.label_len == 0
            || record.label_len as usize >= protocol::MAX_LABEL_U16
            || (record.state != protocol::STATE_SHOW && record.state != protocol::STATE_HIDE) {
            break;
        }
        let slice = &record.label_utf16[..record.label_len as usize];
        match name_from_list(st.list as HWND,index) {
            Some(actual) if actual.as_slice() == slice => {
                if record.state == protocol::STATE_HIDE {
                    hidden[index] = true;
                    hidden_labels.insert(actual);
                }
                matched += 1;
            }
            _ => break,
        }
    }
    (*(st.mapping_ptr as *mut protocol::FilterPolicy)).hook_label_matches=matched as u32;
    if atomic_generation(p).load(Ordering::Acquire) != generation { return; }
    st.validated_labels=matched;
    if matched == live_count {
        if previous_verified && previous_hidden.len() == hidden.len() {
            for (index, (&old, &new)) in previous_hidden.iter().zip(&hidden).enumerate() {
                if old != new { paint_hold::note_item(st.list as HWND, index); }
            }
        } else {
            paint_hold::note_unknown_damage(st.list as HWND);
        }
        st.generation=generation;
        st.observed_count=live_count;
        st.hidden=hidden;
        st.hidden_labels=hidden_labels;
        st.count_mismatch_since.store(0, Ordering::Relaxed);
        st.flags=(*p).policy_flags;
        st.verified=true;
        // Hidden tabs must not leave Explorer native items selected or
        // focused. This is a guarded action only when input filtering is
        // explicitly enabled and the complete native identity map matches.
        if (st.flags & protocol::FLAG_INPUT_HIDE) != 0 {
            for (index,hide) in st.hidden.iter().enumerate() {
                if !hide {continue;}
                let state=SendMessageW(st.list as HWND,LVM_GETITEMSTATE,index,
                    FILTERED_NATIVE_STATE as LPARAM) as u32;
                if state & FILTERED_NATIVE_STATE != 0 {
                    let mut item:LVItem=mem::zeroed();
                    item.state=0;
                    item.state_mask=FILTERED_NATIVE_STATE;
                    SendMessageW(st.list as HWND,LVM_SETITEMSTATE,index,
                        &mut item as *mut LVItem as LPARAM);
                }
            }
        }
    }
}

unsafe fn refresh() {
    let Ok(mut guard)=state().try_lock() else {return};
    let Some(st)=guard.as_mut() else {return};
    let generation=atomic_generation(st.mapping_ptr as *const protocol::FilterPolicy)
        .load(Ordering::Acquire);
    if generation != st.generation || !policy_is_live(st) {
        load_validated_snapshot(st);
    }
}

/// Runs on the ListView's own thread. Painting re-enters the subclasses, so
/// release the policy lock before asking the ListView to repaint.
unsafe fn apply_policy(hwnd: HWND, generation: u64, repaint: bool) -> bool {
    let paint_count = || {
        let guard = state().try_lock().ok()?;
        let st = guard.as_ref()?;
        if st.list != hwnd as usize
            || !protocol::confirms_generation(true, generation, st.generation)
            || !st.verified || !policy_is_live(st) {
            return None;
        }
        let p = st.mapping_ptr as *mut protocol::FilterPolicy;
        Some(counter(ptr::addr_of_mut!((*p).hook_listview_paint)).load(Ordering::Relaxed))
    };
    refresh();
    let Some(before) = paint_count() else { return false };
    if !repaint { return true; }
    let bounds = paint_hold::repaint_bounds(hwnd);
    paint_hold::finish(hwnd, false);
    let paint = |rect| RedrawWindow(hwnd, rect, ptr::null_mut(), RDW_INVALIDATE | RDW_UPDATENOW) != 0
        && paint_count().is_some_and(|after| after != before);
    bounds.as_ref().is_some_and(|rect| paint(rect)) || paint(ptr::null())
}

unsafe fn hold_policy_paint(hwnd: HWND, generation: u64) -> bool {
    if !apply_policy(hwnd, generation, false) { return false; }
    let Ok(mut guard) = state().try_lock() else { return false };
    let Some(st) = guard.as_mut() else { return false };
    // A repeated or late message must not extend Explorer's paint hold.
    if st.last_paint_hold == generation || !paint_hold::begin(hwnd) { return false; }
    st.last_paint_hold = generation;
    true
}
unsafe fn is_target_list(hwnd: HWND) -> bool {
    if hwnd.is_null() {return false;}
    let mut name=[0u16;64];
    let n=GetClassNameW(hwnd,name.as_mut_ptr(),name.len() as i32);
    let mut owner=0;
    let thread=GetWindowThreadProcessId(hwnd,&mut owner);
    owner==GetCurrentProcessId() && thread==GetCurrentThreadId() && n > 0
        && String::from_utf16_lossy(&name[..n as usize]) == "SysListView32"
}
unsafe fn pin_own_module() -> bool {
    *PINNED.get_or_init(|| {
        let mut handle: *mut c_void=ptr::null_mut();
        if GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            DesktopCraftFilterHook as *const () as *const u16,&mut handle)!=0 {
            handle as usize
        } else {0}
    }) != 0
}

unsafe extern "system" fn delayed_module_release(module:*mut c_void)->u32 {
    // The controller removes its WH_GETMESSAGE hook promptly after the
    // subclass detach ACK. FreeLibraryAndExitThread is specifically designed
    // not to return into code that has just been unloaded.
    Sleep(1800);
    FreeLibraryAndExitThread(module,0);
}

unsafe fn schedule_module_release() {
    let reference=PINNED.get().copied().unwrap_or_default();
    if reference==0 {return;}
    let thread=CreateThread(ptr::null_mut(),0,Some(delayed_module_release),
        reference as *mut c_void,0,ptr::null_mut());
    // On failure, deliberately retain the module until Explorer terminates:
    // memory leaks are preferable to a dangling WindowSubclass callback.
    if !thread.is_null(){CloseHandle(thread);}
}

unsafe fn install(hwnd: HWND) {
    // Do not pin the DLL until an exact, complete, live native identity map
    // has passed inside Explorer on the target ListView UI thread.
    if !is_target_list(hwnd) {return;}
    let parent=GetParent(hwnd);
    if parent.is_null(){return;}
    let pid=GetCurrentProcessId();
    let label=protocol::shared_memory_name(pid);
    let handle=OpenFileMappingW(FILE_MAP_ALL_ACCESS,0,label.as_ptr());
    if handle.is_null(){return;}
    let mapping=MapViewOfFile(handle,FILE_MAP_ALL_ACCESS,0,0,
        mem::size_of::<protocol::FilterPolicy>()) as *mut protocol::FilterPolicy;
    if mapping.is_null(){CloseHandle(handle);return;}
    let count=SendMessageW(hwnd,LVM_GETITEMCOUNT,0,0) as usize;
    if count==0 || count>protocol::MAX_ITEMS {
        UnmapViewOfFile(mapping as *const c_void);
        CloseHandle(handle);
        return;
    }
    let mut guard=state().lock().unwrap_or_else(|err|err.into_inner());
    if guard.is_some() {
        UnmapViewOfFile(mapping as *const c_void);
        CloseHandle(handle);
        return;
    }
    let mut st=FilterState {
        list:hwnd as usize, parent:parent as usize,
        mapping_handle:handle as usize, mapping_ptr:mapping as usize,
        generation:0, last_paint_hold:0, observed_count:count, validated_labels:0,
        verified:false,hidden:Vec::new(),hidden_labels:HashSet::new(),
        count_mismatch_since:AtomicU64::new(0),flags:0,
        drag_capture_active:false,drag_capture_index:usize::MAX,
        list_subclass_attached:false,parent_subclass_attached:false,
    };
    load_validated_snapshot(&mut st);
    if !st.verified {
        (*mapping).hook_error=3; // Wrong/missing/stale native label map.
        UnmapViewOfFile(mapping as *const c_void);
        CloseHandle(handle);
        return;
    }
    if !pin_own_module() {
        (*mapping).hook_error=4; // Unable to retain DLL while subclassed.
        UnmapViewOfFile(mapping as *const c_void);
        CloseHandle(handle);
        return;
    }
    // Guarded installation: if either subclass fails, remove the other and
    // unmap. Never leave one hook registered in an unloaded module.
    if SetWindowSubclass(parent,Some(parent_proc),SUBCLASS_PARENT,0)==0 {
        (*mapping).hook_error=1;
        UnmapViewOfFile(mapping as *const c_void);
        CloseHandle(handle);
        schedule_module_release();
        return;
    }
    st.parent_subclass_attached=true;
    if SetWindowSubclass(hwnd,Some(list_proc),SUBCLASS_LIST,0)==0 {
        let removed=RemoveWindowSubclass(parent,Some(parent_proc),SUBCLASS_PARENT);
        (*mapping).hook_error=2;
        if removed!=0 || IsWindow(parent)==0 {
            UnmapViewOfFile(mapping as *const c_void);
            CloseHandle(handle);
            schedule_module_release();
        }else{
            // The parent subclass MAY still be live. Retain the state
            // and pinned DLL until a later verified detach; never unload
            // a module that Explorer can still call.
            st.verified=false;st.flags=0;
            (*mapping).hook_state.store(protocol::HOOK_STATE_ATTACHED, Ordering::Release);
            *guard=Some(st);
        }
        return;
    }
    st.list_subclass_attached=true;
    (*mapping).hook_state.store(protocol::HOOK_STATE_ATTACHED, Ordering::Release);
    *guard=Some(st);
}

/// Unlink the in-process Explorer callbacks WITHOUT holding the native
/// state mutex during window/message operations. Removing a subclass or
/// releasing mouse capture can synchronously dispatch another callback;
/// holding the mutex would deadlock Explorer's UI thread.
fn subclass_removal_confirmed(remove_result:i32,window_still_exists:i32)->bool {
    remove_result!=0 || window_still_exists==0
}

/// Evaluate the two-subclass detach state machine from raw Win32 results.
/// Returns `(remaining_list_attached, remaining_parent_attached, fully_detached)`.
fn detach_subclasses_outcome(
    list_attached: bool,
    list_removed: i32,
    list_alive: i32,
    parent_attached: bool,
    parent_removed: i32,
    parent_alive: i32,
) -> (bool, bool, bool) {
    let remaining_list = list_attached && !subclass_removal_confirmed(list_removed, list_alive);
    let remaining_parent =
        parent_attached && !subclass_removal_confirmed(parent_removed, parent_alive);
    (remaining_list, remaining_parent, !remaining_list && !remaining_parent)
}

#[cfg(test)]
mod subclass_detach_ack_tests {
    use super::{detach_subclasses_outcome, subclass_removal_confirmed};
    #[test]
    fn detachment_requires_real_ack_or_destroyed_window() {
        assert!(subclass_removal_confirmed(1,1));
        assert!(subclass_removal_confirmed(0,0));
        assert!(!subclass_removal_confirmed(0,1));
    }

    #[test]
    fn uninstall_detaches_only_when_both_subclasses_confirm_removal() {
        // (a) Both succeed -> fully detached.
        assert_eq!(detach_subclasses_outcome(true, 1, 1, true, 1, 1), (false, false, true));
        // (b) List removal fails on live window while parent succeeds -> stays attached.
        assert_eq!(detach_subclasses_outcome(true, 0, 1, true, 1, 1), (true, false, false));
        // (c) Parent removal fails on live window while list succeeds -> stays attached.
        assert_eq!(detach_subclasses_outcome(true, 1, 1, true, 0, 1), (false, true, false));
        // (d) Both fail on live windows -> both stay attached.
        assert_eq!(detach_subclasses_outcome(true, 0, 1, true, 0, 1), (true, true, false));
        // (e) Removal returns 0 on destroyed window (IsWindow == 0) -> detached.
        assert_eq!(detach_subclasses_outcome(true, 0, 0, true, 0, 0), (false, false, true));
    }
}

unsafe fn uninstall()->bool {
    let current=match state().try_lock() {
        Ok(mut guard)=>guard.take(),
        Err(std::sync::TryLockError::Poisoned(error))=>error.into_inner().take(),
        Err(std::sync::TryLockError::WouldBlock)=>return false,
    };
    let Some(mut st)=current else {return true;};
    paint_hold::finish(st.list as HWND, true);
    if st.drag_capture_active {
        let p=st.mapping_ptr as *mut protocol::FilterPolicy;
        (*p).drag_phase=3;
        (*p).drag_cancel_reason=6;
        (*p).drag_cancelled_count=(*p).drag_cancelled_count.saturating_add(1);
        st.drag_capture_active=false;
        if GetCapture()==st.list as HWND {ReleaseCapture();}
    }
    let (list_removed, list_alive) = if st.list_subclass_attached {
        let hwnd = st.list as HWND;
        (RemoveWindowSubclass(hwnd, Some(list_proc), SUBCLASS_LIST), IsWindow(hwnd))
    } else {
        (1, 0)
    };
    let (parent_removed, parent_alive) = if st.parent_subclass_attached {
        let hwnd = st.parent as HWND;
        (RemoveWindowSubclass(hwnd, Some(parent_proc), SUBCLASS_PARENT), IsWindow(hwnd))
    } else {
        (1, 0)
    };
    let (remaining_list, remaining_parent, fully_detached) = detach_subclasses_outcome(
        st.list_subclass_attached,
        list_removed,
        list_alive,
        st.parent_subclass_attached,
        parent_removed,
        parent_alive,
    );
    st.list_subclass_attached = remaining_list;
    st.parent_subclass_attached = remaining_parent;
    let p=st.mapping_ptr as *mut protocol::FilterPolicy;
    if !fully_detached {
        // At least one removal did NOT ACK and its window still exists.
        // Fail open while keeping both mapping and pinned DLL alive.
        // Never publish DETACHED or release the module on uncertainty.
        st.flags=0;
        st.verified=false;
        (*p).hook_state.store(protocol::HOOK_STATE_ATTACHED, Ordering::Release);
        let mut guard=state().lock().unwrap_or_else(|err|err.into_inner());
        *guard=Some(st);
        return false;
    }
    (*p).hook_state.store(protocol::HOOK_STATE_DETACHED, Ordering::Release);
    UnmapViewOfFile(p as *const c_void);
    CloseHandle(st.mapping_handle as *mut c_void);
    schedule_module_release();
    true
}

/// A controller that died cannot keep Explorer subclasses alive forever.
/// The shared-memory heartbeat is independent of the current icon-generation
/// number; an expired policy must already be fail-open for both paint/input.
fn native_lease_timed_out(issued:u64,now:u64)->bool {
    issued==0 || now.saturating_sub(issued)>=protocol::HEARTBEAT_TIMEOUT_MS
}
unsafe fn expired_ownerless_native_policy()->bool {
    let Ok(guard)=state().try_lock() else {return false};
    let Some(st)=guard.as_ref() else {return false};
    if st.mapping_ptr==0 {return false;}
    let mapping=st.mapping_ptr as *const protocol::FilterPolicy;
    native_lease_timed_out(atomic_lease_tick(mapping),GetTickCount64())
}
#[cfg(test)]
mod expired_native_lease_tests {
    use super::*;
    #[test]
    fn stale_ownerless_lease_expires_at_exact_deadline() {
        let t=protocol::HEARTBEAT_TIMEOUT_MS;
        assert!(!native_lease_timed_out(100,100+t-1));
        assert!(native_lease_timed_out(100,100+t));
        assert!(native_lease_timed_out(100,100+t+1));
        assert!(native_lease_timed_out(0,100));
        assert!(!native_lease_timed_out(u64::MAX,100));
    }
}

unsafe extern "system" fn parent_proc(hwnd: HWND,msg: UINT,wp:WPARAM,lp:LPARAM,
                                      _:usize,_:usize)->LRESULT {
    if msg == WM_CONTEXTMENU {
        if let Ok(guard)=state().try_lock() {
            if let Some(st)=guard.as_ref() {
                let p=st.mapping_ptr as *mut protocol::FilterPolicy;
                increment(ptr::addr_of_mut!((*p).hook_diag_menu_parent_received));
            }
        }
    }
    if msg == WM_DRAWITEM_LOCAL {
        if let Ok(guard)=state().try_lock() {
            if let Some(st)=guard.as_ref() {
                let p=st.mapping_ptr as *mut protocol::FilterPolicy;
                increment(ptr::addr_of_mut!((*p).hook_parent_drawitem));
            }
        }
    }
    if msg==WM_NOTIFY && lp != 0 {
        let hdr=&*(lp as *const NMHdr);
        let Ok(guard)=state().try_lock() else {return DefSubclassProc(hwnd,msg,wp,lp)};
        if let Some(st)=guard.as_ref() {
            if hdr.hwnd_from as usize == st.list {
                let p=st.mapping_ptr as *mut protocol::FilterPolicy;
                #[cfg(feature = "native-fast-tabs")]
                if hdr.code == LVN_BEGINDRAG {
                    let item = (*(lp as *const NMListView)).item;
                    if item >= 0 {
                        publish_native_drag(st, item as usize);
                    }
                }
                increment(ptr::addr_of_mut!((*p).hook_parent_notify));
                let type_counter = match hdr.code {
                    LVN_GETDISPINFOW => ptr::addr_of_mut!((*p).hook_notify_getdispinfo_w),
                    LVN_GETDISPINFOA => ptr::addr_of_mut!((*p).hook_notify_getdispinfo_a),
                    LVN_ODCACHEHINT => ptr::addr_of_mut!((*p).hook_notify_cachehint),
                    _ => ptr::addr_of_mut!((*p).hook_notify_other),
                };
                increment(type_counter);
                if hdr.code == LVN_ITEMCHANGING || hdr.code == LVN_ITEMCHANGED {
                    increment(ptr::addr_of_mut!((*p).hook_selection_notify));
                }
                if hdr.code == LVN_ITEMCHANGING {
                    let change=&*(lp as *const NMListView);
                    if change.item >= 0
                        && newly_selected_or_focused(change.new_state,change.old_state)
                        && is_hidden(st,change.item as usize,protocol::FLAG_INPUT_HIDE) {
                        increment(ptr::addr_of_mut!((*p).hook_input_suppressed));
                        // Windows LVN_ITEMCHANGING: nonzero vetoes selection.
                        return 1;
                    }
                }
            }
            if hdr.hwnd_from as usize == st.list && hdr.code==LVN_GETDISPINFOW {
                let info=&mut *(lp as *mut NMLVDispInfoW);
                if info.item.item >= 0
                    && IN_LIST_PAINT.with(Cell::get) > 0
                    && !IN_LABEL_LOOKUP.with(Cell::get)
                    && is_hidden(st,info.item.item as usize,protocol::FLAG_PAINT_HIDE) {
                    // Explorer's own display-info callback can synchronously
                    // re-enter the parent's WM_NOTIFY path. NEVER call the
                    // next subclass while holding the native policy mutex.
                    drop(guard);
                    let original=DefSubclassProc(hwnd,msg,wp,lp);
                    if suppress_requested_display_info(&mut info.item) {
                        if let Ok(guard)=state().try_lock() {
                            if let Some(current)=guard.as_ref() {
                                let p=current.mapping_ptr as *mut protocol::FilterPolicy;
                                increment(ptr::addr_of_mut!((*p).hook_draw_suppressed));
                            }
                        }
                    }
                    return original;
                }
            }
            if hdr.hwnd_from as usize == st.list && hdr.code==NM_CUSTOMDRAW {
                let cd=&*(lp as *const NMCustomDraw);
                let p=st.mapping_ptr as *mut protocol::FilterPolicy;
                if cd.stage==CDDS_PREPAINT {
                    increment(ptr::addr_of_mut!((*p).hook_parent_customdraw));
                    // Snapshot the current policy under the lock, then
                    // release it BEFORE forwarding Explorer's notification.
                    // DefSubclassProc can dispatch nested list/parent events.
                    let request_item_draw=st.verified
                        && (st.flags & protocol::FLAG_PAINT_HIDE)!=0
                        && (policy_is_live(st) || policy_count_transition_live(st));
                    drop(guard);
                    let original=DefSubclassProc(hwnd,msg,wp,lp);
                    return if request_item_draw {
                        original|CDRF_NOTIFYITEMDRAW
                    } else { original };
                }
                if cd.stage==CDDS_ITEMPREPAINT {
                    increment(ptr::addr_of_mut!((*p).hook_item_customdraw));
                    if is_hidden(st,cd.item,protocol::FLAG_PAINT_HIDE) {
                        increment(ptr::addr_of_mut!((*p).hook_draw_suppressed));
                        return CDRF_SKIPDEFAULT;
                    }
                    if !IN_LABEL_LOOKUP.with(Cell::get)
                        && (st.flags & protocol::FLAG_PAINT_HIDE)!=0
                        && policy_count_transition_live(st)
                    {
                        let list = st.list as HWND;
                        let item = cd.item;
                        drop(guard);
                        if let Some(label) = name_from_list(list, item) {
                            if let Ok(guard) = state().try_lock() {
                                if let Some(current) = guard.as_ref() {
                                    if (current.flags & protocol::FLAG_PAINT_HIDE) != 0
                                        && current.hidden_labels.contains(&label)
                                        && policy_count_transition_live(current)
                                    {
                                        let p = current.mapping_ptr as *mut protocol::FilterPolicy;
                                        increment(ptr::addr_of_mut!((*p).hook_draw_suppressed));
                                        return CDRF_SKIPDEFAULT;
                                    }
                                }
                            }
                        }
                        return DefSubclassProc(hwnd, msg, wp, lp);
                    }
                }
            }
        }
    }
    DefSubclassProc(hwnd,msg,wp,lp)
}

/// Returns an interior coordinate without overflowing on large desktop layouts.
/// Invalid rectangles fail closed; Explorer never receives a pointer from a
/// different process in this diagnostic path.
fn safe_rect_center(rect:&Rect)->Option<(i32,i32)> {
    if rect.right <= rect.left || rect.bottom <= rect.top {return None;}
    let x=(i64::from(rect.left)+i64::from(rect.right))/2;
    let y=(i64::from(rect.top)+i64::from(rect.bottom))/2;
    Some((i32::try_from(x).ok()?,i32::try_from(y).ok()?))
}

/// The caller sends only an integer item index. Both pointer-bearing ListView
/// requests originate *inside* Explorer, in this ListView's own UI thread.
unsafe fn inspect_native_hit(st:&FilterState, hwnd:HWND, requested:usize, mode:u8)->bool {
    let p=st.mapping_ptr as *mut protocol::FilterPolicy;
    increment(ptr::addr_of_mut!((*p).hook_diag_requests));
    (*p).hook_diag_last_index=requested as u32;
    if !st.verified || requested >= st.observed_count || !policy_is_live(st)
        || SendMessageW(hwnd,LVM_GETITEMCOUNT,0,0) as usize != st.observed_count {
        increment(ptr::addr_of_mut!((*p).hook_diag_policy_rejections));
        increment(ptr::addr_of_mut!((*p).hook_diag_failures));
        return false;
    }
    let mut rect=Rect{left:LVIR_ICON,top:0,right:0,bottom:0};
    // This pointer is on the Explorer thread's stack. NEVER call this from
    // the host with &rect or a cross-process LP.* pointer.
    if SendMessageW(hwnd,LVM_GETITEMRECT,requested,&mut rect as *mut Rect as LPARAM)==0 {
        increment(ptr::addr_of_mut!((*p).hook_diag_rect_failures));
        increment(ptr::addr_of_mut!((*p).hook_diag_failures));
        return false;
    }
    let Some((x,y))=safe_rect_center(&rect) else {
        increment(ptr::addr_of_mut!((*p).hook_diag_rect_failures));
        increment(ptr::addr_of_mut!((*p).hook_diag_failures));
        return false;
    };
    // Exact native ListView-client icon center, measured only in Explorer.
    // The host may convert this INTEGER point to physical screen pixels
    // through ClientToScreen; no cross-process LVHITTEST pointer is sent.
    (*p).hook_diag_hover_client_x=x;
    (*p).hook_diag_hover_client_y=y;
    increment(ptr::addr_of_mut!((*p).hook_diag_geometry_ok));
    let mut hit=LVHitTest{x,y,flags:0,item:-1,subitem:0,group:0};
    let index=SendMessageW(hwnd,LVM_HITTEST,0,&mut hit as *mut LVHitTest as LPARAM);
    if index == requested as isize && hit.item == requested as i32 {
        increment(ptr::addr_of_mut!((*p).hook_diag_hit_matches));
        if st.hidden.get(requested).copied().unwrap_or(false) {
            increment(ptr::addr_of_mut!((*p).hook_diag_hidden_targets));
            if mode != 0 && (st.flags & protocol::FLAG_INPUT_HIDE) != 0
                && (i16::MIN as i32..=i16::MAX as i32).contains(&x)
                && (i16::MIN as i32..=i16::MAX as i32).contains(&y) {
                (*p).hook_diag_hover_client_x=x;
                (*p).hook_diag_hover_client_y=y;
                let lparam=(u32::from(x as u16) |
                    (u32::from(y as u16)<<16)) as isize;
                if mode==1 {
                    if PostMessageW(hwnd,WM_MOUSEMOVE,0,lparam)!=0 {
                        increment(ptr::addr_of_mut!((*p).hook_diag_hover_posted));
                    }
                } else if mode==2 {
                    // Right-button messages use ListView client coordinates.
                    // Only numeric WPARAM/LPARAM values cross the message
                    // queue. The target remains Explorer's native ListView.
                    for message in [WM_RBUTTONDOWN,WM_RBUTTONUP] {
                        if PostMessageW(hwnd,message,0,lparam)!=0 {
                            increment(ptr::addr_of_mut!((*p).hook_diag_right_posted));
                        }
                    }
                    let mut screen=POINT{x,y};
                    if ClientToScreen(hwnd,&mut screen)!=0 &&
                        (i16::MIN as i32..=i16::MAX as i32).contains(&screen.x) &&
                        (i16::MIN as i32..=i16::MAX as i32).contains(&screen.y) {
                        let screen_lp=(u32::from(screen.x as u16) |
                            (u32::from(screen.y as u16)<<16)) as isize;
                        if PostMessageW(hwnd,WM_CONTEXTMENU,hwnd as usize,screen_lp)!=0 {
                            increment(ptr::addr_of_mut!((*p).hook_diag_menu_posted));
                        }
                    }
                }
            }
        }
        true
    } else {
        increment(ptr::addr_of_mut!((*p).hook_diag_hit_misses));
        increment(ptr::addr_of_mut!((*p).hook_diag_failures));
        false
    }
}

/// Resolve the actual Explorer ListView item WITHOUT holding the global
/// subclass state mutex: LVM_HITTEST is a synchronous nested window message
/// and may re-enter Windows Common Controls/Explorer notifications.
unsafe fn native_input_hit_index(hwnd:HWND,x:i32,y:i32)->Option<usize>{
    let mut hit=LVHitTest{x,y,flags:0,item:-1,subitem:0,group:0};
    let index=SendMessageW(hwnd,LVM_HITTEST,0,&mut hit as *mut LVHitTest as LPARAM);
    (index>=0).then_some(index as usize)
}
unsafe fn blocks_hit_index(st:&FilterState,index:Option<usize>)->bool{
    st.verified && (st.flags & protocol::FLAG_INPUT_HIDE)!=0
        && policy_is_live(st)
        && index.is_some_and(|index|is_hidden(st,index,protocol::FLAG_INPUT_HIDE))
}
/// A bounded developer-only native gesture bridge. Explorer's own ListView
/// resolves the source index and retains mouse capture, but its default
/// left-button drag handler never receives the captured down/move/up sequence.
/// The host receives only integer index and physical-screen coordinates
/// through the verified shared memory. This cannot move any Desktop file.
unsafe fn cancel_virtual_drag(st:&mut FilterState,hwnd:HWND){
    if !st.drag_capture_active {return;}
    st.drag_capture_active=false;
    st.drag_capture_index=usize::MAX;
    let p=st.mapping_ptr as *mut protocol::FilterPolicy;
    (*p).drag_cancelled_count=(*p).drag_cancelled_count.saturating_add(1);
    counter(ptr::addr_of_mut!((*p).drag_phase)).store(3,Ordering::Release);
    if GetCapture()==hwnd {ReleaseCapture();}
}

fn virtual_drag_travel_valid(moves:u32,begin:(i32,i32),end:(i32,i32))->bool {
    let dx=i64::from(end.0)-i64::from(begin.0);
    let dy=i64::from(end.1)-i64::from(begin.1);
    moves>=3 && dx*dx+dy*dy>=400
}

unsafe fn handle_virtual_drag(st:&mut FilterState,hwnd:HWND,
                               msg:UINT,wp:WPARAM,lp:LPARAM)->bool{
    if !st.drag_capture_active {
        if msg!=WM_LBUTTONDOWN {return false;}
        let p=st.mapping_ptr as *mut protocol::FilterPolicy;
        increment(ptr::addr_of_mut!((*p).drag_down_seen));
        (*p).drag_selection_count=0;
        (*p).drag_selection_indices=[0u32;protocol::MAX_DRAG_SELECTION];
        if !st.verified || (st.flags & protocol::FLAG_DRAG_CAPTURE)==0 ||
            !policy_is_live(st) {
            (*p).drag_down_reject_reason=1;
            return false;
        }
        if !GetCapture().is_null(){
            (*p).drag_down_reject_reason=2;
            return false;
        }
        let requested=(*p).drag_source_index as usize;
        if requested>=st.observed_count ||
            is_hidden(st,requested,protocol::FLAG_PAINT_HIDE) {
            (*p).drag_down_reject_reason=3;
            return false;
        }
        let mut at=POINT{x:lp as i16 as i32,y:(lp>>16) as i16 as i32};
        (*p).drag_down_client_x=at.x;
        (*p).drag_down_client_y=at.y;
        let mut hit=LVHitTest{x:at.x,y:at.y,flags:0,item:-1,subitem:0,group:0};
        let found=SendMessageW(hwnd,LVM_HITTEST,0,&mut hit as *mut LVHitTest as LPARAM);
        (*p).drag_hit_result=found as i32;
        (*p).drag_hit_item=hit.item;
        (*p).drag_hit_flags=hit.flags;
        if found != requested as isize || hit.item!=requested as i32{
            (*p).drag_down_reject_reason=4;
            // For the one allowlisted debug source, never let a mismatched
            // native hit fall through to Explorer's icon-drag handler.
            // Cancel the trial without taking capture or moving a file.
            let dx=(i64::from(at.x)-i64::from((*p).hook_diag_hover_client_x)).abs();
            let dy=(i64::from(at.y)-i64::from((*p).hook_diag_hover_client_y)).abs();
            if dx<=48 && dy<=48 {
                (*p).drag_cancelled_count=(*p).drag_cancelled_count.saturating_add(1);
                counter(ptr::addr_of_mut!((*p).drag_phase)).store(3,Ordering::Release);
                return true;
            }
            return false;
        }
        // Read actual Explorer ListView selection before taking ownership
        // of this MouseDown. A click on an unselected icon starts a new
        // single-item drag. Clicking an already selected icon preserves the
        // native Ctrl/marquee selection, bounded to 8. Never invent icons.
        let requested_is_selected=(SendMessageW(
            hwnd,LVM_GETITEMSTATE,requested as WPARAM,LVIS_SELECTED as LPARAM)
            as u32 & LVIS_SELECTED)!=0;
        let mut selected=[0u32;protocol::MAX_DRAG_SELECTION];
        let mut selected_count=0usize;
        if requested_is_selected {
            for index in 0..st.observed_count {
                if SendMessageW(hwnd,LVM_GETITEMSTATE,
                    index as WPARAM,LVIS_SELECTED as LPARAM) as u32 & LVIS_SELECTED!=0 {
                    if selected_count>=protocol::MAX_DRAG_SELECTION {
                        (*p).drag_down_reject_reason=7;
                        // Reject this developer gesture without handing a
                        // selected multi-item drag to the normal Explorer
                        // file-move handler.
                        return true;
                    }
                    selected[selected_count]=index as u32;
                    selected_count+=1;
                }
            }
            if selected_count==0 ||
                !selected[..selected_count].contains(&(requested as u32)) {
                (*p).drag_down_reject_reason=8;
                return true;
            }
        }else{
            selected[0]=requested as u32;
            selected_count=1;
        }
        if ClientToScreen(hwnd,&mut at)==0 {
            (*p).drag_down_reject_reason=5;
            return false;
        }
        // SetCapture returns the PREVIOUS capture, not a success indicator.
        SetCapture(hwnd);
        if GetCapture()!=hwnd {
            (*p).drag_down_reject_reason=6;
            return false;
        }
        increment(ptr::addr_of_mut!((*p).drag_down_accepted));
        (*p).drag_down_reject_reason=0;
        (*p).drag_cancel_reason=0;
        (*p).drag_selection_count=selected_count as u32;
        (*p).drag_selection_indices=selected;
        st.drag_capture_active=true;
        st.drag_capture_index=requested;
        (*p).drag_begin_x=at.x;
        (*p).drag_begin_y=at.y;
        (*p).drag_end_x=at.x;
        (*p).drag_end_y=at.y;
        (*p).drag_mouse_moves=0;
        counter(ptr::addr_of_mut!((*p).drag_phase)).store(1,Ordering::Release);
        return true;
    }
    if matches!(msg,WM_CANCELMODE|WM_CAPTURECHANGED){
        let p=st.mapping_ptr as *mut protocol::FilterPolicy;
        let button_released=GetAsyncKeyState(0x01)>=0;
        // Windows sometimes changes capture on the physical release without
        // delivering WM_LBUTTONUP to this Explorer ListView. Treat this ONLY
        // in the explicitly armed, verified development drag mode as a
        // completed gesture if the actual key is UP and native mouse travel
        // passes the same threshold as the normal WM_LBUTTONUP path. Host
        // must independently validate source/target before any virtual Pin.
        if msg==WM_CAPTURECHANGED && button_released
            && policy_is_live(st) && (st.flags & protocol::FLAG_DRAG_CAPTURE)!=0
            && virtual_drag_travel_valid((*p).drag_mouse_moves,
                ((*p).drag_begin_x,(*p).drag_begin_y),
                ((*p).drag_end_x,(*p).drag_end_y))
        {
            st.drag_capture_active=false;
            st.drag_capture_index=usize::MAX;
            (*p).drag_cancel_reason=9; // inferred successful release
            counter(ptr::addr_of_mut!((*p).drag_phase)).store(2,Ordering::Release);
            return false; // ordinary WM_CAPTURECHANGED cleanup
        }
        (*p).drag_cancel_reason=if msg==WM_CANCELMODE{1}
            else if button_released{8}else{2};
        cancel_virtual_drag(st,hwnd);
        return msg==WM_CANCELMODE;
    }
    // The lease must remain valid even if Explorer stops delivering WH_GETMESSAGE.
    if !policy_is_live(st) || (st.flags & protocol::FLAG_DRAG_CAPTURE)==0 ||
        GetCapture()!=hwnd {
        (* (st.mapping_ptr as *mut protocol::FilterPolicy)).drag_cancel_reason=
            if !policy_is_live(st) {4}else{3};
        cancel_virtual_drag(st,hwnd);
        return matches!(msg,WM_MOUSEMOVE|WM_LBUTTONUP|WM_LBUTTONDOWN);
    }
    if msg==WM_MOUSEMOVE || msg==WM_LBUTTONUP {
        let p=st.mapping_ptr as *mut protocol::FilterPolicy;
        let mut pt=POINT{x:lp as i16 as i32,y:(lp>>16) as i16 as i32};
        if ClientToScreen(hwnd,&mut pt)==0 {
            (* (st.mapping_ptr as *mut protocol::FilterPolicy)).drag_cancel_reason=5;
            cancel_virtual_drag(st,hwnd);
            return true;
        }
        (*p).drag_end_x=pt.x;
        (*p).drag_end_y=pt.y;
        if msg==WM_MOUSEMOVE && (wp & 0x0001)!=0 {
            (*p).drag_mouse_moves=(*p).drag_mouse_moves.saturating_add(1);
        }
        if msg==WM_LBUTTONUP {
            increment(ptr::addr_of_mut!((*p).drag_up_seen));
            let accepted=virtual_drag_travel_valid((*p).drag_mouse_moves,
                ((*p).drag_begin_x,(*p).drag_begin_y),
                (pt.x,pt.y));
            st.drag_capture_active=false;
            st.drag_capture_index=usize::MAX;
            if accepted {
                counter(ptr::addr_of_mut!((*p).drag_phase)).store(2,Ordering::Release);
            }else{
                (*p).drag_cancelled_count=(*p).drag_cancelled_count.saturating_add(1);
                (*p).drag_cancel_reason=7;
                counter(ptr::addr_of_mut!((*p).drag_phase)).store(3,Ordering::Release);
            }
            if GetCapture()==hwnd {ReleaseCapture();}
        }
        return true;
    }
    // A stray second down may not invoke Explorer during an active gesture.
    msg==WM_LBUTTONDOWN
}

unsafe extern "system" fn list_proc(hwnd:HWND,msg:UINT,wp:WPARAM,lp:LPARAM,
                                    _:usize,_:usize)->LRESULT {
    // Synthetic diagnostic only. A nested synchronous ListView hit-test
    // while holding the state mutex is unnecessary in Release and can
    // contribute to cross-process modal menu stalls.
    if cfg!(debug_assertions) && msg == WM_CONTEXTMENU {
        if let Ok(guard)=state().try_lock() {
            if let Some(st)=guard.as_ref() {
                let p=st.mapping_ptr as *mut protocol::FilterPolicy;
                increment(ptr::addr_of_mut!((*p).hook_diag_menu_received));
                if lp != -1 {
                    let mut at=POINT{x:lp as i16 as i32,
                        y:(lp>>16) as i16 as i32};
                    if ScreenToClient(hwnd,&mut at)!=0 {
                        (*p).hook_diag_menu_client_x=at.x;
                        (*p).hook_diag_menu_client_y=at.y;
                        let mut hit=LVHitTest{x:at.x,y:at.y,
                            flags:0,item:-1,subitem:0,group:0};
                        let index=SendMessageW(hwnd,LVM_HITTEST,0,
                            &mut hit as *mut LVHitTest as LPARAM);
                        (*p).hook_diag_menu_hit_index=index as i32;
                        (*p).hook_diag_menu_hidden_at_hit=
                            if index>=0 && st.hidden.get(index as usize)
                                .copied().unwrap_or(false){1}else{0};
                        (*p).hook_diag_menu_policy_active=
                            if (st.flags & protocol::FLAG_INPUT_HIDE)!=0
                                && policy_is_live(st){1}else{0};
                    }
                }
            }
        }
    }
    // Out-of-band emergency detach still works when the controller's hook
    // handle is gone: send this harmless registered-by-contract message to
    // the real ListView, which is subclassed inside the Explorer process.
    if msg == protocol::APP_HOOK_REMOVE && wp == protocol::HOOK_COOKIE {
        return if uninstall(){1}else{0};
    }
    if msg == protocol::APP_HOOK_HOLD_PAINT && wp == protocol::HOOK_COOKIE {
        return if hold_policy_paint(hwnd, lp as usize as u64) { lp } else { 0 };
    }
    #[cfg(feature = "native-fast-tabs")]
    if msg == protocol::APP_HOOK_MOVE_TAB && wp == protocol::HOOK_COOKIE {
        return tab_moves::apply(hwnd, lp as usize as u64) as LRESULT;
    }
    #[cfg(feature = "native-fast-tabs")]
    if msg == protocol::APP_HOOK_READ_POSITIONS && wp == protocol::HOOK_COOKIE {
        return tab_moves::read_positions(hwnd, lp as usize as u64) as LRESULT;
    }
    if matches!(msg, protocol::APP_HOOK_APPLY_POLICY | protocol::APP_HOOK_PAINT_POLICY)
        && wp == protocol::HOOK_COOKIE
    {
        let generation = lp as usize as u64;
        return if apply_policy(hwnd, generation, msg == protocol::APP_HOOK_PAINT_POLICY) {
            lp
        } else {
            0
        };
    }
    #[cfg(feature = "native-fast-tabs")]
    if msg == WM_LBUTTONDOWN {
        if let Ok(guard) = state().try_lock() {
            if let Some(st) = guard.as_ref().filter(|st| st.list == hwnd as usize) {
                let policy = st.mapping_ptr as *mut protocol::FilterPolicy;
                let tick = &*ptr::addr_of!((*policy).native_drag_tick).cast::<AtomicU64>();
                tick.store(0, Ordering::Release);
            }
        }
    }
    // After a controller crash, an expired policy must not leave a
    // permanent Explorer subclass or named mapping behind. Detach lazily on
    // ordinary native UI traffic. The absence of a valid lease means all
    // icon visibility/input checks already fail open.
    if matches!(msg,WM_MOUSEMOVE|WM_PAINT_LOCAL|WM_CONTEXTMENU|
        WM_RBUTTONDOWN|WM_LBUTTONDOWN|WM_LBUTTONUP) &&
        expired_ownerless_native_policy() && uninstall() {
        return DefSubclassProc(hwnd,msg,wp,lp);
    }
    if let Some(result) = paint_hold::handle(hwnd, msg, wp) {
        return result;
    }
    let _paint_scope = (msg == WM_PAINT_LOCAL || msg == WM_PRINTCLIENT_LOCAL)
        .then(PaintScope::enter);
    if matches!(msg,protocol::APP_HOOK_INPUT_INSPECT |
        protocol::APP_HOOK_INPUT_HOVER | protocol::APP_HOOK_INPUT_RIGHTCLICK)
        && wp == protocol::HOOK_COOKIE {
        // SendMessageTimeoutW bypasses WH_GETMESSAGE. Read the newly
        // published generation here before doing the in-process probe.
        refresh();
        let Ok(guard)=state().try_lock() else {return 0};
        let Some(st)=guard.as_ref() else {return 0};
        let mode=if msg==protocol::APP_HOOK_INPUT_HOVER {1}
            else if msg==protocol::APP_HOOK_INPUT_RIGHTCLICK {2}else{0};
        return if inspect_native_hit(st,hwnd,lp as usize,mode) {WM_INPUT_DIAG_OK}else{0};
    }
    if matches!(msg,WM_LBUTTONDOWN|WM_MOUSEMOVE|WM_LBUTTONUP|
        WM_CANCELMODE|WM_CAPTURECHANGED) {
        let Ok(mut guard)=state().try_lock() else {
            return DefSubclassProc(hwnd,msg,wp,lp);
        };
        if let Some(st)=guard.as_mut() {
            if handle_virtual_drag(st,hwnd,msg,wp,lp) {
                // The bounded native drag belongs to DesktopCraft's test
                // transaction; Explorer must never receive this drag.
                return 0;
            }
        }
    }
    if msg == WM_PAINT_LOCAL || msg == WM_PRINTCLIENT_LOCAL {
        if let Ok(guard)=state().try_lock() {
            if let Some(st)=guard.as_ref() {
                let p=st.mapping_ptr as *mut protocol::FilterPolicy;
                if msg == WM_PAINT_LOCAL {
                    increment(ptr::addr_of_mut!((*p).hook_listview_paint));
                } else {
                    increment(ptr::addr_of_mut!((*p).hook_listview_printclient));
                }
            }
        }
    }
    if matches!(msg,WM_LBUTTONDOWN|WM_LBUTTONDBLCLK|WM_LBUTTONUP|
                WM_RBUTTONDOWN|WM_RBUTTONDBLCLK|WM_RBUTTONUP|WM_CONTEXTMENU|WM_MOUSEMOVE) {
        // Keep marquee/drag in Explorer's possession. Only suppress ordinary
        // hover and click on hidden items, not movement while a button is held.
        if !(msg==WM_MOUSEMOVE && wp != 0)
            && !(msg==WM_CONTEXTMENU && lp == -1) {
            let mut at=POINT{x:lp as i16 as i32,y:(lp>>16) as i16 as i32};
            if msg==WM_CONTEXTMENU {ScreenToClient(hwnd,&mut at);}
            // Query the native ListView BEFORE acquiring the filter mutex;
            // Windows Common Controls can synchronously re-enter during a
            // context menu, hover or mouse click.
            let hit=native_input_hit_index(hwnd,at.x,at.y);
            let Ok(guard)=state().try_lock() else {return DefSubclassProc(hwnd,msg,wp,lp)};
            if let Some(st)=guard.as_ref() {
                if blocks_hit_index(st,hit) {
                    let p=st.mapping_ptr as *mut protocol::FilterPolicy;
                    increment(ptr::addr_of_mut!((*p).hook_input_suppressed));
                    if at.x==(*p).hook_diag_hover_client_x &&
                        at.y==(*p).hook_diag_hover_client_y {
                        if msg==WM_MOUSEMOVE && (*p).hook_diag_hover_posted>0 {
                            increment(ptr::addr_of_mut!((*p).hook_diag_hover_suppressed));
                        }
                        if matches!(msg,WM_RBUTTONDOWN|WM_RBUTTONUP) &&
                            (*p).hook_diag_right_posted>0 {
                            increment(ptr::addr_of_mut!((*p).hook_diag_right_suppressed));
                        }
                        if msg==WM_CONTEXTMENU && (*p).hook_diag_menu_posted>0 {
                            increment(ptr::addr_of_mut!((*p).hook_diag_menu_suppressed));
                        }
                    }
                    return 0;
                }
            }
        }
    }
    DefSubclassProc(hwnd,msg,wp,lp)
}

/// Windows thread-specific message-hook procedure. The controller must use
/// WH_GETMESSAGE for the actual Explorer desktop LISTVIEW thread and post
/// APP_HOOK_INSTALL/APP_HOOK_REMOVE with the policy's cookie.
#[no_mangle]
pub unsafe extern "system" fn DesktopCraftFilterHook(code:i32,wp:WPARAM,lp:LPARAM)->LRESULT {
    if code>=0 && lp != 0 {
        let message=&mut *(lp as *mut MSG);
        if message.wParam == protocol::HOOK_COOKIE {
            match message.message {
                protocol::APP_HOOK_INSTALL => {
                    install(message.hwnd);
                    message.message=WM_NULL;
                }
                protocol::APP_HOOK_REMOVE => {
                    uninstall();
                    message.message=WM_NULL;
                }
                _ => {}
            }
        }
        refresh();
    }
    CallNextHookEx(ptr::null_mut(),code,wp,lp)
}

/// Tiny static diagnostics independent of Explorer lifecycle.
#[no_mangle]
pub extern "system" fn DesktopCraftFilterAbiVersion()->u32 { protocol::ABI_VERSION }

/// Separate from the shared-memory ABI, which is unchanged. An older
/// DesktopCraft still sees the same policy mapping and cannot attach twice.
#[no_mangle]
pub extern "system" fn DesktopCraftFilterPaintAckVersion() -> u32 { protocol::PAINT_ACK_VERSION }


#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn only_newly_selected_or_focused_state_triggers_native_veto() {
        assert!(newly_selected_or_focused(LVIS_SELECTED, 0));
        assert!(newly_selected_or_focused(LVIS_FOCUSED, 0));
        assert!(newly_selected_or_focused(FILTERED_NATIVE_STATE, LVIS_SELECTED));
        assert!(!newly_selected_or_focused(LVIS_SELECTED, LVIS_SELECTED));
        assert!(!newly_selected_or_focused(0, LVIS_SELECTED));
        assert!(!newly_selected_or_focused(0, 0));
    }

    #[test]
    fn inferred_mouseup_requires_three_moves_and_twenty_pixels() {
        assert!(!virtual_drag_travel_valid(2,(0,0),(100,0)));
        assert!(!virtual_drag_travel_valid(3,(0,0),(19,0)));
        assert!(virtual_drag_travel_valid(3,(0,0),(20,0)));
        assert!(virtual_drag_travel_valid(29,(2226,269),(620,210)));
        assert!(virtual_drag_travel_valid(4,(20,20),(-30,-31)));
        assert!(!virtual_drag_travel_valid(40,(0,0),(0,0)));
    }

    #[test]
    fn in_process_diagnostic_geometry_rejects_invalid_or_overflowing_rectangles() {
        assert_eq!(safe_rect_center(&Rect{left:10,top:20,right:50,bottom:60}),Some((30,40)));
        assert_eq!(safe_rect_center(&Rect{left:100,top:20,right:20,bottom:40}),None);
        assert_eq!(safe_rect_center(&Rect{left:0,top:0,right:100,bottom:0}),None);
        assert_eq!(safe_rect_center(&Rect{left:i32::MIN,top:i32::MIN,right:i32::MAX,bottom:i32::MAX}),Some((0,0)));
    }
    #[test]
    fn transient_dispinfo_does_not_destroy_native_cached_item_metadata() {
        let mut original=[65u16,66,0];
        let mut item:LVItem=unsafe{mem::zeroed()};
        item.mask=LVIF_TEXT | LVIF_IMAGE | LVIF_DI_SETITEM;
        item.text=original.as_mut_ptr();
        item.max_chars=3;
        item.image=47;
        assert!(suppress_requested_display_info(&mut item));
        assert_eq!(item.mask,LVIF_TEXT|LVIF_IMAGE);
        assert_eq!(item.image,-1);
        assert_eq!(item.max_chars,1);
        assert_eq!(original[0],65);
        assert_eq!(unsafe{*item.text},0);
    }

    #[test]
    fn header_validation_accepts_dynamic_desktop_item_count_transitions_and_rejects_mismatch() {
        assert!(protocol::valid_header(
            protocol::MAGIC, protocol::ABI_VERSION, 62, 62, 100, 100, 200, 200, 1000, 1500, 4,
        ));
        assert!(protocol::valid_header(
            protocol::MAGIC, protocol::ABI_VERSION, 60, 60, 100, 100, 200, 200, 1000, 1500, 6,
        ));
        assert!(!protocol::valid_header(
            protocol::MAGIC, protocol::ABI_VERSION, 61, 62, 100, 100, 200, 200, 1000, 1500, 4,
        ));
        assert!(!protocol::valid_header(
            protocol::MAGIC, protocol::ABI_VERSION, 61, 60, 100, 100, 200, 200, 1000, 1500, 4,
        ));
    }

    #[test]
    fn count_transition_keeps_label_masking_bounded_and_fails_open() {
        let valid = |policy_cnt, live_cnt, lease, now, gen_id, exp_gen, since| {
            count_transition_header_valid(
                protocol::MAGIC,
                protocol::ABI_VERSION,
                policy_cnt,
                live_cnt,
                100,
                100,
                200,
                200,
                lease,
                now,
                gen_id,
                exp_gen,
                since,
            )
        };
        assert!(valid(63, 64, 1000, 1500, 4, 4, 1400));
        assert!(valid(63, 62, 1000, 3400, 4, 4, 1400));
        assert!(!valid(63, 63, 1000, 1500, 4, 4, 1400));
        assert!(!valid(63, 64, 1000, 3401, 4, 4, 1400));
        assert!(!valid(63, 64, 1000, 1000 + protocol::HEARTBEAT_TIMEOUT_MS + 1, 4, 4, 5900));
        assert!(!valid(63, 64, 1000, 1500, 6, 4, 1400));
        assert!(!valid(63, 64, 1000, 1500, 5, 5, 1400));
        assert!(!valid(63, 0, 1000, 1500, 4, 4, 1400));
        assert!(!valid(63, (protocol::MAX_ITEMS + 1) as u32, 1000, 1500, 4, 4, 1400));
    }

    #[test]
    fn paint_and_label_scopes_restore_state_on_drop() {
        assert_eq!(IN_LIST_PAINT.with(Cell::get), 0);
        {
            let _outer = PaintScope::enter();
            assert_eq!(IN_LIST_PAINT.with(Cell::get), 1);
            {
                let _inner = PaintScope::enter();
                assert_eq!(IN_LIST_PAINT.with(Cell::get), 2);
            }
            assert_eq!(IN_LIST_PAINT.with(Cell::get), 1);
        }
        assert_eq!(IN_LIST_PAINT.with(Cell::get), 0);

        assert!(!IN_LABEL_LOOKUP.with(Cell::get));
        {
            let _first = LabelLookupScope::enter();
            assert!(IN_LABEL_LOOKUP.with(Cell::get));
            {
                let _second = LabelLookupScope::enter();
                assert!(IN_LABEL_LOOKUP.with(Cell::get));
            }
            assert!(IN_LABEL_LOOKUP.with(Cell::get));
        }
        assert!(!IN_LABEL_LOOKUP.with(Cell::get));
    }
}
