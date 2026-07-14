use cccc_contracts::{Actor, ActorRuntime, ActorSubmit, Event, GroupState};
use cccc_core::{GroupStore, system_prompt};
use std::time::Duration;

use crate::ops::actor_delivery::{DeliveryCompletion, DeliveryJob, record_completion};

const SUBMIT_DELAY: Duration = Duration::from_millis(1_500);
const PREAMBLE_DELAY: Duration = Duration::from_millis(500);
const INPUT_MODE_TIMEOUT: Duration = Duration::from_secs(5);

pub fn process(
    job: DeliveryJob,
    preamble_session: &mut String,
    last_delivery: &mut Option<std::time::Instant>,
) {
    let Ok(current_group) =
        GroupStore::new(job.home.clone()).and_then(|store| store.load(&job.group.group_id))
    else {
        return;
    };
    if matches!(
        current_group.state,
        GroupState::Paused | GroupState::Stopped
    ) {
        return;
    }
    let Ok(status) = cccc_runtime::status(&job.group.group_id, &job.actor.id) else {
        return;
    };
    if !status.running {
        return;
    }
    apply_throttle(&current_group, last_delivery);

    if job.actor.runtime != ActorRuntime::Custom && *preamble_session != status.started_at {
        wait_for_input_mode(&job.group.group_id, &job.actor.id);
        if !submit_text(
            &job.group.group_id,
            &job.actor,
            &system_prompt::render(&current_group, &job.actor),
        ) {
            return;
        }
        preamble_session.clone_from(&status.started_at);
        std::thread::sleep(PREAMBLE_DELAY);
    }

    let Some(payload) = render(&job.event) else {
        return;
    };
    if submit_text(&job.group.group_id, &job.actor, &payload) {
        *last_delivery = Some(std::time::Instant::now());
        record_completion(DeliveryCompletion {
            group_id: job.group.group_id,
            actor_id: job.actor.id,
            event_id: job.event.id,
        });
    }
}

fn apply_throttle(group: &cccc_core::GroupDoc, last_delivery: &Option<std::time::Instant>) {
    let seconds = group
        .extra
        .get("settings")
        .and_then(|value| value.get("min_interval_seconds"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let Some(remaining) =
        last_delivery.and_then(|last| Duration::from_secs(seconds).checked_sub(last.elapsed()))
    else {
        return;
    };
    std::thread::sleep(remaining);
}

fn submit_text(group_id: &str, actor: &Actor, text: &str) -> bool {
    let raw = text.trim_end_matches(['\r', '\n']);
    if raw.is_empty() {
        return false;
    }
    let bracketed = raw.contains(['\r', '\n'])
        && cccc_runtime::bracketed_paste_enabled(group_id, &actor.id).unwrap_or(false);
    let payload = if bracketed {
        format!("\u{1b}[200~{raw}\u{1b}[201~")
    } else if raw.contains(['\r', '\n']) {
        raw.lines().collect::<Vec<_>>().join(" ")
    } else {
        raw.to_owned()
    };
    if cccc_runtime::write(group_id, &actor.id, payload.as_bytes()).is_err() {
        return false;
    }
    let submit = match actor.submit {
        ActorSubmit::Enter => b"\r".as_slice(),
        ActorSubmit::Newline => b"\n".as_slice(),
        ActorSubmit::None => return true,
    };
    std::thread::sleep(SUBMIT_DELAY);
    cccc_runtime::write(group_id, &actor.id, submit).is_ok()
}

fn wait_for_input_mode(group_id: &str, actor_id: &str) {
    let deadline = std::time::Instant::now() + INPUT_MODE_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if cccc_runtime::bracketed_paste_enabled(group_id, actor_id).unwrap_or(false) {
            return;
        }
        if !cccc_runtime::status(group_id, actor_id).is_ok_and(|status| status.running) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn render(event: &Event) -> Option<String> {
    let text = event
        .data
        .get("text")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let targets = event
        .data
        .get("to")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    let targets = if targets.is_empty() {
        "@all".to_owned()
    } else {
        targets.join(", ")
    };
    let attachments = event
        .data
        .get("attachments")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("path").and_then(|value| value.as_str()))
        .map(|path| format!("[attachment: {path}]"))
        .collect::<Vec<_>>();
    let mut body = text.trim_end_matches(['\r', '\n']).to_owned();
    if !attachments.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&attachments.join("\n"));
    }
    if body.is_empty() {
        return None;
    }
    Some(if body.contains(['\r', '\n']) {
        format!("[cccc] {} → {targets}:\n{body}", event.by)
    } else {
        format!("[cccc] {} → {targets}: {body}", event.by)
    })
}

#[cfg(test)]
mod tests {
    use super::render;
    use cccc_contracts::Event;
    use serde_json::json;

    #[test]
    fn renders_text_and_attachments() {
        let mut event = Event::new("chat.message", "g_test");
        event.by = "user".into();
        event.data = json!({
            "to":["peer1"],"text":"hello",
            "attachments":[{"path":"state/blobs/report.txt"}]
        })
        .as_object()
        .cloned()
        .expect("object");
        assert_eq!(
            render(&event).as_deref(),
            Some("[cccc] user → peer1:\nhello\n[attachment: state/blobs/report.txt]")
        );
    }
}
