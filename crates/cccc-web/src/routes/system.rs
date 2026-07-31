use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiResult, call, object, success};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/ping", get(ping))
        .route("/api/v1/health", get(health))
        .route("/api/v1/ready", get(health))
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

async fn ping(State(state): State<AppState>, Query(query): Query<PingQuery>) -> ApiResult {
    let response = call(&state, "ping", Default::default()).await?;
    let daemon = response.0["result"].clone();
    let mut result = json!({
        "daemon": daemon,
        "version": env!("CARGO_PKG_VERSION"),
        "web": {
            "mode": state.web_mode.as_str(),
            "read_only": state.web_mode.is_read_only()
        }
    });
    if query.include_home {
        result["home"] = json!(state.home.root().to_string_lossy());
    }
    Ok(success(result))
}
async fn health(State(state): State<AppState>) -> ApiResult {
    let mut response = call(&state, "ping", Default::default()).await?;
    response
        .0
        .get_mut("result")
        .and_then(Value::as_object_mut)
        .map(|value| value.insert("status".into(), Value::String("ok".into())));
    Ok(response)
}
async fn runtimes() -> Json<Value> {
    let runtimes = cccc_runtime::detect_runtimes();
    let available = runtimes
        .iter()
        .filter(|runtime| runtime.available)
        .map(|runtime| runtime.name.clone())
        .collect::<Vec<_>>();
    success(json!({"available":available,"runtimes":runtimes}))
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
    use super::observability_patch;
    use serde_json::json;

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
