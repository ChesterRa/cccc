//! Strict ACP NDJSON/session contract. Parsing stays independent from DSH's
//! loose stream helper so malformed frames fail explicitly.
use cccc_contracts::DEEPSEEK_PROTOCOL_VERSION;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::Path;

pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_PENDING_REQUESTS: usize = 256;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("frame exceeds byte cap")]
    FrameTooLarge,
    #[error("invalid UTF-8/JSON frame")]
    InvalidFrame,
    #[error("NDJSON frame must be an object")]
    NonObject,
    #[error("jsonrpc must be 2.0")]
    WrongVersion,
    #[error("response frame cannot contain method")]
    MethodResponse,
    #[error("frame must be a response or notification")]
    MissingResponseOrMethod,
    #[error("json-rpc id must be a string or number")]
    InvalidId,
    #[error("unknown response id")]
    UnknownResponse,
    #[error("duplicate request id")]
    DuplicateRequest,
    #[error("pending request cap exceeded")]
    PendingCap,
}

#[derive(Debug, Default)]
pub struct NdjsonSession {
    pending: HashSet<String>,
}

impl NdjsonSession {
    pub fn register(&mut self, id: &Value) -> Result<(), ProtocolError> {
        let key = id_key(id)?;
        if self.pending.len() >= MAX_PENDING_REQUESTS {
            return Err(ProtocolError::PendingCap);
        }
        if !self.pending.insert(key) {
            return Err(ProtocolError::DuplicateRequest);
        }
        Ok(())
    }

    pub fn feed_line(&mut self, raw: &[u8]) -> Result<Value, ProtocolError> {
        if raw.len() > MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge);
        }
        let value: Value = serde_json::from_slice(raw).map_err(|_| ProtocolError::InvalidFrame)?;
        let object = value.as_object().ok_or(ProtocolError::NonObject)?;
        if object.get("jsonrpc") != Some(&Value::String("2.0".into())) {
            return Err(ProtocolError::WrongVersion);
        }
        if let Some(id) = object.get("id") {
            if let Some(method) = object.get("method") {
                // ACP permission prompts are agent->client requests.  They
                // carry an id so the client can answer, but are not responses
                // to one of our pending calls.
                if method != "session/request_permission" {
                    return Err(ProtocolError::MethodResponse);
                }
                let _ = id_key(id)?;
                return Ok(value);
            }
            let key = id_key(id)?;
            if !self.pending.remove(&key) {
                return Err(ProtocolError::UnknownResponse);
            }
        } else if !object.contains_key("method") {
            return Err(ProtocolError::MissingResponseOrMethod);
        }
        Ok(value)
    }

    pub(crate) fn discard_pending(&mut self, id: &Value) {
        if let Ok(key) = id_key(id) {
            self.pending.remove(&key);
        }
    }
}

fn id_key(value: &Value) -> Result<String, ProtocolError> {
    match value {
        Value::String(value) => Ok(format!("s:{value}")),
        Value::Number(value) => Ok(format!("n:{value}")),
        _ => Err(ProtocolError::InvalidId),
    }
}

pub fn initialize_request(client_version: &str) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":DEEPSEEK_PROTOCOL_VERSION,"clientCapabilities":{},"clientInfo":{"name":"cccc","version":client_version}}})
}

pub fn session_new_request(cwd: &str) -> Result<Value, &'static str> {
    if !is_absolute_cwd(cwd) {
        return Err("session/new cwd must be absolute");
    }
    Ok(
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":cwd,"mcpServers":[],"additionalDirectories":[]}}),
    )
}

fn is_absolute_cwd(cwd: &str) -> bool {
    if Path::new(cwd).is_absolute() {
        return true;
    }
    let bytes = cwd.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || cwd.starts_with("\\\\")
}

pub fn validate_initialize_result(message: &Value) -> Result<&Map<String, Value>, ProtocolError> {
    let result = message
        .get("result")
        .and_then(Value::as_object)
        .ok_or(ProtocolError::InvalidFrame)?;
    if result.get("protocolVersion") != Some(&json!(DEEPSEEK_PROTOCOL_VERSION)) {
        return Err(ProtocolError::WrongVersion);
    }
    let agent_name = result
        .get("agentInfo")
        .and_then(Value::as_object)
        .and_then(|info| info.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if agent_name.is_empty() {
        return Err(ProtocolError::InvalidFrame);
    }
    Ok(result)
}

pub fn validate_session_new_result(
    message: &Value,
    seen: &mut HashSet<String>,
) -> Result<String, ProtocolError> {
    let session_id = message
        .get("result")
        .and_then(|result| result.get("sessionId"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if session_id.is_empty() || !seen.insert(session_id.to_owned()) {
        return Err(ProtocolError::InvalidFrame);
    }
    Ok(session_id.to_owned())
}

pub fn permission_outcome(options: &Value, stopping: bool) -> Value {
    if stopping {
        return json!({"outcome":{"outcome":"cancelled"}});
    }
    let selected = options.as_array().and_then(|items| {
        items
            .iter()
            .find(|item| item.get("optionId") == Some(&Value::String("reject-once".into())))
    });
    selected.map_or_else(
        || json!({"outcome":{"outcome":"cancelled"}}),
        |_| json!({"outcome":{"outcome":"selected","optionId":"reject-once"}}),
    )
}

pub fn object_params(value: &Value) -> Option<&Map<String, Value>> {
    value.get("params")?.as_object()
}

pub fn validate_session_update<'a>(
    message: &'a Value,
    expected_session_id: &str,
) -> Result<&'a Map<String, Value>, ProtocolError> {
    if message.get("method") != Some(&Value::String("session/update".into())) {
        return Err(ProtocolError::InvalidFrame);
    }
    let params = object_params(message).ok_or(ProtocolError::InvalidFrame)?;
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if session_id != expected_session_id || expected_session_id.is_empty() {
        return Err(ProtocolError::InvalidFrame);
    }
    Ok(params)
}

pub fn permission_request_id<'a>(
    message: &'a Value,
    expected_session_id: &str,
) -> Result<&'a Value, ProtocolError> {
    if message.get("method") != Some(&Value::String("session/request_permission".into())) {
        return Err(ProtocolError::InvalidFrame);
    }
    let params = object_params(message).ok_or(ProtocolError::InvalidFrame)?;
    let session_id = params
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if session_id != expected_session_id || expected_session_id.is_empty() {
        return Err(ProtocolError::InvalidFrame);
    }
    message.get("id").ok_or(ProtocolError::InvalidId)
}

pub fn terminal_stop_reason(message: &Value) -> Option<&str> {
    message
        .get("result")
        .and_then(Value::as_object)
        .and_then(|result| result.get("stopReason"))
        .and_then(Value::as_str)
}

#[cfg(test)]
#[path = "deepseek_acp/tests.rs"]
mod tests;
