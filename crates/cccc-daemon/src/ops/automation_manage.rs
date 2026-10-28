use cccc_contracts::{ActorRole, DaemonRequest};
use cccc_core::GroupDoc;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

use super::automation_rule_access::{
    expected_version, required_id, required_text, validate as validate_rule,
};
use crate::dispatch::{OpError, string_arg};

/// Rules and snippets as they are saved: custom snippets apart from the
/// override of the built-in `standup` text.
#[derive(Clone, PartialEq)]
pub(super) struct Ruleset {
    pub rules: Vec<Value>,
    pub snippets: Map<String, Value>,
    pub overrides: Map<String, Value>,
}

pub(super) struct Outcome {
    pub ruleset: Ruleset,
    pub applied_actions: Vec<Value>,
    pub changed: bool,
}

/// Applies the actions to `saved`, the ruleset in force, whose rules are
/// objects with distinct ids. The caller authorizes the resulting change as a
/// whole before saving it.
pub(super) fn apply(
    group: &GroupDoc,
    request: &DaemonRequest,
    role: Option<ActorRole>,
    saved: &Ruleset,
) -> Result<Outcome, OpError> {
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
    let mut order = Vec::new();
    let mut rules = BTreeMap::new();
    for rule in &saved.rules {
        let id = rule.get("id").and_then(Value::as_str).unwrap_or_default();
        order.push(id.to_owned());
        rules.insert(id.to_owned(), rule.clone());
    }
    let mut snippets = saved.snippets.clone();
    let mut overrides = saved.overrides.clone();
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
                let id = required_id(&rule, "id")?.to_owned();
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
                validate_rule(&mut rule, &by, role, existing)?;
                if existing.is_none() {
                    order.push(id.clone());
                }
                rules.insert(id.clone(), Value::Object(rule));
                applied.push(json!({"type":kind,"rule_id":id}));
            }
            "set_rule_enabled" => {
                let id = required_id(action, "rule_id")?;
                let Some(&Value::Bool(enabled)) = action.get("enabled") else {
                    return Err(OpError::new(
                        "invalid_request",
                        "set_rule_enabled requires enabled: true or false",
                    ));
                };
                let existing = rules.get_mut(id).ok_or_else(|| {
                    OpError::new(
                        "group_automation_manage_failed",
                        format!("rule not found: {id}"),
                    )
                })?;
                existing["enabled"] = Value::Bool(enabled);
                applied.push(json!({"type":kind,"rule_id":id,"enabled":enabled}));
            }
            "delete_rule" => {
                let id = required_id(action, "rule_id")?;
                if !rules.contains_key(id) {
                    return Err(OpError::new(
                        "group_automation_manage_failed",
                        format!("rule not found: {id}"),
                    ));
                }
                rules.remove(id);
                order.retain(|item| item != id);
                applied.push(json!({"type":kind,"rule_id":id}));
            }
            "replace_all_rules" => {
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
                let submitted = match ruleset.get("snippets") {
                    None => Map::new(),
                    Some(Value::Object(snippets)) if snippets.values().all(Value::is_string) => {
                        snippets.clone()
                    }
                    Some(_) => {
                        return Err(OpError::new(
                            "invalid_request",
                            "snippets must be an object of strings",
                        ));
                    }
                };
                order.clear();
                let previous = std::mem::take(&mut rules);
                for value in replacement {
                    let mut rule = value
                        .as_object()
                        .cloned()
                        .ok_or_else(|| OpError::new("invalid_request", "rule must be an object"))?;
                    let id = required_id(&rule, "id")?.to_owned();
                    if rules.contains_key(&id) {
                        return Err(OpError::new(
                            "group_automation_manage_failed",
                            format!("duplicate rule id: {id}"),
                        ));
                    }
                    validate_rule(&mut rule, &by, role, previous.get(&id))?;
                    order.push(id.clone());
                    rules.insert(id, Value::Object(rule));
                }
                let count = submitted.len();
                (snippets, overrides) = split_snippets(submitted);
                applied.push(json!({"type":kind,"rules":order.len(),"snippets":count}));
            }
            _ => {
                return Err(OpError::new(
                    "group_automation_manage_failed",
                    format!("unsupported action type: {kind}"),
                ));
            }
        }
    }
    let ruleset = Ruleset {
        rules: order
            .iter()
            .filter_map(|id| rules.get(id).cloned())
            .collect(),
        snippets,
        overrides,
    };
    Ok(Outcome {
        changed: ruleset != *saved,
        ruleset,
        applied_actions: applied,
    })
}

/// Submitted snippets as they are saved. The built-in `standup` text is not
/// stored; a different `standup` text becomes its override, which takes
/// precedence when the rule fires.
pub(super) fn split_snippets(
    mut snippets: Map<String, Value>,
) -> (Map<String, Value>, Map<String, Value>) {
    let mut overrides = Map::new();
    if let Some(standup) = snippets.remove("standup")
        && standup.as_str() != Some(cccc_core::automation::STANDUP_SNIPPET)
    {
        overrides.insert("standup".into(), standup);
    }
    (snippets, overrides)
}
