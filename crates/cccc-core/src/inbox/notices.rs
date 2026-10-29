//! Payload-free ordered notice facts, separate from the fixed inbox page.
//! Opt-in repeats prune expired handed-off claims and withdrawn notices.
//! Pending handoffs retain their original delivery ownership.
//! Other historical relations still grow with history; no payload is retained.
use super::*;
use std::collections::HashSet;

type MailClaims = HashMap<(String, String), Vec<MailClaim>>;

struct MailClaim {
    id: String,
    ts: String,
    sources: Vec<String>,
}

#[derive(Default)]
pub(crate) struct NoticeState {
    pub positions: HashMap<String, usize>,
    references: HashSet<String>,
    mail_ids: HashSet<String>,
    pub next_position: usize,
    sources: Vec<Source>,
    generations: HashMap<String, usize>,
    mail_reads: HashMap<String, Vec<(usize, String)>>,
    replies: HashMap<(String, String), usize>,
    cancelled: HashSet<String>,
    deliveries: HashMap<(String, String), (String, String, usize)>,
    actor_resumes: HashMap<String, String>,
    mail_claims: MailClaims,
    reply_claims: HashSet<(String, String, String)>,
    resume_at: Option<String>,
}
struct Source {
    id: String,
    ts: String,
    position: usize,
    mail: bool,
    recipients: Vec<String>,
}

impl NoticeState {
    pub fn reference_ids(event: &Event) -> Vec<String> {
        if event.kind == "mail.read" {
            return event
                .data
                .get("event_id")
                .and_then(Value::as_str)
                .map(|id| vec![id.to_owned()])
                .unwrap_or_default();
        }
        if event.kind == "system.notify" {
            return event
                .data
                .get("context")
                .and_then(|v| v.get("source_event_ids"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
        }
        Vec::new()
    }
    pub fn remember_references(&mut self, event: &Event) {
        self.references.extend(Self::reference_ids(event));
    }
    pub fn remember_cursor(&mut self, id: &str) {
        self.references.insert(id.to_owned());
    }
    pub fn can_append(&self, event: &Event) -> bool {
        Self::reference_ids(event)
            .iter()
            .all(|id| self.positions.contains_key(id) || self.references.contains(id))
    }
    pub fn note(&mut self, group: &GroupDoc, event: &Event) {
        let position = self.next_position;
        self.next_position += 1;
        let mode = event
            .data
            .get("message_mode")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let source = event.kind == "chat.message" && matches!(mode, "mail" | "request_reply");
        if source && mode == "mail" {
            self.mail_ids.insert(event.id.clone());
        }
        if source || self.references.contains(&event.id) {
            self.positions.insert(event.id.clone(), position);
        }
        if let Some(actor) = actor_add_id(event) {
            self.generations.insert(actor.to_owned(), position);
        }
        if event.kind == "group.start"
            || (event.kind == "group.set_state"
                && matches!(
                    event.data.get("new_state").and_then(Value::as_str),
                    Some("active" | "idle")
                ))
        {
            self.resume_at = Some(event.ts.clone());
        }
        match event.kind.as_str() {
            "actor.start" | "actor.restart" | "actor.new_session" => {
                if let Some(id) = event.data.get("actor_id").and_then(Value::as_str) {
                    self.actor_resumes.insert(id.to_owned(), event.ts.clone());
                }
            }
            "mail.read" => {
                if let (Some(actor), Some(boundary)) = (
                    event.data.get("actor_id").and_then(Value::as_str),
                    event.data.get("event_id").and_then(Value::as_str),
                ) {
                    self.mail_reads
                        .entry(actor.to_owned())
                        .or_default()
                        .push((position, boundary.to_owned()));
                }
            }
            "chat.message" => {
                if let Some(id) = event.data.get("reply_to").and_then(Value::as_str) {
                    self.replies
                        .entry((id.to_owned(), event.by.clone()))
                        .or_insert(position);
                }
                let broadcast = event
                    .data
                    .get("to")
                    .and_then(Value::as_array)
                    .is_none_or(|to| {
                        to.is_empty()
                            || to
                                .iter()
                                .filter_map(Value::as_str)
                                .any(|id| matches!(id, "@all" | "@peers" | "@foreman"))
                    });
                if source && (mode != "mail" || !broadcast) {
                    let recipients = group
                        .actors
                        .iter()
                        .filter(|actor| is_for_actor(group, event, &actor.id))
                        .map(|actor| actor.id.clone())
                        .collect::<Vec<_>>();
                    if !recipients.is_empty() {
                        self.sources.push(Source {
                            id: event.id.clone(),
                            ts: event.ts.clone(),
                            position,
                            mail: mode == "mail",
                            recipients,
                        });
                    }
                }
            }
            "chat.reply_request.cancelled" => {
                if let Some(id) = event.data.get("source_event_id").and_then(Value::as_str) {
                    self.cancelled.insert(id.to_owned());
                }
            }
            "runtime.delivery" => {
                if let (Some(id), Some(actor), Some(state)) = (
                    event.data.get("source_event_id").and_then(Value::as_str),
                    event.data.get("actor_id").and_then(Value::as_str),
                    event.data.get("state").and_then(Value::as_str),
                ) {
                    self.deliveries.insert(
                        (id.to_owned(), actor.to_owned()),
                        (state.to_owned(), event.ts.clone(), position),
                    );
                }
            }
            "system.notify" => {
                let kind = event
                    .data
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !matches!(kind, "mail_notice" | "reply_notice") {
                    return;
                }
                let context = event.data.get("context");
                let actor = context
                    .and_then(|v| v.get("actor_id"))
                    .and_then(Value::as_str)
                    .or_else(|| event.data.get("target_actor_id").and_then(Value::as_str))
                    .unwrap_or_default();
                let created = context
                    .and_then(|v| v.get("actor_created_at"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let ids = Self::reference_ids(event);
                if kind == "mail_notice" {
                    self.mail_claims
                        .entry((actor.to_owned(), created.to_owned()))
                        .or_default()
                        .push(MailClaim {
                            id: event.id.clone(),
                            ts: event.ts.clone(),
                            sources: ids,
                        });
                } else {
                    self.reply_claims.extend(
                        ids.into_iter()
                            .map(|id| (actor.to_owned(), created.to_owned(), id)),
                    );
                }
            }
            _ => {}
        }
        // Only notice/delivery facts need append-time pruning. Expiry is also
        // reconciled in due(), without rescanning claims for every unrelated
        // event in a cold history replay.
        if matches!(event.kind.as_str(), "system.notify" | "runtime.delivery") {
            self.prune_mail_claims(group);
        }
    }

    fn prune_mail_claims(&mut self, group: &GroupDoc) {
        let repeat =
            crate::automation::delivery_timing_value(group, "mail_notice_repeat_seconds", 0);
        if repeat <= 0 {
            return;
        }
        let now = Utc::now().timestamp();
        self.mail_claims.retain(|(actor, created), claims| {
            let current = group
                .actors
                .iter()
                .any(|a| a.id == *actor && a.created_at == *created);
            claims.retain(|claim| {
                current
                    && match self
                        .deliveries
                        .get(&(claim.id.clone(), actor.clone()))
                        .map(|value| value.0.as_str())
                    {
                        Some("withdrawn") => false,
                        Some("accepted" | "ambiguous") => {
                            now - crate::automation::timestamp(&claim.ts, now) < repeat
                        }
                        _ => true,
                    }
            });
            !claims.is_empty()
        });
    }

    #[cfg(test)]
    pub(crate) fn retained_mail_claims(&self) -> usize {
        self.mail_claims.values().map(Vec::len).sum()
    }

    pub fn still_awaited(
        &self,
        actor: &str,
        ids: &[String],
        cursor: Option<&String>,
    ) -> HashSet<String> {
        let generation = self.generations.get(actor).copied().unwrap_or(0);
        let cursor = cursor.and_then(|id| self.positions.get(id)).copied();
        ids.iter()
            .filter(|id| {
                self.mail_ids.contains(*id)
                    && self
                        .positions
                        .get(*id)
                        .is_some_and(|position| *position >= generation)
                    && self.positions.get(*id).is_some_and(|position| {
                        crate::mail_pending::awaiting(
                            cursor.is_some_and(|cursor| cursor >= *position),
                            self.replies
                                .contains_key(&((*id).clone(), actor.to_owned())),
                            self.deliveries
                                .get(&((*id).clone(), actor.to_owned()))
                                .map(|v| v.0.as_str()),
                        )
                    })
            })
            .cloned()
            .collect()
    }

    pub fn due(
        &mut self,
        group: &GroupDoc,
        eligible: &[&cccc_contracts::Actor],
        cursors: &BTreeMap<String, String>,
        mail_delays: &crate::automation::MailDelays<'_>,
        reply_after: i64,
    ) -> Vec<Event> {
        use crate::automation::{notice_event, timestamp};
        self.prune_mail_claims(group);
        let now = Utc::now().timestamp();
        let resume_at = self.resume_at.as_deref().map(|ts| timestamp(ts, now));
        let mut notices = Vec::new();
        let resolution = |actor: &str, id: &str| {
            let source = self.positions.get(id)?;
            let read = self.mail_reads.get(actor).and_then(|facts| {
                facts.iter().find_map(|(fact, boundary)| {
                    self.positions
                        .get(boundary)
                        .filter(|position| *position >= source)
                        .map(|_| *fact)
                })
            });
            let reply = self
                .replies
                .get(&(id.to_owned(), actor.to_owned()))
                .copied();
            let delivered = self
                .deliveries
                .get(&(id.to_owned(), actor.to_owned()))
                .and_then(|(state, _, position)| {
                    matches!(state.as_str(), "accepted" | "ambiguous").then_some(*position)
                });
            [read, reply, delivered].into_iter().flatten().min()
        };
        for actor in eligible {
            let after = mail_delays.for_actor(&actor.id);
            let generation = self.generations.get(&actor.id).copied().unwrap_or(0);
            let cursor = cursors
                .get(&actor.id)
                .and_then(|id| self.positions.get(id))
                .copied();
            let mut mail = Vec::new();
            let mut replies = Vec::new();
            for source in self.sources.iter().filter(|source| {
                source.position >= generation && source.recipients.contains(&actor.id)
            }) {
                let key = (source.id.clone(), actor.id.clone());
                let replied = self.replies.contains_key(&key);
                let delivery = self.deliveries.get(&key);
                if source.mail {
                    if crate::mail_pending::awaiting(
                        cursor.is_some_and(|cursor| cursor >= source.position),
                        replied,
                        delivery.map(|v| v.0.as_str()),
                    ) {
                        mail.push(source);
                    }
                } else if !replied
                    && !self.cancelled.contains(&source.id)
                    && !self.reply_claims.contains(&(
                        actor.id.clone(),
                        actor.created_at.clone(),
                        source.id.clone(),
                    ))
                    && let Some((_, at, _)) = delivery.filter(|v| v.0 == "accepted")
                {
                    let started = timestamp(at, now);
                    let started = resume_at.map_or(started, |resume| started.max(resume));
                    if reply_after > 0 && now - started >= reply_after {
                        replies.push(source);
                    }
                }
            }
            if mail_delays.busy > 0 && !mail.is_empty() {
                let active = self
                    .mail_claims
                    .get(&(actor.id.clone(), actor.created_at.clone()))
                    .into_iter()
                    .flatten()
                    .rev()
                    .filter(|claim| {
                        // Expiry is handled by pruning, which keeps pending handoffs.
                        // Zero/default retains historical single-notice claims.
                        self.deliveries
                            .get(&(claim.id.clone(), actor.id.clone()))
                            .is_none_or(|v| v.0 != "withdrawn")
                    })
                    .any(|claim| {
                        let claimed = claim
                            .sources
                            .iter()
                            .filter(|id| self.positions.contains_key(*id))
                            .collect::<Vec<_>>();
                        if claimed.is_empty() {
                            return true;
                        }
                        let resolved = claimed
                            .iter()
                            .map(|id| resolution(&actor.id, id))
                            .collect::<Vec<_>>();
                        if resolved.iter().any(Option::is_none) {
                            return true;
                        }
                        let closure = resolved.into_iter().flatten().max().unwrap_or(0);
                        mail.iter().any(|source| source.position <= closure)
                    });
                let first = timestamp(&mail[0].ts, now);
                let first = resume_at.map_or(first, |resume| first.max(resume));
                let first = self
                    .actor_resumes
                    .get(&actor.id)
                    .map_or(first, |resume| first.max(timestamp(resume, now)));
                if !active && now - first >= after {
                    let mut event = notice_event(
                        group,
                        actor,
                        "mail_notice",
                        "Mail waiting",
                        &format!(
                            "You have {} Mail item(s) waiting. Call cccc_inbox_read when appropriate.",
                            mail.len()
                        ),
                        mail.iter().map(|source| source.id.clone()).collect(),
                    );
                    if after < mail_delays.busy
                        && let Some(deadline) =
                            DateTime::from_timestamp(first + mail_delays.busy, 0)
                        && let Some(context) =
                            event.data.get_mut("context").and_then(Value::as_object_mut)
                    {
                        context.insert("deliver_by".into(), json!(deadline.to_rfc3339()));
                    }
                    notices.push(event);
                }
            }
            if !replies.is_empty() {
                notices.push(notice_event(group,actor,"reply_notice","Reply requested",
                    &format!("{} message(s) still need a concrete reply. Use cccc_message_history if needed, then cccc_message_reply.",replies.len()),
                    replies.iter().map(|source| source.id.clone()).collect()));
            }
        }
        notices
    }
}
