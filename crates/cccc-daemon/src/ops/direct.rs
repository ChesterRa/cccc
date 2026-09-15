use super::operation::{Operation, Policy};
use crate::dispatch::{OpError, OpResult, object, required_arg};
use cccc_contracts::{DaemonRequest, direct::*};
use cccc_core::{HomeLayout, direct};
use serde_json::{Value, json};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "connect_direct_status" => Operation::new(Policy::Read, status),
        "connect_direct_configure" => Operation::new(Policy::ResourceOwned, configure),
        "connect_direct_invite"
        | "connect_direct_join"
        | "connect_direct_approve"
        | "connect_direct_revoke"
        | "connect_direct_remove" => Operation::new(Policy::Write, mutate),
        _ => return None,
    })
}
fn user(request: &DaemonRequest) -> Result<(), OpError> {
    if request
        .args
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or("user")
        != "user"
    {
        return Err(OpError::new(
            "permission_denied",
            "Only the local administrator can manage direct connections",
        ));
    }
    Ok(())
}
#[cfg(test)]
#[path = "direct_tests.rs"]
mod tests;

// Administrator cleanup remains possible for records left by an interrupted
// deletion. New grants and approvals always require an existing Group.
fn management_group(home: &HomeLayout, request: &DaemonRequest) -> Result<String, OpError> {
    let group_id = required_arg(request, "group_id")?;
    match cccc_core::GroupStore::new(home.clone()).and_then(|store| store.load(&group_id)) {
        Ok(group) => cccc_core::permissions::require_group_member(&group, "user")
            .map_err(|error| OpError::new("permission_denied", error.to_string()))?,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && matches!(
                    request.op.as_str(),
                    "connect_direct_status" | "connect_direct_revoke" | "connect_direct_remove"
                ) => {}
        Err(error) => return Err(OpError::not_found(error)),
    }
    Ok(group_id)
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let group = management_group(home, request)?;
    let store = direct::load(home).map_err(OpError::io)?;
    let port = store
        .listener
        .as_ref()
        .and_then(|l| l.bind.parse::<std::net::SocketAddr>().ok())
        .map_or(8847, |a| a.port());
    let addresses = cccc_core::local_network::addresses()
        .unwrap_or_default()
        .into_iter()
        .map(|a| DirectAddress {
            interface: a.interface,
            address: std::net::SocketAddr::new(a.ip, port).to_string(),
            bind: std::net::SocketAddr::new(cccc_core::local_network::wildcard(a.ip), port)
                .to_string(),
        })
        .collect::<Vec<_>>();
    let mut runtime: Value =
        cccc_core::fs::read_json(&home.root().join("state/connect/direct_status.json"))
            .unwrap_or(Value::Null);
    if runtime["bind"].as_str() != store.listener.as_ref().map(|l| l.bind.as_str()) {
        runtime["listener"] = json!(false);
        runtime["error"] = Value::Null;
    }
    let fresh = runtime["checked_at"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|t| t + chrono::Duration::seconds(10) > chrono::Utc::now());
    let relations=store.relations.iter().filter(|r|r.local.group_id==group).map(|r| {
        let current=direct::current(home,&r.local).unwrap_or(false);
        let online=fresh && current && r.state==DirectState::Active && runtime["online"].as_array().is_some_and(|ids|ids.iter().any(|id|id==&r.id));
        json!({"id":r.id,"local":r.local,"remote":r.remote,"state":r.state,"initiated":r.invitation.is_some(),"current":current,"expired":!direct::unexpired(&r.expires_at),"expires_at":r.expires_at,"online":online,"error":if online{Value::Null}else{runtime["errors"][&r.id].clone()}})
    }).collect::<Vec<_>>();
    object(
        json!({"listener":store.listener,"display_name":store.display_name,"addresses":addresses,"runtime":if fresh{runtime}else{Value::Null},"relations":relations}),
    )
}
fn configure(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let listener: Option<DirectListener> =
        serde_json::from_value(request.args.get("listener").cloned().unwrap_or(Value::Null))
            .map_err(OpError::invalid)?;
    let name = request.args.get("display_name").and_then(Value::as_str);
    let expected: Option<Option<DirectListener>> = request
        .args
        .get("expected_listener")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(OpError::invalid)?;
    direct::configure_checked(home, listener, name, expected.as_ref()).map_err(OpError::io)?;
    object(json!({"configured":true}))
}
fn mutate(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let group = management_group(home, request)?;
    if matches!(
        request.op.as_str(),
        "connect_direct_invite" | "connect_direct_join"
    ) && direct::load(home)
        .map_err(OpError::io)?
        .display_name
        .is_empty()
    {
        let name = cccc_core::connect::load(home)
            .ok()
            .flatten()
            .and_then(|s| {
                s.directory.and_then(|d| {
                    d.instances
                        .into_iter()
                        .find(|i| i.instance_id == s.instance_id)
                        .map(|i| i.display_name)
                })
            })
            .filter(|s| !s.is_empty())
            .or_else(super::membership::initial_instance_name);
        if let Some(name) = name {
            direct::update(home, |store| {
                if store.display_name.is_empty() {
                    store.display_name = name;
                }
                Ok(())
            })
            .map_err(OpError::io)?;
        }
    }
    let result = match request.op.as_str() {
        "connect_direct_invite" => {
            let expected: Option<DirectListener> = request
                .args
                .get("expected_listener")
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()
                .map_err(OpError::invalid)?;
            json!({"invitation":direct::invite_checked(home,&group,expected.as_ref()).map_err(OpError::io)?})
        }
        "connect_direct_join" => {
            json!({"id":direct::join(home,&group,&required_arg(request,"invitation")?).map_err(OpError::io)?})
        }
        "connect_direct_remove" => {
            direct::remove(home, &group, &required_arg(request, "id")?).map_err(OpError::io)?;
            json!({"removed":true})
        }
        op => {
            direct::set_state(
                home,
                &group,
                &required_arg(request, "id")?,
                op == "connect_direct_approve",
            )
            .map_err(OpError::io)?;
            json!({"updated":true})
        }
    };
    object(result)
}
