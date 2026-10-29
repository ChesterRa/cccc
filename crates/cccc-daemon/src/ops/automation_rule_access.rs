use cccc_contracts::ActorRole;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

use crate::dispatch::OpError;

/// Checks a rule's shape and fills its defaults. Who may save it is decided
/// afterwards, for the whole request, by `authorize_change`.
pub(super) fn validate(
    rule: &mut Map<String, Value>,
    by: &str,
    role: Option<ActorRole>,
    existing: Option<&Value>,
) -> Result<(), OpError> {
    reject_unknown(
        rule,
        &[
            "id",
            "enabled",
            "scope",
            "owner_actor_id",
            "to",
            "trigger",
            "action",
        ],
        "rule",
    )?;
    required_id(rule, "id")?;
    match rule.get("enabled") {
        Some(value) if value.as_bool().is_none() => {
            return Err(invalid("enabled must be a boolean"));
        }
        None => {
            rule.insert("enabled".into(), Value::Bool(true));
        }
        _ => {}
    }
    // A peer can only remind itself, so that is what the fields it leaves out
    // mean. Values it does send are kept as sent and authorized as such.
    if role == Some(ActorRole::Peer) {
        rule.entry("scope").or_insert_with(|| json!("personal"));
        if rule.get("scope").and_then(Value::as_str) == Some("personal") {
            rule.entry("owner_actor_id").or_insert_with(|| json!(by));
            rule.entry("to").or_insert_with(|| json!([by]));
        }
    }
    let scope = rule
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("group")
        .to_owned();
    if !matches!(scope.as_str(), "group" | "personal") {
        return Err(invalid("invalid scope"));
    }
    rule.entry("scope").or_insert_with(|| json!("group"));
    if scope == "group" {
        rule.insert("owner_actor_id".into(), Value::Null);
    } else {
        required_text(rule, "owner_actor_id")?;
    }
    let to = rule.entry("to").or_insert_with(|| json!(["@foreman"]));
    if !to
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
    {
        return Err(invalid("to must be an array of strings"));
    }
    let stored_cron = existing
        .and_then(|rule| rule.get("trigger"))
        .and_then(|trigger| trigger.get("cron"))
        .and_then(Value::as_str);
    let trigger_kind = validate_trigger(
        rule.get_mut("trigger")
            .ok_or_else(|| invalid("trigger is required"))?,
        stored_cron,
    )?;
    let action = rule.entry("action").or_insert_with(|| {
        json!({
            "kind":"notify","title":"","snippet_ref":null,"message":"",
            "priority":"normal"
        })
    });
    let action_kind = validate_action(action)?;
    if matches!(action_kind.as_str(), "group_state" | "actor_control") && trigger_kind != "at" {
        return Err(invalid(format!(
            "action.kind={action_kind} only supports trigger.kind=at"
        )));
    }
    Ok(())
}

/// An Actor's request may add, change, remove or reorder only the rules its
/// role manages: a foreman every notify rule, a peer its own personal reminder
/// to itself. User and system requests are not checked. Scheduled group_state and actor_control rules, and the shared
/// snippets a peer would change, belong to others. Both the saved and the
/// submitted form of every touched rule are checked, so a rule cannot be
/// converted, disabled or dropped past this check, and the relative order of
/// the untouched rules is kept because the scheduler runs same-time rules in
/// array order. A refused request changes nothing.
pub(super) fn authorize_change(
    before: &[Value],
    after: &[Value],
    snippets_changed: bool,
    by: &str,
    role: ActorRole,
) -> Result<(), OpError> {
    let manages = |rule: &Value| match role {
        ActorRole::Foreman => notifies(rule),
        ActorRole::Peer => own_reminder(rule, by),
    };
    let saved = versions(before);
    let submitted = versions(after);
    let touched = saved
        .keys()
        .chain(submitted.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|id| {
            let (old, new) = (saved.get(id), submitted.get(id));
            old != new
                && old
                    .into_iter()
                    .chain(new)
                    .flatten()
                    .copied()
                    .any(|rule| !manages(rule))
        })
        .collect::<Vec<_>>();
    let reordered = touched.is_empty()
        && !before
            .iter()
            .filter(|rule| !manages(rule))
            .map(rule_id)
            .eq(after.iter().filter(|rule| !manages(rule)).map(rule_id));
    let snippets_denied = snippets_changed && role == ActorRole::Peer;
    if touched.is_empty() && !reordered && !snippets_denied {
        return Ok(());
    }
    let message = match role {
        ActorRole::Foreman if reordered => "permission denied: this request reorders \
            scheduled group_state or actor_control rules, which only the user can change"
            .to_owned(),
        ActorRole::Foreman => format!(
            "permission denied: only the user can change scheduled group_state or \
             actor_control rules ({}); a foreman manages notify rules",
            touched.join(", ")
        ),
        ActorRole::Peer => {
            // A peer sees group rules and its own; the rest are only counted.
            let visible = |id: &&str| {
                saved.get(id).is_none_or(|rules| {
                    rules.iter().any(|rule| {
                        rule.get("scope").and_then(Value::as_str).unwrap_or("group") == "group"
                            || rule.get("owner_actor_id").and_then(Value::as_str) == Some(by)
                    })
                })
            };
            let (shown, hidden): (Vec<&str>, Vec<&str>) =
                touched.iter().copied().partition(visible);
            let mut affected = shown.join(", ");
            if !hidden.is_empty() {
                if !affected.is_empty() {
                    affected.push_str(" and ");
                }
                affected.push_str(&match hidden.len() {
                    1 => "1 rule not visible to you".to_owned(),
                    count => format!("{count} rules not visible to you"),
                });
            }
            if reordered {
                affected = "the order of other rules".into();
            }
            let mut message = "permission denied: a peer can change only its own reminders \
                (scope personal, with owner_actor_id and to set to itself)"
                .to_owned();
            if !affected.is_empty() {
                message.push_str(&format!("; this request would change {affected}"));
            }
            if snippets_denied {
                message.push_str(
                    "; snippets are shared and only the user or the foreman can change them",
                );
            }
            message
        }
    };
    Err(OpError::new("permission_denied", message))
}

fn versions(rules: &[Value]) -> BTreeMap<&str, Vec<&Value>> {
    let mut versions = BTreeMap::<&str, Vec<&Value>>::new();
    for rule in rules {
        versions.entry(rule_id(rule)).or_default().push(rule);
    }
    versions
}

fn rule_id(rule: &Value) -> &str {
    rule.get("id").and_then(Value::as_str).unwrap_or_default()
}

fn notifies(rule: &Value) -> bool {
    rule.get("action")
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        == Some("notify")
}

fn own_reminder(rule: &Value, by: &str) -> bool {
    notifies(rule)
        && rule.get("scope").and_then(Value::as_str) == Some("personal")
        && rule.get("owner_actor_id").and_then(Value::as_str) == Some(by)
        && rule
            .get("to")
            .and_then(Value::as_array)
            .is_some_and(|to| to.len() == 1 && to[0].as_str() == Some(by))
}

pub(super) fn expected_version(args: &Map<String, Value>) -> Result<Option<u64>, OpError> {
    let Some(value) = args.get("expected_version") else {
        return Ok(None);
    };
    value
        .as_u64()
        .or_else(|| Value::as_str(value).and_then(|value| value.trim().parse().ok()))
        .map(Some)
        .ok_or_else(|| OpError::new("invalid_request", "expected_version must be an integer"))
}

pub(super) fn required_text<'a>(
    map: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, OpError> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_request", format!("{key} is required")))
}

/// A rule id exactly as written. The scheduler keys a rule's state by its
/// saved id and authorization compares ids as saved, so an id is never
/// trimmed or rewritten; only a blank one is refused.
pub(super) fn required_id<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, OpError> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| OpError::new("invalid_request", format!("{key} is required")))
}

/// `stored_cron` is the rule's saved expression: an unchanged one is accepted as-is so a
/// legacy unparsable rule cannot block saving the rest of its ruleset.
fn validate_trigger(value: &mut Value, stored_cron: Option<&str>) -> Result<String, OpError> {
    let trigger = value
        .as_object_mut()
        .ok_or_else(|| invalid("trigger must be an object"))?;
    let kind = required_text(trigger, "kind")?.to_owned();
    match kind.as_str() {
        "interval" => {
            reject_unknown(trigger, &["kind", "every_seconds"], "interval trigger")?;
            if trigger
                .get("every_seconds")
                .and_then(Value::as_i64)
                .is_none_or(|seconds| seconds < 1)
            {
                return Err(invalid("interval every_seconds must be an integer >= 1"));
            }
        }
        "cron" => {
            reject_unknown(trigger, &["kind", "cron", "timezone"], "cron trigger")?;
            let expression = required_text(trigger, "cron")?;
            if stored_cron.map(str::trim) != Some(expression)
                && let Some(error) = cccc_core::automation::cron_expression_error(expression)
            {
                return Err(invalid(format!("invalid cron expression: {error}")));
            }
            match trigger.get("timezone") {
                Some(value) if value.as_str().is_none() => {
                    return Err(invalid("cron timezone must be a string"));
                }
                None => {
                    trigger.insert("timezone".into(), json!("UTC"));
                }
                _ => {}
            }
        }
        "at" => {
            reject_unknown(trigger, &["kind", "at"], "at trigger")?;
            required_text(trigger, "at")?;
        }
        _ => return Err(invalid(format!("unsupported trigger kind: {kind}"))),
    }
    Ok(kind)
}

fn validate_action(value: &mut Value) -> Result<String, OpError> {
    let action = value
        .as_object_mut()
        .ok_or_else(|| invalid("action must be an object"))?;
    let kind = required_text(action, "kind")?.to_owned();
    match kind.as_str() {
        "notify" => {
            reject_unknown(
                action,
                &["kind", "title", "snippet_ref", "message", "priority"],
                "notify action",
            )?;
            for key in ["title", "message"] {
                match action.get(key) {
                    Some(value) if value.as_str().is_none() => {
                        return Err(invalid(format!("notify {key} must be a string")));
                    }
                    None => {
                        action.insert(key.into(), json!(""));
                    }
                    _ => {}
                }
            }
            match action.get("snippet_ref") {
                Some(value) if !value.is_null() && value.as_str().is_none() => {
                    return Err(invalid("notify snippet_ref must be a string or null"));
                }
                None => {
                    action.insert("snippet_ref".into(), Value::Null);
                }
                _ => {}
            }
            match action.get("priority") {
                Some(value)
                    if !matches!(value.as_str(), Some("low" | "normal" | "high" | "urgent")) =>
                {
                    return Err(invalid("invalid notify priority"));
                }
                None => {
                    action.insert("priority".into(), json!("normal"));
                }
                _ => {}
            }
        }
        "group_state" => {
            reject_unknown(action, &["kind", "state"], "group_state action")?;
            match action.get("state") {
                Some(value)
                    if matches!(
                        value.as_str(),
                        Some("active" | "idle" | "paused" | "stopped")
                    ) => {}
                Some(_) => return Err(invalid("invalid group_state state")),
                None => {
                    action.insert("state".into(), json!("paused"));
                }
            }
        }
        "actor_control" => {
            reject_unknown(
                action,
                &["kind", "operation", "targets"],
                "actor_control action",
            )?;
            match action.get("operation") {
                Some(value) if matches!(value.as_str(), Some("start" | "stop" | "restart")) => {}
                Some(_) => return Err(invalid("invalid actor_control operation")),
                None => {
                    action.insert("operation".into(), json!("restart"));
                }
            }
            let targets = action.entry("targets").or_insert_with(|| json!(["@all"]));
            if !targets
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
            {
                return Err(invalid("actor_control targets must be an array of strings"));
            }
        }
        _ => return Err(invalid(format!("unsupported action kind: {kind}"))),
    }
    Ok(kind)
}

fn reject_unknown(
    values: &Map<String, Value>,
    allowed: &[&str],
    label: &str,
) -> Result<(), OpError> {
    if let Some(key) = values.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid(format!("unknown {label} field: {key}")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> OpError {
    OpError::new("group_automation_manage_failed", message)
}
