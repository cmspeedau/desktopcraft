use super::*;

/// Diagnostic log for the active workspace. Keep just 3 bounded files,
/// serialize writes, and never rely on the log to recover user data.
/// Only pass redacted metadata to this function, not file paths/names.
pub fn log(message: &str) {
    write_log_at(&data_dir(), message);
}

fn write_log_at(dir: &Path, message: &str) {
    use std::io::Write;
    static LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Ok(_guard) = LOG_LOCK.lock() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join("desktopcraft.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= 512 * 1024) {
        let one = dir.join("desktopcraft.1.log");
        let two = dir.join("desktopcraft.2.log");
        let _ = std::fs::remove_file(&two);
        let _ = std::fs::rename(&one, &two);
        let _ = std::fs::rename(&path, &one);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let safe: String = message
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .take(240)
            .collect();
        let _ = writeln!(f, "[{secs}] {safe}");
    }
}

/// Stable diagnostic code plus *non-personal* metadata; avoid file paths,
/// file names, rule text and full Windows error messages in a shared report.
pub fn event(level: &str, code: &str, detail: &str) {
    log(&format!("{level} {code}: {detail}"));
}
pub fn log_dir() -> PathBuf {
    data_dir()
}

#[cfg(test)]
mod diagnostic_log_tests {
    use super::*;
    #[test]
    fn diagnostic_logs_rotate_and_keep_recent_events_bounded() {
        let dir =
            std::env::temp_dir().join(format!("desktopcraft-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let active = dir.join("desktopcraft.log");
        std::fs::write(&active, vec![b'x'; 512 * 1024]).unwrap();
        write_log_at(&dir, "INFO session.started: count=1");
        assert!(dir.join("desktopcraft.1.log").exists());
        assert!(!dir.join("desktopcraft.2.log").exists());
        assert!(std::fs::read_to_string(&active)
            .unwrap()
            .contains("session.started"));
        std::fs::write(&active, vec![b'y'; 512 * 1024]).unwrap();
        write_log_at(
            &dir,
            &format!("WARN ui.operation_problem: {}", "z".repeat(800)),
        );
        assert!(dir.join("desktopcraft.2.log").exists());
        assert!(std::fs::metadata(&active).unwrap().len() < 400);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

