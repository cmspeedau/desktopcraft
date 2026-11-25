//! Pixel copies of desktop icons as Explorer draws them, for the native Box
//! drag preview: the system image list icon with its overlays (shortcut
//! arrow, cloud state, …) and the label in the icon-title font, wrapped the
//! way the desktop wraps it. Documented Win32/Shell calls only; no Explorer
//! code. See docs/NATIVE_DRAG_ICONS_OPTIONS_20261119.md (Option 1).

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{channel, Receiver, Sender};

type Rgba = (u32, u32, Vec<u8>);

pub enum ArtRequest {
    Icon(String),
    Label { text: String, width: i32, dpi: u32 },
}

pub enum ArtResult {
    Icon(String, Option<Rgba>),
    Label { text: String, width: i32, dpi: u32, art: Option<Rgba> },
}

#[repr(C)]
struct ShFileInfoW {
    icon: *mut c_void,
    index: i32,
    attributes: u32,
    display_name: [u16; 260],
    type_name: [u16; 80],
}
#[repr(C)]
struct BitmapInfoHeader {
    size: u32, width: i32, height: i32, planes: u16, bit_count: u16, compression: u32,
    size_image: u32, x_ppm: i32, y_ppm: i32, clr_used: u32, clr_important: u32,
}
#[repr(C)]
struct BitmapInfo { header: BitmapInfoHeader, colors: [u32; 1] }
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect { l: i32, t: i32, r: i32, b: i32 }
#[repr(C)]
struct Size { cx: i32, cy: i32 }
#[repr(C)]
struct BlendFunction { operation: u8, flags: u8, constant_alpha: u8, alpha_format: u8 }
#[repr(C)]
#[derive(Clone, Copy)]
struct LogFontW {
    height: i32, width: i32, escapement: i32, orientation: i32, weight: i32,
    italic: u8, underline: u8, strike_out: u8, char_set: u8, out_precision: u8,
    clip_precision: u8, quality: u8, pitch_and_family: u8, face_name: [u16; 32],
}
#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);

#[link(name = "shell32")]
extern "system" {
    fn SHGetFileInfoW(path: *const u16, attrs: u32, info: *mut ShFileInfoW, size: u32, flags: u32) -> usize;
    fn SHGetImageList(list: i32, iid: *const Guid, out: *mut *mut c_void) -> i32;
}
#[link(name = "comctl32")]
extern "system" {
    fn ImageList_DrawEx(list: *mut c_void, index: i32, dc: *mut c_void, x: i32, y: i32, w: i32, h: i32,
        bg: u32, fg: u32, style: u32) -> i32;
    fn ImageList_GetIconSize(list: *mut c_void, width: *mut i32, height: *mut i32) -> i32;
}
#[link(name = "user32")]
extern "system" {
    fn DestroyIcon(icon: *mut c_void) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, ini: u32) -> i32;
    fn SystemParametersInfoForDpi(action: u32, param: u32, value: *mut c_void, ini: u32, dpi: u32) -> i32;
    fn DrawTextW(dc: *mut c_void, text: *const u16, len: i32, rect: *mut Rect, format: u32) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
    fn DeleteDC(dc: *mut c_void) -> i32;
    fn CreateDIBSection(dc: *mut c_void, info: *const BitmapInfo, usage: u32, bits: *mut *mut c_void,
        section: *mut c_void, offset: u32) -> *mut c_void;
    fn SelectObject(dc: *mut c_void, object: *mut c_void) -> *mut c_void;
    fn DeleteObject(object: *mut c_void) -> i32;
    fn CreateFontIndirectW(font: *const LogFontW) -> *mut c_void;
    fn SetTextColor(dc: *mut c_void, color: u32) -> u32;
    fn SetBkMode(dc: *mut c_void, mode: i32) -> i32;
    fn GetTextExtentPoint32W(dc: *mut c_void, text: *const u16, len: i32, size: *mut Size) -> i32;
    fn GdiFlush() -> i32;
}
#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
}
#[link(name = "msimg32")]
extern "system" {
    fn AlphaBlend(dst: *mut c_void, x: i32, y: i32, w: i32, h: i32,
        src: *mut c_void, sx: i32, sy: i32, sw: i32, sh: i32, blend: BlendFunction) -> i32;
}

const SHGFI_ICON: u32 = 0x100;
const SHGFI_SYSICONINDEX: u32 = 0x4000;
const SHGFI_OVERLAYINDEX: u32 = 0x40;
const SHIL_LARGE: i32 = 0;
const SHIL_EXTRALARGE: i32 = 2;
const IID_IIMAGELIST: Guid = Guid(0x46EB5926, 0x582E, 0x4017, [0x9F, 0xDF, 0xE8, 0x99, 0x8D, 0xAA, 0x09, 0x50]);
const CLR_NONE: u32 = 0xFFFF_FFFF;
const ILD_TRANSPARENT: u32 = 0x1;
const SPI_GETICONTITLELOGFONT: u32 = 0x1F;
const AC_SRC_OVER: u8 = 0;
const AC_SRC_ALPHA: u8 = 1;
const DT_CENTER: u32 = 0x1;
const DT_SINGLELINE: u32 = 0x20;
const DT_NOPREFIX: u32 = 0x800;
const DT_END_ELLIPSIS: u32 = 0x8000;
const TRANSPARENT: i32 = 1;

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

/// A 32-bit top-down DIB selected into its own DC.
struct Canvas { dc: *mut c_void, bitmap: *mut c_void, old: *mut c_void, bits: *mut u8, w: i32, h: i32 }
impl Canvas {
    unsafe fn new(w: i32, h: i32) -> Option<Self> {
        let info = BitmapInfo {
            header: BitmapInfoHeader { size: std::mem::size_of::<BitmapInfoHeader>() as u32, width: w,
                height: -h, planes: 1, bit_count: 32, compression: 0, size_image: 0, x_ppm: 0,
                y_ppm: 0, clr_used: 0, clr_important: 0 },
            colors: [0],
        };
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() { return None; }
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, std::ptr::null_mut(), 0);
        if bitmap.is_null() || bits.is_null() { DeleteDC(dc); return None; }
        let old = SelectObject(dc, bitmap);
        std::ptr::write_bytes(bits as *mut u8, 0, (w * h * 4) as usize);
        Some(Self { dc, bitmap, old, bits: bits as *mut u8, w, h })
    }
    unsafe fn pixels(&self) -> &[u8] {
        GdiFlush();
        std::slice::from_raw_parts(self.bits, (self.w * self.h * 4) as usize)
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe { SelectObject(self.dc, self.old); DeleteObject(self.bitmap); DeleteDC(self.dc); }
    }
}

struct SystemImageList(*mut c_void);

impl SystemImageList {
    unsafe fn open(kind: i32) -> Option<Self> {
        let mut list = std::ptr::null_mut();
        let hr = SHGetImageList(kind, &IID_IIMAGELIST, &mut list);
        if hr < 0 || list.is_null() { return None; }
        Some(Self(list))
    }

    unsafe fn method(&self, index: usize) -> *const c_void {
        *(*(self.0.cast::<*const *const c_void>())).add(index)
    }

    unsafe fn size(&self) -> Option<(i32, i32)> {
        let (mut w, mut h) = (0, 0);
        let found = ImageList_GetIconSize(self.0, &mut w, &mut h) != 0;
        (found && w > 0 && h > 0).then_some((w, h))
    }

    unsafe fn overlay_image(&self, slot: u32) -> Option<i32> {
        // IImageList::GetOverlayImage, after the IUnknown methods.
        let get: extern "system" fn(*mut c_void, i32, *mut i32) -> i32 =
            std::mem::transmute(self.method(31));
        let mut index = -1;
        (get(self.0, slot as i32, &mut index) >= 0 && index >= 0).then_some(index)
    }

    unsafe fn draw(&self, index: i32, canvas: &Canvas, y: i32, overlay: u32) -> bool {
        ImageList_DrawEx(self.0, index, canvas.dc, 0, y, 0, 0, CLR_NONE, CLR_NONE,
            ILD_TRANSPARENT | (overlay << 8)) != 0
    }
}

impl Drop for SystemImageList {
    fn drop(&mut self) {
        unsafe {
            let release: extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(self.method(2));
            release(self.0);
        }
    }
}

unsafe fn icon_with_shortcut_badge(
    list: &SystemImageList, index: i32, overlay: u32, (w, h): (i32, i32),
) -> Option<Canvas> {
    let badges = SystemImageList::open(SHIL_LARGE)?;
    let (badge_w, badge_h) = badges.size()?;
    // The standard 12 px glyph is 13 px in the desktop's medium view.
    let enlarge = |size| (size * 13 + 6) / 12;
    let (draw_w, draw_h) = (enlarge(badge_w), enlarge(badge_h));
    if draw_w > w || draw_h >= h { return None; }
    let badge = badges.overlay_image(overlay)?;
    let canvas = Canvas::new(w, h)?;
    let badge_canvas = Canvas::new(badge_w, badge_h)?;
    if !list.draw(index, &canvas, 0, 0) || !badges.draw(badge, &badge_canvas, 0, 0) {
        return None;
    }
    let blend = BlendFunction { operation: AC_SRC_OVER, flags: 0,
        constant_alpha: 255, alpha_format: AC_SRC_ALPHA };
    if AlphaBlend(canvas.dc, 0, h - draw_h, draw_w, draw_h,
        badge_canvas.dc, 0, 0, badge_w, badge_h, blend) == 0 {
        return None;
    }
    Some(canvas)
}

/// Shell icon and overlay as straight-alpha RGBA for the drag texture.
pub fn icon_rgba(path: &str) -> Option<Rgba> {
    unsafe {
        let mut info: ShFileInfoW = std::mem::zeroed();
        let found = SHGetFileInfoW(wide(&path.replace('/', "\\")).as_ptr(), 0, &mut info,
            std::mem::size_of::<ShFileInfoW>() as u32,
            SHGFI_ICON | SHGFI_SYSICONINDEX | SHGFI_OVERLAYINDEX);
        if !info.icon.is_null() { DestroyIcon(info.icon); }
        if found == 0 { return None; }
        let index = info.index & 0x00FF_FFFF;
        let overlay = ((info.index as u32) >> 24) & 0xFF;
        let list = SystemImageList::open(SHIL_EXTRALARGE)?;
        let (w, h) = list.size()?;
        let shortcut = std::path::Path::new(path).extension().and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk") || ext.eq_ignore_ascii_case("url"));
        let compact = if shortcut && overlay != 0 {
            icon_with_shortcut_badge(&list, index, overlay, (w, h))
        } else { None };
        let canvas = compact.or_else(|| {
            let canvas = Canvas::new(w, h)?;
            list.draw(index, &canvas, 0, overlay).then_some(canvas)
        })?;
        Some((w as u32, h as u32, unpremultiply_bgra(canvas.pixels())))
    }
}

unsafe fn icon_title_logfont(dpi: u32) -> Option<LogFontW> {
    let mut font: LogFontW = std::mem::zeroed();
    let size = std::mem::size_of::<LogFontW>() as u32;
    let ptr = &mut font as *mut LogFontW as *mut c_void;
    let ok = if dpi > 0 {
        SystemParametersInfoForDpi(SPI_GETICONTITLELOGFONT, size, ptr, 0, dpi) != 0
            || SystemParametersInfoW(SPI_GETICONTITLELOGFONT, size, ptr, 0) != 0
    } else {
        SystemParametersInfoW(SPI_GETICONTITLELOGFONT, size, ptr, 0) != 0
    };
    if !ok {
        return None;
    }
    Some(font)
}

unsafe fn label_rgba_with_font(text: &str, width: i32, font: &LogFontW) -> Option<Rgba> {
    let line = (font.height.abs() * 5 / 4).max(12);
    let canvas = Canvas::new(width.max(8), line * 2)?;
    let gdi_font = CreateFontIndirectW(font);
    if gdi_font.is_null() { return None; }
    let old_font = SelectObject(canvas.dc, gdi_font);
    SetTextColor(canvas.dc, 0x00FF_FFFF);
    SetBkMode(canvas.dc, TRANSPARENT);
    let fits = |s: &[u16]| {
        let mut size = Size { cx: 0, cy: 0 };
        GetTextExtentPoint32W(canvas.dc, s.as_ptr(), s.len() as i32, &mut size);
        size.cx <= width
    };
    let chars: Vec<u16> = text.encode_utf16().collect();
    let (first, rest) = split_first_line(&chars, &fits);
    for (n, part) in [first, rest].into_iter().enumerate() {
        if part.is_empty() { continue; }
        // Use the full cell for wrapping. Give a clipped second line the
        // extra side padding visible on Explorer's desktop titles.
        let padding = if n == 1 && !fits(part) && width > line * 2 { (line + 1) / 2 } else { 0 };
        let mut rect = Rect { l: padding, t: n as i32 * line,
            r: width - padding, b: (n as i32 + 1) * line };
        let flags = DT_CENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS;
        DrawTextW(canvas.dc, part.as_ptr(), part.len() as i32, &mut rect, flags);
    }
    SelectObject(canvas.dc, old_font);
    DeleteObject(gdi_font);
    // GDI painted white text on black. Retain Windows' channel-specific
    // coverage so ClearType edges survive upload to the transparent preview.
    let rgba = canvas.pixels().chunks_exact(4)
        .flat_map(|px| straight_rgba(px, px[0].max(px[1]).max(px[2])))
        .collect();
    Some((canvas.w as u32, canvas.h as u32, rgba))
}

/// Explorer-style desktop label for a monitor at `dpi` (e.g. 96, 120):
/// line 1 is word-wrapped, line 2 holds the rest and ends with "…" when it
/// does not fit. Coverage follows the user's Windows font smoothing settings.
pub fn label_rgba(text: &str, width: i32, dpi: u32) -> Option<Rgba> {
    unsafe {
        let font = icon_title_logfont(dpi)?;
        label_rgba_with_font(text, width, &font)
    }
}

/// Wrap at a fitting word boundary. Keep an overlong first word on one line
/// for GDI's end ellipsis instead of breaking the filename across two lines.
fn split_first_line<'a>(text: &'a [u16], fits: &dyn Fn(&[u16]) -> bool) -> (&'a [u16], &'a [u16]) {
    if fits(text) { return (text, &[]); }
    let space = b' ' as u16;
    let mut best = None;
    for (i, &c) in text.iter().enumerate() {
        if c == space && i > 0 && fits(&text[..i]) { best = Some(i); }
    }
    let Some(cut) = best else { return (text, &[]) };
    let rest = &text[cut..];
    let skip = rest.iter().take_while(|&&c| c == space).count();
    (&text[..cut], &rest[skip..])
}

/// GDI/ImageList output is premultiplied BGRA; textures want straight RGBA.
fn unpremultiply_bgra(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4).flat_map(|px| straight_rgba(px, px[3])).collect()
}

fn straight_rgba(bgra: &[u8], alpha: u8) -> [u8; 4] {
    if alpha == 0 { return [0; 4]; }
    let a = u32::from(alpha);
    let un = |c: u8| ((u32::from(c) * 255 + a / 2) / a).min(255) as u8;
    [un(bgra[2]), un(bgra[1]), un(bgra[0]), alpha]
}

/// Renders icons and labels off the UI thread (Shell/GDI work never blocks frames).
pub struct ArtWorker {
    requests: Sender<ArtRequest>,
    pub results: Receiver<ArtResult>,
}
impl ArtWorker {
    pub fn start() -> Option<Self> {
        let (requests, inbox) = channel::<ArtRequest>();
        let (outbox, results) = channel();
        std::thread::Builder::new().name("desktopcraft-explorer-art".into()).spawn(move || {
            unsafe { CoInitializeEx(std::ptr::null_mut(), 0x2); } // apartment-threaded
            let mut fonts: HashMap<u32, LogFontW> = HashMap::new();
            for req in inbox {
                let res = match req {
                    ArtRequest::Icon(path) => {
                        let art = icon_rgba(&path);
                        ArtResult::Icon(path, art)
                    }
                    ArtRequest::Label { text, width, dpi } => {
                        let art = match fonts.get(&dpi).copied().or_else(|| {
                            let font = unsafe { icon_title_logfont(dpi)? };
                            fonts.insert(dpi, font);
                            Some(font)
                        }) {
                            Some(font) => unsafe { label_rgba_with_font(&text, width, &font) },
                            None => None,
                        };
                        ArtResult::Label { text, width, dpi, art }
                    }
                };
                if outbox.send(res).is_err() { break; }
            }
        }).ok()?;
        Some(Self { requests, results })
    }
    pub fn request(&self, path: &str) {
        let _ = self.requests.send(ArtRequest::Icon(path.to_owned()));
    }
    pub fn request_label(&self, text: &str, width: i32, dpi: u32) {
        let _ = self.requests.send(ArtRequest::Label {
            text: text.to_owned(),
            width,
            dpi,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn u(s: &str) -> Vec<u16> { s.encode_utf16().collect() }
    fn s(v: &[u16]) -> String { String::from_utf16_lossy(v) }

    #[test]
    fn labels_wrap_like_the_desktop() {
        // Pretend each character is 8 px and the cell fits 9 characters.
        let fits = |t: &[u16]| t.len() <= 9;
        let text = u("Mister Download Manager");
        let (a, b) = split_first_line(&text, &fits);
        assert_eq!((s(a), s(b)), ("Mister".into(), "Download Manager".into()));
        let text = u("Discord");
        assert_eq!(split_first_line(&text, &fits).1.len(), 0);
        let text = u("Workstation");
        let (a, b) = split_first_line(&text, &fits);
        assert_eq!((s(a), s(b)), ("Workstation".into(), "".into()));
    }

    #[test]
    fn premultiplied_pixels_become_straight_alpha() {
        assert_eq!(unpremultiply_bgra(&[0, 64, 128, 128]), vec![255, 128, 0, 128]);
        assert_eq!(unpremultiply_bgra(&[9, 9, 9, 0]), vec![0, 0, 0, 0]);
    }
}
