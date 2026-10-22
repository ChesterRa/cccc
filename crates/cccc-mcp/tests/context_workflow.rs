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

/// A slice of one of our real boards, anonymized: a run of consecutive tasks
/// with the board's brief, recent decisions and all its Actor states. Every
/// letter and digit of free text is replaced (`x`, `X`, `0`), Actor and task
/// ids are renumbered, and the clock is shifted to start on 2000-01-01;
/// statuses, fields, checklists, line breaks, punctuation, the byte length of
/// every text and the time between events are as captured. The Actor states
/// alone are larger than a 16 KiB page, and one task's notes are so long that
/// no page holding that task fits the default bound whole.
const CAPTURED_CONTEXT: &str = include_str!("fixtures/captured_context.json");
const CLIP_MARK: &str = " … [clipped to fit the page]";

/// Put the captured context where the group's context store keeps it.
fn install_captured_context(groups: &GroupStore, group_id: &str) -> Value {
    let fixture: Value = serde_json::from_str(CAPTURED_CONTEXT).expect("captured context");
    let context = groups
        .group_dir(group_id)
        .expect("group dir")
        .join("context");
    let write = |name: String, value: &Value| {
        cccc_core::fs::write_yaml(&context.join(name), value).expect("captured context file");
    };
    write("context.yaml".into(), &fixture["context"]);
    write(
        "agents.yaml".into(),
        &json!({"agent_states": fixture["agent_states"]}),
    );
    for task in fixture["tasks"].as_array().expect("tasks") {
        write(
            format!("tasks/{}.yaml", task["id"].as_str().expect("id")),
            task,
        );
    }
    fixture
}

/// The byte length of a task's longest free text, and whether it is a
/// checklist item.
fn longest_text(task: &Value) -> (usize, bool) {
    let field = ["title", "notes", "outcome"]
        .iter()
        .filter_map(|key| task[*key].as_str())
        .map(str::len)
        .max()
        .unwrap_or(0);
    let item = task["checklist"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text"].as_str())
        .map(str::len)
        .max()
        .unwrap_or(0);
    (field.max(item), item > field)
}

#[tokio::test]
async fn context_get_pages_a_captured_board_within_its_byte_bound() {
    const CAP: usize = 256 * 1024;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("captured context", "").expect("group");
    for id in ["lead", "peer"] {
        cccc_core::actors::add(&mut group, cccc_contracts::Actor::new(id)).expect("actor");
    }
    groups.save(&group).expect("save group");
    let fixture = install_captured_context(&groups, &group.group_id);
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
        let tasks = fixture["tasks"].as_array().expect("tasks");
        let captured = |task_id: &Value| {
            tasks
                .iter()
                .find(|task| task["id"] == *task_id)
                .expect("captured task")
        };
        let count = |status: &str| tasks.iter().filter(|task| task["status"] == status).count();
        let live = tasks
            .iter()
            .filter(|task| task["status"] != "archived")
            .map(|task| task["id"].clone())
            .collect::<Vec<_>>();
        let largest = tasks
            .iter()
            .max_by_key(|task| longest_text(task).0)
            .expect("largest task");

        // The default bound, then a smaller one. A full page carries each task
        // twice (the task list and the board) next to the Actor states, so the
        // task with the largest notes is clipped at both, and further at the
        // smaller one.
        let mut page_counts = Vec::new();
        let mut clipped = Vec::new();
        for (max_bytes, cap) in [(Value::Null, CAP), (json!(160 * 1024), 160 * 1024)] {
            let mut clips = Vec::new();
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
                let summary = &page["tasks_summary"];
                assert_eq!(summary["total"], live.len(), "counts cover every task");
                for status in ["planned", "done", "archived"] {
                    assert_eq!(summary[status], count(status), "{status}");
                }
                let page_tasks = page["coordination"]["tasks"].as_array().expect("tasks");
                assert!(!page_tasks.is_empty(), "every page makes progress");
                assert!(page_tasks.len() <= 100);
                for task in page_tasks {
                    let source = captured(&task["id"]);
                    for key in ["title", "notes", "outcome", "checklist"] {
                        if task[key] == source[key] {
                            continue;
                        }
                        let text = task[key].as_str().expect("only text is clipped");
                        let kept = text.strip_suffix(CLIP_MARK).expect("marked clip");
                        let whole = source[key].as_str().expect("captured text");
                        assert!(whole.starts_with(kept), "clipped, not rewritten");
                        clips.push((task["id"].clone(), key, kept.len()));
                    }
                }
                seen.extend(page_tasks.iter().map(|task| task["id"].clone()));
                pages += 1;
                if page["truncated"] != true {
                    assert_eq!(page["next_cursor"], Value::Null);
                    break;
                }
                cursor = page["next_cursor"].clone();
                assert_eq!(cursor, *seen.last().expect("last task"));
            }
            assert_eq!(seen, live, "pages return each task once, in order");
            page_counts.push(pages);
            clipped.push(clips);
        }
        for clips in &clipped {
            let what = clips
                .iter()
                .map(|(task, key, _)| (task.clone(), *key))
                .collect::<Vec<_>>();
            assert_eq!(what, [(largest["id"].clone(), "notes")], "{clipped:?}");
        }
        assert!(clipped[1][0].2 < clipped[0][0].2, "{clipped:?}");
        assert!(page_counts[0] > 1, "the board did not need paging");
        assert!(
            page_counts[1] > page_counts[0],
            "a smaller byte bound needs more pages: {page_counts:?}"
        );

        // A task whose longest text is a checklist item: with a bound just
        // under its one-task page, that item is clipped and nothing else.
        let checklist_task = tasks
            .iter()
            .find(|task| longest_text(task).1)
            .expect("a captured task whose longest text is a checklist item");
        let previous = tasks
            .iter()
            .filter(|task| task["status"] != "archived")
            .take_while(|task| task["id"] != checklist_task["id"])
            .last()
            .map_or(Value::Null, |task| task["id"].clone());
        let one_task =
            |max_bytes: usize| json!({"cursor":previous,"limit":1,"max_bytes":max_bytes});
        let response = tool(&home, id, "peer", "cccc_context_get", one_task(CAP)).await;
        let whole = serde_json::to_vec_pretty(payload(&response))
            .expect("page JSON")
            .len();
        let bound = whole - 40;
        let response = tool(&home, id, "peer", "cccc_context_get", one_task(bound)).await;
        let page = payload(&response);
        let bytes = serde_json::to_vec_pretty(page).expect("page JSON").len();
        assert!(bytes <= bound, "{bytes} bytes, over {bound}");
        let task = &page["coordination"]["tasks"][0];
        assert_eq!(task["id"], checklist_task["id"]);
        for key in ["title", "notes", "outcome"] {
            assert_eq!(task[key], checklist_task[key], "{key}");
        }
        let items = task["checklist"].as_array().expect("checklist");
        let originals = checklist_task["checklist"].as_array().expect("checklist");
        assert_eq!(items.len(), originals.len());
        let longest = longest_text(checklist_task).0;
        for (item, original) in items.iter().zip(originals) {
            let text = original["text"].as_str().expect("item text");
            if text.len() == longest {
                let kept = item["text"]
                    .as_str()
                    .and_then(|clipped| clipped.strip_suffix(CLIP_MARK))
                    .expect("the longest checklist item is clipped and marked");
                assert!(text.starts_with(kept), "clipped, not rewritten");
            } else {
                assert_eq!(item, original);
            }
        }

        // The captured Actor states alone outgrow 16 KiB: no page fits, and the
        // call says so instead of answering over the bound.
        for detail in ["full", "overview"] {
            let refused = tool(
                &home,
                id,
                "peer",
                "cccc_context_get",
                json!({"detail":detail,"max_bytes":16 * 1024}),
            )
            .await;
            assert_eq!(refused["result"]["isError"], true, "{detail}: {refused}");
            let error = &refused["result"]["structuredContent"]["error"];
            assert_eq!(error["code"], "invalid_args");
            assert!(
                error["message"]
                    .as_str()
                    .expect("error message")
                    .contains("max_bytes is smaller than the smallest page of this context"),
                "{detail}: {error}"
            );
        }
        let overview = tool(
            &home,
            id,
            "peer",
            "cccc_context_get",
            json!({"detail":"overview","max_bytes":CAP}),
        )
        .await;
        let overview = payload(&overview);
        assert!(
            serde_json::to_vec_pretty(overview)
                .expect("overview JSON")
                .len()
                <= CAP
        );
        assert!(overview.get("board").is_none());
        assert!(overview["coordination"].get("tasks").is_none());
    })
    .await;
    daemon_task.abort();
    let _ = daemon_task.await;
    outcome.expect("bounded context assertions");
}
