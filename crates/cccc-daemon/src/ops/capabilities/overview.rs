use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::capabilities::{Capability, CapabilityStore};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::dispatch::{OpError, OpResult, object, string_arg};

pub(super) fn run(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let store = CapabilityStore::new(home.clone());
    let state = store.load().map_err(OpError::io)?;
    let group_id = string_arg(request, "group_id").unwrap_or_default();
    let effective =
        super::effective_state::load(home, &group_id, "user", &state).map_err(OpError::io)?;
    let query = string_arg(request, "query")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let kind_filter = string_arg(request, "kind").unwrap_or_default();
    let policy_filter = string_arg(request, "policy").unwrap_or_default();
    let source_filter = string_arg(request, "source_id").unwrap_or_default();
    let offset = usize_arg(request, "offset", 0, usize::MAX);
    let limit = usize_arg(request, "limit", 400, 2_000);
    let catalog = store.catalog().map_err(OpError::io)?;
    let removed = store.removed_for_group(&group_id).map_err(OpError::io)?;

    let mut rows = catalog
        .iter()
        .filter(|capability| !removed.contains(&capability.id))
        .cloned()
        .map(|capability| {
            let blocked = effective.blocked.contains(&capability.id);
            overview_row(capability, blocked)
        })
        .filter(|row| overview_matches(row, &query))
        .collect::<Vec<_>>();

    let mut kind_counts = BTreeMap::from([("skill", 0usize), ("mcp", 0), ("pack", 0)]);
    for row in &rows {
        if let Some(count) = kind_counts.get_mut(overview_kind(row)) {
            *count += 1;
        }
    }

    rows.retain(|row| matches_filters(row, &kind_filter, &policy_filter, &source_filter));
    rows.sort_by(|left, right| {
        row_name(left).cmp(&row_name(right)).then_with(|| {
            left["capability_id"]
                .as_str()
                .cmp(&right["capability_id"].as_str())
        })
    });

    let total_count = rows.len();
    let items = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let sources = source_counts(&catalog);
    let blocked_capabilities = effective
        .blocked
        .iter()
        .map(|id| json!({"capability_id":id,"scope":"global"}))
        .collect::<Vec<_>>();

    object(json!({
        "items":items,
        "count":items.len(),
        "total_count":total_count,
        "offset":offset,
        "limit":limit,
        "has_more":offset.saturating_add(items.len()) < total_count,
        "query":query,
        "kind_counts":kind_counts,
        "sources":sources,
        "source_instances":[],
        "blocked_capabilities":blocked_capabilities,
    }))
}

fn overview_row(capability: Capability, blocked: bool) -> Value {
    let kind = if capability.kind.is_empty() {
        inferred_kind(&capability.id).to_owned()
    } else {
        capability.kind
    };
    let tool_count = capability.tool_names.len();
    json!({
        "capability_id":capability.id,
        "kind":kind,
        "name":capability.name,
        "description_short":capability.description,
        "source_id":capability.source,
        "source_uri":capability.source_uri,
        "tags":capability.tags,
        "capsule_text":capability.capsule_text,
        "tool_names":capability.tool_names,
        "tool_count":tool_count,
        "blocked_global":blocked,
        "policy_visible":!blocked,
        "enable_supported":true,
        "qualification_status":if blocked { "blocked" } else { "qualified" },
    })
}

fn inferred_kind(id: &str) -> &'static str {
    if id.starts_with("mcp:") {
        "mcp_toolpack"
    } else if id.starts_with("pack:") {
        "pack"
    } else {
        "skill"
    }
}

fn overview_kind(row: &Value) -> &str {
    match row.get("kind").and_then(Value::as_str).unwrap_or_default() {
        "mcp" | "mcp_toolpack" => "mcp",
        "pack" => "pack",
        _ => "skill",
    }
}

fn matches_filters(row: &Value, kind: &str, policy: &str, source: &str) -> bool {
    (kind.is_empty()
        || kind == "all"
        || overview_kind(row) == kind
        || (kind == "mcp_toolpack" && overview_kind(row) == "mcp"))
        && (source.is_empty() || row.get("source_id").and_then(Value::as_str) == Some(source))
        && match policy {
            "blocked" => row["blocked_global"].as_bool().unwrap_or(false),
            "actionable" => !row["blocked_global"].as_bool().unwrap_or(false),
            _ => true,
        }
}

fn overview_matches(row: &Value, query: &str) -> bool {
    query.is_empty()
        || ["capability_id", "name", "description_short", "source_id"]
            .iter()
            .filter_map(|key| row.get(key).and_then(Value::as_str))
            .chain(
                row.get("tags")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            )
            .any(|value| value.to_lowercase().contains(query))
}

fn source_counts(catalog: &[Capability]) -> BTreeMap<String, Value> {
    let mut counts = BTreeMap::<String, usize>::new();
    for capability in catalog {
        if !capability.source.is_empty() {
            *counts.entry(capability.source.clone()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(source_id, record_count)| {
            (
                source_id.clone(),
                json!({
                    "source_id":source_id,
                    "enabled":true,
                    "sync_state":"synced",
                    "record_count":record_count,
                }),
            )
        })
        .collect()
}

fn row_name(row: &Value) -> String {
    row["name"].as_str().unwrap_or_default().to_lowercase()
}

fn usize_arg(request: &DaemonRequest, key: &str, default: usize, max: usize) -> usize {
    request
        .args
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
        .min(max)
}
