use cccc_contracts::{ActorRole, DaemonRequest};
use cccc_core::GroupDoc;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

use super::automation_rule_access::{
    authorize as authorize_existing, expected_version, required_text, validate as validate_rule,
};
use crate::dispatch::{OpError, string_arg};

pub(super) struct Outcome {
    pub rules: Vec<Value>,
    pub snippets: Map<String, Value>,
    pub applied_actions: Vec<Value>,
    pub changed: bool,
}

pub(super) fn apply(group: &GroupDoc, request: &DaemonRequest) -> Result<Outcome, OpError> {
    let actions = request
        .args
        .get("actions")
        .and_then(Value::as_array)
        .filter(|actions| !actions.is_empty())
        .ok_or_else(|| OpError::new("invalid_request", "actions must be a non-empty array"))?;
    let current_version = group
        .automation
        .get("version")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    if let Some(expected) = expected_version(&request.args)?
        && expected != current_version
    {
        return Err(OpError::new(
            "version_conflict",
            format!("automation version mismatch: expected {expected}, current {current_version}"),
        ));
    }
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let peer = group
        .actors
        .iter()
        .find(|actor| actor.id == by)
        .is_some_and(|actor| actor.role == Some(ActorRole::Peer));
    let foreman = group
        .actors
        .iter()
        .find(|actor| actor.id == by)
        .is_some_and(|actor| actor.role == Some(ActorRole::Foreman));
    let original_rules = group
        .automation
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let original_snippets = group
        .automation
        .get("snippets")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut order = Vec::new();
    let mut rules = BTreeMap::new();
    for rule in &original_rules {
        let id = rule.get("id").and_then(Value::as_str).unwrap_or("").trim();
        if !id.is_empty() && !rules.contains_key(id) {
            order.push(id.to_owned());
            rules.insert(id.to_owned(), rule.clone());
        }
    }
    let mut snippets = original_snippets.clone();
    let mut applied = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let action = action.as_object().ok_or_else(|| {
            OpError::new(
                "invalid_request",
                format!("action[{index}] must be an object"),
            )
        })?;
        let kind = required_text(action, "type")?;
        match kind {
            "create_rule" | "update_rule" => {
                let mut rule = action
                    .get("rule")
                    .and_then(Value::as_object)
                    .cloned()
                    .ok_or_else(|| {
                        OpError::new("invalid_request", format!("{kind} requires rule"))
                    })?;
                let id = required_text(&rule, "id")?.to_owned();
                let existing = rules.get(&id);
                if kind == "create_rule" && existing.is_some() {
                    return Err(OpError::new(
                        "group_automation_manage_failed",
                        format!("rule already exists: {id}"),
                    ));
                }
                if kind == "update_rule" && existing.is_none() {
                    return Err(OpError::new(
                        "group_automation_manage_failed",
                        format!("rule not found: {id}"),
                    ));
                }
                validate_rule(&mut rule, &by, peer, existing)?;
                if existing.is_none() {
                    order.push(id.clone());
                }
                rules.insert(id.clone(), Value::Object(rule));
                applied.push(json!({"type":kind,"rule_id":id}));
            }
            "set_rule_enabled" => {
                let id = required_text(action, "rule_id")?;
                let existing = rules.get_mut(id).ok_or_else(|| {
                    OpError::new(
                        "group_automation_manage_failed",
                        format!("rule not found: {id}"),
                    )
                })?;
                authorize_existing(existing, &by, peer)?;
                existing["enabled"] = Value::Bool(
                    action
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                );
                applied.push(json!({"type":kind,"rule_id":id,"enabled":existing["enabled"]}));
            }
            "delete_rule" => {
                let id = required_text(action, "rule_id")?;
                let existing = rules.get(id).ok_or_else(|| {
                    OpError::new(
                        "group_automation_manage_failed",
                        format!("rule not found: {id}"),
                    )
                })?;
                authorize_existing(existing, &by, peer)?;
                rules.remove(id);
                order.retain(|item| item != id);
                applied.push(json!({"type":kind,"rule_id":id}));
            }
            "replace_all_rules" => {
                if by != "user" && !foreman {
                    return Err(OpError::new(
                        "permission_denied",
                        "replace_all_rules is foreman-only",
                    ));
                }
                let ruleset = action
                    .get("ruleset")
                    .and_then(Value::as_object)
                    .ok_or_else(|| {
                        OpError::new("invalid_request", "replace_all_rules requires ruleset")
                    })?;
                let replacement = ruleset
                    .get("rules")
                    .and_then(Value::as_array)
                    .ok_or_else(|| OpError::new("invalid_request", "rules must be an array"))?;
                order.clear();
                rules.clear();
                for value in replacement {
                    let mut rule = value
                        .as_object()
                        .cloned()
                        .ok_or_else(|| OpError::new("invalid_request", "rule must be an object"))?;
                    let id = required_text(&rule, "id")?.to_owned();
                    if rules.contains_key(&id) {
                        return Err(OpError::new(
                            "group_automation_manage_failed",
                            format!("duplicate rule id: {id}"),
                        ));
                    }
                    validate_rule(&mut rule, &by, false, None)?;
                    order.push(id.clone());
                    rules.insert(id, Value::Object(rule));
                }
                snippets = ruleset
                    .get("snippets")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                applied.push(json!({"type":kind,"rules":order.len(),"snippets":snippets.len()}));
            }
            _ => {
                return Err(OpError::new(
                    "group_automation_manage_failed",
                    format!("unsupported action type: {kind}"),
                ));
            }
        }
    }
    let rules = order
        .iter()
        .filter_map(|id| rules.get(id).cloned())
        .collect::<Vec<_>>();
    let changed = rules != original_rules || snippets != original_snippets;
    Ok(Outcome {
        rules,
        snippets,
        applied_actions: applied,
        changed,
    })
}
