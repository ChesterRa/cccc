use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::memory::MemoryStore;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "memory_search" | "memory_reme_search" => search(home, request),
        "memory_get" | "memory_reme_get" => get(home, request),
        "memory_write" | "memory_reme_write" => write(home, request),
        "memory_health" => health(home, request),
        "memory_profile_get" => profile(home, request),
        "memory_reme_layout_get" => layout(home, request),
        "memory_reme_index_sync" => index(home, request),
        "memory_reme_context_check" => search(home, request),
        "memory_reme_compact" | "memory_reme_daily_flush" => compact(home, request),
        _ => return None,
    })
}

fn search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let query = required_arg(request, "query")?;
    let limit = request
        .args
        .get("limit")
        .or_else(|| request.args.get("max_results"))
        .and_then(Value::as_u64)
        .unwrap_or(20) as usize;
    let hits = MemoryStore::new(home.clone())
        .search(&group_id, &query, limit)
        .map_err(OpError::io)?;
    object(json!({"hits": hits, "source": "rust-local-index"}))
}

fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let target = string_arg(request, "target").unwrap_or_else(|| "memory".into());
    let (path, content) = MemoryStore::new(home.clone())
        .get(&group_id, &target, string_arg(request, "date").as_deref())
        .map_err(OpError::io)?;
    object(json!({"path": path, "content": content, "offset": 1, "limit": content.lines().count()}))
}

fn write(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let target = string_arg(request, "target").unwrap_or_else(|| "memory".into());
    let content = string_arg(request, "content")
        .or_else(|| string_arg(request, "text"))
        .ok_or_else(|| OpError::new("invalid_args", "content is required"))?;
    let (path, hash, deduped) = MemoryStore::new(home.clone())
        .write(
            &group_id,
            &target,
            &content,
            string_arg(request, "date").as_deref(),
        )
        .map_err(OpError::io)?;
    object(
        json!({"status": "written", "path": path, "contentHash": hash, "dedup": {"deduped": deduped}}),
    )
}

fn health(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let layout = MemoryStore::new(home.clone())
        .layout(&group_id, None)
        .map_err(OpError::io)?;
    object(json!({"ok": true, "backend": "rust-local", "layout": layout}))
}

fn profile(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let mut forwarded = request.clone();
    let query = format!(
        "profile {} {}",
        string_arg(request, "user_id").unwrap_or_default(),
        string_arg(request, "actor_id").unwrap_or_default()
    );
    forwarded.args.insert("query".into(), Value::String(query));
    search(home, &forwarded)
}

fn layout(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let layout = MemoryStore::new(home.clone())
        .layout(
            &required_arg(request, "group_id")?,
            string_arg(request, "date").as_deref(),
        )
        .map_err(OpError::io)?;
    object(json!({"layout": layout}))
}

fn index(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let hits = MemoryStore::new(home.clone())
        .search(&group_id, "#", 100)
        .map_err(OpError::io)?;
    object(json!({"indexed": hits.len(), "backend": "rust-local-index"}))
}

fn compact(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let store = MemoryStore::new(home.clone());
    let (_, daily) = store
        .get(&group_id, "daily", string_arg(request, "date").as_deref())
        .map_err(OpError::io)?;
    let (_, _, deduped) = store
        .write(&group_id, "memory", &daily, None)
        .map_err(OpError::io)?;
    object(json!({"compacted": true, "deduped": deduped}))
}
