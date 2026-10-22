use cccc_core::{GroupStore, HomeLayout, context::ContextStore};
use serde_json::{Map, Value, json};

fn operation(value: Value) -> Map<String, Value> {
    value.as_object().expect("op").clone()
}
fn fixture() -> (tempfile::TempDir, ContextStore, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("task revisions", "").expect("group");
    let directory = groups
        .group_dir(&group.group_id)
        .expect("group dir")
        .join("context/tasks");
    let tasks: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/task-revision/tasks.json")).expect("recording");
    for task in tasks {
        cccc_core::fs::write_yaml(
            &directory.join(format!("{}.yaml", task["id"].as_str().expect("id"))),
            &task,
        )
        .expect("recorded task");
    }
    (
        temp,
        ContextStore::new(home).expect("store"),
        group.group_id,
    )
}
fn revision(store: &ContextStore, group: &str, id: &str) -> u64 {
    store
        .load(group)
        .expect("load")
        .tasks
        .iter()
        .find(|t| t["id"] == id)
        .expect("task")["rev"]
        .as_u64()
        .expect("revision")
}

#[test]
fn task_revision_fences_recorded_task_without_conflicting_on_other_context_writes() {
    let (_temp, store, group) = fixture();
    assert_eq!(revision(&store, &group, "T792"), 0);
    let version = store
        .version(&store.load(&group).expect("load"))
        .expect("version");
    store.sync(&group,&[operation(json!({"op":"agent_state.update","actor_id":"peer","focus":"revision check"})),operation(json!({"op":"task.update","task_id":"T898","waiting_on":"user","if_task_rev":0}))],None,"user",false).expect("unrelated changes");
    let change =
        operation(json!({"op":"task.update","task_id":"T792","waiting_on":"none","if_task_rev":0}));
    assert!(
        store
            .sync(
                &group,
                std::slice::from_ref(&change),
                Some(&version),
                "user",
                false
            )
            .is_err(),
        "context guard remains independent"
    );
    store
        .sync(&group, std::slice::from_ref(&change), None, "user", true)
        .expect("preview");
    assert_eq!(revision(&store, &group, "T792"), 0);
    store
        .sync(&group, std::slice::from_ref(&change), None, "user", false)
        .expect("fresh task guard");
    assert_eq!(revision(&store, &group, "T792"), 1);
    let before = store.load(&group).expect("before");
    for dry in [true, false] {
        let error = store
            .sync(&group, std::slice::from_ref(&change), None, "user", dry)
            .expect_err("stale task");
        assert!(
            error.to_string().contains("T792"),
            "conflict must name the task: {error}"
        );
        assert_eq!(store.load(&group).expect("unchanged").tasks, before.tasks);
    }
}

#[test]
fn task_revision_batch_reads_prior_revisions_and_rejects_atomically() {
    let (_temp, store, group) = fixture();
    let before = store.load(&group).expect("before");
    let first =
        operation(json!({"op":"task.update","task_id":"T792","waiting_on":"none","if_task_rev":0}));
    let stale =
        operation(json!({"op":"task.move","task_id":"T792","status":"active","if_task_rev":0}));
    assert!(
        store
            .sync(&group, &[first.clone(), stale], None, "user", false)
            .is_err()
    );
    assert_eq!(store.load(&group).expect("unchanged").tasks, before.tasks);
    store
        .sync(
            &group,
            &[
                first,
                operation(
                    json!({"op":"task.move","task_id":"T792","status":"active","if_task_rev":1}),
                ),
            ],
            None,
            "user",
            false,
        )
        .expect("sequential guards");
    assert_eq!(revision(&store, &group, "T792"), 2);
    store
        .sync(
            &group,
            &[operation(
                json!({"op":"task.move","task_id":"T792","status":"active","if_task_rev":2}),
            )],
            None,
            "user",
            false,
        )
        .expect("no-op");
    assert_eq!(revision(&store, &group, "T792"), 2);
}

#[test]
fn task_revision_covers_create_restore_delete_and_rejects_invalid_guards() {
    let (_temp, store, group) = fixture();
    for invalid in [json!(-1), json!("0"), Value::Null] {
        assert!(
            store
                .sync(
                    &group,
                    &[operation(
                        json!({"op":"task.update","task_id":"T792","if_task_rev":invalid})
                    )],
                    None,
                    "user",
                    false
                )
                .is_err()
        );
    }
    assert!(
        store
            .sync(
                &group,
                &[operation(
                    json!({"op":"task.create","title":"revision","if_task_rev":0})
                )],
                None,
                "user",
                false
            )
            .is_err()
    );
    assert!(
        store
            .sync(
                &group,
                &[operation(
                    json!({"op":"task.update","task_id":"T792","rev":42})
                )],
                None,
                "user",
                false
            )
            .is_err()
    );
    store
        .sync(
            &group,
            &[operation(json!({"op":"task.create","title":"revision"}))],
            None,
            "user",
            false,
        )
        .expect("create");
    assert_eq!(revision(&store, &group, "T899"), 1);
    store
        .sync(
            &group,
            &[
                operation(
                    json!({"op":"task.move","task_id":"T899","status":"archived","if_task_rev":1}),
                ),
                operation(json!({"op":"task.restore","task_id":"T899","if_task_rev":2})),
            ],
            None,
            "user",
            false,
        )
        .expect("restore");
    assert_eq!(revision(&store, &group, "T899"), 3);
    assert!(
        store
            .sync(
                &group,
                &[operation(
                    json!({"op":"task.delete","task_id":"T899","if_task_rev":2})
                )],
                None,
                "user",
                false
            )
            .is_err()
    );
    store
        .sync(
            &group,
            &[
                operation(json!({"op":"task.delete","task_id":"T899","if_task_rev":3})),
                operation(json!({"op":"task.create","title":"next revision"})),
            ],
            None,
            "user",
            false,
        )
        .expect("delete then create");
    assert_eq!(revision(&store, &group, "T900"), 1);
    assert!(
        store
            .load(&group)
            .expect("load")
            .tasks
            .iter()
            .all(|t| t["id"] != "T899")
    );
}

#[test]
fn task_revision_read_is_non_mutating_and_malformed_or_exhausted_revisions_fail_closed() {
    let (temp, store, group) = fixture();
    let path = temp
        .path()
        .join("groups")
        .join(&group)
        .join("context/tasks/T792.yaml");
    let original = std::fs::read(&path).expect("recorded bytes");
    assert_eq!(revision(&store, &group, "T792"), 0);
    assert_eq!(std::fs::read(&path).expect("unchanged bytes"), original);
    let mut task: Value = cccc_core::fs::read_yaml(&path).expect("task");
    task["rev"] = json!(u64::MAX);
    cccc_core::fs::write_yaml(&path, &task).expect("exhausted fault injection");
    let bytes = std::fs::read(&path).expect("bytes");
    assert!(store.sync(&group,&[operation(json!({"op":"task.update","task_id":"T792","waiting_on":"none","if_task_rev":u64::MAX}))],None,"user",false).is_err());
    assert_eq!(std::fs::read(&path).expect("unchanged"), bytes);
    task["rev"] = json!("broken");
    cccc_core::fs::write_yaml(&path, &task).expect("malformed fault injection");
    assert!(store.load(&group).is_err());
}

#[test]
fn task_revision_legacy_migration_assigns_revisions_and_preserves_source() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("legacy revisions", "").expect("group");
    let tasks: Vec<Map<String, Value>> =
        serde_json::from_str(include_str!("fixtures/task-revision/tasks.json")).expect("recording");
    let legacy = cccc_core::context::ContextDoc {
        tasks,
        ..Default::default()
    };
    let path = groups
        .group_dir(&group.group_id)
        .expect("group dir")
        .join("state/context.json");
    cccc_core::fs::write_json(&path, &legacy).expect("legacy context");
    let bytes = std::fs::read(&path).expect("source bytes");
    let store = ContextStore::new(home).expect("store");
    let migrated = store.load(&group.group_id).expect("migration");
    assert_eq!(migrated.tasks.len(), 3);
    assert!(migrated.tasks.iter().all(|task| task["rev"] == 1));
    assert_eq!(std::fs::read(&path).expect("source unchanged"), bytes);
    assert_eq!(
        store.load(&group.group_id).expect("reload").tasks,
        migrated.tasks
    );
}
