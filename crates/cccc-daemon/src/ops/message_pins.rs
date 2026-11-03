use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use cccc_contracts::message_pins::MessagePin;
use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::{HomeLayout, fs, ledger};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::operation::{
    Operation,
    Policy::{Read, Write},
};
use crate::dispatch::{OpError, OpResult, object, required_arg, store};

const MAX_PINS: usize = 100;
const VERSION: u8 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    v: u8,
    pins: Vec<MessagePin>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            v: VERSION,
            pins: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Detail {
    Ids,
    Full,
}

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "message_pins_list" => Operation::new(Read, list),
        "message_pins_set" => Operation::new(Write, set),
        _ => return None,
    })
}

fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    match request.args.get("by") {
        None => Ok(()),
        Some(Value::String(by)) if by.trim() == "user" => Ok(()),
        Some(_) => Err(OpError::new(
            "permission_denied",
            "only the user may read or change message pins",
        )),
    }
}

fn detail(request: &DaemonRequest) -> Result<Detail, OpError> {
    match request.args.get("detail") {
        None | Some(Value::Null) => Ok(Detail::Full),
        Some(Value::String(value)) if value == "full" => Ok(Detail::Full),
        Some(Value::String(value)) if value == "ids" => Ok(Detail::Ids),
        _ => Err(OpError::new(
            "invalid_args",
            "detail must be 'ids' or 'full'",
        )),
    }
}

fn paths(home: &HomeLayout, group_id: &str) -> Result<(PathBuf, PathBuf), OpError> {
    let groups = store(home)?;
    let directory = groups.group_dir(group_id).map_err(OpError::invalid)?;
    groups.load(group_id).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            OpError::new("group_not_found", format!("group not found: {group_id}"))
        } else {
            OpError::io(error)
        }
    })?;
    Ok((
        directory.join("state/message_pins/pins.json"),
        directory.join("ledger.jsonl"),
    ))
}

fn load(path: &Path) -> Result<Snapshot, OpError> {
    let snapshot: Snapshot = match fs::read_json(path) {
        Ok(snapshot) => snapshot,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Snapshot::default()),
        Err(error) => return Err(OpError::io(error)),
    };
    let mut ids = HashSet::new();
    if snapshot.v != VERSION
        || snapshot.pins.iter().any(|pin| {
            pin.event_id.trim().is_empty()
                || pin.pinned_at.trim().is_empty()
                || !ids.insert(&pin.event_id)
        })
    {
        return Err(OpError::new(
            "pin_state_invalid",
            "invalid message pin state",
        ));
    }
    Ok(snapshot)
}

fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let detail = detail(request)?;
    let group_id = required_arg(request, "group_id")?;
    let (path, ledger_path) = paths(home, &group_id)?;
    let snapshot = load(&path)?;
    project(home, request, &group_id, &ledger_path, &snapshot, detail)
}

fn set(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let detail = detail(request)?;
    let group_id = required_arg(request, "group_id")?;
    let event_id = required_arg(request, "event_id")?;
    let pinned = request
        .args
        .get("pinned")
        .and_then(Value::as_bool)
        .ok_or_else(|| OpError::new("invalid_args", "pinned must be a boolean"))?;
    let (path, ledger_path) = paths(home, &group_id)?;
    let mut snapshot = load(&path)?;
    let existing = snapshot.pins.iter().any(|pin| pin.event_id == event_id);
    let changed = if pinned && !existing {
        let target = ledger::find_event(&ledger_path, &event_id).map_err(OpError::io)?;
        if !target.is_some_and(|event| event.kind == "chat.message" && event.group_id == group_id) {
            return Err(OpError::new(
                "pin_target_not_found",
                "the message is not available in this Group",
            ));
        }
        if snapshot.pins.len() >= MAX_PINS {
            return Err(OpError::new(
                "pin_limit_reached",
                format!("a Group can have at most {MAX_PINS} pinned messages"),
            ));
        }
        snapshot.pins.push(MessagePin {
            event_id,
            pinned_at: utc_now(),
        });
        true
    } else if !pinned && existing {
        // An unavailable target must still be removable from the list.
        snapshot.pins.retain(|pin| pin.event_id != event_id);
        true
    } else {
        false
    };

    // Prepare the response before the durable write. A projection failure must
    // not make a successful change look like a failed operation to the caller.
    let result = project(home, request, &group_id, &ledger_path, &snapshot, detail)?;
    if changed {
        fs::write_json_committed(&path, &snapshot).map_err(OpError::io)?;
    }
    Ok(result)
}

fn project(
    home: &HomeLayout,
    request: &DaemonRequest,
    group_id: &str,
    ledger_path: &Path,
    snapshot: &Snapshot,
    detail: Detail,
) -> OpResult {
    if snapshot.pins.is_empty() {
        return object(json!({"pins":[]}));
    }
    let mut rows = ledger::inspect(ledger_path, |events, positions| {
        snapshot
            .pins
            .iter()
            .enumerate()
            .map(|(index, pin)| {
                let found = positions
                    .get(&pin.event_id)
                    .and_then(|position| events.get(*position).map(|event| (*position, event)))
                    .filter(|(_, event)| {
                        event.kind == "chat.message" && event.group_id == group_id
                    });
                let position = found.map_or(usize::MAX, |(position, _)| position);
                let event = if detail == Detail::Full {
                    found.map(|(_, event)| event.clone())
                } else {
                    None
                };
                (position, index, pin.clone(), event)
            })
            .collect::<Vec<_>>()
    })
    .map_err(OpError::io)?;
    rows.sort_by_key(|(position, index, _, _)| (*position, *index));

    let mut decorated_by_id = HashMap::new();
    if detail == Detail::Full {
        let events = rows
            .iter_mut()
            .filter_map(|(_, _, _, event)| event.take())
            .collect();
        let mut projection_request = request.clone();
        projection_request
            .args
            .insert("with_read_status".into(), Value::Bool(true));
        projection_request
            .args
            .insert("with_obligation_status".into(), Value::Bool(true));
        for value in super::messaging_query_status::decorate(home, &projection_request, events)? {
            if let Some(id) = value.get("id").and_then(Value::as_str) {
                decorated_by_id.insert(id.to_owned(), value);
            }
        }
    }
    let pins = rows
        .into_iter()
        .map(|(_, _, pin, _)| {
            let mut row = json!({"event_id":pin.event_id,"pinned_at":pin.pinned_at});
            if detail == Detail::Full {
                let event = decorated_by_id.remove(&pin.event_id).unwrap_or(Value::Null);
                row["event"] = event;
            }
            row
        })
        .collect::<Vec<_>>();
    object(json!({"pins":pins}))
}
