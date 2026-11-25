    use super::*;
    use macroquad::camera::Camera;

    #[test]
    fn desktop_ui_respects_macroquad_logical_dpi_at_100_125_150_percent() {
        // screen_width() and mouse_position() are both pre-divided by DPI.
        // Region pixels are computed once as logical*dpi in host_after_frame.
        for (physical_width, dpi) in [(1920., 1.), (1920., 1.25), (1920., 1.5)] {
            let logical_width = physical_width / dpi;
            let scale = ui_scale_for_frame(true, logical_width, 1200. / dpi);
            assert_eq!(scale, 1.);
            let ui_width = logical_width / scale;
            let logical_button_w = 870.;
            let clip_px = logical_button_w * dpi;
            // On both the GPU and Win32 region sides the same number of
            // physical pixels must correspond to one logical UI length.
            let render_px = logical_button_w * physical_width / ui_width;
            assert!(
                (clip_px - render_px).abs() < 0.001,
                "dpi={dpi}: render {render_px}px != region {clip_px}px"
            );
        }
        assert!((ui_scale_for_frame(false, 1600., 900.) - 1.125).abs() < 0.001);
    }

    #[test]
    fn workspace_projection_matches_pointer_coordinates_after_resizing() {
        for (width, height) in [(1280., 800.), (1600., 800.), (1280., 1000.)] {
            let matrix = workspace_camera(width, height).matrix();
            for point in [vec2(0., 0.), vec2(width, height), vec2(110., 145.)] {
                let projected = matrix.transform_point3(vec3(point.x, point.y, 0.));
                let screen = vec2(
                    (projected.x + 1.) * width / 2.,
                    (1. - projected.y) * height / 2.,
                );
                assert!(
                    screen.distance(point) < 0.001,
                    "{point:?} rendered at {screen:?}"
                );
            }
        }
    }
