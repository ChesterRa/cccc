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
    let group = if group_id.is_empty() {
        None
    } else {
        cccc_core::GroupStore::new(home.clone())
            .map_err(OpError::io)?
            .load(&group_id)
            .ok()
    };
    let projection = super::projection::Projection::new(home, &store, &group_id, group.as_ref())?;
    let locale = string_arg(request, "locale").unwrap_or_else(|| "en".into());
    let known_groups = cccc_core::GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .list()
        .map_err(OpError::io)?
        .into_iter()
        .map(|group| group.group_id)
        .collect();
    let matching = store
        .search(&query)
        .map_err(OpError::io)?
        .into_iter()
        .map(|capability| capability.id)
        .collect::<std::collections::BTreeSet<_>>();

    let mut rows = catalog
        .iter()
        .filter(|capability| !removed.contains(&capability.id) && matching.contains(&capability.id))
        .cloned()
        .map(|capability| {
            let blocked = effective.blocked.contains(&capability.id);
            let detail = projection.detail(&capability, "user", &locale, &known_groups);
            let global_blocked = state.blocked.contains(&capability.id);
            let mut row = overview_row(
                cccc_core::capability_recommendations::localized(&capability, &locale),
                blocked,
            );
            row.as_object_mut()
                .expect("overview row")
                .extend(detail.as_object().expect("detail").clone());
            row["blocked_global"] = json!(global_blocked);
            row["blocked_group"] = json!(blocked && !global_blocked);
            row
        })
        .collect::<Vec<_>>();

    let mut kind_counts = BTreeMap::from([("skill", 0usize), ("mcp", 0), ("pack", 0)]);
    for row in &rows {
        if let Some(count) = kind_counts.get_mut(overview_kind(row)) {
            *count += 1;
        }
    }

    rows.retain(|row| matches_filters(row, &kind_filter, &policy_filter, &source_filter));
    rows.sort_by(|left, right| {
        let priority = |row: &Value| {
            (
                !(row["capability_id"]
                    .as_str()
                    .is_some_and(|id| id.to_lowercase() == query)
                    || row_name(row) == query),
                row["content_origin"] != "release_recommendation",
            )
        };
        priority(left)
            .cmp(&priority(right))
            .then_with(|| row_name(left).cmp(&row_name(right)))
            .then_with(|| {
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
        .map(|id| json!({"capability_id":id,"scope":if state.blocked.contains(id){"global"}else{"group"}}))
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
        "blocked_capabilities":blocked_capabilities,
        "domains":cccc_core::capability_recommendations::domains(&locale).map_err(OpError::io)?,
    }))
}

fn overview_row(capability: Capability, blocked: bool) -> Value {
    let legacy_search_unverified = capability.is_unverified_remote_search();
    let kind = if capability.kind.is_empty() {
        inferred_kind(&capability.id).to_owned()
    } else {
        capability.kind
    };
    let qualification_status = if blocked {
        "blocked".to_owned()
    } else if capability.qualification_status.is_empty() {
        "qualified".to_owned()
    } else {
        capability.qualification_status
    };
    let enable_supported = capability.enable_supported && qualification_status != "blocked";
    let tool_count = capability.tool_names.len();
    let mut row = json!({
        "capability_id":capability.id,
        "kind":kind,
        "name":capability.name,
        "description_short":capability.description,
        "source_id":capability.source,
        "source_uri":capability.source_uri,
        "sync_state":capability.sync_state,
        "legacy_search_unverified":legacy_search_unverified,
        "tags":capability.tags,
        "tool_names":capability.tool_names,
        "tool_count":tool_count,
        "blocked_global":blocked,
        "policy_visible":!blocked,
        "enable_supported":enable_supported,
        "autoload_candidate":!blocked && enable_supported,
        "qualification_status":qualification_status,
    });
    if capability.source == "agent_self_proposed" {
        row["capsule_text"] = json!(capability.capsule_text);
    }
    row
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
