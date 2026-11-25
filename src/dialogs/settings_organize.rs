use super::*;

impl App {
    pub(super) fn settings_page_organize(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32, pattern: &mut String, mode: &mut u8, target: &mut usize, close: &mut bool, open_instead: &mut Option<Dialog>) {
        p.text("Put files in their place", x, y, 23., TEXT);
        p.text(
            "Rules organize pins. The first matching rule wins.",
            x,
            y + 39.,
            14.,
            MUTED,
        );
        let modes = RULE_MODES;

        let mode_options: Vec<String> =
            modes.iter().map(|label| label.to_string()).collect();
        if let Some(index) = self.dropdown(
            p,
            20,
            Bounds {
                x,
                y: y + 76.,
                w: 260.,
                h: 34.,
            },
            &mode_options,
            (*mode as usize).min(mode_options.len() - 1),
        ) {
            *mode = index as u8;
        }
        if *mode == 10 {
            *pattern = "folder".into();
        }
        if *mode != 10 {
            self.text_field(
                p,
                input,
                rule_field_label(*mode),
                pattern,
                // As wide as the row allows beside Add rule, so long
                // extension lists stay readable.
                Bounds {
                    x,
                    y: y + 145.,
                    w: w - 222.,
                    h: 38.,
                },
                0,
            );
        } else {
            p.text("All real folders on the Desktop", x, y + 155., 14., MUTED);
        }
        // Each regular tab is a distinct rule destination.
        let choices = self.rule_destinations();
        if !choices.is_empty() {
            *target = (*target).min(choices.len() - 1);
            let group_options: Vec<String> = choices
                .iter()
                .map(|(_, _, name)| format!("To: {name}"))
                .collect();
            if let Some(index) = self.dropdown(
                p,
                21,
                Bounds {
                    x: x + 276.,
                    y: y + 76.,
                    w: w - 276.,
                    h: 34.,
                },
                &group_options,
                *target,
            ) {
                *target = index;
            }
            if p.button(
                "Add rule",
                Bounds {
                    x: x + w - 212.,
                    y: y + 145.,
                    w: 110.,
                    h: 38.,
                },
                true,
            ) {
                if pattern.trim().is_empty() {
                    self.notify("Enter a match for this rule.", input.time)
                } else if *mode == 9 && size_range(pattern).is_none() {
                    self.notify("Enter two byte sizes, such as 100,200, with the smaller first.", input.time)
                } else if self.state.rules.len() >= MAX_RULES {
                    self.notify(
                        "All 50 rule slots are in use. Remove a rule first.",
                        input.time,
                    )
                } else {
                    self.state.rules.push(Rule {
                        mode: *mode,
                        pattern: pattern.trim().into(),
                        target: choices[*target].0,
                        target_tab: Some(choices[*target].1),
                        enabled: true,
                    });
                    pattern.clear();
                }
            }
        }
        let mut remove = None;
        let mut reorder = None;
        let mut edit = None;
        self.settings_offset = self
            .settings_offset
            .min(self.state.rules.len().saturating_sub(5));
        for i in self.settings_offset
            ..self.state.rules.len().min(self.settings_offset + 5)
        {
            let rowy = y + 211. + (i - self.settings_offset) as f32 * 43.;
            let rule = &self.state.rules[i];
            let dest = self
                .state
                .rule_destination(rule)
                .and_then(|(gid, tid)| {
                    self.state
                        .group(gid)
                        .and_then(|g| g.tabs.iter().find(|t| t.id == tid))
                        .map(|t| t.name.as_str())
                })
                .unwrap_or("Missing tab");
            let label = format!(
                "{}: {}  to  {}",
                modes.get(rule.mode as usize).unwrap_or(&"Rule"),
                rule.pattern,
                dest
            );
            p.text(&p.fit(&label, 14., w - 302.), x + 3., rowy + 9., 14., TEXT);
            if p.button(
                "Edit",
                Bounds {
                    x: x + w - 290.,
                    y: rowy + 2.,
                    w: 56.,
                    h: 30.,
                },
                false,
            ) {
                edit = Some(i);
            }
            // A binary rule state needs one visible toggle, not
            // a dropdown with only Enabled / Disabled choices.
            if p.button(
                if rule.enabled { "Enabled" } else { "Disabled" },
                Bounds {
                    x: x + w - 228.,
                    y: rowy + 2.,
                    w: 115.,
                    h: 30.,
                },
                rule.enabled,
            ) {
                self.state.rules[i].enabled = !rule.enabled;
            }
            if p.button(
                "Up",
                Bounds {
                    x: x + w - 100.,
                    y: rowy + 2.,
                    w: 38.,
                    h: 30.,
                },
                false,
            ) {
                reorder = Some(i)
            }
            if p.button(
                "×",
                Bounds {
                    x: x + w - 49.,
                    y: rowy + 2.,
                    w: 38.,
                    h: 30.,
                },
                false,
            ) {
                remove = Some(i)
            }
        }
        if let Some(i) = edit {
            // Edit pops out its own window (like renaming a box),
            // with room to show the whole rule.
            *open_instead = Some(self.rule_editor(i));
        }
        if let Some(i) = remove {
            self.state.rules.remove(i);
        }
        if let Some(i) = reorder {
            if i > 0 {
                self.state.rules.swap(i, i - 1);
            }
        }
        if self.state.rules.is_empty() {
            p.text(
                "No rules yet. Add one above to get started.",
                x,
                y + 225.,
                14.,
                MUTED,
            );
        }
        // Action row and rule-list paging have distinct vertical
        // space; neither the count nor the navigation buttons
        // can overlap the Organize button at narrower DPIs.
        if p.button(
            "Apply rules now",
            Bounds {
                x,
                y: y + 444.,
                w: 180.,
                h: 38.,
            },
            false,
        ) {
            if self.host.is_some() {
                self.request_desktop_rule_sort(
                    crate::desktop::RuleSortScope::AllIncludingPins,
                    input.time,
                );
            } else {
                let count = self.state.apply_rules();
                self.notify(format!("Applied rules to {count} item(s)"), input.time);
            }
        }
        if self.host.is_some()
            && p.button(
                "Organize desktop now",
                Bounds {
                    x: x + 196.,
                    y: y + 444.,
                    w: 210.,
                    h: 38.,
                },
                false,
            )
        {
            self.request_organize_desktop(input.time);
            *close = true;
        }

        if p.button(
            "‹",
            Bounds {
                x: x,
                y: y + 498.,
                w: 32.,
                h: 38.,
            },
            false,
        ) {
            self.settings_offset = self.settings_offset.saturating_sub(5);
        }
        if p.button(
            "›",
            Bounds {
                x: x + 44.,
                y: y + 498.,
                w: 32.,
                h: 38.,
            },
            false,
        ) {
            self.settings_offset = (self.settings_offset + 5)
                .min(self.state.rules.len().saturating_sub(5));
        }
        p.text(
            &format!("{} rules", self.state.rules.len()),
            x + 96.,
            y + 505.,
            13.,
            MUTED,
        );
    }
}
