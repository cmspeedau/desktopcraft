use super::*;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Snapshot {
    pub name: String,
    pub groups: Vec<Group>,
    pub pages: Vec<String>,
    #[serde(default)]
    pub page_birth: Vec<u64>,
    pub time: u64,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct State {
    pub version: u32,
    pub groups: Vec<Group>,
    pub pages: Vec<String>,
    /// Persisted creation order remains stable after alphabetical sorting.
    /// Older workspaces get IDs inferred from their existing page order.
    #[serde(default)]
    pub page_birth: Vec<u64>,
    #[serde(default)]
    pub next_page_birth: u64,
    pub page: usize,
    pub next_id: u64,
    pub rules: Vec<Rule>,
    pub snapshots: Vec<Snapshot>,
    pub blend: bool,
    pub tint: bool,
    pub snap: bool,
    pub quick_hide: bool,
    pub auto_rules: bool,
    pub wallpaper: usize,
    /// Desktop mode: one layout per monitor arrangement (signature -> group -> place).
    #[serde(default)]
    pub layouts: std::collections::BTreeMap<String, std::collections::BTreeMap<u64, Placement>>,
    /// Each group's most recent place, used for arrangements not seen before.
    #[serde(default)]
    pub placements: std::collections::BTreeMap<u64, Placement>,
    /// Set while quick-hide has Explorer's icons hidden, so a crash or shutdown
    /// is recovered on the next start.
    #[serde(default)]
    pub explorer_icons_hidden: bool,
    /// Settings that mirror the original manager's pages (all with defaults so
    /// older saved layouts still load).
    #[serde(default)]
    pub prefs: Prefs,
}
/// A group's place on one monitor: the monitor's device name and the position
/// relative to that monitor's work area (logical pixels).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Placement {
    pub monitor: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
