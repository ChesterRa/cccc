use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DaemonRequest {
    #[serde(default = "protocol_version")]
    pub v: u8,
    pub op: String,
    #[serde(default)]
    pub args: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DaemonError {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub details: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DaemonResponse {
    #[serde(default = "protocol_version")]
    pub v: u8,
    pub ok: bool,
    #[serde(default)]
    pub result: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DaemonError>,
}

impl DaemonResponse {
    #[must_use]
    pub fn success(result: Map<String, Value>) -> Self {
        Self {
            v: 1,
            ok: true,
            result,
            error: None,
        }
    }

    #[must_use]
    pub fn failure(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            v: 1,
            ok: false,
            result: Map::new(),
            error: Some(DaemonError {
                code: code.into(),
                message: message.into(),
                details: Map::new(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Unix,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonAddress {
    pub v: u8,
    pub transport: Transport,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    pub pid: u32,
    pub version: String,
    pub ts: String,
}

const fn protocol_version() -> u8 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_rejects_unknown_envelope_fields() {
        let raw = r#"{"v":1,"op":"ping","args":{},"extra":true}"#;
        assert!(serde_json::from_str::<DaemonRequest>(raw).is_err());
    }

    #[test]
    fn response_omits_empty_error() {
        let raw = serde_json::to_value(DaemonResponse::success(Map::new())).expect("serialize");
        assert!(raw.get("error").is_none());
    }
}
