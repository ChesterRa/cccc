use super::*;

pub(super) fn claim(home: &HomeLayout, r: &DaemonRequest) -> OpResult {
    let (g, a) = group_actor(home, r)?;
    let b = r
        .args
        .get("_cccc_web_binding")
        .filter(|b| b["grok_stage"] == "claim" && b["transport"] == "grok_webhook")
        .ok_or_else(|| {
            OpError::new(
                "permission_denied",
                "Use the current Webhook event's claim credential",
            )
        })?;
    if string_arg(r, "by").as_deref() != Some(&a.id) {
        return Err(OpError::new(
            "permission_denied",
            "Claim as the bound Actor",
        ));
    }
    let id = b["connector_id"].as_str().unwrap_or("");
    let mut pulled = r.clone();
    pulled
        .args
        .insert("transport".into(), json!("web_model_pull"));
    // The Group permit precedes this store lock. Canonical active turn state wins
    // even if a process died between turn admission and the private projection.
    let result = private::change(home, id, &g.group_id, &a.id, |c, s| {
        if s["epoch"] != b["grok_epoch"] || s["wake"]["event_id"] != b["grok_wake_id"] {
            return Err(std::io::Error::other("grok_task_expired"));
        }
        let runtime = super::super::runtime_state::actor_state(home, &g.group_id, &a.id)
            .map_err(|_| std::io::Error::other("runtime_unavailable"))?;
        if runtime["active_turn_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
            || s["execution"]["state"] == "active"
        {
            return Ok(Ok(json!({"status":"turn_in_progress","turn":null})));
        }
        if !active(&g, &a)
            || !matches!(
                s["wake"]["state"].as_str(),
                Some("pending" | "sending" | "delivered" | "failed")
            )
        {
            return Ok(Ok(json!({"status":"idle","turn":null})));
        }
        match super::super::runtime_state::wait_next_turn(home, &pulled) {
            Err(error) => Ok(Err(error)),
            Ok(result) => {
                let mut result = Value::Object(result);
                if result["status"] == "work_available" {
                    s["execution"] = json!({
                        "event_id":s["wake"]["event_id"], "turn_id":result["turn"]["turn_id"],
                        "event_ids":result["turn"]["event_ids"], "state":"active"
                    });
                    s["wake"]["state"] = json!("claimed");
                    result["actor_token"] = json!(private::token(
                        c,
                        b,
                        s,
                        "execution",
                        s["wake"]["event_id"].as_str().unwrap_or(""),
                        result["turn"]["turn_id"].as_str().unwrap_or("")
                    )?);
                } else if result["status"] == "idle" {
                    s["wake"]["state"] = json!("completed");
                }
                Ok(Ok(result))
            }
        }
    })
    .map_err(state_error)??;
    object(result)
}

pub(in crate::ops) fn completed(
    home: &HomeLayout,
    g: &GroupDoc,
    actor: &str,
    c: &super::super::runtime_completion::Completion,
) -> Result<(), OpError> {
    let Some(a) = g
        .actors
        .iter()
        .find(|a| a.id == actor && a.runtime == ActorRuntime::GrokWebModel)
    else {
        return Ok(());
    };
    let Some(connector) = connector(home, g, a)? else {
        return Ok(());
    };
    if private::state(&connector, &g.group_id, actor).is_null() {
        return Ok(());
    }
    private::change(
        home,
        connector["connector_id"].as_str().unwrap_or(""),
        &g.group_id,
        actor,
        |_, s| {
            if s["execution"]["turn_id"] == c.turn_id {
                let mut last = s["execution"].clone();
                last["state"] = json!("completed");
                last["status"] = json!(c.status);
                last["completed_at"] = json!(cccc_contracts::utc_now());
                s["last_execution"] = last;
                s["execution"] = Value::Null;
                s["wake"]["state"] = json!("completed");
            }
            Ok(())
        },
    )
    .map_err(state_error)
}

pub(super) fn resolve(home: &HomeLayout, r: &DaemonRequest) -> OpResult {
    user(r)?;
    let (g, a) = group_actor(home, r)?;
    let runtime = super::super::runtime_state::actor_state(home, &g.group_id, &a.id)?;
    let turn = required_arg(r, "turn_id")?;
    let ids = r
        .args
        .get("event_ids")
        .and_then(Value::as_array)
        .filter(|ids| !ids.is_empty() && ids.iter().all(Value::is_string))
        .ok_or_else(|| OpError::new("invalid_event_ids", "Exact active event_ids are required"))?;
    if runtime["active_turn_id"] != turn || runtime["active_event_ids"] != json!(ids) {
        return Err(OpError::new("stale_turn", "The unfinished turn changed"));
    }
    // The Group write permit serializes resolution with claim/completion. Retire
    // this execution's authority without requiring a separate Actor stop/start.
    private::invalidate(home, &g.group_id, &a.id).map_err(state_error)?;
    let ids = ids
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let committed =
        super::super::runtime_completion::committed(home, &g.group_id, &a.id, &turn, &ids)?;
    let c = committed.unwrap_or(super::super::runtime_completion::Completion {
        turn_id: turn.clone(),
        event_ids: ids,
        status: "cancelled".into(),
        delivery_id: format!("runtime:{turn}"),
    });
    for event in &c.event_ids {
        if let Some((state, transport)) =
            super::super::runtime_delivery::latest_state(home, &g.group_id, &a.id, event)?
            && state == "claimed"
        {
            super::super::runtime_delivery::append_state(
                home,
                &g.group_id,
                &a.id,
                &a.created_at,
                event,
                &transport,
                super::super::runtime_delivery::DeliveryOutcome::Ambiguous(
                    "Unfinished native turn explicitly resolved; do not replay",
                ),
            )?;
        }
    }
    let receipt = super::super::runtime_completion::append(home, &g.group_id, &a.id, &c)?;
    super::super::runtime_state::finish_completion(home, &g, &a.id, &c, receipt, r)
}
