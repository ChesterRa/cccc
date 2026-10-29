//! Resume proven conversations at receive attachment, never during discovery.
use super::{actor_runtime, store::Receiver};
use crate::dispatch::OpError;
use cccc_contracts::{Actor, Event, local_session::LocalSessionRuntime, utc_now};
use cccc_core::{
    GroupDoc, GroupStore, HomeLayout, actors, fs, ledger,
    local_sessions::{self as native, NativeSession, RetireReason},
};
use serde_json::json;
use std::{collections::BTreeMap, io};

fn predecessors(
    home: &HomeLayout,
    current: &NativeSession,
) -> io::Result<BTreeMap<String, NativeSession>> {
    let (records, errors) = native::scan_records(home)?;
    for error in errors {
        tracing::warn!(%error, "Skipping unreadable native recovery metadata");
    }
    let matching = records
        .into_iter()
        .filter(|record| {
            record.source_id != current.source_id
                && record.retired != Some(RetireReason::IntegrationDisabled)
                && current.same_native_conversation(record)
        })
        .collect::<Vec<_>>();
    // Two living hosts can resume the same conversation. Do not choose an
    // owner based on attachment order, or treat an inconclusive lookup as exit.
    if matching.iter().any(|record| !record.host.has_exited()) {
        return Ok(BTreeMap::new());
    }
    Ok(matching
        .into_iter()
        .map(|record| (record.source_id.clone(), record))
        .collect())
}

pub(super) fn restore_memberships(home: &HomeLayout, receiver: &Receiver) -> Result<(), OpError> {
    if !matches!(
        receiver.runtime,
        LocalSessionRuntime::Claude | LocalSessionRuntime::Pi
    ) {
        return Ok(());
    }
    let current = native::validate_source(home, &receiver.source_id, &receiver.revision)
        .map_err(OpError::io)?;
    let candidates = predecessors(home, &current).map_err(OpError::io)?;
    if candidates.is_empty() {
        return Ok(());
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    for meta in store.list().map_err(OpError::io)? {
        let group = match store.load(&meta.group_id) {
            Ok(group) => group,
            Err(error) => {
                tracing::warn!(group_id=%meta.group_id,%error,"Skipping unavailable Group during native conversation recovery");
                continue;
            }
        };
        let matching = group
            .actors
            .iter()
            .filter(|actor| {
                actor.runtime == actor_runtime(current.runtime)
                    && candidates.contains_key(&actor.external_session_id)
            })
            .collect::<Vec<_>>();
        let [previous] = matching.as_slice() else {
            if matching.len() > 1 {
                tracing::warn!(group_id=%group.group_id,"Native recovery has several previous members; explicit user selection is required");
            }
            continue;
        };
        if group
            .actors
            .iter()
            .any(|actor| actor.external_session_id == current.source_id)
        {
            // Explicit admission already chose another Actor. Do not merge it
            // with a former member, transfer tasks, or override paused access.
            continue;
        }
        // receiver_attach already owns the daemon's GlobalWrite permit. The
        // lifecycle file lock additionally fences callbacks/disable from CLI
        // processes, which do not use the daemon's dispatch locks.
        let restored =
            fs::with_exclusive_lock(&home.root().join("state/local_sessions.lock"), || {
                let current =
                    native::validate_source(home, &receiver.source_id, &receiver.revision)?;
                let candidates = predecessors(home, &current)?;
                let Some(old) = candidates.get(&previous.external_session_id) else {
                    return Ok(None);
                };
                restore_group(&store, &group, previous, old, &current).map(Some)
            });
        let restored = match restored {
            Ok(restored) => restored,
            Err(error) => {
                cccc_runtime::diagnostic(format_args!(
                    "Could not restore native conversation membership: group_id={} source_id={} error_kind={:?}; other Groups remain available",
                    group.group_id,
                    receiver.source_id,
                    error.kind()
                ));
                continue;
            }
        };
        if let Some(restored) = restored {
            // Release the lifecycle lock before joining a delivery worker.
            // Its unclaimed slots are rebuilt from the existing ledger; the
            // established claim/terminal receipts are never reset or replayed.
            crate::ops::actor_delivery::shutdown_actor(&restored.group_id, &previous.id);
            crate::ops::actor_delivery::dispatch_unread(home, &restored, &previous.id);
        }
    }
    Ok(())
}

fn restore_group(
    store: &GroupStore,
    group: &GroupDoc,
    previous: &Actor,
    old: &NativeSession,
    current: &NativeSession,
) -> io::Result<GroupDoc> {
    let path = store.ledger_path(&group.group_id)?;
    store.mutate_with_rollback(&group.group_id, |doc| {
        if doc.actors.iter().any(|actor| actor.external_session_id == current.source_id) {
            return Err(io::Error::other("Native recovery membership changed"));
        }
        let actor = doc.actors.iter_mut().find(|actor| {
            actor.id == previous.id
                && actor.runtime == previous.runtime
                && actor.external_session_id == old.source_id
                && actors::generation_identity(actor) == actors::generation_identity(previous)
        }).ok_or_else(|| io::Error::other("Native recovery membership changed"))?;
        actor.external_session_id.clone_from(&current.source_id);
        if actor.title == old.title {
            actor.title.clone_from(&current.title);
        }
        actor.updated_at = utc_now();
        if let Some(ids) = doc.extra.get_mut("local_session_conversation")
            .and_then(|value| value.get_mut("source_ids"))
            .and_then(serde_json::Value::as_array_mut) {
            for id in ids.iter_mut() {
                if id == &old.source_id {*id=json!(current.source_id);}
            }
            ids.sort_by(|left,right| left.as_str().cmp(&right.as_str()));
            ids.dedup();
        }
        Ok(doc.clone())
    }, |doc| {
        let actor = actors::find(doc,&previous.id).expect("restored member");
        let mut patch=json!({"external_session_id":current.source_id});
        if actor.title != previous.title {patch["title"]=json!(actor.title);}
        let mut event=Event::new("actor.update",&doc.group_id);
        event.by="svc:cccc.local-session".into();
        event.scope_key=cccc_core::group_scope::effective_scope(doc,Some(actor)).map(|scope|scope.scope_key.clone()).unwrap_or_default();
        event.data=json!({"actor_id":actor.id,"patch":patch,"previous_external_session_id":old.source_id,"reason":"native_session_resumed"}).as_object().expect("recovery audit").clone();
        ledger::append(&path,&event)
    })
}
