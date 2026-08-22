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
