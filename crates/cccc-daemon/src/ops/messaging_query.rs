use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{HomeLayout, ledger};
use serde_json::{Value, json};

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
    result(Page { events, has_more })
}

pub fn search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let events = load(home, request)?;
    let page = page(
        &events,
        Query {
            kind: kind(request, "all"),
            text: string_arg(request, "q").unwrap_or_default(),
            by: string_arg(request, "by").unwrap_or_default(),
            before: nonempty(request, "before"),
            after: nonempty(request, "after"),
            limit: integer(request, "limit", 50).clamp(1, 200),
        },
    )?;
    result(page)
}

pub fn window(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let center_id = required_arg(request, "center")?;
    let events = load(home, request)?;
    let center = events
        .iter()
        .find(|event| event.id == center_id)
        .cloned()
        .ok_or_else(|| OpError::new("event_not_found", format!("event not found: {center_id}")))?;
    let kind = kind(request, "chat");
    if !matches_kind(&center, kind) {
        return Err(OpError::new(
            "invalid_center_kind",
            format!("center event kind must match kind={}", kind.name()),
        ));
    }
    let before = page(
        &events,
        Query {
            kind,
            before: Some(center_id.clone()),
            limit: integer(request, "before", 30).min(200),
            ..Query::default()
        },
    )?;
    let after = page(
        &events,
        Query {
            kind,
            after: Some(center_id.clone()),
            limit: integer(request, "after", 30).min(200),
            ..Query::default()
        },
    )?;
    let center_index = before.events.len();
    let mut combined = before.events;
    combined.push(center);
    combined.extend(after.events);
    object(json!({
        "center_id":center_id,
        "center_index":center_index,
        "events":combined,
        "has_more_before":before.has_more,
        "has_more_after":after.has_more,
        "count":combined.len(),
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
    before: Option<String>,
    after: Option<String>,
    limit: usize,
}

struct Page {
    events: Vec<Event>,
    has_more: bool,
}

fn page(events: &[Event], query: Query) -> Result<Page, OpError> {
    let start = cursor(events, query.after.as_deref())?.map_or(0, |index| index + 1);
    let end = cursor(events, query.before.as_deref())?.unwrap_or(events.len());
    let range = events.get(start.min(end)..end).unwrap_or_default();
    let text = query.text.to_lowercase();
    let mut matches = range
        .iter()
        .filter(|event| matches_kind(event, query.kind))
        .filter(|event| query.by.is_empty() || event.by == query.by)
        .filter(|event| {
            text.is_empty()
                || serde_json::to_string(event)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&text)
        })
        .cloned()
        .collect::<Vec<_>>();
    let has_more = matches.len() > query.limit;
    if query.after.is_some() {
        matches.truncate(query.limit);
    } else if has_more {
        matches.drain(..matches.len() - query.limit);
    }
    Ok(Page {
        events: matches,
        has_more,
    })
}

fn cursor(events: &[Event], id: Option<&str>) -> Result<Option<usize>, OpError> {
    let Some(id) = id else { return Ok(None) };
    events
        .iter()
        .position(|event| event.id == id)
        .map(Some)
        .ok_or_else(|| OpError::new("event_not_found", format!("event not found: {id}")))
}

fn matches_kind(event: &Event, kind: Kind) -> bool {
    match kind {
        Kind::All => true,
        Kind::Chat => event.kind == "chat.message",
        Kind::Notify => event.kind == "system.notify",
    }
}

fn load(home: &HomeLayout, request: &DaemonRequest) -> Result<Vec<Event>, OpError> {
    ledger::read_all(&ledger_path(home, request)?).map_err(OpError::io)
}

fn ledger_path(home: &HomeLayout, request: &DaemonRequest) -> Result<std::path::PathBuf, OpError> {
    let group_id = required_arg(request, "group_id")?;
    store(home)?.ledger_path(&group_id).map_err(OpError::io)
}

fn result(page: Page) -> OpResult {
    object(json!({"count":page.events.len(),"events":page.events,"has_more":page.has_more}))
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
