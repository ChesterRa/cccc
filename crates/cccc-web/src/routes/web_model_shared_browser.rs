//! The shared browser desktop is a global login/maintenance surface. Actor
//! viewers only expose their fixed Page target, never this shared desktop.
use crate::{
    AppState,
    api::{ApiError, ApiResult, success},
};
use axum::{
    Json, Router,
    extract::{Query, State, ws::WebSocketUpgrade},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};

#[derive(Default, serde::Deserialize)]
struct QueryArgs {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    viewer_mode: String,
    #[serde(default)]
    inspect: bool,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/web-model/shared-browser", get(info))
        .route("/api/v1/web-model/shared-browser/open", post(open))
        .route("/api/v1/web-model/shared-browser/close", post(close))
        .route("/api/v1/web-model/shared-browser/ws", get(upgrade))
}
pub(super) fn provider(value: &str) -> Result<&'static str, ApiError> {
    match value {
        "" | "chatgpt_web" => Ok("chatgpt_web"),
        "grok_web" => Ok("grok_web"),
        _ => Err(ApiError::bad("Unsupported Web Model provider")),
    }
}
fn key(provider: &str) -> &'static str {
    if provider == "grok_web" {
        "web-model-login:grok_web"
    } else {
        "web-model-login"
    }
}
pub(super) fn profile(state: &AppState, provider: &str) -> std::path::PathBuf {
    state
        .home
        .root()
        .join("state/web_model_browser/_shared")
        .join(provider)
        .join("chrome_profile")
}
async fn payload(state: &AppState, provider: &str, inspect: bool) -> ApiResult {
    let mut surface = state.browser_surfaces.info(key(provider)).await;
    let readiness = if inspect && surface["active"] == true {
        state
            .browser_surfaces
            .prompt_readiness(key(provider))
            .await
            .map_err(|e| ApiError::bad(e.to_string()))?
    } else {
        surface["metadata"]["prompt_readiness"].clone()
    };
    if inspect {
        surface = state.browser_surfaces.info(key(provider)).await;
    }
    Ok(success(
        json!({"browser_surface":surface,"browser_session":{"active":surface["active"],"ready":readiness["ready"],"login_required":readiness["login_required"],"verification_required":readiness["verification_required"],"tab_url":surface["url"]}}),
    ))
}
async fn info(State(state): State<AppState>, Query(query): Query<QueryArgs>) -> ApiResult {
    payload(&state, provider(&query.provider)?, query.inspect).await
}
async fn open(
    State(state): State<AppState>,
    Query(query): Query<QueryArgs>,
    Json(_body): Json<Value>,
) -> ApiResult {
    let provider = provider(&query.provider)?;
    state
        .browser_surfaces
        .ensure_open_shared_system(
            key(provider),
            &profile(&state, provider),
            if provider == "grok_web" {
                "https://grok.com/"
            } else {
                "https://chatgpt.com/"
            },
            1366,
            900,
        )
        .await
        .map_err(|e| ApiError::bad(format!("{e:#}")))?;
    payload(&state, provider, false).await
}
async fn close(State(state): State<AppState>, Query(query): Query<QueryArgs>) -> ApiResult {
    let provider = provider(&query.provider)?;
    // The settings action closes the owned login window, not the shared
    // process. Actor windows and authenticated profile remain available.
    state
        .browser_surfaces
        .close(key(provider))
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    payload(&state, provider, false).await
}
async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<QueryArgs>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if state.web_mode.is_read_only() {
        return Err(ApiError::forbidden(
            "Shared login is unavailable in read-only mode",
        ));
    }
    let provider = provider(&query.provider)?;
    Ok(ws.on_upgrade(move |socket| async move {
        if query.mode == "vnc" {
            crate::browser_surface::serve_vnc_socket(
                socket,
                &state.browser_surfaces,
                key(provider),
                state.shutdown.subscribe(),
            )
            .await;
        } else {
            crate::browser_surface::serve_socket(
                socket,
                &state.browser_surfaces,
                key(provider),
                &query.viewer_mode,
                state.shutdown.subscribe(),
            )
            .await;
        }
    }))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn closing_login_keeps_actor_page_draft_and_shared_login() {
        if crate::system_browser_path().is_none()
            || !std::path::Path::new("/usr/bin/Xvfb").is_file()
        {
            return;
        }
        let _chrome = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("temp");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let (_, _, _, state) = crate::app_with_shutdown(
            home,
            tokio::sync::broadcast::channel(1).0,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "login-close-fixture".into(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listen");
        let url = format!("http://{}/", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(|| async { axum::response::Html("<textarea></textarea>") }),
            )
            .await
        });
        for provider in ["chatgpt_web", "grok_web"] {
            let profile = profile(&state, provider);
            state
                .browser_surfaces
                .ensure_open_shared_system(key(provider), &profile, &url, 800, 600)
                .await
                .expect("login");
            state
                .browser_surfaces
                .ensure_open_shared_actor("actor", &profile, &url, (800, 600), "gen")
                .await
                .expect("actor");
            let page = state
                .browser_surfaces
                .sessions
                .lock()
                .await
                .get("actor")
                .expect("actor")
                .page
                .clone();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if page
                        .evaluate("Boolean(document.querySelector('textarea'))")
                        .await
                        .ok()
                        .and_then(|r| r.into_value::<bool>().ok())
                        == Some(true)
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("fixture ready");
            page.evaluate("document.cookie='fixture_login=shared;Path=/';document.querySelector('textarea').value='keep Actor draft'").await.expect("fixture login and draft");
            let before = state.browser_surfaces.info("actor").await;
            let result = close(
                State(state.clone()),
                Query(QueryArgs {
                    provider: provider.into(),
                    ..Default::default()
                }),
            )
            .await
            .expect("close login route");
            assert_eq!(result.0["result"]["browser_surface"]["active"], false);
            let after = state.browser_surfaces.info("actor").await;
            assert_eq!(after["active"], true);
            assert_eq!(after["metadata"]["pid"], before["metadata"]["pid"]);
            assert!(page.evaluate("document.cookie.includes('fixture_login=shared') && document.querySelector('textarea').value==='keep Actor draft'").await.expect("Actor survives close").into_value::<bool>().expect("bool"));
            // Reopening the settings window must reuse the authenticated owner.
            let reopened = state
                .browser_surfaces
                .ensure_open_shared_system(key(provider), &profile, &url, 800, 600)
                .await
                .expect("reopen login");
            assert_eq!(reopened["metadata"]["pid"], before["metadata"]["pid"]);
            assert!(state.browser_surfaces.page_available("actor").await);
            state
                .browser_surfaces
                .close("actor")
                .await
                .expect("close fixture Actor");
        }
        state
            .browser_surfaces
            .shutdown_all()
            .await
            .expect("cleanup");
        server.abort();
    }
}
