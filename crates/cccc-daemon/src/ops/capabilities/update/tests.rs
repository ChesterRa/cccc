use super::*;
use crate::ops::capabilities::{mcp_stdio, package_install, recipe};

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    record: Value,
    evidence: std::path::PathBuf,
    first: String,
    second: String,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        let first = groups.create("first", "").expect("first").group_id;
        let second = groups.create("second", "").expect("second").group_id;
        let script = temp.path().join("server.py");
        std::fs::write(&script, mcp_stdio::STDIO_FIXTURE).expect("server");
        let evidence = temp.path().join("evidence.jsonl");
        let python = ["python3", "python"]
            .into_iter()
            .find(|name| {
                std::process::Command::new(name)
                    .arg("--version")
                    .output()
                    .is_ok_and(|result| result.status.success())
            })
            .expect("Python");
        let record = json!({"capability_id":"mcp:test:update","kind":"mcp_toolpack","name":"Fixture",
            "install_mode":"command","install_spec":{"command_candidates":[[python,"-u",script,mode,evidence]]}});
        let store = CapabilityStore::new(home.clone());
        store.import_record(record.clone()).expect("record");
        store
            .set_enabled_for("mcp:test:update", true, &first, "user", "group", 3600)
            .expect("group binding");
        store
            .set_enabled_for("mcp:test:update", true, &second, "user", "actor", 3600)
            .expect("actor binding");
        write_json(&home.root().join("state/capabilities/runtime.json"),&json!({
            "artifacts":{"old":{"artifact_id":"old","state":"installed","recipe_fingerprint":"old-recipe","invoker":{"type":"command_stdio","command":["old"]}},
                "older":{"artifact_id":"older","state":"installed"}},
            "capability_artifacts":{"mcp:test:update":"old","mcp:test:shared":"old"},
            "actor_instances":{
                first.clone():{"user":{"mcp:test:update":{"artifact_id":"old","state":"verified"},"mcp:test:shared":{"artifact_id":"old"}}},
                second.clone():{"user":{"mcp:test:update":{"artifact_id":"older","state":"verified"}}}
            }
        })).expect("runtime");
        Self {
            _temp: temp,
            home,
            record,
            evidence,
            first,
            second,
        }
    }

    fn request(&self, by: Value, expected: &str) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: "capability_update".into(),
            args: json!({"capability_id":"mcp:test:update",
            "by":by,"expected_recipe_fingerprint":expected})
            .as_object()
            .expect("args")
            .clone(),
        }
    }
    fn state(&self) -> Value {
        read_json(&self.home.root().join("state/capabilities/runtime.json")).expect("runtime")
    }
}

#[test]
fn update_switches_every_same_capability_pointer_and_keeps_other_references() {
    let fixture = Fixture::new("normal");
    let fingerprint = recipe::fingerprint(&fixture.record).expect("recipe");
    let result = run(&fixture.home, &fixture.request(json!("user"), &fingerprint)).expect("update");
    assert_eq!(result["updated"], true);
    assert_eq!(result["enabled_group_count"], 2);
    let state = fixture.state();
    let installed = state["capability_artifacts"]["mcp:test:update"]
        .as_str()
        .expect("installed id");
    assert_ne!(installed, "old");
    assert_eq!(
        state["actor_instances"][&fixture.first]["user"]["mcp:test:update"]["artifact_id"],
        installed
    );
    assert_eq!(
        state["actor_instances"][&fixture.second]["user"]["mcp:test:update"]["artifact_id"],
        installed
    );
    assert_eq!(state["capability_artifacts"]["mcp:test:shared"], "old");
    assert_eq!(
        state["actor_instances"][&fixture.first]["user"]["mcp:test:shared"]["artifact_id"],
        "old"
    );
    assert!(state["artifacts"].get("old").is_some());
    assert!(state["artifacts"].get("older").is_none());
    assert_eq!(
        state["artifacts"][installed]["recipe_fingerprint"],
        fingerprint
    );
}

#[test]
fn rejected_or_failed_updates_preserve_the_running_configuration() {
    let fixture = Fixture::new("exit");
    let path = fixture.home.root().join("state/capabilities/runtime.json");
    let before = std::fs::read(&path).expect("before");
    let fingerprint = recipe::fingerprint(&fixture.record).expect("fingerprint");
    for by in [json!("peer"), json!("system"), json!(false)] {
        let error = run(&fixture.home, &fixture.request(by, &fingerprint)).expect_err("user-only");
        assert_eq!(error.code, "permission_denied");
    }
    let error = run(
        &fixture.home,
        &fixture.request(json!("user"), "stale-preview"),
    )
    .expect_err("changed recipe");
    assert_eq!(error.code, "capability_recipe_conflict");
    assert!(!fixture.evidence.exists());
    let mut authenticated = fixture.record.clone();
    authenticated["support"] = json!({"auth":"bearer"});
    CapabilityStore::new(fixture.home.clone())
        .import_record(authenticated)
        .expect("authenticated fixture import");
    let error = run(&fixture.home, &fixture.request(json!("user"), &fingerprint))
        .expect_err("no credential route");
    assert_eq!(error.code, "capability_update_unsupported");
    assert!(!fixture.evidence.exists());
    CapabilityStore::new(fixture.home.clone())
        .import_record(fixture.record.clone())
        .expect("restore original fixture");
    let mut state = fixture.state();
    state["artifacts"]["older"]["invoker"] = json!({"token":"fixture-private-member"});
    write_json(&path, &state).expect("write authenticated runtime");
    let error = run(&fixture.home, &fixture.request(json!("user"), &fingerprint))
        .expect_err("a different member artifact is authenticated");
    assert_eq!(error.code, "capability_update_unsupported");
    assert!(!fixture.evidence.exists());
    std::fs::write(&path, &before).expect("restore original runtime");
    let error = run(&fixture.home, &fixture.request(json!("user"), &fingerprint))
        .expect_err("failed probe");
    assert!(error.message.contains("enable prereleases"));
    assert_eq!(std::fs::read(&path).expect("after"), before);
}

#[test]
fn ordinary_use_and_metadata_edits_never_upgrade_an_installation() {
    let fixture = Fixture::new("normal");
    let old = package_install::ensure_installed(&fixture.home, "mcp:test:update", &fixture.record)
        .expect("ordinary enable")
        .expect("cached");
    assert_eq!(old["artifact_id"], "old");
    assert!(!fixture.evidence.exists());
    let fingerprint = recipe::fingerprint(&fixture.record).expect("fingerprint");
    let mut state = fixture.state();
    state["artifacts"]["old"]["recipe_fingerprint"] = json!(fingerprint);
    let path = fixture.home.root().join("state/capabilities/runtime.json");
    write_json(&path, &state).expect("same recipe");
    let before = std::fs::read(&path).expect("before");
    let mut record = fixture.record.clone();
    record["description_short"] = json!("a new recommendation reason");
    CapabilityStore::new(fixture.home.clone())
        .import_record(record)
        .expect("metadata edit");
    let result =
        run(&fixture.home, &fixture.request(json!("user"), &fingerprint)).expect("unchanged");
    assert_eq!(result["updated"], false);
    assert!(!fixture.evidence.exists());
    assert_eq!(std::fs::read(&path).expect("unchanged"), before);
}

#[test]
fn enabled_group_count_uses_all_live_binding_scopes_without_double_counting() {
    let fixture = Fixture::new("normal");
    let third = cccc_core::GroupStore::new(fixture.home.clone())
        .expect("groups")
        .create("third", "")
        .expect("third")
        .group_id;
    let path = fixture.home.root().join("state/capabilities/state.json");
    let mut state: Value = read_json(&path).expect("controls");
    state["session_enabled"] = json!({
        third:{"user":[{"capability_id":"mcp:test:update","expires_at":"2000-01-01T00:00:00Z"}]},
        fixture.second.clone():{"user":[{"capability_id":"mcp:test:update","expires_at":"2999-01-01T00:00:00Z"}]}
    });
    write_json(&path, &state).expect("controls");
    assert_eq!(
        enabled_group_count(&fixture.home, "mcp:test:update").expect("count"),
        2
    );
    assert_eq!(
        recorded_fingerprint(&json!({"invoker":{"command":["old"]}})),
        None
    );
    assert_eq!(
        recorded_fingerprint(
            &json!({"install_mode":fixture.record["install_mode"],"install_spec":fixture.record["install_spec"]})
        ),
        Some(recipe::fingerprint(&fixture.record).expect("stored recipe"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_update_waits_for_inflight_call_and_its_state_write() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let fixture = Fixture::new("normal");
    let release = fixture._temp.path().join("release-call");
    let mut old_record = fixture.record.clone();
    old_record["install_spec"]["command_candidates"][0][3] = json!("wait_call");
    old_record["install_spec"]["command_candidates"][0]
        .as_array_mut()
        .expect("fixture command candidates")
        .push(json!(release));
    let old = package_install::prepare("mcp:test:update", &old_record).expect("old service");
    let old_id = old["artifact_id"].as_str().expect("old artifact id");
    let tool_name = old["tools"][0]["name"].as_str().expect("old tool name");
    let mut state = fixture.state();
    state["artifacts"][old_id] = old.clone();
    state["capability_artifacts"]["mcp:test:update"] = json!(old_id);
    state["actor_instances"][&fixture.first]["user"]["mcp:test:update"]["artifact_id"] =
        json!(old_id);
    write_json(
        &fixture.home.root().join("state/capabilities/runtime.json"),
        &state,
    )
    .expect("write old runtime");
    std::fs::write(&fixture.evidence, "").expect("reset fixture evidence");

    async fn transact(
        home: HomeLayout,
        locks: crate::dispatch_concurrency::DispatchLocks,
        request: DaemonRequest,
    ) -> cccc_contracts::DaemonResponse {
        // Exercise the connection handler and its real dispatch permit, rather
        // than calling the update function behind the serialization boundary.
        let (mut client, server) = tokio::io::duplex(16 * 1024);
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let connection = crate::server_connection::spawn_connection(server, home, shutdown, locks);
        let mut bytes = serde_json::to_vec(&request).expect("serialize daemon request");
        bytes.push(b'\n');
        client.write_all(&bytes).await.expect("send daemon request");
        let mut response = String::new();
        BufReader::new(&mut client)
            .read_line(&mut response)
            .await
            .expect("read daemon response");
        drop(client);
        connection.await.expect("connection task");
        serde_json::from_str(&response).expect("parse daemon response")
    }

    let locks = crate::dispatch_concurrency::DispatchLocks::default();
    let call = DaemonRequest {
        v: 1,
        op: "capability_tool_call".into(),
        args: json!({"group_id":fixture.first,"by":"user","tool_name":tool_name,
            "arguments":{"value":42}})
        .as_object()
        .expect("tool call arguments")
        .clone(),
    };
    let running = tokio::spawn(transact(fixture.home.clone(), locks.clone(), call));
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let evidence = std::fs::read_to_string(&fixture.evidence).unwrap_or_default();
            if evidence
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .any(|row| row.get("arguments").is_some())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("old call reached the service");
    let fingerprint = recipe::fingerprint(&fixture.record).expect("fixture recipe fingerprint");
    let mut updating = tokio::spawn(transact(
        fixture.home.clone(),
        locks,
        fixture.request(json!("user"), &fingerprint),
    ));
    let early_result = tokio::time::timeout(std::time::Duration::from_secs(1), &mut updating).await;
    let remained_pending = early_result.is_err();
    // Always release the fixture before asserting, so a failed assertion cannot
    // leave a blocking worker waiting for its full production call deadline.
    std::fs::write(&release, "continue").expect("release running call");
    let call = running.await.expect("running call task");
    let update = match early_result {
        Ok(result) => result.expect("early update task"),
        Err(_) => updating.await.expect("pending update task"),
    };
    assert!(remained_pending, "update must wait for the running call");
    assert!(call.ok, "{:?}", call.error);
    assert!(update.ok, "{:?}", update.error);
    let state = fixture.state();
    let current = state["capability_artifacts"]["mcp:test:update"]
        .as_str()
        .expect("updated artifact id");
    assert_ne!(current, old_id);
    assert_eq!(
        state["actor_instances"][&fixture.first]["user"]["mcp:test:update"]["artifact_id"],
        current
    );
    assert_eq!(
        state["actor_instances"][&fixture.first]["user"]["mcp:test:update"]["last_call"]["ok"],
        true
    );
    assert_eq!(
        state["actor_instances"][&fixture.first]["user"]["mcp:test:update"]["state"],
        "runnable"
    );
}
