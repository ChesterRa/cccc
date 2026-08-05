use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::fs::read_json;
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

const STANDUP_SNIPPET: &str = "{{interval_minutes}} minutes have passed. Stand-up checkpoint (foreman only).\n\nUse MCP chat for any visible update. Keep this short.";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_automation_update" => update(home, request),
        "group_automation_state" => state(home, request),
        "group_automation_manage" => manage(home, request),
        "group_automation_reset_baseline" => reset(home, request),
        _ => return None,
    })
}

fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    let patch = request
        .args
        .get("patch")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| OpError::new("invalid_args", "patch must be an object"))?;
    let expected = patch.get("expected_version").and_then(Value::as_u64);
    let current = group
        .automation
        .get("version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if expected.is_some_and(|expected| expected != current) {
        return Err(OpError::new(
            "version_conflict",
            format!("automation version changed: expected {expected:?}, current {current}"),
        ));
    }
    let updated = store(home)?
        .mutate(&group.group_id, |doc| {
            if let Some(rules) = patch.get("rules").and_then(Value::as_array) {
                doc.automation
                    .insert("rules".into(), Value::Array(rules.clone()));
            }
            if let Some(snippets) = patch.get("snippets").and_then(Value::as_object) {
                let mut custom = snippets.clone();
                let mut overrides = Map::new();
                if let Some(standup) = custom.remove("standup")
                    && standup.as_str() != Some(STANDUP_SNIPPET)
                {
                    overrides.insert("standup".into(), standup);
                }
                doc.automation
                    .insert("snippets".into(), Value::Object(custom));
                doc.automation
                    .insert("snippet_overrides".into(), Value::Object(overrides));
            }
            doc.automation.insert("version".into(), json!(current + 1));
            Ok(doc.clone())
        })
        .map_err(OpError::io)?;
    append_event(home, &group.group_id, request, "group.automation_update")?;
    object(payload(home, &updated))
}

fn state(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    object(payload(home, &load(home, request)?))
}

fn manage(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    let outcome = super::automation_manage::apply(&group, request)?;
    let updated = if outcome.changed {
        let current = group
            .automation
            .get("version")
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let rules = outcome.rules.clone();
        let snippets = outcome.snippets.clone();
        let updated = store(home)?
            .mutate(&group.group_id, |doc| {
                doc.automation.insert("rules".into(), Value::Array(rules));
                doc.automation
                    .insert("snippets".into(), Value::Object(snippets));
                doc.automation.insert("version".into(), json!(current + 1));
                Ok(doc.clone())
            })
            .map_err(OpError::io)?;
        append_event(home, &group.group_id, request, "group.automation_update")?;
        updated
    } else {
        group
    };
    let mut result = payload(home, &updated);
    result["applied_actions"] = Value::Array(outcome.applied_actions);
    result["changed"] = Value::Bool(outcome.changed);
    object(result)
}

fn reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    authorize(&group, request)?;
    let current = group
        .automation
        .get("version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if let Some(expected) = request.args.get("expected_version").and_then(Value::as_u64)
        && expected != current
    {
        return Err(OpError::new(
            "version_conflict",
            format!("automation version mismatch: expected {expected}, current {current}"),
        ));
    }
    let updated = store(home)?
        .mutate(&group.group_id, |doc| {
            doc.automation = json!({
                "version":current + 1,
                "rules":default_rules(),
                "snippets":{},
                "snippet_overrides":{}
            })
            .as_object()
            .cloned()
            .unwrap_or_default();
            Ok(doc.clone())
        })
        .map_err(OpError::io)?;
    append_event(home, &group.group_id, request, "group.automation_update")?;
    object(payload(home, &updated))
}

fn payload(home: &HomeLayout, group: &GroupDoc) -> Value {
    let rules = group
        .automation
        .get("rules")
        .cloned()
        .unwrap_or_else(default_rules);
    let custom = group
        .automation
        .get("snippets")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let overrides = group
        .automation
        .get("snippet_overrides")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut effective_snippets = custom.as_object().cloned().unwrap_or_default();
    if let Some(items) = overrides.as_object() {
        effective_snippets.extend(items.clone());
    }
    let runtime_path = store(home)
        .ok()
        .and_then(|store| store.state_dir(&group.group_id).ok())
        .map(|path| path.join("automation.json"));
    let runtime: Value = runtime_path
        .as_deref()
        .filter(|path| path.exists())
        .and_then(|path| read_json(path).ok())
        .unwrap_or_else(|| json!({}));
    let status = runtime
        .get("rules")
        .and_then(Value::as_object)
        .map(|items| {
            items
                .iter()
                .map(|(id, entry)| {
                    let at = entry
                        .get("last_fired_at")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    (id.clone(), json!({"last_fired_at":at}))
                })
                .collect::<Map<_, _>>()
        })
        .unwrap_or_default();
    json!({
        "ruleset":{"rules":rules,"snippets":effective_snippets},
        "snippet_catalog":{"built_in":{"standup":STANDUP_SNIPPET},"built_in_overrides":overrides,"custom":custom},
        "status":status,
        "config_path":format!("groups/{}/group.yaml",group.group_id),
        "supported_vars":["group_id","group_title","actor_id","now"],
        "version":group.automation.get("version").and_then(Value::as_u64).unwrap_or(1),
        "server_now":utc_now(),
    })
}

fn default_rules() -> Value {
    json!([{
        "id":"standup","enabled":false,"scope":"group","owner_actor_id":null,
        "to":["@foreman"],"trigger":{"kind":"interval","every_seconds":900},
        "action":{"kind":"notify","priority":"normal","requires_ack":false,
            "title":"Stand-up reminder","snippet_ref":"standup","message":""}
    }])
}

fn load(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)
}

fn authorize(group: &GroupDoc, request: &DaemonRequest) -> Result<(), OpError> {
    permissions::require_group(
        group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )
    .map_err(OpError::invalid)
}

fn append_event(
    home: &HomeLayout,
    group_id: &str,
    request: &DaemonRequest,
    kind: &str,
) -> Result<(), OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    cccc_core::ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)
}
