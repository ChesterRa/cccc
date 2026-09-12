use cccc_core::context::ContextStore;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

fn op(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("operation")
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
