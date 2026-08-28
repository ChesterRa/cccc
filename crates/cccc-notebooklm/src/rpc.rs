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

pub(crate) fn decode(raw: &str, rpc_id: &str, allow_null: bool) -> Result<Value> {
    let body = raw
        .strip_prefix(")]}'\r\n")
        .or_else(|| raw.strip_prefix(")]}'\n"))
        .unwrap_or(raw);
    let chunks = parse_chunks(body)?;
    let mut found = Vec::new();
    let mut result = None;
    let mut terminal_error = None;
    let mut null_status = None;
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
                terminal_error = Some(Error::Rpc {
                    rpc_id: rpc_id.into(),
                    message: format!("server error frame {:?}", frame.get(2)),
                });
            } else if tag == "wrb.fr" {
                match frame.get(2) {
                    Some(Value::String(inner)) => match serde_json::from_str(inner) {
                        Ok(value) => result = Some(value),
                        Err(error) => {
                            terminal_error = Some(Error::drift("rpc result", error.to_string()))
                        }
                    },
                    Some(Value::Null) | None => {
                        let status = frame.get(5).cloned().unwrap_or(Value::Null);
                        if status.to_string().contains("UserDisplayableError") {
                            terminal_error = Some(Error::RateLimited(status.to_string()));
                        } else if allow_null {
                            result.get_or_insert(Value::Null);
                        } else if result.is_none() {
                            null_status = Some(status);
                        }
                    }
                    Some(value) => result = Some(value.clone()),
                }
            }
        });
    }
    if let Some(error) = terminal_error {
        return Err(error);
    }
    if result.is_none()
        && let Some(status) = null_status
    {
        return Err(Error::Rpc {
            rpc_id: rpc_id.into(),
            message: format!("server returned null result; status={status}"),
        });
    }
    match result {
        Some(value) => Ok(value),
        None if !found.is_empty() => Err(Error::drift(
            "RPC id",
            format!("expected {rpc_id}, response contained {found:?}"),
        )),
        None => Err(Error::drift(
            "batchexecute response",
            "no NotebookLM RPC frames were present",
        )),
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
            decode(body, LIST_NOTEBOOKS, false).expect("decode")[0][0][0],
            "Title"
        );
        assert!(matches!(
            decode(body, GET_NOTEBOOK, false),
            Err(Error::SchemaDrift { .. })
        ));
    }

    #[test]
    fn rejects_unframed_html_and_rpc_errors() {
        assert!(decode("<html>login</html>", LIST_NOTEBOOKS, false).is_err());
        assert!(decode("[[\"er\",\"wXbhsf\",429]]", LIST_NOTEBOOKS, false).is_err());
    }
}
