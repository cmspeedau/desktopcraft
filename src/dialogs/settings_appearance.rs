use super::*;

impl App {
    pub(super) fn settings_page_appearance(&mut self, p: &mut Painter, input: &Input, x: f32, y: f32, w: f32, target: &mut usize, close: &mut bool) {
        // Appearance (with the original manager's Personalization).
        p.text("Appearance", x, y - 4., 23., TEXT);
        let names: Vec<(Option<u64>, String)> =
            std::iter::once((None, "All groups".to_string()))
                .chain(
                    self.state
                        .groups
                        .iter()
                        .map(|g| (Some(g.id), g.tab().name.clone())),
                )
                .collect();
        *target = (*target).min(names.len() - 1);
        let which = names[*target].0;
        let selected = which
            .and_then(|id| self.state.group(id))
            .or(self.state.groups.first());
        let (custom, preset, opacity) = selected
            .map_or((None, 0, 0.78), |g| (g.custom_color, g.color, g.opacity));
        // Colour presets: a clearly visible colour each.
        p.text("Group colour", x, y + 78., 14., MUTED);
        for (i, c) in COLORS.iter().enumerate() {
            let bb = Bounds {
                x: x + 110. + i as f32 * 40.,
                y: y + 72.,
                w: 28.,
                h: 28.,
            };
            round(bb, 6., *c);
            if i == preset && custom == Some(preset_color(i)) {
                outline(
                    Bounds {
                        x: bb.x - 3.,
                        y: bb.y - 3.,
                        w: 34.,
                        h: 34.,
                    },
                    TEXT,
                );
            }
            if input.pressed && bb.contains(input.pos.x, input.pos.y) {
                for g in self
                    .state
                    .groups
                    .iter_mut()
                    .filter(|g| which.is_none_or(|id| id == g.id))
                {
                    g.color = i;
                    g.custom_color = Some(preset_color(i));
                }
            }
        }
        if p.button(
            "Default",
            Bounds {
                x: x + 110. + 6. * 40.,
                y: y + 72.,
                w: 80.,
                h: 28.,
            },
            false,
        ) {
            for g in self
                .state
                .groups
                .iter_mut()
                .filter(|g| which.is_none_or(|id| id == g.id))
            {
                g.custom_color = None;
                g.opacity = 0.78;
            }
            if which.is_none() {
                self.state.prefs.blend_opacity = 0.25;
            }
        }
        // Use the actual preset colour for editable HSV values. The
        // old near-black default made Hue/Saturation appear broken
        // until Brightness was adjusted first.
        let base = custom.unwrap_or_else(|| preset_color(preset % 6));
        let (mut hue, mut sat, mut val) = rgb_to_hsv(base);
        let mut opacity = opacity;
        let mut colour_changed = false;
        let mut opacity_changed = false;
        // True hue spectrum, rather than a generic grey slider.
        p.text("Spectrum", x, y + 118., 14., MUTED);
        let spectrum = Bounds {
            x: x + 120.,
            y: y + 119.,
            w: w - 168.,
            h: 15.,
        };
        let bands = 120;
        for i in 0..bands {
            let t = i as f32 / bands as f32;
            let [r, g, b] = hsv_to_rgb(t, 1., 1.);
            draw_rectangle(
                spectrum.x + t * spectrum.w,
                spectrum.y,
                spectrum.w / bands as f32 + 0.5,
                spectrum.h,
                Color::new(r, g, b, 1.),
            );
        }
        draw_rectangle_lines(
            spectrum.x - 1.,
            spectrum.y - 1.,
            spectrum.w + 2.,
            spectrum.h + 2.,
            1.,
            MUTED,
        );
        draw_circle(
            spectrum.x + hue * spectrum.w,
            spectrum.y + spectrum.h / 2.,
            6.,
            TEXT,
        );
        let hue_hit = Bounds {
            x: spectrum.x - 7.,
            y: spectrum.y - 12.,
            w: spectrum.w + 14.,
            h: 38.,
        };
        if input.down && hue_hit.contains(input.pos.x, input.pos.y) {
            hue = ((input.pos.x - spectrum.x) / spectrum.w).clamp(0., 0.99999);
            colour_changed = true;
        }
        let opacity_label = if self.state.blend {
            "Hover opacity"
        } else {
            "Opacity"
        };
        for (i, (label, value)) in [
            ("Brightness", &mut val),
            ("Saturation", &mut sat),
            (opacity_label, &mut opacity),
        ]
        .into_iter()
        .enumerate()
        {
            let slider_w = if i == 2 { w - 86. } else { w - 40. };
            if let Some(v) = slider(
                p,
                input,
                label,
                *value,
                x,
                y + 148. + i as f32 * 27.,
                slider_w,
            ) {
                *value = v;
                if i == 2 {
                    opacity_changed = true;
                } else {
                    colour_changed = true;
                }
            }
        }
        p.text(
            &format!("{}%", (opacity * 100.).round() as u32),
            x + w - 70.,
            y + 206.,
            14.,
            MUTED,
        );
        let screen_rgb = hsv_to_rgb(hue, sat, val);
        let hex = format!(
            "#{:02X}{:02X}{:02X}",
            (screen_rgb[0] * 255.).round() as u8,
            (screen_rgb[1] * 255.).round() as u8,
            (screen_rgb[2] * 255.).round() as u8
        );
        round(
            Bounds {
                x: x + 260.,
                y: y + 247.,
                w: 24.,
                h: 22.,
            },
            4.,
            Color::new(screen_rgb[0], screen_rgb[1], screen_rgb[2], 1.),
        );
        p.text(&hex, x + 295., y + 249., 13., TEXT);
        if self.host.is_some() {
            if p.button(
                "Pick screen pixel",
                Bounds {
                    x,
                    y: y + 242.,
                    w: 180.,
                    h: 29.,
                },
                false,
            ) {
                self.color_pick_target = Some(*target);
                *close = true;
            }
        }
        if colour_changed || opacity_changed {
            let rgb = hsv_to_rgb(hue, sat, val);
            for g in self
                .state
                .groups
                .iter_mut()
                .filter(|g| which.is_none_or(|id| id == g.id))
            {
                if colour_changed {
                    g.custom_color = Some(rgb);
                }
                if opacity_changed {
                    g.opacity = opacity.clamp(0.0, 1.0);
                }
            }
        }
        p.text("Personalization", x, y + 282., 17., TEXT);
        p.text("Titlebars", x, y + 329., 14., MUTED);
        for (i, label) in ["Always visible", "On mouseover", "Never"]
            .iter()
            .enumerate()
        {
            if p.button(
                label,
                Bounds {
                    x: x + 110. + i as f32 * 140.,
                    y: y + 320.,
                    w: 132.,
                    h: 32.,
                },
                self.state.prefs.titlebar == i as u8,
            ) {
                self.state.prefs.titlebar = i as u8;
            }
        }
        if p.checkbox(
            "Tint icons with the group colour",
            Bounds {
                x,
                y: y + 367.,
                w,
                h: 32.,
            },
            self.state.tint,
        ) {
            self.state.tint = !self.state.tint;
        }
        if p.checkbox(
            "Hide scrollbars until mouseover",
            Bounds {
                x,
                y: y + 404.,
                w,
                h: 32.,
            },
            self.state.prefs.hide_scrollbars,
        ) {
            self.state.prefs.hide_scrollbars = !self.state.prefs.hide_scrollbars;
        }
        if p.checkbox(
            "Blend groups into the wallpaper until mouseover",
            Bounds {
                x,
                y: y + 441.,
                w,
                h: 32.,
            },
            self.state.blend,
        ) {
            self.state.blend = !self.state.blend;
        }
        if let Some(v) = slider(
            p,
            input,
            "Not hovered",
            self.state.prefs.blend_opacity.clamp(0., 1.),
            x,
            y + 480.,
            w - 86.,
        ) {
            self.state.prefs.blend_opacity = v.clamp(0., 1.);
            self.state.blend = true;
        }
        p.text(
            &format!("{}%", (self.state.prefs.blend_opacity.clamp(0., 1.) * 100.).round() as u32),
            x + w - 70.,
            y + 484.,
            14.,
            MUTED,
        );
        // Group chooser last, so its open list draws over the rest.
        p.text("Group", x, y + 40., 14., MUTED);
        let labels: Vec<String> = names.iter().map(|n| n.1.clone()).collect();
        if let Some(i) = self.dropdown(
            p,
            1,
            Bounds {
                x: x + 110.,
                y: y + 32.,
                w: 330.,
                h: 32.,
            },
            &labels,
            *target,
        ) {
            *target = i;
        }
    }
}
