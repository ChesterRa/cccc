use super::*;

fn reject_stale_write(op: &str, extra: Value) {
    let (_temp, home, store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/stale.md","content":"original"}),
    );
    let source = std::path::Path::new(
        saved.result["document"]["absolute_path"]
            .as_str()
            .expect("serialized document path"),
    );
    ok(
        &home,
        "assistant_voice_document_delete",
        json!({"group_id":group_id,"document_path":"voice/stale.md"}),
    );
    let index = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("documents/index.json");
    let before_index = std::fs::read(&index).expect("read fixture bytes");
    let before_state = assistant_state::load(&home, &group_id).expect("read assistant state");
    let ledger = store.ledger_path(&group_id).expect("resolve ledger path");
    let before_ledger = std::fs::read(&ledger).expect("read fixture bytes");
    let mut args = extra;
    args["group_id"] = json!(group_id);
    args["document_path"] = json!("voice/stale.md");
    let response = call(&home, op, args);
    assert!(!response.ok, "stale write unexpectedly succeeded: {op}");
    assert!(
        response
            .error
            .expect("rejection error")
            .message
            .contains("deleted")
    );
    assert!(!source.exists(), "deleted Markdown must not be recreated");
    assert_eq!(
        std::fs::read(index).expect("read fixture bytes"),
        before_index
    );
    assert_eq!(
        assistant_state::load(&home, &group_id).expect("read assistant state"),
        before_state
    );
    assert_eq!(
        std::fs::read(ledger).expect("read fixture bytes"),
        before_ledger
    );
}

#[test]
fn deleted_document_rejects_stale_save_without_recreating_file() {
    reject_stale_write(
        "assistant_voice_document_save",
        json!({"content":"stale editor content"}),
    );
}

#[test]
fn deleted_document_rejects_transcript_before_any_persistence() {
    reject_stale_write(
        "assistant_voice_transcript_append",
        json!({"session_id":"stale-session","segment_id":"late-segment","text":"late transcript","is_final":true,"flush":true}),
    );
}

#[test]
fn deletion_ledger_failure_restores_file_index_and_allows_retry() {
    let (_temp, home, store, group_id) = enabled_voice_group();
    ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/other.md","content":"other"}),
    );
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/delete.md","content":"original"}),
    );
    let source = std::path::Path::new(
        saved.result["document"]["absolute_path"]
            .as_str()
            .expect("serialized document path"),
    );
    let index = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("documents/index.json");
    let before_index = std::fs::read(&index).expect("read fixture bytes");
    let ledger = store.ledger_path(&group_id).expect("resolve ledger path");
    let backup = ledger.with_extension("fixture-backup");
    std::fs::rename(&ledger, &backup).expect("move fixture file");
    std::fs::create_dir(&ledger).expect("create ledger failure fixture"); // Real append failure, no mocked transaction.
    let args = json!({"group_id":group_id,"document_path":"voice/delete.md"});
    let response = call(&home, "assistant_voice_document_delete", args.clone());
    assert!(!response.ok);
    assert_eq!(
        std::fs::read_to_string(source).expect("read document text"),
        "original"
    );
    assert_eq!(
        std::fs::read(&index).expect("read fixture bytes"),
        before_index
    );
    std::fs::remove_dir(&ledger).expect("remove ledger failure fixture");
    std::fs::rename(&backup, &ledger).expect("move fixture file");
    let retried = ok(&home, "assistant_voice_document_delete", args);
    assert_eq!(retried.result["document"]["status"], "deleted");
    assert!(!source.exists());
    let trash = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("trash");
    assert_eq!(
        std::fs::read_dir(trash)
            .expect("read recovery directory")
            .count(),
        0
    );
}
