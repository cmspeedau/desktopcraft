    use super::*;

    #[test]
    fn rule_sort_atomically_assigns_loose_and_misfiled_explorer_icons() {
        use crate::model::{Bounds, Item, Rule};
        let marker = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("desktopcraft-rule-sort-{marker}"));
        std::fs::create_dir(&root).unwrap();
        let mut before = State::empty();
        let bounds = Bounds { x: 100., y: 100., w: 300., h: 300. };
        let source = before.add_group("Source", bounds);
        let documents = before.add_group("Documents", Bounds { x: 500., ..bounds });
        let downloads = before.add_group("Downloads", Bounds { x: 900., ..bounds });
        before.rules = vec![Rule { mode: 0, pattern: "txt".into(), target: documents,
            target_tab: None, enabled: true }, Rule { mode: 0, pattern: "zip".into(),
            target: downloads, target_tab: None, enabled: true }];

        let mut desktop = Vec::new();
        for index in 0..10 {
            let name = format!("fixture-{index}.{}", if index % 2 == 0 { "txt" } else { "zip" });
            let path = root.join(&name);
            std::fs::write(&path, b"unchanged source").unwrap();
            if index == 0 {
                let id = before.id();
                let pin = Item::from_path(&path, id).unwrap();
                before.group_mut(source).unwrap().tab_mut().items.push(pin);
            }
            desktop.push(ViewItem { label: name, path: path.to_string_lossy().into_owned(), pos: (index * 75, 0) });
        }
        let stale = Item { id: before.id(), name: "Missing.txt".into(),
            path: root.join("Missing.txt").to_string_lossy().into_owned(),
            kind: "txt".into(), size: 0, modified: 0, label: None };
        before.group_mut(source).unwrap().tab_mut().items.push(stale.clone());
        let (after, paths) = plan_native_rules_sort(&before, &desktop, &before.rules, before.next_id, true);
        assert_eq!(paths.len(), 10);
        assert_eq!(after.group(source).unwrap().tab().items, vec![stale]);
        assert_eq!(after.group(documents).unwrap().tab().items.len(), 5);
        assert_eq!(after.group(downloads).unwrap().tab().items.len(), 5);
        assert_eq!(crate::native_ole_persist::pins_change_onto(&before, &after, &paths).unwrap(), after);
        let workspace = root.join("workspace.json");
        before.save(&workspace).unwrap();
        assert!(crate::native_ole_persist::commit_verified_native_pin_drops_over(
            &workspace, &before, &before, &after, &paths).is_err());
        crate::native_ole_persist::commit_verified_native_rules_sort_over(
            &workspace, &before, &before, &after, &paths).unwrap();
        assert_eq!(State::load(&workspace).unwrap(), after);
        for item in desktop {
            assert_eq!(std::fs::read(item.path).unwrap(), b"unchanged source");
        }
    }

    #[test]
    fn loose_only_rule_sort_leaves_existing_box_pins_untouched() {
        use crate::model::{Bounds, Item, Rule};
        let marker = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("desktopcraft-loose-rule-sort-{marker}"));
        std::fs::create_dir(&root).unwrap();
        let mut before = State::empty();
        let bounds = Bounds { x: 100., y: 100., w: 300., h: 300. };
        let source = before.add_group("Source", bounds);
        let documents = before.add_group("Documents", Bounds { x: 500., ..bounds });
        let downloads = before.add_group("Downloads", Bounds { x: 900., ..bounds });
        before.rules = vec![
            Rule { mode: 0, pattern: "txt".into(), target: documents, target_tab: None, enabled: true },
            Rule { mode: 0, pattern: "zip".into(), target: downloads, target_tab: None, enabled: true },
        ];

        let mut desktop = Vec::new();
        let mut pinned_first = None;
        for index in 0..10 {
            let name = format!("fixture-{index}.{}", if index % 2 == 0 { "txt" } else { "zip" });
            let path = root.join(&name);
            std::fs::write(&path, b"unchanged source").unwrap();
            if index == 0 {
                let id = before.id();
                let pin = Item::from_path(&path, id).unwrap();
                before.group_mut(source).unwrap().tab_mut().items.push(pin.clone());
                pinned_first = Some(pin);
            }
            desktop.push(ViewItem { label: name, path: path.to_string_lossy().into_owned(), pos: (index * 75, 0) });
        }
        let stale = Item {
            id: before.id(),
            name: "Missing.txt".into(),
            path: root.join("Missing.txt").to_string_lossy().into_owned(),
            kind: "txt".into(),
            size: 0,
            modified: 0,
            label: None,
        };
        before.group_mut(source).unwrap().tab_mut().items.push(stale.clone());

        let (after, paths) = plan_native_rules_sort(&before, &desktop, &before.rules, before.next_id, false);
        assert_eq!(paths.len(), 9);
        assert_eq!(after.group(source).unwrap().tab().items, vec![pinned_first.unwrap(), stale]);
        assert_eq!(after.group(documents).unwrap().tab().items.len(), 4);
        assert_eq!(after.group(downloads).unwrap().tab().items.len(), 5);
        assert_eq!(crate::native_ole_persist::pins_change_onto(&before, &after, &paths).unwrap(), after);
        let workspace = root.join("workspace.json");
        before.save(&workspace).unwrap();
        crate::native_ole_persist::commit_verified_native_rules_sort_over(
            &workspace, &before, &before, &after, &paths,
        ).unwrap();
        assert_eq!(State::load(&workspace).unwrap(), after);
        for item in &desktop {
            assert_eq!(std::fs::read(&item.path).unwrap(), b"unchanged source");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn layout_restore_atomically_restores_pins_unpins_new_items_skips_missing_and_rejects_mismatch() {
        use crate::model::{Bounds, Item};
        let marker = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("desktopcraft-layout-restore-{marker}"));
        std::fs::create_dir(&root).unwrap();

        let make_file = |name: &str| {
            let p = root.join(name);
            std::fs::write(&p, b"keep-bytes").unwrap();
            p
        };
        let a = make_file("GameA.lnk");
        let b = make_file("GameB.url");
        let c = make_file("ProgC.lnk");
        let d = make_file("CraftD.lnk");
        let e_loose = make_file("ProgE.lnk");
        let extra = make_file("ExtraNew.pdf");
        let missing = root.join("DeletedApp.exe");

        let desktop = vec![
            ViewItem { label: "GameA".into(), path: a.to_string_lossy().into_owned(), pos: (0, 0) },
            ViewItem { label: "GameB".into(), path: b.to_string_lossy().into_owned(), pos: (75, 0) },
            ViewItem { label: "ProgC".into(), path: c.to_string_lossy().into_owned(), pos: (150, 0) },
            ViewItem { label: "CraftD".into(), path: d.to_string_lossy().into_owned(), pos: (225, 0) },
            ViewItem { label: "ProgE".into(), path: e_loose.to_string_lossy().into_owned(), pos: (300, 0) },
            ViewItem { label: "ExtraNew".into(), path: extra.to_string_lossy().into_owned(), pos: (375, 0) },
        ];

        // Build Snapshot "Layout 1": Games=[A, B, Missing], Programs=[C, E], Craft=[D]
        let mut target = State::empty();
        let games = target.add_group("Games", Bounds { x: 40., y: 40., w: 300., h: 300. });
        let programs = target.add_group("Programs", Bounds { x: 400., y: 40., w: 340., h: 300. });
        target.group_mut(programs).unwrap().tabs[0].name = "Programs".into();
        let _craft_tab_id = target.add_tab(programs).unwrap();
        let craft_idx = 1usize;
        target.group_mut(programs).unwrap().tabs[craft_idx].name = "Craft".into();

        let pin_a = Item::from_path(&a, target.id()).unwrap();
        let pin_b = Item::from_path(&b, target.id()).unwrap();
        let pin_c = Item::from_path(&c, target.id()).unwrap();
        let pin_d = Item::from_path(&d, target.id()).unwrap();
        let pin_e_snap = Item::from_path(&e_loose, target.id()).unwrap();
        let pin_missing = Item {
            id: target.id(),
            name: "DeletedApp.exe".into(),
            path: missing.to_string_lossy().into_owned(),
            kind: "exe".into(),
            size: 12,
            modified: 1,
            label: None,
        };
        target.group_mut(games).unwrap().tabs[0].items = vec![pin_a.clone(), pin_b.clone(), pin_missing];
        target.group_mut(programs).unwrap().tabs[0].items = vec![pin_c.clone(), pin_e_snap];
        target.group_mut(programs).unwrap().tabs[craft_idx].items = vec![pin_d.clone()];
        target.group_mut(games).unwrap().rolled = true;

        // Build degraded current workspace after buggy Sort: Games=[], Programs=[],
        // Craft=[D, A, B, C, ExtraNew] while E became loose on desktop.
        let mut before = target.clone();
        let pin_extra = Item::from_path(&extra, before.id()).unwrap();
        before.group_mut(games).unwrap().rolled = false;
        before.group_mut(games).unwrap().tabs[0].items.clear();
        before.group_mut(programs).unwrap().tabs[0].items.clear();
        before.group_mut(programs).unwrap().tabs[craft_idx].items = vec![
            pin_d.clone(), pin_a.clone(), pin_b.clone(), pin_c.clone(), pin_extra,
        ];

        let (after, paths) = plan_native_layout_restore(&before, &desktop, &target, before.next_id).unwrap();
        // Existing items preserve original Item identities & rolled flag stays `before`'s until phase 2.
        assert!(!after.group(games).unwrap().rolled);
        assert_eq!(after.group(games).unwrap().tabs[0].items, vec![pin_a.clone(), pin_b.clone()]);
        let prog_items = &after.group(programs).unwrap().tabs[0].items;
        assert_eq!(prog_items.len(), 2);
        assert_eq!(prog_items[0], pin_c);
        assert_eq!(prog_items[1].path, e_loose.to_string_lossy().as_ref());
        assert_eq!(prog_items[1].id, before.next_id);
        assert_eq!(after.next_id, before.next_id + 1);
        assert_eq!(after.group(programs).unwrap().tabs[craft_idx].items, vec![pin_d]);
        assert!(paths.iter().any(|p| p.eq_ignore_ascii_case(&extra.to_string_lossy())));
        assert!(!paths.iter().any(|p| p.eq_ignore_ascii_case(&missing.to_string_lossy())));

        let workspace = root.join("workspace.json");
        let mut raw = serde_json::to_value(&before).unwrap();
        raw["future_restore_guard"] = serde_json::json!({ "keep": true });
        std::fs::write(&workspace, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();

        // Drag and SortRules modes MUST reject mixed unpin + cross-box restore.
        assert!(crate::native_ole_persist::commit_verified_native_pin_drops_over(
            &workspace, &before, &before, &after, &paths,
        ).is_err());
        assert!(crate::native_ole_persist::commit_verified_native_rules_sort_over(
            &workspace, &before, &before, &after, &paths,
        ).is_err());

        crate::native_ole_persist::commit_verified_native_layout_restore_over(
            &workspace, &before, &before, &after, &paths,
        ).unwrap();
        assert_eq!(State::load(&workspace).unwrap(), after);
        let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&workspace).unwrap()).unwrap();
        assert_eq!(written["future_restore_guard"], raw["future_restore_guard"]);
        for item in &desktop {
            assert_eq!(std::fs::read(&item.path).unwrap(), b"keep-bytes");
        }

        // Structural mismatches must fail closed.
        let mut wrong_pages = target.clone();
        wrong_pages.pages.push("Page 2".into());
        assert!(plan_native_layout_restore(&before, &desktop, &wrong_pages, before.next_id).is_err());
        let mut wrong_groups = target.clone();
        wrong_groups.groups.pop();
        assert!(plan_native_layout_restore(&before, &desktop, &wrong_groups, before.next_id).is_err());
        let mut wrong_tab_id = target.clone();
        wrong_tab_id.groups[0].tabs[0].id += 100;
        assert!(plan_native_layout_restore(&before, &desktop, &wrong_tab_id, before.next_id).is_err());
        let mut wrong_portal = target.clone();
        wrong_portal.groups[0].tabs[0].folder = Some("C:/Folder".into());
        assert!(plan_native_layout_restore(&before, &desktop, &wrong_portal, before.next_id).is_err());
        let mut bad_bounds = target.clone();
        bad_bounds.groups[0].bounds.w = f32::NAN;
        assert!(plan_native_layout_restore(&before, &desktop, &bad_bounds, before.next_id).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn native_workspace_can_move_and_resize_without_mutating_membership() {
        let mut previous = State::empty();
        previous.add_group("Box", crate::model::Bounds { x:100., y:100., w:300., h:300. });
        let mut next = previous.clone();
        next.groups[0].bounds.x += 150.;
        next.groups[0].bounds.h += 60.;
        assert!(validate_native_workspace_edit(&previous, &next).is_ok());
        next.groups[0].bounds.x = f32::NAN;
        assert!(validate_native_workspace_edit(&previous, &next).is_err());
    }

    #[test]
    fn native_workspace_allows_empty_group_and_tab_add_remove_and_rejects_pin_or_portal_bypass() {
        use crate::model::{Bounds, Item};
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
        let g1 = base.add_group("Work", Bounds { x: 100., y: 100., w: 320., h: 240. });
        base.group_mut(g1).unwrap().tab_mut().items.push(pin(100, "C:/Desktop/A.lnk"));
        base.group_mut(g1).unwrap().tab_mut().items.push(pin(101, "C:/Desktop/Z.lnk"));

        // Creating an empty regular group succeeds, and removing/undoing it succeeds
        // (including multi-tab empty groups for symmetric Ctrl+Z Undo).
        let mut with_new_group = base.clone();
        let g2 = with_new_group.add_group("New group", Bounds { x: 460., y: 100., w: 330., h: 260. });
        assert!(validate_native_workspace_edit(&base, &with_new_group).is_ok());
        assert!(validate_native_workspace_edit(&with_new_group, &base).is_ok());

        let mut with_two_tab_empty_group = with_new_group.clone();
        with_two_tab_empty_group.add_tab(g2).unwrap();
        assert!(validate_native_workspace_edit(&with_two_tab_empty_group, &base).is_ok());
        assert!(validate_native_workspace_edit(&base, &with_two_tab_empty_group).is_ok());

        // Adding/removing a Portal group or tab succeeds while preserving regular pin conservation.
        let mut portal_new_group = with_new_group.clone();
        portal_new_group.group_mut(g2).unwrap().tab_mut().folder = Some("C:/Folder".into());
        assert!(validate_native_workspace_edit(&base, &portal_new_group).is_ok());
        assert!(validate_native_workspace_edit(&portal_new_group, &base).is_ok());

        // Adding an empty regular tab succeeds, and removing/undoing it succeeds.
        let mut with_new_tab = base.clone();
        let t2 = with_new_tab.add_tab(g1).unwrap();
        assert_eq!(with_new_tab.group(g1).unwrap().tabs[1].id, t2);
        assert!(validate_native_workspace_edit(&base, &with_new_tab).is_ok());
        assert!(validate_native_workspace_edit(&with_new_tab, &base).is_ok());

        // Sorting pins within a tab, adding/renaming pages, and moving tabs across groups succeed.
        let mut sorted = base.clone();
        sorted.group_mut(g1).unwrap().tab_mut().items.reverse();
        assert!(validate_native_workspace_edit(&base, &sorted).is_ok());

        let mut with_page = base.clone();
        with_page.pages.push("Space 2".into());
        with_page.page = 1;
        with_page.group_mut(g1).unwrap().page = 1;
        assert!(validate_native_workspace_edit(&base, &with_page).is_ok());

        let moved_tab_id = with_new_tab.group(g1).unwrap().tabs[1].id;
        let mut moved_to_g2 = with_new_tab.clone();
        let moved_tab = moved_to_g2.group_mut(g1).unwrap().tabs.pop().unwrap();
        moved_to_g2.group_mut(g1).unwrap().active = 0;
        let g2_added = moved_to_g2.add_group("Target", Bounds { x: 460., y: 100., w: 330., h: 260. });
        moved_to_g2.group_mut(g2_added).unwrap().tabs[0] = moved_tab;
        assert_eq!(moved_to_g2.group(g2_added).unwrap().tabs[0].id, moved_tab_id);
        assert!(validate_native_workspace_edit(&with_new_tab, &moved_to_g2).is_ok());

        // Reject adding or deleting non-empty regular tabs/groups without a pin transaction.
        let mut pinned_new_group = with_new_group.clone();
        pinned_new_group.group_mut(g2).unwrap().tab_mut().items.push(pin(200, "C:/Desktop/B.lnk"));
        assert!(validate_native_workspace_edit(&base, &pinned_new_group).is_err());
        assert!(validate_native_workspace_edit(&pinned_new_group, &base).is_err());

        let mut pinned_new_tab = with_new_tab.clone();
        pinned_new_tab.group_mut(g1).unwrap().tabs[1].items.push(pin(201, "C:/Desktop/C.lnk"));
        assert!(validate_native_workspace_edit(&base, &pinned_new_tab).is_err());
        assert!(validate_native_workspace_edit(&pinned_new_tab, &base).is_err());

        // Reject removing the last regular tab (even if empty or converted to a Portal).
        let mut only_portal_next = base.clone();
        only_portal_next.groups.clear();
        assert!(validate_native_workspace_edit(&base, &only_portal_next).is_err());

        let mut empty_base = State::empty();
        let empty_g = empty_base.add_group("Only", Bounds { x: 100., y: 100., w: 300., h: 240. });
        let mut converted_last = empty_base.clone();
        converted_last.group_mut(empty_g).unwrap().tab_mut().folder = Some("C:/Folder".into());
        assert!(validate_native_workspace_edit(&empty_base, &converted_last).is_err());

        // Reject duplicate group/tab IDs, empty pages, and direct pin mutation.
        let mut dup_group = with_new_group.clone();
        dup_group.group_mut(g2).unwrap().id = g1;
        assert!(validate_native_workspace_edit(&base, &dup_group).is_err());

        let mut dup_tab = with_new_tab.clone();
        dup_tab.group_mut(g1).unwrap().tabs[1].id = dup_tab.group(g1).unwrap().tabs[0].id;
        assert!(validate_native_workspace_edit(&base, &dup_tab).is_err());

        let mut empty_pages = base.clone();
        empty_pages.pages.clear();
        assert!(validate_native_workspace_edit(&base, &empty_pages).is_err());

        let mut swapped_pin = base.clone();
        swapped_pin.group_mut(g1).unwrap().tab_mut().items[0].path = "C:/Desktop/Other.lnk".into();
        assert!(validate_native_workspace_edit(&base, &swapped_pin).is_err());
    }

    #[test]
    fn plan_native_pin_paths_handles_loose_existing_duplicate_and_non_desktop_paths() {
        use crate::model::{Bounds, Item};
        let root = std::env::temp_dir().join(format!(
            "dc-plan-pin-paths-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let desktop_dir = root.join("Desktop");
        let other_dir = root.join("Other");
        std::fs::create_dir_all(&desktop_dir).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();

        let loose = desktop_dir.join("Loose.txt");
        let moved = desktop_dir.join("Moved.txt");
        let already = desktop_dir.join("Already.txt");
        let non_desktop = other_dir.join("Elsewhere.txt");
        std::fs::write(&loose, b"loose").unwrap();
        std::fs::write(&moved, b"moved").unwrap();
        std::fs::write(&already, b"already").unwrap();
        std::fs::write(&non_desktop, b"outside").unwrap();

        let mut state = State::empty();
        let g1 = state.add_group("Work", Bounds { x: 80., y: 80., w: 300., h: 240. });
        let g2 = state.add_group("Inbox", Bounds { x: 420., y: 80., w: 300., h: 240. });
        let t1 = state.group(g1).unwrap().tabs[0].id;
        let t2 = state.group(g2).unwrap().tabs[0].id;
        state.next_id = 500;
        state.group_mut(g1).unwrap().tabs[0].items.push(Item {
            id: 41,
            name: "Already.txt".into(),
            path: already.to_string_lossy().into(),
            kind: "txt".into(),
            size: 7,
            modified: 10,
            label: Some("KeepLabel".into()),
        });
        state.group_mut(g2).unwrap().tabs[0].items.push(Item {
            id: 77,
            name: "Moved.txt".into(),
            path: moved.to_string_lossy().into(),
            kind: "txt".into(),
            size: 5,
            modified: 20,
            label: Some("CustomLabel".into()),
        });

        let view_items = vec![
            crate::shell::ViewItem {
                path: loose.to_string_lossy().into(),
                label: "Loose".into(),
                pos: (100, 100),
            },
            crate::shell::ViewItem {
                path: moved.to_string_lossy().into(),
                label: "Moved".into(),
                pos: (200, 100),
            },
            crate::shell::ViewItem {
                path: already.to_string_lossy().into(),
                label: "Already".into(),
                pos: (300, 100),
            },
        ];

        // Pinning Loose + Moved (preserving ID=77 & label) while skipping Already in dest.
        let req = vec![
            loose.to_string_lossy().into_owned(),
            moved.to_string_lossy().into_owned(),
            already.to_string_lossy().into_owned(),
        ];
        let (next, changed) = plan_native_pin_paths(&state, &view_items, g1, t1, &req, state.next_id).unwrap();
        assert_eq!(changed.len(), 2);
        assert_eq!(next.next_id, 501);
        assert!(next.group(g2).unwrap().tabs[0].items.is_empty());
        let g1_items = &next.group(g1).unwrap().tabs[0].items;
        assert_eq!(g1_items.len(), 3);
        assert_eq!(g1_items[0].id, 41);
        assert_eq!(g1_items[1].id, 500);
        assert_eq!(g1_items[2].id, 77);
        assert_eq!(g1_items[2].label.as_deref(), Some("CustomLabel"));

        // Pinning a non-Desktop path fails cleanly with helpful Folder View guidance.
        let err = plan_native_pin_paths(
            &state,
            &view_items,
            g1,
            t1,
            &[non_desktop.to_string_lossy().into_owned()],
            state.next_id,
        )
        .unwrap_err();
        assert!(err.contains("Folder View"));

        // Pinning an already-present item returns empty `changed` cleanly.
        let (same_next, same_changed) = plan_native_pin_paths(
            &state,
            &view_items,
            g1,
            t1,
            &[already.to_string_lossy().into_owned()],
            state.next_id,
        )
        .unwrap();
        assert!(same_changed.is_empty());
        assert_eq!(same_next.next_id, state.next_id);

        // Targeting a Portal tab fails cleanly.
        let mut with_portal = state.clone();
        with_portal.group_mut(g2).unwrap().tabs[0].folder = Some(other_dir.to_string_lossy().into());
        assert!(plan_native_pin_paths(&with_portal, &view_items, g2, t2, &req, 500).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn rejects_replay_and_modified_destination(){
        let mut previous=State::empty();
        previous.add_group("Source",crate::model::Bounds{
            x:100.,y:100.,w:300.,h:300.});
        let mut proposed=previous.clone();
        proposed.groups[0].tabs[0].name="Moved".into();
        let event=NativeVerifiedDrop{
            sequence:7,before:previous.clone(),next:proposed.clone(),
            kind:crate::native_pin_drop::NativeMultiDropKind::SameBox{group:1},
            source_paths:vec!["C:/Desktop/Test.lnk".into()],
            desktop_drop_targets:Vec::new(),
        };
        assert!(validate_native_drop_permit(Some(&event),7,&previous,&proposed).is_ok());
        assert!(validate_native_drop_permit(Some(&event),8,&previous,&proposed).is_err());
        assert!(validate_native_drop_permit(None,7,&previous,&proposed).is_err());
        let mut forged=proposed.clone();
        forged.groups[0].bounds.x+=25.;
        assert!(validate_native_drop_permit(Some(&event),7,&previous,&forged).is_err());
    }

    #[test]
    fn verified_layout_is_queued_without_waiting_for_ui_and_reports_its_ack() {
        let (sender, events) = mpsc::sync_channel(1);
        let mut commands = VecDeque::new();
        let proposal = NativeVerifiedDrop {
            sequence: 1,
            before: State::empty(),
            next: State::empty(),
            kind: crate::native_pin_drop::NativeMultiDropKind::SameBox { group: 1 },
            source_paths: vec!["C:/Desktop/Test.lnk".into()],
            desktop_drop_targets: Vec::new(),
        };
        queue_verified_drop(&sender, &mut commands, &proposal).unwrap();
        assert!(queue_verified_drop(&sender, &mut commands, &proposal).is_err());
        let command = commands.pop_front().unwrap();
        assert!(commands.is_empty());
        assert_eq!(command.validated_drop_sequence, Some(proposal.sequence));
        assert_eq!(command.workspace_state.as_ref(), Some(&proposal.next));
        command.result.send(Ok(0)).unwrap();
        let (event, ticket) = events.try_recv().unwrap();
        assert_eq!(event.sequence, proposal.sequence);
        assert_eq!(ticket.poll().unwrap(), NativeWorkspaceReply::Applied(0));
    }

    #[test]
    fn disk_sync_accepts_pin_patch_after_in_memory_box_move_and_tab_switch_and_rejects_tamper() {
        use crate::model::{Bounds, Item, Tab};
        let mut persisted = State::empty();
        let a = persisted.add_group("BoxA", Bounds { x: 100., y: 100., w: 300., h: 300. });
        let b = persisted.add_group("BoxB", Bounds { x: 500., y: 100., w: 300., h: 300. });
        persisted.group_mut(a).unwrap().tabs.push(Tab::new(10, "More"));
        let pin = Item {
            id: persisted.id(),
            name: "Loose".into(),
            path: "C:/Desktop/Loose.lnk".into(),
            kind: "lnk".into(),
            size: 16,
            modified: 1,
            label: None,
        };
        let persisted_bytes = serde_json::to_vec(&persisted).unwrap();

        // Simulate in-memory Box move + Tab switch that stays off disk.
        let mut memory_before_drop = persisted.clone();
        memory_before_drop.group_mut(b).unwrap().bounds.x = 640.;
        memory_before_drop.group_mut(a).unwrap().active = 1;

        // First drag pins `Loose.lnk` into BoxB.
        let mut memory_after_drop = memory_before_drop.clone();
        memory_after_drop.group_mut(b).unwrap().tab_mut().items.push(pin);
        let paths = vec!["C:/Desktop/Loose.lnk".to_string()];
        let expected_disk = crate::native_ole_persist::pins_change_onto(
            &persisted, &memory_after_drop, &paths,
        ).unwrap();
        let committed_disk_bytes = serde_json::to_vec(&expected_disk).unwrap();

        let (accepted_disk, used_drop_patch) = accept_expected_workspace_disk_change(
            &committed_disk_bytes,
            &persisted_bytes,
            Some(&expected_disk),
            &memory_after_drop,
            &[],
        ).unwrap().unwrap();
        assert!(used_drop_patch);
        assert_eq!(accepted_disk.group(b).unwrap().bounds.x, 500.);
        assert_eq!(accepted_disk.group(a).unwrap().active, 0);
        assert_eq!(accepted_disk.group(b).unwrap().tab().items.len(), 1);

        // Second consecutive drag moves Pin to BoxA's active Tab 1 using updated persisted disk state.
        let mut memory_after_second = memory_after_drop.clone();
        memory_after_second.unassign("C:/Desktop/Loose.lnk");
        let moved_pin = memory_after_drop.group(b).unwrap().tab().items[0].clone();
        memory_after_second.group_mut(a).unwrap().tab_mut().items.push(moved_pin);
        let expected_disk_2 = crate::native_ole_persist::pins_change_onto(
            &accepted_disk, &memory_after_second, &paths,
        ).unwrap();
        let committed_disk_bytes_2 = serde_json::to_vec(&expected_disk_2).unwrap();
        let (accepted_disk_2, used_drop_patch_2) = accept_expected_workspace_disk_change(
            &committed_disk_bytes_2,
            &committed_disk_bytes,
            Some(&expected_disk_2),
            &memory_after_second,
            &[],
        ).unwrap().unwrap();
        assert!(used_drop_patch_2);
        assert!(accepted_disk_2.group(b).unwrap().tab().items.is_empty());
        assert_eq!(accepted_disk_2.group(a).unwrap().tabs[1].items.len(), 1);

        // Normal autosave writing real `rolled` flags while worker holds effective hover-reveal
        // state must also be accepted via `rolled_overrides`.
        let mut real_saved_under_reveal = accepted_disk_2.clone();
        real_saved_under_reveal.group_mut(a).unwrap().rolled = true;
        real_saved_under_reveal.group_mut(b).unwrap().rolled = false;
        let real_saved_bytes = serde_json::to_vec(&real_saved_under_reveal).unwrap();
        let mut effective_revealed = real_saved_under_reveal.clone();
        effective_revealed.group_mut(a).unwrap().rolled = false;
        effective_revealed.group_mut(b).unwrap().rolled = true;
        let (accepted_reveal_save, reveal_drop_patch) = accept_expected_workspace_disk_change(
            &real_saved_bytes,
            &committed_disk_bytes_2,
            None,
            &effective_revealed,
            &[(a, true), (b, false)],
        ).unwrap().unwrap();
        assert!(!reveal_drop_patch);
        assert!(accepted_reveal_save.group(a).unwrap().rolled);
        assert!(!accepted_reveal_save.group(b).unwrap().rolled);

        // Unrelated external modification to disk must still fail closed.
        let mut tampered = accepted_disk_2.clone();
        tampered.group_mut(b).unwrap().bounds.w = 999.;
        let tampered_bytes = serde_json::to_vec(&tampered).unwrap();
        assert!(accept_expected_workspace_disk_change(
            &tampered_bytes,
            &committed_disk_bytes_2,
            None,
            &memory_after_second,
            &[],
        ).is_err());

        // Saving after worker-ACKed Box move + `layouts`/`placements` (`remember_layout`)
        // update and cosmetic/Settings edits without worker reflow must be accepted.
        let mut with_layouts = memory_after_second.clone();
        with_layouts.group_mut(b).unwrap().bounds.x = 720.;
        with_layouts.group_mut(b).unwrap().color = 4;
        with_layouts.prefs.rollup = !with_layouts.prefs.rollup;
        let mut disk_saved_layout = with_layouts.clone();
        disk_saved_layout.placements.insert(
            b,
            crate::model::Placement {
                monitor: "MAIN".into(),
                x: 720.,
                y: 100.,
                w: 300.,
                h: 300.,
            },
        );
        let saved_layout_bytes = serde_json::to_vec(&disk_saved_layout).unwrap();
        let (accepted_layout, used_drop) = accept_expected_workspace_disk_change(
            &saved_layout_bytes,
            &committed_disk_bytes_2,
            None,
            &with_layouts,
            &[],
        ).unwrap().unwrap();
        assert!(!used_drop);
        assert_eq!(accepted_layout.group(b).unwrap().bounds.x, 720.);
    }

    #[test]
    fn rollup_toggle_disk_save_accepted_after_overrides_sync_and_rejected_when_stale() {
        use crate::model::Bounds;
        let mut expanded = State::empty();
        let a = expanded.add_group("Games", Bounds { x: 100., y: 100., w: 300., h: 300. });
        let expanded_bytes = serde_json::to_vec(&expanded).unwrap();

        let mut rolled = expanded.clone();
        rolled.group_mut(a).unwrap().rolled = true;
        let rolled_bytes = serde_json::to_vec(&rolled).unwrap();

        // Roll up under hover: worker effective state keeps `rolled = false`.
        // Before `rolled_overrides` updates (`&[]`), saving `rolled = true` fails closed.
        // Once synced (`&[(a, true)]`), the same disk save is accepted.
        assert!(accept_expected_workspace_disk_change(
            &rolled_bytes,
            &expanded_bytes,
            None,
            &expanded,
            &[],
        )
        .is_err());
        let (saved_rolled, used_drop) = accept_expected_workspace_disk_change(
            &rolled_bytes,
            &expanded_bytes,
            None,
            &expanded,
            &[(a, true)],
        )
        .unwrap()
        .unwrap();
        assert!(!used_drop);
        assert!(saved_rolled.group(a).unwrap().rolled);

        // Mirror unroll while hovered: disk now has `rolled = false` while worker effective
        // state still has `rolled = false`. Stale `rolled_overrides` (`&[(a, true)]`) fails
        // closed; synced `&[]` accepts cleanly.
        assert!(accept_expected_workspace_disk_change(
            &expanded_bytes,
            &rolled_bytes,
            None,
            &expanded,
            &[(a, true)],
        )
        .is_err());
        let (saved_unrolled, used_drop_2) = accept_expected_workspace_disk_change(
            &expanded_bytes,
            &rolled_bytes,
            None,
            &expanded,
            &[],
        )
        .unwrap()
        .unwrap();
        assert!(!used_drop_2);
        assert!(!saved_unrolled.group(a).unwrap().rolled);
    }

    #[test]
    fn pin_drop_bridge_hide_covers_dropped_and_changed_active_tabs_only() {
        use crate::model::{Bounds, Item};
        use crate::shell::ViewItem;
        let pin = |id: u64, path: &str| Item {
            id,
            name: path.into(),
            path: path.into(),
            kind: "lnk".into(),
            size: 16,
            modified: 1,
            label: None,
        };
        let mut s0 = State::empty();
        let b_dest = s0.add_group("Dest", Bounds { x: 100., y: 100., w: 300., h: 300. });
        let b_other = s0.add_group("Other", Bounds { x: 500., y: 100., w: 300., h: 300. });
        s0.group_mut(b_dest).unwrap().tab_mut().items.push(pin(1, "C:/d/InDest.lnk"));
        s0.group_mut(b_other).unwrap().tab_mut().items.push(pin(2, "C:/d/InOther.lnk"));

        let order = vec![
            ViewItem { path: "C:/d/InDest.lnk".into(), label: "InDest".into(), pos: (0, 0) },
            ViewItem { path: "C:/d/InOther.lnk".into(), label: "InOther".into(), pos: (0, 0) },
            ViewItem { path: "C:/d/Loose.lnk".into(), label: "Loose".into(), pos: (0, 0) },
        ];

        // DesktopToBox: hides Loose + Dest active tab, leaves Other visible.
        let mut s_d2b = s0.clone();
        s_d2b.group_mut(b_dest).unwrap().tab_mut().items.push(pin(3, "C:/d/Loose.lnk"));
        assert_eq!(
            pin_drop_bridge_hide_indices(&s0, &s_d2b, &["C:/d/Loose.lnk".into()], &order),
            vec![0, 2]
        );

        // BoxToBox: hides Both Dest & Other active tabs + moved pin.
        let mut s_b2b = s0.clone();
        let moved = s_b2b.group_mut(b_other).unwrap().tab_mut().items.pop().unwrap();
        s_b2b.group_mut(b_dest).unwrap().tab_mut().items.push(moved);
        assert_eq!(
            pin_drop_bridge_hide_indices(&s0, &s_b2b, &["C:/d/InOther.lnk".into()], &order),
            vec![0, 1]
        );

        // BoxToDesktop: hides Dest active tab + extracted pin, leaves Other visible.
        let mut s_b2d = s0.clone();
        s_b2d.group_mut(b_dest).unwrap().tab_mut().items.clear();
        assert_eq!(
            pin_drop_bridge_hide_indices(&s0, &s_b2d, &["C:/d/InDest.lnk".into()], &order),
            vec![0]
        );
    }
