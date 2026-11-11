use super::*;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};

fn request(group_id: &str, args: Value) -> DaemonRequest {
    let mut args = args.as_object().cloned().expect("args");
    args.insert("group_id".into(), json!(group_id));
    DaemonRequest {
        v: 1,
        op: "task_list".into(),
        args,
    }
}

fn seeded() -> (tempfile::TempDir, HomeLayout, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("tasks", "").expect("group");
    let contexts = ContextStore::new(home.clone()).expect("contexts");
    let ops =
        (1..=35)
            .map(|index| {
                json!({
            "op":"task.create",
            "title":format!("planned {index:02}"),
            "assignee":if index % 2 == 0 { Value::String("peer".into()) } else { Value::Null },
        }).as_object().cloned().expect("op")
            })
            .collect::<Vec<Map<String, Value>>>();
    contexts
        .sync(&group.group_id, &ops, None, "user", false)
        .expect("seed");
    (temp, home, group.group_id)
}

fn seeded_with_finished_blocker() -> (tempfile::TempDir, HomeLayout, String) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("tasks", "").expect("group");
    let contexts = ContextStore::new(home.clone()).expect("contexts");
    let ops = [
        json!({
            "op":"task.create",
            "title":"finished blocker",
            "status":"done",
            "client_request_id":"finished-blocker",
        }),
        json!({
            "op":"task.create",
            "title":"blocked task",
            "client_request_id":"blocked-task",
            "blocked_by_refs":["finished-blocker"],
        }),
        json!({
            "op":"task.create",
            "title":"blocked child",
            "parent_ref":"blocked-task",
            "blocked_by_refs":["finished-blocker"],
        }),
    ]
    .into_iter()
    .map(|op| op.as_object().cloned().expect("op"))
    .collect::<Vec<Map<String, Value>>>();
    contexts
        .sync(&group.group_id, &ops, None, "user", false)
        .expect("seed");
    (temp, home, group.group_id)
}

fn task_named<'a>(tasks: &'a Value, title: &str) -> &'a Value {
    tasks
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["title"] == title)
        .expect("named task")
}

#[test]
fn pagination_is_stable_and_reports_facets() {
    let (_temp, home, group_id) = seeded();
    let first = run(
        &home,
        &request(
            &group_id,
            json!({"status":"planned","offset":"0","limit":"30"}),
        ),
    )
    .expect("first");
    let second = run(
        &home,
        &request(
            &group_id,
            json!({"status":"planned","offset":"30","limit":"30"}),
        ),
    )
    .expect("second");

    assert_eq!(first["count"], 30);
    assert_eq!(first["total_count"], 35);
    assert_eq!(first["has_more"], true);
    assert_eq!(second["count"], 5);
    assert_eq!(second["has_more"], false);
    assert_eq!(first["facets"]["status_counts"]["planned"], 35);
    assert_eq!(first["facets"]["unassigned"], 18);
    assert_eq!(first["tasks"][0]["id"], "T035");
}

#[test]
fn legacy_listing_and_exact_task_remain_compatible() {
    let (_temp, home, group_id) = seeded();
    let listed = run(&home, &request(&group_id, json!({}))).expect("listed");
    assert_eq!(listed["tasks"].as_array().expect("tasks").len(), 35);
    assert!(listed.get("total_count").is_none());

    let exact = run(&home, &request(&group_id, json!({"task_id":"T001"}))).expect("exact");
    assert_eq!(exact["task"]["id"], "T001");
    assert_eq!(exact["delete_info"]["allowed"], true);
    assert!(exact["tasks_version"].as_str().is_some());
}

#[test]
fn filters_apply_before_pagination() {
    let (_temp, home, group_id) = seeded();
    let filtered = run(
        &home,
        &request(
            &group_id,
            json!({
                "status":"planned","assignee":"peer","query":"planned","limit":10
            }),
        ),
    )
    .expect("filtered");
    assert_eq!(filtered["total_count"], 17);
    assert_eq!(filtered["count"], 10);
    assert!(
        filtered["tasks"]
            .as_array()
            .expect("tasks")
            .iter()
            .all(|task| task["assignee"] == "peer")
    );

    let unassigned = run(
        &home,
        &request(
            &group_id,
            json!({"status":"planned","attention":"unassigned","limit":30}),
        ),
    )
    .expect("unassigned");
    assert_eq!(unassigned["total_count"], 18);
}

#[test]
fn batch_pages_share_one_version_and_include_an_unfiltered_index() {
    let (_temp, home, group_id) = seeded();
    let result = run(
        &home,
        &request(
            &group_id,
            json!({
                "statuses":"planned,active,done",
                "query":"planned 01",
                "limit":30,
                "include_index":true
            }),
        ),
    )
    .expect("batch");

    assert_eq!(result["pages"]["planned"]["count"], 1);
    assert_eq!(result["pages"]["active"]["count"], 0);
    assert_eq!(result["pages"]["done"]["count"], 0);
    assert_eq!(result["task_index"].as_array().expect("index").len(), 35);
    assert!(
        result["tasks_version"]
            .as_str()
            .is_some_and(|value| value.starts_with("tasksv:"))
    );
}

#[test]
fn batch_exact_ids_preserve_requested_order() {
    let (_temp, home, group_id) = seeded();
    let result = run(
        &home,
        &request(&group_id, json!({"task_ids":"T003,T001,T404"})),
    )
    .expect("batch ids");
    let ids = result["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .map(|task| task["id"].as_str().expect("id"))
        .collect::<Vec<_>>();
    assert_eq!(ids, ["T003", "T001"]);
}

#[test]
fn attention_facets_and_filters_ignore_completed_history() {
    let tasks = [
        json!({"id":"T001","status":"active","blocked_by":["T000"]}),
        json!({"id":"T002","status":"done","blocked_by":["T000"]}),
    ]
    .map(|value| value.as_object().cloned().expect("task"));

    let facets = super::task_list_query::facets(&tasks);

    assert_eq!(facets["blocked"], 1);
    assert!(super::task_list_query::matches_task(
        &tasks,
        &tasks[0],
        None,
        Some("blocked"),
        "",
        ""
    ));
    assert!(!super::task_list_query::matches_task(
        &tasks,
        &tasks[1],
        None,
        Some("blocked"),
        "",
        ""
    ));
}

#[test]
fn a_blocker_that_is_done_or_archived_stops_blocking() {
    let blocker = |status: &str| {
        json!({"id":"blocker-a","status":status})
            .as_object()
            .cloned()
            .expect("blocker")
    };
    let task = || {
        json!({"id":"task-a","status":"active","blocked_by":["blocker-a"]})
            .as_object()
            .cloned()
            .expect("task")
    };
    for status in ["planned", "active"] {
        let tasks = [blocker(status), task()];
        assert_eq!(
            super::task_list_query::facets(&tasks)["blocked"],
            1,
            "{status}"
        );
        assert!(super::task_list_query::matches_task(
            &tasks,
            &tasks[1],
            None,
            Some("blocked"),
            "",
            ""
        ));
    }
    for status in ["done", "archived"] {
        let tasks = [blocker(status), task()];
        assert_eq!(
            super::task_list_query::facets(&tasks)["blocked"],
            0,
            "{status}"
        );
        assert!(!super::task_list_query::matches_task(
            &tasks,
            &tasks[1],
            None,
            Some("blocked"),
            "",
            ""
        ));
    }
    // Restoring the blocker to an unfinished status blocks again; the set is
    // derived on every read, nothing is stored.
    let mut restored = blocker("done");
    restored.insert("status".into(), json!("planned"));
    let tasks = [restored, task()];
    assert_eq!(super::task_list_query::facets(&tasks)["blocked"], 1);
}

#[test]
fn a_blocker_id_that_names_no_task_keeps_blocking() {
    let tasks = [
        json!({"id":"task-a","status":"active","blocked_by":["missing-blocker"]})
            .as_object()
            .cloned()
            .expect("task"),
    ];
    assert_eq!(super::task_list_query::facets(&tasks)["blocked"], 1);
    assert!(super::task_list_query::matches_task(
        &tasks,
        &tasks[0],
        None,
        Some("blocked"),
        "",
        ""
    ));
}

#[test]
fn waiting_on_still_blocks_without_any_open_blocker() {
    let tasks = [
        json!({"id":"blocker-a","status":"done"})
            .as_object()
            .cloned()
            .expect("blocker"),
        json!({"id":"task-a","status":"active","blocked_by":["blocker-a"],"waiting_on":"actor"})
            .as_object()
            .cloned()
            .expect("task"),
    ];
    assert_eq!(super::task_list_query::facets(&tasks)["blocked"], 1);
    assert!(super::task_list_query::matches_task(
        &tasks,
        &tasks[1],
        None,
        Some("blocked"),
        "",
        ""
    ));
}

#[test]
fn every_task_list_response_stamps_open_blocked_by() {
    let (_temp, home, group_id) = seeded_with_finished_blocker();
    let plain = run(&home, &request(&group_id, json!({}))).expect("plain");
    let blocked = task_named(&plain["tasks"], "blocked task");
    let blocked_id = blocked["id"].as_str().expect("blocked id").to_owned();
    assert_eq!(
        blocked["blocked_by"]
            .as_array()
            .expect("stored blockers")
            .len(),
        1
    );
    assert_eq!(blocked["open_blocked_by"], json!([]));

    let page = run(
        &home,
        &request(&group_id, json!({"status":"planned","limit":10})),
    )
    .expect("page");
    assert_eq!(
        task_named(&page["tasks"], "blocked task")["open_blocked_by"],
        json!([])
    );

    let pages = run(
        &home,
        &request(&group_id, json!({"statuses":"planned,done","limit":10})),
    )
    .expect("pages");
    assert_eq!(
        task_named(&pages["pages"]["planned"]["tasks"], "blocked task")["open_blocked_by"],
        json!([])
    );

    let one = run(
        &home,
        &request(&group_id, json!({"task_id":blocked_id.clone()})),
    )
    .expect("one");
    assert_eq!(one["task"]["open_blocked_by"], json!([]));
    let child = task_named(&one["task"]["children"], "blocked child");
    assert_eq!(child["open_blocked_by"], json!([]));

    let many = run(
        &home,
        &request(&group_id, json!({"task_ids":blocked_id.clone()})),
    )
    .expect("many");
    assert_eq!(many["tasks"][0]["open_blocked_by"], json!([]));

    let indexed = run(
        &home,
        &request(&group_id, json!({"limit":10,"include_index":true})),
    )
    .expect("indexed");
    let index_item = indexed["task_index"]
        .as_array()
        .expect("task index")
        .iter()
        .find(|task| task["id"].as_str() == Some(blocked_id.as_str()))
        .expect("indexed task");
    assert_eq!(index_item["open_blocked_by"], json!([]));
}

#[test]
fn forged_open_blocked_by_cannot_hide_a_planned_blocker() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("tasks", "").expect("group");
    let contexts = ContextStore::new(home.clone()).expect("contexts");
    let ops = [
        json!({
            "op":"task.create",
            "title":"planned blocker",
            "client_request_id":"planned-blocker",
        }),
        json!({
            "op":"task.create",
            "title":"blocked task",
            "client_request_id":"blocked-task",
            "blocked_by_refs":["planned-blocker"],
            "open_blocked_by":[],
        }),
    ]
    .into_iter()
    .map(|op| op.as_object().cloned().expect("op"))
    .collect::<Vec<Map<String, Value>>>();
    contexts
        .sync(&group.group_id, &ops, None, "user", false)
        .expect("seed");

    let stored = contexts.load(&group.group_id).expect("context");
    let blocker_id = stored
        .tasks
        .iter()
        .find(|task| task["title"] == "planned blocker")
        .and_then(|task| task["id"].as_str())
        .expect("blocker id")
        .to_owned();
    let blocked = stored
        .tasks
        .iter()
        .find(|task| task["title"] == "blocked task")
        .expect("blocked task");
    assert!(blocked.get("open_blocked_by").is_none());

    let listed = run(&home, &request(&group.group_id, json!({}))).expect("listed");
    assert_eq!(
        task_named(&listed["tasks"], "blocked task")["open_blocked_by"],
        json!([blocker_id.clone()])
    );

    let projected = super::super::context_projection::project(
        contexts.load(&group.group_id).expect("context"),
        "v1".into(),
        "summary",
    );
    assert_eq!(
        task_named(&projected["coordination"]["tasks"], "blocked task")["open_blocked_by"],
        json!([blocker_id])
    );
    assert_eq!(projected["attention"]["blocked"], 1);
}
