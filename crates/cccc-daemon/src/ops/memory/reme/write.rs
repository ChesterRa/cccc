use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::memory::MemoryStore;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

use super::common::{
    EntryInput, append_entry, build_entry, dedup_intent, dedup_precheck, string_list, write_raw,
};

pub(super) fn reme_write(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let target = required_arg(request, "target")?.to_ascii_lowercase();
    if !matches!(target.as_str(), "memory" | "daily") {
        return Err(OpError::new(
            "invalid_args",
            "target must be one of: memory, daily",
        ));
    }
    let content = required_arg(request, "content")?;
    let mode = string_arg(request, "mode")
        .unwrap_or_else(|| "append".into())
        .to_ascii_lowercase();
    if !matches!(mode.as_str(), "append" | "replace") {
        return Err(OpError::new(
            "invalid_args",
            "mode must be one of: append, replace",
        ));
    }
    let date = string_arg(request, "date").filter(|date| !date.trim().is_empty());
    if target == "daily" && date.is_none() {
        return Err(OpError::new(
            "invalid_args",
            "date is required when target=daily",
        ));
    }
    let effective_date = date.clone().unwrap_or_else(today);
    let layout = MemoryStore::new(home.clone())
        .layout(&group_id, date.as_deref())
        .map_err(OpError::io)?;
    let path = if target == "memory" {
        layout.memory_file.clone()
    } else {
        layout.today_file.clone()
    };
    let intent = dedup_intent(request.args.get("dedup_intent"));
    let query = string_arg(request, "dedup_query")
        .filter(|query| !query.trim().is_empty())
        .unwrap_or_else(|| content.clone());
    let dedup = dedup_precheck(home, &group_id, &query, intent.clone());
    if dedup.precheck_is_silent() {
        return object(json!({
            "file_path":path,
            "line_count":0,
            "content_hash":"",
            "status":"silent",
            "reason":"precheck_silent",
            "dedup":dedup.finalize("silent", "precheck_silent"),
        }));
    }

    let idempotency_key = string_arg(request, "idempotency_key")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let mut shadow = None;
    let outcome = if mode == "replace" {
        write_raw(&path, &content, &mode).map_err(OpError::io)?
    } else {
        let mut supersedes = string_list(request.args.get("supersedes"));
        if intent == "supersede" && supersedes.is_empty() {
            supersedes = dedup
                .hits()
                .iter()
                .filter_map(|hit| {
                    let path = hit.get("path")?.as_str()?.trim();
                    (!path.is_empty()).then(|| {
                        format!(
                            "{path}#L{}",
                            hit.get("start_line").and_then(Value::as_u64).unwrap_or(1)
                        )
                    })
                })
                .take(3)
                .collect();
        }
        let actor_id = string_arg(request, "actor_id").unwrap_or_default();
        let entry = build_entry(
            home,
            &group_id,
            EntryInput {
                kind: if target == "memory" {
                    "stable_knowledge"
                } else {
                    "daily_note"
                },
                summary: &content,
                actor_id: &actor_id,
                source_refs: string_list(request.args.get("source_refs")),
                tags: string_list(request.args.get("tags")),
                supersedes,
                date: &effective_date,
            },
        )?;
        let outcome = append_entry(&path, &entry, &idempotency_key).map_err(OpError::io)?;
        if target == "memory" {
            let entry_id = entry
                .get("entry_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let shadow_key = if idempotency_key.is_empty() {
                format!("memory_shadow:{entry_id}")
            } else {
                format!("memory_shadow:{idempotency_key}")
            };
            shadow =
                Some(append_entry(&layout.today_file, &entry, &shadow_key).map_err(OpError::io)?);
        }
        outcome
    };
    let mut result = json!({
        "file_path":outcome.path,
        "line_count":outcome.line_count,
        "content_hash":outcome.content_hash,
        "status":outcome.status,
        "reason":if outcome.status == "silent" { outcome.reason.as_str() } else { "" },
        "dedup":dedup.finalize(&outcome.status, &outcome.reason),
    });
    if let Some(shadow) = shadow {
        result["shadow_daily"] = json!({
            "file_path":shadow.path,
            "status":shadow.status,
            "reason":shadow.reason,
            "content_hash":shadow.content_hash,
        });
    }
    object(result)
}

fn today() -> String {
    cccc_contracts::utc_now()[..10].to_owned()
}
