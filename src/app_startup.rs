use crate::*;

pub(super) fn startup() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if developer_experiment_requested(&args) && !cfg!(all(windows, debug_assertions, feature="native-lab")) {
        eprintln!("Developer experiment commands are unavailable in this build");
        std::process::exit(2);
    }
    if external_native_probe_requested(&args) && !cfg!(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab")) {
        eprintln!("External native probe commands are unavailable in this build");
        std::process::exit(2);
    }
    #[cfg(windows)]
    if args.get(1).is_some_and(|a| a == "--native-crash-watch") {
        std::process::exit(run_native_crash_watch(&args[2..]));
    }
    if has("--modern-menu-screenshot") || has("--modern-menu-preview") {
        eprintln!("The simulated menu preview was removed. Capture the Explorer menu from a physical right-click in an isolated Windows 11 session.");
        std::process::exit(2);
    }
    // User-initiated, outside-of-GUI Workspace safety operations.
    // A checkpoint never writes the live Workspace. Rollback is a separate
    // explicit command with three different absolute paths AND confirmation;
    // the post-commit image must byte-match the current file, refusing if
    // the user has edited their layout since the expected snapshot.
    if let Some(i)=args.iter().position(|a|a=="--native-workspace-safety-backup"){
        let (Some(src),Some(dest))=(args.get(i+1),args.get(i+2)) else {
            eprintln!("Usage: --native-workspace-safety-backup <absolute-workspace> <new-absolute-checkpoint>");
            std::process::exit(2);
        };
        match native_ole_persist::create_verified_workspace_safety_checkpoint(
            &PathBuf::from(src),&PathBuf::from(dest)){
            Ok(bytes)=>{
                println!("{}",serde_json::json!({
                    "mode":"native-workspace-user-safety-checkpoint",
                    "new_verified_checkpoint_created":true,
                    "checkpoint_bytes":bytes,
                    "live_workspace_modified":false,
                    "native_hooks_installed":false,
                    "requires_separate_confirmed_rollback":true
                }));
                std::process::exit(0);
            },
            Err(e)=>{
                eprintln!("Workspace safety checkpoint refused: {e}");
                std::process::exit(3);
            }
        }
    }
    if let Some(i)=args.iter().position(|a|a=="--native-workspace-safety-rollback"){
        let (Some(src),Some(backup),Some(expected))=
            (args.get(i+1),args.get(i+2),args.get(i+3)) else{
            eprintln!("Usage: --native-workspace-safety-rollback <absolute-workspace> <verified-backup> <expected-current-snapshot> --confirm-rollback");
            std::process::exit(2);
        };
        if !args.iter().any(|arg|arg=="--confirm-rollback"){
            eprintln!("Rollback refused: --confirm-rollback must be explicitly supplied");
            std::process::exit(2);
        }
        match native_ole_persist::rollback_workspace_safety_checkpoint(
            &PathBuf::from(src),&PathBuf::from(backup),&PathBuf::from(expected)){
            Ok(bytes)=>{
                println!("{}",serde_json::json!({
                    "mode":"native-workspace-explicit-exact-rollback",
                    "restored_original_bytes":bytes,
                    "live_workspace_restored":true,
                    "overwritten_post_commit_image_preserved":true,
                    "native_hooks_installed":false,
                    "manual_recovery_artifacts_retained":true
                }));
                std::process::exit(0);
            },
            Err(e)=>{
                eprintln!("Conditional Workspace rollback refused: {e}");
                std::process::exit(3);
            }
        }
    }
    // Explicit recovery commands operate only on paths supplied by the
    // caller. Inspection is read-only; export creates a NEW file and refuses
    // to overwrite the live Workspace or the original backup.
    if let Some(index)=args.iter().position(|arg|arg=="--native-ole-recovery-inspect") {
        let Some(path)=args.get(index+1).filter(|s|!s.starts_with("--"))
            .map(PathBuf::from) else {
            eprintln!("Usage: --native-ole-recovery-inspect <absolute-workspace-json-path>");
            std::process::exit(2);
        };
        if !path.is_absolute() {
            eprintln!("OLE recovery inspection requires an absolute Workspace path");
            std::process::exit(2);
        }
        let inspection=native_ole_persist::inspect_native_ole_recovery(&path);
        println!("{}",serde_json::json!({
            "mode":"native_ole_recovery_readonly",
            "workspace_exists":inspection.workspace_exists,
            "workspace_valid":inspection.workspace_valid,
            "backup_exists":inspection.backup_exists,
            "backup_valid":inspection.backup_valid,
            "native_target_backup_exists":inspection.native_target_backup_exists,
            "native_target_backup_valid":inspection.native_target_backup_valid,
            "rollback_after_exists":inspection.rollback_after_exists,
            "rollback_after_valid":inspection.rollback_after_valid,
            "lease_present":inspection.lease_present,
            "staged_artifact_count":inspection.staged_artifact_count,
            "single_stage_valid":inspection.single_stage_valid,
            "stage_scan_ok":inspection.stage_scan_ok,
            "needs_manual_review":inspection.needs_manual_review,
            "disk_mutation":false,
            "auto_restore":false
        }));
        std::process::exit(0);
    }
    if let Some(index)=args.iter().position(|arg|arg=="--native-ole-recovery-stage-export") {
        let (Some(workspace),Some(destination))=(args.get(index+1),args.get(index+2)) else {
            eprintln!("Usage: --native-ole-recovery-stage-export <absolute-workspace-json-path> <new-absolute-destination>");
            std::process::exit(2);
        };
        match native_ole_persist::export_native_ole_stage(
            &PathBuf::from(workspace),&PathBuf::from(destination)) {
            Ok(())=>{
                println!("{}",serde_json::json!({
                    "mode":"native_ole_staged_recovery_export",
                    "new_file_created":true,
                    "workspace_modified":false,
                    "backup_modified":false,
                    "pending_stage_modified":false,
                    "auto_restore":false
                }));
                std::process::exit(0);
            }
            Err(error)=>{
                eprintln!("Staged recovery export refused: {error}");
                std::process::exit(3);
            }
        }
    }
    if let Some(index)=args.iter().position(|arg|arg=="--native-ole-recovery-export") {
        let (Some(workspace),Some(destination))=
            (args.get(index+1),args.get(index+2)) else {
            eprintln!("Usage: --native-ole-recovery-export <absolute-workspace-json-path> <new-absolute-destination>");
            std::process::exit(2);
        };
        match native_ole_persist::export_native_ole_backup(
            &PathBuf::from(workspace),&PathBuf::from(destination)) {
            Ok(())=>{
                println!("{}",serde_json::json!({
                    "mode":"native_ole_recovery_export",
                    "new_file_created":true,
                    "workspace_modified":false,
                    "backup_modified":false,
                    "auto_restore":false
                }));
                std::process::exit(0);
            }
            Err(error)=>{
                eprintln!("Recovery export refused: {error}");
                std::process::exit(3);
            }
        }
    }
    if has("--native-desktop-check") {
        #[cfg(windows)]
        if !enable_native_audit_physical_dpi(){
            eprintln!("Cannot enable physical per-monitor DPI awareness for Native Desktop audit");
            std::process::exit(2);
        }
        // Read-only FIRST: no GUI, hook, Explorer icon movement, or Workspace
        // mutation is needed to learn whether the actual Desktop is eligible.
        shell::init();
        let report=shell::DesktopView::acquire()
            .ok_or_else(||"Explorer Desktop view unavailable".to_string())
            .and_then(|view|model::State::load(&host::data_dir()
                .join("data/workspace.json")).map_err(|e|e.to_string())
                .and_then(|state|native_box_live_layout
                    ::native_desktop_preflight(&state,&view)));
        match report {
            Ok(result)=>{println!("{result}");std::process::exit(0);}
            Err(reason)=>{
                println!("{}",serde_json::json!({
                    "ready":false,"reason":reason,
                    "native_icons_moved":false,"workspace_modified":false}));
                std::process::exit(3);
            }
        }
    }
    if has("--legacy-desktop") {
        host::log("Legacy desktop mode has been removed; no desktop ownership requested");
        eprintln!("Legacy desktop mode has been removed. Start DesktopCraft normally to use Windows Explorer mode.");
        std::process::exit(2);
    }
    if has("--native-desktop") && !cfg!(feature="native-release") {
        eprintln!("Real native desktop mode requires the version-matched DesktopCraft native-release build");
        std::process::exit(2);
    }
    if native_desktop_launch_requested(&args) {
        std::env::set_var("DESKTOPCRAFT_NATIVE_CHROME_ONLY","1");
    }
    if has("--native-explorer-chrome") {
        if !has("--unsafe-native-test") {
            eprintln!("Native Explorer UI preview requires --unsafe-native-test");
            std::process::exit(2);
        }
        std::env::set_var("DESKTOPCRAFT_NATIVE_CHROME_ONLY","1");
    }
    // Experimental Shell View bridge is an explicit pre-host command. Nothing
    // in the normal DesktopCraft launch path enables this automatically.
    if let Some(code) = native_box::handle_cli(&args) {
        std::process::exit(code);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    // Agent interfaces never create a second GUI or desktop owner.
    // Keep stdout exclusively for CLI JSON / MCP JSON-RPC.
    if has("--mcp") {
        ai_tools::serve_mcp();
        std::process::exit(0);
    }
    if has("--tools-list") {
        println!("{}", serde_json::json!({"tools":ai_tools::tools()}));
        std::process::exit(0);
    }
    if has("--notices") {
        print!("{THIRD_PARTY_NOTICES}");
        std::process::exit(0);
    }
    if has("--show-explorer-icons") {
        shell::init();
        let mut visible = None;
        let mut snap = None;
        if let Some(view) = shell::DesktopView::acquire() {
            if view.icons_visible() == Some(false) {
                let _ = view.set_icons_visible(true);
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            visible = view.icons_visible();
        }
        if has("--restore-snap-to-grid") {
            if let Some(view) = shell::DesktopView::acquire() {
                if view.snap_to_grid() == Some(false) {
                    let _ = view.set_snap_to_grid(true);
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
                snap = view.snap_to_grid();
            }
        }
        println!(
            "{}",
            serde_json::json!({
                "explorer_icons_visible": visible,
                "explorer_align_to_grid": snap,
            })
        );
        std::process::exit(if visible == Some(true) { 0 } else { 1 });
    }
    if let Some(i) = args.iter().position(|a| a == "--tool") {
        let name = args.get(i + 1).map(String::as_str).unwrap_or("");
        let arguments = args.get(i + 2).map(String::as_str).unwrap_or("{}");
        std::process::exit(ai_tools::cli(name, arguments));
    }
    if has("--uninstall") {
        if host::data_dir().join("native-inbox-recovery.json").exists(){
            host::message("Cannot uninstall while Native Box icon recovery is pending. Restore the Explorer icon positions first.",false);
            std::process::exit(3);
        }
        if host::message(
            "Remove DesktopCraft from this Windows account?\n\nYour desktop files stay where they are, and your group layouts are kept in your profile in case you install it again.",
            true,
        ) {
            shell::init();
            if let Err(e) = host::uninstall() {
                host::event("ERROR", "uninstall.refused", "quiescence or lock preflight refused uninstall");
                host::message(&format!("DesktopCraft could not be removed safely:\n\n{e}"), false);
                std::process::exit(3);
            }
            // The detached cleanup reports its own final result after this process exits.
        }
        std::process::exit(0);
    }
    // The setup file installs when simply opened; with options (--command, --probe...)
    // it behaves like the program itself.
    if has("--install") || (host::is_setup_name(&exe) && args.len() == 1) {
        let this = env!("CARGO_PKG_VERSION");
        // Already installed: this setup updates that copy in place, keeping
        // the person's layout, rules and start-at-sign-in choice.
        let existing = host::installed_version();
        // The user already approved this newer GitHub update. The verified
        // installer may reuse the existing in-place upgrade without a second prompt.
        let approved_update = has("--approved-update")
            && host::is_setup_name(&exe)
            && existing.as_ref().is_some_and(|old| host::version_newer(this, old));
        let go = if approved_update {
            Some(host::autostart(None))
        } else {
            match &existing {
            Some(old) => {
                let old_label = if old.is_empty() { "an earlier version".to_string() } else { old.clone() };
                let question = if !old.is_empty() && host::version_newer(old, this) {
                    format!("A newer DesktopCraft ({old}) is already installed.\n\nReplace it with this older version ({this})?")
                } else if old == this {
                    format!("DesktopCraft {this} is already installed.\n\nInstall it again? Your boxes, pages and rules are kept.")
                } else {
                    format!("Update DesktopCraft from {old_label} to {this}?\n\nThe running DesktopCraft closes, is replaced and starts again. Your boxes, pages, rules and settings are kept.")
                };
                host::message(&question, true).then(|| host::autostart(None))
            }
            None => host::message(
                "Install DesktopCraft for this Windows account?\n\nIt organizes icons into groups. No administrator rights are needed; remove it later through Settings > Apps.",
                true,
            )
            // Opt-in, default No: nothing starts with Windows unless asked for.
            .then(|| {
                let login = host::ask_default_no(
                    "Start DesktopCraft automatically every time you sign in to Windows?\n\nYou can change this later in DesktopCraft Settings > Behavior.",
                );
                let workspace_root = host::data_dir();
                if !host::workspace_allows_onboarding(&workspace_root) {
                    // Reinstall after uninstall must reuse the saved layout.
                    return login;
                }
                let onboard = host::message(
                    "How would you like to set up your desktop boxes?\n\n• Click Yes for Recommended Onboarding layout:\n  - Center Box: General desktop icons + your Windows Downloads folder (List view)\n  - Right Drawer: Documents (Word, Excel, PDF) + your Windows Pictures folder\n\n• Click No to start with Custom boxes.",
                    true,
                );
                let choice_path = workspace_root.join("data/onboarding-choice.txt");
                if let Some(parent) = choice_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&choice_path, if onboard { "onboarding" } else { "custom" });
                login
            }),
            }
        };
        if let Some(login) = go {
            // Setup runs before the desktop UI initializes the Shell COM apartment.
            shell::init();
            // The updater is deliberately launched BEFORE the existing app
            // shuts down, so its source Setup EXE remains alive. Never install
            // while Native Explorer still has relocated icons or an active
            // recovery journal. The originating GUI must first close cleanly
            // and restore its original Explorer layout. No file or registry
            // modifications occur while this bounded wait is in progress.
            if approved_update && has("--wait-for-app-rollback") {
                let started=std::time::Instant::now();
                while started.elapsed()<std::time::Duration::from_secs(55)
                    && !update_install_quiescent() {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                if !update_install_quiescent() {
                    host::event("WARN","install.update_wait_timeout",
                        "original DesktopCraft session or native recovery still active");
                    host::message(
                        "DesktopCraft cannot update yet because the previous session has not safely closed and restored its Explorer icons. The installed version has NOT been replaced.",
                        false);
                    std::process::exit(5);
                }
            }
            match host::install(login) {
                Ok(mut commit) => {
                    commit.release_desktop_instance();
                    // Let a copy that was running finish closing first, or the new
                    // one would only hand it a command and exit.
                    for _ in 0..50 {
                        if !host::instance_running() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                    let installed = commit.exe().to_path_buf();
                    let mut start = std::process::Command::new(&installed);
                    if existing.is_some() {
                        // The new copy says which version it now is.
                        start.args(["--command", "updated"]);
                    }
                    match start.spawn() {
                        Ok(_) => {
                            if let Err(e) = commit.commit() {
                                host::event("WARN", "install.commit_marker_cleanup_failed", &e);
                            }
                            host::event(
                                "INFO",
                                if existing.is_some() {
                                    "install.updated"
                                } else {
                                    "install.completed"
                                },
                                "per-user registration succeeded",
                            );
                        }
                        Err(e) => {
                            let rollback_note = match commit.rollback() {
                                Ok(()) => "Rolled back executable, notices, shortcut, and registry.".to_string(),
                                Err(rb) => format!("Rollback incomplete: {rb}"),
                            };
                            host::event(
                                "ERROR",
                                "install.launch_failed",
                                "Windows rejected the installed executable",
                            );
                            host::message(
                                &format!("Installed copy could not start ({e}).\n\n{rollback_note}"),
                                false,
                            );
                            std::process::exit(4);
                        }
                    }
                }
                Err(e) => {
                    host::event(
                        "ERROR",
                        "install.failed",
                        "per-user installation did not finish",
                    );
                    host::message(&format!("Installation did not finish: {e}"), false);
                    std::process::exit(3);
                }
            }
        } else {
            host::log("install: declined");
        }
        std::process::exit(0);
    }
    if let Some(i) = args.iter().position(|a| a == "--diagnose") {
        // Read-only report for another PC: no hidden icons, file paths,
        // private names, rules, screenshots, installation or registry writes.
        shell::init();
        let monitors = host::monitors();
        let workspace = host::data_dir().join("data/workspace.json");
        let workspace_valid = workspace.is_file().then(|| State::load(&workspace).is_ok());
        let windows_version = std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_else(|| "unavailable".to_string());
        let report = serde_json::json!({
            "app": "DesktopCraft",
            "version": env!("CARGO_PKG_VERSION"),
            "architecture": std::env::consts::ARCH,
            "windows_version": windows_version,
            "windows_process_architecture": std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_default(),
            "windows_native_architecture": std::env::var("PROCESSOR_ARCHITEW6432").unwrap_or_default(),
            "monitor_count": monitors.len(),
            "primary_monitor_found": monitors.iter().any(|m| m.primary),
            "explorer_desktop_available": shell::DesktopView::acquire().is_some(),
            "desktop_entries_count": shell::desktop_entries().len(),
            "workspace_present": workspace.exists(),
            "workspace_valid": workspace_valid,
            "report_contains_personal_paths": false,
        });
        let out = args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| host::data_dir().join("desktopcraft-diagnostics.json"));
        let result = std::fs::write(&out, serde_json::to_vec_pretty(&report).unwrap_or_default());
        match result {
            Ok(()) => {
                host::event(
                    "INFO",
                    "diagnostics.generated",
                    "read-only report generated",
                );
                std::process::exit(0);
            }
            Err(_) => {
                host::event(
                    "ERROR",
                    "diagnostics.write_failed",
                    "report could not be saved",
                );
                std::process::exit(2);
            }
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--perf-shell-probe") {
        // Read-only Explorer profiling. Never hides icons or changes positions.
        shell::init();
        perf::enable();
        for _ in 0..8 {
            let start = std::time::Instant::now();
            let files = shell::desktop_entries();
            perf::record("desktop_entries_ms", start.elapsed());
            perf::record_value("desktop_entry_count", files.len() as f64);
            let start = std::time::Instant::now();
            let view = shell::DesktopView::acquire();
            perf::record("desktop_view_acquire_ms", start.elapsed());
            if let Some(view) = view {
                let start = std::time::Instant::now();
                let items = view.items();
                perf::record("explorer_items_ms", start.elapsed());
                perf::record_value("explorer_icon_count", items.len() as f64);
            }
            perf::sample_memory();
        }
        let path = args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("phase-d-explorer-read-only.json"));
        if let Err(e) = perf::finish(&path, "explorer_read_only", 0) {
            host::log(&format!("perf probe failed: {e}"));
        }
        std::process::exit(0);
    }
    #[cfg(windows)]
    if let Some(i)=args.iter().position(|a|a=="--native-restore-exact-snapshot") {
        // Emergency rollback of only icon coordinates from a backed-up
        // read-only --probe JSON. Never copy/move/rename a Desktop file.
        // Require explicit developer flags and a FULL identity bijection.
        if !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Snapshot restore requires both native safety acknowledgement flags");
            std::process::exit(2);
        }
        let Some(path)=args.get(i+1) else {
            eprintln!("usage: --native-restore-exact-snapshot <saved-before.json> --unsafe-native-test --acknowledge-explorer-risk");
            std::process::exit(2);
        };
        let source=std::path::Path::new(path);
        let base=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3)
                .map(|root|root.join("checks/explorer-owned")))
            .unwrap_or_default();
        let trusted=std::fs::canonicalize(source).ok().zip(
            std::fs::canonicalize(base).ok()).is_some_and(|(src,root)|
            src.starts_with(root) && src.extension().is_some_and(|ext|ext=="json"));
        if !trusted {
            eprintln!("Refusing to restore icon positions from outside this project's verified checks folder");
            std::process::exit(2);
        }
        let data=std::fs::read(source).ok()
            .and_then(|bytes|serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        let Some(snapshot)=data else {
            eprintln!("Snapshot not readable as JSON");std::process::exit(2);
        };
        // Older probes recorded the logical (DPI-unaware) monitor signature;
        // --probe now records physical pixels. On a mixed-scale desktop the
        // two differ, so accept a snapshot matching either form.
        let logical=host::signature(&host::monitors());
        if !enable_native_audit_physical_dpi() {
            eprintln!("Cannot verify physical-coordinate restoration");
            std::process::exit(2);
        }
        let physical=host::signature(&host::monitors());
        let recorded=snapshot["monitor_signature"].as_str();
        if recorded!=Some(logical.as_str()) && recorded!=Some(physical.as_str()) {
            eprintln!("Monitor layout changed since snapshot; refusing automated rollback");
            std::process::exit(2);
        }
        shell::init();
        let Some(view)=shell::DesktopView::acquire() else {
            eprintln!("Native Explorer Desktop unavailable");std::process::exit(2);
        };
        if view.icons_visible()!=Some(true) {
            eprintln!("Desktop native icons not visible; refusing rollback");
            std::process::exit(2);
        }
        let current=view.items();
        let Some(known)=snapshot["icon_positions"].as_array() else {
            eprintln!("Snapshot lacks icon positions");std::process::exit(2);
        };
        if known.len()!=current.len() || known.len()<10 || known.len()>2048 {
            eprintln!("Native item count differs from snapshot; refusing rollback");
            std::process::exit(2);
        }
        let mut prior=std::collections::BTreeMap::new();
        for item in known {
            let (Some(path),Some(x),Some(y))=(item["path"].as_str(),
                item["x"].as_i64(),item["y"].as_i64()) else {
                eprintln!("Snapshot item missing exact identity and coordinates");
                std::process::exit(2);
            };
            if x<i32::MIN as i64 || x>i32::MAX as i64 ||
                y<i32::MIN as i64 || y>i32::MAX as i64 ||
                prior.insert(path.to_lowercase(),(x as i32,y as i32)).is_some() {
                eprintln!("Snapshot has invalid/duplicate native coordinates");
                std::process::exit(2);
            }
        }
        let current_keys=current.iter().map(|i|i.path.to_lowercase())
            .collect::<std::collections::HashSet<_>>();
        if current_keys.len()!=current.len() ||
            prior.keys().any(|name|!current_keys.contains(name)) {
            eprintln!("Desktop native identities changed; refusing rollback");
            std::process::exit(2);
        }
        let moves=current.iter().filter_map(|item|{
            let old=prior.get(&item.path.to_lowercase())?;
            (item.pos!=*old).then(||(item.path.clone(),*old))
        }).collect::<Vec<_>>();
        if moves.len()>30 {
            eprintln!("Too many icons moved since snapshot; refusing broad restoration");
            std::process::exit(2);
        }
        let attempted=view.position(&moves);
        std::thread::sleep(std::time::Duration::from_millis(650));
        let final_items=view.items();
        let outstanding=final_items.iter().filter(|item|
            prior.get(&item.path.to_lowercase())
                .is_none_or(|old|*old!=item.pos)).count();
        let (hr,requested,matched)=shell::DesktopView::last_position_result();
        println!("{}",serde_json::json!({
            "command":"restore_native_icon_positions_from_verified_snapshot",
            "source":path,
            "requested_restore_count":moves.len(),
            "matched_native_items":matched,
            "attempted_native_items":attempted,
            "position_hresult":hr,
            "position_requests":requested,
            "remaining_position_differences":outstanding,
            "native_items_after":final_items.len(),
            "workspace_modified":false,
            "desktop_files_moved_or_renamed":false,
            "restore_complete":outstanding==0,
        }));
        std::process::exit(if outstanding==0 {0}else{1});
    }
    #[cfg(windows)]
    if let Some(i)=args.iter().position(|a|a=="--native-position-one-item") {
        // Diagnostic only: restore ONE exact native Explorer icon position
        // from an already verified --probe snapshot. LVM_SETITEMPOSITION
        // carries packed integer coordinates, not an unsafe remote pointer.
        if !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Single-item native coordinate restore requires two explicit safety flags");
            std::process::exit(2);
        }
        let (Some(file),Some(wanted))=(args.get(i+1),args.get(i+2)) else {
            eprintln!("usage: --native-position-one-item <verified-before.json> <exact-icon-path> --unsafe-native-test --acknowledge-explorer-risk");
            std::process::exit(2);
        };
        let save_root=std::env::current_exe().ok()
            .and_then(|e|e.ancestors().nth(3).map(|p|p.join("checks/explorer-owned")))
            .unwrap_or_default();
        let trusted=std::fs::canonicalize(file).ok().zip(
            std::fs::canonicalize(save_root).ok())
            .is_some_and(|(source,root)|source.starts_with(root)
                && source.extension().is_some_and(|ext|ext=="json"));
        if !trusted {eprintln!("Refusing unverified snapshot path");std::process::exit(2);}
        let Some(raw)=std::fs::read(file).ok()
            .and_then(|bytes|serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            else {eprintln!("Snapshot JSON unavailable");std::process::exit(2);};
        if raw["monitor_signature"].as_str()!=Some(host::signature(&host::monitors()).as_str()){
            eprintln!("Monitor topology changed; single-icon restore refused");
            std::process::exit(2);
        }
        if !enable_native_audit_physical_dpi(){
            eprintln!("Physical-pixel desktop context unavailable");
            std::process::exit(2);
        }
        shell::init();
        let Some(view)=shell::DesktopView::acquire() else {
            eprintln!("Explorer Desktop not available");std::process::exit(2);
        };
        if view.icons_visible()!=Some(true) {
            eprintln!("Explorer native icons not visible");std::process::exit(2);
        }
        let before=view.items();
        let Some(entries)=raw["icon_positions"].as_array() else {
            eprintln!("Snapshot icon coordinates unavailable");std::process::exit(2);
        };
        let current=before.iter().map(|v|v.path.to_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let snapshot_names=entries.iter().filter_map(|x|x["path"].as_str())
            .map(str::to_lowercase).collect::<std::collections::HashSet<_>>();
        if current.len()!=before.len() || snapshot_names!=current ||
            before.len()!=entries.len() {
            eprintln!("Full native Desktop namespace identity check failed");
            std::process::exit(2);
        }
        let hits=entries.iter().filter(|entry|entry["path"].as_str()
            .is_some_and(|v|v.eq_ignore_ascii_case(wanted))).collect::<Vec<_>>();
        if hits.len()!=1 {eprintln!("Snapshot exact icon target is ambiguous");std::process::exit(2);}
        let Some((x,y))=hits[0]["x"].as_i64().zip(hits[0]["y"].as_i64())
            else {eprintln!("Snapshot lacks target coordinates");std::process::exit(2);};
        if x<0 || x>=32700 || y<0 || y>=32700 {
            eprintln!("Refusing parked/offscreen target coordinates");
            std::process::exit(2);
        }
        let names=explorer_owned_box::win::list_items(view.list_window())
            .into_iter().map(|it|it.name).collect::<Vec<_>>();
        let Ok(ordered)=explorer_native::native_view_in_msaa_order(&before,&names) else {
            eprintln!("Native ListView/MSAA identity reconciliation failed");
            std::process::exit(2);
        };
        let indexes=ordered.iter().enumerate()
            .filter(|(_,item)|item.path.eq_ignore_ascii_case(wanted))
            .map(|(index,_)|index).collect::<Vec<_>>();
        if indexes.len()!=1 {
            eprintln!("Native item has ambiguous ListView index");std::process::exit(2);
        }
        let original=before.iter().find(|it|it.path.eq_ignore_ascii_case(wanted)).unwrap();
        let target=[x as i32,y as i32];
        let mut client=winapi::shared::windef::POINT{x:target[0],y:target[1]};
        unsafe{winapi::um::winuser::ScreenToClient(view.list_window(),&mut client);}
        let within=(i16::MIN as i32..=i16::MAX as i32).contains(&client.x)
            && (i16::MIN as i32..=i16::MAX as i32).contains(&client.y);
        if !within {eprintln!("Native client point exceeds Win32 packed coordinate range");std::process::exit(2);}
        let packed=(u32::from(client.x as u16)
            |(u32::from(client.y as u16)<<16)) as isize;
        use winapi::um::winuser::SendMessageTimeoutW;
        let mut result=0usize;
        let delivered=unsafe{SendMessageTimeoutW(view.list_window(),0x1000+15,
            indexes[0],packed,0x0002,850,&mut result)}!=0;
        std::thread::sleep(std::time::Duration::from_millis(350));
        let after=view.items();
        let final_item=after.iter().find(|it|it.path.eq_ignore_ascii_case(wanted));
        let others_changed=before.iter().filter(|b|!b.path.eq_ignore_ascii_case(wanted)
            && after.iter().any(|a|a.path.eq_ignore_ascii_case(&b.path)
                && b.pos!=a.pos)).count();
        let restored=delivered && final_item.is_some_and(|a|a.pos==(
            target[0],target[1]));
        println!("{}",serde_json::json!({
            "mode":"single_item_native_integer_position_probe",
            "source_snapshot":file,
            "path":wanted,
            "native_index":indexes[0],
            "old_position":original.pos,
            "requested_position":target,
            "new_position":final_item.map(|a|a.pos),
            "message_delivered":delivered,
            "message_result":result,
            "restored":restored,
            "other_icons_changed":others_changed,
            "workspace_modified":false,
            "files_moved":false
        }));
        std::process::exit(if restored && others_changed==0 {0}else{1});
    }
    #[cfg(windows)]
    if has("--native-bundle-audit") {
        let expected = args.iter().position(|arg| arg == "--expect-native-dll");
        let output=match native_bundle::embedded_filter(){
            Ok(bytes)=>serde_json::json!({
                "version":env!("CARGO_PKG_VERSION"),
                "native_bundle_embedded":true,
                "native_filter_bytes":bytes.len(),
                "native_bundle_matches_expected":expected.map(|index| args.get(index + 1)
                    .and_then(|path|std::fs::read(path).ok()).is_some_and(|dll| dll == bytes)),
                "executable":std::env::current_exe().ok(),
                "disk_or_workspace_changed":false,
                "release_enabled_for_users":false
            }),
            Err(error)=>serde_json::json!({
                "native_bundle_embedded":false,
                "reason":error,
                "release_enabled_for_users":false
            })
        };
        let success=output["native_bundle_embedded"]==true
            && (expected.is_none() || output["native_bundle_matches_expected"] == true);
        println!("{output}");
        std::process::exit(if success{0}else{2});
    }
    #[cfg(windows)]
    if has("--native-inbox-recovery-status") || has("--native-inbox-recover") {
        let write=has("--native-inbox-recover");
        if write && (!has("--acknowledge-explorer-risk")
            || !has("--acknowledge-bulk-explorer-hide")
            || !has("--unsafe-native-test")){
            eprintln!("Manual Native Box recovery requires all Explorer risk confirmations");
            std::process::exit(2);
        }
        if !enable_native_audit_physical_dpi(){
            eprintln!("Native recovery refused without physical screen coordinates");
            std::process::exit(2);
        }
        shell::init();
        let Some(view)=shell::DesktopView::acquire() else {
            eprintln!("Native recovery refused: Explorer view unavailable");
            std::process::exit(2);
        };
        if native_explorer_guard::desktop_owner(&view).is_err()
            || (write && !view.native_placement_settings(false)) {
            eprintln!("Native recovery rejected: Explorer owner or arrangement could not be verified");
            std::process::exit(2);
        }
        if write && (host::instance_running()
            || native_filter_host::native_policy_status()["mapping_absence_confirmed"] != true) {
            eprintln!("Native Box recovery requires DesktopCraft to exit and Explorer callbacks to detach");
            std::process::exit(2);
        }
        match native_box_live_layout::recovery_status_or_restore(&view,write,has("--reconcile-legacy-grid")){
            Ok(report)=>{
                println!("{report}");
                // A recovery CLI is terminal: returning from startup()
                // would initialize the full native/legacy GUI and could
                // inadvertently hide desktop icons immediately after repair.
                std::process::exit(0);
            }
            Err(error)=>{
                println!("{}",serde_json::json!({
                    "status":"RECOVERY_BLOCKED","reason":error,
                    "manual_workspace_restore_performed":false}));
                std::process::exit(3);
            }
        }
    }
    #[cfg(windows)]
    if has("--native-box-geometry-audit") {
        // RELEASE READINESS AUDIT: no hook, no input, no Explorer item moves.
        // Explicitly rejects the earlier false-positive: Tab headers can
        // switch even if every Explorer icon remains outside its Box.
        if !enable_native_audit_physical_dpi(){
            eprintln!("Native Box geometry audit requires physical DPI");
            std::process::exit(2);
        }
        shell::init();
        let Some(desktop)=shell::DesktopView::acquire() else{
            eprintln!("Native Explorer Desktop unavailable");
            std::process::exit(2)
        };
        let items=desktop.items();
        let workspace=host::data_dir().join("data/workspace.json");
        let state=match model::State::load(&workspace){
            Ok(s)=>s,Err(e)=>{eprintln!("Workspace audit unavailable: {e}");std::process::exit(2)}
        };
        let spacing=desktop.spacing().unwrap_or((75,107));
        let screens=host::monitors().iter().map(|m|native_box_placement::Rect{
            x:m.rect[0],y:m.rect[1],w:m.rect[2],h:m.rect[3]
        }).collect::<Vec<_>>();
        let mut groups=Vec::new();
        let mut total_missing=0usize;
        let mut total_overflow=0usize;
        let mut all_active_in_box=true;
        for id in [144u64,168u64]{
            let Some(group)=state.group(id) else{
                eprintln!("Required native Box{id} not in persisted Workspace");
                std::process::exit(2)
            };
            let bounds=native_box_placement::Rect{
                x:group.bounds.x.round() as i32,y:group.bounds.y.round() as i32,
                w:group.bounds.w.round() as i32,h:group.bounds.h.round() as i32
            };
            let cols=((bounds.w-79)/spacing.0).max(0) as usize;
            let rows=((bounds.h-79)/spacing.1).max(0) as usize;
            let capacity=cols.saturating_mul(rows);
            let mut tab_reports=Vec::new();
            for (index,tab) in group.tabs.iter().enumerate(){
                if tab.folder.is_some(){
                    tab_reports.push(serde_json::json!({
                        "index":index,"name":tab.name,"portal":true,
                        "native_icon_count":0,"capacity":0}));
                    continue;
                }
                let missing=tab.items.iter().filter(|pin|!items.iter()
                    .any(|icon|icon.path.eq_ignore_ascii_case(&pin.path)))
                    .map(|pin|pin.path.clone()).collect::<Vec<_>>();
                let real=tab.items.iter().filter(|pin|items.iter()
                    .any(|icon|icon.path.eq_ignore_ascii_case(&pin.path)))
                    .collect::<Vec<_>>();
                let within=real.iter().filter(|pin|items.iter().any(|icon|
                    icon.path.eq_ignore_ascii_case(&pin.path)
                    && icon.pos.0>=bounds.x && icon.pos.0+spacing.0<bounds.x+bounds.w
                    && icon.pos.1>=bounds.y && icon.pos.1+spacing.1<bounds.y+bounds.h))
                    .count();
                total_missing+=missing.len();
                total_overflow+=real.len().saturating_sub(capacity);
                if index==group.active && (within!=real.len() || !missing.is_empty()){
                    all_active_in_box=false;
                }
                let proposals=native_box_placement::plan_box_cells(
                    bounds,&tab.items.iter().map(|p|p.path.clone())
                        .collect::<Vec<_>>(),
                    &items.iter().map(|i|(i.path.clone(),i.pos))
                        .collect::<Vec<_>>(),spacing,&screens);
                tab_reports.push(serde_json::json!({
                    "index":index,"name":tab.name,"active":index==group.active,
                    "native_icon_count":real.len(),"saved_pin_count":tab.items.len(),
                    "missing_saved_pin_paths":missing,
                    "currently_physically_inside_box":within,
                    "estimated_visible_native_cell_capacity":capacity,
                    "needs_scroll_or_overflow":real.len()>capacity,
                    "pure_batch_planner_eligible":proposals.is_ok(),
                    "pure_batch_planner_blocker":proposals.err(),
                }));
            }
            groups.push(serde_json::json!({
                "group_id":id,"box_bounds":[bounds.x,bounds.y,bounds.w,bounds.h],
                "active_tab":group.active,"tabs":tab_reports,
            }));
        }
        let ready=total_missing==0 && total_overflow==0 && all_active_in_box;
        println!("{}",serde_json::json!({
            "mode":"read_only_original_box_native_icon_geometry_release_audit",
            "desktop_icons":items.len(),"explorer_hooks_installed":false,
            "workspace_or_icons_modified":false,
            "native_icon_spacing":spacing,
            "native_boxes":groups,
            "stale_saved_pin_count":total_missing,
            "overflow_native_icon_count":total_overflow,
            "all_active_native_icons_inside_their_boxes":all_active_in_box,
            "actual_in_box_geometry_release_gate":ready,
        }));
        std::process::exit(0);
    }
    #[cfg(windows)]
    if has("--native-monitor-audit") {
        if !enable_native_audit_physical_dpi() {
            eprintln!("Cannot establish physical-pixel DPI awareness for native monitor audit; refusing misleading coordinates");
            std::process::exit(3);
        }
        // Read-only, physical-pixel-aware diagnostics. WinForms in a DPI-
        // unaware PowerShell host can report *logical* dimensions (e.g.
        // 2048x1152 for a 2560x1440 monitor at 125%) while IFolderView icon
        // positions are in physical screen coordinates. Do not confuse them.
        shell::init();
        let monitors=host::monitors();
        let virtual_rect=host::virtual_rect();
        let items=shell::DesktopView::acquire()
            .map(|v|v.items()).unwrap_or_default();
        let outside=items.iter().filter(|item|{
            !monitors.iter().any(|m|{
                let (x,y)=item.pos;
                x>=m.rect[0] && y>=m.rect[1]
                    && x<m.rect[0]+m.rect[2]
                    && y<m.rect[1]+m.rect[3]
            })
        }).count();
        let described=monitors.iter().map(|m|serde_json::json!({
            "key":m.key,"physical_rect":m.rect,
            "physical_work_area":m.work,"dpi":m.dpi,
            "scale":m.dpi as f64/96.0,"primary":m.primary,
        })).collect::<Vec<_>>();
        println!("{}",serde_json::json!({
            "mode":"native_monitor_readonly_audit",
            "monitor_count":monitors.len(),
            "physical_virtual_rect":virtual_rect,
            "physical_monitor_signature":host::signature(&monitors),
            "monitors":described,
            "native_icon_count":items.len(),
            "native_icons_outside_physical_monitors":outside,
            "explorer_hooks_installed":false,
            "desktop_file_changes":0,
            "icon_position_changes":0,
            "workspace_changes":0,
            "important":"Use physical monitor geometry for native IFolderView screen coordinates; DPI-unaware tools may return smaller logical dimensions."
        }));
        std::process::exit(if monitors.is_empty(){1}else{0});
    }
    #[cfg(windows)]
    if has("--native-multibox-audit") {
        // Policy-planning only: never inject DLLs or hide a single Explorer
        // item. Require complete current Shell/ListView identity reconciliation.
        if !enable_native_audit_physical_dpi() {
            eprintln!("Multi-Box audit requires physical-screen DPI context");
            std::process::exit(3);
        }
        shell::init();
        let Some(view)=shell::DesktopView::acquire() else {
            eprintln!("Native Explorer Desktop View unavailable");
            std::process::exit(3);
        };
        let shell_items=view.items();
        let labels=explorer_owned_box::win::list_items(view.list_window())
            .into_iter().map(|it|it.name).collect::<Vec<_>>();
        let Ok(items)=explorer_native::native_view_in_msaa_order(&shell_items,&labels) else {
            eprintln!("Native Shell/ListView identities are ambiguous; no policy was generated");
            std::process::exit(3);
        };
        let Ok(state)=State::load(&host::data_dir().join("data/workspace.json")) else {
            eprintln!("DesktopCraft persisted workspace unavailable");
            std::process::exit(3);
        };
        let result=explorer_native::native_multi_box_plan(&state,&items,16,64);
        match result {
            Ok(plan)=>{
                let hidden=plan.hidden_for(&[]).unwrap_or_default();
                let groups=plan.groups.iter().map(|group|serde_json::json!({
                    "group_id":group.group_id,
                    "active_tab":group.active,
                    "tab_names":group.names,
                    "tab_native_item_counts":group.native_indices.iter()
                        .map(Vec::len).collect::<Vec<_>>(),
                    "planned_hidden_items":group.hidden_for(group.active).unwrap_or_default().len(),
                })).collect::<Vec<_>>();
                let skipped=plan.skipped.iter().map(|(id,reason)|serde_json::json!({
                    "group_id":id,"reason":reason,
                })).collect::<Vec<_>>();
                println!("{}",serde_json::json!({
                    "mode":"native_multibox_readonly_audit",
                    "validated_native_items":items.len(),
                    "planned_group_count":groups.len(),
                    "groups":groups,
                    "skipped_groups":skipped,
                    "would_hide_if_enabled":hidden.len(),
                    "unclaimed_native_items":plan.unclaimed_native_items,
                    "actual_items_hidden":0,
                    "hook_installed":false,
                    "files_or_workspace_modified":false,
                    "note":"Read-only multi-Box policy model; not evidence of installed multi-group filtering",
                }));
                std::process::exit(0);
            },
            Err(reason)=>{
                println!("{}",serde_json::json!({
                    "mode":"native_multibox_readonly_audit",
                    "validation_passed":false,
                    "reason":reason,
                    "actual_items_hidden":0,
                    "hook_installed":false,
                }));
                std::process::exit(3);
            }
        }
    }
    #[cfg(windows)]
    if has("--native-ole-drop-audit") {
        // Read-only OLE path->virtual Pin audit using four REAL Explorer Desktop
        // identities in an in-memory two-Box fixture. This does not register
        // an OLE target, inject a DLL, send mouse input or save State.
        if !enable_native_audit_physical_dpi() {
            eprintln!("Physical DPI context is required for read-only OLE audit");
            std::process::exit(3);
        }
        shell::init();
        let Some(view)=shell::DesktopView::acquire() else {
            eprintln!("Explorer Desktop unavailable for read-only OLE audit");
            std::process::exit(3);
        };
        let before=view.items();
        let names=explorer_owned_box::win::list_items(view.list_window())
            .into_iter().map(|item|item.name).collect::<Vec<_>>();
        let Ok(items)=explorer_native::native_view_in_msaa_order(&before,&names) else {
            eprintln!("Explorer identities could not be reconciled");
            std::process::exit(3);
        };
        let Ok(saved)=State::load(&host::data_dir().join("data/workspace.json")) else {
            eprintln!("Existing DesktopCraft workspace cannot be read");
            std::process::exit(3);
        };
        let Some(group)=saved.group(144) else {
            eprintln!("Native four-item fixture source group not present");
            std::process::exit(3);
        };
        if group.tabs.len()<3 || group.tabs[1].items.len()<2 ||
            group.tabs[2].items.len()<2 {
            eprintln!("Native OLE read-only fixture needs two items in Craft and Work tabs");
            std::process::exit(3);
        }
        let source=[group.tabs[1].items[0].clone(),
            group.tabs[2].items[0].clone(),
            group.tabs[1].items[1].clone(),
            group.tabs[2].items[1].clone()];
        let mut fixture=State::empty();
        for (number,part) in [[0usize,1usize],[2usize,3usize]].iter().enumerate() {
            let id=fixture.add_group(&format!("OLE Audit {}",number+1),
                Bounds{x:100.+number as f32*360.,y:80.,w:320.,h:260.});
            fixture.group_mut(id).unwrap().tab_mut().items.push(source[part[0]].clone());
            if fixture.add_tab(id).is_err() {
                eprintln!("Could not construct read-only OLE Tab fixture");
                std::process::exit(3);
            }
            fixture.group_mut(id).unwrap().tab_mut().items.push(source[part[1]].clone());
            fixture.group_mut(id).unwrap().active=0;
        }
        let audit=(||->Result<serde_json::Value,String>{
            let plan=explorer_native::native_multi_box_plan(&fixture,&items,2,2)?;
            if plan.groups.len()!=2 {
                return Err("OLE fixture requires exactly two verified native Boxes".into());
            }
            let from=fixture.groups[0].id;
            let to=fixture.groups[1].id;
            let to_tab=fixture.groups[1].tabs[0].id;
            let paths=[std::path::PathBuf::from(&source[0].path)];
            let (updated,after_plan)=explorer_native::plan_native_ole_drop(
                &fixture,&items,&plan,&paths,to,to_tab,2,2)?;
            if updated.group(from).unwrap().tabs[0].items.len()!=0 ||
               updated.group(to).unwrap().tabs[0].items.len()!=2 {
                return Err("OLE preview did not produce expected virtual Pin counts".into());
            }
            let hidden_before=plan.hidden_for(&[])?.len();
            let hidden_after=after_plan.hidden_for(&[])?.len();
            Ok(serde_json::json!({
                "validated":true,
                "source_native_path":source[0].path,
                "source_box_pin_count_before":1,
                "source_box_pin_count_after":0,
                "target_box_pin_count_before":1,
                "target_box_pin_count_after":2,
                "hidden_items_before":hidden_before,
                "hidden_items_after":hidden_after,
                "filesystem_operation":false,
                "persistent_workspace_save":false,
            }))
        })();
        let current=view.items();
        let unchanged=before.len()==current.len() && before.iter().all(|old|
            current.iter().any(|new|new.path.eq_ignore_ascii_case(&old.path)
                && new.pos==old.pos));
        let success=audit.is_ok() && unchanged;
        println!("{}",serde_json::json!({
            "mode":"native_ole_drop_readonly_audit",
            "explorer_item_count":before.len(),
            "native_item_positions_unchanged":unchanged,
            "hook_installed":false,
            "mouse_input_sent":false,
            "ole_drop_target_registered":false,
            "files_moved":false,
            "real_workspace_modified":false,
            "ole_virtual_pin_preview":audit.unwrap_or_else(|reason|
                serde_json::json!({"validated":false,"reason":reason})),
        }));
        std::process::exit(if success {0}else{3});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if has("--native-multibox-fixture-test") {
        // Refuse the legacy physical Explorer MouseDown experiment BEFORE
        // reading DPI, Shell, the user's workspace, or loading any DLL.
        let drag_capture=has("--native-drag-capture");
        if has("--native-drag-persist-isolated") &&
            (!drag_capture || !has("--native-drag-capture-multi")) {
            eprintln!("Isolated native persistent multi-drag requires --native-drag-capture --native-drag-capture-multi");
            std::process::exit(2);
        }
        if drag_capture && !cfg!(feature="native-unsafe-drag-lab") {
            eprintln!("Legacy Explorer MouseDown capture is disabled in this build; only an isolated VM test build with --features native-unsafe-drag-lab can enable it");
            std::process::exit(2);
        }
        // Explicit lab mode. Temporarily group FOUR existing real Explorer
        // icons into TWO independent logical Boxes in a disposable State.
        // Do not create, move, rename, delete or reposition any Desktop file.
        if !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Multi-Box native fixture requires both --unsafe-native-test and --acknowledge-explorer-risk");
            std::process::exit(2);
        }
        if !enable_native_audit_physical_dpi() {
            eprintln!("Cannot validate DPI physical desktop before native fixture");
            std::process::exit(2);
        }
        if drag_capture {
            if !cfg!(debug_assertions) {
                eprintln!("Native drag capture fixture is disabled in Release");
                std::process::exit(2);
            }
            let Some(ready_path)=args.iter().position(|a|a=="--native-drag-ready-file")
                .and_then(|i|args.get(i+1))
                .filter(|s|!s.starts_with("--"))
                else {
                    eprintln!("Native drag fixture requires --native-drag-ready-file <absolute temporary path>");
                    std::process::exit(2);
                };
            let ready=std::path::Path::new(ready_path);
            if !ready.is_absolute() || ready.parent().is_none_or(|dir|!dir.is_dir())
                || ready.exists() {
                eprintln!("Native drag ready path must be a NEW absolute path in an existing directory");
                std::process::exit(2);
            }
            std::env::set_var("DESKTOPCRAFT_NATIVE_DRAG_TEST","1");
            std::env::set_var("DESKTOPCRAFT_NATIVE_DRAG_READY_FILE",ready_path);
        }
        shell::init();
        let source=match State::load(&host::data_dir().join("data/workspace.json")) {
            Ok(state)=>state,
            Err(reason)=>{eprintln!("Cannot read real workspace: {reason}");std::process::exit(2)}
        };
        // A known 3/2 real-item pair from the current Workspace is merely a
        // SOURCE of four unique existing Shell identities. No source State
        // object is modified or saved.
        let Some(existing)=source.group(144) else {
            eprintln!("Missing original Native test Box 144");
            std::process::exit(2);
        };
        if existing.tabs.len()<3 || existing.tabs[1].items.len()<2
            || existing.tabs[2].items.len()<2 {
            eprintln!("Native multi-Box fixture requires Craft/Work with two real icons each");
            std::process::exit(2);
        }
        let selections=[
            existing.tabs[1].items[0].clone(),
            existing.tabs[2].items[0].clone(),
            existing.tabs[1].items[1].clone(),
            existing.tabs[2].items[1].clone()
        ];
        let mut selections=selections;
        if drag_capture {
            if has("--native-drag-capture-multi") {
                // Only this explicit lab mode verifies the real native
                // selection of TWO visible Craft icons in ONE source Tab.
                std::env::set_var("DESKTOPCRAFT_NATIVE_DRAG_MULTI_TEST","1");
            }
            // On this Windows workspace the first Craft icon overlaps an
            // unrelated Explorer-owned modal Properties dialog. Choose the
            // second existing Craft icon as source instead of attempting to
            // close or click through any dialog. No Shell positions change.
            selections.swap(0,2);
            if has("--native-drag-capture-multi") {
                // Craft icon 2 and Craft icon 1 form source Box 0;
                // Work icons form destination Box 1. Keep the actual user
                // Workspace untouched; only this disposable fixture changes.
                selections.swap(1,2);
            }
        }
        let unique=selections.iter()
            .map(|item|item.path.to_lowercase())
            .collect::<std::collections::HashSet<_>>();
        if unique.len()!=4 {
            eprintln!("Native multi-Box fixture paths overlap; hook refused");
            std::process::exit(2);
        }
        let mut fixture=State::empty();
        for (box_index,pair) in [[0usize,1usize],[2usize,3usize]].iter().enumerate(){
            let id=fixture.add_group(
                &format!("Native Test Box {}",box_index+1),
                Bounds{x:100.+box_index as f32*360.,y:80.,w:320.,h:260.});
            fixture.group_mut(id).unwrap().tab_mut().items
                .push(selections[pair[0]].clone());
            if fixture.add_tab(id).is_err(){
                eprintln!("Cannot create isolated native fixture Tab");
                std::process::exit(2);
            }
            fixture.group_mut(id).unwrap().tab_mut().items
                .push(selections[pair[1]].clone());
            if drag_capture && has("--native-drag-capture-multi") && box_index==0 {
                // Both real preselected Craft shortcuts are in the SAME
                // visible Source Tab; the second Tab remains empty.
                // The user's genuine Workspace never receives this fixture.
                let group=fixture.group_mut(id).unwrap();
                let second=group.tabs[1].items.remove(0);
                group.tabs[0].items.push(second);
            }
            fixture.group_mut(id).unwrap().active=0;
        }
        let unique_stamp=std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|t|t.as_millis()).unwrap_or(0);
        let temp=std::env::temp_dir().join(format!(
            "DesktopCraftNativeTwoBox-{}-{}",std::process::id(),unique_stamp));
        let workspace=temp.join("workspace.json");
        if std::fs::create_dir_all(&temp).is_err()
            || fixture.save(&workspace).is_err() {
            eprintln!("Could not create isolated native fixture; hook refused");
            std::process::exit(2);
        }
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3).map(|root|
                root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_multibox_fixture_probe(12,&workspace,&dll);
        // Model-only dry-run AFTER native hook detaches. It uses four real
        // Shell identities but does not send any mouse events, move files or
        // commit a layout. A rejected transfer must leave State untouched.
        let drop_preview=(||->Result<serde_json::Value,String>{
            if drag_capture {
                return if report["native_drag_virtual_drop_preview"]["validated"]==true {
                    Ok(report["native_drag_virtual_drop_preview"].clone())
                }else{
                    Err(format!("Real native drag preview rejected: {}",
                        report["native_drag_virtual_drop_preview"]["reason"]))
                };
            }
            let view=shell::DesktopView::acquire()
                .ok_or("Explorer view unavailable after Multi-Box test")?;
            let raw=view.items();
            let labels=explorer_owned_box::win::list_items(view.list_window())
                .into_iter().map(|item|item.name).collect::<Vec<_>>();
            let items=explorer_native::native_view_in_msaa_order(&raw,&labels)?;
            let from_group=&fixture.groups[0];
            let to_group=&fixture.groups[1];
            let (after,plan)=explorer_native::plan_native_pin_transfer(
                &fixture,&items,from_group.id,from_group.tabs[0].id,
                &selections[0].path,to_group.id,to_group.tabs[0].id,2,2)?;
            Ok(serde_json::json!({
                "validated":true,
                "mode":"uncommitted_virtual_pin_transfer",
                "source_pin_count_before":from_group.tabs[0].items.len(),
                "source_pin_count_after":after.groups[0].tabs[0].items.len(),
                "destination_pin_count_before":to_group.tabs[0].items.len(),
                "destination_pin_count_after":after.groups[1].tabs[0].items.len(),
                "new_hidden_count":plan.hidden_for(&[])?.len(),
                "windows_mouse_drag_sent":false,
                "filesystem_move_performed":false,
                "real_workspace_saved":false,
            }))
        })();
        let drop_validated=drop_preview.is_ok();
        let isolated_multi_persist=
            if drag_capture && has("--native-drag-capture-multi")
                && has("--native-drag-persist-isolated") {
                Some((||->Result<serde_json::Value,String>{
                    // This is deliberately NOT the user's real DesktopCraft
                    // Workspace. Only a fresh per-run TEMP fixture may be
                    // modified, and only after the genuine Explorer gesture,
                    // release, preview, icon-position audit and DLL detach.
                    let canonical=temp.canonicalize()
                        .map_err(|e|format!("Temp fixture unavailable: {e}"))?;
                    let expected_root=std::env::temp_dir().canonicalize()
                        .map_err(|e|format!("OS temp directory unavailable: {e}"))?;
                    if !canonical.starts_with(&expected_root)
                        || !canonical.file_name().is_some_and(|n|
                            n.to_string_lossy().starts_with("DesktopCraftNativeTwoBox-"))
                        || workspace!=temp.join("workspace.json") {
                        return Err("Refusing non-disposable Workspace for real multi-drag persistence".into());
                    }
                    if report["native_drag_accepted_without_reposition"]!=true
                        || report["native_drag_virtual_drop_preview"]["validated"]!=true
                        || report["native_drag_virtual_drop_preview"]["selected_items_count"]!=2
                        || report["callback_detached"]!=true
                        || report["all_native_icon_positions_unchanged"]!=true {
                        return Err("Real native multi-drag did not pass every preflight for temporary persistence".into());
                    }
                    let offered=report["native_drag_virtual_drop_preview"]["selected_native_paths"]
                        .as_array().filter(|paths|paths.len()==2)
                        .ok_or("Real Explorer selection did not contain two source paths")?
                        .iter().map(|value|value.as_str()
                            .filter(|s|!s.is_empty())
                            .map(PathBuf::from)
                            .ok_or("A real native selected source path is invalid"))
                        .collect::<Result<Vec<_>,_>>()?;
                    if offered.iter().any(|path|!path.is_file()) {
                        return Err("An offered native source file disappeared".into());
                    }
                    let view=shell::DesktopView::acquire()
                        .ok_or("Explorer Desktop disappeared before staging")?;
                    let native_names=explorer_owned_box::win::list_items(view.list_window())
                        .into_iter().map(|item|item.name).collect::<Vec<_>>();
                    let current_native=explorer_native::native_view_in_msaa_order(
                        &view.items(),&native_names)?;
                    let sig=host::signature(&host::monitors());
                    let virtual_screen=host::virtual_rect();
                    if virtual_screen[2]<=0 || virtual_screen[3]<=0 {
                        return Err("Native physical desktop viewport unavailable".into());
                    }
                    let viewport=native_ole_gate::NativeOleViewport{
                        screen_origin_px:(virtual_screen[0],virtual_screen[1]),
                        client_size_px:(virtual_screen[2] as u32,virtual_screen[3] as u32),
                        logical_size:(virtual_screen[2] as f64,virtual_screen[3] as f64),
                    };
                    let end=&report["native_drag_telemetry"]["end_physical"];
                    let x=end[0].as_i64().ok_or("Native drag endpoint X missing")?;
                    let y=end[1].as_i64().ok_or("Native drag endpoint Y missing")?;
                    let x=i32::try_from(x).map_err(|_|"Native drag X out of range")?;
                    let y=i32::try_from(y).map_err(|_|"Native drag Y out of range")?;
                    let visible=fixture.groups.iter().map(|g|g.id).collect::<Vec<_>>();
                    let mut next=fixture.clone();
                    let transaction=native_ole_persist::NativeOlePersistedGesture::begin(
                        &workspace,&fixture,&current_native,&sig,
                        viewport,&offered,&visible,2,4)?;
                    transaction.commit(&mut next,&current_native,&sig,
                        viewport,&visible,&offered,(x,y),4)?;
                    let on_disk=State::load(&workspace)?;
                    if serde_json::to_vec(&next).map_err(|e|e.to_string())?
                        !=serde_json::to_vec(&on_disk).map_err(|e|e.to_string())?
                        || next.groups[0].tabs[0].items.len()!=0
                        || next.groups[1].tabs[0].items.len()!=3 {
                        return Err("Multi-drag staging postflight Workspace content mismatch".into());
                    }
                    if std::fs::read_dir(&temp).map_err(|e|e.to_string())?.count()!=1 {
                        return Err("Multi-drag staging left a recovery artifact for review".into());
                    }
                    if offered.iter().any(|path|!path.is_file()) {
                        return Err("Multi-drag persisted but one original file was moved".into());
                    }
                    Ok(serde_json::json!({
                        "validated":true,
                        "mode":"user-approved-real-selection-to-disposable-workspace",
                        "selected_native_files":offered.len(),
                        "persisted_virtual_pin_items":2,
                        "destination_tab_items":3,
                        "source_tab_items":0,
                        "workspace_saved_to":"unique_disposable_temp_only",
                        "disk_state_matches_memory":true,
                        "original_source_files_preserved":true,
                        "recovery_artifacts_left":0,
                        "real_user_workspace_modified":false,
                        "shell_file_operations":false,
                        "real_explorer_ole_link_effect_checked":false
                    }))
                })())
            }else{None};
        let isolated_persist_passed=isolated_multi_persist.as_ref()
            .is_none_or(|result|result.is_ok());
        let temp_deleted=std::fs::remove_dir_all(&temp).is_ok();
        let mut report=report;
        report["isolated_fixture_removed"]=serde_json::json!(temp_deleted);
        report["real_workspace_modified_by_test"]=serde_json::json!(false);
        report["source_paths_count"]=serde_json::json!(4);
        report["virtual_pin_drop_preview"]=drop_preview.unwrap_or_else(|reason|
            serde_json::json!({"validated":false,"reason":reason}));
        report["isolated_multi_pin_persist"]=isolated_multi_persist
            .map(|result|result.unwrap_or_else(|reason|
                serde_json::json!({"validated":false,"reason":reason})))
            .unwrap_or_else(||serde_json::json!({"requested":false}));
        println!("{}",serde_json::to_string_pretty(&report).unwrap_or_default());
        std::process::exit(if (if drag_capture {
                report["native_drag_accepted_without_reposition"]==true
            }else{
                report["multi_box_trial_completed"]==true
            })
            && report["callback_detached"]==true
            && report["all_native_icon_positions_unchanged"]==true
            && report["stats"]["attached_during_probe"]==true
            && drop_validated && isolated_persist_passed && temp_deleted {0}else{1});
    }
    if let Some((i, action)) = args.iter().enumerate().find_map(|(i, a)| match a.as_str() {
        "--explorer-bridge-probe" => Some((i, "probe")),
        "--explorer-bridge-audit" => Some((i, "audit")),
        "--explorer-bridge-select" => Some((i, "select")),
        "--explorer-bridge-menu" => Some((i, "menu")),
        _ => None,
    }) {
        if action == "menu" && !cfg!(all(debug_assertions, feature = "native-lab")) {
            eprintln!("The Explorer context-message probe is available only in a Debug native-lab build");
            std::process::exit(2);
        }
        // Explorer-first laboratory commands. Probe is read-only and the
        // explicit menu probe is separate from desktop mouse dispatch.
        shell::init();
        let target = args
            .get(i + 1)
            .map(String::as_str)
            .filter(|p| !p.starts_with("--"));
        let view = shell::DesktopView::acquire();
        let result = match action {
            "probe" => Ok(()),
            "select" => target
                .ok_or("Provide the exact existing Desktop icon path")
                .and_then(|p| {
                    view.as_ref()
                        .ok_or("Explorer Desktop View unavailable")
                        .map_err(|_| "Explorer Desktop View unavailable")
                        .and_then(|v| {
                            v.select_explorer_icon(p).map_err(|_| {
                                "Selection failed or target not in Explorer Desktop View"
                            })
                        })
                }),
            "menu" => target
                .ok_or("Provide the exact existing Desktop icon path")
                .and_then(|p| {
                    view.as_ref()
                        .ok_or("Explorer Desktop View unavailable")
                        .map_err(|_| "Explorer Desktop View unavailable")
                        .and_then(|v| {
                            v.request_explorer_context_menu(p).map_err(|_| {
                                "Explorer menu request refused or Desktop icons hidden"
                            })
                        })
                }),
            _ => Ok(()),
        };
        let probe = view
            .as_ref()
            .map(|v| v.explorer_bridge_probe(target))
            .unwrap_or_else(
                || serde_json::json!({"bridge":"explorer-desktop-view-lab","available":false}),
            );
        let audit = if action == "audit" {
            let workspace = host::data_dir().join("data/workspace.json");
            State::load(&workspace).ok().map(|state| {
                let items = view.as_ref().map(|v| v.items()).unwrap_or_default();
                let coverage = explorer_bridge::analyze(&state, &items).to_json();
                let presentation = explorer_native::NativePresentation::decide(&state, &items).summary();
                #[cfg(windows)]
                let native_names: Vec<String> = view.as_ref()
                    .map(|v| explorer_owned_box::win::list_items(v.list_window())
                        .into_iter().map(|i| i.name).collect())
                    .unwrap_or_default();
                #[cfg(not(windows))]
                let native_names: Vec<String> = Vec::new();
                let native_index = explorer_native::audit_native_indices(&items, &native_names).to_json();
                (coverage, presentation, native_index)
            })
        } else {
            None
        };
        let success = result.is_ok();
        println!(
            "{}",
            serde_json::json!({
                "mode":action,
                "success":success,
                "diagnostics":probe,
                "workspace_coverage":audit.as_ref().map(|(coverage, _, _)| coverage),
                "native_presentation":audit.as_ref().map(|(_, presentation, _)| presentation),
                "native_index_audit":audit.as_ref().map(|(_, _, index)| index),
                "message":match action {
                    "probe"=>"Read-only probe complete; no Desktop files, selections or positions were changed",
                    "audit"=>"Read-only Explorer/DesktopCraft workspace coverage audit complete; counts only, no paths disclosed or state changed",
                    "select" if success=>"Explorer item selection requested; no file or icon positions changed",
                    "menu" if success=>"Explorer context-menu message queued; appearance of modern menu NOT verified",
                    _=>"The bridge could not complete the request; no file or icon positions changed",
                }
            })
        );
        std::process::exit(if success { 0 } else { 1 });
    }
    #[cfg(windows)]
    if let Some(i)=args.iter().position(|a|a=="--native-input-preflight") {
        // Pure read-only Shell/MSAA reconciliation before any future native
        // input experiment. No native DLL, Windows hook, pointer-bearing
        // cross-process calls, files or icon positions are modified.
        shell::init();
        let Some(index)=args.get(i+1).and_then(|v|v.parse::<usize>().ok()) else {
            eprintln!("usage: --native-input-preflight <item-index>");
            std::process::exit(2);
        };
        let workspace=host::data_dir().join("data/workspace.json");
        let report=native_filter_host::native_input_preflight(&workspace,index);
        explorer_owned::emit(&report);
        std::process::exit(if report["preflight_passed"]==true {0}else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i)=args.iter().position(|a|a=="--native-input-rightclick-probe") {
        // Developer-only test: DLL posts a synthetic right-button pair and
        // WM_CONTEXTMENU to a single, temporarily hidden native item.
        // Every event must be vetoed before reaching Explorer's menu layer.
        if !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Native context-menu test requires two explicit safety flags");
            std::process::exit(2);
        }
        shell::init();
        let Some(index)=args.get(i+1).and_then(|v|v.parse::<usize>().ok()) else {
            eprintln!("usage: --native-input-rightclick-probe <item-index> [seconds] --unsafe-native-test --acknowledge-explorer-risk");
            std::process::exit(2);
        };
        let seconds=args.get(i+2).and_then(|v|v.parse::<u32>().ok()).unwrap_or(4);
        let workspace=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3)
                .map(|root|root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_input_rightclick_probe(
            seconds,&workspace,&dll,index);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"]==true
            && report["callback_detached"]==true
            && report["stats"]["native_diag_right_suppressed"].as_u64().unwrap_or(0)>=2
            && report["stats"]["native_diag_menu_suppressed"].as_u64().unwrap_or(0)>=1 {
            0
        }else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i)=args.iter().position(|a|a=="--native-input-hover-probe") {
        // Explicit test-only native input exercise: Explorer DLL finds a
        // validated item's RECT and posts a synthetic WM_MOUSEMOVE onto its
        // own ListView. No mouse cursor movement, file operation or menu.
        if !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Hover probe requires both --unsafe-native-test and --acknowledge-explorer-risk");
            std::process::exit(2);
        }
        shell::init();
        let Some(index)=args.get(i+1).and_then(|v|v.parse::<usize>().ok()) else {
            eprintln!("usage: --native-input-hover-probe <item-index> [seconds] --unsafe-native-test --acknowledge-explorer-risk");
            std::process::exit(2);
        };
        let seconds=args.get(i+2).and_then(|v|v.parse::<u32>().ok()).unwrap_or(4);
        let workspace=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3)
                .map(|root|root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_input_hover_probe(
            seconds,&workspace,&dll,index);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"]==true
            && report["callback_detached"]==true
            && report["stats"]["native_diag_hover_suppressed"].as_u64().unwrap_or(0)>0 {
            0
        }else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i)=args.iter().position(|a|a=="--native-input-observe") {
        // Experimental in-Explorer hit-test remains UNVALIDATED after a
        // Windows Desktop/OneDrive incident. The safer default is always the
        // read-only --native-input-preflight command above. This explicitly
        // requires TWO developer gates and must never run on app startup.
        // Integer-only messages still enter Explorer; no host pointers.
        if !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Native in-Explorer input test is disabled by default. Run --native-input-preflight first; live experiment requires BOTH --unsafe-native-test and --acknowledge-explorer-risk.");
            std::process::exit(2);
        }
        shell::init();
        let Some(index)=args.get(i+1).and_then(|v|v.parse::<usize>().ok()) else {
            eprintln!("usage: --native-input-observe <item-index> [seconds] --unsafe-native-test");
            std::process::exit(2);
        };
        let seconds=args.get(i+2).and_then(|v|v.parse::<u32>().ok()).unwrap_or(4);
        let workspace=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3)
                .map(|root|root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_input_observe(seconds,&workspace,&dll,index);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"]==true
            && report["callback_detached"]==true
            && report["input_diagnostic_passes"].as_u64().unwrap_or(0)>0 {0}else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i) = args.iter().position(|a| a == "--native-filter-hook-observe") {
        if !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Native observer requires explicit risk acknowledgements");
            std::process::exit(2);
        }
        // Opt-in and observer-only: install our own native hook on the
        // Explorer desktop UI thread, count drawing notifications, detach.
        // The hook never requests icon paint or input suppression here.
        shell::init();
        let seconds = args.get(i + 1).and_then(|v| v.parse::<u32>().ok()).unwrap_or(3);
        let workspace = host::data_dir().join("data/workspace.json");
        let dll = std::env::current_exe().ok()
            .and_then(|exe| exe.ancestors().nth(3)
                .map(|root| root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report = native_filter_host::native_hook_observe(seconds, &workspace, &dll, None);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"] == true && report["callback_detached"] == true {0}else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i) = args.iter().position(|a| a == "--native-filter-hide-probe") {
        // Explicitly gated single-native-icon experiment. It must never
        // activate with a normal user launch or a read-only audit.
        if !has("--unsafe-native-test") {
            eprintln!("Native visibility test requires --unsafe-native-test");
            std::process::exit(2);
        }
        shell::init();
        let Some(index) = args.get(i + 1).and_then(|n| n.parse::<usize>().ok()) else {
            eprintln!("usage: --native-filter-hide-probe <index> [seconds] --unsafe-native-test");
            std::process::exit(2);
        };
        let seconds = args.get(i + 2).and_then(|n| n.parse::<u32>().ok()).unwrap_or(4);
        let workspace = host::data_dir().join("data/workspace.json");
        let dll = std::env::current_exe().ok()
            .and_then(|exe| exe.ancestors().nth(3)
                .map(|root| root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report = native_filter_host::native_hook_observe(seconds, &workspace, &dll, Some(index));
        explorer_owned::emit(&report);
        std::process::exit(if report["started"] == true && report["callback_detached"] == true {0}else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i)=args.iter().position(|a|a=="--native-filter-tab-switch-probe") {
        // Native A->B->A safety trial: one Explorer icon from each existing
        // DesktopCraft Tab. Workspace is read only, other icons pass through.
        // Explicit --unsafe-native-test is mandatory; no normal product path
        // automatically installs Explorer hooks.
        if !has("--unsafe-native-test") {
            eprintln!("Native A-B-A test requires --unsafe-native-test");
            std::process::exit(2);
        }
        shell::init();
        let Some(group_id)=args.get(i+1).and_then(|s|s.parse::<u64>().ok()) else {
            eprintln!("usage: --native-filter-tab-switch-probe <group-id> [seconds] [to-tab] --unsafe-native-test");
            std::process::exit(2);
        };
        let seconds=args.get(i+2).and_then(|v|v.parse::<u32>().ok()).unwrap_or(12);
        let to_tab=args.get(i+3).and_then(|v|v.parse::<usize>().ok());
        let workspace=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3)
                .map(|root|root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_tab_switch_probe(
            seconds,&workspace,&dll,group_id,to_tab);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"]==true
            && report["callback_detached"]==true
            && report["tab_a_b_a_completed"]==true {0}else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i)=args.iter().position(|a|a=="--native-filter-tab-full-probe") {
        // Strict scoped test: A/B groups must be regular native desktop
        // Tabs with at most three unique native icons in each. Nothing about
        // the stored workspace/Page/Tab is changed by this debug-only mode.
        if !has("--unsafe-native-test") {
            eprintln!("Native complete-tab test requires --unsafe-native-test");
            std::process::exit(2);
        }
        shell::init();
        let Some(group_id)=args.get(i+1).and_then(|s|s.parse::<u64>().ok()) else {
            eprintln!("usage: --native-filter-tab-full-probe <group-id> [seconds] [to-tab] --unsafe-native-test");
            std::process::exit(2);
        };
        let seconds=args.get(i+2).and_then(|v|v.parse::<u32>().ok()).unwrap_or(12);
        let to_tab=args.get(i+3).and_then(|v|v.parse::<usize>().ok());
        let workspace=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3)
                .map(|root|root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_tab_full_probe(
            seconds,&workspace,&dll,group_id,to_tab);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"]==true
            && report["callback_detached"]==true
            && report["tab_a_b_a_completed"]==true {0}else{1});
    }
    #[cfg(windows)]
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i)=args.iter().position(|a|a=="--native-filter-tab-click-probe") {
        // A clickable, small developer Tab header controlling the *real*
        // Explorer native icon filter. It never starts DesktopCraft's legacy
        // replica-icon renderer or changes saved workspace Tab selection.
        if !has("--unsafe-native-test") {
            eprintln!("Interactive Native Tab requires --unsafe-native-test");
            std::process::exit(2);
        }
        shell::init();
        let Some(group_id)=args.get(i+1).and_then(|s|s.parse::<u64>().ok()) else {
            eprintln!("usage: --native-filter-tab-click-probe <group-id> [seconds] [to-tab] --unsafe-native-test");
            std::process::exit(2);
        };
        let seconds=args.get(i+2).and_then(|s|s.parse::<u32>().ok()).unwrap_or(35);
        let to_tab=args.get(i+3).and_then(|s|s.parse::<usize>().ok());
        let workspace=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3)
                .map(|root|root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_tab_click_probe(
            seconds,&workspace,&dll,group_id,to_tab,has("--automated-ui-test"));
        explorer_owned::emit(&report);
        std::process::exit(if report["started"]==true
            && report["callback_detached"]==true
            && report["tab_a_b_a_completed"]==true {0}else{1});
    }
    #[cfg(windows)]
    if has("--native-filter-policy-status") {
        shell::init();
        let report=native_filter_host::native_policy_status();
        explorer_owned::emit(&report);
        std::process::exit(0);
    }
    #[cfg(windows)]
    if has("--native-filter-emergency-off") {
        shell::init();
        let report = native_filter_host::emergency_unhook();
        explorer_owned::emit(&report);
        std::process::exit(if report["command_sent"] == true {0}else{1});
    }
    #[cfg(windows)]
    if let Some(i) = args.iter().position(|a| a == "--native-filter-policy-preview") {
        // Publishes a briefly leased native-visibility policy into shared
        // memory, validates it in-process, then destroys the mapping.
        // Importantly this NEVER installs or activates Explorer hooks.
        shell::init();
        let seconds = args.get(i + 1).and_then(|v| v.parse::<u32>().ok()).unwrap_or(2);
        let workspace = host::data_dir().join("data/workspace.json");
        let report = native_filter_host::policy_preview(seconds, &workspace);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"] == true { 0 } else { 1 });
    }
    // Explorer-owned icons plan, Phase 0: make the desktop usable again.
    if let Some(i) = args.iter().position(|a| a == "--recover-desktop") {
        shell::init();
        let snapshot = args.get(i + 1).filter(|p| !p.starts_with("--")).map(PathBuf::from);
        let report = explorer_owned::recover_desktop(snapshot.as_deref());
        explorer_owned::emit(&report);
        std::process::exit(if report["recovered"] == true { 0 } else { 1 });
    }
    // Phase 1: one DesktopCraft frame around ONE real Explorer-owned icon.
    if let Some(i) = args.iter().position(|a| a == "--explorer-owned-lab") {
        shell::init();
        let Some(target) = args.get(i + 1).filter(|p| !p.starts_with("--")) else {
            eprintln!("usage: --explorer-owned-lab <desktop item path> [seconds] [report.json] [x y]");
            std::process::exit(2);
        };
        let seconds = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(120);
        let num = |k: usize| args.get(i + k).and_then(|v| v.parse::<i32>().ok());
        let place = num(4).zip(num(5));
        // Optional measured hole: dx dy w h relative to Explorer's position.
        let hole = match (num(6), num(7), num(8), num(9)) {
            (Some(a), Some(b), Some(c), Some(d)) => Some((a, b, c, d)),
            _ => None,
        };
        let report = explorer_owned::run_lab(target, seconds, place, hole);
        if let Some(out) = args.get(i + 3) {
            let _ = std::fs::write(out, serde_json::to_vec_pretty(&report).unwrap_or_default());
        }
        explorer_owned::emit(&report);
        std::process::exit(if report["started"] == true { 0 } else { 1 });
    }
    // Non-destructive native underlay compositor preview. It draws a single
    // Box background below Explorer's real desktop ListView for a few seconds,
    // and leaves all icons, positions, selections and files untouched.
    if let Some(i) = args.iter().position(|a| a == "--explorer-underlay-probe") {
        shell::init();
        let n = |k: usize| args.get(i + k).and_then(|v| v.parse::<i32>().ok());
        let (Some(x), Some(y), Some(w), Some(h)) = (n(1), n(2), n(3), n(4)) else {
            eprintln!("usage: --explorer-underlay-probe <x> <y> <width> <height> [seconds]");
            std::process::exit(2);
        };
        let seconds = n(5).unwrap_or(10).max(2) as u32;
        let report = explorer_owned_box::run_underlay_probe([x,y,w,h], seconds);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"] == true { 0 } else { 1 });
    }
    // Phase 2: one DesktopCraft box frame around SEVERAL Explorer-owned icons.
    if let Some(i) = args.iter().position(|a| a == "--explorer-owned-box") {
        shell::init();
        let num = |k: usize| args.get(i + k).and_then(|v| v.parse::<i32>().ok());
        let (Some(list), Some(x), Some(y), Some(cols)) = (args.get(i + 1), num(2), num(3), num(4)) else {
            eprintln!("usage: --explorer-owned-box <items.txt> <x> <y> <columns> [seconds] [status.json] [title]");
            std::process::exit(2);
        };
        let paths = explorer_owned_box::parse_icon_path_list(&std::fs::read_to_string(list).unwrap_or_default());
        let seconds = num(5).unwrap_or(180).max(5) as u32;
        let status = args.get(i + 6).filter(|p| !p.starts_with("--")).map(PathBuf::from);
        let title = args.get(i + 7).cloned().unwrap_or_else(|| "DesktopCraft box".into());
        let report = explorer_owned_box::run_box_lab(&paths, (x, y), cols.max(1) as usize, seconds, status, &title);
        explorer_owned::emit(&report);
        std::process::exit(if report["started"] == true { 0 } else { 1 });
    }
    if let Some(i)=args.iter().position(|a|a=="--native-target-create") {
        // One deliberate pre-window transaction. NO native Explorer Hook,
        // NO physical Shell file operation and NO automatic normal startup.
        // The exact independent checkpoint and read-only candidate report
        // must agree with the CURRENT Explorer icon/order/monitor snapshot.
        let (Some(source),Some(checkpoint),Some(report_path),Some(expected_post))=
            (args.get(i+1).and_then(|v|v.parse::<u64>().ok()),
             args.get(i+2),args.get(i+3),args.get(i+4)) else{
            eprintln!("Usage: --native-target-create <source-id> <absolute-checkpoint> <absolute-preflight-report> <new-absolute-expected-post> --confirm-native-target-create");
            std::process::exit(2);
        };
        if !has("--confirm-native-target-create"){
            eprintln!("Native target creation refused: explicit --confirm-native-target-create required");
            std::process::exit(2);
        }
        if !host::first_instance(){
            eprintln!("Native target creation refused: DesktopCraft already running");
            std::process::exit(2);
        }
        if !enable_native_audit_physical_dpi(){
            eprintln!("Native target creation requires physical DPI context");
            std::process::exit(2);
        }
        shell::init();
        let workspace=if let Some(index)=args.iter().position(|a|a=="--native-target-test-workspace"){
            let source=args.get(index+1).map(PathBuf::from)
                .unwrap_or_default();
            let valid=source.canonicalize().ok().is_some_and(|w|{
                let temp=std::env::temp_dir().canonicalize().ok();
                w.file_name().is_some_and(|n|n=="workspace.json")
                    && w.parent().is_some_and(|p|p.file_name().is_some_and(|n|n=="data"))
                    && w.parent().and_then(|p|p.parent()).is_some_and(|root|
                        root.file_name().is_some_and(|n|n.to_string_lossy()
                            .starts_with("DesktopCraftNativeTarget-"))
                        && root.parent()==temp.as_deref())
            });
            if !has("--isolated-workspace-only") || !valid {
                eprintln!("Native target test Workspace requires unique OS TEMP DesktopCraftNativeTarget-*/data/workspace.json");
                std::process::exit(2);
            }
            source
        }else{
            host::data_dir().join("data/workspace.json")
        };
        let result=(||->Result<serde_json::Value,String>{
            let old_bytes=std::fs::read(&workspace).map_err(|e|e.to_string())?;
            let state=State::load(&workspace)?;
            let view=shell::DesktopView::acquire()
                .ok_or("Explorer Desktop ListView missing during Native target creation")?;
            if view.icons_visible()!=Some(true) {
                return Err("Explorer Desktop icons must remain visible during Native Box creation".into());
            }
            let names=explorer_owned_box::win::list_items(view.list_window())
                .into_iter().map(|v|v.name).collect::<Vec<_>>();
            let items=explorer_native::native_view_in_msaa_order(&view.items(),&names)?;
            let monitors=host::monitors();
            let signature=host::signature(&monitors);
            let proposed=native_target::plan_new_native_destination(
                &state,&items,&monitors,source)?;
            let saved_report=serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(report_path).map_err(|e|
                    format!("Native target preflight report unavailable: {e}"))?)
                .map_err(|e|format!("Native target preflight JSON invalid: {e}"))?;
            let bounds=proposed.bounds;
            if saved_report["mode"]!="native-target-readonly-preflight"
                || saved_report["valid"]!=true
                || saved_report["workspace_written"]!=false
                || saved_report["native_hook_installed"]!=false
                || saved_report["source_group_id"]!=source
                || saved_report["proposed_destination_group_id"]!=proposed.destination_id
                || saved_report["proposed_destination_tab_id"]!=proposed.destination_tab
                || saved_report["proposed_bounds"]!=serde_json::json!({
                    "x":bounds.x,"y":bounds.y,"w":bounds.w,"h":bounds.h})
                || saved_report["monitor_signature"]!=signature
                || saved_report["physical_monitor"]!=proposed.monitor
                || saved_report["native_icon_count"]!=items.len()
                || saved_report["eligible_native_group_count_after_proposal"]!=proposed.policy.groups.len()
                || saved_report["source_and_destination_verified"]!=true {
                return Err("Native target creation refused: read-only preflight stale or different from current Explorer Desktop".into());
            }
            let check_checkpoint=std::fs::read(checkpoint)
                .map_err(|e|format!("Native target safety checkpoint missing: {e}"))?;
            if old_bytes!=check_checkpoint{
                return Err("Native target create refused: checkpoint is not byte-identical to current user Workspace".into());
            }
            // Refuse if monitor topology or any real icon was modified
            // after the fresh target policy was constructed.
            if host::signature(&host::monitors())!=signature
                || std::fs::read(&workspace).map_err(|e|e.to_string())?!=old_bytes {
                return Err("Native target creation refused: monitor or Workspace changed during preflight".into());
            }
            let expected=PathBuf::from(expected_post);
            let bytes=native_ole_persist::commit_checked_native_destination(
                &workspace,&PathBuf::from(checkpoint),&expected,
                &state,&proposed.state)?;
            let check=State::load(&workspace)?;
            if serde_json::to_vec(&check).map_err(|e|e.to_string())?
                !=serde_json::to_vec(&proposed.state).map_err(|e|e.to_string())? {
                return Err("Native destination on-disk State differed after atomic replace; rollback checkpoint retained".into());
            }
            let renewed=shell::DesktopView::acquire()
                .ok_or("Explorer disappeared after Native target saved; manual review required")?;
            let after=explorer_native::native_view_in_msaa_order(
                &renewed.items(),
                &explorer_owned_box::win::list_items(renewed.list_window())
                    .into_iter().map(|v|v.name).collect::<Vec<_>>())?;
            if after.len()!=items.len() ||
                after.iter().zip(&items).any(|(a,b)|
                    !a.path.eq_ignore_ascii_case(&b.path) || a.pos!=b.pos)
                || host::signature(&host::monitors())!=signature {
                return Err("Explorer icon layout changed during target commit; STOP, preserve user backup for manual recovery".into());
            }
            Ok(serde_json::json!({
                "mode":"confirmed-native-desktop-destination-created",
                "workspace_saved":true,
                "workspace_is_isolated_test":has("--native-target-test-workspace"),
                "source_group_id":source,
                "new_group_id":proposed.destination_id,
                "new_tab_id":proposed.destination_tab,
                "new_group_bounds":{"x":bounds.x,"y":bounds.y,"w":bounds.w,"h":bounds.h},
                "monitor":proposed.monitor,
                "native_eligible_group_count":proposed.policy.groups.len(),
                "expected_post_snapshot":expected,
                "independent_original_checkpoint":checkpoint,
                "bytes_committed":bytes,
                "real_explorer_icon_positions_unchanged":true,
                "explorer_icons_checked":items.len(),
                "native_hook_installed":false,
                "shell_file_operations":false,
                "rollback_is_separate_explicit_action":true
            }))
        })();
        let good=result.is_ok();
        let message=result.unwrap_or_else(|reason|serde_json::json!({
            "mode":"confirmed-native-desktop-destination-created",
            "saved":false,"reason":reason,
            "native_hook_installed":false,"shell_file_operations":false
        }));
        println!("{}",serde_json::to_string_pretty(&message).unwrap_or_default());
        std::process::exit(if good{0}else{3});
    }
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if has("--native-two-box-tab-cycle-probe") || has("--native-two-box-input-probe"){
        let input_trial=has("--native-two-box-input-probe");
        // Real Desktop two-Box policy transition, isolated experimental Debug
        // build only. The user-facing safe executable cannot enable this.
        if !cfg!(debug_assertions) || !cfg!(feature="native-lab")
            || !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk")
            || !has("--acknowledge-bulk-explorer-hide") {
            eprintln!("Native two-Box cycle requires isolated Debug native-lab and all three explicit risk acknowledgements");
            std::process::exit(2);
        }
        if !enable_native_audit_physical_dpi(){
            eprintln!("Native two-Box cycle requires physical DPI context");
            std::process::exit(2);
        }
        shell::init();
        let ws=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3).map(|root|
                root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_two_box_tab_cycle_probe(16,&ws,&dll,input_trial);
        println!("{}",serde_json::to_string_pretty(&report).unwrap_or_default());
        std::process::exit(if report["native_two_box_tab_cycle_completed"]==true
            && (!input_trial || report["native_hidden_input_completed"]==true)
            && report["staged_native_hide_peak_count"]
                .as_u64().is_some_and(|peak|peak<=34)
            && report["all_native_icon_positions_unchanged"]==true
            && report["callback_detached"]==true {0}else{1});
    }
    #[cfg(all(windows, debug_assertions, feature="native-lab", feature="native-unsafe-drag-lab"))]
    if let Some(i)=args.iter().position(|a|a=="--native-staged-hide-probe"){
        // Opt-in lab ONLY. Never included in normal builds. The underlying
        // 34-icon full visibility policy remains blocked by the 8-item cap.
        if !cfg!(debug_assertions) || !cfg!(feature="native-lab") ||
            !has("--unsafe-native-test") || !has("--acknowledge-explorer-risk") {
            eprintln!("Native staged hide requires Debug native-lab and both explicit risk gates");
            std::process::exit(2);
        }
        let Some(cap)=args.get(i+1).and_then(|s|s.parse::<usize>().ok()) else {
            eprintln!("usage: --native-staged-hide-probe <1..=34> --unsafe-native-test --acknowledge-explorer-risk [--acknowledge-bulk-explorer-hide]");
            std::process::exit(2);
        };
        if cap==0 || cap>native_live_membership::MAX_EXPERIMENTAL_STAGED_HIDE {
            eprintln!("Native staged hide limit must remain between 1 and 34");
            std::process::exit(2);
        }
        if cap>native_live_membership::MAX_SAFE_NATIVE_HIDE_ON_REAL_DESKTOP &&
            !has("--acknowledge-bulk-explorer-hide") {
            eprintln!("Debug bulk hide above eight items requires a third explicit acknowledgement");
            std::process::exit(2);
        }
        if !enable_native_audit_physical_dpi() {
            eprintln!("Native staged hide requires physical DPI context");
            std::process::exit(2);
        }
        shell::init();
        let ws=host::data_dir().join("data/workspace.json");
        let dll=std::env::current_exe().ok()
            .and_then(|exe|exe.ancestors().nth(3).map(|root|
                root.join("native-filter-build/debug/desktopcraft_native_filter.dll")))
            .unwrap_or_default();
        let report=native_filter_host::native_staged_hide_probe(12,&ws,&dll,cap);
        println!("{}",serde_json::to_string_pretty(&report).unwrap_or_default());
        std::process::exit(if report["staged_native_hide_completed"]==true &&
            report["staged_native_hide_peak_count"]==cap &&
            report["all_native_icon_positions_unchanged"]==true &&
            report["callback_detached"]==true {0}else{1});
    }
    if let Some(i)=args.iter().position(|a|a=="--native-two-box-policy-readonly"){
        // NO Explorer injection, no Windows menu click, no Workspace change.
        // This is a fresh per-Box/per-Tab membership index for GUI policy
        // planning after a durable Pin transfer.
        let output=args.get(i+1).map(PathBuf::from);
        if !enable_native_audit_physical_dpi() {
            eprintln!("Native policy requires physical DPI awareness");
            std::process::exit(2);
        }
        shell::init();
        let result=(||->Result<serde_json::Value,String>{
            let ws=host::data_dir().join("data/workspace.json");
            let before=std::fs::read(&ws).map_err(|e|e.to_string())?;
            let state=State::load(&ws)?;
            let desktop=shell::DesktopView::acquire()
                .ok_or("Real Explorer Desktop is unavailable")?;
            if desktop.icons_visible()!=Some(true) {
                return Err("Native read-only policy requires visible Explorer icons".into());
            }
            let labels=explorer_owned_box::win::list_items(desktop.list_window())
                .into_iter().map(|i|i.name).collect::<Vec<_>>();
            let items=explorer_native::native_view_in_msaa_order(
                &desktop.items(),&labels)?;
            let membership=native_live_membership::NativeTwoBoxMembership::prepare(
                1,&state,&items,144,168)?;
            let initial=membership.snapshot(&items)?;
            let safety=membership.safe_to_publish(&items)
                .map(|_|serde_json::json!({"safe_to_publish":true}))
                .unwrap_or_else(|reason|serde_json::json!({
                    "safe_to_publish":false,"reason":reason
                }));
            let mut candidates=Vec::new();
            for group in &membership.plan.groups{
                for index in 0..group.native_indices.len(){
                    let modified=membership.switch_tab(&state,&items,group.group_id,index)?;
                    let result=modified.snapshot(&items)?;
                    candidates.push(serde_json::json!({
                        "group_id":group.group_id,
                        "tab_index":index,
                        "native_policy_generation":result.generation,
                        "source_active_native_count":result.source_index_count,
                        "destination_active_native_count":result.target_index_count,
                        "would_hide_native_indices":result.hidden_indices
                    }));
                }
            }
            if std::fs::read(&ws).map_err(|e|e.to_string())?!=before{
                return Err("Real user Workspace was changed during native policy read-only survey".into());
            }
            Ok(serde_json::json!({
                "mode":"two-native-box-membership-read-only",
                "valid":true,
                "source_group":membership.source_group,
                "destination_group":membership.target_group,
                "native_item_count":items.len(),
                "current_source_active_count":initial.source_index_count,
                "current_destination_active_count":initial.target_index_count,
                "current_hidden_index_count":initial.hidden_indices.len(),
                "real_desktop_native_paint_hook_safety":safety,
                "all_tabs_previewed":candidates,
                "native_hooks_installed":false,
                "real_workspace_written":false,
                "native_icons_repositioned":false,
                "windows_11_context_menu_ownership":"Explorer remains the native menu owner"
            }))
        })();
        let okay=result.is_ok();
        let report=result.unwrap_or_else(|reason|serde_json::json!({
            "mode":"two-native-box-membership-read-only",
            "valid":false,"reason":reason,
            "native_hooks_installed":false,"real_workspace_written":false
        }));
        if let Some(path)=output{
            let _=std::fs::write(path,
                serde_json::to_vec_pretty(&report).unwrap_or_default());
        }
        println!("{}",serde_json::to_string_pretty(&report).unwrap_or_default());
        std::process::exit(if okay{0}else{3});
    }
    if let Some(i)=args.iter().position(|a|a=="--native-target-preflight") {
        // Read-only inspection of actual Explorer Desktop + user layout.
        // Never installs a hook, repositions an icon, or writes any Workspace.
        let source_id=args.get(i+1).and_then(|v|v.parse::<u64>().ok())
            .unwrap_or(144);
        if !enable_native_audit_physical_dpi() {
            eprintln!("Native target preflight needs physical DPI awareness");
            std::process::exit(2);
        }
        shell::init();
        let workspace=host::data_dir().join("data/workspace.json");
        let outcome=(||->Result<serde_json::Value,String>{
            let before=std::fs::read(&workspace).map_err(|e|e.to_string())?;
            let state=State::load(&workspace)?;
            let view=shell::DesktopView::acquire()
                .ok_or("Explorer Desktop ListView unavailable")?;
            if view.icons_visible()!=Some(true) {
                return Err("Native target requires visible real Explorer icons".into());
            }
            let names=explorer_owned_box::win::list_items(view.list_window())
                .into_iter().map(|v|v.name).collect::<Vec<_>>();
            let items=explorer_native::native_view_in_msaa_order(&view.items(),&names)?;
            let monitors=host::monitors();
            let proposal=native_target::plan_new_native_destination(
                &state,&items,&monitors,source_id)?;
            if std::fs::read(&workspace).map_err(|e|e.to_string())?!=before {
                return Err("Live user Workspace changed during read-only native target planning".into());
            }
            let target=proposal.bounds;
            Ok(serde_json::json!({
                "mode":"native-target-readonly-preflight",
                "valid":true,
                "source_group_id":proposal.source_id,
                "proposed_destination_group_id":proposal.destination_id,
                "proposed_destination_tab_id":proposal.destination_tab,
                "proposed_bounds":{"x":target.x,"y":target.y,"w":target.w,"h":target.h},
                "physical_monitor":proposal.monitor,
                "monitor_signature":host::signature(&monitors),
                "native_icon_count":items.len(),
                "eligible_native_group_count_after_proposal":proposal.policy.groups.len(),
                "source_and_destination_verified":
                    proposal.policy.groups.iter().any(|g|g.group_id==source_id)
                    && proposal.policy.groups.iter().any(|g|g.group_id==proposal.destination_id),
                "minimum_clearance_px":proposal.icon_clearance_px,
                "no_existing_boxes_modified":true,
                "workspace_written":false,
                "desktop_icons_repositioned":false,
                "native_hook_installed":false,
                "destination_is_only_a_proposal":true
            }))
        })();
        let good=outcome.is_ok();
        let report=outcome.unwrap_or_else(|reason|serde_json::json!({
            "mode":"native-target-readonly-preflight",
            "valid":false,"reason":reason,
            "workspace_written":false,
            "native_hook_installed":false,
            "desktop_icons_repositioned":false
        }));
        if let Some(out)=args.get(i+2).filter(|s|!s.starts_with("--")){
            let _=std::fs::write(out,
                serde_json::to_vec_pretty(&report).unwrap_or_default());
        }
        println!("{}",serde_json::to_string_pretty(&report).unwrap_or_default());
        std::process::exit(if good{0}else{3});
    }
    if let Some(i)=args.iter().position(|a|a=="--native-ole-readonly-audit") {
        // Read-only diagnostic against the currently saved user Box layout.
        // No hook, receiver, mouse event, icon state or Workspace writes.
        if !enable_native_audit_physical_dpi(){
            eprintln!("Native OLE audit requires physical-DPI-aware thread");
            std::process::exit(2);
        }
        shell::init();
        let source=host::data_dir().join("data/workspace.json");
        let outcome=(||->Result<serde_json::Value,String>{
            let state=State::load(&source)?;
            let desktop=shell::DesktopView::acquire()
                .ok_or("Real Explorer Desktop ListView is unavailable")?;
            if desktop.icons_visible()!=Some(true){
                return Err("Explorer icons must be visible for native policy audit".into());
            }
            let snapshot=desktop.items();
            let actual_names=explorer_owned_box::win::list_items(desktop.list_window())
                .into_iter().map(|item|item.name).collect::<Vec<_>>();
            let native=explorer_native::native_view_in_msaa_order(&snapshot,&actual_names)?;
            let group_records=state.groups.iter().map(|group|{
                let can=explorer_native::native_group_tab_plan(&state,&native,group.id,128);
                serde_json::json!({
                    "group_id":group.id,
                    "active_page":group.page==state.page,
                    "rolled_up":group.rolled,
                    "active_tab_index":group.active,
                    "tab_count":group.tabs.len(),
                    "tab_item_counts":group.tabs.iter().map(|tab|tab.items.len()).collect::<Vec<_>>(),
                    "folder_portal_tabs":group.tabs.iter().filter(|tab|tab.folder.is_some()).count(),
                    "single_box_native_policy_eligible":can.is_ok(),
                    "reason":can.err(),
                })
            }).collect::<Vec<_>>();
            let full=explorer_native::native_multi_box_plan(&state,&native,16,128);
            let (eligible_group_ids,global_error)=match full {
                Ok(plan)=>(plan.groups.iter().map(|g|g.group_id).collect::<Vec<_>>(),None),
                Err(e)=>(Vec::<u64>::new(),Some(e))
            };
            let signature=host::signature(&host::monitors());
            if signature.is_empty(){return Err("Physical desktop monitor signature unavailable".into());}
            let cross_box_eligible=eligible_group_ids.len()>=2;
            Ok(serde_json::json!({
                "mode":"readonly-native-ole-user-workspace-preflight",
                "workspace_exists":source.is_file(),
                "workspace_written":false,
                "native_hook_installed":false,
                "explorer_icons_changed":false,
                "native_icon_count":native.len(),
                "active_page":state.page,
                "monitor_signature":signature,
                "groups":group_records,
                "eligible_native_group_ids":eligible_group_ids,
                "cross_box_eligible":cross_box_eligible,
                "global_reason":global_error,
                "real_explorer_ole_link_effect_checked":false,
            }))
        })();
        let good=outcome.is_ok();
        let report=outcome.unwrap_or_else(|error|serde_json::json!({
            "mode":"readonly-native-ole-user-workspace-preflight",
            "eligible":false,"reason":error,
            "workspace_written":false,"native_hook_installed":false,
        }));
        if let Some(out)=args.get(i+1).filter(|s|!s.starts_with("--")){
            let path=PathBuf::from(out);
            let _=std::fs::write(&path,serde_json::to_vec_pretty(&report).unwrap_or_default());
        }
        println!("{}",serde_json::to_string_pretty(&report).unwrap_or_default());
        std::process::exit(if good{0}else{2});
    }
    if let Some(i) = args.iter().position(|a| a == "--probe") {
        // Read-only check of the live Windows integration: nothing is moved,
        // hidden or registered. Writes a JSON report and exits.
        // CRITICAL: on mixed 100/125%-DPI desktops a DPI-unaware thread
        // reports 2048 logical pixels for a real 2560-physical-pixel
        // monitor. Explorer icon positions and the saved recovery baseline
        // are PHYSICAL. Normalize before querying any monitor geometry.
        if !enable_native_audit_physical_dpi() {
            eprintln!("Read-only Explorer probe requires a physical-DPI aware thread");
            std::process::exit(2);
        }
        shell::init();
        let report = probe();
        let out = args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("probe.json"));
        let _ = std::fs::write(&out, serde_json::to_vec_pretty(&report).unwrap_or_default());
        std::process::exit(0);
    }
    #[cfg(windows)]
    if let Some(i) = args.iter().position(|a| a == "--position-test") {
        // Moves ONE desktop icon by one cell and straight back, reporting what
        // Explorer answered each time (diagnoses icons not moving under groups).
        shell::init();
        let mut report = serde_json::Map::new();
        if let Some(view) = shell::DesktopView::acquire() {
            let items = view.items();
            let pick = args
                .get(i + 2)
                .cloned()
                .or_else(|| items.first().map(|x| x.path.clone()));
            report.insert("icons".into(), items.len().into());
            if args.iter().any(|a| a == "--hidden-icons") {
                // With Explorer's "Show desktop icons" off: can positions still be
                // read and set? (Restores everything and turns icons back on.)
                if let Some(first) = items.iter().find(|x| !x.path.starts_with("::")).cloned() {
                    let was_visible = view.icons_visible();
                    view.set_icons_visible(false);
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    report.insert(
                        "hidden_flag_now_visible".into(),
                        format!("{:?}", view.icons_visible()).into(),
                    );
                    report.insert(
                        "hidden_read".into(),
                        format!("{:?} was {:?}", view.position_of(&first.path), first.pos).into(),
                    );
                    let (sx, _) = view.spacing().unwrap_or((75, 98));
                    view.position(&[(first.path.clone(), (first.pos.0 + sx, first.pos.1))]);
                    let r = shell::DesktopView::last_position_result();
                    report.insert(
                        "hidden_move_hresult".into(),
                        format!("0x{:08X} matched {}/{}", r.0, r.2, r.1).into(),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    report.insert(
                        "hidden_after_move".into(),
                        format!("{:?}", view.position_of(&first.path)).into(),
                    );
                    view.position(&[(first.path.clone(), first.pos)]);
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    view.set_icons_visible(was_visible.unwrap_or(true));
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    report.insert(
                        "restored".into(),
                        format!(
                            "{:?} visible {:?}",
                            view.position_of(&first.path),
                            view.icons_visible()
                        )
                        .into(),
                    );
                    report.insert(
                        "labels".into(),
                        items
                            .iter()
                            .take(4)
                            .map(|x| format!("{} -> {}", x.path, x.label))
                            .collect::<Vec<_>>()
                            .into(),
                    );
                }
            } else if args.iter().any(|a| a == "--offscreen") {
                // Park one icon outside every monitor, read it back, then restore it.
                if let Some(first) = items.iter().find(|x| !x.path.starts_with("::")).cloned() {
                    let v = host::virtual_rect();
                    let spot = (v[0] + v[2] + 4000, first.pos.1);
                    view.position(&[(first.path.clone(), spot)]);
                    let r = shell::DesktopView::last_position_result();
                    report.insert(
                        "offscreen_hresult".into(),
                        format!("0x{:08X} matched {}/{}", r.0, r.2, r.1).into(),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    report.insert("offscreen_target".into(), format!("{spot:?}").into());
                    report.insert(
                        "offscreen_after".into(),
                        format!("{:?}", view.position_of(&first.path)).into(),
                    );
                    if let Some(again) = shell::DesktopView::acquire() {
                        report.insert(
                            "offscreen_after_reacquire".into(),
                            format!("{:?}", again.position_of(&first.path)).into(),
                        );
                    }
                    view.position(&[(first.path.clone(), first.pos)]);
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    report.insert(
                        "offscreen_restored".into(),
                        format!("{:?} was {:?}", view.position_of(&first.path), first.pos).into(),
                    );
                }
            } else if args.iter().any(|a| a == "--stack") {
                // Send three icons to one point in a single call, then put them back.
                let three: Vec<_> = items
                    .iter()
                    .filter(|x| !x.path.starts_with("::"))
                    .take(3)
                    .cloned()
                    .collect();
                if let Some(first) = three.first() {
                    let (sx, _) = view.spacing().unwrap_or((75, 98));
                    let spot = (first.pos.0 + sx, first.pos.1);
                    let step: i32 = args
                        .iter()
                        .position(|a| a == "--step")
                        .and_then(|i| args.get(i + 1))
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    let moves: Vec<_> = three
                        .iter()
                        .enumerate()
                        .map(|(k, x)| {
                            (
                                x.path.clone(),
                                (spot.0 + k as i32 * step, spot.1 + k as i32 * step),
                            )
                        })
                        .collect();
                    view.position(&moves);
                    let r = shell::DesktopView::last_position_result();
                    report.insert(
                        "stack_hresult".into(),
                        format!("0x{:08X} matched {}/{}", r.0, r.2, r.1).into(),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let after: Vec<String> = three
                        .iter()
                        .map(|x| format!("{:?}", view.position_of(&x.path)))
                        .collect();
                    report.insert("stack_target".into(), format!("{spot:?}").into());
                    report.insert("stack_after".into(), after.into());
                    let back: Vec<_> = three.iter().map(|x| (x.path.clone(), x.pos)).collect();
                    view.position(&back);
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let restored: Vec<String> = three
                        .iter()
                        .map(|x| format!("{:?} was {:?}", view.position_of(&x.path), x.pos))
                        .collect();
                    report.insert("stack_restored".into(), restored.into());
                }
            } else if let Some(path) = pick {
                let before = view.position_of(&path);
                let (sx, _) = view.spacing().unwrap_or((75, 98));
                report.insert("item".into(), path.clone().into());
                report.insert("before".into(), format!("{before:?}").into());
                if let Some((x, y)) = before {
                    view.position(&[(path.clone(), (x + sx, y))]);
                    let r = shell::DesktopView::last_position_result();
                    report.insert(
                        "move_hresult".into(),
                        format!("0x{:08X} matched {}/{}", r.0, r.2, r.1).into(),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    report.insert(
                        "after_move".into(),
                        format!("{:?}", view.position_of(&path)).into(),
                    );
                    view.position(&[(path.clone(), (x, y))]);
                    let r = shell::DesktopView::last_position_result();
                    report.insert("back_hresult".into(), format!("0x{:08X}", r.0).into());
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    report.insert(
                        "after_back".into(),
                        format!("{:?}", view.position_of(&path)).into(),
                    );
                }
            }
        } else {
            report.insert("error".into(), "Explorer desktop view not available".into());
        }
        let out = args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("position-test.json"));
        let _ = std::fs::write(
            out,
            serde_json::to_vec_pretty(&serde_json::Value::Object(report)).unwrap_or_default(),
        );
        std::process::exit(0);
    }
    // The OLE main-window fixture is *always* a disposable window, never an
    // Explorer desktop host. Rejecting/omitting this early would allow a lab
    // flag to reach the real desktop startup path before main() could guard it.
    if !cfg!(windows) || explicitly_windowed_launch(&args) {
        host::log("start: window mode");
        return;
    }
    // Only native Explorer integration may own the real desktop. Unknown
    // arguments and builds without the embedded native DLL must fail closed.
    // Explicit Debug native-lab previews remain subject to their existing guards.
    let native_lab_preview = cfg!(all(windows, debug_assertions, feature = "native-lab"))
        && has("--native-explorer-chrome") && has("--unsafe-native-test");
    if !native_desktop_launch_requested(&args) && !native_lab_preview {
        host::log("Desktop launch refused: a native Explorer entry point and matching native build are required");
        eprintln!("Desktop mode requires a native Explorer build and supported launch arguments.");
        std::process::exit(2);
    }
    let command = args
        .iter()
        .position(|a| a == "--pin-file")
        .and_then(|i| args.get(i + 1))
        .map(|path| format!("pin-file {path}"))
        .or_else(|| {
            args.iter()
                .position(|a| a == "--command")
                .and_then(|i| args.get(i + 1))
                .cloned()
        });
    if has("--at-sign-in")
        && command.is_none()
        && State::load(&host::data_dir().join("data/workspace.json"))
            .is_ok_and(|st| !st.prefs.desktop_enabled)
    {
        host::event(
            "INFO",
            "startup.desktop_disabled_sign_in_skip",
            "DesktopCraft is disabled in Settings; skipping sign-in launch",
        );
        std::process::exit(0);
    }
    if !host::first_instance() {
        host::allow_foreground();
        let _ = host::send_command(command.as_deref().unwrap_or("settings"));
        std::process::exit(0);
    }
    // Leftover commands from an earlier session are not replayed.
    let _ = host::take_commands();
    if let Some(c) = command {
        let _ = host::send_command(&c);
    }
    host::event(
        "INFO",
        "startup.desktop",
        if has("--at-sign-in") {
            "sign-in launch"
        } else {
            "interactive launch"
        },
    );
    macroquad::miniquad::DESKTOP_HOST.store(true, std::sync::atomic::Ordering::Relaxed);
}
