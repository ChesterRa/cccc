use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cccc_core::access_tokens::{AccessToken, AccessTokenStore, token_id};
use serde_json::{Value, json};

use crate::AppState;

pub fn mask(token: &AccessToken) -> Value {
    let raw = &token.token;
    let preview = if raw.len() > 8 {
        format!("{}...{}", &raw[..4], &raw[raw.len() - 4..])
    } else {
        "****".into()
    };
    json!({"token_id":token_id(raw),"token_preview":preview,"user_id":token.user_id,"allowed_groups":token.allowed_groups,"is_admin":token.is_admin,"created_at":token.created_at,"updated_at":token.updated_at})
}

pub fn store(state: &AppState) -> std::io::Result<AccessTokenStore> {
    AccessTokenStore::new(state.home.clone())
}

pub fn clean_groups(groups: Vec<String>) -> Vec<String> {
    let mut output = Vec::new();
    for group in groups {
        let group = group.trim().to_owned();
        if !group.is_empty() && !output.contains(&group) {
            output.push(group);
        }
    }
    output
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn cookie(token: &str, secure: bool) -> String {
    let policy = if secure {
        "SameSite=None; Secure"
    } else {
        "SameSite=Lax"
    };
    format!("cccc_access_token={token}; Path=/; HttpOnly; {policy}")
}

pub fn server_error(error_value: impl std::fmt::Display) -> Response {
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "access_token_store_error",
        &error_value.to_string(),
    )
}

pub fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"ok":false,"error":{"code":code,"message":message,"details":{}}})),
    )
        .into_response()
}
