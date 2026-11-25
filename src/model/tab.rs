use super::*;

#[derive(Clone, Deserialize, Debug, PartialEq)]
pub struct Tab {
    pub id: u64,
    pub name: String,
    pub items: Vec<Item>,
    pub folder: Option<String>,
    pub history: Vec<String>,
    pub list: bool,
    pub sort: u8,
    pub scroll: usize,
    pub icon_scale: f32,
    /// Folder View filter typed in its folder bar: only files whose name
    /// contains it (any case) are shown, e.g. "report" or ".exe". Not saved.
    #[serde(skip)]
    pub filter: String,
}

impl Serialize for Tab {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut tab = serializer.serialize_struct("Tab", 9)?;
        tab.serialize_field("id", &self.id)?;
        tab.serialize_field("name", &self.name)?;
        let pins = if self.folder.is_some() { &[][..] } else { self.items.as_slice() };
        tab.serialize_field("items", pins)?;
        tab.serialize_field("folder", &self.folder)?;
        tab.serialize_field("history", &self.history)?;
        tab.serialize_field("list", &self.list)?;
        tab.serialize_field("sort", &self.sort)?;
        tab.serialize_field("scroll", &self.scroll)?;
        tab.serialize_field("icon_scale", &self.icon_scale)?;
        tab.end()
    }
}

pub fn read_folder_items(tab_id: u64, folder: &str, sort: u8) -> Result<Vec<Item>, String> {
    let read = fs::read_dir(folder).map_err(|e| format!("Cannot read this folder: {e}"))?;
    let mut items = vec![];
    for entry in read.filter_map(Result::ok) {
        let mut key = std::collections::hash_map::DefaultHasher::new();
        tab_id.hash(&mut key);
        entry.path().hash(&mut key);
        if let Ok(item) = Item::from_path(&entry.path(), key.finish() | (1u64 << 63)) {
            if !item.name.starts_with('.') && !item.name.eq_ignore_ascii_case("desktop.ini") {
                items.push(item);
            }
        }
    }
    sort_items_slice(&mut items, sort);
    Ok(items)
}

fn sort_items_slice(items: &mut [Item], sort: u8) {
    match sort {
        // Build each expensive Unicode lowercase sort key just once per
        // item, instead of on every comparator invocation (O(n log n)).
        1 => items.sort_by_cached_key(|x| (x.kind.clone(), x.name.to_lowercase())),
        2 => items.sort_by_key(|x| std::cmp::Reverse(x.modified)),
        3 => items.sort_by_key(|x| std::cmp::Reverse(x.size)),
        _ => items.sort_by_cached_key(|x| (x.kind != "folder", x.name.to_lowercase())),
    }
}

impl Tab {
    pub fn new(id: u64, name: &str) -> Self {
        Self {
            id,
            name: name.into(),
            items: vec![],
            folder: None,
            history: vec![],
            list: false,
            sort: 0,
            scroll: 0,
            icon_scale: 1.0,
            filter: String::new(),
        }
    }
    pub fn refresh(&mut self) -> Result<(), String> {
        if let Some(folder) = &self.folder {
            self.items = read_folder_items(self.id, folder, self.sort)?;
        } else {
            self.sort_items();
        }
        Ok(())
    }
    pub fn sort_items(&mut self) {
        sort_items_slice(&mut self.items, self.sort);
    }
    pub fn navigate(&mut self, path: &str) -> Result<(), String> {
        if !Path::new(path).is_dir() {
            return Err("Choose an existing folder.".into());
        }
        let old = self.clone();
        if let Some(folder) = self.folder.take() {
            self.history.push(folder)
        }
        self.folder = Some(path.into());
        self.scroll = 0;
        self.filter.clear();
        if let Err(e) = self.refresh() {
            *self = old;
            return Err(e);
        }
        Ok(())
    }
    pub fn back(&mut self) -> Result<(), String> {
        if let Some(previous) = self.history.pop() {
            self.folder = Some(previous);
            self.scroll = 0;
            self.refresh()?;
            self.filter.clear();
        }
        Ok(())
    }
}
