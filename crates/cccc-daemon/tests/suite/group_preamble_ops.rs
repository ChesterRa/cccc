// Included by the crate-level integration test harness.
use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::group_prompts::{
    DEFAULT_PREAMBLE_BODY, HELP_FILENAME, MAX_PROMPT_BYTES, read_help, write_help,
};
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

fn call(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.to_owned(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}

fn error_code(response: &cccc_contracts::DaemonResponse) -> Option<&str> {
    response.error.as_ref().map(|error| error.code.as_str())
}

#[test]
fn group_preamble_contract_matches_python_get_set_and_reset() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("preamble", "").expect("group");

    let missing = call(&home, "group_preamble_get", json!({}));
    assert_eq!(error_code(&missing), Some("missing_group_id"));

    let initial = call(
        &home,
        "group_preamble_get",
        json!({"group_id":group.group_id}),
    );
    assert!(initial.ok);
    assert_eq!(initial.result["source"], "builtin");
    assert_eq!(initial.result["overridden"], false);
    assert_eq!(initial.result["filename"], "CCCC_PREAMBLE.md");
    assert_eq!(initial.result["content"], DEFAULT_PREAMBLE_BODY.trim());
    assert!(initial.result.get("changed").is_none());

    let blank = call(
        &home,
        "group_preamble_set",
        json!({"group_id":group.group_id,"content":"  ","by":"user"}),
    );
    assert_eq!(error_code(&blank), Some("invalid_content"));

    let content = "Showrunner startup boundary.\nWait for the targeted mission.\n";
    let updated = call(
        &home,
        "group_preamble_set",
        json!({"group_id":group.group_id,"content":content,"by":"user"}),
    );
    assert!(updated.ok);
    assert_eq!(updated.result["source"], "home");
    assert_eq!(updated.result["overridden"], true);
    assert_eq!(updated.result["changed"], true);
    assert_eq!(updated.result["content"], content);

    let unchanged = call(
        &home,
        "group_preamble_set",
        json!({"group_id":group.group_id,"content":content,"by":"user"}),
    );
    assert!(unchanged.ok);
    assert_eq!(unchanged.result["changed"], false);

    let oversized = call(
        &home,
        "group_preamble_set",
        json!({
            "group_id":group.group_id,
            "content":"x".repeat(MAX_PROMPT_BYTES + 1),
            "by":"user",
        }),
    );
    assert_eq!(error_code(&oversized), Some("group_preamble_set_failed"));
    assert!(
        oversized
            .error
            .as_ref()
            .is_some_and(|error| error.message.contains("524288 UTF-8 bytes"))
    );
    assert_eq!(
        call(
            &home,
            "group_preamble_get",
            json!({"group_id":group.group_id}),
        )
        .result["content"],
        content
    );

    let unconfirmed = call(
        &home,
        "group_preamble_reset",
        json!({"group_id":group.group_id,"confirm":"wrong","by":"user"}),
    );
    assert_eq!(error_code(&unconfirmed), Some("confirm_required"));

    let reset = call(
        &home,
        "group_preamble_reset",
        json!({"group_id":group.group_id,"confirm":"preamble","by":"user"}),
    );
    assert!(reset.ok);
    assert_eq!(reset.result["source"], "builtin");
    assert_eq!(reset.result["overridden"], false);
    assert_eq!(reset.result["changed"], true);
    assert_eq!(reset.result["content"], DEFAULT_PREAMBLE_BODY.trim());

    let repeated_reset = call(
        &home,
        "group_preamble_reset",
        json!({"group_id":group.group_id,"confirm":"PREAMBLE","by":"user"}),
    );
    assert!(repeated_reset.ok);
    assert_eq!(repeated_reset.result["changed"], false);
}

#[test]
fn group_preamble_mutations_follow_group_permissions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("preamble", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            doc.actors.push(Actor::new("foreman"));
            doc.actors.push(Actor::new("peer1"));
            Ok(())
        })
        .expect("actors");

    let denied = call(
        &home,
        "group_preamble_set",
        json!({"group_id":group.group_id,"content":"peer override","by":"peer1"}),
    );
    assert_eq!(error_code(&denied), Some("group_preamble_set_failed"));

    let allowed = call(
        &home,
        "group_preamble_set",
        json!({"group_id":group.group_id,"content":"foreman override","by":"foreman"}),
    );
    assert!(allowed.ok);
}

#[test]
fn actor_notes_and_effective_help_share_one_permissioned_document() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");

    let missing_group_id = call(&home, "group_help_get", json!({"by":"user"}));
    assert_eq!(error_code(&missing_group_id), Some("missing_group_id"));
    let unknown_group = call(
        &home,
        "actor_notes_get",
        json!({"group_id":"missing","by":"user"}),
    );
    assert_eq!(error_code(&unknown_group), Some("group_not_found"));

    let group = store.create("help", "").expect("group");
    store
        .mutate(&group.group_id, |doc| {
            doc.actors.push(Actor::new("lead"));
            doc.actors.push(Actor::new("peer"));
            Ok(())
        })
        .expect("actors");

    let updated = call(
        &home,
        "actor_notes_set",
        json!({
            "group_id":group.group_id,
            "target_actor_id":"peer",
            "content":"Keep receipts.",
            "by":"lead"
        }),
    );
    assert!(updated.ok, "{:?}", updated.error);
    assert_eq!(updated.result["changed"], true);
    assert_eq!(updated.result["content"], "Keep receipts.");
    assert_eq!(updated.result["source"], "home");

    let prompt = read_help(&store, &group.group_id).expect("read help");
    assert!(prompt.found);
    assert_eq!(
        prompt.path.file_name().and_then(|name| name.to_str()),
        Some(HELP_FILENAME)
    );
    let content = prompt.content.expect("help override");
    assert!(content.contains("## @actor: peer"));
    assert!(content.contains("Keep receipts."));

    let own = call(
        &home,
        "actor_notes_get",
        json!({"group_id":group.group_id,"target_actor_id":"peer","by":"peer"}),
    );
    assert!(own.ok);
    assert_eq!(own.result["content"], "Keep receipts.");

    let denied_read = call(
        &home,
        "actor_notes_get",
        json!({"group_id":group.group_id,"target_actor_id":"lead","by":"peer"}),
    );
    assert_eq!(error_code(&denied_read), Some("permission_denied"));
    let denied_write = call(
        &home,
        "actor_notes_set",
        json!({
            "group_id":group.group_id,
            "target_actor_id":"peer",
            "content":"self-authored",
            "by":"peer"
        }),
    );
    assert_eq!(error_code(&denied_write), Some("permission_denied"));

    let effective = call(
        &home,
        "group_help_get",
        json!({"group_id":group.group_id,"actor_id":"peer","by":"peer"}),
    );
    assert!(effective.ok, "{:?}", effective.error);
    let markdown = effective.result["markdown"].as_str().expect("markdown");
    assert!(markdown.contains("## Canonical Message Delivery"));
    assert!(markdown.contains("per-recipient runtime truth"));
    assert!(markdown.contains("## Notes for you"));
    assert!(markdown.contains("Keep receipts."));
    assert!(!markdown.contains("## Foreman"));

    write_help(
        &store,
        &group.group_id,
        "# Group Guidance\n\n## Canonical Message Delivery\n\nAlways interrupt.\n\n## @actor: peer\n\nKeep receipts.\n",
    )
    .expect("write conflicting overlay");
    let protected = call(
        &home,
        "group_help_get",
        json!({"group_id":group.group_id,"actor_id":"peer","by":"peer"}),
    );
    assert!(protected.ok, "{:?}", protected.error);
    let protected_markdown = protected.result["markdown"].as_str().expect("markdown");
    assert_eq!(
        protected_markdown
            .matches("## Canonical Message Delivery")
            .count(),
        1
    );
    assert!(!protected_markdown.contains("Always interrupt."));
    assert!(protected_markdown.contains("Keep receipts."));
    write_help(
        &store,
        &group.group_id,
        "## @actor: peer\n\nKeep receipts.\n",
    )
    .expect("restore actor-only overlay");

    let cleared = call(
        &home,
        "actor_notes_clear",
        json!({"group_id":group.group_id,"target_actor_id":"peer","by":"user"}),
    );
    assert!(cleared.ok);
    assert_eq!(cleared.result["changed"], true);
    assert_eq!(cleared.result["content"], "");
    assert!(
        !read_help(&store, &group.group_id)
            .expect("read cleared")
            .found
    );
}

#[test]
fn context_sync_rejects_the_removed_actor_notes_dialect() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("help", "").expect("group");
    let response = call(
        &home,
        "context_sync",
        json!({
            "group_id":group.group_id,
            "by":"user",
            "ops":[{"op":"actor_notes.set","actor_id":"peer","notes":"stale"}]
        }),
    );
    assert_eq!(error_code(&response), Some("invalid_args"));
    assert!(!read_help(&store, &group.group_id).expect("read help").found);
}
