use super::*;

fn library_update(home: &HomeLayout, group: &str, mut args: Value) -> DaemonResponse {
    args["group_id"] = json!(group);
    ok(home, "assistant_voice_document_library_update", args)
}

#[test]
fn folders_persist_through_document_save_and_removal_preserves_documents() {
    let (_temp, home, _store, group) = enabled_voice_group();
    ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group,"document_path":"voice/notes.md","content":"original"}),
    );
    let created = library_update(
        &home,
        &group,
        json!({"action":"create_folder","name":"会议"}),
    );
    let id = &created.result["folders"][0]["folder_id"];
    library_update(
        &home,
        &group,
        json!({"action":"move","folder_id":id,"document_path":"voice/notes.md"}),
    );
    ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group,"document_path":"voice/notes.md","content":"edited"}),
    );
    let read = ok(
        &home,
        "assistant_voice_document_library",
        json!({"group_id":group}),
    );
    assert_eq!(&read.result["documents"][0]["folder_id"], id);
    assert_eq!(read.result["folders"][0]["name"], "会议");
    assert_eq!(
        read.result["documents"][0]["document_path"],
        "voice/notes.md"
    );
    let renamed = library_update(
        &home,
        &group,
        json!({"action":"rename_folder","folder_id":id,"name":"项目会议"}),
    );
    assert_eq!(renamed.result["folders"][0]["name"], "项目会议");
    let removed = library_update(
        &home,
        &group,
        json!({"action":"remove_folder","folder_id":id}),
    );
    assert_eq!(removed.result["folders"], json!([]));
    assert_eq!(removed.result["documents"][0]["folder_id"], "");
    assert_eq!(removed.result["documents"][0]["content"], "edited");
}

#[test]
fn archive_is_viewable_and_restorable_but_deleted_documents_are_excluded() {
    let (_temp, home, _store, group) = enabled_voice_group();
    ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group,"document_path":"voice/notes.md","content":"original"}),
    );
    ok(
        &home,
        "assistant_voice_document_archive",
        json!({"group_id":group,"document_path":"voice/notes.md"}),
    );
    let archived = ok(
        &home,
        "assistant_voice_document_library",
        json!({"group_id":group}),
    );
    assert_eq!(archived.result["documents"][0]["status"], "archived");
    assert_eq!(archived.result["documents"][0]["content"], "original");
    let restored = library_update(
        &home,
        &group,
        json!({"action":"restore","document_path":"voice/notes.md"}),
    );
    assert_eq!(restored.result["documents"][0]["status"], "active");
    ok(
        &home,
        "assistant_voice_document_delete",
        json!({"group_id":group,"document_path":"voice/notes.md"}),
    );
    let deleted = ok(
        &home,
        "assistant_voice_document_library",
        json!({"group_id":group}),
    );
    assert_eq!(deleted.result["documents"], json!([]));
    let index = home
        .root()
        .join("voice-secretary")
        .join(&group)
        .join("documents/index.json");
    let before = std::fs::read(&index).expect("read fixture bytes");
    // A stale client's archive request must not resurrect the deleted tombstone.
    assert!(
        !call(
            &home,
            "assistant_voice_document_archive",
            json!({"group_id":group,"document_path":"voice/notes.md"}),
        )
        .ok
    );
    assert_eq!(std::fs::read(index).expect("read fixture bytes"), before);
    assert!(
        !call(
            &home,
            "assistant_voice_document_library_update",
            json!({"group_id":group,"action":"restore","document_path":"voice/notes.md"})
        )
        .ok
    );
}

#[test]
fn library_rejects_invalid_actions_without_mutating_state() {
    let (_temp, home, _store, group) = enabled_voice_group();
    library_update(
        &home,
        &group,
        json!({"action":"create_folder","name":"会议"}),
    );
    for mut args in [
        json!({"action":"create_folder","name":"会议"}),
        json!({"action":"create_folder","name":"  "}),
        json!({"action":"move","folder_id":"missing","document_path":"voice/a.md"}),
        json!({"action":"remove_folder","folder_id":"missing"}),
        json!({"action":"create_folder","name":"拒绝","by":"unknown"}),
    ] {
        args["group_id"] = json!(group);
        assert!(!call(&home, "assistant_voice_document_library_update", args).ok);
    }
    let read = ok(
        &home,
        "assistant_voice_document_library",
        json!({"group_id":group}),
    );
    assert_eq!(
        read.result["folders"]
            .as_array()
            .expect("document array")
            .len(),
        1
    );
}

#[test]
fn renaming_a_document_changes_only_its_title() {
    let (_temp, home, _store, group_id) = enabled_voice_group();
    let saved = ok(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/rename.md","content":"body","title":"Before"}),
    );
    let renamed = library_update(
        &home,
        &group_id,
        json!({"action":"rename","document_path":"voice/rename.md","name":"  After  "}),
    );
    assert!(renamed.ok, "{:?}", renamed.error);
    let document = renamed.result["documents"]
        .as_array()
        .and_then(|documents| {
            documents
                .iter()
                .find(|d| d["document_path"] == "voice/rename.md")
        })
        .cloned()
        .expect("renamed document");
    assert_eq!(document["title"], "After");
    assert_eq!(document["content"], "body");
    assert_eq!(document["status"], "active");
    assert_eq!(
        document["document_id"],
        saved.result["document"]["document_id"]
    );
    assert_eq!(
        std::fs::read_to_string(
            saved.result["document"]["absolute_path"]
                .as_str()
                .expect("path")
        )
        .expect("file"),
        "body"
    );
    for args in [
        json!({"group_id":group_id,"action":"rename","document_path":"voice/rename.md","name":""}),
        json!({"group_id":group_id,"action":"rename","document_path":"voice/rename.md","name":"x".repeat(81)}),
        json!({"group_id":group_id,"action":"rename","document_path":"voice/missing.md","name":"Nope"}),
    ] {
        assert!(!call(&home, "assistant_voice_document_library_update", args).ok);
    }
    let listed = ok(
        &home,
        "assistant_voice_document_list",
        json!({"group_id":group_id}),
    );
    assert_eq!(listed.result["documents"][0]["title"], "After");
}
