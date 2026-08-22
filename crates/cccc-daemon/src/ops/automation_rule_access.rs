use serde_json::{Map, Value, json};

use crate::dispatch::OpError;

pub(super) fn validate(
    rule: &mut Map<String, Value>,
    by: &str,
    peer: bool,
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
    required_text(rule, "id")?;
    match rule.get("enabled") {
        Some(value) if value.as_bool().is_none() => {
            return Err(invalid("enabled must be a boolean"));
        }
        None => {
            rule.insert("enabled".into(), Value::Bool(true));
        }
        _ => {}
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
    let trigger_kind = validate_trigger(
        rule.get_mut("trigger")
            .ok_or_else(|| invalid("trigger is required"))?,
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
    if !matches!(by, "" | "user" | "system") && action_kind != "notify" {
        return Err(invalid("agents can only manage notify automation rules"));
    }
    if peer {
        if let Some(existing) = existing {
            authorize(existing, by, true)?;
        }
        authorize(&Value::Object(rule.clone()), by, true)?;
    }
    Ok(())
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

pub(super) fn authorize(rule: &Value, by: &str, peer: bool) -> Result<(), OpError> {
    if !peer {
        return Ok(());
    }
    let owned = rule.get("scope").and_then(Value::as_str) == Some("personal")
        && rule.get("owner_actor_id").and_then(Value::as_str) == Some(by)
        && rule
            .get("action")
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str)
            == Some("notify")
        && rule
            .get("to")
            .and_then(Value::as_array)
            .is_some_and(|to| to.len() == 1 && to[0].as_str() == Some(by));
    if owned {
        Ok(())
    } else {
        Err(OpError::new(
            "permission_denied",
            "peer can only manage own personal notify rules",
        ))
    }
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

fn validate_trigger(value: &mut Value) -> Result<String, OpError> {
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
            required_text(trigger, "cron")?;
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
