use cccc_contracts::{DaemonRequest, Event};
use cccc_core::automation::{ScheduledAction, TickResult};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, actors, inbox};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::dispatch::dispatch;
use crate::ops::{actor_delivery, actor_runtime, actor_runtime_status, group_runtime};

pub fn prepare_exited() -> BTreeMap<String, Vec<cccc_runtime::SessionStatus>> {
    let exited = match actor_runtime::reap_exited() {
        Ok(exited) => exited,
        Err(error) => {
            tracing::warn!(message = %error.message, "runtime reap failed");
            Vec::new()
        }
    };
    let mut grouped = BTreeMap::<String, Vec<_>>::new();
    for status in exited {
        grouped
            .entry(status.group_id.clone())
            .or_default()
            .push(status);
    }
    grouped
}

pub fn pending_delivery_group_ids() -> Vec<String> {
    actor_delivery::pending_group_ids()
}

pub fn maintain_group(home: &HomeLayout, group_id: &str, exited: Vec<cccc_runtime::SessionStatus>) {
    actor_delivery::drain_group(home, group_id);
    if let Err(error) = actor_runtime::reconcile_exited(home, exited) {
        tracing::warn!(message = %error.message, %group_id, "runtime reconciliation failed");
    }
}

pub fn group_ids(home: &HomeLayout) -> Vec<String> {
    match cccc_core::automation::group_ids(home) {
        Ok(group_ids) => group_ids,
        Err(error) => {
            tracing::warn!(%error, "automation group discovery failed");
            Vec::new()
        }
    }
}

pub fn tick_group(home: &HomeLayout, group_id: &str, include_unread: bool, cancelled: &AtomicBool) {
    if cancelled.load(Ordering::Acquire) {
        return;
    }
    let delivery_actor_ids = if include_unread {
        GroupStore::new(home.clone())
            .and_then(|store| store.load(group_id))
            .map(|group| {
                actors::visible(&group)
                    .filter(|actor| actor.enabled)
                    .filter(|actor| actor_runtime_status::resolve(&group, actor).running)
                    .map(|actor| actor.id.clone())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default()
    } else {
        HashSet::new()
    };
    let idle_actor_ids = idle_actor_ids(group_id, &delivery_actor_ids);
    match cccc_core::automation::tick_group_with_idle_actors(
        home,
        group_id,
        include_unread,
        &delivery_actor_ids,
        &idle_actor_ids,
    ) {
        Ok(result) => apply(home, result, cancelled),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(%error, %group_id, "automation group tick failed"),
    }
}

/// Actors whose managed session is idle. Waiting Mail reaches them promptly;
/// actors that are working, waiting on approval, or whose state is unknown (no
/// managed session) keep the longer interruption-avoiding delay. Delivery
/// rechecks this at admission, since the Actor may start work in between.
fn idle_actor_ids(group_id: &str, actor_ids: &HashSet<String>) -> HashSet<String> {
    actor_ids
        .iter()
        .filter(|actor_id| super::local_headless::ready_for_mail_notice(group_id, actor_id))
        .cloned()
        .collect()
}

fn apply(home: &HomeLayout, result: TickResult, cancelled: &AtomicBool) {
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    let mut completed_notifications = BTreeSet::new();
    for event in &result.notifications {
        let rule_id = event
            .data
            .get("context")
            .and_then(|context| context.get("rule_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if rule_id.is_empty()
            || event.data.get("kind").and_then(serde_json::Value::as_str) != Some("automation")
        {
            continue;
        }
        let Ok(group) = store.load(&event.group_id) else {
            continue;
        };
        let one_time = group
            .automation
            .get("rules")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .any(|rule| {
                rule.get("id").and_then(serde_json::Value::as_str) == Some(rule_id)
                    && rule
                        .get("trigger")
                        .and_then(|trigger| trigger.get("kind"))
                        .and_then(serde_json::Value::as_str)
                        == Some("at")
            });
        let key = (event.group_id.clone(), rule_id.to_owned());
        if one_time && completed_notifications.insert(key) {
            let fired_at = chrono::DateTime::parse_from_rfc3339(&event.ts)
                .map(|value| value.timestamp())
                .unwrap_or_else(|_| chrono::Utc::now().timestamp());
            complete_rule(home, &event.group_id, rule_id, fired_at, true);
        }
    }
    for event in result.notifications {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        if let Ok(group) = store.load(&event.group_id) {
            actor_delivery::dispatch(home, &group, &event);
        }
    }
    for action in result.actions {
        if cancelled.load(Ordering::Acquire) {
            return;
        }
        match action {
            ScheduledAction::GroupState {
                group_id,
                state,
                rule_id,
                fired_at,
                one_time,
            } => {
                let op = if state == "stopped" {
                    "group_stop"
                } else {
                    "group_set_state"
                };
                let mut succeeded = true;
                if state == "active"
                    && store.load(&group_id).is_ok_and(|group| {
                        !group_runtime::status(&group)["runtime_running"]
                            .as_bool()
                            .unwrap_or(false)
                    })
                {
                    if cancelled.load(Ordering::Acquire) {
                        return;
                    }
                    succeeded = call(
                        home,
                        "group_start",
                        json!({"group_id":group_id,"by":"user"}),
                    );
                }
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                if succeeded {
                    succeeded = call(
                        home,
                        op,
                        json!({"group_id":group_id,"state":state,"by":"user"}),
                    );
                }
                if succeeded {
                    complete_rule(home, &group_id, &rule_id, fired_at, one_time);
                }
            }
            ScheduledAction::ActorControl {
                group_id,
                operation,
                targets,
                rule_id,
                fired_at,
                one_time,
            } => {
                let Ok(group) = store.load(&group_id) else {
                    continue;
                };
                let op = match operation.as_str() {
                    "start" => "actor_start",
                    "stop" => "actor_stop",
                    "restart" => "actor_restart",
                    _ => continue,
                };
                let mut succeeded = false;
                for actor_id in matching_actors(&group, &targets) {
                    if cancelled.load(Ordering::Acquire) {
                        return;
                    }
                    succeeded |= call(
                        home,
                        op,
                        json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
                    );
                }
                if succeeded {
                    complete_rule(home, &group_id, &rule_id, fired_at, one_time);
                }
            }
        }
    }
}

fn matching_actors(group: &GroupDoc, targets: &[String]) -> Vec<String> {
    if targets.is_empty() {
        return Vec::new();
    }
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "system".into();
    event.data = json!({"to":targets})
        .as_object()
        .cloned()
        .unwrap_or_default();
    actors::visible(group)
        .filter(|actor| inbox::is_for_actor(group, &event, &actor.id))
        .map(|actor| actor.id.clone())
        .collect()
}

fn complete_rule(home: &HomeLayout, group_id: &str, rule_id: &str, fired_at: i64, one_time: bool) {
    if let Err(error) = cccc_core::automation::mark_rule_fired(home, group_id, rule_id, fired_at) {
        tracing::warn!(%error, %group_id, %rule_id, "automation completion state failed");
        return;
    }
    if one_time {
        call(
            home,
            "group_automation_manage",
            json!({
                "group_id":group_id,
                "by":"user",
                "actions":[{"type":"set_rule_enabled","rule_id":rule_id,"enabled":false}]
            }),
        );
    }
}

fn call(home: &HomeLayout, op: &str, value: serde_json::Value) -> bool {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: value.as_object().cloned().unwrap_or_default(),
    };
    let response = dispatch(home, &request);
    if !response.ok {
        tracing::warn!(%op, "scheduled automation action failed");
    }
    response.ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;
    use cccc_core::{GroupStore, HomeLayout, actors};

    /// Issue the next unread-tick Mail notice for an idle `peer`, if any.
    fn idle_notice(home: &HomeLayout, group_id: &str) -> Option<Event> {
        let peer = HashSet::from(["peer".to_owned()]);
        cccc_core::automation::tick_group_with_idle_actors(home, group_id, true, &peer, &peer)
            .expect("tick")
            .notifications
            .into_iter()
            .find(|event| event.data["kind"] == "mail_notice")
    }

    #[test]
    fn a_withheld_early_notice_survives_a_restart_and_is_reissued_only_while_mail_is_unread() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("withheld notice", "").expect("group");
        store
            .mutate(&group.group_id, |group| {
                group.state = cccc_contracts::GroupState::Active;
                actors::add(group, Actor::new("peer"))?;
                group
                    .extra
                    .insert("delivery".into(), json!({"mail_notice_after_seconds":3600}));
                Ok(())
            })
            .expect("group");
        let mut mail = Event::new("chat.message", &group.group_id);
        mail.by = "user".into();
        mail.ts = (chrono::Utc::now() - chrono::Duration::seconds(120)).to_rfc3339();
        mail.data = json!({"text":"review","to":["peer"],"message_mode":"mail"})
            .as_object()
            .cloned()
            .expect("mail");
        let ledger = store.ledger_path(&group.group_id).expect("ledger");
        cccc_core::ledger::append(&ledger, &mail).expect("Mail");
        // The unread tick records each notice it issues.
        let first = idle_notice(&home, &group.group_id).expect("early notice");
        let group = store.load(&group.group_id).expect("group");
        let peer = group.actors[0].clone();

        // The worker claims the notice, then withholds it: the peer turned busy.
        let claim =
            crate::ops::runtime_delivery::claim(&home, &group, &peer, &first.id, "pty", false)
                .expect("claim");
        assert_eq!(claim, crate::ops::runtime_delivery::ClaimResult::Claimed);
        let job = super::super::actor_delivery::DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: peer.clone(),
            event: first.clone(),
        };
        super::super::actor_delivery::withdraw_jobs(std::slice::from_ref(&job), "held");

        // A restart neither strands it as ambiguous nor redelivers it.
        crate::ops::runtime_delivery::settle_stranded_claims(&home, &group).expect("restart");
        let state =
            crate::ops::runtime_delivery::latest_state(&home, &group.group_id, "peer", &first.id)
                .expect("state")
                .expect("delivery recorded");
        assert_eq!(state.0, "withdrawn");
        let pending = crate::ops::runtime_delivery::pending_sources(&home, &group, &peer, 10)
            .expect("pending");
        assert!(pending.iter().all(|event| event.id != first.id));
        assert_eq!(
            crate::ops::runtime_delivery::claim(&home, &group, &peer, &first.id, "pty", false)
                .expect("claim"),
            crate::ops::runtime_delivery::ClaimResult::Terminal("withdrawn".into()),
            "a withdrawn notice is never dispatched again"
        );

        // The still-unread Mail gets a fresh notice once the peer is idle again.
        let second = idle_notice(&home, &group.group_id).expect("re-issued notice");
        assert_ne!(second.id, first.id);
        assert_eq!(second.data["context"]["source_event_ids"], json!([mail.id]));
        // Re-issuing does not push the deadline back for the same unread Mail.
        assert_eq!(
            second.data["context"]["deliver_by"],
            first.data["context"]["deliver_by"]
        );
        let events = cccc_core::ledger::read_all(&ledger).expect("events");
        for notice in [&first, &second] {
            assert_eq!(
                events.iter().filter(|event| event.id == notice.id).count(),
                1
            );
        }

        // Mail read before that notice is delivered is never announced again.
        let claim =
            crate::ops::runtime_delivery::claim(&home, &group, &peer, &second.id, "pty", false)
                .expect("claim");
        assert_eq!(claim, crate::ops::runtime_delivery::ClaimResult::Claimed);
        cccc_core::inbox::consume_unread(&home, &group, "peer", "peer", 50).expect("read Mail");
        super::super::actor_delivery::withdraw_jobs(
            &[super::super::actor_delivery::DeliveryJob {
                event: second,
                ..job
            }],
            "held",
        );
        assert!(idle_notice(&home, &group.group_id).is_none());
    }

    #[test]
    fn actors_without_a_managed_session_are_never_treated_as_idle() {
        // No session reports a finished turn, so Mail keeps the longer delay.
        let actors = HashSet::from(["pty-peer".to_owned(), "stopped-peer".to_owned()]);
        assert!(idle_actor_ids("g_no_sessions", &actors).is_empty());
    }
    use serde_json::json;

    #[test]
    fn failed_action_is_left_due_for_the_next_tick() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("automation failure", "").expect("group");
        set_rule(
            &store,
            &group.group_id,
            json!({"kind":"actor_control","operation":"start","targets":["missing"]}),
        );

        tick_group(&home, &group.group_id, false, &AtomicBool::new(false));

        assert_eq!(
            cccc_core::automation::tick_group(&home, &group.group_id, false)
                .expect("retry tick")
                .actions
                .len(),
            1
        );
    }

    #[test]
    fn successful_one_time_action_commits_and_disables_the_rule() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("automation success", "").expect("group");
        set_rule(
            &store,
            &group.group_id,
            json!({"kind":"group_state","state":"paused"}),
        );

        tick_group(&home, &group.group_id, false, &AtomicBool::new(false));

        let updated = store.load(&group.group_id).expect("updated group");
        assert_eq!(updated.state, cccc_contracts::GroupState::Paused);
        assert_eq!(updated.automation["rules"][0]["enabled"], false);
        let state: serde_json::Value = cccc_core::fs::read_json(
            &store
                .state_dir(&group.group_id)
                .expect("state dir")
                .join("automation.json"),
        )
        .expect("automation state");
        assert!(state["rules"]["once"]["last_fired_at"].is_string());
    }

    #[test]
    fn successful_one_time_notification_disables_the_rule() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("automation notify", "").expect("group");
        store
            .mutate(&group.group_id, |group| {
                actors::add(group, Actor::new("peer"))?;
                Ok(())
            })
            .expect("recipient");
        set_rule(
            &store,
            &group.group_id,
            json!({"kind":"notify","message":"run once"}),
        );

        tick_group(&home, &group.group_id, false, &AtomicBool::new(false));

        let updated = store.load(&group.group_id).expect("updated group");
        assert_eq!(
            updated.automation["rules"][0]["enabled"], false,
            "a durably appended one-time notification is complete and must not survive export/import as enabled"
        );
    }

    fn set_rule(store: &GroupStore, group_id: &str, action: serde_json::Value) {
        store
            .mutate(group_id, |group| {
                group.automation = json!({
                    "version":1,
                    "rules":[{
                        "id":"once","enabled":true,"scope":"group",
                        "trigger":{"kind":"at","at":"2020-01-01T00:00:00Z"},
                        "action":action
                    }]
                })
                .as_object()
                .cloned()
                .expect("automation");
                Ok(())
            })
            .expect("automation rule");
    }
}
