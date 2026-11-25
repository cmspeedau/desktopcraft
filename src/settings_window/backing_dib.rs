use super::*;

/// Convert OpenGL RGBA bytes into Win32 32-bpp BGRX bytes (opaque alpha byte).
pub(crate) fn rgba_to_bgrx(rgba: &[u8], bgrx: &mut [u8]) {
    assert_eq!(rgba.len(), bgrx.len());
    assert_eq!(rgba.len() % 4, 0);
    for (src, dst) in rgba.chunks_exact(4).zip(bgrx.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = 0xFF;
    }
}

pub(super) struct BackingDib {
    pub(super) mem_dc: HDC,
    bitmap: HBITMAP,
    old_obj: *mut c_void,
    pub(super) bits: *mut u8,
    pub(super) pw: i32,
    pub(super) ph: i32,
}

impl BackingDib {
    pub(super) unsafe fn create(hwnd: HWND, pw: i32, ph: i32) -> Option<Self> {
        let pw = pw.max(1);
        let ph = ph.max(1);
        let screen_dc = GetDC(hwnd);
        if screen_dc.is_null() {
            return None;
        }
        let mem_dc = CreateCompatibleDC(screen_dc);
        ReleaseDC(hwnd, screen_dc);
        if mem_dc.is_null() {
            return None;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = pw;
        // Positive height matches bottom-up OpenGL FBO scanlines without Y-flipping.
        bmi.bmiHeader.biHeight = ph;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut raw_bits: *mut c_void = ptr::null_mut();
        let bitmap = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut raw_bits, ptr::null_mut(), 0);
        if bitmap.is_null() || raw_bits.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap as *mut c_void);
            }
            DeleteDC(mem_dc);
            return None;
        }
        let old_obj = SelectObject(mem_dc, bitmap as *mut c_void);
        Some(Self {
            mem_dc,
            bitmap,
            old_obj,
            bits: raw_bits.cast::<u8>(),
            pw,
            ph,
        })
    }
}

impl Drop for BackingDib {
    fn drop(&mut self) {
        unsafe {
            if !self.mem_dc.is_null() {
                if !self.old_obj.is_null() {
                    SelectObject(self.mem_dc, self.old_obj);
                }
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap as *mut c_void);
                }
                DeleteDC(self.mem_dc);
            }
        }
    }
}
