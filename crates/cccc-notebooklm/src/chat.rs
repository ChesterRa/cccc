use serde_json::Value;

use crate::error::{Error, Result};
use crate::models::{QueryResult, Reference};
use crate::rpc::{parse_chunks, visit_frames};

pub(crate) fn decode(raw: &str) -> Result<QueryResult> {
    let body = raw
        .strip_prefix(")]}'\r\n")
        .or_else(|| raw.strip_prefix(")]}'\n"))
        .unwrap_or(raw);
    let chunks = parse_chunks(body)?;
    let mut best_marked = None;
    let mut best_unmarked = None;
    let mut parseable = 0usize;
    let mut failure = None;
    for chunk in &chunks {
        visit_frames(
            chunk,
            &mut |frame| match frame.first().and_then(Value::as_str) {
                Some("er") => {
                    failure = Some(Error::Refused(format!(
                        "server error frame {:?}",
                        frame.get(2)
                    )));
                }
                Some("wrb.fr") => match frame.get(2) {
                    Some(Value::String(inner)) => match serde_json::from_str::<Value>(inner) {
                        Ok(value) => {
                            parseable += 1;
                            if let Some(candidate) = answer_candidate(&value) {
                                let target = if candidate.0 {
                                    &mut best_marked
                                } else {
                                    &mut best_unmarked
                                };
                                if target.as_ref().is_none_or(|old: &QueryResult| {
                                    old.answer.len() < candidate.1.answer.len()
                                }) {
                                    *target = Some(candidate.1);
                                }
                            }
                        }
                        Err(error) => {
                            failure = Some(Error::drift("streamed chat JSON", error.to_string()))
                        }
                    },
                    Some(Value::Null) | None => {
                        failure = Some(Error::Refused(format!(
                            "server rejected chat; status={:?}",
                            frame.get(5)
                        )));
                    }
                    _ => {
                        failure = Some(Error::drift(
                            "streamed chat frame",
                            "inner result was not JSON text",
                        ))
                    }
                },
                _ => {}
            },
        );
    }
    if let Some(error) = failure {
        return Err(error);
    }
    if parseable == 0 {
        return Err(Error::drift(
            "streamed chat response",
            "no parseable answer frames",
        ));
    }
    Ok(best_marked.or(best_unmarked).unwrap_or(QueryResult {
        answer: String::new(),
        references: Vec::new(),
    }))
}

fn answer_candidate(inner: &Value) -> Option<(bool, QueryResult)> {
    let row = inner.as_array()?.first()?.as_array()?;
    let answer = row.first()?.as_str()?.to_owned();
    let type_info = row.get(4).and_then(Value::as_array);
    let marked = type_info
        .and_then(|value| value.last())
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
    Some((marked, QueryResult { answer, references }))
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

    #[test]
    fn selects_longest_marked_answer_and_fails_on_rejection() {
        let inner = serde_json::json!([["answer", null, null, null, [null, null, null, [], 1]]])
            .to_string();
        let frame = serde_json::json!([["wrb.fr", "", inner]]).to_string();
        let body = format!(")]}}'\n{}\n{}\n", frame.len(), frame).replace(")]}'", ")]}'");
        assert_eq!(decode(&body).expect("chat").answer, "answer");
        assert!(decode("[[\"wrb.fr\",null,null,null,null,[3]]]").is_err());
    }
}
