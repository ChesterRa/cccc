use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use cccc_core::access_tokens::{AccessToken, AccessTokenStore, token_id};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde_json::{Value, json};

use crate::AppState;

pub fn mask(token: &AccessToken) -> Value {
    let raw = &token.token;
    let characters = raw.chars().collect::<Vec<_>>();
    let preview = if characters.len() > 8 {
        format!(
            "{}...{}",
            characters[..4].iter().collect::<String>(),
            characters[characters.len() - 4..]
                .iter()
                .collect::<String>()
        )
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
    let encoded = utf8_percent_encode(token, COOKIE_VALUE_ENCODE_SET);
    format!("cccc_access_token={encoded}; Path=/; HttpOnly; {policy}")
}

const COOKIE_VALUE_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b',')
    .add(b';')
    .add(b'\\')
    .add(b'%');

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

#[cfg(test)]
mod tests {
    use super::{cookie, mask};
    use cccc_core::access_tokens::AccessToken;

    #[test]
    fn masks_unicode_tokens_by_characters() {
        let token = AccessToken {
            token: "令牌甲乙丙丁戊己庚辛壬癸".into(),
            user_id: "user".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            created_at: String::new(),
            updated_at: String::new(),
        };
        assert_eq!(mask(&token)["token_preview"], "令牌甲乙...庚辛壬癸");
    }

    #[test]
    fn cookie_percent_encodes_unsafe_token_characters() {
        let value = cookie("token;含 空格", false);
        assert!(value.starts_with("cccc_access_token=token%3B"));
        assert!(!value.contains("含 空格"));
    }
}
