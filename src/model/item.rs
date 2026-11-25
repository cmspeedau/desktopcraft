use super::*;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub modified: u64,
    /// What Explorer shows for it (e.g. shortcuts without ".lnk"); rules still
    /// match the real file name in `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}
impl Item {
    pub fn display(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.name)
    }
    pub fn from_path(path: &Path, id: u64) -> std::io::Result<Self> {
        let meta = fs::metadata(path)?;
        let kind = if meta.is_dir() {
            "folder".into()
        } else {
            path.extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
        };
        Ok(Self {
            id,
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            path: path.to_string_lossy().into(),
            kind,
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|x| x.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |x| x.as_secs()),
            label: None,
        })
    }
}
