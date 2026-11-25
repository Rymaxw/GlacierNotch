use glacier::notes::{Action, Notes};

#[test]
fn quick_note_lifecycle_survives_reload() {
    let root = std::env::temp_dir().join(format!("glacier-notes-test-{}", std::process::id()));
    let path = root.join("notes.json");
    let mut notes = Notes::from_path(path.clone());
    assert!(notes.items.is_empty());
    notes.act(Action::Add("A note with café and 日本語".into()));
    assert!(notes.error.is_none());
    let id = notes.items[0].id;
    let mut reloaded = Notes::from_path(path.clone());
    assert_eq!(
        reloaded.act(Action::Copy(id)).as_deref(),
        Some("A note with café and 日本語")
    );
    reloaded.act(Action::Edit {
        id,
        text: "Edited note\nsecond line".into(),
    });
    let mut edited = Notes::from_path(path.clone());
    assert_eq!(edited.items[0].id, id);
    assert_eq!(edited.items[0].text, "Edited note\nsecond line");
    edited.act(Action::Remove(id));
    assert!(Notes::from_path(path).items.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}
