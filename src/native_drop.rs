//! Windows Shell Drag & Drop -> DesktopCraft virtual Pin membership.
//!
//! This target implements the OLE IDropTarget interface. The older
//! disconnected DropZone queues virtual pin updates (no files moved). A
//! separate opt-in existing-HWND lab persists a validated virtual Pin BEFORE
//! returning DROPEFFECT_LINK, then hands the result to the UI on its next
//! frame. Normal DesktopCraft never registers this experimental receiver.
#![allow(dead_code)]
use crate::model::State;
use crate::native_ole_gate::NativeOleViewport;
use crate::native_ole_persist::NativeOlePersistedSession;
use crate::shell::ViewItem;
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use winapi::shared::windef::HWND;
use winapi::um::winuser::{
    CreateWindowExW, DestroyWindow, MoveWindow, ShowWindow, SW_HIDE, SW_SHOW, WS_BORDER, WS_CHILD,
    WS_VISIBLE,
};

const EFFECT_LINK: u32 = 4;
const EFFECT_COPY: u32 = 1;
/// OLE advertises the effects allowed by the drag *source* on every call.
/// Never return LINK unless the source offered it, even after DragEnter.
fn negotiated_link(accepted: bool, offered: u32) -> u32 {
    if accepted && (offered & EFFECT_LINK) != 0 {
        EFFECT_LINK
    } else {
        0
    }
}
const E_NOINTERFACE: i32 = 0x80004002u32 as i32;
const E_POINTER: i32 = 0x80004003u32 as i32;
const FORMAT_HDROP: u16 = 15;

#[repr(C)]
#[derive(PartialEq, Eq)]
struct Guid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}
const IID_UNKNOWN: Guid = Guid {
    a: 0,
    b: 0,
    c: 0,
    d: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_DROP_TARGET: Guid = Guid {
    a: 0x00000122,
    b: 0,
    c: 0,
    d: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
#[repr(C)]
#[derive(Clone, Copy)]
struct PointL {
    x: i32,
    y: i32,
}
#[repr(C)]
struct FormatEtc {
    format: u16,
    ptd: *mut c_void,
    aspect: u32,
    index: i32,
    medium: u32,
}
#[repr(C)]
struct StgMedium {
    tymed: u32,
    handle: *mut c_void,
    unknown: *mut c_void,
}
#[repr(C)]
struct DataObject {
    vtbl: *const DataObjectVtbl,
}
#[repr(C)]
struct DataObjectVtbl {
    query_interface: usize,
    add_ref: usize,
    release: usize,
    get_data: unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium) -> i32,
}

/// Experimental OLE receiver with a disposable Workspace snapshot. The
/// isolated Macroquad main-HWND lab synchronizes accepted results back into
/// the real App.state before the next frame handles input or autosave.
/// It never registers against a user's production desktop/Workspace.
pub struct NativeOlePersistedLabContext {
    pub workspace: PathBuf,
    pub state: State,
    pub explorer: Vec<ViewItem>,
    pub monitor_signature: String,
    pub viewport: NativeOleViewport,
    pub visible_box_ids: Vec<u64>,
    pub max_groups: usize,
    pub max_items_per_tab: usize,
    session: NativeOlePersistedSession,
    offered_paths: Vec<PathBuf>,
    pub successful_drops: u32,
    pub last_error: Option<String>,
    /// Captured pre-Drop UI State awaiting application on the next normal
    /// DesktopCraft frame. Until it is consumed, new drags fail closed.
    pending_ui_before: Option<Vec<u8>>,
}
impl NativeOlePersistedLabContext {
    pub fn new(
        workspace: PathBuf,
        state: State,
        explorer: Vec<ViewItem>,
        monitor_signature: String,
        viewport: NativeOleViewport,
        visible_box_ids: Vec<u64>,
        max_groups: usize,
        max_items_per_tab: usize,
    ) -> Self {
        Self {
            workspace,
            state,
            explorer,
            monitor_signature,
            viewport,
            visible_box_ids,
            max_groups,
            max_items_per_tab,
            session: NativeOlePersistedSession::default(),
            offered_paths: Vec::new(),
            successful_drops: 0,
            last_error: None,
            pending_ui_before: None,
        }
    }

    /// Only called on DesktopCraft's real UI thread, before processing input,
    /// host ticks or autosave. This is the guarded handoff from a COM Drop
    /// that persisted on the same event-loop thread into App.state.
    /// Conflicts do not discard the handoff or restore any user file.
    pub fn synchronize_main_thread(
        &mut self,
        live: &mut State,
        viewport: NativeOleViewport,
        monitor_signature: &str,
    ) -> Result<bool,String> {
        if let Some(expected)=&self.pending_ui_before {
            let actual=serde_json::to_vec(live).map_err(|e|e.to_string())?;
            if &actual!=expected {
                return Err("OLE committed but UI State diverged; freeze normal saves and review".into());
            }
            let saved=State::load(&self.workspace)?;
            if serde_json::to_vec(&saved).map_err(|e|e.to_string())?
                !=serde_json::to_vec(&self.state).map_err(|e|e.to_string())? {
                return Err("OLE persisted Workspace diverged before UI acknowledgment".into());
            }
            *live=self.state.clone();
            self.pending_ui_before.take();
            self.viewport=viewport;
            self.monitor_signature=monitor_signature.to_string();
            return Ok(true);
        }
        let live_bytes=serde_json::to_vec(live).map_err(|e|e.to_string())?;
        let my_bytes=serde_json::to_vec(&self.state).map_err(|e|e.to_string())?;
        if live_bytes!=my_bytes {
            // A real UI operation changed Box/Tab membership during a drag.
            // Cancel the old gesture: it cannot commit on a stale Box snapshot.
            self.reset();
            self.state=live.clone();
        }
        if self.viewport!=viewport || self.monitor_signature!=monitor_signature {
            // DPI change invalidates the gesture's hit coordinates.
            self.reset();
            self.viewport=viewport;
            self.monitor_signature=monitor_signature.to_string();
        }
        Ok(false)
    }

    /// A new OLE gesture may not begin until the committed state has been
    /// applied to the UI; useful for host lifecycle and test assertions.
    pub fn ui_handoff_pending(&self)->bool {
        self.pending_ui_before.is_some()
    }


    /// The existing-HWND laboratory receiver must never use an obsolete
    /// cached screen rectangle if a resize/move/DPI transition happens
    /// between UI frames and synchronous COM callbacks.
    fn verified_hwnd_geometry(&self, hwnd:HWND)->bool {
        if hwnd.is_null(){return true;} // older hidden HWND COM-only tests
        let logical=self.viewport.logical_size;
        measured_main_hwnd_viewport(hwnd,logical.0,logical.1)
            .is_ok_and(|current| current==self.viewport)
    }

    fn reset(&mut self) {
        self.session.drag_leave();
        self.offered_paths.clear();
    }
    fn enter(&mut self, paths: Option<Vec<PathBuf>>, screen: (i32, i32), effect: u32) -> u32 {
        if self.pending_ui_before.is_some() {
            self.last_error = Some("Previous OLE Drop not synchronized with DesktopCraft UI".into());
            return 0;
        }
        self.reset();
        let Some(paths) = paths else { return 0 };
        if effect & EFFECT_LINK == 0 {
            return 0;
        }
        let result = self.session.drag_enter(
            &self.workspace,
            &self.state,
            &self.explorer,
            &self.monitor_signature,
            self.viewport,
            &paths,
            &self.visible_box_ids,
            self.max_groups,
            self.max_items_per_tab,
        );
        match result {
            Ok(()) => {
                self.offered_paths = paths;
                self.over(screen, effect)
            }
            Err(e) => {
                self.last_error = Some(e);
                0
            }
        }
    }
    fn over(&self, screen: (i32, i32), effect: u32) -> u32 {
        self.session.drag_over_effect(
            &self.state,
            &self.explorer,
            &self.monitor_signature,
            self.viewport,
            &self.visible_box_ids,
            &self.offered_paths,
            screen,
            effect,
        )
    }
    fn release(&mut self, paths: Option<Vec<PathBuf>>, screen: (i32, i32), effect: u32) -> u32 {
        if self.pending_ui_before.is_some() {
            self.last_error = Some("OLE UI handoff still pending; Drop denied".into());
            return 0;
        }
        let before = match serde_json::to_vec(&self.state) {
            Ok(bytes)=>bytes,
            Err(e)=>{self.last_error=Some(e.to_string());self.reset();return 0;}
        };
        let paths = paths.unwrap_or_default();
        let result = self.session.drop(
            &mut self.state,
            &self.explorer,
            &self.monitor_signature,
            self.viewport,
            &self.visible_box_ids,
            &paths,
            screen,
            effect,
        );
        self.offered_paths.clear();
        match result {
            Ok(_) => {
                self.pending_ui_before = Some(before);
                self.successful_drops += 1;
                self.last_error = None;
                EFFECT_LINK
            }
            Err(e) => {
                self.last_error = Some(e);
                0
            }
        }
    }
}

#[repr(C)]
struct DropTarget {
    vtbl: *const DropTargetVtbl,
    refs: AtomicU32,
    accepted: AtomicBool,
    /// Freeze the source identity from DragEnter through Drop: reject a
    /// replacement CF_HDROP, even when both sets point to existing files.
    offered_paths: Mutex<Option<Vec<PathBuf>>>,
    group: u64,
    queue: Arc<Mutex<Vec<PinDrop>>>,
    persisted_lab: Option<Arc<Mutex<NativeOlePersistedLabContext>>>,
    window_drop: Option<Arc<Mutex<WindowDropState>>>,
    /// Non-null only for the isolated Miniquad MAIN HWND receiver: callbacks
    /// independently re-measure physical geometry before accepting any Drop.
    verified_main_hwnd: HWND,
}
#[repr(C)]
struct DropTargetVtbl {
    query_interface:
        unsafe extern "system" fn(*mut DropTarget, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut DropTarget) -> u32,
    release: unsafe extern "system" fn(*mut DropTarget) -> u32,
    drag_enter:
        unsafe extern "system" fn(*mut DropTarget, *mut DataObject, u32, PointL, *mut u32) -> i32,
    drag_over: unsafe extern "system" fn(*mut DropTarget, u32, PointL, *mut u32) -> i32,
    drag_leave: unsafe extern "system" fn(*mut DropTarget) -> i32,
    drop: unsafe extern "system" fn(*mut DropTarget, *mut DataObject, u32, PointL, *mut u32) -> i32,
}
#[derive(Debug)]
pub struct PinDrop {
    pub group: u64,
    pub paths: Vec<PathBuf>,
}
pub type Queue = Arc<Mutex<Vec<PinDrop>>>;

#[derive(Clone, Copy)]
pub(crate) enum WindowDropRoute {
    Tab(u64, u64),
    Folder,
}

pub(crate) struct WindowDrop {
    pub route: WindowDropRoute,
    pub screen: (i32, i32),
    pub paths: Vec<PathBuf>,
}

#[derive(Default)]
struct WindowDropState {
    targets: Vec<([i32; 4], WindowDropRoute)>,
    pending: Option<WindowDrop>,
}

impl WindowDropState {
    fn target(&self, point: PointL, offered: u32) -> Option<(WindowDropRoute, u32)> {
        if self.pending.is_some() {
            return None;
        }
        let (_, route) = self.targets.iter().find(|([x, y, w, h], _)| {
            point.x >= *x && point.x < x.saturating_add(*w)
                && point.y >= *y && point.y < y.saturating_add(*h)
        })?;
        let effect = match route {
            WindowDropRoute::Tab(_, _) => EFFECT_LINK,
            WindowDropRoute::Folder => EFFECT_COPY,
        };
        (offered & effect != 0).then_some((*route, effect))
    }
}

#[link(name = "ole32")]
extern "system" {
    fn RegisterDragDrop(hwnd: HWND, target: *mut c_void) -> i32;
    fn RevokeDragDrop(hwnd: HWND) -> i32;
    fn ReleaseStgMedium(medium: *mut StgMedium);
}
#[link(name = "shell32")]
extern "system" {
    fn DragQueryFileW(drop: *mut c_void, index: u32, buffer: *mut u16, len: u32) -> u32;
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn paths(data: *mut DataObject) -> Option<Vec<PathBuf>> {
    paths_with_limit(data, 64)
}

unsafe fn paths_with_limit(data: *mut DataObject, limit: u32) -> Option<Vec<PathBuf>> {
    if data.is_null() {
        return None;
    }
    let fmt = FormatEtc {
        format: FORMAT_HDROP,
        ptd: ptr::null_mut(),
        aspect: 1,
        index: -1,
        medium: 1,
    };
    let mut medium: StgMedium = std::mem::zeroed();
    let hr = ((*(*data).vtbl).get_data)(data, &fmt, &mut medium);
    if hr < 0 {
        return None;
    }
    let mut found = Vec::new();
    let mut complete = false;
    // IDataObject may be untrusted: only TYMED_HGLOBAL is a DROPFILES handle.
    // Never pass an IStream/IStorage pointer to DragQueryFileW.
    if medium.tymed == 1 && !medium.handle.is_null() {
        let count = DragQueryFileW(medium.handle, u32::MAX, ptr::null_mut(), 0);
        if (1..=limit).contains(&count) {
            complete = true;
            for i in 0..count {
                let size = DragQueryFileW(medium.handle, i, ptr::null_mut(), 0);
                if size == 0 || size > 32767 {
                    complete = false;
                    break;
                }
                let mut w = vec![0u16; size as usize + 1];
                let n = DragQueryFileW(medium.handle, i, w.as_mut_ptr(), w.len() as u32);
                if n != size {
                    complete = false;
                    break;
                }
                let path = PathBuf::from(String::from_utf16_lossy(&w[..n as usize]));
                if !path.exists() {
                    complete = false;
                    break;
                }
                found.push(path);
            }
            complete &= found.len() == count as usize;
        }
    }
    ReleaseStgMedium(&mut medium);
    complete.then_some(found)
}
unsafe extern "system" fn qi(
    this: *mut DropTarget,
    iid: *const Guid,
    result: *mut *mut c_void,
) -> i32 {
    if iid.is_null() || result.is_null() {
        return E_POINTER;
    }
    *result = ptr::null_mut();
    if *iid == IID_UNKNOWN || *iid == IID_DROP_TARGET {
        *result = this.cast();
        add_ref(this);
        return 0;
    }
    E_NOINTERFACE
}
unsafe extern "system" fn add_ref(this: *mut DropTarget) -> u32 {
    (*this).refs.fetch_add(1, Ordering::AcqRel) + 1
}
unsafe extern "system" fn release(this: *mut DropTarget) -> u32 {
    let refs = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
    if refs == 0 {
        drop(Box::from_raw(this));
    }
    refs
}
unsafe extern "system" fn drag_enter(
    this: *mut DropTarget,
    data: *mut DataObject,
    _keys: u32,
    _point: PointL,
    effect: *mut u32,
) -> i32 {
    if effect.is_null() {
        return E_POINTER;
    }
    if let Some(ctx) = (*this).persisted_lab.as_ref() {
        *effect = ctx.lock().ok().map_or(0, |mut lab| {
            if !lab.verified_hwnd_geometry((*this).verified_main_hwnd) {
                lab.reset();
                return 0;
            }
            lab.enter(paths(data), (_point.x, _point.y), *effect)
        });
        return 0;
    }
    if let Some(ctx) = (*this).window_drop.as_ref() {
        let candidate = if *effect & (EFFECT_LINK | EFFECT_COPY) != 0 {
            paths_with_limit(data, crate::native_ole_persist::MAX_NATIVE_PIN_PATHS as u32)
        } else {
            None
        };
        let valid = candidate.is_some();
        if let Ok(mut offered) = (*this).offered_paths.lock() {
            *offered = candidate;
        }
        (*this).accepted.store(valid, Ordering::Release);
        *effect = if valid {
            ctx.lock().ok().and_then(|state| state.target(_point, *effect)).map_or(0, |(_, effect)| effect)
        } else { 0 };
        return 0;
    }
    let allows_link = (*effect & EFFECT_LINK) != 0;
    let candidate = if allows_link { paths(data) } else { None };
    let accept = if let Ok(mut offered) = (*this).offered_paths.lock() {
        let valid = candidate.is_some();
        *offered = candidate;
        valid
    } else {
        false
    };
    (*this).accepted.store(accept, Ordering::Release);
    *effect = negotiated_link(accept, *effect);
    0
}
unsafe extern "system" fn drag_over(
    this: *mut DropTarget,
    _keys: u32,
    _point: PointL,
    effect: *mut u32,
) -> i32 {
    if effect.is_null() {
        return E_POINTER;
    }
    if let Some(ctx) = (*this).persisted_lab.as_ref() {
        *effect = ctx
            .lock()
            .ok()
            .map_or(0, |mut lab| {
                if !lab.verified_hwnd_geometry((*this).verified_main_hwnd) {
                    lab.reset();
                    return 0;
                }
                lab.over((_point.x, _point.y), *effect)
            });
        return 0;
    }
    if let Some(ctx) = (*this).window_drop.as_ref() {
        *effect = if (*this).accepted.load(Ordering::Acquire) {
            ctx.lock().ok().and_then(|state| state.target(_point, *effect)).map_or(0, |(_, effect)| effect)
        } else { 0 };
        return 0;
    }
    *effect = negotiated_link((*this).accepted.load(Ordering::Acquire), *effect);
    0
}
unsafe extern "system" fn drag_leave(this: *mut DropTarget) -> i32 {
    if let Some(ctx) = (*this).persisted_lab.as_ref() {
        if let Ok(mut lab) = ctx.lock() {
            lab.reset();
        }
        return 0;
    }
    (*this).accepted.store(false, Ordering::Release);
    if let Ok(mut offered) = (*this).offered_paths.lock() {
        *offered = None;
    }
    0
}
unsafe extern "system" fn drop_items(
    this: *mut DropTarget,
    data: *mut DataObject,
    _keys: u32,
    _point: PointL,
    effect: *mut u32,
) -> i32 {
    if effect.is_null() {
        return E_POINTER;
    }
    if let Some(ctx) = (*this).persisted_lab.as_ref() {
        *effect = ctx.lock().ok().map_or(0, |mut lab| {
            if !lab.verified_hwnd_geometry((*this).verified_main_hwnd) {
                lab.reset();
                return 0;
            }
            lab.release(paths(data), (_point.x, _point.y), *effect)
        });
        return 0;
    }
    let accept = (*this).accepted.swap(false, Ordering::AcqRel);
    let offered = (*this)
        .offered_paths
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(ctx) = (*this).window_drop.as_ref() {
        let allowed = *effect;
        *effect = 0;
        if !accept || allowed & (EFFECT_LINK | EFFECT_COPY) == 0 {
            return 0;
        }
        let Some(paths) = paths_with_limit(data, crate::native_ole_persist::MAX_NATIVE_PIN_PATHS as u32) else { return 0; };
        if offered.as_ref() != Some(&paths) {
            return 0;
        }
        if let Ok(mut state) = ctx.lock() {
            if let Some((route, effect_value)) = state.target(_point, allowed) {
                state.pending = Some(WindowDrop { route, screen: (_point.x, _point.y), paths });
                *effect = effect_value;
            }
        }
        return 0;
    }
    if negotiated_link(accept, *effect) == 0 {
        *effect = 0;
        return 0;
    }
    if let (Some(offered), Some(paths)) = (offered, paths(data)) {
        if offered != paths {
            *effect = 0;
            return 0;
        }
        if let Ok(mut queue) = (*this).queue.lock() {
            queue.push(PinDrop {
                group: (*this).group,
                paths,
            });
            *effect = EFFECT_LINK;
            return 0;
        }
    }
    *effect = 0;
    0
}
static VTABLE: DropTargetVtbl = DropTargetVtbl {
    query_interface: qi,
    add_ref,
    release,
    drag_enter,
    drag_over,
    drag_leave,
    drop: drop_items,
};


/// Real UI HWND geometry, measured without changing the window, DPI setting,
/// monitor arrangement or drawing surface. This does NOT install the OLE
/// receiver; only the caller's explicit isolated-lab branch can do so.
pub fn measured_main_hwnd_viewport(
    hwnd: HWND, logical_width:f64, logical_height:f64
)->Result<NativeOleViewport,String>{
    #[repr(C)]
    struct Rect { left:i32, top:i32, right:i32, bottom:i32 }
    #[repr(C)]
    struct Point { x:i32, y:i32 }
    #[link(name="user32")]
    extern "system" {
        fn GetClientRect(hwnd:HWND,rect:*mut Rect)->i32;
        fn ClientToScreen(hwnd:HWND,point:*mut Point)->i32;
        fn GetWindowThreadProcessId(hwnd:HWND,pid:*mut u32)->u32;
        fn GetClassNameW(hwnd:HWND,name:*mut u16,max_count:i32)->i32;
    }
    #[link(name="kernel32")]
    extern "system" {
        fn GetCurrentThreadId()->u32;
    }
    if hwnd.is_null(){return Err("Main HWND cannot be null".into());}
    let mut pid=0u32;
    let owner=unsafe{GetWindowThreadProcessId(hwnd,&mut pid)};
    if pid!=std::process::id() || owner==0 ||
        owner!=unsafe{GetCurrentThreadId()} {
        return Err("OLE main HWND is not owned by the current UI thread".into());
    }
    let mut class=[0u16;64];
    let name_len=unsafe{GetClassNameW(hwnd,class.as_mut_ptr(),class.len() as i32)};
    if name_len<=0 || &String::from_utf16_lossy(&class[..name_len as usize])!="MINIQUADAPP"{
        return Err("OLE receiver refused non-Miniquad main HWND".into());
    }
    let mut rect=Rect{left:0,top:0,right:0,bottom:0};
    let mut origin=Point{x:0,y:0};
    if unsafe{GetClientRect(hwnd,&mut rect)}==0 ||
       unsafe{ClientToScreen(hwnd,&mut origin)}==0 {
        return Err("Could not measure OLE main client region".into());
    }
    let width=rect.right.checked_sub(rect.left)
        .filter(|x|*x>0).ok_or("Invalid OLE main client width")? as u32;
    let height=rect.bottom.checked_sub(rect.top)
        .filter(|x|*x>0).ok_or("Invalid OLE main client height")? as u32;
    let view=NativeOleViewport {
        screen_origin_px:(origin.x,origin.y),
        client_size_px:(width,height),
        logical_size:(logical_width,logical_height),
    };
    // Validate DPI, finite dimensions and physical/logical aspect together.
    view.logical_at((origin.x,origin.y))?;
    Ok(view)
}

/// Lab-only OLE registration on an ALREADY EXISTING HWND.
/// Unlike the legacy DropZone, it does not create a BUTTON child or draw a
/// white rectangle. Caller must initialize OLE STA on this HWND's UI thread
/// before calling new and must keep context live until this RAII object drops.
pub struct NativeOlePersistedReceiver {
    hwnd: HWND,
    target: *mut DropTarget,
}
impl NativeOlePersistedReceiver {
    pub fn register_existing_hwnd(
        hwnd: HWND,
        context: Arc<Mutex<NativeOlePersistedLabContext>>,
    ) -> Result<Self, String> {
        if hwnd.is_null() {
            return Err("OLE receiver HWND must not be null".into());
        }
        let target = Box::into_raw(Box::new(DropTarget {
            vtbl: &VTABLE,
            refs: AtomicU32::new(1),
            accepted: AtomicBool::new(false),
            offered_paths: Mutex::new(None),
            group: 0,
            queue: Arc::new(Mutex::new(Vec::new())),
            persisted_lab: Some(context),
            window_drop: None,
            verified_main_hwnd: ptr::null_mut(),
        }));
        let hr = unsafe { RegisterDragDrop(hwnd, target.cast()) };
        if hr < 0 {
            unsafe {
                release(target);
            }
            return Err(format!(
                "Existing-HWND RegisterDragDrop failed: 0x{:08X}",
                hr as u32
            ));
        }
        Ok(Self { hwnd, target })
    }
}

impl NativeOlePersistedReceiver {
    /// Only for the explicitly isolated sample-workspace main-window lab;
    /// HWND validation and native coordinate measurements are mandatory.
    pub fn register_isolated_main_hwnd(
        hwnd: HWND, logical_width:f64, logical_height:f64,
        context:Arc<Mutex<NativeOlePersistedLabContext>>,
    )->Result<Self,String>{
        let measured=measured_main_hwnd_viewport(hwnd,logical_width,logical_height)?;
        if context.lock().map_err(|_|"OLE context poisoned")?.viewport!=measured {
            return Err("OLE main HWND viewport does not match validated lab geometry".into());
        }
        let receiver=Self::register_existing_hwnd(hwnd,context)?;
        // Set on the registering UI thread before the event loop dispatches
        // another OLE callback.
        unsafe{(*receiver.target).verified_main_hwnd=hwnd;}
        Ok(receiver)
    }
}
impl Drop for NativeOlePersistedReceiver {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.is_null() {
                RevokeDragDrop(self.hwnd);
                self.hwnd = ptr::null_mut(); // never destroy caller-owned HWND
            }
            if !self.target.is_null() {
                release(self.target);
                self.target = ptr::null_mut();
            }
        }
    }
}

/// Registers the production overlay's visible drop areas using the same
/// Shell data-object parser as the lab. Dispatch happens on the next UI frame.
pub(crate) struct NativeWindowDropReceiver {
    state: Arc<Mutex<WindowDropState>>,
    _registration: NativeOlePersistedReceiver,
}

impl NativeWindowDropReceiver {
    pub(crate) fn register(hwnd: HWND) -> Result<Self, String> {
        let mut pid = 0;
        let thread = unsafe { winapi::um::winuser::GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid != std::process::id() || thread != unsafe { winapi::um::processthreadsapi::GetCurrentThreadId() } {
            return Err("Native drop receiver requires the app's own UI window".into());
        }
        let state = Arc::new(Mutex::new(WindowDropState::default()));
        let target = Box::into_raw(Box::new(DropTarget {
            vtbl: &VTABLE,
            refs: AtomicU32::new(1),
            accepted: AtomicBool::new(false),
            offered_paths: Mutex::new(None),
            group: 0,
            queue: Arc::new(Mutex::new(Vec::new())),
            persisted_lab: None,
            window_drop: Some(state.clone()),
            verified_main_hwnd: ptr::null_mut(),
        }));
        let status = unsafe { RegisterDragDrop(hwnd, target.cast()) };
        if status < 0 {
            unsafe { release(target); }
            return Err(format!("RegisterDragDrop failed: 0x{:08X}", status as u32));
        }
        Ok(Self { state, _registration: NativeOlePersistedReceiver { hwnd, target } })
    }

    pub(crate) fn set_targets(&self, targets: Vec<([i32; 4], WindowDropRoute)>) {
        if let Ok(mut state) = self.state.lock() {
            state.targets = targets;
        }
    }

    pub(crate) fn take_drop(&self) -> Option<WindowDrop> {
        self.state.lock().ok()?.pending.take()
    }
}

pub struct DropZone {
    hwnd: HWND,
    target: *mut DropTarget,
}
impl DropZone {
    pub fn new(parent: HWND, group: u64, queue: Queue) -> Result<Self, String> {
        let class = wide("BUTTON");
        let title = wide("DROP TO PIN (no file move)");
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_BORDER,
                0,
                0,
                200,
                25,
                parent,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            if hwnd.is_null() {
                return Err("Could not create Pin Drop Target HWND".into());
            }
            let target = Box::into_raw(Box::new(DropTarget {
                vtbl: &VTABLE,
                refs: AtomicU32::new(1),
                accepted: AtomicBool::new(false),
                offered_paths: Mutex::new(None),
                group,
                queue,
                persisted_lab: None,
                window_drop: None,
                verified_main_hwnd: ptr::null_mut(),
            }));
            let hr = RegisterDragDrop(hwnd, target.cast());
            if hr < 0 {
                release(target);
                DestroyWindow(hwnd);
                return Err(format!("RegisterDragDrop 0x{:08X}", hr as u32));
            }
            Ok(Self { hwnd, target })
        }
    }
    pub fn set_rect(&self, rect: [i32; 4]) {
        let [x, y, w, h] = rect;
        unsafe {
            MoveWindow(self.hwnd, x, y, w.max(90), h.max(22), 1);
        }
    }
    pub fn set_visible(&self, yes: bool) {
        unsafe {
            ShowWindow(self.hwnd, if yes { SW_SHOW } else { SW_HIDE });
        }
    }
}
impl Drop for DropZone {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.is_null() {
                RevokeDragDrop(self.hwnd);
                DestroyWindow(self.hwnd);
                self.hwnd = ptr::null_mut();
            }
            if !self.target.is_null() {
                release(self.target);
                self.target = ptr::null_mut();
            }
        }
    }
}


#[cfg(debug_assertions)]
#[repr(C)]
struct SyntheticLabFiles {
    data: DataObject,
    filenames: Vec<PathBuf>,
}

#[cfg(debug_assertions)]
unsafe extern "system" fn synthetic_lab_get_data(
    data: *mut DataObject, fmt: *const FormatEtc, out: *mut StgMedium,
)->i32 {
    if data.is_null() || fmt.is_null() || out.is_null() {
        return E_POINTER;
    }
    if (*fmt).format!=FORMAT_HDROP || (*fmt).medium!=1 {
        return E_NOINTERFACE;
    }
    #[link(name="kernel32")]
    extern "system" {
        fn GlobalAlloc(flags:u32,size:usize)->*mut c_void;
        fn GlobalLock(handle:*mut c_void)->*mut c_void;
        fn GlobalUnlock(handle:*mut c_void)->i32;
        fn GlobalFree(handle:*mut c_void)->*mut c_void;
    }
    let files=&*(data as *const SyntheticLabFiles);
    if files.filenames.is_empty() || files.filenames.len()>8 {return E_NOINTERFACE;}
    let mut names=Vec::new();
    for path in &files.filenames {
        names.extend(path.to_string_lossy().encode_utf16());
        names.push(0); // CF_HDROP has one NUL after EACH filename
    }
    names.push(0); // and an additional NUL after the last filename
    let size=20+names.len()*2;
    let global=GlobalAlloc(2,size); // GMEM_MOVEABLE
    if global.is_null(){return E_NOINTERFACE;}
    let raw=GlobalLock(global) as *mut u8;
    if raw.is_null(){
        GlobalFree(global);
        return E_NOINTERFACE;
    }
    std::ptr::write_unaligned(raw as *mut u32,20);
    std::ptr::write_unaligned(raw.add(16) as *mut u32,1); // fWide
    std::ptr::copy_nonoverlapping(names.as_ptr(),raw.add(20) as *mut u16,names.len());
    GlobalUnlock(global);
    (*out).tymed=1;
    (*out).handle=global;
    (*out).unknown=ptr::null_mut();
    0
}

#[cfg(debug_assertions)]
static SYNTHETIC_LAB_VTABLE: DataObjectVtbl=DataObjectVtbl{
    query_interface:0,add_ref:0,release:0,get_data:synthetic_lab_get_data,
};

#[cfg(debug_assertions)]
impl NativeOlePersistedReceiver {
    /// Debug-only synthetic IDataObject handoff, invoked on the actual main
    /// UI thread in the disposable 45-frame smoke. It calls the exact COM
    /// callback implementation but DOES NOT invoke Explorer or move a mouse.
    /// This must never become a production shortcut for real Shell drag.
    pub fn synthetic_disposable_pin_smoke(&self,multi:bool)->Result<(),String>{
        let context=unsafe{(*self.target).persisted_lab.clone()}
            .ok_or("Synthetic OLE smoke has no isolated Workspace context")?;
        let (sources,point)={
            let lab=context.lock().map_err(|_|"Synthetic OLE context poisoned")?;
            if !lab.verified_hwnd_geometry(unsafe{(*self.target).verified_main_hwnd}) {
                return Err("Synthetic OLE smoke HWND geometry is stale".into());
            }
            let source_id=*lab.visible_box_ids.first()
                .ok_or("No verified source Box for OLE sample")?;
            let dest_id=*lab.visible_box_ids.get(1)
                .ok_or("No verified destination Box for OLE sample")?;
            let source=lab.state.group(source_id)
                .ok_or("Native OLE source Box was removed")?;
            let count=if multi{2}else{1};
            let pins=source.tab().items.iter().take(count)
                .collect::<Vec<_>>();
            if pins.len()!=count || pins.iter().any(|pin|
                lab.explorer.iter().filter(|item|
                    item.path.eq_ignore_ascii_case(&pin.path)).count()!=1) {
                return Err("OLE sample source does not match the verified native Explorer identities".into());
            }
            let paths=pins.iter().map(|pin|PathBuf::from(&pin.path))
                .collect::<Vec<_>>();
            let target=lab.state.group(dest_id)
                .ok_or("Missing verified OLE destination Box")?;
            let desired=(target.bounds.x as f64+target.bounds.w as f64*0.5,
                         target.bounds.y as f64+target.bounds.h as f64*0.55);
            let v=lab.viewport;
            let physical=(
                v.screen_origin_px.0 as f64+desired.0*f64::from(v.client_size_px.0)/v.logical_size.0,
                v.screen_origin_px.1 as f64+desired.1*f64::from(v.client_size_px.1)/v.logical_size.1);
            (paths,PointL{x:physical.0.round() as i32,
                                       y:physical.1.round() as i32})
        };
        if sources.iter().any(|path|!path.is_file()){
            return Err("Synthetic OLE source is not an actual disposable file".into());
        }
        let mut obj=SyntheticLabFiles{
            data:DataObject{vtbl:&SYNTHETIC_LAB_VTABLE},
            filenames:sources,
        };
        let mut enter=EFFECT_LINK;
        unsafe{drag_enter(self.target,&mut obj.data,0,point,&mut enter);}
        if enter!=EFFECT_LINK {
            return Err(format!("Synthetic OLE DragEnter refused LINK: {enter}"));
        }
        let mut hover=EFFECT_LINK;
        unsafe{drag_over(self.target,0,point,&mut hover);}
        if hover!=EFFECT_LINK{
            unsafe{drag_leave(self.target);}
            return Err("Synthetic OLE DragOver rejected target geometry".into());
        }
        let mut drop_effect=EFFECT_LINK;
        unsafe{drop_items(self.target,&mut obj.data,0,point,&mut drop_effect);}
        if drop_effect!=EFFECT_LINK{
            let reason=context.lock().ok().and_then(|g|g.last_error.clone());
            return Err(format!("Synthetic OLE persisted Drop rejected: {reason:?}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod native_ole_contract_tests {
    use super::*;
    #[test]
    fn never_advertise_move_copy_or_unoffered_link() {
        assert_eq!(negotiated_link(true, 0), 0);
        assert_eq!(negotiated_link(true, 1), 0); // COPY only
        assert_eq!(negotiated_link(true, 2), 0); // MOVE only
        assert_eq!(negotiated_link(true, 3), 0);
        assert_eq!(negotiated_link(true, 4), 4);
        assert_eq!(negotiated_link(true, 7), 4);
        assert_eq!(negotiated_link(false, 7), 0);
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(handle: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(handle: *mut c_void) -> i32;
        fn GlobalFree(handle: *mut c_void) -> *mut c_void;
    }
    #[repr(C)]
    struct MockFiles {
        data: DataObject,
        filenames: Vec<PathBuf>,
    }
    unsafe extern "system" fn mock_get_data(
        data: *mut DataObject,
        fmt: *const FormatEtc,
        out: *mut StgMedium,
    ) -> i32 {
        if data.is_null() || fmt.is_null() || out.is_null() {
            return E_POINTER;
        }
        if (*fmt).format != FORMAT_HDROP || (*fmt).medium != 1 {
            return E_NOINTERFACE;
        }
        let files = &*(data as *const MockFiles);
        let mut names: Vec<u16> = Vec::new();
        for path in &files.filenames {
            names.extend(path.to_string_lossy().encode_utf16());
            names.push(0);
        }
        names.push(0);
        // DROPFILES = DWORD pFiles, POINT, BOOL fNC, BOOL fWide (20 bytes).
        let length = 20 + names.len() * 2;
        let hglobal = GlobalAlloc(2, length); // GMEM_MOVEABLE for TYMED_HGLOBAL.
        if hglobal.is_null() {
            return E_NOINTERFACE;
        }
        let raw = GlobalLock(hglobal) as *mut u8;
        if raw.is_null() {
            GlobalFree(hglobal);
            return E_NOINTERFACE;
        }
        std::ptr::write_unaligned(raw as *mut u32, 20);
        std::ptr::write_unaligned(raw.add(16) as *mut u32, 1); // UTF-16 filenames.
        std::ptr::copy_nonoverlapping(names.as_ptr(), raw.add(20) as *mut u16, names.len());
        GlobalUnlock(hglobal);
        (*out).tymed = 1;
        (*out).handle = hglobal;
        (*out).unknown = ptr::null_mut();
        0
    }
    static MOCK_VTABLE: DataObjectVtbl = DataObjectVtbl {
        query_interface: 0,
        add_ref: 0,
        release: 0,
        get_data: mock_get_data,
    };
    #[test]
    fn disposable_ole_window_registers_and_revokes_drop_target() {
        #[link(name = "ole32")]
        extern "system" {
            fn OleInitialize(reserved: *mut c_void) -> i32;
            fn OleUninitialize();
        }
        unsafe {
            let hr = OleInitialize(ptr::null_mut());
            assert!(hr >= 0, "OLE apartment initialization failed: {hr:#x}");
            let class = wide("STATIC");
            let title = wide("DesktopCraft isolated OLE target test");
            let parent = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                0x8000_0000u32,
                0,
                0,
                320,
                240,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(
                !parent.is_null(),
                "Could not create disposable OLE lab window"
            );
            let queue: Queue = Arc::new(Mutex::new(Vec::new()));
            let zone = DropZone::new(parent, 42, queue.clone())
                .expect("RegisterDragDrop on disposable child HWND failed");
            zone.set_rect([8, 8, 180, 30]);
            zone.set_visible(false);
            assert!(queue.lock().unwrap().is_empty());
            drop(zone); // explicitly RevokeDragDrop before destroying parent.
            let receiver = NativeWindowDropReceiver::register(parent).unwrap();
            receiver.set_targets(vec![
                ([10, 10, 100, 30], WindowDropRoute::Tab(42, 7)),
            ]);
            let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
            let mut data = MockFiles {
                data: DataObject { vtbl: &MOCK_VTABLE },
                filenames: vec![file.clone()],
            };
            let point = PointL { x: 24, y: 20 };
            let target = receiver._registration.target;
            let mut offered = 7;
            drag_enter(target, &mut data.data, 0, PointL { x: 0, y: 0 }, &mut offered);
            assert_eq!(offered, 0);
            offered = 7;
            drag_over(target, 0, point, &mut offered);
            assert_eq!(offered, EFFECT_LINK);
            offered = 7;
            drop_items(target, &mut data.data, 0, point, &mut offered);
            assert_eq!(offered, EFFECT_LINK);
            let drop = receiver.take_drop().unwrap();
            assert!(matches!(drop.route, WindowDropRoute::Tab(42, 7)));
            assert_eq!(drop.paths, vec![file]);
            assert!(receiver.take_drop().is_none());
            std::mem::drop(receiver);
            assert_ne!(DestroyWindow(parent), 0);
            OleUninitialize();
        }
    }
    #[test]
    fn real_hdrop_medium_rejects_partial_data_and_only_queues_link() {
        let source = std::env::temp_dir().join(format!(
            "dc-ole-fixture-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let contents = b"DesktopCraft model-only OLE drop";
        std::fs::write(&source, contents).unwrap();
        let queue: Queue = Arc::new(Mutex::new(Vec::new()));
        let mut target = DropTarget {
            vtbl: &VTABLE,
            refs: AtomicU32::new(1),
            accepted: AtomicBool::new(false),
            offered_paths: Mutex::new(None),
            group: 42,
            queue: queue.clone(),
            persisted_lab: None,
            window_drop: None,
            verified_main_hwnd: ptr::null_mut(),
        };
        let point = PointL { x: 0, y: 0 };
        let mut data = MockFiles {
            data: DataObject { vtbl: &MOCK_VTABLE },
            filenames: vec![source.clone()],
        };
        unsafe {
            let mut offered = 7;
            assert_eq!(
                drag_enter(&mut target, &mut data.data, 0, point, &mut offered),
                0
            );
            assert_eq!(offered, EFFECT_LINK);
            let mut copy_only = 1;
            assert_eq!(drag_over(&mut target, 0, point, &mut copy_only), 0);
            assert_eq!(copy_only, 0);
            let mut move_only = 2;
            assert_eq!(
                drop_items(&mut target, &mut data.data, 0, point, &mut move_only),
                0
            );
            assert_eq!(move_only, 0);
            assert!(queue.lock().unwrap().is_empty());
            // A single malformed CF_HDROP must not turn into partial success.
            data.filenames.push(source.with_extension("missing"));
            let mut invalid = 7;
            assert_eq!(
                drag_enter(&mut target, &mut data.data, 0, point, &mut invalid),
                0
            );
            assert_eq!(invalid, 0);
            let mut invalid_drop = 7;
            drop_items(&mut target, &mut data.data, 0, point, &mut invalid_drop);
            assert_eq!(invalid_drop, 0);
            assert!(queue.lock().unwrap().is_empty());
            data.filenames.pop();
            // Even two individually valid files must not allow source
            // substitution between DragEnter and Drop.
            let replacement = source.with_extension("replacement");
            std::fs::write(&replacement, b"Different original file").unwrap();
            let mut original_offer = 7;
            drag_enter(&mut target, &mut data.data, 0, point, &mut original_offer);
            assert_eq!(original_offer, EFFECT_LINK);
            data.filenames[0] = replacement.clone();
            let mut changed = 7;
            drop_items(&mut target, &mut data.data, 0, point, &mut changed);
            assert_eq!(changed, 0);
            assert!(queue.lock().unwrap().is_empty());
            data.filenames[0] = source.clone();
            std::fs::remove_file(&replacement).unwrap();
            let mut link = 7;
            drag_enter(&mut target, &mut data.data, 0, point, &mut link);
            assert_eq!(link, EFFECT_LINK);
            let mut final_effect = 7;
            drop_items(&mut target, &mut data.data, 0, point, &mut final_effect);
            assert_eq!(final_effect, EFFECT_LINK);
            let events = queue.lock().unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].group, 42);
            assert_eq!(events[0].paths, vec![source.clone()]);
        }
        assert_eq!(std::fs::read(&source).unwrap(), contents);
        std::fs::remove_file(&source).unwrap();
    }

    #[test]
    fn existing_hidden_hwnd_persisted_ole_target_commits_only_after_validated_drop() {
        use crate::model::{Bounds, Item};
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
        static CASE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "DesktopCraft-Persisted-HWND-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            CASE.fetch_add(1, AtomicOrdering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let paths = ["A", "B", "C", "D"]
            .iter()
            .map(|name| {
                let path = root.join(format!("{name}.lnk"));
                std::fs::write(&path, format!("disposable-{name}")).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let mut state = State::empty();
        let source = state.add_group(
            "Source",
            Bounds {
                x: 50.,
                y: 50.,
                w: 280.,
                h: 260.,
            },
        );
        state
            .group_mut(source)
            .unwrap()
            .tab_mut()
            .items
            .push(Item::from_path(&paths[0], 1).unwrap());
        state.add_tab(source).unwrap();
        state
            .group_mut(source)
            .unwrap()
            .tab_mut()
            .items
            .push(Item::from_path(&paths[1], 2).unwrap());
        state.group_mut(source).unwrap().active = 0;
        let target = state.add_group(
            "Target",
            Bounds {
                x: 400.,
                y: 50.,
                w: 280.,
                h: 260.,
            },
        );
        state
            .group_mut(target)
            .unwrap()
            .tab_mut()
            .items
            .push(Item::from_path(&paths[2], 3).unwrap());
        state.add_tab(target).unwrap();
        state
            .group_mut(target)
            .unwrap()
            .tab_mut()
            .items
            .push(Item::from_path(&paths[3], 4).unwrap());
        state.group_mut(target).unwrap().active = 0;
        let workspace = root.join("workspace.json");
        state.save(&workspace).unwrap();
        let saved_initial = std::fs::read(&workspace).unwrap();
        let icons = paths
            .iter()
            .enumerate()
            .map(|(i, path)| ViewItem {
                path: path.to_string_lossy().into_owned(),
                label: format!("Desktop item {i}"),
                pos: (i as i32 * 110, 50),
            })
            .collect::<Vec<_>>();
        let viewport = NativeOleViewport {
            screen_origin_px: (0, 0),
            client_size_px: (800, 400),
            logical_size: (800., 400.),
        };
        let context = Arc::new(Mutex::new(NativeOlePersistedLabContext::new(
            workspace.clone(),
            state,
            icons,
            "Lab monitor".into(),
            viewport,
            vec![source, target],
            4,
            3,
        )));

        #[link(name = "ole32")]
        extern "system" {
            fn OleInitialize(reserved: *mut c_void) -> i32;
            fn OleUninitialize();
        }
        #[link(name = "user32")]
        extern "system" {
            fn GetWindow(hwnd: HWND, command: u32) -> HWND;
        }
        unsafe {
            assert!(OleInitialize(ptr::null_mut()) >= 0);
            let window_class = wide("STATIC");
            let title = wide("DesktopCraft persisted OLE COM lab (not shown)");
            let hwnd = CreateWindowExW(
                0,
                window_class.as_ptr(),
                title.as_ptr(),
                0x8000_0000u32,
                0,
                0,
                800,
                400,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert!(!hwnd.is_null());
            let old_children = GetWindow(hwnd, 5); // GW_CHILD
            let receiver =
                NativeOlePersistedReceiver::register_existing_hwnd(hwnd, context.clone()).unwrap();
            assert_eq!(GetWindow(hwnd, 5), old_children);
            let mut data = MockFiles {
                data: DataObject { vtbl: &MOCK_VTABLE },
                filenames: vec![paths[0].clone()],
            };
            let hit = PointL { x: 500, y: 165 };
            // A non-Miniquad HWND cannot bypass the new main-window gate.
            (*receiver.target).verified_main_hwnd=hwnd;
            let mut invalid_window=7;
            drag_enter(receiver.target,&mut data.data,0,hit,&mut invalid_window);
            assert_eq!(invalid_window,0);
            assert_eq!(context.lock().unwrap().successful_drops,0);
            (*receiver.target).verified_main_hwnd=ptr::null_mut();
            let mut enter = 7;
            assert_eq!(
                drag_enter(receiver.target, &mut data.data, 0, hit, &mut enter),
                0
            );
            assert_eq!(enter, EFFECT_LINK);
            let mut move_only = 2;
            drag_over(receiver.target, 0, hit, &mut move_only);
            assert_eq!(move_only, 0);
            let mut wrong_box = 7;
            drag_over(
                receiver.target,
                0,
                PointL { x: 100, y: 155 },
                &mut wrong_box,
            );
            assert_eq!(wrong_box, 0);
            let mut valid = 7;
            drag_over(receiver.target, 0, hit, &mut valid);
            assert_eq!(valid, EFFECT_LINK);

            // The target can offer LINK at DragOver, yet the OS callback MUST
            // report NONE if another writer changes Workspace before Drop.
            let mut altered = context.lock().unwrap().state.clone();
            altered.tint = !altered.tint;
            std::fs::write(&workspace, serde_json::to_vec_pretty(&altered).unwrap()).unwrap();
            let bytes_external = std::fs::read(&workspace).unwrap();
            let mut denied = 7;
            drop_items(receiver.target, &mut data.data, 0, hit, &mut denied);
            assert_eq!(denied, 0);
            assert_eq!(std::fs::read(&workspace).unwrap(), bytes_external);
            assert_eq!(context.lock().unwrap().successful_drops, 0);
            // Restore the LAB fixture (never the user's real Workspace).
            std::fs::write(&workspace, &saved_initial).unwrap();

            let mut reenter = 7;
            drag_enter(receiver.target, &mut data.data, 0, hit, &mut reenter);
            assert_eq!(reenter, EFFECT_LINK);
            data.filenames[0] = paths[2].clone(); // valid but substituted source
            let mut substituted = 7;
            drop_items(receiver.target, &mut data.data, 0, hit, &mut substituted);
            assert_eq!(substituted, 0);
            assert_eq!(context.lock().unwrap().successful_drops, 0);
            data.filenames[0] = paths[0].clone();

            // The real Macroquad App still owns its pre-gesture State.
            let mut main_ui_state = context.lock().unwrap().state.clone();
            let mut final_enter = 7;
            drag_enter(receiver.target, &mut data.data, 0, hit, &mut final_enter);
            assert_eq!(final_enter, EFFECT_LINK);
            let mut final_effect = 7;
            assert_eq!(
                drop_items(receiver.target, &mut data.data, 0, hit, &mut final_effect),
                0
            );
            assert_eq!(final_effect, EFFECT_LINK);
            assert_eq!(context.lock().unwrap().successful_drops, 1);
            assert!(context.lock().unwrap().ui_handoff_pending());
            let mut blocked_second = 7;
            drag_enter(receiver.target, &mut data.data, 0, hit, &mut blocked_second);
            assert_eq!(blocked_second, 0); // refuse next gesture until UI ack
            let before_ui_sync = serde_json::to_vec(&main_ui_state).unwrap();
            let mut stale_ui = main_ui_state.clone();
            stale_ui.tint = !stale_ui.tint; // simulated UI model conflict
            let sync_conflict = context.lock().unwrap().synchronize_main_thread(
                &mut stale_ui, viewport, "Lab monitor");
            assert!(sync_conflict.is_err());
            assert!(context.lock().unwrap().ui_handoff_pending());
            assert_eq!(serde_json::to_vec(&main_ui_state).unwrap(), before_ui_sync);
            let synchronized = context.lock().unwrap().synchronize_main_thread(
                &mut main_ui_state, viewport, "Lab monitor").unwrap();
            assert!(synchronized);
            assert!(!context.lock().unwrap().ui_handoff_pending());
            let persisted = State::load(&workspace).unwrap();
            assert_eq!(serde_json::to_vec(&main_ui_state).unwrap(),
                serde_json::to_vec(&persisted).unwrap());
            assert!(persisted.group(source).unwrap().tabs[0].items.is_empty());
            assert_eq!(persisted.group(target).unwrap().tabs[0].items.len(), 2);
            let runtime = context.lock().unwrap();
            assert_eq!(
                serde_json::to_vec(&runtime.state).unwrap(),
                serde_json::to_vec(&persisted).unwrap()
            );
            drop(runtime);
            let mut replay = 7;
            drop_items(receiver.target, &mut data.data, 0, hit, &mut replay);
            assert_eq!(replay, 0);
            assert_eq!(GetWindow(hwnd, 5), old_children); // never a white child
            drop(receiver); // RevokeDragDrop; never destroy caller's HWND
            assert_ne!(DestroyWindow(hwnd), 0);
            OleUninitialize();
        }
        for file in &paths {
            assert!(std::fs::read(file).unwrap().starts_with(b"disposable-"));
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
