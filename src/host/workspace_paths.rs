use super::*;

/// Keep existing users' workspace untouched across the product rename.
/// A new installation uses %APPDATA%\DesktopCraft. Existing Desktop Spaces
/// workspace data remains where it is; no automatic moves or file deletion.
pub(super) fn choose_workspace_dir(appdata: &Path) -> PathBuf {
    let current = appdata.join(APP_NAME);
    let legacy = appdata.join(LEGACY_APP_NAME);
    if current.join("data/workspace.json").is_file() {
        current
    } else if legacy.join("data/workspace.json").is_file() {
        legacy
    } else {
        current
    }
}
pub fn data_dir() -> PathBuf {
    if let Some(dir) =
        std::env::var_os("DESKTOPCRAFT_DATA").or_else(|| std::env::var_os("DESKTOP_SPACES_DATA"))
    {
        return PathBuf::from(dir);
    }
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    choose_workspace_dir(&appdata)
}
/// Onboarding is destructive to a saved layout. Treat unknown filesystem
/// state and recovery artifacts as existing work, never as a fresh profile.
pub fn workspace_allows_onboarding(root: &Path) -> bool {
    let workspace = root.join("data/workspace.json");
    if !matches!(workspace.try_exists(), Ok(false))
        || !matches!(workspace.with_extension("ole-backup").try_exists(), Ok(false))
        || !matches!(root.join("native-inbox-recovery.json").try_exists(), Ok(false))
    {
        return false;
    }
    match root.join("data").try_exists() {
        Ok(false) => true,
        Ok(true) => crate::native_ole_persist::unresolved_workspace_ole_stages(&workspace)
            .is_ok_and(|stages| stages.is_empty()),
        Err(_) => false,
    }
}

pub fn install_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Programs")
        .join(APP_NAME)
}
pub fn commands_dir() -> PathBuf {
    data_dir().join("commands")
}

#[cfg(test)]
#[test]
fn onboarding_requires_absent_workspace_and_no_recovery() {
    let root = std::env::temp_dir().join(format!(
        "desktopcraft-onboarding-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let data = root.join("data");
    let workspace = data.join("workspace.json");
    assert!(workspace_allows_onboarding(&root));
    std::fs::create_dir_all(&data).unwrap();
    let choice = data.join("onboarding-choice.txt");
    for selection in ["onboarding", "custom"] {
        std::fs::write(&choice, selection).unwrap();
        assert!(workspace_allows_onboarding(&root));
        let retained = br#"{"groups":[{"id":42}],"rules":[{"target":42}]}"#;
        std::fs::write(&workspace, retained).unwrap();
        assert!(!workspace_allows_onboarding(&root));
        assert_eq!(std::fs::read(&workspace).unwrap(), retained);
        std::fs::remove_file(&workspace).unwrap();
    }
    let backup = workspace.with_extension("ole-backup");
    std::fs::write(&backup, b"saved").unwrap();
    assert!(!workspace_allows_onboarding(&root));
    std::fs::remove_file(&backup).unwrap();
    let stage = data.join(".workspace.json.desktopcraft-ole-test.tmp");
    std::fs::write(&stage, b"pending").unwrap();
    assert!(!workspace_allows_onboarding(&root));
    std::fs::remove_file(&stage).unwrap();
    let journal = root.join("native-inbox-recovery.json");
    std::fs::write(&journal, b"pending").unwrap();
    assert!(!workspace_allows_onboarding(&root));
    std::fs::remove_file(&journal).unwrap();
    assert!(workspace_allows_onboarding(&root));
    std::fs::remove_dir_all(root).unwrap();
}
