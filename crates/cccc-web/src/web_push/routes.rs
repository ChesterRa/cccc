//! The browser's side of Web Push: read the state, make the key, bind and
//! unbind a subscription. Each acts for the signed-in principal only, and
//! endpoints come back only as hashes.

use axum::extract::{DefaultBodyLimit, Extension, State};
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use cccc_core::access_tokens::{AccessTokenStore, token_id};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

use super::crypto::{CryptoError, ReceiverKeys};
use super::endpoint;
use super::notifier::{SenderState, starting_points};
use super::store::{BindError, NewSubscription, Owner, PushStore};
use crate::AppState;
use crate::api::{ApiError, ApiResult, success};
use crate::auth::Principal;

// A browser subscription has a 2 KiB endpoint and two short encoded keys.
const MAX_SUBSCRIPTION_BODY: usize = 8 * 1024;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/push/config", get(config))
        .route("/api/v1/push/key", post(key))
        .route(
            "/api/v1/push/subscription",
            axum::routing::put(bind).delete(unbind),
        )
        .layer(DefaultBodyLimit::max(MAX_SUBSCRIPTION_BODY))
}

#[derive(Deserialize)]
struct SubscriptionBody {
    endpoint: String,
    keys: SubscriptionKeys,
    application_server_key: String,
}

#[derive(Deserialize)]
struct SubscriptionKeys {
    p256dh: String,
    auth: String,
}

#[derive(Deserialize)]
struct EndpointBody {
    endpoint: String,
}

fn store(state: &AppState) -> PushStore {
    PushStore::new(state.home.clone())
}

fn unavailable(error: impl std::fmt::Display) -> ApiError {
    tracing::warn!(%error, "Web Push state is unavailable");
    ApiError::unavailable(
        "push_unavailable",
        "notification settings cannot be read or saved",
    )
}

/// The principal a binding belongs to. A token is named by its id and
/// creation time, never stored itself.
fn owner(state: &AppState, principal: &Principal) -> Result<Owner, ApiError> {
    if principal.raw_token.is_empty() {
        return if principal.user_id == "local" {
            Ok(Owner::Local)
        } else {
            Err(ApiError::forbidden("notifications need a signed-in user"))
        };
    }
    let token = AccessTokenStore::new(state.home.clone())
        .and_then(|store| store.lookup(&principal.raw_token))
        .map_err(unavailable)?
        .filter(|token| token.user_id == principal.user_id)
        .ok_or_else(|| ApiError::forbidden("the access token is no longer valid"))?;
    Ok(Owner::Token {
        user_id: token.user_id,
        token_id: token_id(&principal.raw_token),
        created_at: token.created_at,
    })
}

/// The state differs per principal and changes with every bind, so it is
/// never cached.
async fn config(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Response, ApiError> {
    let owner = owner(&state, &principal)?;
    let store = store(&state);
    let vapid_public_key = store.public_key().map_err(unavailable)?;
    let subscriptions = store.owned(&owner).map_err(unavailable)?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        success(json!({
            "vapid_public_key": vapid_public_key,
            "subscriptions": subscriptions,
            "sender": state.push.state().as_str(),
        })),
    )
        .into_response())
}

/// Make the key the first time a browser asks to subscribe; return it after.
async fn key(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> ApiResult {
    owner(&state, &principal)?;
    let key = store(&state).ensure_key().map_err(unavailable)?;
    Ok(success(json!({ "vapid_public_key": key })))
}

async fn bind(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    Json(body): Json<SubscriptionBody>,
) -> ApiResult {
    // Notification links lead back to the page that subscribed, so its
    // origin must be one this server is reached at.
    let origin = crate::request_origin::source_origin(&headers)
        .filter(|origin| {
            crate::request_origin::origin_allowed_with_proxy(
                &headers,
                origin,
                crate::request_origin::proxy_headers_trusted(&state),
            )
        })
        .ok_or_else(|| {
            ApiError::forbidden_code(
                "push_origin_invalid",
                "subscribe from a CCCC page this server is reached at",
            )
        })?;
    let owner = owner(&state, &principal)?;
    if state.push.state() == SenderState::Unavailable {
        return Err(ApiError::unavailable(
            "push_sender_unavailable",
            "notifications cannot be sent from this server; see its log",
        ));
    }
    if endpoint::validate(&body.endpoint).is_none() {
        return Err(ApiError::bad_code(
            "push_service_unsupported",
            "this browser's push service is not supported",
            json!({}),
        ));
    }
    let keys = match ReceiverKeys::parse(&body.keys.p256dh, &body.keys.auth) {
        Ok(keys) => keys,
        // This server's own cryptography failing says nothing about the keys.
        Err(CryptoError::Internal) => {
            return Err(ApiError::unavailable(
                "push_sender_unavailable",
                "notifications cannot be prepared on this server right now",
            ));
        }
        Err(_) => {
            return Err(ApiError::bad_code(
                "push_subscription_invalid",
                "the subscription keys are not valid",
                json!({}),
            ));
        }
    };
    // Only what is written after this point is announced to it. A token is
    // read only where it can see, and can still name a deleted Group.
    let visible = (!principal.is_admin).then(|| {
        principal
            .allowed_groups
            .iter()
            .cloned()
            .collect::<HashSet<_>>()
    });
    let since = starting_points(&state.home, visible.as_ref()).map_err(unavailable)?;
    let (p256dh, auth) = keys.encoded();
    let subscription = NewSubscription {
        endpoint: body.endpoint,
        p256dh,
        auth,
        application_server_key: body.application_server_key,
        origin,
    };
    match store(&state).bind(subscription, owner, since) {
        Ok(()) => {
            state.push.wake.notify_one();
            Ok(success(json!({ "subscribed": true })))
        }
        Err(BindError::NoKey | BindError::KeyMismatch) => Err(ApiError::conflict(
            "push_key_mismatch",
            "the browser subscribed with a different key; subscribe again",
            json!({}),
        )),
        Err(BindError::Io(error)) => Err(unavailable(error)),
    }
}

/// Remove this principal's binding of the endpoint. Whether another
/// principal holds it is not revealed.
async fn unbind(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Json(body): Json<EndpointBody>,
) -> ApiResult {
    let owner = owner(&state, &principal)?;
    let removed = store(&state)
        .unbind(&body.endpoint, &owner)
        .map_err(unavailable)?;
    Ok(success(json!({ "removed": removed })))
}

#[cfg(test)]
mod tests;
