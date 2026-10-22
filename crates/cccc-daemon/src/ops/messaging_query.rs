use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{HomeLayout, ledger};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub fn tail(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let limit = integer(request, "limit", 50).min(1000);
    if limit == 0 {
        return object(json!({"events":[],"has_more":false,"count":0}));
    }
    let kind = kind(request, "all");
    let path = ledger_path(home, request)?;
    let (events, has_more) =
        ledger::tail_filtered(&path, limit, kind.filter()).map_err(OpError::io)?;
    result(home, request, Page { events, has_more })
}

pub fn search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let path = ledger_path(home, request)?;
    let query = Query {
        kind: kind(request, "all"),
        text: string_arg(request, "q").unwrap_or_default(),
        by: string_arg(request, "by").unwrap_or_default(),
        by_any: sender_ids(request)?,
        recipient: recipient(request)?,
        since: timestamp(request, "since_ts")?,
        until: timestamp(request, "until_ts")?,
        before: nonempty(request, "before"),
        after: nonempty(request, "after"),
        limit: integer(request, "limit", 50).clamp(1, 200),
    };
    if matches!((query.since, query.until), (Some(since), Some(until)) if since > until) {
        return Err(OpError::new(
            "invalid_args",
            "since_ts must not be later than until_ts",
        ));
    }
    let page = ledger::inspect(&path, |events, positions| page(events, positions, query))
        .map_err(OpError::io)??;
    result(home, request, page)
}

pub fn window(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let center_id = required_arg(request, "center")?;
    let path = ledger_path(home, request)?;
    let kind = kind(request, "chat");
    let before_limit = integer(request, "before", 30).min(200);
    let after_limit = integer(request, "after", 30).min(200);
    let (combined, center_index, has_more_before, has_more_after) =
        ledger::inspect(&path, |events, positions| -> Result<_, OpError> {
            let center = positions
                .get(&center_id)
                .and_then(|position| events.get(*position))
                .cloned()
                .ok_or_else(|| {
                    OpError::new("event_not_found", format!("event not found: {center_id}"))
                })?;
            if !matches_kind(&center, kind) {
                return Err(OpError::new(
                    "invalid_center_kind",
                    format!("center event kind must match kind={}", kind.name()),
                ));
            }
            let before = page(
                events,
                positions,
                Query {
                    kind,
                    before: Some(center_id.clone()),
                    limit: before_limit,
                    ..Query::default()
                },
            )?;
            let after = page(
                events,
                positions,
                Query {
                    kind,
                    after: Some(center_id.clone()),
                    limit: after_limit,
                    ..Query::default()
                },
            )?;
            let center_index = before.events.len();
            let mut combined = before.events;
            combined.push(center);
            combined.extend(after.events);
            Ok((combined, center_index, before.has_more, after.has_more))
        })
        .map_err(OpError::io)??;
    let count = combined.len();
    let events = super::messaging_query_status::decorate(home, request, combined)?;
    object(json!({
        "center_id":center_id,
        "center_index":center_index,
        "events":events,
        "has_more_before":has_more_before,
        "has_more_after":has_more_after,
        "count":count,
    }))
}

#[derive(Clone, Copy, Default)]
enum Kind {
    #[default]
    All,
    Chat,
    Notify,
}

impl Kind {
    const fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Chat => "chat",
            Self::Notify => "notify",
        }
    }

    const fn filter(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Chat => Some("chat"),
            Self::Notify => Some("system.notify"),
        }
    }
}

#[derive(Default)]
struct Query {
    kind: Kind,
    text: String,
    by: String,
    by_any: HashSet<String>,
    recipient: Option<String>,
    since: Option<chrono::DateTime<chrono::FixedOffset>>,
    until: Option<chrono::DateTime<chrono::FixedOffset>>,
    before: Option<String>,
    after: Option<String>,
    limit: usize,
}

struct Page {
    events: Vec<Event>,
    has_more: bool,
}

fn page(
    events: &[Event],
    positions: &HashMap<String, usize>,
    query: Query,
) -> Result<Page, OpError> {
    let start = cursor(events, positions, query.after.as_deref())?
        .map_or(0, |index| index.saturating_add(1));
    let end = cursor(events, positions, query.before.as_deref())?.unwrap_or(events.len());
    let range = events.get(start.min(end)..end).unwrap_or_default();
    let text = query.text.to_lowercase();
    let matches_query = |event: &&Event| {
        matches_kind(event, query.kind)
            && (query.by.is_empty() || event.by == query.by)
            && (query.by_any.is_empty() || query.by_any.contains(&event.by))
            && query.recipient.as_ref().is_none_or(|recipient| {
                event
                    .data
                    .get("to")
                    .and_then(Value::as_array)
                    .is_some_and(|to| {
                        to.iter()
                            .any(|value| value.as_str() == Some(recipient.as_str()))
                    })
            })
            && matches_time(event, query.since, query.until)
            && (text.is_empty()
                || serde_json::to_string(event)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&text))
    };
    let take_limit = query.limit.saturating_add(1);
    if query.after.is_some() {
        let mut matches = range
            .iter()
            .filter(matches_query)
            .take(take_limit)
            .cloned()
            .collect::<Vec<_>>();
        let has_more = matches.len() > query.limit;
        if has_more {
            matches.truncate(query.limit);
        }
        return Ok(Page {
            events: matches,
            has_more,
        });
    }
    let mut matches = range
        .iter()
        .rev()
        .filter(matches_query)
        .take(take_limit)
        .cloned()
        .collect::<Vec<_>>();
    let has_more = matches.len() > query.limit;
    if has_more {
        matches.truncate(query.limit);
    }
    matches.reverse();
    Ok(Page {
        events: matches,
        has_more,
    })
}

fn cursor(
    events: &[Event],
    positions: &HashMap<String, usize>,
    id: Option<&str>,
) -> Result<Option<usize>, OpError> {
    let Some(id) = id else { return Ok(None) };
    positions
        .get(id)
        .copied()
        .filter(|&index| index < events.len())
        .or_else(|| events.iter().position(|event| event.id == id))
        .map(Some)
        .ok_or_else(|| OpError::new("event_not_found", format!("event not found: {id}")))
}

fn recipient(request: &DaemonRequest) -> Result<Option<String>, OpError> {
    request
        .args
        .get("to")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    OpError::new("invalid_args", "to must be a nonempty recipient identity")
                })
        })
        .transpose()
}

fn timestamp(
    request: &DaemonRequest,
    name: &str,
) -> Result<Option<chrono::DateTime<chrono::FixedOffset>>, OpError> {
    request
        .args
        .get(name)
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .ok_or_else(|| {
                    OpError::new(
                        "invalid_args",
                        format!("{name} must be an RFC3339 timestamp"),
                    )
                })
        })
        .transpose()
}

fn matches_time(
    event: &Event,
    since: Option<chrono::DateTime<chrono::FixedOffset>>,
    until: Option<chrono::DateTime<chrono::FixedOffset>>,
) -> bool {
    if since.is_none() && until.is_none() {
        return true;
    }
    chrono::DateTime::parse_from_rfc3339(&event.ts).is_ok_and(|at| {
        since.is_none_or(|since| at >= since) && until.is_none_or(|until| at <= until)
    })
}

fn sender_ids(request: &DaemonRequest) -> Result<HashSet<String>, OpError> {
    let Some(value) = request.args.get("by_any").filter(|value| !value.is_null()) else {
        return Ok(HashSet::new());
    };
    let invalid = || {
        OpError::new(
            "invalid_args",
            "by_any must be an array of nonempty sender identities",
        )
    };
    value
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|id| !id.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(invalid)
        })
        .collect()
}

fn matches_kind(event: &Event, kind: Kind) -> bool {
    match kind {
        Kind::All => true,
        Kind::Chat => event.kind == "chat.message",
        Kind::Notify => event.kind == "system.notify",
    }
}

fn ledger_path(home: &HomeLayout, request: &DaemonRequest) -> Result<std::path::PathBuf, OpError> {
    let group = super::messaging::load(home, request)?;
    store(home)?
        .ledger_path(&group.group_id)
        .map_err(OpError::io)
}

fn result(home: &HomeLayout, request: &DaemonRequest, page: Page) -> OpResult {
    let count = page.events.len();
    let events = super::messaging_query_status::decorate(home, request, page.events)?;
    object(json!({"count":count,"events":events,"has_more":page.has_more}))
}

fn kind(request: &DaemonRequest, default: &str) -> Kind {
    match string_arg(request, "kind")
        .unwrap_or_else(|| default.into())
        .to_lowercase()
        .as_str()
    {
        "chat" => Kind::Chat,
        "notify" => Kind::Notify,
        _ => Kind::All,
    }
}

fn nonempty(request: &DaemonRequest, name: &str) -> Option<String> {
    string_arg(request, name).filter(|value| !value.trim().is_empty())
}

fn integer(request: &DaemonRequest, name: &str, default: usize) -> usize {
    request
        .args
        .get(name)
        .and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.parse().ok(),
            _ => None,
        })
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}
