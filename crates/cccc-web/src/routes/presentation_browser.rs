use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use cccc_core::GroupStore;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

#[derive(Debug, Deserialize)]
struct SlotQuery {
    slot: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    viewer_mode: String,
    connect_frame: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/groups/{group_id}/presentation/browser_surface/session",
            get(info).post(open),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/browser_surface/session/close",
            axum::routing::post(close),
        )
        .route(
            "/api/v1/groups/{group_id}/presentation/browser_surface/ws",
            get(upgrade),
        )
}

async fn info(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SlotQuery>,
) -> ApiResult {
    validate_group(&state, &group_id)?;
    let slot = normalize_slot(&query.slot)?;
    Ok(success(
        json!({"group_id":group_id,"browser_surface":state.browser_surfaces.info(&key(&group_id,&slot)).await}),
    ))
}

async fn open(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_group(&state, &group_id)?;
    let slot = normalize_slot(body.get("slot").and_then(Value::as_str).unwrap_or(""))?;
    let url = body.get("url").and_then(Value::as_str).unwrap_or("");
    let reuse_existing = match body.get("reuse_existing") {
        None | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(_) => return Err(ApiError::bad("reuse_existing must be a boolean")),
    };
    let width = body
        .get("width")
        .and_then(Value::as_u64)
        .unwrap_or(1280)
        .clamp(640, 2560) as u32;
    let height = body
        .get("height")
        .and_then(Value::as_u64)
        .unwrap_or(800)
        .clamp(480, 1600) as u32;
    let profile = state
        .home
        .root()
        .join("state/presentation_browser")
        .join(&group_id)
        .join(&slot)
        .join("profile");
    let surface_key = key(&group_id, &slot);
    let surface = state
        .browser_surfaces
        .open_presentation(&surface_key, &profile, url, width, height, reuse_existing)
        .await
        .map_err(|error| {
            if error.is::<crate::browser_surface::BrowserSurfaceContentChanged>() {
                ApiError::conflict(
                    "presentation_browser_content_changed",
                    "The shared browser is showing another published address.",
                    json!({}),
                )
            } else {
                ApiError::bad(error.to_string())
            }
        })?;
    Ok(success(
        json!({"group_id":group_id,"browser_surface":surface}),
    ))
}

async fn close(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    validate_group(&state, &group_id)?;
    let slot = normalize_slot(body.get("slot").and_then(Value::as_str).unwrap_or(""))?;
    let closed = state
        .browser_surfaces
        .close(&key(&group_id, &slot))
        .await
        .map_err(|error| ApiError::bad(error.to_string()))?;
    Ok(success(
        json!({"group_id":group_id,"closed":closed,"browser_surface":state.browser_surfaces.info(&key(&group_id,&slot)).await}),
    ))
}

async fn upgrade(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<SlotQuery>,
    Extension(principal): Extension<crate::auth::Principal>,
    ws: WebSocketUpgrade,
) -> Response {
    if state.web_mode.is_read_only() {
        return ws.on_upgrade(|socket| async move {
            crate::readonly::reject_socket(
                socket,
                "read_only_browser_surface",
                "Presentation browser surface is disabled in read-only mode.",
            )
            .await;
        });
    }
    if let Err(error) = validate_group(&state, &group_id) {
        return error.into_response();
    }
    let slot = match normalize_slot(&query.slot) {
        Ok(slot) => slot,
        Err(error) => return error.into_response(),
    };
    let vnc = query.mode.trim().eq_ignore_ascii_case("vnc");
    ws.on_upgrade(move |socket| async move {
        crate::connect_frames::while_group_access(
            &state,
            &principal,
            &group_id,
            query.connect_frame.as_deref(),
            serve(
                socket,
                state.clone(),
                key(&group_id, &slot),
                vnc,
                query.viewer_mode,
            ),
        )
        .await;
    })
}

async fn serve(socket: WebSocket, state: AppState, key: String, vnc: bool, viewer_mode: String) {
    if vnc {
        crate::browser_surface::serve_vnc_socket(
            socket,
            &state.browser_surfaces,
            &key,
            state.shutdown.subscribe(),
        )
        .await;
    } else {
        crate::browser_surface::serve_socket(
            socket,
            &state.browser_surfaces,
            &key,
            &viewer_mode,
            state.shutdown.subscribe(),
        )
        .await;
    }
}

fn validate_group(state: &AppState, group_id: &str) -> Result<(), ApiError> {
    let store =
        GroupStore::new(state.home.clone()).map_err(|error| ApiError::bad(error.to_string()))?;
    match store.load(group_id) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
            ApiError::not_found_code("group_not_found", format!("group not found: {group_id}")),
        ),
        Err(error) => Err(ApiError::bad(error.to_string())),
    }
}

fn normalize_slot(value: &str) -> Result<String, ApiError> {
    let mut slot = value.trim().to_ascii_lowercase().replace('_', "-");
    if slot.chars().all(|character| character.is_ascii_digit()) && !slot.is_empty() {
        slot = format!("slot-{}", slot.parse::<u8>().unwrap_or_default());
    }
    if !matches!(slot.as_str(), "slot-1" | "slot-2" | "slot-3" | "slot-4") {
        return Err(ApiError::bad(
            "slot must be one of: slot-1, slot-2, slot-3, slot-4",
        ));
    }
    Ok(slot)
}

fn key(group_id: &str, slot: &str) -> String {
    format!("{group_id}::{slot}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::HomeLayout;

    #[tokio::test]
    async fn concurrent_open_with_reuse_keeps_the_existing_shared_session() {
        if ![
            "/usr/bin/google-chrome",
            "/usr/bin/chromium",
            "/opt/homebrew/bin/chromium",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ]
        .iter()
        .any(|path| std::path::Path::new(path).is_file())
        {
            return;
        }
        let _chrome_guard = crate::browser_surface::chrome_test_guard().await;
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("group store")
            .create("fixture", "")
            .expect("group");
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (_, _, surfaces, state) = crate::app_with_shutdown(
            home,
            shutdown,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "fixture".into(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("fixture listener");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/", get(|| async { "fixture" }))
                    .route("/{*path}", get(|| async { "fixture" })),
            )
            .await
            .expect("fixture server");
        });
        let body = json!({
            "slot": "slot-1",
            "url": format!("http://{address}/"),
            "reuse_existing": true,
        });
        let (first, second) = tokio::join!(
            open(
                State(state.clone()),
                Path(group.group_id.clone()),
                Json(body.clone())
            ),
            open(
                State(state.clone()),
                Path(group.group_id.clone()),
                Json(body.clone())
            ),
        );
        let first = first.expect("first open").0["result"]["browser_surface"].clone();
        let second = second.expect("second open").0["result"]["browser_surface"].clone();
        assert_eq!(first["state"], "ready");
        assert_eq!(first["initial_url"], format!("http://{address}/"));
        assert!(
            first["started_at"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        assert_eq!(first["started_at"], second["started_at"]);

        // A person's navigation does not change which card the session was opened for.
        surfaces
            .command(
                &key(&group.group_id, "slot-1"),
                &json!({"t":"navigate", "url":format!("http://{address}/other")}),
            )
            .await
            .expect("navigate inside the page");
        let reused = open(
            State(state.clone()),
            Path(group.group_id.clone()),
            Json(body.clone()),
        )
        .await
        .expect("reuse navigated session")
        .0["result"]["browser_surface"]
            .clone();
        assert_eq!(reused["started_at"], first["started_at"]);
        assert_eq!(reused["url"], format!("http://{address}/other"));

        // A changed card may not silently replace the page another client is using.
        let mut new_content = body.clone();
        new_content["url"] = json!(format!("http://{address}/new"));
        let conflict = open(
            State(state.clone()),
            Path(group.group_id.clone()),
            Json(new_content.clone()),
        )
        .await
        .expect_err("changed card requires an explicit replacement");
        assert_eq!(
            conflict.into_response().status(),
            axum::http::StatusCode::CONFLICT
        );
        assert_eq!(
            surfaces.info(&key(&group.group_id, "slot-1")).await["started_at"],
            first["started_at"]
        );

        let mut malformed = body.clone();
        malformed["reuse_existing"] = json!("true");
        let response = open(
            State(state.clone()),
            Path(group.group_id.clone()),
            Json(malformed),
        )
        .await
        .expect_err("invalid reuse flag must not replace a session");
        assert_eq!(
            response.into_response().status(),
            axum::http::StatusCode::BAD_REQUEST
        );
        assert_eq!(
            surfaces.info(&key(&group.group_id, "slot-1")).await["started_at"],
            first["started_at"]
        );

        // Without the new flag the existing explicit-open behavior still replaces the session.
        let mut replace = new_content;
        replace
            .as_object_mut()
            .expect("request body")
            .remove("reuse_existing");
        let replaced = open(
            State(state.clone()),
            Path(group.group_id.clone()),
            Json(replace),
        )
        .await
        .expect("explicit replacement")
        .0["result"]["browser_surface"]
            .clone();
        assert_ne!(replaced["started_at"], first["started_at"]);
        assert_eq!(replaced["initial_url"], format!("http://{address}/new"));
        let old_client = open(
            State(state.clone()),
            Path(group.group_id.clone()),
            Json(body),
        )
        .await
        .expect_err("a stale client must not replace the new page");
        assert_eq!(
            old_client.into_response().status(),
            axum::http::StatusCode::CONFLICT
        );
        assert_eq!(
            surfaces.info(&key(&group.group_id, "slot-1")).await["started_at"],
            replaced["started_at"]
        );
        assert!(
            surfaces
                .close(&key(&group.group_id, "slot-1"))
                .await
                .expect("close")
        );
        server.abort();
    }
}
