use cccc_contracts::Actor;
use cccc_core::{GroupDoc, HomeLayout};

pub(super) fn has_completed_event(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event_id: &str,
) -> bool {
    let _ = actor;
    crate::ops::local_headless::contains_event_dedupe(
        home,
        &group.group_id,
        &format!("deepseek.turn:headless.turn.completed:{event_id}"),
    )
    .unwrap_or(false)
}

/// A delivery is settled by a provider completion or a durable Foreman handoff.
/// Failure handoff accepts the original input; it never completes its task.
pub(crate) fn settle_delivery(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    source: &cccc_contracts::Event,
) -> std::io::Result<bool> {
    use cccc_core::{GroupStore, actors, ledger};
    use serde_json::{Value, json};

    let store = GroupStore::new(home.clone())?;
    let events = ledger::read_all(&store.ledger_path(&group.group_id)?)?;
    let turn_id = format!("deepseek:delivery:{}", source.id);
    let reports = events
        .iter()
        .filter(|event| {
            event.kind == "chat.message"
                && event.by == actor.id
                && event.data.get("reply_to").and_then(Value::as_str) == Some(&source.id)
        })
        .map(|event| &event.id)
        .collect::<Vec<_>>();
    if events.iter().any(|event| {
        (event.kind == "coordination.handoff"
            && event.by == actor.id
            && event.data.get("turn_id").and_then(Value::as_str) == Some(turn_id.as_str()))
            || (event.kind == "coordination.decision"
                && event.data.get("status").and_then(Value::as_str) == Some("applied")
                && event
                    .data
                    .get("source_event_ids")
                    .and_then(Value::as_array)
                    .is_some_and(|ids| {
                        reports
                            .iter()
                            .any(|id| ids.iter().any(|value| value.as_str() == Some(id.as_str())))
                    }))
    }) {
        return Ok(true);
    }
    let directory = store.state_dir(&group.group_id)?.join("headless");
    let file = match std::fs::File::open(directory.join("events.jsonl")) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let (attempts, completed, error) =
        cccc_core::fs::with_exclusive_lock(&directory.join("events.lock"), || {
            let mut attempts = 0_usize;
            let mut completed = false;
            let mut error = Value::Null;
            for event in serde_json::Deserializer::from_reader(std::io::BufReader::new(file))
                .into_iter::<Value>()
            {
                let event = event?;
                if event["actor_id"] != actor.id || event["data"]["event_id"] != source.id {
                    continue;
                }
                match event["type"].as_str() {
                    Some("headless.turn.started") => attempts += 1,
                    Some("headless.turn.completed") => completed = true,
                    Some("headless.turn.failed")
                        if event["data"]["error"]["code"] == "cancelled" =>
                    {
                        attempts = attempts.saturating_sub(1);
                    }
                    Some("headless.turn.failed") => error = event["data"]["error"].clone(),
                    _ => {}
                }
            }
            Ok((attempts, completed, error))
        })?;
    let permanent = matches!(
        error["code"].as_str(),
        Some("credential_unavailable" | "context_window_exceeded")
    );
    if !completed && attempts < 3 && !permanent {
        return Ok(false);
    }
    let current = store.load(&group.group_id)?;
    let web_lead = actors::unique_available_foreman(&current).is_ok_and(|lead| {
        lead.runtime == cccc_contracts::ActorRuntime::WebModel && lead.id != actor.id
    });
    if !web_lead {
        return Ok(completed);
    }
    if !current.running
        || matches!(
            current.state,
            cccc_contracts::GroupState::Paused | cccc_contracts::GroupState::Stopped
        )
    {
        return Ok(false);
    }
    // Provider diagnostics can contain credentials; forward only known public summaries.
    let reason = if error["message"]
        .as_str()
        .is_some_and(|message| message.contains("SSE stream ended without [DONE]"))
    {
        "DSH output stream ended without [DONE]"
    } else {
        match error["code"].as_str() {
            Some("credential_unavailable") => "DSH credential is unavailable",
            Some("context_window_exceeded") => "DSH model context window exceeded",
            Some("timeout") => "DSH turn timed out",
            _ => "DSH turn did not complete; inspect local runtime events for details",
        }
    };
    super::super::local_headless::notify_web_foreman(
        home,
        &group.group_id,
        &actor.id,
        &turn_id,
        &source.ts,
        &json!({"params":{"turn":{"status":if completed {"completed"} else {"failed"},
            "source_event_id":source.id,"error":{"message":if completed {""} else {reason}}}}}),
    )
}
