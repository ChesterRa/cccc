// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, Event, GroupState};
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};
use cccc_core::{group_copy, ledger, ledger_archive, user_requests};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[test]
fn compacted_requests_survive_copy_with_a_new_group_and_scope() {
    for compressed in [false, true] {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("copy", "").expect("group");
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
        .expect("attach");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "peer".into();
        event.scope_key = "s_original".into();
        event.data = serde_json::json!({
            "text": "Please answer", "to": ["user"], "message_mode": "request_reply"
        })
        .as_object()
        .expect("object")
        .clone();
        ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
            .expect("append");
        let segment = ledger_archive::compact(&home, &group.group_id, "fixture")
            .expect("compact")
            .expect("segment");
        let original_segment = std::fs::read(&segment).expect("read segment");
        if compressed {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&original_segment).expect("gzip");
            // Keep the plain file too: the reader selects gzip, but both must
            // carry the copied identity if the selection changes later.
            std::fs::write(
                segment.with_extension("jsonl.gz"),
                encoder.finish().expect("gzip"),
            )
            .expect("write gzip");
        }
        let snapshot = store
            .state_dir(&group.group_id)
            .expect("state")
            .join("ledger/snapshot.latest.json");
        let original_snapshot = std::fs::read(&snapshot).expect("snapshot");
        let (package, _, _) = group_copy::export(&store, &group.group_id).expect("export");
        let imported =
            group_copy::import(&store, &package, &repo.to_string_lossy(), "").expect("import");
        assert_ne!(imported.group_id, group.group_id);
        assert_ne!(imported.active_scope_key, "s_original");
        let page = user_requests::page(&home, &imported.group_id, None, 20).expect("pending page");
        assert_eq!(page.total, 1);
        assert_eq!(page.events[0].id, event.id);
        assert_eq!(page.events[0].group_id, imported.group_id);
        assert_eq!(page.events[0].scope_key, imported.active_scope_key);

        let target = store.group_dir(&imported.group_id).expect("target");
        assert_eq!(
            std::fs::read(target.join("state/ledger/snapshot.latest.json")).expect("snapshot"),
            original_snapshot
        );
        let name = segment.file_name().expect("segment name");
        let copied_plain = target.join("state/ledger/segments").join(name);
        let plain = std::fs::read(&copied_plain).expect("copied plain segment");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&copied_plain)
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o077,
                0,
                "rewritten history must not become accessible to other users"
            );
        }
        let copied_event: Event = serde_json::from_slice(
            plain
                .split(|byte| *byte == b'\n')
                .find(|line| {
                    serde_json::from_slice::<Event>(line)
                        .is_ok_and(|candidate| candidate.id == event.id)
                })
                .expect("request line"),
        )
        .expect("event");
        assert_eq!(copied_event.group_id, imported.group_id);
        assert_eq!(copied_event.scope_key, imported.active_scope_key);
        if compressed {
            let mut decoded = Vec::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(copied_plain.with_extension("jsonl.gz"))
                        .expect("metadata")
                        .permissions()
                        .mode()
                        & 0o077,
                    0
                );
            }
            GzDecoder::new(
                std::fs::File::open(copied_plain.with_extension("jsonl.gz")).expect("gzip"),
            )
            .read_to_end(&mut decoded)
            .expect("decompress");
            assert_eq!(
                decoded, plain,
                "both physical representations were rewritten"
            );
        } else {
            // The two identities can change independently. Both must trigger
            // history rewriting, without altering the other identity.
            let group_only =
                group_copy::import(&store, &package, "", "").expect("copy with new group only");
            assert_ne!(group_only.group_id, group.group_id);
            assert_eq!(group_only.active_scope_key, "s_original");
            let page = user_requests::page(&home, &group_only.group_id, None, 20)
                .expect("group-only pending page");
            assert_eq!(page.events[0].group_id, group_only.group_id);
            assert_eq!(page.events[0].scope_key, "s_original");

            let target_home = HomeLayout::from_path(temp.path().join("scope-only-home"))
                .expect("scope-only home");
            let target_store = GroupStore::new(target_home.clone()).expect("scope-only store");
            let new_repo = temp.path().join("different-repo");
            std::fs::create_dir_all(&new_repo).expect("new repo");
            let scope_only =
                group_copy::import(&target_store, &package, &new_repo.to_string_lossy(), "")
                    .expect("copy with new scope only");
            assert_eq!(scope_only.group_id, group.group_id);
            assert_ne!(scope_only.active_scope_key, "s_original");
            let page = user_requests::page(&target_home, &scope_only.group_id, None, 20)
                .expect("scope-only pending page");
            assert_eq!(page.events[0].group_id, group.group_id);
            assert_eq!(page.events[0].scope_key, scope_only.active_scope_key);
        }
    }
}

#[test]
fn copied_history_is_byte_identical_when_identity_is_unchanged() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source_home = HomeLayout::from_path(temp.path().join("source")).expect("source home");
    let source_store = GroupStore::new(source_home.clone()).expect("source store");
    let source = source_store.create("source", "").expect("group");
    ledger::append(
        &source_store.ledger_path(&source.group_id).expect("ledger"),
        &Event::new("group.create", &source.group_id),
    )
    .expect("append");
    let segment = ledger_archive::compact(&source_home, &source.group_id, "fixture")
        .expect("compact")
        .expect("segment");
    let original = std::fs::read(&segment).expect("segment");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&original).expect("gzip");
    let mut gzip = encoder.finish().expect("gzip");
    let mut extra = GzEncoder::new(Vec::new(), Compression::default());
    extra.write_all(b"extra member").expect("gzip extra member");
    gzip.extend(extra.finish().expect("gzip extra member"));
    std::fs::write(segment.with_extension("jsonl.gz"), &gzip).expect("write gzip");
    let (package, _, _) = group_copy::export(&source_store, &source.group_id).expect("export");
    let target_home = HomeLayout::from_path(temp.path().join("target")).expect("target home");
    let target_store = GroupStore::new(target_home).expect("target store");
    let imported = group_copy::import(&target_store, &package, "", "").expect("import");
    assert_eq!(imported.group_id, source.group_id);
    let copied = target_store
        .group_dir(&imported.group_id)
        .expect("target")
        .join("state/ledger/segments")
        .join(segment.file_name().expect("segment name"));
    assert_eq!(std::fs::read(&copied).expect("plain"), original);
    assert_eq!(
        std::fs::read(copied.with_extension("jsonl.gz")).expect("gzip"),
        gzip
    );
}

#[test]
fn active_history_uses_the_same_scoped_rewrite_as_archived_history() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let source = store.create("source", "").expect("group");
    group_scope::attach(
        &store,
        &source.group_id,
        Scope {
            scope_key: "s_original".into(),
            url: repo.to_string_lossy().into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let history = format!(
        "{{\"data\":{{\"group_id\":\"{}\",\"scope_key\":\"s_original\"}},\"group_id\":\"{}\",\"scope_key\":\"s_original\"}}\r\n{{\"group_id\":\"other\",\"scope_key\":\"s_original\"}}\n{{\"scope_key\":\"s_original\"}}",
        source.group_id, source.group_id
    );
    std::fs::write(
        store.ledger_path(&source.group_id).expect("ledger"),
        &history,
    )
    .expect("history");
    let (package, _, _) = group_copy::export(&store, &source.group_id).expect("export");
    let copied = group_copy::import(&store, &package, &repo.to_string_lossy(), "")
        .expect("import with new identity");
    let actual = std::fs::read(store.ledger_path(&copied.group_id).expect("copied ledger"))
        .expect("read copied ledger");
    let expected_prefix = format!(
        "{{\"data\":{{\"group_id\":\"{}\",\"scope_key\":\"s_original\"}},\"group_id\":\"{}\",\"scope_key\":\"{}\"}}\r\n{{\"group_id\":\"other\",\"scope_key\":\"s_original\"}}\n{{\"scope_key\":\"s_original\"}}\n",
        source.group_id, copied.group_id, copied.active_scope_key
    );
    assert!(actual.starts_with(expected_prefix.as_bytes()));
    let marker: Event = serde_json::from_slice(&actual[expected_prefix.len()..]).expect("marker");
    assert_eq!(marker.group_id, copied.group_id);
    assert_eq!(marker.data.get("imported"), Some(&serde_json::json!(true)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(store.ledger_path(&copied.group_id).expect("path"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }

    let target_home = HomeLayout::from_path(temp.path().join("other-home")).expect("target home");
    let target = GroupStore::new(target_home).expect("target store");
    let unchanged = group_copy::import(&target, &package, "", "").expect("unchanged identity");
    let actual = std::fs::read(target.ledger_path(&unchanged.group_id).expect("path"))
        .expect("read unchanged ledger");
    assert!(actual.starts_with(history.as_bytes()));
    assert_eq!(actual.get(history.len()), Some(&b'\n'));
}

#[test]
fn unparseable_active_history_is_retained_and_remains_unreadable_to_strict_projections() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let source = store.create("source", "").expect("group");
    let damaged = b"invalid \xff\n";
    std::fs::write(
        store.ledger_path(&source.group_id).expect("ledger"),
        damaged,
    )
    .expect("history");
    let (package, _, _) = group_copy::export(&store, &source.group_id).expect("export");
    let copied = group_copy::import(&store, &package, "", "").expect("retain damaged history");
    let actual = std::fs::read(store.ledger_path(&copied.group_id).expect("path"))
        .expect("read copied history");
    assert!(actual.starts_with(damaged));
    assert!(user_requests::page(&home, &source.group_id, None, 20).is_err());
    assert!(user_requests::page(&home, &copied.group_id, None, 20).is_err());
    assert!(
        std::fs::read_dir(home.groups_dir())
            .expect("groups")
            .all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".importing-"))
    );
}

#[test]
fn a_multi_member_history_segment_fails_before_the_copy_is_published() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let source = store.create("source", "").expect("group");
    ledger::append(
        &store.ledger_path(&source.group_id).expect("ledger"),
        &Event::new("group.create", &source.group_id),
    )
    .expect("append");
    let segment = ledger_archive::compact(&home, &source.group_id, "fixture")
        .expect("compact")
        .expect("segment");
    let history = std::fs::read(&segment).expect("history");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&history).expect("first member");
    let mut gzip = encoder.finish().expect("finish");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&history).expect("second member");
    gzip.extend(encoder.finish().expect("finish"));
    std::fs::write(segment.with_extension("jsonl.gz"), gzip).expect("gzip");
    std::fs::remove_file(&segment).expect("remove plain");
    let (package, _, _) = group_copy::export(&store, &source.group_id).expect("export");
    let before = store.list().expect("before");
    let error = group_copy::import(&store, &package, "", "")
        .expect_err("multi-member gzip must not be changed into newly visible history");
    assert!(error.to_string().contains("more than one member"));
    assert_eq!(store.list().expect("after").len(), before.len());
    assert!(
        std::fs::read_dir(home.groups_dir())
            .expect("groups")
            .all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".importing-")),
        "failed staging is removed"
    );
}

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
            doc.extra.insert(
                "im_bridge".into(),
                serde_json::json!({
                    "config": {
                        "platform": "slack",
                        "bot_token_env": "must-not-export-bot-token",
                        "app_token_env": "must-not-export-app-token"
                    },
                    "enabled": true,
                    "running": true
                }),
            );
            doc.extra.insert(
                "im".into(),
                serde_json::json!({"platform":"telegram","token":"must-not-export-legacy"}),
            );
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
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("export archive");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("archive entry");
        if entry.is_dir() {
            continue;
        }
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).expect("archive contents");
        for secret in [
            "must-not-export-bot-token",
            "must-not-export-app-token",
            "must-not-export-legacy",
        ] {
            assert!(
                !contents
                    .windows(secret.len())
                    .any(|bytes| bytes == secret.as_bytes())
            );
        }
    }
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
    assert!(
        doc.actors[0].env.is_empty(),
        "Python group copies clear all actor env values"
    );
    assert!(!doc.extra.contains_key("im_bridge"));
    assert!(!doc.extra.contains_key("im"));
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

#[test]
fn import_treats_a_registered_group_being_deleted_as_an_id_conflict() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("deleting", "").expect("group");
    let (package, _, _) = group_copy::export(&store, &group.group_id).expect("export");
    // Deletion renames the directory before unregistering the Group.
    let tombstone = home
        .groups_dir()
        .join(format!(".{}.deleting-test", group.group_id));
    std::fs::rename(store.group_dir(&group.group_id).expect("path"), &tombstone)
        .expect("simulate deletion window");

    assert!(
        group_copy::preview(&store, &package)
            .expect("preview")
            .group_id_conflict
    );
    let imported = group_copy::import(&store, &package, "", "").expect("import");
    assert!(imported.group_id_conflict);
    assert_ne!(imported.group_id, group.group_id);
    assert!(tombstone.is_dir());
    let registered = store.list().expect("registry");
    assert!(
        registered
            .iter()
            .any(|meta| meta.group_id == group.group_id)
    );
    assert!(
        registered
            .iter()
            .any(|meta| meta.group_id == imported.group_id)
    );
}

#[test]
fn import_chooses_a_new_id_when_an_unpublished_directory_is_retained() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source_home = HomeLayout::from_path(temp.path().join("source")).expect("source home");
    let source_store = GroupStore::new(source_home).expect("source store");
    let source = source_store.create("source", "").expect("source group");
    let (package, _, _) = group_copy::export(&source_store, &source.group_id).expect("export");

    let target_home = HomeLayout::from_path(temp.path().join("target")).expect("target home");
    let target = GroupStore::new(target_home).expect("target store");
    let retained = target.group_dir(&source.group_id).expect("retained path");
    std::fs::create_dir_all(retained.join("state")).expect("retained directory");
    std::fs::write(retained.join("state/external.txt"), b"concurrent write").expect("old write");
    assert!(
        group_copy::preview(&target, &package)
            .expect("preview")
            .group_id_conflict
    );
    let imported = group_copy::import(&target, &package, "", "").expect("import with new id");
    assert!(imported.group_id_conflict);
    assert_ne!(imported.group_id, source.group_id);
    assert_eq!(
        std::fs::read(retained.join("state/external.txt")).expect("old write retained"),
        b"concurrent write"
    );
    assert!(target.load(&imported.group_id).is_ok());
}
