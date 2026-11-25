    use super::*;
    #[test]
    fn ctrl_click_and_box_marquee_support_batch_drag_to_another_box() {
        let mut app = App::new(
            std::env::temp_dir().join("desktopcraft-ui-batch-fixture"),
            true,
            false,
        );
        app.state = State::empty();
        let a = app.state.add_group(
            "Source",
            Bounds {
                x: 100.,
                y: 100.,
                w: 310.,
                h: 300.,
            },
        );
        let b = app.state.add_group(
            "Destination",
            Bounds {
                x: 450.,
                y: 100.,
                w: 310.,
                h: 300.,
            },
        );
        let tid = app.state.group(a).unwrap().tab().id;
        for id in [91, 92, 93] {
            app.state.group_mut(a).unwrap().tab_mut().items.push(Item {
                id,
                name: format!("Craft{id}.zip"),
                path: format!("C:/Desktop/Craft{id}.zip"),
                kind: "zip".into(),
                size: 1,
                modified: 0,
                label: None,
            });
        }
        let cells = [
            Hit {
                bounds: Bounds {
                    x: 120.,
                    y: 170.,
                    w: 80.,
                    h: 80.,
                },
                target: Target::Item(a, 91),
            },
            Hit {
                bounds: Bounds {
                    x: 215.,
                    y: 170.,
                    w: 80.,
                    h: 80.,
                },
                target: Target::Item(a, 92),
            },
            Hit {
                bounds: Bounds {
                    x: 310.,
                    y: 170.,
                    w: 80.,
                    h: 80.,
                },
                target: Target::Item(a, 93),
            },
        ];
        app.pointer(
            &Input {
                pos: vec2(140., 190.),
                pressed: true,
                ctrl: true,
                time: 1.,
                ..Default::default()
            },
            &cells,
        );
        app.pointer(
            &Input {
                pos: vec2(230., 190.),
                pressed: true,
                ctrl: true,
                time: 2.,
                ..Default::default()
            },
            &cells,
        );
        assert_eq!(app.multi_selected.len(), 2);
        app.pointer(
            &Input {
                pos: vec2(140., 190.),
                pressed: true,
                time: 3.,
                ..Default::default()
            },
            &cells,
        );
        app.pointer(
            &Input {
                pos: vec2(470., 180.),
                released: true,
                time: 3.2,
                ..Default::default()
            },
            &cells,
        );
        assert_eq!(app.state.group(b).unwrap().tab().items.len(), 2);
        assert_eq!(app.state.group(a).unwrap().tab().items[0].id, 93);
        // Create a blank-space marquee over the remaining item and verify
        // the selection uses actual item-hit geometry in the active tab.
        let body = Hit {
            bounds: Bounds {
                x: 100.,
                y: 140.,
                w: 310.,
                h: 260.,
            },
            target: Target::Body(a),
        };
        let source_hits = vec![body, cells[2].clone()];
        app.pointer(
            &Input {
                pos: vec2(300., 155.),
                pressed: true,
                time: 4.,
                ..Default::default()
            },
            &source_hits,
        );
        app.pointer(
            &Input {
                pos: vec2(399., 269.),
                released: true,
                time: 4.2,
                ..Default::default()
            },
            &source_hits,
        );
        assert_eq!(app.multi_selected, vec![(a, tid, 93)]);
    }

    #[test]
    fn normal_boxes_change_pins_without_moving_files() {
        let root = std::env::temp_dir().join(format!(
            "desktop-spaces-virtual-box-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("sample.txt");
        std::fs::write(&original, "original bytes unchanged").unwrap();
        let mut app = App::new(root.join("workspace"), true, false);
        app.state = State::empty();
        let a = app.state.add_group(
            "Regular A",
            Bounds {
                x: 10.,
                y: 10.,
                w: 200.,
                h: 200.,
            },
        );
        let b = app.state.add_group(
            "Regular B",
            Bounds {
                x: 220.,
                y: 10.,
                w: 200.,
                h: 200.,
            },
        );
        app.state.import(&original, a).unwrap();
        app.drop_path_into_box(&original.to_string_lossy(), b, 1.)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&original).unwrap(),
            "original bytes unchanged"
        );
        assert!(!app
            .state
            .group(a)
            .unwrap()
            .tab()
            .items
            .iter()
            .any(|i| i.path == original.to_string_lossy()));
        assert!(app
            .state
            .group(b)
            .unwrap()
            .tab()
            .items
            .iter()
            .any(|i| i.path == original.to_string_lossy()));
        // Re-dropping the same pin must not duplicate it or change the file.
        app.drop_path_into_box(&original.to_string_lossy(), b, 2.)
            .unwrap();
        assert_eq!(app.state.group(b).unwrap().tab().items.len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn only_a_linked_folder_view_moves_the_disposable_test_file() {
        crate::shell::init();
        let root = std::env::temp_dir().join(format!(
            "desktop-spaces-linked-folder-test-{}",
            std::process::id()
        ));
        let source_dir = root.join("source");
        let linked = root.join("linked");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&linked).unwrap();
        let original = source_dir.join("sample.txt");
        std::fs::write(&original, "only moved when linked").unwrap();
        let mut app = App::new(root.join("workspace"), true, false);
        app.state = State::empty();
        let linked_id = app
            .state
            .add_portal(
                &linked,
                Bounds {
                    x: 10.,
                    y: 10.,
                    w: 200.,
                    h: 200.,
                },
            )
            .unwrap();
        app.drop_path_into_box(&original.to_string_lossy(), linked_id, 1.)
            .unwrap();
        let destination = linked.join("sample.txt");
        assert!(!original.exists());
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "only moved when linked"
        );
        assert!(app
            .state
            .group(linked_id)
            .unwrap()
            .tab()
            .items
            .iter()
            .any(|i| i.path == destination.to_string_lossy()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn classic_file_menu_command_pins_without_moving_the_source_file() {
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-shell-pin-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("original with spaces.txt");
        std::fs::write(&source, "stay here").unwrap();
        let mut app = App::new(root.join("workspace"), true, false);
        app.state = State::empty();
        app.run_command(&format!("pin-file {}", source.display()), 1.);
        let box_ref = app.state.groups.first().unwrap();
        assert!(box_ref.tab().folder.is_none());
        assert_eq!(box_ref.tab().items.len(), 1);
        assert_eq!(box_ref.tab().items[0].path, source.to_string_lossy());
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "stay here");
        app.run_command(&format!("pin-file {}", source.display()), 2.);
        assert_eq!(app.state.groups[0].tab().items.len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn box_type_ahead_selects_latin_and_thai_and_scrolls_into_view() {
        let root = std::env::temp_dir().join("desktop-spaces-typeahead-test");
        let mut app = App::new(root, true, false);
        app.state = State::empty();
        let id = app.state.add_group(
            "Alphabet",
            Bounds {
                x: 50.,
                y: 50.,
                w: 180.,
                h: 180.,
            },
        );
        for i in 0..18 {
            app.state.group_mut(id).unwrap().tab_mut().items.push(Item {
                id: i + 100,
                name: format!("item{i}.txt"),
                path: format!("item{i}.txt"),
                kind: "txt".into(),
                size: 0,
                modified: 0,
                label: None,
            });
        }
        app.state.group_mut(id).unwrap().tab_mut().items.push(Item {
            id: 900,
            name: "Apple.txt".into(),
            path: "Apple.txt".into(),
            kind: "txt".into(),
            size: 0,
            modified: 0,
            label: None,
        });
        app.state.group_mut(id).unwrap().tab_mut().items.push(Item {
            id: 901,
            name: "กุญแจ.txt".into(),
            path: "กุญแจ.txt".into(),
            kind: "txt".into(),
            size: 0,
            modified: 0,
            label: None,
        });
        app.selected = Some((id, 0));
        app.shortcuts(&Input {
            typed: "a".into(),
            time: 2.,
            ..Default::default()
        });
        assert_eq!(app.selected, Some((id, 900)));
        assert!(app.state.group(id).unwrap().tab().scroll > 0);
        app.shortcuts(&Input {
            typed: "ก".into(),
            time: 4.,
            ..Default::default()
        });
        assert_eq!(app.selected, Some((id, 901)));
    }

    #[test]
    fn first_start_initializes_three_boxes_only_for_missing_workspace() {
        let root =
            std::env::temp_dir().join(format!("desktopcraft-new-defaults-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = App::new(root.clone(), false, true);
        assert!(app.host.as_ref().unwrap().first_run);
        assert_eq!(
            app.state
                .groups
                .iter()
                .map(|g| g.tab().name.as_str())
                .collect::<Vec<_>>(),
            vec!["Shortcuts", "Documents", "Folders"]
        );
        assert_eq!(app.state.rules.len(), 3);
        assert!(app
            .state
            .rules
            .iter()
            .any(|rule| rule.mode == 10 && rule.enabled));
        assert!(!app.state.auto_rules);
        drop(app);
        let path = root.join("data/workspace.json");
        let mut old = State::empty();
        old.add_group(
            "Keep my boxes",
            Bounds {
                x: 10.,
                y: 10.,
                w: 300.,
                h: 260.,
            },
        );
        old.prefs.peek_shortcut = "Ctrl+Shift+P".into();
        old.save(&path).unwrap();
        let reopened = App::new(root.clone(), false, true);
        assert_eq!(reopened.state.groups.len(), 1);
        assert_eq!(reopened.state.groups[0].tab().name, "Keep my boxes");
        assert_eq!(reopened.state.prefs.peek_shortcut, "Ctrl+Shift+P");
        assert!(!reopened.host.as_ref().unwrap().first_run);
        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn inactive_tab_drop_and_reorder_preserve_files_rules_and_active_tab_preference() {
        let root =
            std::env::temp_dir().join(format!("desktopcraft-tab-actions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut app = App::new(root.join("workspace"), true, false);
        app.state = State::empty();
        let b = Bounds {
            x: 40.,
            y: 80.,
            w: 340.,
            h: 280.,
        };
        let group = app.state.add_group("Main", b);
        let second = app.state.add_group("Other", b);
        let destination = app.state.group(second).unwrap().tab().id;
        assert!(app.state.merge(second, group));
        let source = app.state.group(group).unwrap().tab().id;
        let path = root.join("first.txt");
        std::fs::write(&path, b"untouched").unwrap();
        app.drop_path_into_tab(&path.to_string_lossy(), group, destination, 1.)
            .unwrap();
        assert_eq!(app.state.group(group).unwrap().tab().id, source);
        assert_eq!(app.state.group(group).unwrap().tabs[1].items.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), b"untouched");
        app.state.prefs.switch_tab_after_drop = true;
        let next = root.join("second.txt");
        std::fs::write(&next, b"still unchanged").unwrap();
        app.drop_path_into_tab(&next.to_string_lossy(), group, destination, 2.)
            .unwrap();
        assert_eq!(app.state.group(group).unwrap().tab().id, destination);
        let bounds = app.state.group(group).unwrap().header_tabs().0;
        assert!(bounds.iter().all(|(_, r)| r.x + r.w <= b.x + b.w - 82.));
        app.state.rules.push(Rule {
            mode: 0,
            pattern: "txt".into(),
            target: group,
            target_tab: Some(destination),
            enabled: true,
        });
        assert!(app.state.reorder_tab(group, destination, 0));
        assert_eq!(app.state.group(group).unwrap().tabs[0].id, destination);
        assert_eq!(app.state.group(group).unwrap().active, 0);
        assert_eq!(
            app.state.rule_destination(app.state.rules.last().unwrap()),
            Some((group, destination))
        );
        assert_eq!(std::fs::read(&next).unwrap(), b"still unchanged");
        drop(app);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stacked_rollups_keep_first_hovered_box_open_until_pointer_exits() {
        let root = std::env::temp_dir().join("desktopcraft-stacked-rollup-test");
        let mut app = App::new(root, true, false);
        app.state = State::empty();
        let upper = app.state.add_group(
            "Top",
            Bounds {
                x: 100.,
                y: 100.,
                w: 240.,
                h: 260.,
            },
        );
        let lower = app.state.add_group(
            "Below",
            Bounds {
                x: 100.,
                y: 335.,
                w: 240.,
                h: 260.,
            },
        );
        app.state.group_mut(upper).unwrap().rolled = true;
        app.state.group_mut(lower).unwrap().rolled = true;
        let pointer = |y: f32, time| Input {
            pos: vec2(140., y),
            time,
            ..Default::default()
        };
        app.state.prefs.rollup_open = 0;
        app.update_rollup_reveal(&pointer(115., 1.));
        assert_eq!(app.revealed, Some(upper));
        // The lower title is underneath the open upper body's last 25 px.
        app.update_rollup_reveal(&pointer(350., 1.1));
        assert_eq!(app.revealed, Some(upper));
        assert_eq!(
            paint_order(&app.state.groups, 0, app.revealed)
                .map(|g| g.id)
                .collect::<Vec<_>>(),
            vec![lower, upper]
        );
        // Only AFTER leaving the upper bounds may the lower title respond.
        app.update_rollup_reveal(&pointer(365., 1.2));
        assert_eq!(app.revealed, Some(lower));
        app.state.prefs.rollup_open = 1;
        app.revealed = None;
        app.hover_rolled = None;
        app.update_rollup_reveal(&pointer(115., 2.));
        app.update_rollup_reveal(&pointer(115., 2. + ROLLUP_DELAY + 0.01));
        assert_eq!(app.revealed, Some(upper));
        app.update_rollup_reveal(&pointer(350., 3.));
        assert_eq!(app.revealed, Some(upper));
        app.update_rollup_reveal(&pointer(365., 3.1));
        assert_eq!(app.revealed, None);
        app.update_rollup_reveal(&pointer(365., 3.1 + ROLLUP_DELAY + 0.01));
        assert_eq!(app.revealed, Some(lower));
        app.state.prefs.rollup_open = 2;
        app.revealed = Some(upper);
        app.update_rollup_reveal(&pointer(350., 4.));
        assert_eq!(app.revealed, Some(upper));
        app.update_rollup_reveal(&pointer(365., 4.1));
        assert_eq!(app.revealed, None);
    }

    #[test]
    fn reveal_requires_tab_hover_and_keeps_contents_usable() {
        let root = std::env::temp_dir().join("desktop-spaces-rollup-test-data");
        let mut app = App::new(root, true, false);
        app.state = State::empty();
        let id = app.state.add_group(
            "Rolled",
            Bounds {
                x: 100.,
                y: 100.,
                w: 240.,
                h: 230.,
            },
        );
        app.state.group_mut(id).unwrap().rolled = true;
        let pointer = |x, y, time| Input {
            pos: vec2(x, y),
            time,
            ..Default::default()
        };
        // The phantom body must not open the rolled group.
        app.state.prefs.rollup_open = 0;
        app.update_rollup_reveal(&pointer(140., 260., 1.));
        assert_eq!(app.revealed, None);
        // Only touching the title/tabs initiates the reveal.
        app.update_rollup_reveal(&pointer(140., 115., 1.1));
        assert_eq!(app.revealed, Some(id));
        // Once open, moving into the actual content must not instantly close it.
        app.update_rollup_reveal(&pointer(140., 260., 1.2));
        assert_eq!(app.revealed, Some(id));
        app.update_rollup_reveal(&pointer(500., 500., 1.3));
        assert_eq!(app.revealed, None);
        app.state.prefs.rollup_open = 1;
        app.update_rollup_reveal(&pointer(140., 115., 2.));
        assert_eq!(app.revealed, None);
        app.update_rollup_reveal(&pointer(140., 115., 2. + ROLLUP_DELAY + 0.01));
        assert_eq!(app.revealed, Some(id));
        app.update_rollup_reveal(&pointer(140., 260., 3.));
        assert_eq!(app.revealed, Some(id));
        app.update_rollup_reveal(&pointer(500., 500., 3.1));
        assert_eq!(app.revealed, None);
    }

    #[test]
    fn native_floating_header_rollup_reveals_on_floating_header_and_stays_open_in_body() {
        let root = std::env::temp_dir().join("desktop-spaces-native-rollup-test-data");
        let mut app = App::new(root, true, false);
        app.state = State::empty();
        app.host = Some(desktop::HostState::new(false));
        app.native_env.chrome_only = true;
        let id = app.state.add_group(
            "Games",
            Bounds {
                x: 1200.,
                y: 80.,
                w: 360.,
                h: 320.,
            },
        );
        app.state.group_mut(id).unwrap().rolled = true;
        app.state.prefs.rollup_open = 0;
        let pointer = |x, y, time| Input {
            pos: vec2(x, y),
            time,
            ..Default::default()
        };
        // Native header floats ABOVE g.bounds.y (y = 80 - HEADER .. 80); body alone must not open it.
        app.update_rollup_reveal(&pointer(1280., 160., 1.0));
        assert_eq!(app.revealed, None);
        app.update_rollup_reveal(&pointer(1280., 58., 1.1));
        assert_eq!(app.revealed, Some(id));
        // Moving into the expanded body keeps the rolled group revealed.
        app.update_rollup_reveal(&pointer(1280., 260., 1.2));
        assert_eq!(app.revealed, Some(id));
        app.update_rollup_reveal(&pointer(500., 500., 1.3));
        assert_eq!(app.revealed, None);

        let revealed_rect = rollup_revealed_bounds(app.state.group(id).unwrap(), true);
        assert_eq!(revealed_rect.y, 80. - HEADER);
        assert_eq!(revealed_rect.h, 320. + HEADER);
    }

    #[test]
    fn new_rollup_reveal_holds_during_live_pointer_drag_and_release_cooldown() {
        let mut hold_until = 0.;
        // Non-chrome or automation modes never hold hover-reveal.
        assert!(!should_hold_new_rollup_reveal(false, true, 10.0, &mut hold_until));
        assert_eq!(hold_until, 0.);

        // Active live pointer button hold suppresses new reveals and arms 0.35s cooldown.
        assert!(should_hold_new_rollup_reveal(true, true, 10.0, &mut hold_until));
        assert!((hold_until - 10.35).abs() < 1e-9);

        // Cooldown holds immediately after button release so a Tab drop settles before reflow.
        assert!(should_hold_new_rollup_reveal(true, false, 10.20, &mut hold_until));
        assert!(!should_hold_new_rollup_reveal(true, false, 10.35, &mut hold_until));
        assert!(!should_hold_new_rollup_reveal(true, false, 10.50, &mut hold_until));
    }

    #[test]
    fn native_effective_state_reveals_rolled_box_and_occludes_overlapped_box() {
        let mut s = State::empty();
        let games = s.add_group("Games", Bounds { x: 1284., y: 1., w: 467., h: 637. });
        let docs = s.add_group("Docs", Bounds { x: 1284., y: 69., w: 467., h: 569. });
        let programs = s.add_group("Programs", Bounds { x: 1788., y: 31., w: 740., h: 416. });
        s.group_mut(games).unwrap().rolled = true;
        s.group_mut(docs).unwrap().tabs.push(Tab {
            folder: Some("C:/Users/demo/OneDrive".into()),
            ..Tab::new(900, "Cloud")
        });

        let (idle_eff, idle_over) = native_effective_state(&s, None, false);
        assert!(idle_over.is_empty());
        assert!(idle_eff.group(games).unwrap().rolled);
        assert!(!idle_eff.group(docs).unwrap().rolled);
        assert!(!idle_eff.group(programs).unwrap().rolled);

        // Hover-revealing `games` temporarily unrolls `games` and occludes overlapping `docs`
        // (both regular Tab 0 and Folder Portal Tab 1) while leaving `programs` untouched.
        assert!(native_revealed_occludes(&s, Some(games), s.group(docs).unwrap()));
        assert!(!native_revealed_occludes(&s, Some(games), s.group(programs).unwrap()));
        let (eff, overrides) = native_effective_state(&s, Some(games), false);
        assert!(!eff.group(games).unwrap().rolled);
        assert!(eff.group(docs).unwrap().rolled);
        assert!(!eff.group(programs).unwrap().rolled);
        assert_eq!(overrides, vec![(games, true), (docs, false)]);

        s.group_mut(docs).unwrap().active = 1;
        assert!(native_revealed_occludes(&s, Some(games), s.group(docs).unwrap()));
        let (portal_eff, portal_over) = native_effective_state(&s, Some(games), false);
        assert!(!portal_eff.group(games).unwrap().rolled);
        assert!(portal_eff.group(docs).unwrap().rolled);
        assert_eq!(portal_over, vec![(games, true), (docs, false)]);

        // If the revealed group itself is on a Folder Portal tab, native_effective_state leaves native state alone.
        s.group_mut(games).unwrap().tab_mut().folder = Some("C:/GamesPortal".into());
        let (portal_rev_eff, portal_rev_over) = native_effective_state(&s, Some(games), false);
        assert!(portal_rev_over.is_empty());
        assert!(portal_rev_eff.group(games).unwrap().rolled);
    }

    #[test]
    fn native_effective_state_handles_quick_hide_keep_visible_and_disabled_rollup() {
        let mut s = State::empty();
        let normal = s.add_group("Normal", Bounds { x: 80., y: 80., w: 300., h: 240. });
        let keep = s.add_group("Pinned", Bounds { x: 420., y: 80., w: 300., h: 240. });
        s.group_mut(keep).unwrap().keep_visible = true;

        // Quick-Hide rolls non-keep_visible groups and leaves keep_visible expanded.
        let (hidden_eff, hidden_over) = native_effective_state(&s, None, true);
        assert!(hidden_eff.group(normal).unwrap().rolled);
        assert!(!hidden_eff.group(keep).unwrap().rolled);
        assert_eq!(hidden_over, vec![(normal, false)]);

        // Roll-up Enabled = OFF unrolls rolled groups while recording original rolled=true override.
        s.group_mut(normal).unwrap().rolled = true;
        s.prefs.rollup = false;
        let (unrolled_eff, unrolled_over) = native_effective_state(&s, None, false);
        assert!(!unrolled_eff.group(normal).unwrap().rolled);
        assert!(!unrolled_eff.group(keep).unwrap().rolled);
        assert_eq!(unrolled_over, vec![(normal, true)]);
    }

    #[test]
    fn native_point_hits_box_or_loose_icon_distinguishes_blank_desktop_from_targets() {
        let dir = std::env::temp_dir().join(format!(
            "dc-quick-hide-hit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        let mut app = App::new(dir.clone(), false, false);
        app.state = State::empty();
        let _b1 = app.state.add_group("Work", Bounds { x: 100., y: 100., w: 280., h: 220. });
        let b2 = app.state.add_group("Pinned", Bounds { x: 450., y: 100., w: 260., h: 220. });
        app.state.group_mut(b2).unwrap().keep_visible = true;
        app.native_visible_loose_rects = std::sync::Arc::from(vec![[820, 120, 72, 96]]);

        // Hits Box header/body and loose icon rect, misses empty desktop background.
        assert!(app.native_point_hits_box_or_loose_icon(140, 82));
        assert!(app.native_point_hits_box_or_loose_icon(180, 180));
        assert!(app.native_point_hits_box_or_loose_icon(850, 160));
        assert!(!app.native_point_hits_box_or_loose_icon(410, 40));
        assert!(!app.native_point_hits_box_or_loose_icon(40, 40));

        // While hidden, non-keep_visible Box is hidden so blank area under it toggles unhide,
        // while keep_visible Box still intercepts double-click.
        app.hidden = true;
        assert!(!app.native_point_hits_box_or_loose_icon(180, 180));
        assert!(app.native_point_hits_box_or_loose_icon(500, 180));
        assert!(!app.native_point_hits_box_or_loose_icon(410, 40));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn visual_revealed_waits_for_native_ack_on_regular_box_but_not_portal() {
        let dir = std::env::temp_dir().join(format!(
            "dc-visual-reveal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        let mut app = App::new(dir.clone(), false, false);
        let reg = app.state.add_group("Games", Bounds { x: 100., y: 80., w: 300., h: 240. });
        let portal = app.state.add_group("Cloud", Bounds { x: 460., y: 80., w: 300., h: 240. });
        app.state.group_mut(reg).unwrap().rolled = true;
        app.state.group_mut(portal).unwrap().rolled = true;
        app.state.group_mut(portal).unwrap().tab_mut().folder = Some("C:/Cloud".into());
        app.native_env.in_box_layout = true;

        // Regular rolled Box waits for worker ACK (`native_visual_revealed`) in live Chrome mode.
        app.revealed = Some(reg);
        app.native_visual_revealed = None;
        assert_eq!(app.visual_revealed(true), None);
        app.native_visual_revealed = Some(reg);
        assert_eq!(app.visual_revealed(true), Some(reg));

        // Folder Portal expands immediately without waiting on Explorer ACK.
        app.revealed = Some(portal);
        app.native_visual_revealed = None;
        assert_eq!(app.visual_revealed(true), Some(portal));

        // Non-chrome or automation test mode always reads `app.revealed` directly.
        app.revealed = Some(reg);
        assert_eq!(app.visual_revealed(false), Some(reg));
        app.automation = true;
        assert_eq!(app.visual_revealed(true), Some(reg));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn verified_drop_ignores_transient_hover_reveal_flip_and_blocks_real_edits() {
        let mut accepted = State::empty();
        let games = accepted.add_group(
            "Games",
            Bounds {
                x: 80.,
                y: 80.,
                w: 320.,
                h: 260.,
            },
        );
        let tools = accepted.add_group(
            "Tools",
            Bounds {
                x: 460.,
                y: 80.,
                w: 320.,
                h: 260.,
            },
        );
        let pin_path = "C:/Desktop/Game.lnk".to_string();
        accepted.group_mut(games).unwrap().rolled = true;
        accepted.group_mut(games).unwrap().tab_mut().items.push(Item {
            id: 116,
            name: "Game.lnk".into(),
            path: pin_path.clone(),
            kind: "lnk".into(),
            size: 1,
            modified: 0,
            label: None,
        });
        accepted.add_tab(games).unwrap();
        accepted.group_mut(games).unwrap().active = 0;
        accepted.group_mut(tools).unwrap().rolled = true;

        // Case 1: BoxToDesktop from a hover-revealed rolled Box succeeds even after
        // the pointer leaves and transient `app.revealed` flips back to `None`.
        let (proposal_before, _) = native_effective_state(&accepted, Some(games), false);
        assert!(!proposal_before.group(games).unwrap().rolled);
        let mut proposal_next = proposal_before.as_ref().clone();
        proposal_next.group_mut(games).unwrap().tab_mut().items.clear();
        assert!(native_drop_matches_acknowledged_ui(
            &accepted,
            Some(games),
            false,
            &accepted,
            false,
            &proposal_before,
        ));
        let patched_real =
            native_ole_persist::pins_change_onto(&accepted, &proposal_next, &[pin_path]).unwrap();
        assert!(patched_real.group(games).unwrap().rolled);
        assert!(patched_real.group(games).unwrap().tab().items.is_empty());

        // Case 2: Unrolled Box drop succeeds even if transient hover touches another rolled header.
        let mut unrolled = accepted.clone();
        unrolled.group_mut(games).unwrap().rolled = false;
        assert!(native_drop_matches_acknowledged_ui(
            &unrolled,
            None,
            false,
            &unrolled,
            false,
            &unrolled,
        ));

        // Case 3: Real un-acknowledged edits on `current_state` fail closed.
        let edits: [&dyn Fn(&mut State); 4] = [
            &|s: &mut State| s.group_mut(games).unwrap().bounds.x += 24.,
            &|s: &mut State| s.group_mut(games).unwrap().active = 1,
            &|s: &mut State| s.group_mut(games).unwrap().rolled = false,
            &|s: &mut State| s.page = 1,
        ];
        for mutate in edits {
            let mut edited = accepted.clone();
            mutate(&mut edited);
            assert!(!native_drop_matches_acknowledged_ui(
                &accepted,
                Some(games),
                false,
                &edited,
                false,
                &proposal_before,
            ));
        }

        // Case 4: Mismatched `proposal.before` (without acknowledged reveal) fails closed.
        assert!(!native_drop_matches_acknowledged_ui(
            &accepted,
            Some(games),
            false,
            &accepted,
            false,
            &accepted,
        ));
    }

    #[cfg(windows)]
    #[test]
    fn rollup_toggle_under_hover_requires_worker_sync_for_rolled_overrides() {
        let mut expanded = State::empty();
        let games = expanded.add_group(
            "Games",
            Bounds {
                x: 80.,
                y: 80.,
                w: 320.,
                h: 260.,
            },
        );
        let mut rolled = expanded.clone();
        rolled.group_mut(games).unwrap().rolled = true;

        // Direction 1: Rolling up while hovered (`rollup_open == 0`) leaves the effective
        // layout unchanged (`rolled == false` in both), but changes `rolled_overrides`
        // from `[]` to `[(games, true)]` so worker sync MUST fire.
        let (eff_before_up, over_before_up) = native_effective_state(&expanded, None, false);
        let (eff_after_up, over_after_up) = native_effective_state(&rolled, Some(games), false);
        assert!(!native_desktop_requires_worker_sync(&eff_before_up, &eff_after_up));
        assert_ne!(over_before_up, over_after_up);
        assert!(native_effective_sync_needed(
            &expanded,
            None,
            false,
            &rolled,
            Some(games),
            false,
        ));

        // Direction 2: Unrolling a hover-revealed Box (`rolled: true -> false`,
        // `revealed: Some(games) -> None`) likewise leaves effective state identical while
        // clearing `rolled_overrides` `[(games, true)] -> []`.
        let (eff_before_down, over_before_down) = native_effective_state(&rolled, Some(games), false);
        let (eff_after_down, over_after_down) = native_effective_state(&expanded, None, false);
        assert!(!native_desktop_requires_worker_sync(&eff_before_down, &eff_after_down));
        assert_ne!(over_before_down, over_after_down);
        assert!(native_effective_sync_needed(
            &rolled,
            Some(games),
            false,
            &expanded,
            None,
            false,
        ));

        // Cosmetic pref edits without reveal/rollup override changes must NOT trigger worker sync.
        let mut cosmetic = rolled.clone();
        cosmetic.prefs.rollup_open = 1;
        assert!(!native_effective_sync_needed(
            &rolled,
            Some(games),
            false,
            &cosmetic,
            Some(games),
            false,
        ));
    }
