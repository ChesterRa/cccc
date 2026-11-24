use super::{
    apply_actor_context, apply_request_context, help_markdown, is_message_operation,
    is_task_mail_boundary, postprocess_task_result, prepare_task_arguments,
    remove_untrusted_authority, runtime_group_id, with_post_message_context,
};
use crate::RequestContext;
use serde_json::{Map, Value, json};

#[tokio::test]
async fn message_tools_reject_model_supplied_im_provenance() {
    if crate::lib_tests::isolate_embedded_daemon_test(concat!(
        module_path!(),
        "::",
        "message_tools_reject_model_supplied_im_provenance"
    )) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
    let mut group = groups.create("message origin", "").expect("group");
    for id in ["writer", "receiver"] {
        let mut actor = cccc_contracts::Actor::new(id);
        actor.runtime = cccc_contracts::ActorRuntime::Custom;
        actor.command = vec!["/bin/true".into()];
        cccc_core::actors::add(&mut group, actor).expect("actor");
    }
    groups.save(&group).expect("save");
    let client = cccc_client::DaemonClient::new(home.clone());
    let ledger = groups.ledger_path(&group.group_id).expect("ledger");
    let before = std::fs::read(&ledger).expect("events before");
    let base = json!({
        "group_id":group.group_id,"by":"writer","actor_id":"writer",
        "text":"model message","to":["receiver"],"mode":"mail",
        "src_group_id":"g_reference","src_event_id":"reference-event",
        "insight":"Connector origin must remain separate from model-authored content."
    });
    // No daemon exists yet: these errors must come from the actual router,
    // before IPC, local file handling or any message-side effect.
    for tool in [
        "cccc_message_send",
        "cccc_message_reply",
        "cccc_file_send",
        "cccc_tracked_send",
    ] {
        for key in [
            "transport",
            "im_platform",
            "im_chat_id",
            "im_thread_id",
            "source_platform",
            "source_user_id",
            "source_message_id",
        ] {
            let mut args = base.as_object().expect("args").clone();
            args.insert(key.into(), json!("do-not-echo-source-value"));
            let error = super::call(&home, &client, tool, args)
                .await
                .expect_err("IM origin rejected");
            assert_eq!(
                error.to_string(),
                format!("IM provenance is host-owned: {key}")
            );
            assert!(!error.to_string().contains("do-not-echo-source-value"));
        }
    }
    assert_eq!(std::fs::read(&ledger).expect("events after"), before);

    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    for _ in 0..100 {
        if home.daemon_dir().join("ccccd.addr.json").is_file() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let sent = super::call(
        &home,
        &client,
        "cccc_message_send",
        base.as_object().expect("args").clone(),
    )
    .await
    .expect("ordinary message");
    assert_eq!(sent["structuredContent"]["event"]["by"], "writer");
    let event_id = sent["structuredContent"]["event"]["id"]
        .as_str()
        .expect("event id");
    let stored = cccc_core::ledger::read_all(&ledger).expect("stored events");
    let event = stored
        .iter()
        .find(|event| event.id == event_id)
        .expect("stored message");
    assert_eq!(event.data["text"], "model message");
    assert_eq!(
        sent["structuredContent"]["event"]["data"]["src_group_id"],
        "g_reference"
    );
    let _ = client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: serde_json::Map::new(),
        })
        .await;
    daemon.await.expect("daemon task").expect("daemon");
}

#[test]
fn exact_native_history_identifies_this_server_not_the_supplied_value() {
    let mut args = json!({"event_id":"event", "_cccc_mcp_identity":{"version":"forged"}})
        .as_object()
        .expect("arguments")
        .clone();
    super::apply_mcp_identity("cccc_message_history", &mut args, true);
    assert_eq!(args["_cccc_mcp_identity"], super::current_mcp_identity());

    super::apply_mcp_identity("cccc_message_history", &mut args, false);
    assert!(!args.contains_key("_cccc_mcp_identity"));
    args.remove("event_id");
    super::apply_mcp_identity("cccc_message_history", &mut args, true);
    assert!(!args.contains_key("_cccc_mcp_identity"));
    args.insert("event_id".into(), json!("event"));
    super::apply_mcp_identity("cccc_task", &mut args, true);
    assert!(!args.contains_key("_cccc_mcp_identity"));
}

#[test]
fn an_explicit_empty_runtime_group_never_inherits_the_active_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::active::set(&home, "g_active").expect("active group");

    assert_eq!(runtime_group_id(&home, None).as_deref(), Some("g_active"));
    assert_eq!(runtime_group_id(&home, Some(String::new())), None);
    assert_eq!(
        runtime_group_id(&home, Some("g_explicit".into())).as_deref(),
        Some("g_explicit")
    );
}

#[test]
fn web_model_tool_authorization_is_role_aware_and_capability_routed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");

    let mut foreman_group = store.create("web model foreman", "").expect("group");
    let mut web_foreman = cccc_contracts::Actor::new("web-foreman");
    web_foreman.runtime = cccc_contracts::ActorRuntime::WebModel;
    cccc_core::actors::add(&mut foreman_group, web_foreman).expect("web foreman");
    store.save(&foreman_group).expect("save foreman group");
    let foreman_args = json!({
        "group_id":foreman_group.group_id,
        "by":"web-foreman"
    })
    .as_object()
    .cloned()
    .expect("foreman args");
    assert!(super::authorize_tool(&home, "cccc_repo", &foreman_args, false).is_ok());
    assert_eq!(
        super::authorize_tool(&home, "cccc_actor", &foreman_args, false)
            .expect_err("direct management call must fail"),
        "cccc_actor is not available to Web Model actors"
    );
    assert!(super::authorize_tool(&home, "cccc_actor", &foreman_args, true).is_ok());

    let mut peer_group = store.create("web model peer", "").expect("group");
    cccc_core::actors::add(
        &mut peer_group,
        cccc_contracts::Actor::new("native-foreman"),
    )
    .expect("native foreman");
    let mut web_peer = cccc_contracts::Actor::new("web-peer");
    web_peer.runtime = cccc_contracts::ActorRuntime::WebModel;
    cccc_core::actors::add(&mut peer_group, web_peer).expect("web peer");
    cccc_core::actors::add(&mut peer_group, cccc_contracts::Actor::new("native-peer"))
        .expect("native peer");
    store.save(&peer_group).expect("save peer group");
    let peer_args = json!({"group_id":peer_group.group_id,"by":"web-peer"})
        .as_object()
        .cloned()
        .expect("peer args");
    assert_eq!(
        super::authorize_tool(&home, "cccc_actor", &peer_args, true)
            .expect_err("peer management call must fail"),
        "cccc_actor requires a Web Model foreman actor"
    );
    // Automation is decided per change by the daemon, so a Web Model peer may reach
    // it through capability use, though not by naming it directly.
    assert!(super::authorize_tool(&home, "cccc_automation", &peer_args, true).is_ok());
    assert!(super::authorize_tool(&home, "cccc_automation", &peer_args, false).is_err());
    let native_peer_args = json!({
        "group_id":peer_group.group_id,
        "by":"native-peer"
    })
    .as_object()
    .cloned()
    .expect("native peer args");
    assert_eq!(
        super::authorize_tool(&home, "cccc_capability_import", &native_peer_args, false,)
            .expect_err("peer capability administration must fail"),
        "cccc_capability_import is not available to peer actors"
    );
}

#[tokio::test]
async fn capability_use_returns_the_rules_of_a_skill() {
    if crate::lib_tests::isolate_embedded_daemon_test(concat!(
        module_path!(),
        "::",
        "capability_use_returns_the_rules_of_a_skill"
    )) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.create("skill use", ""))
        .expect("group");
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
    let context = |group_id| {
        Some(RequestContext {
            native_binding: None,
            binding: None,
            group_id,
            actor_id: "user",
        })
    };
    let args = |capability_id: &str| {
        json!({"capability_id":capability_id,"scope":"session"})
            .as_object()
            .cloned()
            .expect("use args")
    };

    let skill = super::call_with_context(
        &home,
        &client,
        "cccc_capability_use",
        args("skill:cccc:self-evolution"),
        context(&group.group_id),
        false,
    )
    .await
    .expect("use skill");
    assert_eq!(skill["structuredContent"]["enabled"], true);
    assert!(
        skill["structuredContent"]["capsule_text"]
            .as_str()
            .expect("skill rules")
            .contains("## Review")
    );
    assert!(skill["structuredContent"].get("gotchas").is_none());
    let published = cccc_core::capabilities::CapabilityStore::new(home.clone())
        .catalog()
        .expect("catalog")
        .into_iter()
        .find(|capability| capability.kind == "skill" && capability.is_release_recommendation())
        .expect("release skill");
    let release_skill = super::call_with_context(
        &home,
        &client,
        "cccc_capability_use",
        args(&published.id),
        context(&group.group_id),
        false,
    )
    .await
    .expect("release skill");
    let release_payload = &release_skill["structuredContent"];
    assert_eq!(
        release_payload["capsule_text"], published.capsule_text,
        "notes must not be concatenated into the upstream body"
    );
    for (key, value) in cccc_core::capability_recommendations::notes(&published) {
        assert_eq!(release_payload[&key], value);
    }

    let pack = super::call_with_context(
        &home,
        &client,
        "cccc_capability_use",
        args("pack:space"),
        context(&group.group_id),
        false,
    )
    .await
    .expect("use pack");
    assert!(pack["structuredContent"].get("capsule_text").is_none());

    super::daemon(
        &client,
        "capability_block",
        json!({
            "group_id":group.group_id,"actor_id":"user","by":"user",
            "capability_id":"skill:cccc:self-evolution","scope":"group","blocked":true,
        })
        .as_object()
        .cloned()
        .expect("block args"),
    )
    .await
    .expect("block skill");
    let blocked = super::call_with_context(
        &home,
        &client,
        "cccc_capability_use",
        args("skill:cccc:self-evolution"),
        context(&group.group_id),
        false,
    )
    .await
    .expect("use blocked skill");
    assert_eq!(blocked["structuredContent"]["enabled"], false);
    assert!(blocked["structuredContent"].get("capsule_text").is_none());

    daemon_task.abort();
}

#[tokio::test]
async fn undisplayed_external_tools_remain_authorized_and_tool_errors_are_not_verified() {
    if crate::lib_tests::isolate_embedded_daemon_test(concat!(
        module_path!(),
        "::",
        "undisplayed_external_tools_remain_authorized_and_tool_errors_are_not_verified"
    )) {
        return;
    }
    let temp = tempfile::tempdir().expect("temp");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.create("external fixture", ""))
        .expect("group");
    let script = temp.path().join("server.py");
    std::fs::write(
        &script,
        include_str!("../../cccc-daemon/src/ops/capabilities/mcp_stdio/fixture.py"),
    )
    .expect("script");
    let python = ["python3", "python"]
        .into_iter()
        .find(|name| {
            std::process::Command::new(name)
                .arg("--version")
                .output()
                .is_ok_and(|value| value.status.success())
        })
        .expect("python");
    let command = json!([
        python,
        "-u",
        script,
        "tool_error",
        temp.path().join("evidence.jsonl")
    ]);
    let mut tools = (0..501).map(|index|json!({"name":format!("cccc_ext_fixture_{index:04}"),"real_tool_name":"echo","inputSchema":{"type":"object"}})).collect::<Vec<_>>();
    tools[0] = json!({"name":cccc_core::capabilities::external_tool_name("mcp:fixture", "cccc_task"),
        "real_tool_name":"cccc_task","inputSchema":{"type":"object"}});
    tools[1] = json!({"name":cccc_core::capabilities::external_tool_name("mcp:fixture", "echo"),
        "real_tool_name":"echo","inputSchema":{"type":"object"}});
    let store = cccc_core::capabilities::CapabilityStore::new(home.clone());
    store
        .import_record(
            json!({"capability_id":"mcp:fixture","kind":"mcp_toolpack","name":"Fixture"}),
        )
        .expect("import");
    store
        .set_enabled_for("mcp:fixture", true, &group.group_id, "user", "actor", 3600)
        .expect("enable");
    cccc_core::fs::write_json(&home.root().join("state/capabilities/runtime.json"),&json!({
        "artifacts":{"fixture":{"state":"installed","tools":tools,"invoker":{"type":"command_stdio","command":command}}},
        "capability_artifacts":{"mcp:fixture":"fixture"}
    })).expect("runtime");
    let daemon_home = home.clone();
    let daemon_task = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    for _ in 0..100 {
        if client
            .call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: serde_json::Map::new(),
            })
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let direct = crate::handle_request_for_actor(
        &home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"cccc_ext_fixture_0500","arguments":{
                "group_id":"external-tenant","actor_id":"external-subject","by":"external-owner","value":42
            }
        }}),
        &group.group_id,
        "user",
    )
    .await;
    assert_eq!(direct["result"]["isError"], true, "{direct}");
    assert!(
        direct["result"]["structuredContent"]
            .get("gotchas")
            .is_none()
    );
    assert_eq!(
        direct["result"]["structuredContent"]["result"]["isError"], true,
        "must reach the external tool rather than fail admission: {direct}"
    );
    let delegated = crate::handle_request_for_actor(&home,&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"cccc_capability_use","arguments":{"capability_id":"mcp:fixture","tool_name":"cccc_ext_fixture_0500",
            "tool_arguments":{"group_id":"external-tenant","actor_id":"external-subject","by":"external-owner","value":42}}
    }}),&group.group_id,"user").await;
    assert_eq!(delegated["result"]["isError"], true, "{delegated}");
    assert!(
        delegated["result"]["structuredContent"]
            .get("gotchas")
            .is_none()
    );
    assert_eq!(
        delegated["result"]["structuredContent"]["tool_called"],
        true
    );
    assert_eq!(
        delegated["result"]["structuredContent"]["state"],
        "runnable"
    );
    let user_delegated = super::call(&home, &client, "cccc_capability_use",
        json!({"group_id":group.group_id,"by":"user","capability_id":"mcp:fixture",
            "tool_name":"cccc_ext_fixture_0500",
            "tool_arguments":{"group_id":"external-tenant","actor_id":"external-subject","by":"external-owner","value":42}})
            .as_object().expect("user arguments").clone()).await.expect("unbound user delegation");
    assert_eq!(user_delegated["structuredContent"]["tool_called"], true);
    let qualified = crate::handle_request_for_actor(&home,&json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
        "name":"cccc_capability_use","arguments":{"capability_id":"mcp:fixture","tool_name":"cccc_task",
            "tool_arguments":{"group_id":"external-tenant","actor_id":"external-subject","by":"external-owner","value":42}}
    }}),&group.group_id,"user").await;
    assert_eq!(
        qualified["result"]["structuredContent"]["tool_called"], true,
        "a qualified external name must not call a CCCC operation: {qualified}"
    );
    assert_eq!(
        qualified["result"]["structuredContent"]["tool_result"]["result"]["isError"],
        true
    );
    let inferred = crate::handle_request_for_actor(&home,&json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{
        "name":"cccc_capability_use","arguments":{"tool_name":"echo",
            "tool_arguments":{"group_id":"external-tenant","actor_id":"external-subject","by":"external-owner","value":42}}
    }}),&group.group_id,"user").await;
    assert_eq!(
        inferred["result"]["structuredContent"]["tool_called"], true,
        "a unique discovered real name must keep its inferred capability: {inferred}"
    );
    assert_eq!(
        inferred["result"]["structuredContent"]["capability_id"],
        "mcp:fixture"
    );
    let core = crate::handle_request_for_actor(&home,&json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{
        "name":"cccc_capability_use","arguments":{"tool_name":"cccc_task","tool_arguments":{"action":"list"}}
    }}),&group.group_id,"user").await;
    assert_eq!(
        core["result"]["structuredContent"]["capability_id"], "core",
        "an external tool must not shadow an unqualified CCCC name: {core}"
    );
    let observed = std::fs::read_to_string(temp.path().join("evidence.jsonl"))
        .expect("external requests")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("JSON request"))
        .filter_map(|row| row.get("arguments").cloned())
        .collect::<Vec<_>>();
    let expected = json!({"group_id":"external-tenant","actor_id":"external-subject","by":"external-owner","value":42});
    assert_eq!(
        observed,
        vec![
            expected.clone(),
            expected.clone(),
            expected.clone(),
            expected.clone(),
            expected
        ],
        "host identity must never enter external parameters"
    );
    let runtime: serde_json::Value =
        cccc_core::fs::read_json(&home.root().join("state/capabilities/runtime.json"))
            .expect("runtime");
    assert!(
        runtime
            .get("recent_success")
            .and_then(|value| value.get("mcp:fixture"))
            .is_none()
    );
    store
        .import_record(
            json!({"capability_id":"mcp:other","kind":"mcp_toolpack","name":"Other","tool_names":["echo"]}),
        )
        .expect("second capability");
    store
        .set_enabled_for("mcp:other", true, &group.group_id, "user", "actor", 3600)
        .expect("second binding");
    let mut shared = runtime.clone();
    shared["capability_artifacts"]["mcp:other"] = json!("fixture");
    cccc_core::fs::write_json(
        &home.root().join("state/capabilities/runtime.json"),
        &shared,
    )
    .expect("second artifact reference");
    let ambiguous = crate::handle_request_for_actor(
        &home,
        &json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{
            "name":"cccc_capability_use","arguments":{"tool_name":"echo","tool_arguments":{}}
        }}),
        &group.group_id,
        "user",
    )
    .await;
    assert_eq!(ambiguous["result"]["isError"], true);
    assert!(
        ambiguous
            .to_string()
            .contains("capability_use_ambiguous_tool"),
        "{ambiguous}"
    );
    store
        .set_enabled_for("mcp:other", false, &group.group_id, "user", "actor", 3600)
        .expect("remove second binding");
    store
        .set_blocked_for("mcp:fixture", true, &group.group_id, "fixture", "user", 0)
        .expect("block");
    let blocked = crate::handle_request_for_actor(
        &home,
        &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
            "name":"cccc_ext_fixture_0500","arguments":{}
        }}),
        &group.group_id,
        "user",
    )
    .await;
    assert_eq!(blocked["result"]["isError"], true);
    assert!(
        blocked.to_string().contains("capability_tool_not_found"),
        "{blocked}"
    );

    // Use a release identity with a local installed fixture; no upstream request is needed.
    let published = store
        .catalog()
        .expect("catalog")
        .into_iter()
        .find(|capability| {
            capability.kind == "mcp_toolpack"
                && capability.is_release_recommendation()
                && capability.metadata["support"]["auth"] == "none"
        })
        .expect("anonymous release MCP");
    let name = cccc_core::capabilities::external_tool_name(&published.id, "echo");
    let mut runtime =
        cccc_core::fs::read_json::<Value>(&home.root().join("state/capabilities/runtime.json"))
            .expect("current runtime");
    runtime["artifacts"]["release-fixture"] = json!({"state":"installed",
        "tools":[{"name":name,"real_tool_name":"echo","inputSchema":{"type":"object"}}],
        "invoker":{"type":"command_stdio","command":command}});
    runtime["capability_artifacts"][&published.id] = json!("release-fixture");
    cccc_core::fs::write_json(
        &home.root().join("state/capabilities/runtime.json"),
        &runtime,
    )
    .expect("release fixture");
    store
        .set_enabled_for(&published.id, true, &group.group_id, "user", "actor", 3600)
        .expect("release binding");
    let prepared = super::call(
        &home,
        &client,
        "cccc_capability_use",
        json!({"group_id":group.group_id,"by":"user","capability_id":published.id})
            .as_object()
            .expect("prepare args")
            .clone(),
    )
    .await
    .expect("prepare release MCP");
    assert_eq!(prepared["structuredContent"]["tool_called"], false);
    let release_direct = crate::handle_request_for_actor(
        &home,
        &json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{
            "name":name,"arguments":{"value":42}}}),
        &group.group_id,
        "user",
    )
    .await;
    let release_use = crate::handle_request_for_actor(
        &home,
        &json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{
            "name":"cccc_capability_use","arguments":{"capability_id":published.id,
                "tool_name":"echo","tool_arguments":{"value":42}}}}),
        &group.group_id,
        "user",
    )
    .await;
    let direct_payload = &release_direct["result"]["structuredContent"];
    let use_payload = &release_use["result"]["structuredContent"];
    for (key, value) in cccc_core::capability_recommendations::notes(&published) {
        assert_eq!(prepared["structuredContent"][&key], value);
        assert_eq!(direct_payload[&key], value);
        assert_eq!(use_payload[&key], value);
        assert!(
            use_payload["tool_result"].get(&key).is_none(),
            "notes only at the outer level"
        );
        assert!(
            direct_payload["result"].get(&key).is_none(),
            "upstream result stays intact"
        );
    }
    assert_eq!(
        direct_payload["result"],
        direct["result"]["structuredContent"]["result"]
    );
    let mut expected_tool_result = direct_payload.clone();
    for key in cccc_core::capability_recommendations::NOTE_FIELDS {
        expected_tool_result
            .as_object_mut()
            .expect("direct payload")
            .remove(key);
    }
    assert_eq!(use_payload["tool_result"], expected_tool_result);
    assert_eq!(
        release_use["result"]["isError"], true,
        "tool errors retain both error status and notes"
    );
    let received = std::fs::read_to_string(temp.path().join("evidence.jsonl"))
        .expect("external receipts")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("receipt"))
        .filter_map(|row| row.get("arguments").cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        &received[received.len() - 2..],
        &[json!({"value":42}), json!({"value":42})]
    );
    daemon_task.abort();
}

#[tokio::test]
async fn capability_use_enables_hidden_pack_and_calls_inferred_builtin() {
    if crate::lib_tests::isolate_embedded_daemon_test(concat!(
        module_path!(),
        "::",
        "capability_use_enables_hidden_pack_and_calls_inferred_builtin"
    )) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.create("capability use", ""))
        .expect("group");
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

    let context = Some(RequestContext {
        native_binding: None,
        binding: None,
        group_id: &group.group_id,
        actor_id: "user",
    });
    let enabled = super::call_with_context(
        &home,
        &client,
        "cccc_capability_use",
        json!({"capability_id":"pack:space","scope":"session"})
            .as_object()
            .cloned()
            .expect("enable args"),
        context,
        false,
    )
    .await
    .expect("enable pack");
    assert_eq!(enabled["structuredContent"]["enabled"], true);
    assert_eq!(enabled["structuredContent"]["capability_id"], "pack:space");
    assert_eq!(enabled["structuredContent"]["tool_called"], false);

    let state = super::daemon(
        &client,
        "capability_state",
        json!({"group_id":group.group_id,"actor_id":"user","by":"user"})
            .as_object()
            .cloned()
            .expect("state args"),
    )
    .await
    .expect("capability state");
    assert!(
        state["enabled_capabilities"]
            .as_array()
            .expect("enabled capabilities")
            .contains(&json!("pack:space"))
    );

    let called = super::call_with_context(
        &home,
        &client,
        "cccc_capability_use",
        json!({"tool_name":"cccc_context_sync","tool_arguments":{
            "dry_run":true,"ops":[{"op":"coordination.brief.update","current_focus":"fixture"}]
        }})
        .as_object()
        .cloned()
        .expect("call args"),
        context,
        false,
    )
    .await
    .expect("call inferred built-in");
    assert_eq!(
        called["structuredContent"]["capability_id"],
        "pack:context-advanced"
    );
    assert_eq!(called["structuredContent"]["tool_called"], true);
    assert_eq!(
        called["structuredContent"]["tool_name"],
        "cccc_context_sync"
    );

    daemon_task.abort();
}

#[tokio::test]
async fn bound_notifications_reach_their_recipients_and_never_widen_a_target() {
    if crate::lib_tests::isolate_embedded_daemon_test(concat!(
        module_path!(),
        "::",
        "bound_notifications_reach_their_recipients_and_never_widen_a_target"
    )) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("groups");
    let mut group = store.create("notify routing", "").expect("group");
    for id in ["lead", "peer", "other"] {
        cccc_core::actors::add(&mut group, cccc_contracts::Actor::new(id)).expect("actor");
    }
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

    let context = Some(RequestContext {
        native_binding: None,
        binding: None,
        group_id: &group.group_id,
        actor_id: "lead",
    });
    for (route, recipients) in [
        // No target is a broadcast to everyone but the sender.
        (json!({}), vec!["peer", "other"]),
        (json!({"target_actor_id":"peer"}), vec!["peer"]),
        // A target naming the sender reaches no one rather than everyone.
        (json!({"target_actor_id":"lead"}), vec![]),
        // The caller's identity must not override the recipients it names.
        (json!({"to":["peer"]}), vec!["peer"]),
    ] {
        let mut args = route.as_object().cloned().expect("route");
        args.insert("title".into(), json!("Build finished"));
        let result = super::call_with_context(&home, &client, "cccc_notify", args, context, false)
            .await
            .expect("notify");
        let event: cccc_contracts::Event =
            serde_json::from_value(result["structuredContent"]["event"].clone())
                .expect("stored notification");
        assert_eq!(event.kind, "system.notify", "{route}");
        assert_eq!(event.by, "lead", "{route}");
        assert!(!event.data.contains_key("actor_id"), "{route}");
        let reached: Vec<_> = ["lead", "peer", "other"]
            .into_iter()
            .filter(|actor| cccc_core::inbox::is_for_actor(&group, &event, actor))
            .collect();
        assert_eq!(reached, recipients, "{route}");
    }

    // Delivery bookkeeping may still be appended, so count notifications.
    let ledger = store.ledger_path(&group.group_id).expect("ledger path");
    let notifications = || {
        cccc_core::ledger::read_all(&ledger)
            .expect("ledger")
            .iter()
            .filter(|event| event.kind == "system.notify")
            .count()
    };
    let before = notifications();
    for refused in [
        // The tool promises an internal notification.
        json!({"im_visibility":"public"}),
        // Routing cannot read these, and would treat them as no target.
        json!({"target_actor_id":["peer"]}),
        json!({"to":[7]}),
    ] {
        let mut args = refused.as_object().cloned().expect("refused");
        args.insert("title".into(), json!("Build finished"));
        assert!(
            super::call_with_context(&home, &client, "cccc_notify", args, context, false)
                .await
                .is_err(),
            "{refused}"
        );
    }
    assert_eq!(notifications(), before);

    daemon_task.abort();
}

#[tokio::test]
async fn user_control_plane_routes_an_explicit_cross_group_target() {
    if crate::lib_tests::isolate_embedded_daemon_test(concat!(
        module_path!(),
        "::",
        "user_control_plane_routes_an_explicit_cross_group_target"
    )) {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("groups");
    let focus = store.create("focus", "").expect("focus group");
    let target = store.create("target", "").expect("target group");
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

    let listed = super::call(
        &home,
        &client,
        "cccc_group",
        json!({"action":"list","by":"user"})
            .as_object()
            .cloned()
            .expect("list arguments"),
    )
    .await
    .expect("list groups");
    let listed_ids = listed["structuredContent"]["groups"]
        .as_array()
        .expect("listed groups")
        .iter()
        .filter_map(|group| group["group_id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(listed_ids.contains(focus.group_id.as_str()));
    assert!(listed_ids.contains(target.group_id.as_str()));

    let mut arguments = json!({
        "action":"info",
        "group_id":target.group_id,
        "actor_id":"user",
        "by":"intruder"
    })
    .as_object()
    .cloned()
    .expect("arguments");
    apply_actor_context(&mut arguments, Some("user"));
    let result = super::call(&home, &client, "cccc_group", arguments)
        .await
        .expect("explicit cross-group query");

    assert_eq!(
        result["structuredContent"]["group"]["group_id"],
        target.group_id
    );
    assert_ne!(
        result["structuredContent"]["group"]["group_id"],
        focus.group_id
    );
    daemon_task.abort();
}

#[test]
fn runtime_actor_is_authoritative_without_replacing_explicit_targets() {
    let mut args = json!({
        "group_id":"g_explicit",
        "actor_id":"target-peer",
        "by":"intruder"
    })
    .as_object()
    .cloned()
    .expect("args");

    apply_actor_context(&mut args, Some("user"));

    assert_eq!(args["group_id"], "g_explicit");
    assert_eq!(args["actor_id"], "target-peer");
    assert_eq!(args["by"], "user");
}

#[test]
fn runtime_actor_populates_missing_self_context() {
    let mut args = serde_json::Map::new();

    apply_actor_context(&mut args, Some("backend"));

    assert_eq!(args["actor_id"], "backend");
    assert_eq!(args["by"], "backend");
}

#[test]
fn model_arguments_cannot_supply_human_profile_authority() {
    let mut args = json!({
        "caller_id":"profile-owner",
        "is_admin":true,
        "allowed_groups":["g_other"],
        "view":"all"
    })
    .as_object()
    .expect("args")
    .clone();

    remove_untrusted_authority(&mut args);

    assert!(!args.contains_key("caller_id"));
    assert!(!args.contains_key("is_admin"));
    assert!(!args.contains_key("allowed_groups"));
    assert_eq!(args["view"], "all");
}

#[test]
fn request_scoped_actor_binding_overrides_model_controlled_identity() {
    let mut args = json!({"group_id":"other","actor_id":"other","by":"user"})
        .as_object()
        .cloned()
        .expect("args");

    apply_request_context(
        "cccc_message_send",
        &mut args,
        RequestContext {
            native_binding: None,
            binding: None,
            group_id: "bound-group",
            actor_id: "bound-actor",
        },
    );

    assert_eq!(args["group_id"], "bound-group");
    assert_eq!(args["actor_id"], "bound-actor");
    assert_eq!(args["by"], "bound-actor");
}

#[test]
fn request_context_separates_management_targets_from_identity() {
    for name in [
        "cccc_actor",
        "cccc_message_send",
        "cccc_runtime_complete_turn",
        "cccc_capability_use",
        "cccc_inbox_read",
    ] {
        let mut args = json!({"group_id":"other","actor_id":"beta","by":"user"})
            .as_object()
            .expect("arguments")
            .clone();
        apply_request_context(
            name,
            &mut args,
            RequestContext {
                group_id: "bound-group",
                actor_id: "alpha",
                native_binding: None,
                binding: None,
            },
        );
        assert_eq!(args["group_id"], "bound-group");
        assert_eq!(args["by"], "alpha", "{name} cannot elevate the caller");
        assert_eq!(
            args["actor_id"],
            if name == "cccc_actor" {
                "beta"
            } else {
                "alpha"
            },
            "{name}"
        );
    }
}

#[test]
fn task_create_defaults_peer_assignment_and_list_hides_archived_tasks() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let group = store.create("tasks", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            cccc_core::actors::add(group, cccc_contracts::Actor::new("lead"))?;
            cccc_core::actors::add(group, cccc_contracts::Actor::new("peer"))?;
            Ok(())
        })
        .expect("actors");
    let mut args = json!({
        "action":"create","group_id":group.group_id,"actor_id":"peer","by":"peer",
        "title":"self-owned"
    })
    .as_object()
    .cloned()
    .expect("args");
    prepare_task_arguments(&home, &mut args).expect("prepare task");
    assert_eq!(args["assignee"], "peer");

    let mut result = json!({
        "tasks":[
            {"id":"T001","status":"planned"},
            {"id":"T002","status":"archived"}
        ]
    })
    .as_object()
    .cloned()
    .expect("result");
    postprocess_task_result(
        &mut result,
        &json!({"action":"list"})
            .as_object()
            .cloned()
            .expect("list args"),
    )
    .expect("filter");
    assert_eq!(result["tasks"].as_array().expect("tasks").len(), 1);
    assert_eq!(result["tasks"][0]["id"], "T001");
}

#[test]
fn task_reads_keep_parent_details_and_only_summarize_children() {
    let original = json!({
        "task": {
            "id": "T100", "title": "Release", "status": "active", "rev": 9,
            "notes": "Keep the parent's complete working history",
            "outcome": "Package is awaiting review",
            "checklist": [{"text": "Review the package", "status": "pending"}],
            "children": [
                {"id": "T101", "title": "Check delivery", "status": "planned",
                 "parent_id": "T100", "assignee": "worker", "waiting_on": "user", "rev": 4,
                 "notes": "The child's detailed history", "outcome": "Not delivered yet",
                 "checklist": [{"text": "Build", "status": "done"},
                               {"text": "Smoke the package", "status": "pending"}]},
                {"id": "T102", "title": "Old investigation", "status": "archived", "rev": 2,
                 "notes": "Archived child details", "outcome": "Investigation complete"}
            ]
        },
        "tasks_version": "tasksv:17",
        "delete_info": {"allowed": false, "total": 3, "reason": "has_children"}
    });
    let mut expected = original.clone();
    expected["task"]["children"] = json!([
        {"id": "T101", "title": "Check delivery", "status": "planned",
         "parent_id": "T100", "assignee": "worker", "waiting_on": "user",
         "checklist": "1/2", "rev": 4},
        {"id": "T102", "title": "Old investigation", "status": "archived", "rev": 2}
    ]);
    expected["note"] =
        json!("Rows are summaries; read the full task by task_id before updating its fields.");
    for action in [None, Some("list"), Some("get")] {
        let mut arguments = json!({"task_id": "T100"});
        if let Some(action) = action {
            arguments["action"] = json!(action);
        }
        let mut result = original.as_object().expect("response").clone();
        postprocess_task_result(&mut result, arguments.as_object().expect("arguments"))
            .expect("task projection");
        assert_eq!(Value::Object(result), expected);
    }
}

#[test]
fn task_leaf_reads_and_write_responses_keep_their_full_fields() {
    let leaf = json!({"task": {
        "id": "T101", "status": "planned", "rev": 4, "children": [],
        "notes": "Read the full child before editing it", "outcome": "Pending",
        "checklist": [{"text": "Smoke the package", "status": "pending"}]
    }, "tasks_version": "tasksv:17"});
    let mut read = leaf.as_object().expect("leaf response").clone();
    postprocess_task_result(
        &mut read,
        json!({"action": "list", "task_id": "T101"})
            .as_object()
            .expect("read arguments"),
    )
    .expect("full leaf read");
    assert_eq!(Value::Object(read), leaf);

    let mut parent = leaf;
    parent["task"]["children"] = json!([
        {"id": "T102", "notes": "Write responses are not read projections",
         "checklist": [{"text": "Review", "status": "pending"}]}
    ]);
    for action in ["create", "update", "move"] {
        let mut result = parent.as_object().expect("write response").clone();
        postprocess_task_result(
            &mut result,
            json!({"action": action})
                .as_object()
                .expect("write arguments"),
        )
        .expect("unmodified write response");
        assert_eq!(Value::Object(result), parent);
    }
}

#[test]
fn identifies_message_operations_for_compact_mail_context() {
    assert!(is_message_operation("cccc_message_send"));
    assert!(is_message_operation("cccc_file_send"));
    assert!(!is_message_operation("cccc_file"));
}

#[test]
fn message_results_preserve_receipts_and_errors_without_behavior_instructions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    for payload in [
        json!({"event":{"id":"event-1"}}),
        json!({"receipt":{"status":"retrying"}}),
        json!({"sent":true,"result":{"event":{"id":"event-2"}}}),
        json!({"partial_failure":true,"event":{"id":"event-1"}}),
        json!({"message_sent":false}),
        json!({"receipt":{"status":"failed"}}),
        json!({"error":{"code":"invalid_recipient","message":"unknown actor"}}),
    ] {
        let result = with_post_message_context(
            &home,
            super::tool_result(payload.clone()),
            "",
            "",
            "cccc_message_send",
            &Default::default(),
        );
        assert_eq!(result["structuredContent"], payload);
        let text: serde_json::Value =
            serde_json::from_str(result["content"][0]["text"].as_str().expect("text"))
                .expect("payload JSON");
        assert_eq!(text, payload);
    }
}

#[test]
fn pending_mail_context_preserves_message_outcome() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("message context", "").expect("group");
    cccc_core::actors::add(&mut group, cccc_contracts::Actor::new("peer1")).expect("add peer");
    store.save(&group).expect("save group");
    let mut mail = cccc_contracts::Event::new("chat.message", &group.group_id);
    mail.by = "user".into();
    mail.data = json!({"to":["peer1"],"text":"later","message_mode":"mail"})
        .as_object()
        .cloned()
        .expect("mail data");
    cccc_core::ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &mail)
        .expect("append mail");

    let fields = crate::response_projection::EchoedFields::capture(
        json!({"text":"my update"}).as_object().expect("args"),
    );
    let success = with_post_message_context(
        &home,
        super::tool_result(
            json!({"event":{"id":"event-1","kind":"chat.message","data":{"text":"my update"}}}),
        ),
        &group.group_id,
        "peer1",
        "cccc_message_send",
        &fields,
    );
    assert_eq!(success["structuredContent"]["event"]["id"], "event-1");
    assert!(
        success["structuredContent"]
            .get("post_message_nudge")
            .is_none()
    );
    assert_eq!(success["structuredContent"]["mail_pending"]["count"], 1);
    assert!(
        success["structuredContent"]["event"]["data"]
            .get("text")
            .is_none()
    );
    assert_eq!(
        success["structuredContent"]["mcp_omitted_fields"]["event"],
        json!(["text"])
    );
    let content: serde_json::Value = serde_json::from_str(
        success["content"][0]["text"]
            .as_str()
            .expect("text content"),
    )
    .expect("JSON payload");
    assert_eq!(content, success["structuredContent"]);

    let incomplete = with_post_message_context(
        &home,
        super::tool_result(json!({"partial_failure":true})),
        &group.group_id,
        "peer1",
        "cccc_message_send",
        &Default::default(),
    );
    assert!(incomplete["structuredContent"]["post_message_nudge"].is_null());
    assert_eq!(incomplete["structuredContent"]["mail_pending"]["count"], 1);
}

#[test]
fn identifies_task_completion_and_block_boundaries_for_mail_context() {
    for arguments in [
        json!({"action":"move","status":"done"}),
        json!({"action":"update","waiting_on":"external"}),
        json!({"action":"update","blocked_by":["T001"]}),
    ] {
        assert!(is_task_mail_boundary(
            arguments.as_object().expect("arguments")
        ));
    }
    assert!(!is_task_mail_boundary(
        json!({"action":"update","status":"active"})
            .as_object()
            .expect("active arguments")
    ));
}

#[test]
fn help_uses_the_complete_shared_peer_insight_contract() {
    let help = help_markdown();
    for required in [
        "one move on a living\ndecision path",
        "where reality could break it",
        "switch to Plan B",
        "one fallible projection of the situation",
        "do not inherit the level or frame it claims",
        "clear-sighted, exacting supervisor",
    ] {
        assert!(help.contains(required), "missing help contract: {required}");
    }
    assert_eq!(help.matches("## Peer Insight Contract").count(), 1);
}
#[tokio::test]
async fn delegated_and_direct_external_results_preserve_skipped_verification() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for skipped in [false, true] {
        for failed in [false, true] {
            let temp = tempfile::tempdir().expect("fixture");
            let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
            let group = cccc_core::GroupStore::new(home.clone())
                .expect("groups")
                .create("MCP outcome fixture", "")
                .expect("group");
            let id = "mcp:test:skipped-proof";
            cccc_core::capabilities::CapabilityStore::new(home.clone())
                .import_record(json!({"capability_id":id,"kind":"mcp_toolpack","name":"Fixture"}))
                .expect("record");
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("IPC fixture");
            cccc_core::fs::write_json(&cccc_client::address_path(&home), &json!({
                "v":1,"transport":"tcp","host":"127.0.0.1","port":listener.local_addr().expect("address").port(),
                "pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":"2026-11-15T00:00:00Z"
            })).expect("address");
            let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let seen = calls.clone();
            let server = tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.expect("accept");
                    let mut stream = BufReader::new(stream);
                    let mut line = String::new();
                    stream.read_line(&mut line).await.expect("request");
                    let request: cccc_contracts::DaemonRequest =
                        serde_json::from_str(&line).expect("request JSON");
                    let payload = match request.op.as_str() {
                        "capability_state" => json!({"enabled_capabilities":[id]}),
                        "capability_tool_call" => {
                            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let mut result = json!({"capability_id":id,"tool_name":request.args["tool_name"],
                                "result":{"isError":failed,"content":[{"type":"text","text":"actual external result"}]}});
                            if skipped {
                                result["state_update_skipped"] = json!(true);
                                result["state_update_skipped_reason"] =
                                    json!("installation_changed");
                            }
                            result
                        }
                        other => panic!("unexpected fixture operation {other}"),
                    };
                    let response = cccc_contracts::DaemonResponse::success(
                        payload.as_object().expect("result").clone(),
                    );
                    let mut bytes = serde_json::to_vec(&response).expect("response JSON");
                    bytes.push(b'\n');
                    stream.get_mut().write_all(&bytes).await.expect("response");
                }
            });
            let client = cccc_client::DaemonClient::new(home.clone()).without_voice_origin();
            let context = || {
                Some(RequestContext {
                    native_binding: None,
                    binding: None,
                    group_id: &group.group_id,
                    actor_id: "user",
                })
            };
            let delegated = super::call_with_context(
                &home,
                &client,
                "cccc_capability_use",
                json!({"capability_id":id,"tool_name":"echo","arguments":{}})
                    .as_object()
                    .expect("args")
                    .clone(),
                context(),
                false,
            )
            .await
            .expect("delegated call");
            let payload = &delegated["structuredContent"];
            assert_eq!(
                payload["state"],
                if skipped {
                    "state_update_skipped"
                } else if failed {
                    "runnable"
                } else {
                    "verified"
                }
            );
            if skipped {
                assert_eq!(payload["state_update_skipped"], true);
                assert_eq!(
                    payload["state_update_skipped_reason"],
                    "installation_changed"
                );
                assert!(
                    payload.get("enabled").is_none(),
                    "a historical enable is not proof of a current binding"
                );
            }
            assert_eq!(delegated["isError"] == true, failed);
            let direct = super::call_with_context(
                &home,
                &client,
                &cccc_core::capabilities::external_tool_name(id, "echo"),
                Map::new(),
                context(),
                false,
            )
            .await
            .expect("direct call");
            assert_eq!(
                direct["structuredContent"]["state_update_skipped"] == true,
                skipped
            );
            assert_eq!(
                direct["structuredContent"]["result"]["content"][0]["text"],
                "actual external result"
            );
            assert_eq!(direct["isError"] == true, failed);
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
            server.abort();
            let _ = server.await;
        }
    }
}
