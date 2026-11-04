use serde_json::{Value, json};

use crate::error::{Error, Result};

pub(crate) const LIST_NOTEBOOKS: &str = "wXbhsf";
pub(crate) const CREATE_NOTEBOOK: &str = "CCqFvf";
pub(crate) const GET_NOTEBOOK: &str = "rLM1Ne";
pub(crate) const ADD_SOURCE: &str = "izAoDd";
pub(crate) const ADD_SOURCE_FILE: &str = "o4cbdc";
pub(crate) const DELETE_SOURCE: &str = "tGMBJ";
pub(crate) const REFRESH_SOURCE: &str = "FLmJqe";
pub(crate) const UPDATE_SOURCE: &str = "b7Wfje";
pub(crate) const CREATE_ARTIFACT: &str = "R7cb6c";
pub(crate) const LIST_ARTIFACTS: &str = "gArtLc";

pub(crate) fn encode(rpc_id: &str, params: Value) -> Result<String> {
    let params = serde_json::to_string(&params).map_err(|error| Error::Rpc {
        rpc_id: rpc_id.into(),
        message: error.to_string(),
    })?;
    serde_json::to_string(&json!([[[rpc_id, params, null, "generic"]]])).map_err(|error| {
        Error::Rpc {
            rpc_id: rpc_id.into(),
            message: error.to_string(),
        }
    })
}

/// How [`decode`] treats a `wrb.fr` frame whose result slot is null.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NullResult {
    /// The call must return data, so a null result is an error.
    Reject,
    /// A null result without a status, or with status OK, is an empty
    /// success. A non-OK status is the server refusing the request, and a
    /// status this client cannot read is reported rather than taken as
    /// success.
    AllowUnlessRejected,
}

/// What the index-5 payload of a null-result frame says.
enum NullStatus {
    Absent,
    Ok,
    Rejected(u64),
    Unrecognized,
}

fn null_status(status: &Value) -> NullStatus {
    if status.is_null() {
        return NullStatus::Absent;
    }
    // A `google.rpc.Status` array: the leading integer is the gRPC code.
    match status
        .as_array()
        .and_then(|items| items.first())
        .and_then(Value::as_u64)
    {
        Some(0) => NullStatus::Ok,
        Some(code @ 1..=16) => NullStatus::Rejected(code),
        _ => NullStatus::Unrecognized,
    }
}

/// gRPC canonical status names, indexed by code.
const GRPC_STATUS_NAMES: [&str; 17] = [
    "ok",
    "cancelled",
    "unknown",
    "invalid argument",
    "deadline exceeded",
    "not found",
    "already exists",
    "permission denied",
    "resource exhausted",
    "failed precondition",
    "aborted",
    "out of range",
    "unimplemented",
    "internal",
    "unavailable",
    "data loss",
    "unauthenticated",
];

pub(crate) fn decode(raw: &str, rpc_id: &str, null_result: NullResult) -> Result<Value> {
    let body = raw
        .strip_prefix(")]}'\r\n")
        .or_else(|| raw.strip_prefix(")]}'\n"))
        .unwrap_or(raw);
    let chunks = parse_chunks(body)?;
    let mut found = Vec::new();
    let mut result = None;
    let mut terminal_error = None;
    let mut null_frame = None;
    let mut rejection = None;
    let mut unrecognized = None;
    for chunk in &chunks {
        visit_frames(chunk, &mut |frame| {
            let Some(tag) = frame.first().and_then(Value::as_str) else {
                return;
            };
            let Some(id) = frame.get(1).and_then(Value::as_str) else {
                return;
            };
            if matches!(tag, "wrb.fr" | "er") {
                found.push(id.to_owned());
            }
            if id != rpc_id {
                return;
            }
            if tag == "er" {
                terminal_error = Some(error_frame(rpc_id, frame));
            } else if tag == "wrb.fr" {
                // The result is JSON text; a text `null` is the same null result.
                let value = match frame.get(2) {
                    Some(Value::String(inner)) => match serde_json::from_str(inner) {
                        Ok(value) => value,
                        Err(error) => {
                            terminal_error = Some(Error::drift("rpc result", error.to_string()));
                            return;
                        }
                    },
                    Some(value) => value.clone(),
                    None => Value::Null,
                };
                match value {
                    Value::Null => {
                        let status = frame.get(5).cloned().unwrap_or(Value::Null);
                        if status.to_string().contains("UserDisplayableError") {
                            terminal_error = Some(Error::RateLimited(status.to_string()));
                        } else {
                            match null_status(&status) {
                                NullStatus::Rejected(code) => {
                                    rejection.get_or_insert(code);
                                }
                                NullStatus::Unrecognized => {
                                    unrecognized.get_or_insert_with(|| status.clone());
                                }
                                NullStatus::Absent | NullStatus::Ok => {}
                            }
                            null_frame.get_or_insert(status);
                        }
                    }
                    value => result = Some(value),
                }
            }
        });
    }
    if let Some(error) = terminal_error {
        return Err(error);
    }
    if let Some(value) = result {
        return Ok(value);
    }
    if let Some(status) = null_frame {
        if let Some(code) = rejection {
            return Err(status_error(rpc_id, code));
        }
        return match (null_result, unrecognized) {
            (NullResult::AllowUnlessRejected, None) => Ok(Value::Null),
            (NullResult::AllowUnlessRejected, Some(status)) => Err(Error::drift(
                "null result status",
                format!("unrecognized status {status}"),
            )),
            (NullResult::Reject, _) => Err(Error::Rpc {
                rpc_id: rpc_id.into(),
                message: format!("server returned null result; status={status}"),
            }),
        };
    }
    if found.is_empty() {
        Err(Error::drift(
            "batchexecute response",
            "no NotebookLM RPC frames were present",
        ))
    } else {
        Err(Error::drift(
            "RPC id",
            format!("expected {rpc_id}, response contained {found:?}"),
        ))
    }
}

/// Classifies an `er` frame by the HTTP-style code in its third slot. Only
/// codes known to reject the request are definite; anything else may have
/// taken effect.
pub(crate) fn error_frame(rpc_id: &str, frame: &[Value]) -> Error {
    let code = frame.get(2).and_then(Value::as_u64);
    let message = match code {
        Some(code) => format!("server error frame with code {code}"),
        None => format!("server error frame {:?}", frame.get(2)),
    };
    match code {
        Some(401 | 403) => Error::Authentication,
        Some(429) => Error::RateLimited(message),
        Some(400 | 404) => Error::Refused(message),
        _ => Error::Rpc {
            rpc_id: rpc_id.into(),
            message,
        },
    }
}

/// The non-OK gRPC code of a null-result frame's index-5 payload, if any.
pub(crate) fn rejected_status(status: &Value) -> Option<u64> {
    match null_status(status) {
        NullStatus::Rejected(code) => Some(code),
        _ => None,
    }
}

/// Classifies a null result the server tagged with a non-OK status.
pub(crate) fn status_error(rpc_id: &str, code: u64) -> Error {
    let name = usize::try_from(code)
        .ok()
        .and_then(|index| GRPC_STATUS_NAMES.get(index))
        .copied()
        .unwrap_or("unknown");
    let message = format!("server returned status {code} ({name}) without a result");
    match code {
        4 => Error::Timeout(message),
        8 => Error::RateLimited(message),
        // The server failed or stopped while handling the request, so the
        // request may still have taken effect.
        1 | 2 | 10 | 13 | 14 | 15 => Error::Rpc {
            rpc_id: rpc_id.into(),
            message,
        },
        5 | 7 => Error::Refused(format!(
            "{message}; if several Google accounts are signed in, the notebook may belong to a different account than the connected one"
        )),
        16 => Error::Authentication,
        _ => Error::Refused(message),
    }
}

pub(crate) fn parse_chunks(body: &str) -> Result<Vec<Value>> {
    let lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let mut chunks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let payload = if line.parse::<usize>().is_ok() {
            index += 1;
            lines
                .get(index)
                .copied()
                .ok_or_else(|| Error::drift("batchexecute framing", "byte count had no payload"))?
        } else {
            line
        };
        chunks.push(
            serde_json::from_str(payload)
                .map_err(|error| Error::drift("batchexecute JSON", error.to_string()))?,
        );
        index += 1;
    }
    Ok(chunks)
}

pub(crate) fn visit_frames(value: &Value, visitor: &mut impl FnMut(&[Value])) {
    let Some(items) = value.as_array() else {
        return;
    };
    if items.first().and_then(Value::as_str).is_some() {
        visitor(items);
        return;
    }
    for item in items {
        visit_frames(item, visitor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_chunked_rpc_and_rejects_id_drift() {
        let body = ")]}'\n42\n[[\"wrb.fr\",\"wXbhsf\",\"[[[\\\"Title\\\",null,\\\"id\\\"]]]\"]]\n";
        assert_eq!(
            decode(body, LIST_NOTEBOOKS, NullResult::Reject).expect("decode")[0][0][0],
            "Title"
        );
        assert!(matches!(
            decode(body, GET_NOTEBOOK, NullResult::Reject),
            Err(Error::SchemaDrift { .. })
        ));
    }

    #[test]
    fn rejects_unframed_html_and_rpc_errors() {
        assert!(decode("<html>login</html>", LIST_NOTEBOOKS, NullResult::Reject).is_err());
        assert!(
            decode(
                "[[\"er\",\"wXbhsf\",429]]",
                LIST_NOTEBOOKS,
                NullResult::Reject
            )
            .is_err()
        );
    }

    fn null_frame(rpc_id: &str, status: Value) -> String {
        json!([["wrb.fr", rpc_id, null, null, null, status, "generic"]]).to_string()
    }

    #[test]
    fn status_tagged_null_is_a_rejection() {
        // The shape NotebookLM sends when it refuses a source refresh.
        let refused = null_frame(REFRESH_SOURCE, json!([3]));
        assert!(matches!(
            decode(&refused, REFRESH_SOURCE, NullResult::AllowUnlessRejected),
            Err(Error::Refused(message)) if message.contains("invalid argument")
        ));
        assert!(matches!(
            decode(&refused, REFRESH_SOURCE, NullResult::Reject),
            Err(Error::Refused(_))
        ));
        // Deleting a source that is already gone answers without a status.
        let denied = null_frame(DELETE_SOURCE, json!([7]));
        assert!(matches!(
            decode(&denied, DELETE_SOURCE, NullResult::AllowUnlessRejected),
            Err(Error::Refused(_))
        ));
        let gone = null_frame(DELETE_SOURCE, Value::Null);
        assert_eq!(
            decode(&gone, DELETE_SOURCE, NullResult::AllowUnlessRejected).expect("no-op delete"),
            Value::Null
        );

        let empty = null_frame(UPDATE_SOURCE, Value::Null);
        assert_eq!(
            decode(&empty, UPDATE_SOURCE, NullResult::AllowUnlessRejected).expect("empty success"),
            Value::Null
        );
        assert!(matches!(
            decode(&empty, UPDATE_SOURCE, NullResult::Reject),
            Err(Error::Rpc { .. })
        ));
    }

    #[test]
    fn error_frames_with_rejection_codes_are_definite() {
        let decode_er = |code: Value| {
            decode(
                &json!([["er", ADD_SOURCE_FILE, code]]).to_string(),
                ADD_SOURCE_FILE,
                NullResult::Reject,
            )
        };
        assert!(matches!(decode_er(json!(403)), Err(Error::Authentication)));
        assert!(matches!(decode_er(json!(401)), Err(Error::Authentication)));
        assert!(matches!(decode_er(json!(429)), Err(Error::RateLimited(_))));
        assert!(matches!(decode_er(json!(400)), Err(Error::Refused(_))));
        assert!(matches!(decode_er(json!(404)), Err(Error::Refused(_))));
        for unclear in [json!(408), json!(500), json!(null), json!("boom")] {
            let error = decode_er(unclear.clone()).expect_err("error frame");
            assert!(matches!(error, Error::Rpc { .. }), "{unclear}");
            assert!(error.may_have_committed(), "{unclear}");
        }
    }

    #[test]
    fn a_null_result_sent_as_text_gets_the_same_status_checks() {
        let text_null = |status: Value| {
            json!([["wrb.fr", UPDATE_SOURCE, "null", null, null, status]]).to_string()
        };
        assert!(matches!(
            decode(
                &text_null(json!([3])),
                UPDATE_SOURCE,
                NullResult::AllowUnlessRejected
            ),
            Err(Error::Refused(_))
        ));
        assert!(matches!(
            decode(&text_null(json!([3])), UPDATE_SOURCE, NullResult::Reject),
            Err(Error::Refused(_))
        ));
        assert_eq!(
            decode(
                &text_null(Value::Null),
                UPDATE_SOURCE,
                NullResult::AllowUnlessRejected
            )
            .expect("empty success"),
            Value::Null
        );
        assert!(matches!(
            decode(&text_null(Value::Null), UPDATE_SOURCE, NullResult::Reject),
            Err(Error::Rpc { .. })
        ));
    }

    #[test]
    fn classifies_status_codes_on_null_results() {
        let decode_status = |status: Value| {
            decode(
                &null_frame(LIST_ARTIFACTS, status),
                LIST_ARTIFACTS,
                NullResult::AllowUnlessRejected,
            )
        };
        assert!(matches!(
            decode_status(json!([8])),
            Err(Error::RateLimited(_))
        ));
        assert!(matches!(decode_status(json!([4])), Err(Error::Timeout(_))));
        assert!(matches!(decode_status(json!([13])), Err(Error::Rpc { .. })));
        assert!(matches!(
            decode_status(json!([14, "busy"])),
            Err(Error::Rpc { .. })
        ));
        assert!(matches!(
            decode_status(json!([7])),
            Err(Error::Refused(message)) if message.contains("different account")
        ));
        assert!(matches!(
            decode_status(json!([16])),
            Err(Error::Authentication)
        ));
        assert_eq!(decode_status(json!([0])).expect("OK status"), Value::Null);
        // A status this client cannot read is not taken as success.
        for status in [
            json!([true]),
            json!([[3]]),
            json!([17]),
            json!([]),
            json!("3"),
            json!({"code": 3}),
        ] {
            assert!(
                matches!(
                    decode_status(status.clone()),
                    Err(Error::SchemaDrift { .. })
                ),
                "{status}"
            );
        }
    }

    #[test]
    fn a_populated_frame_supersedes_a_rejected_placeholder() {
        let body = format!(
            "{}\n{}",
            null_frame(LIST_ARTIFACTS, json!([3])),
            json!([["wrb.fr", LIST_ARTIFACTS, "[[]]"]])
        );
        assert_eq!(
            decode(&body, LIST_ARTIFACTS, NullResult::AllowUnlessRejected).expect("final frame"),
            json!([[]])
        );
    }
}
