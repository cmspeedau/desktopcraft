    use super::*;

    #[test]
    fn groups_resize_from_every_edge_and_keep_the_opposite_edge() {
        use macroquad::math::vec2;
        let before = Bounds {
            x: 100.,
            y: 100.,
            w: 300.,
            h: 200.,
        };
        let area = Bounds {
            x: 0.,
            y: 0.,
            w: 1000.,
            h: 800.,
        };
        let min = (100., 80.);
        // Left edge: moves x and width, right edge fixed.
        let r = resize_bounds(before, vec2(-40., 0.), 1, min, area);
        assert_eq!((r.x, r.w, r.x + r.w), (60., 340., 400.));
        // Bottom-left corner.
        let r = resize_bounds(before, vec2(50., 30.), 1 | 8, min, area);
        assert_eq!((r.x, r.y, r.w, r.h), (150., 100., 250., 230.));
        // Top edge can't shrink below the minimum or leave the area.
        let r = resize_bounds(before, vec2(0., 500.), 4, min, area);
        assert_eq!((r.y, r.h), (220., 80.));
        let r = resize_bounds(before, vec2(0., -500.), 4, min, area);
        assert_eq!((r.y, r.h), (0., 300.));
        // Lower-right corner works as before.
        let r = resize_bounds(before, vec2(20., 10.), 2 | 8, min, area);
        assert_eq!((r.w, r.h), (320., 210.));
    }

    #[test]
    fn page_manager_deletes_only_empty_pages_and_reindexes_groups() {
        let mut s = state();
        s.pages = vec!["A".into(), "B".into(), "C".into()];
        s.page = 2;
        let id = s.add_group(
            "Keep",
            Bounds {
                x: 10.,
                y: 10.,
                w: 120.,
                h: 120.,
            },
        );
        assert!(s.delete_empty_page(2).is_err());
        s.delete_empty_page(1).unwrap();
        assert_eq!(s.pages, ["A", "C"]);
        assert_eq!(s.page, 1);
        assert_eq!(s.group(id).unwrap().page, 1);
        assert!(s.delete_empty_page(1).is_err());
        s.delete_empty_page(0).unwrap();
        assert_eq!(s.page, 0);
        assert_eq!(s.group(id).unwrap().page, 0);
        assert!(s.delete_empty_page(0).is_err());
    }

    #[test]
    fn bind_existing_empty_box_to_folder_never_moves_files_or_discards_pins() {
        let root =
            std::env::temp_dir().join(format!("desktopcraft-bind-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let folder = root.join("work");
        std::fs::create_dir_all(&folder).unwrap();
        let original = root.join("note.txt");
        let in_folder = folder.join("inside.txt");
        std::fs::write(&original, "untouched").unwrap();
        std::fs::write(&in_folder, "folder untouched").unwrap();
        let mut s = State::empty();
        let id = s.add_group(
            "Empty box",
            Bounds {
                x: 10.,
                y: 10.,
                w: 320.,
                h: 240.,
            },
        );
        s.import(&original, id).unwrap();
        assert!(s.bind_group_folder(id, &folder).is_err());
        assert!(s.group(id).unwrap().tab().folder.is_none());
        assert_eq!(s.group(id).unwrap().tab().items.len(), 1);
        s.group_mut(id).unwrap().tab_mut().items.clear();
        s.bind_group_folder(id, &folder).unwrap();
        assert_eq!(
            s.group(id).unwrap().tab().folder.as_deref(),
            Some(folder.to_str().unwrap())
        );
        assert!(s
            .group(id)
            .unwrap()
            .tab()
            .items
            .iter()
            .any(|i| i.path == in_folder.to_string_lossy()));
        assert_eq!(std::fs::read_to_string(&original).unwrap(), "untouched");
        assert_eq!(
            std::fs::read_to_string(&in_folder).unwrap(),
            "folder untouched"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn page_rename_preserves_box_assignments_and_rejects_blank_names() {
        let mut s = State::empty();
        let first = s.add_group(
            "Test",
            Bounds {
                x: 0.,
                y: 0.,
                w: 250.,
                h: 190.,
            },
        );
        assert_eq!(s.create_page("Work").unwrap(), 1);
        let second = s.add_group(
            "Secondary",
            Bounds {
                x: 20.,
                y: 20.,
                w: 250.,
                h: 190.,
            },
        );
        assert!(s.rename_page(0, "Home").is_ok());
        assert!(s.rename_page(1, "งาน").is_ok());
        assert_eq!(s.pages, vec!["Home", "งาน"]);
        assert_eq!(s.group(first).unwrap().page, 0);
        assert_eq!(s.group(second).unwrap().page, 1);
        assert!(s.rename_page(1, " ").is_err());
        assert_eq!(s.pages[1], "งาน");
        assert!(s.rename_page(33, "Missing").is_err());
        assert!(s.create_page("").is_err());
        assert_eq!(s.pages.len(), 2);
    }
    #[test]
    fn add_tab_in_box_preserves_existing_contents_and_rule_tab_ids() {
        let mut state = State::empty();
        let b = Bounds {
            x: 10.,
            y: 20.,
            w: 250.,
            h: 240.,
        };
        let group = state.add_group("Documents", b);
        let original = state.group(group).unwrap().tab().id;
        let existing_file = Item {
            id: 1000,
            name: "report.pdf".into(),
            path: "C:/Desktop/report.pdf".into(),
            kind: "pdf".into(),
            size: 42,
            modified: 0,
            label: None,
        };
        state
            .group_mut(group)
            .unwrap()
            .tab_mut()
            .items
            .push(existing_file);
        state.rules.push(Rule {
            mode: 0,
            pattern: "pdf".into(),
            target: group,
            target_tab: Some(original),
            enabled: true,
        });
        let id = state.add_tab(group).unwrap();
        assert_ne!(id, original);
        let g = state.group(group).unwrap();
        assert_eq!(g.tabs.len(), 2);
        assert_eq!(g.active, 1);
        assert_eq!(g.tab().name, "Tab 2");
        assert!(g.tab().items.is_empty());
        assert!(g.tab().folder.is_none());
        assert_eq!(g.tabs[0].items[0].name, "report.pdf");
        assert_eq!(state.rules[0].target_tab, Some(original));
        assert_eq!(
            state.rule_destination(&state.rules[0]),
            Some((group, original))
        );
        let third = state.add_tab(group).unwrap();
        assert_ne!(third, id);
        assert_eq!(state.group(group).unwrap().tab().name, "Tab 3");
        state.group_mut(group).unwrap().tabs[1].name = "Custom".into();
        let fourth = state.add_tab(group).unwrap();
        assert_ne!(fourth, third);
        assert_eq!(state.group(group).unwrap().tab().name, "Tab 2");
        assert_eq!(state.groups.len(), 1);
        assert_eq!(state.pages.len(), 1);
        assert!(state.add_tab(999999).is_err());
    }
    #[test]
    fn clipboard_pins_are_virtual_atomic_and_keep_existing_pin_ids() {
        let root =
            std::env::temp_dir().join(format!("desktopcraft-virtual-paste-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a.txt");
        let b = root.join("ภาษาไทย.txt");
        std::fs::write(&a, b"hello").unwrap();
        std::fs::write(&b, b"world").unwrap();
        let mut state = State::empty();
        let bounds = Bounds {
            x: 20.,
            y: 20.,
            w: 300.,
            h: 260.,
        };
        let one = state.add_group("One", bounds);
        let two = state.add_group("Two", bounds);
        let first = state.pin_clipboard_paths(one, &[a.clone()]).unwrap();
        assert_eq!(first, 1);
        let preserved = state.group(one).unwrap().tab().items[0].id;
        assert_eq!(
            state
                .pin_clipboard_paths(two, &[a.clone(), b.clone(), b.clone()])
                .unwrap(),
            2
        );
        assert!(state.group(one).unwrap().tab().items.is_empty());
        assert_eq!(state.group(two).unwrap().tab().items.len(), 2);
        assert_eq!(state.group(two).unwrap().tab().items[0].id, preserved);
        assert!(a.exists() && b.exists(), "Virtual paste cannot move files");
        assert_eq!(state.pin_clipboard_paths(two, &[b.clone()]).unwrap(), 0);
        // One missing path rejects the whole batch; existing pins stay put.
        assert!(state
            .pin_clipboard_paths(one, &[a.clone(), root.join("gone.txt")])
            .is_err());
        assert_eq!(state.group(two).unwrap().tab().items.len(), 2);
        assert_eq!(state.group(one).unwrap().tab().items.len(), 0);
        let tab_id = state.add_tab(two).unwrap();
        state.group_mut(two).unwrap().tab_mut().folder = Some(root.to_string_lossy().to_string());
        assert!(state.pin_clipboard_paths(two, &[a.clone()]).is_err());
        assert_eq!(state.group(two).unwrap().tab().id, tab_id);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cached_sort_matches_previous_stable_sort_for_all_modes() {
        let items: Vec<Item> = (0..600u64)
            .map(|id| Item {
                id,
                name: format!(
                    "{}-{}-{:03}.txt",
                    ["กุญแจ", "Äpple", "zeta", "Alpha"][id as usize % 4],
                    id % 19,
                    599 - id
                ),
                path: format!("C:/fixtures/file{id}.txt"),
                kind: ["txt", "folder", "pdf", "jpg"][id as usize % 4].into(),
                size: id % 37,
                modified: id % 11,
                label: None,
            })
            .collect();
        for mode in 0..4 {
            let mut expected = items.clone();
            match mode {
                1 => expected.sort_by_key(|i| (i.kind.clone(), i.name.to_lowercase())),
                2 => expected.sort_by_key(|i| std::cmp::Reverse(i.modified)),
                3 => expected.sort_by_key(|i| std::cmp::Reverse(i.size)),
                _ => expected.sort_by_key(|i| (i.kind != "folder", i.name.to_lowercase())),
            }
            let mut tab = Tab::new(1, "fixture");
            tab.sort = mode;
            tab.items = items.clone();
            tab.sort_items();
            assert_eq!(
                tab.items.iter().map(|i| i.id).collect::<Vec<_>>(),
                expected.iter().map(|i| i.id).collect::<Vec<_>>(),
                "sort mode {mode}"
            );
        }
    }

    #[test]
    #[ignore = "Run explicitly for local sort-performance numbers"]
    fn bench_cached_sort_vs_original_comparator() {
        use std::hint::black_box;
        use std::time::Instant;
        let items: Vec<Item> = (0..6000u64)
            .rev()
            .map(|id| Item {
                id,
                name: format!(
                    "{}-{:07}-{}",
                    ["งาน", "Alpha", "Äpple", "File"][id as usize % 4],
                    id,
                    id % 17
                ),
                path: format!("C:/fixtures/{id}.lnk"),
                kind: ["lnk", "exe", "url"][id as usize % 3].into(),
                size: id * 10,
                modified: id % 51,
                label: None,
            })
            .collect();
        let mut baseline_us = 0u128;
        let mut optimized_us = 0u128;
        for _ in 0..5 {
            let mut old = items.clone();
            let start = Instant::now();
            old.sort_by_key(|i| (i.kind.clone(), i.name.to_lowercase()));
            baseline_us += start.elapsed().as_micros();
            let mut tab = Tab::new(1, "benchmark");
            tab.sort = 1;
            tab.items = items.clone();
            let start = Instant::now();
            tab.sort_items();
            optimized_us += start.elapsed().as_micros();
            assert_eq!(
                black_box(old.iter().map(|i| i.id).collect::<Vec<_>>()),
                black_box(tab.items.iter().map(|i| i.id).collect::<Vec<_>>())
            );
        }
        println!(
            "6000 icon sort / 5 passes: comparator={}us cached={}us speedup={:.2}x",
            baseline_us,
            optimized_us,
            baseline_us as f64 / optimized_us.max(1) as f64
        );
    }

    #[test]
    fn a_single_wheel_notch_advances_only_one_row() {
        assert_eq!(scroll_rows(0, 40, 9, 3, -1), 3);
        assert_eq!(scroll_rows(3, 40, 9, 3, -1), 6);
        assert_eq!(scroll_rows(0, 100, 8, 1, -1), 1);
        assert_eq!(scroll_rows(0, 40, 9, 3, -100), 33);
        assert_eq!(scroll_rows(33, 40, 9, 3, -1), 33);
        assert_eq!(scroll_rows(0, 3, 9, 3, -1), 0);
    }

    #[test]
    fn minimum_content_is_one_icon_cell_plus_two_pixels() {
        assert_eq!(min_content((75.0, 86.0)), (75.0, 88.0));
    }

    #[test]
    fn scroll_thumb_follows_page_ratio_with_an_eight_pixel_minimum() {
        assert_eq!(scroll_thumb(10, 5, 0, 200.0), (0.0, 100.0));
        assert_eq!(scroll_thumb(10, 5, 5, 200.0), (100.0, 100.0));
        assert_eq!(scroll_thumb(1000, 1, 0, 100.0).1, 8.0);
        assert_eq!(scroll_thumb(1000, 1, 999, 100.0), (92.0, 8.0));
    }

    fn state() -> State {
        State {
            version: 1,
            groups: vec![],
            pages: vec!["One".into(), "Two".into()],
            page_birth: vec![0, 1],
            next_page_birth: 2,
            page: 0,
            next_id: 1,
            rules: vec![],
            snapshots: vec![],
            blend: false,
            tint: false,
            snap: true,
            quick_hide: true,
            auto_rules: false,
            wallpaper: 0,
            layouts: Default::default(),
            placements: Default::default(),
            explorer_icons_hidden: false,
            prefs: Default::default(),
        }
    }

    #[test]
    fn sorting_pages_keeps_boxes_attached_and_newest_order_stable() {
        let mut state = State::empty();
        let area = Bounds {
            x: 10.,
            y: 20.,
            w: 220.,
            h: 180.,
        };
        let home = state.add_group("Home", area);
        state.create_page("Zed").unwrap();
        let zed = state.add_group("Zed items", area);
        state.create_page("Alpha").unwrap();
        let alpha = state.add_group("Alpha items", area);
        state.sort_pages(false);
        assert_eq!(state.pages, ["Alpha", "Desktop", "Zed"]);
        assert_eq!(state.page, 0); // Alpha remains active
        assert_eq!(state.group(home).unwrap().page, 1);
        assert_eq!(state.group(zed).unwrap().page, 2);
        assert_eq!(state.group(alpha).unwrap().page, 0);
        state.sort_pages(true);
        assert_eq!(state.pages, ["Alpha", "Zed", "Desktop"]);
        assert_eq!(state.group(home).unwrap().page, 2);
        assert_eq!(state.group(zed).unwrap().page, 1);
        assert_eq!(state.group(alpha).unwrap().page, 0);
        state.sort_pages(true);
        assert_eq!(state.pages, ["Alpha", "Zed", "Desktop"]);
        state.create_page("New arrival").unwrap();
        state.sort_pages(true);
        assert_eq!(state.pages[0], "New arrival");
        state.snapshot("Sort view");
        let json = serde_json::to_string(&state).unwrap();
        let loaded: State = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.page_birth, state.page_birth);
    }

    #[test]
    fn alignment_uses_inclusive_twenty_pixel_boundary_without_a_grid() {
        let mut s = state();
        let id = s.add_group(
            "Move",
            Bounds {
                x: 80.,
                y: 180.,
                w: 100.,
                h: 100.,
            },
        );
        let peer = s.add_group(
            "Peer",
            Bounds {
                x: 100.,
                y: 500.,
                w: 100.,
                h: 100.,
            },
        );
        let b = s.group(id).unwrap().bounds;
        assert_eq!(s.snap_position(id, b).x, 100.);
        assert_eq!(s.snap_position(id, Bounds { x: 79., ..b }).x, 79.);
        s.group_mut(peer).unwrap().page = 1;
        assert_eq!(s.snap_position(id, Bounds { x: 83., ..b }).x, 83.);
    }

    #[test]
    fn matches_existing_gap_when_third_box_joins_a_row() {
        let mut s = state();
        s.add_group(
            "First",
            Bounds {
                x: 100.,
                y: 100.,
                w: 100.,
                h: 100.,
            },
        );
        s.add_group(
            "Second",
            Bounds {
                x: 220.,
                y: 100.,
                w: 100.,
                h: 100.,
            },
        );
        let third = s.add_group(
            "Third",
            Bounds {
                x: 345.,
                y: 100.,
                w: 100.,
                h: 100.,
            },
        );
        assert_eq!(
            s.snap_position(third, s.group(third).unwrap().bounds).x,
            340.
        );
    }

    #[test]
    fn centers_a_box_between_neighbors_with_equal_clearance() {
        let mut s = state();
        s.add_group(
            "Left",
            Bounds {
                x: 100.,
                y: 100.,
                w: 100.,
                h: 100.,
            },
        );
        s.add_group(
            "Right",
            Bounds {
                x: 400.,
                y: 100.,
                w: 100.,
                h: 100.,
            },
        );
        let id = s.add_group(
            "Middle",
            Bounds {
                x: 256.,
                y: 100.,
                w: 100.,
                h: 100.,
            },
        );
        let snapped = s.snap_position(id, s.group(id).unwrap().bounds);
        assert_eq!(snapped.x, 250.);
        assert_eq!(snapped.x - 200., 400. - (snapped.x + 100.));
    }

    #[test]
    fn resize_snaps_only_dragged_edge_to_neighbor_and_shift_can_bypass() {
        use macroquad::math::vec2;
        let mut s = state();
        s.add_group(
            "Above",
            Bounds {
                x: 100.,
                y: 50.,
                w: 200.,
                h: 100.,
            },
        );
        let id = s.add_group(
            "Resizing",
            Bounds {
                x: 100.,
                y: 161.,
                w: 190.,
                h: 140.,
            },
        );
        let min = (90., 80.);
        let area = Bounds {
            x: 0.,
            y: 0.,
            w: 900.,
            h: 800.,
        };
        let before = s.group(id).unwrap().bounds;
        let proposed = resize_bounds(before, vec2(0., 0.), 2, min, area);
        let snapped = s.snap_resize(id, proposed, 2, min, area);
        assert_eq!(snapped.x, before.x);
        assert_eq!(snapped.w, 200.);
        assert_eq!(snapped.y, before.y);
        let peer = s.add_group(
            "Beside",
            Bounds {
                x: 300.,
                y: 101.,
                w: 120.,
                h: 200.,
            },
        );
        let proposed = resize_bounds(before, vec2(0., -10.), 8, min, area);
        let snapped = s.snap_resize(id, proposed, 8, min, area);
        assert_eq!(snapped.y, before.y);
        assert_eq!(
            snapped.y + snapped.h,
            s.group(peer).unwrap().bounds.y + 200.
        );
    }

    #[test]
    fn horizontal_resize_snaps_to_boxes_stacked_across_native_header_gap() {
        use macroquad::math::vec2;
        let mut s = state();
        s.add_group(
            "Top",
            Bounds {
                x: 100.,
                y: 50.,
                w: 320.,
                h: 180.,
            },
        );
        // 55px below `Top` (> 42px Native floating header height).
        let bottom_id = s.add_group(
            "Bottom",
            Bounds {
                x: 108.,
                y: 285.,
                w: 300.,
                h: 160.,
            },
        );
        let min = (90., 80.);
        let area = Bounds {
            x: 0.,
            y: 0.,
            w: 1200.,
            h: 900.,
        };
        let before = s.group(bottom_id).unwrap().bounds;
        let snap_left = s.snap_resize(bottom_id, before, 1, min, area);
        assert_eq!(snap_left.x, 100.);
        let wider = resize_bounds(before, vec2(4., 0.), 2, min, area);
        let snap_right = s.snap_resize(bottom_id, wider, 2, min, area);
        assert_eq!(snap_right.x + snap_right.w, 420.);
    }

    #[test]
    fn prefs_defaults_keep_legacy_workspaces_compatible() {
        let prefs: Prefs = serde_json::from_str("{}").unwrap();
        assert!((prefs.blend_opacity - 0.25).abs() < 1e-6);
        assert!(prefs.desktop_enabled);
        assert_eq!(prefs.peek_shortcut, "Ctrl+Shift+Tab");
        assert!(!prefs.peek_toggle);
        assert!(prefs.show_page_bar);
        assert_eq!(prefs.page_bar_offset_y, 0.0);
        assert!(prefs.drag_tab_to_rearrange);
        assert!(!prefs.lock_layout);
    }

    #[test]
    fn separated_neighbor_edges_snap_flush_without_forcing_unrelated_axis() {
        let mut s = state();
        s.add_group(
            "Left",
            Bounds {
                x: 100.,
                y: 160.,
                w: 100.,
                h: 100.,
            },
        );
        let id = s.add_group(
            "Right",
            Bounds {
                x: 215.,
                y: 185.,
                w: 100.,
                h: 100.,
            },
        );
        let proposed = s.group(id).unwrap().bounds;
        let snapped = s.snap_position(id, proposed);
        assert_eq!(snapped.x, 200.);
        assert_eq!(snapped.y, 185.); // beyond 20 px from top AND bottom align
    }

    #[test]
    fn overlapping_edges_separate_only_inside_forty_pixel_boundary() {
        let mut s = state();
        let id = s.add_group(
            "Move",
            Bounds {
                x: 139.,
                y: 160.,
                w: 100.,
                h: 100.,
            },
        );
        s.add_group(
            "Peer",
            Bounds {
                x: 200.,
                y: 150.,
                w: 100.,
                h: 100.,
            },
        );
        let b = s.group(id).unwrap().bounds;
        assert_eq!(s.snap_position(id, b).x, 100.);
        assert_eq!(s.snap_position(id, Bounds { x: 140., ..b }).x, 140.);
    }

    fn item() -> Item {
        Item {
            id: 7,
            name: "Brief.TXT".into(),
            path: "Brief.TXT".into(),
            kind: "txt".into(),
            size: 100,
            modified: 0,
            label: None,
        }
    }

    #[test]
    fn top_name_rule_beats_lower_type_rule() {
        // The person's case: 'Name contains craft' to Craft is at the top,
        // an Applications type rule below it.
        let mut state = State::empty();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 250.,
            h: 230.,
        };
        let craft_box = state.add_group("Craft", b);
        let apps = state.add_group("Applications", b);
        state.rules.clear();
        state.rules.push(Rule {
            mode: 1,
            pattern: "craft".into(),
            target: craft_box,
            target_tab: None,
            enabled: true,
        });
        state.rules.push(Rule {
            mode: 0,
            pattern: "exe,lnk,url".into(),
            target: apps,
            target_tab: None,
            enabled: true,
        });
        let shortcut = |id: u64, name: &str| Item {
            id,
            name: name.into(),
            path: format!("C:/Desktop/{name}"),
            kind: "lnk".into(),
            size: 10,
            modified: 0,
            label: None,
        };
        let craft_app = shortcut(60, "DesktopCraft.lnk");
        let wallcraft = shortcut(61, "WallCraft.exe");
        let other = shortcut(62, "Notepad.lnk");
        assert_eq!(
            state.best_rule_destination(&craft_app).unwrap().0,
            craft_box
        );
        assert_eq!(
            state.best_rule_destination(&wallcraft).unwrap().0,
            craft_box
        );
        assert_eq!(state.best_rule_destination(&other).unwrap().0, apps);
        // Pins already sitting in Applications are moved to Craft by Apply.
        state
            .group_mut(apps)
            .unwrap()
            .tab_mut()
            .items
            .extend([craft_app, wallcraft, other]);
        assert_eq!(state.apply_rules(), 2);
        assert_eq!(state.group(craft_box).unwrap().tab().items.len(), 2);
        assert_eq!(state.group(apps).unwrap().tab().items.len(), 1);
        assert_eq!(state.apply_rules(), 0);
        // Disabling the top rule lets the next matching rule take over.
        state.rules[0].enabled = false;
        assert_eq!(state.apply_rules(), 2);
        assert_eq!(state.group(apps).unwrap().tab().items.len(), 3);
    }

    /// Manual check against a saved layout copy (never the live file):
    /// DESKTOPCRAFT_RULES_PREVIEW=<copy>.json cargo test preview_rules -- --ignored --nocapture
    #[test]
    #[ignore]
    fn preview_rules_on_saved_workspace_copy() {
        let Some(path) = std::env::var_os("DESKTOPCRAFT_RULES_PREVIEW") else {
            return;
        };
        let state = State::load(Path::new(&path)).expect("readable workspace copy");
        let tab_name = |gid: u64, tid: u64| {
            state
                .group(gid)
                .and_then(|g| g.tabs.iter().find(|t| t.id == tid))
                .map_or("?".to_string(), |t| t.name.clone())
        };
        for g in &state.groups {
            for t in g.tabs.iter().filter(|t| t.folder.is_none()) {
                for item in &t.items {
                    if let Some((gid, tid)) = state.best_rule_destination(item) {
                        if (gid, tid) != (g.id, t.id) {
                            println!(
                                "MOVE {}  from {}  to {}",
                                item.name,
                                t.name,
                                tab_name(gid, tid)
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rule_order_alone_decides_and_missing_boxes_fall_through() {
        let mut state = State::empty();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 250.,
            h: 230.,
        };
        let apps = state.add_group("Applications", b);
        let archive = state.add_group("Archives", b);
        let folders = state.add_group("Folders", b);
        state.rules.clear();
        state.rules.push(Rule {
            mode: 1,
            pattern: "craft".into(),
            target: apps,
            target_tab: None,
            enabled: true,
        });
        state.rules.push(Rule {
            mode: 0,
            pattern: "zip,rar,7z".into(),
            target: archive,
            target_tab: None,
            enabled: true,
        });
        state.rules.push(Rule {
            mode: 10,
            pattern: "folder".into(),
            target: folders,
            target_tab: None,
            enabled: true,
        });
        let zip = Item {
            id: 55,
            name: "DesktopCraft.ZIP".into(),
            path: "C:/Desktop/DesktopCraft.ZIP".into(),
            kind: "zip".into(),
            size: 10,
            modified: 0,
            label: None,
        };
        let folder = Item {
            id: 56,
            name: "Craft project".into(),
            path: "C:/Desktop/Craft project".into(),
            kind: "folder".into(),
            size: 0,
            modified: 0,
            label: None,
        };
        let plain_zip = Item {
            id: 58,
            name: "Photos.zip".into(),
            path: "C:/Desktop/Photos.zip".into(),
            kind: "zip".into(),
            size: 10,
            modified: 0,
            label: None,
        };
        // The craft rule is on top, so it takes every craft-named item.
        assert_eq!(state.best_rule_destination(&zip).unwrap().0, apps);
        assert_eq!(state.best_rule_destination(&folder).unwrap().0, apps);
        assert_eq!(state.best_rule_destination(&plain_zip).unwrap().0, archive);
        // Raising the ZIP rule above it (Settings' Up button) changes the winner.
        state.rules.swap(0, 1);
        assert_eq!(state.best_rule_destination(&zip).unwrap().0, archive);
        assert_eq!(state.best_rule_destination(&folder).unwrap().0, apps);
        // A rule pointing at a deleted box is skipped, not a dead end.
        state.rules[0].target = 9999;
        assert_eq!(state.best_rule_destination(&zip).unwrap().0, apps);
    }

    #[test]
    fn multi_pin_transfer_is_ordered_atomic_and_never_touches_disk() {
        let mut state = State::empty();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 250.,
            h: 230.,
        };
        let src = state.add_group("A", b);
        let dst = state.add_group("B", b);
        let src_tab = state.group(src).unwrap().tab().id;
        let dst_tab = state.group(dst).unwrap().tab().id;
        for (id, name) in [
            (71, "Craft A.txt"),
            (72, "Craft B.zip"),
            (73, "Craft C.png"),
        ] {
            state.group_mut(src).unwrap().tab_mut().items.push(Item {
                id,
                name: name.into(),
                path: format!("C:/Desktop/{name}"),
                kind: "txt".into(),
                size: 1,
                modified: 0,
                label: None,
            });
        }
        let before = state.group(src).unwrap().tab().items.clone();
        assert!(state
            .transfer_many_between_tabs(src, src_tab, &[71, 99], dst, dst_tab)
            .is_err());
        assert_eq!(state.group(src).unwrap().tab().items.len(), 3);
        assert!(state.group(dst).unwrap().tab().items.is_empty());
        assert!(state
            .transfer_many_between_tabs(src, src_tab, &[71, 71], dst, dst_tab)
            .is_err());
        assert_eq!(
            state
                .transfer_many_between_tabs(src, src_tab, &[73, 71], dst, dst_tab)
                .unwrap(),
            2
        );
        assert_eq!(state.group(src).unwrap().tab().items[0].id, 72);
        assert_eq!(
            state
                .group(dst)
                .unwrap()
                .tab()
                .items
                .iter()
                .map(|i| i.id)
                .collect::<Vec<_>>(),
            vec![71, 73]
        );
        assert_eq!(
            state.group(dst).unwrap().tab().items[0].path,
            before[0].path
        );
        assert_eq!(
            state.group(dst).unwrap().tab().items[1].path,
            before[2].path
        );
        assert_eq!(
            state
                .transfer_many_between_tabs(src, src_tab, &[72], src, src_tab)
                .unwrap(),
            0
        );
        assert!(state
            .transfer_many_between_tabs(src, src_tab, &[72], dst, 999)
            .is_err());
        assert_eq!(state.group(src).unwrap().tab().items.len(), 1);
    }

    #[test]
    fn zip_and_folder_rules_match_their_real_file_kinds() {
        let zip = Item {
            id: 1,
            name: "Backup.ZIP".into(),
            path: "C:/Desktop/Backup.ZIP".into(),
            kind: "zip".into(),
            size: 100,
            modified: 0,
            label: None,
        };
        let folder = Item {
            id: 2,
            name: "Project".into(),
            path: "C:/Desktop/Project".into(),
            kind: "folder".into(),
            size: 0,
            modified: 0,
            label: None,
        };
        let mut rule = Rule {
            mode: 0,
            pattern: "zip,rar,7z".into(),
            target: 1,
            target_tab: None,
            enabled: true,
        };
        assert!(rule.matches(&zip));
        assert!(!rule.matches(&folder));
        rule.mode = 10;
        rule.pattern = "folder".into();
        assert!(rule.matches(&folder));
        assert!(!rule.matches(&zip));
        rule.mode = 0;
        assert!(rule.matches(&folder)); // legacy extension-list 'folder'
        rule.enabled = false;
        assert!(!rule.matches(&folder));
    }

    #[test]
    fn default_folder_and_archive_rules_are_migrated_once_without_overwriting_custom_rules() {
        let mut state = State::empty();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 250.,
            h: 220.,
        };
        let folders = state.add_group("Folders", b);
        let archives = state.add_group("Archives & installers", b);
        assert_eq!(state.ensure_builtin_sort_rules(), 2);
        assert_eq!(state.ensure_builtin_sort_rules(), 0);
        assert!(state
            .rules
            .iter()
            .any(|r| r.mode == 10 && r.target == folders));
        assert!(state.rules.iter().any(|r| r.target == archives
            && r.mode == 0
            && extension_matches("example.zip", &r.pattern)));
        let mut custom = State::empty();
        let folders = custom.add_group("Folders", b);
        let tab_id = custom.group(folders).unwrap().tab().id;
        custom.rules.push(Rule {
            mode: 10,
            pattern: "folder".into(),
            target: folders,
            target_tab: Some(tab_id),
            enabled: false,
        });
        assert_eq!(custom.ensure_builtin_sort_rules(), 0);
        assert_eq!(custom.rules.len(), 1);
        assert!(!custom.rules[0].enabled);
    }

    #[test]
    fn size_rules_keep_the_original_strict_upper_and_inclusive_lower_bounds() {
        let mut file = item();
        file.path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into();
        let mut r = Rule {
            mode: 6,
            pattern: "100".into(),
            target: 1,
            target_tab: None,
            enabled: true,
        };
        assert!(r.matches(&file));
        r.mode = 7;
        assert!(!r.matches(&file));
        r.pattern = "101".into();
        assert!(r.matches(&file));
        r.mode = 1;
        r.pattern = "BRIEF".into();
        assert!(r.matches(&item()));
    }

    #[test]
    fn name_suffix_ignores_only_the_final_extension() {
        let mut file = item();
        file.name = "Quarterly.Brief.TXT".into();
        let mut r = Rule {
            mode: 3,
            pattern: "BRIEF".into(),
            target: 1,
            target_tab: None,
            enabled: true,
        };
        assert!(r.matches(&file));
        r.pattern = "txt".into();
        assert!(!r.matches(&file));
        r.pattern = "quarterly.brief".into();
        assert!(r.matches(&file));
        file.name = "Brief".into();
        r.pattern = "brief".into();
        assert!(r.matches(&file));
        file.name = ".env".into();
        r.pattern = "env".into();
        assert!(!r.matches(&file));
    }

    #[test]
    fn name_and_extension_rules_fold_case_the_same_everywhere() {
        // Implementation decision: simple Unicode lowercase folding, independent
        // of the system or CRT locale (the original's runtime locale is unknown).
        let mut file = item();
        file.name = "MÉMO.TXT".into();
        let mut r = Rule {
            mode: 1,
            pattern: "mémo".into(),
            target: 1,
            target_tab: None,
            enabled: true,
        };
        assert!(r.matches(&file));
        r.mode = 2;
        assert!(r.matches(&file));
        r.mode = 5;
        assert!(!r.matches(&file));
        r.mode = 3;
        assert!(r.matches(&file));
        r.pattern = "MÉmo".into();
        assert!(r.matches(&file));
        file.name = "📁 ReadMe.TXT".into();
        r.pattern = "README".into();
        assert!(r.matches(&file));
        r.mode = 2;
        assert!(!r.matches(&file));
        r.mode = 1;
        assert!(r.matches(&file));
        r.pattern = "ReadMe.TXT.extra".into();
        r.mode = 3;
        assert!(!r.matches(&file));
    }

    #[test]
    fn thai_greek_cyrillic_and_ascii_names_and_extensions_match() {
        let ext = |name: &str, list: &str| {
            let mut f = item();
            f.name = name.into();
            Rule {
                mode: 0,
                pattern: list.into(),
                target: 1,
                target_tab: None,
                enabled: true,
            }
            .matches(&f)
        };
        let name = |name: &str, pattern: &str, mode: u8| {
            let mut f = item();
            f.name = name.into();
            Rule {
                mode,
                pattern: pattern.into(),
                target: 1,
                target_tab: None,
                enabled: true,
            }
            .matches(&f)
        };
        // ASCII extensions in any case, with or without the dot.
        assert!(ext("Report.PDF", "pdf"));
        assert!(ext("report.pdf", ".PDF, docx"));
        assert!(!ext("report.pdf.bak", "pdf"));
        assert!(!ext("README", "readme"));
        assert!(!ext(".env", "env"));
        // Thai has no case: exact matches only, extensions and names alike.
        assert!(ext("รายงานประจำเดือน.เอกสาร", "เอกสาร"));
        assert!(!ext("รายงาน.เอกสาร", "เอกสารใหม่"));
        assert!(ext("รายงาน.PDF", "pdf"));
        assert!(name("รายงานประจำเดือน.pdf", "ประจำ", 1));
        assert!(name("รายงานประจำเดือน.pdf", "รายงาน", 2));
        assert!(name("รายงานประจำเดือน.pdf", "เดือน", 3));
        assert!(!name("รายงานประจำเดือน.pdf", "pdf", 3));
        assert!(name("รายงาน.pdf", "ภาพ", 5));
        // Greek and Cyrillic fold case.
        assert!(ext("ΣΗΜΕΙΩΣΕΙΣ.ΚΕΙΜ", "κειμ"));
        assert!(name("ΣΗΜΕΙΩΣΕΙΣ.txt", "σημειωσεισ", 2));
        assert!(ext("ОТЧЁТ.ДОК", "док"));
        assert!(name("ОТЧЁТ.docx", "отчёт", 3));
        // German sharp s and accents stay distinct from their base letters.
        assert!(name("Straße.txt", "STRASSE", 1) == false);
        assert!(!name("resume.txt", "résumé", 1));
        // The same answers on every computer: folding never uses the locale.
        assert_eq!(fold("İSTANBUL"), "i̇stanbul");
        assert_eq!(fold("ΆΈΉ"), "άέή");
    }

    #[test]
    fn size_and_age_rules_require_an_existing_file() {
        let mut file = item();
        file.path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into();
        file.modified = now_seconds();
        for (mode, pattern) in [(6, "0"), (7, "101"), (8, "1"), (9, "100,100")] {
            let r = Rule {
                mode,
                pattern: pattern.into(),
                target: 1,
                target_tab: None,
                enabled: true,
            };
            assert!(r.matches(&file), "file should match mode {mode}");
            let mut folder = file.clone();
            folder.path = env!("CARGO_MANIFEST_DIR").into();
            folder.kind = "folder".into();
            assert!(!r.matches(&folder), "folder must not match mode {mode}");
            let mut missing = file.clone();
            missing.path = concat!(env!("CARGO_MANIFEST_DIR"), "/__missing-rule-fixture__").into();
            assert!(
                !r.matches(&missing),
                "missing file must not match mode {mode}"
            );
        }
    }

    #[test]
    fn size_between_includes_both_endpoints_and_rejects_invalid_ranges() {
        let mut file = item();
        file.path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into();
        let mut r = Rule {
            mode: 9,
            pattern: "100,200".into(),
            target: 1,
            target_tab: None,
            enabled: true,
        };
        for (size, expected) in [
            (99, false),
            (100, true),
            (150, true),
            (200, true),
            (201, false),
        ] {
            file.size = size;
            assert_eq!(r.matches(&file), expected, "size {size}");
        }
        for invalid in ["200,100", "100", "100,", ",200", "a,200", "1,2,3", "-1,200"] {
            r.pattern = invalid.into();
            assert!(!r.matches(&file), "invalid range {invalid}");
        }
        r.pattern = format!("0,{}", u64::MAX);
        file.size = u64::MAX;
        assert!(r.matches(&file));
        assert_eq!(size_range(" 100 , 200 "), Some((100, 200)));
    }

    #[test]
    fn file_age_uses_absolute_seconds_and_a_strict_limit() {
        assert!(modified_within(100_000 - 86399, 100_000, 1));
        assert!(!modified_within(100_000 - 86400, 100_000, 1));
        assert!(modified_within(100_000 + 86399, 100_000, 1));
        assert!(!modified_within(100_000 + 86400, 100_000, 1));
        assert!(!modified_within(100_000, 100_000, 0));
    }

    #[test]
    fn rule_slot_fifty_is_used_but_fifty_one_is_not() {
        let mut s = state();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 100.,
            h: 100.,
        };
        let from = s.add_group("From", b);
        let to = s.add_group("To", b);
        s.group_mut(from).unwrap().tab_mut().items.push(item());
        let r = Rule {
            mode: 0,
            pattern: "txt".into(),
            target: to,
            target_tab: None,
            enabled: false,
        };
        s.rules = vec![r; 51];
        s.rules[50].enabled = true;
        assert_eq!(s.apply_rules(), 0);
        s.rules[49].enabled = true;
        assert_eq!(s.apply_rules(), 1);
    }

    #[test]
    fn rules_can_target_a_non_active_tab_inside_a_combined_box() {
        let mut state = state();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 250.,
            h: 250.,
        };
        let a = state.add_group("Documents", b);
        let b_id = state.add_group("Pictures", b);
        let source_tab = state.group(a).unwrap().tab().id;
        let destination_tab = state.group(b_id).unwrap().tab().id;
        state.group_mut(a).unwrap().tab_mut().items.push(item());
        assert!(state.merge(b_id, a));
        state.group_mut(a).unwrap().active = 0;
        state.rules.push(Rule {
            mode: 0,
            pattern: "txt".into(),
            target: a,
            target_tab: Some(destination_tab),
            enabled: true,
        });
        assert_eq!(state.apply_rules(), 1);
        let g = state.group(a).unwrap();
        assert!(g
            .tabs
            .iter()
            .find(|t| t.id == source_tab)
            .unwrap()
            .items
            .is_empty());
        assert_eq!(
            g.tabs
                .iter()
                .find(|t| t.id == destination_tab)
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(state.apply_rules(), 0);
    }

    #[test]
    fn dragging_a_pin_between_tabs_of_the_same_box_does_not_move_disk_files() {
        let mut state = state();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 250.,
            h: 250.,
        };
        let a = state.add_group("One", b);
        let other = state.add_group("Two", b);
        let source_tab = state.group(a).unwrap().tab().id;
        let dest_tab = state.group(other).unwrap().tab().id;
        state.group_mut(a).unwrap().tab_mut().items.push(item());
        assert!(state.merge(other, a));
        state
            .transfer_between_tabs(a, source_tab, 7, a, dest_tab, 0)
            .unwrap();
        let g = state.group(a).unwrap();
        assert!(g
            .tabs
            .iter()
            .find(|t| t.id == source_tab)
            .unwrap()
            .items
            .is_empty());
        assert_eq!(
            g.tabs.iter().find(|t| t.id == dest_tab).unwrap().items[0].path,
            "Brief.TXT"
        );
    }

    #[test]
    fn destination_tab_survives_merge_detach_and_old_rule_json() {
        let mut state = state();
        let b = Bounds {
            x: 0.,
            y: 0.,
            w: 250.,
            h: 250.,
        };
        let old = state.add_group("Old", b);
        let new = state.add_group("New", b);
        let tab_id = state.group(old).unwrap().tab().id;
        state.rules.push(Rule {
            mode: 0,
            pattern: "txt".into(),
            target: old,
            target_tab: Some(tab_id),
            enabled: true,
        });
        assert!(state.merge(old, new));
        assert_eq!(state.rules[0].target, new);
        assert_eq!(state.rule_destination(&state.rules[0]), Some((new, tab_id)));
        let idx = state
            .group(new)
            .unwrap()
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .unwrap();
        let separate = state.detach(new, idx, b).unwrap();
        assert_eq!(
            state.rule_destination(&state.rules[0]),
            Some((separate, tab_id))
        );
        let legacy: Rule = serde_json::from_str(&format!(
            r#"{{"mode":0,"pattern":"txt","target":{},"enabled":true}}"#,
            separate
        ))
        .unwrap();
        assert_eq!(legacy.target_tab, None);
        assert_eq!(state.rule_destination(&legacy), Some((separate, tab_id)));
    }
