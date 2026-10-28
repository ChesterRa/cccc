use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use cccc_contracts::{ActorRole, DaemonRequest, Event, utc_now};
use cccc_core::automation::STANDUP_SNIPPET;
use cccc_core::fs::read_json;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};

use super::automation_manage::{Ruleset, split_snippets};
use super::automation_rule_access::{
    authorize_change, expected_version, validate as validate_rule,
};
use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "group_automation_update" => Operation::new(Write, update),
        "group_automation_state" => Operation::new(Read, state),
        "group_automation_manage" => Operation::new(Write, manage),
        "group_automation_reset_baseline" => Operation::new(Write, reset),
        _ => return None,
    })
}

fn update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let by = caller(request);
    let role = caller_role(&group, &by)?;
    let ruleset = request
        .args
        .get("ruleset")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| OpError::new("invalid_args", "ruleset must be an object"))?;
    let stored = group.automation.get("rules").and_then(Value::as_array);
    let (rules, snippets) = normalize_ruleset(&ruleset, &by, role, stored)?;
    let expected = expected_version(&request.args)?;
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
    let (custom, overrides) = split_snippets(snippets);
    if let Some(role) = role {
        let saved = saved_ruleset(&group)?;
        authorize_change(
            &saved.rules,
            &rules,
            custom != saved.snippets || overrides != saved.overrides,
            &by,
            role,
        )?;
    }
    let updated = mutate_automation(home, &group.group_id, |doc| {
        doc.automation.insert("rules".into(), Value::Array(rules));
        doc.automation
            .insert("snippets".into(), Value::Object(custom));
        doc.automation
            .insert("snippet_overrides".into(), Value::Object(overrides));
        doc.automation.insert("version".into(), json!(current + 1));
    })?;
    let event = append_event(home, &group.group_id, request, "group.automation_update")?;
    let mut result = payload(home, &updated, &caller(request))?;
    result["event"] = json!(event);
    object(result)
}

fn state(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    object(payload(home, &load(home, request)?, &caller(request))?)
}

fn manage(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let by = caller(request);
    let role = caller_role(&group, &by)?;
    let saved = saved_ruleset(&group)?;
    let outcome = super::automation_manage::apply(&group, request, role, &saved)?;
    if let Some(role) = role {
        let after = &outcome.ruleset;
        authorize_change(
            &saved.rules,
            &after.rules,
            after.snippets != saved.snippets || after.overrides != saved.overrides,
            &by,
            role,
        )?;
    }
    let updated = if outcome.changed {
        let current = group
            .automation
            .get("version")
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let Ruleset {
            rules,
            snippets,
            overrides,
        } = outcome.ruleset.clone();
        let updated = mutate_automation(home, &group.group_id, |doc| {
            doc.automation.insert("rules".into(), Value::Array(rules));
            doc.automation
                .insert("snippets".into(), Value::Object(snippets));
            doc.automation
                .insert("snippet_overrides".into(), Value::Object(overrides));
            doc.automation.insert("version".into(), json!(current + 1));
        })?;
        let event = append_event(home, &group.group_id, request, "group.automation_update")?;
        (updated, Some(event))
    } else {
        (group, None)
    };
    let (updated, event) = updated;
    let mut result = payload(home, &updated, &caller(request))?;
    result["applied_actions"] = Value::Array(outcome.applied_actions);
    result["changed"] = Value::Bool(outcome.changed);
    result["event"] = event.map_or(Value::Null, |event| json!(event));
    object(result)
}

fn reset(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let by = caller(request);
    let role = caller_role(&group, &by)?;
    let current = group
        .automation
        .get("version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if let Some(expected) = expected_version(&request.args)?
        && expected != current
    {
        return Err(OpError::new(
            "version_conflict",
            format!("automation version mismatch: expected {expected}, current {current}"),
        ));
    }
    if let Some(role) = role {
        let saved = saved_ruleset(&group)?;
        authorize_change(
            &saved.rules,
            &baseline_rules(),
            !saved.snippets.is_empty() || !saved.overrides.is_empty(),
            &by,
            role,
        )?;
    }
    let updated = mutate_automation(home, &group.group_id, |doc| {
        doc.automation = json!({
            "version":current + 1,
            "rules":baseline_rules(),
            "snippets":{},
            "snippet_overrides":{}
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
    })?;
    let event = append_event(home, &group.group_id, request, "group.automation_update")?;
    let mut result = payload(home, &updated, &caller(request))?;
    result["event"] = json!(event);
    object(result)
}

fn payload(home: &HomeLayout, group: &GroupDoc, by: &str) -> Result<Value, OpError> {
    let peer = caller_role(group, by)? == Some(ActorRole::Peer);
    // A malformed saved list reads as the baseline, so the user can still open
    // and replace it; writes that start from the saved list report it instead.
    let mut rules = group
        .automation
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(baseline_rules);
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
    effective_snippets
        .entry("standup")
        .or_insert_with(|| json!(STANDUP_SNIPPET));
    let runtime_path = store(home)
        .ok()
        .and_then(|store| store.state_dir(&group.group_id).ok())
        .map(|path| path.join("automation.json"));
    let runtime: Value = runtime_path
        .as_deref()
        .filter(|path| path.exists())
        .and_then(|path| read_json(path).ok())
        .unwrap_or_else(|| json!({}));
    let runtime_rules = runtime
        .get("rules")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let now = chrono::Utc::now();
    let mut status = rules
        .iter()
        .filter_map(|rule| {
            let id = rule
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())?;
            let entry = runtime_rules.get(id).and_then(Value::as_object);
            let last_fired_at = entry
                .and_then(|entry| entry.get("last_fired_at"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let last_fired = chrono::DateTime::parse_from_rfc3339(last_fired_at)
                .ok()
                .map(|value| value.timestamp());
            let trigger = rule.get("trigger").and_then(Value::as_object);
            let trigger_kind = trigger
                .and_then(|trigger| trigger.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("interval");
            let enabled = rule.get("enabled").and_then(Value::as_bool) != Some(false);
            let next_fire_at = enabled
                .then(|| cccc_core::automation::next_rule_fire_at(trigger, last_fired, now))
                .flatten()
                .map(|value| value.to_rfc3339())
                .unwrap_or_default();
            let completed = trigger_kind == "at"
                && (last_fired.is_some()
                    || entry
                        .and_then(|entry| entry.get("at_fired"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false));
            Some((
                id.to_owned(),
                json!({
                    "last_fired_at":last_fired_at,
                    "last_error_at":entry.and_then(|entry|entry.get("last_error_at")).and_then(Value::as_str).unwrap_or_default(),
                    "last_error":entry.and_then(|entry|entry.get("last_error")).and_then(Value::as_str).unwrap_or_default(),
                    "next_fire_at":next_fire_at,
                    "completed":completed,
                    "completed_at":if completed { last_fired_at } else { "" },
                }),
            ))
        })
        .collect::<Map<_, _>>();
    let mut built_in = Map::from_iter([("standup".into(), json!(STANDUP_SNIPPET))]);
    if peer {
        rules.retain(|rule| {
            rule.get("scope").and_then(Value::as_str).unwrap_or("group") == "group"
                || rule.get("owner_actor_id").and_then(Value::as_str) == Some(by)
        });
        let visible_ids = rules
            .iter()
            .filter_map(|rule| rule.get("id").and_then(Value::as_str))
            .collect::<std::collections::BTreeSet<_>>();
        let snippet_refs = rules
            .iter()
            .filter_map(|rule| {
                rule.get("action")
                    .and_then(|action| action.get("snippet_ref"))
                    .and_then(Value::as_str)
            })
            .collect::<std::collections::BTreeSet<_>>();
        effective_snippets.retain(|key, _| snippet_refs.contains(key.as_str()));
        built_in.retain(|key, _| snippet_refs.contains(key.as_str()));
        status.retain(|id, _| visible_ids.contains(id.as_str()));
    }
    let filtered_custom = custom
        .as_object()
        .map(|items| {
            items
                .iter()
                .filter(|(key, _)| effective_snippets.contains_key(key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>()
        })
        .unwrap_or_default();
    let filtered_overrides = overrides
        .as_object()
        .map(|items| {
            items
                .iter()
                .filter(|(key, _)| effective_snippets.contains_key(key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Map<_, _>>()
        })
        .unwrap_or_default();
    Ok(json!({
        "group_id":group.group_id,
        "ruleset":{"rules":rules,"snippets":effective_snippets},
        "snippet_catalog":{"built_in":built_in,"built_in_overrides":filtered_overrides,"custom":filtered_custom},
        "status":status,
        "config_path":home.groups_dir().join(&group.group_id).join("group.yaml").display().to_string(),
        "supported_vars":["interval_minutes","group_title","actor_names","scheduled_at"],
        "version":group.automation.get("version").and_then(Value::as_u64).unwrap_or(1),
        "server_now":utc_now(),
    }))
}

fn baseline_rules() -> Vec<Value> {
    vec![json!({
        "id":"standup","enabled":false,"scope":"group","owner_actor_id":null,
        "to":["@foreman"],"trigger":{"kind":"interval","every_seconds":900},
        "action":{"kind":"notify","priority":"normal",
            "title":"Stand-up reminder","snippet_ref":"standup","message":""}
    })]
}

/// The ruleset in force: the saved rules, or the built-in baseline while the
/// Group has never saved a list. A saved empty list stays empty. A saved value
/// that a change could not carry over intact (the wrong type, or a rule that
/// is not an object or lacks a distinct id) is reported, not dropped.
fn saved_ruleset(group: &GroupDoc) -> Result<Ruleset, OpError> {
    let invalid = |problem: &str| {
        OpError::new(
            "group_automation_state_invalid",
            format!(
                "the saved automation {problem}; \
                 the user can replace it by saving or resetting the rules"
            ),
        )
    };
    let map = |key: &str| match group.automation.get(key) {
        None => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(_) => Err(invalid(&format!("{key} has the wrong type"))),
    };
    let rules = match group.automation.get("rules") {
        None => baseline_rules(),
        Some(Value::Array(rules)) => {
            let mut ids = std::collections::BTreeSet::new();
            for rule in rules {
                let id = rule
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty());
                if !id.is_some_and(|id| ids.insert(id)) {
                    return Err(invalid(
                        "rules include one without an id or with the id of another",
                    ));
                }
            }
            rules.clone()
        }
        Some(_) => return Err(invalid("rules have the wrong type")),
    };
    Ok(Ruleset {
        rules,
        snippets: map("snippets")?,
        overrides: map("snippet_overrides")?,
    })
}

fn normalize_ruleset(
    ruleset: &Map<String, Value>,
    by: &str,
    role: Option<ActorRole>,
    stored: Option<&Vec<Value>>,
) -> Result<(Vec<Value>, Map<String, Value>), OpError> {
    if let Some(key) = ruleset
        .keys()
        .find(|key| !matches!(key.as_str(), "rules" | "snippets"))
    {
        return Err(OpError::new(
            "group_automation_update_failed",
            format!("unknown ruleset field: {key}"),
        ));
    }
    let raw_rules = match ruleset.get("rules") {
        Some(Value::Array(rules)) => rules.clone(),
        Some(_) => {
            return Err(OpError::new(
                "group_automation_update_failed",
                "rules must be an array",
            ));
        }
        None => Vec::new(),
    };
    let snippets = match ruleset.get("snippets") {
        Some(Value::Object(snippets))
            if snippets.values().all(|value| value.as_str().is_some()) =>
        {
            snippets.clone()
        }
        Some(_) => {
            return Err(OpError::new(
                "group_automation_update_failed",
                "snippets must be an object of strings",
            ));
        }
        None => Map::new(),
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut rules = Vec::with_capacity(raw_rules.len());
    for (index, value) in raw_rules.into_iter().enumerate() {
        let mut rule = value.as_object().cloned().ok_or_else(|| {
            OpError::new(
                "group_automation_update_failed",
                format!("rules[{index}] must be an object"),
            )
        })?;
        let previous = stored_rule(stored, rule.get("id"));
        validate_rule(&mut rule, by, role, previous)?;
        let id = rule
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if !seen.insert(id.clone()) {
            return Err(OpError::new(
                "group_automation_update_failed",
                format!("duplicate rule id: {id}"),
            ));
        }
        rules.push(Value::Object(rule));
    }
    Ok((rules, snippets))
}

/// The saved rule a submitted rule replaces, matched by id.
fn stored_rule<'a>(stored: Option<&'a Vec<Value>>, id: Option<&Value>) -> Option<&'a Value> {
    let id = id?.as_str()?;
    stored?
        .iter()
        .find(|rule| rule.get("id").and_then(Value::as_str) == Some(id))
}

fn mutate_automation(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut GroupDoc),
) -> Result<GroupDoc, OpError> {
    let store = store(home)?;
    let (updated, _) = store
        .mutate_with_rollback(
            group_id,
            |document| {
                let previous = automation_rules(document);
                change(document);
                Ok((document.clone(), previous))
            },
            |(document, previous)| {
                cccc_core::automation::reconcile_rule_state(
                    &store,
                    group_id,
                    previous,
                    &automation_rules(document),
                )
            },
        )
        .map_err(OpError::io)?;
    Ok(updated)
}

fn automation_rules(group: &GroupDoc) -> Vec<Value> {
    group
        .automation
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn load(home: &HomeLayout, request: &DaemonRequest) -> Result<GroupDoc, OpError> {
    store(home)?
        .load(&required_arg(request, "group_id")?)
        .map_err(OpError::not_found)
}

fn caller(request: &DaemonRequest) -> String {
    string_arg(request, "by").unwrap_or_else(|| "user".into())
}

/// The caller is the Actor named exactly: rules are validated and owned
/// under the same `by`, so a padded name is not that Actor.
fn caller_role(group: &GroupDoc, by: &str) -> Result<Option<ActorRole>, OpError> {
    if by.is_empty() || matches!(by, "user" | "system") {
        return Ok(None);
    }
    cccc_core::actors::effective_role(group, by)
        .map(Some)
        .ok_or_else(|| OpError::new("permission_denied", format!("unknown actor: {by}")))
}

fn append_event(
    home: &HomeLayout,
    group_id: &str,
    request: &DaemonRequest,
    kind: &str,
) -> Result<Event, OpError> {
    let mut event = Event::new(kind, group_id);
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    cccc_core::ledger::append(
        &store(home)?.ledger_path(group_id).map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    Ok(event)
}
