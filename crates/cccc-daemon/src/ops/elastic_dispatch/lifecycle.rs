use cccc_contracts::{Actor, ActorRole, DaemonRequest, ElasticActorLease, utc_now};
use cccc_core::{GroupDoc, HomeLayout, actors};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, store, string_arg};

const DEFAULT_MAX_PARALLEL_PEERS: usize = 8;
const HARD_MAX_PARALLEL_PEERS: usize = 32;

pub(super) fn provision_elastic_peer(
    home: &HomeLayout,
    group: &GroupDoc,
    request: &DaemonRequest,
    by: &str,
    owner: &str,
) -> Result<Actor, OpError> {
    let max = request
        .args
        .get("max_parallel_peers")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(DEFAULT_MAX_PARALLEL_PEERS)
        .clamp(1, HARD_MAX_PARALLEL_PEERS);
    let peers = group
        .actors
        .iter()
        .filter(|actor| {
            actor.enabled
                && actor.internal_kind.is_none()
                && actors::effective_role(group, &actor.id) == Some(ActorRole::Peer)
        })
        .count();
    if peers >= max {
        return Err(OpError::new(
            "dispatch_capacity_exhausted",
            format!("all peers are busy and max_parallel_peers={max}"),
        ));
    }
    let template = template_actor(group, request)?;
    let mut actor = template.clone();
    actor.id = format!("elastic-{}", &Uuid::new_v4().simple().to_string()[..8]);
    actor.role = None;
    actor.title = "Elastic peer".into();
    actor.enabled = true;
    actor.internal_kind = None;
    actor.elastic_lease = Some(ElasticActorLease {
        owner_actor_id: owner.into(),
        task_id: String::new(),
    });
    actor.created_at = utc_now();
    actor.updated_at = actor.created_at.clone();
    let secrets = if actor.profile_id.is_empty() {
        Some(super::super::actor_secrets::values(
            home,
            &group.group_id,
            &template.id,
        )?)
    } else {
        None
    };
    let mut args = Map::from_iter([
        ("group_id".into(), json!(group.group_id)),
        ("by".into(), json!(by)),
        (
            "actor".into(),
            serde_json::to_value(&actor).map_err(OpError::invalid)?,
        ),
    ]);
    if let Some(secrets) = secrets {
        args.insert("env_private".into(), json!(secrets));
    }
    actor_operation(home, "actor_add", args)?;
    let start = actor_operation(
        home,
        "actor_start",
        Map::from_iter([
            ("group_id".into(), json!(group.group_id)),
            ("actor_id".into(), json!(actor.id)),
            ("by".into(), json!(by)),
        ]),
    );
    if let Err(mut error) = start {
        if let Err(cleanup) = remove_actor(home, &group.group_id, &actor.id, by) {
            error.details.insert(
                "cleanup_error".into(),
                json!({"code":cleanup.code,"message":cleanup.message}),
            );
        }
        return Err(error);
    }
    Ok(actor)
}

pub(super) fn set_elastic_task(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    task_id: &str,
) -> Result<(), OpError> {
    store(home)?
        .mutate(group_id, |group| {
            let actor = group
                .actors
                .iter_mut()
                .find(|actor| actor.id == actor_id)
                .ok_or_else(|| std::io::Error::other("actor not found"))?;
            let lease = actor
                .elastic_lease
                .as_mut()
                .ok_or_else(|| std::io::Error::other("actor is not elastic"))?;
            lease.task_id = task_id.into();
            actor.updated_at = utc_now();
            Ok(())
        })
        .map_err(OpError::io)
}

pub(super) fn remove_actor(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    by: &str,
) -> OpResult {
    actor_operation(
        home,
        "actor_remove",
        Map::from_iter([
            ("group_id".into(), json!(group_id)),
            ("actor_id".into(), json!(actor_id)),
            ("by".into(), json!(by)),
        ]),
    )
}

fn template_actor<'a>(group: &'a GroupDoc, request: &DaemonRequest) -> Result<&'a Actor, OpError> {
    if let Some(actor_id) = string_arg(request, "template_actor_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        return group
            .actors
            .iter()
            .find(|actor| actor.id == actor_id && actor.internal_kind.is_none())
            .ok_or_else(|| {
                OpError::new(
                    "template_actor_not_found",
                    format!("template actor not found: {actor_id}"),
                )
            });
    }
    let requested_runtime = string_arg(request, "runtime")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let matches_runtime = |actor: &&Actor| {
        requested_runtime.as_deref().is_none_or(|wanted| {
            serde_json::to_value(actor.runtime)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .as_deref()
                == Some(wanted)
        })
    };
    group
        .actors
        .iter()
        .find(|actor| {
            actor.internal_kind.is_none()
                && actors::effective_role(group, &actor.id) == Some(ActorRole::Peer)
                && matches_runtime(actor)
        })
        .or_else(|| {
            group.actors.iter().find(|actor| {
                actor.internal_kind.is_none()
                    && actors::effective_role(group, &actor.id) == Some(ActorRole::Foreman)
                    && matches_runtime(actor)
            })
        })
        .ok_or_else(|| {
            OpError::new(
                "template_actor_not_found",
                "group has no compatible actor to clone",
            )
        })
}

fn actor_operation(home: &HomeLayout, op: &str, args: Map<String, Value>) -> OpResult {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args,
    };
    super::super::actors::handle(home, &request).ok_or_else(|| {
        OpError::new(
            "dispatch_internal_error",
            format!("actor operation is unavailable: {op}"),
        )
    })?
}
