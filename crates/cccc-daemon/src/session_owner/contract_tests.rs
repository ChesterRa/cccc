//! Owner contract 1 as its owners and daemons write it. A release that still
//! takes over contract 1 owners must read and write these messages unchanged:
//! an owner kept from an earlier release keeps its own code until it stops.

use super::protocol::{CONTRACT, Request, Response};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// Written by a running contract 1 owner; see fixtures/contract1/README.md.
const HELLO: &str = include_str!("fixtures/contract1/hello.json");
const SNAPSHOT: &str = include_str!("fixtures/contract1/snapshot.json");

/// Read a message with this release's types and write it again. Contract 1
/// needs the same JSON back, so a lost, changed or added field fails.
fn reread<T: Serialize + DeserializeOwned>(written: &Value) -> Result<T, String> {
    let message: T = serde_json::from_value(written.clone()).map_err(|error| error.to_string())?;
    let again = serde_json::to_value(&message).map_err(|error| error.to_string())?;
    if again == *written {
        Ok(message)
    } else {
        Err(format!("written {written}\nread back as {again}"))
    }
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("contract 1 message")
}

#[test]
fn this_release_takes_over_contract_1_owners() {
    assert_eq!(
        CONTRACT, 1,
        "a new owner contract decides what happens to owners kept from the previous release \
         (see Contract versions in docs/standards/CCCC_SESSION_OWNER_V1.md); keep \
         fixtures/contract1 as the sample of contract 1 instead of regenerating it"
    );
}

#[test]
fn a_contract_1_hello_is_read_without_loss() {
    let Response::Hello {
        contract,
        instance,
        pid,
        build,
        accepting_starts,
        mcp_registration,
    } = reread::<Response>(&json(HELLO)).expect("contract 1 Hello")
    else {
        panic!("not a Hello");
    };
    assert_eq!(
        (contract, instance.as_str(), pid),
        (1, "98ab8ea9bc2e46cc8a5c592dc6ef5ef9", 120)
    );
    assert!(accepting_starts);
    assert_eq!(mcp_registration, 1);
    // The controller compares this key with its own build before reusing the
    // owner for a new launch.
    assert!(build["source_id"].as_str().is_some_and(|id| id.len() == 64));
}

#[test]
fn a_contract_1_snapshot_is_read_without_loss() {
    let Response::Snapshot { snapshot } =
        reread::<Response>(&json(SNAPSHOT)).expect("contract 1 Snapshot")
    else {
        panic!("not a Snapshot");
    };
    let members = snapshot
        .actors
        .iter()
        .map(|member| member.actor.actor_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(members, ["controlled", "structured", "terminal"]);
    let controlled = &snapshot.actors[0];
    let mcp = controlled.mcp.as_ref().expect("controlled MCP binding");
    assert_eq!(mcp.source_controlled, Some(true));
    assert_eq!(mcp.processes.len(), 1);
    assert!(
        controlled
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.running)
    );
    assert!(controlled.managed_status.is_some() && controlled.workspace.is_some());
    let structured = &snapshot.actors[1];
    assert!(structured.terminal.is_none());
    assert_eq!(
        structured.structured.as_ref().expect("ACP state")["runner"],
        "headless"
    );
    let terminal = &snapshot.actors[2];
    assert!(terminal.terminal_launch.is_some() && terminal.managed_status.is_none());
    let [browser] = snapshot.browsers.as_slice() else {
        panic!("one kept browser");
    };
    assert!(browser.reattached);
    assert_eq!(
        browser.resource.state,
        super::browser::BrowserResourceState::Ready
    );
    let connection = browser.resource.connection.as_ref().expect("connection");
    assert_eq!(connection.vnc_port, Some(46189));
    let [page] = browser.resource.sessions.as_slice() else {
        panic!("one kept page");
    };
    assert_eq!(
        (page.key.as_str(), page.actor.is_none()),
        ("web-model-login", true)
    );
    assert_eq!(browser.identities().count(), 0);
}

/// The check above has to notice a type that no longer reads a field, a field
/// that a contract 1 owner never sends, and a renamed variant.
#[test]
fn a_changed_reader_is_noticed() {
    let mut lost = json(SNAPSHOT);
    lost["actors"][0]["ready_for_mail_notice_v2"] = Value::Bool(true);
    assert!(reread::<Response>(&lost).is_err());
    let mut missing = json(SNAPSHOT);
    missing["actors"][0]
        .as_object_mut()
        .expect("member")
        .remove("provider_exited");
    assert!(reread::<Response>(&missing).is_err());
    let mut renamed = json(SNAPSHOT);
    let terminal = renamed["actors"][2]["owner"]["identity"]["Pty"].take();
    renamed["actors"][2]["owner"]["identity"] = serde_json::json!({ "Terminal": terminal });
    assert!(reread::<Response>(&renamed).is_err());
}

/// What a newer daemon, Web or MCP server sends a kept contract 1 owner: take
/// it over, settle the input it already admitted, show and answer its members,
/// use its kept browser, review it and stop it. These encodings are fixed by
/// hand, so they notice a change, not whether a contract 1 owner accepts it.
/// Updates that kept real agents sent only some of these operations to such an
/// owner. New input carries the Group, Actor and event types that the ledger
/// keeps compatible, and new members and browsers start under the newer
/// release's own owner, so neither is fixed here.
#[test]
fn contract_1_requests_keep_their_encoding() {
    let actor = r#"{"group_id":"g_b2f072eca5b4","actor_id":"terminal","actor_created_at":"2026-11-19T01:02:38.953557Z","generation":"2026-11-19T01:02:39.011764Z"}"#;
    let operation = r#"{"instance":"98ab8ea9bc2e46cc8a5c592dc6ef5ef9","epoch":3,"sequence":7}"#;
    let source = format!(
        r#"{{"actor":{actor},"event_ids":["2f4c6a1e9b8d4c3fa1b2c3d4e5f60718"],"input_sha256":"{}"}}"#,
        "0".repeat(64)
    );
    let page = serde_json::to_string(&json(SNAPSHOT)["browsers"][0]["resource"]["sessions"][0])
        .expect("page");
    let mutate = |mutation: &str| {
        format!(r#"{{"op":"mutate","epoch":3,"sequence":8,"mutation":{mutation}}}"#)
    };
    let requests = [
        r#"{"op":"hello","contract":1}"#.to_owned(),
        r#"{"op":"acquire","contract":1,"instance":"98ab8ea9bc2e46cc8a5c592dc6ef5ef9"}"#.to_owned(),
        r#"{"op":"snapshot"}"#.to_owned(),
        r#"{"op":"browser_resources"}"#.to_owned(),
        format!(r#"{{"op":"query","operation":{operation}}}"#),
        format!(r#"{{"op":"find","source":{source}}}"#),
        r#"{"op":"find_event","group_id":"g_b2f072eca5b4","actor_id":"terminal","actor_created_at":"2026-11-19T01:02:38.953557Z","event_id":"2f4c6a1e9b8d4c3fa1b2c3d4e5f60718"}"#.to_owned(),
        mutate(&format!(r#"{{"kind":"acknowledge","operation":{operation}}}"#)),
        mutate(&format!(
            r#"{{"kind":"browser_pages_set","resource_id":"b1c67e1f4b454168b8a0a5426747a277","instance":"98ab8ea9bc2e46cc8a5c592dc6ef5ef9","sessions":[{page}]}}"#
        )),
        mutate(r#"{"kind":"browser_close","resource_id":"b1c67e1f4b454168b8a0a5426747a277","instance":"98ab8ea9bc2e46cc8a5c592dc6ef5ef9"}"#),
        mutate(&format!(r#"{{"kind":"stop","actor":{actor}}}"#)),
        mutate(r#"{"kind":"stop_all"}"#),
        mutate(r#"{"kind":"retire"}"#),
        format!(r#"{{"op":"terminal_read","actor":{actor},"read":{{"kind":"since","after":10,"limit":100}}}}"#),
        mutate(&format!(r#"{{"kind":"terminal","actor":{actor},"action":{{"kind":"write","data":[104,105]}}}}"#)),
        mutate(&format!(r#"{{"kind":"terminal","actor":{actor},"action":{{"kind":"resize","cols":120,"rows":40}}}}"#)),
        mutate(&format!(r#"{{"kind":"permission","actor":{actor},"request_id":"r1","allow":true}}"#)),
        mutate(&format!(r#"{{"kind":"interaction","actor":{actor},"request_id":"r2","reply":{{"answer":"yes"}}}}"#)),
        mutate(&format!(r#"{{"kind":"cancel_turn","actor":{actor}}}"#)),
        mutate(&format!(r#"{{"kind":"ensure_viewer","actor":{actor}}}"#)),
        mutate(&format!(r#"{{"kind":"retry_confirmation","actor":{actor}}}"#)),
        r#"{"op":"recovery_list"}"#.to_owned(),
        mutate(r#"{"kind":"recovery","action":{"Retry":{"id":"record","version":"2"}}}"#),
        format!(
            r#"{{"op":"mcp_register","report":{}}}"#,
            json(SNAPSHOT)["actors"][0]["mcp"]["processes"][0]["report"]
        ),
    ];
    for request in requests {
        if let Err(error) = reread::<Request>(&json(&request)) {
            panic!("{request}: {error}");
        }
    }
}

/// What a contract 1 owner answers to those requests.
#[test]
fn contract_1_answers_keep_their_encoding() {
    let operation = r#"{"instance":"98ab8ea9bc2e46cc8a5c592dc6ef5ef9","epoch":3,"sequence":7}"#;
    let fact = format!(
        r#"{{"operation":{operation},"source":{{"actor":{{"group_id":"g_b2f072eca5b4","actor_id":"terminal","actor_created_at":"2026-11-19T01:02:38.953557Z","generation":null}},"event_ids":["2f4c6a1e9b8d4c3fa1b2c3d4e5f60718"],"input_sha256":"{}"}},"state":{{"state":"unknown"}}}}"#,
        "0".repeat(64)
    );
    let answers = [
        r#"{"result":"acquired","epoch":3}"#.to_owned(),
        format!(r#"{{"result":"admitted","operation":{operation}}}"#),
        r#"{"result":"operation","state":{"state":"pending"}}"#.to_owned(),
        r#"{"result":"operation","state":{"state":"finished","result":{"Ok":{"accepted":true}}}}"#
            .to_owned(),
        r#"{"result":"operation","state":{"state":"finished","result":{"Err":{"code":"stopped","message":"the member stopped"}}}}"#.to_owned(),
        r#"{"result":"operation","state":{"state":"not_admitted"}}"#.to_owned(),
        format!(r#"{{"result":"found","operations":[{operation}]}}"#),
        format!(r#"{{"result":"found_event","operations":[{fact}]}}"#),
        r#"{"result":"acknowledged"}"#.to_owned(),
        r#"{"result":"retired"}"#.to_owned(),
        r#"{"result":"failed","error":{"code":"incompatible_contract","message":"session owner contract is incompatible"}}"#.to_owned(),
        format!(
            r#"{{"result":"browser_resources","resources":[{}]}}"#,
            json(SNAPSHOT)["browsers"][0]["resource"]
        ),
        r#"{"result":"mcp_registered","registration":1,"instance":"98ab8ea9bc2e46cc8a5c592dc6ef5ef9","launch_id":"e4c458fc9b1f478586e431ee87a0e5e9"}"#.to_owned(),
        r#"{"result":"terminal","value":{"after":10,"chunks":[]}}"#.to_owned(),
    ];
    for answer in answers {
        if let Err(error) = reread::<Response>(&json(&answer)) {
            panic!("{answer}: {error}");
        }
    }
}
