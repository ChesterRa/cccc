use super::{cached_tools, prepare};
use crate::ops::codex_voice_analyst::managed_runtime;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use cccc_contracts::{Actor, ActorRuntime, DaemonRequest, DaemonResponse};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, capabilities::CapabilityStore};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const ENV_KEY: &str = "CCCC_FIXTURE_BEARER";
const TOKEN_A: &str = "fixture-member-alpha-private";
const TOKEN_B: &str = "fixture-member-beta-private";
const TOKEN_ROTATED: &str = "fixture-member-rotated-private";
const TOKEN_PROFILE: &str = "fixture-profile-private";
const CAPABILITY: &str = "mcp:test:member-bearer";

#[derive(Clone, Copy, Default)]
enum Reply {
    #[default]
    Success,
    Http(StatusCode, bool),
    Rpc,
}

struct Observed {
    method: String,
    authorization: String,
}

#[derive(Default)]
struct Capture {
    requests: Vec<Observed>,
    reply: Reply,
}

async fn endpoint(
    State(state): State<Arc<Mutex<Capture>>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> axum::response::Response {
    let method = request["method"].as_str().unwrap_or("");
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let reply = {
        let mut capture = state.lock().expect("capture");
        capture.requests.push(Observed {
            method: method.into(),
            authorization: authorization.into(),
        });
        capture.reply
    };
    if let Reply::Http(status, challenge) = reply {
        let mut response = axum::http::Response::builder().status(status);
        if challenge {
            response = response.header(
                "WWW-Authenticate",
                "Bearer realm=\"fixture\", error=\"insufficient_scope\"",
            );
        }
        return response
            .body(axum::body::Body::from(format!(
                "upstream diagnostic mentions HTTP 401: {TOKEN_A}"
            )))
            .expect("HTTP fixture response")
            .into_response();
    }
    let token = authorization.strip_prefix("Bearer ").unwrap_or("");
    let tool = match token {
        TOKEN_A => "member_a",
        TOKEN_B => "member_b",
        TOKEN_ROTATED => "member_rotated",
        TOKEN_PROFILE => "member_profile",
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };
    if method == "notifications/initialized" {
        return StatusCode::ACCEPTED.into_response();
    }
    if matches!(reply, Reply::Rpc) && method == "tools/list" {
        return Json(json!({"jsonrpc":"2.0","id":request["id"],
            "error":{"code":-32603,"message":format!("body says HTTP 401: {token}")}}))
        .into_response();
    }
    let result = match method {
        "initialize" => json!({"protocolVersion":request["params"]["protocolVersion"],
            "capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}),
        "tools/list" => json!({"tools":[{"name":tool,
            "description":format!("member-visible description {token}"),
            "inputSchema":{"type":"object","properties":{"value":{"type":"integer"}}}}]}),
        "tools/call" if request["params"]["name"] == tool => json!({
            "content":[{"type":"text","text":format!("member result {token}")}],
            "structuredContent":{"value":request["params"]["arguments"]["value"],
                "echoed_credential":token},"isError":false}),
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
}

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    group: GroupDoc,
    record: Value,
    capture: Arc<Mutex<Capture>>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temporary home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("groups");
        let mut group = store
            .create("Member credential fixture", "")
            .expect("group");
        for id in ["member-a", "member-b", "native"] {
            let mut actor = Actor::new(id);
            actor.runtime = ActorRuntime::Codex;
            actor.generation = format!("fixture-{id}");
            actor.enabled = false;
            if id == "native" {
                actor.external_session_id = "fixture-external-session".into();
            }
            group.actors.push(actor);
        }
        store.save(&group).expect("actors");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listen");
        listener.set_nonblocking(true).expect("nonblocking");
        let url = format!("http://{}/mcp", listener.local_addr().expect("address"));
        let capture = Arc::new(Mutex::new(Capture::default()));
        let app = Router::new()
            .route(
                "/mcp",
                post(endpoint)
                    .get(|| async { StatusCode::METHOD_NOT_ALLOWED })
                    .delete(|| async { StatusCode::OK }),
            )
            .with_state(capture.clone());
        let task = managed_runtime().spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("async listener");
            axum::serve(listener, app).await.expect("fixture server");
        });
        let record = json!({"capability_id":CAPABILITY,"kind":"mcp_toolpack",
            "name":"Member credential fixture","install_mode":"remote_only",
            "install_spec":{"url":url},"support":{"form":"remote_http","auth":"bearer",
                "credential":{"kind":"bearer","env":ENV_KEY}},
            "qualification_status":"qualified","enable_supported":true});
        CapabilityStore::new(home.clone())
            .import_record(record.clone())
            .expect("public record");
        let fixture = Self {
            _temp: temp,
            home,
            group,
            record,
            capture,
            task,
        };
        fixture.set_secret("member-a", TOKEN_A);
        fixture.set_secret("member-b", TOKEN_B);
        // A secret in a native actor's store must not make that route supported.
        fixture.set_secret("native", TOKEN_A);
        fixture
    }

    fn set_secret(&self, actor: &str, token: &str) {
        crate::ops::actor_secrets::replace(
            &self.home,
            &self.group.group_id,
            actor,
            BTreeMap::from([(ENV_KEY.into(), token.into())]),
        )
        .expect("fixture private environment");
    }

    fn request(&self, op: &str, args: Value) -> DaemonResponse {
        crate::dispatch::dispatch(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("arguments").clone(),
            },
        )
    }

    fn enable_group(&self) -> DaemonResponse {
        let response = self.request(
            "capability_enable",
            json!({"group_id":self.group.group_id,
            "actor_id":"member-a","by":"user","scope":"group","capability_id":CAPABILITY}),
        );
        assert!(
            response.ok,
            "human group enable must succeed without discovery"
        );
        response
    }

    fn requests(&self) -> usize {
        self.capture.lock().expect("capture").requests.len()
    }

    fn execution_record(&self, record: &Value) -> Value {
        super::installed_record(&self.home, record).expect("installed public recipe")
    }

    fn for_you(&self, actor: &str) -> Value {
        let store = CapabilityStore::new(self.home.clone());
        let group = GroupStore::new(self.home.clone())
            .expect("groups")
            .load(&self.group.group_id)
            .expect("current group");
        super::super::projection::Projection::new(
            &self.home,
            &store,
            &self.group.group_id,
            Some(&group),
        )
        .expect("member projection")
        .for_you(&store.require(CAPABILITY).expect("capability"), actor)
    }

    fn assert_shared_files_have_no_credentials(&self) {
        let directory = self.home.root().join("state/capabilities");
        for entry in std::fs::read_dir(directory).expect("capability directory") {
            let path = entry.expect("entry").path();
            if path.is_file() {
                assert_no_credentials(&std::fs::read_to_string(path).expect("shared state"));
            }
        }
        let ledger = GroupStore::new(self.home.clone())
            .expect("groups")
            .ledger_path(&self.group.group_id)
            .expect("ledger path");
        if ledger.exists() {
            assert_no_credentials(&std::fs::read_to_string(ledger).expect("ledger"));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn assert_no_credentials(text: &str) {
    for token in [TOKEN_A, TOKEN_B, TOKEN_ROTATED, TOKEN_PROFILE] {
        // Do not print the response, raw header or credential on failure.
        assert!(
            !text.contains(token),
            "credential escaped into a shared result"
        );
    }
}

fn names(tools: &[Value]) -> Vec<&str> {
    tools
        .iter()
        .filter_map(|tool| tool["real_tool_name"].as_str())
        .collect()
}

#[test]
fn member_discovery_is_isolated_and_rotating_a_secret_invalidates_the_cached_list() {
    let fixture = Fixture::new();
    let a = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &fixture.record,
    )
    .expect("member A discovery");
    let b = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-b",
        &fixture.record,
    )
    .expect("member B discovery");
    assert_eq!(names(&a), ["member_a"]);
    assert_eq!(names(&b), ["member_b"]);
    assert_no_credentials(&serde_json::to_string(&a).expect("member A tool list JSON"));
    assert_no_credentials(&serde_json::to_string(&b).expect("member B tool list JSON"));
    fixture.set_secret("member-a", TOKEN_ROTATED);
    let before = fixture.requests();
    assert!(
        cached_tools(
            &fixture.home,
            &fixture.group.group_id,
            "member-a",
            &fixture.record
        )
        .is_empty()
    );
    assert_eq!(
        names(&cached_tools(
            &fixture.home,
            &fixture.group.group_id,
            "member-b",
            &fixture.record
        )),
        ["member_b"]
    );
    assert_eq!(
        fixture.requests(),
        before,
        "reading a cache must not issue discovery"
    );
    let rotated = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &fixture.record,
    )
    .expect("explicit discovery after rotation");
    assert_eq!(names(&rotated), ["member_rotated"]);
    assert!(fixture.requests() > before);
}

#[test]
fn linked_profile_credentials_win_over_dormant_custom_secrets() {
    let mut fixture = Fixture::new();
    let profiles = cccc_core::profiles::ProfileStore::new(fixture.home.clone()).expect("profiles");
    profiles
        .upsert(
            json!({"id":"credential-profile","scope":"user","owner_id":"fixture-owner",
        "name":"Credential profile","runtime":"codex"})
            .as_object()
            .expect("profile object")
            .clone(),
            None,
        )
        .expect("profile");
    profiles
        .replace_secrets_ref(
            "credential-profile",
            "user",
            "fixture-owner",
            BTreeMap::from([(ENV_KEY.into(), TOKEN_PROFILE.into())]),
        )
        .expect("profile private environment");
    let actor = fixture
        .group
        .actors
        .iter_mut()
        .find(|actor| actor.id == "member-a")
        .expect("fixture member A");
    actor.profile_id = "credential-profile".into();
    actor.profile_scope = "user".into();
    actor.profile_owner = "fixture-owner".into();
    GroupStore::new(fixture.home.clone())
        .expect("group store")
        .save(&fixture.group)
        .expect("profile link");
    fixture.enable_group();
    let before = fixture.requests();
    let view = fixture.for_you("member-a");
    assert_eq!(view["credential_source"], "profile");
    assert_eq!(view["credential_env"], ENV_KEY);
    assert_eq!(view["activation_pending"], true);
    assert_eq!(fixture.requests(), before);
    let execution = fixture.execution_record(&fixture.record);
    let tools = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &execution,
    )
    .expect("profile discovery");
    assert_eq!(names(&tools), ["member_profile"]);
    let before = fixture.requests();
    assert_eq!(fixture.for_you("member-a")["activation_pending"], false);
    assert_eq!(fixture.requests(), before);
    let captured = fixture.capture.lock().expect("capture");
    assert!(!captured.requests.is_empty());
    assert!(
        captured
            .requests
            .iter()
            .all(|row| row.authorization == format!("Bearer {TOKEN_PROFILE}")),
        "profile route must not use a dormant custom secret"
    );
}

#[test]
fn human_and_native_callers_cannot_borrow_a_managed_members_identity() {
    let fixture = Fixture::new();
    fixture.enable_group();
    let tools = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &fixture.record,
    )
    .expect("managed member discovery");
    let name = tools[0]["name"].as_str().expect("discovered tool name");
    let before = fixture.requests();
    for caller in ["user", "native"] {
        assert_eq!(
            prepare(
                &fixture.home,
                &fixture.group.group_id,
                caller,
                &fixture.record
            )
            .expect_err("unsupported credential identity")
            .code,
            "credential_route_not_supported"
        );
        let response = fixture.request(
            "capability_tool_call",
            json!({"group_id":fixture.group.group_id,
            "actor_id":"member-a","by":caller,"capability_id":CAPABILITY,
            "tool_name":name,"arguments":{"value":42}}),
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("unsupported caller error").code,
            "credential_route_not_supported"
        );
    }
    let untrusted = fixture.request(
        "capability_tool_call",
        json!({"group_id":fixture.group.group_id,
            "actor_id":"member-a","capability_id":CAPABILITY,
            "tool_name":name,"arguments":{"value":42}}),
    );
    assert!(
        !untrusted.ok,
        "actor_id alone must not supply a Bearer caller identity"
    );
    assert_eq!(
        untrusted.error.expect("untrusted caller error").code,
        "credential_route_not_supported"
    );
    assert_eq!(
        fixture.requests(),
        before,
        "unsupported callers must not reach the endpoint"
    );
}

#[test]
fn blocked_member_enable_sends_no_credentials_and_unblocked_prepare_still_works() {
    for global in [true, false] {
        let fixture = Fixture::new();
        let store = CapabilityStore::new(fixture.home.clone());
        let blocked_group = if global {
            ""
        } else {
            fixture.group.group_id.as_str()
        };
        store
            .set_blocked_for(CAPABILITY, true, blocked_group, "fixture policy", "user", 0)
            .expect("block capability");
        let args = json!({"group_id":fixture.group.group_id,"actor_id":"member-a",
            "by":"member-a","scope":"session","capability_id":CAPABILITY,"prepare":true});
        let response = fixture.request("capability_enable", args.clone());
        assert!(response.ok, "blocked enable retains its policy response");
        assert_eq!(response.result["state"], "blocked");
        assert_eq!(response.result["enabled"], false);
        assert_eq!(
            response.result["blocked_scope"],
            if global { "global" } else { "group" }
        );
        assert_eq!(
            fixture.requests(),
            0,
            "blocked preparation must not send a token"
        );
        assert!(
            super::super::package_install::installed_artifact(&fixture.home, CAPABILITY)
                .expect("installation state")
                .is_none(),
            "blocked preparation must not install an artifact"
        );
        assert!(
            !store
                .is_enabled_for(CAPABILITY, &fixture.group.group_id, "member-a", "session")
                .expect("binding state")
        );

        store
            .set_blocked_for(CAPABILITY, false, blocked_group, "", "user", 0)
            .expect("unblock capability");
        let response = fixture.request("capability_enable", args);
        assert!(response.ok, "unblocked member preparation must work");
        assert_eq!(response.result["enabled"], true);
        assert_eq!(
            names(
                response.result["dynamic_tools"]
                    .as_array()
                    .expect("member tools")
            ),
            ["member_a"]
        );
        assert!(
            fixture.requests() > 0,
            "the positive control must reach the server"
        );
        fixture.assert_shared_files_have_no_credentials();
    }
}

#[test]
fn unqualified_member_enable_does_not_install_or_discover() {
    for (field, value) in [
        ("enable_supported", json!(false)),
        ("qualification_status", json!("blocked")),
    ] {
        let fixture = Fixture::new();
        let store = CapabilityStore::new(fixture.home.clone());
        let mut record = fixture.record.clone();
        record[field] = value;
        store.import_record(record).expect("unqualified record");
        let response = fixture.request(
            "capability_enable",
            json!({"group_id":fixture.group.group_id,"actor_id":"member-a","by":"member-a",
                "scope":"session","capability_id":CAPABILITY,"prepare":true}),
        );
        assert!(!response.ok, "qualification must reject preparation");
        assert_eq!(
            response.error.expect("qualification error").code,
            "capability_not_enableable"
        );
        assert_eq!(
            fixture.requests(),
            0,
            "unqualified preparation must not send a token"
        );
        assert!(
            super::super::package_install::installed_artifact(&fixture.home, CAPABILITY)
                .expect("installation state")
                .is_none()
        );
    }
}

#[test]
fn foreman_group_enable_records_intent_without_using_any_member_credential() {
    for has_credential in [false, true] {
        let mut fixture = Fixture::new();
        fixture.group.actors[0].role = Some(cccc_contracts::ActorRole::Foreman);
        GroupStore::new(fixture.home.clone())
            .expect("groups")
            .save(&fixture.group)
            .expect("foreman role");
        if !has_credential {
            crate::ops::actor_secrets::replace(
                &fixture.home,
                &fixture.group.group_id,
                "member-a",
                BTreeMap::new(),
            )
            .expect("foreman without a credential; peer retains its own");
        }
        let response = fixture.request(
            "capability_enable",
            json!({"group_id":fixture.group.group_id,"actor_id":"member-a","by":"member-a",
                "scope":"group","capability_id":CAPABILITY,"prepare":true}),
        );
        assert!(
            response.ok,
            "foreman group intent must not require its token"
        );
        assert_eq!(response.result["enabled"], true);
        assert_eq!(response.result["state"], "activation_pending");
        assert_eq!(
            fixture.requests(),
            0,
            "Group enable must not discover as any member"
        );
        assert!(response.result.get("dynamic_tools").is_none());
        assert!(
            CapabilityStore::new(fixture.home.clone())
                .is_enabled_for(CAPABILITY, &fixture.group.group_id, "member-a", "group")
                .expect("group binding")
        );
        fixture.assert_shared_files_have_no_credentials();
    }
}

#[test]
fn direct_bearer_alias_recovers_after_only_its_member_cache_is_lost() {
    let fixture = Fixture::new();
    fixture.enable_group();
    let tools = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &fixture.execution_record(&fixture.record),
    )
    .expect("initial discovery");
    let name = tools[0]["name"].as_str().expect("advertised name");
    super::cache()
        .lock()
        .expect("cache")
        .entries
        .retain(|scope, _| scope.home.as_path() != fixture.home.root());
    let before = fixture.requests();
    let invalid_name = format!(
        "{}{}{}",
        &name[..9],
        if name.as_bytes()[9] == b'0' { '1' } else { '0' },
        &name[10..]
    );
    let rejected = fixture.request(
        "capability_tool_call",
        json!({"group_id":fixture.group.group_id,"by":"member-a","tool_name":invalid_name,
            "arguments":{"value":40}}),
    );
    assert!(!rejected.ok, "a forged hash must not select a capability");
    assert_eq!(
        fixture.requests(),
        before,
        "ownership lookup must not probe endpoints"
    );
    let rejected = fixture.request(
        "capability_tool_call",
        json!({"group_id":fixture.group.group_id,"by":"member-a",
            "tool_name":cccc_core::capabilities::external_tool_name(CAPABILITY,"member-a"),
            "arguments":{"value":40}}),
    );
    assert!(
        !rejected.ok,
        "a name with lost punctuation needs explicit preparation"
    );
    assert!(
        rejected
            .error
            .expect("direct-name error")
            .message
            .contains("cccc_capability_use"),
        "the failure must explain the preparation path"
    );
    assert_eq!(fixture.requests(), before);

    let response = fixture.request(
        "capability_tool_call",
        json!({"group_id":fixture.group.group_id,"by":"member-a","tool_name":name,
            "arguments":{"value":41}}),
    );
    assert!(
        response.ok,
        "a verifiable alias must recover without an explicit capability id"
    );
    assert_eq!(response.result["result"]["structuredContent"]["value"], 41);
    let capture = fixture.capture.lock().expect("capture");
    let received = &capture.requests[before..];
    assert!(
        received
            .iter()
            .any(|request| request.method == "tools/list")
    );
    assert!(
        received
            .iter()
            .any(|request| request.method == "tools/call")
    );
    assert!(
        received
            .iter()
            .all(|request| request.authorization == format!("Bearer {TOKEN_A}")),
        "cold discovery and call must retain the same member identity"
    );
    drop(capture);
    assert_no_credentials(&serde_json::to_string(&response).expect("response"));
    fixture.assert_shared_files_have_no_credentials();
}

#[test]
fn human_group_enable_persists_only_the_public_recipe_and_binding_without_discovery() {
    let fixture = Fixture::new();
    let store = CapabilityStore::new(fixture.home.clone());
    store
        .restore_record(CAPABILITY, None)
        .expect("fresh import");
    let imported = fixture.request(
        "capability_import",
        json!({"group_id":fixture.group.group_id,"actor_id":"user","by":"user",
            "scope":"group","record":fixture.record,"probe":true}),
    );
    assert!(
        imported.ok,
        "the real import entry must accept a Bearer env reference"
    );
    assert_eq!(imported.result["probe"]["state"], "configured");
    assert_eq!(
        store
            .resolved_record(CAPABILITY)
            .expect("read imported capability")
            .expect("imported capability")["support"],
        fixture.record["support"]
    );
    let response = fixture.enable_group();
    assert_eq!(fixture.requests(), 0);
    assert_no_credentials(&serde_json::to_string(&imported).expect("import response JSON"));
    assert_no_credentials(&serde_json::to_string(&response).expect("group enable response JSON"));
    let installed = super::super::package_install::installed_artifact(&fixture.home, CAPABILITY)
        .expect("installation")
        .expect("public installation");
    assert_eq!(
        installed["invoker"]["url"],
        fixture.record["install_spec"]["url"]
    );
    assert_eq!(
        installed["invoker"]["credential"],
        json!({"kind":"bearer","env":ENV_KEY})
    );
    assert!(installed["tools"].as_array().is_some_and(Vec::is_empty));
    assert!(
        CapabilityStore::new(fixture.home.clone())
            .is_enabled_for(CAPABILITY, &fixture.group.group_id, "member-a", "group")
            .expect("group binding lookup")
    );
    assert!(
        cached_tools(
            &fixture.home,
            &fixture.group.group_id,
            "member-a",
            &fixture.record
        )
        .is_empty()
    );
    fixture.assert_shared_files_have_no_credentials();
    let before = fixture.requests();
    let view = fixture.for_you("member-a");
    assert_eq!(view["credential_env"], ENV_KEY);
    assert_eq!(view["credential_source"], "actor");
    assert_eq!(view["activation_pending"], true);
    assert_eq!(
        fixture.requests(),
        before,
        "projection must not discover tools"
    );
    let execution = fixture.execution_record(&fixture.record);
    prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &execution,
    )
    .expect("member explicitly activates discovery");
    let before = fixture.requests();
    assert_eq!(fixture.for_you("member-a")["activation_pending"], false);
    crate::ops::actor_secrets::replace(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        BTreeMap::new(),
    )
    .expect("remove fixture credential");
    let view = fixture.for_you("member-a");
    assert_eq!(view["availability"], "needs_setup");
    assert_eq!(view["reason_codes"], json!(["missing_member_credential"]));
    assert_eq!(view["activation_pending"], false);
    assert_eq!(
        fixture.requests(),
        before,
        "projection after removal must remain read-only"
    );
}

#[test]
fn explicit_member_prepare_and_call_share_authorization_and_never_persist_the_secret() {
    let fixture = Fixture::new();
    let enabled = fixture.enable_group();
    let execution = fixture.execution_record(&fixture.record);
    let tools = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &execution,
    )
    .expect("explicit member discovery");
    let response = fixture.request(
        "capability_tool_call",
        json!({"group_id":fixture.group.group_id,
        "actor_id":"member-b","by":"member-a","capability_id":CAPABILITY,
        "tool_name":tools[0]["name"],"arguments":{"value":42}}),
    );
    assert!(
        response.ok,
        "member call must use its own identity despite a different target actor_id"
    );
    assert_eq!(response.result["result"]["structuredContent"]["value"], 42);
    assert_eq!(
        response.result["result"]["structuredContent"]["echoed_credential"],
        "<redacted>"
    );
    for value in [json!(enabled), json!(tools), json!(response)] {
        assert_no_credentials(&value.to_string());
    }
    let captured = fixture.capture.lock().expect("capture");
    for method in ["initialize", "tools/list", "tools/call"] {
        assert!(
            captured.requests.iter().any(|row| row.method == method),
            "missing request: {method}"
        );
    }
    assert_eq!(
        captured
            .requests
            .iter()
            .filter(|row| row.method == "tools/list")
            .count(),
        1,
        "calling a known tool reuses only the current identity's discovery"
    );
    assert_eq!(
        captured
            .requests
            .iter()
            .filter(|row| row.method == "tools/call")
            .count(),
        1
    );
    assert!(
        captured
            .requests
            .iter()
            .all(|row| row.authorization == format!("Bearer {TOKEN_A}")),
        "all discovery and call posts must use the same member credential"
    );
    drop(captured);
    fixture.assert_shared_files_have_no_credentials();
}

#[test]
fn only_http_auth_statuses_are_classified_as_credential_failures() {
    let fixture = Fixture::new();
    for (reply, expected) in [
        (
            Reply::Http(StatusCode::UNAUTHORIZED, false),
            "credential_rejected",
        ),
        (
            Reply::Http(StatusCode::UNAUTHORIZED, true),
            "credential_rejected",
        ),
        (
            Reply::Http(StatusCode::FORBIDDEN, false),
            "credential_permission_denied",
        ),
        (
            Reply::Http(StatusCode::FORBIDDEN, true),
            "credential_permission_denied",
        ),
        (
            Reply::Http(StatusCode::INTERNAL_SERVER_ERROR, false),
            "capability_transport_error",
        ),
        (Reply::Rpc, "capability_tool_failed"),
    ] {
        fixture.capture.lock().expect("capture").reply = reply;
        let error = prepare(
            &fixture.home,
            &fixture.group.group_id,
            "member-a",
            &fixture.record,
        )
        .expect_err("fixture failure");
        assert_no_credentials(&error.message);
        assert_eq!(error.code, expected, "classification: {}", error.message);
        assert_eq!(error.details["reason_codes"], json!([expected]));
        assert_no_credentials(&error.message);
        assert_no_credentials(&serde_json::to_string(&error.details).expect("error details JSON"));
        assert!(
            cached_tools(
                &fixture.home,
                &fixture.group.group_id,
                "member-a",
                &fixture.record
            )
            .is_empty()
        );
    }
}

#[test]
fn bearer_update_switches_only_the_public_recipe_then_rediscovers_as_the_calling_member() {
    let fixture = Fixture::new();
    let next_endpoint = Fixture::new();
    fixture.enable_group();
    let installed = fixture.execution_record(&fixture.record);
    let tools = prepare(
        &fixture.home,
        &fixture.group.group_id,
        "member-a",
        &installed,
    )
    .expect("old endpoint discovery");
    assert_eq!(fixture.for_you("member-a")["activation_pending"], false);
    let mut edited = fixture.record.clone();
    edited["install_spec"]["url"] = next_endpoint.record["install_spec"]["url"].clone();
    CapabilityStore::new(fixture.home.clone())
        .import_record(edited.clone())
        .expect("new public candidate recipe");
    let call_args = json!({"group_id":fixture.group.group_id,"by":"member-a",
        "actor_id":"member-a","capability_id":CAPABILITY,
        "tool_name":tools[0]["name"],"arguments":{"value":42}});
    let before_old = fixture.requests();
    let ordinary = fixture.request("capability_tool_call", call_args.clone());
    assert!(
        ordinary.ok,
        "ordinary use must keep using the installed recipe"
    );
    assert!(fixture.requests() > before_old);
    assert_eq!(
        next_endpoint.requests(),
        0,
        "catalog edits must not silently switch endpoints"
    );
    let before_old = fixture.requests();
    let fingerprint = super::super::recipe::fingerprint(&edited).expect("new recipe fingerprint");
    let updated = fixture.request(
        "capability_update",
        json!({"capability_id":CAPABILITY,
        "by":"user","expected_recipe_fingerprint":fingerprint}),
    );
    assert!(
        updated.ok,
        "explicit Bearer update changes only the public recipe"
    );
    assert_eq!(updated.result["state"], "configured");
    assert_eq!(updated.result["updated"], true);
    assert_eq!(fixture.requests(), before_old);
    assert_eq!(
        next_endpoint.requests(),
        0,
        "updating must not borrow a member identity"
    );
    let execution = fixture.execution_record(&edited);
    assert_eq!(
        execution["install_spec"]["url"],
        edited["install_spec"]["url"]
    );
    assert!(
        cached_tools(
            &fixture.home,
            &fixture.group.group_id,
            "member-a",
            &execution
        )
        .is_empty()
    );
    assert!(
        super::super::external_runtime::all_dynamic_tools(
            &fixture.home,
            &fixture.group.group_id,
            "member-a",
            &[CAPABILITY.into()]
        )
        .expect("visible tools after update")
        .is_empty()
    );
    assert_eq!(fixture.for_you("member-a")["activation_pending"], true);
    assert_eq!(fixture.requests(), before_old);
    assert_eq!(
        next_endpoint.requests(),
        0,
        "pending projection must not send HTTP"
    );
    let called = fixture.request("capability_tool_call", call_args);
    assert!(
        called.ok,
        "the next explicit use discovers the new endpoint as self"
    );
    assert_eq!(called.result["result"]["structuredContent"]["value"], 42);
    assert_eq!(
        fixture.requests(),
        before_old,
        "the old endpoint must no longer be used"
    );
    assert!(next_endpoint.requests() > 0);
    assert_eq!(fixture.for_you("member-a")["activation_pending"], false);
    let capture = next_endpoint.capture.lock().expect("capture");
    assert_eq!(
        capture
            .requests
            .iter()
            .filter(|row| row.method == "tools/call")
            .count(),
        1
    );
    assert!(
        capture
            .requests
            .iter()
            .all(|row| row.authorization == format!("Bearer {TOKEN_A}")),
        "the new endpoint must receive the same caller's credential"
    );
    drop(capture);
    for response in [ordinary, updated, called] {
        assert_no_credentials(&serde_json::to_string(&response).expect("update response JSON"));
    }
    fixture.assert_shared_files_have_no_credentials();
}
