use super::*;

/// Explorer's icon (or thumbnail) for a file, as RGBA pixels.
pub fn image(path: &str, size: i32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut factory = ptr::null_mut();
        if SHCreateItemFromParsingName(
            wide(&path.replace('/', "\\")).as_ptr(),
            ptr::null_mut(),
            &IID_IIMAGE_FACTORY,
            &mut factory,
        ) < 0
        {
            return None;
        }
        let factory = Com(factory);
        let mut bitmap = ptr::null_mut();
        let get: extern "system" fn(Ptr, SIZE, u32, *mut HBITMAP) -> HRESULT =
            mem::transmute(vfn(factory.0, 3));
        // SIIGBF_BIGGERSIZEOK: icons or thumbnails, as Explorer shows them.
        if get(factory.0, SIZE { cx: size, cy: size }, 0x1, &mut bitmap) < 0 || bitmap.is_null()
        {
            return None;
        }
        let mut bm: BITMAP = mem::zeroed();
        GetObjectW(
            bitmap as _,
            mem::size_of::<BITMAP>() as i32,
            &mut bm as *mut _ as _,
        );
        let (w, h) = (bm.bmWidth.max(1), bm.bmHeight.abs().max(1));
        let mut info: BITMAPINFO = mem::zeroed();
        info.bmiHeader.biSize = mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(ptr::null_mut());
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            h as u32,
            pixels.as_mut_ptr() as _,
            &mut info,
            DIB_RGB_COLORS,
        );
        ReleaseDC(ptr::null_mut(), dc);
        DeleteObject(bitmap as _);
        if lines == 0 {
            return None;
        }
        let has_alpha = pixels.chunks(4).any(|p| p[3] != 0);
        for p in pixels.chunks_mut(4) {
            p.swap(0, 2);
            if !has_alpha {
                p[3] = 255;
            } else if p[3] > 0 && p[3] < 255 {
                // The shell returns premultiplied alpha.
                for c in 0..3 {
                    p[c] = ((p[c] as u32 * 255) / p[3] as u32).min(255) as u8;
                }
            }
        }
        Some((w as u32, h as u32, pixels))
    }
}
