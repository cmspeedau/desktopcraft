    use super::*;

    fn fixture() -> (App, u64, Bounds) {
        let mut app = App::new(std::env::temp_dir().join("desktopcraft-docking-test"), true, true);
        app.state = State::empty();
        app.native_env.chrome_only = true;
        app.native_env.in_box_layout = true;
        app.width = 2400.;
        app.height = 1800.;
        let host = app.host.as_mut().unwrap();
        host.virtual_rect = [-1200, 0, 2400, 1800];
        host.monitors = vec![
            host::Monitor { key: "Left".into(), rect: [-1200, 0, 1200, 900], work: [-1200, 0, 1200, 852], dpi: 96, primary: false },
            host::Monitor { key: "Right".into(), rect: [0, 0, 1200, 900], work: [0, 0, 1200, 900], dpi: 96, primary: true },
            host::Monitor { key: "Lower".into(), rect: [-1200, 900, 1200, 900], work: [-1200, 900, 1200, 900], dpi: 96, primary: false },
        ];
        let bounds = Bounds { x: 400., y: 300., w: 320., h: 260. };
        let id = app.state.add_group("Box", bounds);
        (app, id, bounds)
    }

    #[test]
    fn bottom_edge_snaps_before_the_cursor_reaches_taskbar_and_can_cross_to_the_lower_monitor() {
        let (mut app, id, before) = fixture();
        let start = vec2(450., 280.);
        let contact = Input { pos: vec2(450., 572.), ..Default::default() };
        app.move_group(id, start, before, None, &contact, false);
        let preview = app.state.group(id).unwrap();
        assert_eq!(preview.bounds.y + preview.bounds.h, 852.);
        assert!(!preview.rolled);

        let crossing = Input { pos: vec2(450., 980.), ..Default::default() };
        app.move_group(id, start, before, None, &crossing, false);
        assert_eq!(app.state.group(id).unwrap().bounds.y, 1000.);

        assert!(app.move_group(id, start, before, None, &contact, true));
        let docked = app.state.group(id).unwrap();
        assert!(docked.rolled);
        assert_eq!(docked.dock, Some(DockEdge::Bottom));
        let header = paint::native_group_header(docked);
        assert_eq!(header.y + header.h, 852.);
        assert_eq!(paint::native_group_body(docked).y + docked.bounds.h, header.y);

        let before = docked.bounds;
        let resize = vec2(before.x + 30., before.y);
        app.revealed = Some(id);
        app.drag = Some(Drag::Resize { id, start: resize, before, edges: 4 });
        let pos = resize - vec2(0., 90.);
        app.pointer(&Input { pos, down: true, ..Default::default() }, &[]);
        app.pointer(&Input { pos, released: true, ..Default::default() }, &[]);
        let group = app.state.group(id).unwrap();
        assert!(group.bounds.h > before.h);
        assert_eq!(paint::native_group_header(group).y + HEADER, 852.);

        let before = group.bounds;
        let start = vec2(before.x + 60., 827.);
        let away = Input { pos: vec2(start.x + 90., 470.), ..Default::default() };
        assert!(!app.move_group(id, start, before, Some(DockEdge::Bottom), &away, true));
        let group = app.state.group(id).unwrap();
        assert_eq!(group.dock, None);
        assert!(!group.rolled);
        assert!(paint::native_group_header(group).contains(away.pos.x, away.pos.y));
    }

    #[test]
    fn side_docking_selects_the_pointer_monitor_but_measures_the_box_edge() {
        let (mut app, id, before) = fixture();
        let start = vec2(440., 280.);
        let right = Input { pos: vec2(920., 280.), ..Default::default() };
        assert!(app.move_group(id, start, before, None, &right, true));
        let group = app.state.group(id).unwrap();
        assert_eq!(group.dock, Some(DockEdge::Right));
        let bar = paint::native_group_header(group);
        assert_eq!((bar.w, bar.h), (HEADER, before.h));
        assert_eq!(bar.x + bar.w, 1200.);
        assert_eq!(group.bounds.x + group.bounds.w, bar.x);

        let left = Input { pos: vec2(1244., 280.), ..Default::default() };
        assert!(app.move_group(id, start, before, None, &left, true));
        let group = app.state.group(id).unwrap();
        assert_eq!(group.dock, Some(DockEdge::Left));
        let bar = paint::native_group_header(group);
        assert_eq!((bar.x, bar.w, bar.h), (1200., HEADER, before.h));
        assert_eq!(group.bounds.x, bar.x + bar.w);

        let past_band = Input { pos: vec2(1270., 280.), ..Default::default() };
        assert!(!app.move_group(id, start, before, None, &past_band, true));
        assert!(app.state.group(id).unwrap().bounds.x > 1200.);
    }

    #[test]
    fn vertical_header_can_slide_on_the_edge_and_pull_off_under_the_cursor() {
        let (mut app, id, before) = fixture();
        let edge = DockEdge::Right;
        let before = edge.fit_body(before, Bounds { x: 0., y: 0., w: 1200., h: 852. });
        let group = app.state.group_mut(id).unwrap();
        group.bounds = before;
        group.dock = Some(edge);
        group.rolled = true;
        let start = vec2(1180., 420.);
        let sliding = Input { pos: start + vec2(-4., 100.), ..Default::default() };
        app.move_group(id, start, before, Some(edge), &sliding, false);
        let group = app.state.group(id).unwrap();
        assert_eq!(group.dock, Some(edge));
        assert_eq!(group.bounds.y, before.y + 100.);
        assert!(paint::native_group_header(group).contains(sliding.pos.x, sliding.pos.y));

        let release = Input { pos: start + vec2(-100., 100.), ..Default::default() };
        assert!(!app.move_group(id, start, before, Some(edge), &release, true));
        let group = app.state.group(id).unwrap();
        assert_eq!(group.dock, None);
        assert!(!group.rolled);
        assert!(paint::native_group_header(group).contains(release.pos.x, release.pos.y));
    }

    #[test]
    fn top_contact_previews_a_rollup_and_old_workspaces_default_to_floating() {
        let (mut app, id, before) = fixture();
        let start = vec2(450., 280.);
        app.drag = Some(Drag::Group { id, start, before, before_dock: None });
        let top = Input { pos: vec2(450., 22.), ..Default::default() };
        app.move_group(id, start, before, None, &top, false);
        assert_eq!(app.previewing_top_dock(), Some(id));
        assert!(!app.state.group(id).unwrap().rolled);
        assert!(app.move_group(id, start, before, None, &top, true));
        let group = app.state.group(id).unwrap();
        assert_eq!(paint::native_group_header(group).y, 0.);
        let mut saved = serde_json::to_value(group).unwrap();
        assert_eq!(serde_json::from_value::<Group>(saved.clone()).unwrap().dock, Some(DockEdge::Top));
        saved.as_object_mut().unwrap().remove("dock");
        assert!(serde_json::from_value::<Group>(saved).unwrap().dock.is_none());
    }
