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
    if !principal.is_admin {
        return Err(ApiError::forbidden("administrator access is required"));
    }
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
    let store = cccc_core::access_tokens::AccessTokenStore::new(home.clone())
        .map_err(|error| ApiError::bad(error.to_string()))?;
    let token = if principal.raw_token.is_empty() && principal.user_id == "local" {
        // A direct localhost administrator has no bearer to exchange. Reuse an
        // existing administrator credential; never mint a new long-lived token.
        store
            .list()
            .map_err(|error| ApiError::bad(error.to_string()))?
            .into_iter()
            .find(|token| token.is_admin)
    } else {
        // A remote administrator remains bound to their own credential, even if
        // it was revoked while checking the tunnel. Do not fall back to another.
        store
            .lookup(&principal.raw_token)
            .map_err(|error| ApiError::bad(error.to_string()))?
            .filter(|token| token.is_admin)
    }
    .ok_or_else(|| ApiError::forbidden("an active administrator access token is required"))?;
    let grant = cccc_core::web_login_grants::issue(
        home,
        origin,
        &token.token_id(),
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

    #[tokio::test]
    async fn reach_login_from_local_admin_resolves_an_existing_admin_credential() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        let store = cccc_core::access_tokens::AccessTokenStore::new(home.clone()).expect("store");
        let token = store
            .create("owner", Vec::new(), true, None)
            .expect("admin token");
        let principal = Principal {
            user_id: "local".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            raw_token: String::new(),
        };
        let result = issue_reach_web_login(
            &home,
            &json!({"online":true,"hostname":"https://reach.example"}),
            &principal,
        )
        .expect("login link");
        let url = url::Url::parse(result["web_url"].as_str().expect("URL")).expect("URL");
        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .expect("code")
            .1;
        let token_id = cccc_core::web_login_grants::consume(&home, &code, "https://reach.example")
            .expect("consume")
            .expect("grant");
        assert_eq!(token_id, token.token_id());
        assert_eq!(store.list().expect("tokens").len(), 1);

        // Exercise the remote browser exchange as well as the stored mapping.
        use tower::ServiceExt;
        let result = issue_reach_web_login(
            &home,
            &json!({"online":true,"hostname":"http://reach.example"}),
            &principal,
        )
        .expect("second link");
        let url = url::Url::parse(result["web_url"].as_str().expect("URL")).expect("URL");
        let response = crate::app(home)
            .oneshot(
                axum::http::Request::get(format!(
                    "{}?{}",
                    url.path(),
                    url.query().expect("exchange code")
                ))
                .header(axum::http::header::HOST, "reach.example")
                .body(axum::body::Body::empty())
                .expect("request"),
            )
            .await
            .expect("exchange response");
        assert_eq!(response.status(), axum::http::StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[axum::http::header::LOCATION], "/ui/");
        assert!(
            response.headers()[axum::http::header::SET_COOKIE]
                .to_str()
                .expect("cookie")
                .contains(&token.token)
        );
    }

    #[test]
    fn reach_login_link_contains_only_a_short_lived_exchange_code() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        cccc_core::access_tokens::AccessTokenStore::new(home.clone())
            .expect("store")
            .create("admin", Vec::new(), true, Some("acc_admin_secret"))
            .expect("admin");
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

    #[test]
    fn reach_login_never_substitutes_another_token_for_a_revoked_remote_admin() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        cccc_core::access_tokens::AccessTokenStore::new(home.clone())
            .expect("store")
            .create("other-admin", Vec::new(), true, None)
            .expect("other admin");
        let principal = Principal {
            user_id: "revoked-admin".into(),
            allowed_groups: Vec::new(),
            is_admin: true,
            raw_token: "acc_revoked".into(),
        };
        let result = issue_reach_web_login(
            &home,
            &json!({"online":true,"hostname":"https://reach.example"}),
            &principal,
        );
        assert!(result.is_err());
        assert!(!home.root().join("web_login_grants.json").exists());
    }
}
