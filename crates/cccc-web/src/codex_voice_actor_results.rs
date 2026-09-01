use cccc_contracts::Event;
use cccc_core::HomeLayout;
use cccc_core::context::ContextStore;
use std::sync::Weak;
use std::time::Duration;

use crate::codex_voice::AnalystRuntime;
use crate::ledger_event_hub::LedgerEventHub;
use cccc_daemon::experimental_codex_voice::TrackedWork;

const REPLAY_PAGE_SIZE: usize = 2_048;
const RESULT_GRACE: Duration = Duration::from_secs(5);
const RUNTIME_POLL: Duration = Duration::from_secs(1);
const MAX_RESULT_CHARS: usize = 16_000;

pub(crate) struct ObservedActorResult {
    pub correlation_id: String,
    pub prompt: String,
}

pub(crate) fn spawn(
    runtime: Weak<AnalystRuntime>,
    home: HomeLayout,
    events: LedgerEventHub,
    call_generation: String,
    tracking_key: String,
    work: TrackedWork,
) {
    tokio::spawn(async move {
        let outcome = observe(&runtime, &home, &events, &call_generation, &work).await;
        let Some(runtime) = runtime.upgrade() else {
            return;
        };
        runtime.finish_tracking(&tracking_key);
        match outcome {
            Ok(Some(result)) => {
                runtime.accept_actor_result(&call_generation, result).await;
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(
                %error,
                group_id = %work.group_id,
                task_id = %work.task_id,
                "Voice Analyst linked-work observer stopped"
            ),
        }
    });
}

async fn observe(
    runtime: &Weak<AnalystRuntime>,
    home: &HomeLayout,
    events: &LedgerEventHub,
    _call_generation: &str,
    work: &TrackedWork,
) -> std::io::Result<Option<ObservedActorResult>> {
    let (mut receiver, _) = events.subscribe_group_with_cursor(&work.group_id)?;
    let mut cursor = work.source_event_id.clone();
    let mut done_since = task_done(home, work).then(tokio::time::Instant::now);

    if let Some(result) = replay_result(events, home, work, &mut cursor, &mut done_since)? {
        return Ok(Some(result));
    }

    let mut runtime_poll = tokio::time::interval(RUNTIME_POLL);
    runtime_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if done_since.is_some_and(|started| started.elapsed() >= RESULT_GRACE) {
            return Ok(Some(missing_result(work)));
        }
        tokio::select! {
            _ = runtime_poll.tick() => {
                if runtime.upgrade().is_none() {
                    return Ok(None);
                }
            }
            event = receiver.recv() => match event {
                Ok(event) => {
                    cursor.clone_from(&event.id);
                    if let Some(result) = actor_reply(&event, work) {
                        return Ok(Some(result));
                    }
                    if done_since.is_none() && event.kind == "context.sync" && task_done(home, work) {
                        done_since = Some(tokio::time::Instant::now());
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if let Some(result) = replay_result(
                        events,
                        home,
                        work,
                        &mut cursor,
                        &mut done_since,
                    )? {
                        return Ok(Some(result));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(None),
            }
        }
    }
}

fn replay_result(
    events: &LedgerEventHub,
    home: &HomeLayout,
    work: &TrackedWork,
    cursor: &mut String,
    done_since: &mut Option<tokio::time::Instant>,
) -> std::io::Result<Option<ObservedActorResult>> {
    loop {
        let page = events.replay_after(&work.group_id, cursor, REPLAY_PAGE_SIZE)?;
        let page_len = page.len();
        for event in page {
            cursor.clone_from(&event.id);
            if let Some(result) = actor_reply(&event, work) {
                return Ok(Some(result));
            }
            if done_since.is_none() && event.kind == "context.sync" && task_done(home, work) {
                *done_since = Some(tokio::time::Instant::now());
            }
        }
        if page_len < REPLAY_PAGE_SIZE {
            return Ok(None);
        }
    }
}

fn actor_reply(event: &Event, work: &TrackedWork) -> Option<ObservedActorResult> {
    if event.kind != "chat.message"
        || event.by.trim() != work.actor_id
        || event.data.get("reply_to")?.as_str()?.trim() != work.source_event_id
    {
        return None;
    }
    let text = event.data.get("text")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let text = text.chars().take(MAX_RESULT_CHARS).collect::<String>();
    Some(ObservedActorResult {
        correlation_id: format!("actor-result:{}:{}", work.source_event_id, event.id),
        prompt: format!(
            "Tracked CCCC work {} in Group {} returned this authoritative Actor reply:\n\n{}\n\nVerify any material claim with current CCCC facts if needed, then give the user one concise result and any essential caveat. Do not start duplicate work.",
            work.task_id, work.group_id, text
        ),
    })
}

fn missing_result(work: &TrackedWork) -> ObservedActorResult {
    ObservedActorResult {
        correlation_id: format!("actor-result-missing:{}", work.source_event_id),
        prompt: format!(
            "Tracked CCCC task {} in Group {} reached done, but no usable reply to source event {} was recorded. Report exactly that the work completed but its result report is missing. Do not infer or recreate the result.",
            work.task_id, work.group_id, work.source_event_id
        ),
    }
}

fn task_done(home: &HomeLayout, work: &TrackedWork) -> bool {
    ContextStore::new(home.clone())
        .and_then(|store| store.load(&work.group_id))
        .ok()
        .and_then(|context| {
            context.tasks.into_iter().find(|task| {
                task.get("id").and_then(serde_json::Value::as_str) == Some(work.task_id.as_str())
            })
        })
        .and_then(|task| task.get("status").cloned())
        .and_then(|status| status.as_str().map(str::to_owned))
        .is_some_and(|status| status == "done")
}

#[cfg(test)]
mod tests {
    use super::{actor_reply, missing_result};
    use cccc_contracts::Event;
    use cccc_daemon::experimental_codex_voice::TrackedWork;
    use serde_json::json;

    fn work() -> TrackedWork {
        TrackedWork {
            group_id: "g_target".into(),
            task_id: "T007".into(),
            source_event_id: "event-source".into(),
            actor_id: "worker".into(),
        }
    }

    #[test]
    fn accepts_only_a_non_empty_reply_to_the_tracked_source() {
        let mut event = Event::new("chat.message", "g_target");
        event.by = "other-actor".into();
        event.data.insert("reply_to".into(), json!("event-source"));
        event.data.insert("text".into(), json!(" ACTOR_RESULT "));
        assert!(actor_reply(&event, &work()).is_none());

        event.by = "worker".into();
        let result = actor_reply(&event, &work()).expect("matching reply");
        assert!(result.prompt.contains("ACTOR_RESULT"));

        event.data.insert("reply_to".into(), json!("another-event"));
        assert!(actor_reply(&event, &work()).is_none());
    }

    #[test]
    fn missing_result_never_invents_actor_output() {
        let result = missing_result(&work());
        assert!(result.prompt.contains("result report is missing"));
        assert!(result.prompt.contains("Do not infer"));
    }
}
