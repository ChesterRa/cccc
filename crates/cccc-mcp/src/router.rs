use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

use crate::mapping;
use crate::{RequestContext, ToolCallError};

#[cfg(test)]
async fn call(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    args: Map<String, Value>,
) -> Result<Value, ToolCallError> {
    call_with_context(home, client, name, args, None, false).await
}

pub(crate) async fn call_with_context(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    arguments: Map<String, Value>,
    context: Option<RequestContext<'_>>,
    via_capability_use: bool,
) -> Result<Value, ToolCallError> {
    let external_arguments = if name.starts_with("cccc_ext_") {
        arguments.clone()
    } else {
        Map::new()
    };
    let arguments = if name.starts_with("cccc_ext_") {
        // The external schema does not contain CCCC routing fields. A bound
        // session supplies them; a local user session uses its active Group.
        let mut envelope = Map::new();
        if let Some(group) = runtime_group_id(home, cccc_core::home::context_var("CCCC_GROUP_ID")) {
            envelope.insert("group_id".into(), json!(group));
        }
        envelope
    } else {
        arguments
    };
    call_scoped(
        home,
        client,
        name,
        arguments,
        external_arguments,
        context,
        via_capability_use,
    )
    .await
}

async fn call_scoped(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    mut arguments: Map<String, Value>,
    external_arguments: Map<String, Value>,
    context: Option<RequestContext<'_>>,
    via_capability_use: bool,
) -> Result<Value, ToolCallError> {
    if let Some(binding) = context.and_then(|context| context.native_binding) {
        crate::remote_native::authorize(home, binding, name, &arguments)?;
        if name == "cccc_session" && cccc_core::session_nodes::is_remote_source(&binding.source_id)
        {
            return crate::remote_native::session(home, client, binding, arguments).await;
        }
    }
    if name == "cccc_session" {
        return Err(
            "This tool requires native local-session integration; use cccc sessions enable".into(),
        );
    }
    arguments.remove("_cccc_secretary_token");
    if crate::secretary_task_mode() {
        let token = crate::secretary_task_token(home).ok_or(
            "Secretary task grant is unavailable; no user or Actor authority is available",
        )?;
        if name != "cccc_voice_secretary_task" || via_capability_use {
            return Err("Secretary task sessions cannot call Actor or user tools".into());
        }
        if ["group_id", "actor_id", "by", "document_path", "scope_key"]
            .iter()
            .any(|key| arguments.contains_key(*key))
        {
            return Err("Secretary task targets are fixed by the host".into());
        }
        arguments.insert("_cccc_secretary_token".into(), Value::String(token));
        return Ok(tool_result(Value::Object(
            daemon(client, "voice_secretary_task", arguments).await?,
        )));
    }
    if name == "cccc_voice_secretary_task" {
        return Err("This tool requires a host-issued secretary task grant".into());
    }
    if is_message_operation(name)
        && let Some(key) = arguments
            .keys()
            .find(|key| cccc_contracts::message_args::IM.contains(&key.as_str()))
    {
        return Err(format!("IM provenance is host-owned: {key}").into());
    }
    crate::tools::apply_default_action(name, &mut arguments);
    remove_untrusted_authority(&mut arguments);
    if name == "cccc_headless"
        && let Some(context) = context
        && arguments
            .get("actor_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|target| !target.is_empty() && target != context.actor_id)
    {
        return Err("invalid_args: A bound session can only use this tool for itself. Use cccc_actor(action=\"list\") to check another member.".into());
    }
    // The bound identity replaces `actor_id` below. A read of another member's state names
    // that member in the same field, so keep the target the caller gave for the projection,
    // before the runtime environment fills an omitted one.
    let agent_state_target = agent_state_read_target(name, &arguments);
    add_runtime_context(home, &mut arguments);
    arguments.remove("_cccc_web_binding");
    arguments.remove("_cccc_native_binding");
    apply_mcp_identity(
        name,
        &mut arguments,
        context.is_some_and(|context| context.native_binding.is_some()),
    );
    if matches!(name, "cccc_message_send" | "cccc_file_send") {
        crate::argument_normalization::check_recipients(&arguments)?;
        if !arguments.contains_key("mode") {
            let native = context.is_some_and(|context| context.native_binding.is_some());
            let mode = crate::argument_normalization::default_new_message_mode(&arguments, native);
            arguments.insert("mode".into(), json!(mode));
        }
    }
    if let Some(context) = context {
        if let Some(binding) = context.native_binding {
            arguments.insert(
                "_cccc_native_binding".into(),
                serde_json::to_value(binding).map_err(|e| e.to_string())?,
            );
        }
        if let Some(binding) = context.binding {
            cccc_core::web_model_connectors::validate_binding(home, binding)
                .map_err(|e| e.to_string())?;
            arguments.insert("_cccc_web_binding".into(), binding.clone());
        }
        apply_request_context(name, &mut arguments, context);
    }
    if name == "cccc_task" {
        prepare_task_arguments(home, &mut arguments)?;
    }
    authorize_tool(home, name, &arguments, via_capability_use)?;
    if name == "cccc_capability_use" {
        return capability_use(home, client, arguments, context).await;
    }
    let message_operation = is_message_operation(name);
    let message_context = message_operation.then(|| {
        (
            arguments
                .get("group_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            arguments
                .get("by")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            crate::response_projection::EchoedFields::capture(&arguments),
        )
    });
    if message_operation {
        arguments.insert("require_peer_insight".into(), Value::Bool(true));
    }
    if name == "cccc_message_send" {
        crate::cross_group::apply_cross_group_default(&mut arguments)?;
    }
    let payload = match name {
        "cccc_help" => {
            return group_help(client, arguments).await;
        }
        "cccc_bootstrap" => {
            let mut payload = crate::bootstrap::build(home, client, arguments).await?;
            if let Some(binding) = context.and_then(|context| context.native_binding)
                && cccc_core::session_nodes::is_remote_source(&binding.source_id)
            {
                let source = cccc_core::session_nodes::source(home, &binding.source_id)
                    .map_err(|error| error.to_string())?;
                payload["session"]["effective_scope"] = json!({"path":source.source.cwd,"source":"remote_native","node_id":source.node_id});
                payload["remote_execution"] = json!({"cwd":source.source.cwd,"node_name":source.node_name,"collaboration_only":true,"instructions":"Use this native application's own file, command and approval tools on its original machine. CCCC filesystem/exec and attachment transfer are unavailable through this LAN connector."});
            }
            if context.is_none_or(|context| context.native_binding.is_some()) {
                payload["mcp_server"] = current_mcp_identity();
            }
            return Ok(tool_result(payload));
        }
        "cccc_runtime_list" => json!({"runtimes": cccc_runtime::detect_runtimes()
            .into_iter()
            .map(|runtime| runtime.name)
            .collect::<Vec<_>>() }),
        name if is_repo_tool(name) => {
            let result = crate::local_tools::call(home, client, name, arguments).await?;
            return Ok(if message_operation {
                let (group_id, actor_id, fields) =
                    message_context.as_ref().expect("message context");
                with_post_message_context(home, result, group_id, actor_id, name, fields)
            } else {
                result
            });
        }
        _ => {
            let (op, args) = match mapping::daemon_call(name, arguments.clone()) {
                Ok(mapped) => mapped,
                Err(error) if error.starts_with("tool is not a daemon operation:") => {
                    let mut dynamic = Map::new();
                    if let Some(value) = arguments.get("group_id").cloned() {
                        dynamic.insert("group_id".into(), value);
                    }
                    if let Some(value) = arguments.get("actor_id").cloned() {
                        dynamic.insert("actor_id".into(), value);
                    }
                    if let Some(value) = arguments.get("by").cloned() {
                        dynamic.insert("by".into(), value);
                    }
                    if let Some(value) = arguments.get("capability_id").cloned() {
                        dynamic.insert("capability_id".into(), value);
                    }
                    for key in ["_cccc_native_binding", "_cccc_web_binding"] {
                        if let Some(value) = arguments.get(key).cloned() {
                            dynamic.insert(key.into(), value);
                        }
                    }
                    dynamic.insert("tool_name".into(), Value::String(name.into()));
                    // Host authority stays in the daemon envelope. External
                    // parameters may legitimately use the same field names.
                    dynamic.insert("arguments".into(), Value::Object(external_arguments));
                    let payload = daemon(client, "capability_tool_call", dynamic).await?;
                    let failed = payload
                        .get("result")
                        .and_then(|value| value.get("isError"))
                        .and_then(Value::as_bool)
                        == Some(true);
                    let mut result = tool_result(Value::Object(payload));
                    if failed {
                        result["isError"] = json!(true);
                    }
                    return Ok(result);
                }
                Err(error) => return Err(error.into()),
            };
            let mut result = daemon(client, &op, args).await?;
            match &agent_state_target {
                Some(target) => {
                    let mut projection = arguments.clone();
                    projection.insert("actor_id".into(), target.clone());
                    crate::context_projection::apply(name, &mut result, &projection);
                }
                None => crate::context_projection::apply(name, &mut result, &arguments),
            }
            crate::history_projection::apply(name, &mut result, &arguments);
            if name == "cccc_backlog" {
                crate::backlog_projection::apply(&mut result, &arguments)?;
            }
            if name == "cccc_task" {
                postprocess_task_result(&mut result, &arguments)?;
                if is_task_mail_boundary(&arguments) {
                    let group_id = arguments
                        .get("group_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let actor_id = arguments
                        .get("by")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    insert_mail_pending_context(home, &mut result, group_id, actor_id);
                }
            }
            if name == "cccc_actor_notes" {
                postprocess_actor_notes(client, &mut result, &arguments).await;
            }
            Value::Object(result)
        }
    };
    let result = tool_result(payload);
    Ok(if message_operation {
        let (group_id, actor_id, fields) = message_context.as_ref().expect("message context");
        with_post_message_context(home, result, group_id, actor_id, name, fields)
    } else {
        result
    })
}

fn authorize_tool(
    home: &HomeLayout,
    name: &str,
    arguments: &Map<String, Value>,
    via_capability_use: bool,
) -> Result<(), String> {
    let actor_id = arguments
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let group_id = arguments
        .get("group_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let group = if actor_id == "user" || group_id.is_empty() {
        None
    } else {
        Some(
            cccc_core::GroupStore::new(home.clone())
                .and_then(|store| store.load(group_id))
                .map_err(|error| error.to_string())?,
        )
    };
    if let Some(group) = group.as_ref()
        && group
            .actors
            .iter()
            .find(|actor| actor.id == actor_id)
            .is_some_and(|actor| actor.runtime.is_web_model())
    {
        let role = cccc_core::actors::effective_role(group, actor_id)
            .unwrap_or(cccc_contracts::ActorRole::Peer);
        // The daemon authorizes each automation change by the caller's role, so a peer
        // manages only reminders to itself; a Web Model peer may reach it like any peer.
        let open_to_peers = name == "cccc_automation";
        if cccc_core::web_model_tool_names().any(|tool| tool == name)
            || (via_capability_use
                && (role == cccc_contracts::ActorRole::Foreman || open_to_peers)
                && cccc_core::is_builtin_capability_pack_tool(name))
        {
            return Ok(());
        }
        return if role == cccc_contracts::ActorRole::Peer {
            Err(format!("{name} requires a Web Model foreman actor"))
        } else {
            Err(format!("{name} is not available to Web Model actors"))
        };
    }
    if !matches!(
        name,
        "cccc_capability_import" | "cccc_capability_block" | "cccc_capability_uninstall"
    ) || actor_id == "user"
    {
        return Ok(());
    }
    if group_id.is_empty() {
        return Err("group_id is required".into());
    }
    let group = match group {
        Some(group) => group,
        None => cccc_core::GroupStore::new(home.clone())
            .and_then(|store| store.load(group_id))
            .map_err(|error| error.to_string())?,
    };
    let peer = cccc_core::actors::effective_role(&group, actor_id)
        == Some(cccc_contracts::ActorRole::Peer);
    if peer {
        Err(format!("{name} is not available to peer actors"))
    } else {
        Ok(())
    }
}

async fn capability_use(
    home: &HomeLayout,
    client: &DaemonClient,
    arguments: Map<String, Value>,
    context: Option<RequestContext<'_>>,
) -> Result<Value, ToolCallError> {
    let group_id = arguments
        .get("group_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    let actor_id = arguments
        .get("actor_id")
        .and_then(Value::as_str)
        .or_else(|| arguments.get("by").and_then(Value::as_str))
        .unwrap_or("user")
        .trim()
        .to_owned();
    let by = arguments
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or(&actor_id)
        .trim()
        .to_owned();
    let requested_scope = arguments
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("session")
        .trim()
        .to_ascii_lowercase();
    let ttl_seconds = arguments
        .get("ttl_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(3600)
        .clamp(60, 86_400);
    let reason = arguments
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let tool_name = arguments
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if tool_name == "cccc_capability_use" {
        return Err(
            "capability_use_invalid_tool: cccc_capability_use cannot recursively call itself"
                .into(),
        );
    }
    let mut capability_id = arguments
        .get("capability_id")
        .or_else(|| arguments.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();

    let state_probe = if tool_name.is_empty() {
        Map::new()
    } else {
        capability_state(client, &group_id, &actor_id, &by, &arguments)
            .await
            .unwrap_or_default()
    };
    if capability_id.is_empty() && crate::is_core_tool(&tool_name) {
        capability_id = "core".into();
    }
    if capability_id.is_empty() && !tool_name.is_empty() {
        let builtin_pack_tool = cccc_core::is_builtin_capability_pack_tool(&tool_name);
        let mut candidates = cccc_core::capabilities::CapabilityStore::new(home.clone())
            .catalog()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|capability| {
                !builtin_pack_tool
                    || cccc_core::capabilities::CapabilityStore::builtin(&capability.id).is_some()
            })
            .filter(|capability| capability.tool_names.iter().any(|name| name == &tool_name))
            .map(|capability| capability.id)
            .collect::<Vec<_>>();
        if !builtin_pack_tool {
            candidates.extend(
                state_probe
                    .get("dynamic_tools")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|tool| {
                        tool.get("name").and_then(Value::as_str) == Some(tool_name.as_str())
                            || tool.get("real_tool_name").and_then(Value::as_str)
                                == Some(tool_name.as_str())
                    })
                    .filter_map(|tool| tool.get("capability_id").and_then(Value::as_str))
                    .map(str::to_owned),
            );
        }
        candidates.sort();
        candidates.dedup();
        if candidates.len() > 1 {
            return Err(format!(
                "capability_use_ambiguous_tool: tool maps to multiple capabilities: {tool_name} ({})",
                candidates.join(", ")
            )
            .into());
        }
        capability_id = candidates.pop().unwrap_or_default();
    }
    if capability_id.is_empty() {
        return Err(
            "missing_capability_id: pass capability_id when it cannot be inferred from tool_name"
                .into(),
        );
    }
    // A real name resolved to an MCP stays in the external namespace even if
    // the server happens to name a tool after a CCCC operation.
    let routed_tool_name = if !tool_name.is_empty()
        && !tool_name.starts_with("cccc_ext_")
        && capability_id != "core"
    {
        let capability = cccc_core::capabilities::CapabilityStore::new(home.clone())
            .require(&capability_id)
            .map_err(|error| error.to_string())?;
        if capability_id.starts_with("mcp:")
            || matches!(capability.kind.as_str(), "mcp" | "mcp_toolpack")
        {
            cccc_core::capabilities::external_tool_name(&capability_id, &tool_name)
        } else {
            tool_name.clone()
        }
    } else {
        tool_name.clone()
    };

    let already_enabled = state_probe
        .get("enabled_capabilities")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.as_str() == Some(&capability_id))
        });
    let reused_existing_binding = !tool_name.is_empty() && already_enabled;
    let enable_result = if capability_id == "core" {
        json!({
            "state":"runnable","enabled":true,"refresh_required":false,"scope":"core"
        })
        .as_object()
        .cloned()
        .expect("core enable result")
    } else if reused_existing_binding {
        json!({
            "state":"runnable","enabled":true,"refresh_required":false,
            "reused_existing_binding":true,"scope":requested_scope
        })
        .as_object()
        .cloned()
        .expect("reused enable result")
    } else {
        let mut enable_args = json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "by":by,
            "capability_id":capability_id,
            "scope":requested_scope,
            "enabled":true,
            "ttl_seconds":ttl_seconds,
            "reason":reason,
            "prepare":tool_name.is_empty(),
        })
        .as_object()
        .cloned()
        .expect("capability enable args");
        for key in ["_cccc_web_binding", "_cccc_native_binding"] {
            if let Some(binding) = arguments.get(key) {
                enable_args.insert(key.into(), binding.clone());
            }
        }
        daemon(client, "capability_enable", enable_args).await?
    };
    let state = enable_result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("runnable")
        .trim()
        .to_ascii_lowercase();
    let enabled = matches!(
        state.as_str(),
        "ready" | "activation_pending" | "runnable" | "verified"
    );
    let scope = enable_result
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or(&requested_scope)
        .to_owned();
    if !enabled || tool_name.is_empty() {
        let mut result = json!({
            "group_id":group_id,
            "actor_id":actor_id,
            "capability_id":capability_id,
            "scope":scope,
            "requested_scope":requested_scope,
            "enabled":enabled,
            "state":state,
            "refresh_required":enable_result
                .get("refresh_required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            "enable_result":enable_result,
            "tool_called":false,
        });
        // A skill has no tool to call: its rules are what the caller came for.
        if capability_id != "core" {
            let store = cccc_core::capabilities::CapabilityStore::new(home.clone());
            let needs_body = enabled && capability_id.starts_with("skill:");
            // Optional guidance must not replace an enable diagnostic when a later catalog read fails.
            let capability = if needs_body {
                Some(
                    store
                        .require(&capability_id)
                        .map_err(|error| error.to_string())?,
                )
            } else {
                store.require(&capability_id).ok()
            };
            if let Some(capability) = capability {
                result
                    .as_object_mut()
                    .expect("capability response")
                    .extend(cccc_core::capability_recommendations::notes(&capability));
                if needs_body {
                    result["capsule_text"] = Value::String(capability.capsule_text);
                }
            }
        }
        return Ok(tool_result(result));
    }

    let mut tool_arguments = arguments
        .get("tool_arguments")
        .or_else(|| arguments.get("arguments"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let external_arguments = tool_arguments.clone();
    tool_arguments
        .entry("group_id")
        .or_insert_with(|| Value::String(group_id.clone()));
    tool_arguments
        .entry("by")
        .or_insert_with(|| Value::String(by.clone()));
    tool_arguments
        .entry("actor_id")
        .or_insert_with(|| Value::String(actor_id.clone()));
    if routed_tool_name.starts_with("cccc_ext_") {
        tool_arguments =
            json!({"group_id":group_id,"actor_id":actor_id,"by":by,"capability_id":capability_id})
                .as_object()
                .expect("external routing envelope")
                .clone();
    }
    let nested = Box::pin(call_scoped(
        home,
        client,
        &routed_tool_name,
        tool_arguments,
        external_arguments,
        context,
        true,
    ))
    .await?;
    let tool_failed = nested.get("isError").and_then(Value::as_bool) == Some(true)
        || nested
            .pointer("/structuredContent/result/isError")
            .and_then(Value::as_bool)
            == Some(true);
    let state_update_skipped = nested
        .pointer("/structuredContent/state_update_skipped")
        .and_then(Value::as_bool)
        == Some(true);
    let state_update_skipped_reason = nested
        .pointer("/structuredContent/state_update_skipped_reason")
        .cloned();
    let mut nested_payload = nested.get("structuredContent").cloned().unwrap_or(nested);
    let mut notes = Map::new();
    if routed_tool_name.starts_with("cccc_ext_")
        && let Some(payload) = nested_payload.as_object_mut()
    {
        for key in cccc_core::capability_recommendations::NOTE_FIELDS {
            if let Some(value) = payload.remove(key) {
                notes.insert(key.into(), value);
            }
        }
    }
    let mut payload = json!({
        "group_id":group_id,
        "actor_id":actor_id,
        "capability_id":capability_id,
        "scope":scope,
        "requested_scope":requested_scope,
        "enabled":true,
        "state":if state_update_skipped {"state_update_skipped"} else if tool_failed {"runnable"} else {"verified"},
        "refresh_required":false,
        "verification_source":if state_update_skipped {""} else if tool_failed {"tool_error"} else {"tool_call"},
        "reused_existing_binding":reused_existing_binding,
        "enable_result":enable_result,
        "tool_called":true,
        "tool_name":tool_name,
        "tool_result":nested_payload,
    });
    payload
        .as_object_mut()
        .expect("capability response")
        .extend(notes);
    if state_update_skipped {
        payload
            .as_object_mut()
            .expect("capability response")
            .remove("enabled");
        payload["state_update_skipped"] = json!(true);
        if let Some(reason) = state_update_skipped_reason {
            payload["state_update_skipped_reason"] = reason;
        }
    }
    let mut result = tool_result(payload);
    if tool_failed {
        result["isError"] = json!(true);
    }
    Ok(result)
}

async fn capability_state(
    client: &DaemonClient,
    group_id: &str,
    actor_id: &str,
    by: &str,
    authority: &Map<String, Value>,
) -> Result<Map<String, Value>, ToolCallError> {
    let mut args = json!({"group_id":group_id,"actor_id":actor_id,"by":by})
        .as_object()
        .cloned()
        .expect("capability state args");
    for key in ["_cccc_web_binding", "_cccc_native_binding"] {
        if let Some(binding) = authority.get(key) {
            args.insert(key.into(), binding.clone());
        }
    }
    daemon(client, "capability_state", args).await
}

fn prepare_task_arguments(
    home: &HomeLayout,
    arguments: &mut Map<String, Value>,
) -> Result<(), String> {
    if arguments.get("action").and_then(Value::as_str) != Some("create")
        || arguments.contains_key("assignee")
    {
        return Ok(());
    }
    let by = arguments
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if by.is_empty() || matches!(by.as_str(), "user" | "system") {
        return Ok(());
    }
    let group_id = arguments
        .get("group_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "group_id is required".to_owned())?;
    let group = cccc_core::GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(|error| error.to_string())?;
    if cccc_core::actors::effective_role(&group, &by) == Some(cccc_contracts::ActorRole::Peer) {
        arguments.insert("assignee".into(), Value::String(by));
    }
    Ok(())
}

fn postprocess_task_result(
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) -> Result<(), String> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("list");
    if !matches!(action, "get" | "list") {
        return Ok(());
    }
    let include_archived = arguments
        .get("include_archived")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !include_archived
        && result
            .get("task")
            .and_then(Value::as_object)
            .and_then(|task| task.get("status"))
            .and_then(Value::as_str)
            == Some("archived")
    {
        return Err("archived_hidden: archived task is hidden by default".into());
    }
    crate::task_summary::summarize_children(result);
    crate::task_summary::list_page(result, arguments)
}

fn is_task_mail_boundary(arguments: &Map<String, Value>) -> bool {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("list");
    if !matches!(action, "create" | "update" | "move") {
        return false;
    }
    if arguments.get("status").and_then(Value::as_str) == Some("done") {
        return true;
    }
    matches!(
        arguments.get("waiting_on").and_then(Value::as_str),
        Some("actor" | "external")
    ) || arguments
        .get("blocked_by")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty())
}

fn help_markdown() -> String {
    format!(
        "{}\n\n{}\n",
        cccc_core::group_prompts::BUILTIN_HELP_MARKDOWN.trim_end(),
        cccc_core::peer_insight::PEER_INSIGHT_RUNTIME_HELP.as_str()
    )
}

async fn group_help(
    client: &DaemonClient,
    arguments: Map<String, Value>,
) -> Result<Value, ToolCallError> {
    if !arguments.contains_key("group_id") {
        return Ok(tool_result(json!({
            "markdown": help_markdown(),
            "source": "resources/cccc-help.md",
        })));
    }
    let result = daemon(client, "group_help_get", arguments).await?;
    let base = result
        .get("markdown")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let source = result
        .get("source_path")
        .and_then(Value::as_str)
        .unwrap_or("resources/cccc-help.md");
    Ok(tool_result(json!({
        "markdown": append_peer_insight_runtime_help(base),
        "source": source,
    })))
}

fn append_peer_insight_runtime_help(markdown: &str) -> String {
    let reserved = "## Peer Insight Contract (Runtime)";
    let mut kept = Vec::new();
    let mut current = Vec::new();
    let mut skip = false;
    let flush = |kept: &mut Vec<String>, current: &mut Vec<String>, skip: bool| {
        if !skip {
            kept.append(current);
        } else {
            current.clear();
        }
    };
    for line in markdown.lines() {
        if line.starts_with("## ") && !line.starts_with("### ") {
            flush(&mut kept, &mut current, skip);
            skip = line.trim() == reserved;
        }
        current.push(line.to_owned());
    }
    flush(&mut kept, &mut current, skip);
    format!(
        "{}\n\n{}\n",
        kept.join("\n").trim_end(),
        cccc_core::peer_insight::PEER_INSIGHT_RUNTIME_HELP.as_str()
    )
}

async fn postprocess_actor_notes(
    client: &DaemonClient,
    result: &mut Map<String, Value>,
    arguments: &Map<String, Value>,
) {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("get");
    let changed = result
        .remove("changed")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !matches!(action, "set" | "clear") || !changed {
        if matches!(action, "set" | "clear") {
            result.insert("notified_actor_ids".into(), json!([]));
        }
        return;
    }
    let Some(group_id) = arguments.get("group_id").and_then(Value::as_str) else {
        result.insert("notified_actor_ids".into(), json!([]));
        return;
    };
    let target = result
        .get("target_actor_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut list_args = Map::new();
    list_args.insert("group_id".into(), Value::String(group_id.into()));
    let actors = daemon(client, "actor_list", list_args)
        .await
        .ok()
        .and_then(|value| value.get("actors").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    let running = actors.iter().find(|actor| {
        actor.get("id").and_then(Value::as_str) == Some(target)
            && actor.get("running").and_then(Value::as_bool) == Some(true)
    });
    if running.is_none() {
        result.insert("notified_actor_ids".into(), json!([]));
        return;
    }
    let notify_args = json!({
        "group_id": group_id,
        "by": "system",
        "kind": "info",
        "priority": "normal",
        "title": "Help updated: your actor note",
        "message": "Updated: your actor note. Run `cccc_help` now to refresh your effective protocol reference; then update `cccc_agent_state` if your plan changes.",
        "target_actor_id": target,
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    let notified = daemon(client, "system_notify", notify_args).await.is_ok();
    result.insert(
        "notified_actor_ids".into(),
        if notified { json!([target]) } else { json!([]) },
    );
}

pub(crate) async fn daemon(
    client: &DaemonClient,
    op: &str,
    args: Map<String, Value>,
) -> Result<Map<String, Value>, ToolCallError> {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .map_err(|error| ToolCallError::from(error.to_string()))?;
    if response.ok {
        return Ok(response.result);
    }
    Err(response.error.map_or_else(
        || ToolCallError::from("daemon operation failed"),
        ToolCallError::from_daemon,
    ))
}

fn add_runtime_context(home: &HomeLayout, args: &mut Map<String, Value>) {
    if !args.contains_key("group_id") {
        let group = runtime_group_id(home, cccc_core::home::context_var("CCCC_GROUP_ID"));
        if let Some(group) = group {
            args.insert("group_id".into(), Value::String(group));
        }
    }
    let actor =
        cccc_core::home::context_var("CCCC_ACTOR_ID").filter(|value| !value.trim().is_empty());
    apply_actor_context(args, actor.as_deref());
}

fn runtime_group_id(home: &HomeLayout, configured: Option<String>) -> Option<String> {
    match configured {
        Some(value) => (!value.trim().is_empty()).then_some(value),
        None => cccc_core::active::get(home).ok().flatten(),
    }
}

fn apply_actor_context(args: &mut Map<String, Value>, actor: Option<&str>) {
    if let Some(actor) = actor.map(str::trim).filter(|actor| !actor.is_empty()) {
        args.entry("actor_id")
            .or_insert_with(|| Value::String(actor.to_owned()));
        // The process environment is set by the runtime and is authoritative.
        // Tool arguments are model-controlled and must not be able to impersonate user.
        args.insert("by".into(), Value::String(actor.to_owned()));
    }
}

/// The member a `cccc_agent_state` read asks for. Omitted, the read stays on the caller; writes
/// never take a target.
fn agent_state_read_target(name: &str, args: &Map<String, Value>) -> Option<Value> {
    if name != "cccc_agent_state"
        || !matches!(
            args.get("action").and_then(Value::as_str),
            None | Some("get" | "list")
        )
    {
        return None;
    }
    args.get("actor_id")
        .filter(|value| value.as_str().is_some_and(|id| !id.trim().is_empty()))
        .cloned()
}

fn remove_untrusted_authority(args: &mut Map<String, Value>) {
    // These fields belong to authenticated human-facing ports. MCP arguments are
    // model-controlled; Actor authority is supplied separately through `by`.
    for key in ["caller_id", "is_admin", "allowed_groups"] {
        args.remove(key);
    }
}

fn apply_request_context(name: &str, args: &mut Map<String, Value>, context: RequestContext<'_>) {
    // A remote connector is bound to exactly one actor and group. Its request
    // arguments are model-controlled, so the request-scoped binding is authoritative.
    args.insert(
        "group_id".into(),
        Value::String(context.group_id.to_owned()),
    );
    // Actor management addresses a target; `by` remains the authenticated caller.
    // Identity-bearing tools must still use the bound Actor, including nested calls.
    if name == "cccc_actor" {
        args.entry("actor_id")
            .or_insert_with(|| Value::String(context.actor_id.to_owned()));
    } else {
        args.insert(
            "actor_id".into(),
            Value::String(context.actor_id.to_owned()),
        );
    }
    args.insert("by".into(), Value::String(context.actor_id.to_owned()));
}

pub(crate) fn tool_result(payload: Value) -> Value {
    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into());
    json!({"content":[{"type":"text","text":text}],"structuredContent":payload})
}

pub(crate) fn is_message_operation(name: &str) -> bool {
    matches!(
        name,
        "cccc_message_send" | "cccc_tracked_send" | "cccc_message_reply" | "cccc_file_send"
    )
}

fn with_post_message_context(
    home: &HomeLayout,
    mut result: Value,
    group_id: &str,
    actor_id: &str,
    name: &str,
    fields: &crate::response_projection::EchoedFields,
) -> Value {
    if let Some(payload) = result
        .get_mut("structuredContent")
        .and_then(Value::as_object_mut)
    {
        insert_mail_pending_context(home, payload, group_id, actor_id);
        crate::response_projection::apply(name, payload, fields);
        let text = serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".into());
        result["content"] = json!([{"type":"text","text":text}]);
    }
    result
}

fn insert_mail_pending_context(
    home: &HomeLayout,
    payload: &mut Map<String, Value>,
    group_id: &str,
    actor_id: &str,
) {
    if group_id.is_empty() || matches!(actor_id, "" | "user" | "system") {
        return;
    }
    let Ok(store) = cccc_core::GroupStore::new(home.clone()) else {
        return;
    };
    let Ok(group) = store.load(group_id) else {
        return;
    };
    let Ok(Some(pending)) = cccc_core::inbox::mail_pending_summary(home, &group, actor_id) else {
        return;
    };
    payload.insert("mail_pending".into(), pending);
}

fn is_repo_tool(name: &str) -> bool {
    matches!(
        name,
        "cccc_repo"
            | "cccc_repo_edit"
            | "cccc_apply_patch"
            | "cccc_shell"
            | "cccc_exec_command"
            | "cccc_write_stdin"
            | "cccc_git"
            | "cccc_code_exec"
            | "cccc_code_wait"
            | "cccc_file"
            | "cccc_file_send"
    )
}

fn current_mcp_identity() -> Value {
    let mut identity = cccc_core::build_info::current();
    identity["version"] = json!(env!("CARGO_PKG_VERSION"));
    identity
}

/// Diagnostic metadata describes this request's native MCP, not Actor authority.
fn apply_mcp_identity(name: &str, arguments: &mut Map<String, Value>, native: bool) {
    arguments.remove("_cccc_mcp_identity");
    if native
        && name == "cccc_message_history"
        && arguments
            .get("event_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
    {
        arguments.insert("_cccc_mcp_identity".into(), current_mcp_identity());
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
