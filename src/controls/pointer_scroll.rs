use super::*;

impl App {
    pub(super) fn pointer_scroll(&mut self, input: &Input, hit: &Option<Target>) {
        // High-resolution Logitech free-spin wheels can send fractional
        // notches. Consume them smoothly without jumping dozens of rows or
        // switching pages for every tiny WM_MOUSEWHEEL packet.
        if input.scroll != 0. {
            let target = hit
                .as_ref()
                .map(target_group)
                .filter(|id| *id != 0 && self.state.group(*id).is_some());
            if self.wheel_target != target {
                self.wheel_target = target;
                self.wheel_remainder = 0.;
            }
            let delta = input.scroll.clamp(-8., 8.);
            self.wheel_remainder += delta;
            let notches = self.wheel_remainder.trunc() as i32;
            self.wheel_remainder -= notches as f32;
            if let Some(id) = target {
                let native_regular = self.native_env.chrome_only
                    && self
                        .state
                        .group(id)
                        .is_some_and(|g| g.tab().folder.is_none());
                if native_regular {
                    if !input.ctrl && notches != 0 {
                        self.queue_native_scroll(id, -notches);
                    }
                } else {
                    let g = self.state.group_mut(id).unwrap();
                    if input.ctrl {
                        g.tab_mut().icon_scale = (g.tab().icon_scale + delta * 0.1).clamp(0.7, 1.6);
                    } else if notches != 0 {
                        let tab = g.tab();
                        let cols = if tab.list {
                            1
                        } else {
                            ((g.bounds.w - 20.) / (cell_w() * tab.icon_scale))
                                .floor()
                                .max(1.) as usize
                        };
                        let content =
                            g.bounds.h - HEADER - FOOTER - if tab.folder.is_some() { 38. } else { 12. };
                        let rows = (content
                            / if tab.list {
                                36.
                            } else {
                                cell_h() * tab.icon_scale
                            })
                        .floor()
                        .max(1.) as usize;
                        let next = scroll_rows(tab.scroll, tab.items.len(), rows * cols, cols, notches);
                        g.tab_mut().scroll = next;
                    }
                }
            }
        }
    }
}
