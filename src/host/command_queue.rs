use super::*;

/// Write a command for the running instance (new-group, settings, exit...).
pub fn send_command(command: &str) -> std::io::Result<()> {
    let dir = commands_dir();
    std::fs::create_dir_all(&dir)?;
    let name = format!(
        "{}-{}.cmd",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id()
    );
    std::fs::write(dir.join(name), command)
}
/// Commands waiting for this instance, oldest first (each read once).
pub fn take_commands() -> Vec<String> {
    let Ok(read) = std::fs::read_dir(commands_dir()) else {
        return vec![];
    };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "cmd"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok();
            let _ = std::fs::remove_file(&p);
            text.map(|t| t.trim().to_string())
        })
        .collect()
}

