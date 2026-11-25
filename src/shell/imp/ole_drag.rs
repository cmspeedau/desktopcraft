use super::*;

// A minimal IDropSource: drop on button release, cancel on Escape.
#[repr(C)]
struct DropVtbl {
    query: unsafe extern "system" fn(Ptr, *const GUID, *mut Ptr) -> HRESULT,
    add_ref: unsafe extern "system" fn(Ptr) -> u32,
    release: unsafe extern "system" fn(Ptr) -> u32,
    query_continue: unsafe extern "system" fn(Ptr, i32, u32) -> HRESULT,
    feedback: unsafe extern "system" fn(Ptr, u32) -> HRESULT,
}
#[repr(C)]
struct DropSource {
    vtbl: *const DropVtbl,
}
unsafe extern "system" fn ds_query(this: Ptr, iid: *const GUID, out: *mut Ptr) -> HRESULT {
    let iid = &*iid;
    let same = |g: &GUID| {
        g.Data1 == iid.Data1
            && g.Data2 == iid.Data2
            && g.Data3 == iid.Data3
            && g.Data4 == iid.Data4
    };
    if same(&IID_IUNKNOWN) || same(&IID_IDROP_SOURCE) {
        *out = this;
        0
    } else {
        *out = ptr::null_mut();
        0x80004002u32 as i32
    }
}
unsafe extern "system" fn ds_ref(_: Ptr) -> u32 {
    1
}
unsafe extern "system" fn ds_continue(_: Ptr, escape: i32, keys: u32) -> HRESULT {
    if escape != 0 {
        0x00040101 // DRAGDROP_S_CANCEL
    } else if keys & 0x1 == 0 {
        0x00040100 // DRAGDROP_S_DROP
    } else {
        0
    }
}
unsafe extern "system" fn ds_feedback(_: Ptr, _: u32) -> HRESULT {
    0x00040102 // DRAGDROP_S_USEDEFAULTCURSORS
}
static DROP_VTBL: DropVtbl = DropVtbl {
    query: ds_query,
    add_ref: ds_ref,
    release: ds_ref,
    query_continue: ds_continue,
    feedback: ds_feedback,
};

/// Start a normal Windows drag of one file or folder (blocks until dropped).
/// Returns the drop effect (0 none, 1 copy, 2 move, 4 link).
pub fn drag(path: &str) -> u32 {
    unsafe {
        let Some((folder, child, _pidl)) = parent_and_child(path) else {
            return 0;
        };
        let mut data = ptr::null_mut();
        let get_ui: extern "system" fn(
            Ptr,
            HWND,
            u32,
            *const Ptr,
            *const GUID,
            *mut u32,
            *mut Ptr,
        ) -> HRESULT = mem::transmute(vfn(folder.0, 10));
        if get_ui(
            folder.0,
            ptr::null_mut(),
            1,
            &child,
            &IID_IDATA_OBJECT,
            ptr::null_mut(),
            &mut data,
        ) < 0
            || data.is_null()
        {
            return 0;
        }
        let data = Com(data);
        let mut source = DropSource { vtbl: &DROP_VTBL };
        let mut effect = 0u32;
        DoDragDrop(data.0, &mut source as *mut _ as Ptr, 1 | 2 | 4, &mut effect);
        effect
    }
}

