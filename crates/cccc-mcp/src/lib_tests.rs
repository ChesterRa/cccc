use std::collections::BTreeSet;

use serde_json::json;

#[test]
fn initialize_negotiates_supported_legacy_protocol_versions() {
    for version in crate::SUPPORTED_LEGACY_PROTOCOL_VERSIONS {
        let request = json!({"params":{"protocolVersion":version}});
        assert_eq!(crate::negotiated_protocol_version(&request), *version);
    }
    assert_eq!(
        crate::negotiated_protocol_version(&json!({"params":{"protocolVersion":"2099-01-01"}})),
        crate::DEFAULT_LEGACY_PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn initialize_truthfully_disables_tool_list_change_notifications() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let response = crate::handle_request(
        &home,
        &json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{"protocolVersion":"2025-11-25","capabilities":{}}
        }),
    )
    .await;

    assert_eq!(
        response["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
}

#[tokio::test]
async fn protocol_and_tool_execution_errors_use_distinct_envelopes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");

    let unknown_method = crate::handle_request(
        &home,
        &json!({"jsonrpc":"2.0","id":1,"method":"unknown/method","params":{}}),
    )
    .await;
    assert_eq!(unknown_method["error"]["code"], -32601);

    let invalid_request = crate::handle_request(&home, &json!([])).await;
    assert_eq!(invalid_request["error"]["code"], -32600);

    let notification = crate::handle_request(
        &home,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    )
    .await;
    assert_eq!(notification, json!({}));

    let malformed = crate::handle_request(
        &home,
        &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":[]}),
    )
    .await;
    assert_eq!(malformed["error"]["code"], -32602);

    let unknown_tool = crate::handle_request(
        &home,
        &json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"not_a_tool","arguments":{}}
        }),
    )
    .await;
    assert_eq!(unknown_tool["error"]["code"], -32602);
    assert!(
        unknown_tool["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Unknown tool"))
    );

    let omitted_arguments = crate::handle_request(
        &home,
        &json!({
            "jsonrpc":"2.0","id":5,"method":"tools/call",
            "params":{"name":"cccc_help"}
        }),
    )
    .await;
    assert!(omitted_arguments["result"]["content"].is_array());
    assert!(omitted_arguments["result"]["structuredContent"].is_object());

    let execution_error = crate::handle_request(
        &home,
        &json!({
            "jsonrpc":"2.0","id":6,"method":"tools/call",
            "params":{"name":"cccc_repo","arguments":{"action":"info"}}
        }),
    )
    .await;
    assert_eq!(execution_error["result"]["isError"], true);
    assert_eq!(
        execution_error["result"]["structuredContent"]["error"]["code"],
        "tool_execution_error"
    );
    assert!(execution_error.get("error").is_none());
}

#[tokio::test]
async fn daemon_error_details_survive_the_native_mcp_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("error details", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("add peer");
    store.save(&group).expect("save group");

    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: "group_list".into(),
                args: serde_json::Map::new(),
            })
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let response = crate::handle_request(
        &home,
        &json!({
            "jsonrpc":"2.0",
            "id":7,
            "method":"tools/call",
            "params":{
                "name":"cccc_message_send",
                "arguments":{
                    "group_id":group.group_id,
                    "by":"user",
                    "to":["peer1"],
                    "text":"review this",
                    "mode":"send"
                }
            }
        }),
    )
    .await;
    let error = &response["result"]["structuredContent"]["error"];
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(error["code"], "peer_insight_required");
    assert_eq!(error["details"]["delivery_state"], "not_sent");
    assert_eq!(error["details"]["new_side_effects"], false);
    assert!(
        error["details"]["recommended_action"]
            .as_str()
            .is_some_and(|value| value.contains("Do not mechanically add"))
    );

    let success = crate::handle_request(
        &home,
        &json!({
            "jsonrpc":"2.0",
            "id":8,
            "method":"tools/call",
            "params":{
                "name":"cccc_message_send",
                "arguments":{
                    "group_id":group.group_id,
                    "by":"user",
                    "to":["peer1"],
                    "text":"review this later",
                    "insight":"The decision boundary matters more than the local wording.",
                    "mode":"mail"
                }
            }
        }),
    )
    .await;
    assert_ne!(success["result"]["isError"], true);
    assert_eq!(
        success["result"]["structuredContent"]["post_message_nudge"]["kind"],
        "whole_situation_reconstruction"
    );
    daemon_task.abort();
}

#[tokio::test]
async fn daemon_error_details_survive_nested_code_mode_calls() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("code mode error details", "").expect("group");
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).expect("root");
    group = cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::Scope {
            scope_key: "s_code_mode".into(),
            url: root.to_string_lossy().into_owned(),
            label: "Code mode".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach root");
    let mut web = cccc_contracts::Actor::new("web1");
    web.runtime = cccc_contracts::ActorRuntime::WebModel;
    cccc_core::actors::add(&mut group, web).expect("add Web Model");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("add peer");
    store.save(&group).expect("save group");

    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: "group_list".into(),
                args: serde_json::Map::new(),
            })
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let response = crate::handle_request(
        &home,
        &json!({
            "jsonrpc":"2.0",
            "id":9,
            "method":"tools/call",
            "params":{
                "name":"cccc_code_exec",
                "arguments":{
                    "group_id":group.group_id,
                    "by":"web1",
                    "source":"try { await tools.cccc_message_send({to:['peer1'], text:'review this', mode:'send'}); } catch (error) { text(JSON.stringify({code:error.code, delivery_state:error.details?.delivery_state, new_side_effects:error.details?.new_side_effects})); }"
                }
            }
        }),
    )
    .await;
    let output = response["result"]["structuredContent"]["output"]
        .as_str()
        .expect("code mode output");
    let error: serde_json::Value = serde_json::from_str(output).expect("structured nested error");
    assert_eq!(error["code"], "peer_insight_required");
    assert_eq!(error["delivery_state"], "not_sent");
    assert_eq!(error["new_side_effects"], false);
    daemon_task.abort();
}

#[test]
fn unscoped_fallback_remains_the_fifteen_core_tools() {
    let names = crate::core_tools(crate::tools::catalog())
        .into_iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let expected = [
        "cccc_agent_state",
        "cccc_bootstrap",
        "cccc_capability_search",
        "cccc_capability_use",
        "cccc_context_get",
        "cccc_coordination",
        "cccc_file",
        "cccc_help",
        "cccc_inbox_read",
        "cccc_message_history",
        "cccc_message_deliver",
        "cccc_message_reply",
        "cccc_message_send",
        "cccc_reply_request_cancel",
        "cccc_task",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();

    assert_eq!(names, expected);
}

#[tokio::test]
async fn user_fallback_includes_the_minimal_group_control_plane() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.create("user tools", ""))
        .expect("group");
    let client = cccc_client::DaemonClient::new(home.clone());

    let names = crate::visible_tools_for_actor(&home, &client, &group.group_id, "user")
        .await
        .into_iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();

    assert!(names.contains("cccc_group"));
    assert!(names.contains("cccc_actor"));
    assert!(!names.contains("cccc_runtime_list"));
}

#[tokio::test]
async fn web_model_schema_stays_fixed_while_daemon_is_unavailable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("web model schema", "").expect("group");
    let mut actor = cccc_contracts::Actor::new("web1");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    cccc_core::actors::add(&mut group, actor).expect("add actor");
    store.save(&group).expect("save group");

    let client = cccc_client::DaemonClient::new(home.clone());
    let names = crate::visible_tools_for_actor(&home, &client, &group.group_id, "web1")
        .await
        .into_iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut expected = cccc_core::WEB_MODEL_CORE_TOOL_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    if !crate::code_mode::enabled() {
        expected.remove("cccc_code_exec");
        expected.remove("cccc_code_wait");
    }

    assert_eq!(names, expected);
}
