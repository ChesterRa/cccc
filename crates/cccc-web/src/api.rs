use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cccc_contracts::DaemonRequest;
use serde_json::{Map, Value, json};

use crate::AppState;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
    details: Value,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl ApiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_request".into(),
            message: message.into(),
            details: json!({}),
        }
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found".into(),
            message: message.into(),
            details: json!({}),
        }
    }
    pub fn not_found_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: code.into(),
            message: message.into(),
            details: json!({}),
        }
    }
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "permission_denied".into(),
            message: message.into(),
            details: json!({}),
        }
    }
    pub fn forbidden_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: code.into(),
            message: message.into(),
            details: json!({}),
        }
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>, details: Value) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    pub fn unavailable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: code.into(),
            message: message.into(),
            details: json!({}),
        }
    }

    pub fn payload_too_large(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: code.into(),
            message: message.into(),
            details: json!({}),
        }
    }

    pub fn bad_code(code: impl Into<String>, message: impl Into<String>, details: Value) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: code.into(),
            message: message.into(),
            details,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"ok":false,"error":{"code":self.code,"message":self.message,"details":self.details}})),
        )
            .into_response()
    }
}

pub type ApiResult = Result<Json<Value>, ApiError>;

pub async fn call(state: &AppState, op: &str, args: Map<String, Value>) -> ApiResult {
    let response = state
        .client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .map_err(|error| {
            tracing::warn!(%error, op, "CCCC Web could not reach the daemon");
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "daemon_unavailable".into(),
                message: "CCCC daemon unavailable".into(),
                details: json!({}),
            }
        })?;
    if response.ok {
        return Ok(Json(json!({"ok":true,"result":response.result})));
    }
    let error = response.error.map_or_else(
        || {
            (
                "daemon_error".into(),
                "daemon operation failed".into(),
                Map::new(),
            )
        },
        |error| (error.code, error.message, error.details),
    );
    let status = if matches!(error.0.as_str(), "foreman_not_found" | "foreman_not_unique") {
        StatusCode::BAD_REQUEST
    } else if error.0.contains("not_found") {
        StatusCode::NOT_FOUND
    } else if error.0.contains("permission") {
        StatusCode::FORBIDDEN
    } else if error.0.ends_with("_busy")
        || error.0.ends_with("_conflict")
        || error.0.ends_with("_lease_lost")
    {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    Err(ApiError {
        status,
        code: error.0,
        message: error.1,
        details: Value::Object(error.2),
    })
}

pub fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}
pub fn success(value: Value) -> Json<Value> {
    Json(json!({"ok":true,"result":value}))
}
pub fn body_object(body: Value) -> Result<Map<String, Value>, ApiError> {
    body.as_object()
        .cloned()
        .ok_or_else(|| ApiError::bad("JSON object body required"))
}
