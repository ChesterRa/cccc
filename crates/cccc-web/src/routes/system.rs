use axum::extract::{Extension, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, call, object, success};
use crate::auth::Principal;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ping", get(ping))
        .route("/api/v1/health", get(health))
        .route("/api/v1/ready", get(ready))
        .route("/api/v1/runtimes", get(runtimes))
        .route(
            "/api/v1/observability",
            get(observability_get).put(observability_update),
        )
        .route(
            "/api/v1/registry/reconcile",
            get(reconcile_preview).post(reconcile_apply),
        )
}

#[derive(serde::Deserialize)]
struct PingQuery {
    #[serde(default)]
    include_home: bool,
}

#[derive(serde::Deserialize)]
struct ReadyQuery {
    #[serde(default)]
    challenge: String,
}

async fn ping(
    State(state): State<AppState>,
    Query(query): Query<PingQuery>,
    principal: Option<Extension<Principal>>,
) -> ApiResult {
    let response = call(&state, "ping", Default::default()).await?;
    if principal.is_none() {
        return Ok(success(json!({"status":"ok"})));
    }
    let include_local_paths = query.include_home && principal.is_some_and(|value| value.is_admin);
    let mut daemon = response.0["result"].clone();
    if !include_local_paths {
        if let Some(value) = daemon.as_object_mut() {
            value.remove("executable");
        }
    }
    let assets = crate::web_assets_info();
    let mut result = json!({
        "daemon": daemon,
        "version": env!("CARGO_PKG_VERSION"),
        "build": cccc_core::build_info::current(),
        "web": {
            "assets_id": assets.as_ref().map(|info| &info.assets_id),
            "entry_script": assets.as_ref().map(|info| &info.entry_script),
            "mode": state.web_mode.as_str(),
            "read_only": state.web_mode.is_read_only()
        }
    });
    if include_local_paths {
        result["home"] = json!(state.home.root().to_string_lossy());
        result["executable"] = json!(std::env::current_exe().ok());
    }
    Ok(success(result))
}
async fn health(
    State(state): State<AppState>,
    principal: Option<Extension<Principal>>,
) -> ApiResult {
    let mut response = call(&state, "ping", Default::default()).await?;
    if principal.is_none() {
        return Ok(success(json!({"status":"ok"})));
    }
    response
        .0
        .get_mut("result")
        .and_then(Value::as_object_mut)
        .map(|value| value.insert("status".into(), Value::String("ok".into())));
    if let Some(value) = response.0["result"].as_object_mut() {
        value.remove("executable");
    }
    Ok(response)
}
async fn ready(
    State(state): State<AppState>,
    Query(query): Query<ReadyQuery>,
    principal: Option<Extension<Principal>>,
) -> Json<Value> {
    if principal.is_some() {
        return success(json!({"web":"ready","runtime_id":state.runtime_id}));
    }
    let proof = cccc_core::web_runtime_proof::sign(&state.runtime_proof_key, &query.challenge);
    success(match proof {
        Some(proof) => json!({"web":"ready","runtime_id":state.runtime_id,"proof":proof}),
        None => json!({"web":"ready"}),
    })
}
async fn runtimes(State(state): State<AppState>) -> ApiResult {
    let result = tokio::task::spawn_blocking(move || {
        runtime_catalog_value(
            cccc_runtime::detect_runtimes()
                .into_iter()
                .map(|runtime| json!(runtime))
                .collect(),
            cccc_daemon::antigravity_acp_setup::installed(&state.home),
        )
    })
    .await
    .map_err(|_| {
        ApiError::unavailable(
            "runtime_detection_failed",
            "Could not detect local Runtimes",
        )
    })?;
    Ok(success(result))
}

fn runtime_catalog_value(runtimes: Vec<Value>, acp_installed: bool) -> Value {
    let available = runtimes
        .iter()
        .filter(|runtime| runtime["available"] == true)
        .map(|runtime| runtime["name"].clone())
        .collect::<Vec<_>>();
    let runtimes = runtimes
        .into_iter()
        .map(|mut value| {
            if value["name"] == "antigravity" {
                value["mode_availability"] =
                    json!({"default":value["available"],"acp":acp_installed});
            } else if matches!(value["name"].as_str(), Some("copilot" | "devin" | "cursor")) {
                value["mode_availability"] =
                    json!({"default":value["available"],"acp":value["available"]});
            }
            value
        })
        .collect::<Vec<_>>();
    json!({"available":available,"runtimes":runtimes})
}
async fn observability_get(State(state): State<AppState>) -> ApiResult {
    call(&state, "observability_get", Default::default()).await
}
async fn observability_update(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    call(
        &state,
        "observability_update",
        object(json!({"by":body.get("by").cloned().unwrap_or_else(|| json!("user")),"patch":observability_patch(&body)})),
    )
    .await
}

fn observability_patch(body: &Value) -> serde_json::Map<String, Value> {
    let mut patch = serde_json::Map::new();
    for key in ["developer_mode", "log_level", "logger_levels"] {
        if let Some(value) = body.get(key) {
            patch.insert(key.into(), value.clone());
        }
    }
    for (request_key, section, nested_key) in [
        (
            "terminal_transcript_per_actor_bytes",
            "terminal_transcript",
            "per_actor_bytes",
        ),
        (
            "terminal_ui_scrollback_lines",
            "terminal_ui",
            "scrollback_lines",
        ),
        (
            "peer_runtime_visibility",
            "runtime_visibility",
            "peer_runtime",
        ),
        (
            "assistant_runtime_visibility",
            "runtime_visibility",
            "assistant_runtime",
        ),
    ] {
        let Some(value) = body.get(request_key) else {
            continue;
        };
        patch
            .entry(section)
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .expect("observability section is an object")
            .insert(nested_key.into(), value.clone());
    }
    patch
}
async fn reconcile_preview(State(state): State<AppState>) -> ApiResult {
    call(
        &state,
        "registry_reconcile",
        object(json!({"remove_missing":false,"by":"user"})),
    )
    .await
}

async fn reconcile_apply(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let remove_missing = body
        .get("remove_missing")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    call(
        &state,
        "registry_reconcile",
        object(json!({
            "remove_missing":remove_missing,
            "by":body.get("by").and_then(Value::as_str).unwrap_or("user")
        })),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{observability_patch, runtime_catalog_value};
    use serde_json::json;

    #[test]
    fn antigravity_catalog_keeps_native_and_acp_detection_independent() {
        for native in [false, true] {
            for acp in [false, true] {
                let result = runtime_catalog_value(
                    vec![json!({"name":"antigravity","available":native})],
                    acp,
                );
                assert_eq!(result["runtimes"][0]["available"], native);
                assert_eq!(
                    result["runtimes"][0]["mode_availability"],
                    json!({"default":native,"acp":acp})
                );
                assert_eq!(
                    result["available"],
                    if native {
                        json!(["antigravity"])
                    } else {
                        json!([])
                    }
                );
            }
        }
    }

    #[test]
    fn catalog_preserves_other_runtime_records() {
        let records = vec![
            json!({"name":"codex","available":true,"recommended_command":"codex","path":"/fixture/codex"}),
            json!({"name":"custom","available":true}),
        ];
        let result = runtime_catalog_value(records.clone(), true);
        assert_eq!(result["runtimes"], json!(records));
        assert_eq!(result["available"], json!(["codex", "custom"]));
    }

    #[tokio::test]
    async fn catalog_recheck_does_not_install_or_create_provider_state() {
        use axum::{
            body::Body,
            http::{Request, StatusCode, header},
        };
        use tower::ServiceExt;
        let temp = tempfile::tempdir().expect("temp");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize");
        let token = cccc_core::access_tokens::AccessTokenStore::new(home.clone())
            .expect("tokens")
            .create("admin", vec![], true, None)
            .expect("admin");
        let router = crate::app(home.clone());
        for _ in 0..2 {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/api/v1/runtimes")
                        .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
            let info = value["result"]["runtimes"]
                .as_array()
                .expect("catalog")
                .iter()
                .find(|item| item["name"] == "antigravity")
                .expect("Antigravity");
            assert_eq!(info["mode_availability"]["acp"], false);
            assert_eq!(info["mode_availability"]["default"], info["available"]);
        }
        assert!(!home.root().join("runtimes/antigravity-acp").exists());
        assert!(!home.root().join("state/antigravity-acp").exists());
    }

    #[test]
    fn observability_update_maps_flat_request_fields_to_persisted_sections() {
        let patch = observability_patch(&json!({
            "by": "user",
            "developer_mode": false,
            "terminal_transcript_per_actor_bytes": 10485760,
            "terminal_ui_scrollback_lines": 8000,
            "peer_runtime_visibility": "visible",
            "assistant_runtime_visibility": "visible"
        }));

        assert_eq!(
            json!(patch),
            json!({
                "developer_mode": false,
                "terminal_transcript": {"per_actor_bytes": 10485760},
                "terminal_ui": {"scrollback_lines": 8000},
                "runtime_visibility": {
                    "peer_runtime": "visible",
                    "assistant_runtime": "visible"
                }
            })
        );
    }
}
