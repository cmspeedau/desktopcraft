    use super::*;
    fn fixture() -> (App, String, u64, u64) {
        static NEXT_FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let seq = NEXT_FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "desktopcraft-file-menu-test-{}-{}",
            std::process::id(),
            seq
        ));
        let path = root.join("Draft.zip");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, b"fixture").unwrap();
        let mut app = App::new(root, true, true);
        app.state = State::empty();
        let group = app.state.add_group(
            "Archive",
            Bounds {
                x: 20.,
                y: 20.,
                w: 270.,
                h: 260.,
            },
        );
        let iid = 809;
        let path_string = path.to_string_lossy().to_string();
        app.state
            .group_mut(group)
            .unwrap()
            .tab_mut()
            .items
            .push(Item {
                id: iid,
                name: "Draft.zip".into(),
                path: path_string.clone(),
                kind: "zip".into(),
                size: 7,
                modified: 0,
                label: None,
            });
        let h = app.host.as_mut().unwrap();
        h.loose.push(desktop::Loose {
            path: path_string.clone(),
            key: path_string.to_lowercase(),
            label: "Draft.zip".into(),
            pos: (100, 120),
        });
        (app, path_string, group, iid)
    }
    #[test]
    fn f2_loose_file_wins_over_selected_box_and_escape_does_not_rename() {
        let (mut app, path, group, _) = fixture();
        app.selected = Some((group, 0));
        app.host.as_mut().unwrap().selected_loose = Some(path.clone());
        app.shortcuts(&Input {
            keys: vec![KeyCode::F2],
            time: 1.,
            ..Default::default()
        });
        assert!(app.dialog.is_none(), "F2 must not rename a Box");
        assert!(matches!(&app.inline_rename,Some((0,0,p,_)) if p==&path));
        assert_eq!(
            app.inline_rename.as_ref().unwrap().3.selection,
            NameSelection::Stem
        );
        app.shortcuts(&Input {
            typed: "Budget".into(),
            time: 2.,
            ..Default::default()
        });
        assert_eq!(app.inline_rename.as_ref().unwrap().3.text, "Budget.zip");
        app.shortcuts(&Input {
            keys: vec![KeyCode::Escape],
            time: 3.,
            ..Default::default()
        });
        assert!(app.inline_rename.is_none());
        assert!(Path::new(&path).exists(), "Esc must not alter disk files");
    }
    #[test]
    fn keyboard_context_menu_and_enter_work_on_loose_desktop_icon() {
        let (mut app, path, group, _) = fixture();
        app.selected = Some((group, 0));
        app.host.as_mut().unwrap().selected_loose = Some(path.clone());
        app.host.as_mut().unwrap().virtual_rect = [-1920, -400, 3840, 1480];
        let hit = Hit {
            target: Target::Loose(0),
            bounds: Bounds { x: 100., y: 120., w: 80., h: 60. },
        };
        assert!(app.keyboard_file_menu(&Input {
            keys: vec![KeyCode::F10],
            shift: true,
            pos: vec2(500., 900.),
            time: 1.,
            ..Default::default()
        }, &[hit]));
        let menu = app.classic_file_menu.take().unwrap();
        assert_eq!(menu.path, path);
        assert_eq!(menu.anchor, Some((-1780, -250)));
        assert!(menu.pin.is_none());
        app.shortcuts(&Input {
            keys: vec![KeyCode::Enter],
            time: 3.,
            ..Default::default()
        });
        assert_eq!(app.opened.last(), Some(&path));
        assert!(app.dialog.is_none());
    }
    #[test]
    fn alt_enter_requests_windows_properties_for_pin_and_loose_icon() {
        let (mut app, path, group, item) = fixture();
        app.selected = Some((group, item));
        app.shortcuts(&Input {
            keys: vec![KeyCode::Enter],
            alt: true,
            time: 1.,
            ..Default::default()
        });
        assert_eq!(app.properties_requested.last(), Some(&path));
        assert!(app.opened.is_empty(), "Alt+Enter must not open the file");
        app.selected = None;
        app.properties_requested.clear();
        app.host.as_mut().unwrap().selected_loose = Some(path.clone());
        app.shortcuts(&Input {
            keys: vec![KeyCode::Enter],
            alt: true,
            time: 2.,
            ..Default::default()
        });
        assert_eq!(app.properties_requested.last(), Some(&path));
        assert!(app.opened.is_empty());
        assert!(app.dialog.is_none());
    }
    #[test]
    fn paste_into_virtual_box_is_not_a_disk_move_and_cut_is_rejected() {
        let (mut app, path, group, _) = fixture();
        let next = Path::new(&path).with_file_name("another.txt");
        std::fs::write(&next, b"safe").unwrap();
        let clip = shell::FileClipboard {
            paths: vec![next.clone()],
            cut: false,
        };
        app.paste_files(Some(group), clip.clone(), 1.);
        assert_eq!(app.state.group(group).unwrap().tab().items.len(), 2);
        assert!(next.exists(), "Pinning must not move the disk file");
        app.paste_files(
            Some(group),
            shell::FileClipboard {
                paths: vec![next.clone(), PathBuf::from("C:/does/not/exist")],
                cut: false,
            },
            2.,
        );
        assert_eq!(app.state.group(group).unwrap().tab().items.len(), 2);
        let two = Path::new(&path).with_file_name("not-moved.txt");
        std::fs::write(&two, b"still here").unwrap();
        app.paste_files(
            Some(group),
            shell::FileClipboard {
                paths: vec![two.clone()],
                cut: true,
            },
            3.,
        );
        assert_eq!(app.state.group(group).unwrap().tab().items.len(), 2);
        assert!(
            two.exists(),
            "Cut into virtual box must never move a real file"
        );
        assert!(app.toast.contains("Cut files cannot move"));
        assert!(
            app.undo.len() <= 1,
            "Errors should not append undo snapshots"
        );
    }
    #[test]
    fn right_click_box_pin_queues_classic_menu_on_release() {
        let (mut app, path, group, item) = fixture();
        let hits = [Hit {
            target: Target::Item(group, item),
            bounds: Bounds {
                x: 100.,
                y: 100.,
                w: 80.,
                h: 70.,
            },
        }];
        app.pointer(
            &Input {
                pos: vec2(130., 130.),
                right_pressed: true,
                time: 1.,
                ..Default::default()
            },
            &hits,
        );
        assert!(app.classic_file_menu.is_none());
        app.pointer(&Input {
            pos: vec2(130., 130.),
            right_released: true,
            ..Default::default()
        }, &hits);
        let menu = app.classic_file_menu.take().unwrap();
        assert_eq!(menu.path, path);
        assert_eq!(menu.pin, Some((group, app.state.group(group).unwrap().tab().id, item)));
        assert!(app.dialog.is_none());
        assert_eq!(app.selected, Some((group, item)));
        app.apply_classic_file_menu_choice(menu, shell::MenuChoice::Unpin, 1.);
        assert!(app.state.group(group).unwrap().tab().items.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), b"fixture");
    }

    #[test]
    fn explorer_box_body_leaves_mouse_and_keyboard_file_menu_to_explorer() {
        let (mut app, _, group, item) = fixture();
        app.selected = Some((group, item)); // stale model selection is not a visible file hit
        let bounds = app.state.group(group).unwrap().bounds;
        let pos = vec2(bounds.x + 65., bounds.y + 65.);
        let header = paint::box_input_region(bounds, true, true);
        assert!(bounds.contains(pos.x, pos.y));
        assert!(!header.contains(pos.x, pos.y));
        let hits = [Hit { target: Target::Tab(group, 0), bounds: header }];
        app.pointer(&Input { pos, right_pressed: true, right_released: true,
            ..Default::default() }, &hits);
        app.pointer(&Input { pos: vec2(header.x + 15., header.y + 15.), right_released: true,
            ..Default::default() }, &hits);
        assert!(!app.keyboard_file_menu(&Input { keys: vec![KeyCode::F10], shift: true,
            ..Default::default() }, &hits));
        assert!(app.classic_file_menu.is_none());
        assert!(app.host.as_ref().unwrap().pending.is_none());
        assert!(app.menu.is_none());
    }

    #[test]
    fn folder_portal_uses_classic_menu_without_unpin_for_mouse_and_keyboard() {
        let (mut app, path, group, item) = fixture();
        let g = app.state.group_mut(group).unwrap();
        g.tab_mut().folder = Some(Path::new(&path).parent().unwrap().to_string_lossy().into());
        let body = paint::box_input_region(g.bounds, false, true);
        let hit = Hit { target: Target::Item(group, item),
            bounds: Bounds { x: 100., y: 100., w: 80., h: 70. } };
        let pos = vec2(130., 130.);
        assert!(body.contains(pos.x, pos.y));
        app.pointer(&Input { pos, right_released: true, ..Default::default() },
            std::slice::from_ref(&hit));
        let mouse_menu = app.classic_file_menu.take().unwrap();
        assert_eq!(mouse_menu.path, path);
        assert!(mouse_menu.pin.is_none());
        assert!(app.keyboard_file_menu(&Input { keys: vec![KeyCode::F10], shift: true,
            ..Default::default() }, &[hit]));
        let key_menu = app.classic_file_menu.take().unwrap();
        assert_eq!(key_menu.path, path);
        assert!(key_menu.pin.is_none());
        app.apply_classic_file_menu_choice(key_menu, shell::MenuChoice::Unpin, 1.);
        assert_eq!(app.state.group(group).unwrap().tab().items.len(), 1);
        assert_eq!(std::fs::read(path).unwrap(), b"fixture");
    }
    #[test]
    fn file_menu_routes_rename_to_correct_box_pin_or_loose_icon() {
        let (mut app, path, group, item) = fixture();
        app.selected = Some((group, item));
        app.file_action(&path, FileAction::Rename, 1.);
        assert!(matches!(&app.inline_rename,Some((g,i,p,_)) if *g==group&&*i==item&&p==&path));
        assert!(app.dialog.is_none());
        app.inline_rename = None;
        app.selected = None;
        app.host.as_mut().unwrap().selected_loose = Some(path.clone());
        app.file_action(&path, FileAction::Rename, 2.);
        assert!(matches!(&app.inline_rename,Some((0,0,p,_)) if p==&path));
    }
    #[test]
    fn native_scrollbar_thumb_drag_queues_scroll_while_moving() {
        let (mut app, _, group, _) = fixture();
        app.native_pages = std::sync::Arc::from([(group, 0usize, 12usize, 3usize)]);
        let body = Bounds { x: 0., y: 0., w: 300., h: 412. };
        let (track, thumb) = paint::native_scrollbar(body, 0, 12, 3);
        let hit = Hit {
            bounds: thumb,
            target: Target::NativeScroll {
                group,
                track_y: track.y,
                track_h: track.h,
                thumb_y: thumb.y,
                thumb_h: thumb.h,
                positions: 12,
            },
        };
        let press_pos = vec2(thumb.x + 2., thumb.y + thumb.h / 2.);
        app.pointer(&Input { pos: press_pos, pressed: true, down: true, ..Default::default() }, &[hit]);
        assert!(matches!(app.drag, Some(Drag::NativeScroll { group: g, row: 0, .. }) if g == group));
        let bottom_pos = vec2(thumb.x + 2., track.y + track.h);
        app.pointer(&Input { pos: bottom_pos, down: true, ..Default::default() }, &[]);
        assert!(matches!(app.drag, Some(Drag::NativeScroll { group: g, row: 11, .. }) if g == group));
        assert_eq!(app.native_page_request, Some((group, native_scroll_limit() as i8)));
        app.pointer(&Input { pos: bottom_pos, released: true, ..Default::default() }, &[]);
        assert!(app.drag.is_none());
        assert_eq!(app.native_scroll_target, Some((group, 11)));
        assert_eq!(app.native_page_request, Some((group, native_scroll_limit() as i8)));
        // After the first bounded 8-row step ACKs, the remaining 3 rows queue automatically.
        app.native_page_request = None;
        app.native_pages = std::sync::Arc::from([(group, 8usize, 12usize, 3usize)]);
        app.advance_native_scroll_target();
        assert_eq!(app.native_page_request, Some((group, 3)));
        app.native_page_request = None;
        app.native_pages = std::sync::Arc::from([(group, 11usize, 12usize, 3usize)]);
        app.advance_native_scroll_target();
        assert_eq!(app.native_scroll_target, None);
        assert_eq!(app.native_page_request, None);
    }
