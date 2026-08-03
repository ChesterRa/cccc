use cccc_contracts::DaemonRequest;
use cccc_core::context::ContextStore;
use cccc_core::{HomeLayout, actors};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

mod lifecycle;
mod selection;
mod task_state;

use lifecycle::{provision_elastic_peer, remove_actor, set_elastic_task};
use selection::{actor_row, authorize_foreman, select_idle_peer};
use task_state::{existing_idempotent_assignee, has_active_task, terminal_task};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "elastic_dispatch" => dispatch(home, request),
        "elastic_release" => release(home, request),
        _ => return None,
    })
}

fn dispatch(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    if request.args.contains_key("dst_group_id") {
        return Err(OpError::new(
            "cross_group_dispatch_unsupported",
            "elastic dispatch only supports the current local group",
        ));
    }
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    authorize_foreman(&group, &by)?;
    let contexts = ContextStore::new(home.clone()).map_err(OpError::io)?;
    let context = contexts.load(&group_id).map_err(OpError::io)?;

    let existing_assignee = existing_idempotent_assignee(&context, &group_id, &by, request);
    let selected = if let Some(actor_id) = existing_assignee {
        group
            .actors
            .iter()
            .find(|actor| actor.id == actor_id)
            .cloned()
            .ok_or_else(|| {
                OpError::new(
                    "dispatch_assignee_missing",
                    format!("idempotent task assignee no longer exists: {actor_id}"),
                )
            })?
    } else if let Some(actor) = select_idle_peer(home, &group, request)? {
        actor
    } else {
        let owner = if matches!(by.as_str(), "user" | "system") {
            actors::unique_available_foreman(&group)
                .map(|actor| actor.id.as_str())
                .unwrap_or(by.as_str())
        } else {
            by.as_str()
        };
        provision_elastic_peer(home, &group, request, &by, owner)?
    };
    let created = selected.elastic_lease.as_ref().is_some_and(|lease| {
        lease.task_id.is_empty() && !group.actors.iter().any(|actor| actor.id == selected.id)
    });

    let mut forwarded = request.clone();
    forwarded.op = "tracked_send".into();
    forwarded.args.remove("action");
    forwarded
        .args
        .insert("to".into(), json!([selected.id.clone()]));
    forwarded
        .args
        .insert("assignee".into(), json!(selected.id.clone()));
    forwarded
        .args
        .entry("waiting_on")
        .or_insert_with(|| json!("actor"));

    let result = match super::messaging::handle(home, &forwarded) {
        Some(result) => result,
        None => Err(OpError::new(
            "dispatch_internal_error",
            "tracked send handler is unavailable",
        )),
    };
    let result = match result {
        Ok(result) => result,
        Err(mut error) => {
            if created {
                if let Err(cleanup) = remove_actor(home, &group_id, &selected.id, &by) {
                    error.details.insert(
                        "cleanup_error".into(),
                        json!({"code":cleanup.code,"message":cleanup.message}),
                    );
                }
            }
            return Err(error);
        }
    };
    let task_id = result
        .get("task_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if selected.elastic_lease.is_some() && !task_id.is_empty() {
        set_elastic_task(home, &group_id, &selected.id, &task_id)?;
    }
    let message_sent = result
        .get("message_sent")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    object(json!({
        "actor_id":selected.id,
        "elastic":selected.elastic_lease.is_some(),
        "created":created,
        "task_id":task_id,
        "message_sent":message_sent,
        "dispatch":result,
    }))
}

fn release(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let actor_id = required_arg(request, "actor_id")?;
    let task_id = required_arg(request, "task_id")?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let group = store(home)?.load(&group_id).map_err(OpError::not_found)?;
    authorize_foreman(&group, &by)?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", format!("actor not found: {actor_id}")))?;
    let Some(lease) = actor.elastic_lease.as_ref() else {
        return object(
            json!({"actor_id":actor_id,"released":false,"retained":true,"reason":"resident_peer"}),
        );
    };
    if !matches!(by.as_str(), "user" | "system") && lease.owner_actor_id != by {
        return Err(OpError::new(
            "permission_denied",
            "elastic peer is owned by another foreman",
        ));
    }
    if lease.task_id != task_id {
        return Err(OpError::new(
            "elastic_lease_mismatch",
            format!("elastic peer is leased to task {}", lease.task_id),
        ));
    }

    let context = ContextStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::io)?;
    let task = context
        .tasks
        .iter()
        .find(|task| task.get("id").and_then(Value::as_str) == Some(task_id.as_str()))
        .ok_or_else(|| OpError::new("task_not_found", format!("task not found: {task_id}")))?;
    if !terminal_task(task) {
        return Err(OpError::new(
            "elastic_task_active",
            "elastic peer can only be released after its task is done or archived",
        ));
    }
    if has_active_task(&context, &actor_id) {
        return Err(OpError::new(
            "elastic_peer_assigned",
            "elastic peer still has another active task",
        ));
    }
    let row = actor_row(home, &group, &actor_id, &by)?;
    let state = row
        .get("effective_working_state")
        .and_then(Value::as_str)
        .unwrap_or("waiting");
    if state == "working" {
        return Err(OpError::new(
            "elastic_peer_busy",
            "elastic peer still reports active work",
        ));
    }
    remove_actor(home, &group_id, &actor_id, &by)?;
    object(json!({"actor_id":actor_id,"task_id":task_id,"released":true,"removed":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::{Actor, ElasticActorLease};
    use cccc_core::GroupStore;
    use cccc_core::context::ContextDoc;
    use serde_json::Map;
    #[test]
    fn active_task_detection_ignores_terminal_tasks() {
        let context = ContextDoc {
            tasks: vec![
                Map::from_iter([
                    ("id".into(), json!("T001")),
                    ("assignee".into(), json!("busy")),
                    ("status".into(), json!("active")),
                ]),
                Map::from_iter([
                    ("id".into(), json!("T002")),
                    ("assignee".into(), json!("done")),
                    ("status".into(), json!("done")),
                ]),
            ],
            ..ContextDoc::default()
        };
        assert!(has_active_task(&context, "busy"));
        assert!(!has_active_task(&context, "done"));
    }

    #[test]
    fn idempotent_dispatch_keeps_original_assignee() {
        let mut context = ContextDoc::default();
        let request = DaemonRequest {
            v: 1,
            op: "elastic_dispatch".into(),
            args: Map::from_iter([("idempotency_key".into(), json!("same"))]),
        };
        let client_id =
            super::super::message_idempotency::tracked_client_id("g", "foreman", "same");
        context.tasks.push(Map::from_iter([
            ("client_request_id".into(), json!(client_id)),
            ("assignee".into(), json!("peer-1")),
        ]));
        assert_eq!(
            existing_idempotent_assignee(&context, "g", "foreman", &request).as_deref(),
            Some("peer-1")
        );
    }

    #[test]
    fn dispatch_rejects_cross_group_target_before_delivery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "elastic_dispatch".into(),
            args: Map::from_iter([
                ("group_id".into(), json!("g_local")),
                ("dst_group_id".into(), json!("g_remote")),
            ]),
        };

        let error = dispatch(&home, &request).expect_err("cross-group dispatch must fail");
        assert_eq!(error.code, "cross_group_dispatch_unsupported");
    }

    #[test]
    fn release_removes_only_terminal_elastic_peer() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let group = groups.create("elastic", "").expect("group");
        groups
            .mutate(&group.group_id, |doc| {
                actors::add(doc, Actor::new("lead"))?;
                let mut peer = Actor::new("elastic-peer");
                peer.enabled = false;
                peer.elastic_lease = Some(ElasticActorLease {
                    owner_actor_id: "lead".into(),
                    task_id: "T001".into(),
                });
                actors::add(doc, peer)?;
                Ok(())
            })
            .expect("actors");
        ContextStore::new(home.clone())
            .expect("contexts")
            .sync(
                &group.group_id,
                &[Map::from_iter([
                    ("op".into(), json!("task.create")),
                    ("title".into(), json!("done")),
                    ("status".into(), json!("done")),
                    ("assignee".into(), json!("elastic-peer")),
                ])],
                None,
                "lead",
                false,
            )
            .expect("task");
        let request = DaemonRequest {
            v: 1,
            op: "elastic_release".into(),
            args: Map::from_iter([
                ("group_id".into(), json!(group.group_id)),
                ("actor_id".into(), json!("elastic-peer")),
                ("task_id".into(), json!("T001")),
                ("by".into(), json!("lead")),
            ]),
        };
        let result = release(&home, &request).expect("release");
        assert_eq!(result["released"], true);
        assert!(
            groups
                .load(&group.group_id)
                .expect("group")
                .actors
                .iter()
                .all(|actor| actor.id != "elastic-peer")
        );
    }
}
