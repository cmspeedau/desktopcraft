use super::*;

/// Stage and verify the new executable before touching the installed binary.
/// If activation fails, restore the original path; the previous.exe backup is
/// created by install() before this helper is called.
#[cfg(windows)]
fn activate_staged_installer(stage: &Path, destination: &Path, backup: &Path)
    -> std::io::Result<()> {
    if !destination.exists() {
        return std::fs::rename(stage, destination);
    }
    // ReplaceFileW is a single filesystem replacement operation, not
    // rename-away then rename-new. Windows keeps old bytes at backup.
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(replaced: *const u16, replacement: *const u16,
            backup: *const u16, flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void) -> i32;
    }
    let wide = |path: &Path| path.as_os_str().encode_wide()
        .chain(std::iter::once(0)).collect::<Vec<u16>>();
    let to=wide(destination);
    let from=wide(stage);
    let old=wide(backup);
    let result=unsafe {ReplaceFileW(to.as_ptr(),from.as_ptr(),old.as_ptr(),
        0,std::ptr::null_mut(),std::ptr::null_mut())};
    if result!=0 {Ok(())}else{Err(std::io::Error::last_os_error())}
}

pub(super) fn install_executable_staged(source: &Path, destination: &Path) -> Result<(), String> {
    install_executable_with(source, destination, |from, to, backup| {
        #[cfg(windows)]
        {activate_staged_installer(from, to, backup)}
        #[cfg(not(windows))]
        {
            let _ = backup;
            if to.exists() {
                return Err(std::io::Error::new(std::io::ErrorKind::Unsupported,
                    "Atomic upgrade requires Windows ReplaceFileW"));
            }
            std::fs::rename(from,to)
        }
    })
}

fn install_executable_with<F>(
    source: &Path,
    destination: &Path,
    activate: F,
) -> Result<(), String>
where
    F: FnOnce(&Path, &Path, &Path) -> std::io::Result<()>,
{
    use std::io::Write;
    let parent = destination.parent().ok_or("Missing installation directory")?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let stage = parent.join(format!(".desktopcraft-{}-{stamp}.stage", std::process::id()));
    let aside = parent.join(format!("{APP_NAME}.exe.{stamp}.old"));
    let had_previous = destination.is_file();
    let outcome = (|| -> Result<(), String> {
        let mut input = std::fs::File::open(source).map_err(|e| e.to_string())?;
        let mut staged = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut input, &mut staged).map_err(|e| e.to_string())?;
        staged.flush().map_err(|e| e.to_string())?;
        staged.sync_all().map_err(|e| e.to_string())?;
        drop(staged);

        // Compare actual bytes, not just file lengths. Never activate a
        // truncated or modified staging copy.
        let source_bytes = std::fs::read(source).map_err(|e| e.to_string())?;
        let staged_bytes = std::fs::read(&stage).map_err(|e| e.to_string())?;
        if source_bytes != staged_bytes || staged_bytes.is_empty() {
            return Err("Installer staging verification failed".into());
        }
        drop(staged_bytes);

        if destination.exists() && !had_previous {
            return Err("Installation target is not a regular file".into());
        }
        // Atomic ReplaceFileW keeps the installed path present across an
        // interrupted upgrade. There is no moment when a shortcut points
        // at a deliberately missing executable.
        if let Err(error) = activate(&stage, destination, &aside) {
            if had_previous && !destination.exists() && aside.is_file() {
                std::fs::rename(&aside,destination).map_err(|restore_error|
                    format!("Activation failed ({error}); backup recovery at {} failed: {restore_error}",
                        aside.display()))?;
            }
            if had_previous && !destination.exists() {
                return Err(format!("Activation failed ({error}); installed EXE missing. Preserve staged file {} and recovery backup {}",
                    stage.display(),aside.display()));
            }
            return Err(format!("Could not atomically activate staged executable: {error}"));
        }
        if !std::fs::read(destination).is_ok_and(|bytes| bytes == source_bytes) {
            if had_previous {
                std::fs::rename(&aside, destination).map_err(|error|
                    format!("Activated EXE failed verification; restore from {} failed: {error}",
                        aside.display()))?;
            } else {
                std::fs::remove_file(destination).map_err(|error|
                    format!("Activated EXE failed verification; cleanup failed: {error}"))?;
            }
            return Err("Activated executable differed from the verified staging bytes".into());
        }
        if had_previous {
            // The persistent DesktopCraft.previous.exe snapshot is retained
            // by the installer. Remove this temporary ReplaceFileW backup
            // only after the replacement has been verified.
            let _ = std::fs::remove_file(&aside);
        }
        Ok(())
    })();
    // In the extraordinary case that Windows failed after removing the
    // destination AND could not create a backup, preserve staging for
    // emergency recovery rather than deleting the only remaining EXE.
    if !had_previous || destination.exists() {
        let _ = std::fs::remove_file(&stage);
    }
    outcome
}

#[cfg(test)]
mod installer_transaction_tests {
    use super::*;
    use std::fs;

    fn isolated_dir(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "desktopcraft-installer-{case}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn staged_upgrade_activates_exact_bytes_and_leaves_unrelated_data_unchanged() {
        let dir = isolated_dir("upgrade");
        let source = dir.join("incoming.exe");
        let installed = dir.join("DesktopCraft.exe");
        let workspace = dir.join("workspace.json");
        fs::write(&source, b"new tested installer binary").unwrap();
        fs::write(&installed, b"prior user installer").unwrap();
        fs::write(&workspace, b"{\"keep\":true}").unwrap();
        install_executable_staged(&source, &installed).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), fs::read(&source).unwrap());
        assert_eq!(fs::read(&workspace).unwrap(), b"{\"keep\":true}");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            3,
            "No stage or parked binary should remain after successful upgrade"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_activation_restores_original_binary_and_cleans_staging() {
        let dir = isolated_dir("rollback");
        let source = dir.join("incoming.exe");
        let installed = dir.join("DesktopCraft.exe");
        fs::write(&source, b"replacement").unwrap();
        fs::write(&installed, b"original installed bytes").unwrap();
        let result = install_executable_with(&source, &installed, |_, _, _| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "simulated activation failure",
            ))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&installed).unwrap(), b"original installed bytes");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_activation_restores_the_verified_predecessor() {
        let dir = isolated_dir("corrupt-activation");
        let source = dir.join("incoming.exe");
        let installed = dir.join("DesktopCraft.exe");
        fs::write(&source, b"replacement").unwrap();
        fs::write(&installed, b"original").unwrap();
        let result = install_executable_with(&source, &installed, |_, target, backup| {
            fs::rename(target, backup)?;
            fs::write(target, b"corrupt")
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&installed).unwrap(), b"original");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_invalid_upgrade_source_never_replaces_installed_binary() {
        let dir = isolated_dir("reject");
        let installed = dir.join("DesktopCraft.exe");
        fs::write(&installed, b"known good version").unwrap();
        assert!(install_executable_staged(&dir.join("missing.exe"), &installed).is_err());
        assert_eq!(fs::read(&installed).unwrap(), b"known good version");
        let empty = dir.join("empty.exe");
        fs::write(&empty, b"").unwrap();
        assert!(install_executable_staged(&empty, &installed).is_err());
        assert_eq!(fs::read(&installed).unwrap(), b"known good version");
        fs::remove_dir_all(dir).unwrap();
    }

    #[derive(Default)]
    struct MockInstallOps {
        fail_at: Option<&'static str>,
        start_at_login_seen: Option<bool>,
        exe_installed: bool,
        notices_written: bool,
        shortcut_written: bool,
        registry_written: bool,
        autostart_applied: bool,
        rollback_order: Vec<&'static str>,
    }

    impl InstallOps for MockInstallOps {
        fn swap_executable(&mut self) -> Result<(), String> {
            if self.fail_at == Some("exe") {
                return Err("injected exe failure".into());
            }
            self.exe_installed = true;
            Ok(())
        }
        fn rollback_executable(&mut self) -> Result<(), String> {
            self.rollback_order.push("exe");
            self.exe_installed = false;
            Ok(())
        }
        fn write_notices(&mut self) -> Result<(), String> {
            self.notices_written = true;
            if self.fail_at == Some("notices") {
                return Err("injected notices failure".into());
            }
            Ok(())
        }
        fn rollback_notices(&mut self) -> Result<(), String> {
            self.rollback_order.push("notices");
            self.notices_written = false;
            Ok(())
        }
        fn write_shortcut(&mut self) -> Result<(), String> {
            self.shortcut_written = true;
            if self.fail_at == Some("shortcut") {
                return Err("injected shortcut failure".into());
            }
            Ok(())
        }
        fn rollback_shortcut(&mut self) -> Result<(), String> {
            self.rollback_order.push("shortcut");
            self.shortcut_written = false;
            Ok(())
        }
        fn write_registry(&mut self, _start_at_login: bool) -> Result<(), String> {
            self.registry_written = true;
            if self.fail_at == Some("registry") {
                return Err("injected registry failure".into());
            }
            Ok(())
        }
        fn rollback_registry(&mut self) -> Result<(), String> {
            self.rollback_order.push("registry");
            self.registry_written = false;
            Ok(())
        }
        fn apply_autostart(&mut self, start_at_login: bool) -> Result<(), String> {
            self.start_at_login_seen = Some(start_at_login);
            self.autostart_applied = true;
            if self.fail_at == Some("autostart") {
                return Err("injected autostart failure".into());
            }
            Ok(())
        }
        fn rollback_autostart(&mut self) -> Result<(), String> {
            self.rollback_order.push("autostart");
            self.autostart_applied = false;
            Ok(())
        }
    }

    #[test]
    fn install_transaction_rolls_back_prior_steps_in_reverse_order_on_any_failure() {
        for (stage, expected_order) in [
            ("exe", vec![]),
            ("notices", vec!["notices", "exe"]),
            ("shortcut", vec!["shortcut", "notices", "exe"]),
            ("registry", vec!["registry", "shortcut", "notices", "exe"]),
            ("autostart", vec!["autostart", "registry", "shortcut", "notices", "exe"]),
        ] {
            let mut ops = MockInstallOps { fail_at: Some(stage), ..Default::default() };
            let err = run_install_transaction(&mut ops, true).unwrap_err();
            assert!(!err.is_empty());
            assert_eq!(ops.rollback_order, expected_order, "stage {stage}");
            assert!(!ops.exe_installed);
            assert!(!ops.notices_written);
            assert!(!ops.shortcut_written);
            assert!(!ops.registry_written);
            assert!(!ops.autostart_applied);
        }
    }

    #[test]
    fn post_install_launch_failure_rolls_back_all_committed_registration_stages() {
        let mut ops = MockInstallOps::default();
        run_install_transaction(&mut ops, false).unwrap();
        assert!(ops.exe_installed && ops.notices_written && ops.shortcut_written && ops.registry_written && ops.autostart_applied);
        assert_eq!(rollback_install_stages(&mut ops, 4), Ok(()));
        assert_eq!(ops.rollback_order, vec!["autostart", "registry", "shortcut", "notices", "exe"]);
        assert!(!ops.exe_installed && !ops.notices_written && !ops.shortcut_written && !ops.registry_written && !ops.autostart_applied);
    }
}

pub trait InstallOps {
    fn swap_executable(&mut self) -> Result<(), String>;
    fn rollback_executable(&mut self) -> Result<(), String>;
    fn write_notices(&mut self) -> Result<(), String>;
    fn rollback_notices(&mut self) -> Result<(), String>;
    fn write_shortcut(&mut self) -> Result<(), String>;
    fn rollback_shortcut(&mut self) -> Result<(), String>;
    fn write_registry(&mut self, start_at_login: bool) -> Result<(), String>;
    fn rollback_registry(&mut self) -> Result<(), String>;
    fn apply_autostart(&mut self, start_at_login: bool) -> Result<(), String>;
    fn rollback_autostart(&mut self) -> Result<(), String>;
}

pub(super) fn rollback_install_stages<Ops: InstallOps>(ops: &mut Ops, through_stage: usize) -> Result<(), String> {
    let mut failures = Vec::new();
    for stage in (0..=through_stage).rev() {
        let res = match stage {
            4 => ops.rollback_autostart().map_err(|e| format!("autostart: {e}")),
            3 => ops.rollback_registry().map_err(|e| format!("registry: {e}")),
            2 => ops.rollback_shortcut().map_err(|e| format!("shortcut: {e}")),
            1 => ops.rollback_notices().map_err(|e| format!("notices: {e}")),
            0 => ops.rollback_executable().map_err(|e| format!("EXE: {e}")),
            _ => Ok(()),
        };
        if let Err(e) = res {
            failures.push(e);
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(failures.join("; ")) }
}

pub fn run_install_transaction<Ops: InstallOps>(ops: &mut Ops, start_at_login: bool) -> Result<(), String> {
    if let Err(e) = ops.swap_executable() {
        return Err(format!("Could not update DesktopCraft: {e}"));
    }
    let steps: [(&str, fn(&mut Ops, bool) -> Result<(), String>); 4] = [
        ("Third-party notices write failed", |o, _| o.write_notices()),
        ("Start Menu shortcut failed", |o, _| o.write_shortcut()),
        ("Registry registration failed", |o, login| o.write_registry(login)),
        ("Autostart registration failed", |o, login| o.apply_autostart(login)),
    ];
    for (idx, (label, step)) in steps.iter().enumerate() {
        if let Err(error) = step(ops, start_at_login) {
            let stage = idx + 1;
            let rb = match rollback_install_stages(ops, stage) {
                Ok(()) => "attempt completed".to_owned(),
                Err(failures) => failures,
            };
            return Err(format!("{label}: {error}. Rollback: {rb}"));
        }
    }
    Ok(())
}

