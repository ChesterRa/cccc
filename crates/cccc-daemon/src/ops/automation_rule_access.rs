use serde_json::{Map, Value};

use crate::dispatch::OpError;

pub(super) fn validate(
    rule: &mut Map<String, Value>,
    by: &str,
    peer: bool,
    existing: Option<&Value>,
) -> Result<(), OpError> {
    let scope = rule.get("scope").and_then(Value::as_str).unwrap_or("group");
    if !matches!(scope, "group" | "personal") {
        return Err(OpError::new(
            "group_automation_manage_failed",
            "invalid scope",
        ));
    }
    if scope == "group" {
        rule.insert("owner_actor_id".into(), Value::Null);
    } else if rule
        .get("owner_actor_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        return Err(OpError::new(
            "group_automation_manage_failed",
            "personal rule requires owner_actor_id",
        ));
    }
    if peer {
        if let Some(existing) = existing {
            authorize(existing, by, true)?;
        }
        authorize(&Value::Object(rule.clone()), by, true)?;
    }
    Ok(())
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
