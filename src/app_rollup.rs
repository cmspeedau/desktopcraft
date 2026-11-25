use crate::*;

impl App {
    /// Roll-up "after a delay" and "on click": which rolled group is open. It
    /// closes again when the pointer leaves the group.
    pub(crate) fn active_native_drag_group(&self) -> Option<u64> {
        match &self.drag {
            Some(Drag::Group { id, before, .. }) | Some(Drag::Resize { id, before, .. })
                if self.state.group(*id).is_some_and(|g| g.bounds != *before) => Some(*id),
            _ => None,
        }
    }
    pub(crate) fn is_active_native_box_drag(&self) -> bool {
        self.active_native_drag_group().is_some()
    }
    pub(super) fn previewing_top_dock(&self) -> Option<u64> {
        self.top_dock_preview.filter(|group| {
            matches!(self.drag, Some(Drag::Group { id, .. }) if id == *group)
        })
    }
    pub(super) fn update_rollup_reveal(&mut self, input: &Input) {
        if !self.state.prefs.rollup {
            self.revealed = None;
            self.hover_rolled = None;
            return;
        }
        if matches!(self.drag, Some(Drag::Group { .. } | Drag::Resize { .. })) {
            return;
        }
        let chrome_native = self.host.is_some() && self.native_env.chrome_only;
        let chrome_native_live = chrome_native && !self.automation;
        let page = self.state.page;
        let dock_block = self.dock_reveal_block.filter(|id| {
            self.state.group(*id).is_some_and(|g| {
                g.page == page && g.rolled
                    && rollup_header_bounds(g, chrome_native).contains(input.pos.x, input.pos.y)
            })
        });
        self.dock_reveal_block = dock_block;
        let (mx, my) = if chrome_native_live { host::cursor() } else { (0, 0) };
        let over_other_app = chrome_native_live && host::over_other_app(mx, my);
        let under = if over_other_app {
            None
        } else {
            self.state
                .groups
                .iter()
                .rev()
                .filter(|g| g.page == page)
                .find(|g| {
                    // A rolled group is revealed ONLY while hovering its title/tabs.
                    // Never treat the invisible body underneath as a hover target.
                    rollup_header_bounds(g, chrome_native).contains(input.pos.x, input.pos.y)
                })
                .filter(|g| dock_block != Some(g.id))
                .map(|g| (g.id, g.rolled))
        };
        if chrome_native_live
            && self.revealed.is_some()
            && (host::mouse_left_down() || host::mouse_right_down())
        {
            self.rollup_interact_until = input.time + 0.60;
        }
        // A pointer starting inside the invisible rolled-up body NEVER opens
        // it. Once triggered from the title, however, keep the revealed
        // content usable while the pointer moves into that visible body or
        // while Windows Explorer owns a context menu / drag / inline rename.
        let still_inside_revealed = self.revealed.is_some_and(|id| {
            self.state.group(id).is_some_and(|g| {
                if g.page != page || !g.rolled {
                    return false;
                }
                let inside_bounds =
                    rollup_revealed_bounds(g, chrome_native).contains(input.pos.x, input.pos.y);
                let explorer_busy = chrome_native_live
                    && (input.time < self.rollup_interact_until
                        || ((!inside_bounds || over_other_app)
                            && host::explorer_interaction_active()));
                (inside_bounds && !over_other_app)
                    || explorer_busy
                    || self.menu.is_some_and(|(menu_id, _)| menu_id == id)
                    || matches!(
                        self.drag,
                        Some(Drag::Scroll { group, .. } | Drag::NativeScroll { group, .. })
                            if group == id
                    )
            })
        });
        // The expanded panel owns its full bounds until the pointer exits.
        // A lower stacked box's title may lie underneath this body; hovering
        // it must not steal the reveal (nor its delayed hover timer).
        if still_inside_revealed {
            self.hover_rolled = None;
            return;
        }
        self.revealed = None;
        let pointer_button_down =
            chrome_native_live && (host::mouse_left_down() || host::mouse_right_down());
        if should_hold_new_rollup_reveal(
            chrome_native_live,
            pointer_button_down,
            input.time,
            &mut self.rollup_reveal_hold_until,
        ) {
            self.hover_rolled = None;
            return;
        }
        match self.state.prefs.rollup_open {
            0 => {
                if let Some((id, true)) = under {
                    self.revealed = Some(id);
                }
            }
            1 => match (under, self.hover_rolled) {
                (Some((id, true)), Some((hid, since))) if hid == id => {
                    if input.time - since >= ROLLUP_DELAY {
                        self.revealed = Some(id);
                    }
                }
                (Some((id, true)), _) => {
                    self.hover_rolled = Some((id, input.time));
                    if self.revealed != Some(id) {
                        self.revealed = None;
                    }
                }
                _ => {
                    self.hover_rolled = None;
                }
            },
            2 => { /* Title click sets revealed; body hover does not trigger it. */ }
            _ => self.revealed = None,
        }
    }
}
