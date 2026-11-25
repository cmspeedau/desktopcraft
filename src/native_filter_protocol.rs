//! Versioned, fail-open policy shared between DesktopCraft and its optional
//! independently built Explorer-native integration DLL.
//!
//! This file is compiled by both crates. Never reinterpret a pointer to this
//! payload unless magic, ABI size, version, Explorer PID, exact HWND, item count,
//! generation (even/stable) and heartbeat lease all match. With no valid
//! policy, the DLL must let Explorer draw and handle every native item.
use std::collections::HashSet;
use std::sync::atomic::AtomicU32;

pub const MAGIC: u32 = 0x4443_464C; // DCFL
pub const ABI_VERSION: u32 = if cfg!(feature = "native-fast-tabs") { 19 } else { 15 };
pub const MAX_ITEMS: usize = 2048;
pub const MAX_DRAG_SELECTION: usize = 8;
pub const MAX_LABEL_U16: usize = 128;
pub const HEARTBEAT_TIMEOUT_MS: u64 = 5000;
pub const STATE_SHOW: u32 = 0;
pub const STATE_HIDE: u32 = 1;
/// OBSERVE is safe and makes no changes to Explorer's rendering or input.
pub const FLAG_PAINT_HIDE: u32 = 1 << 0;
pub const FLAG_INPUT_HIDE: u32 = 1 << 1;
/// Developer-only native gesture capture for one verified visible icon.
pub const FLAG_DRAG_CAPTURE: u32 = 1 << 2;
pub const HOOK_STATE_DETACHED: u32 = 0;
pub const HOOK_STATE_ATTACHED: u32 = 1;
pub const HOOK_STATE_ERROR: u32 = 2;
pub const APP_HOOK_INSTALL: u32 = 0x8000 + 0x4e9;
pub const APP_HOOK_REMOVE: u32 = 0x8000 + 0x4ea;
/// Pointer-free request; the native DLL allocates its own ListView structs.
pub const APP_HOOK_INPUT_INSPECT: u32 = 0x8000 + 0x4eb;
/// Synthetic hover targets are resolved entirely inside Explorer.
pub const APP_HOOK_INPUT_HOVER: u32 = 0x8000 + 0x4ec;
pub const APP_HOOK_INPUT_RIGHTCLICK: u32 = 0x8000 + 0x4ed;
/// Pointer-free request to apply and repaint one exact policy generation.
pub const APP_HOOK_PAINT_POLICY: u32 = 0x8000 + 0x4ee;
/// Adopt a transitional hide policy without forcing an intermediate frame.
pub const APP_HOOK_APPLY_POLICY: u32 = 0x8000 + 0x4ef;
/// Retain the last ListView frame before installing a transition policy.
pub const APP_HOOK_HOLD_PAINT: u32 = 0x8000 + 0x4f0;
pub const APP_HOOK_MOVE_TAB: u32 = 0x8000 + 0x4f1;
#[cfg(feature = "native-fast-tabs")]
pub const APP_HOOK_READ_POSITIONS: u32 = 0x8000 + 0x4f2;
pub const MAX_TAB_MOVES: usize = MAX_ITEMS;
pub const TAB_MOVES_APPLIED: usize = 1;
pub const TAB_MOVES_UNCERTAIN: usize = 2;
pub const PAINT_ACK_VERSION: u32 = 2;
pub const HOOK_COOKIE: usize = 0x4443_5649; // DCVI

pub fn confirms_generation(delivered: bool, expected: u64, echoed: u64) -> bool {
    delivered && expected != 0 && expected % 2 == 0 && echoed == expected
}

#[cfg(test)]
#[test]
fn policy_confirmation_requires_a_delivered_reply_for_the_exact_published_generation() {
    assert!(confirms_generation(true, 8, 8));
    assert!(!confirms_generation(true, 8, 6));
    assert!(!confirms_generation(false, 8, 8));
    assert!(!confirms_generation(true, 0, 0));
    assert!(!confirms_generation(true, 7, 7));
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FilterRecord {
    pub index: u32,
    pub state: u32,
    pub label_len: u32,
    pub label_utf16: [u16; MAX_LABEL_U16],
}

impl Default for FilterRecord {
    fn default() -> Self {
        Self { index: 0, state: STATE_SHOW, label_len: 0, label_utf16: [0; MAX_LABEL_U16] }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TabMove {
    pub index: u32,
    pub from: [i32; 2],
    pub to: [i32; 2],
}

#[repr(C, align(8))]
pub struct FilterPolicy {
    pub magic: u32,
    pub abi_version: u32,
    /// Writer: odd during update; publish even with release ordering.
    /// Reader: reject odd or changed generation.
    pub generation: u64,
    /// 8-byte aligned cross-process heartbeat. Both host and Explorer
    /// access via AtomicU64, so a constant generation can be kept for
    /// an entire live MouseDown→MouseUp gesture.
    pub lease_tick_ms: u64,
    pub explorer_pid: u32,
    pub listview_item_count: u32,
    pub listview_hwnd: u64,
    pub record_count: u32,
    pub policy_flags: u32,
    // In-process module writes only these diagnostics; it never modifies the
    // host-owned payload fields. They are excluded from generation protection.
    pub hook_state: AtomicU32,
    pub hook_error: u32,
    pub hook_parent_customdraw: u32,
    pub hook_item_customdraw: u32,
    pub hook_label_matches: u32,
    pub hook_draw_suppressed: u32,
    pub hook_input_suppressed: u32,
    pub hook_rejected_stale: u32,
    pub hook_listview_paint: u32,
    pub hook_listview_printclient: u32,
    pub hook_parent_notify: u32,
    pub hook_parent_drawitem: u32,
    pub hook_selection_notify: u32,
    pub hook_notify_getdispinfo_w: u32,
    pub hook_notify_getdispinfo_a: u32,
    pub hook_notify_cachehint: u32,
    pub hook_notify_other: u32,
    /// Counters written only inside the Explorer process. No foreign pointers.
    pub hook_diag_requests: u32,
    pub hook_diag_geometry_ok: u32,
    pub hook_diag_hit_matches: u32,
    pub hook_diag_hidden_targets: u32,
    pub hook_diag_failures: u32,
    pub hook_diag_last_index: u32,
    pub hook_diag_policy_rejections: u32,
    pub hook_diag_rect_failures: u32,
    pub hook_diag_hit_misses: u32,
    pub hook_diag_hover_posted: u32,
    pub hook_diag_hover_suppressed: u32,
    pub hook_diag_hover_client_x: i32,
    pub hook_diag_hover_client_y: i32,
    pub hook_diag_right_posted: u32,
    pub hook_diag_right_suppressed: u32,
    pub hook_diag_menu_posted: u32,
    pub hook_diag_menu_suppressed: u32,
    /// Debug-only instrumentation for a WM_CONTEXTMENU that actually reached
    /// the Explorer ListView subclass. All coordinates remain plain integers.
    pub hook_diag_menu_received: u32,
    pub hook_diag_menu_parent_received: u32,
    pub hook_diag_menu_client_x: i32,
    pub hook_diag_menu_client_y: i32,
    pub hook_diag_menu_hit_index: i32,
    pub hook_diag_menu_hidden_at_hit: u32,
    pub hook_diag_menu_policy_active: u32,
    pub drag_source_index: u32,
    /// Explorer telemetry: 0 idle; 1 capturing; 2 released; 3 cancelled.
    pub drag_phase: u32,
    pub drag_begin_x: i32,
    pub drag_begin_y: i32,
    pub drag_end_x: i32,
    pub drag_end_y: i32,
    pub drag_mouse_moves: u32,
    pub drag_cancelled_count: u32,
    /// Debug-only gesture gate diagnostics; no native pointers cross processes.
    pub drag_down_seen: u32,
    pub drag_down_accepted: u32,
    pub drag_up_seen: u32,
    /// 1=policy, 2=preexisting capture, 3=source, 4=hit, 5=conversion,
    /// 6=capture, 7=more than 8 selected native Explorer items,
    /// 8=source was not included in native ListView selection.
    pub drag_down_reject_reason: u32,
    /// Debug-only cancellation cause after a valid capture:
    /// 1=cancel-mode, 2=capture changed, 3=capture lost,
    /// 4=lease/policy stale, 5=coordinate conversion,
    /// 6=hook uninstalled, 7=insufficient movement,
    /// 8=capture changed AFTER physical left button released but rejected;
    /// 9=completed valid Drag by inferred physical release on WM_CAPTURECHANGED.
    pub drag_cancel_reason: u32,
    pub drag_down_client_x: i32,
    pub drag_down_client_y: i32,
    pub drag_hit_result: i32,
    pub drag_hit_item: i32,
    pub drag_hit_flags: u32,
    /// Captured real ListView selection at initial MouseDown. Validated
    /// again against Explorer path and active Source Tab by the host.
    pub drag_selection_count: u32,
    pub drag_selection_indices: [u32; MAX_DRAG_SELECTION],
    pub records: [FilterRecord; MAX_ITEMS],
    /// The host stages one move request under the already applied hide policy.
    /// Only the ListView thread can claim the generation before moving items.
    #[cfg(feature = "native-fast-tabs")]
    pub tab_move_generation: u64,
    #[cfg(feature = "native-fast-tabs")]
    pub tab_move_count: u32,
    #[cfg(feature = "native-fast-tabs")]
    pub tab_moves: [TabMove; MAX_TAB_MOVES],
    /// A read-only request identified by a unique ticket. Only the owning
    /// ListView thread populates the coordinate buffer and acknowledges it.
    #[cfg(feature = "native-fast-tabs")]
    pub position_request: u64,
    #[cfg(feature = "native-fast-tabs")]
    pub position_ack: u64,
    #[cfg(feature = "native-fast-tabs")]
    pub positions: [[i32; 2]; MAX_ITEMS],
    /// Explorer publishes actual ListView drag starts. The host correlates
    /// them with the physical press/release before using the selection.
    #[cfg(feature = "native-fast-tabs")]
    pub native_drag_tick: u64,
    #[cfg(feature = "native-fast-tabs")]
    pub native_drag_generation: u64,
    #[cfg(feature = "native-fast-tabs")]
    pub native_drag_source: u32,
    #[cfg(feature = "native-fast-tabs")]
    pub native_drag_count: u32,
    #[cfg(feature = "native-fast-tabs")]
    pub native_drag_selection: [u32; MAX_ITEMS],
}

pub fn shared_memory_name(explorer_pid: u32) -> Vec<u16> {
    // Keep the fast-filter lock name across ABI upgrades so both versions
    // cannot attach at the same time. The header validates the payload ABI.
    let namespace = if cfg!(feature = "native-fast-tabs") { 16 } else { ABI_VERSION };
    format!("Local\\DesktopCraft.NativeFilter.v{namespace}.{explorer_pid}")
        .encode_utf16().chain(Some(0)).collect()
}

pub fn encode_record(index: usize, label: &str, hidden: bool) -> Option<FilterRecord> {
    if index >= MAX_ITEMS || label.is_empty() || label.chars().any(|ch| ch == '\0') { return None; }
    let utf16: Vec<u16> = label.encode_utf16().collect();
    if utf16.is_empty() || utf16.len() >= MAX_LABEL_U16 { return None; }
    let mut record = FilterRecord {
        index: index as u32, state: if hidden { STATE_HIDE } else { STATE_SHOW },
        label_len: utf16.len() as u32, ..FilterRecord::default()
    };
    record.label_utf16[..utf16.len()].copy_from_slice(&utf16);
    Some(record)
}

/// The writer must supply a full bijection to one *observed* ListView order.
/// No partial filtering; if an item cannot be verified, return None and leave
/// all icons visible. Display-label collisions are rejected instead of risking
/// hiding somebody else's similarly named file.
pub fn verified_records<'a>(items: impl IntoIterator<Item = (usize, &'a str, bool)>,
                             actual_count: usize) -> Option<Vec<FilterRecord>> {
    if actual_count == 0 || actual_count > MAX_ITEMS { return None; }
    let mut seen_indexes = HashSet::new();
    let mut seen_names = HashSet::new();
    let mut result = Vec::new();
    for (index, name, hide) in items {
        if index >= actual_count || !seen_indexes.insert(index)
            || !seen_names.insert(name.to_lowercase()) { return None; }
        result.push(encode_record(index, name, hide)?);
    }
    if result.len() != actual_count { return None; }
    result.sort_by_key(|r| r.index);
    if result.iter().enumerate().any(|(i, r)| r.index != i as u32) { return None; }
    Some(result)
}

/// Validating a copy of a shared-memory header is mandatory at every runtime
/// transition. The DLL additionally checks the generation before and after
/// copying the fixed-size snapshot from mapped memory.
pub fn valid_header(magic: u32, version: u32, count: u32,
                    expected_count: u32, target_pid: u32, current_pid: u32,
                    target_hwnd: u64, current_hwnd: u64,
                    lease_tick: u64, now_tick: u64, generation: u64) -> bool {
    magic == MAGIC && version == ABI_VERSION
        && count == expected_count && count > 0 && count as usize <= MAX_ITEMS
        && target_pid == current_pid && current_pid != 0
        && target_hwnd == current_hwnd && current_hwnd != 0
        && generation > 0 && generation % 2 == 0
        && now_tick >= lease_tick && now_tick - lease_tick <= HEARTBEAT_TIMEOUT_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_name_keeps_the_fast_filter_lock_namespace() {
        let raw=shared_memory_name(12345);
        assert_eq!(raw.last(),Some(&0));
        let title=String::from_utf16(&raw[..raw.len()-1]).unwrap();
        let namespace = if cfg!(feature = "native-fast-tabs") { 16 } else { ABI_VERSION };
        assert_eq!(title,format!("Local\\DesktopCraft.NativeFilter.v{namespace}.12345"));
    }
    #[test]
    fn full_unique_order_is_required() {
        assert!(verified_records([(0,"A",true),(1,"B",false)],2).is_some());
        assert!(verified_records([(0,"A",true)],2).is_none());
        assert!(verified_records([(0,"A",true),(1,"a",false)],2).is_none());
        assert!(verified_records([(0,"A",true),(0,"B",false)],2).is_none());
        assert!(verified_records([(2,"A",true),(1,"B",false)],2).is_none());
    }
    #[test]
    fn labels_and_high_native_indexes_are_bounded() {
        assert!(encode_record(0,"ไฟล์ภาษาไทย",true).is_some());
        assert!(encode_record(0,"",true).is_none());
        assert!(encode_record(MAX_ITEMS,"file",false).is_none());
        assert!(encode_record(1,&"x".repeat(MAX_LABEL_U16),false).is_none());
        assert!(encode_record(1,"name\0suffix",false).is_none());
    }
    #[test]
    fn stale_wrong_target_and_in_progress_generation_are_rejected() {
        let ok=|p,h,t,n,g| valid_header(MAGIC,ABI_VERSION,61,61,p,99,h,0x1234,t,n,g);
        assert!(ok(99,0x1234,1000,1001,2));
        assert!(!ok(100,0x1234,1000,1001,2));
        assert!(!ok(99,0x1235,1000,1001,2));
        assert!(!ok(99,0x1234,1000,7000,2));
        assert!(!ok(99,0x1234,1000,1001,3));
        assert!(!ok(99,0x1234,1000,999,2));
    }
}
