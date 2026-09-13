use axum::Json;
use axum::extract::{Path, State};
use cccc_contracts::ActorRuntime;
use cccc_core::{GroupStore, settings};
use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, ApiResult, success};

use super::web_model_connector_store as store;
use super::web_model_connectors::required;

pub(super) async fn list(State(state): State<AppState>) -> ApiResult {
    let mut connectors = store::load(&state)?;
    connectors.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    let base_url = connector_base_url(&state)?;
    Ok(success(json!({
        "connectors": connectors.iter().map(|item| public(item, &base_url)).collect::<Vec<_>>()
    })))
}

pub(super) async fn create(State(state): State<AppState>, Json(body): Json<Value>) -> ApiResult {
    let group_id = required(&body, "group_id")?;
    let actor_id = required(&body, "actor_id")?;
    let group = GroupStore::new(state.home.clone())
        .map_err(store::io_error)?
        .load(&group_id)
        .map_err(|_| ApiError::not_found(format!("group not found: {group_id}")))?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| ApiError::not_found(format!("actor not found: {actor_id}")))?;
    if actor.runtime != ActorRuntime::WebModel {
        return Err(ApiError::bad(
            "web-model connectors require an actor with runtime=web_model",
        ));
    }

    let home = state.home.clone();
    let (connector, replaced) = tokio::task::spawn_blocking(move || {
        cccc_core::web_model_connectors::create(
            &home,
            &group_id,
            &actor_id,
            body.get("provider")
                .and_then(Value::as_str)
                .unwrap_or("chatgpt"),
            body.get("label").and_then(Value::as_str).unwrap_or(""),
        )
    })
    .await
    .map_err(|error| ApiError::bad(error.to_string()))?
    .map_err(store::io_error)?;
    let base_url = connector_base_url(&state)?;
    Ok(success(json!({
        "connector": public(&connector, &base_url),
        "secret": connector["secret"],
        "replaced_connector_ids": replaced
    })))
}

fn connector_base_url(state: &AppState) -> Result<String, ApiError> {
    let settings = settings::load(&state.home).map_err(store::io_error)?;
    let mut base = settings
        .remote_access
        .get("web_public_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_owned();
    if base.ends_with("/ui") {
        base.truncate(base.len() - "/ui".len());
    }
    Ok(base)
}

fn public(item: &Value, base_url: &str) -> Value {
    let mut result = item.as_object().cloned().unwrap_or_default();
    let secret = result
        .remove("secret")
        .and_then(|value| value.as_str().map(str::to_owned));
    result.remove("secret_hash");
    result.insert(
        "session_bound".into(),
        json!(
            item["session_hash"].as_str().is_some_and(|v| !v.is_empty()) && item["revoked"] != true
        ),
    );
    result.insert("session_bound_at".into(), item["session_bound_at"].clone());
    for key in ["session_hash", "previous_session_hash", "binding_code_hash"] {
        result.remove(key);
    }

    let id = item["connector_id"].as_str().unwrap_or("");
    let secret_value = secret.as_deref().unwrap_or_default();
    let connector_path = format!("/mcp/web-model/{id}");
    let connector_url = if base_url.is_empty() {
        connector_path
    } else {
        format!("{base_url}{connector_path}")
    };
    result.insert("secret_available".into(), Value::Bool(secret.is_some()));
    let preview = result
        .get("secret_preview")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    result.insert(
        "secret_preview".into(),
        Value::String(if preview.is_empty() {
            secret.as_deref().map_or(String::new(), |value| {
                if value.len() <= 10 {
                    "****".into()
                } else {
                    format!("{}...{}", &value[..6], &value[value.len() - 4..])
                }
            })
        } else {
            preview
        }),
    );
    result.insert("connector_url".into(), json!(connector_url));
    result.insert(
        "connector_url_path_token".into(),
        json!(format!("{connector_url}/token/{secret_value}")),
    );
    result.insert(
        "connector_url_with_token".into(),
        json!(format!("{connector_url}?token={secret_value}")),
    );
    Value::Object(result)
}

pub(super) async fn revoke(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
) -> ApiResult {
    let home = state.home.clone();
    let id = connector_id.clone();
    let revoked =
        tokio::task::spawn_blocking(move || cccc_core::web_model_connectors::revoke(&home, &id))
            .await
            .map_err(|error| ApiError::bad(error.to_string()))?
            .map_err(store::io_error)?;
    if !revoked {
        return Err(ApiError::not_found("web-model connector not found"));
    }
    Ok(success(json!({"revoked":true,"connector_id":connector_id})))
}

/// Local Web control plane issues a code; the selected Chat consumes it via the gateway.
pub(super) async fn prepare_binding(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
) -> ApiResult {
    let connector = store::find_authorized(&state, &connector_id, None)?;
    let mut binding =
        cccc_core::web_model_connectors::prepare_binding(&state.home, &connector_id, 600)
            .map_err(store::io_error)?;
    binding["group_id"] = connector["group_id"].clone();
    binding["actor_id"] = connector["actor_id"].clone();
    binding["session_bound"] = json!(
        connector["session_hash"]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    );
    Ok(success(binding))
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn revoke_waits_for_send_without_blocking_the_async_executor() {
        use cccc_core::{GroupStore, HomeLayout, web_model_connectors};
        use std::future::{Future, poll_fn};
        use std::task::Poll;
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("async revoke", "").expect("group");
        let mut actor = cccc_contracts::Actor::new("web");
        actor.runtime = cccc_contracts::ActorRuntime::WebModel;
        cccc_core::actors::add(&mut group, actor).expect("actor");
        store.save(&group).expect("save actor");
        let (connector, _) =
            web_model_connectors::create(&home, &group.group_id, "web", "chatgpt", "")
                .expect("connector");
        let id = connector["connector_id"]
            .as_str()
            .expect("F1 revoke fixture value")
            .to_owned();
        web_model_connectors::save_browser_target(
            &home,
            &group.group_id,
            "web",
            Some(serde_json::json!({"kind":"existing_chat","url":"https://chatgpt.com/c/fixture"})),
        )
        .expect("target");
        let (_, owner) =
            web_model_connectors::browser_target_snapshot(&home, &group.group_id, "web")
                .expect("owner");
        let permit =
            web_model_connectors::browser_dispatch_permit(&home, &group.group_id, "web", &owner)
                .expect("permit")
                .expect("current owner");
        let (release, waiting) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let resumed = waiting
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_ok();
            drop(permit);
            resumed
        });
        let (shutdown, _) = tokio::sync::broadcast::channel(1);
        let (_, _, _, state) = crate::app_with_shutdown(
            home.clone(),
            shutdown,
            crate::WebMode::Normal,
            None,
            crate::LiveBinding {
                host: "127.0.0.1".into(),
                port: 0,
            },
            "async-revoke".into(),
        );
        let request = super::revoke(axum::extract::State(state), axum::extract::Path(id));
        tokio::pin!(request);
        let pending = poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx).is_pending())).await;
        let _ = release.send(());
        let executor_progressed = holder.join().expect("permit owner thread");
        assert!(
            pending && executor_progressed,
            "revocation blocked the thread needed to finish Send"
        );
        let response = request.await.expect("revoke after Send");
        assert_eq!(response.0["result"]["revoked"], true);
        assert_eq!(
            web_model_connectors::browser_target(&home, &group.group_id, "web")
                .expect("F1 revoke fixture value"),
            serde_json::json!({})
        );
    }
}
