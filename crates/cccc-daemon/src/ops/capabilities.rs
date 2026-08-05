use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::capabilities::{Capability, CapabilityStore};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};

mod allowlist;
mod effective_state;
mod external_runtime;
mod overview;
mod package_install;
mod target_install;
mod uninstall;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "capability_overview" => overview::run(home, request),
        "capability_search" => search(home, request),
        "capability_enable" => enable(home, request),
        "capability_visibility" => visibility(home, request),
        "capability_block" => block(home, request),
        "capability_state" => state(home, request),
        "capability_import" => import(home, request),
        "capability_uninstall" => uninstall::run(home, request),
        "capability_install" | "capability_install_target" => target_install::run(home, request),
        "capability_source_delete" => source_delete(home, request),
        "capability_tool_call" => use_capability(home, request),
        "capability_allowlist_get" => allowlist::get(home),
        "capability_allowlist_validate" => allowlist::validate(home, request),
        "capability_allowlist_update" => allowlist::update(home, request),
        "capability_allowlist_reset" => allowlist::reset(home, request),
        _ => return None,
    })
}

fn search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let query = string_arg(request, "query").unwrap_or_default();
    object(
        json!({"capabilities": CapabilityStore::new(home.clone()).search(&query).map_err(OpError::io)?}),
    )
}
fn enable(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let enabled = bool_arg(request, "enabled", true);
    let store = CapabilityStore::new(home.clone());
    if enabled {
        return target_install::enable(
            home,
            &required_arg(request, "group_id")?,
            &string_arg(request, "actor_id").unwrap_or_default(),
            &string_arg(request, "scope").unwrap_or_else(|| "group".into()),
            request
                .args
                .get("ttl_seconds")
                .and_then(Value::as_i64)
                .unwrap_or(3600),
            &id,
        );
    }
    let state = store
        .set_enabled_for(
            &id,
            enabled,
            &required_arg(request, "group_id")?,
            &string_arg(request, "actor_id").unwrap_or_default(),
            &string_arg(request, "scope").unwrap_or_else(|| "group".into()),
            request
                .args
                .get("ttl_seconds")
                .and_then(Value::as_i64)
                .unwrap_or(3600),
        )
        .map_err(OpError::invalid)?;
    object(json!({"capability_id": id, "state": state, "enabled":false}))
}
fn visibility(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let hidden = string_arg(request, "visibility").as_deref() == Some("hidden")
        || bool_arg(request, "hidden", false);
    let state = CapabilityStore::new(home.clone())
        .set_hidden_for(
            &id,
            hidden,
            &required_arg(request, "group_id")?,
            &required_arg(request, "actor_id")?,
        )
        .map_err(OpError::invalid)?;
    object(json!({"capability_id": id, "state": state}))
}
fn block(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    let scope = string_arg(request, "scope").unwrap_or_else(|| "group".into());
    let state = CapabilityStore::new(home.clone())
        .set_blocked_for(
            &id,
            bool_arg(request, "blocked", true),
            if scope == "global" { "" } else { &group_id },
            &string_arg(request, "reason").unwrap_or_default(),
            &string_arg(request, "by")
                .or_else(|| string_arg(request, "actor_id"))
                .unwrap_or_default(),
            request
                .args
                .get("ttl_seconds")
                .and_then(Value::as_i64)
                .unwrap_or(0),
        )
        .map_err(OpError::invalid)?;
    object(json!({"capability_id": id, "state": state}))
}
fn state(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    let actor_id = string_arg(request, "actor_id").unwrap_or_else(|| "user".into());
    let store = CapabilityStore::new(home.clone());
    let native = store.load().map_err(OpError::io)?;
    let effective =
        effective_state::load(home, &group_id, &actor_id, &native).map_err(OpError::io)?;
    let enabled = effective.enabled;
    let blocked = effective.blocked;
    let hidden = effective.hidden;
    let enabled_capabilities = enabled.difference(&blocked).cloned().collect::<Vec<_>>();
    let enabled_rows = enabled_capabilities
        .iter()
        .map(|id| json!({"capability_id":id,"scope":"group"}))
        .collect::<Vec<_>>();
    let active_capsule_skills = store
        .catalog()
        .map_err(OpError::io)?
        .into_iter()
        .filter(|capability| {
            enabled.contains(&capability.id)
                && !blocked.contains(&capability.id)
                && !hidden.contains(&capability.id)
                && (capability.id.starts_with("skill:") || !capability.capsule_text.is_empty())
        })
        .map(|capability| {
            let preview = capability
                .capsule_text
                .chars()
                .take(240)
                .collect::<String>();
            json!({
                "capability_id":capability.id,
                "name":capability.name,
                "description_short":capability.description,
                "capsule_preview":preview,
                "source_uri":capability.source_uri,
            })
        })
        .collect::<Vec<_>>();
    let catalog = store.catalog().map_err(OpError::io)?;
    let dynamic_tools =
        external_runtime::dynamic_tools(home, &group_id, &actor_id, &enabled_capabilities)?;
    let visible_tools = visible_tools(
        home,
        &group_id,
        &actor_id,
        &enabled_capabilities,
        &catalog,
        &dynamic_tools,
    )?;
    object(json!({
        "group_id":group_id,
        "actor_id":actor_id,
        "view":string_arg(request, "view").unwrap_or_default(),
        "enabled":enabled_rows,
        "enabled_capabilities":enabled_capabilities,
        "visible_tools":visible_tools,
        "dynamic_tools":dynamic_tools,
        "active_capsule_skills":active_capsule_skills,
        "actor_hidden_capabilities":hidden.into_iter().collect::<Vec<_>>(),
        "state":native,
    }))
}
fn import(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let raw = request
        .args
        .get("record")
        .or_else(|| request.args.get("capability"))
        .cloned()
        .ok_or_else(|| OpError::new("missing_argument", "missing capability record"))?;
    let store = CapabilityStore::new(home.clone());
    let capability = store.validate_record(&raw).map_err(OpError::invalid)?;
    if bool_arg(request, "dry_run", false) {
        return object(json!({
            "group_id":string_arg(request,"group_id").unwrap_or_default(),
            "actor_id":string_arg(request,"actor_id").or_else(||string_arg(request,"by")).unwrap_or_default(),
            "capability_id":capability.id,"kind":capability.kind,"dry_run":true,
            "imported":false,"record":raw,
            "would_enable":bool_arg(request,"enable_after_import",false),
            "refresh_required":false,"state":"validated"
        }));
    }
    let capability = store.import_record(raw.clone()).map_err(OpError::invalid)?;
    let enabled = if bool_arg(request, "enable_after_import", false) {
        Some(target_install::enable(
            home,
            &required_arg(request, "group_id")?,
            &string_arg(request, "actor_id").unwrap_or_else(|| "user".into()),
            &string_arg(request, "scope").unwrap_or_else(|| "actor".into()),
            request
                .args
                .get("ttl_seconds")
                .and_then(Value::as_i64)
                .unwrap_or(3600),
            &capability.id,
        )?)
    } else {
        None
    };
    object(json!({"capability": capability, "record":raw, "enable_result":enabled}))
}
fn source_delete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = required_arg(request, "source_id")?;
    let removed = CapabilityStore::new(home.clone())
        .delete_source(&source)
        .map_err(OpError::io)?;
    object(json!({"source_id":source,"removed":removed,"deleted":true}))
}
fn use_capability(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if let Some(tool_name) = string_arg(request, "tool_name").filter(|value| !value.is_empty()) {
        return external_runtime::call(home, request, &tool_name);
    }
    let id = required_arg(request, "capability_id")?;
    let capability = CapabilityStore::new(home.clone())
        .require(&id)
        .map_err(OpError::not_found)?;
    object(json!({"capability": capability, "input": request.args.get("input"), "ready": true}))
}

const CORE_BASIC_TOOLS: &[&str] = &[
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
const CAPABILITY_ADMIN_TOOLS: &[&str] = &[
    "cccc_capability_import",
    "cccc_capability_block",
    "cccc_capability_uninstall",
];
const VOICE_SECRETARY_TOOLS: &[&str] = &[
    "cccc_help",
    "cccc_bootstrap",
    "cccc_project_info",
    "cccc_inbox_list",
    "cccc_inbox_mark_read",
    "cccc_context_get",
    "cccc_agent_state",
    "cccc_voice_secretary_document",
    "cccc_voice_secretary_composer",
    "cccc_voice_secretary_request",
];
const WEB_MODEL_CORE_TOOLS: &[&str] = &[
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
    "cccc_project_info",
    "cccc_capability_state",
    "cccc_capability_enable",
    "cccc_capability_install",
    "cccc_tracked_send",
    "cccc_repo",
    "cccc_presentation",
    "cccc_memory",
    "cccc_runtime_wait_next_turn",
    "cccc_runtime_complete_turn",
    "cccc_code_exec",
    "cccc_code_wait",
    "cccc_repo_edit",
    "cccc_apply_patch",
    "cccc_shell",
    "cccc_exec_command",
    "cccc_write_stdin",
    "cccc_git",
];

fn visible_tools(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    enabled: &[String],
    catalog: &[Capability],
    dynamic: &[Value],
) -> Result<Vec<String>, OpError> {
    use std::collections::BTreeSet;
    let group = (!group_id.is_empty())
        .then(|| cccc_core::GroupStore::new(home.clone()))
        .transpose()
        .map_err(OpError::io)?
        .and_then(|store| store.load(group_id).ok());
    let actor = group
        .as_ref()
        .and_then(|group| group.actors.iter().find(|actor| actor.id == actor_id));
    let voice_secretary = actor_id == "voice-secretary"
        || actor.and_then(|actor| actor.internal_kind.as_deref()) == Some("voice_secretary");
    let web_model =
        actor.map(|actor| actor.runtime) == Some(cccc_contracts::ActorRuntime::WebModel);
    let peer = group.as_ref().is_some_and(|group| {
        cccc_core::actors::effective_role(group, actor_id) == Some(cccc_contracts::ActorRole::Peer)
    });
    let mut names = if voice_secretary {
        VOICE_SECRETARY_TOOLS
            .iter()
            .map(|value| (*value).to_owned())
            .collect()
    } else if web_model {
        WEB_MODEL_CORE_TOOLS
            .iter()
            .map(|value| (*value).to_owned())
            .collect()
    } else {
        CORE_BASIC_TOOLS
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<BTreeSet<_>>()
    };
    if !web_model {
        for capability in catalog.iter().filter(|item| enabled.contains(&item.id)) {
            names.extend(capability.tool_names.iter().cloned());
        }
    }
    names.extend(
        dynamic
            .iter()
            .filter_map(|item| item["name"].as_str().map(str::to_owned)),
    );
    if peer {
        for name in CAPABILITY_ADMIN_TOOLS {
            names.remove(*name);
        }
    }
    Ok(names.into_iter().collect())
}
