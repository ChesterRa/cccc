//! The user's reply requests: answering one in place, and reading the open
//! ones of a Group or a summary across Groups. Chet's authenticated reply
//! path also uses this writer for unattended answers.

use cccc_contracts::reply_question::{self, Answer, FieldError, Stored};
use cccc_contracts::{DaemonRequest, Event, utc_now};
use cccc_core::chet_history::{GroupInputStage, GroupInputStates, Store as ChetHistoryStore};
use cccc_core::user_requests::{self, OpenRequestFact, PageError};
use cccc_core::{
    GroupDoc, HomeLayout, actors, connect_groups, ledger, unattended, voice_notifications,
};
use serde_json::{Map, Value, json};
use std::sync::Arc;

use super::messaging::InPlace;
use super::messaging_status::{self, Requester};
use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

const MAX_CLIENT_ID_CHARS: usize = 200;
const DEFAULT_PAGE: usize = 50;
const ANSWER_ARGS: [&str; 6] = [
    "group_id",
    "source_event_id",
    "by",
    "client_id",
    "answer",
    cccc_core::actors::CHET_WEB_ADMISSION_ARG,
];

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
    record_answer(home, request, false)
}

pub(super) fn answer_as_chet(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = required_arg(request, "reply_to")?;
    let mut forwarded = request.clone();
    forwarded.args.remove("actor_id");
    forwarded.args.remove("message_mode");
    forwarded.args.remove("reply_to");
    forwarded.args.remove("require_peer_insight");
    forwarded
        .args
        .insert("source_event_id".into(), json!(source));
    record_answer(home, &forwarded, true)
}

fn record_answer(home: &HomeLayout, request: &DaemonRequest, via_chet: bool) -> OpResult {
    super::messaging::reject_chet_provenance(&request.args)?;
    require_user(request)?;
    if let Some(key) = request.args.keys().find(|key| {
        !(ANSWER_ARGS.contains(&key.as_str())
            || (via_chet && key.as_str() == voice_notifications::ORIGIN_ARG))
    }) {
        return Err(OpError::new(
            "invalid_args",
            format!("unexpected argument: {key}"),
        ));
    }
    let author = if via_chet {
        let author = super::messaging::effective_message_author(home, request, "user")?;
        if author != actors::CHET_SERVICE_PRINCIPAL {
            return Err(OpError::new(
                "invalid_args",
                "a structured reply is available to Chet in unattended mode",
            ));
        }
        author
    } else {
        "user".to_owned()
    };
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
        ledger::find_idempotent(&path, "chat.message", &author, &client_id).map_err(|error| {
            OpError::new(
                "idempotency_unavailable",
                format!("cannot check client_id: {error}"),
            )
        })?;
    let state = messaging_status::user_request(home, &group.group_id, &source_event_id);
    if let Some(existing) = existing {
        return match state {
            Ok(Some(state))
                if (!via_chet
                    || (user_requests::is_chet_answer(&existing)
                        && actors::has_current_chet_provenance(&group, &existing)))
                    && same_answer(&existing, &state.target, &answer) =>
            {
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
    if via_chet
        && !unattended::copies(
            &unattended::load(home).map_err(OpError::io)?,
            &group,
            &target,
        )
    {
        return Err(OpError::new(
            "unattended_unavailable",
            "this request is outside the current unattended period",
        ));
    }
    super::messaging::reject_retired_reply(home, &target)?;
    let service_reply = target.by == cccc_core::actors::CHET_SERVICE_PRINCIPAL;
    if service_reply
        && !cccc_core::actors::is_chet_message(home, &group, &target).map_err(OpError::io)?
    {
        return Err(OpError::new(
            "invalid_source_event",
            "Cannot verify this Chet request's origin",
        ));
    }
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
    if !service_reply {
        args.insert("to".into(), json!([target.by]));
    }
    if let Some(admission) = request.args.get(cccc_core::actors::CHET_WEB_ADMISSION_ARG) {
        args.insert(
            cccc_core::actors::CHET_WEB_ADMISSION_ARG.into(),
            admission.clone(),
        );
    }
    if let Some(origin) = request.args.get(voice_notifications::ORIGIN_ARG) {
        args.insert(voice_notifications::ORIGIN_ARG.into(), origin.clone());
    }
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
    super::messaging::send_answer(home, &forwarded, in_place, service_reply)
}

/// Whether `existing` is the reply this answer sends to the Actor that asked,
/// replying to `request`, in the form `answer` takes there.
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

fn require_unbound_user_for_chet_breakdown(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.contains_key("_cccc_native_binding")
        || request.args.contains_key("_cccc_native_source_binding")
        || request.args.contains_key("_cccc_web_binding")
    {
        return Err(OpError::new(
            "permission_denied",
            "user authority is required for unattended breakdown",
        ));
    }
    Ok(())
}

enum BreakdownContext {
    Disabled,
    Unreadable(&'static str),
    Ready {
        unattended: cccc_contracts::chet::UnattendedSettings,
        history: Option<Arc<GroupInputStates>>,
        history_unreadable: bool,
    },
}

fn classify_open_requests(
    group: &GroupDoc,
    facts: &[OpenRequestFact],
    unattended_settings: &cccc_contracts::chet::UnattendedSettings,
    history: &GroupInputStates,
) -> Value {
    let group_generation = connect_groups::generation(group);
    let mut needs_user = 0_usize;
    let mut waiting = 0_usize;
    let mut waiting_unregistered = 0_usize;
    let mut dispatched_input_ids = Vec::<String>::new();

    for fact in facts {
        // 1. Chet already posted a normal (non-answering) Group reply in this generation: handed back.
        if group_generation != "legacy:"
            && fact
                .chet_reply_generations
                .iter()
                .any(|generation| generation == &group_generation)
        {
            needs_user += 1;
            continue;
        }
        let eligible_now = fact.after_imported_marker
            && unattended::copies_fact(
                unattended_settings,
                group,
                &fact.ts,
                &fact.by,
                fact.copy_shape,
            );
        // 2. Check durable Chet history for this Group generation and event ID.
        if let Some(entry) = history.by_source.get(&(
            group.group_id.clone(),
            group_generation.clone(),
            fact.id.clone(),
        )) {
            match entry.stage {
                GroupInputStage::Terminal => {
                    needs_user += 1;
                }
                GroupInputStage::Dispatched => {
                    dispatched_input_ids.push(entry.input_id.to_string());
                }
                GroupInputStage::Waiting => {
                    let generation_matches =
                        entry
                            .sender_actor_generation
                            .as_deref()
                            .is_none_or(|expected| {
                                actors::find(group, &fact.by).is_some_and(|actor| {
                                    actors::generation_identity(actor) == expected
                                })
                            });
                    if eligible_now && generation_matches {
                        waiting += 1;
                    } else {
                        needs_user += 1;
                    }
                }
            }
            continue;
        }
        // 3. No Chet history record yet: only count as waiting_unregistered while currently
        // covered by unattended mode and the sender has not been removed/recreated since.
        if eligible_now && fact.sender_added_before_request {
            waiting_unregistered += 1;
        } else {
            needs_user += 1;
        }
    }

    json!({
        "state": "fresh",
        "needs_user": needs_user,
        "waiting": waiting,
        "waiting_unregistered": waiting_unregistered,
        "dispatched_input_ids": dispatched_input_ids,
    })
}

pub(super) fn summary(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let include_chet_breakdown = match request.args.get("include_chet_breakdown") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(OpError::new(
                "invalid_args",
                "include_chet_breakdown must be a boolean",
            ));
        }
    };
    if include_chet_breakdown {
        require_unbound_user_for_chet_breakdown(request)?;
    }
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
    let mut breakdown_ctx = if include_chet_breakdown {
        match unattended::load(home) {
            Ok(settings) if settings.enabled => BreakdownContext::Ready {
                unattended: settings,
                history: None,
                history_unreadable: false,
            },
            Ok(_) => BreakdownContext::Disabled,
            Err(_) => BreakdownContext::Unreadable("unattended_unreadable"),
        }
    } else {
        BreakdownContext::Disabled
    };
    let as_of = utc_now();
    let groups = group_ids
        .iter()
        .map(|group_id| {
            let loaded_group = store.load(group_id).map_err(|_| "group_not_found");
            match loaded_group {
                Err(code) => json!({
                    "group_id": group_id,
                    "state": "unknown",
                    "pending": null,
                    "error": code,
                    "as_of": as_of,
                }),
                Ok(group) => {
                    let read_breakdown = !matches!(breakdown_ctx, BreakdownContext::Disabled);
                    let projection = if read_breakdown {
                        user_requests::summary_with_open_facts(home, group_id)
                            .map_err(|_| "unreadable")
                    } else {
                        user_requests::summary(home, group_id)
                            .map(|summary| (summary, Vec::new()))
                            .map_err(|_| "unreadable")
                    };
                    match projection {
                        Err(code) => json!({
                            "group_id": group_id,
                            "state": "unknown",
                            "pending": null,
                            "error": code,
                            "as_of": as_of,
                        }),
                        Ok((summary, facts)) => {
                            let mut entry = json!({
                                "group_id": group_id,
                                "state": "fresh",
                                "pending": summary.open,
                                "oldest_ts": summary.oldest_ts,
                                "revision": summary.revision,
                                "as_of": as_of,
                            });
                            if read_breakdown {
                                let chet_breakdown = match &mut breakdown_ctx {
                                    BreakdownContext::Disabled => unreachable!(),
                                    BreakdownContext::Unreadable(code) => json!({
                                        "state": "unknown",
                                        "error": code,
                                    }),
                                    BreakdownContext::Ready {
                                        unattended: unattended_settings,
                                        history,
                                        history_unreadable,
                                    } => {
                                        if summary.open == 0 {
                                            json!({
                                                "state": "fresh",
                                                "needs_user": 0,
                                                "waiting": 0,
                                                "waiting_unregistered": 0,
                                                "dispatched_input_ids": [],
                                            })
                                        } else {
                                            if history.is_none() && !*history_unreadable {
                                                match ChetHistoryStore::new(home.root())
                                                    .try_group_input_states()
                                                {
                                                    Ok(states) => *history = Some(states),
                                                    Err(_) => *history_unreadable = true,
                                                }
                                            }
                                            if let Some(states) = history.as_ref() {
                                                classify_open_requests(
                                                    &group,
                                                    &facts,
                                                    unattended_settings,
                                                    states,
                                                )
                                            } else {
                                                json!({
                                                    "state": "unknown",
                                                    "error": "chet_history_unreadable",
                                                })
                                            }
                                        }
                                    }
                                };
                                entry
                                    .as_object_mut()
                                    .expect("summary object")
                                    .insert("chet_breakdown".into(), chet_breakdown);
                            }
                            entry
                        }
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    object(json!({"groups": groups, "as_of": as_of}))
}
