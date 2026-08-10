use cccc_contracts::{Actor, ActorRole, DaemonRequest};
use cccc_core::capabilities::{Capability, CapabilityStore};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Value, json};
use std::collections::BTreeSet;

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};

mod allowlist;
mod effective_state;
mod external_runtime;
mod import;
mod overview;
mod package_install;
mod target_install;
mod uninstall;

const FOREMAN_STARTUP_CAPABILITIES: &[&str] = &["pack:group-runtime", "pack:diagnostics"];

#[derive(Debug)]
pub(super) struct ActorContext {
    pub group_id: String,
    pub actor_id: String,
    pub by: String,
    pub group: GroupDoc,
}

#[derive(Debug)]
pub(super) struct ScopeMutation {
    pub actor: ActorContext,
    pub scope: String,
}

pub(super) fn actor_context(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<ActorContext, OpError> {
    let group_id = required_arg(request, "group_id")?;
    let group = cccc_core::GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    let by = string_arg(request, "by")
        .or_else(|| string_arg(request, "actor_id"))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "user".into());
    let actor_id = string_arg(request, "actor_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| by.clone());
    if !matches!(by.as_str(), "user" | "system")
        && cccc_core::actors::effective_role(&group, &by).is_none()
    {
        return Err(OpError::new(
            "actor_not_found",
            format!("actor not found in group: {by}"),
        ));
    }
    Ok(ActorContext {
        group_id,
        actor_id,
        by,
        group,
    })
}

pub(super) fn authorize_self(context: &ActorContext, action: &str) -> Result<(), OpError> {
    if matches!(context.by.as_str(), "user" | "system") || context.actor_id == context.by {
        return Ok(());
    }
    Err(OpError::new(
        "permission_denied",
        format!("actor can only {action} as self"),
    ))
}

pub(super) fn authorize_group_admin(context: &ActorContext, action: &str) -> Result<(), OpError> {
    if matches!(context.by.as_str(), "user" | "system")
        || cccc_core::actors::effective_role(&context.group, &context.by)
            == Some(ActorRole::Foreman)
    {
        return Ok(());
    }
    Err(OpError::new(
        "permission_denied",
        format!("only user or foreman can {action}"),
    ))
}

pub(super) fn authorize_scope_mutation(
    home: &HomeLayout,
    request: &DaemonRequest,
    default_scope: &str,
) -> Result<ScopeMutation, OpError> {
    let actor = actor_context(home, request)?;
    let scope = string_arg(request, "scope")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_scope.to_owned());
    if !matches!(scope.as_str(), "group" | "actor" | "session") {
        return Err(OpError::new(
            "invalid_scope",
            format!("invalid capability scope: {scope}"),
        ));
    }
    if !matches!(actor.by.as_str(), "user" | "system") {
        if scope == "group" {
            if cccc_core::actors::effective_role(&actor.group, &actor.by)
                != Some(ActorRole::Foreman)
            {
                return Err(OpError::new(
                    "permission_denied",
                    "only foreman can mutate group capability scope",
                ));
            }
        } else {
            authorize_self(&actor, "mutate actor/session capability scope")?;
        }
    }
    Ok(ScopeMutation { actor, scope })
}

pub(super) fn apply_actor_startup_baseline(home: &HomeLayout, group: &GroupDoc, actor: &Actor) {
    if cccc_core::actors::effective_role(group, &actor.id) == Some(ActorRole::Foreman) {
        enable_startup_capabilities(
            home,
            &group.group_id,
            &actor.id,
            FOREMAN_STARTUP_CAPABILITIES.iter().copied(),
            "actor",
            3600,
            "foreman role default",
        );
    }
    match super::actor_profile_runtime::capability_defaults(home, actor) {
        Ok(Some(defaults)) => enable_startup_capabilities(
            home,
            &group.group_id,
            &actor.id,
            defaults.autoload_capabilities.iter().map(String::as_str),
            &defaults.default_scope,
            defaults.session_ttl_seconds,
            "actor profile default",
        ),
        Ok(None) => {}
        Err(error) => tracing::warn!(
            group_id = %group.group_id,
            actor_id = %actor.id,
            message = %error.message,
            "failed to resolve actor profile capability defaults"
        ),
    }
    enable_startup_capabilities(
        home,
        &group.group_id,
        &actor.id,
        actor.capability_autoload.iter().map(String::as_str),
        "actor",
        3600,
        "actor autoload",
    );
}

fn enable_startup_capabilities<'a>(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    capability_ids: impl IntoIterator<Item = &'a str>,
    scope: &str,
    ttl_seconds: i64,
    source: &str,
) {
    for capability_id in capability_ids
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Err(error) =
            target_install::enable(home, group_id, actor_id, scope, ttl_seconds, capability_id)
        {
            tracing::warn!(
                %group_id,
                %actor_id,
                %capability_id,
                %source,
                message = %error.message,
                "failed to apply startup capability"
            );
        }
    }
}

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "capability_overview" => overview::run(home, request),
        "capability_search" => search(home, request),
        "capability_enable" => enable(home, request),
        "capability_visibility" => visibility(home, request),
        "capability_block" => block(home, request),
        "capability_state" => state(home, request),
        "capability_import" => import::run(home, request),
        "capability_uninstall" => uninstall::run(home, request),
        "capability_install_target" => target_install::run(home, request),
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
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    let store = CapabilityStore::new(home.clone());
    let removed = store.removed_for_group(&group_id).map_err(OpError::io)?;
    let capabilities = store
        .search(&query)
        .map_err(OpError::io)?
        .into_iter()
        .filter(|capability| !removed.contains(&capability.id))
        .collect::<Vec<_>>();
    object(json!({"capabilities":capabilities}))
}
fn enable(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let enabled = bool_arg(request, "enabled", true);
    let access = authorize_scope_mutation(home, request, "session")?;
    let store = CapabilityStore::new(home.clone());
    if enabled {
        return target_install::enable(
            home,
            &access.actor.group_id,
            &access.actor.actor_id,
            &access.scope,
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
            &access.actor.group_id,
            &access.actor.actor_id,
            &access.scope,
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
    let access = actor_context(home, request)?;
    authorize_self(&access, "change capability visibility")?;
    let state = CapabilityStore::new(home.clone())
        .set_hidden_for(&id, hidden, &access.group_id, &access.actor_id)
        .map_err(OpError::invalid)?;
    object(json!({"capability_id": id, "state": state}))
}
fn block(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let id = required_arg(request, "capability_id")?;
    let access = actor_context(home, request)?;
    authorize_self(&access, "mutate capability block state")?;
    let scope = string_arg(request, "scope")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "group".into());
    if !matches!(scope.as_str(), "group" | "global") {
        return Err(OpError::new(
            "invalid_scope",
            format!("invalid capability block scope: {scope}"),
        ));
    }
    if scope == "global" && !matches!(access.by.as_str(), "user" | "system") {
        return Err(OpError::new(
            "permission_denied",
            "only user can mutate global capability block state",
        ));
    }
    if scope == "group" {
        authorize_group_admin(&access, "mutate group capability block state")?;
    }
    let state = CapabilityStore::new(home.clone())
        .set_blocked_for(
            &id,
            bool_arg(request, "blocked", true),
            if scope == "global" {
                ""
            } else {
                &access.group_id
            },
            &string_arg(request, "reason").unwrap_or_default(),
            &access.by,
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
    let mut hidden = effective.hidden;
    let actor = (!group_id.is_empty())
        .then(|| cccc_core::GroupStore::new(home.clone()))
        .transpose()
        .map_err(OpError::io)?
        .and_then(|store| store.load(&group_id).ok())
        .and_then(|group| group.actors.into_iter().find(|actor| actor.id == actor_id));
    let actor_autoload_capabilities = actor
        .as_ref()
        .map(|actor| normalized_ids(&actor.capability_autoload))
        .unwrap_or_default();
    let actor_configured_hidden = actor
        .as_ref()
        .map(|actor| normalized_ids(&actor.capability_hidden))
        .unwrap_or_default();
    hidden.extend(actor_configured_hidden);
    let profile_autoload_capabilities = match actor.as_ref() {
        Some(actor) => super::actor_profile_runtime::capability_defaults(home, actor)
            .ok()
            .flatten()
            .map(|defaults| defaults.autoload_capabilities)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let autoload_capabilities = normalized_ids(
        &profile_autoload_capabilities
            .iter()
            .chain(&actor_autoload_capabilities)
            .cloned()
            .collect::<Vec<_>>(),
    );
    let enabled_capabilities = enabled.difference(&blocked).cloned().collect::<Vec<_>>();
    let enabled_rows = enabled_capabilities
        .iter()
        .map(|id| json!({"capability_id":id,"scope":"group"}))
        .collect::<Vec<_>>();
    let catalog = store.catalog().map_err(OpError::io)?;
    let active_capsule_skills = catalog
        .iter()
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
    let autoload_skills = catalog
        .iter()
        .filter(|capability| {
            autoload_capabilities.contains(&capability.id)
                && capability.kind == "skill"
                && !blocked.contains(&capability.id)
                && !hidden.contains(&capability.id)
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
                "capsule_text":capability.capsule_text,
                "source_id":capability.source,
            })
        })
        .collect::<Vec<_>>();
    let hidden_capabilities = catalog
        .iter()
        .filter(|capability| {
            enabled.contains(&capability.id)
                && !blocked.contains(&capability.id)
                && hidden.contains(&capability.id)
                && (capability.id.starts_with("skill:") || !capability.capsule_text.is_empty())
        })
        .map(|capability| {
            json!({
                "capability_id":capability.id,
                "reason":"actor_hidden",
                "name":capability.name,
                "description_short":capability.description,
                "kind":capability.kind,
                "source_id":capability.source,
            })
        })
        .collect::<Vec<_>>();
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
        "autoload_skills":autoload_skills,
        "autoload_capabilities":autoload_capabilities,
        "actor_autoload_capabilities":actor_autoload_capabilities,
        "profile_autoload_capabilities":profile_autoload_capabilities,
        "actor_hidden_capabilities":hidden.into_iter().collect::<Vec<_>>(),
        "hidden_capabilities":hidden_capabilities,
        "state":native,
    }))
}

fn normalized_ids(values: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert((*value).to_owned()))
        .map(str::to_owned)
        .collect()
}
fn source_delete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = required_arg(request, "source_id")?;
    let actor = actor_context(home, request)?;
    authorize_group_admin(&actor, "delete capability sources")?;
    if !matches!(
        source.as_str(),
        "manual_import" | "agent_self_proposed" | "github_import" | "url_import" | "local_import"
    ) {
        return Err(OpError::new(
            "protected_capability_source",
            "this capability source cannot be deleted; disable it instead",
        ));
    }
    let removed = CapabilityStore::new(home.clone())
        .delete_source(&source)
        .map_err(OpError::io)?;
    let cleanup = uninstall::cleanup_global_references(home, &removed)?;
    object(json!({
        "group_id":actor.group_id,
        "actor_id":actor.actor_id,
        "source_id":source,
        "removed_records":removed.len(),
        "removed_capability_ids":removed,
        "removed_runtime_bindings":cleanup.removed_runtime_bindings,
        "removed_installations":cleanup.removed_installations,
        "removed_recent_success":cleanup.removed_recent_success,
        "removed_actor_autoload":cleanup.removed_actor_autoload,
        "removed_profile_autoload":cleanup.removed_profile_autoload,
        "deleted":true
    }))
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
