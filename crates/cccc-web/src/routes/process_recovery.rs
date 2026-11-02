//! Private review of one owner's uncertain process records.
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use cccc_runtime::process_recovery::RecoveryError;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState, VoiceRecoveryRequest,
    api::{ApiError, ApiResult, call, object, success},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/process-recovery/{owner}", get(list))
        .route("/api/v1/process-recovery/{owner}/retry", post(retry))
        .route("/api/v1/process-recovery/{owner}/accept", post(accept))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordRevision {
    id: String,
    version: String,
}

async fn list(State(state): State<AppState>, Path(owner): Path<String>) -> ApiResult {
    match owner.as_str() {
        "daemon" => review_daemon(&state, VoiceRecoveryRequest::List).await,
        "web" => review_web(&state, VoiceRecoveryRequest::List).await,
        _ => Err(ApiError::not_found("Unknown recovery owner")),
    }
}

async fn retry(
    State(state): State<AppState>,
    Path(owner): Path<String>,
    Json(record): Json<RecordRevision>,
) -> ApiResult {
    act(state, owner, record, false).await
}

async fn accept(
    State(state): State<AppState>,
    Path(owner): Path<String>,
    Json(record): Json<RecordRevision>,
) -> ApiResult {
    act(state, owner, record, true).await
}

async fn act(state: AppState, owner: String, record: RecordRevision, accept: bool) -> ApiResult {
    if record.id.is_empty()
        || record.version.is_empty()
        || record.id.len() > 256
        || record.version.len() > 256
    {
        return Err(ApiError::bad(
            "A reviewed record ID and version are required",
        ));
    }
    match owner.as_str() {
        "daemon" | "web" => {
            let request = if accept {
                VoiceRecoveryRequest::Accept {
                    id: record.id,
                    version: record.version,
                }
            } else {
                VoiceRecoveryRequest::Retry {
                    id: record.id,
                    version: record.version,
                }
            };
            if owner == "daemon" {
                review_daemon(&state, request).await
            } else {
                review_web(&state, request).await
            }
        }
        _ => Err(ApiError::not_found("Unknown recovery owner")),
    }
}

async fn review_web(state: &AppState, request: VoiceRecoveryRequest) -> ApiResult {
    let recovery = state.codex_voice.process_recovery().ok_or_else(|| {
        ApiError::unavailable(
            "process_recovery_unavailable",
            "This Web host cannot review its process records.",
        )
    })?;
    let result: Result<Value, RecoveryError> =
        tokio::task::spawn_blocking(move || (recovery.review)(request))
            .await
            .map_err(|_| {
                ApiError::unavailable(
                    "process_recovery_unavailable",
                    "Process recovery is unavailable.",
                )
            })?;
    result.map(success).map_err(recovery_error)
}

fn recovery_error(error: RecoveryError) -> ApiError {
    match error {
        RecoveryError::Changed => ApiError::conflict(
            "process_recovery_changed",
            "Recovery evidence changed; review the updated list.",
            json!({}),
        ),
        RecoveryError::Unavailable => ApiError::unavailable(
            "process_recovery_unavailable",
            "Process recovery is unavailable; try again or use offline review.",
        ),
    }
}

async fn review_daemon(state: &AppState, request: VoiceRecoveryRequest) -> ApiResult {
    if let Some(recovery) = state.codex_voice.process_recovery() {
        let offline_request = request.clone();
        let result =
            tokio::task::spawn_blocking(move || (recovery.review_daemon_offline)(offline_request))
                .await
                .map_err(|_| recovery_error(RecoveryError::Unavailable))?
                .map_err(recovery_error)?;
        if let Some(result) = result {
            return Ok(success(result));
        }
    }
    let (op, args) = match request {
        VoiceRecoveryRequest::List => ("owned_process_recovery_list", json!({"by":"user"})),
        VoiceRecoveryRequest::Retry { id, version } => (
            "owned_process_recovery_retry",
            json!({"by":"user","id":id,"version":version}),
        ),
        VoiceRecoveryRequest::Accept { id, version } => (
            "owned_process_recovery_accept",
            json!({"by":"user","id":id,"version":version}),
        ),
    };
    // If IPC fails after a mutation begins, do not fall back or replay it.
    call(state, op, object(args)).await
}

/// Check both arrays, so an unrecognized or malformed result cannot masquerade
/// as an empty daemon ledger. A failed read is unconfirmed, not "no records".
pub(super) async fn daemon_guard_pending(state: &AppState) -> Result<bool, ()> {
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        review_daemon(state, VoiceRecoveryRequest::List),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())?;
    let result = &response.0["result"];
    if result["owner"].as_str() != Some("daemon") {
        return Err(());
    }
    let records = result["records"].as_array().ok_or(())?;
    let diagnostics = result["diagnostics"].as_array().ok_or(())?;
    Ok(!records.is_empty() || !diagnostics.is_empty())
}

/// Called before starting a Voice provider. A missing daemon response is not
/// an empty ledger; report both relevant owners so the user can review them.
pub(crate) async fn ensure_voice_start_allowed(state: &AppState) -> Result<(), ApiError> {
    let web_pending = match state.codex_voice.recover_before_start().await {
        Ok(()) => false,
        Err(crate::codex_voice::restart::VoiceGateError::ProcessRecoveryPending) => true,
        Err(error) => return Err(error.into_api_error()),
    };
    let check_daemon = state
        .codex_voice
        .process_recovery()
        .is_some_and(|recovery| recovery.check_daemon_guard);
    let daemon_pending = check_daemon && daemon_guard_pending(state).await.unwrap_or(true);
    if !web_pending && !daemon_pending {
        return Ok(());
    }
    let mut owners = Vec::new();
    if daemon_pending {
        owners.push("daemon");
    }
    if web_pending {
        owners.push("web");
    }
    Err(ApiError::unavailable(
        "codex_voice_recovery_pending",
        "Previous process recovery is not yet confirmed.",
    )
    .with_details(json!({"retryable":true,"recovery_owners":owners})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode, header},
    };
    use cccc_core::{HomeLayout, access_tokens::AccessTokenStore};
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    #[tokio::test]
    async fn web_review_is_private_versioned_and_unavailable_to_exhibits() {
        let temp = tempfile::tempdir().expect("temporary home");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
        let admin = tokens.create("admin", vec![], true, None).expect("admin");
        let viewer = tokens
            .create("viewer", vec![], false, None)
            .expect("viewer");
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        let fixture = Arc::clone(&calls);
        let recovery = crate::VoiceProcessRecovery {
            reconcile: Arc::new(|| Ok(())),
            review_daemon_offline: Arc::new(|_| Ok(None)),
            check_daemon_guard: true,
            review: Arc::new(move |request| match request {
                VoiceRecoveryRequest::List => {
                    fixture
                        .lock()
                        .expect("fixture calls lock")
                        .push("list".into());
                    Ok(
                        json!({"owner":"web","records":[{"id":"a","version":"v1"}],"diagnostics":[]}),
                    )
                }
                VoiceRecoveryRequest::Retry { id, version } => {
                    fixture
                        .lock()
                        .expect("fixture calls lock")
                        .push(format!("retry:{id}:{version}"));
                    Ok(json!({"owner":"web","result":"still_pending"}))
                }
                VoiceRecoveryRequest::Accept { id, version } => {
                    fixture
                        .lock()
                        .expect("fixture calls lock")
                        .push(format!("accept:{id}:{version}"));
                    if version != "v1" {
                        Err(RecoveryError::Changed)
                    } else {
                        Ok(json!({"owner":"web","accepted":true}))
                    }
                }
            }),
        };
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let router_for = |mode| {
            crate::app_with_shutdown_and_recovery(
                home.clone(),
                shutdown.clone(),
                mode,
                None,
                crate::LiveBinding {
                    host: "127.0.0.1".into(),
                    port: 0,
                },
                "test".into(),
                Some(recovery.clone()),
            )
            .0
        };
        let router = router_for(crate::WebMode::Normal);
        let request = |method: &str, path: &str, token: &str, body: &str| {
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
                .expect("request")
        };
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                "/api/v1/process-recovery/web",
                &viewer.token,
                "",
            ))
            .await
            .expect("viewer response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(calls.lock().expect("fixture calls lock").is_empty());
        let response = router
            .clone()
            .oneshot(request(
                "GET",
                "/api/v1/process-recovery/web",
                &admin.token,
                "",
            ))
            .await
            .expect("list response");
        assert_eq!(response.status(), StatusCode::OK);
        let response = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/v1/process-recovery/web/retry",
                &admin.token,
                r#"{"id":"a","version":"v1"}"#,
            ))
            .await
            .expect("retry response");
        assert_eq!(response.status(), StatusCode::OK);
        let response = router
            .clone()
            .oneshot(request(
                "POST",
                "/api/v1/process-recovery/web/accept",
                &admin.token,
                r#"{"id":"a","version":"stale"}"#,
            ))
            .await
            .expect("accept response");
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let response = router
            .oneshot(request(
                "POST",
                "/api/v1/process-recovery/web/accept",
                &admin.token,
                r#"{"id":"a","version":"v1"}"#,
            ))
            .await
            .expect("accept response");
        assert_eq!(response.status(), StatusCode::OK);
        let before = calls.lock().expect("fixture calls lock").len();
        let response = router_for(crate::WebMode::Exhibit)
            .oneshot(request(
                "GET",
                "/api/v1/process-recovery/web",
                &admin.token,
                "",
            ))
            .await
            .expect("exhibit response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(calls.lock().expect("fixture calls lock").len(), before);
        assert_eq!(
            *calls.lock().expect("fixture calls lock"),
            ["list", "retry:a:v1", "accept:a:stale", "accept:a:v1"]
        );
    }

    #[tokio::test]
    async fn voice_start_checks_both_owner_ledgers_without_starting_a_provider() {
        for (web_pending, daemon_case, expected) in [
            (false, "empty", vec![]),
            (true, "empty", vec!["web"]),
            (false, "pending", vec!["daemon"]),
            (true, "pending", vec!["daemon", "web"]),
            (false, "unavailable", vec!["daemon"]),
            (false, "malformed", vec!["daemon"]),
        ] {
            let temp = tempfile::tempdir().expect("temporary home");
            let home = HomeLayout::from_path(temp.path()).expect("home");
            home.initialize().expect("initialize");
            let recovery = crate::VoiceProcessRecovery {
                reconcile: Arc::new(move || {
                    if web_pending {
                        Err(std::io::Error::other("fixture pending"))
                    } else {
                        Ok(())
                    }
                }),
                review: Arc::new(|_| Err(RecoveryError::Unavailable)),
                review_daemon_offline: Arc::new(move |_| match daemon_case {
                    "empty" => Ok(Some(
                        json!({"owner":"daemon","records":[],"diagnostics":[]}),
                    )),
                    "pending" => Ok(Some(
                        json!({"owner":"daemon","records":[{"id":"old"}],"diagnostics":[]}),
                    )),
                    "malformed" => Ok(Some(json!({"owner":"daemon","records":[]}))),
                    _ => Err(RecoveryError::Unavailable),
                }),
                check_daemon_guard: true,
            };
            let (shutdown, _) = tokio::sync::broadcast::channel(1);
            let state = crate::app_with_shutdown_and_recovery(
                home,
                shutdown,
                crate::WebMode::Normal,
                None,
                crate::LiveBinding {
                    host: "127.0.0.1".into(),
                    port: 0,
                },
                "test".into(),
                Some(recovery),
            )
            .3;
            let result = ensure_voice_start_allowed(&state).await;
            if expected.is_empty() {
                assert!(result.is_ok());
                continue;
            }
            use axum::response::IntoResponse;
            let response = result.expect_err("fixture must block").into_response();
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "{web_pending}/{daemon_case}"
            );
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            let body: Value = serde_json::from_slice(&bytes).expect("JSON");
            assert_eq!(body["error"]["code"], "codex_voice_recovery_pending");
            assert_eq!(body["error"]["details"]["recovery_owners"], json!(expected));
        }
    }
}
