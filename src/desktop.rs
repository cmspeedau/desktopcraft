//! Desktop mode: groups that live on the real Windows desktop.
//!
//! How it fits together (all supported Windows interfaces, nothing injected):
//! - One borderless tool window owned by the shell's desktop spans every monitor
//!   and is clipped (SetWindowRgn) to the groups, so the rest is Explorer's own
//!   desktop: its icons, wallpaper, right-click menu and drag-and-drop.
//! - A group holds real items from the Desktop and Public Desktop folders. Each
//!   grouped item's real Explorer icon is moved (IFolderView) under its group, so
//!   the desktop stays organized even when this program is closed.
//! - Items in a group use Explorer's context menu, Windows drag-and-drop and
//!   undoable shell file operations.
//! - Layouts are kept per monitor arrangement and switch automatically.
use crate::*;
use std::collections::{BTreeMap, HashMap, HashSet};

mod native_restore;
mod rules;
mod monitor_layout;
mod synchronization;
mod pins_and_drop;
mod organize;
mod commands;
mod host_lifecycle;
mod docking;
mod loose_icons;
mod labels;

pub(crate) use native_restore::{
    PendingNativeLayoutRestore, validate_native_structural_edit, native_pins_differ,
    apply_non_pin_layout_from, native_layout_structure_matches, native_regular_pins_match,
    stage1_unpin_incompatible_tabs, stage2_apply_target_structure_preserving_pins,
};
pub(crate) use rules::{RuleSortScope, format_pin_destinations, format_rule_sort_toast};
pub use host_lifecycle::restore_explorer_icons;
pub use labels::{is_combining_mark, wrap_label};
use loose_icons::is_recycle_bin_shell_item;

#[cfg(test)]
use loose_icons::{snap_loose_icon, grid_phase, snap_loose_icon_phase};

/// A desktop icon outside any group, drawn by DesktopCraft while Explorer's
/// own icon layer is switched off ("Show desktop icons").
#[derive(Clone, Debug)]
pub struct Loose {
    pub path: String,
    /// Lowercased path cached at Explorer refresh, never recomputed each frame.
    pub key: String,
    pub label: String,
    /// Explorer's stored position (screen, physical pixels).
    pub pos: (i32, i32),
}

pub enum Pending {
    Drag {
        path: String,
    },
}

/// A classic Shell menu for an item drawn by DesktopCraft.
pub(crate) struct ClassicFileMenu {
    pub path: String,
    pub anchor: Option<(i32, i32)>,
    /// Group, stable Tab and item IDs for a regular virtual pin.
    pub pin: Option<(u64, u64, u64)>,
}

pub struct HostState {
    pub monitors: Vec<host::Monitor>,
    pub signature: String,
    pub virtual_rect: [i32; 4],
    pub scale: f32,
    pub folders: Vec<PathBuf>,
    pub last_sync: f64,
    pub last_shown: f64,
    pub region: Vec<[i32; 5]>,
    pub front: bool,
    pub popup_mouse_down: bool,
    pub taskbar_visible: bool,
    pub first_run: bool,
    pub pending: Option<Pending>,
    pub quit: bool,
    pub loose: Vec<Loose>,
    pub spacing: (i32, i32),
    pub selected_loose: Option<String>,
    pub marquee_selected: HashSet<String>,
}

impl HostState {
    pub fn new(first_run: bool) -> Self {
        Self {
            monitors: vec![],
            signature: String::new(),
            virtual_rect: [0, 0, 0, 0],
            scale: 1.0,
            folders: shell::desktop_folders(),
            last_sync: -10.,
            last_shown: 0.,
            region: vec![],
            front: false,
            popup_mouse_down: false,
            taskbar_visible: false,
            first_run,
            pending: None,
            quit: false,
            loose: vec![],
            spacing: (75, 98),
            selected_loose: None,
            marquee_selected: HashSet::new(),
        }
    }
}

impl App {
    fn _desktop_marker(&self) {}
}

#[cfg(test)]
mod desktop_refresh_tests;
