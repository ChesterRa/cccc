use super::operation::{Operation, Policy::Write};
use cccc_contracts::{DaemonRequest, Event};
use cccc_core::active;
use cccc_core::group_scope;
use cccc_core::ledger;
use cccc_core::scope;
use cccc_core::{GroupDoc, HomeLayout, Registry, permissions};
use serde_json::{Value, json};
use std::collections::BTreeSet;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "attach" => Operation::new(Write, attach),
        "group_detach_scope" => Operation::new(Write, detach),
        "group_use" => Operation::new(Write, use_group),
        "registry_reconcile" => Operation::new(Write, reconcile),
        _ => return None,
    })
}

fn attach(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = string_arg(request, "group_id").filter(|id| !id.is_empty());
    if let Some(id) = &group_id {
        require_scope_authority(home, request, id)?;
    }
    let path = string_arg(request, "path").unwrap_or_else(|| ".".into());
    let detected = scope::detect(std::path::Path::new(&path)).map_err(OpError::invalid)?;
    let event_data = json!({
        "url": detected.url.clone(),
        "label": detected.label.clone(),
        "git_remote": detected.git_remote.clone(),
    });
    let group = if let Some(id) = group_id {
        group_scope::attach(&store(home)?, &id, detected).map_err(OpError::io)?
    } else {
        let created = store(home)?
            .create(&detected.label, "")
            .map_err(OpError::io)?;
        group_scope::attach(&store(home)?, &created.group_id, detected).map_err(OpError::io)?
    };
    append_scope_event(
        home,
        &group,
        "group.attach",
        &group.active_scope_key,
        request,
        event_data,
    )?;
    active::set(home, &group.group_id).map_err(OpError::io)?;
    object(json!({
        "group_id":group.group_id,
        "scope_key":group.active_scope_key,
        "title":group.title,
    }))
}

fn detach(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let scope_key = required_arg(request, "scope_key")?;
    require_scope_authority(home, request, &group_id)?;
    let group = group_scope::detach(&store(home)?, &group_id, &scope_key).map_err(OpError::io)?;
    let event = append_scope_event(
        home,
        &group,
        "group.detach_scope",
        &scope_key,
        request,
        json!({"scope_key":scope_key}),
    )?;
    object(json!({"group_id":group.group_id,"event":event}))
}

fn use_group(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = required_arg(request, "path")?;
    require_scope_authority(home, request, &group_id)?;
    let detected = scope::detect(std::path::Path::new(&path)).map_err(OpError::invalid)?;
    let updated =
        group_scope::activate(&store(home)?, &group_id, &detected.scope_key).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                OpError::new("scope_not_attached", error.to_string())
            } else {
                OpError::io(error)
            }
        })?;
    let event = append_scope_event(
        home,
        &updated,
        "group.set_active_scope",
        &detected.scope_key,
        request,
        json!({"path":detected.url}),
    )?;
    object(json!({
        "group_id":updated.group_id,
        "active_scope_key":updated.active_scope_key,
        "event":event,
    }))
}

fn require_scope_authority(
    home: &HomeLayout,
    request: &DaemonRequest,
    group_id: &str,
) -> Result<(), OpError> {
    let group = store(home)?.load(group_id).map_err(OpError::not_found)?;
    permissions::require_group(
        &group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )
    .map_err(|error| OpError::new("permission_denied", error.to_string()))
}

fn append_scope_event(
    home: &HomeLayout,
    group: &GroupDoc,
    kind: &str,
    scope_key: &str,
    request: &DaemonRequest,
    data: Value,
) -> Result<Event, OpError> {
    let mut event = Event::new(kind, &group.group_id);
    event.scope_key = scope_key.into();
    event.by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    event.data = data.as_object().cloned().unwrap_or_default();
    ledger::append(
        &store(home)?
            .ledger_path(&group.group_id)
            .map_err(OpError::io)?,
        &event,
    )
    .map_err(OpError::io)?;
    Ok(event)
}

fn reconcile(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let registry = Registry::load(home).map_err(OpError::io)?;
    let scanned_groups = registry.groups.len();
    let mut missing_group_ids = Vec::new();
    let mut corrupt_group_ids = Vec::new();
    let group_store = store(home)?;
    for (group_id, meta) in &registry.groups {
        let document = std::path::Path::new(&meta.path).join("group.yaml");
        if !document.is_file() {
            missing_group_ids.push(group_id.clone());
        } else if group_store.load(group_id).is_err() {
            corrupt_group_ids.push(group_id.clone());
        }
    }
    let remove_missing = request
        .args
        .get("remove_missing")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let mut removed_group_ids = Vec::new();
    let mut removed_default_scope_keys = Vec::new();
    if remove_missing && !missing_group_ids.is_empty() {
        let missing = missing_group_ids.iter().cloned().collect::<BTreeSet<_>>();
        Registry::mutate(home, |registry| {
            for group_id in &missing {
                if registry.groups.remove(group_id).is_some() {
                    removed_group_ids.push(group_id.clone());
                }
            }
            registry.defaults.retain(|scope_key, group_id| {
                if missing.contains(group_id) {
                    removed_default_scope_keys.push(scope_key.clone());
                    false
                } else {
                    true
                }
            });
            Ok(())
        })
        .map_err(OpError::io)?;
    }
    missing_group_ids.sort();
    corrupt_group_ids.sort();
    removed_group_ids.sort();
    removed_default_scope_keys.sort();
    object(json!({
        "dry_run":!remove_missing,
        "scanned_groups":scanned_groups,
        "missing_group_ids":missing_group_ids,
        "corrupt_group_ids":corrupt_group_ids,
        "removed_group_ids":removed_group_ids,
        "removed_default_scope_keys":removed_default_scope_keys,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;

    #[test]
    fn shared_scope_mutations_require_group_authority_before_any_write() {
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let groups = store(&home).expect("store");
        let created = groups.create("scope permissions", "").expect("group");
        groups
            .mutate(&created.group_id, |group| {
                group.actors = vec![Actor::new("lead"), Actor::new("peer")];
                Ok(())
            })
            .expect("actors");
        let projects = ["one", "two", "three"].map(|name| temp.path().join(name));
        for project in &projects {
            std::fs::create_dir(project).expect("project");
        }
        let request = |op: &str, args: Value| DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("args").clone(),
        };
        let one = attach(
            &home,
            &request(
                "attach",
                json!({"group_id":created.group_id,"path":projects[0],"by":"user"}),
            ),
        )
        .expect("user attach");
        attach(
            &home,
            &request(
                "attach",
                json!({"group_id":created.group_id,"path":projects[1],"by":"lead"}),
            ),
        )
        .expect("foreman attach");
        let snapshot = || {
            (
                serde_json::to_value(groups.load(&created.group_id).expect("group"))
                    .expect("group snapshot"),
                serde_json::to_value(Registry::load(&home).expect("registry"))
                    .expect("registry snapshot"),
                std::fs::read(groups.ledger_path(&created.group_id).expect("ledger"))
                    .expect("ledger bytes"),
                active::get(&home).expect("active group"),
            )
        };
        let before = snapshot();
        for by in ["peer", "unknown-actor"] {
            for (op, args) in [
                (
                    "attach",
                    json!({"group_id":created.group_id,"path":projects[2],"by":by}),
                ),
                (
                    "group_use",
                    json!({"group_id":created.group_id,"path":projects[0],"by":by}),
                ),
                (
                    "group_detach_scope",
                    json!({"group_id":created.group_id,"scope_key":one["scope_key"],"by":by}),
                ),
            ] {
                let request = request(op, args);
                let error = resolve_operation(&request)
                    .expect("operation")
                    .execute(&home, &request)
                    .expect_err("unauthorized scope mutation");
                assert_eq!(error.code, "permission_denied", "{op}: {by}");
                assert_eq!(snapshot(), before, "{op}: {by} changed shared state");
            }
        }
        use_group(
            &home,
            &request(
                "group_use",
                json!({"group_id":created.group_id,"path":projects[0],"by":"lead"}),
            ),
        )
        .expect("foreman use");
        detach(
            &home,
            &request(
                "group_detach_scope",
                json!({"group_id":created.group_id,"scope_key":one["scope_key"],"by":"user"}),
            ),
        )
        .expect("user detach");
    }
}
