use cccc_contracts::{DaemonRequest, Event};
use cccc_core::permissions;
use cccc_core::{GroupDoc, HomeLayout, inbox, ledger};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};
use crate::ops::messaging::load;

pub fn summary(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = load(home, request)?;
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    // A Group-wide view must also reject peers in an empty Group; checking
    // only each returned row would accidentally make that permission vacuous.
    permissions::require_group(&group, &by).map_err(OpError::invalid)?;
    let cutoff = required_arg(request, "cutoff")?;
    let cutoff = chrono::DateTime::parse_from_rfc3339(&cutoff)
        .map_err(|_| OpError::invalid("cutoff must be an RFC3339 timestamp"))?
        .with_timezone(&chrono::Utc);
    if cutoff > chrono::Utc::now() {
        return Err(OpError::invalid("cutoff must not be in the future"));
    }
    let (observed_at, summary) =
        inbox::overdue_summary(home, &group, cutoff).map_err(OpError::io)?;
    if cutoff > observed_at {
        return Err(OpError::invalid("cutoff must not be after observed_at"));
    }
    object(json!({
        "group_id": group.group_id,
        "observed_at": observed_at.to_rfc3339(),
        "cutoff": cutoff.to_rfc3339(),
        "actors": summary,
    }))
}

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
    if request.args.contains_key("event_id") {
        let event_id = required_arg(request, "event_id")?;
        if ["mode", "query", "before_event_id", "limit"]
            .iter()
            .any(|name| request.args.contains_key(*name))
        {
            return Err(OpError::new(
                "invalid_args",
                "event_id cannot be combined with history filters or pagination",
            ));
        }
        return history_event(home, &group, &actor_id, &event_id, request);
    }
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
    let result = ledger::inspect_status(&path, |events, _, generations| {
        let generation = if actor_id == "user" {
            0
        } else {
            generations.get(&actor_id).copied().unwrap_or(0)
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

fn history_event(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    event_id: &str,
    request: &DaemonRequest,
) -> OpResult {
    let path = crate::dispatch::store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let event = ledger::inspect_status(&path, |events, positions, generations| {
        let generation = if actor_id == "user" {
            0
        } else {
            generations.get(actor_id).copied().unwrap_or(0)
        };
        let readable = |event: &Event| super::event_prefix::readable_by(group, event, actor_id);
        match positions.get(event_id) {
            Some(index) => Ok(events
                .get(*index)
                .filter(|event| *index >= generation && readable(event))
                .cloned()),
            // Only an ID that matches no event exactly is read as the short
            // `(reply:<id>)` hint the delivery header prints.
            None if super::event_prefix::is_prefix(event_id) => {
                super::event_prefix::resolve(events, generation, event_id, readable)
                    .map(|event| event.cloned())
            }
            None => Ok(None),
        }
    })
    .map_err(OpError::io)??
    .ok_or_else(|| {
        OpError::new(
            "event_not_found",
            "Message is unavailable in this Actor's history",
        )
    })?;
    // Render after releasing the ledger reader: Mail context may inspect the
    // same ledger. This is a read, never a delivery claim or Mail consumption.
    let mut delivery_text = super::actor_delivery_render::render_batch_with_mail_context(
        home,
        group,
        actor_id,
        std::slice::from_ref(&event),
    );
    if let Some(actor) = cccc_core::actors::find(group, actor_id)
        .filter(|actor| !actor.external_session_id.is_empty())
        && let Some(text) = delivery_text.as_mut()
    {
        let notice = (request.args.get("by").and_then(Value::as_str) == Some(actor_id))
            .then(|| {
                super::mcp_identity_notice::for_read(
                    home,
                    &group.group_id,
                    actor,
                    request.args.get("_cccc_mcp_identity"),
                )
            })
            .flatten();
        *text = format!(
            "{}{}\n\n{text}",
            notice.map_or_else(String::new, |notice| format!("{notice}\n\n")),
            super::actor_delivery_preamble::render(home, group, actor)
        );
    }
    object(json!({"messages":[event],"has_more":false,"delivery_text":delivery_text}))
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
    use super::{history, history_limit, limit};
    use cccc_contracts::{Actor, DaemonRequest, Event, local_session::LocalSessionRuntime};
    use cccc_core::{GroupStore, HomeLayout, actors, inbox, ledger, local_sessions};
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

    #[test]
    fn native_exact_history_reports_unverified_client_without_consuming_delivery() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("layout");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("history", "").expect("group");
        let mut actor = Actor::new("peer");
        actor.external_session_id = "codex:fixture".into();
        actors::add(&mut group, actor).expect("actor");
        store.save(&group).expect("save group");
        local_sessions::write(
            &home,
            local_sessions::NativeSession {
                source_id: "codex:fixture".into(),
                runtime: LocalSessionRuntime::Codex,
                native_session_id: "fixture".into(),
                host: local_sessions::NativeHost::capture(std::process::id()).expect("host"),
                cwd: temp.path().to_string_lossy().into_owned(),
                title: "Fixture".into(),
                origin: None,
                revision: "revision-one".into(),
                codex_home: None,
                codex_can_receive: false,
                codex_program: None,
                claude_config_dir: None,
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            },
        )
        .expect("session");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["peer"],"text":"fixture message","message_mode":"send"})
            .as_object()
            .expect("data")
            .clone();
        let path = store.ledger_path(&group.group_id).expect("ledger");
        ledger::append(&path, &event).expect("append");
        let before = std::fs::read(&path).expect("ledger before");
        let cursor = inbox::cursor_details(&home, &group.group_id, "peer").expect("cursor");
        let read = |by: &str, identity: Option<Value>| {
            let mut args =
                json!({"group_id":group.group_id,"actor_id":"peer","by":by,"event_id":event.id})
                    .as_object()
                    .expect("args")
                    .clone();
            if let Some(identity) = identity {
                args.insert("_cccc_mcp_identity".into(), identity);
            }
            history(
                &home,
                &DaemonRequest {
                    v: 1,
                    op: "message_history".into(),
                    args,
                },
            )
            .expect("history")
        };
        let matching = json!({"version":env!("CARGO_PKG_VERSION"),"source_id":cccc_core::build_info::current()["source_id"]});
        assert!(
            !read("peer", Some(matching))["delivery_text"]
                .as_str()
                .expect("text")
                .contains("source identity")
        );
        assert!(
            !read("user", None)["delivery_text"]
                .as_str()
                .expect("text")
                .contains("source identity")
        );
        let first = read("peer", None);
        assert!(
            first["delivery_text"]
                .as_str()
                .expect("text")
                .contains("client did not report")
        );
        assert_eq!(first["messages"][0]["id"], event.id);
        assert!(
            !read("peer", None)["delivery_text"]
                .as_str()
                .expect("text")
                .contains("client did not report")
        );
        assert_eq!(std::fs::read(&path).expect("ledger after"), before);
        assert_eq!(
            inbox::cursor_details(&home, &group.group_id, "peer").expect("cursor"),
            cursor
        );
    }
}
