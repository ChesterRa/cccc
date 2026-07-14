use cccc_contracts::{DaemonRequest, utc_now};
use cccc_core::integration_state;
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};
use std::io;
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

const KEY: &str = "group_space";

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "group_space_status" => status(home, request),
        "group_space_capabilities" => capabilities(request),
        "group_space_bind" => bind(home, request),
        "group_space_ingest" => ingest(home, request),
        "group_space_query" => query(home, request),
        "group_space_sources" => sources(home, request),
        "group_space_artifact" => artifact(home, request),
        "group_space_jobs" => jobs(home, request),
        "group_space_sync" => sync(home, request),
        "group_space_provider_auth" => provider_auth(home, request),
        _ => return None,
    })
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let provider = provider(request);
    let value = load(home, &group_id)?;
    object(
        json!({"group_id":group_id,"provider":{"provider":provider,"enabled":true,"real_enabled":false,"mode":"degraded","write_ready":true},"bindings":value["bindings"],"queue_summary":{"work":summary(&value),"memory":summary(&value)},"sync":value["sync"]}),
    )
}
fn capabilities(request: &DaemonRequest) -> OpResult {
    object(
        json!({"provider":provider(request),"capabilities":["bind","ingest","query","sources","artifact","jobs","sync"],"mode":"degraded"}),
    )
}
fn bind(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let provider = provider(request);
    let action = string_arg(request, "action").unwrap_or_else(|| "bind".into());
    let remote = string_arg(request, "remote_space_id").unwrap_or_default();
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
fn ingest(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let provider = provider(request);
    let kind = string_arg(request, "kind").unwrap_or_else(|| "context_sync".into());
    let payload = request
        .args
        .get("payload")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let idempotency = string_arg(request, "idempotency_key").unwrap_or_default();
    let (job, deduped) = update(home, &group_id, |value| {
        let root = root(value);
        if !idempotency.is_empty()
            && let Some(item) = array_mut(root, "jobs")
                .iter()
                .find(|item| item["idempotency_key"] == idempotency)
        {
            return Ok((item.clone(), true));
        }
        let job = json!({"job_id":format!("gsj_{}",short_id()),"group_id":group_id,"provider":provider,"lane":lane,"remote_space_id":root["bindings"][&lane]["remote_space_id"],"kind":kind,"payload":payload,"idempotency_key":idempotency,"state":"succeeded","attempt":1,"max_attempts":3,"created_at":utc_now(),"updated_at":utc_now()});
        array_mut(root, "jobs").push(job.clone());
        array_mut(root,"sources").push(json!({"source_id":format!("gss_{}",short_id()),"provider":provider,"lane":lane,"title":payload["title"],"kind":kind,"status":"ready","payload":payload,"created_at":utc_now()}));
        Ok((job, false))
    })?;
    object(
        json!({"group_id":group_id,"job_id":job["job_id"],"accepted":true,"completed":true,"deduped":deduped,"job":job,"queue_summary":summary(&load(home,&group_id)?),"provider_mode":"degraded"}),
    )
}
fn query(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let query = required_arg(request, "query")?;
    let lane = lane(request)?;
    let value = load(home, &group_id)?;
    let terms = query
        .to_ascii_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut refs=array(&value,"sources").iter().filter(|item|item["lane"]==lane).filter_map(|item|{let text=item.to_string();let lower=text.to_ascii_lowercase();terms.iter().any(|term|lower.contains(term)).then(||json!({"source_id":item["source_id"],"title":item["title"],"excerpt":text.chars().take(400).collect::<String>()}))}).take(8).collect::<Vec<_>>();
    if refs.is_empty() {
        let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
        for event in ledger::tail(&store.ledger_path(&group_id).map_err(OpError::io)?, 50)
            .map_err(OpError::io)?
            .into_iter()
            .rev()
        {
            let text = serde_json::to_string(&event).unwrap_or_default();
            let lower = text.to_ascii_lowercase();
            if terms.iter().any(|term| lower.contains(term)) {
                refs.push(json!({"event_id":event.id,"kind":event.kind,"excerpt":text.chars().take(400).collect::<String>()}));
                if refs.len() == 8 {
                    break;
                }
            }
        }
    }
    let answer = refs
        .iter()
        .map(|item| item["excerpt"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    object(
        json!({"group_id":group_id,"provider":provider(request),"provider_mode":"degraded","degraded":true,"answer":answer,"references":refs,"error":null}),
    )
}
fn sources(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let action = string_arg(request, "action").unwrap_or_else(|| "list".into());
    if action == "list" {
        let value = load(home, &group_id)?;
        return object(
            json!({"group_id":group_id,"provider":provider(request),"provider_mode":"degraded","action":"list","sources":array(&value,"sources").iter().filter(|item|item["lane"]==lane).cloned().collect::<Vec<_>>()}),
        );
    }
    let id = required_arg(request, "source_id")?;
    let changed = update(home, &group_id, |value| {
        let items = array_mut(root(value), "sources");
        if action == "delete" {
            let before = items.len();
            items.retain(|item| item["source_id"] != id);
            return Ok(before != items.len());
        }
        let item = items
            .iter_mut()
            .find(|item| item["source_id"] == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "source not found"))?;
        if action == "rename" {
            item["title"] = json!(string_arg(request, "new_title").unwrap_or_default());
        }
        item["updated_at"] = json!(utc_now());
        Ok(true)
    })?;
    object(json!({"group_id":group_id,"action":action,"source_id":id,"changed":changed}))
}
fn artifact(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let kind = required_arg(request, "kind")?;
    let item = update(home, &group_id, |value| {
        let item = json!({"artifact_id":format!("gsa_{}",short_id()),"provider":provider(request),"lane":lane,"kind":kind,"title":format!("{kind} artifact"),"status":"completed","created_at":utc_now()});
        array_mut(root(value), "artifacts").push(item.clone());
        Ok(item)
    })?;
    object(
        json!({"group_id":group_id,"action":string_arg(request,"action").unwrap_or_else(||"generate".into()),"kind":kind,"accepted":true,"completed":true,"generate_result":item}),
    )
}
fn jobs(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let action = string_arg(request, "action").unwrap_or_else(|| "list".into());
    if action == "list" {
        let value = load(home, &group_id)?;
        return object(
            json!({"group_id":group_id,"provider":provider(request),"jobs":array(&value,"jobs"),"queue_summary":summary(&value)}),
        );
    }
    let id = required_arg(request, "job_id")?;
    let job = update(home, &group_id, |value| {
        let item = array_mut(root(value), "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        item["state"] = json!(if action == "cancel" {
            "canceled"
        } else {
            "succeeded"
        });
        item["updated_at"] = json!(utc_now());
        Ok(item.clone())
    })?;
    object(json!({"group_id":group_id,"job":job,"queue_summary":summary(&load(home,&group_id)?)}))
}
fn sync(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let result = json!({"available":true,"group_id":group_id,"provider":provider(request),"last_run_at":utc_now(),"converged":true,"unsynced_count":0});
    update(home, &group_id, |value| {
        root(value).insert("sync".into(), result.clone());
        Ok(())
    })?;
    object(
        json!({"group_id":group_id,"provider":provider(request),"sync":result,"sync_result":result}),
    )
}
fn provider_auth(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let provider = provider(request);
    let action = string_arg(request, "action").unwrap_or_else(|| "status".into());
    let value = integration_state::global_update(home, "space_providers", |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let item = value
            .as_object_mut()
            .expect("providers initialized")
            .entry(&provider)
            .or_insert_with(|| json!({}));
        if !item.is_object() {
            *item = json!({});
        }
        if action != "status" {
            item["auth_state"] = json!(if matches!(action.as_str(), "cancel" | "disconnect") {
                "canceled"
            } else {
                "running"
            });
            item["updated_at"] = json!(utc_now());
        }
        Ok(item.clone())
    })
    .map_err(OpError::io)?;
    object(
        json!({"provider":provider,"provider_state":{"provider":provider,"enabled":true,"mode":"degraded"},"credential":{"provider":provider,"configured":value["auth_json"].as_str().is_some_and(|v|!v.is_empty())},"auth":{"provider":provider,"state":value["auth_state"].as_str().unwrap_or("idle"),"updated_at":utc_now()}}),
    )
}

fn load(home: &HomeLayout, group_id: &str) -> Result<Value, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_get(&store, group_id, KEY).map_err(OpError::io)
}
fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_update(&store, group_id, KEY, change).map_err(OpError::io)
}
fn root(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("space initialized");
    for key in ["bindings", "sources", "artifacts", "jobs"] {
        root.entry(key).or_insert_with(|| {
            if key == "bindings" {
                json!({})
            } else {
                json!([])
            }
        });
    }
    root
}
fn map_field<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("map initialized")
}
fn array_mut<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    root.get_mut(key)
        .and_then(Value::as_array_mut)
        .expect("array initialized")
}
fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}
fn summary(value: &Value) -> Value {
    let jobs = array(value, "jobs");
    json!({"pending":jobs.iter().filter(|item|item["state"]=="pending").count(),"running":jobs.iter().filter(|item|item["state"]=="running").count(),"failed":jobs.iter().filter(|item|item["state"]=="failed").count()})
}
fn provider(request: &DaemonRequest) -> String {
    string_arg(request, "provider").unwrap_or_else(|| "notebooklm".into())
}
fn lane(request: &DaemonRequest) -> Result<String, OpError> {
    let value = required_arg(request, "lane")?;
    matches!(value.as_str(), "work" | "memory")
        .then_some(value)
        .ok_or_else(|| OpError::new("invalid_args", "lane must be work or memory"))
}
fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].into()
}
