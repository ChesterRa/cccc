//! The send side of structured questions to the user. The question becomes
//! the stored structure and generates the message text; the caller's text is
//! kept as its context.

use cccc_contracts::Event;
use cccc_contracts::reply_question::{FieldError, Question};
use cccc_core::{HomeLayout, ledger, user_requests, voice_notifications};
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, store};

/// Paths that cannot carry a question or write an answer reject both rather
/// than store or drop them silently. Only `reply_request_answer` writes an
/// answer, and it passes it out of band.
pub(crate) fn reject_fields(args: &Map<String, Value>, path: &str) -> Result<(), OpError> {
    if args.get("question").is_some_and(|value| !value.is_null()) {
        return Err(OpError::new(
            "invalid_question",
            format!(
                "{path} cannot carry a question; send a new request_reply message to user instead"
            ),
        ));
    }
    if args.get("answer").is_some_and(|value| !value.is_null()) {
        return Err(OpError::new(
            "invalid_args",
            "answers are written only by reply_request_answer",
        ));
    }
    Ok(())
}

/// Replace a `question` argument with the stored question and its generated
/// text. Run before any check that requires message text.
pub(super) fn prepare(data: &mut Map<String, Value>) -> Result<bool, OpError> {
    let Some(input) = data.remove("question").filter(|value| !value.is_null()) else {
        return Ok(false);
    };
    let context = match data.get("text") {
        None | Some(Value::Null) => "",
        Some(Value::String(text)) => text.as_str(),
        Some(_) => return Err(OpError::new("invalid_args", "text must be a string")),
    };
    let question = Question::from_request(&input, context).map_err(invalid)?;
    data.insert("text".into(), json!(question.text()));
    data.insert(
        "question".into(),
        serde_json::to_value(&question).map_err(OpError::invalid)?,
    );
    // `replay` looks the key up as it is stored, so both use the trimmed value.
    match data.get("client_id") {
        None => {}
        Some(Value::Null) => {
            data.remove("client_id");
        }
        Some(Value::String(key)) if !key.trim().is_empty() => {
            let key = key.trim().to_owned();
            data.insert("client_id".into(), json!(key));
        }
        Some(_) => {
            return Err(OpError::new(
                "invalid_args",
                "client_id must be a non-empty string",
            ));
        }
    }
    Ok(true)
}

/// What a stored message records about its sender's current state rather
/// than about the request; a retry after the sender's title changed is still
/// the same request.
const SENDER_SNAPSHOT: [&str; 3] = ["sender_title", "sender_runtime", "sender_avatar_path"];

/// A question is created once per client_id: a retry of the same normalized
/// request returns the first one, anything else under that client_id
/// conflicts, and a lookup that cannot complete refuses the send rather than
/// create a second.
pub(super) fn replay(
    home: &HomeLayout,
    group_id: &str,
    by: &str,
    data: &Map<String, Value>,
) -> Result<Option<Event>, OpError> {
    let Some(client_id) = data.get("client_id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let unavailable = |error: std::io::Error| {
        OpError::new(
            "idempotency_unavailable",
            format!("cannot check client_id: {error}"),
        )
    };
    // The lookup skips damaged records, and one could be the first send.
    user_requests::summary(home, group_id).map_err(unavailable)?;
    let path = store(home)?.ledger_path(group_id).map_err(OpError::io)?;
    match ledger::find_idempotent(&path, "chat.message", by, client_id).map_err(unavailable)? {
        None => Ok(None),
        Some(existing) if same_request(&existing.data, data) => Ok(Some(existing)),
        Some(_) => Err(OpError::new(
            "idempotency_conflict",
            "client_id was already used for a different message",
        )),
    }
}

fn same_request(stored: &Map<String, Value>, data: &Map<String, Value>) -> bool {
    let request = |data: &Map<String, Value>| {
        data.iter()
            .filter(|(key, _)| {
                !SENDER_SNAPSHOT.contains(&key.as_str())
                    && key.as_str() != voice_notifications::ORIGIN_ARG
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Map<_, _>>()
    };
    request(stored) == request(data)
}

/// After recipient normalization: a question asks the user alone and waits
/// for an answer, and is one the user's open requests would list.
pub(super) fn require_user_request(by: &str, data: &Map<String, Value>) -> Result<(), OpError> {
    let requested = data.get("message_mode").and_then(Value::as_str) == Some("request_reply");
    let user_only = data
        .get("to")
        .and_then(Value::as_array)
        .is_some_and(|to| to.len() == 1 && matches!(to[0].as_str(), Some("user" | "@user")));
    if requested && user_only && user_requests::addresses_user(by, data) {
        return Ok(());
    }
    Err(OpError::new(
        "invalid_question",
        "a question needs message_mode=request_reply and to=[\"user\"] only, from a sender other than the user, within this Group",
    ))
}

pub(crate) fn invalid(error: FieldError) -> OpError {
    let mut result = OpError::new("invalid_question", error.to_string());
    result.details.insert("field".into(), json!(error.field));
    if let Some(limit) = error.limit {
        result.details.insert("limit".into(), json!(limit));
    }
    if let Some(actual) = error.actual {
        result.details.insert("actual".into(), json!(actual));
    }
    result
}
