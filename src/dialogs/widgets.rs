use crate::*;

impl App {
    pub(crate) fn text_edit(&mut self, value: &mut String, input: &Input) {
        if input.ctrl && input.key(KeyCode::A) {
            self.select_all = true;
        }
        if input.ctrl && input.key(KeyCode::C) {
            macroquad::miniquad::window::clipboard_set(value);
        }
        let paste = if input.ctrl && input.key(KeyCode::V) {
            macroquad::miniquad::window::clipboard_get().unwrap_or_default()
        } else {
            String::new()
        };
        let text = if !paste.is_empty() {
            paste
        } else if input.ctrl {
            String::new()
        } else {
            input.typed.clone()
        };
        if !text.is_empty() {
            if self.select_all {
                value.clear();
                self.select_all = false
            }
            value.push_str(&text.replace(['\n', '\r'], ""));
        }
        // Holding Backspace keeps deleting at the keyboard's repeat rate.
        if input.key_or_repeat(KeyCode::Backspace) {
            if self.select_all {
                value.clear();
                self.select_all = false
            } else {
                value.pop();
            }
        }
    }
    pub(super) fn text_field(
        &mut self,
        p: &Painter,
        input: &Input,
        label: &str,
        value: &mut String,
        b: Bounds,
        id: usize,
    ) {
        p.text(label, b.x, b.y - 24., 13., MUTED);
        round(b, 5., Color::new(0.04, 0.075, 0.12, 1.));
        outline(
            b,
            if self.edit_focus == id {
                ACCENT
            } else {
                Color::new(0.24, 0.31, 0.40, 1.)
            },
        );
        if input.pressed && b.contains(input.pos.x, input.pos.y) {
            self.edit_focus = id;
            self.select_all = false;
        }
        if self.edit_focus == id {
            self.text_edit(value, input);
        }
        // While typing, a long value scrolls to show its end (where the caret
        // is); otherwise it shows its start.
        let text = if self.edit_focus == id {
            p.fit_tail(value, 16., b.w - 24.)
        } else {
            p.fit(value, 16., b.w - 24.)
        };
        p.text(&text, b.x + 12., b.y + 9., 16., TEXT);
        if self.edit_focus == id && (input.time * 2.).fract() < 0.65 {
            let x = (b.x + 12. + p.measure(&text, 16.)).min(b.x + b.w - 8.);
            draw_line(x, b.y + 10., x, b.y + 29., 1., ACCENT);
        }
    }
    /// A dropdown list: shows the current choice with ▾; clicking opens the list
    /// below it (drawn on top at the end of the dialog). Returns a new choice.
    pub(super) fn dropdown(
        &mut self,
        p: &Painter,
        id: u32,
        b: Bounds,
        options: &[String],
        current: usize,
    ) -> Option<usize> {
        let pick = match self.dropdown_pick {
            Some((pid, i)) if pid == id => {
                self.dropdown_pick = None;
                Some(i)
            }
            _ => None,
        };
        let label = options.get(current).cloned().unwrap_or_default();
        let open = self.dropdown_open == Some(id);
        round(
            b,
            6.,
            if p.hover(b) || open {
                Color::new(0.22, 0.30, 0.39, 1.)
            } else {
                Color::new(0.14, 0.20, 0.28, 0.96)
            },
        );
        p.text(
            &p.fit(&label, 15., b.w - 44.),
            b.x + 12.,
            b.y + (b.h - 15.) / 2. - 2.,
            15.,
            TEXT,
        );
        // ASCII arrow marker avoids missing-glyph squares on some Windows fonts.
        p.text(
            "v",
            b.x + b.w - 23.,
            b.y + (b.h - 15.) / 2. - 2.,
            15.,
            MUTED,
        );
        if p.hover(b) && p.click && self.dropdown_suppress != Some(id) {
            self.dropdown_open = if open { None } else { Some(id) };
            if !open {
                self.dropdown_scroll = current.saturating_sub(3);
            }
        }
        if self.dropdown_open == Some(id) {
            self.dropdown_list = Some((id, b, options.to_vec(), current));
        }
        pick
    }
    /// A multi-line text box: the whole value wraps onto as many lines as fit,
    /// and while typing the last lines (where the caret is) stay in view.
    pub(super) fn text_area(
        &mut self,
        p: &Painter,
        input: &Input,
        label: &str,
        value: &mut String,
        b: Bounds,
    ) {
        p.text(label, b.x, b.y - 24., 13., MUTED);
        round(b, 5., Color::new(0.04, 0.075, 0.12, 1.));
        let focused = self.edit_focus == 0;
        outline(
            b,
            if focused {
                ACCENT
            } else {
                Color::new(0.24, 0.31, 0.40, 1.)
            },
        );
        if input.pressed && b.contains(input.pos.x, input.pos.y) {
            self.edit_focus = 0;
            self.select_all = false;
        }
        if focused {
            self.text_edit(value, input);
        }
        let (size, line_h) = (16., 24.);
        let lines = wrap_text(p, value, size, b.w - 24.);
        let fit = (((b.h - 16.) / line_h).floor() as usize).max(1);
        let first = lines.len().saturating_sub(fit);
        for (row, line) in lines.iter().skip(first).enumerate() {
            p.text(line, b.x + 12., b.y + 9. + row as f32 * line_h, size, TEXT);
        }
        if first > 0 {
            // Plain text, not an arrow glyph some Windows fonts lack.
            let note = format!(
                "{first} more line{} above",
                if first == 1 { "" } else { "s" }
            );
            p.text(
                &note,
                b.x + b.w - p.measure(&note, 12.),
                b.y - 22.,
                12.,
                MUTED,
            );
        }
        if focused && (input.time * 2.).fract() < 0.65 {
            let last = lines.last().map(String::as_str).unwrap_or("");
            let row = lines.len() - 1 - first;
            let cx = (b.x + 12. + p.measure(last, size)).min(b.x + b.w - 8.);
            let cy = b.y + 10. + row as f32 * line_h;
            draw_line(cx, cy, cx, cy + 19., 1., ACCENT);
        }
    }
}

/// Break text into lines no wider than `width`, preferring to break after a
/// comma or space (rule lists), and inside a word only when it must.
pub(crate) fn wrap_text(p: &Painter, text: &str, size: f32, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for token in text.split_inclusive([',', ' ']) {
        let joined = format!("{line}{token}");
        if p.measure(&joined, size) <= width {
            line = joined;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for c in token.chars() {
            line.push(c);
            if p.measure(&line, size) > width && line.chars().count() > 1 {
                line.pop();
                lines.push(std::mem::take(&mut line));
                line.push(c);
            }
        }
    }
    lines.push(line);
    lines
}

/// A labelled 0..1 slider; returns the new value while it is being dragged.
pub(super) fn slider(
    p: &Painter,
    input: &Input,
    label: &str,
    value: f32,
    x: f32,
    y: f32,
    w: f32,
) -> Option<f32> {
    p.text(label, x, y + 4., 14., TEXT);
    let track = Bounds {
        x: x + 120.,
        y: y + 12.,
        w: w - 120.,
        h: 5.,
    };
    round(track, 2., Color::new(0.2, 0.28, 0.38, 1.));
    round(
        Bounds {
            w: (track.w * value).max(1.),
            ..track
        },
        2.,
        ACCENT,
    );
    draw_circle(track.x + track.w * value, track.y + 2.5, 7., TEXT);
    let grab = Bounds {
        x: track.x - 6.,
        y: track.y - 12.,
        w: track.w + 12.,
        h: 30.,
    };
    (input.down && grab.contains(input.pos.x, input.pos.y))
        .then(|| ((input.pos.x - track.x) / track.w).clamp(0., 1.))
}
