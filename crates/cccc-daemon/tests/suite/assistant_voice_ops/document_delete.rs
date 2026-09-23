use super::*;

#[test]
fn deleting_document_removes_its_file_and_all_visible_references() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let first = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/first.md","content":"first"}),
    );
    let second = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/second.md","content":"second"}),
    );
    let path = second.result["document"]["absolute_path"]
        .as_str()
        .expect("path");
    let deleted = ok(
        &home,
        "assistant_voice_document_delete",
        json!({"group_id":group_id,"document_path":"voice/second.md"}),
    );
    assert_eq!(deleted.result["document"]["status"], "deleted");
    assert_eq!(deleted.result["event"]["data"]["action"], "deleted");
    assert!(!std::path::Path::new(path).exists());
    assert!(deleted.result["document"]["trash_path"].is_null());
    let trash = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("trash");
    assert!(
        std::fs::read_dir(&trash).map_or(true, |mut entries| entries.next().is_none()),
        "no recovery copy may remain"
    );
    for op in ["assistant_index", "assistant_voice_document_list"] {
        let result = ok(
            &home,
            op,
            json!({"group_id":group_id,"include_archived":true}),
        );
        assert_eq!(
            result.result["documents"]
                .as_array()
                .expect("document array")
                .len(),
            1
        );
        assert_eq!(
            result.result["active_document_id"],
            first.result["document"]["document_id"]
        );
    }
    assert!(
        !call(
            &home,
            "assistant_voice_document_select",
            json!({"group_id":group_id,"document_path":"voice/second.md"})
        )
        .ok
    );
}

#[test]
fn deleting_document_rejects_unknown_paths_and_unauthorized_actor() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/keep.md","content":"keep"}),
    );
    for args in [
        json!({"group_id":group_id,"document_path":"../keep.md"}),
        json!({"group_id":group_id,"document_path":"voice/unknown.md"}),
        json!({"group_id":group_id,"document_path":"voice/keep.md","by":"unknown"}),
    ] {
        assert!(!call(&home, "assistant_voice_document_delete", args).ok);
    }
    assert_eq!(
        std::fs::read_to_string(
            saved.result["document"]["absolute_path"]
                .as_str()
                .expect("serialized document path")
        )
        .expect("read preserved document text"),
        "keep"
    );
}

#[test]
fn deleting_document_during_recording_is_rejected() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/recording.md","content":"keep"}),
    );
    ok(
        &home,
        "assistant_voice_recording_lease",
        json!({"group_id":group_id,"action":"acquire","owner_id":"test-recorder"}),
    );
    let result = call(
        &home,
        "assistant_voice_document_delete",
        json!({"group_id":group_id,"document_path":"voice/recording.md"}),
    );
    assert_eq!(
        result.error.expect("rejection error").code,
        "voice_recording_active"
    );
    assert_eq!(
        load_voice_state(&home, &group_id)["documents"]
            .as_array()
            .expect("document array")
            .len(),
        1
    );
}

#[cfg(unix)]
#[test]
fn deleting_document_rejects_a_symlink_without_touching_its_target() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/link.md","content":"keep"}),
    );
    let path = std::path::Path::new(
        saved.result["document"]["absolute_path"]
            .as_str()
            .expect("serialized document path"),
    );
    let outside = _temp.path().join("outside.md");
    std::fs::write(&outside, "outside").expect("write fixture bytes");
    std::fs::remove_file(path).expect("remove fixture file");
    std::os::unix::fs::symlink(&outside, path).expect("create fixture symlink");
    assert!(
        !call(
            &home,
            "assistant_voice_document_delete",
            json!({"group_id":group_id,"document_path":"voice/link.md"})
        )
        .ok
    );
    assert_eq!(
        std::fs::read_to_string(outside).expect("read document text"),
        "outside"
    );
}
