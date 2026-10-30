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

#[cfg(test)]
mod summary_tests {
    use super::*;
    use cccc_contracts::Actor;

    #[test]
    fn unresolved_cache_follows_recipient_generation_and_delivery_facts() {
        // Reducer unit controls: these in-memory facts are synthetic, unlike
        // the IPC integration tests' daemon-produced events.
        let temp = tempfile::tempdir().expect("create temporary test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("create test home");
        let store = GroupStore::new(home).expect("open test group store");
        let mut group = store.create("Mail reducer", "").expect("create test group");
        group.actors.push(Actor::new("reader"));
        group.actors.push(Actor::new("other"));
        let mut state = NoticeState::default();
        let mut mail = Event::new("chat.message", &group.group_id);
        mail.by = "user".into();
        mail.data = json!({"to":["reader"],"message_mode":"mail"})
            .as_object()
            .expect("Mail fixture is an object")
            .clone();
        state.note(&group, &mail);
        let cutoff = Utc::now();
        let summarize = |state: &mut NoticeState| {
            state.overdue_summary(&group, &BTreeMap::new(), cutoff, cutoff)
        };
        assert_eq!(summarize(&mut state)[0].awaiting_count, 1);
        assert_eq!(summarize(&mut state)[1].awaiting_count, 0);

        let mut delivery = Event::new("runtime.delivery", &group.group_id);
        delivery.data =
            json!({"actor_id":"reader","source_event_id":"notice-id","state":"accepted"})
                .as_object()
                .expect("delivery fixture is an object")
                .clone();
        state.note(&group, &delivery);
        assert_eq!(summarize(&mut state)[0].awaiting_count, 1);
        delivery
            .data
            .insert("source_event_id".into(), json!(mail.id));
        for status in ["accepted", "ambiguous"] {
            delivery.data.insert("state".into(), json!(status));
            state.note(&group, &delivery);
            assert_eq!(summarize(&mut state)[0].awaiting_count, 0);
            delivery.data.insert("state".into(), json!("failed"));
            state.note(&group, &delivery);
            assert_eq!(summarize(&mut state)[0].awaiting_count, 1);
        }
        let mut reply = Event::new("chat.message", &group.group_id);
        reply.by = "reader".into();
        reply.data.insert("reply_to".into(), json!(mail.id));
        state.note(&group, &reply);
        assert_eq!(summarize(&mut state)[0].awaiting_count, 0);

        mail.id = "second-mail".into();
        state.note(&group, &mail);
        assert_eq!(summarize(&mut state)[0].awaiting_count, 1);
        let cursor = BTreeMap::from([("reader".into(), mail.id.clone())]);
        assert_eq!(
            state.overdue_summary(&group, &cursor, cutoff, cutoff)[0].awaiting_count,
            0
        );
        mail.id = "third-mail".into();
        state.note(&group, &mail);
        assert_eq!(summarize(&mut state)[0].awaiting_count, 1);
        let mut added = Event::new("actor.add", &group.group_id);
        added.data.insert("actor".into(), json!({"id":"reader"}));
        state.note(&group, &added);
        assert_eq!(summarize(&mut state)[0].awaiting_count, 0);
        mail.id = "current-generation".into();
        state.note(&group, &mail);
        assert_eq!(summarize(&mut state)[0].awaiting_count, 1);
    }

    #[test]
    fn mail_read_boundaries_remain_per_actor_and_do_not_move_backwards() {
        let temp = tempfile::tempdir().expect("temporary test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("read boundary", "").expect("group");
        group.actors.push(Actor::new("reader"));
        group.actors.push(Actor::new("other"));
        let mut state = NoticeState::default();
        let mail = |id: &str| {
            let mut event = Event::new("chat.message", &group.group_id);
            event.id = id.to_owned();
            event.by = "user".into();
            event.data = json!({"to":["reader","other"],"message_mode":"mail"})
                .as_object()
                .expect("fixture data is an object")
                .clone();
            event
        };
        let first = mail("first-mail");
        let second = mail("second-mail");
        state.note(&group, &first);
        state.note(&group, &second);
        let read = |actor: &str, boundary: &str| {
            let mut event = Event::new("mail.read", &group.group_id);
            event.by = actor.to_owned();
            event.data = json!({"actor_id":actor,"event_id":boundary})
                .as_object()
                .expect("fixture data is an object")
                .clone();
            event
        };
        // A duplicate/older fact must not move the read boundary backwards.
        state.note(&group, &read("reader", "second-mail"));
        state.note(&group, &read("reader", "first-mail"));
        let mut delivery = Event::new("runtime.delivery", &group.group_id);
        delivery.data =
            json!({"actor_id":"reader","source_event_id":"second-mail","state":"failed"})
                .as_object()
                .expect("fixture data is an object")
                .clone();
        state.note(&group, &delivery);
        let now = Utc::now();
        let rows = state.overdue_summary(&group, &BTreeMap::new(), now, now);
        assert_eq!(
            rows[0].awaiting_count, 0,
            "an old read fact must not reopen read mail"
        );
        assert_eq!(
            rows[1].awaiting_count, 2,
            "another Actor has an independent read boundary"
        );
        let mut added = Event::new("actor.add", &group.group_id);
        added.data.insert("actor".into(), json!({"id":"reader"}));
        state.note(&group, &added);
        state.note(&group, &mail("new-generation"));
        let rows = state.overdue_summary(&group, &BTreeMap::new(), now, Utc::now());
        assert_eq!(
            rows[0].awaiting_count, 1,
            "old reads cannot cover newly created mail"
        );
        assert_eq!(rows[1].awaiting_count, 3);
    }

    #[test]
    fn read_reference_seen_before_its_source_is_resolved_on_replay() {
        let temp = tempfile::tempdir().expect("temporary test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let mut group = store.create("delayed reference", "").expect("group");
        group.actors.push(Actor::new("reader"));
        group.actors.push(Actor::new("other"));
        let mut state = NoticeState::default();
        let mut read = Event::new("mail.read", &group.group_id);
        read.by = "reader".into();
        read.data = json!({"actor_id":"reader","event_id":"late-source"})
            .as_object()
            .expect("fixture data is an object")
            .clone();
        state.remember_references(&read);
        state.note(&group, &read);
        let mut mail = Event::new("chat.message", &group.group_id);
        mail.id = "late-source".into();
        mail.by = "user".into();
        mail.data = json!({"to":["reader","other"],"message_mode":"mail"})
            .as_object()
            .expect("fixture data is an object")
            .clone();
        state.note(&group, &mail);
        let now = Utc::now();
        let rows = state.overdue_summary(&group, &BTreeMap::new(), now, now);
        assert_eq!(
            rows[0].awaiting_count, 0,
            "late reference must still resolve for reader"
        );
        assert_eq!(
            rows[1].awaiting_count, 1,
            "other Actor has not read the mail"
        );
    }
    #[test]
    fn discard_pending_through_includes_boundary_and_handles_maximum() {
        let mut pending = BTreeMap::from([(0usize, 10usize), (1, 11), (2, 12), (usize::MAX, 13)]);
        NoticeState::discard_pending_through(&mut pending, 1);
        assert_eq!(pending, BTreeMap::from([(2, 12), (usize::MAX, 13)]));
        NoticeState::discard_pending_through(&mut pending, 1);
        assert_eq!(pending, BTreeMap::from([(2, 12), (usize::MAX, 13)]));
        NoticeState::discard_pending_through(&mut pending, 2);
        assert_eq!(pending, BTreeMap::from([(usize::MAX, 13)]));
        NoticeState::discard_pending_through(&mut pending, usize::MAX);
        assert!(pending.is_empty());
        NoticeState::discard_pending_through(&mut pending, usize::MAX);
        assert!(pending.is_empty());
    }
}

#[derive(Default)]
pub(crate) struct NoticeState {
    pub positions: HashMap<String, usize>,
    references: HashSet<String>,
    mail_ids: HashSet<String>,
    pub next_position: usize,
    sources: Vec<Source>,
    mail_sources: HashMap<String, usize>,
    // Payload-free references to unresolved Mail only. Resolved history is
    // removed on each fact, so a summary poll never walks historical sources.
    awaiting_mail: BTreeMap<String, BTreeMap<usize, usize>>,
    generations: HashMap<String, usize>,
    mail_reads: HashMap<String, Vec<(usize, String)>>,
    // The latest resolved Mail read boundary by Actor. Keep the original read
    // facts above for notice resolution, which needs their individual times.
    read_boundary_max: HashMap<String, usize>,
    // A referenced boundary can be encountered later in a historical replay.
    // Resolve it when its source event appears instead of scanning all reads
    // for every subsequent Mail or delivery fact.
    unresolved_read_boundaries: HashMap<String, Vec<String>>,
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
    notice: bool,
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
            if let Some(actors) = self.unresolved_read_boundaries.remove(&event.id) {
                for actor in actors {
                    self.note_read_boundary(&actor, position);
                }
            }
        }
        if let Some(actor) = actor_add_id(event) {
            self.generations.insert(actor.to_owned(), position);
            self.awaiting_mail.remove(actor);
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
                    if let Some(boundary_position) = self.positions.get(boundary).copied() {
                        self.note_read_boundary(actor, boundary_position);
                        if let Some(pending) = self.awaiting_mail.get_mut(actor) {
                            Self::discard_pending_through(pending, boundary_position);
                        }
                    } else {
                        self.unresolved_read_boundaries
                            .entry(boundary.to_owned())
                            .or_default()
                            .push(actor.to_owned());
                    }
                }
            }
            "chat.message" => {
                if let Some(id) = event.data.get("reply_to").and_then(Value::as_str) {
                    self.replies
                        .entry((id.to_owned(), event.by.clone()))
                        .or_insert(position);
                    self.refresh_mail(id, &event.by);
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
                if source {
                    let recipients = group
                        .actors
                        .iter()
                        .filter(|actor| is_for_actor(group, event, &actor.id))
                        .map(|actor| actor.id.clone())
                        .collect::<Vec<_>>();
                    if !recipients.is_empty() {
                        let index = self.sources.len();
                        self.sources.push(Source {
                            id: event.id.clone(),
                            ts: event.ts.clone(),
                            position,
                            mail: mode == "mail",
                            notice: mode != "mail" || !broadcast,
                            recipients,
                        });
                        if mode == "mail" {
                            self.mail_sources.insert(event.id.clone(), index);
                            for actor in self.sources[index].recipients.clone() {
                                self.refresh_mail(&event.id, &actor);
                            }
                        }
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
                    self.refresh_mail(id, actor);
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

    // The pending keys are ordered positions; discard only the prefix. Avoid
    // incrementing the boundary, which can overflow for the maximum key.
    fn discard_pending_through(pending: &mut BTreeMap<usize, usize>, boundary: usize) {
        let mut remaining = pending.split_off(&boundary);
        remaining.remove(&boundary);
        *pending = remaining;
    }

    fn note_read_boundary(&mut self, actor: &str, position: usize) {
        let maximum = self
            .read_boundary_max
            .entry(actor.to_owned())
            .or_insert(position);
        *maximum = (*maximum).max(position);
    }

    fn refresh_mail(&mut self, id: &str, actor: &str) {
        let Some(&index) = self.mail_sources.get(id) else {
            return;
        };
        let source = &self.sources[index];
        let generation = self.generations.get(actor).copied().unwrap_or(0);
        let read = self
            .read_boundary_max
            .get(actor)
            .is_some_and(|position| *position >= source.position);
        let key = (id.to_owned(), actor.to_owned());
        let awaiting = source.position >= generation
            && source.recipients.iter().any(|recipient| recipient == actor)
            && crate::mail_pending::awaiting(
                read,
                self.replies.contains_key(&key),
                self.deliveries.get(&key).map(|value| value.0.as_str()),
            );
        let pending = self.awaiting_mail.entry(actor.to_owned()).or_default();
        if awaiting {
            pending.insert(source.position, index);
        } else {
            pending.remove(&source.position);
        }
    }

    pub fn overdue_summary(
        &mut self,
        group: &GroupDoc,
        cursors: &BTreeMap<String, String>,
        cutoff: DateTime<Utc>,
        observed_at: DateTime<Utc>,
    ) -> Vec<OverdueMailSummary> {
        group
            .actors
            .iter()
            .map(|actor| {
                let cursor = cursors.get(&actor.id).and_then(|id| self.positions.get(id));
                let pending = self.awaiting_mail.entry(actor.id.clone()).or_default();
                if let Some(cursor) = cursor {
                    Self::discard_pending_through(pending, *cursor);
                }
                OverdueMailSummary::from_timestamps(
                    &actor.id,
                    pending
                        .values()
                        .map(|index| self.sources[*index].ts.as_str()),
                    cutoff,
                    observed_at,
                )
            })
            .collect()
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
                source.notice
                    && source.position >= generation
                    && source.recipients.contains(&actor.id)
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
