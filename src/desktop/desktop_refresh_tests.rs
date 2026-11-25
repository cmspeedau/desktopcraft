    use super::*;

    #[test]
    fn page_bar_follows_selected_display_with_windows_primary_fallback() {
        let root = std::env::temp_dir().join(format!("desktopcraft-primary-page-bar-{}", std::process::id()));
        let mut app = App::new(root, true, true);
        app.state = State::empty();
        app.state.prefs.workspace_monitor = "secondary".into();
        let h = app.host.as_mut().unwrap();
        h.scale = 1.0;
        h.virtual_rect = [-1200, 0, 3760, 1440];
        h.monitors = vec![
            host::Monitor { key: "secondary".into(), rect: [-1200, 0, 1200, 800],
                work: [-1200, 0, 1200, 760], dpi: 96, primary: false },
            host::Monitor { key: "primary".into(), rect: [0, 0, 2560, 1440],
                work: [0, 0, 2560, 1400], dpi: 96, primary: true },
        ];
        assert_eq!(app.windows_primary_view(), Bounds { x: 1200., y: 0., w: 2560., h: 1400. });
        assert_eq!(app.primary_view(), Bounds { x: 0., y: 0., w: 1200., h: 760. });
        app.state.prefs.workspace_monitor.clear();
        assert_eq!(app.primary_view(), app.windows_primary_view());
        app.state.prefs.workspace_monitor = "disconnected".into();
        assert_eq!(app.primary_view(), app.windows_primary_view());
        app.state.prefs.workspace_monitor = "secondary".into();
        assert_eq!(app.primary_view(), Bounds { x: 0., y: 0., w: 1200., h: 760. });
    }

    #[test]
    fn restored_dock_uses_saved_monitor_when_its_old_center_crosses_the_new_boundary() {
        let root = std::env::temp_dir().join(format!("desktopcraft-dock-restore-{}", std::process::id()));
        let mut app = App::new(root, true, true);
        app.state = State::empty();
        app.native_env.chrome_only = true;
        let host = app.host.as_mut().unwrap();
        host.signature = "two-displays".into();
        host.monitors = vec![
            host::Monitor { key: "left".into(), rect: [0, 0, 800, 800],
                work: [0, 0, 800, 760], dpi: 96, primary: true },
            host::Monitor { key: "right".into(), rect: [800, 0, 1200, 800],
                work: [800, 0, 1200, 760], dpi: 96, primary: false },
        ];
        let old_bounds = Bounds { x: 910., y: 120., w: 300., h: 240. };
        let id = app.state.add_group("Docked box", old_bounds);
        let box_state = app.state.group_mut(id).unwrap();
        box_state.rolled = true;
        box_state.dock = Some(DockEdge::Right);
        app.state.layouts.insert("two-displays".into(), BTreeMap::from([(
            id, Placement { monitor: "left".into(), x: old_bounds.x, y: old_bounds.y,
                w: old_bounds.w, h: old_bounds.h },
        )]));

        let saved = serde_json::to_vec(&app.state).unwrap();
        app.state = State::from_saved_bytes(&saved).unwrap();
        app.apply_layout();
        let restored = app.state.group(id).unwrap();
        assert_eq!(restored.dock, Some(DockEdge::Right));
        assert!(restored.rolled);
        assert_eq!(restored.bounds, Bounds { x: 800. - HEADER - old_bounds.w, ..old_bounds });
    }

    #[test]
    fn bottom_sort_moves_only_loose_icons_and_keeps_manual_pins_and_pages() {
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-bottom-rule-sort-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut app = App::new(root.join("workspace"), true, false);
        app.state = State::empty();
        let bounds = Bounds {
            x: 30.,
            y: 40.,
            w: 270.,
            h: 220.,
        };
        let pdf_box = app.state.add_group("Documents", bounds);
        let pdf_tab = app.state.group(pdf_box).unwrap().tab().id;
        app.state.create_page("Other page").unwrap();
        let text_box = app.state.add_group("Notes", bounds);
        let text_tab = app.state.group(text_box).unwrap().tab().id;
        app.state.rules.push(Rule {
            mode: 0,
            pattern: "pdf".into(),
            target: pdf_box,
            target_tab: Some(pdf_tab),
            enabled: true,
        });
        app.state.rules.push(Rule {
            mode: 0,
            pattern: "txt".into(),
            target: text_box,
            target_tab: Some(text_tab),
            enabled: true,
        });
        let pdf = root.join("new.pdf");
        let txt = root.join("new.txt");
        let ignored = root.join("unmatched.png");
        let manually_placed = root.join("already-pinned.pdf");
        for file in [&pdf, &txt, &ignored, &manually_placed] {
            std::fs::write(file, b"untouched").unwrap();
        }
        app.state.import(&manually_placed, text_box).unwrap();
        let before_pages = app.state.pages.clone();
        let before_page = app.state.page;
        let count = app.sort_paths_by_rules(vec![
            pdf.clone(),
            txt.clone(),
            ignored.clone(),
            manually_placed.clone(),
        ]);
        assert_eq!(count, 2);
        assert_eq!(app.state.pages, before_pages);
        assert_eq!(app.state.page, before_page);
        assert_eq!(app.state.group(pdf_box).unwrap().page, 0);
        assert_eq!(app.state.group(text_box).unwrap().page, 1);
        let pdf_items = &app.state.group(pdf_box).unwrap().tab().items;
        let txt_items = &app.state.group(text_box).unwrap().tab().items;
        assert_eq!(pdf_items.len(), 1);
        assert_eq!(pdf_items[0].path, pdf.to_string_lossy());
        assert_eq!(txt_items.len(), 2);
        assert!(txt_items
            .iter()
            .any(|i| i.path == manually_placed.to_string_lossy()));
        assert!(txt_items.iter().any(|i| i.path == txt.to_string_lossy()));
        let id = app.state.next_id;
        assert_eq!(
            app.sort_paths_by_rules(vec![pdf.clone(), txt.clone(), ignored, manually_placed]),
            0
        );
        assert_eq!(app.state.next_id, id);
        assert_eq!(std::fs::read(pdf).unwrap(), b"untouched");
        assert_eq!(std::fs::read(txt).unwrap(), b"untouched");
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recycle_bin_target_requires_windows_shell_guid_not_an_ordinary_name() {
        assert!(is_recycle_bin_shell_item(
            "::{645FF040-5081-101B-9F08-00AA002F954E}"
        ));
        assert!(is_recycle_bin_shell_item(
            "shell:::{645ff040-5081-101b-9f08-00aa002f954e}"
        ));
        assert!(!is_recycle_bin_shell_item(
            "C:\\Users\\Test\\Desktop\\Recycle Bin.lnk"
        ));
        assert!(!is_recycle_bin_shell_item(
            "C:\\Temp\\645ff040-5081-101b-9f08-00aa002f954e.txt"
        ));
        assert!(!is_recycle_bin_shell_item("::{some-other-guid}"));
    }
    #[test]
    fn loose_desktop_drop_snaps_to_nearest_free_grid_cell() {
        let work = [0, 0, 360, 320];
        let spacing = (90, 80);
        assert_eq!(
            snap_loose_icon((101, 92), work, spacing, &[], &[]),
            (90, 80)
        );
        assert_eq!(
            snap_loose_icon((177, 155), work, spacing, &[(180, 160)], &[]),
            (180, 80)
        );
        // The closest slot is under a group; choose a free neighbor.
        let blocked = [[90, 80, 95, 85]];
        let found = snap_loose_icon((101, 92), work, spacing, &[], &blocked);
        assert_ne!(found, (90, 80));
        assert_eq!(found.0.rem_euclid(90), 0);
        assert_eq!(found.1.rem_euclid(80), 0);
    }
    #[test]
    fn windows_server_grid_preserves_explorers_observed_horizontal_offset() {
        let work = [0, 0, 450, 400];
        let spacing = (90, 80);
        let phase = grid_phase(work, spacing, &[(22, 15), (112, 95), (202, 175)]);
        assert_eq!(phase, (22, 15));
        assert_eq!(
            snap_loose_icon_phase((110, 98), work, spacing, &[], &[], phase),
            (112, 95)
        );
    }
    #[test]
    fn loose_grid_handles_negative_multi_monitor_coordinates() {
        let work = [-1920, -120, 1920, 1080];
        let at = snap_loose_icon((-1830, -40), work, (90, 80), &[], &[]);
        assert_eq!(at, (-1830, -40));
    }
    #[test]
    fn destination_toast_and_non_pin_layout_restore_preserve_pins_and_report_tabs() {
        let mut before = State::empty();
        let games = before.add_group("Games", Bounds { x: 10., y: 20., w: 300., h: 200. });
        let programs = before.add_group("Programs", Bounds { x: 350., y: 20., w: 400., h: 300. });
        before.add_tab(programs).unwrap();
        before.group_mut(programs).unwrap().tabs[1].name = "Craft".into();
        before.create_page("Page 2").unwrap();
        before.page = 0;

        let mut after = before.clone();
        let make_pin = |id, path: &str| Item {
            id,
            name: path.into(),
            path: path.into(),
            kind: "lnk".into(),
            size: 1,
            modified: 1,
            label: None,
        };
        after.group_mut(games).unwrap().tabs[0].items.push(make_pin(1, "C:/Desktop/G.lnk"));
        after.group_mut(programs).unwrap().tabs[1].items.push(make_pin(2, "C:/Desktop/P.lnk"));

        assert!(native_pins_differ(&before, &after));
        assert!(!native_pins_differ(&before, &before));
        let dests = format_pin_destinations(&before, &after);
        assert_eq!(dests, "Games, Programs / Craft");
        assert!(format_rule_sort_toast(2, RuleSortScope::LooseOnly, &dests)
            .contains("Organized 2 loose icon(s) into Games, Programs / Craft"));

        let mut current = after.clone();
        current.rules.push(Rule {
            mode: 0,
            pattern: "lnk".into(),
            target: programs,
            target_tab: None,
            enabled: true,
        });
        let mut target = before.clone();
        target.page = 1;
        target.group_mut(games).unwrap().rolled = true;
        target.group_mut(games).unwrap().bounds.w = 420.;
        target.group_mut(programs).unwrap().active = 1;
        target.group_mut(programs).unwrap().custom_color = Some([0.2, 0.3, 0.4]);
        target.group_mut(programs).unwrap().tabs[1].sort = 2;

        assert!(!native_pins_differ(&after, &current));
        apply_non_pin_layout_from(&mut current, &target);
        assert_eq!(current.page, 1);
        assert!(current.group(games).unwrap().rolled);
        assert_eq!(current.group(games).unwrap().bounds.w, 420.);
        assert_eq!(current.group(programs).unwrap().active, 1);
        assert_eq!(current.group(programs).unwrap().custom_color, Some([0.2, 0.3, 0.4]));
        assert_eq!(current.group(programs).unwrap().tabs[1].sort, 2);
        // Pins and rules must remain intact during non-pin layout sync.
        assert_eq!(current.group(games).unwrap().tabs[0].items.len(), 1);
        assert_eq!(current.group(programs).unwrap().tabs[1].items.len(), 1);
        assert_eq!(current.rules.len(), 1);
    }

    #[test]
    fn native_pins_differ_treats_empty_group_and_tab_edits_as_non_pin_changes() {
        let mut base = State::empty();
        let g1 = base.add_group("Work", Bounds { x: 10., y: 20., w: 300., h: 200. });
        let pin = Item {
            id: 50,
            name: "Doc".into(),
            path: "C:/Desktop/Doc.lnk".into(),
            kind: "lnk".into(),
            size: 1,
            modified: 1,
            label: None,
        };
        base.group_mut(g1).unwrap().tab_mut().items.push(pin.clone());

        let mut with_empty_group = base.clone();
        let g2 = with_empty_group.add_group("New group", Bounds { x: 350., y: 20., w: 330., h: 260. });
        assert!(!native_pins_differ(&base, &with_empty_group));
        assert!(!native_pins_differ(&with_empty_group, &base));

        let mut with_two_tab_empty_group = with_empty_group.clone();
        with_two_tab_empty_group.add_tab(g2).unwrap();
        assert!(!native_pins_differ(&with_two_tab_empty_group, &base));
        assert!(!native_pins_differ(&base, &with_two_tab_empty_group));

        let mut with_empty_tab = base.clone();
        with_empty_tab.add_tab(g1).unwrap();
        assert!(!native_pins_differ(&base, &with_empty_tab));
        assert!(!native_pins_differ(&with_empty_tab, &base));

        let mut with_pinned_group = with_empty_group.clone();
        with_pinned_group.group_mut(g2).unwrap().tab_mut().items.push(pin.clone());
        assert!(native_pins_differ(&base, &with_pinned_group));
        assert!(native_pins_differ(&with_pinned_group, &base));

        let mut with_pinned_tab = with_empty_tab.clone();
        with_pinned_tab.group_mut(g1).unwrap().tabs[1].items.push(pin);
        assert!(native_pins_differ(&base, &with_pinned_tab));
        assert!(native_pins_differ(&with_pinned_tab, &base));

        // IPC `new-group` must wait when a native transaction is in flight.
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-ipc-newgrp-guard-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = App::new(root.join("workspace"), false, true);
        app.native_env.chrome_only = true;
        app.native_env.general_workspace = true;
        app.state = base.clone();
        app.native_tx_inflight = true;
        app.run_command("new-group", 1.0);
        assert_eq!(app.state.groups.len(), base.groups.len());
        assert!(app.dialog.is_none());
        assert!(app.undo.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn native_tx_inflight_guards_undo_restore_and_preserves_rollback_stack() {
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-tx-inflight-guard-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut app = App::new(root.join("workspace"), false, true);
        app.native_env.chrome_only = true;
        app.native_env.general_workspace = true;
        app.state = State::empty();
        let gid = app.state.add_group("Box", Bounds { x: 40., y: 40., w: 300., h: 240. });
        app.state.snapshot("Snap 1");

        let mut prior = app.state.clone();
        prior.group_mut(gid).unwrap().rolled = false;
        app.push_undo(prior.clone());
        app.state.group_mut(gid).unwrap().rolled = true;
        app.state.rules.push(Rule {
            mode: 0,
            pattern: "txt".into(),
            target: gid,
            target_tab: None,
            enabled: true,
        });

        // While a native transaction is in flight, Undo / Restore / Sort must NOT drop or race state.
        app.native_tx_inflight = true;
        app.request_undo(1.0);
        assert_eq!(app.undo.len(), 1);
        assert!(app.state.group(gid).unwrap().rolled);
        assert!(app.native_undo_inflight.is_none());

        app.request_restore_snapshot(0, 2.0);
        assert!(app.native_layout_restore_requested.is_none());
        assert!(app.toast.contains("still applying"));

        app.request_desktop_rule_sort(RuleSortScope::LooseOnly, 3.0);
        assert!(app.native_rules_sort_requested.is_none());

        // Once idle, non-pin Ctrl+Z pops into native_undo_inflight so rejected Phase 2 can recover it.
        app.native_tx_inflight = false;
        app.request_undo(4.0);
        assert!(app.undo.is_empty());
        assert!(!app.state.group(gid).unwrap().rolled);
        assert_eq!(app.native_undo_inflight.as_ref(), Some(&prior));
        assert!(app.native_tx_inflight);

        drop(app);
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn native_chrome_shortcuts_dispatches_ctrl_z_in_native_desktop_and_blocks_text_inputs() {
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-native-ctrl-z-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let make_app = |tag: &str| {
            let mut app = App::new(root.join(tag), false, false);
            app.native_env.chrome_only = true;
            app.native_env.general_workspace = true;
            app.host = None;
            app.automation = false;
            app.state = State::empty();
            let gid = app
                .state
                .add_group("Box", Bounds { x: 40., y: 40., w: 300., h: 240. });
            let prior = app.state.clone();
            app.state.group_mut(gid).unwrap().tabs[0].items.push(Item {
                id: 99,
                name: "Item.lnk".into(),
                path: "C:/Desktop/Item.lnk".into(),
                kind: "lnk".into(),
                size: 1,
                modified: 1,
                label: None,
            });
            (app, gid, prior)
        };
        let ctrl_z = || Input {
            ctrl: true,
            keys: vec![KeyCode::Z],
            time: 1.0,
            ..Default::default()
        };

        // Empty undo stack or wrong modifier chord must be ignored.
        let (mut app, gid, prior) = make_app("guards");
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert!(app.native_layout_restore_requested.is_none());

        app.push_undo(prior.clone());
        assert!(!app.native_chrome_shortcuts(&Input {
            ctrl: true,
            shift: true,
            keys: vec![KeyCode::Z],
            time: 1.0,
            ..Default::default()
        }));
        assert_eq!(app.undo.len(), 1);
        assert!(!app.native_chrome_shortcuts(&Input {
            ctrl: true,
            alt: true,
            keys: vec![KeyCode::Z],
            time: 1.0,
            ..Default::default()
        }));
        assert_eq!(app.undo.len(), 1);

        // Text-input / modal guards must not steal Ctrl+Z.
        app.inline_rename = Some((
            gid,
            99,
            "C:/Desktop/Item.lnk".into(),
            model::NameEdit::new("Item.lnk", "Item.lnk", false),
        ));
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert_eq!(app.undo.len(), 1);
        app.inline_rename = None;

        let portal_gid = app
            .state
            .add_group("Portal", Bounds { x: 380., y: 40., w: 300., h: 240. });
        app.state.group_mut(portal_gid).unwrap().tabs[0].folder = Some("C:/Portal".into());
        app.filter_focus = Some(portal_gid);
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert_eq!(app.undo.len(), 1);
        app.filter_focus = None;

        app.search_focus = true;
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert_eq!(app.undo.len(), 1);
        app.search_focus = false;

        app.peek = true;
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert_eq!(app.undo.len(), 1);
        app.peek = false;

        app.shortcut_recording = true;
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert_eq!(app.undo.len(), 1);
        app.shortcut_recording = false;

        app.dialog = Some(Dialog::Settings {
            page: 2,
            pattern: "pdf".into(),
            mode: 0,
            target: 0,
        });
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert_eq!(app.undo.len(), 1);

        app.dialog = Some(Dialog::EditRule {
            index: 0,
            mode: 0,
            pattern: String::new(),
            target: 0,
        });
        assert!(!app.native_chrome_shortcuts(&ctrl_z()));
        assert_eq!(app.undo.len(), 1);
        drop(app);

        // Settings page 3 ("Saved layouts") must dispatch Ctrl+Z immediately and clear scroll state.
        let (mut app, gid, prior) = make_app("settings-page-3");
        app.push_undo(prior);
        app.native_scroll_target = Some((gid, 2));
        app.native_page_request = Some((gid, 1));
        app.native_wheel_remainder = (gid, 60);
        app.dialog = Some(Dialog::Settings {
            page: 3,
            pattern: String::new(),
            mode: 0,
            target: 0,
        });
        assert!(app.native_chrome_shortcuts(&ctrl_z()));
        assert!(app.native_layout_restore_requested.is_some());
        assert!(app.undo.is_empty());
        assert!(app.native_tx_inflight);
        assert!(app.native_scroll_target.is_none());
        assert!(app.native_page_request.is_none());
        assert_eq!(app.native_wheel_remainder, (0, 0));
        drop(app);

        // Settings page 2 with an empty rule pattern must also allow Ctrl+Z.
        let (mut app, _, prior) = make_app("settings-page-2-empty");
        app.push_undo(prior);
        app.dialog = Some(Dialog::Settings {
            page: 2,
            pattern: String::new(),
            mode: 0,
            target: 0,
        });
        assert!(app.native_chrome_shortcuts(&ctrl_z()));
        assert!(app.native_layout_restore_requested.is_some());
        assert!(app.undo.is_empty());
        assert!(app.native_tx_inflight);
        drop(app);

        // Live desktop (no dialog) must dispatch Ctrl+Z via request_undo and clear scroll/selection/menu state.
        let (mut app, gid, prior) = make_app("desktop-none");
        app.push_undo(prior);
        app.selected = Some((gid, 99));
        app.multi_selected = vec![(gid, gid, 99)];
        app.classic_file_menu = Some(ClassicFileMenu {
            path: "C:/Desktop/Item.lnk".into(),
            anchor: Some((100, 100)),
            pin: Some((gid, gid, 99)),
        });
        app.native_scroll_target = Some((gid, 3));
        app.native_page_request = Some((gid, 1));
        app.native_wheel_remainder = (gid, 90);
        assert!(app.native_chrome_shortcuts(&ctrl_z()));
        assert!(app.native_layout_restore_requested.is_some());
        assert!(app.undo.is_empty());
        assert!(app.native_tx_inflight);
        assert!(app.selected.is_none());
        assert!(app.multi_selected.is_empty());
        assert!(app.classic_file_menu.is_none());
        assert!(app.native_scroll_target.is_none());
        assert!(app.native_page_request.is_none());
        assert_eq!(app.native_wheel_remainder, (0, 0));
        drop(app);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_native_structural_edit_enforces_pin_tab_conservation_and_allows_rich_operations() {
        let pin = |id: u64, path: &str| Item {
            id,
            name: path.into(),
            path: path.into(),
            kind: "lnk".into(),
            size: 1,
            modified: 1,
            label: None,
        };
        let mut base = State::empty();
        let g1 = base.add_group("Work", Bounds { x: 80., y: 80., w: 300., h: 240. });
        let g2 = base.add_group("Tools", Bounds { x: 420., y: 80., w: 300., h: 240. });
        base.group_mut(g1).unwrap().tabs[0].items = vec![
            pin(10, "C:/Desktop/Beta.lnk"),
            pin(11, "C:/Desktop/Alpha.lnk"),
        ];
        base.group_mut(g2).unwrap().tabs[0].items = vec![pin(20, "C:/Desktop/Tool.lnk")];

        // Sort within regular tab passes.
        let mut sorted = base.clone();
        sorted.group_mut(g1).unwrap().tabs[0].items.sort_by(|a, b| a.name.cmp(&b.name));
        assert!(validate_native_structural_edit(&base, &sorted, true).is_ok());

        // Adding/renaming/removing pages & moving a Box to another page passes.
        let mut with_page = base.clone();
        with_page.create_page("Project").unwrap();
        with_page.rename_page(1, "Work P2").unwrap();
        with_page.group_mut(g2).unwrap().page = 1;
        assert!(validate_native_structural_edit(&base, &with_page, true).is_ok());
        with_page.group_mut(g2).unwrap().page = 0;
        assert!(with_page.delete_empty_page(1).is_ok());
        assert!(validate_native_structural_edit(&base, &with_page, true).is_ok());

        // Merge -> reorder_tab -> detach all conserve pin-tab identity cleanly.
        let mut merged = base.clone();
        let g2_tab_id = merged.group(g2).unwrap().tabs[0].id;
        assert!(merged.merge(g2, g1));
        assert!(validate_native_structural_edit(&base, &merged, true).is_ok());
        assert!(merged.reorder_tab(g1, g2_tab_id, 0));
        assert!(validate_native_structural_edit(&base, &merged, true).is_ok());
        assert!(merged.detach(g1, 0, Bounds { x: 440., y: 100., w: 300., h: 240. }).is_some());
        assert!(validate_native_structural_edit(&base, &merged, true).is_ok());

        // Bind empty tab to folder, unlink, and remove Portal group all pass.
        let temp_dir = std::env::temp_dir().join(format!("dc-struct-portal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        let mut with_portal = base.clone();
        let gp = with_portal.add_group("Empty", Bounds { x: 760., y: 80., w: 280., h: 220. });
        with_portal.bind_group_folder(gp, &temp_dir).unwrap();
        assert!(validate_native_structural_edit(&base, &with_portal, true).is_ok());
        with_portal.group_mut(gp).unwrap().tabs[0].folder = None;
        assert!(validate_native_structural_edit(&base, &with_portal, true).is_ok());
        let _ = std::fs::remove_dir_all(&temp_dir);

        // Pin smuggling and last-regular-tab removal fail closed.
        let mut smuggled = base.clone();
        smuggled.group_mut(g1).unwrap().tabs[0].items.push(pin(99, "C:/Desktop/Sneaky.lnk"));
        assert!(validate_native_structural_edit(&base, &smuggled, true).is_err());

        let mut cleared_all = base.clone();
        cleared_all.groups.clear();
        assert!(validate_native_structural_edit(&base, &cleared_all, true).is_err());
    }

    #[test]
    fn stage1_and_stage2_helpers_preserve_native_conservation_law() {
        let pin = |id: u64, path: &str| Item {
            id,
            name: path.into(),
            path: path.into(),
            kind: "lnk".into(),
            size: 1,
            modified: 1,
            label: None,
        };
        let mut current = State::empty();
        let g1 = current.add_group("Keep", Bounds { x: 80., y: 80., w: 300., h: 240. });
        let g2 = current.add_group("RemoveMe", Bounds { x: 420., y: 80., w: 300., h: 240. });
        current.group_mut(g1).unwrap().tabs[0].items = vec![pin(10, "C:/Desktop/A.lnk")];
        current.group_mut(g2).unwrap().tabs[0].items = vec![pin(20, "C:/Desktop/B.lnk")];

        let mut target = current.clone();
        target.groups.retain(|g| g.id != g2);
        let g3 = target.add_group("Created", Bounds { x: 440., y: 120., w: 300., h: 240. });
        target.group_mut(g3).unwrap().tabs[0].items = vec![pin(30, "C:/Desktop/C.lnk")];

        let stage1 = stage1_unpin_incompatible_tabs(&current, &target).expect("Stage 1 unpins RemoveMe");
        assert!(native_layout_structure_matches(&current, &stage1));
        assert_eq!(stage1.group(g1).unwrap().tabs[0].items.len(), 1);
        assert!(stage1.group(g2).unwrap().tabs[0].items.is_empty());

        let stage2 = stage2_apply_target_structure_preserving_pins(&stage1, &target);
        assert!(native_layout_structure_matches(&stage2, &target));
        assert!(validate_native_structural_edit(&stage1, &stage2, true).is_ok());
        assert_eq!(stage2.group(g1).unwrap().tabs[0].items.len(), 1);
        assert!(stage2.group(g3).unwrap().tabs[0].items.is_empty());
        assert!(!native_regular_pins_match(&stage2, &target));
    }

    #[test]
    fn native_chrome_shortcuts_handles_esc_dismissal_and_page_switching() {
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-native-esc-page-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut app = App::new(root.join("ws"), false, false);
        app.native_env.chrome_only = true;
        app.native_env.general_workspace = true;
        app.state = State::empty();
        app.state.create_page("Second").unwrap();
        app.state.prefs.pages = true;
        app.state.page = 0;

        let esc = Input {
            keys: vec![KeyCode::Escape],
            time: 1.0,
            ..Default::default()
        };
        app.dialog = Some(Dialog::EditRule {
            index: 0,
            mode: 0,
            pattern: String::new(),
            target: 0,
        });
        assert!(!app.native_chrome_shortcuts(&esc));
        assert!(matches!(app.dialog, Some(Dialog::Settings { page: 2, .. })));

        assert!(!app.native_chrome_shortcuts(&esc));
        assert!(app.dialog.is_none());

        app.menu = Some((1, Vec2::new(10., 10.)));
        assert!(!app.native_chrome_shortcuts(&esc));
        assert!(app.menu.is_none());

        app.peek = true;
        assert!(!app.native_chrome_shortcuts(&esc));
        assert!(!app.peek);

        // Ctrl+Right / Ctrl+Left switch pages cleanly when idle.
        let right = Input {
            ctrl: true,
            keys: vec![KeyCode::Right],
            time: 2.0,
            ..Default::default()
        };
        let left = Input {
            ctrl: true,
            keys: vec![KeyCode::Left],
            time: 3.0,
            ..Default::default()
        };
        assert!(!app.native_chrome_shortcuts(&right));
        assert_eq!(app.state.page, 1);
        assert!(!app.native_chrome_shortcuts(&left));
        assert_eq!(app.state.page, 0);

        // Disabled prefs.pages or open dialog blocks page hotkeys.
        app.state.prefs.pages = false;
        assert!(!app.native_chrome_shortcuts(&right));
        assert_eq!(app.state.page, 0);

        drop(app);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn disabled_desktop_blocks_workspace_commands_and_preserves_esc_isolation() {
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-disabled-esc-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut app = App::new(root.join("ws"), false, true);
        app.native_env.chrome_only = true;
        app.native_env.general_workspace = true;
        app.state = State::empty();
        app.state.prefs.desktop_enabled = false;
        app.settings_bring_to_front = false;

        app.run_command("new-group", 1.0);
        assert!(app.state.groups.is_empty());
        assert!(matches!(app.dialog, Some(Dialog::Settings { page: 4, .. })));
        assert!(app.settings_bring_to_front);

        // Esc delivered to host does not close independent SettingsWindow.
        let esc = Input {
            keys: vec![KeyCode::Escape],
            time: 2.0,
            ..Default::default()
        };
        app.native_chrome_shortcuts(&esc);
        if cfg!(windows) {
            assert!(matches!(app.dialog, Some(Dialog::Settings { page: 4, .. })));
        }

        // Sub-modal opened while Settings is open restores Settings on Esc.
        app.settings_bring_to_front = false;
        app.open_modal_preserving_settings(Dialog::Rename {
            group: 42,
            value: "Box".into(),
        });
        assert!(app.has_blocking_dialog());
        app.native_chrome_shortcuts(&esc);
        assert!(matches!(app.dialog, Some(Dialog::Settings { page: 4, .. })));
        assert!(app.settings_return.is_none());
        assert!(app.settings_bring_to_front);

        // Switching Settings to another page stays on that page when "settings" repeats.
        app.dialog = Some(Dialog::Settings {
            page: 1,
            pattern: String::new(),
            mode: 0,
            target: 0,
        });
        app.settings_bring_to_front = false;
        app.run_command("settings", 3.0);
        assert!(matches!(app.dialog, Some(Dialog::Settings { page: 1, .. })));
        assert!(app.settings_bring_to_front);

        drop(app);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn onboarding_and_custom_layouts_and_popup_portal_visibility_invariants() {
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-onboarding-layout-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let dl_dir = root.join("Downloads");
        let pic_dir = root.join("Pictures");
        let desk_dir = root.join("Desktop");
        std::fs::create_dir_all(&dl_dir).unwrap();
        std::fs::create_dir_all(&pic_dir).unwrap();
        std::fs::create_dir_all(&desk_dir).unwrap();
        std::fs::write(dl_dir.join("setup.exe"), b"bin").unwrap();
        std::fs::write(pic_dir.join("photo.png"), b"img").unwrap();
        let app_lnk = desk_dir.join("Browser.lnk");
        let report_pdf = desk_dir.join("Report.pdf");
        let sheet_xlsx = desk_dir.join("Budget.xlsx");
        std::fs::write(&app_lnk, b"lnk").unwrap();
        std::fs::write(&report_pdf, b"pdf").unwrap();
        std::fs::write(&sheet_xlsx, b"xlsx").unwrap();

        let mut app = App::new(root.join("ws"), false, true);
        app.apply_onboarding_layout(
            Some(dl_dir.clone()),
            Some(pic_dir.clone()),
            vec![app_lnk.clone(), report_pdf.clone(), sheet_xlsx.clone()],
        );
        assert_eq!(app.state.groups.len(), 2);
        let center = &app.state.groups[0];
        let right = &app.state.groups[1];
        assert_eq!(center.tabs.len(), 2);
        assert_eq!(center.tabs[0].name, "Shortcuts");
        assert!(center.tabs[0].folder.is_none());
        assert_eq!(center.tabs[0].items.len(), 1);
        assert_eq!(center.tabs[1].name, "Downloads");
        assert_eq!(
            center.tabs[1].folder.as_deref(),
            Some(dl_dir.to_string_lossy().as_ref())
        );
        assert!(center.tabs[1].list);

        assert_eq!(right.dock, Some(DockEdge::Right));
        assert!(!right.rolled);
        assert_eq!(right.tabs.len(), 2);
        assert_eq!(right.tabs[0].name, "Documents");
        assert!(right.tabs[0].folder.is_none());
        assert_eq!(right.tabs[0].items.len(), 2);
        assert_eq!(right.tabs[1].name, "Pictures");
        assert_eq!(
            right.tabs[1].folder.as_deref(),
            Some(pic_dir.to_string_lossy().as_ref())
        );
        assert!(!right.tabs[1].list);
        assert!(center.bounds.x + center.bounds.w <= right.bounds.x);

        // Hover opacity and not-hovered opacity share the exact same 0..1 scale.
        let mut g = center.clone();
        g.opacity = 0.50;
        let rgba = paint::panel_rgba(&g);
        assert!((rgba[3] - 0.50).abs() < 1e-5);
        let (hovered_a, _) = paint::effective_panel_alpha_and_fade(rgba[3], true, 0.50, true, false);
        let (unhovered_a, _) = paint::effective_panel_alpha_and_fade(rgba[3], true, 0.50, false, false);
        assert!((hovered_a - unhovered_a).abs() < 1e-5);

        // Clicking `...` menu or Portal sort menu never lifts/clears host (so Portal icons stay visible).
        app.dialog = None;
        app.menu = Some((center.id, vec2(100., 100.)));
        assert!(!app.host_lifted_for_popup());
        app.menu = None;
        app.folder_sort_menu = Some((center.id, vec2(120., 120.)));
        assert!(!app.host_lifted_for_popup());
        app.folder_sort_menu = None;
        app.dialog = Some(Dialog::Rename {
            group: center.id,
            value: "Box".into(),
        });
        assert!(app.host_lifted_for_popup());

        // Custom starter layout creates a single centered regular box.
        app.dialog = None;
        app.apply_custom_starter_layout();
        assert_eq!(app.state.groups.len(), 1);
        assert_eq!(app.state.groups[0].tabs.len(), 1);
        assert!(app.state.groups[0].tabs[0].folder.is_none());

        drop(app);
        let _ = std::fs::remove_dir_all(&root);
    }
