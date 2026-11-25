mod rule_text;
pub use self::rule_text::{extension_matches, extension_of, filter_matches, fold, name_rule_matches, size_range, stem_of};
mod geometry;
pub use self::geometry::{Bounds, min_content, resize_bounds, scroll_row_at, scroll_rows, scroll_thumb};
mod item;
pub use self::item::{Item};
mod tab;
pub use self::tab::{Tab, read_folder_items};
mod name_edit;
pub use self::name_edit::{BAD_NAME_CHARS, NameSelection, NameEdit};
mod box_model;
pub use self::box_model::{DockEdge, Group};
mod sorting_rules;
pub use self::sorting_rules::{Rule};
mod workspace;
pub use self::workspace::{Snapshot, State, Placement};
mod preferences;
pub use self::preferences::{Prefs, default_peek_shortcut};
mod color;
pub use self::color::{hsv_to_rgb, rgb_to_hsv};
mod state_layout;
mod state_pages;
mod state_boxes;
mod state_pins;
mod state_rules;
mod state_snapshots;
mod state_lifecycle;
#[cfg(test)]
mod original_mechanics_tests;
use self::rule_text::modified_within;
use self::sorting_rules::best_destination_from_rules;

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub const HEADER: f32 = 42.0;
pub(crate) const MAX_WORKSPACE_BYTES: usize = 32 * 1024 * 1024;

// ---- Tunable implementation estimates (not verified original values) ----
// Distance the pointer must travel (strictly more than this) before a press
// on a group header or a file becomes a drag. No SM_CXDRAG use was found in
// the original; adjust here.
pub const DRAG_THRESHOLD: f32 = 5.0;
// Scrollbar opacity while the pointer is not over it (original: window alpha =
// Opacity*BaseOpacity/100, values not recovered).
pub const SCROLL_REST_OPACITY: f32 = 0.6;
// Drawn width of the scrollbar, and the wider area that accepts clicks.
pub const SCROLL_BAR_WIDTH: f32 = 5.0;
pub const SCROLL_HIT_WIDTH: f32 = 14.0;
// --------------------------------------------------------------------------

pub const REFRESH: f64 = 3.0;
// Fallback icon cell when Windows' desktop spacing can't be read (estimated).
pub const DEFAULT_CELL: (f32, f32) = (94.0, 90.0);
// Padding around icons and beneath folder-view navigation.
pub const CONTENT_SIDE: f32 = 10.0;
pub const CONTENT_TOP: f32 = 12.0;
pub const FOLDER_CONTENT_TOP: f32 = 38.0;
// Bottom margin below icon rows.
pub const FOOTER: f32 = 10.0;
// Delay global search filtering while the user is still typing.
pub const SEARCH_UPDATE_DELAY: f64 = 0.350;
// Minimum scrollbar thumb ensures it remains clickable.
pub const SCROLL_MIN_THUMB: f32 = 8.0;

/// Icon cell dimensions come from Windows desktop metrics in 96-DPI
/// logical pixels, with fallbacks for systems without Explorer.
pub fn cell() -> (f32, f32) {
    static CELL: std::sync::OnceLock<(f32, f32)> = std::sync::OnceLock::new();
    *CELL.get_or_init(|| {
        crate::platform::desktop_icon_spacing()
            // Clamp extreme system metrics so labels stay readable.
            .map(|(w, h)| (w.clamp(56.0, 200.0), h.clamp(64.0, 200.0)))
            .unwrap_or(DEFAULT_CELL)
    })
}
pub fn cell_w() -> f32 {
    cell().0
}
pub fn cell_h() -> f32 {
    cell().1
}

pub fn min_size(folder: bool) -> (f32, f32) {
    let (w, h) = min_content(cell());
    let top = if folder {
        FOLDER_CONTENT_TOP
    } else {
        CONTENT_TOP
    };
    (w + 2.0 * CONTENT_SIDE, HEADER + top + h + FOOTER)
}

// Visual and layout constants for this independent desktop organizer.
pub const GROUP_CORNER_RADIUS: f32 = 4.0;
pub const MAX_RULES: usize = 50;
// Alignment and overlap tolerances for drag snapping.
pub const SNAP_ALIGNMENT: f32 = 20.0;
pub const SNAP_OVERLAP: f32 = SNAP_ALIGNMENT * 2.0;
pub const SNAP_PADDING: f32 = 0.0;


#[cfg(test)]
#[test]
fn workspace_saves_portal_location_without_its_filesystem_listing() {
    let mut tab = Tab::new(11, "Files");
    let cached = Item {
        id: 30, name: "File.txt".into(), path: "C:/Files/File.txt".into(),
        kind: "txt".into(), size: 25, modified: 0, label: None,
    };
    tab.items.push(cached.clone());
    let pins: Tab = serde_json::from_slice(&serde_json::to_vec(&tab).unwrap()).unwrap();
    assert_eq!(pins.items, [cached.clone()]);

    tab.folder = Some("C:/Files".into());
    tab.history.push("C:/".into());
    let portal: Tab = serde_json::from_slice(&serde_json::to_vec(&tab).unwrap()).unwrap();
    assert!(portal.items.is_empty());
    assert_eq!(portal.folder, tab.folder);
    assert_eq!(portal.history, tab.history);
    assert_eq!(tab.items, [cached]);
}
#[cfg(test)]
mod name_edit_tests {
    use super::*;
    #[test]
    fn visible_extension_keeps_while_the_name_is_replaced() {
        let mut e = NameEdit::new("Plan.csv", "Plan.csv", false);
        assert_eq!(e.selection, NameSelection::Stem);
        assert_eq!(e.selected_chars(), 4);
        e.type_text("Budget");
        assert_eq!(e.text, "Budget.csv");
        e.type_text(" 2026"); // the caret stays before the extension
        assert_eq!(e.text, "Budget 2026.csv");
        assert_eq!(e.new_file_name().as_deref(), Some("Budget 2026.csv"));
        e.end();
        e.backspace();
        assert_eq!(e.text, "Budget 2026.cs");
        e.home();
        e.delete();
        assert_eq!(e.text, "udget 2026.cs");
    }
    #[test]
    fn hidden_shortcut_extension_is_put_back() {
        let mut e = NameEdit::new("ExampleApp", "ExampleApp.lnk", false);
        assert_eq!(e.hidden_ext, ".lnk");
        assert_eq!(e.selection, NameSelection::All);
        e.type_text("Art tools");
        assert_eq!(e.new_file_name().as_deref(), Some("Art tools.lnk"));
        assert_eq!(e.shown(), "Art tools");
    }
    #[test]
    fn folders_select_everything_and_backspace_clears_selection() {
        let mut e = NameEdit::new("Photos 2024.old", "Photos 2024.old", true);
        assert_eq!(e.selection, NameSelection::All);
        e.backspace();
        assert_eq!(e.text, "");
        assert_eq!(e.new_file_name(), None); // an empty name renames nothing
        let mut f = NameEdit::new("notes.txt", "notes.txt", false);
        f.backspace();
        assert_eq!(f.text, ".txt");
        f.backspace(); // caret is at the start now: nothing before it to delete
        assert_eq!(f.text, ".txt");
        f.type_text("todo");
        assert_eq!(f.text, "todo.txt");
        let mut g = NameEdit::new("Plan.csv", "Plan.csv", false);
        g.right(); // collapse to the end of the selected name, before ".csv"
        g.type_text("s");
        assert_eq!(g.text, "Plans.csv");
    }
    #[test]
    fn refuses_characters_windows_forbids_and_unchanged_names() {
        let mut e = NameEdit::new("a.txt", "a.txt", false);
        assert!(!e.type_text("b:c?"));
        assert_eq!(e.text, "bc.txt");
        let same = NameEdit::new("a.txt", "a.txt", false);
        assert_eq!(same.new_file_name(), None);
        let mut dots = NameEdit::new("x", "x", false);
        dots.type_text("report. ");
        assert_eq!(dots.new_file_name().as_deref(), Some("report"));
        let mut all = NameEdit::new("Plan.csv", "Plan.csv", false);
        all.select_all();
        all.type_text("Q");
        assert_eq!(all.text, "Q");
    }
}

/// Recovered roll-up reveal delay when "after a delay" is chosen (DelayTime).
pub const ROLLUP_DELAY: f64 = 0.5;

pub fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl State {
    pub fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    pub fn group(&self, id: u64) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }
    pub fn group_mut(&mut self, id: u64) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        // Cooperate with the OLE transaction's exclusive sidecar lease.
        // Never write a workspace while a staged OLE replace is in progress.
        // This does not coordinate with external processes that ignore the
        // lock; OLE commit also compares the exact original file bytes.
        let _lease = crate::native_ole_persist::WorkspaceLease::acquire(path)?;
        // A crashed OLE ReplaceFile operation may leave a recoverable
        // original. Do not erase the current layout while backup is pending.
        if path.with_extension("ole-backup").exists()
            || path.with_extension("native-target-backup").exists()
            || path.with_extension("rollback-after").exists() {
            return Err("Workspace has an unresolved recovery backup or predecessor; inspect before saving".into());
        }
        if !crate::native_ole_persist::unresolved_workspace_ole_stages(path)?.is_empty() {
            return Err("Workspace has an unresolved OLE staged transaction; inspect before saving".into());
        }
        let text = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if text.len() > MAX_WORKSPACE_BYTES {
            return Err("Workspace exceeds the readable size limit; previous layout preserved".into());
        }
        let temp = path.with_extension("tmp");
        fs::write(&temp, &text).map_err(|e| e.to_string())?;
        fs::rename(temp, path).map_err(|e| e.to_string())
    }
    pub fn save_returning_bytes(
        &self,
        path: &Path,
        expected_predecessor: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        use std::collections::HashMap;
        fn collect_tabs_by_id(
            raw: &serde_json::Value,
            modeled: &serde_json::Value,
            out: &mut HashMap<u64, (serde_json::Value, serde_json::Value)>,
        ) {
            let (Some(raw_groups), Some(mod_groups)) = (
                raw.get("groups").and_then(|v| v.as_array()),
                modeled.get("groups").and_then(|v| v.as_array()),
            ) else {
                return;
            };
            for rg in raw_groups {
                let Some(gid) = rg.get("id").and_then(|v| v.as_u64()) else {
                    continue;
                };
                let Some(mg) = mod_groups
                    .iter()
                    .find(|g| g.get("id").and_then(|v| v.as_u64()) == Some(gid))
                else {
                    continue;
                };
                let (Some(raw_tabs), Some(mod_tabs)) = (
                    rg.get("tabs").and_then(|v| v.as_array()),
                    mg.get("tabs").and_then(|v| v.as_array()),
                ) else {
                    continue;
                };
                for rt in raw_tabs {
                    let Some(tid) = rt.get("id").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    if let Some(mt) = mod_tabs
                        .iter()
                        .find(|t| t.get("id").and_then(|v| v.as_u64()) == Some(tid))
                    {
                        out.insert(tid, (rt.clone(), mt.clone()));
                    }
                }
            }
        }
        fn merge_preserving_unknown(
            base: &mut serde_json::Value,
            old_modeled: &serde_json::Value,
            updated: serde_json::Value,
            tabs_by_id: &HashMap<u64, (serde_json::Value, serde_json::Value)>,
            is_tabs_array: bool,
        ) {
            match (base, old_modeled, updated) {
                (
                    serde_json::Value::Object(base_map),
                    serde_json::Value::Object(old_map),
                    serde_json::Value::Object(new_map),
                ) => {
                    // Remove modeled keys that were cleared (e.g., skip_serializing_if = Option::is_none).
                    for old_key in old_map.keys() {
                        if !new_map.contains_key(old_key) {
                            base_map.remove(old_key);
                        }
                    }
                    for (k, v) in new_map {
                        let child_is_tabs = k == "tabs";
                        if let (Some(existing), Some(old_child)) =
                            (base_map.get_mut(&k), old_map.get(&k))
                        {
                            merge_preserving_unknown(
                                existing,
                                old_child,
                                v,
                                tabs_by_id,
                                child_is_tabs,
                            );
                        } else {
                            base_map.insert(k, v);
                        }
                    }
                }
                (
                    serde_json::Value::Array(base_arr),
                    serde_json::Value::Array(old_arr),
                    serde_json::Value::Array(new_arr),
                ) => {
                    let has_ids = !new_arr.is_empty()
                        && new_arr.iter().all(|v| {
                            v.as_object()
                                .and_then(|o| o.get("id"))
                                .and_then(|id| id.as_u64())
                                .is_some()
                        });
                    if has_ids {
                        let merged: Vec<serde_json::Value> = new_arr
                            .into_iter()
                            .map(|new_item| {
                                let id = new_item["id"].as_u64().unwrap();
                                if let Some(existing) = base_arr.iter().find(|b| {
                                    b.as_object()
                                        .and_then(|o| o.get("id"))
                                        .and_then(|bid| bid.as_u64())
                                        == Some(id)
                                }) {
                                    let old_child = old_arr
                                        .iter()
                                        .find(|o| {
                                            o.as_object()
                                                .and_then(|m| m.get("id"))
                                                .and_then(|oid| oid.as_u64())
                                                == Some(id)
                                        })
                                        .unwrap_or(&serde_json::Value::Null);
                                    let mut cloned = existing.clone();
                                    merge_preserving_unknown(
                                        &mut cloned,
                                        old_child,
                                        new_item,
                                        tabs_by_id,
                                        false,
                                    );
                                    cloned
                                } else if is_tabs_array {
                                    if let Some((raw_tab, mod_tab)) = tabs_by_id.get(&id) {
                                        let mut cloned = raw_tab.clone();
                                        merge_preserving_unknown(
                                            &mut cloned,
                                            mod_tab,
                                            new_item,
                                            tabs_by_id,
                                            false,
                                        );
                                        cloned
                                    } else {
                                        new_item
                                    }
                                } else {
                                    new_item
                                }
                            })
                            .collect();
                        *base_arr = merged;
                    } else {
                        // Preserve unknown fields on unchanged non-ID array entries (e.g., snapshots, rules).
                        let merged: Vec<serde_json::Value> = new_arr
                            .into_iter()
                            .enumerate()
                            .map(|(i, new_item)| {
                                if old_arr.get(i) == Some(&new_item) {
                                    if let Some(raw_item) = base_arr.get(i) {
                                        return raw_item.clone();
                                    }
                                }
                                if let Some(idx) = old_arr.iter().position(|o| o == &new_item) {
                                    if let Some(raw_item) = base_arr.get(idx) {
                                        return raw_item.clone();
                                    }
                                }
                                new_item
                            })
                            .collect();
                        *base_arr = merged;
                    }
                }
                (slot, _, new_val) => *slot = new_val,
            }
        }
        let mut base = serde_json::from_slice::<serde_json::Value>(expected_predecessor)
            .map_err(|e| format!("Could not parse existing Workspace JSON to preserve metadata: {e}"))?;
        if !base.is_object() {
            return Err("Existing Workspace JSON root is not an object".into());
        }
        let old_state = Self::from_saved_bytes(expected_predecessor)?;
        let old_modeled = serde_json::to_value(&old_state).map_err(|e| e.to_string())?;
        let updated = serde_json::to_value(self).map_err(|e| e.to_string())?;
        if !updated.is_object() {
            return Err("Serialized Workspace root is not an object".into());
        }
        let mut tabs_by_id = HashMap::new();
        collect_tabs_by_id(&base, &old_modeled, &mut tabs_by_id);
        merge_preserving_unknown(&mut base, &old_modeled, updated, &tabs_by_id, false);
        let text = serde_json::to_vec_pretty(&base).map_err(|e| e.to_string())?;
        if text.len() > MAX_WORKSPACE_BYTES {
            return Err("Workspace exceeds the readable size limit; previous layout preserved".into());
        }
        let verify = Self::from_saved_bytes(&text)?;
        if serde_json::to_vec(&verify).map_err(|e| e.to_string())?
            != serde_json::to_vec(self).map_err(|e| e.to_string())?
        {
            return Err("Merged Workspace JSON diverged from intended State; refusing save".into());
        }
        crate::native_ole_persist::replace_verified_workspace_bytes(
            path,
            expected_predecessor,
            &text,
        )
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let file = fs::File::open(path).map_err(|e| e.to_string())?;
        let mut text = Vec::new();
        file.take(MAX_WORKSPACE_BYTES as u64 + 1)
            .read_to_end(&mut text).map_err(|e| e.to_string())?;
        Self::from_saved_bytes(&text)
    }
    /// Parse the exact bytes already read by a transaction or recovery
    /// exporter. This avoids validating a different version of a file
    /// after an external writer changes it between two read calls.
    pub(crate) fn from_saved_bytes(text: &[u8]) -> Result<Self, String> {
        if text.is_empty() || text.len() > MAX_WORKSPACE_BYTES {
            return Err("Saved Workspace is empty or exceeds 32 MiB".into());
        }
        let mut state: Self = serde_json::from_slice(text).map_err(|e| e.to_string())?;
        state.sync_page_birth();
        if state.version != 1
            || state.pages.is_empty()
            || state.page >= state.pages.len()
            || state.groups.iter().any(|g| {
                g.tabs.is_empty()
                    || g.active >= g.tabs.len()
                    || g.page >= state.pages.len()
                    || !g.bounds.w.is_finite()
                    || !g.bounds.x.is_finite()
                    || !g.bounds.y.is_finite()
                    || !g.bounds.h.is_finite()
            })
        {
            return Err("The saved layout is invalid; the original has been kept.".into());
        }
        Ok(state)
    }
}
pub fn create_demo(root: &Path) -> std::io::Result<()> {
    for folder in [
        "Projects/Studio",
        "Projects/Website",
        "Projects/Research",
        "Notes",
        "Library/Design",
        "Library/Documents",
        "Library/Photos",
    ] {
        fs::create_dir_all(root.join("demo").join(folder))?;
    }
    let files=[("Projects/Project brief.txt","PROJECT BRIEF\n\nA clear place for the things you are working on.\n"),("Projects/Timeline.csv","Task,Status\nPlan,Complete\nBuild,In progress\nReview,Next\n"),("Projects/Checklist.txt","[x] Create a workspace\n[ ] Connect your own folders\n[ ] Save a layout\n"),("Notes/Ideas.txt","IDEAS\n\nStart with one small thing.\n"),("Notes/Reading list.txt","READING LIST\n\nAdd a few things you want to explore.\n"),("Notes/This week.txt","THIS WEEK\n\nMake room for focused work.\n"),("Library/Documents/Notes.txt","Your folder view can browse subfolders.\n"),("Library/Welcome.txt","This is a real sample folder. Choose Folder view to connect your own.\n"),("Library/Resources.txt","Keep your most useful files close by.\n"),("Library/Plan.csv","Day,Focus\nMonday,Planning\nTuesday,Creating\n"),("Welcome.html","<!doctype html><meta charset='utf-8'><title>Welcome</title><body style='font:20px system-ui;margin:10%;background:#eff4f8;color:#203545'><h1>Your space, organized.</h1><p>These are real sample files. Add your files or connect a folder to make this workspace yours.</p><p>Drag a header to move a group. Double-click a header to roll it up. Draw a rectangle on empty space to create a group.</p></body>")];
    for (name, contents) in files {
        let path = root.join("demo").join(name);
        if !path.exists() {
            fs::write(path, contents)?;
        }
    }
    Ok(())
}
/// Where the window-mode sample workspace lives: the project folder during
/// development and checks, otherwise the user's own data folder (the installed
/// program never depends on a development path).
pub fn app_root() -> PathBuf {
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if dev.join("Cargo.toml").exists() { return dev; }
    }
    crate::host::data_dir().join("sample")
}
