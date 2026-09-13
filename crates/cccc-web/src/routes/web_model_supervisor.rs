use cccc_contracts::{Actor, ActorRuntime, GroupState, RunnerKind};
use cccc_core::{GroupDoc, GroupStore};
use std::time::Duration;
use tokio::sync::broadcast;

use crate::AppState;

const SUPERVISOR_INTERVAL: Duration = Duration::from_secs(30);

pub(crate) fn spawn(state: AppState) {
    if state.web_mode.is_read_only() {
        return;
    }
    tokio::spawn(async move {
        let mut events = state.ledger_events.subscribe_global();
        let mut shutdown = state.shutdown.subscribe();
        let mut interval = tokio::time::interval(SUPERVISOR_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.recv() => break,
                _ = interval.tick() => ensure_running_actor(&state, None).await,
                event = events.recv() => match event {
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        ensure_running_actor(&state, None).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
}

pub(super) async fn ensure_running_actor(state: &AppState, preferred_group: Option<&str>) {
    for (group_id, actor_id) in running_browser_actors(state, preferred_group) {
        ensure_actor(state, group_id, actor_id).await;
    }
}

async fn ensure_actor(state: &AppState, group_id: String, actor_id: String) {
    if super::web_model_browser::automation_hold(&state.home).is_some() {
        return;
    }
    let Ok(target) = super::web_model_delivery_state::target(state, &group_id, &actor_id) else {
        return;
    };
    if target["kind"] != "new_chat" && target["url"].as_str().is_none_or(str::is_empty) {
        return;
    }
    ensure_relay_decision_reminder(state, &group_id, &actor_id).await;
    // The native queue is checked before opening. This existing tick wakes a
    // short visit, not a resident browser or a second delivery loop.
    super::web_model_delivery::ensure_worker(state.clone(), group_id, actor_id).await;
}

async fn ensure_relay_decision_reminder(state: &AppState, group_id: &str, actor_id: &str) {
    let browser_idle = {
        let _operation = state.browser_surfaces.web_model_operation.lock().await;
        state
            .browser_surfaces
            .relay_surface_idle(super::web_model_browser::surface_key())
            .await
            .unwrap_or(false)
    };
    let mut args = super::web_model_delivery_completion::args(group_id, actor_id);
    args.insert("by".into(), serde_json::json!(actor_id));
    args.insert("browser_idle".into(), serde_json::json!(browser_idle));
    if let Err(error) =
        super::web_model_delivery_completion::call(state, "coordination_relay_remind", args).await
    {
        tracing::warn!(%error, group_id, actor_id, "relay decision reminder check failed");
    }
}

pub(super) fn actor_delivery_enabled(state: &AppState, group_id: &str, actor_id: &str) -> bool {
    let Ok(store) = GroupStore::new(state.home.clone()) else {
        return false;
    };
    let Ok(group) = store.load(group_id) else {
        return false;
    };
    let Some(actor) = group.actors.iter().find(|actor| actor.id == actor_id) else {
        return false;
    };
    group_actor_delivery_enabled(state, &group, actor)
}

fn running_browser_actors(
    state: &AppState,
    preferred_group: Option<&str>,
) -> Vec<(String, String)> {
    let Ok(store) = GroupStore::new(state.home.clone()) else {
        return Vec::new();
    };
    let groups: Vec<GroupDoc> =
        if let Some(group_id) = preferred_group.filter(|value| !value.is_empty()) {
            store.load(group_id).into_iter().collect()
        } else {
            store
                .list()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|group| store.load(&group.group_id).ok())
                .collect()
        };
    let mut actors: Vec<_> = groups
        .into_iter()
        .flat_map(|group| {
            group
                .actors
                .iter()
                .filter(|actor| group_actor_delivery_enabled(state, &group, actor))
                .map(|actor| (group.group_id.clone(), actor.id.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    // Reuse the native attempt timestamp: an unchecked group gets a turn before
    // the group that just spent the shared account's last browser visit.
    actors.sort_by_cached_key(|(group, actor)| {
        super::web_model_delivery_state::target(state, group, actor)
            .ok()
            .and_then(|target| {
                target["last_delivery_started_at"]
                    .as_str()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            })
    });
    actors
}

fn group_actor_delivery_enabled(state: &AppState, group: &GroupDoc, actor: &Actor) -> bool {
    if actor.runtime != ActorRuntime::WebModel
        || actor.runner != RunnerKind::Headless
        || !actor.enabled
        || !group_state_allows_delivery(group.running, group.state)
    {
        return false;
    }
    let provider = actor_setting(
        actor,
        &["CCCC_WEB_MODEL_PROVIDER", "CCCC_WEB_MODEL_BROWSER_PROVIDER"],
    );
    let provider = if provider.is_empty() {
        super::web_model_connector_store::for_actor(state, &group.group_id, &actor.id)
            .and_then(|connector| connector["provider"].as_str().map(normalize))
            .unwrap_or_default()
    } else {
        provider
    };
    browser_delivery_requested(actor, &provider)
}

fn browser_delivery_requested(actor: &Actor, provider: &str) -> bool {
    let mode = actor_setting(
        actor,
        &["CCCC_WEB_MODEL_DELIVERY_MODE", "CCCC_WEB_MODEL_DELIVERY"],
    );
    if matches!(
        mode.as_str(),
        "pull" | "native" | "remote_mcp" | "off" | "disabled" | "none"
    ) {
        return false;
    }
    if matches!(
        mode.as_str(),
        "browser" | "chatgpt" | "chatgpt_browser" | "browser_delivery"
    ) {
        return true;
    }
    matches!(
        provider,
        "chatgpt" | "chatgpt_web" | "browser_web_model" | "chatgpt_browser"
    )
}

fn actor_setting(actor: &Actor, names: &[&str]) -> String {
    actor_setting_with_process(actor, names, |name| std::env::var(name).ok())
}

fn actor_setting_with_process(
    actor: &Actor,
    names: &[&str],
    mut process_setting: impl FnMut(&str) -> Option<String>,
) -> String {
    names
        .iter()
        .filter_map(|name| actor.env.get(*name).map(normalize))
        .find(|value| !value.is_empty())
        .or_else(|| {
            names
                .iter()
                .filter_map(|name| process_setting(name).map(normalize))
                .find(|value| !value.is_empty())
        })
        .unwrap_or_default()
}

fn group_state_allows_delivery(running: bool, state: GroupState) -> bool {
    running && !matches!(state, GroupState::Paused | GroupState::Stopped)
}

fn normalize(value: impl AsRef<str>) -> String {
    value.as_ref().trim().to_ascii_lowercase()
}
