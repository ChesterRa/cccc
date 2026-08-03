#![cfg(unix)]

use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn idle_resident_peer_is_reused_even_with_a_nonterminal_task() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(
        &home,
        "group_create",
        json!({"title":"elastic reuse","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"lead",
            "runner":"pty",
            "runtime":"custom",
            "command":["sh","-c","sleep 30"],
            "by":"user"
        }),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"idle-peer",
            "runner":"headless",
            "runtime":"custom",
            "by":"user"
        }),
    );
    call(
        &home,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"idle-peer","by":"user"}),
    );
    call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"lead",
            "ops":[{"op":"task.create","title":"Stale task","status":"active","assignee":"idle-peer"}]
        }),
    );

    let dispatched = call(
        &home,
        "elastic_dispatch",
        json!({
            "group_id":group_id,
            "by":"lead",
            "title":"Fresh task",
            "text":"Handle the fresh task.",
            "outcome":"Return a verified result",
            "idempotency_key":"fresh-task"
        }),
    );

    assert_eq!(dispatched.result["actor_id"], "idle-peer");
    assert_eq!(dispatched.result["created"], false);
    assert_eq!(dispatched.result["elastic"], false);
}

#[test]
fn foreman_scales_out_and_releases_an_elastic_peer_after_acceptance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(
        &home,
        "group_create",
        json!({"title":"elastic","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"lead",
            "runner":"pty",
            "runtime":"custom",
            "command":["sh","-c","sleep 30"],
            "by":"user"
        }),
    );
    for actor_id in ["peer-1", "peer-2", "peer-3"] {
        call(
            &home,
            "actor_add",
            json!({
                "group_id":group_id,
                "actor_id":actor_id,
                "runner":"pty",
                "runtime":"custom",
                "command":["sh","-c","sleep 30"],
                "by":"user"
            }),
        );
    }
    call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"lead",
            "ops":[
                {"op":"task.create","title":"Busy 1","status":"active","assignee":"peer-1"},
                {"op":"task.create","title":"Busy 2","status":"active","assignee":"peer-2"},
                {"op":"task.create","title":"Busy 3","status":"active","assignee":"peer-3"}
            ]
        }),
    );

    let dispatched = call(
        &home,
        "elastic_dispatch",
        json!({
            "group_id":group_id,
            "by":"lead",
            "title":"Fourth task",
            "text":"Handle the independent fourth task.",
            "outcome":"Return a verified result",
            "idempotency_key":"fourth-task"
        }),
    );
    assert_eq!(dispatched.result["created"], true);
    assert_eq!(dispatched.result["elastic"], true);
    assert_eq!(dispatched.result["message_sent"], true);
    let actor_id = dispatched.result["actor_id"]
        .as_str()
        .expect("actor id")
        .to_owned();
    let task_id = dispatched.result["task_id"]
        .as_str()
        .expect("task id")
        .to_owned();
    assert_eq!(task_id, "T004");
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"lead"}),
    );
    let elastic = actors.result["actors"]
        .as_array()
        .and_then(|actors| actors.iter().find(|actor| actor["id"] == actor_id))
        .expect("elastic actor");
    assert_eq!(elastic["elastic_lease"]["owner_actor_id"], "lead");
    assert_eq!(elastic["elastic_lease"]["task_id"], task_id);

    call(
        &home,
        "context_sync",
        json!({
            "group_id":group_id,
            "by":"lead",
            "ops":[{"op":"task.move","task_id":task_id,"status":"done"}]
        }),
    );
    let released = call(
        &home,
        "elastic_release",
        json!({"group_id":group_id,"by":"lead","actor_id":actor_id,"task_id":task_id}),
    );
    assert_eq!(released.result["released"], true);
    let actors = call(
        &home,
        "actor_list",
        json!({"group_id":group_id,"by":"lead"}),
    );
    assert!(
        actors.result["actors"]
            .as_array()
            .is_some_and(|actors| actors.iter().all(|actor| actor["id"] != actor_id))
    );
    assert!(cccc_runtime::status(&group_id, &actor_id).is_err());
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(
        response.ok,
        "{op} failed: {:?}",
        response.error.as_ref().map(|error| &error.message)
    );
    response
}
