use super::*;

fn vk_to_keycode(vk: usize) -> Option<KeyCode> {
    Some(match vk as i32 {
        VK_BACK => KeyCode::Backspace,
        VK_RETURN => KeyCode::Enter,
        VK_ESCAPE => KeyCode::Escape,
        VK_TAB => KeyCode::Tab,
        VK_DELETE => KeyCode::Delete,
        VK_LEFT => KeyCode::Left,
        VK_RIGHT => KeyCode::Right,
        VK_UP => KeyCode::Up,
        VK_DOWN => KeyCode::Down,
        VK_HOME => KeyCode::Home,
        VK_END => KeyCode::End,
        VK_SPACE => KeyCode::Space,
        0x30 => KeyCode::Key0,
        0x31 => KeyCode::Key1,
        0x32 => KeyCode::Key2,
        0x33 => KeyCode::Key3,
        0x34 => KeyCode::Key4,
        0x35 => KeyCode::Key5,
        0x36 => KeyCode::Key6,
        0x37 => KeyCode::Key7,
        0x38 => KeyCode::Key8,
        0x39 => KeyCode::Key9,
        0x41 => KeyCode::A,
        0x42 => KeyCode::B,
        0x43 => KeyCode::C,
        0x44 => KeyCode::D,
        0x45 => KeyCode::E,
        0x46 => KeyCode::F,
        0x47 => KeyCode::G,
        0x48 => KeyCode::H,
        0x49 => KeyCode::I,
        0x4A => KeyCode::J,
        0x4B => KeyCode::K,
        0x4C => KeyCode::L,
        0x4D => KeyCode::M,
        0x4E => KeyCode::N,
        0x4F => KeyCode::O,
        0x50 => KeyCode::P,
        0x51 => KeyCode::Q,
        0x52 => KeyCode::R,
        0x53 => KeyCode::S,
        0x54 => KeyCode::T,
        0x55 => KeyCode::U,
        0x56 => KeyCode::V,
        0x57 => KeyCode::W,
        0x58 => KeyCode::X,
        0x59 => KeyCode::Y,
        0x5A => KeyCode::Z,
        VK_F1 => KeyCode::F1,
        VK_F2 => KeyCode::F2,
        v if v == VK_F10 => KeyCode::F10,
        v if v == VK_F12 => KeyCode::F12,
        _ => return None,
    })
}

pub(super) unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowInputState;
    let state = ptr.as_mut();
    match msg {
        WM_ERASEBKGND => 1,
        WM_SETCURSOR => {
            if (lparam & 0xFFFF) as isize == HTCLIENT {
                SetCursor(LoadCursorW(ptr::null_mut(), IDC_ARROW));
                return 1;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            if let Some(s) = state {
                if s.paint_ready && !s.paint_dc.is_null() && s.paint_w > 0 && s.paint_h > 0 {
                    BitBlt(dc, 0, 0, s.paint_w, s.paint_h, s.paint_dc, 0, 0, SRCCOPY);
                }
            }
            EndPaint(hwnd, &ps);
            0
        }
        WM_CLOSE => {
            if let Some(s) = state {
                s.close_requested = true;
            }
            0
        }
        WM_LBUTTONDOWN => {
            SetFocus(hwnd);
            SetCapture(hwnd);
            if let Some(s) = state {
                s.left_pressed = true;
                s.left_down = true;
            }
            0
        }
        WM_LBUTTONUP => {
            if GetCapture() == hwnd {
                ReleaseCapture();
            }
            if let Some(s) = state {
                s.left_released = true;
                s.left_down = false;
            }
            0
        }
        WM_RBUTTONDOWN => {
            SetFocus(hwnd);
            if let Some(s) = state {
                s.right_pressed = true;
                s.right_down = true;
            }
            0
        }
        WM_RBUTTONUP => {
            if let Some(s) = state {
                s.right_released = true;
                s.right_down = false;
            }
            0
        }
        WM_MBUTTONDOWN => {
            SetFocus(hwnd);
            if let Some(s) = state {
                s.middle_pressed = true;
                s.middle_down = true;
            }
            0
        }
        WM_MBUTTONUP => {
            if let Some(s) = state {
                s.middle_released = true;
                s.middle_down = false;
            }
            0
        }
        WM_CAPTURECHANGED | WM_KILLFOCUS => {
            if let Some(s) = state {
                s.left_down = false;
                s.right_down = false;
                s.middle_down = false;
                s.high_surrogate = None;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEWHEEL => {
            if let Some(s) = state {
                let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
                s.scroll += delta;
            }
            0
        }
        WM_SYSKEYDOWN => {
            if wparam as i32 == VK_F4 {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            if let Some(s) = state {
                if let Some(code) = vk_to_keycode(wparam) {
                    if (lparam & (1 << 30)) != 0 {
                        s.repeats.push(code);
                    } else {
                        s.keys.push(code);
                    }
                }
            }
            0
        }
        WM_KEYDOWN => {
            if let Some(s) = state {
                if let Some(code) = vk_to_keycode(wparam) {
                    if (lparam & (1 << 30)) != 0 {
                        s.repeats.push(code);
                    } else {
                        s.keys.push(code);
                    }
                }
            }
            0
        }
        WM_CHAR => {
            if let Some(s) = state {
                s.push_utf16_unit((wparam & 0xFFFF) as u16);
            }
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl SettingsWindow {
    pub fn drain_input(&mut self, time: f64, dragging_settings: bool) -> (Input, bool) {
        unsafe {
            let hwnd = self.hwnd;
            let Some(s) = self.state_mut() else {
                return (
                    Input {
                        pos: vec2(-10000.0, -10000.0),
                        time,
                        ..Default::default()
                    },
                    true,
                );
            };
            let mut cursor_pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut cursor_pt);
            let captured = GetCapture() == hwnd;
            let over = IsWindowVisible(hwnd) != 0
                && IsIconic(hwnd) == 0
                && (captured || dragging_settings || WindowFromPoint(cursor_pt) == hwnd);
            let pos = if over {
                let mut client = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                GetClientRect(hwnd, &mut client);
                ScreenToClient(hwnd, &mut cursor_pt);
                let cw = (client.right - client.left).max(1) as f32;
                let ch = (client.bottom - client.top).max(1) as f32;
                vec2(
                    cursor_pt.x as f32 * (SETTINGS_LOGICAL_W / cw),
                    cursor_pt.y as f32 * (SETTINGS_LOGICAL_H / ch),
                )
            } else {
                vec2(-10000.0, -10000.0)
            };
            let physical_left_down = (GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0;
            if (captured || dragging_settings) && s.left_down && !physical_left_down {
                s.left_down = false;
                s.left_released = true;
                if captured {
                    ReleaseCapture();
                }
            }
            let focused = GetForegroundWindow() == hwnd;
            let key_down = |vk: i32| focused && ((GetKeyState(vk) as u16 & 0x8000) != 0);
            let input = Input {
                pos,
                pressed: std::mem::take(&mut s.left_pressed),
                released: std::mem::take(&mut s.left_released),
                down: s.left_down || ((captured || dragging_settings) && physical_left_down),
                right_pressed: std::mem::take(&mut s.right_pressed),
                right_released: std::mem::take(&mut s.right_released),
                right_down: s.right_down,
                middle_pressed: std::mem::take(&mut s.middle_pressed),
                middle_released: std::mem::take(&mut s.middle_released),
                middle_down: s.middle_down,
                scroll: std::mem::replace(&mut s.scroll, 0.0),
                ctrl: key_down(VK_CONTROL),
                shift: key_down(VK_SHIFT),
                alt: key_down(VK_MENU),
                keys: std::mem::take(&mut s.keys),
                repeats: std::mem::take(&mut s.repeats),
                typed: std::mem::take(&mut s.typed),
                time,
            };
            let close_requested = std::mem::replace(&mut s.close_requested, false);
            (input, close_requested)
        }
    }
}
