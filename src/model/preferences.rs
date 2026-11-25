use super::*;

fn yes() -> bool {
    true
}
fn default_blend_opacity() -> f32 {
    0.25
}
pub fn default_peek_shortcut() -> String {
    "Ctrl+Shift+Tab".to_string()
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Prefs {
    /// Titlebars: 0 always visible, 1 on mouseover, 2 never (manager "Titlebars").
    #[serde(default)]
    pub titlebar: u8,
    /// Personalization "Hide scrollbars until mouseover" (original default: on).
    #[serde(default = "yes")]
    pub hide_scrollbars: bool,
    /// Background alpha when "Blend groups into the wallpaper until mouseover" is active (0.0..=1.0).
    #[serde(default = "default_blend_opacity")]
    pub blend_opacity: f32,
    /// Roll-up: enabled, double-click titlebar toggles, how a rolled group opens
    /// (0 on mouseover immediately, 1 after a delay, 2 on click), and docking
    /// (roll up desktop groups at display edges).
    /// Default: rollup on, double-click title toggles, reveal on hover.
    #[serde(default = "yes")]
    pub rollup: bool,
    #[serde(default = "yes")]
    pub rollup_double_click: bool,
    #[serde(default)]
    pub rollup_open: u8,
    #[serde(default = "yes")]
    pub rollup_dock: bool,
    /// Page visibility and old workspace flags. Legacy flags below stay only
    /// for JSON compatibility: only the bottom selector and Ctrl/Alt+arrows
    /// navigate, and only the confirmed + action creates a page.
    #[serde(default = "yes")]
    pub pages: bool,
    #[serde(default = "yes")]
    pub show_page_bar: bool,
    #[serde(default)]
    pub page_bar_offset_y: f32,
    #[serde(default = "yes")]
    pub page_edge: bool,
    #[serde(default = "yes")]
    pub page_keys: bool,
    #[serde(default = "yes")]
    pub page_middle: bool,
    /// Empty = follow Windows primary; otherwise use this monitor device key
    /// for the app's tools, dialogs, and new-group fallback placement.
    /// This never changes the Windows operating system's primary display.
    #[serde(default)]
    pub workspace_monitor: String,
    /// Peek can be toggled with a user-chosen Ctrl+Shift or Ctrl+Alt
    /// shortcut; never intercept Windows language switching (Win+Space).
    #[serde(default = "yes")]
    pub peek_hotkeys: bool,
    #[serde(default = "default_peek_shortcut")]
    pub peek_shortcut: String,
    #[serde(default)]
    pub peek_toggle: bool,
    /// After moving a pin to an inactive tab, show destination (true) or stay
    /// on the source tab (false). Stored per workspace for all drag routes.
    #[serde(default)]
    pub switch_tab_after_drop: bool,
    /// Allow dragging a tab header left/right to rearrange tab order.
    #[serde(default = "yes")]
    pub drag_tab_to_rearrange: bool,
    /// Prevent accidental moving, resizing, merging, or detaching of boxes.
    #[serde(default)]
    pub lock_layout: bool,
    /// Master switch for DesktopCraft desktop integration (boxes/hooks vs paused Settings-only).
    #[serde(default = "yes")]
    pub desktop_enabled: bool,
}
impl Default for Prefs {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}
