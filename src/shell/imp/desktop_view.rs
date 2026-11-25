use super::*;

impl DesktopView {
    pub fn acquire() -> Option<Self> {
        unsafe {
            let mut windows = ptr::null_mut();
            if CoCreateInstance(
                &CLSID_SHELL_WINDOWS,
                ptr::null_mut(),
                0x17,
                &IID_ISHELL_WINDOWS,
                &mut windows,
            ) < 0
            {
                return None;
            }
            let windows = Com(windows);
            let location = Variant {
                vt: 3,
                ..Default::default()
            }; // VT_I4 CSIDL_DESKTOP
            let root = Variant::default();
            let mut hwnd = 0i32;
            let mut dispatch = ptr::null_mut();
            let find: extern "system" fn(
                Ptr,
                *const Variant,
                *const Variant,
                i32,
                *mut i32,
                i32,
                *mut Ptr,
            ) -> HRESULT = mem::transmute(vfn(windows.0, 15));
            find(windows.0, &location, &root, 8, &mut hwnd, 1, &mut dispatch);
            let dispatch = Com(dispatch);
            let provider = dispatch.query(&IID_ISERVICE_PROVIDER);
            if !provider.ok() {
                return None;
            }
            let mut browser = ptr::null_mut();
            let query_service: extern "system" fn(
                Ptr,
                *const GUID,
                *const GUID,
                *mut Ptr,
            ) -> HRESULT = mem::transmute(vfn(provider.0, 3));
            query_service(
                provider.0,
                &SID_STOP_LEVEL_BROWSER,
                &IID_ISHELL_BROWSER,
                &mut browser,
            );
            let browser = Com(browser);
            if !browser.ok() {
                return None;
            }
            let mut shell_view = ptr::null_mut();
            let active: extern "system" fn(Ptr, *mut Ptr) -> HRESULT =
                mem::transmute(vfn(browser.0, 15));
            active(browser.0, &mut shell_view);
            let shell_view = Com(shell_view);
            let view = shell_view.query(&IID_IFOLDER_VIEW);
            if !view.ok() {
                return None;
            }
            let mut folder = ptr::null_mut();
            let get_folder: extern "system" fn(Ptr, *const GUID, *mut Ptr) -> HRESULT =
                mem::transmute(vfn(view.0, 5));
            get_folder(view.0, &IID_ISHELL_FOLDER, &mut folder);
            let mut defview: HWND = ptr::null_mut();
            let get_window: extern "system" fn(Ptr, *mut HWND) -> HRESULT =
                mem::transmute(vfn(shell_view.0, 3));
            if get_window(shell_view.0, &mut defview) < 0 || defview.is_null() {
                return None;
            }
            let mut list = FindWindowExW(
                defview,
                ptr::null_mut(),
                wide("SysListView32").as_ptr(),
                ptr::null(),
            );
            if list.is_null() {
                list = defview;
            }
            Some(DesktopView {
                view,
                folder: Com(folder),
                list,
            })
        }
    }
    unsafe fn name(&self, pidl: Ptr) -> Option<String> {
        self.name_with(pidl, 0x8000) // SHGDN_FORPARSING: full path
    }
    unsafe fn name_with(&self, pidl: Ptr, flags: u32) -> Option<String> {
        if !self.folder.ok() {
            return None;
        }
        let mut ret = StrRet([0; 272]);
        let f: extern "system" fn(Ptr, Ptr, u32, *mut StrRet) -> HRESULT =
            mem::transmute(vfn(self.folder.0, 11));
        if f(self.folder.0, pidl, flags, &mut ret) < 0 {
            return None;
        }
        let mut out = ptr::null_mut();
        if StrRetToStrW(&mut ret, pidl, &mut out) < 0 || out.is_null() {
            return None;
        }
        let s = from_wide(out);
        CoTaskMemFree(out as Ptr);
        Some(s)
    }
    unsafe fn entries(&self) -> Vec<(String, POINT, Pidl)> {
        explorer_enumeration_diagnostics();
        let mut out = vec![];
        let mut en = ptr::null_mut();
        let items: extern "system" fn(Ptr, u32, *const GUID, *mut Ptr) -> HRESULT =
            mem::transmute(vfn(self.view.0, 8));
        if items(self.view.0, 2, &IID_IENUM_IDLIST, &mut en) < 0 || en.is_null() {
            return out;
        }
        let en = Com(en);
        let next: extern "system" fn(Ptr, u32, *mut Ptr, *mut u32) -> HRESULT =
            mem::transmute(vfn(en.0, 3));
        let position: extern "system" fn(Ptr, Ptr, *mut POINT) -> HRESULT =
            mem::transmute(vfn(self.view.0, 11));
        let mut batch: [Ptr; 64] = [ptr::null_mut(); 64];
        loop {
            let mut fetched = 0u32;
            let hr = next(en.0, batch.len() as u32, batch.as_mut_ptr(), &mut fetched);
            if hr < 0 || fetched == 0 {
                break;
            }
            for &raw in &batch[..(fetched as usize).min(batch.len())] {
                let pidl = Pidl(raw);
                if out.len() <= 20000 {
                    let mut pt = POINT { x: 0, y: 0 };
                    position(self.view.0, pidl.0, &mut pt);
                    ClientToScreen(self.list, &mut pt);
                    if let Some(name) = self.name(pidl.0) {
                        out.push((name, pt, pidl));
                    }
                }
            }
            if hr != 0 || out.len() > 20000 {
                break;
            }
        }
        out
    }
    fn enumerate_ids(&self, flags: u32, limit: usize) -> Option<Vec<Pidl>> {
        unsafe {
            let mut enumerator = ptr::null_mut();
            let items: extern "system" fn(Ptr, u32, *const GUID, *mut Ptr) -> HRESULT =
                mem::transmute(vfn(self.view.0, 8));
            if items(self.view.0, flags, &IID_IENUM_IDLIST, &mut enumerator) < 0
                || enumerator.is_null()
            {
                return None;
            }
            let enumerator = Com(enumerator);
            let next: extern "system" fn(Ptr, u32, *mut Ptr, *mut u32) -> HRESULT =
                mem::transmute(vfn(enumerator.0, 3));
            let mut result = Vec::new();
            let mut batch: [Ptr; 64] = [ptr::null_mut(); 64];
            loop {
                let mut fetched = 0u32;
                let status = next(enumerator.0, batch.len() as u32, batch.as_mut_ptr(), &mut fetched);
                let fetched = (fetched as usize).min(batch.len());
                result.extend(batch[..fetched].iter().copied().map(Pidl));
                if status < 0 || result.len() > limit {
                    return None;
                }
                if fetched == 0 || status != 0 {
                    return Some(result);
                }
            }
        }
    }

    unsafe fn entries_for_position(&self) -> Vec<(String, Pidl)> {
        let mut out = vec![];
        let mut en = ptr::null_mut();
        let items: extern "system" fn(Ptr, u32, *const GUID, *mut Ptr) -> HRESULT =
            mem::transmute(vfn(self.view.0, 8));
        if items(self.view.0, 2, &IID_IENUM_IDLIST, &mut en) < 0 || en.is_null() {
            return out;
        }
        let en = Com(en);
        let next: extern "system" fn(Ptr, u32, *mut Ptr, *mut u32) -> HRESULT =
            mem::transmute(vfn(en.0, 3));
        let mut batch: [Ptr; 64] = [ptr::null_mut(); 64];
        loop {
            let mut fetched = 0u32;
            let hr = next(en.0, batch.len() as u32, batch.as_mut_ptr(), &mut fetched);
            if hr < 0 || fetched == 0 {
                break;
            }
            for &raw in &batch[..(fetched as usize).min(batch.len())] {
                let pidl = Pidl(raw);
                if out.len() <= 20000 {
                    if let Some(name) = self.name(pidl.0) {
                        out.push((name, pidl));
                    }
                }
            }
            if hr != 0 || out.len() > 20000 {
                break;
            }
        }
        out
    }
    /// Every icon on the desktop with its screen position.
    pub fn items(&self) -> Vec<ViewItem> {
        unsafe {
            self.entries()
                .into_iter()
                .map(|(path, pt, pidl)| ViewItem {
                    label: self.name_with(pidl.0, 0).unwrap_or_else(|| path.clone()),
                    path,
                    pos: (pt.x, pt.y),
                })
                .collect()
        }
    }

    /// Build the identity cache during the existing start-of-session audit.
    pub(crate) fn items_with_drop_identities(&self) -> (Vec<ViewItem>, Option<DropIdentityCache>) {
        let entries = unsafe { self.entries() };
        let mut cache = std::collections::HashMap::with_capacity(entries.len());
        let items: Vec<_> = entries.iter().map(|(path, pt, pidl)| {
            let item = ViewItem {
                label: unsafe { self.name_with(pidl.0, 0) }.unwrap_or_else(|| path.clone()),
                path: path.clone(),
                pos: (pt.x, pt.y),
            };
            if let Some(id) = pidl.bytes() {
                cache.insert(id.to_vec(), item.clone());
            }
            item
        }).collect();
        let identities = (!items.is_empty() && cache.len() == items.len())
            .then_some(DropIdentityCache { items: cache });
        (items, identities)
    }

    pub(crate) fn selected_drop_paths(&self, cache: &DropIdentityCache) -> Option<Vec<String>> {
        self.enumerate_ids(1 /* SVGIO_SELECTION */, cache.items.len())?.iter().map(|pidl| {
            Some(cache.items.get(pidl.bytes()?)?.path.clone())
        }).collect()
    }

    /// Revalidate the exact inventory, then read positions without asking
    /// the Shell to resolve the two names for each item a second time.
    #[cfg(test)]
    pub(crate) fn drop_positions(&self, cache: &DropIdentityCache) -> Option<Vec<ViewItem>> {
        self.cached_drop_entries(cache)?.into_iter().map(|(mut item, pidl)| {
            item.pos = self.drop_item_position(&pidl)?;
            Some(item)
        }).collect()
    }

    fn cached_drop_entries(&self, cache: &DropIdentityCache) -> Option<Vec<(ViewItem, Pidl)>> {
        let current = self.enumerate_ids(2 /* SVGIO_ALLVIEW */, cache.items.len())?;
        if current.len() != cache.items.len() {
            return None;
        }
        let mut seen = std::collections::HashSet::with_capacity(current.len());
        let items = current.iter().map(|pidl| {
            let id = pidl.bytes()?;
            if !seen.insert(id) {
                return None;
            }
            Some(cache.items.get(id)?.clone())
        }).collect::<Option<Vec<_>>>()?;
        Some(items.into_iter().zip(current).collect())
    }

    fn drop_item_position(&self, pidl: &Pidl) -> Option<(i32, i32)> {
        unsafe {
            let position: extern "system" fn(Ptr, Ptr, *mut POINT) -> HRESULT =
                mem::transmute(vfn(self.view.0, 11));
            let mut pt = POINT { x: 0, y: 0 };
            (position(self.view.0, pidl.0, &mut pt) == 0 && ClientToScreen(self.list, &mut pt) != 0)
                .then_some((pt.x, pt.y))
        }
    }

    fn batched_drop_entries(
        &self, cache: &DropIdentityCache, indexed: &[ViewItem], positions: &[(i32, i32)],
        grabbed: &str, expected: &[ViewItem],
    ) -> Option<Vec<(ViewItem, Pidl)>> {
        if indexed.len() != positions.len() {
            return None;
        }
        let by_path: std::collections::HashMap<_, _> = indexed.iter().zip(positions)
            .map(|(item, pos)| (item.path.as_str(), (&item.label, *pos))).collect();
        let entries = self.cached_drop_entries(cache)?;
        let before: std::collections::HashMap<_, _> = expected.iter()
            .map(|item| (item.path.as_str(), item)).collect();
        if entries.len() != by_path.len() || entries.len() != before.len() {
            return None;
        }
        let mut anchor_verified = false;
        let entries = entries.into_iter().map(|(mut item, pidl)| {
            let (label, pos) = by_path.get(item.path.as_str())?;
            let previous = before.get(item.path.as_str())?;
            if *label != &item.label || previous.label != item.label {
                return None;
            }
            let is_anchor = item.path.eq_ignore_ascii_case(grabbed);
            if (is_anchor || previous.pos != *pos) && self.drop_item_position(&pidl) != Some(*pos) {
                return None;
            }
            anchor_verified |= is_anchor;
            item.pos = *pos;
            Some((item, pidl))
        }).collect::<Option<Vec<_>>>()?;
        anchor_verified.then_some(entries)
    }
    /// Keep the freshly observed Shell item IDs for one verified drop.
    /// After placement, verify the unchanged IDs and only read the moved coordinates.
    pub(crate) fn items_for_drop(
        &self, grabbed: &str, expected: &[ViewItem],
        batch: Option<(&DropIdentityCache, &[ViewItem], &[(i32, i32)])>,
        cached_shell: Option<&DropIdentityCache>,
    ) -> (Vec<ViewItem>, impl FnOnce(&[(String, (i32, i32))]) -> bool + '_, &'static str) {
        let batched = batch.and_then(|(cache, indexed, positions)| {
            self.batched_drop_entries(cache, indexed, positions, grabbed, expected)
        });
        let (entries, source) = if let Some(entries) = batched {
            (entries, "listview")
        } else if let Some(entries) = cached_shell.and_then(|cache| {
            self.cached_drop_entries(cache)?.into_iter().map(|(mut item, pidl)| {
                item.pos = self.drop_item_position(&pidl)?;
                Some((item, pidl))
            }).collect::<Option<Vec<_>>>()
        }) {
            (entries, "cached_shell")
        } else {
            let entries = unsafe { self.entries().into_iter().map(|(path, pt, pidl)| {
                let item = ViewItem {
                    label: self.name_with(pidl.0, 0).unwrap_or_else(|| path.clone()),
                    path,
                    pos: (pt.x, pt.y),
                };
                (item, pidl)
            }).collect() };
            (entries, "shell")
        };
        let items = entries.iter().map(|(item, _)| item.clone()).collect();
        let place = move |moves: &[(String, (i32, i32))]| unsafe {
            let ids = entries.iter().map(|(_, pidl)| pidl.bytes())
                .collect::<Option<std::collections::HashSet<_>>>();
            if self.position_from(moves, entries.iter().map(|(item, pidl)| (item.path.as_str(), pidl)))
                != moves.len()
            {
                return false;
            }
            let (Some(ids), Some(latest)) = (ids, self.enumerate_ids(2 /* SVGIO_ALLVIEW */, entries.len())) else {
                return false;
            };
            let current_ids = latest.iter().map(Pidl::bytes)
                .collect::<Option<std::collections::HashSet<_>>>();
            if ids.len() != entries.len() || latest.len() != entries.len() || current_ids.as_ref() != Some(&ids) {
                return false;
            }
            moves.iter().all(|(path, target)| {
                let Some((_, pidl)) = entries.iter().find(|(item, _)| item.path.eq_ignore_ascii_case(path)) else {
                    return false;
                };
                self.drop_item_position(pidl) == Some(*target)
            })
        };
        (items, place, source)
    }

    /// Move desktop icons (by full path) to screen positions.
    pub fn position(&self, moves: &[(String, (i32, i32))]) -> usize {
        if moves.is_empty() {
            return 0;
        }
        unsafe {
            let entries = self.entries_for_position();
            self.position_from(moves, entries.iter().map(|(path, pidl)| (path.as_str(), pidl)))
        }
    }

    unsafe fn position_from<'a>(
        &self,
        moves: &[(String, (i32, i32))],
        entries: impl Iterator<Item = (&'a str, &'a Pidl)>,
    ) -> usize {
        let by_path = entries.map(|(path, pidl)| (path.to_lowercase(), pidl))
            .collect::<std::collections::HashMap<_, _>>();
        let mut pidls = Vec::with_capacity(moves.len());
        let mut points = Vec::with_capacity(moves.len());
        for (path, (x, y)) in moves {
            if let Some(pidl) = by_path.get(&path.to_lowercase()) {
                let mut pt = POINT { x: *x, y: *y };
                ScreenToClient(self.list, &mut pt);
                pidls.push(pidl.0 as *const c_void);
                points.push(pt);
            }
        }
        if pidls.is_empty() {
            LAST_POSITION.with(|c| c.set((0, moves.len(), 0)));
            return 0;
        }
        let f: extern "system" fn(
            Ptr,
            u32,
            *const *const c_void,
            *const POINT,
            u32,
        ) -> HRESULT = mem::transmute(vfn(self.view.0, 16));
        // SVSI_POSITIONITEM
        let hr = f(
            self.view.0,
            pidls.len() as u32,
            pidls.as_ptr(),
            points.as_ptr(),
            0x80,
        );
        LAST_POSITION.with(|c| c.set((hr, moves.len(), pidls.len())));
        if hr >= 0 {
            pidls.len()
        } else {
            0
        }
    }
    /// (HRESULT, moves asked, items matched) of the last position() call.
    pub fn last_position_result() -> (i32, usize, usize) {
        LAST_POSITION.with(|c| c.get())
    }
    /// Current screen position of one icon.
    pub fn position_of(&self, path: &str) -> Option<(i32, i32)> {
        unsafe {
            self.entries()
                .into_iter()
                .find(|(p, _, _)| p.eq_ignore_ascii_case(path))
                .map(|(_, pt, _)| (pt.x, pt.y))
        }
    }
    unsafe fn flags(&self) -> Option<(Com, u32)> {
        let v2 = self.view.query(&IID_IFOLDER_VIEW2);
        if !v2.ok() {
            return None;
        }
        let mut flags = 0u32;
        let get: extern "system" fn(Ptr, *mut u32) -> HRESULT = mem::transmute(vfn(v2.0, 25));
        (get(v2.0, &mut flags) >= 0).then_some((v2, flags))
    }
    unsafe fn set_flags(&self, mask: u32, value: u32) -> bool {
        let Some((v2, _)) = self.flags() else {
            return false;
        };
        let set: extern "system" fn(Ptr, u32, u32) -> HRESULT = mem::transmute(vfn(v2.0, 24));
        set(v2.0, mask, value) >= 0
    }
    /// Prevent Explorer from automatically rearranging desktop icons,
    /// but NEVER change the person's Align icons to grid setting.
    /// DesktopCraft snaps its own drawn loose icons during manual drops.
    pub fn disable_auto_arrange(&self) -> bool {
        unsafe {
            match self.flags() {
                Some((_, flags)) if flags & 0x1 != 0 => self.set_flags(0x1, 0),
                _ => false,
            }
        }
    }
    pub fn snap_to_grid(&self) -> Option<bool> {
        // LVS_EX_SNAPTOGRID is an extended ListView style. The same bit
        // in IFolderView2's FOLDERFLAGS means FWF_NOENUMREFRESH.
        const LVM_GETEXTENDEDLISTVIEWSTYLE: u32 = 0x1000 + 55;
        const LVS_EX_SNAPTOGRID: usize = 0x0008_0000;
        const FWF_SNAPTOGRID: u32 = 0x0000_0004;
        let mut style = 0;
        unsafe {
            let (_, flags) = self.flags()?;
            if SendMessageTimeoutW(self.list, LVM_GETEXTENDEDLISTVIEWSTYLE, 0, 0,
                SMTO_ABORTIFHUNG, 250, &mut style) == 0 {
                return Some(flags & FWF_SNAPTOGRID != 0);
            }
            Some(style & LVS_EX_SNAPTOGRID != 0 || flags & FWF_SNAPTOGRID != 0)
        }
    }
    /// Toggle Explorer's global "Align icons to grid" setting. Used by
    /// Native Desktop two-grid mode (docs/decisions/0001-native-two-grid-placement.md):
    /// disabled while Boxes are active, and restored on exit or recovery.
    pub fn set_snap_to_grid(&self, enabled: bool) -> bool {
        const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = 0x1000 + 54;
        const LVS_EX_SNAPTOGRID: usize = 0x0008_0000;
        const FWF_SNAPTOGRID: u32 = 0x0000_0004;
        unsafe {
            let _ = self.set_flags(FWF_SNAPTOGRID, if enabled { FWF_SNAPTOGRID } else { 0 });
            let mut out = 0usize;
            let value = if enabled { LVS_EX_SNAPTOGRID as isize } else { 0 };
            let _ = SendMessageTimeoutW(
                self.list,
                LVM_SETEXTENDEDLISTVIEWSTYLE,
                LVS_EX_SNAPTOGRID,
                value,
                SMTO_ABORTIFHUNG,
                250,
                &mut out,
            );
        }
        self.snap_to_grid() == Some(enabled)
    }
    pub fn auto_arrange(&self) -> Option<bool> {
        unsafe { self.flags().map(|(_, f)| f & 0x1 != 0) }
    }
    /// Use Explorer's own Auto arrange icons setting, not a custom sorter.
    pub fn enable_auto_arrange(&self) -> bool {
        self.auto_arrange() == Some(true)
            || (unsafe { self.set_flags(0x1, 0x1) } && self.auto_arrange() == Some(true))
    }
    /// Explorer's "Show desktop icons" (FWF_NOICONS), used by quick-hide.
    pub fn set_icons_visible(&self, visible: bool) -> bool {
        unsafe { self.set_flags(0x1000, if visible { 0 } else { 0x1000 }) }
    }
    pub fn icons_visible(&self) -> Option<bool> {
        unsafe { self.flags().map(|(_, f)| f & 0x1000 == 0) }
    }
    /// Read one COM snapshot for the layout and recovery arrangement gates.
    pub(crate) fn native_placement_settings(&self, require_visible: bool) -> bool {
        unsafe { self.flags() }.is_some_and(|(_, flags)| {
            flags & 0x1 == 0 && (!require_visible || flags & 0x1000 == 0)
        })
    }
    /// Explorer's own desktop icon spacing (physical pixels).
    pub fn spacing(&self) -> Option<(i32, i32)> {
        unsafe {
            let mut pt = POINT { x: 0, y: 0 };
            let f: extern "system" fn(Ptr, *mut POINT) -> HRESULT =
                mem::transmute(vfn(self.view.0, 12));
            (f(self.view.0, &mut pt) >= 0 && pt.x > 0 && pt.y > 0).then_some((pt.x, pt.y))
        }
    }
    /// Explorer's logical icon bitmap size via IFolderView2::GetViewModeAndIconSize.
    pub fn icon_size(&self) -> Option<i32> {
        unsafe {
            let v2 = self.view.query(&IID_IFOLDER_VIEW2);
            if !v2.ok() {
                return None;
            }
            let mut mode = 0i32;
            let mut size = 0i32;
            let get: extern "system" fn(Ptr, *mut i32, *mut i32) -> HRESULT =
                mem::transmute(vfn(v2.0, 36));
            (get(v2.0, &mut mode, &mut size) >= 0 && (16..=256).contains(&size))
                .then_some(size)
        }
    }
    pub fn list_window(&self) -> HWND {
        self.list
    }

    /// Bridge phase 1: resolve a real Explorer-owned Desktop icon to its
    /// IFolderView item index. This is read-only: no selection or position
    /// is changed. A Box pin whose file is not on the Windows Desktop is
    /// not represented by this Shell View and must not be faked here.
    fn index_of(&self, path: &str) -> Option<i32> {
        unsafe {
            let mut count = 0i32;
            let item_count: extern "system" fn(Ptr, u32, *mut i32) -> HRESULT =
                mem::transmute(vfn(self.view.0, 7));
            if item_count(self.view.0, 0, &mut count) < 0 || !(0..=20000).contains(&count) {
                return None;
            }
            let item: extern "system" fn(Ptr, i32, *mut Ptr) -> HRESULT =
                mem::transmute(vfn(self.view.0, 6));
            for index in 0..count {
                let mut pidl = ptr::null_mut();
                if item(self.view.0, index, &mut pidl) < 0 || pidl.is_null() {
                    continue;
                }
                let pidl = Pidl(pidl);
                if self
                    .name(pidl.0)
                    .is_some_and(|p| p.eq_ignore_ascii_case(path))
                {
                    return Some(index);
                }
            }
            None
        }
    }

    /// Opt-in Explorer-first prototype: selection uses Microsoft's
    /// IFolderView::SelectItem, not input injection, undocumented Shell
    /// hooks, Explorer code injection or icon repositioning. The selection
    /// is the only side effect, and only an explicit CLI command calls it.
    pub fn select_explorer_icon(&self, path: &str) -> Result<(), String> {
        let index = self
            .index_of(path)
            .ok_or("This file is not an icon in Explorer's Desktop View")?;
        unsafe {
            let f: extern "system" fn(Ptr, i32, u32) -> HRESULT =
                mem::transmute(vfn(self.view.0, 15));
            // SELECT | DESELECTOTHERS | FOCUSED | SELECTIONMARK
            let hr = f(self.view.0, index, 0x1 | 0x4 | 0x10 | 0x40);
            if hr < 0 {
                return Err(format!(
                    "IFolderView::SelectItem failed (0x{:08X})",
                    hr as u32
                ));
            }
        }
        Ok(())
    }

    /// Experimental: send a *screen-coordinate* context request for the
    /// selected real Explorer Desktop item. Keyboard-style LPARAM=-1
    /// was verified on 2026-11-16 to show the Desktop BACKGROUND menu,
    /// even after IFolderView::SelectItem selected a file. Never use
    /// -1 as a substitute for the file's actual context-menu location.
    ///
    /// This preserves Explorer ownership of the menu. It never draws a
    /// fake Windows 11 menu and does not inject any native DLL.
    pub fn request_explorer_context_menu(&self, path: &str) -> Result<(), String> {
        if self.icons_visible() != Some(true) || unsafe { IsWindowVisible(self.list) } == 0 {
            return Err(
                "Explorer Desktop icons are hidden; modern-menu bridge is unavailable".into(),
            );
        }
        let (x, y) = self.position_of(path)
            .ok_or("Target is not a real Explorer Desktop icon")?;
        // IFolderView::GetItemPosition returns the native icon's origin,
        // not the item's visible center. The offset aims within a normal
        // Windows Desktop icon (including standard 32/48 px icon sizes).
        let coords=native_icon_menu_point(x,y)
            .ok_or("Native icon coordinates are offscreen or outside signed 16-bit WM_CONTEXTMENU limits")?;
        if unsafe { MonitorFromPoint(POINT { x: x + 24, y: y + 24 }, MONITOR_DEFAULTTONULL) }.is_null() {
            return Err("Native icon is outside connected monitors".into());
        }
        self.select_explorer_icon(path)?;
        unsafe {
            if PostMessageW(self.list, WM_CONTEXTMENU, self.list as usize, coords) == 0 {
                return Err("Explorer Desktop View rejected the positioned context-menu request".into());
            }
        }
        Ok(())
    }

    pub fn explorer_bridge_probe(&self, target: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "bridge":"explorer-desktop-view-lab",
            "available":true,
            "icons_visible":self.icons_visible(),
            "item_count":self.items().len(),
            "target_resolved":target.map(|p|self.index_of(p).is_some()),
            "select_item_supported":true,
            "menu_dispatch_experimental":true,
            "explorer_code_injection":false,
            "icon_positions_modified":false,
            "limitation":"Selection is documented; modern Windows 11 menu dispatch is experimental and not guaranteed."
        })
    }
}
