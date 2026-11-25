use super::*;

pub(super) fn plan_native_rules_sort(
    before: &State, desktop: &[ViewItem], rules: &[crate::model::Rule], next_id: u64,
    include_existing_pins: bool,
) -> (State, Vec<String>) {
    let mut owners = HashSet::new();
    let mut duplicates = HashSet::new();
    for pin in before.groups.iter().flat_map(|g| &g.tabs)
        .filter(|tab| tab.folder.is_none()).flat_map(|tab| &tab.items)
    {
        let key = pin.path.to_lowercase();
        if !owners.insert(key.clone()) {
            duplicates.insert(key);
        }
    }
    let paths = desktop.iter().map(|item| std::path::Path::new(&item.path))
        .filter(|path| path.is_absolute()).map(ToOwned::to_owned).collect::<Vec<_>>();
    let desktop_paths = paths.iter().map(|path| path.to_string_lossy().to_lowercase())
        .collect::<HashSet<_>>();
    let mut next = before.clone();
    next.next_id = next_id;
    next.rules = rules.to_vec();
    let mut changed = next.sort_loose_paths_by_rules(paths);
    if include_existing_pins {
        changed.extend(next.apply_rules_to_pins(|pin| {
            let key = pin.path.to_lowercase();
            desktop_paths.contains(&key) && !duplicates.contains(&key)
        }));
    }
    next.rules.clone_from(&before.rules);
    (next, changed)
}

pub(super) fn plan_native_layout_restore(
    before: &State,
    desktop: &[ViewItem],
    target: &State,
    next_id: u64,
) -> Result<(State, Vec<String>), String> {
    if before.groups.len() != target.groups.len() || before.pages != target.pages {
        return Err("Snapshot Boxes or Pages differ from the active Native Desktop workspace".into());
    }
    for (prior, goal) in before.groups.iter().zip(&target.groups) {
        if prior.id != goal.id || prior.page != goal.page || prior.tabs.len() != goal.tabs.len() {
            return Err("Snapshot Boxes or Tabs differ from the active Native Desktop workspace".into());
        }
        let b = goal.bounds;
        if ![b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite()) || b.w <= 0. || b.h <= 0. {
            return Err("Snapshot Box geometry is invalid".into());
        }
        for (lt, rt) in prior.tabs.iter().zip(&goal.tabs) {
            if lt.id != rt.id || lt.folder.is_some() != rt.folder.is_some() {
                return Err("Snapshot Tab types differ from the active Native Desktop workspace".into());
            }
        }
    }
    let mut desktop_by_key = HashMap::new();
    for item in desktop {
        let path = std::path::Path::new(&item.path);
        if path.is_absolute() {
            desktop_by_key.entry(item.path.to_lowercase()).or_insert(path);
        }
    }
    let mut before_pos: HashMap<String, (u64, u64, usize)> = HashMap::new();
    let mut before_pins: HashMap<String, crate::model::Item> = HashMap::new();
    let mut duplicates = HashSet::new();
    for group in &before.groups {
        for tab in group.tabs.iter().filter(|t| t.folder.is_none()) {
            for (idx, pin) in tab.items.iter().enumerate() {
                let key = pin.path.to_lowercase();
                if before_pins.contains_key(&key) {
                    duplicates.insert(key);
                } else {
                    before_pos.insert(key.clone(), (group.id, tab.id, idx));
                    before_pins.insert(key, pin.clone());
                }
            }
        }
    }
    for key in &duplicates {
        before_pos.remove(key);
        before_pins.remove(key);
    }

    let mut next = before.clone();
    let mut alloc_id = next_id;
    let mut placed_keys = HashSet::new();
    let mut after_pos: HashMap<String, (u64, u64, usize)> = HashMap::new();
    for (group, goal_group) in next.groups.iter_mut().zip(&target.groups) {
        for (tab, goal_tab) in group.tabs.iter_mut().zip(&goal_group.tabs) {
            if tab.folder.is_some() {
                continue;
            }
            let preserved_dups: Vec<crate::model::Item> = tab
                .items
                .iter()
                .filter(|item| duplicates.contains(&item.path.to_lowercase()))
                .cloned()
                .collect();
            let mut restored_items = Vec::new();
            for goal_item in &goal_tab.items {
                let key = goal_item.path.to_lowercase();
                if duplicates.contains(&key) || placed_keys.contains(&key) {
                    continue;
                }
                if let Some(existing) = before_pins.get(&key) {
                    placed_keys.insert(key);
                    restored_items.push(existing.clone());
                } else if let Some(path) = desktop_by_key.get(&key) {
                    if let Ok(mut item) = crate::model::Item::from_path(path, 0) {
                        item.id = alloc_id;
                        alloc_id += 1;
                        placed_keys.insert(key);
                        restored_items.push(item);
                    }
                }
            }
            restored_items.extend(preserved_dups);
            for (idx, item) in restored_items.iter().enumerate() {
                let key = item.path.to_lowercase();
                if !duplicates.contains(&key) {
                    after_pos.insert(key, (group.id, tab.id, idx));
                }
            }
            tab.items = restored_items;
        }
    }
    next.next_id = alloc_id;

    let mut changed_paths = Vec::new();
    let mut seen = HashSet::new();
    for group in &before.groups {
        for tab in group.tabs.iter().filter(|t| t.folder.is_none()) {
            for item in &tab.items {
                let key = item.path.to_lowercase();
                if !duplicates.contains(&key)
                    && before_pos.get(&key) != after_pos.get(&key)
                    && seen.insert(key)
                {
                    changed_paths.push(item.path.clone());
                }
            }
        }
    }
    for group in &next.groups {
        for tab in group.tabs.iter().filter(|t| t.folder.is_none()) {
            for item in &tab.items {
                let key = item.path.to_lowercase();
                if !duplicates.contains(&key)
                    && before_pos.get(&key) != after_pos.get(&key)
                    && seen.insert(key)
                {
                    changed_paths.push(item.path.clone());
                }
            }
        }
    }
    if changed_paths.len() > crate::native_ole_persist::MAX_NATIVE_PIN_PATHS {
        return Err("Too many icons changed for a single Native Desktop transaction".into());
    }
    Ok((next, changed_paths))
}

fn norm_pin_key(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    let normalized = trimmed.replace('/', "\\");
    let stripped = if let Some(rest) = normalized
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| normalized.strip_prefix(r"\\?\unc\"))
    {
        format!(r"\\{rest}")
    } else if let Some(rest) = normalized.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        normalized
    };
    stripped.to_lowercase()
}

pub(crate) fn plan_native_pin_paths(
    before: &State,
    desktop: &[ViewItem],
    group_id: u64,
    tab_id: u64,
    paths: &[String],
    next_id: u64,
) -> Result<(State, Vec<String>), String> {
    let dest_tab = before
        .group(group_id)
        .and_then(|g| g.tabs.iter().find(|t| t.id == tab_id))
        .ok_or_else(|| "Target Box or Tab no longer exists".to_string())?;
    if dest_tab.folder.is_some() {
        return Err("Files can only be pinned to a regular Desktop Box tab".into());
    }

    let mut desktop_by_key = HashMap::new();
    for item in desktop {
        let path = std::path::Path::new(&item.path);
        if path.is_absolute() {
            desktop_by_key
                .entry(norm_pin_key(&item.path))
                .or_insert(path);
        }
    }

    let mut before_pins: HashMap<String, crate::model::Item> = HashMap::new();
    let mut before_owner: HashMap<String, (u64, u64)> = HashMap::new();
    let mut duplicates = HashSet::new();
    for group in &before.groups {
        for tab in group.tabs.iter().filter(|t| t.folder.is_none()) {
            for pin in &tab.items {
                let key = norm_pin_key(&pin.path);
                if before_pins.contains_key(&key) {
                    duplicates.insert(key);
                } else {
                    before_owner.insert(key.clone(), (group.id, tab.id));
                    before_pins.insert(key, pin.clone());
                }
            }
        }
    }
    for key in &duplicates {
        before_pins.remove(key);
        before_owner.remove(key);
    }

    let mut seen_input = HashSet::new();
    let mut non_desktop_count = 0usize;
    let mut additions: Vec<crate::model::Item> = Vec::new();
    let mut remove_keys: HashSet<String> = HashSet::new();
    let mut changed_paths: Vec<String> = Vec::new();
    let mut alloc_id = next_id;

    for raw in paths {
        let trimmed = raw.trim().trim_matches('"');
        if trimmed.is_empty() {
            continue;
        }
        let key = norm_pin_key(trimmed);
        if !seen_input.insert(key.clone()) || duplicates.contains(&key) {
            continue;
        }
        if let Some(existing) = before_pins.get(&key) {
            if before_owner.get(&key).copied() == Some((group_id, tab_id)) {
                continue;
            }
            remove_keys.insert(key);
            additions.push(existing.clone());
            changed_paths.push(existing.path.clone());
        } else if let Some(desktop_path) = desktop_by_key.get(&key) {
            match crate::model::Item::from_path(desktop_path, 0) {
                Ok(mut item) => {
                    item.id = alloc_id;
                    alloc_id += 1;
                    additions.push(item.clone());
                    changed_paths.push(item.path);
                }
                Err(_) => {
                    non_desktop_count += 1;
                }
            }
        } else {
            non_desktop_count += 1;
        }
    }

    if changed_paths.is_empty() && non_desktop_count > 0 {
        return Err("Only files on your Windows Desktop can be pinned to a Native Desktop Box. Link another folder as a Folder View instead.".into());
    }
    if changed_paths.len() > crate::native_ole_persist::MAX_NATIVE_PIN_PATHS {
        return Err("Too many icons selected for a single Native Desktop transaction".into());
    }

    let mut next = before.clone();
    next.next_id = alloc_id;
    if !remove_keys.is_empty() {
        for group in &mut next.groups {
            for tab in group.tabs.iter_mut().filter(|t| t.folder.is_none()) {
                tab.items
                    .retain(|pin| !remove_keys.contains(&norm_pin_key(&pin.path)));
            }
        }
    }
    if !additions.is_empty() {
        let target = next
            .groups
            .iter_mut()
            .find(|g| g.id == group_id)
            .and_then(|g| g.tabs.iter_mut().find(|t| t.id == tab_id))
            .ok_or_else(|| "Target Box or Tab no longer exists".to_string())?;
        target.items.extend(additions);
    }

    Ok((next, changed_paths))
}
