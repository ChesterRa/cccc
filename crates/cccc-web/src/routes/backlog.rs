use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, body_object, call};

/// The backlog operations the Web asks the daemon for. The daemon owns the file,
/// its validation and the Group's authority; this port binds the Group from the
/// route and acts as the user.
const OPERATIONS: &[&str] = &[
    "use", "create", "exclude", "add", "edit", "move", "close", "promote",
];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/groups/{group_id}/backlog", get(backlog_get))
        .route("/api/v1/groups/{group_id}/backlog/{op}", post(backlog_op))
}

#[derive(Default, Deserialize)]
struct BacklogQuery {
    #[serde(default)]
    scope_key: String,
    #[serde(default)]
    scope_url: String,
}

async fn backlog_get(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Query(query): Query<BacklogQuery>,
) -> ApiResult {
    call(&state, "backlog_get", get_args(group_id, query)).await
}

async fn backlog_op(
    State(state): State<AppState>,
    Path((group_id, op)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> ApiResult {
    let args = op_args(group_id, &op, body)?;
    call(&state, &format!("backlog_{op}"), args).await
}

fn get_args(group_id: String, query: BacklogQuery) -> Map<String, Value> {
    let mut args = Map::new();
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("by".into(), Value::String("user".into()));
    for (name, value) in [
        ("scope_key", query.scope_key),
        ("scope_url", query.scope_url),
    ] {
        let value = value.trim();
        if !value.is_empty() {
            args.insert(name.into(), Value::String(value.into()));
        }
    }
    args
}

fn op_args(group_id: String, op: &str, body: Value) -> Result<Map<String, Value>, ApiError> {
    if !OPERATIONS.contains(&op) {
        return Err(ApiError::bad_code(
            "unknown_backlog_operation",
            "unknown backlog operation",
            json!({"op":op}),
        ));
    }
    let mut args = body_object(body)?;
    args.insert("group_id".into(), Value::String(group_id));
    args.insert("by".into(), Value::String("user".into()));
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_bind_the_group_and_the_user_from_the_route() {
        let args = op_args(
            "g_actual".into(),
            "edit",
            json!({
                "group_id":"g_other",
                "by":"codex-1",
                "scope_key":"s_1",
                "entry":{"id":"0123456789ab"},
            }),
        )
        .expect("object body");
        assert_eq!(args["group_id"], "g_actual");
        assert_eq!(args["by"], "user");
        assert_eq!(args["scope_key"], "s_1");
        assert_eq!(args["entry"]["id"], "0123456789ab");
    }

    #[test]
    fn unknown_operations_and_non_object_bodies_are_refused_before_the_daemon() {
        for op in ["get", "delete", "backlog_edit", ""] {
            let error = op_args("g_1".into(), op, json!({})).expect_err(op);
            assert!(
                error.to_string().starts_with("unknown_backlog_operation:"),
                "{op}: {error}"
            );
        }
        assert!(op_args("g_1".into(), "add", json!([])).is_err());
    }

    #[test]
    fn reads_pass_only_the_scope_the_caller_named() {
        let args = get_args(
            "g_1".into(),
            BacklogQuery {
                scope_key: " s_1 ".into(),
                scope_url: String::new(),
            },
        );
        assert_eq!(args["group_id"], "g_1");
        assert_eq!(args["by"], "user");
        assert_eq!(args["scope_key"], "s_1");
        assert!(!args.contains_key("scope_url"));
    }
}
