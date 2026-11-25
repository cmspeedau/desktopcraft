use super::*;

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DockEdge {
    Top,
    Bottom,
    Left,
    Right,
}

impl DockEdge {
    pub fn is_side(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    pub fn side_header(self, body: Bounds) -> Option<Bounds> {
        let x = match self {
            Self::Left => body.x - HEADER,
            Self::Right => body.x + body.w,
            _ => return None,
        };
        Some(Bounds { x, w: HEADER, ..body })
    }

    /// The editable Box body stays inside the monitor, with space for its header.
    pub fn body_area(self, work: Bounds) -> Bounds {
        match self {
            Self::Left => Bounds { x: work.x + HEADER, w: (work.w - HEADER).max(0.), ..work },
            Self::Right => Bounds { w: (work.w - HEADER).max(0.), ..work },
            Self::Top => Bounds { y: work.y + HEADER, h: (work.h - HEADER).max(0.), ..work },
            Self::Bottom => Bounds { h: (work.h - HEADER).max(0.), ..work },
        }
    }

    pub fn fit_body(self, body: Bounds, work: Bounds) -> Bounds {
        let area = self.body_area(work);
        let w = body.w.min(area.w);
        let h = body.h.min(area.h);
        let right = area.x + (area.w - w);
        let bottom = area.y + (area.h - h);
        let x = match self {
            Self::Left => area.x,
            Self::Right => right,
            _ => body.x.clamp(area.x, right),
        };
        let y = match self {
            Self::Top => area.y,
            Self::Bottom => bottom,
            _ => body.y.clamp(area.y, bottom),
        };
        Bounds { x, y, w, h }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Group {
    pub id: u64,
    pub bounds: Bounds,
    pub page: usize,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub color: usize,
    pub opacity: f32,
    pub rolled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dock: Option<DockEdge>,
    pub locked: bool,
    pub keep_visible: bool,
    /// Background colour chosen with the hue / brightness / saturation sliders
    /// (red, green, blue 0..1); None uses the preset colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_color: Option<[f32; 3]>,
}
impl Group {
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }
    pub fn tab_mut(&mut self) -> &mut Tab {
        let i = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[i]
    }
    /// Reserve the right-hand titlebar controls and a tab-cycling button
    /// when tabs are too numerous to be shown without overlapping controls.
    pub fn header_tabs(&self) -> (Vec<(usize, Bounds)>, Option<Bounds>) {
        self.header_tabs_in(self.bounds)
    }
    pub(crate) fn header_tabs_in(&self, b: Bounds) -> (Vec<(usize, Bounds)>, Option<Bounds>) {
        if self.tabs.len() <= 1 {
            return (vec![], None);
        }
        let total_width = (b.w - 105.).max(40.);
        let capacity = ((total_width / 64.).floor() as usize).max(1);
        let scrolling = self.tabs.len() > capacity;
        let tab_width = if scrolling {
            (total_width - 29.).max(35.)
        } else {
            total_width
        };
        let visible = ((tab_width / 64.).floor() as usize)
            .max(1)
            .min(self.tabs.len());
        let first = if scrolling {
            self.active
                .saturating_sub(visible - 1)
                .min(self.tabs.len() - visible)
        } else {
            0
        };
        let width = (tab_width / visible as f32).min(150.).max(1.);
        let mut tabs = Vec::with_capacity(visible);
        for i in first..first + visible {
            let x = b.x + 7. + (i - first) as f32 * width;
            tabs.push((
                i,
                Bounds {
                    x,
                    y: b.y + 6.,
                    w: (width - 3.).max(1.),
                    h: 30.,
                },
            ));
        }
        let next = scrolling.then_some(Bounds {
            x: b.x + b.w - 104.,
            y: b.y + 7.,
            w: 24.,
            h: 28.,
        });
        (tabs, next)
    }
    pub fn drop_tab_at(&self, x: f32, y: f32) -> Option<usize> {
        self.header_tabs()
            .0
            .into_iter()
            .find_map(|(index, bounds)| bounds.contains(x, y).then_some(index))
    }
}
