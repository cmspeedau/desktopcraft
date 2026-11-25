    use super::*;

    #[test]
    fn moving_between_groups_changes_only_pins_not_the_disk_file() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let before = std::fs::metadata(&path).unwrap().len();
        let mut state = State::empty();
        let bounds = Bounds {
            x: 0.,
            y: 0.,
            w: 300.,
            h: 220.,
        };
        let from = state.add_group("From", bounds);
        let to = state.add_group("To", Bounds { x: 310., ..bounds });
        state.import(&path, from).unwrap();
        let duplicate = state.add_group("Old duplicate", Bounds { x: 650., ..bounds });
        state.import(&path, duplicate).unwrap();
        let item_id = state.group(from).unwrap().tab().items[0].id;
        state.transfer(from, item_id, to, 0).unwrap();
        assert!(state.group(from).unwrap().tab().items.is_empty());
        assert!(
            state.group(duplicate).unwrap().tab().items.is_empty(),
            "Old duplicate pin must not reappear"
        );
        assert_eq!(state.group(to).unwrap().tab().items[0].id, item_id);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), before);
        state.unassign(&path.to_string_lossy());
        assert!(state.group(to).unwrap().tab().items.is_empty());
        assert!(
            path.exists(),
            "Ungrouping a desktop icon must not delete its file"
        );
    }
