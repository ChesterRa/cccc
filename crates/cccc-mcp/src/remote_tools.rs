use cccc_core::{HomeLayout, integration_state};
use serde_json::{Map, Value, json};

use crate::router::tool_result;

const STORE_KEY: &str = "group_bridge";

pub(crate) fn is_remote_tool(name: &str) -> bool {
    matches!(
        name,
        "cccc_remote_access"
            | "cccc_remote_context"
            | "cccc_remote_repo"
            | "cccc_remote_git"
            | "cccc_remote_repo_edit"
            | "cccc_remote_apply_patch"
            | "cccc_remote_shell"
            | "cccc_remote_exec_command"
            | "cccc_remote_write_stdin"
    )
}

pub(crate) async fn call(
    home: &HomeLayout,
    name: &str,
    args: Map<String, Value>,
) -> Result<Value, String> {
    cccc_core::group_bridge_legacy::import_if_changed(home).map_err(|error| error.to_string())?;
    let group_id = text(&args, "group_id").ok_or("group_id is required")?;
    let state =
        integration_state::global_get(home, STORE_KEY).map_err(|error| error.to_string())?;
    if name == "cccc_remote_access" {
        return Ok(tool_result(remote_access(&state, group_id, &args)?));
    }
    let remote_group_id = text(&args, "remote_group_id").ok_or("remote_group_id is required")?;
    let trust = find_trust(&state, group_id, remote_group_id)?;
    enforce_access(name, &args, trust)?;
    call_remote(name, &args, trust).await
}

fn remote_access(
    state: &Value,
    group_id: &str,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let action = text(args, "action").unwrap_or("list");
    if !matches!(action, "list" | "status" | "explain_permissions") {
        return Err("action must be list, status, or explain_permissions".into());
    }
    let remote_group_id = text(args, "remote_group_id");
    let targets = trusts(state)
        .iter()
        .filter(|trust| trust["group_id"] == group_id && trust["status"] == "active")
        .filter(|trust| remote_group_id.is_none_or(|id| trust["remote_group_id"] == id))
        .map(public_target)
        .collect::<Vec<_>>();
    if remote_group_id.is_some() {
        let target = targets
            .into_iter()
            .next()
            .ok_or_else(|| "Group Bridge target not found".to_owned())?;
        return Ok(json!({
            "remote_group_id":target["remote_group_id"],
            "access_level":target["remote_access_level"],
            "permissions":target["permissions"],
            "status":target["status"]
        }));
    }
    Ok(json!({"targets":targets}))
}

async fn call_remote(
    name: &str,
    args: &Map<String, Value>,
    trust: &Value,
) -> Result<Value, String> {
    let endpoint = trust["remote_endpoint"]
        .as_str()
        .map(str::trim)
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))
        .ok_or("remote Group Bridge endpoint is unavailable")?
        .trim_end_matches('/');
    let credential = trust["credential"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("remote Group Bridge credential is unavailable")?;
    let mut arguments = args.clone();
    for key in ["group_id", "actor_id", "by"] {
        arguments.remove(key);
    }
    let timeout = remote_timeout(name, &arguments);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .post(format!("{endpoint}/mcp/group-bridge"))
        .bearer_auth(credential)
        .json(&json!({
            "jsonrpc":"2.0","id":"cccc-group-bridge-call","method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }))
        .send()
        .await
        .map_err(|error| format!("remote Group Bridge request failed: {error}"))?;
    let status = response.status();
    let value = response
        .json::<Value>()
        .await
        .map_err(|error| format!("remote Group Bridge returned invalid JSON: {error}"))?;
    if !status.is_success() {
        return Err(format!("remote Group Bridge HTTP {status}: {value}"));
    }
    if let Some(error) = value.get("error") {
        return Err(error["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string()));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| "remote Group Bridge response has no result".into())
}

fn find_trust<'a>(
    state: &'a Value,
    group_id: &str,
    remote_group_id: &str,
) -> Result<&'a Value, String> {
    trusts(state)
        .iter()
        .find(|trust| {
            trust["group_id"] == group_id
                && trust["remote_group_id"] == remote_group_id
                && trust["status"] == "active"
        })
        .ok_or_else(|| format!("Group Bridge target not found: {remote_group_id}"))
}

fn enforce_access(name: &str, args: &Map<String, Value>, trust: &Value) -> Result<(), String> {
    let access = trust["remote_access_level"].as_str().unwrap_or("messages");
    let mutating_git =
        name == "cccc_remote_git" && matches!(text(args, "action"), Some("add" | "commit"));
    let allowed = match name {
        "cccc_remote_git" if mutating_git => access == "full",
        "cccc_remote_context" | "cccc_remote_repo" | "cccc_remote_git" => {
            matches!(access, "read" | "full")
        }
        "cccc_remote_repo_edit"
        | "cccc_remote_apply_patch"
        | "cccc_remote_shell"
        | "cccc_remote_exec_command"
        | "cccc_remote_write_stdin" => access == "full",
        _ => true,
    };
    allowed
        .then_some(())
        .ok_or_else(|| format!("remote Group Bridge access={access} does not allow {name}"))
}

fn public_target(trust: &Value) -> Value {
    let access = trust["remote_access_level"].as_str().unwrap_or("messages");
    json!({
        "remote_group_id":trust["remote_group_id"],
        "remote_group_title":trust["remote_group_title"],
        "remote_peer_id":trust["remote_peer_id"],
        "trust_id":trust["trust_id"],
        "registration_id":trust["registration_id"],
        "endpoint":trust["remote_endpoint"],
        "status":trust["status"],
        "session_connected":trust["session_connected"].as_bool().unwrap_or(false),
        "session_connected_at":trust["session_connected_at"],
        "session_last_error":trust["session_last_error"],
        "session_last_error_at":trust["session_last_error_at"],
        "remote_access_level":access,
        "permissions":{
            "messages":true,
            "read":matches!(access,"read"|"full"),
            "full":access=="full"
        }
    })
}

fn trusts(state: &Value) -> &[Value] {
    state["trusts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn remote_timeout(name: &str, args: &Map<String, Value>) -> std::time::Duration {
    let requested = args
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(1, 600);
    std::time::Duration::from_secs(
        if matches!(
            name,
            "cccc_remote_shell" | "cccc_remote_exec_command" | "cccc_remote_write_stdin"
        ) {
            requested.saturating_add(5)
        } else {
            15
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, header};
    use axum::routing::post;
    use axum::{Json, Router};

    #[tokio::test]
    async fn access_list_projects_permissions_without_credentials() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        integration_state::global_update(&home, STORE_KEY, |value| {
            *value = json!({"trusts":[{
                "trust_id":"trust_1","group_id":"g_local","remote_group_id":"g_remote",
                "remote_endpoint":"https://remote.example","credential":"secret",
                "remote_access_level":"read","status":"active",
                "session_connected":true,"session_connected_at":"2026-07-29T00:00:00Z",
                "session_last_error":"","session_last_error_at":null
            }]});
            Ok(())
        })
        .expect("state");
        let result = call(
            &home,
            "cccc_remote_access",
            json!({"group_id":"g_local","action":"list"})
                .as_object()
                .cloned()
                .expect("args"),
        )
        .await
        .expect("access");
        assert_eq!(
            result["structuredContent"]["targets"][0]["permissions"]["read"],
            true
        );
        assert_eq!(
            result["structuredContent"]["targets"][0]["session_connected"],
            true
        );
        assert!(!result.to_string().contains("secret"));
    }

    #[tokio::test]
    async fn remote_repo_is_forwarded_with_bridge_credential() {
        let remote = Router::new().route(
            "/mcp/group-bridge",
            post(
                |headers: HeaderMap, Json(request): Json<Value>| async move {
                    assert_eq!(
                        headers
                            .get(header::AUTHORIZATION)
                            .and_then(|value| value.to_str().ok()),
                        Some("Bearer bridge-token")
                    );
                    assert_eq!(request["params"]["name"], "cccc_remote_repo");
                    Json(json!({
                        "jsonrpc":"2.0","id":request["id"],
                        "result":{"content":[{"type":"text","text":"{\"path\":\"README.md\"}"}]}
                    }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let remote_task = tokio::spawn(async move { axum::serve(listener, remote).await });
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        integration_state::global_update(&home, STORE_KEY, |value| {
            *value = json!({"trusts":[{
                "trust_id":"trust_1","group_id":"g_local","remote_group_id":"g_remote",
                "remote_endpoint":endpoint,"credential":"bridge-token",
                "remote_access_level":"read","status":"active"
            }]});
            Ok(())
        })
        .expect("state");

        let result = call(
            &home,
            "cccc_remote_repo",
            json!({
                "group_id":"g_local","remote_group_id":"g_remote",
                "action":"read","path":"README.md"
            })
            .as_object()
            .cloned()
            .expect("args"),
        )
        .await
        .expect("remote repo");
        assert_eq!(result["content"][0]["type"], "text");
        remote_task.abort();
    }
}
