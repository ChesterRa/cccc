// Included by the crate-level integration test harness.
use cccc_core::presentation::{self, Publish};
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};

#[test]
fn presentation_persists_contract_and_protects_workspace_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join("docs")).expect("docs");
    std::fs::write(repo.join("docs/report.md"), "# report\n").expect("report");
    std::fs::write(temp.path().join("outside.md"), "secret").expect("outside");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let group = store.create("presentation", "").expect("group");
    group_scope::attach(
        &store,
        &group.group_id,
        Scope {
            scope_key: "scope_repo".into(),
            url: repo.to_string_lossy().into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");

    let (slot_id, card, snapshot, replaced) = presentation::publish(
        &store,
        &group.group_id,
        Publish {
            slot: "1".into(),
            path: "docs/report.md".into(),
            by: "user".into(),
            ..Publish::default()
        },
    )
    .expect("publish workspace");
    assert_eq!(slot_id, "slot-1");
    assert_eq!(card.slot_id, "slot-1");
    assert_eq!(card.card_type, "markdown");
    assert_eq!(card.published_by, "user");
    assert_eq!(card.content.mode, "workspace_link");
    assert_eq!(
        card.content.workspace_rel_path.as_deref(),
        Some("docs/report.md")
    );
    assert!(!replaced);
    assert_eq!(snapshot.slots[0].index, 1);

    let reloaded = presentation::load(&store, &group.group_id).expect("reload");
    assert_eq!(reloaded, snapshot);
    assert!(
        presentation::resolve_workspace_path(
            &store.load(&group.group_id).expect("group"),
            "../outside.md"
        )
        .is_err()
    );

    let (cleared, cleared_snapshot) =
        presentation::clear(&store, &group.group_id, "slot_1", "user").expect("clear");
    assert_eq!(cleared, ["slot-1"]);
    assert!(cleared_snapshot.slots[0].card.is_none());
    assert!(cleared_snapshot.highlight_slot_id.is_empty());
}

#[test]
fn presentation_supports_inline_table_and_blob_cards() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("cards", "").expect("group");

    let (_, markdown, _, _) = presentation::publish(
        &store,
        &group.group_id,
        Publish {
            content: "# hello".into(),
            by: "user".into(),
            ..Publish::default()
        },
    )
    .expect("markdown");
    assert_eq!(markdown.content.markdown.as_deref(), Some("# hello"));

    let (_, table, _, _) = presentation::publish(
        &store,
        &group.group_id,
        Publish {
            card_type: "table".into(),
            table: Some(serde_json::json!([{"name":"a","value":1},{"name":"b","value":2}])),
            by: "user".into(),
            ..Publish::default()
        },
    )
    .expect("table");
    assert_eq!(
        table.content.table.expect("table").columns,
        ["name", "value"]
    );

    let blob = cccc_core::blobs::store(&home, &group.group_id, b"image").expect("blob");
    let (_, image, _, _) = presentation::publish(
        &store,
        &group.group_id,
        Publish {
            card_type: "image".into(),
            title: "preview.png".into(),
            blob_rel_path: blob.path,
            by: "user".into(),
            ..Publish::default()
        },
    )
    .expect("image");
    assert_eq!(image.content.mime_type.as_deref(), Some("image/png"));
}

#[test]
fn presentation_rejects_non_http_remote_references_without_mutating_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let group = store.create("url boundary", "").expect("group");

    let result = presentation::publish(
        &store,
        &group.group_id,
        Publish {
            slot: "slot-1".into(),
            card_type: "web_preview".into(),
            url: "javascript:alert(document.domain)".into(),
            by: "user".into(),
            ..Publish::default()
        },
    );

    assert!(result.is_err());
    assert!(
        presentation::load(&store, &group.group_id)
            .expect("presentation")
            .slots
            .iter()
            .all(|slot| slot.card.is_none())
    );
}
