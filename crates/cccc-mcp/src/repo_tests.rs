use serde_json::{Map, json};

#[test]
fn rejects_parent_and_absolute_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(crate::repo::resolve(temp.path(), "../outside", false).is_err());
    assert!(crate::repo::resolve(temp.path(), "/tmp/outside", false).is_err());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().expect("tempdir");
    symlink("/tmp", temp.path().join("outside")).expect("symlink");
    assert!(crate::repo::resolve(temp.path(), "outside", false).is_err());
}

#[test]
fn writes_and_replaces_exact_text() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut write = Map::new();
    write.insert("path".into(), json!("notes.txt"));
    write.insert("content".into(), json!("alpha beta"));
    crate::repo::call(temp.path(), "write", &write).expect("write");

    let mut replace = Map::new();
    replace.insert("path".into(), json!("notes.txt"));
    replace.insert("old_text".into(), json!("beta"));
    replace.insert("new_text".into(), json!("gamma"));
    crate::repo::call(temp.path(), "replace", &replace).expect("replace");
    assert_eq!(
        std::fs::read_to_string(temp.path().join("notes.txt")).expect("read"),
        "alpha gamma"
    );
}
