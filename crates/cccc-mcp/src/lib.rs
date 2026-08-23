mod actions;
mod argument_normalization;
mod bootstrap;
mod code_mode;
mod context_projection;
mod local_sessions;
mod local_tools;
mod mapping;
mod remote_messages;
mod remote_tools;
mod repo;
mod router;
mod tools;

#[cfg(test)]
mod lib_tests;
#[cfg(test)]
mod repo_tests;

use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const SUPPORTED_LEGACY_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2024-11-05"];
const DEFAULT_LEGACY_PROTOCOL_VERSION: &str = SUPPORTED_LEGACY_PROTOCOL_VERSIONS[0];
const CORE_TOOL_NAMES: &[&str] = &[
    "cccc_help",
    "cccc_bootstrap",
    "cccc_capability_search",
    "cccc_capability_use",
    "cccc_inbox_read",
    "cccc_message_history",
    "cccc_message_send",
    "cccc_message_reply",
    "cccc_message_deliver",
    "cccc_reply_request_cancel",
    "cccc_file",
    "cccc_context_get",
    "cccc_coordination",
    "cccc_task",
    "cccc_agent_state",
];

pub async fn run_stdio(home: HomeLayout) -> Result<()> {
    let result = run_stdio_loop(&home).await;
    code_mode::shutdown(&home).await;
    result
}

async fn run_stdio_loop(home: &HomeLayout) -> Result<()> {
    let client = DaemonClient::new(home.clone());
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = input.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_response(&mut output, &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":error.to_string()}})).await?;
                continue;
            }
        };
        if !request.is_object() {
            let response = handle(home, &client, &request, None).await;
            write_response(&mut output, &response).await?;
            continue;
        }
        if request.get("id").is_none() {
            continue;
        }
        let response = handle(home, &client, &request, None).await;
        write_response(&mut output, &response).await?;
    }
    Ok(())
}

pub async fn shutdown(home: &HomeLayout) {
    code_mode::shutdown(home).await;
}

pub async fn handle_request(home: &HomeLayout, request: &Value) -> Value {
    let client = DaemonClient::new(home.clone());
    handle(home, &client, request, None).await
}

pub async fn handle_request_for_actor(
    home: &HomeLayout,
    request: &Value,
    group_id: &str,
    actor_id: &str,
) -> Value {
    let client = DaemonClient::new(home.clone());
    handle(
        home,
        &client,
        request,
        Some(RequestContext { group_id, actor_id }),
    )
    .await
}

#[derive(Clone, Copy)]
pub(crate) struct RequestContext<'a> {
    group_id: &'a str,
    actor_id: &'a str,
}

async fn handle(
    home: &HomeLayout,
    client: &DaemonClient,
    request: &Value,
    context: Option<RequestContext<'_>>,
) -> Value {
    if !request.is_object() {
        return protocol_error(Value::Null, -32600, "Invalid Request");
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": negotiated_protocol_version(request),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "cccc-mcp", "version": env!("CARGO_PKG_VERSION")},
        }),
        "ping" => json!({}),
        "tools/list" => {
            json!({"tools": visible_tools_with_context(home, client, context).await})
        }
        "tools/call" => {
            let Some(params) = request.get("params").and_then(Value::as_object) else {
                return protocol_error(id, -32602, "tools/call params must be an object");
            };
            let Some(name) = params
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
            else {
                return protocol_error(id, -32602, "tools/call name must be a non-empty string");
            };
            let arguments = match params.get("arguments") {
                None => serde_json::Map::new(),
                Some(Value::Object(arguments)) => arguments.clone(),
                Some(_) => {
                    return protocol_error(id, -32602, "tools/call arguments must be an object");
                }
            };
            if !tools::contains(name)
                && !visible_tools_with_context(home, client, context)
                    .await
                    .iter()
                    .any(|tool| tool["name"].as_str() == Some(name))
            {
                return protocol_error(id, -32602, &format!("Unknown tool: {name}"));
            }
            return match router::call_with_context(home, client, name, arguments, context, false)
                .await
            {
                Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                Err(message) => {
                    json!({"jsonrpc":"2.0","id":id,"result":tool_error_result(&message)})
                }
            };
        }
        notification if notification.starts_with("notifications/") => return json!({}),
        _ => return protocol_error(id, -32601, &format!("Method not found: {method}")),
    };
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn protocol_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

fn tool_error_result(message: &str) -> Value {
    let (code, message) = message
        .split_once(": ")
        .filter(|(code, _)| {
            !code.is_empty()
                && code
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        })
        .unwrap_or(("tool_execution_error", message));
    let payload = json!({"error":{"code":code,"message":message}});
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
    json!({
        "content":[{"type":"text","text":text}],
        "structuredContent":payload,
        "isError":true
    })
}

fn negotiated_protocol_version(request: &Value) -> &'static str {
    let requested = request
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    SUPPORTED_LEGACY_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .find(|version| *version == requested)
        .unwrap_or(DEFAULT_LEGACY_PROTOCOL_VERSION)
}

pub(crate) async fn visible_tools_for_actor(
    home: &HomeLayout,
    client: &DaemonClient,
    group_id: &str,
    actor_id: &str,
) -> Vec<Value> {
    visible_tools_with_context(home, client, Some(RequestContext { group_id, actor_id })).await
}

async fn visible_tools_with_context(
    home: &HomeLayout,
    client: &DaemonClient,
    context: Option<RequestContext<'_>>,
) -> Vec<Value> {
    let mut catalog = tools::catalog();
    if context.is_none()
        && std::env::var("CCCC_MCP_TOOL_PROFILE")
            .ok()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("full"))
    {
        hide_disabled_code_mode_tools(&mut catalog);
        return catalog;
    }
    let group_id = context.map(|value| value.group_id.to_owned()).or_else(|| {
        std::env::var("CCCC_GROUP_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| cccc_core::active::get(home).ok().flatten())
    });
    let actor_id = context.map(|value| value.actor_id.to_owned()).or_else(|| {
        std::env::var("CCCC_ACTOR_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
    });
    let (Some(group_id), Some(actor_id)) = (group_id, actor_id) else {
        return core_tools(catalog);
    };
    let response = client
        .call(&cccc_contracts::DaemonRequest {
            v: 1,
            op: "capability_state".into(),
            args: serde_json::Map::from_iter([
                ("group_id".into(), Value::String(group_id.clone())),
                ("actor_id".into(), Value::String(actor_id.clone())),
                ("by".into(), Value::String(actor_id.clone())),
            ]),
        })
        .await;
    let Ok(response) = response else {
        return actor_fallback_tools(home, catalog, &group_id, &actor_id);
    };
    if !response.ok {
        return actor_fallback_tools(home, catalog, &group_id, &actor_id);
    }
    let visible = response
        .result
        .get("visible_tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let mut output = catalog
        .into_iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| visible.contains(name))
        })
        .collect::<Vec<_>>();
    hide_disabled_code_mode_tools(&mut output);
    for tool in response
        .result
        .get("dynamic_tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if tool["name"]
            .as_str()
            .is_some_and(|name| visible.contains(name))
        {
            output.push(tool.clone());
        }
    }
    output
}

fn actor_fallback_tools(
    home: &HomeLayout,
    catalog: Vec<Value>,
    group_id: &str,
    actor_id: &str,
) -> Vec<Value> {
    let web_model = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .ok()
        .and_then(|group| group.actors.into_iter().find(|actor| actor.id == actor_id))
        .is_some_and(|actor| actor.runtime == cccc_contracts::ActorRuntime::WebModel);
    if !web_model {
        return core_tools(catalog);
    }
    let mut output = catalog
        .into_iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| cccc_core::WEB_MODEL_CORE_TOOL_NAMES.contains(&name))
        })
        .collect::<Vec<_>>();
    hide_disabled_code_mode_tools(&mut output);
    output
}

fn hide_disabled_code_mode_tools(tools: &mut Vec<Value>) {
    if code_mode::enabled() {
        return;
    }
    tools.retain(|tool| {
        !matches!(
            tool["name"].as_str(),
            Some("cccc_code_exec" | "cccc_code_wait")
        )
    });
}

fn core_tools(catalog: Vec<Value>) -> Vec<Value> {
    catalog
        .into_iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| CORE_TOOL_NAMES.contains(&name))
        })
        .collect()
}

fn is_core_tool(name: &str) -> bool {
    CORE_TOOL_NAMES.contains(&name)
}

async fn write_response(output: &mut tokio::io::Stdout, response: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(response)?;
    bytes.push(b'\n');
    output.write_all(&bytes).await?;
    output.flush().await?;
    Ok(())
}
