use cccc_core::context::{ContextDoc, ContextStore};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

fn op(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("operation")
}

#[test]
fn legacy_migration_preserves_resolved_canonical_relationships() {
    for (identity, retired_reference) in [
        ("request", false),
        ("creation", false),
        ("unchanged", false),
        ("request", true),
    ] {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let group = groups.create("migration collision", "").expect("group");
        let group_dir = groups.group_dir(&group.group_id).expect("group dir");
        let directory = group_dir.join("context");
        let mut owner = json!({"id":"T001", "title":"owner", "parent_id":"T002",
            "blocked_by":["T002"], "status":"active", "notes":"canonical"});
        if retired_reference {
            // Deletion can leave an unrelated, user-owned reference behind.
            owner["blocked_by"] = json!(["T002", "T003"]);
        }
        match identity {
            "request" => owner["client_request_id"] = json!("owner-request"),
            "creation" => {
                owner["created_at"] = json!("2026-10-10T00:00:00Z");
                owner["created_by"] = json!("user");
            }
            _ => {}
        }
        let dependency = json!({"id":"T002", "title":"canonical dependency"});
        for task in [
            owner.clone(),
            dependency.clone(),
            json!({"id":"T004", "title":"other"}),
        ] {
            let id = task["id"].as_str().expect("id");
            cccc_core::fs::write_yaml(&directory.join(format!("tasks/{id}.yaml")), &task)
                .expect("canonical task");
        }
        let owner_path = directory.join("tasks/T001.yaml");
        let dependency_path = directory.join("tasks/T002.yaml");
        let owner_bytes = std::fs::read(&owner_path).expect("owner bytes");
        let dependency_bytes = std::fs::read(&dependency_path).expect("dependency bytes");
        let mut legacy_owner = owner.clone();
        legacy_owner["blocked_by"] = json!(["T002"]);
        let legacy = ContextDoc {
            tasks: vec![
                op(legacy_owner),
                op(json!({"id":"T002", "title":"unrelated legacy dependency"})),
                op(
                    json!({"id":"T003", "title":"legacy child", "parent_id":"T001", "blocked_by":["T002"]}),
                ),
            ],
            ..ContextDoc::default()
        };
        let legacy_path = group_dir.join("state/context.json");
        cccc_core::fs::write_json(&legacy_path, &legacy).expect("legacy file");
        let legacy_bytes = std::fs::read(&legacy_path).expect("legacy bytes");

        let contexts = ContextStore::new(home).expect("contexts");
        let actual = contexts.load(&group.group_id).expect("migration");
        let find = |id: &str| {
            actual
                .tasks
                .iter()
                .find(|task| task["id"] == id)
                .expect("task")
        };
        assert_eq!(
            find("T001"),
            owner.as_object().expect("owner"),
            "{identity}"
        );
        assert_eq!(find("T002"), dependency.as_object().expect("dependency"));
        assert_eq!(find("T005")["title"], "unrelated legacy dependency");
        assert_eq!(find("T006")["parent_id"], "T001");
        assert_eq!(find("T006")["blocked_by"], json!(["T005"]));
        assert_eq!(
            std::fs::read(&owner_path).expect("owner remains"),
            owner_bytes
        );
        assert_eq!(
            std::fs::read(&dependency_path).expect("dependency remains"),
            dependency_bytes
        );
        assert_eq!(
            std::fs::read(&legacy_path).expect("legacy remains"),
            legacy_bytes
        );
        assert_eq!(
            contexts.load(&group.group_id).expect("reload").tasks,
            actual.tasks
        );
        let marker: Value =
            cccc_core::fs::read_json(&directory.join(".rust-state-migrated-v1.json"))
                .expect("migration receipt");
        assert_eq!(marker["task_id_mappings"]["T002"], "T005");
    }
}

#[test]
fn unreadable_canonical_context_is_not_treated_as_empty_or_overwritten() {
    let mut failures = Vec::new();
    for (relative, change) in [
        (
            "context.yaml",
            json!({"op":"coordination.brief.update", "objective":"replacement"}),
        ),
        (
            "tasks/T001.yaml",
            json!({"op":"task.create", "title":"replacement"}),
        ),
        (
            "agents.yaml",
            json!({"op":"agent_state.update", "actor_id":"peer", "focus":"replacement"}),
        ),
        (
            "version_state.json",
            json!({"op":"coordination.brief.update", "objective":"replacement"}),
        ),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let group = groups.create("storage", "").expect("group");
        let contexts = ContextStore::new(home).expect("contexts");
        contexts
            .sync(
                &group.group_id,
                &[
                    op(json!({"op":"coordination.brief.update", "objective":"original"})),
                    op(json!({"op":"task.create", "title":"original"})),
                    op(json!({"op":"agent_state.update", "actor_id":"lead", "focus":"original"})),
                ],
                None,
                "user",
                false,
            )
            .expect("seed context");
        let path = groups
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("context")
            .join(relative);
        let malformed = b"[interrupted document";
        std::fs::write(&path, malformed).expect("corrupt isolated fixture");
        let rejected_read = contexts.load(&group.group_id).is_err();
        let rejected_write = contexts
            .sync(&group.group_id, &[op(change)], None, "user", false)
            .is_err();
        let preserved = std::fs::read(&path).expect("source remains") == malformed;
        if !(rejected_read && rejected_write && preserved) {
            failures.push(format!("{relative}: read_error={rejected_read}, write_error={rejected_write}, preserved={preserved}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn malformed_context_shapes_and_identity_metadata_are_not_discarded() {
    for (relative, text) in [
        ("context.yaml", "[]"),
        ("context.yaml", "coordination: []"),
        ("context.yaml", "meta: null"),
        ("tasks/T001.yaml", "id: T002\ntitle: mismatched"),
        ("agents.yaml", "agent_states: {}"),
        ("agents.yaml", "agent_states: [null]"),
        (
            "agents.yaml",
            "agent_states: [{actor_id: peer}, {actor_id: peer}]",
        ),
        ("version_state.json", "{}"),
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let group = groups.create("structural corruption", "").expect("group");
        let path = groups
            .group_dir(&group.group_id)
            .expect("group dir")
            .join("context")
            .join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
        std::fs::write(&path, text).expect("fixture");
        let contexts = ContextStore::new(home).expect("contexts");
        assert!(
            contexts.load(&group.group_id).is_err(),
            "{relative}: {text}"
        );
        assert!(
            contexts
                .sync(
                    &group.group_id,
                    &[op(json!({"op":"task.create","title":"replacement"}))],
                    None,
                    "user",
                    false
                )
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(path).expect("preserved"), text);
    }
}

#[test]
fn task_numbers_survive_deletion_reload_and_rejected_or_dry_run_batches() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("task identity", "").expect("group");
    let contexts = ContextStore::new(home.clone()).expect("contexts");
    let create = |title: &str| op(json!({"op":"task.create", "title":title}));
    contexts
        .sync(&group.group_id, &[create("parent")], None, "user", false)
        .expect("parent");
    contexts
        .sync(
            &group.group_id,
            &[op(
                json!({"op":"task.create", "title":"child", "parent_id":"T001"}),
            )],
            None,
            "user",
            false,
        )
        .expect("child");
    contexts
        .sync(
            &group.group_id,
            &[op(json!({"op":"task.delete", "task_id":"T001"}))],
            None,
            "user",
            false,
        )
        .expect("delete subtree");
    assert!(
        contexts
            .load(&group.group_id)
            .expect("deleted")
            .tasks
            .is_empty()
    );
    let contexts = ContextStore::new(home).expect("reload store");
    let before = contexts.load(&group.group_id).expect("reloaded");
    contexts
        .sync(&group.group_id, &[create("dry")], None, "user", true)
        .expect("dry run");
    assert_eq!(contexts.load(&group.group_id).expect("dry state"), before);
    assert!(
        contexts
            .sync(
                &group.group_id,
                &[create("discarded"), op(json!({"op":"unknown"}))],
                None,
                "user",
                false,
            )
            .is_err()
    );
    assert_eq!(
        contexts.load(&group.group_id).expect("rejected state"),
        before
    );
    let result = contexts
        .sync(
            &group.group_id,
            &[create("replacement")],
            None,
            "user",
            false,
        )
        .expect("replacement");
    assert_eq!(result.context.tasks[0]["id"], "T003");
    assert!(
        contexts
            .sync(
                &group.group_id,
                &[op(
                    json!({"op":"task.update", "task_id":"T001", "notes":"late"})
                )],
                None,
                "user",
                false,
            )
            .is_err()
    );
    assert!(!contexts.load(&group.group_id).expect("unchanged").tasks[0].contains_key("notes"));
}

#[test]
fn existing_task_numbers_seed_the_durable_sequence_before_last_task_deletion() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("existing tasks", "").expect("group");
    let directory = groups
        .group_dir(&group.group_id)
        .expect("directory")
        .join("context");
    std::fs::create_dir_all(directory.join("tasks")).expect("tasks");
    cccc_core::fs::write_yaml(
        &directory.join("tasks/T042.yaml"),
        &json!({"id":"T042", "title":"existing", "status":"planned"}),
    )
    .expect("existing task");
    let contexts = ContextStore::new(home.clone()).expect("contexts");
    contexts
        .sync(
            &group.group_id,
            &[op(json!({"op":"task.delete", "task_id":"T042"}))],
            None,
            "user",
            false,
        )
        .expect("delete existing task");
    let result = ContextStore::new(home)
        .expect("reloaded store")
        .sync(
            &group.group_id,
            &[op(json!({"op":"task.create", "title":"after deletion"}))],
            None,
            "user",
            false,
        )
        .expect("new task");
    assert_eq!(result.context.tasks[0]["id"], "T043");
}

#[test]
fn accepted_create_delete_batch_still_retires_its_task_number() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("batch identity", "")
        .expect("group");
    ContextStore::new(home.clone())
        .expect("contexts")
        .sync(
            &group.group_id,
            &[
                op(json!({"op":"task.create", "title":"temporary"})),
                op(json!({"op":"task.delete", "task_id":"T001"})),
            ],
            None,
            "user",
            false,
        )
        .expect("accepted batch");
    let contexts = ContextStore::new(home).expect("reloaded store");
    assert!(
        contexts
            .load_overview(&group.group_id)
            .expect("overview")
            .tasks
            .is_empty()
    );
    let result = contexts
        .sync(
            &group.group_id,
            &[op(json!({"op":"task.create", "title":"replacement"}))],
            None,
            "user",
            false,
        )
        .expect("replacement");
    assert_eq!(result.context.tasks[0]["id"], "T002");
}
