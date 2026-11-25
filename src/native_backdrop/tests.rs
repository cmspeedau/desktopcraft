    use super::*;

    #[test]
    fn wheel_targets_the_topmost_box_body_only() {
        let panel = |id, x| Panel { id, x, y: 100, w: 200, h: 100, rgb: (0, 0, 0), alpha: 0, radius: 0 };
        let panels = [panel(1, 0), panel(2, 150)];
        assert_eq!(panel_at(&panels, (10, 150)), Some(1));
        assert_eq!(panel_at(&panels, (160, 150)), Some(2)); // overlap: later = on top
        assert_eq!(panel_at(&panels, (10, 99)), None);
        assert_eq!(panel_at(&panels, (350, 150)), None); // right edge is exclusive
    }

    #[test]
    fn panel_color_clamps_and_rounds() {
        assert_eq!(panel_color([0.07, 0.5, 1.2, 0.86]), ((18, 128, 255), 219));
        assert_eq!(panel_color([-1., 0., 0., 0.]), ((0, 0, 0), 0));
    }

    #[test]
    fn header_color_darkens_titleband_and_keeps_text_legible() {
        let (rgb, alpha) = header_color([0.2, 0.6, 0.2, 0.86], 1.0);
        assert!(rgb.1 > rgb.0 && rgb.1 < 153);
        assert!(alpha > 200 && alpha <= 255);

        let (low_rgb, low_alpha) = header_color([1.5, -0.2, 0.5, 0.0], 1.0);
        assert_eq!(low_rgb, (0, 0, 0));
        assert_eq!(low_alpha, 115);

        let (_, faded_alpha) = header_color([0.2, 0.6, 0.2, 0.86], 0.42);
        assert!(faded_alpha < alpha && faded_alpha >= 115);
    }

    #[test]
    fn backdrop_frame_tracks_header_mirror_changes() {
        let h = HeaderPanel {
            id: 1,
            x: 10,
            y: 20,
            w: 300,
            h: 42,
            rgb: (20, 60, 20),
            alpha: 226,
            radius: 10,
            corners: HeaderCorners::TopRounded,
            vertical: false,
            font_px: 15,
            title: "Box".into(),
            tabs: vec![],
        };
        let a = BackdropFrame { panels: vec![], headers: vec![h.clone()], tints: vec![] };
        let b = BackdropFrame { panels: vec![], headers: vec![h.clone()], tints: vec![] };
        let mut c = a.clone();
        c.headers[0].title = "Other".into();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn precision_touchpad_two_finger_scroll_tracks_vertical_motion_and_direction() {
        let mut tp = TouchpadScrollState::default();
        let t0 = std::time::Instant::now();
        // Single finger never scrolls.
        assert_eq!(tp.update(&[(0.5, 0.5)], t0, true), None);
        // First two-finger frame establishes anchor without jumping.
        assert_eq!(tp.update(&[(0.4, 0.50), (0.6, 0.50)], t0, true), None);
        // Natural scrolling: two fingers moving up (dy < 0) yields negative wheel delta (scrolls down).
        let t1 = t0 + std::time::Duration::from_millis(16);
        let d_nat = tp.update(&[(0.4, 0.45), (0.6, 0.45)], t1, true).unwrap();
        assert!(d_nat < -120);
        // Reverse scrolling flips the sign.
        let t2 = t1 + std::time::Duration::from_millis(16);
        let d_rev = tp.update(&[(0.4, 0.40), (0.6, 0.40)], t2, false).unwrap();
        assert!(d_rev > 120);
        // Horizontal two-finger swipe is ignored.
        let t3 = t2 + std::time::Duration::from_millis(16);
        assert_eq!(tp.update(&[(0.50, 0.401), (0.70, 0.401)], t3, true), None);
        // Three fingers resets gesture.
        let t4 = t3 + std::time::Duration::from_millis(16);
        assert_eq!(tp.update(&[(0.3, 0.5), (0.5, 0.5), (0.7, 0.5)], t4, true), None);
    }
