use super::*;

impl App {
    pub(crate) fn pointer(&mut self, input: &Input, hits: &[Hit]) {
        let hit = hits
            .iter()
            .rev()
            .find(|h| h.bounds.contains(input.pos.x, input.pos.y))
            .map(|h| h.target.clone());
        // In-place rename: a click inside its box keeps editing; a click
        // anywhere else saves it first (as Explorer does), then acts normally.
        if (input.pressed || input.right_pressed) && self.inline_rename.is_some() {
            if self
                .inline_rename_rect
                .is_some_and(|r| r.contains(input.pos.x, input.pos.y))
            {
                return;
            }
            self.commit_inline_rename(input.time);
        }
        // Explorer-owned icons are outside our window region and have no Hit.
        // Miniquad releases mouse capture before delivering this button-up.
        if input.right_released && self.drag.is_none()
            && hit.as_ref().is_some_and(|target| self.queue_classic_file_menu(target, input.pos))
        {
            return;
        }
        if input.pressed || input.right_pressed {
            if let Some(h) = self.host.as_mut() {
                h.selected_loose = None;
            }
        }
        // Right-clicks on tabs, headers and Box bodies remain inert.
        if input.right_pressed && hit.is_none() && self.host.is_none() && !self.state.prefs.lock_layout {
            self.drag = Some(Drag::Create { start: input.pos });
        }
        self.pointer_scroll(input, &hit);
        if self.pointer_press(input, hit) {
            return;
        }
        self.pointer_drag_update(input);
        self.pointer_drag_release(input, hits);
    }
}
