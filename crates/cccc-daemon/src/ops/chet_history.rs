//! Private durable facts for the global assistant. These operations record
//! evidence; they do not start a Runtime or replay an input.
use super::operation::{Operation, Policy::ResourceOwned};
use crate::dispatch::{OpError, OpResult, object};
use cccc_contracts::DaemonRequest;
use cccc_contracts::chet::{GroupInput, GroupInputSender};
use cccc_core::chet_history::{
    self, Outcome, Receipt, Registration, StartFailure, Store, TextRequest, Transcript,
    VoiceDelegation,
};
use cccc_core::{HomeLayout, actors, connect_groups, ledger};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

const MAX_SOURCE_TEXT_BYTES: usize = 8 * 1024;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "chet_history_list" => Operation::new(ResourceOwned, list),
        "chet_history_find" => Operation::new(ResourceOwned, find),
        "chet_history_register" => Operation::new(ResourceOwned, register),
        "chet_history_group_input" => Operation::new(ResourceOwned, group_input),
        "chet_history_claim_group_input" => Operation::new(ResourceOwned, claim_group_input),
        "chet_history_voice_delegation" => Operation::new(ResourceOwned, voice_delegation),
        "chet_history_receipt" => Operation::new(ResourceOwned, receipt),
        "chet_history_result" => Operation::new(ResourceOwned, result),
        "chet_history_not_started" => Operation::new(ResourceOwned, not_started),
        "chet_history_transcript" => Operation::new(ResourceOwned, transcript),
        _ => return None,
    })
}

fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.get("by").and_then(Value::as_str) != Some("user")
        || request.args.contains_key("_cccc_native_binding")
        || request.args.contains_key("_cccc_native_source_binding")
        || request.args.contains_key("_cccc_web_binding")
        || request.args.contains_key("group_id")
    {
        return Err(OpError::new(
            "permission_denied",
            "user authority is required",
        ));
    }
    Ok(())
}

fn value<T: DeserializeOwned>(request: &DaemonRequest, key: &str) -> Result<T, OpError> {
    let value = request
        .args
        .get(key)
        .ok_or_else(|| OpError::invalid(format!("{key} is required")))?;
    serde_json::from_value(value.clone()).map_err(OpError::invalid)
}

fn map_error(error: chet_history::Error) -> OpError {
    let code = match &error {
        chet_history::Error::Invalid(_) => "invalid_args",
        chet_history::Error::IdConflict => "chet_history_id_conflict",
        chet_history::Error::OutcomeConflict => "chet_history_outcome_conflict",
        chet_history::Error::ReceiptConflict => "chet_history_receipt_conflict",
        chet_history::Error::UnknownInput => "chet_history_unknown_input",
        chet_history::Error::Corrupt => "chet_history_corrupt",
        chet_history::Error::Rejected(_) => "chet_history_unavailable",
        chet_history::Error::Unknown(_) => "chet_history_unknown",
    };
    OpError::new(code, error.to_string())
}

fn registration(value: Registration) -> &'static str {
    match value {
        Registration::Inserted => "inserted",
        Registration::Existing => "existing",
    }
}

fn resolve_sources(request: &DaemonRequest) -> Result<bool, OpError> {
    match request.args.get("resolve_sources") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(OpError::invalid("resolve_sources must be a boolean")),
    }
}

/// Sources remain in their Group ledgers. A missing, inaccessible, or replaced
/// Group is deliberately indistinguishable in this read-only UI projection.
fn sources(home: &HomeLayout, records: &[chet_history::Record]) -> Map<String, Value> {
    let mut by_group = BTreeMap::<&str, Vec<(String, &GroupInput)>>::new();
    let mut result = Map::new();
    for record in records {
        if let chet_history::Record::GroupInput { id, input, .. } = record {
            let id = id.to_string();
            result.insert(id.clone(), json!({"status":"unavailable"}));
            by_group
                .entry(&input.group_id)
                .or_default()
                .push((id, input));
        }
    }
    if by_group.is_empty() {
        return result;
    }
    let Ok(groups) = cccc_core::GroupStore::new(home.clone()) else {
        return result;
    };
    for (group_id, inputs) in by_group {
        let Ok(group) = groups.load(group_id) else {
            continue;
        };
        let generation = connect_groups::generation(&group);
        let Ok(path) = groups.ledger_path(group_id) else {
            continue;
        };
        let projected = ledger::inspect(&path, |events, positions| {
            let requested = inputs
                .iter()
                .filter(|(_, input)| input.group_generation == generation)
                .filter_map(|(_, input)| {
                    positions
                        .get(&input.event_id)
                        .map(|position| (input.event_id.as_str(), (*input, *position)))
                })
                .collect::<HashMap<_, _>>();
            let first = requested
                .values()
                .map(|(_, position)| *position)
                .min()
                .unwrap_or(events.len());
            let mut replies = HashMap::<&str, &str>::new();
            for (position, reply) in events.iter().enumerate().skip(first.saturating_add(1)) {
                let Some(reply_to) = reply.data.get("reply_to").and_then(Value::as_str) else {
                    continue;
                };
                let Some((input, source_position)) = requested.get(reply_to) else {
                    continue;
                };
                let sender = match &input.sender {
                    GroupInputSender::User => "user",
                    GroupInputSender::Actor { actor_id, .. } => actor_id.as_str(),
                };
                if position > *source_position
                    && !replies.contains_key(reply_to)
                    && actors::has_current_chet_provenance(&group, reply)
                    && reply
                        .data
                        .get("to")
                        .and_then(Value::as_array)
                        .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(sender)))
                {
                    replies.insert(reply_to, &reply.id);
                }
            }
            inputs
                .iter()
                .filter_map(|(id, input)| {
                    if input.group_generation != generation {
                        return None;
                    }
                    let event = positions
                        .get(&input.event_id)
                        .and_then(|position| events.get(*position))?;
                    let sender = match &input.sender {
                        GroupInputSender::User => "user",
                        GroupInputSender::Actor { actor_id, .. } => actor_id.as_str(),
                    };
                    if event.kind != "chat.message"
                        || event.group_id != group_id
                        || event.by != sender
                        || event.data.get("to") != Some(&json!([actors::CHET_SERVICE_PRINCIPAL]))
                    {
                        return None;
                    }
                    let text = event.data.get("text").and_then(Value::as_str).unwrap_or("");
                    let mut end = text.len().min(MAX_SOURCE_TEXT_BYTES);
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    Some((id.clone(), json!({
                        "status":"available",
                        "group_title":group.title,
                        "sender_title":event.data.get("sender_title").and_then(Value::as_str).unwrap_or(""),
                        "text":&text[..end],
                        "sent_at":event.ts,
                        "has_attachments":event.data.get("attachments").and_then(Value::as_array)
                            .is_some_and(|items| !items.is_empty()),
                        "truncated":end < text.len(),
                        "reply_event_id":replies.get(input.event_id.as_str()).copied(),
                    })))
                })
                .collect::<Vec<_>>()
        });
        let Ok(projected) = projected else {
            continue;
        };
        // The Group can be removed or imported while this ResourceOwned read
        // runs. Never apply an old title or ledger to a known new generation.
        if !groups
            .load(group_id)
            .is_ok_and(|current| connect_groups::generation(&current) == generation)
        {
            continue;
        }
        result.extend(projected);
    }
    result
}

/// A forward page may contain only a later fact, not the input itself.
/// Resolve all of its Group references in a single journal scan.
fn page_sources(
    home: &HomeLayout,
    records: &[chet_history::Record],
) -> Result<Map<String, Value>, OpError> {
    use chet_history::Record;
    let mut inputs = Vec::new();
    let mut present = HashSet::new();
    let mut referenced = HashSet::new();
    for record in records {
        match record {
            Record::GroupInput { id, .. } => {
                present.insert(*id);
                inputs.push(record.clone());
            }
            Record::Input { id, .. } | Record::VoiceDelegation { id, .. } => {
                present.insert(*id);
            }
            Record::GroupDispatch { input_id, .. }
            | Record::Receipt { input_id, .. }
            | Record::NotStarted { input_id, .. } => {
                referenced.insert(*input_id);
            }
            Record::Result { input_ids, .. } => referenced.extend(input_ids.iter().copied()),
            _ => {}
        }
    }
    referenced.retain(|id| !present.contains(id));
    if !referenced.is_empty() {
        inputs.extend(
            Store::new(home.root())
                .find_group_inputs(&referenced)
                .map_err(map_error)?,
        );
    }
    Ok(sources(home, &inputs))
}

fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let with_sources = resolve_sources(request)?;
    let limit = match request.args.get("limit") {
        None | Some(Value::Null) => 50,
        Some(_) => value(request, "limit")?,
    };
    let before = match request.args.get("before") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            value::<String>(request, "before")?
                .parse::<u64>()
                .map_err(OpError::invalid)?,
        ),
    };
    let after = match request.args.get("after") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            value::<String>(request, "after")?
                .parse::<u64>()
                .map_err(OpError::invalid)?,
        ),
    };
    if before.is_some() && after.is_some() {
        return Err(OpError::invalid("before and after cannot be combined"));
    }
    if let Some(after) = after {
        let page = Store::new(home.root())
            .after(after, limit)
            .map_err(map_error)?;
        let projection = if with_sources {
            Some(page_sources(home, &page.records)?)
        } else {
            None
        };
        let mut result =
            json!({"records":page.records,"after":page.next.to_string(),"has_more":page.has_more})
                .as_object()
                .expect("fixed object")
                .clone();
        if let Some(projection) = projection {
            result.insert("sources".into(), Value::Object(projection));
        }
        return object(Value::Object(result));
    }
    let page = Store::new(home.root())
        .page(before, limit)
        .map_err(map_error)?;
    let projection = if with_sources {
        Some(page_sources(home, &page.records)?)
    } else {
        None
    };
    let mut result = json!({"records":page.records,"before":page.before.map(|n| n.to_string()),"after":page.head.to_string()})
        .as_object().expect("fixed object").clone();
    if let Some(projection) = projection {
        result.insert("sources".into(), Value::Object(projection));
    }
    object(Value::Object(result))
}

fn find(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let with_sources = resolve_sources(request)?;
    let id: Uuid = value(request, "id")?;
    let records = Store::new(home.root()).find_input(id).map_err(map_error)?;
    let projection = if with_sources {
        Some(page_sources(home, records.as_deref().unwrap_or_default())?)
    } else {
        None
    };
    let mut result = json!({"records":records})
        .as_object()
        .expect("fixed object")
        .clone();
    if let Some(projection) = projection {
        result.insert("sources".into(), Value::Object(projection));
    }
    object(Value::Object(result))
}

fn register(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input: TextRequest = value(request, "request")?;
    let status = Store::new(home.root())
        .register_input(id, input)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn group_input(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let input: GroupInput = value(request, "input")?;
    let groups = cccc_core::GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = match groups.load(&input.group_id) {
        Ok(group) => group,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Deletion first renames the Group and can still roll back while
            // it remains registered. Treat that interval as unavailable, not
            // as a permanent deletion.
            if groups
                .list()
                .map_err(OpError::io)?
                .iter()
                .any(|group| group.group_id == input.group_id)
            {
                return Err(OpError::io(error));
            }
            return Err(OpError::new("group_not_found", "Group not found"));
        }
        Err(error) => return Err(OpError::io(error)),
    };
    if connect_groups::generation(&group) != input.group_generation {
        return Err(OpError::new("group_changed", "Group generation changed"));
    }
    let path = groups.ledger_path(&group.group_id).map_err(OpError::io)?;
    let (valid, unavailable) = ledger::inspect_status(&path, |events, positions, generations| {
        let Some(index) = positions.get(&input.event_id).copied() else {
            return (false, true);
        };
        let Some(event) = events.get(index) else {
            return (false, true);
        };
        // Imported events remain visible history but are not fresh work in
        // this Group generation.
        if events
            .iter()
            .rposition(|event| {
                event.kind == "group.create"
                    && event.data.get("imported") == Some(&Value::Bool(true))
            })
            .is_some_and(|imported| index <= imported)
        {
            return (false, true);
        }
        if event.kind != "chat.message"
            || event.data.get("to") != Some(&json!([actors::CHET_SERVICE_PRINCIPAL]))
            || !matches!(
                event.data.get("message_mode").and_then(Value::as_str),
                Some("send" | "request_reply")
            )
            || event.data.get("transport").and_then(Value::as_str) == Some("im")
            || event
                .data
                .get("reply_to")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                != input.reply_to.as_deref()
        {
            return (false, true);
        }
        match &input.sender {
            GroupInputSender::User => (event.by == "user", event.by != "user"),
            GroupInputSender::Actor {
                actor_id,
                actor_generation,
            } => {
                if event.by != *actor_id {
                    return (false, true);
                }
                // actor_add saves the Group before appending its ledger event.
                // While it is in flight, an older actor.add can still be the
                // last registration for this ID. Do not attribute that old
                // Actor's input to the replacement. Some older ledgers lack a
                // usable generation, so retain their existing behavior.
                let matches_recorded = generations
                    .get(actor_id)
                    .and_then(|position| events.get(*position))
                    .and_then(|event| event.data.get("actor"))
                    .and_then(|value| {
                        serde_json::from_value::<cccc_contracts::Actor>(value.clone()).ok()
                    })
                    .filter(|actor| !actor.generation.is_empty() || !actor.created_at.is_empty())
                    .is_none_or(|recorded| {
                        actors::generation_identity(&recorded) == *actor_generation
                    });
                let valid = actors::find(&group, actor_id).is_some_and(|actor| {
                    actor.internal_kind.is_none()
                        && actors::generation_identity(actor) == *actor_generation
                }) && index >= generations.get(actor_id).copied().unwrap_or(0)
                    && matches_recorded;
                // Group mutations publish their document before recording
                // actor.add/remove and may roll back on failure. A changed or
                // missing member alone cannot establish a terminal failure.
                let retired = !valid
                    && events.iter().skip(index + 1).any(|later| {
                        if later.kind == "actor.remove" {
                            return later.data.get("actor_id").and_then(Value::as_str)
                                == Some(actor_id.as_str());
                        }
                        later.kind == "actor.add"
                            && later
                                .data
                                .get("actor")
                                .and_then(|value| {
                                    serde_json::from_value::<cccc_contracts::Actor>(value.clone())
                                        .ok()
                                })
                                .filter(|actor| {
                                    actor.id == *actor_id
                                        && (!actor.generation.is_empty()
                                            || !actor.created_at.is_empty())
                                })
                                .is_some_and(|actor| {
                                    actors::generation_identity(&actor) != *actor_generation
                                })
                    });
                (valid, retired)
            }
        }
    })
    .map_err(OpError::io)?;
    if !valid {
        return Err(if unavailable {
            OpError::new(
                "chet_source_unavailable",
                "Chet input source is unavailable",
            )
        } else {
            OpError::new(
                "invalid_chet_source",
                "Chet input no longer matches a visible Group message",
            )
        });
    }
    let (id, status) = Store::new(home.root())
        .register_group_input(input)
        .map_err(map_error)?;
    object(json!({"id":id,"registration":registration(status)}))
}

fn claim_group_input(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "input_id")?;
    let status = Store::new(home.root())
        .claim_group_input(id)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn result(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input_ids: Vec<Uuid> = value(request, "input_ids")?;
    let outcome: Outcome = value(request, "outcome")?;
    let status = Store::new(home.root())
        .record_result(id, input_ids, outcome)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn voice_delegation(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let delegation: VoiceDelegation = value(request, "delegation")?;
    let status = Store::new(home.root())
        .record_voice_delegation(id, delegation)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn receipt(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input_id: Uuid = value(request, "input_id")?;
    let receipt: Receipt = value(request, "receipt")?;
    let status = Store::new(home.root())
        .record_receipt(id, input_id, receipt)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn not_started(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input_id: Uuid = value(request, "input_id")?;
    let reason: StartFailure = value(request, "reason")?;
    let status = Store::new(home.root())
        .record_not_started(id, input_id, reason)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn transcript(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let update: Transcript = value(request, "transcript")?;
    let status = Store::new(home.root())
        .record_transcript(id, update)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
        crate::dispatch::dispatch(
            home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("fixture object").clone(),
            },
        )
    }

    #[test]
    fn history_rejects_bound_and_non_user_callers() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home layout");
        for args in [
            json!({}),
            json!({"by":"actor"}),
            json!({"by":"user","_cccc_native_binding":{}}),
            json!({"by":"user","_cccc_web_binding":{}}),
        ] {
            let response = call(&home, "chet_history_list", args);
            assert_eq!(response.error.expect("denial").code, "permission_denied");
        }
    }

    #[test]
    fn ipc_registration_is_idempotent_and_history_is_read_only() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home layout");
        home.initialize().expect("initialize");
        let empty = call(&home, "chet_history_list", json!({"by":"user"}));
        assert!(empty.ok);
        assert!(!temp.path().join("state/chet").exists());
        let id = Uuid::new_v4();
        let args = json!({"by":"user","id":id,"request":{"text":"question","context":null}});
        let first = call(&home, "chet_history_register", args.clone());
        let retry = call(&home, "chet_history_register", args);
        assert_eq!(first.result["registration"], "inserted");
        assert_eq!(retry.result["registration"], "existing");
        let history = call(&home, "chet_history_list", json!({"by":"user"}));
        assert_eq!(
            history.result["records"].as_array().expect("records").len(),
            1
        );
    }

    #[test]
    fn group_input_registration_verifies_ledger_source_and_generation() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home layout");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        let mut group = groups.create("Chet input", "").expect("group");
        let mut actor = cccc_contracts::Actor::new("worker");
        actor.runtime = cccc_contracts::ActorRuntime::WebModel;
        group.actors.push(actor.clone());
        groups.save(&group).expect("save");
        let sent = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"worker",
            "to":["@chet"],"text":"Please review","message_mode":"send"}),
        );
        assert!(sent.ok, "{sent:?}");
        let input = GroupInput {
            group_id: group.group_id.clone(),
            group_generation: connect_groups::generation(&group),
            event_id: sent.result["event"]["id"]
                .as_str()
                .expect("event ID")
                .into(),
            sender: GroupInputSender::Actor {
                actor_id: "worker".into(),
                actor_generation: actors::generation_identity(&actor),
            },
            reply_to: None,
        };
        let args = json!({"by":"user","input":input});
        let inserted = call(&home, "chet_history_group_input", args.clone());
        let existing = call(&home, "chet_history_group_input", args);
        assert_eq!(inserted.result["registration"], "inserted");
        assert_eq!(existing.result["registration"], "existing");
        assert_eq!(inserted.result["id"], existing.result["id"]);
        let mut invalid = input.clone();
        invalid.sender = GroupInputSender::User;
        assert!(
            !call(
                &home,
                "chet_history_group_input",
                json!({"by":"user","input":invalid})
            )
            .ok
        );
        let mut marker = cccc_contracts::Event::new("group.create", &group.group_id);
        marker.data.insert("imported".into(), json!(true));
        ledger::append(
            &groups.ledger_path(&group.group_id).expect("ledger"),
            &marker,
        )
        .expect("marker");
        assert!(
            !call(
                &home,
                "chet_history_group_input",
                json!({"by":"user","input":input})
            )
            .ok
        );
        let after = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"worker",
            "to":["@chet"],"text":"New request","message_mode":"send"}),
        );
        let mut post_import = input.clone();
        post_import.event_id = after.result["event"]["id"]
            .as_str()
            .expect("event ID")
            .into();
        assert!(
            call(
                &home,
                "chet_history_group_input",
                json!({"by":"user","input":post_import})
            )
            .ok
        );
        let mut invalid = input;
        invalid.group_generation = "prior-generation".into();
        assert!(
            !call(
                &home,
                "chet_history_group_input",
                json!({"by":"user","input":invalid})
            )
            .ok
        );
    }

    #[test]
    fn group_input_rejects_a_replaced_actor_before_its_add_event_is_written() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home layout");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        let mut group = groups.create("Chet input", "").expect("group");
        let mut original = cccc_contracts::Actor::new("worker");
        original.generation = "original-generation".into();
        original.runtime = cccc_contracts::ActorRuntime::WebModel;
        group.actors.push(original.clone());
        groups.save(&group).expect("save original");
        let path = groups.ledger_path(&group.group_id).expect("ledger");
        let mut add = cccc_contracts::Event::new("actor.add", &group.group_id);
        add.data.insert("actor".into(), json!(original));
        ledger::append(&path, &add).expect("record original");
        let old = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"worker",
            "to":["@chet"],"text":"Original request","message_mode":"send"}),
        );
        assert!(old.ok, "{old:?}");

        // Group configuration is written before the add event. Simulate a
        // crash between those writes without fabricating a new ledger fact.
        group.actors[0].generation = "replacement-generation".into();
        groups.save(&group).expect("save replacement");
        let mut input = GroupInput {
            group_id: group.group_id.clone(),
            group_generation: connect_groups::generation(&group),
            event_id: old.result["event"]["id"].as_str().expect("event").into(),
            sender: GroupInputSender::Actor {
                actor_id: "worker".into(),
                actor_generation: "replacement-generation".into(),
            },
            reply_to: None,
        };
        let stale = call(
            &home,
            "chet_history_group_input",
            json!({"by":"user","input":input}),
        );
        assert!(
            !stale.ok,
            "an old message must not be attributed to the replacement"
        );

        let mut add = cccc_contracts::Event::new("actor.add", &group.group_id);
        add.data.insert("actor".into(), json!(group.actors[0]));
        ledger::append(&path, &add).expect("record replacement");
        let new = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"worker",
            "to":["@chet"],"text":"New request","message_mode":"send"}),
        );
        assert!(new.ok, "{new:?}");
        input.event_id = new.result["event"]["id"].as_str().expect("event").into();
        let valid = call(
            &home,
            "chet_history_group_input",
            json!({"by":"user","input":input}),
        );
        assert!(
            valid.ok,
            "current registration must remain usable: {valid:?}"
        );
    }

    #[test]
    fn history_source_projection_is_bounded_and_refuses_a_replaced_group() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home layout");
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        let mut group = groups.create("Source group", "").expect("group");
        let mut actor = cccc_contracts::Actor::new("worker");
        actor.title = "Worker title".into();
        actor.runtime = cccc_contracts::ActorRuntime::WebModel;
        group.actors.push(actor.clone());
        let mut other = cccc_contracts::Actor::new("other");
        other.runtime = cccc_contracts::ActorRuntime::WebModel;
        group.actors.push(other);
        groups.save(&group).expect("save");
        let mut added = cccc_contracts::Event::new("actor.add", &group.group_id);
        added.data.insert("actor".into(), json!(actor));
        ledger::append(
            &groups.ledger_path(&group.group_id).expect("ledger"),
            &added,
        )
        .expect("legacy actor registration");
        // The four-byte character crosses the projection's byte boundary.
        let text = format!("{}😀 tail", "a".repeat(8191));
        let sent = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"worker","to":["@chet"],
                   "text":text,"message_mode":"send"}),
        );
        assert!(sent.ok, "{sent:?}");
        let input = GroupInput {
            group_id: group.group_id.clone(),
            group_generation: connect_groups::generation(&group),
            event_id: sent.result["event"]["id"]
                .as_str()
                .expect("event ID")
                .into(),
            sender: GroupInputSender::Actor {
                actor_id: actor.id.clone(),
                actor_generation: actors::generation_identity(&actor),
            },
            reply_to: None,
        };
        let registered = call(
            &home,
            "chet_history_group_input",
            json!({"by":"user","input":input}),
        );
        assert!(registered.ok, "{registered:?}");
        let input_id = registered.result["id"].as_str().expect("input ID");
        let second = call(
            &home,
            "send",
            json!({"group_id":group.group_id,"by":"worker",
            "to":["@chet"],"text":"Second source","message_mode":"send"}),
        );
        assert!(second.ok, "{second:?}");
        let mut second_input = input.clone();
        second_input.event_id = second.result["event"]["id"]
            .as_str()
            .expect("event ID")
            .into();
        let second_registered = call(
            &home,
            "chet_history_group_input",
            json!({"by":"user","input":second_input}),
        );
        assert!(second_registered.ok, "{second_registered:?}");
        let second_id = second_registered.result["id"]
            .as_str()
            .expect("second input ID");

        let plain = call(&home, "chet_history_list", json!({"by":"user"}));
        let cursor = plain.result["after"]
            .as_str()
            .expect("history cursor")
            .to_owned();
        assert!(
            !plain.result.contains_key("sources"),
            "existing callers stay unchanged"
        );
        for (op, args) in [
            (
                "chet_history_list",
                json!({"by":"user","resolve_sources":true}),
            ),
            (
                "chet_history_list",
                json!({"by":"user","after":"0","resolve_sources":true}),
            ),
            (
                "chet_history_find",
                json!({"by":"user","id":input_id,"resolve_sources":true}),
            ),
        ] {
            let projected = call(&home, op, args);
            assert!(projected.ok, "{projected:?}");
            let source = &projected.result["sources"][input_id];
            assert_eq!(source["status"], "available");
            assert_eq!(source["group_title"], "Source group");
            assert_eq!(source["sender_title"], "Worker title");
            assert_eq!(source["text"].as_str().expect("text").len(), 8191);
            assert_eq!(source["truncated"], true);
            assert_eq!(source["has_attachments"], false);
            assert!(source["reply_event_id"].is_null());
        }
        let invalid = call(
            &home,
            "chet_history_list",
            json!({"by":"user","resolve_sources":"yes"}),
        );
        assert_eq!(invalid.error.expect("error").code, "invalid_args");

        let origin = "f".repeat(32);
        cccc_core::voice_notifications::register_origin(
            &home,
            &origin,
            "analyst",
            "thread",
            cccc_contracts::ActorRuntime::Codex,
        )
        .expect("origin");
        let elsewhere = call(
            &home,
            "reply",
            json!({"group_id":group.group_id,"by":"user","_voice_origin":origin,
                   "reply_to":input.event_id,"to":["other"],"text":"For someone else"}),
        );
        assert!(elsewhere.ok, "{elsewhere:?}");
        let no_reply = call(
            &home,
            "chet_history_find",
            json!({"by":"user","id":input_id,"resolve_sources":true}),
        );
        assert!(no_reply.result["sources"][input_id]["reply_event_id"].is_null());
        let reply = call(
            &home,
            "reply",
            json!({"group_id":group.group_id,"by":"user","_voice_origin":origin,
                   "reply_to":input.event_id,"text":"A direct answer"}),
        );
        assert!(reply.ok, "{reply:?}");
        let result = call(
            &home,
            "chet_history_result",
            json!({"by":"user","id":Uuid::new_v4(),
            "input_ids":[input_id,second_id],"outcome":{"generation":"analyst","turn_id":"turn",
                "status":"completed","text":"A direct answer","error":""}}),
        );
        assert!(result.ok, "{result:?}");
        let update = call(
            &home,
            "chet_history_list",
            json!({"by":"user","after":cursor,
            "resolve_sources":true}),
        );
        assert!(update.ok, "{update:?}");
        assert_eq!(
            update.result["records"].as_array().expect("records").len(),
            1
        );
        assert_eq!(update.result["records"][0]["kind"], "result");
        assert_eq!(
            update.result["sources"][input_id]["reply_event_id"], reply.result["event"]["id"],
            "an incremental result refreshes the source without reloading the panel"
        );
        assert_eq!(update.result["sources"][second_id]["text"], "Second source");
        let replied = call(
            &home,
            "chet_history_find",
            json!({"by":"user","id":input_id,"resolve_sources":true}),
        );
        assert_eq!(
            replied.result["sources"][input_id]["reply_event_id"],
            reply.result["event"]["id"]
        );

        group.generation = "replacement".into();
        groups.save(&group).expect("replace generation");
        let stale = call(
            &home,
            "chet_history_find",
            json!({"by":"user","id":input_id,"resolve_sources":true}),
        );
        assert_eq!(
            stale.result["sources"][input_id],
            json!({"status":"unavailable"})
        );
    }
}
