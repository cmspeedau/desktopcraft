use crate::*;

pub(super) fn probe() -> serde_json::Value {
    use serde_json::json;
    let monitors = host::monitors();
    let folders = shell::desktop_folders();
    let entries = shell::desktop_entries();
    let view = shell::DesktopView::acquire();
    let items = view.as_ref().map(|v| v.items()).unwrap_or_default();
    let matched = entries
        .iter()
        .filter(|e| {
            items
                .iter()
                .any(|i| i.path.eq_ignore_ascii_case(&e.to_string_lossy()))
        })
        .count();
    let sample = entries.first().map(|p| p.to_string_lossy().to_string());
    let icon = sample
        .as_deref()
        .and_then(|p| shell::image(p, 64))
        .map(|(w, h, _)| format!("{w}x{h}"));
    json!({
        "version": 1,
        "monitors": monitors.iter().map(|m| json!({"key": m.key, "rect": m.rect, "work": m.work, "dpi": m.dpi, "primary": m.primary})).collect::<Vec<_>>(),
        "monitor_signature": host::signature(&monitors),
        "virtual_screen": host::virtual_rect(),
        "desktop_folders": folders.iter().map(|f| f.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "desktop_items": entries.len(),
        "explorer_desktop_view": view.is_some(),
        "explorer_icons_reported": items.len(),
        "desktop_items_with_icon_position": matched,
        "explorer_icon_spacing": view.as_ref().and_then(|v| v.spacing()),
        "explorer_auto_arrange": view.as_ref().and_then(|v| v.auto_arrange()),
        "explorer_align_to_grid": view.as_ref().and_then(|v| v.snap_to_grid()),
        "icon_positions": items.iter().map(|i| json!({"path": i.path, "x": i.pos.0, "y": i.pos.1})).collect::<Vec<_>>(),
        "explorer_icons_visible": view.as_ref().and_then(|v| v.icons_visible()),
        "first_item_icon": icon,
        "start_at_sign_in": host::autostart(None),
        "installed": host::install_dir().join(format!("{}.exe", host::APP_NAME)).exists(),
        "data_folder": host::data_dir().to_string_lossy(),
        "already_running": !host::first_instance(),
    })
}
/// Any unexpected stop is written to the log and, outside automated checks,
/// shown to the person instead of the program silently disappearing.
pub(super) fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = format!("{info}");
        let location = info
            .location()
            .map(|location| {
                let source = std::path::Path::new(location.file())
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("unknown");
                format!("{source}:{}", location.line())
            })
            .unwrap_or_else(|| "unknown".into());
        host::event("ERROR", "panic", &format!("source={location}"));
        if host_mode() && !capture_mode()
            && std::env::var_os("DESKTOPCRAFT_NATIVE_CHROME_ONLY").is_none() {
            // Never leave the person without desktop icons.
            desktop::restore_explorer_icons();
        }
        if !capture_mode()
            && std::env::var_os("DESKTOPCRAFT_SMOKE").is_none()
            && std::env::var_os("DESKTOP_SPACES_SMOKE").is_none()
        {
            host::message(
                &format!(
                    "DesktopCraft stopped because of an error:\n\n{message}\n\nDetails are saved in {}",
                    host::data_dir().join("desktopcraft.log").display()
                ),
                false,
            );
        }
    }));
}
pub(super) fn configuration() -> Conf {
    #[cfg(all(windows, feature = "native-lab"))]
    native_integrated::configure_dpi_early();
    #[cfg(windows)]
    {
        let args=std::env::args().collect::<Vec<_>>();
        if (native_desktop_launch_requested(&args) ||
            args.iter().any(|arg|arg=="--native-desktop-check"))
            && !enable_native_audit_physical_dpi(){
            eprintln!("Native Desktop requires physical, per-monitor DPI coordinates");
            std::process::exit(2);
        }
    }
    // Older builds put these in the environment, so programs opened from the
    // desktop (and anything they start) may have inherited them: ignore them.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("DESKTOP_SPACES_") {
            std::env::remove_var(key);
        }
    }
    install_panic_hook();
    if capture_mode() {
        std::env::set_var("DESKTOPCRAFT_CAPTURE", "1");
    } else {
        startup();
    }
    Conf {
        window_title: "DesktopCraft".into(),
        window_width: 1280,
        window_height: 800,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        icon: Some(macroquad::miniquad::conf::Icon {
            small: *include_bytes!("../assets/desktopcraft-16.rgba"),
            medium: *include_bytes!("../assets/desktopcraft-32.rgba"),
            big: *include_bytes!("../assets/desktopcraft-64.rgba"),
        }),
        platform: macroquad::miniquad::conf::Platform {
            blocking_event_loop: false,
            swap_interval: Some(1),
            ..Default::default()
        },
        ..Default::default()
    }
}
