use serde_json::{Map, json};

#[test]
fn mkdir_honors_exist_ok_without_accepting_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut args = json!({"path":"directory"})
        .as_object()
        .cloned()
        .expect("fixture operation");
    crate::repo::call(temp.path(), "mkdir", &args).expect("first mkdir");
    crate::repo::call(temp.path(), "mkdir", &args).expect("default is idempotent");
    args.insert("exist_ok".into(), json!(true));
    crate::repo::call(temp.path(), "mkdir", &args).expect("explicit idempotence");
    args.insert("exist_ok".into(), json!(false));
    assert!(crate::repo::call(temp.path(), "mkdir", &args).is_err());
    std::fs::write(temp.path().join("file"), "keep").expect("file");
    args.insert("path".into(), json!("file"));
    args.insert("exist_ok".into(), json!(true));
    assert!(crate::repo::call(temp.path(), "mkdir", &args).is_err());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("file")).expect("fixture operation"),
        "keep"
    );
}

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
