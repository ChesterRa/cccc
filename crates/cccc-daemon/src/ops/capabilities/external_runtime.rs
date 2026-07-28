use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::dispatch::{OpError, OpResult, object, string_arg};

pub(super) fn dynamic_tools(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    enabled: &[String],
) -> Result<Vec<Value>, OpError> {
    let runtime = load(home)?;
    let artifacts = object_field(&runtime, "artifacts");
    let capability_artifacts = object_field(&runtime, "capability_artifacts");
    let actor_instances = object_field(&runtime, "actor_instances");
    let bindings = actor_instances
        .get(group_id)
        .and_then(Value::as_object)
        .and_then(|groups| groups.get(actor_id))
        .and_then(Value::as_object);
    let enabled = enabled.iter().collect::<BTreeSet<_>>();
    let mut output = Vec::new();
    for capability_id in enabled {
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
    output.truncate(dynamic_tool_limit());
    Ok(output)
}

pub(super) fn call(home: &HomeLayout, request: &DaemonRequest, tool_name: &str) -> OpResult {
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    // `by` is injected from the MCP process environment and is authoritative.
    // `actor_id` is model-controlled and may also mean a target actor on other
    // tools, so it must never override the caller identity here.
    let actor_id = caller_actor_id(request);
    let native = cccc_core::capabilities::CapabilityStore::new(home.clone())
        .load()
        .map_err(OpError::io)?;
    let effective =
        super::effective_state::load(home, &group_id, &actor_id, &native).map_err(OpError::io)?;
    let enabled = effective
        .enabled
        .difference(&effective.blocked)
        .cloned()
        .collect::<Vec<_>>();
    let tools = dynamic_tools(home, &group_id, &actor_id, &enabled)?;
    let tool = tools
        .iter()
        .find(|item| item["name"] == tool_name)
        .ok_or_else(|| {
            OpError::new(
                "capability_tool_not_found",
                format!("tool not found or not enabled: {tool_name}"),
            )
        })?;
    let capability_id = tool["capability_id"].as_str().unwrap_or("");
    let real_name = tool["real_tool_name"].as_str().unwrap_or("");
    let runtime = load(home)?;
    let artifact_id =
        runtime["actor_instances"][&group_id][&actor_id][capability_id]["artifact_id"]
            .as_str()
            .or_else(|| runtime["capability_artifacts"][capability_id].as_str())
            .ok_or_else(|| {
                OpError::new(
                    "capability_tool_not_found",
                    "capability runtime artifact is missing",
                )
            })?;
    let artifact = &runtime["artifacts"][artifact_id];
    let invoker = artifact["invoker"].as_object().ok_or_else(|| {
        OpError::new(
            "capability_runtime_unavailable",
            "external capability invoker is missing",
        )
    })?;
    let kind = invoker.get("type").and_then(Value::as_str).unwrap_or("");
    let arguments = request
        .args
        .get("arguments")
        .or_else(|| request.args.get("args"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let result = match kind {
        "remote_http" | "streamable_http" | "http" => {
            call_http(invoker, tool_name, real_name, arguments)?
        }
        "npm_stdio" | "package_stdio" | "command_stdio" => {
            call_stdio(invoker, real_name, arguments)?
        }
        _ => {
            return Err(OpError::new(
                "capability_runtime_unavailable",
                format!("unsupported Rust external capability invoker: {kind}"),
            ));
        }
    };
    object(json!({
        "capability_id":capability_id,
        "tool_name":tool_name,
        "real_tool_name":real_name,
        "result":result
    }))
}

fn caller_actor_id(request: &DaemonRequest) -> String {
    string_arg(request, "by")
        .or_else(|| string_arg(request, "actor_id"))
        .unwrap_or_else(|| "user".into())
}

fn call_http(
    invoker: &Map<String, Value>,
    tool_name: &str,
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
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(OpError::invalid)?;
    let token = invoker
        .get("token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let (initialize, session_id) = http_jsonrpc(
        &client,
        url,
        token,
        None,
        &json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2024-11-05","capabilities":{},
                "clientInfo":{"name":"cccc-capability-runtime","version":"1.0"}}
        }),
    )?;
    if let Some(error) = initialize.get("error") {
        return Err(OpError::new(
            "capability_tool_failed",
            format!("remote initialize failed: {error}"),
        ));
    }
    let (value, _) = http_jsonrpc(
        &client,
        url,
        token,
        session_id.as_deref(),
        &json!({
            "jsonrpc":"2.0","id":format!("cccc-{tool_name}"),"method":"tools/call",
            "params":{"name":real_name,"arguments":arguments}
        }),
    )?;
    if let Some(error) = value.get("error") {
        return Err(OpError::new("capability_tool_failed", error.to_string()));
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

fn http_jsonrpc(
    client: &reqwest::blocking::Client,
    url: &str,
    token: Option<&str>,
    session_id: Option<&str>,
    payload: &Value,
) -> Result<(Value, Option<String>), OpError> {
    let mut request = client
        .post(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(payload);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(session_id) = session_id {
        request = request.header("Mcp-Session-Id", session_id);
    }
    let response = request
        .send()
        .map_err(|error| OpError::new("capability_transport_error", error.to_string()))?;
    let status = response.status();
    let next_session = response
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| session_id.map(str::to_owned));
    let value = response
        .json::<Value>()
        .map_err(|error| OpError::new("capability_transport_error", error.to_string()))?;
    if !status.is_success() {
        return Err(OpError::new(
            "capability_transport_error",
            format!("HTTP {status}: {value}"),
        ));
    }
    Ok((value, next_session))
}

fn call_stdio(
    invoker: &Map<String, Value>,
    real_name: &str,
    arguments: Value,
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
    let mut process = Command::new(&command[0]);
    process
        .args(&command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(env) = invoker.get("env").and_then(Value::as_object) {
        process.envs(
            env.iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key, value))),
        );
    }
    let mut child = process
        .spawn()
        .map_err(|error| OpError::new("capability_transport_error", error.to_string()))?;
    let requests = [
        json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2024-11-05","capabilities":{},
                "clientInfo":{"name":"cccc-capability-runtime","version":"1.0"}}
        }),
        json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":real_name,"arguments":arguments}
        }),
    ];
    {
        let stdin = child.stdin.as_mut().ok_or_else(|| {
            OpError::new("capability_transport_error", "stdio stdin is unavailable")
        })?;
        for request in requests {
            serde_json::to_writer(&mut *stdin, &request).map_err(OpError::invalid)?;
            stdin
                .write_all(b"\n")
                .map_err(|error| OpError::new("capability_transport_error", error.to_string()))?;
        }
    }
    drop(child.stdin.take());
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| OpError::new("capability_transport_error", "stdio stdout is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| OpError::new("capability_transport_error", "stdio stderr is unavailable"))?;
    let stdout_thread = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = std::io::BufReader::new(stdout).read_to_string(&mut output);
        output
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut output = String::new();
        let _ = std::io::BufReader::new(stderr).read_to_string(&mut output);
        output
    });
    let timeout = std::env::var("CCCC_CAPABILITY_PACKAGE_CALL_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(45)
        .clamp(5, 180);
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OpError::new(
                    "capability_transport_error",
                    "stdio mcp request timed out",
                ));
            }
            Err(error) => {
                return Err(OpError::new(
                    "capability_transport_error",
                    error.to_string(),
                ));
            }
        }
    };
    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    if !status.success() && stdout.trim().is_empty() {
        return Err(OpError::new(
            "capability_transport_error",
            format!("stdio mcp exited with {status}: {}", stderr.trim()),
        ));
    }
    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if value["id"] != 2 {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(OpError::new("capability_tool_failed", error.to_string()));
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(OpError::new(
        "capability_tool_failed",
        "stdio tools/call returned no response",
    ))
}

fn load(home: &HomeLayout) -> Result<Value, OpError> {
    let path = home.root().join("state/capabilities/runtime.json");
    if !path.exists() {
        return Ok(json!({}));
    }
    cccc_core::fs::read_json(&path).map_err(OpError::io)
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
    matches!(state, "installed" | "ready" | "active")
}

fn binding_state_ready(state: &str) -> bool {
    state.is_empty() || matches!(state, "bound" | "ready" | "active")
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
}
