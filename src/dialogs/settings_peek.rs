use super::*;

impl App {
    pub(super) fn settings_page_peek(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32) {
        p.text("Peek", x, y, 23., TEXT);
        p.text(
            "Bring your groups above all windows for a moment.",
            x,
            y + 34.,
            14.,
            MUTED,
        );
        if p.checkbox(
            "Show groups with hotkeys",
            Bounds {
                x,
                y: y + 66.,
                w,
                h: 32.,
            },
            self.state.prefs.peek_hotkeys,
        ) {
            self.state.prefs.peek_hotkeys = !self.state.prefs.peek_hotkeys;
        }
        if p.checkbox(
            "Toggle Peek on shortcut press (uncheck to hold shortcut)",
            Bounds {
                x,
                y: y + 102.,
                w,
                h: 32.,
            },
            self.state.prefs.peek_toggle,
        ) {
            self.state.prefs.peek_toggle = !self.state.prefs.peek_toggle;
        }
        p.text("Peek shortcut", x, y + 146., 14., TEXT);
        p.text(&self.state.prefs.peek_shortcut, x + 9., y + 190., 17., TEXT);
        if p.button(
            if self.shortcut_recording {
                "Stop recording"
            } else {
                "Record shortcut"
            },
            Bounds {
                x: x + 178.,
                y: y + 183.,
                w: 164.,
                h: 40.,
            },
            false,
        ) {
            self.shortcut_recording = !self.shortcut_recording;
            self.shortcut_candidate = None;
            self.shortcut_recording_rejected = false;
        }
        if p.button(
            "Reset default",
            Bounds {
                x: x + 352.,
                y: y + 183.,
                w: 138.,
                h: 40.,
            },
            false,
        ) {
            self.state.prefs.peek_shortcut = default_peek_shortcut();
            self.shortcut_recording = false;
            self.shortcut_candidate = None;
            self.shortcut_recording_rejected = false;
        }
        if self.shortcut_recording {
            let held = platform::peek_pressed_keys();
            if held.len() > 3 {
                // A four-key press is not a valid recording.
                self.shortcut_recording_rejected = true;
                self.shortcut_candidate = None;
            } else if held.len() >= 2 && !self.shortcut_recording_rejected {
                if let Some(combo) = platform::describe_peek_keys(held.clone()) {
                    // Keep the longest simultaneous chord seen.
                    // Releasing one key from Z+X+C must not
                    // silently overwrite it with just X+Z.
                    let old_count = self
                        .shortcut_candidate
                        .as_ref()
                        .map_or(0, |candidate| candidate.split('+').count());
                    if held.len() >= old_count {
                        self.shortcut_candidate = Some(combo);
                    }
                }
            } else if held.is_empty() {
                if self.shortcut_recording_rejected {
                    self.shortcut_candidate = None;
                    self.shortcut_recording_rejected = false;
                } else if let Some(combo) = self.shortcut_candidate.take() {
                    self.state.prefs.peek_shortcut = combo;
                    self.shortcut_recording = false;
                }
            }
            let status = if self.shortcut_recording_rejected {
                "Too many keys. Release and try 2–3 keys."
            } else {
                self.shortcut_candidate
                    .as_deref()
                    .unwrap_or("Hold 2–3 keys")
            };
            p.text(
                &p.fit(&format!("{status}  Release keys to save.",), 14., w),
                x,
                y + 240.,
                14.,
                ACCENT,
            );
        } else {
            p.text(
                "Record any 2–3 keys, then release. Default: Ctrl+Shift+Tab.",
                x,
                y + 240.,
                13.,
                MUTED,
            );
        }
        p.text(
            if self.state.prefs.peek_toggle {
                "Press your Peek shortcut to toggle boxes in front; press again or Escape to return."
            } else {
                "Hold your Peek shortcut to preview boxes; release keys or press Escape to return."
            },
            x,
            y + 266.,
            13.,
            MUTED,
        );
        draw_peek_preview(
            p,
            Bounds {
                x,
                y: y + 306.,
                w: w - 18.,
                h: 222.,
            },
            input.time,
            self.state.prefs.peek_hotkeys,
            &self.state.prefs.peek_shortcut,
            self.peek,
        );
    }
}
