    use super::*;
    #[test]
    fn floating_settings_can_move_fully_to_secondary_display() {
        let virtual_screen = Bounds {
            x: 0.,
            y: 0.,
            w: 3840.,
            h: 1080.,
        };
        let primary = Bounds {
            x: 0.,
            y: 0.,
            w: 1920.,
            h: 1080.,
        };
        let initial = floating_settings_bounds(virtual_screen, primary, None);
        assert!(initial.x >= 0. && initial.x < 1920.);
        let moved = floating_settings_bounds(virtual_screen, primary, Some(vec2(2330., 200.)));
        assert_eq!(moved.x, 2330.);
        assert!(moved.x + moved.w <= virtual_screen.w);
        assert!(moved.x > primary.x + primary.w);
    }
    #[test]
    fn settings_clamps_to_virtual_desktop_not_primary_monitor() {
        let virtual_screen = Bounds {
            x: 0.,
            y: 0.,
            w: 3840.,
            h: 1080.,
        };
        let primary = Bounds {
            x: 1920.,
            y: 0.,
            w: 1920.,
            h: 1080.,
        };
        let moved = floating_settings_bounds(virtual_screen, primary, Some(vec2(-200., 100.)));
        assert_eq!(moved.x, 0.);
        assert_eq!(
            floating_settings_bounds(virtual_screen, primary, Some(vec2(9999., 100.))).x,
            2970.
        );
    }
