    use super::*;
    #[test]
    fn right_arrow_stops_at_last_page_and_plus_only_requests_confirmation() {
        let dir =
            std::env::temp_dir().join(format!("desktopcraft-pages-test-{}", std::process::id()));
        let mut app = App::new(dir, true, false);
        app.state = State::empty();
        assert_eq!(app.state.pages.len(), 1);
        for _ in 0..5 {
            app.switch_page(1);
        }
        assert_eq!(app.state.page, 0);
        assert_eq!(app.state.pages.len(), 1);
        app.request_new_page();
        assert!(matches!(app.dialog, Some(Dialog::NewPage { .. })));
        assert_eq!(app.state.pages.len(), 1); // request is not creation
        app.dialog = None; // user cancelled
        assert_eq!(app.state.pages.len(), 1);
        app.state.create_page("Work").unwrap(); // only confirmed action
        assert_eq!(app.state.page, 1);
        for _ in 0..5 {
            app.switch_page(1);
        }
        assert_eq!(app.state.page, 1);
        assert_eq!(app.state.pages.len(), 2);
        app.switch_page(-1);
        assert_eq!(app.state.page, 0);
    }

    #[test]
    fn settings_window_does_not_block_desktop_and_preserves_return_modal() {
        let dir = std::env::temp_dir().join(format!(
            "desktopcraft-settings-own-win-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let mut app = App::new(dir.clone(), false, true);
        app.state = State::empty();
        app.dialog = Some(Dialog::Settings {
            page: 6,
            pattern: String::new(),
            mode: 0,
            target: 0,
        });
        assert_eq!(app.settings_in_own_window(), cfg!(windows));
        assert_eq!(app.has_blocking_dialog(), !cfg!(windows));
        assert!(!app.host_lifted_for_popup());

        app.open_modal_preserving_settings(Dialog::Rename {
            group: 1,
            value: "Work".into(),
        });
        assert!(app.has_blocking_dialog());
        assert!(!app.settings_in_own_window());
        assert!(app.host_lifted_for_popup());
        assert!(matches!(
            app.settings_return,
            Some(Dialog::Settings { page: 6, .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
