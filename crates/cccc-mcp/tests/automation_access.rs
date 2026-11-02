//! The automation tool through the real MCP admission path and a real daemon:
//! what an Actor may change follows the authenticated Actor, not the arguments
//! a model sends. No provider or terminal is started.
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};

async fn automation(home: &HomeLayout, group_id: &str, actor: &str, arguments: Value) -> Value {
    cccc_mcp::handle_request_for_actor(
        home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"cccc_automation","arguments":arguments}}),
        group_id,
        actor,
    )
    .await["result"]
        .clone()
}

/// The route a Web Model Actor takes: its remote schema lists only the core
/// tools, so it reaches `cccc_automation` through `cccc_capability_use`.
async fn automation_via_capability_use(
    home: &HomeLayout,
    group_id: &str,
    actor: &str,
    arguments: Value,
) -> Value {
    cccc_mcp::handle_request_for_actor(
        home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"cccc_capability_use","arguments":{
                "tool_name":"cccc_automation","tool_arguments":arguments}}}),
        group_id,
        actor,
    )
    .await["result"]
        .clone()
}

fn rule<'a>(result: &'a Value, id: &str) -> Option<&'a Value> {
    result["structuredContent"]["ruleset"]["rules"]
        .as_array()?
        .iter()
        .find(|rule| rule["id"] == id)
}

#[tokio::test]
async fn automation_permissions_follow_the_authenticated_actor() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("automation access", "").expect("group");
    for id in ["lead", "peer1"] {
        cccc_core::actors::add(&mut group, cccc_contracts::Actor::new(id)).expect("actor");
    }
    let mut web_peer = cccc_contracts::Actor::new("web1");
    // The runtime of the report: a Grok Web Model peer. The router unit test covers
    // the ChatGPT Web Model.
    web_peer.runtime = cccc_contracts::ActorRuntime::GrokWebModel;
    cccc_core::actors::add(&mut group, web_peer).expect("web model peer");
    store.save(&group).expect("save");
    let group_id = group.group_id.as_str();
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    let request = |op: &str, args: Value| DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_default(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while client.call(&request("ping", json!({}))).await.is_err() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("daemon ready");
    let seeded = client
        .call(&request(
            "group_automation_manage",
            json!({"group_id":group_id,"by":"user","actions":[{"type":"create_rule","rule":{
                "id":"nightly-pause","trigger":{"kind":"at","at":"2030-01-01T00:00:00Z"},
                "action":{"kind":"group_state","state":"paused"}
            }}]}),
        ))
        .await
        .expect("seed");
    assert!(seeded.ok, "{:?}", seeded.error);

    // A peer sets a reminder for itself with the tool's defaults.
    let own = automation(
        &home,
        group_id,
        "peer1",
        json!({"action":"manage","actions":[{"type":"create_rule","rule":{
            "id":"peer1-ci","trigger":{"kind":"interval","every_seconds":600},
            "action":{"kind":"notify","message":"check CI"}
        }}]}),
    )
    .await;
    assert_ne!(own["isError"], true, "{own}");
    let reminder = rule(&own, "peer1-ci").expect("saved reminder");
    assert_eq!(reminder["scope"], "personal");
    assert_eq!(reminder["to"], json!(["peer1"]));
    let version = own["structuredContent"]["version"].clone();

    // Claiming to be the user does not change who calls.
    let forged = automation(
        &home,
        group_id,
        "peer1",
        json!({"action":"manage","by":"user",
            "actions":[{"type":"delete_rule","rule_id":"nightly-pause"}]}),
    )
    .await;
    assert_eq!(forged["isError"], true, "{forged}");
    assert!(forged.to_string().contains("permission_denied"), "{forged}");

    // Nor may the foreman change the user's scheduled operation, and the
    // reminder in the same refused batch is not saved either.
    let foreman = automation(
        &home,
        group_id,
        "lead",
        json!({"action":"manage","actions":[
            {"type":"create_rule","rule":{"id":"lead-note","trigger":{"kind":"interval","every_seconds":60},
                "action":{"kind":"notify","message":"note"}}},
            {"type":"set_rule_enabled","rule_id":"nightly-pause","enabled":false}
        ]}),
    )
    .await;
    assert_eq!(foreman["isError"], true, "{foreman}");
    assert!(
        foreman.to_string().contains("permission_denied"),
        "{foreman}"
    );

    // The refused batches above wrote nothing.
    let before_web = automation(&home, group_id, "lead", json!({"action":"state"})).await;
    assert_eq!(before_web["structuredContent"]["version"], version);

    // A Web Model peer cannot name the tool directly, but through capability use
    // it sets a reminder for itself, as any peer can.
    let direct = automation(&home, group_id, "web1", json!({"action":"state"})).await;
    assert_eq!(direct["isError"], true, "{direct}");
    let web_own = automation_via_capability_use(
        &home,
        group_id,
        "web1",
        json!({"action":"manage","actions":[{"type":"create_rule","rule":{
            "id":"web1-hourly","trigger":{"kind":"interval","every_seconds":3600},
            "action":{"kind":"notify","message":"hourly check"}
        }}]}),
    )
    .await;
    assert_ne!(web_own["isError"], true, "{web_own}");
    // Capability use returns the tool's own result under `tool_result`.
    let web_result = json!({"structuredContent": web_own["structuredContent"]["tool_result"]});
    let web_reminder = rule(&web_result, "web1-hourly").expect("saved Web Model reminder");
    assert_eq!(web_reminder["scope"], "personal");
    assert_eq!(web_reminder["to"], json!(["web1"]));
    let web_version = web_result["structuredContent"]["version"].clone();

    // The route grants no more than a peer holds: a reminder for another Actor,
    // or one claimed as the user, is refused and nothing is written.
    for (id, arguments) in [
        (
            "web1-nudge",
            json!({"action":"manage","actions":[{"type":"create_rule","rule":{
            "id":"web1-nudge","scope":"group","to":["peer1"],
            "trigger":{"kind":"interval","every_seconds":600},
            "action":{"kind":"notify","message":"nudge"}}}]}),
        ),
        (
            "web1-as-user",
            json!({"action":"manage","by":"user","group_id":group_id,
            "actions":[{"type":"create_rule","rule":{"id":"web1-as-user","to":["@all"],
            "trigger":{"kind":"interval","every_seconds":600},
            "action":{"kind":"notify","message":"all hands"}}}]}),
        ),
    ] {
        let refused = automation_via_capability_use(&home, group_id, "web1", arguments).await;
        assert_eq!(refused["isError"], true, "{id}: {refused}");
        assert!(
            refused.to_string().contains("permission_denied"),
            "{id}: {refused}"
        );
        let after = automation(&home, group_id, "lead", json!({"action":"state"})).await;
        assert!(rule(&after, id).is_none(), "{id} was saved");
    }

    let state = automation(&home, group_id, "lead", json!({"action":"state"})).await;
    daemon.abort();
    let _ = daemon.await;
    assert_eq!(state["structuredContent"]["version"], web_version);
    assert_eq!(
        rule(&state, "nightly-pause").expect("kept")["enabled"],
        true
    );
    assert!(rule(&state, "lead-note").is_none());
}
