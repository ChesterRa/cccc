//! A declined/closed confirmation must not be reopened by queued delivery.
//! This is daemon-local intent, not a provider receipt or a change to Actor configuration.
use super::supervisor::Key;
use std::collections::HashSet;
use std::fmt;
use std::io;
use std::sync::{Mutex, OnceLock};

fn paused() -> &'static Mutex<HashSet<Key>> {
    static PAUSED: OnceLock<Mutex<HashSet<Key>>> = OnceLock::new();
    PAUSED.get_or_init(|| Mutex::new(HashSet::new()))
}

pub(super) fn pause(key: Key) -> io::Result<()> {
    paused().lock().map_err(|_| super::poisoned())?.insert(key);
    Ok(())
}

pub(super) fn retry(key: &Key) -> io::Result<()> {
    paused().lock().map_err(|_| super::poisoned())?.remove(key);
    Ok(())
}

pub(super) fn clear_group(group_id: &str) -> io::Result<()> {
    paused()
        .lock()
        .map_err(|_| super::poisoned())?
        .retain(|(paused_group, _)| paused_group != group_id);
    Ok(())
}

#[derive(Debug)]
struct ConfirmationPaused;

impl fmt::Display for ConfirmationPaused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Claude's confirmation ended before the managed session connected, so this daemon pauses automatic starts and starts requested by agents; ask the user to launch the agent again")
    }
}

impl std::error::Error for ConfirmationPaused {}

pub(super) fn is_paused_error(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<ConfirmationPaused>())
}

pub(super) fn is_paused(key: &Key) -> io::Result<bool> {
    Ok(paused()
        .lock()
        .map_err(|_| super::poisoned())?
        .contains(key))
}

pub(super) fn ensure_allowed(key: &Key) -> io::Result<()> {
    if is_paused(key)? {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            ConfirmationPaused,
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "confirmation_pause_group_tests.rs"]
mod group_tests;

#[cfg(test)]
mod tests {
    use super::super::workspace_trust_recovery::Recovery;
    use super::*;

    #[test]
    fn replaced_or_cancelled_confirmation_cannot_pause_a_new_attempt() {
        let key = (uuid::Uuid::new_v4().to_string(), "actor".to_owned());
        let old = Recovery::register(key.clone()).expect("old owner");
        let current = Recovery::register(key.clone()).expect("replacement owner");
        assert!(!old.pause_if_current().expect("retired owner"));
        ensure_allowed(&key).expect("old refusal is ignored");
        assert!(current.pause_if_current().expect("current refusal"));
        assert_eq!(
            ensure_allowed(&key).expect_err("paused").kind(),
            io::ErrorKind::WouldBlock
        );
        super::super::retry_confirmation(&key.0, &key.1).expect("explicit retry");
        assert!(current.cancelled());
        assert!(
            !current
                .pause_if_current()
                .expect("late exit from cancelled owner")
        );
        ensure_allowed(&key).expect("late exit cannot undo explicit retry");
    }

    #[test]
    fn queued_retry_cannot_cancel_a_confirmation_owner_after_its_pty_has_exited() {
        use crate::ops::{actor_delivery::DeliveryJob, actor_delivery_worker};
        use cccc_contracts::{Actor, ActorRuntime, Event};
        use cccc_core::{GroupStore, HomeLayout};
        use std::sync::atomic::AtomicBool;
        let temp = tempfile::tempdir().expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("Pending confirmation", "").expect("group");
        let mut actor = Actor::new("claude");
        actor.runtime = ActorRuntime::Claude;
        group.actors.push(actor.clone());
        group.running = true;
        store.save(&group).expect("save actor");
        let key = (group.group_id.clone(), actor.id.clone());
        // Exact watcher state during the exit-to-poll gap: registered, no live PTY.
        let recovery = Recovery::register(key.clone()).expect("pending owner");
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = serde_json::json!({"to":[actor.id],"text":"PENDING","message_mode":"send"})
            .as_object()
            .expect("data")
            .clone();
        let id = event.id.clone();
        let mut jobs = vec![DeliveryJob {
            home,
            group,
            actor,
            event,
            claimed: true,
        }];
        for _ in 0..3 {
            assert_eq!(
                actor_delivery_worker::process_batch(
                    &mut jobs,
                    &mut String::new(),
                    &AtomicBool::new(false)
                ),
                actor_delivery_worker::BatchOutcome::Retry
            );
            assert!(
                !recovery.cancelled(),
                "auto-start must not steal the pending owner"
            );
            assert_eq!(jobs[0].event.id, id);
            assert!(jobs[0].claimed);
        }
        assert!(
            recovery
                .pause_if_current()
                .expect("watcher can still record refusal")
        );
        super::super::retry_confirmation(&key.0, &key.1).expect("explicit retry");
        ensure_allowed(&key).expect("retry released the pause");
    }
}
