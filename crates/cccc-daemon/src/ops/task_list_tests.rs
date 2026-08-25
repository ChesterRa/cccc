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
        &tasks[0],
        None,
        Some("blocked"),
        "",
        ""
    ));
    assert!(!super::task_list_query::matches_task(
        &tasks[1],
        None,
        Some("blocked"),
        "",
        ""
    ));
}
