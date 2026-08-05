use cccc_contracts::{ActorRole, DaemonRequest, utc_now};
use cccc_core::HomeLayout;
use cccc_core::capabilities::CapabilityStore;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, bool_arg, object, string_arg};

const SOURCE_IDS: &[&str] = &[
    "manual_import",
    "agent_self_proposed",
    "github_import",
    "url_import",
    "local_import",
    "mcp_registry_official",
    "anthropic_skills",
    "github_skills_curated",
    "skillsmp_remote",
    "clawhub_remote",
    "openclaw_skills_remote",
    "clawskills_remote",
];
const SELF_PROPOSED_PREFIX: &str = "skill:agent_self_proposed:";
const SELF_PROPOSED_SECTIONS: &[&str] = &[
    "when to use",
    "avoid when",
    "procedure",
    "pitfalls",
    "verification",
];

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let context = super::actor_context(home, request)?;
    super::authorize_self(&context, "import capabilities")?;
    let scope = scope(request)?;
    let enable_after_import = bool_arg(request, "enable_after_import", false);
    if enable_after_import {
        super::authorize_scope_mutation(home, request, "session")?;
    }
    let raw = request
        .args
        .get("record")
        .or_else(|| request.args.get("capability"))
        .ok_or_else(|| OpError::new("missing_argument", "missing capability record"))?;
    let raw_has_updated_at_source = raw
        .get("updated_at_source")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let mut record = normalize_record(raw)?;
    let capability_id = text(&record, "capability_id");
    let kind = text(&record, "kind");
    let store = CapabilityStore::new(home.clone());
    store
        .validate_record(&record)
        .map_err(|error| OpError::new("capability_import_invalid", error.to_string()))?;
    let existing = store.catalog_record(&capability_id).map_err(OpError::io)?;
    if is_self_proposed_skill(&record) {
        let origin_group_id = existing
            .as_ref()
            .map(|value| text(value, "origin_group_id"))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| context.group_id.clone());
        record["origin_group_id"] = json!(origin_group_id);
    }

    let actor_role = if matches!(context.actor_id.as_str(), "user" | "system") {
        ""
    } else {
        match cccc_core::actors::effective_role(&context.group, &context.actor_id) {
            Some(ActorRole::Foreman) => "foreman",
            Some(ActorRole::Peer) => "peer",
            None => "",
        }
    };
    let policy_level = super::allowlist::effective_policy_level(
        home,
        &capability_id,
        &kind,
        &text(&record, "source_id"),
        actor_role,
    )?;
    let qualification = text(&record, "qualification_status");
    let enable_supported = record
        .get("enable_supported")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let enable_block_reason = if policy_level == "indexed" {
        "policy_level_indexed"
    } else if qualification == "blocked" {
        "qualification_blocked"
    } else if qualification == "unavailable" || !enable_supported {
        "capability_unavailable"
    } else {
        ""
    };
    let enableable_now = enable_block_reason.is_empty();
    let probe_enabled = bool_arg(request, "probe", true);
    let (probe, diagnostics) = probe_record(home, &record, probe_enabled);
    let already_active =
        enabled_for_actor(&store, &capability_id, &context.group_id, &context.actor_id)?;
    let readiness_preview =
        readiness_preview(enable_block_reason, &probe, already_active, enableable_now);
    let dry_run = bool_arg(request, "dry_run", false);
    let action_id = format!("cact_{}", &Uuid::new_v4().simple().to_string()[..16]);
    if dry_run {
        let preview_status = text(&readiness_preview, "preview_status");
        return object(json!({
            "action_id":action_id,
            "group_id":context.group_id,
            "actor_id":context.actor_id,
            "capability_id":capability_id,
            "kind":kind,
            "dry_run":true,
            "imported":false,
            "scope":scope,
            "already_active":already_active,
            "record":record,
            "probe":probe,
            "diagnostics":diagnostics,
            "would_enable":enable_after_import,
            "effective_policy_level":policy_level,
            "enableable_now":enableable_now,
            "enable_block_reason":enable_block_reason,
            "refresh_required":false,
            "state":if preview_status=="active"{"runnable"}else{&preview_status},
            "readiness_preview":readiness_preview,
        }));
    }

    if let Some(missing) = missing_self_proposed_sections_reason(&record) {
        let mut error = OpError::new(
            "capability_import_invalid",
            format!(
                "agent_self_proposed skill capsule_text is missing required sections: {}",
                missing.join(", ")
            ),
        );
        error.details.insert("action_id".into(), json!(action_id));
        error
            .details
            .insert("capability_id".into(), json!(capability_id));
        error.details.insert(
            "reason".into(),
            json!(format!(
                "missing_agent_self_proposed_sections:{}",
                missing.join(",")
            )),
        );
        error
            .details
            .insert("missing_sections".into(), json!(missing));
        error
            .details
            .insert("active_record_preserved".into(), json!(true));
        return Err(error);
    }

    let record_existed = existing.is_some();
    let record_unchanged = existing.as_ref().is_some_and(|value| {
        semantic_record(value, !raw_has_updated_at_source)
            == semantic_record(&record, !raw_has_updated_at_source)
    });
    let record_changed = record_existed && !record_unchanged;
    let import_action = if record_unchanged {
        "unchanged"
    } else if record_existed {
        "updated"
    } else {
        "created"
    };
    let stored_record = if record_unchanged {
        existing.expect("unchanged record exists")
    } else {
        store
            .import_record(record.clone())
            .map_err(|error| OpError::new("capability_import_invalid", error.to_string()))?;
        record.clone()
    };

    let ttl_seconds = request
        .args
        .get("ttl_seconds")
        .and_then(Value::as_i64)
        .unwrap_or(3600);
    let mut enable_result: Option<Value> = None;
    let mut refresh_required = false;
    let mut result_reason = String::new();
    let mut state = text(&readiness_preview, "preview_status");
    if state == "active" {
        state = "runnable".into();
    }
    if enable_after_import {
        if enableable_now {
            match super::target_install::enable(
                home,
                &context.group_id,
                &context.actor_id,
                &scope,
                ttl_seconds,
                &capability_id,
            ) {
                Ok(result) => {
                    refresh_required = result
                        .get("refresh_required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    state = match result.get("state").and_then(Value::as_str) {
                        Some("ready") => "activation_pending".into(),
                        Some(value) => value.to_owned(),
                        None => "activation_pending".into(),
                    };
                    enable_result = Some(Value::Object(result));
                }
                Err(error) => {
                    state = "blocked".into();
                    result_reason = error.message.clone();
                    enable_result = Some(json!({
                        "state":"failed",
                        "error":{"code":error.code,"message":error.message,"details":error.details}
                    }));
                }
            }
        } else {
            state = "blocked".into();
            result_reason = enable_block_reason.into();
        }
    }
    let active_after_import =
        enabled_for_actor(&store, &capability_id, &context.group_id, &context.actor_id)?
            && enableable_now
            && probe.get("state").and_then(Value::as_str) != Some("failed");
    if active_after_import && kind == "skill" {
        state = "runnable".into();
    } else if probe.get("state").and_then(Value::as_str) == Some("failed")
        && !matches!(state.as_str(), "blocked" | "needs_inspect")
    {
        state = "needs_inspect".into();
    }

    let mut result = json!({
        "action_id":action_id,
        "group_id":context.group_id,
        "actor_id":context.actor_id,
        "capability_id":capability_id,
        "kind":kind,
        "dry_run":false,
        "imported":true,
        "scope":scope,
        "import_action":import_action,
        "record_changed":record_changed,
        "already_active":already_active,
        "active_after_import":active_after_import,
        "record":stored_record,
        "probe":probe,
        "diagnostics":diagnostics,
        "effective_policy_level":policy_level,
        "enableable_now":enableable_now,
        "enable_block_reason":enable_block_reason,
        "enable_after_import":enable_after_import,
        "refresh_required":refresh_required,
        "state":state,
        "readiness_preview":readiness_preview,
    });
    if let Some(value) = enable_result {
        result["enable_result"] = value;
    }
    if !result_reason.is_empty() {
        result["reason"] = json!(result_reason);
    }
    object(result)
}

fn scope(request: &DaemonRequest) -> Result<String, OpError> {
    let scope = string_arg(request, "scope")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "session".into());
    if matches!(scope.as_str(), "group" | "actor" | "session") {
        Ok(scope)
    } else {
        Err(OpError::new(
            "invalid_scope",
            format!("invalid capability scope: {scope}"),
        ))
    }
}

fn normalize_record(raw: &Value) -> Result<Value, OpError> {
    let raw = raw
        .as_object()
        .ok_or_else(|| OpError::new("capability_import_invalid", "record must be an object"))?;
    let capability_id = map_text(raw, "capability_id");
    if capability_id.is_empty() {
        return Err(OpError::new(
            "capability_import_invalid",
            "record.capability_id is required",
        ));
    }
    let kind = match map_text(raw, "kind").to_ascii_lowercase().as_str() {
        "mcp" | "mcp_tool" | "mcp_toolpack" => "mcp_toolpack",
        "skill" => "skill",
        _ => {
            return Err(OpError::new(
                "capability_import_invalid",
                "record.kind must be mcp_toolpack or skill",
            ));
        }
    };
    if kind == "mcp_toolpack" && !capability_id.starts_with("mcp:") {
        return Err(OpError::new(
            "capability_import_invalid",
            "mcp capability_id must start with mcp:",
        ));
    }
    if kind == "skill" && !capability_id.starts_with("skill:") {
        return Err(OpError::new(
            "capability_import_invalid",
            "skill capability_id must start with skill:",
        ));
    }
    let raw_source = map_text(raw, "source_id");
    let source_id = if SOURCE_IDS.contains(&raw_source.as_str()) {
        raw_source
    } else {
        "manual_import".into()
    };
    if kind == "skill"
        && source_id == "agent_self_proposed"
        && !capability_id.starts_with(SELF_PROPOSED_PREFIX)
    {
        return Err(OpError::new(
            "capability_import_invalid",
            "agent_self_proposed skill capability_id must start with skill:agent_self_proposed:",
        ));
    }
    let now = utc_now();
    let name = nonempty(map_text(raw, "name"), display_name(&capability_id));
    let description = nonempty(
        map_text(raw, "description_short"),
        format!("Imported capability {capability_id}"),
    );
    let mut tags = string_list(raw.get("tags"), 64);
    for token in [
        "external",
        "imported",
        if kind == "skill" { "skill" } else { "mcp" },
    ] {
        push_unique(&mut tags, token, 64);
    }
    let mut qualification = map_text(raw, "qualification_status").to_ascii_lowercase();
    if !matches!(
        qualification.as_str(),
        "qualified" | "unavailable" | "blocked"
    ) {
        qualification.clear();
    }
    let mut reasons = string_list(raw.get("qualification_reasons"), 32);
    let mut record = Map::from_iter([
        ("capability_id".into(), json!(capability_id)),
        ("kind".into(), json!(kind)),
        ("name".into(), json!(name)),
        ("description_short".into(), json!(description)),
        ("tags".into(), json!(tags)),
        ("source_id".into(), json!(source_id)),
        (
            "source_tier".into(),
            json!(nonempty(map_text(raw, "source_tier"), "tier2".into())),
        ),
        ("source_uri".into(), json!(map_text(raw, "source_uri"))),
        (
            "source_record_id".into(),
            json!(nonempty(
                map_text(raw, "source_record_id"),
                capability_id.clone()
            )),
        ),
        (
            "source_record_version".into(),
            json!(map_text(raw, "source_record_version")),
        ),
        (
            "updated_at_source".into(),
            json!(nonempty(map_text(raw, "updated_at_source"), now.clone())),
        ),
        ("last_synced_at".into(), json!(now)),
        ("sync_state".into(), json!("imported")),
        ("requirements".into(), json!({})),
        ("license".into(), json!(map_text(raw, "license"))),
        (
            "trust_tier".into(),
            json!(nonempty(map_text(raw, "trust_tier"), "tier2".into())),
        ),
        ("health_status".into(), json!("imported")),
    ]);
    copy_recommendation_fields(raw, &mut record);

    if kind == "mcp_toolpack" {
        let install_mode = map_text(raw, "install_mode").to_ascii_lowercase();
        if !matches!(install_mode.as_str(), "remote_only" | "package" | "command") {
            return Err(OpError::new(
                "capability_import_invalid",
                "mcp import requires record.install_mode in {remote_only, package, command}",
            ));
        }
        let mut install_spec = raw
            .get("install_spec")
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| {
                OpError::new(
                    "capability_import_invalid",
                    "mcp import requires record.install_spec object",
                )
            })?;
        for key in [
            "command",
            "command_candidates",
            "fallback_command",
            "fallback_command_candidates",
        ] {
            if !install_spec.contains_key(key)
                && let Some(value) = raw.get(key)
            {
                install_spec.insert(key.into(), value.clone());
            }
        }
        if install_mode == "command"
            && !has_command(install_spec.get("command"))
            && !has_command_candidates(install_spec.get("command_candidates"))
        {
            return Err(OpError::new(
                "capability_import_invalid",
                "command install_mode requires install_spec.command or install_spec.command_candidates",
            ));
        }
        let unsupported_reason = unsupported_mcp_reason(&install_mode, &install_spec);
        if qualification.is_empty() {
            qualification = if unsupported_reason.is_empty() {
                "qualified".into()
            } else {
                "unavailable".into()
            };
        }
        if !unsupported_reason.is_empty() {
            push_unique(&mut reasons, &unsupported_reason, 32);
        }
        let enable_supported = qualification != "blocked" && unsupported_reason.is_empty();
        record.insert("install_mode".into(), json!(install_mode));
        record.insert("install_spec".into(), Value::Object(install_spec));
        record.insert("qualification_status".into(), json!(qualification));
        record.insert("qualification_reasons".into(), json!(reasons));
        record.insert("enable_supported".into(), json!(enable_supported));
        return Ok(Value::Object(record));
    }

    let capsule_text = map_text(raw, "capsule_text");
    if capsule_text.is_empty() {
        return Err(OpError::new(
            "capability_import_invalid",
            "skill import requires record.capsule_text",
        ));
    }
    if source_id == "agent_self_proposed" {
        let missing = missing_sections(&capsule_text);
        if !missing.is_empty() {
            qualification = "blocked".into();
            push_unique(
                &mut reasons,
                &format!("missing_agent_self_proposed_sections:{}", missing.join(",")),
                32,
            );
        }
    }
    if qualification.is_empty() {
        qualification = "qualified".into();
    }
    record.insert("install_mode".into(), json!("builtin"));
    record.insert("install_spec".into(), json!({}));
    record.insert("qualification_status".into(), json!(qualification));
    record.insert("qualification_reasons".into(), json!(reasons));
    record.insert(
        "enable_supported".into(),
        json!(record["qualification_status"] != "blocked"),
    );
    record.insert(
        "capsule_text".into(),
        json!(capsule_text.chars().take(2400).collect::<String>()),
    );
    record.insert(
        "requires_capabilities".into(),
        json!(string_list(raw.get("requires_capabilities"), 32)),
    );
    Ok(Value::Object(record))
}

fn probe_record(home: &HomeLayout, record: &Value, enabled: bool) -> (Value, Vec<Value>) {
    if !enabled {
        return (json!({"state":"skipped"}), Vec::new());
    }
    let kind = text(record, "kind");
    if kind == "skill" {
        return (
            json!({
                "state":"runnable",
                "kind":"skill",
                "capsule_present":!text(record,"capsule_text").is_empty()
            }),
            Vec::new(),
        );
    }
    let capability_id = text(record, "capability_id");
    match super::package_install::ensure_installed(home, &capability_id, record) {
        Ok(artifact) => {
            let tools = artifact
                .as_ref()
                .and_then(|value| value.get("tools"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let names = tools
                .iter()
                .filter_map(|value| {
                    value
                        .get("real_tool_name")
                        .or_else(|| value.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect::<Vec<_>>();
            (
                json!({
                    "state":"runnable","kind":"mcp_toolpack",
                    "install_state":"installed",
                    "tool_count":names.len(),"tool_names":names
                }),
                Vec::new(),
            )
        }
        Err(error) => {
            let code = error.code;
            let message = error.message;
            (
                json!({
                    "state":"failed","kind":"mcp_toolpack",
                    "reason":format!("probe_failed:{code}"),
                    "install_error_code":code,"install_error":message,"retryable":false
                }),
                vec![json!({
                    "code":code,"message":message,"retryable":false,
                    "action_hints":["inspect_install_spec_and_runtime_requirements"]
                })],
            )
        }
    }
}

fn readiness_preview(
    enable_block_reason: &str,
    probe: &Value,
    already_active: bool,
    enableable_now: bool,
) -> Value {
    let probe_failed = probe.get("state").and_then(Value::as_str) == Some("failed");
    let preview_status = if already_active && enableable_now && !probe_failed {
        "active"
    } else if !enable_block_reason.is_empty() {
        "blocked"
    } else if probe_failed {
        "needs_inspect"
    } else {
        "enableable"
    };
    let next_step = match preview_status {
        "active" => "use_capability",
        "blocked" => "inspect_qualification_or_policy",
        "needs_inspect" => "inspect_probe_diagnostics",
        _ => "enable_capability_at_the_narrowest_required_scope",
    };
    json!({
        "preview_status":preview_status,
        "next_step":next_step,
        "already_active":already_active,
        "preview_basis":["qualification","policy","probe","binding_state"],
        "enable_block_reason":enable_block_reason,
    })
}

fn enabled_for_actor(
    store: &CapabilityStore,
    capability_id: &str,
    group_id: &str,
    actor_id: &str,
) -> Result<bool, OpError> {
    for scope in ["group", "actor", "session"] {
        if store
            .is_enabled_for(capability_id, group_id, actor_id, scope)
            .map_err(OpError::io)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn semantic_record(record: &Value, ignore_updated_at_source: bool) -> Value {
    let mut record = record.clone();
    if let Some(record) = record.as_object_mut() {
        record.remove("last_synced_at");
        if ignore_updated_at_source {
            record.remove("updated_at_source");
        }
    }
    record
}

fn is_self_proposed_skill(record: &Value) -> bool {
    text(record, "kind") == "skill" && text(record, "source_id") == "agent_self_proposed"
}

fn missing_self_proposed_sections_reason(record: &Value) -> Option<Vec<String>> {
    if !is_self_proposed_skill(record) {
        return None;
    }
    let missing = missing_sections(&text(record, "capsule_text"));
    (!missing.is_empty()).then_some(missing)
}

fn missing_sections(capsule_text: &str) -> Vec<String> {
    let body = capsule_text.to_ascii_lowercase();
    SELF_PROPOSED_SECTIONS
        .iter()
        .filter(|section| !body.contains(**section))
        .map(|section| (*section).to_owned())
        .collect()
}

fn unsupported_mcp_reason(mode: &str, spec: &Map<String, Value>) -> String {
    match mode {
        "remote_only" => {
            let transport = map_text(spec, "transport").to_ascii_lowercase();
            if !matches!(transport.as_str(), "" | "streamable-http" | "http" | "sse") {
                return format!("unsupported_remote_transport:{transport}");
            }
            let url = map_text(spec, "url");
            if url.starts_with("http://") || url.starts_with("https://") {
                String::new()
            } else {
                "missing_remote_url".into()
            }
        }
        "package" => {
            let registry = map_text(spec, "registry_type").to_ascii_lowercase();
            let identifier = map_text(spec, "identifier");
            let supported_registry = matches!(
                registry.as_str(),
                "npm"
                    | "javascript"
                    | "node"
                    | "nodejs"
                    | "pypi"
                    | "python"
                    | "pip"
                    | "pipx"
                    | "uvx"
                    | "oci"
                    | "docker"
                    | "container"
                    | "podman"
            );
            if supported_registry && !identifier.is_empty()
                || has_command(spec.get("fallback_command"))
                || has_command_candidates(spec.get("fallback_command_candidates"))
            {
                String::new()
            } else if !supported_registry {
                "unsupported_registry_type".into()
            } else {
                "missing_package_identifier".into()
            }
        }
        "command" => {
            if has_command(spec.get("command"))
                || has_command_candidates(spec.get("command_candidates"))
            {
                String::new()
            } else {
                "missing_command_candidate".into()
            }
        }
        _ => format!("unsupported_install_mode:{mode}"),
    }
}

fn has_command(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(value)) => !value.trim().is_empty(),
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|value| !value.trim().is_empty())),
        _ => false,
    }
}

fn has_command_candidates(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|value| has_command(Some(value)))
}

fn copy_recommendation_fields(raw: &Map<String, Value>, record: &mut Map<String, Value>) {
    for field in ["use_when", "avoid_when", "gotchas"] {
        let values = recommendation_lines(raw.get(field));
        if !values.is_empty() {
            record.insert(field.into(), json!(values));
        }
    }
    let evidence_kind = compact_text(&map_text(raw, "evidence_kind"), 180);
    if !evidence_kind.is_empty() {
        record.insert("evidence_kind".into(), json!(evidence_kind));
    }
}

fn recommendation_lines(value: Option<&Value>) -> Vec<String> {
    let values = match value {
        Some(Value::Array(values)) => values.clone(),
        Some(value) => vec![value.clone()],
        None => Vec::new(),
    };
    let mut output = Vec::new();
    for value in values {
        let Some(value) = value.as_str() else {
            continue;
        };
        let value = compact_text(value, 180);
        if !value.is_empty()
            && !output
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&value))
        {
            output.push(value);
        }
        if output.len() == 4 {
            break;
        }
    }
    output
}

fn compact_text(value: &str, max_chars: usize) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() <= max_chars {
        return value;
    }
    let mut value = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    value = value.trim_end().to_owned();
    value.push_str("...");
    value
}

fn string_list(value: Option<&Value>, max_items: usize) -> Vec<String> {
    let mut output = Vec::new();
    for value in value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        push_unique(&mut output, value.trim(), max_items);
    }
    output
}

fn push_unique(values: &mut Vec<String>, value: &str, max_items: usize) {
    if !value.is_empty() && values.len() < max_items && !values.iter().any(|item| item == value) {
        values.push(value.to_owned());
    }
}

fn display_name(capability_id: &str) -> String {
    let token = if capability_id.starts_with("skill:") {
        capability_id.rsplit(':').next().unwrap_or(capability_id)
    } else if let Some(token) = capability_id.strip_prefix("mcp:") {
        token.rsplit('/').next().unwrap_or(token)
    } else {
        capability_id
    };
    let token = token.replace(['_', '-'], " ").trim().to_owned();
    nonempty(token, capability_id.to_owned())
}

fn nonempty(value: String, fallback: String) -> String {
    if value.is_empty() { fallback } else { value }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned()
}

fn map_text(value: &Map<String, Value>, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_owned()
}
