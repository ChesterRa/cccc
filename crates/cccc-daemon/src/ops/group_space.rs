use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::{GroupStore, HomeLayout, ledger};
use cccc_core::{integration_state, space_credentials};
use serde_json::{Map, Value, json};
use std::io;
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

mod artifacts;
mod notebooklm;
mod operations;
mod provider_ops;
mod state;
mod sync;

use state::*;

const KEY: &str = "group_space";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_space_status" => status(home, request),
        "group_space_capabilities" => capabilities(request),
        "group_space_bind" => bind(home, request),
        "group_space_ingest" => operations::ingest(home, request),
        "group_space_query" => operations::query(home, request),
        "group_space_sources" => operations::sources(home, request),
        "group_space_artifact" => operations::artifact(home, request),
        "group_space_jobs" => operations::jobs(home, request),
        "group_space_sync" => operations::sync(home, request),
        "group_space_provider_credential_status" => provider_ops::credential_status(home, request),
        "group_space_provider_credential_update" => provider_ops::credential_update(home, request),
        "group_space_provider_health_check" => provider_ops::provider_health(home, request),
        "group_space_spaces" => provider_ops::spaces(home, request),
        "group_space_provider_auth" => provider_ops::provider_auth(home, request),
        _ => return None,
    })
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let provider = provider(request);
    let value = load(home, &group_id)?;
    let local = provider == "local";
    let auth_configured = local
        || space_credentials::status(home, &provider).map_err(OpError::io)?["configured"]
            .as_bool()
            .unwrap_or(false);
    let provider_record = integration_state::global_get(home, "space_providers")
        .map_err(OpError::io)?
        .get(&provider)
        .cloned()
        .unwrap_or_else(|| json!({}));
    let configured = auth_configured && provider_record["healthy"].as_bool() != Some(false);
    object(json!({
        "group_id":group_id,
        "provider":{"provider":provider,"enabled":configured,"real_enabled":!local,"real_adapter_enabled":!local,"auth_configured":auth_configured,"mode":if local{"local"}else if configured{"active"}else{"disabled"},"write_ready":configured,"readiness_reason":if !auth_configured{"credential missing"}else if configured{"ready"}else{"health check failed"},"last_health_at":provider_record["last_health_at"],"last_error":provider_record["last_error"]},
        "bindings":value["bindings"],
        "queue_summary":{"work":summary(&value),"memory":summary(&value)},
        "sync":value.get("sync").cloned().unwrap_or(json!({"available":false,"converged":false,"reason":"provider_unavailable"}))
    }))
}
fn capabilities(request: &DaemonRequest) -> OpResult {
    let provider = provider(request);
    let local = provider == "local";
    object(json!({
        "provider":provider,
        "capabilities":json!(["bind","ingest","query","sources","artifact","jobs","sync"]),
        "unavailable_capabilities":json!([]),
        "mode":if local{"local"}else{"remote"}
    }))
}
fn bind(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let provider = provider(request);
    let action = string_arg(request, "action").unwrap_or_else(|| "bind".into());
    let mut remote = string_arg(request, "remote_space_id").unwrap_or_default();
    if action != "unbind" && provider == "notebooklm" && remote.is_empty() {
        let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
        let group = store.load(&group_id).map_err(OpError::io)?;
        remote = notebooklm::create_notebook(home, &format!("{} - {}", group.title, lane))?.id;
    }
    update(home, &group_id, |value| {
        let bindings = map_field(root(value), "bindings");
        if action == "unbind" {
            bindings.remove(&lane);
        } else {
            bindings.insert(lane.clone(),json!({"group_id":group_id,"provider":provider,"lane":lane,"remote_space_id":remote,"status":"bound","bound_at":utc_now()}));
        }
        Ok(())
    })?;
    status(home, request)
}
