//! Read-only user reply requests, plus a separately acknowledged answer.
//! A failed read is never presented as a zero count or a pending request.

use super::daemon;
use cccc_client::DaemonClient;
use cccc_contracts::reply_question::{self, Answer, Question, Stored};
use cccc_contracts::{DaemonRequest, Event};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Count {
    Fresh(u64),
    Stale(u64),
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Summary {
    pub(super) groups: HashMap<String, Count>,
    pub(super) last_ok: Option<Instant>,
    pub(super) error: bool,
}

impl Summary {
    pub(super) fn record(&mut self, result: Result<HashMap<String, Count>, String>, at: Instant) {
        match result {
            Ok(groups) => {
                self.groups = groups;
                self.last_ok = Some(at);
                self.error = false;
            }
            Err(_) => {
                self.error = true;
                for count in self.groups.values_mut() {
                    if let Count::Fresh(n) = count {
                        *count = Count::Stale(*n);
                    }
                }
            }
        }
    }
    pub(super) fn count(&self, group: &str) -> Option<Count> {
        self.groups
            .get(group)
            .cloned()
            .or_else(|| (self.error || self.last_ok.is_some()).then_some(Count::Unknown))
    }
}

pub(super) async fn summary(client: &DaemonClient) -> Result<HashMap<String, Count>, String> {
    let result = daemon::read(client, "user_reply_requests_summary", Map::new()).await?;
    let groups = result
        .get("groups")
        .and_then(Value::as_array)
        .ok_or("missing reply counts")?;
    let mut counts = HashMap::new();
    for group in groups {
        let id = group
            .get("group_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("invalid group count")?;
        let count = match (
            group.get("state").and_then(Value::as_str),
            group.get("pending").and_then(Value::as_u64),
        ) {
            (Some("fresh"), Some(n)) => Count::Fresh(n),
            _ => Count::Unknown,
        };
        if counts.insert(id.to_owned(), count).is_some() {
            return Err("duplicate group count".into());
        }
    }
    Ok(counts)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    Pending,
    Answered,
    Withdrawn,
    Unavailable,
    RequesterUnknown,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) event: Event,
    pub(super) question: Option<Question>,
    pub(super) status: Status,
}

impl Row {
    pub(super) fn from_event(event: Event, status: Option<&Value>) -> Self {
        let mut row = Self {
            event,
            question: None,
            status: Status::Unknown,
        };
        row.update(status);
        row
    }
    pub(super) fn update(&mut self, status: Option<&Value>) {
        self.status = Status::Unknown;
        self.question = None;
        let Some(status) = status else {
            return;
        };
        let Some(obligation) = status.get("obligation_status").and_then(|v| v.get("user")) else {
            return;
        };
        if obligation.get("reply_requested").and_then(Value::as_bool) != Some(true) {
            return;
        }
        let (Some(replied), Some(cancelled)) = (
            obligation.get("replied").and_then(Value::as_bool),
            obligation.get("cancelled").and_then(Value::as_bool),
        ) else {
            return;
        };
        if replied && cancelled {
            return;
        }
        let Some(meta) = status.get("user_request") else {
            return;
        };
        self.question = match (
            meta.get("question").and_then(Value::as_str),
            reply_question::stored(&self.event.data),
        ) {
            (Some("valid"), Stored::Valid(question)) => Some(question),
            (Some("none"), Stored::None) | (Some("unavailable"), Stored::Unavailable) => None,
            _ => return,
        };
        self.status = if replied {
            Status::Answered
        } else if cancelled {
            Status::Withdrawn
        } else {
            match meta.get("requester").and_then(Value::as_str) {
                Some("available") => Status::Pending,
                Some("unavailable") => Status::Unavailable,
                Some("unsupported") => Status::Unsupported,
                Some("unknown") => Status::RequesterUnknown,
                _ => Status::Unknown,
            }
        };
    }
    pub(super) fn text(&self) -> &str {
        self.event
            .data
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Request {
    pub(super) group_id: String,
    pub(super) before: Option<String>,
    pub(super) detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Page {
    pub(super) rows: Vec<Row>,
    pub(super) before: Option<String>,
    pub(super) has_more: bool,
    pub(super) total: u64,
    pub(super) detail_status: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Snapshot {
    pub(super) request: Option<Request>,
    pub(super) page: Option<Page>,
    pub(super) last_ok: Option<Instant>,
    pub(super) error: Option<String>,
}

impl Snapshot {
    pub(super) fn record(&mut self, request: &Request, read: Result<Page, String>, at: Instant) {
        if self.request.as_ref() != Some(request) {
            *self = Self {
                request: Some(request.clone()),
                ..Self::default()
            };
        }
        match read {
            Ok(page) => {
                self.page = Some(page);
                self.last_ok = Some(at);
                self.error = None;
            }
            Err(error) => {
                self.page = None;
                self.error = Some(error);
            }
        }
    }
}

pub(super) async fn page(client: &DaemonClient, request: &Request) -> Result<Page, String> {
    let mut args = json!({"group_id": request.group_id, "by": "user", "limit": 20})
        .as_object()
        .expect("literal page arguments")
        .clone();
    if let Some(before) = &request.before {
        args.insert("before".into(), json!(before));
    }
    let result = daemon::read(client, "user_reply_requests", args).await?;
    let events = result
        .get("events")
        .and_then(Value::as_array)
        .ok_or("invalid request page")?;
    let statuses = result
        .get("statuses")
        .and_then(Value::as_object)
        .ok_or("missing request statuses")?;
    let mut rows = Vec::new();
    for event in events {
        let event: Event =
            serde_json::from_value(event.clone()).map_err(|_| "invalid request event")?;
        if event.group_id != request.group_id || event.kind != "chat.message" || event.id.is_empty()
        {
            return Err("unexpected request event".into());
        }
        let status = statuses.get(&event.id);
        rows.push(Row::from_event(event, status));
    }
    let has_more = result
        .get("has_more")
        .and_then(Value::as_bool)
        .ok_or("missing page flag")?;
    let before = result
        .get("before")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if has_more && before.as_deref().is_none_or(str::is_empty) {
        return Err("missing page cursor".into());
    }
    let total = result
        .get("total")
        .and_then(Value::as_u64)
        .ok_or("missing request count")?;
    let detail_status =
        if let Some(status) = request.detail.as_ref().and_then(|id| statuses.get(id)) {
            Some(status.clone())
        } else if let Some(id) = &request.detail {
            let result = daemon::read(
                client,
                "ledger_statuses",
                json!({"group_id":request.group_id, "event_ids":[id]})
                    .as_object()
                    .expect("literal status arguments")
                    .clone(),
            )
            .await?;
            result
                .get("statuses")
                .and_then(|statuses| statuses.get(id))
                .cloned()
        } else {
            None
        };
    Ok(Page {
        rows,
        before,
        has_more,
        total,
        detail_status,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Command {
    pub(super) group_id: String,
    pub(super) event_id: String,
    pub(super) recipient: String,
    pub(super) client_id: String,
    pub(super) answer: Answer,
    /// The snapshot used to construct the answer; never sent as an IPC argument.
    pub(super) question: Option<Question>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Recorded,
    Closed(Status),
    Rejected(String),
    Uncertain,
}

/// These rejections are evaluated only after the daemon checked for the
/// same client id, so they resolve an earlier uncertain attempt as unwritten.
pub(super) fn after_idempotency_lookup(code: &str) -> bool {
    matches!(
        code,
        "invalid_source_event"
            | "requester_unavailable"
            | "requester_unknown"
            | "unsupported_request_source"
            | "question_unavailable"
    )
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Progress {
    pub(super) last: Option<(Command, Outcome)>,
}

pub(super) async fn act(
    home: HomeLayout,
    mut commands: mpsc::UnboundedReceiver<Command>,
    progress: watch::Sender<Progress>,
) {
    let client = DaemonClient::new(home).with_timeout(Duration::from_secs(10));
    while let Some(command) = commands.recv().await {
        let args = json!({"group_id":command.group_id, "source_event_id":command.event_id, "by":"user", "client_id":command.client_id, "answer":command.answer});
        let request = DaemonRequest {
            v: 1,
            op: "reply_request_answer".into(),
            args: args.as_object().expect("literal answer arguments").clone(),
        };
        let outcome = match client.call(&request).await {
            Ok(response) if response.ok => {
                let valid = response
                    .result
                    .get("event")
                    .is_some_and(|event| matches_receipt(event, &command));
                if valid {
                    Outcome::Recorded
                } else {
                    Outcome::Uncertain
                }
            }
            Ok(response) => match response.error {
                Some(error) if error.code == "reply_request_closed" => {
                    match error.details.get("state").and_then(Value::as_str) {
                        Some("answered") => Outcome::Closed(Status::Answered),
                        Some("cancelled") => Outcome::Closed(Status::Withdrawn),
                        _ => Outcome::Uncertain,
                    }
                }
                Some(error)
                    if [
                        "invalid_answer",
                        "invalid_args",
                        "invalid_source_event",
                        "requester_unavailable",
                        "requester_unknown",
                        "unsupported_request_source",
                        "question_unavailable",
                        "permission_denied",
                        "idempotency_conflict",
                    ]
                    .contains(&error.code.as_str()) =>
                {
                    Outcome::Rejected(error.code)
                }
                _ => Outcome::Uncertain,
            },
            Err(_) => Outcome::Uncertain,
        };
        progress.send_modify(|progress| progress.last = Some((command, outcome)));
    }
}

fn matches_receipt(event: &Value, command: &Command) -> bool {
    if event
        .get("id")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
        || event.get("kind").and_then(Value::as_str) != Some("chat.message")
        || event.get("by").and_then(Value::as_str) != Some("user")
        || event.get("group_id").and_then(Value::as_str) != Some(&command.group_id)
    {
        return false;
    }
    let Some(data) = event.get("data").and_then(Value::as_object) else {
        return false;
    };
    if data.get("reply_to").and_then(Value::as_str) != Some(&command.event_id)
        || data.get("client_id").and_then(Value::as_str) != Some(&command.client_id)
        || data.get("message_mode").and_then(Value::as_str) != Some("send")
        || data.get("to") != Some(&json!([command.recipient]))
    {
        return false;
    }
    let text = data.get("text").and_then(Value::as_str);
    match &command.question {
        Some(question) => {
            data.get("answer")
                .and_then(|value| Answer::parse(value).ok())
                .is_some_and(|answer| answer == command.answer)
                && command.answer.reply_text(question).ok().as_deref() == text
        }
        None => {
            !data.contains_key("answer")
                && command.answer.option_id.is_none()
                && command.answer.text.as_deref() == text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, Command, Question, matches_receipt};
    use serde_json::json;

    #[test]
    fn a_receipt_confirms_only_the_answer_that_was_sent() {
        let question = Question::from_request(
            &json!({"v":1,"prompt":"Which?","options":[{"label":"First"},{"label":"Second"}]}),
            "",
        )
        .expect("question");
        let mut command = Command {
            group_id: "g1".into(),
            event_id: "r1".into(),
            recipient: "peer".into(),
            client_id: "c1".into(),
            answer: Answer {
                v: 1,
                option_id: Some("B".into()),
                text: None,
            },
            question: Some(question),
        };
        let receipt = json!({
            "id":"a1", "kind":"chat.message", "group_id":"g1", "by":"user",
            "data":{"reply_to":"r1", "to":["peer"], "client_id":"c1", "message_mode":"send",
                    "answer":{"v":1,"option_id":"B"}, "text":"Selected B: Second"}
        });
        assert!(matches_receipt(&receipt, &command));
        let mut different = receipt.clone();
        different["data"]["text"] = json!("Selected A: First");
        assert!(!matches_receipt(&different, &command));
        different["data"]["answer"] = json!({"v":1,"option_id":"A"});
        assert!(!matches_receipt(&different, &command));
        command.question = None;
        command.answer = Answer {
            v: 1,
            option_id: None,
            text: Some("words".into()),
        };
        let mut plain = receipt;
        plain["data"]
            .as_object_mut()
            .expect("data")
            .remove("answer");
        plain["data"]["text"] = json!("words");
        assert!(matches_receipt(&plain, &command));
        plain["data"]["message_mode"] = json!("mail");
        assert!(!matches_receipt(&plain, &command));
    }
}
