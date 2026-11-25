use std::os::unix::fs::PermissionsExt;

#[test]
fn persistent_text_round_trips_in_private_files() {
    let root = std::env::temp_dir().join(format!("glacier-storage-test-{}", std::process::id()));
    let path = root.join("notes.json");
    let values = vec![
        "café and 日本語".to_owned(),
        "second line\nthird line".to_owned(),
    ];
    glacier::storage::write_at(&path, &values).unwrap();
    let read: Vec<String> = glacier::storage::read_at(&path).unwrap();
    assert_eq!(read, values);
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    std::fs::remove_dir_all(root).unwrap();
}
