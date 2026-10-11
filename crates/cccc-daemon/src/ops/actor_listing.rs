use cccc_contracts::{Actor, DaemonRequest};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, actors, inbox, ledger};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

use crate::dispatch::OpError;

use super::{actor_runtime_status, runtime_session};

pub(super) fn list(
    home: &HomeLayout,
    group: &GroupDoc,
    request: &DaemonRequest,
) -> Result<Vec<Value>, OpError> {
    let include_internal = bool_arg(request, "include_internal");
    let include_unread = bool_arg(request, "include_unread");
    let actor_ids = group
        .actors
        .iter()
        .filter(|actor| include_internal || actor.internal_kind.is_none())
        .map(|actor| actor.id.clone())
        .collect::<Vec<_>>();
    let unread = if include_unread {
        inbox::list_unread_many(home, group, &actor_ids, 1000)
            .map_err(OpError::io)?
            .into_iter()
            .map(|(actor_id, events)| (actor_id, events.len()))
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };
    group
        .actors
        .iter()
        .filter(|actor| include_internal || actor.internal_kind.is_none())
        .cloned()
        .map(|mut actor| -> Result<Value, OpError> {
            actor.normalize_runtime_constraints();
            actor.role = actors::effective_role(group, &actor.id);
            let status = actor_runtime_status::resolve(group, &actor);
            let mut value = serde_json::to_value(&actor).unwrap_or_else(|_| json!({}));
            if let Some(object) = value.as_object_mut() {
                object.extend(runtime_session::actor_fields(
                    home,
                    &group.group_id,
                    &actor.id,
                ));
                object.insert("running".into(), Value::Bool(status.running));
                let actual = super::local_headless::structured_state(&group.group_id, &actor.id);
                let retained_pty = cccc_runtime::status(&group.group_id, &actor.id)
                    .is_ok_and(|status| status.running);
                if let Some(actual) = actual {
                    object.insert("runtime_state_source".into(), json!("managed_session"));
                    object.insert("runner_effective".into(), actual["runner"].clone());
                    object.insert(
                        "effective_runtime_mode".into(),
                        actual["runtime_mode"].clone(),
                    );
                    object.insert("effective_runtime".into(), actual["runtime"].clone());
                } else if retained_pty {
                    object.insert("runtime_state_source".into(), json!("terminal"));
                    object.insert("runner_effective".into(), json!("pty"));
                    object.insert("effective_runtime_mode".into(), json!("default"));
                }
                object.insert(
                    "pid".into(),
                    status
                        .pid
                        .map_or(Value::Null, |pid| Value::from(u64::from(pid))),
                );
                object.extend(super::working_state::runtime_actor_fields(
                    home,
                    &actor,
                    &group.group_id,
                    status.running,
                ));
                if actor.runtime.is_web_model() {
                    object.extend(web_model_queue_fields(home, group, &actor)?);
                }
                if include_unread {
                    object.insert(
                        "unread_count".into(),
                        Value::from(
                            u64::try_from(unread.get(&actor.id).copied().unwrap_or_default())
                                .unwrap_or(1000),
                        ),
                    );
                }
            }
            Ok(value)
        })
        .collect()
}

fn web_model_queue_fields(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
) -> Result<Map<String, Value>, OpError> {
    let mut fields = Map::from_iter([("web_model_queued_count".into(), Value::from(0))]);
    let state = super::runtime_state::actor_state(home, &group.group_id, &actor.id)?;
    if state.get("status").and_then(Value::as_str) != Some("working") {
        return Ok(fields);
    }
    let active_event_id = state
        .get("latest_event_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let active_turn_id = state
        .get("active_turn_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if active_event_id.is_empty() || active_turn_id.is_empty() {
        return Ok(fields);
    }

    let pending = super::runtime_delivery::pending_sources(home, group, actor, 10_000)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let path = store.ledger_path(&group.group_id).map_err(OpError::io)?;
    ledger::inspect(&path, |events, positions| {
        let Some(active_position) = events.iter().position(|event| event.id == active_event_id)
        else {
            return fields;
        };
        let queued = pending
            .iter()
            .filter(|event| {
                positions
                    .get(event.id.as_str())
                    .is_some_and(|position| *position > active_position)
            })
            .collect::<Vec<_>>();
        fields.insert(
            "web_model_queued_count".into(),
            Value::from(u64::try_from(queued.len()).unwrap_or(u64::MAX)),
        );
        if let Some(latest) = queued.last() {
            fields.insert(
                "web_model_queued_after_event_id".into(),
                Value::String(active_event_id.into()),
            );
            fields.insert(
                "web_model_queued_latest_event_id".into(),
                Value::String(latest.id.clone()),
            );
            fields.insert(
                "web_model_queued_latest_ts".into(),
                Value::String(latest.ts.clone()),
            );
        }
        fields
    })
    .map_err(OpError::io)
}

fn bool_arg(request: &DaemonRequest, name: &str) -> bool {
    request.args.get(name).is_some_and(|value| match value {
        Value::Bool(value) => *value,
        Value::String(value) => matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"),
        _ => false,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use cccc_contracts::{ActorRuntime, RuntimeMode};
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn saved_native_mode_keeps_the_running_managed_state_authority() {
        for runtime in [ActorRuntime::Kimi, ActorRuntime::Auggie] {
            let temp = tempfile::tempdir().expect("fixture directory");
            let home = HomeLayout::from_path(temp.path().join("cccc")).expect("fixture home");
            home.initialize().expect("initialize fixture home");
            let workspace = temp.path().join("workspace");
            std::fs::create_dir(&workspace).expect("fixture workspace");
            let store = GroupStore::new(home.clone()).expect("fixture store");
            let group = store.create("Actual surface", "").expect("fixture group");
            let scope = cccc_core::scope::detect(&workspace).expect("fixture scope");
            let mut group = cccc_core::group_scope::attach(&store, &group.group_id, scope)
                .expect("attach fixture scope");
            let program = temp.path().join(if runtime == ActorRuntime::Kimi {
                "kimi"
            } else {
                "auggie"
            });
            std::fs::write(
                &program,
                include_str!("codex_voice_analyst/native_acp/candidate_fixture.py"),
            )
            .expect("write provider fixture");
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
                .expect("executable provider fixture");
            let mut actor = Actor::new("controlled");
            actor.runtime = runtime;
            actor.runtime_mode = RuntimeMode::Acp;
            actor.command = vec![program.to_string_lossy().into_owned()];
            actor.env = BTreeMap::from([
                (
                    "HOME".into(),
                    temp.path().join("provider").to_string_lossy().into_owned(),
                ),
                (
                    "KIMI_CODE_HOME".into(),
                    temp.path()
                        .join("provider/kimi")
                        .to_string_lossy()
                        .into_owned(),
                ),
                ("ACP_FIXTURE_CLI".into(), "/bin/true".into()),
            ]);
            group.actors.push(actor.clone());
            store.save(&group).expect("save controlled actor");
            super::super::local_headless::start(&home, &group, &actor)
                .expect("start controlled actor");
            struct Stop(String);
            impl Drop for Stop {
                fn drop(&mut self) {
                    let _ = super::super::local_headless::stop(&self.0, "controlled");
                }
            }
            let _stop = Stop(group.group_id.clone());
            group.actors[0].runtime_mode = RuntimeMode::Default;
            store
                .save(&group)
                .expect("save native mode without restarting");
            let request = DaemonRequest {
                v: 1,
                op: "actor_list".into(),
                args: Map::new(),
            };
            let values = list(&home, &group, &request).expect("list actual actor surface");
            assert_eq!(values[0]["runtime_mode"], Value::Null);
            assert_eq!(values[0]["runner"], "pty");
            assert_eq!(values[0]["runner_effective"], "headless");
            assert_eq!(values[0]["effective_runtime_mode"], "acp");
            assert_eq!(values[0]["runtime_state_source"], "managed_session");
            assert_eq!(values[0]["running"], true);
        }
    }
}
