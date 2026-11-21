use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::HomeLayout;
use cccc_core::fs::{read_json, with_exclusive_lock, write_json};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

use crate::dispatch::{OpError, string_arg};

pub(super) fn all_dynamic_tools(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    enabled: &[String],
) -> Result<Vec<Value>, OpError> {
    let runtime = load(home)?;
    let artifacts = object_field(&runtime, "artifacts");
    let capability_artifacts = object_field(&runtime, "capability_artifacts");
    let actor_instances = object_field(&runtime, "actor_instances");
    let records = cccc_core::capabilities::CapabilityStore::new(home.clone())
        .resolved_records()
        .map_err(OpError::io)?;
    let bindings = actor_instances
        .get(group_id)
        .and_then(Value::as_object)
        .and_then(|groups| groups.get(actor_id))
        .and_then(Value::as_object);
    let enabled = enabled.iter().collect::<BTreeSet<_>>();
    let mut output = Vec::new();
    for capability_id in enabled {
        if let Some(record) = records.get(capability_id) {
            let execution = super::credentials::execution_record(record, &runtime);
            if super::credentials::bearer(&execution) {
                for mut tool in
                    super::credentials::cached_tools(home, group_id, actor_id, &execution)
                {
                    tool["capability_id"] = json!(capability_id);
                    output.push(tool);
                }
                continue;
            }
        }
        let artifact_id = bindings
            .and_then(|items| items.get(capability_id))
            .filter(|binding| binding_state_ready(binding["state"].as_str().unwrap_or("")))
            .and_then(|binding| binding["artifact_id"].as_str())
            .or_else(|| {
                capability_artifacts
                    .get(capability_id)
                    .and_then(Value::as_str)
            });
        let Some(artifact) = artifact_id.and_then(|id| artifacts.get(id)) else {
            continue;
        };
        if !install_state_ready(artifact["state"].as_str().unwrap_or("")) {
            continue;
        }
        for tool in artifact["tools"].as_array().into_iter().flatten() {
            let Some(name) = tool["name"].as_str().filter(|value| !value.is_empty()) else {
                continue;
            };
            let Some(real_name) = tool["real_tool_name"]
                .as_str()
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let schema = tool
                .get("inputSchema")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| json!({"type":"object","properties":{},"required":[]}));
            output.push(json!({
                "name":name,
                "description":tool["description"].as_str().unwrap_or(""),
                "inputSchema":schema,
                "capability_id":capability_id,
                "real_tool_name":real_name,
            }));
        }
    }
    output.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    Ok(output)
}

pub(super) fn dynamic_tools(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    enabled: &[String],
) -> Result<Vec<Value>, OpError> {
    let mut tools = all_dynamic_tools(home, group_id, actor_id, enabled)?;
    tools.truncate(dynamic_tool_limit());
    Ok(tools)
}

pub(super) fn caller_actor_id(request: &DaemonRequest) -> String {
    string_arg(request, "by")
        .or_else(|| string_arg(request, "actor_id"))
        .unwrap_or_else(|| "user".into())
}

fn call_http(
    invoker: &Map<String, Value>,
    real_name: &str,
    arguments: Value,
) -> Result<Value, OpError> {
    let url = invoker
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            OpError::new(
                "capability_runtime_unavailable",
                "external capability URL is missing",
            )
        })?;
    let token = invoker
        .get("token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let arguments = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| OpError::new("invalid_args", "tool arguments must be an object"))?;
    super::mcp_http::call_tool(url, token, real_name, arguments, Duration::from_secs(120)).map_err(
        |error| {
            OpError::new(
                if error.remote {
                    "capability_tool_failed"
                } else {
                    "capability_transport_error"
                },
                error.message,
            )
        },
    )
}

pub(super) fn invoke_guarded(
    home: &HomeLayout,
    invoker: &Map<String, Value>,
    real: &str,
    arguments: Value,
    record: &Value,
) -> Result<Value, OpError> {
    match invoker.get("type").and_then(Value::as_str).unwrap_or("") {
        "remote_http" | "streamable_http" | "http" => call_http(invoker, real, arguments),
        "npm_stdio" | "package_stdio" | "command_stdio" => {
            call_stdio_inner(home, invoker, real, arguments, record)
        }
        kind => Err(OpError::new(
            "capability_runtime_unavailable",
            format!("unsupported Rust external capability invoker: {kind}"),
        )),
    }
}

fn call_stdio_inner(
    home: &HomeLayout,
    invoker: &Map<String, Value>,
    real_name: &str,
    arguments: Value,
    record: &Value,
) -> Result<Value, OpError> {
    let command = invoker
        .get("command")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .ok_or_else(|| {
            OpError::new(
                "capability_runtime_unavailable",
                "external capability command is missing",
            )
        })?;
    // These values were already inherited from the daemon. Making them explicit
    // lets diagnostics redact required keys whose names do not look like secrets.
    let mut env = super::package_install::required_environment_values(record);
    if let Some(overrides) = invoker.get("env").and_then(Value::as_object) {
        env.extend(overrides.clone());
    }
    let arguments = arguments
        .as_object()
        .cloned()
        .ok_or_else(|| OpError::new("invalid_args", "tool arguments must be an object"))?;
    let timeout = std::env::var("CCCC_CAPABILITY_PACKAGE_CALL_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(45)
        .clamp(5, 180);
    let result = super::mcp_stdio::call_tool_guarded(
        home,
        &command,
        &env,
        real_name,
        arguments,
        Duration::from_secs(timeout),
    );
    result.map_err(|error| {
        OpError::new(
            if error.remote {
                "capability_tool_failed"
            } else {
                "capability_transport_error"
            },
            error.message,
        )
    })
}

/// Recover only aliases whose suffix reconstructs the original name and verifies
/// the complete hash. Never discover unrelated endpoints to guess ownership.
pub(super) fn cold_bearer_capability(
    home: &HomeLayout,
    store: &cccc_core::capabilities::CapabilityStore,
    enabled: &BTreeSet<String>,
    name: &str,
) -> Result<Option<String>, OpError> {
    let Some((hash, real)) = name
        .strip_prefix("cccc_ext_")
        .and_then(|suffix| suffix.split_once('_'))
    else {
        return Ok(None);
    };
    if hash.len() != 8 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) || real.is_empty() {
        return Ok(None);
    }
    let runtime = load(home)?;
    let records = store.resolved_records().map_err(OpError::io)?;
    let mut matches = enabled.iter().filter(|id| {
        cccc_core::capabilities::external_tool_name(id, real) == name
            && records.get(*id).is_some_and(|record| {
                super::credentials::bearer(&super::credentials::execution_record(record, &runtime))
            })
    });
    let first = matches.next().cloned();
    if matches.next().is_some() {
        return Err(OpError::new(
            "capability_tool_not_found",
            "tool ownership is ambiguous; use cccc_capability_use with an explicit capability_id",
        ));
    }
    Ok(first)
}

pub(super) fn load(home: &HomeLayout) -> Result<Value, OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    if !path.exists() {
        return Ok(json!({}));
    }
    read_json(&path).map_err(OpError::io)
}

pub(super) fn record_call_state(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    capability_id: &str,
    artifact_id: &str,
    state: &str,
    error: &str,
) -> Result<(), OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut runtime = if path.exists() {
            read_json::<Value>(&path)?
        } else {
            json!({"v":2,"created_at":utc_now()})
        };
        runtime["v"] = json!(2);
        runtime["updated_at"] = json!(utc_now());
        if let Some(artifact) = runtime
            .get_mut("artifacts")
            .and_then(Value::as_object_mut)
            .and_then(|artifacts| artifacts.get_mut(artifact_id))
            .and_then(Value::as_object_mut)
        {
            artifact.insert("last_error".into(), json!(error));
            artifact.insert("updated_at".into(), json!(utc_now()));
        }
        let groups = object_field_mut(&mut runtime, "actor_instances");
        let actors = nested_object_mut(groups, group_id);
        let capabilities = nested_object_mut(actors, actor_id);
        let mut binding = json!({
            "state":state,"last_error":error,
            "last_call":{"at":utc_now(),"ok":state == "verified"},
            "updated_at":utc_now()
        });
        if !artifact_id.is_empty() {
            binding["artifact_id"] = json!(artifact_id);
        }
        capabilities.insert(capability_id.into(), binding);
        if state == "verified" {
            let recent = object_field_mut(&mut runtime, "recent_success");
            let previous = recent
                .get(capability_id)
                .and_then(|entry| entry.get("success_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            recent.insert(
                capability_id.into(),
                json!({
                    "capability_id":capability_id,"success_count":previous + 1,
                    "last_success_at":utc_now(),"last_group_id":group_id,
                    "last_actor_id":actor_id,"last_action":"tool_call"
                }),
            );
        }
        write_json(&path, &runtime)
    })
    .map_err(OpError::io)
}

fn object_field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("object initialized");
    let field = root.entry(key).or_insert_with(|| json!({}));
    if !field.is_object() {
        *field = json!({});
    }
    field.as_object_mut().expect("field initialized")
}

fn nested_object_mut<'a>(
    parent: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    let value = parent.entry(key).or_insert_with(|| json!({}));
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("nested object initialized")
}

fn object_field<'a>(value: &'a Value, key: &str) -> &'a Map<String, Value> {
    match value.get(key).and_then(Value::as_object) {
        Some(object) => object,
        None => empty(),
    }
}

fn empty() -> &'static Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Map::new)
}

fn install_state_ready(state: &str) -> bool {
    matches!(
        state,
        "installed" | "installed_degraded" | "ready" | "active"
    )
}

fn binding_state_ready(state: &str) -> bool {
    state.is_empty()
        || matches!(
            state,
            "bound"
                | "ready"
                | "active"
                | "activation_pending"
                | "runnable"
                | "verified"
                | "tool_call_failed"
        )
}

fn dynamic_tool_limit() -> usize {
    std::env::var("CCCC_CAPABILITY_MAX_DYNAMIC_TOOLS_VISIBLE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(32)
        .clamp(1, 500)
}

#[cfg(test)]
mod tests {
    use super::{caller_actor_id, dynamic_tools};
    use cccc_contracts::DaemonRequest;
    use cccc_core::HomeLayout;
    use serde_json::json;
    use tempfile::tempdir;

    fn call(home: &HomeLayout, request: &DaemonRequest, name: &str) -> crate::dispatch::OpResult {
        let mut request = request.clone();
        request.args.insert("tool_name".into(), json!(name));
        super::super::staged::run_sync(home, &request)
    }

    #[test]
    fn reads_python_runtime_dynamic_tools() {
        let temp = tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path()).expect("home path");
        home.initialize().expect("home");
        let path = home.root().join("state/capabilities/runtime.json");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        cccc_core::fs::write_json(
            &path,
            &json!({
                "artifacts":{"artifact_1":{
                    "state":"installed",
                    "invoker":{"type":"remote_http","url":"http://127.0.0.1:9900/mcp"},
                    "tools":[{"name":"cccc_ext_deadbeef_echo","real_tool_name":"echo",
                        "description":"Echo","inputSchema":{"type":"object","properties":{},"required":[]}}]
                }},
                "capability_artifacts":{"mcp:test":"artifact_1"},
                "actor_instances":{}
            }),
        )
        .expect("runtime");
        let tools = dynamic_tools(&home, "g_test", "peer-1", &["mcp:test".into()]).expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "cccc_ext_deadbeef_echo");
        assert_eq!(tools[0]["real_tool_name"], "echo");
    }

    #[test]
    fn authoritative_by_wins_over_model_controlled_actor_id() {
        let request = DaemonRequest {
            v: 1,
            op: "capability_tool_call".into(),
            args: json!({"by":"peer-a","actor_id":"foreman"})
                .as_object()
                .cloned()
                .expect("args"),
        };
        assert_eq!(caller_actor_id(&request), "peer-a");
    }
    fn runtime_fixture(
        mode: &str,
        count: usize,
    ) -> (
        tempfile::TempDir,
        HomeLayout,
        String,
        Vec<serde_json::Value>,
    ) {
        let temp = tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let group = cccc_core::GroupStore::new(home.clone())
            .and_then(|store| store.create("fixture", ""))
            .expect("group");
        let script = temp.path().join("server.py");
        std::fs::write(&script, super::super::mcp_stdio::STDIO_FIXTURE).expect("script");
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
            mode,
            temp.path().join("evidence.jsonl")
        ]);
        let tools = (0..count).map(|index| json!({"name":format!("cccc_ext_fixture_{index:04}"),"real_tool_name":"echo","inputSchema":{"type":"object"}})).collect::<Vec<_>>();
        let store = cccc_core::capabilities::CapabilityStore::new(home.clone());
        store.import_record(json!({"capability_id":"mcp:fixture","kind":"mcp_toolpack","name":"Fixture","tool_names":tools.iter().map(|tool|tool["name"].clone()).collect::<Vec<_>>()})).expect("record");
        store
            .set_enabled_for("mcp:fixture", true, &group.group_id, "user", "actor", 3600)
            .expect("enable");
        cccc_core::fs::write_json(&home.root().join("state/capabilities/runtime.json"), &json!({
            "artifacts":{"fixture":{"state":"installed","invoker":{"type":"command_stdio","command":command},"tools":tools}},
            "capability_artifacts":{"mcp:fixture":"fixture"},
            "recent_success":{"mcp:fixture":{"success_count":7}}
        })).expect("runtime");
        (temp, home, group.group_id, tools)
    }

    #[test]
    fn tools_outside_the_display_budget_still_require_a_live_authorized_binding() {
        let (_temp, home, group, tools) =
            runtime_fixture("normal", super::dynamic_tool_limit() + 1);
        let shown = dynamic_tools(&home, &group, "user", &["mcp:fixture".into()]).expect("shown");
        let last = tools.last().expect("tool")["name"].as_str().expect("name");
        assert!(!shown.iter().any(|tool| tool["name"] == last));
        let request = DaemonRequest {
            v: 1,
            op: "capability_tool_call".into(),
            args: json!({"group_id":group,"by":"user","arguments":{}})
                .as_object()
                .expect("args")
                .clone(),
        };
        let result = call(&home, &request, last).expect("undisplayed authorized tool");
        assert_eq!(
            result["result"]["content"][0]["text"],
            "stdio-fixture-result"
        );
        let mut qualified = request.clone();
        qualified
            .args
            .insert("capability_id".into(), json!("mcp:fixture"));
        let result = call(&home, &qualified, "echo").expect("qualified real name");
        assert_eq!(
            result["result"]["content"][0]["text"],
            "stdio-fixture-result"
        );
        qualified
            .args
            .insert("capability_id".into(), json!("mcp:other"));
        assert_eq!(
            call(&home, &qualified, "echo")
                .expect_err("wrong capability")
                .code,
            "capability_tool_not_found"
        );
        cccc_core::capabilities::CapabilityStore::new(home.clone())
            .set_blocked_for("mcp:fixture", true, &group, "fixture", "user", 0)
            .expect("block");
        let denied = call(&home, &request, last).expect_err("blocked tool must stay denied");
        assert_eq!(denied.code, "capability_tool_not_found");
        qualified
            .args
            .insert("capability_id".into(), json!("mcp:fixture"));
        assert_eq!(
            call(&home, &qualified, "echo")
                .expect_err("blocked real name")
                .code,
            "capability_tool_not_found"
        );
    }

    #[test]
    fn a_tool_error_does_not_increment_recent_success_or_mark_the_binding_verified() {
        let (_temp, home, group, tools) = runtime_fixture("tool_error", 1);
        let request = DaemonRequest {
            v: 1,
            op: "capability_tool_call".into(),
            args: json!({"group_id":group,"by":"user","arguments":{}})
                .as_object()
                .expect("args")
                .clone(),
        };
        let result =
            call(&home, &request, tools[0]["name"].as_str().expect("name")).expect("tool result");
        assert_eq!(result["result"]["isError"], true);
        let runtime = super::load(&home).expect("runtime");
        assert_eq!(runtime["recent_success"]["mcp:fixture"]["success_count"], 7);
        assert_eq!(
            runtime["actor_instances"][&group]["user"]["mcp:fixture"]["state"],
            "tool_call_failed"
        );
    }

    #[test]
    fn declared_environment_is_redacted_on_calls_and_invoker_overrides_win() {
        const WORKER: &str = "CCCC_TEST_REQUIRED_ENV_WORKER";
        const INHERITED: &str = "postgresql://fixture-user:fixture-private-secret@localhost/demo";
        const OVERRIDE: &str = "fixture-invoker-override";
        if std::env::var_os(WORKER).is_none() {
            // Set only the child test's environment; parallel tests keep their process state.
            let full_name = concat!(
                module_path!(),
                "::declared_environment_is_redacted_on_calls_and_invoker_overrides_win"
            );
            let name = full_name.split_once("::").expect("test module").1;
            let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args(["--exact", name, "--test-threads=1"])
                .env(WORKER, "1")
                .env("DATABASE_URL", INHERITED)
                .output()
                .expect("isolated environment worker");
            let diagnostic = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .replace(INHERITED, "<redacted>")
            .replace(OVERRIDE, "<redacted>");
            assert!(
                output.status.success() && diagnostic.contains("1 passed"),
                "required-environment worker failed: {diagnostic}"
            );
            return;
        }

        let (temp, home, group, tools) = runtime_fixture("exit_required", 1);
        let store = cccc_core::capabilities::CapabilityStore::new(home.clone());
        let mut record = store
            .catalog_record("mcp:fixture")
            .expect("catalog")
            .expect("record");
        record["required_env"] = json!(["DATABASE_URL"]);
        record["install_spec"] = json!({"required_env":["DATABASE_URL"]});
        store
            .import_record(record)
            .expect("required environment declaration");
        let request = DaemonRequest {
            v: 1,
            op: "capability_tool_call".into(),
            args: json!({"group_id":group,"by":"user","arguments":{}})
                .as_object()
                .expect("args")
                .clone(),
        };
        let name = tools[0]["name"].as_str().expect("tool name");
        let error = call(&home, &request, name).expect_err("fixture exits");
        assert!(error.message.contains("fixture connection failed"));
        assert!(error.message.contains("<redacted>"));
        assert!(
            !error.message.contains(INHERITED),
            "inherited credential escaped in diagnostics"
        );
        let mut runtime = super::load(&home).expect("runtime");
        assert!(
            !runtime.to_string().contains(INHERITED),
            "inherited value must not enter shared state"
        );

        runtime["artifacts"]["fixture"]["invoker"]["env"] = json!({"DATABASE_URL":OVERRIDE});
        cccc_core::fs::write_json(
            &home.root().join("state/capabilities/runtime.json"),
            &runtime,
        )
        .expect("explicit invoker override");
        let error = call(&home, &request, name).expect_err("fixture exits");
        assert!(
            !error.message.contains(OVERRIDE),
            "override escaped in diagnostics"
        );
        let receipts = std::fs::read_to_string(temp.path().join("evidence.jsonl"))
            .expect("receipts")
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("receipt"))
            .filter_map(|row| row.get("environment_matches_override").cloned())
            .collect::<Vec<_>>();
        assert_eq!(
            receipts,
            vec![json!(false), json!(true)],
            "the explicit invoker environment stays authoritative"
        );
    }
}
