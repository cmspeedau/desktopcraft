use crate::{atomic_generation, name_from_list, paint_hold, policy_is_live, protocol, state, LVM_FIRST};
use std::{ptr, sync::atomic::{AtomicU64, Ordering}};
use winapi::shared::windef::{HWND, POINT};
use winapi::um::winuser::{GetAsyncKeyState, GetCapture, GetWindowLongW, ScreenToClient, SendMessageW, GWL_STYLE, VK_LBUTTON};

const LVM_GETITEMPOSITION: u32 = LVM_FIRST + 16;
const LVM_GETORIGIN: u32 = LVM_FIRST + 41;
const LVM_SETITEMPOSITION32: u32 = LVM_FIRST + 49;
const LVM_GETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 55;
const LVS_AUTOARRANGE: i32 = 0x0100;
const LVS_TYPE_MASK: i32 = 0x0003;
const LVS_EX_SNAPTOGRID: isize = 0x0008_0000;

struct Move {
    index: usize,
    from: POINT,
    to: POINT,
}

unsafe fn client_point(hwnd: HWND, [x, y]: [i32; 2]) -> Option<POINT> {
    let mut point = POINT { x, y };
    (ScreenToClient(hwnd, &mut point) != 0).then_some(point)
}

unsafe fn at(hwnd: HWND, index: usize, point: &POINT) -> bool {
    let mut actual = POINT { x: 0, y: 0 };
    SendMessageW(hwnd, LVM_GETITEMPOSITION, index, &mut actual as *mut _ as isize) != 0
        && actual.x == point.x && actual.y == point.y
}

unsafe fn move_to(hwnd: HWND, index: usize, point: &POINT) {
    SendMessageW(hwnd, LVM_SETITEMPOSITION32, index, point as *const _ as isize);
}

/// Gather live coordinates in one call to the Explorer thread. Recheck each
/// index's label so a change in ListView order invalidates the whole reply.
pub(super) unsafe fn read_positions(hwnd: HWND, ticket: u64) -> u64 {
    let Ok(guard) = state().try_lock() else { return 0 };
    let Some(filter) = guard.as_ref() else { return 0 };
    if ticket == 0 || filter.list != hwnd as usize || !filter.verified
        || !GetCapture().is_null() || GetAsyncKeyState(VK_LBUTTON) < 0
        || !policy_is_live(filter)
    {
        return 0;
    }
    let policy = filter.mapping_ptr as *mut protocol::FilterPolicy;
    let request = &*ptr::addr_of!((*policy).position_request).cast::<AtomicU64>();
    let generation = atomic_generation(policy);
    if request.load(Ordering::Acquire) != ticket
        || generation.load(Ordering::Acquire) != filter.generation
    {
        return 0;
    }
    let count = filter.observed_count;
    if !(1..=protocol::MAX_ITEMS).contains(&count) || (*policy).record_count as usize != count {
        return 0;
    }
    let Some(positions) = read_indexed_positions(hwnd, &(&(*policy).records)[..count]) else {
        return 0;
    };
    if generation.load(Ordering::Acquire) != filter.generation
        || !policy_is_live(filter) || GetAsyncKeyState(VK_LBUTTON) < 0
        || request.load(Ordering::Acquire) != ticket
    {
        return 0;
    }
    (&mut (*policy).positions)[..count].copy_from_slice(&positions);
    let ack = &*ptr::addr_of!((*policy).position_ack).cast::<AtomicU64>();
    ack.store(ticket, Ordering::Release);
    ticket
}

pub(super) unsafe fn read_indexed_positions(
    hwnd: HWND, records: &[protocol::FilterRecord],
) -> Option<Vec<[i32; 2]>> {
    // Only zero-origin icon views share the desktop's client coordinates.
    let mut origin = POINT { x: 0, y: 0 };
    if SendMessageW(hwnd, LVM_GETORIGIN, 0, &mut origin as *mut _ as isize) == 0
        || origin.x != 0 || origin.y != 0
    {
        return None;
    }
    let mut positions = Vec::with_capacity(records.len());
    for (index, label) in records.iter().enumerate() {
        let len = label.label_len as usize;
        if label.index as usize != index || !(1..protocol::MAX_LABEL_U16).contains(&len)
            || name_from_list(hwnd, index).as_deref() != Some(&label.label_utf16[..len])
        {
            return None;
        }
        let mut point = POINT { x: 0, y: 0 };
        if SendMessageW(hwnd, LVM_GETITEMPOSITION, index, &mut point as *mut _ as isize) == 0 {
            return None;
        }
        positions.push([point.x, point.y]);
    }
    Some(positions)
}

/// Called on the existing Explorer ListView subclass with all moving icons
/// hidden by the transition policy and its previous frame retained.
pub(super) unsafe fn apply(hwnd: HWND, generation: u64) -> usize {
    let Ok(guard) = state().try_lock() else { return 0 };
    let Some(filter) = guard.as_ref() else { return 0 };
    if filter.list != hwnd as usize || filter.generation != generation
        || !filter.verified || !policy_is_live(filter) || !paint_hold::is_active(hwnd)
        || !GetCapture().is_null() {
        return 0;
    }
    let style = GetWindowLongW(hwnd, GWL_STYLE);
    if style == 0 || style & LVS_AUTOARRANGE != 0 || !matches!(style & LVS_TYPE_MASK, 0 | 2)
        || SendMessageW(hwnd, LVM_GETEXTENDEDLISTVIEWSTYLE, 0, 0) & LVS_EX_SNAPTOGRID != 0 {
        return 0;
    }

    let policy = filter.mapping_ptr as *const protocol::FilterPolicy;
    let request = &*ptr::addr_of!((*policy).tab_move_generation).cast::<AtomicU64>();
    if generation == 0 || request.load(Ordering::Acquire) != generation {
        return 0;
    }
    let count = (*policy).tab_move_count as usize;
    if !(1..=protocol::MAX_TAB_MOVES).contains(&count) {
        return 0;
    }
    let mut moves = Vec::with_capacity(count);
    let mut seen = [false; protocol::MAX_ITEMS];
    for i in 0..count {
        let record = (*policy).tab_moves[i];
        let index = record.index as usize;
        if index >= filter.observed_count || seen[index] || !filter.hidden[index] {
            return 0;
        }
        seen[index] = true;
        let label = &(*policy).records[index];
        // The policy lock prevents our label-redaction callback from masking
        // this synchronous ListView identity check.
        let label_len = label.label_len as usize;
        if label.index as usize != index || !(1..protocol::MAX_LABEL_U16).contains(&label_len)
            || name_from_list(hwnd, index).as_deref() != Some(&label.label_utf16[..label_len]) {
            return 0;
        }
        let (Some(from), Some(to)) = (client_point(hwnd, record.from), client_point(hwnd, record.to)) else {
            return 0;
        };
        if !at(hwnd, index, &from) {
            return 0;
        }
        moves.push(Move { index, from, to });
    }
    // A timed-out request which hasn't started can no longer write icons.
    if !paint_hold::is_active(hwnd) || !policy_is_live(filter)
        || request.compare_exchange(generation, 0, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return 0;
    }

    let mut attempted = 0;
    for item in &moves { paint_hold::note_item(hwnd, item.index); }
    for item in &moves {
        move_to(hwnd, item.index, &item.to);
        attempted += 1;
        if !at(hwnd, item.index, &item.to) {
            break;
        }
    }
    if attempted == moves.len() && moves.iter().all(|item| at(hwnd, item.index, &item.to)) {
        for item in &moves { paint_hold::note_item(hwnd, item.index); }
        paint_hold::verified_move();
        return protocol::TAB_MOVES_APPLIED;
    }
    for item in moves[..attempted].iter().rev() {
        if at(hwnd, item.index, &item.to) {
            move_to(hwnd, item.index, &item.from);
        }
    }
    if moves.iter().all(|item| at(hwnd, item.index, &item.from)) {
        0 // Safe to use the existing Shell path from the old layout.
    } else {
        protocol::TAB_MOVES_UNCERTAIN
    }
}
