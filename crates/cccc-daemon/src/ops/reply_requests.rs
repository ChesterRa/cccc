//! The user's reply requests: answering one in place, and reading the open
//! ones of a Group or a summary across Groups. Each acts only as the user;
//! a call made under an Actor identity is refused.

use cccc_contracts::reply_question::{self, Answer, FieldError, Stored};
use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::user_requests::{self, PageError};
use cccc_core::{HomeLayout, ledger};
use serde_json::{Map, Value, json};

use super::messaging::InPlace;
use super::messaging_status::{self, Requester};
use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

const MAX_CLIENT_ID_CHARS: usize = 200;
const DEFAULT_PAGE: usize = 50;
const ANSWER_ARGS: [&str; 5] = ["group_id", "source_event_id", "by", "client_id", "answer"];

/// Only a missing `by` defaults to the user; any other value, including
/// `null` or a non-string, is refused rather than read as the user.
fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    match request.args.get("by") {
        None => Ok(()),
        Some(Value::String(by)) if by.trim() == "user" => Ok(()),
        Some(_) => Err(OpError::new(
            "permission_denied",
            "only the user answers or lists the user's reply requests",
        )),
    }
}

pub(super) fn answer(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    if let Some(key) = request
        .args
        .keys()
        .find(|key| !ANSWER_ARGS.contains(&key.as_str()))
    {
        return Err(OpError::new(
            "invalid_args",
            format!("unexpected argument: {key}"),
        ));
    }
    let group = super::messaging::load(home, request)?;
    let source_event_id = required_arg(request, "source_event_id")?;
    let client_id = string_arg(request, "client_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && value.chars().count() <= MAX_CLIENT_ID_CHARS)
        .ok_or_else(|| {
            OpError::new(
                "invalid_args",
                format!("client_id is required (at most {MAX_CLIENT_ID_CHARS} characters)"),
            )
        })?;
    let answer = Answer::parse(request.args.get("answer").unwrap_or(&Value::Null))
        .map_err(invalid_answer)?;

    // The lookups below read the ledger leniently, skipping damaged records.
    // A skipped record could be an earlier answer or the request's closure,
    // so first prove every stored record is readable.
    user_requests::summary(home, &group.group_id).map_err(unreadable)?;

    // A retry returns the first result, even after the request closed or its
    // requester left. The same client_id with anything else conflicts, and a
    // lookup that cannot complete refuses the answer. Clients rely on this
    // order: every later refusal proves that no earlier attempt was recorded.
    let path = store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)?;
    let existing =
        ledger::find_idempotent(&path, "chat.message", "user", &client_id).map_err(|error| {
            OpError::new(
                "idempotency_unavailable",
                format!("cannot check client_id: {error}"),
            )
        })?;
    let state = messaging_status::user_request(home, &group.group_id, &source_event_id);
    if let Some(existing) = existing {
        return match state {
            Ok(Some(state)) if same_answer(&existing, &state.target, &answer) => {
                object(json!({"event": existing, "message_mode": "send", "duplicate": true}))
            }
            // The source was read, so the earlier message is another one.
            Ok(_) => Err(OpError::new(
                "idempotency_conflict",
                "client_id was already used for a different message",
            )),
            // Without the request, the earlier message cannot be compared
            // with the answer it would send.
            Err(_) => Err(OpError::new(
                "idempotency_unavailable",
                "cannot check client_id: the request it would answer cannot be read",
            )),
        };
    }
    let invalid_source = || {
        OpError::new(
            "invalid_source_event",
            "source_event_id must identify a request_reply message to user",
        )
    };
    let state = state.map_err(|error| match error.code.as_str() {
        "event_not_found" => invalid_source(),
        _ => error,
    })?;
    let Some(state) = state else {
        return Err(invalid_source());
    };

    if let Some(closed) = state.closed {
        let mut error = OpError::new(
            "reply_request_closed",
            format!("this request is already {closed}"),
        );
        error.details.insert("state".into(), json!(closed));
        return Err(error);
    }
    match state.requester {
        Requester::Available => {}
        Requester::Unavailable => {
            return Err(OpError::new(
                "requester_unavailable",
                "the Actor that asked was removed or added again; close the request or send a new message",
            ));
        }
        Requester::Unknown => {
            return Err(OpError::new(
                "requester_unknown",
                "the ledger cannot confirm the Actor that asked is still the same; close the request or send a new message",
            ));
        }
        Requester::Unsupported => {
            let mut error = OpError::new(
                "unsupported_request_source",
                "this request came from another Group or instance; answer it with an ordinary reply",
            );
            error
                .details
                .insert("reply_route".into(), json!("composer"));
            return Err(error);
        }
    }
    let target = state.target;
    super::messaging::reject_retired_reply(home, &target)?;
    let stored = reply_question::stored(&target.data);
    let (text, in_place) = match &stored {
        Stored::Valid(question) => (
            answer.reply_text(question).map_err(invalid_answer)?,
            InPlace::Structured(serde_json::to_value(&answer).map_err(OpError::invalid)?),
        ),
        Stored::Unavailable if answer.option_id.is_some() => {
            return Err(OpError::new(
                "question_unavailable",
                "this question cannot be read reliably; answer it in your own words",
            ));
        }
        Stored::None if answer.option_id.is_some() => {
            return Err(OpError::new(
                "invalid_answer",
                "this request has no options; answer it in your own words",
            ));
        }
        // A plain request gets an ordinary reply in the user's own words.
        Stored::None | Stored::Unavailable => {
            (answer.text.clone().unwrap_or_default(), InPlace::Text)
        }
    };
    let mut args = Map::new();
    args.insert("group_id".into(), json!(group.group_id));
    args.insert("by".into(), json!("user"));
    args.insert("text".into(), json!(text));
    args.insert("to".into(), json!([target.by]));
    args.insert("message_mode".into(), json!("send"));
    args.insert("reply_to".into(), json!(target.id));
    args.insert("client_id".into(), json!(client_id));
    match &stored {
        // The requester sees which question was answered even when a long
        // context fills the start of its text.
        Stored::Valid(question) => {
            super::message_metadata::add_quote_snapshot(&question.prompt, &mut args)
        }
        Stored::None | Stored::Unavailable => {
            super::message_metadata::add_reply_snapshot(&target, &mut args)
        }
    }
    let forwarded = DaemonRequest {
        v: request.v,
        op: "reply".into(),
        args,
    };
    super::messaging::send_answer(home, &forwarded, in_place)
}

/// Whether `existing` is the reply this answer sends: the user's send to the
/// Actor that asked, replying to `request`, in the form `answer` takes there.
/// A valid question is answered with a stored `answer`, which only this
/// operation writes; any other request with the user's words alone.
fn same_answer(existing: &Event, request: &Event, answer: &Answer) -> bool {
    let data = &existing.data;
    if data.get("reply_to").and_then(Value::as_str) != Some(request.id.as_str())
        || data.get("to") != Some(&json!([request.by]))
        || data.get("message_mode").and_then(Value::as_str) != Some("send")
    {
        return false;
    }
    let text = data.get("text").and_then(Value::as_str);
    match (reply_question::stored(&request.data), data.get("answer")) {
        (Stored::Valid(question), Some(stored)) => {
            Answer::parse(stored).is_ok_and(|stored| &stored == answer)
                && answer.reply_text(&question).ok().as_deref() == text
        }
        (Stored::None | Stored::Unavailable, None) => {
            answer.option_id.is_none() && text == answer.text.as_deref()
        }
        _ => false,
    }
}

/// A ledger with a damaged record cannot prove which requests are open.
fn unreadable(error: std::io::Error) -> OpError {
    if error.kind() == std::io::ErrorKind::InvalidData {
        return OpError::new(
            "ledger_unreadable",
            format!(
                "this Group's history has a damaged record, so its open requests cannot be confirmed: {error}"
            ),
        );
    }
    OpError::io(error)
}

fn invalid_answer(error: FieldError) -> OpError {
    let mut result = super::messaging::reply_questions::invalid(error);
    result.code = "invalid_answer".into();
    result
}

pub(super) fn page(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let group = super::messaging::load(home, request)?;
    let limit = match request.args.get("limit") {
        None | Some(Value::Null) => DEFAULT_PAGE,
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=user_requests::MAX_PAGE).contains(limit))
            .ok_or_else(|| {
                OpError::new(
                    "invalid_args",
                    format!("limit must be 1 to {}", user_requests::MAX_PAGE),
                )
            })?,
    };
    let before = string_arg(request, "before");
    let page =
        user_requests::page(home, &group.group_id, before.as_deref(), limit).map_err(|error| {
            match error {
                PageError::CursorStale => OpError::new(
                    "cursor_stale",
                    "the requests changed since this page was read; read again from the newest",
                ),
                PageError::InvalidCursor => {
                    OpError::new("invalid_cursor", "before is not a cursor from this list")
                }
                PageError::Io(error) => unreadable(error),
            }
        })?;
    let ids = page
        .events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let statuses = messaging_status::for_events(home, &group.group_id, &ids)?;
    object(json!({
        "events": page.events,
        "statuses": statuses,
        "total": page.total,
        "has_more": page.next.is_some(),
        "before": page.next,
    }))
}

pub(super) fn summary(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let store = store(home)?;
    let group_ids = match request.args.get("group_ids") {
        None | Some(Value::Null) => store
            .list()
            .map_err(OpError::io)?
            .into_iter()
            .map(|meta| meta.group_id)
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| OpError::new("invalid_args", "group_ids must be strings"))?,
        Some(_) => {
            return Err(OpError::new(
                "invalid_args",
                "group_ids must be an array of strings",
            ));
        }
    };
    let as_of = utc_now();
    let groups = group_ids
        .iter()
        .map(|group_id| {
            let summary = store
                .load(group_id)
                .map_err(|_| "group_not_found")
                .and_then(|_| user_requests::summary(home, group_id).map_err(|_| "unreadable"));
            match summary {
                Ok(summary) => json!({
                    "group_id": group_id,
                    "state": "fresh",
                    "pending": summary.open,
                    "oldest_ts": summary.oldest_ts,
                    "revision": summary.revision,
                    "as_of": as_of,
                }),
                Err(code) => json!({
                    "group_id": group_id,
                    "state": "unknown",
                    "pending": null,
                    "error": code,
                    "as_of": as_of,
                }),
            }
        })
        .collect::<Vec<_>>();
    object(json!({"groups": groups, "as_of": as_of}))
}
