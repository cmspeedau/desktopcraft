use super::*;

impl SharedPolicy {
    pub(super) unsafe fn create(pid: u32) -> Result<Self, String> {
        let label = proto::shared_memory_name(pid);
        let bytes = mem::size_of::<proto::FilterPolicy>();
        let handle = CreateFileMappingW(!0usize as *mut c_void, ptr::null_mut(),
            PAGE_READWRITE, (bytes as u64 >> 32) as u32, bytes as u32, label.as_ptr());
        if handle.is_null() { return Err(format!("CreateFileMappingW: {}", GetLastError())); }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return Err("An active native-policy mapping already exists; refusing to overwrite it".into());
        }
        let mapped = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, bytes) as *mut proto::FilterPolicy;
        if mapped.is_null() {
            CloseHandle(handle);
            return Err(format!("MapViewOfFile: {}", GetLastError()));
        }
        Ok(Self {
            handle, mapped,
            #[cfg(feature = "native-fast-tabs")]
            position_ticket: std::cell::Cell::new(0),
        })
    }

    pub(super) unsafe fn generation(&self) -> &AtomicU64 {
        // #[repr(C, align(8))] and a page-aligned mapping guarantee alignment.
        &*(ptr::addr_of!((*self.mapped).generation) as *const AtomicU64)
    }

    pub(super) unsafe fn lease_tick(&self) -> &AtomicU64 {
        &*(ptr::addr_of!((*self.mapped).lease_tick_ms) as *const AtomicU64)
    }

    #[cfg(not(feature = "native-fast-tabs"))]
    pub(super) fn native_drag_paths(&self, _: u32, _: u32, _: u64, _: &[ViewItem]) -> Option<Vec<String>> { None }

    #[cfg(not(feature = "native-fast-tabs"))]
    pub(super) fn native_drag_diag(&self) -> (u64, u64, u64, u32) { (0, 0, 0, 0) }

    #[cfg(feature = "native-fast-tabs")]
    pub(super) fn native_drag_paths(
        &self,
        press: u32,
        release: u32,
        inventory_generation: u64,
        icons: &[ViewItem],
    ) -> Option<Vec<String>> {
        unsafe {
            let p = self.mapped;
            let tick = &*ptr::addr_of!((*p).native_drag_tick).cast::<AtomicU64>();
            let started = tick.load(Ordering::Acquire);
            let generation = self.generation().load(Ordering::Acquire);
            let drag_generation = (*p).native_drag_generation;
            if started == 0
                || !drag_tick_is_current(press, started as u32, release)
                || !drag_generation_is_compatible(drag_generation, inventory_generation, generation)
            {
                return None;
            }
            let count = (*p).native_drag_count as usize;
            if !(1..=icons.len().min(proto::MAX_ITEMS)).contains(&count) {
                return None;
            }
            let source = (*p).native_drag_source;
            let indices = &(&(*p).native_drag_selection)[..count];
            if !indices.contains(&source) {
                return None;
            }
            let paths = indices.iter().map(|&index| {
                icons.get(index as usize).map(|item| item.path.clone())
            }).collect::<Option<Vec<_>>>()?;
            (tick.load(Ordering::Acquire) == started
                && (*p).native_drag_generation == drag_generation
                && self.generation().load(Ordering::Acquire) == generation).then_some(paths)
        }
    }

    #[cfg(feature = "native-fast-tabs")]
    pub(super) fn native_drag_diag(&self) -> (u64, u64, u64, u32) {
        unsafe {
            let p = self.mapped;
            let started = (&*ptr::addr_of!((*p).native_drag_tick).cast::<AtomicU64>()).load(Ordering::Acquire);
            let cur_gen = self.generation().load(Ordering::Acquire);
            (started, (*p).native_drag_generation, cur_gen, (*p).native_drag_count)
        }
    }

    #[cfg(not(feature = "native-fast-tabs"))]
    pub(super) fn read_drop_positions(&self, _: HWND, _: usize) -> Option<Vec<(i32, i32)>> { None }

    #[cfg(feature = "native-fast-tabs")]
    pub(super) fn read_drop_positions(&self, list: HWND, count: usize) -> Option<Vec<(i32, i32)>> {
        if !(1..=proto::MAX_ITEMS).contains(&count) {
            return None;
        }
        let next = self.position_ticket.get().checked_add(1)?;
        self.position_ticket.set(next);
        let ticket = next as u64;
        unsafe {
            let generation = self.generation().load(Ordering::Acquire);
            if generation == 0 || generation & 1 != 0 || (*self.mapped).record_count as usize != count {
                return None;
            }
            let request = &*ptr::addr_of!((*self.mapped).position_request).cast::<AtomicU64>();
            let ack = &*ptr::addr_of!((*self.mapped).position_ack).cast::<AtomicU64>();
            request.store(ticket, Ordering::Release);
            let mut echoed = 0;
            let delivered = SendMessageTimeoutW(
                list, proto::APP_HOOK_READ_POSITIONS, proto::HOOK_COOKIE, next as isize,
                0x0002, 20, &mut echoed,
            );
            request.store(0, Ordering::Release);
            if delivered == 0 || echoed != next || ack.load(Ordering::Acquire) != ticket
                || self.generation().load(Ordering::Acquire) != generation
            {
                return None;
            }
            (&(*self.mapped).positions)[..count].iter().map(|&[x, y]| {
                let mut point = POINT { x, y };
                (ClientToScreen(list, &mut point) != 0).then_some((point.x, point.y))
            }).collect()
        }
    }

    /// Extend ONLY the time-bound lease. For a verified unchanging native
    /// drag policy, do not publish a new generation while Windows is about
    /// to dispatch WM_LBUTTONDOWN or while its ListView has mouse capture.
    pub(super) unsafe fn renew_lease(&self) {
        self.lease_tick().store(GetTickCount64(),Ordering::Release);
    }

    pub(super) unsafe fn publish(&self, pid: u32, list: u64, full_count: usize,
                      records: &[proto::FilterRecord], sequence: u64, flags: u32) {
        self.generation().store(sequence | 1, Ordering::Release);
        let p = &mut *self.mapped;
        p.magic = proto::MAGIC;
        p.abi_version = proto::ABI_VERSION;
        p.explorer_pid = pid;
        p.listview_hwnd = list;
        p.listview_item_count = full_count as u32;
        p.record_count = records.len() as u32;
        p.policy_flags = flags;
        self.renew_lease();
        for (dst, src) in p.records.iter_mut().zip(records) { *dst = *src; }
        self.generation().store((sequence | 1) + 1, Ordering::Release);
    }

    /// Called off the GUI thread. A timed-out or stale reply cannot authorize
    /// Explorer icon movement or dropping the on-screen drag preview.
    pub(super) fn confirm_policy(&self, list: HWND, message: u32) -> Result<(), String> {
        let generation = unsafe { self.generation().load(Ordering::Acquire) };
        let operation = match message {
            proto::APP_HOOK_HOLD_PAINT => "hold",
            proto::APP_HOOK_PAINT_POLICY => "paint",
            _ => "apply",
        };
        let started = std::time::Instant::now();
        let mut echoed = 0;
        let delivered = unsafe {
            SendMessageTimeoutW(list, message, proto::HOOK_COOKIE,
                generation as isize, 0x0002, 400, &mut echoed)
        };
        if proto::confirms_generation(delivered != 0, generation, echoed as u64) {
            crate::host::event("INFO", "native_desktop.policy_confirmed",
                &format!("generation={generation} operation={operation} elapsed_ms={}", started.elapsed().as_millis()));
            Ok(())
        } else {
            Err(format!("Explorer did not confirm native policy generation {generation} (operation={operation})"))
        }
    }

    #[cfg(not(feature = "native-fast-tabs"))]
    pub(super) fn move_hidden_items(&self, _: HWND, _: &[proto::TabMove], _: &str) -> Result<bool, String> {
        Ok(false)
    }

    #[cfg(feature = "native-fast-tabs")]
    pub(super) fn move_hidden_items(&self, list: HWND, moves: &[proto::TabMove], event: &str) -> Result<bool, String> {
        if moves.is_empty() || moves.len() > proto::MAX_TAB_MOVES {
            return Ok(false);
        }
        let started = std::time::Instant::now();
        let mut reply = 0usize;
        let (delivered, unclaimed) = unsafe {
            let generation = self.generation().load(Ordering::Acquire);
            let request = &*ptr::addr_of!((*self.mapped).tab_move_generation).cast::<AtomicU64>();
            (*self.mapped).tab_move_count = moves.len() as u32;
            (&mut (*self.mapped).tab_moves)[..moves.len()].copy_from_slice(moves);
            request.store(generation, Ordering::Release);
            let delivered = SendMessageTimeoutW(list, proto::APP_HOOK_MOVE_TAB,
                proto::HOOK_COOKIE, generation as isize, 0x0002, 150, &mut reply) != 0;
            (delivered, request.swap(0, Ordering::AcqRel) == generation)
        };
        let applied = match (delivered, reply, unclaimed) {
            (true, proto::TAB_MOVES_APPLIED, false) => true,
            (true, 0, _) | (false, _, true) => false,
            _ => return Err("Explorer could not confirm its native icon move batch".into()),
        };
        crate::host::event("INFO", event, &format!(
            "items={} applied={applied} elapsed_us={}", moves.len(), started.elapsed().as_micros()));
        Ok(applied)
    }

    pub(super) unsafe fn verify(&self, pid: u32, hwnd: u64, count: usize) -> bool {
        let a = self.generation().load(Ordering::Acquire);
        if a % 2 != 0 || a == 0 { return false; }
        let p = &*self.mapped;
        let correct = proto::valid_header(
            p.magic, p.abi_version, p.listview_item_count, count as u32,
            p.explorer_pid, pid, p.listview_hwnd, hwnd,
            self.lease_tick().load(Ordering::Acquire),
            GetTickCount64(), a,
        ) && p.record_count as usize == count;
        let b = self.generation().load(Ordering::Acquire);
        correct && a == b
    }
}
#[cfg(test)]
mod atomic_native_lease_tests {
    use super::*;
    #[test]
    pub(super) fn renewing_immutable_native_drag_policy_preserves_generation_and_lease_expires_if_worker_stops(){
        // In-process shared mapping only: never hook or manipulate any
        // Explorer window. The first physical MouseDown must observe the
        // exact same generation as the original installed subclass.
        let unique=u32::MAX.wrapping_sub(std::process::id());
        let policy=unsafe{SharedPolicy::create(unique)}.unwrap();
        let native_records=vec![proto::encode_record(0,"Verified Icon",false).unwrap()];
        unsafe{
            policy.publish(unique,0x1001,1,&native_records,2,proto::FLAG_DRAG_CAPTURE);
            let generation=policy.generation().load(Ordering::Acquire);
            assert_eq!(generation%2,0);
            policy.lease_tick().store(1,Ordering::Release);
            policy.renew_lease();
            let renewed=policy.lease_tick().load(Ordering::Acquire);
            assert!(renewed>1);
            assert_eq!(policy.generation().load(Ordering::Acquire),generation);
            assert!(policy.verify(unique,0x1001,1));
            // If the owner stalls or exits, the same immutable generation
            // cannot make an expired lease valid indefinitely.
            let header=&*policy.mapped;
            assert!(!proto::valid_header(
                header.magic,header.abi_version,header.listview_item_count,1,
                header.explorer_pid,unique,header.listview_hwnd,0x1001,
                renewed,renewed+proto::HEARTBEAT_TIMEOUT_MS+1,generation));
        }
    }
}

impl Drop for SharedPolicy {
    fn drop(&mut self) {
        unsafe {
            // Fail open at shutdown: prevent stale policies from being accepted.
            self.generation().store(1, Ordering::Release);
            (*self.mapped).magic = 0;
            UnmapViewOfFile(self.mapped as *const c_void);
            CloseHandle(self.handle);
        }
    }
}
