use cccc_contracts::DaemonRequest;
use serde_json::Value;

use crate::dispatch::{OpError, bool_arg, string_arg};

pub(super) fn list(request: &DaemonRequest, profiles: Vec<Value>) -> Result<Vec<Value>, OpError> {
    let caller = caller(request);
    let admin = admin(request);
    let view = string_arg(request, "view").unwrap_or_else(|| "global".into());
    if view == "all" && !admin {
        return Err(denied());
    }
    Ok(profiles
        .into_iter()
        .filter(|profile| match view.as_str() {
            "all" => admin,
            "my" => is_owned(profile, &caller),
            "accessible" => is_global(profile) || is_owned(profile, &caller),
            _ => is_global(profile),
        })
        .collect())
}

pub(super) fn require_read(request: &DaemonRequest, profile: &Value) -> Result<(), OpError> {
    require_reference(request, profile)?;
    if admin(request) || is_global(profile) || is_owned(profile, &caller(request)) {
        Ok(())
    } else {
        Err(denied())
    }
}

pub(super) fn require_write(request: &DaemonRequest, profile: &Value) -> Result<(), OpError> {
    require_reference(request, profile)?;
    if admin(request) || is_owned(profile, &caller(request)) {
        Ok(())
    } else {
        Err(denied())
    }
}

pub(super) fn normalize_new(
    request: &DaemonRequest,
    profile: &mut serde_json::Map<String, Value>,
) -> Result<(), OpError> {
    let scope = profile
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global");
    if !matches!(scope, "global" | "user") {
        return Err(OpError::new(
            "invalid_args",
            "profile scope must be global or user",
        ));
    }
    if scope == "global" {
        profile.insert("owner_id".into(), Value::String(String::new()));
        if !admin(request) {
            return Err(denied());
        }
    } else {
        let caller = caller(request);
        let owner = profile
            .get("owner_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !admin(request) && owner != caller {
            return Err(denied());
        }
        if owner.is_empty() {
            profile.insert("owner_id".into(), Value::String(caller));
        }
    }
    Ok(())
}

pub(super) fn require_group(request: &DaemonRequest, group_id: &str) -> Result<(), OpError> {
    if admin(request)
        || request
            .args
            .get("allowed_groups")
            .and_then(Value::as_array)
            .is_some_and(|groups| groups.iter().any(|group| group.as_str() == Some(group_id)))
    {
        Ok(())
    } else {
        Err(denied())
    }
}

fn require_reference(request: &DaemonRequest, profile: &Value) -> Result<(), OpError> {
    for (args, field) in [
        (["profile_scope", "scope"], "scope"),
        (["profile_owner", "owner_id"], "owner_id"),
    ] {
        if let Some(expected) = args.iter().find_map(|arg| string_arg(request, arg))
            && !expected.is_empty()
            && profile.get(field).and_then(Value::as_str) != Some(expected.as_str())
        {
            return Err(OpError::new("profile_not_found", "profile not found"));
        }
    }
    Ok(())
}

fn caller(request: &DaemonRequest) -> String {
    string_arg(request, "caller_id").unwrap_or_default()
}

fn admin(request: &DaemonRequest) -> bool {
    if !request.args.contains_key("caller_id") && !request.args.contains_key("is_admin") {
        true
    } else {
        bool_arg(request, "is_admin", false)
    }
}

fn is_global(profile: &Value) -> bool {
    profile
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("global")
        == "global"
}

fn is_owned(profile: &Value, caller: &str) -> bool {
    profile.get("scope").and_then(Value::as_str) == Some("user")
        && !caller.is_empty()
        && profile.get("owner_id").and_then(Value::as_str) == Some(caller)
}

fn denied() -> OpError {
    OpError::new("permission_denied", "profile access denied")
}
