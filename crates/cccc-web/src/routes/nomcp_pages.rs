use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use serde::Deserialize;
use serde_json::json;

use super::nomcp::{auth_failure, authorize, failure, formatted, page};
use super::{nomcp_render, nomcp_resources};
use crate::AppState;

#[derive(Debug, Deserialize)]
struct TokenQuery {
    token: String,
    format: Option<String>,
}
#[derive(Debug, Deserialize)]
struct ReadQuery {
    token: String,
    path: String,
    start: Option<usize>,
    end: Option<usize>,
    format: Option<String>,
}
#[derive(Debug, Deserialize)]
struct SearchQuery {
    token: String,
    q: String,
    format: Option<String>,
}
#[derive(Debug, Deserialize)]
struct DiffQuery {
    token: String,
    path: Option<String>,
    format: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/nomcp/s/{sid}", get(home))
        .route("/nomcp/s/{sid}/resources", get(resources))
        .route("/nomcp/s/{sid}/status", get(status))
        .route("/nomcp/s/{sid}/read", get(read))
        .route("/nomcp/s/{sid}/search", get(search))
        .route("/nomcp/s/{sid}/diff", get(diff))
}

async fn home(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<TokenQuery>,
) -> Response {
    let Ok((session, root)) = authorize(&state, &sid, &query.token) else {
        return auth_failure(&state, &sid, &query.token);
    };
    let files = nomcp_resources::resources(&root, &session).unwrap_or_default();
    let status = nomcp_render::git_status(&root);
    let diff = nomcp_render::git_diff(&root, "");
    let body = format!(
        "{}\n\nRecommended Reading Order\n{}\n\nStatus Summary\n{}\n\nDiff Summary\n{}\n\nChanged Files\n{}",
        session.brief,
        files.join("\n"),
        status,
        diff,
        status["changed_files"]
    );
    page(&session.title, &body, query.format.as_deref())
}

async fn resources(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<TokenQuery>,
) -> Response {
    let Ok((session, root)) = authorize(&state, &sid, &query.token) else {
        return auth_failure(&state, &sid, &query.token);
    };
    match nomcp_resources::resources(&root, &session) {
        Ok(files) => page(
            "No-MCP Resources",
            &files
                .iter()
                .map(|path| {
                    format!(
                        "- [{path}](/nomcp/s/{sid}/read?path={}&token={})",
                        path.replace('/', "%2F"),
                        query.token
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            query.format.as_deref(),
        ),
        Err(error) => failure(StatusCode::BAD_REQUEST, error),
    }
}

async fn status(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<TokenQuery>,
) -> Response {
    let Ok((_session, root)) = authorize(&state, &sid, &query.token) else {
        return auth_failure(&state, &sid, &query.token);
    };
    formatted(
        "No-MCP Status",
        json!({"ok":true,"result":{"status":nomcp_render::git_status(&root)}}),
        query.format.as_deref(),
    )
}

async fn read(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<ReadQuery>,
) -> Response {
    let Ok((session, root)) = authorize(&state, &sid, &query.token) else {
        return auth_failure(&state, &sid, &query.token);
    };
    match nomcp_resources::read(
        &root,
        &session,
        &query.path,
        query.start.unwrap_or(1),
        query.end.unwrap_or(0),
    ) {
        Ok(value) => formatted(
            "No-MCP Read",
            json!({"ok":true,"result":value}),
            query.format.as_deref(),
        ),
        Err(error) => failure(error.status, error.message),
    }
}

async fn search(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<SearchQuery>,
) -> Response {
    let Ok((session, root)) = authorize(&state, &sid, &query.token) else {
        return auth_failure(&state, &sid, &query.token);
    };
    match nomcp_resources::search(&root, &session, &query.q) {
        Ok(value) => formatted(
            "No-MCP Search",
            json!({"ok":true,"result":value}),
            query.format.as_deref(),
        ),
        Err(error) => failure(StatusCode::BAD_REQUEST, error),
    }
}

async fn diff(
    State(state): State<AppState>,
    Path(sid): Path<String>,
    Query(query): Query<DiffQuery>,
) -> Response {
    let Ok((_session, root)) = authorize(&state, &sid, &query.token) else {
        return auth_failure(&state, &sid, &query.token);
    };
    formatted(
        "No-MCP Diff",
        json!({"ok":true,"result":nomcp_render::git_diff(&root,query.path.as_deref().unwrap_or(""))}),
        query.format.as_deref(),
    )
}
