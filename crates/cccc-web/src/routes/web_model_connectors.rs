use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::Value;

use crate::AppState;
use crate::api::ApiError;

use super::web_model_connector_activity::{self as activity, Activity};
use super::web_model_connector_provisioning as provisioning;
use super::web_model_connector_store as store;

#[derive(Default, serde::Deserialize)]
struct TokenQuery {
    #[serde(default)]
    token: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/web-model/connectors",
            get(provisioning::list).post(provisioning::create),
        )
        .route(
            "/api/v1/web-model/connectors/{connector_id}",
            axum::routing::delete(provisioning::revoke),
        )
        .route("/api/v1/mcp", post(admin_mcp))
        .route(
            "/mcp/web-model/{connector_id}",
            get(mcp_info).post(mcp_with_header).options(mcp_options),
        )
        .route(
            "/mcp/web-model/{connector_id}/token/{secret}",
            get(mcp_info_token)
                .post(mcp_with_path_token)
                .options(mcp_options_token),
        )
}

async fn admin_mcp(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    Json(cccc_mcp::handle_request(&state.home, &body).await)
}

async fn mcp_info(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let secret = connector_secret(&headers, &query)?;
    store::find_authorized(&state, &connector_id, Some(secret))?;
    activity::record(
        &state,
        &connector_id,
        Activity {
            method: "GET",
            tool_name: "",
            call_status: "ok",
            wait_status: "",
            turn_id: "",
            error: "",
        },
    )?;
    Ok(sse_probe_response())
}

async fn mcp_info_token(
    State(state): State<AppState>,
    Path((connector_id, secret)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    store::find_authorized(&state, &connector_id, Some(&secret))?;
    activity::record(
        &state,
        &connector_id,
        Activity {
            method: "GET",
            tool_name: "",
            call_status: "ok",
            wait_status: "",
            turn_id: "",
            error: "",
        },
    )?;
    Ok(sse_probe_response())
}

async fn mcp_with_header(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let secret = connector_secret(&headers, &query)?;
    let connector = store::find_authorized(&state, &connector_id, Some(secret))?;
    run_connector_mcp(&state, &connector, body).await
}

async fn mcp_with_path_token(
    State(state): State<AppState>,
    Path((connector_id, secret)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let connector = store::find_authorized(&state, &connector_id, Some(&secret))?;
    run_connector_mcp(&state, &connector, body).await
}

async fn run_connector_mcp(
    state: &AppState,
    connector: &Value,
    request: Value,
) -> Result<Response, ApiError> {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let mut tool_name = String::new();
    if request.get("method").and_then(Value::as_str) == Some("tools/call") {
        if let Some(params) = request.get("params").and_then(Value::as_object) {
            tool_name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if let Some(arguments) = params.get("arguments").and_then(Value::as_object) {
                let bound_group = connector["group_id"].as_str().unwrap_or("");
                if arguments
                    .get("group_id")
                    .and_then(Value::as_str)
                    .is_some_and(|group_id| group_id != bound_group)
                {
                    return Err(ApiError::forbidden("connector cannot access another group"));
                }
            }
        }
    }
    if request.get("id").is_none() && method.starts_with("notifications/") {
        activity::record(
            state,
            connector["connector_id"].as_str().unwrap_or(""),
            Activity {
                method: &method,
                tool_name: "",
                call_status: "ok",
                wait_status: "",
                turn_id: "",
                error: "",
            },
        )?;
        return Ok(StatusCode::ACCEPTED.into_response());
    }
    let response = cccc_mcp::handle_request_for_actor(
        &state.home,
        &request,
        connector["group_id"].as_str().unwrap_or(""),
        connector["actor_id"].as_str().unwrap_or(""),
    )
    .await;
    let call_status = if response.get("error").is_some()
        || response["result"]["isError"].as_bool().unwrap_or(false)
    {
        "error"
    } else {
        "ok"
    };
    let (wait_status, turn_id, error) = activity::details(&tool_name, &response);
    activity::record(
        state,
        connector["connector_id"].as_str().unwrap_or(""),
        Activity {
            method: &method,
            tool_name: &tool_name,
            call_status,
            wait_status: &wait_status,
            turn_id: &turn_id,
            error: &error,
        },
    )?;
    Ok(Json(response).into_response())
}

async fn mcp_options() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn mcp_options_token() -> StatusCode {
    StatusCode::NO_CONTENT
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn connector_secret<'a>(
    headers: &'a HeaderMap,
    query: &'a TokenQuery,
) -> Result<&'a str, ApiError> {
    bearer(headers)
        .or_else(|| (!query.token.trim().is_empty()).then_some(query.token.trim()))
        .ok_or_else(|| ApiError::forbidden("connector token required"))
}

fn sse_probe_response() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from(": cccc web-model connector ready\n\n"))
        .expect("static SSE probe response")
}

pub(super) fn required(body: &Value, key: &str) -> Result<String, ApiError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ApiError::bad(format!("{key} is required")))
}
