use super::*;

#[test]
fn exact_anchor_selects_the_intended_repeated_block() {
    let text = "fn first() {\n    old();\n}\nfn second() {\n    old();\n}\n";
    assert_eq!(
        update(text, &["@@ fn second() {", "-    old();", "+    new();"])
            .expect("fixture operation"),
        "fn first() {\n    old();\n}\nfn second() {\n    new();\n}\n"
    );
    assert!(update(text, &["@@", "-    old();", "+    new();"]).is_err());
    assert!(update(text, &["@@ missing", "-    old();", "+    new();"]).is_err());
    assert!(update("prefix old suffix\n", &["@@", "-old", "+new"]).is_err());
}

#[test]
fn multiple_hunks_are_ordered_and_cannot_match_inserted_text() {
    assert_eq!(
        update("a\nb\nc\nd\n", &["@@", "-a", "+A", "@@", "-d", "+D"]).expect("fixture operation"),
        "A\nb\nc\nD\n"
    );
    assert!(update("a\nb\n", &["@@", "-b", "+B", "@@", "-a", "+A"]).is_err());
    assert!(update("a\n", &["@@", "-a", "+new", "@@", "-new", "+wrong"]).is_err());
}

#[test]
fn eof_anchor_append_empty_files_and_crlf_are_preserved() {
    assert_eq!(
        update(
            "same\nmiddle\nsame\n",
            &["@@", "-same", "+last", "*** End of File"]
        )
        .expect("fixture operation"),
        "same\nmiddle\nlast\n"
    );
    assert!(
        update(
            "same\nmiddle\n",
            &["@@", "-same", "+last", "*** End of File"]
        )
        .is_err()
    );
    assert_eq!(
        update("a\r\nb\r\n", &["@@", "-b", "+B", "*** End of File"]).expect("fixture operation"),
        "a\r\nB\r\n"
    );
    assert_eq!(
        update("a\r\nb", &["@@", "-b", "+B"]).expect("fixture operation"),
        "a\r\nB"
    );
    assert_eq!(
        update("", &["@@", "+first"]).expect("fixture operation"),
        "first\n"
    );
    assert_eq!(
        update("last", &["@@", "+added"]).expect("fixture operation"),
        "last\nadded"
    );
    assert_eq!(
        update(
            "head\r\ncontext\nold\r\ntail\n",
            &["@@", " context", "-old", "+new"]
        )
        .expect("fixture operation"),
        "head\r\ncontext\nnew\r\ntail\n"
    );
}

#[test]
fn move_edit_add_nested_file_and_delete_work_together() {
    let temp = tempfile::tempdir().expect("fixture operation");
    std::fs::write(temp.path().join("source.rs"), "old\n").expect("fixture operation");
    std::fs::write(temp.path().join("gone"), "delete\n").expect("fixture operation");
    let patch = "*** Begin Patch\n*** Update File: source.rs\n*** Move to: nested/destination.rs\n@@\n-old\n+new\n*** Add File: tests/new.txt\n+test\n*** Delete File: gone\n*** End Patch";
    let files = apply(temp.path(), patch).expect("fixture operation");
    assert_eq!(
        files,
        [
            "source.rs",
            "nested/destination.rs",
            "tests/new.txt",
            "gone"
        ]
    );
    assert!(!temp.path().join("source.rs").exists());
    assert!(!temp.path().join("gone").exists());
    assert_eq!(
        std::fs::read_to_string(temp.path().join("nested/destination.rs"))
            .expect("fixture operation"),
        "new\n"
    );
}

#[test]
fn rejected_later_hunk_and_destination_conflicts_do_not_partially_apply() {
    let temp = tempfile::tempdir().expect("fixture operation");
    std::fs::write(temp.path().join("a"), "original\n").expect("fixture operation");
    std::fs::write(temp.path().join("b"), "existing\n").expect("fixture operation");
    for patch in [
        "*** Begin Patch\n*** Add File: new\n+created\n*** Update File: a\n@@\n-missing\n+bad\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n*** Move to: b\n@@\n-original\n+changed\n*** End Patch",
        "*** Begin Patch\n*** Update File: a\n@@\n-original\n+changed\n*** Update File: a\n@@\n-original\n+changed-again\n*** End Patch",
        "*** Begin Patch\n*** Add File: new\n+created\n*** Delete File: b\n-garbage\n*** End Patch",
    ] {
        assert!(apply(temp.path(), patch).is_err());
        assert_eq!(
            std::fs::read_to_string(temp.path().join("a")).expect("fixture operation"),
            "original\n"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("b")).expect("fixture operation"),
            "existing\n"
        );
        assert!(!temp.path().join("new").exists());
    }
}

#[test]
fn unicode_malformed_input_is_an_error_instead_of_a_panic() {
    assert!(update("中文\n", &["@@", "中文"]).is_err());
    assert_eq!(
        update("中文\n", &["@@", "-中文", "+日本語"]).expect("fixture operation"),
        "日本語\n"
    );
}

#[cfg(unix)]
#[test]
fn edits_and_moves_preserve_executable_permissions_and_reject_symlink_escape() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let temp = tempfile::tempdir().expect("fixture operation");
    let outside = tempfile::tempdir().expect("fixture operation");
    let path = temp.path().join("run.sh");
    std::fs::write(&path, "echo before\n").expect("fixture operation");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("fixture operation");
    apply(
        temp.path(),
        "*** Begin Patch\n*** Update File: run.sh\n@@\n-echo before\n+echo after\n*** End Patch",
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
    apply(
        temp.path(),
        "*** Begin Patch\n*** Update File: run.sh\n*** Move to: moved.sh\n*** End Patch",
    )
    .expect("fixture operation");
    assert_eq!(
        std::fs::metadata(temp.path().join("moved.sh"))
            .expect("fixture operation")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    symlink(outside.path(), temp.path().join("escape")).expect("fixture operation");
    for raw in ["escape/nested/file", "../outside"] {
        let patch = format!("*** Begin Patch\n*** Add File: {raw}\n+bad\n*** End Patch");
        assert!(apply(temp.path(), &patch).is_err());
    }
    assert!(!outside.path().join("nested").exists());
}

#[tokio::test]
async fn public_patch_alias_and_read_hash_support_safe_followup_edits() {
    use serde_json::json;
    let temp = tempfile::tempdir().expect("fixture operation");
    let args = json!({"input":"*** Begin Patch\n*** Add File: a\n+original\n*** End Patch"});
    super::super::local_tools::apply_patch(
        temp.path(),
        args.as_object().expect("fixture operation"),
    )
    .await
    .expect("fixture operation");
    let read = json!({"path":"a"});
    let result = crate::repo::call(
        temp.path(),
        "read",
        read.as_object().expect("fixture operation"),
    )
    .expect("fixture operation");
    assert!(result["sha256"].as_str().is_some_and(|v| v.len() == 64));
    let edit = json!({"path":"a","old_text":"original","new_text":"updated","expected_sha256":result["sha256"]});
    crate::repo::call(
        temp.path(),
        "replace",
        edit.as_object().expect("fixture operation"),
    )
    .expect("fixture operation");
    assert!(
        crate::repo::call(
            temp.path(),
            "replace",
            edit.as_object().expect("fixture operation")
        )
        .is_err()
    );
}
