use cccc_contracts::{Actor, GroupState};
use cccc_core::group_copy;
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[test]
fn copy_excludes_secrets_and_imports_with_new_identity_on_conflict() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let group = store.create("Copy Me", "topic").expect("group");
    group_scope::attach(
        &store,
        &group.group_id,
        Scope {
            scope_key: "s_original".into(),
            url: repo.to_string_lossy().into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("scope");
    store
        .mutate(&group.group_id, |doc| {
            let mut actor = Actor::new("peer1");
            actor.env.insert("PUBLIC_VALUE".into(), "ok".into());
            actor
                .env
                .insert("API_TOKEN".into(), "must-not-export".into());
            actor.default_scope_key = "s_original".into();
            doc.actors.push(actor);
            Ok(())
        })
        .expect("actor");
    let state = store.state_dir(&group.group_id).expect("state");
    std::fs::write(state.join("env_private.json"), "secret").expect("secret");
    std::fs::create_dir_all(state.join("runtime_sessions")).expect("runtime");
    std::fs::write(state.join("runtime_sessions/session.json"), "runtime").expect("runtime file");
    std::fs::write(state.join("note.txt"), "preserved").expect("note");

    let (bytes, manifest, _) = group_copy::export(&store, &group.group_id).expect("export");
    assert!(!manifest.contains_secrets);
    let preview = group_copy::preview(&store, &bytes).expect("preview");
    assert!(preview.group_id_conflict);
    assert_eq!(preview.actor_count, 1);

    let imported =
        group_copy::import(&store, &bytes, &repo.to_string_lossy(), "Imported").expect("import");
    assert_ne!(imported.group_id, group.group_id);
    assert!(imported.group_id_conflict);
    let doc = store.load(&imported.group_id).expect("imported group");
    assert_eq!(doc.title, "Imported");
    assert_eq!(doc.state, GroupState::Idle);
    assert!(!doc.running);
    assert_eq!(
        doc.actors[0].env.get("PUBLIC_VALUE").map(String::as_str),
        Some("ok")
    );
    assert!(!doc.actors[0].env.contains_key("API_TOKEN"));
    let imported_state = store.state_dir(&imported.group_id).expect("imported state");
    assert_eq!(
        std::fs::read_to_string(imported_state.join("note.txt")).expect("note"),
        "preserved"
    );
    assert!(!imported_state.join("env_private.json").exists());
    assert!(!imported_state.join("runtime_sessions").exists());
}

#[test]
fn copy_rejects_zip_path_traversal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer.start_file("../escape", options).expect("entry");
    writer.write_all(b"bad").expect("write");
    let bytes = writer.finish().expect("zip").into_inner();
    assert!(group_copy::preview(&store, &bytes).is_err());
}
