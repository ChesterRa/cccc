//! Read-only task pages. Page offsets belong to each requested status, as in
//! task_list; every answer is fenced by Group, page and exact task identity.
use super::daemon;
use cccc_client::DaemonClient;
use serde_json::{Map, Value, json};
use std::time::Instant;

pub(super) const PAGE_SIZE: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Selection {
    Page { done: bool, page: usize },
    Detail(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Request {
    pub(super) group_id: String,
    pub(super) selection: Selection,
}

impl Request {
    pub(super) fn open(group_id: String) -> Self {
        Self {
            group_id,
            selection: Selection::Page {
                done: false,
                page: 0,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Task {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) status: String,
    pub(super) assignee: String,
    pub(super) fields: Map<String, Value>,
}

impl Task {
    fn parse(value: &Value) -> Result<Self, String> {
        let fields = value.as_object().ok_or("unreadable task")?.clone();
        let id = fields
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("task has no ID")?
            .to_owned();
        Ok(Self {
            id,
            title: string(&fields, "title"),
            status: string(&fields, "status"),
            assignee: string(&fields, "assignee"),
            fields,
        })
    }

    pub(super) fn name(&self) -> &str {
        if self.title.trim().is_empty() {
            &self.id
        } else {
            &self.title
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) active: u64,
    pub(super) planned: u64,
    pub(super) done: u64,
    pub(super) waiting_user: u64,
    pub(super) blocked: u64,
    pub(super) handoffs: u64,
}

impl Counts {
    fn parse(result: &Map<String, Value>) -> Result<Self, String> {
        let facets = result
            .get("facets")
            .and_then(Value::as_object)
            .ok_or("daemon did not return task counts")?;
        let counts = facets
            .get("status_counts")
            .and_then(Value::as_object)
            .ok_or("daemon did not return task status counts")?;
        let number = |key: &str| counts.get(key).and_then(Value::as_u64).unwrap_or(0);
        let attention = |key: &str| facets.get(key).and_then(Value::as_u64).unwrap_or(0);
        Ok(Self {
            active: number("active"),
            planned: number("planned"),
            done: number("done"),
            waiting_user: attention("waiting_user"),
            blocked: attention("blocked"),
            handoffs: attention("pending_handoffs"),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Page {
    pub(super) tasks: Vec<Task>,
    pub(super) counts: Counts,
    pub(super) has_more: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Data {
    Page(Page),
    Detail(Task),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) request: Option<Request>,
    pub(super) data: Option<Data>,
    pub(super) last_ok: Option<Instant>,
    pub(super) error: Option<String>,
}

impl Snapshot {
    pub(super) fn record(&mut self, request: &Request, read: Result<Data, String>, at: Instant) {
        if self.request.as_ref() != Some(request) {
            *self = Self {
                request: Some(request.clone()),
                ..Self::default()
            };
        }
        match read {
            Ok(data) => {
                self.data = Some(data);
                self.last_ok = Some(at);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn page(&self) -> Option<&Page> {
        match self.data.as_ref()? {
            Data::Page(page) => Some(page),
            Data::Detail(_) => None,
        }
    }
}

pub(super) async fn read(client: &DaemonClient, request: &Request) -> Result<Data, String> {
    let mut args = json!({"group_id":request.group_id})
        .as_object()
        .expect("task args")
        .clone();
    match &request.selection {
        Selection::Detail(id) => {
            args.insert("task_id".into(), json!(id));
            let result = daemon::read(client, "task_list", args).await?;
            Ok(Data::Detail(Task::parse(
                result.get("task").ok_or("daemon returned no task")?,
            )?))
        }
        Selection::Page { done, page } => {
            let statuses = if *done { "done" } else { "active,planned" };
            args.insert("statuses".into(), json!(statuses));
            args.insert("limit".into(), json!(PAGE_SIZE));
            args.insert("offset".into(), json!(page.saturating_mul(PAGE_SIZE)));
            let result = daemon::read(client, "task_list", args).await?;
            parse_page(&result, statuses).map(Data::Page)
        }
    }
}

fn parse_page(result: &Map<String, Value>, statuses: &str) -> Result<Page, String> {
    let pages = result
        .get("pages")
        .and_then(Value::as_object)
        .ok_or("daemon returned no task pages")?;
    let mut tasks = Vec::new();
    let mut has_more = false;
    // The order is explicit, not the JSON object's alphabetical order.
    for status in statuses.split(',') {
        let page = pages
            .get(status)
            .ok_or("daemon returned an incomplete task page")?;
        for value in page["tasks"].as_array().ok_or("unreadable task page")? {
            tasks.push(Task::parse(value)?);
        }
        has_more |= page["has_more"]
            .as_bool()
            .ok_or("task page has no pagination state")?;
    }
    Ok(Page {
        tasks,
        counts: Counts::parse(result)?,
        has_more,
    })
}

pub(super) fn string(fields: &Map<String, Value>, name: &str) -> String {
    fields
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub(super) fn status_label(status: &str) -> &str {
    match status {
        "active" => "Active",
        "planned" => "Planned",
        "done" => "Done",
        "archived" => "Archived",
        _ => "Unknown",
    }
}

#[cfg(test)]
#[path = "tasks_tests.rs"]
mod tests;
