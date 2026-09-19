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

#[test]
fn multi_replace_validates_every_edit_before_writing_and_honors_counts() {
    let temp = tempfile::tempdir().expect("fixture operation");
    let path = temp.path().join("a");
    std::fs::write(&path, "one two two\n").expect("fixture operation");
    let args = json!({"path":"a","replacements":[{"old_text":"one","new_text":"changed"},{"old_text":"missing","new_text":"wrong"}]});
    assert!(
        crate::repo::call(
            temp.path(),
            "multi_replace",
            args.as_object().expect("fixture operation")
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("fixture operation"),
        "one two two\n"
    );
    let replace = json!({"path":"a","old_text":"two","new_text":"three","replace_all":true,"expected_replacements":3});
    assert!(
        crate::repo::call(
            temp.path(),
            "replace",
            replace.as_object().expect("fixture operation")
        )
        .is_err()
    );
    let args = json!({"path":"a","replacements":[{"old_text":"one","new_text":"changed"},{"old_text":"two","new_text":"three","replace_all":true,"expected_replacements":2}]});
    crate::repo::call(
        temp.path(),
        "multi_replace",
        args.as_object().expect("fixture operation"),
    )
    .expect("fixture operation");
    assert_eq!(
        std::fs::read_to_string(&path).expect("fixture operation"),
        "changed three three\n"
    );
}

#[cfg(unix)]
#[test]
fn exact_edits_preserve_executable_mode_and_read_hash_covers_the_whole_file() {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("fixture operation");
    let path = temp.path().join("run.sh");
    let source = "#!/bin/sh\r\necho before\r\n";
    std::fs::write(&path, source).expect("fixture operation");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("fixture operation");
    let args = json!({"path":"run.sh","start_line":2,"end_line":2});
    let read = crate::repo::call(
        temp.path(),
        "read",
        args.as_object().expect("fixture operation"),
    )
    .expect("fixture operation");
    assert_eq!(
        read["sha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    let edit = json!({"path":"run.sh","old_text":"before","new_text":"after","expected_sha256":read["sha256"]});
    crate::repo::call(
        temp.path(),
        "replace",
        edit.as_object().expect("fixture operation"),
    )
    .expect("fixture operation");
    assert_eq!(
        std::fs::metadata(&path)
            .expect("fixture operation")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("fixture operation"),
        "#!/bin/sh\r\necho after\r\n"
    );
}

#[test]
fn explicit_empty_content_and_deletion_replacements_are_supported() {
    let temp = tempfile::tempdir().expect("fixture operation");
    let args = json!({"path":"empty","content":""});
    crate::repo::call(
        temp.path(),
        "write",
        args.as_object().expect("fixture operation"),
    )
    .expect("fixture operation");
    std::fs::write(temp.path().join("a"), "remove-me").expect("fixture operation");
    let edit = json!({"path":"a","old_text":"remove-me","new_text":""});
    crate::repo::call(
        temp.path(),
        "replace",
        edit.as_object().expect("fixture operation"),
    )
    .expect("fixture operation");
    assert_eq!(
        std::fs::read(temp.path().join("a")).expect("fixture operation"),
        b""
    );
    let missing = json!({"path":"a","old_text":"anything"});
    assert!(
        crate::repo::call(
            temp.path(),
            "replace",
            missing.as_object().expect("fixture operation")
        )
        .is_err()
    );
}

#[test]
fn public_read_only_surface_rejects_every_edit_action_before_touching_files() {
    let temp = tempfile::tempdir().expect("workspace");
    std::fs::write(temp.path().join("keep"), "original").expect("fixture");
    let catalog = crate::tools::catalog();
    let edit = catalog
        .iter()
        .find(|t| t["name"] == "cccc_repo_edit")
        .expect("edit schema");
    let args = json!({"path":"keep","content":"wrong","old_text":"original","new_text":"wrong","dest_path":"moved","replacements":[{"old_text":"original","new_text":"wrong"}]});
    for action in edit["inputSchema"]["properties"]["action"]["enum"]
        .as_array()
        .expect("actions")
    {
        let action = action.as_str().expect("action");
        let error = crate::repo::call_tool(
            temp.path(),
            "cccc_repo",
            action,
            args.as_object().expect("args"),
        )
        .expect_err("read only");
        assert!(error.contains("not supported"));
        assert_eq!(
            std::fs::read_to_string(temp.path().join("keep")).expect("file"),
            "original"
        );
        assert!(!temp.path().join("moved").exists());
    }
}

#[test]
fn read_and_edit_share_path_aliases_and_moves_accept_the_advertised_destinations() {
    let temp = tempfile::tempdir().expect("workspace");
    for alias in ["dest_path", "to_path", "new_path"] {
        std::fs::write(temp.path().join("from"), "old").expect("fixture");
        let read = crate::repo::call(
            temp.path(),
            "read",
            json!({"file_path":"from"}).as_object().expect("args"),
        )
        .expect("read");
        crate::repo::call(temp.path(),"replace",json!({"file_path":"from","old_text":"old","new_text":"new","expected_sha256":read["sha256"]}).as_object().expect("args")).expect("edit");
        let mut args = json!({"file_path":"from"});
        args[alias] = json!(alias);
        crate::repo::call(temp.path(), "move", args.as_object().expect("args")).expect("move");
        assert_eq!(
            std::fs::read_to_string(temp.path().join(alias)).expect("moved"),
            "new"
        );
    }
    std::fs::write(temp.path().join("keep"), "unchanged").expect("fixture");
    assert!(
        crate::repo::call(
            temp.path(),
            "move",
            json!({"path":"keep","dest_path":"a","to_path":"b"})
                .as_object()
                .expect("args")
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("keep")).expect("file"),
        "unchanged"
    );
}
