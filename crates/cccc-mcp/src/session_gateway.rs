//! An opt-in boundary for a trusted ChatGPT tunnel, not an HTTP authentication layer.
//! The host supplies conversation metadata outside the model-controlled arguments.
use crate::{RequestContext, ToolCallError};
use cccc_client::DaemonClient;
use cccc_core::{HomeLayout, web_model_connectors};
use serde_json::{Map, Value, json};

fn catalog() -> Vec<Value> {
    let mut tools = crate::tools::catalog()
        .into_iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .is_some_and(|name| cccc_core::WEB_MODEL_CORE_TOOL_NAMES.contains(&name))
        })
        .collect::<Vec<_>>();
    crate::hide_disabled_code_mode_tools(&mut tools);
    tools.push(json!({
        "name":"cccc_session_bind",
        "description":"Bind this ChatGPT conversation to the group selected by a one-use connection code. The code is issued locally; conversation identity is supplied by ChatGPT, never by tool arguments.",
        "inputSchema":{"type":"object","properties":{"code":{"type":"string","minLength":1}},"required":["code"],"additionalProperties":false},
        "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":false}
    }));
    for (name, required, description, properties) in [
        (
            "cccc_group_create",
            "path",
            "Create a group from this ChatGPT conversation and bind its web Foreman. Retries reuse the same group. Supply this chat's URL for browser return messages; a missing URL does not prevent local dispatch.",
            json!({"path":{"type":"string"},"title":{"type":"string"},"chat_url":{"type":"string"}}),
        ),
        (
            "cccc_group_bind",
            "group",
            "Bind this Chat to an existing unowned group, or update the callback URL of its own group. A different Chat must use a freshly issued connection code to replace a binding; local Foremen are never silently replaced.",
            json!({"group":{"type":"string"},"chat_url":{"type":"string"}}),
        ),
    ] {
        tools.push(json!({"name":name,"description":description,
            "inputSchema":{"type":"object","properties":properties,"required":[required],"additionalProperties":false},
            "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
    }
    tools
}

fn binding_error(error: std::io::Error) -> ToolCallError {
    let message = error.to_string();
    let code = if crate::valid_error_code(&message) {
        message.as_str()
    } else {
        "session_binding_failed"
    };
    ToolCallError::new(
        code,
        "The local conversation binding is unavailable or invalid.",
        Map::new(),
    )
}

fn required_binding() -> ToolCallError {
    ToolCallError::new(
        "session_binding_required",
        "This conversation is not bound. Use a locally issued connection code with cccc_session_bind. A group_id argument cannot select a group.",
        Map::new(),
    )
}

pub(crate) async fn handle(home: &HomeLayout, client: &DaemonClient, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request["method"].as_str() {
        Some("tools/list") => return json!({"jsonrpc":"2.0","id":id,"result":{"tools":catalog()}}),
        Some("tools/call") => {}
        _ => return crate::handle(home, client, request, None).await,
    }
    let Some(params) = request.get("params").and_then(Value::as_object) else {
        return crate::protocol_error(id, -32602, "tools/call params must be an object");
    };
    let Some(name) = params
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
    else {
        return crate::protocol_error(id, -32602, "tools/call name must be a non-empty string");
    };
    let arguments = match params.get("arguments") {
        None => Map::new(),
        Some(Value::Object(arguments)) => arguments.clone(),
        _ => return crate::protocol_error(id, -32602, "tools/call arguments must be an object"),
    };
    if !catalog().iter().any(|tool| tool["name"] == name) {
        return crate::protocol_error(id, -32602, "Unknown gateway tool");
    }
    let session = params
        .get("_meta")
        .and_then(|meta| meta.get("openai/session"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let result = match session {
        None => Err(required_binding()),
        Some(session) => call(home, client, name, arguments, session).await,
    };
    json!({"jsonrpc":"2.0","id":id,"result":match result {
        Ok(result)=>result, Err(error)=>crate::tool_error_result(&error)
    }})
}

async fn call(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    arguments: Map<String, Value>,
    session: &str,
) -> Result<Value, ToolCallError> {
    if matches!(name, "cccc_group_create" | "cccc_group_bind") {
        let mut args = arguments;
        if args.contains_key("session") || args.contains_key("by") {
            return Err("invalid_request: identity is supplied by the ChatGPT transport".into());
        }
        args.insert("session".into(), json!(session));
        args.insert("by".into(), json!("user"));
        let op = if name == "cccc_group_create" {
            "web_model_chat_create"
        } else {
            "web_model_chat_bind"
        };
        return crate::router::daemon(client, op, args)
            .await
            .map(|value| crate::router::tool_result(Value::Object(value)));
    }
    if name == "cccc_session_bind" {
        if arguments.len() != 1 {
            return Err("invalid_args: cccc_session_bind accepts only code".into());
        }
        let code = arguments
            .get("code")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .ok_or("session_binding_code_invalid: code must be a non-empty string")?;
        let connector = web_model_connectors::find_binding_code(home, code)
            .map_err(binding_error)?
            .ok_or("session_binding_code_invalid: connection code is invalid or already used")?;
        let connector_id = connector["connector_id"]
            .as_str()
            .ok_or("session_binding_failed: connector has no identity")?;
        let home = home.clone();
        let connector_id = connector_id.to_owned();
        let code = code.to_owned();
        let session = session.to_owned();
        return tokio::task::spawn_blocking(move || {
            web_model_connectors::bind_session(&home, &connector_id, &code, &session)
        })
        .await
        .map_err(|error| binding_error(std::io::Error::other(error)))?
        .map(crate::router::tool_result)
        .map_err(binding_error);
    }
    let connector = web_model_connectors::find_session(home, session)
        .map_err(binding_error)?
        .ok_or_else(required_binding)?;
    let group_id = connector["group_id"]
        .as_str()
        .ok_or_else(required_binding)?;
    let actor_id = connector["actor_id"]
        .as_str()
        .ok_or_else(required_binding)?;
    crate::router::call_with_context(
        home,
        client,
        name,
        arguments,
        Some(RequestContext {
            group_id,
            actor_id,
            gateway_session: Some(session),
        }),
        false,
    )
    .await
}

/// Applied at the shared dispatch boundary, including nested capability calls.
pub(crate) fn authorize_call(
    home: &HomeLayout,
    name: &str,
    arguments: &Map<String, Value>,
    context: RequestContext<'_>,
) -> Result<(), ToolCallError> {
    let Some(session) = context.gateway_session else {
        return Ok(());
    };
    let connector = web_model_connectors::find_session(home, session)
        .map_err(binding_error)?
        .ok_or_else(required_binding)?;
    if connector["group_id"] != context.group_id || connector["actor_id"] != context.actor_id {
        return Err(required_binding());
    }
    for key in ["group_id", "dst_group_id"] {
        if let Some(value) = arguments.get(key).filter(|v| !v.is_null()) {
            let value = value
                .as_str()
                .ok_or("invalid_args: group identifiers must be strings")?
                .trim();
            if !value.is_empty() && value != context.group_id {
                return Err(
                    "group_scope_mismatch: this conversation can only access its bound group"
                        .into(),
                );
            }
        }
    }
    if name.starts_with("cccc_remote_")
        || matches!(
            name,
            "cccc_im_bind" | "cccc_capability_import" | "cccc_capability_uninstall"
        )
    {
        return Err("group_scope_mismatch: global and remote administration is not available to a bound conversation".into());
    }
    if name == "cccc_group"
        && matches!(
            arguments.get("action").and_then(Value::as_str),
            Some("create" | "resolve" | "use")
        )
    {
        return Err(
            "group_scope_mismatch: this operation changes or searches outside the bound group"
                .into(),
        );
    }
    if name == "cccc_capability_use" {
        let nested = arguments
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if nested == "cccc_capability_use" {
            return Err(
                "capability_use_invalid_tool: cccc_capability_use cannot recursively call itself"
                    .into(),
            );
        }
        if !nested.is_empty() {
            let args = arguments
                .get("tool_arguments")
                .or_else(|| arguments.get("arguments"));
            let empty = Map::new();
            let args = match args {
                None | Some(Value::Null) => &empty,
                Some(Value::Object(args)) => args,
                _ => return Err("invalid_args: tool_arguments must be an object".into()),
            };
            authorize_call(home, nested, args, context)?;
        }
    }
    Ok(())
}
