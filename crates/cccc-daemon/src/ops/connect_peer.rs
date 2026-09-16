//! Narrow peer port: authenticate the current device, then apply resource scope.
use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use crate::dispatch::{OpError, OpResult, object, required_arg};
use cccc_contracts::{
    DaemonRequest,
    connect::{ConnectPeerOperation, ConnectPeerRequest},
};
use cccc_core::{
    GroupStore, HomeLayout, Registry, actors,
    connect_peer::{self, PeerScope},
};
use serde_json::{Value, json};

#[cfg(test)]
#[path = "connect_peer_tests.rs"]
mod tests;

const CATALOG_PAGE_GROUPS: usize = 64;
const MAX_CATALOG_BYTES: usize = 1024 * 1024;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "connect_peer_receive" => Operation::new(
            if matches!(
                request
                    .args
                    .get("envelope")
                    .and_then(|envelope| envelope.get("operation"))
                    .and_then(|op| op.get("op"))
                    .and_then(Value::as_str),
                Some("deliver" | "cancel")
            ) {
                Write
            } else {
                Read
            },
            receive,
        ),
        "connect_catalog" => Operation::new(Read, cached_catalog),
        _ => return None,
    })
}

fn external_groups(
    home: &HomeLayout,
    source_group: &str,
    links: Option<&cccc_contracts::connect_groups::ConnectGroupLinks>,
) -> Result<Vec<Value>, OpError> {
    let mut external =
        links
            .map(|links| {
                links.links.iter().filter_map(|link| {
            let (local, remote) = cccc_core::connect_groups::local_endpoint(links, link)?;
            if local.group_id != source_group
                || !cccc_core::connect_groups::resource_current(home, local).unwrap_or(false)
            {
                return None;
            }
            Some(json!({
                "connection_id": link.id, "instance": remote.instance,
                "group_id": remote.group_id, "title": remote.title, "transport": "account"
            }))
        }).collect::<Vec<_>>()
            })
            .unwrap_or_default();
    for relation in cccc_core::direct::load(home)
        .map_err(OpError::io)?
        .relations
    {
        if relation.local.group_id == source_group
            && let Some(remote) = &relation.remote
        {
            // A direct pin also suppresses the account row while that relation is offline/revoked.
            external.retain(|row| {
                row["instance"]["instance_id"] != remote.instance_id
                    || row["group_id"] != remote.group_id
            });
            if cccc_core::direct::binding(home, &remote.instance_id, &relation.id).is_ok() {
                external.push(json!({"connection_id":relation.id,"instance":cccc_core::direct::instance(remote,&relation.created_at),"group_id":remote.group_id,"title":remote.title,"transport":"direct"}));
            }
        }
    }
    Ok(external)
}

fn cached_catalog(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source_group = authorize_local_source(home, request)?;
    let instance_id = request
        .args
        .get("instance_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(instance_id) = instance_id else {
        if request.args.contains_key("target_group_id") {
            return Err(OpError::new(
                "connect_instance_required",
                "target_group_id requires instance_id from cccc_connect()",
            ));
        }
        let snapshot = cccc_core::connect::load(home);
        let links = cccc_core::connect_groups::load(home);
        let account_error =
            (snapshot.is_err() || links.is_err()).then_some("connect_account_unavailable");
        let snapshot = snapshot.ok().flatten();
        let directory = snapshot.as_ref().and_then(|s| s.directory.as_ref());
        let own = cccc_core::instance_identity::InstanceIdentity::load(home)
            .ok()
            .map(|i| i.peer_id);
        let instances = directory
            .map(|d| {
                d.instances
                    .iter()
                    .filter(|i| Some(i.instance_id.as_str()) != own.as_deref())
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let external = external_groups(home, &source_group, links.ok().flatten().as_ref())?;
        let status = if directory.is_some() || !external.is_empty() {
            "ready"
        } else if snapshot.is_some() || account_error.is_some() {
            "unavailable"
        } else {
            "not_linked"
        };
        return object(
            json!({"self_instance_id":own,"instances":instances,"external_groups":external,"status":status,"account_error":account_error,"checked_at":snapshot.as_ref().map(|s|&s.checked_at),"expires_at":directory.map(|d|&d.expires_at)}),
        );
    };
    let target = request.args.get("target_group_id").and_then(Value::as_str);
    let binding = if let Some(target) = target {
        connect_peer::group_binding(home, instance_id, &source_group, target)
    } else {
        match connect_peer::binding(home, instance_id) {
            Ok(binding) => Ok(binding),
            Err(message) => {
                let links = cccc_core::connect_groups::load(home).ok().flatten();
                let groups = external_groups(home, &source_group, links.as_ref())?
                    .into_iter().filter(|row| row["instance"]["instance_id"] == instance_id)
                    .collect::<Vec<_>>();
                if !groups.is_empty() {
                    let mut error = OpError::new("connect_target_group_required",
                        "This instance is available through Group connections. Pass instance_id and target_group_id from external_groups to read a connected Group's Actors.");
                    error.details.insert("external_groups".into(), json!(groups));
                    return Err(error);
                }
                Err(message)
            }
        }
    }
    .map_err(|message| OpError::new("connect_peer_unavailable", message))?;
    let mut catalog = cccc_core::connect_catalog::load_scoped(
        home,
        instance_id,
        binding.group.as_ref().map(|g| g.id.as_str()),
    )
    .map_err(OpError::io)?;
    let fresh = catalog
        .as_ref()
        .is_some_and(cccc_core::connect_catalog::is_fresh);
    let after = request.args.get("after").and_then(Value::as_str);
    let limit = request
        .args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 64) as usize;
    let mut next = None;
    if let Some(catalog) = &mut catalog {
        catalog.groups.retain(|group| {
            after.is_none_or(|after| group.group_id.as_str() > after)
                && target.is_none_or(|target| group.group_id == target)
        });
        if catalog.groups.len() > limit {
            catalog.groups.truncate(limit);
            next = catalog.groups.last().map(|group| group.group_id.clone());
        }
    }
    object(json!({"instance":binding.remote,"catalog":catalog,"fresh":fresh,"next":next}))
}

fn receive(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let envelope: ConnectPeerRequest =
        serde_json::from_value(request.args.get("envelope").cloned().unwrap_or(Value::Null))
            .map_err(|error| OpError::new("invalid_connect_request", error.to_string()))?;
    if request.args.get("group_id").and_then(Value::as_str) != envelope.operation.target_group_id()
    {
        return Err(OpError::new(
            "invalid_connect_request",
            "target Group must match dispatcher scope",
        ));
    }
    let scope = connect_peer::authenticate(home, &envelope)
        .map_err(|message| OpError::new("connect_peer_denied", message))?;
    let result = match &envelope.operation {
        ConnectPeerOperation::Cancel { cancellation } => {
            super::connect_cancellation::receive(home, &envelope, &scope, cancellation)
        }
        ConnectPeerOperation::Deliver { message, blobs } => {
            super::connect_messages::deliver(home, request, &envelope, &scope, message, blobs)
        }
        ConnectPeerOperation::Receipt { .. } => {
            super::connect_messages::receipt(home, &envelope, &scope)
        }
        ConnectPeerOperation::Catalog {
            source_group_id,
            target_group_id,
            after,
            ..
        } => catalog(
            home,
            &scope,
            source_group_id,
            target_group_id.as_deref(),
            after.as_deref(),
        ),
    };
    let result = match result {
        Ok(value) => json!({"ok":true,"result":value}),
        Err(error) => json!({"ok":false,"error":{"code":error.code,"message":error.message}}),
    };
    let response = connect_peer::sign_response(home, &envelope, result)
        .map_err(|message| OpError::new("connect_peer_denied", message))?;
    object(json!({"response":response}))
}

fn catalog(
    home: &HomeLayout,
    scope: &PeerScope,
    source_group_id: &str,
    target_group_id: Option<&str>,
    after: Option<&str>,
) -> OpResult {
    if !scope.allows(source_group_id, target_group_id) {
        return Err(OpError::new(
            "connect_scope_denied",
            "peer cannot discover this Group scope",
        ));
    }
    // Only navigation and recipient identity are disclosed, never commands, paths,
    // environment, full context, history, or terminal credentials.
    let registry = Registry::load(home).map_err(OpError::io)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let eligible = registry
        .groups
        .keys()
        .filter(|id| target_group_id.is_none_or(|target| id.as_str() == target))
        .filter(|id| after.is_none_or(|cursor| id.as_str() > cursor))
        .take(CATALOG_PAGE_GROUPS + 1)
        .cloned()
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    for id in eligible.iter().take(CATALOG_PAGE_GROUPS) {
        let group = store.load(id).map_err(OpError::io)?;
        let recipients = actors::visible(&group).map(|actor| json!({
            "id":actor.id,"title":actor.title,"enabled":actor.enabled,
            "role":actors::effective_role(&group, &actor.id),
            "generation":if actor.generation.is_empty() {format!("legacy:{}",actor.created_at)} else {actor.generation.clone()},
        })).collect::<Vec<_>>();
        groups.push(json!({"group_id":group.group_id,"title":group.title,"actors":recipients}));
    }
    let next =
        (eligible.len() > CATALOG_PAGE_GROUPS).then(|| eligible[CATALOG_PAGE_GROUPS - 1].clone());
    let result = json!({"groups":groups,"next":next});
    if serde_json::to_vec(&result).map_err(OpError::invalid)?.len() > MAX_CATALOG_BYTES {
        return Err(OpError::new(
            "connect_catalog_too_large",
            "Group directory page exceeds 1 MiB",
        ));
    }
    object(result)
}

pub(super) fn authorize_local_source(
    home: &HomeLayout,
    request: &DaemonRequest,
) -> Result<String, OpError> {
    let group_id = required_arg(request, "group_id")?;
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(&group_id))
        .map_err(OpError::not_found)?;
    let by = request
        .args
        .get("by")
        .and_then(Value::as_str)
        .unwrap_or("user");
    cccc_core::permissions::require_group_member(&group, by)
        .map_err(|error| OpError::new("permission_denied", error.to_string()))?;
    Ok(group_id)
}
