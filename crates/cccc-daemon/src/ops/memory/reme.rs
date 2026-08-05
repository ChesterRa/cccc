use std::fs;
use std::time::Instant;

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::integration_state;
use cccc_core::memory::MemoryStore;
use serde_json::{Value, json};
use sha2::Digest;

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

pub(super) fn reme_search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let started = Instant::now();
    let group_id = required_arg(request, "group_id")?;
    let query = required_arg(request, "query")?;
    let limit = request
        .args
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 50) as usize;
    let min_score = request
        .args
        .get("min_score")
        .and_then(Value::as_f64)
        .unwrap_or(0.1)
        .clamp(0.0, 1.0);
    let hits = MemoryStore::new(home.clone())
        .search(&group_id, &query, limit)
        .map_err(OpError::io)?
        .into_iter()
        .filter(|hit| hit.score >= min_score)
        .map(|hit| json!({"path":hit.path,"start_line":hit.start_line,"end_line":hit.start_line,"score":hit.score,"snippet":hit.snippet,"source":"memory","metadata":{}}))
        .collect::<Vec<_>>();
    object(json!({"count":hits.len(),"hits":hits,"took_ms":started.elapsed().as_millis()}))
}

pub(super) fn reme_get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let relative = required_arg(request, "path")?;
    let offset = request
        .args
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(1) as usize;
    let limit = request
        .args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(200)
        .clamp(1, 5000) as usize;
    let layout = MemoryStore::new(home.clone())
        .layout(&group_id, None)
        .map_err(OpError::io)?;
    let candidate = layout.root.join(&relative);
    let root = layout.root.canonicalize().map_err(OpError::io)?;
    let path = candidate.canonicalize().map_err(OpError::io)?;
    if !path.starts_with(&root) || path.extension().is_none_or(|ext| ext != "md") {
        return Err(OpError::new(
            "invalid_args",
            "memory path escapes the memory root",
        ));
    }
    let text = fs::read_to_string(&path).map_err(OpError::io)?;
    let lines = text.lines().collect::<Vec<_>>();
    let content = lines
        .iter()
        .skip(offset - 1)
        .take(limit)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    object(
        json!({"path":relative,"offset":offset,"limit":limit,"total_lines":lines.len(),"content":content}),
    )
}

pub(super) fn context_check(request: &DaemonRequest) -> OpResult {
    let _ = required_arg(request, "group_id")?;
    let messages = request
        .args
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "messages must be an array"))?;
    let window = request
        .args
        .get("context_window_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(128_000)
        .max(1024) as usize;
    let reserve = request
        .args
        .get("reserve_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(36_000) as usize;
    let keep = request
        .args
        .get("keep_recent_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(20_000)
        .max(256) as usize;
    let tokens = messages.iter().map(estimate_tokens).sum::<usize>();
    let threshold = window.saturating_sub(reserve);
    let needs = tokens > threshold;
    let mut remaining = tokens;
    let mut cut = 0;
    if needs {
        while cut < messages.len() && remaining > keep {
            remaining = remaining.saturating_sub(estimate_tokens(&messages[cut]));
            cut += 1;
        }
    }
    object(
        json!({"needs_compaction":needs,"token_count":tokens,"threshold":threshold,"messages_to_summarize":messages[..cut].to_vec(),"turn_prefix_messages":[],"left_messages":messages[cut..].to_vec(),"is_split_turn":false,"cut_index":cut}),
    )
}

pub(super) fn compact(request: &DaemonRequest) -> OpResult {
    let _ = required_arg(request, "group_id")?;
    let messages = request
        .args
        .get("messages_to_summarize")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "messages_to_summarize must be an array"))?;
    let previous = string_arg(request, "previous_summary").unwrap_or_default();
    let prefix = request
        .args
        .get("turn_prefix_messages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let transcript = prefix
        .iter()
        .chain(messages)
        .filter_map(|item| item.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    if request
        .args
        .get("return_prompt")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return object(
            json!({"prompt":{"system":"Summarize the conversation faithfully and preserve decisions, constraints, and unfinished work.","user":format!("Previous summary:\n{previous}\n\nConversation:\n{transcript}")}}),
        );
    }
    let summary = [previous.trim(), transcript.trim()]
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    object(json!({"summary":summary}))
}

pub(super) fn daily_flush(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let messages = request
        .args
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "messages must be an array"))?;
    let content = messages
        .iter()
        .filter_map(|item| item.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let intent = string_arg(request, "dedup_intent").unwrap_or_else(|| "new".into());
    if request
        .args
        .get("return_prompt")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return object(
            json!({"prompt":{"system":"Summarize durable facts and decisions for daily memory.","user":content}}),
        );
    }
    if content.trim().is_empty() || intent == "silent" {
        return object(
            json!({"status":"silent","reason":"empty_summary","target_file":"","content_hash":"","bytes_written":0,"dedup":{"intent":intent,"decision":"silent","final_decision":"silent","final_reason":"empty_summary","hits":[]}}),
        );
    }
    let (path, hash, deduped) = MemoryStore::new(home.clone())
        .write(
            &group_id,
            "daily",
            &content,
            string_arg(request, "date").as_deref(),
        )
        .map_err(OpError::io)?;
    object(
        json!({"status":if deduped{"silent"}else{"written"},"reason":if deduped{"persistence_content_hash"}else{""},"target_file":path,"content_hash":hash,"bytes_written":if deduped{0}else{content.len()},"signal_pack":request.args.get("signal_pack").cloned().unwrap_or(Value::Null),"dedup":{"intent":intent,"decision":if deduped{"silent"}else{"new"},"final_decision":if deduped{"silent"}else{"new"},"final_reason":if deduped{"persistence_content_hash"}else{"accepted"},"hits":[]}}),
    )
}

fn estimate_tokens(message: &Value) -> usize {
    message
        .get("content")
        .and_then(Value::as_str)
        .map_or(1, |text| text.chars().count().div_ceil(4).max(1))
}

pub(super) fn reme_write(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let target = required_arg(request, "target")?;
    if !matches!(target.as_str(), "memory" | "daily") {
        return Err(OpError::new(
            "invalid_args",
            "target must be memory or daily",
        ));
    }
    let content = required_arg(request, "content")?;
    let mode = string_arg(request, "mode").unwrap_or_else(|| "append".into());
    if !matches!(mode.as_str(), "append" | "replace") {
        return Err(OpError::new(
            "invalid_args",
            "mode must be append or replace",
        ));
    }
    let date = string_arg(request, "date");
    if target == "daily" && date.as_deref().is_none_or(str::is_empty) {
        return Err(OpError::new(
            "invalid_args",
            "date is required when target=daily",
        ));
    }
    let intent = string_arg(request, "dedup_intent").unwrap_or_else(|| "new".into());
    if !matches!(intent.as_str(), "new" | "update" | "supersede" | "silent") {
        return Err(OpError::new("invalid_args", "invalid dedup_intent"));
    }
    let store = MemoryStore::new(home.clone());
    let layout = store
        .layout(&group_id, date.as_deref())
        .map_err(OpError::io)?;
    let path = if target == "daily" {
        layout.today_file
    } else {
        layout.memory_file
    };
    let hash = format!("{:x}", sha2::Sha256::digest(content.trim().as_bytes()));
    let idempotency = string_arg(request, "idempotency_key").unwrap_or_default();
    let group_store = cccc_core::GroupStore::new(home.clone()).map_err(OpError::io)?;
    let state = integration_state::group_get(&group_store, &group_id, "memory_reme_idempotency")
        .map_err(OpError::io)?;
    let previous = (!idempotency.is_empty())
        .then(|| state.get(&idempotency))
        .flatten();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let (status, reason, line_count) = if intent == "silent" {
        ("silent", "precheck_silent", 0)
    } else if previous.is_some() {
        ("silent", "persistence_idempotency_key", 0)
    } else if existing.contains(content.trim()) {
        ("silent", "persistence_content_hash", 0)
    } else {
        if mode == "replace" {
            fs::write(&path, format!("{}\n", content.trim())).map_err(OpError::io)?;
        } else {
            store
                .write(&group_id, &target, &content, date.as_deref())
                .map_err(OpError::io)?;
        }
        if !idempotency.is_empty() {
            integration_state::group_update(
                &group_store,
                &group_id,
                "memory_reme_idempotency",
                |value| {
                    if !value.is_object() {
                        *value = json!({});
                    }
                    value[&idempotency] = json!({"content_hash":hash,"file_path":path});
                    Ok(())
                },
            )
            .map_err(OpError::io)?;
        }
        ("written", "accepted", content.lines().count())
    };
    object(json!({
        "file_path":path,"line_count":line_count,"content_hash":hash,"status":status,
        "reason":if status=="silent"{reason}else{""},
        "dedup":{"intent":intent,"query":string_arg(request,"dedup_query").unwrap_or_default(),"candidate_count":usize::from(previous.is_some()),"top_score":if previous.is_some(){1.0}else{0.0},"precheck_decision":if status=="silent"{"silent"}else{"new"},"final_decision":if status=="silent"{"silent"}else{intent.as_str()},"final_reason":reason,"decision":if status=="silent"{"silent"}else{intent.as_str()},"hits":[]}
    }))
}
