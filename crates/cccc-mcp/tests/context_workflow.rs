use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};

async fn tool(home: &HomeLayout, group: &str, actor: &str, name: &str, args: Value) -> Value {
    cccc_mcp::handle_request_for_actor(home, &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}), group, actor).await
}
fn payload(response: &Value) -> &Value {
    assert_ne!(response["result"]["isError"], true, "{response}");
    assert!(response.get("error").is_none(), "{response}");
    &response["result"]["structuredContent"]
}

#[tokio::test]
async fn declared_context_tools_preserve_authority_recovery_and_query_semantics() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("context workflow", "").expect("group");
    for id in ["lead", "peer", "other"] {
        cccc_core::actors::add(&mut group, cccc_contracts::Actor::new(id)).expect("actor");
    }
    groups.save(&group).expect("save group");
    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            })
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let outcome = tokio::spawn(async move {
        let id = group.group_id.as_str();
        let catalog = cccc_mcp::handle_request_for_actor(
            &home,
            &json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}),
            id,
            "peer",
        )
        .await;
        for name in [
            "cccc_context_get",
            "cccc_coordination",
            "cccc_task",
            "cccc_agent_state",
        ] {
            assert!(
                catalog["result"]["tools"]
                    .as_array()
                    .expect("tools")
                    .iter()
                    .any(|tool| tool["name"] == name),
                "missing {name}"
            );
        }
        for (action, field) in [
            ("add_decision", "recent_decisions"),
            ("add_handoff", "recent_handoffs"),
        ] {
            payload(
                &tool(
                    &home,
                    id,
                    "peer",
                    "cccc_coordination",
                    json!({"action":action,"summary":"reviewed fixture"}),
                )
                .await,
            );
            let response = tool(
                &home,
                id,
                "peer",
                "cccc_coordination",
                json!({"action":"get"}),
            )
            .await;
            assert_eq!(
                payload(&response)["coordination"][field][0]["summary"],
                "reviewed fixture"
            );
        }
        payload(
            &tool(
                &home,
                id,
                "peer",
                "cccc_task",
                json!({"action":"create","title":"owned task"}),
            )
            .await,
        );
        let list = tool(&home, id, "peer", "cccc_task", json!({"action":"list"})).await;
        let tasks = payload(&list)["tasks"].as_array().expect("tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["assignee"], "peer");
        let task_id = tasks[0]["id"].as_str().expect("id").to_owned();
        let denied = tool(
            &home,
            id,
            "other",
            "cccc_task",
            json!({"action":"update","task_id":task_id,"notes":"unauthorized","by":"user"}),
        )
        .await;
        assert_eq!(
            denied["result"]["structuredContent"]["error"]["code"],
            "permission_denied"
        );

        let before_rejections = tool(&home, id, "peer", "cccc_context_get", json!({})).await;
        for dry_run in [true, false] {
            for (actor, operations) in [
                ("other", json!([{"op":"task.update","task_id":format!(" {task_id} "),"notes":"bypass"}])),
                ("peer", json!([{"op":"task.create","title":"batch task","assignee":"peer"},{"op":"task.update","task_id":"T002","assignee":"other"}])),
                ("peer", json!([{"op":"task.update","task_id":task_id,"assignee":null},{"op":"task.move","task_id":task_id,"status":"archived"}])),
            ] {
                let response = tool(&home, id, actor, "cccc_context_sync", json!({"ops":operations,"dry_run":dry_run})).await;
                assert_eq!(response["result"]["structuredContent"]["error"]["code"], "permission_denied", "{actor}: {response}");
            }
        }
        let after_rejections = tool(&home, id, "peer", "cccc_context_get", json!({})).await;
        assert_eq!(payload(&before_rejections), payload(&after_rejections), "rejected batch changed stored state or revision");
        // Whitespace accepted by task.update remains usable for the owner.
        payload(&tool(&home, id, "peer", "cccc_context_sync", json!({"ops":[{"op":"task.update","task_id":format!(" {task_id} "),"notes":"owned"}]})).await);

        let snapshot = tool(&home, id, "peer", "cccc_context_get", json!({})).await;
        let version = payload(&snapshot)["version"]
            .as_str()
            .expect("version")
            .to_owned();
        let operations = json!([{"op":"task.update","task_id":task_id,"notes":"preview"}]);
        let dry = tool(
            &home,
            id,
            "peer",
            "cccc_context_sync",
            json!({"ops":operations,"if_version":version,"dry_run":true}),
        )
        .await;
        assert_eq!(payload(&dry)["dry_run"], true);
        assert_eq!(payload(&dry)["version"], version);
        let actual = tool(
            &home,
            id,
            "peer",
            "cccc_task",
            json!({"action":"list","task_id":task_id}),
        )
        .await;
        assert_ne!(payload(&actual)["task"]["notes"], "preview");
        payload(
            &tool(
                &home,
                id,
                "peer",
                "cccc_context_sync",
                json!({"ops":operations,"if_version":version}),
            )
            .await,
        );
        let stale = tool(
            &home,
            id,
            "peer",
            "cccc_context_sync",
            json!({"ops":operations,"if_version":version}),
        )
        .await;
        assert_eq!(
            stale["result"]["structuredContent"]["error"]["code"],
            "version_conflict"
        );

        payload(
            &tool(
                &home,
                id,
                "peer",
                "cccc_agent_state",
                json!({"action":"update","focus":"fixture"}),
            )
            .await,
        );
        let state = tool(
            &home,
            id,
            "peer",
            "cccc_agent_state",
            json!({"action":"get","include_warm":false}),
        )
        .await;
        assert_eq!(payload(&state)["agent_state"]["hot"]["focus"], "fixture");
        assert!(payload(&state)["agent_state"].get("warm").is_none());
        let path = groups
            .group_dir(id)
            .expect("group dir")
            .join("context/tasks/T001.yaml");
        let original = std::fs::read(&path).expect("task bytes");
        std::fs::write(&path, "[damaged fixture").expect("damage fixture");
        let failed = tool(&home, id, "peer", "cccc_task", json!({"action":"list"})).await;
        assert_eq!(failed["result"]["isError"], true);
        assert_eq!(
            failed["result"]["structuredContent"]["error"]["code"],
            "io_error"
        );
        assert!(
            failed["result"]["structuredContent"]["error"]["message"]
                .as_str()
                .expect("error message")
                .contains("T001.yaml")
        );
        std::fs::write(&path, original).expect("restore exact fixture");
        let restored = tool(
            &home,
            id,
            "peer",
            "cccc_task",
            json!({"action":"list","task_id":task_id}),
        )
        .await;
        assert_eq!(payload(&restored)["task"]["notes"], "preview");
        payload(&tool(&home, id, "peer", "cccc_task", json!({"action":"move","task_id":task_id,"status":"archived"})).await);
        for include_archived in [false, true] {
            let response = tool(&home, id, "peer", "cccc_context_get", json!({"include_archived":include_archived})).await;
            assert_eq!(payload(&response)["coordination"]["tasks"].as_array().expect("tasks").len(), usize::from(include_archived));
        }

    })
    .await;
    daemon_task.abort();
    let _ = daemon_task.await;
    outcome.expect("context workflow assertions");
}

const CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// Every bullet of this repository's own changelog becomes one task, so the
/// board is large with real text rather than generated filler.
fn changelog_tasks() -> Vec<Value> {
    CHANGELOG
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|entry| {
            let title = entry.chars().take(120).collect::<String>();
            json!({"op":"task.create","title":title,"notes":entry})
        })
        .collect()
}

#[tokio::test]
async fn context_get_answers_a_large_board_in_bounded_pages() {
    const CAP: usize = 256 * 1024;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("bounded context", "").expect("group");
    for id in ["lead", "peer"] {
        cccc_core::actors::add(&mut group, cccc_contracts::Actor::new(id)).expect("actor");
    }
    groups.save(&group).expect("save group");
    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        let ping = cccc_contracts::DaemonRequest {
            v: 1,
            op: "ping".into(),
            args: Default::default(),
        };
        if client.call(&ping).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let outcome = tokio::spawn(async move {
        let id = group.group_id.as_str();
        let mut operations = changelog_tasks();
        // One task larger than a page on its own: the whole changelog as notes.
        let heading = CHANGELOG.lines().next().unwrap_or_default();
        operations.push(json!({"op":"task.create","title":heading,"notes":CHANGELOG}));
        let created = operations.len();
        payload(
            &tool(
                &home,
                id,
                "lead",
                "cccc_context_sync",
                json!({"ops":operations}),
            )
            .await,
        );

        let expected = (1..=created)
            .map(|number| json!(format!("T{number:03}")))
            .collect::<Vec<_>>();
        // The default bound, then a caller asking for smaller answers.
        let mut page_counts = Vec::new();
        let mut clipped = Vec::new();
        for (max_bytes, cap) in [(Value::Null, CAP), (json!(16 * 1024), 16 * 1024)] {
            let mut clips = 0;
            let mut seen = Vec::new();
            let mut cursor = Value::Null;
            let mut pages = 0;
            loop {
                let response = tool(
                    &home,
                    id,
                    "peer",
                    "cccc_context_get",
                    json!({"cursor":cursor,"max_bytes":max_bytes}),
                )
                .await;
                let page = payload(&response);
                // The text an Actor reads is the pretty-printed payload.
                let bytes = serde_json::to_vec_pretty(page).expect("page JSON").len();
                assert!(bytes <= cap, "page {pages} is {bytes} bytes, over {cap}");
                assert_eq!(
                    page["tasks_summary"]["planned"], created,
                    "counts cover every task"
                );
                let tasks = page["coordination"]["tasks"].as_array().expect("tasks");
                assert!(!tasks.is_empty(), "every page makes progress");
                assert!(tasks.len() <= 100);
                seen.extend(tasks.iter().map(|task| task["id"].clone()));
                for task in tasks.iter().filter(|task| task["title"] == heading) {
                    let notes = task["notes"].as_str().expect("notes");
                    match notes.strip_suffix(" … [clipped to fit the page]") {
                        Some(kept) => {
                            assert!(CHANGELOG.starts_with(kept), "clipped, not rewritten");
                            clips += 1;
                        }
                        None => assert_eq!(notes, CHANGELOG),
                    }
                }
                pages += 1;
                if page["truncated"] == false {
                    assert_eq!(page["next_cursor"], Value::Null);
                    break;
                }
                cursor = page["next_cursor"].clone();
                assert_eq!(cursor, *seen.last().expect("last task"));
            }
            assert_eq!(seen, expected, "pages return each task once, in order");
            page_counts.push(pages);
            clipped.push(clips);
        }
        assert_eq!(
            clipped[1], 1,
            "the changelog task is clipped to the smaller bound"
        );
        assert!(page_counts[0] > 1, "the board did not need paging");
        assert!(
            page_counts[1] > page_counts[0],
            "a smaller byte bound needs more pages: {page_counts:?}"
        );
    })
    .await;
    daemon_task.abort();
    let _ = daemon_task.await;
    outcome.expect("bounded context assertions");
}
