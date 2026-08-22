use cccc_contracts::DaemonRequest;
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout, inbox, ledger};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use crate::ops::messaging::load;

pub fn peek(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    require_actor_inbox(&actor_id)?;
    authorize(&group, request, &actor_id)?;
    let messages =
        inbox::list_unread(home, &group, &actor_id, limit(request)?).map_err(OpError::io)?;
    let cursor = peek_cursor_value(home, &group.group_id, &actor_id)?;
    object(json!({"messages":messages,"cursor":cursor}))
}

pub fn read(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    require_actor_inbox(&actor_id)?;
    authorize(&group, request, &actor_id)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let consumed = inbox::consume_unread(home, &group, &actor_id, &by, limit(request)?)
        .map_err(OpError::io)?;
    object(json!({
        "messages":consumed.messages,
        "cursor":{
            "event_id":consumed.cursor_event_id,
            "ts":consumed.cursor_ts,
            "updated_at":consumed.cursor_updated_at,
        },
        "event":consumed.read_event,
    }))
}

pub fn history(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let actor_id = required_arg(request, "actor_id")?;
    authorize(&group, request, &actor_id)?;
    let mode = string_arg(request, "mode")
        .unwrap_or_else(|| "all".into())
        .replace('-', "_");
    if !matches!(mode.as_str(), "all" | "send" | "request_reply" | "mail") {
        return Err(OpError::new(
            "invalid_message_mode",
            "mode must be all, send, request_reply, or mail",
        ));
    }
    let query = string_arg(request, "query")
        .unwrap_or_default()
        .to_lowercase();
    let before = string_arg(request, "before_event_id").unwrap_or_default();
    let limit = history_limit(request)?;
    let path = crate::dispatch::store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let result = ledger::inspect(&path, |events, _| {
        let generation = if actor_id == "user" {
            0
        } else {
            inbox::actor_generation_positions(events)
                .get(&actor_id)
                .copied()
                .unwrap_or(0)
        };
        let visible = events[generation..]
            .iter()
            .filter(|event| {
                event.kind == "chat.message"
                    && (event.by == actor_id || inbox::is_for_actor(&group, event, &actor_id))
            })
            .collect::<Vec<_>>();
        let end = if before.is_empty() {
            Ok(visible.len())
        } else {
            visible
                .iter()
                .position(|event| event.id == before)
                .ok_or_else(|| {
                    OpError::new(
                        "event_not_found",
                        format!("history anchor not found: {before}"),
                    )
                })
        }?;
        let mut matches = Vec::new();
        for event in visible[..end].iter().rev() {
            let event_mode = event
                .data
                .get("message_mode")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if mode != "all" && event_mode != mode {
                continue;
            }
            if !query.is_empty() {
                let searchable = ["text", "insight", "quote_text"]
                    .iter()
                    .filter_map(|key| event.data.get(*key).and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .to_lowercase();
                if !searchable.contains(&query) {
                    continue;
                }
            }
            matches.push((*event).clone());
            if matches.len() > limit {
                break;
            }
        }
        let has_more = matches.len() > limit;
        matches.truncate(limit);
        Ok::<_, OpError>((matches, has_more))
    })
    .map_err(OpError::io)??;
    object(json!({"messages":result.0,"has_more":result.1}))
}

fn limit(request: &DaemonRequest) -> Result<usize, OpError> {
    bounded_limit(request, 200)
}

fn history_limit(request: &DaemonRequest) -> Result<usize, OpError> {
    bounded_limit(request, 100)
}

fn bounded_limit(request: &DaemonRequest, maximum: u64) -> Result<usize, OpError> {
    let Some(value) = request.args.get("limit") else {
        return Ok(50);
    };
    let Some(limit) = value.as_u64() else {
        return Err(OpError::new(
            "invalid_limit",
            format!("limit must be an integer between 1 and {maximum}"),
        ));
    };
    if !(1..=maximum).contains(&limit) {
        return Err(OpError::new(
            "invalid_limit",
            format!("limit must be an integer between 1 and {maximum}"),
        ));
    }
    usize::try_from(limit).map_err(OpError::invalid)
}

fn peek_cursor_value(home: &HomeLayout, group_id: &str, actor_id: &str) -> Result<Value, OpError> {
    let (event_id, ts, _updated_at) =
        inbox::cursor_details(home, group_id, actor_id).map_err(OpError::io)?;
    Ok(json!({"event_id":event_id,"ts":ts}))
}

fn authorize(group: &GroupDoc, request: &DaemonRequest, actor_id: &str) -> Result<(), OpError> {
    permissions::require_inbox(
        group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
        actor_id,
    )
    .map_err(OpError::invalid)
}

fn require_actor_inbox(actor_id: &str) -> Result<(), OpError> {
    if matches!(actor_id.trim(), "user" | "@user") {
        return Err(OpError::new(
            "invalid_inbox_recipient",
            "Inbox is only available for agents",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{history_limit, limit};
    use cccc_contracts::DaemonRequest;
    use serde_json::{Map, Value, json};

    fn request(value: Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: "inbox_read".into(),
            args: json!({"limit":value})
                .as_object()
                .cloned()
                .unwrap_or_else(Map::new),
        }
    }

    #[test]
    fn limits_reject_non_integer_and_out_of_range_values() {
        for value in [json!(true), json!("2"), json!(1.5), json!(0), json!(201)] {
            assert!(limit(&request(value)).is_err());
        }
        for value in [json!(true), json!("2"), json!(1.5), json!(0), json!(101)] {
            assert!(history_limit(&request(value)).is_err());
        }
        assert_eq!(limit(&request(json!(200))).expect("read limit"), 200);
        assert_eq!(
            history_limit(&request(json!(100))).expect("history limit"),
            100
        );
    }
}
