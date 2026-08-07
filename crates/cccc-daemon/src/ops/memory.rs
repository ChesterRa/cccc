use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::memory::MemoryStore;
use serde_json::{Value, json};
use std::fs;

use crate::dispatch::{OpError, OpResult, first_non_blank_arg, object, required_arg, string_arg};

mod reme;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "memory_search" => search(home, request),
        "memory_reme_search" => reme::reme_search(home, request),
        "memory_get" => get(home, request),
        "memory_reme_get" => reme::reme_get(home, request),
        "memory_write" => write(home, request),
        "memory_reme_write" => reme::reme_write(home, request),
        "memory_health" => health(home, request),
        "memory_profile_get" => profile(home, request),
        "memory_reme_layout_get" => layout(home, request),
        "memory_reme_index_sync" => index(home, request),
        "memory_reme_context_check" => reme::context_check(request),
        "memory_reme_compact" => reme::compact(request),
        "memory_reme_daily_flush" => reme::daily_flush(home, request),
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
    let content = first_non_blank_arg(request, &["content", "text"])
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
    let group_label = layout
        .today_file
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split_once("__"))
        .map(|(_, label)| label)
        .unwrap_or("group");
    object(json!({
        "group_label":group_label,
        "memory_root":layout.root,
        "memory_file":layout.memory_file,
        "daily_dir":layout.daily_dir,
        "today_daily_file":layout.today_file,
        "backend":{"name":"local","vector_enabled":false,"fts_enabled":true},
    }))
}

fn index(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let mode = string_arg(request, "mode").unwrap_or_else(|| "scan".into());
    if !matches!(mode.as_str(), "scan" | "rebuild") {
        return Err(OpError::new("invalid_args", "mode must be scan or rebuild"));
    }
    let layout = MemoryStore::new(home.clone())
        .layout(&group_id, None)
        .map_err(OpError::io)?;
    let mut files = vec![layout.memory_file.clone()];
    files.extend(
        fs::read_dir(&layout.daily_dir)
            .map_err(OpError::io)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md")),
    );
    let chunks = files
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count()
        })
        .sum::<usize>();
    let watched_paths = files.clone();
    object(
        json!({"indexed_files":files.len(),"indexed_chunks":chunks,"watched_paths":watched_paths,"last_sync_at":cccc_contracts::utc_now()}),
    )
}
