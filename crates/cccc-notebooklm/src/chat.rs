use serde_json::Value;

use crate::error::{Error, Result};
use crate::models::{QueryResult, Reference};
use crate::rpc::{error_frame, parse_chunks, rejected_status, status_error, visit_frames};

/// The streamed chat endpoint, named in errors that carry an RPC id.
const CHAT_RPC: &str = "GenerateFreeFormStreamed";

pub(crate) fn decode(raw: &str) -> Result<QueryResult> {
    let body = raw
        .strip_prefix(")]}'\r\n")
        .or_else(|| raw.strip_prefix(")]}'\n"))
        .unwrap_or(raw);
    let chunks = parse_chunks(body)?;
    let mut final_marked = None;
    let mut best_marked = None;
    let mut best_unmarked = None;
    let mut parseable = 0usize;
    let mut terminated = false;
    let mut failure = None;
    for chunk in &chunks {
        visit_frames(
            chunk,
            &mut |frame| match frame.first().and_then(Value::as_str) {
                Some("er") => failure = Some(error_frame(CHAT_RPC, frame)),
                // Stream bookkeeping. Its number is a sequence, not a status.
                Some("e") => terminated |= frame.get(1).and_then(Value::as_i64).is_some(),
                Some("wrb.fr") => {
                    // The answer is JSON text; a text `null` is the same null result.
                    let value = match frame.get(2) {
                        Some(Value::String(inner)) => match serde_json::from_str::<Value>(inner) {
                            Ok(value) => value,
                            Err(error) => {
                                failure =
                                    Some(Error::drift("streamed chat JSON", error.to_string()));
                                return;
                            }
                        },
                        Some(Value::Null) | None => Value::Null,
                        Some(_) => {
                            failure = Some(Error::drift(
                                "streamed chat frame",
                                "inner result was not JSON text",
                            ));
                            return;
                        }
                    };
                    if value.is_null() {
                        let status = frame.get(5).cloned().unwrap_or(Value::Null);
                        failure = Some(if status.to_string().contains("UserDisplayableError") {
                            Error::RateLimited(status.to_string())
                        } else if let Some(code) = rejected_status(&status) {
                            status_error(CHAT_RPC, code)
                        } else {
                            Error::Refused(format!("server rejected chat; status={status}"))
                        });
                        return;
                    }
                    parseable += 1;
                    match answer_candidate(&value) {
                        Ok(Some(candidate)) if candidate.marked => {
                            // The server marks the last chunk final; a longer
                            // earlier chunk can be a draft.
                            if candidate.is_final {
                                final_marked = Some(candidate.result.clone());
                            }
                            keep_longest(&mut best_marked, candidate.result);
                        }
                        Ok(Some(candidate)) => keep_longest(&mut best_unmarked, candidate.result),
                        Ok(None) => {}
                        Err(error) => failure = Some(error),
                    }
                }
                _ => {}
            },
        );
    }
    if let Some(error) = failure {
        return Err(error);
    }
    if parseable == 0 {
        // A stream that ends without any answer payload is how the Web
        // backend answers when the account's chat quota is exhausted.
        if terminated {
            return Err(Error::RateLimited(
                "chat ended before NotebookLM returned an answer; the chat quota may be exhausted"
                    .into(),
            ));
        }
        return Err(Error::drift(
            "streamed chat response",
            "no parseable answer frames",
        ));
    }
    Ok(final_marked
        .or(best_marked)
        .or(best_unmarked)
        .unwrap_or(QueryResult {
            answer: String::new(),
            references: Vec::new(),
        }))
}

struct Candidate {
    marked: bool,
    is_final: bool,
    result: QueryResult,
}

fn keep_longest(best: &mut Option<QueryResult>, candidate: QueryResult) {
    if best
        .as_ref()
        .is_none_or(|old| old.answer.len() < candidate.answer.len())
    {
        *best = Some(candidate);
    }
}

/// Reads one decoded chunk. `[]` and `[[]]` are heartbeats; a payload that is
/// not a list, or a populated one whose answer row is not a list, means the wire
/// format changed.
fn answer_candidate(inner: &Value) -> Result<Option<Candidate>> {
    let Some(items) = inner.as_array() else {
        return Err(Error::drift(
            "streamed chat answer",
            "answer payload was not a list",
        ));
    };
    let Some(first) = items.first() else {
        return Ok(None);
    };
    let Some(row) = first.as_array() else {
        return Err(Error::drift(
            "streamed chat answer",
            "answer row was not a list",
        ));
    };
    let Some(answer) = row
        .first()
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    else {
        return Ok(None);
    };
    let type_info = row.get(4).and_then(Value::as_array);
    let marked = type_info
        .and_then(|value| value.get(4))
        .and_then(Value::as_i64)
        == Some(1);
    let references = type_info
        .and_then(|value| value.get(3))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, citation)| parse_reference(index + 1, citation))
        .collect();
    Ok(Some(Candidate {
        marked,
        is_final: inner.get(4) == Some(&Value::Bool(true)),
        result: QueryResult {
            answer: answer.to_owned(),
            references,
        },
    }))
}

fn parse_reference(number: usize, citation: &Value) -> Option<Reference> {
    let detail = citation.as_array()?.get(1)?.as_array()?;
    let source_id = find_uuid(detail.get(5)?, 0)?;
    let text = detail
        .get(4)
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_array)
        .and_then(|value| value.first())
        .and_then(Value::as_array)
        .and_then(|value| value.get(2))
        .and_then(find_first_string);
    Some(Reference {
        citation_number: number,
        source_id,
        text,
    })
}

fn find_uuid(value: &Value, depth: usize) -> Option<String> {
    if depth > 12 {
        return None;
    }
    if let Some(value) = value.as_str().filter(|value| is_uuid(value)) {
        return Some(value.to_owned());
    }
    value
        .as_array()?
        .iter()
        .find_map(|value| find_uuid(value, depth + 1))
}

fn find_first_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_array()?.iter().find_map(find_first_string))
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body(chunks: &[Value]) -> String {
        let mut body = String::from(")]}'\n");
        for chunk in chunks {
            let chunk = chunk.to_string();
            body.push_str(&format!("{}\n{chunk}\n", chunk.len()));
        }
        body
    }

    fn envelope(inner: Value) -> Value {
        json!([["wrb.fr", null, inner.to_string()]])
    }

    fn answer(text: &str, is_final: bool) -> Value {
        envelope(json!([
            [text, null, null, null, [[], null, null, [], 1]],
            null,
            null,
            null,
            is_final
        ]))
    }

    #[test]
    fn selects_longest_marked_answer_and_fails_on_rejection() {
        let inner = json!([["answer", null, null, null, [null, null, null, [], 1]]]);
        assert_eq!(
            decode(&body(&[envelope(inner)])).expect("chat").answer,
            "answer"
        );
        assert!(matches!(
            decode("[[\"wrb.fr\",null,null,null,null,[3]]]"),
            Err(Error::Refused(_))
        ));
    }

    #[test]
    fn the_final_marked_chunk_beats_a_longer_draft() {
        let chunks = [
            answer("A longer preliminary answer that was corrected.", false),
            answer("Short corrected answer.", true),
        ];
        assert_eq!(
            decode(&body(&chunks)).expect("chat").answer,
            "Short corrected answer."
        );

        // A final chunk without text leaves the longest marked answer in place.
        let chunks = [answer("Complete answer.", false), answer("", true)];
        assert_eq!(
            decode(&body(&chunks)).expect("chat").answer,
            "Complete answer."
        );
    }

    #[test]
    fn a_stream_that_ends_without_payload_is_rate_limited() {
        let quota = [
            json!([["di", 651], ["af.httprm", 651, "request", 15]]),
            json!([["e", 3, null, null, 91]]),
        ];
        assert!(matches!(decode(&body(&quota)), Err(Error::RateLimited(_))));

        let answered = [
            answer("Real answer.", true),
            json!([["e", 15, null, null, 41687]]),
        ];
        assert_eq!(
            decode(&body(&answered)).expect("chat").answer,
            "Real answer."
        );

        let heartbeat = [envelope(json!([[]])), json!([["e", 4, null, null, 120]])];
        assert_eq!(decode(&body(&heartbeat)).expect("heartbeat").answer, "");

        // The terminal number is a sequence; only an integer marks the frame.
        for unframed in [json!([["di", 651]]), json!([["e", true, null, null, 91]])] {
            assert!(matches!(
                decode(&body(&[unframed])),
                Err(Error::SchemaDrift { .. })
            ));
        }
    }

    #[test]
    fn rejections_keep_their_rate_limit_class() {
        let displayable = [json!([[
            "wrb.fr",
            null,
            null,
            null,
            null,
            [
                8,
                null,
                [["type.googleapis.com/google.internal.labs.tailwind.UserDisplayableError"]]
            ]
        ]])];
        assert!(matches!(
            decode(&body(&displayable)),
            Err(Error::RateLimited(_))
        ));
        let text_null = [json!([["wrb.fr", null, "null", null, null, [8]]])];
        assert!(matches!(
            decode(&body(&text_null)),
            Err(Error::RateLimited(_))
        ));
        let bare_text_null = [json!([["wrb.fr", null, "null"]])];
        assert!(matches!(
            decode(&body(&bare_text_null)),
            Err(Error::Refused(_))
        ));
        let error_frame = [json!([["er", "GenerateFreeFormStreamed", 429]])];
        assert!(matches!(
            decode(&body(&error_frame)),
            Err(Error::RateLimited(_))
        ));
        let exhausted = [json!([["wrb.fr", null, null, null, null, [8]]])];
        assert!(matches!(
            decode(&body(&exhausted)),
            Err(Error::RateLimited(_))
        ));
    }

    #[test]
    fn a_reshaped_answer_row_is_drift_but_heartbeats_are_not() {
        for drifted in [
            json!({"answer": "x"}),
            json!("answer"),
            json!(["scalar"]),
            json!([{"answer": "x"}]),
            json!([42]),
            json!([null]),
        ] {
            assert!(
                matches!(
                    decode(&body(&[envelope(drifted.clone())])),
                    Err(Error::SchemaDrift { .. })
                ),
                "{drifted}"
            );
        }
        for heartbeat in [json!([]), json!([[]])] {
            assert_eq!(
                decode(&body(&[envelope(heartbeat)]))
                    .expect("heartbeat")
                    .answer,
                ""
            );
        }
    }
}
