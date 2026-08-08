mod actions;
mod code_mode;
mod local_sessions;
mod local_tools;
mod mapping;
mod remote_messages;
mod remote_tools;
mod repo;
mod router;
mod tools;

#[cfg(test)]
mod repo_tests;

use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {"listChanged": true}},
            "serverInfo": {"name": "cccc-mcp", "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => {
            Ok(json!({"tools": visible_tools_with_context(home, client, context).await}))
        }
        "tools/call" => {
            let params = request.get("params").and_then(Value::as_object);
            let name = params
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = params
                .and_then(|value| value.get("arguments"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            router::call_with_context(home, client, name, arguments, context).await
        }
        _ => Err(format!("unknown method: {method}")),
    };
    match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(message) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":message}}),
    }
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
                ("group_id".into(), Value::String(group_id)),
                ("actor_id".into(), Value::String(actor_id.clone())),
                ("by".into(), Value::String(actor_id)),
            ]),
        })
        .await;
    let Ok(response) = response else {
        return core_tools(catalog);
    };
    if !response.ok {
        return core_tools(catalog);
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
    const CORE: &[&str] = &[
        "cccc_help",
        "cccc_bootstrap",
        "cccc_capability_search",
        "cccc_capability_use",
        "cccc_inbox_list",
        "cccc_inbox_mark_read",
        "cccc_message_send",
        "cccc_message_reply",
        "cccc_file",
        "cccc_context_get",
        "cccc_coordination",
        "cccc_task",
        "cccc_agent_state",
    ];
    catalog
        .into_iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| CORE.contains(&name))
        })
        .collect()
}

async fn write_response(output: &mut tokio::io::Stdout, response: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(response)?;
    bytes.push(b'\n');
    output.write_all(&bytes).await?;
    output.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    #[test]
    fn unscoped_fallback_remains_the_thirteen_core_tools() {
        let names = super::core_tools(super::tools::catalog())
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
            "cccc_inbox_list",
            "cccc_inbox_mark_read",
            "cccc_message_reply",
            "cccc_message_send",
            "cccc_task",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

        assert_eq!(names, expected);
    }
}
