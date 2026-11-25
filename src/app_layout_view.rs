use crate::*;

#[cfg(windows)]
pub(super) fn native_scroll_limit() -> u8 { native_filter_host::MAX_SCROLL_ROWS_PER_CHANGE }
#[cfg(not(windows))]
pub(super) fn native_scroll_limit() -> u8 { 8 }

/// Paint/hit-test order follows the current raised roll-up, not creation
/// order. The active expanded box must take precedence over overlapping tabs.
pub(super) fn paint_order<'a>(
    groups: &'a [Group],
    page: usize,
    revealed: Option<u64>,
) -> impl Iterator<Item = &'a Group> + 'a {
    groups
        .iter()
        .filter(move |g| g.page == page && Some(g.id) != revealed)
        .chain(
            groups
                .iter()
                .filter(move |g| g.page == page && Some(g.id) == revealed),
        )
}

/// A floating tool window is initially centered on the selected primary
/// monitor, but its drag constraints use the ENTIRE virtual desktop. When
/// crossing a monitor edge, no primary-monitor clamp can trap the window.
pub(super) fn floating_settings_bounds(workspace: Bounds, primary: Bounds, location: Option<Vec2>) -> Bounds {
    let (w, h) = (870.0f32, 650.0f32);
    let default = vec2(
        primary.x + ((primary.w - w) / 2.).max(0.),
        primary.y + ((primary.h - h) / 2.).max(0.),
    );
    let origin = location.unwrap_or(default);
    Bounds {
        x: origin.x.clamp(
            workspace.x,
            (workspace.x + workspace.w - w).max(workspace.x),
        ),
        y: origin.y.clamp(
            workspace.y,
            (workspace.y + workspace.h - h).max(workspace.y),
        ),
        w,
        h,
    }
}

/// Macroquad exposes logical screen dimensions (already DPI-normalized).
/// Do not divide desktop UI dimensions or mouse input by DPI again.
pub(super) fn ui_scale_for_frame(desktop_mode: bool, logical_width: f32, logical_height: f32) -> f32 {
    if desktop_mode {
        1.0
    } else {
        (logical_width / 1280.).min(logical_height / 800.).max(0.2)
    }
}

pub(super) fn workspace_camera(width: f32, height: f32) -> Camera2D {
    // Screen cameras invert Y internally; positive zoom keeps (0, 0) at the top left.
    Camera2D {
        target: vec2(width / 2., height / 2.),
        zoom: vec2(2. / width, 2. / height),
        ..Default::default()
    }
}

