use axum::Router;
use axum::extract::{Extension, Query, State};
use axum::routing::{get, post};
use serde_json::json;
use std::collections::HashMap;

use crate::AppState;
use crate::api::{ApiError, ApiResult, call, object, success};
use crate::auth::Principal;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/membership", get(state))
        .route("/api/v1/membership/login", post(login))
        .route("/api/v1/membership/login/poll", post(login_poll))
        .route("/api/v1/membership/logout", post(logout))
        .route("/api/v1/membership/reach/on", post(reach_on))
        .route("/api/v1/membership/reach/off", post(reach_off))
        .route("/api/v1/membership/reach/web-login", post(reach_web_login))
}

async fn state(State(state): State<AppState>) -> ApiResult {
    call(&state, "membership_status", object(json!({"by": "user"}))).await
}

async fn login(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_login",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn login_poll(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_login_poll",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn logout(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_logout",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn reach_on(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_reach_on",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn reach_off(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    call(
        &state,
        "membership_reach_off",
        object(json!({"by": query.get("by").map(String::as_str).unwrap_or("user")})),
    )
    .await
}

async fn reach_web_login(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    let status = call(&state, "membership_status", object(json!({"by": "user"}))).await?;
    let result = issue_reach_web_login(&state.home, &status.0["result"]["membership"], &principal)?;
    Ok(success(result))
}

fn issue_reach_web_login(
    home: &cccc_core::HomeLayout,
    membership: &serde_json::Value,
    principal: &Principal,
) -> Result<serde_json::Value, ApiError> {
    if membership
        .get("online")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err(ApiError::unavailable(
            "membership_reach_offline",
            "membership reach is not online",
        ));
    }
    let origin = membership
        .get("hostname")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let grant = cccc_core::web_login_grants::issue(
        home,
        origin,
        &cccc_core::access_tokens::token_id(&principal.raw_token),
        cccc_core::web_login_grants::DEFAULT_TTL_SECONDS,
    )
    .map_err(|error| ApiError::bad(error.to_string()))?;
    let web_url = format!(
        "{}/api/v1/web_access/exchange?code={}",
        grant.origin, grant.code
    );
    Ok(json!({
        "web_url":web_url,
        "expires_at_epoch":grant.expires_at_epoch
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_login_link_contains_only_a_short_lived_exchange_code() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        let principal = Principal {
            user_id: "admin".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            raw_token: "acc_admin_secret".into(),
        };
        let result = issue_reach_web_login(
            &home,
            &json!({"online":true,"hostname":"https://reach.example"}),
            &principal,
        )
        .expect("login link");
        let web_url = result["web_url"].as_str().expect("web URL");
        assert!(web_url.starts_with("https://reach.example/api/v1/web_access/exchange?code=wlg_"));
        assert!(!web_url.contains(&principal.raw_token));
        let code = url::Url::parse(web_url)
            .expect("URL")
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .expect("code");
        assert_eq!(
            cccc_core::web_login_grants::consume(&home, &code, "https://reach.example")
                .expect("consume"),
            Some(cccc_core::access_tokens::token_id(&principal.raw_token))
        );
    }
}
