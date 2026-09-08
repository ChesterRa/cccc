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
