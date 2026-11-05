use axum::extract::{Extension, Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call, object};
use crate::auth::Principal;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/actors", get(list).post(create))
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}",
            post(update).delete(remove),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/start",
            post(start),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/stop",
            post(stop),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/restart",
            post(restart),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/runtime_switch",
            post(runtime_switch),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/runtime_switch/complete",
            post(runtime_switch_complete),
        )
        .route(
            "/api/v1/groups/{group_id}/actors/{actor_id}/new_session",
            post(new_session),
        )
        .merge(super::actor_assets::routes())
        .merge(super::actor_profiles::routes())
}

#[derive(Default, serde::Deserialize)]
struct ActorListQuery {
    #[serde(default)]
    include_unread: bool,
    #[serde(default)]
    include_internal: bool,
}

async fn list(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<ActorListQuery>,
) -> ApiResult {
    call(
        &state,
        "actor_list",
        object(json!({
            "group_id":group_id,
            "by":"user",
            "include_unread":query.include_unread,
            "include_internal":query.include_internal,
        })),
    )
    .await
}
async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path(group_id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    normalize_command(&mut args)?;
    args.insert("group_id".into(), Value::String(group_id));
    call(
        &state,
        "actor_add",
        super::actor_profiles::auth_args(&principal, args),
    )
    .await
}
async fn update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let mut args = body_object(body)?;
    normalize_command(&mut args)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("actor_id".into(), Value::String(actor_id));
    call(
        &state,
        "actor_update",
        super::actor_profiles::auth_args(&principal, args),
    )
    .await
}
async fn remove(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    let mut response = lifecycle(&state, &group_id, &actor_id, "actor_remove").await?;
    let key = format!("web-model::{group_id}::{actor_id}");
    if let Err(error) = state.browser_surfaces.close(&key).await {
        response.0["result"]["browser_cleanup_warning"] = Value::String(error.to_string());
    }
    Ok(response)
}
async fn start(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_start").await
}
async fn stop(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_stop").await
}
async fn restart(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_restart").await
}

fn require_runtime_switch_admin(state: &AppState, principal: &Principal) -> Result<(), ApiError> {
    if state.web_mode.is_read_only() || !principal.current_admin(&state.home).unwrap_or(false) {
        return Err(ApiError::forbidden(
            "Runtime switching is available only to this instance's administrator",
        ));
    }
    Ok(())
}

async fn runtime_switch(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((group_id, actor_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    require_runtime_switch_admin(&state, &principal)?;
    let args = runtime_switch_args(&principal, group_id, actor_id, body)?;
    call(&state, "actor_runtime_switch", args).await
}

async fn runtime_switch_complete(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    require_runtime_switch_admin(&state, &principal)?;
    call(
        &state,
        "actor_runtime_switch_complete",
        super::actor_profiles::auth_args(
            &principal,
            object(json!({"group_id":group_id,"actor_id":actor_id})),
        ),
    )
    .await
}

fn runtime_switch_args(
    principal: &Principal,
    group_id: String,
    actor_id: String,
    body: Value,
) -> Result<Map<String, Value>, ApiError> {
    let mut args = body_object(body)?;
    args.retain(|key, _| {
        matches!(
            key.as_str(),
            "expected_actor_generation"
                | "expected_actor_updated_at"
                | "expected_source_id"
                | "target"
        )
    });
    if let Some(Value::Object(target)) = args.get_mut("target")
        && let Some(Value::Object(patch)) = target.get_mut("patch")
    {
        normalize_command(patch)?;
    }
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("actor_id".into(), Value::String(actor_id));
    Ok(super::actor_profiles::auth_args(principal, args))
}
async fn new_session(
    State(state): State<AppState>,
    Path((group_id, actor_id)): Path<(String, String)>,
) -> ApiResult {
    lifecycle(&state, &group_id, &actor_id, "actor_new_session").await
}
async fn lifecycle(state: &AppState, group_id: &str, actor_id: &str, op: &str) -> ApiResult {
    call(
        state,
        op,
        object(json!({"group_id":group_id,"actor_id":actor_id,"by":"user"})),
    )
    .await
}

fn normalize_command(args: &mut Map<String, Value>) -> Result<(), crate::api::ApiError> {
    if let Some(Value::String(command)) = args.get("command").cloned() {
        let command = shell_words::split(&command).map_err(|error| {
            crate::api::ApiError::bad_code("invalid_command", error.to_string(), json!({}))
        })?;
        args.insert("command".into(), json!(command));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::runtime_switch_args;
    use crate::auth::Principal;
    use serde_json::json;

    #[test]
    fn runtime_switch_binds_actor_and_authority_to_the_route_and_principal() {
        let principal = Principal {
            user_id: "admin".into(),
            allowed_groups: vec!["g_actual".into()],
            is_admin: true,
            raw_token: String::new(),
        };
        let args = runtime_switch_args(
            &principal,
            "g_actual".into(),
            "real-actor".into(),
            json!({
                "group_id":"g_other",
                "actor_id":"other",
                "by":"other",
                "caller_id":"forged",
                "is_admin":false,
                "allowed_groups":["g_other"],
                "expected_actor_generation":"generation-1",
                "expected_actor_updated_at":"2026-01-01T00:00:00Z",
                "expected_source_id":"codex:old",
                "target":{"kind":"managed","patch":{"runtime":"custom","command":"tool --model 'a b'"}}
            }),
        )
        .expect("object body");
        assert_eq!(args["group_id"], "g_actual");
        assert_eq!(args["actor_id"], "real-actor");
        assert_eq!(args["by"], "user");
        assert_eq!(args["caller_id"], "admin");
        assert_eq!(args["is_admin"], true);
        assert_eq!(args["allowed_groups"], json!(["g_actual"]));
        assert_eq!(
            args["target"]["patch"]["command"],
            json!(["tool", "--model", "a b"])
        );
    }
}
