use super::*;

impl App {
    pub(super) fn scroll_item_into_view(&mut self, id: u64, index: usize) {
        let Some(g) = self.state.group(id) else {
            return;
        };
        let tab = g.tab();
        let cols = if tab.list {
            1
        } else {
            ((g.bounds.w - 20.) / (cell_w() * tab.icon_scale))
                .floor()
                .max(1.) as usize
        };
        let content = g.bounds.h - HEADER - FOOTER - if tab.folder.is_some() { 38. } else { 12. };
        let rows = (content
            / if tab.list {
                36.
            } else {
                cell_h() * tab.icon_scale
            })
        .floor()
        .max(1.) as usize;
        let count = rows * cols;
        let next = if index < tab.scroll {
            index / cols * cols
        } else if index >= tab.scroll + count {
            (index / cols + 1).saturating_sub(rows) * cols
        } else {
            tab.scroll
        };
        self.state.group_mut(id).unwrap().tab_mut().scroll = next;
    }
    /// Start renaming a loose Desktop icon without renaming a previously
    /// selected Box. Special shell items without a real path are not editable.
    pub(crate) fn start_inline_rename_loose(&mut self, path: &str) {
        if !Path::new(path).exists() {
            return;
        }
        let Some(l) = self
            .host
            .as_ref()
            .and_then(|h| h.loose.iter().find(|l| l.path.eq_ignore_ascii_case(path)))
            .cloned()
        else {
            return;
        };
        let filename = Path::new(path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let edit = NameEdit::new(&l.label, &filename, Path::new(path).is_dir());
        self.close_popups();
        self.selected = None;
        self.multi_selected.clear();
        if let Some(h) = self.host.as_mut() {
            h.selected_loose = Some(path.to_string());
        }
        self.inline_rename = Some((0, 0, path.to_string(), edit));
    }
    /// Start renaming an icon in place, the way Explorer does (F2 or Rename in
    /// its right-click menu): the name, without a visible extension, starts selected.
    pub(crate) fn start_inline_rename(&mut self, group: u64, item: u64) {
        let Some(g) = self.state.group(group) else {
            return;
        };
        let Some((index, it)) = g.tab().items.iter().enumerate().find(|(_, i)| i.id == item) else {
            return;
        };
        let edit = NameEdit::new(it.display(), &it.name, it.kind == "folder");
        let path = it.path.clone();
        self.scroll_item_into_view(group, index);
        self.close_popups();
        self.selected = Some((group, item));
        self.multi_selected.clear();
        self.inline_rename = Some((group, item, path, edit));
    }
    /// Save an in-place rename (Enter, or clicking somewhere else, like Windows).
    pub(crate) fn commit_inline_rename(&mut self, time: f64) {
        let Some((_, _, path, edit)) = self.inline_rename.take() else {
            return;
        };
        self.inline_rename_rect = None;
        let Some(new_name) = edit.new_file_name() else {
            return;
        };
        if !self.rename_item(&path, &new_name, time) {
            return;
        }
        // Show the new name at once; Explorer's own label catches up later.
        let renamed = Path::new(&path)
            .with_file_name(&new_name)
            .to_string_lossy()
            .to_string();
        for g in &mut self.state.groups {
            for tab in &mut g.tabs {
                for it in &mut tab.items {
                    if it.path.eq_ignore_ascii_case(&renamed) && it.label.is_some() {
                        it.label = Some(edit.shown().to_string());
                    }
                }
            }
        }
    }
    /// Keys while renaming in place: typing edits the name, Enter saves,
    /// Escape cancels, and nothing else (like Delete on the icon) reacts.
    pub(super) fn inline_rename_typing(&mut self, input: &Input) -> bool {
        let Some((group, item, _, _)) = self.inline_rename.as_ref() else {
            return false;
        };
        let still_there = !self.has_blocking_dialog()
            && if *group == 0 {
                self.host.as_ref().is_some_and(|h| {
                    h.selected_loose.as_deref().is_some_and(|path| {
                        h.loose.iter().any(|l| l.path.eq_ignore_ascii_case(path))
                    })
                })
            } else {
                self.state
                    .group(*group)
                    .is_some_and(|g| g.tab().items.iter().any(|i| i.id == *item))
            };
        if !still_there {
            self.inline_rename = None;
            self.inline_rename_rect = None;
            return false;
        }
        if input.key(KeyCode::Escape) {
            self.inline_rename = None;
            self.inline_rename_rect = None;
            return true;
        }
        if input.key(KeyCode::Enter) || input.key(KeyCode::KpEnter) {
            self.commit_inline_rename(input.time);
            return true;
        }
        let paste = if input.ctrl && input.key(KeyCode::V) {
            macroquad::miniquad::window::clipboard_get().unwrap_or_default()
        } else {
            String::new()
        };
        let edit = &mut self.inline_rename.as_mut().unwrap().3;
        let mut refused = false;
        if input.ctrl && input.key(KeyCode::A) {
            edit.select_all();
        } else if input.ctrl && input.key(KeyCode::C) {
            macroquad::miniquad::window::clipboard_set(&edit.text);
        } else if !paste.is_empty() {
            refused = !edit.type_text(&paste.replace(['\r', '\n'], ""));
        } else if !input.ctrl && !input.typed.is_empty() {
            refused = !edit.type_text(&input.typed);
        }
        if input.key_or_repeat(KeyCode::Backspace) {
            edit.backspace();
        }
        if input.key_or_repeat(KeyCode::Delete) {
            edit.delete();
        }
        if input.key_or_repeat(KeyCode::Left) {
            edit.left();
        }
        if input.key_or_repeat(KeyCode::Right) {
            edit.right();
        }
        if input.key(KeyCode::Home) {
            edit.home();
        }
        if input.key(KeyCode::End) {
            edit.end();
        }
        if refused {
            self.notify(
                "A file name can't contain any of these characters: \\ / : * ? \" < > |",
                input.time,
            );
        }
        true
    }
    /// Draw the in-place rename box over the icon's label, Windows-style:
    /// white box, dark text, the selected part highlighted in blue.
    pub(crate) fn draw_inline_rename(&mut self, p: &mut Painter, hits: &[Hit], time: f64) {
        self.inline_rename_rect = None;
        let Some((group, item, path, edit)) = self.inline_rename.as_ref() else {
            return;
        };
        let bounds = if *group == 0 {
            let index = self.host.as_ref().and_then(|h| {
                h.loose
                    .iter()
                    .position(|l| l.path.eq_ignore_ascii_case(path))
            });
            index
                .and_then(|index| {
                    hits.iter()
                        .rev()
                        .find(|h| h.target == Target::Loose(index))
                        .map(|h| h.bounds)
                })
                .map(|cell| {
                    let scale = self.host.as_ref().map_or(1., |h| h.scale);
                    let size = (48. / scale.max(1.)).min(cell.w - 16.).max(24.);
                    (12., cell.x - 6., cell.y + size + 5., cell.w + 12.)
                })
        } else {
            let cell = hits
                .iter()
                .rev()
                .find(|h| h.target == Target::Item(*group, *item))
                .map(|h| h.bounds);
            cell.and_then(|cell| {
                self.state.group(*group).map(|g| {
                    let tab = g.tab();
                    if tab.list {
                        (14., cell.x + 38., cell.y + 4., cell.w - 128.)
                    } else {
                        let size = (48. * tab.icon_scale)
                            .min(cell.w - 12.)
                            .min(cell.h - 36.)
                            .max(16.);
                        (12., cell.x - 6., cell.y + size + 5., cell.w + 12.)
                    }
                })
            })
        };
        let Some((font, x, y, w)) = bounds else {
            return;
        };
        let line_h = font + 4.;
        let mut lines = crate::dialogs::wrap_text(p, &edit.text, font, w - 8.);
        if lines.is_empty() {
            lines.push(String::new());
        }
        let b = Bounds {
            x,
            y,
            w,
            h: lines.len() as f32 * line_h + 6.,
        };
        draw_rectangle(b.x, b.y, b.w, b.h, WHITE);
        draw_rectangle_lines(b.x, b.y, b.w, b.h, 1., Color::new(0.0, 0.33, 0.6, 1.));
        let selected = edit.selected_chars();
        let dark = Color::new(0.05, 0.05, 0.05, 1.);
        let mut offset = 0;
        let mut caret_drawn = false;
        for (n, line) in lines.iter().enumerate() {
            let (lx, ly) = (b.x + 4., b.y + 3. + n as f32 * line_h);
            let count = line.chars().count();
            let in_line = selected.saturating_sub(offset).min(count);
            if in_line > 0 {
                // Selected part white on blue, then the rest right after it
                // (so a narrow "." beside the highlight stays visible).
                let picked: String = line.chars().take(in_line).collect();
                let rest: String = line.chars().skip(in_line).collect();
                let pw = p.measure(&picked, font);
                draw_rectangle(lx, ly - 1., pw, line_h, Color::new(0.0, 0.47, 0.84, 1.));
                p.text(&picked, lx, ly, font, WHITE);
                p.text(&rest, lx + pw, ly, font, dark);
            } else {
                p.text(line, lx, ly, font, dark);
            }
            if edit.selection == NameSelection::None
                && !caret_drawn
                && edit.caret >= offset
                && edit.caret <= offset + count
                && (time * 2.).fract() < 0.65
            {
                let before: String = line.chars().take(edit.caret - offset).collect();
                let cx = lx + p.measure(&before, font);
                draw_line(cx, ly - 1., cx, ly + font + 1., 1., dark);
                caret_drawn = true;
            }
            offset += count;
        }
        p.push_region(b, 0.);
        self.inline_rename_rect = Some(b);
    }
}
