use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::HomeLayout;
use cccc_core::profiles::ProfileStore;
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

/// Check the caller before an Actor begins referencing a Profile. Existing links
/// may still be started by Group members; this gate protects creation, relinking,
/// and conversion paths that would newly expose Profile-controlled configuration
/// or private environment to the Group Actor.
pub(super) fn require_actor_link(
    home: &HomeLayout,
    request: &DaemonRequest,
    actor: &Actor,
) -> Result<(), OpError> {
    if actor.profile_id.is_empty() {
        return Ok(());
    }
    let profile = ProfileStore::new(home.clone())
        .map_err(OpError::io)?
        .get_ref(
            &actor.profile_id,
            &actor.profile_scope,
            &actor.profile_owner,
        )
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("profile_not_found", "profile not found"))?;
    let mut access = request.clone();
    access.args.insert(
        "profile_scope".into(),
        Value::String(actor.profile_scope.clone()),
    );
    access.args.insert(
        "profile_owner".into(),
        Value::String(actor.profile_owner.clone()),
    );
    require_read(&access, &profile)
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
        || (!actor_origin(request)
            && request
                .args
                .get("allowed_groups")
                .and_then(Value::as_array)
                .is_some_and(|groups| groups.iter().any(|group| group.as_str() == Some(group_id))))
    {
        Ok(())
    } else {
        Err(denied())
    }
}

/// Actor-originated requests never inherit a human Profile owner or administrator.
/// Runtime ports bind `by` to the authenticated Actor; local human/CLI requests use
/// the reserved `user` identity and keep the existing local-administrator default.
pub(super) fn actor_origin(request: &DaemonRequest) -> bool {
    string_arg(request, "by").is_some_and(|by| {
        let by = by.trim();
        !by.is_empty() && by != "user" && by != "system"
    })
}

/// A local user or an administrator of the Web port. Actor-originated
/// requests and Web users without administration rights are refused.
pub(super) fn require_admin_user(request: &DaemonRequest) -> Result<(), OpError> {
    if actor_origin(request)
        || string_arg(request, "by").is_some_and(|by| by.trim() == "system")
        || !admin(request)
    {
        return Err(OpError::new(
            "permission_denied",
            "Only an administrator can switch a member's runtime",
        ));
    }
    Ok(())
}

pub(super) fn explicit_admin(request: &DaemonRequest) -> bool {
    !actor_origin(request) && bool_arg(request, "is_admin", false)
}

fn require_reference(request: &DaemonRequest, profile: &Value) -> Result<(), OpError> {
    // Legacy records predate stored identity fields; read them with the same
    // defaults as listing and lookup (global, no owner), so explicit references
    // to them resolve while other scopes and owners still do not match.
    for (args, field, default) in [
        (["profile_scope", "scope"], "scope", "global"),
        (["profile_owner", "owner_id"], "owner_id", ""),
    ] {
        if let Some(expected) = args.iter().find_map(|arg| string_arg(request, arg))
            && !expected.is_empty()
            && profile
                .get(field)
                .and_then(Value::as_str)
                .unwrap_or(default)
                != expected
        {
            return Err(OpError::new("profile_not_found", "profile not found"));
        }
    }
    Ok(())
}

fn caller(request: &DaemonRequest) -> String {
    if actor_origin(request) {
        String::new()
    } else {
        string_arg(request, "caller_id").unwrap_or_default()
    }
}

fn admin(request: &DaemonRequest) -> bool {
    if actor_origin(request) {
        false
    } else if !request.args.contains_key("caller_id") && !request.args.contains_key("is_admin") {
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
