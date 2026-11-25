    use super::*;

    #[test]
    fn resizing_previews_the_next_row_at_half_icon_and_snaps_on_drop() {
        let cell = (75, 107);
        let almost_half = ghost_resize_positions(12, None, cell, [100, 100, 339, 142], 0, Some(24));
        let half = ghost_resize_positions(12, None, cell, [100, 100, 339, 143], 0, Some(24));
        let committed = ghost_resize_positions(12, None, cell, [100, 100, 339, 143], 0, None);
        assert_eq!((almost_half.len(), half.len(), committed.len()), (4, 8, 4));
        assert_eq!(half[4], (4, (122, 219)));

        let bounds = Bounds { x: 100., y: 300., w: 339., h: 190. };
        let bottom_drag = snap_native_row_height(bounds, 8, 1000., 1., cell);
        let top_drag = snap_native_row_height(bounds, 4, 1000., 1., cell);
        assert_eq!(bottom_drag, Bounds { h: 240., ..bounds });
        assert_eq!(top_drag, Bounds { y: 250., h: 240., ..bounds });
        // When there's no room for the nearest larger snapped height, use a
        // full row count that stays inside the display.
        assert_eq!(snap_native_row_height(bounds, 8, 490., 1., cell).h, 133.);
    }

    #[test]
    fn ghost_copies_land_on_the_cells_the_planner_uses() {
        // Icons at (1811,109)+cells; Box dragged so its top-left is (1311,509).
        let icons = [(1811, 109), (1886, 109), (1811, 216)];
        let shift = ghost_drop_shift(&icons, (75, 107), [1311, 509, 740, 416]).unwrap();
        // First grid point at/after (1311,509) on the 75x107 grid through (1811,109): (1361,537).
        assert_eq!((1811 + shift.0, 109 + shift.1), (1361, 537));
        assert!(ghost_drop_shift(&[] as &[(i32, i32)], (75, 107), [0, 0, 1, 1]).is_none());
    }

    #[test]
    fn ghost_scales_cell_and_icon_size_for_125_percent_monitor() {
        assert_eq!(ghost_cell_for_dpi((75, 107), 96, 96), (75, 107));
        assert_eq!(ghost_cell_for_dpi((75, 107), 120, 96), (94, 134));
        assert_eq!(ghost_icon_px(48, 96), 48.);
        assert_eq!(ghost_icon_px(48, 120), 60.);
        assert_eq!(ghost_icon_px(96, 96), 96.);
        assert_eq!(ghost_icon_px(96, 120), 120.);
        // On the 125% monitor, dropping with Snap-to-Grid snaps onto the 94x134 grid immediately.
        let icons = [(2950, 270), (3044, 270)];
        let dropped = ghost_icon_positions(
            &icons, false, true, true, (94, 134), (94, 134), (10, 10), [2960, 280, 740, 416],
        );
        assert_eq!(dropped, vec![(0, (3044, 404)), (1, (3138, 404))]);
    }

    #[test]
    fn native_box_min_size_tracks_explorer_icon_size_and_dpi() {
        // Small (56, 68), Medium (75, 97) / (75, 107), Large (154, 194), Extra-Large (354, 394).
        assert_eq!(native_box_min_size((56, 68), 1.0, 200.0), (95.0, 92.0));
        assert_eq!(native_box_min_size((75, 97), 1.0, 200.0), (111.0, 121.0));
        assert_eq!(native_box_min_size((75, 107), 1.0, 200.0), (111.0, 133.0));
        assert_eq!(native_box_min_size((94, 134), 1.0, 200.0), (135.0, 163.0));
        assert_eq!(native_box_min_size((94, 134), 1.25, 200.0), (108.0, 131.0));
        assert_eq!(native_box_min_size((154, 194), 1.0, 200.0), (204.0, 229.0));
        assert_eq!(native_box_min_size((354, 394), 1.0, 200.0), (404.0, 432.0));
        // Top-edge Box preserves one full icon row below the clamped 0..HEADER bar.
        assert_eq!(native_box_min_size((75, 97), 1.0, 0.0), (111.0, 121.0 + HEADER));
    }

    #[test]
    fn ghost_resize_reflows_columns_and_clips_overflow() {
        let icons = [(1811, 109), (1886, 109), (1961, 109), (1811, 216), (1886, 216)];
        // Narrow Box to fit 2 cols × 2 rows = 4 icons (1811 + 2*75 + 14 = 1975 -> w=190 from x=1788).
        let resized = ghost_icon_positions(
            &icons, true, false, true, (75, 107), (75, 107), (0, 0), [1788, 31, 190, 310],
        );
        assert_eq!(
            resized,
            vec![(0, (1811, 109)), (1, (1886, 109)), (2, (1811, 216)), (3, (1886, 216))]
        );
    }

    #[test]
    fn ghost_two_grid_mode_has_zero_drop_jump_and_box_origin_resize() {
        // In two-grid mode (snap_to_grid=false), icons align inside Box padding
        // and keep exact drag delta on drop with zero Desktop-grid jump.
        let icons = [(422, 412), (497, 412), (572, 412)];
        let dropped = ghost_icon_positions(
            &icons, false, true, false, (75, 107), (75, 107), (17, 23), [417, 423, 300, 260],
        );
        assert_eq!(dropped, vec![(0, (439, 435)), (1, (514, 435)), (2, (589, 435))]);
        // Resizing in two-grid mode anchors at Box padded origin (439, 435) and wraps to 2 cols.
        let resized = ghost_icon_positions(
            &icons, true, false, false, (75, 107), (75, 107), (0, 0), [417, 423, 200, 260],
        );
        assert_eq!(resized, vec![(0, (439, 435)), (1, (514, 435)), (2, (439, 542))]);
    }

    #[cfg(windows)]
    #[test]
    fn instant_ghost_icons_prepopulate_from_box_items_without_waiting_for_worker() {
        let mut s = State::empty();
        let gid = s.add_group("Apps", Bounds { x: 400., y: 400., w: 300., h: 260. });
        let g = s.group_mut(gid).unwrap();
        for n in 0..8u64 {
            g.tab_mut().items.push(Item {
                id: 100 + n,
                name: format!("App{n}"),
                path: format!("C:/d/App{n}.lnk"),
                kind: "lnk".into(),
                size: 16,
                modified: 0,
                label: None,
            });
        }
        let rect = native_box_placement::Rect { x: 400, y: 400, w: 300, h: 260 };
        let page1 = build_instant_ghost_icons(&g, rect, (75, 107), 1, false);
        // 3 cols x 2 rows = 6 visible; first_row = 1 starts at App3..App7 (5 items).
        assert_eq!(page1.len(), 5);
        assert_eq!(page1[0].label, "App3");
        assert_eq!(page1[0].pos, (422, 412));
        assert_eq!(page1[1].pos, (497, 412));
        assert_eq!(page1[3].pos, (422, 519));
    }

    #[test]
    fn ghost_cancel_or_refuse_restores_preview_but_applied_move_or_rollup_does_not() {
        let origin = Bounds { x: 100., y: 100., w: 300., h: 300. };
        let moved = Bounds { x: 220., y: 140., w: 300., h: 300. };
        assert!(ghost_should_end_preview(Some((origin, false)), origin));
        assert!(ghost_should_end_preview(None, origin));
        assert!(!ghost_should_end_preview(Some((moved, false)), origin));
        assert!(!ghost_should_end_preview(Some((origin, true)), origin));
    }
