use super::*;

pub(super) fn claim(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group, actor) = group_actor(home, request)?;
    let captured = request
        .args
        .get("_cccc_web_binding")
        .filter(|b| b.is_object())
        .ok_or_else(|| {
            OpError::new(
                "permission_denied",
                "A current paired Dots session is required",
            )
        })?;
    if string_arg(request, "by").as_deref() != Some(actor.id.as_str()) {
        return Err(OpError::new(
            "permission_denied",
            "Dots work must be claimed by its bound Actor",
        ));
    }
    if captured["dots_stage"] != "claim" {
        return Err(OpError::new(
            "permission_denied",
            "Dots work requires its paired session's claim authority",
        ));
    }
    let id = captured["connector_id"]
        .as_str()
        .ok_or_else(|| OpError::new("permission_denied", "Dots binding is required"))?;
    let captured_wake = captured
        .get("dots_wake_id")
        .filter(|wake| wake.is_null() || wake.as_str().is_some_and(|id| !id.is_empty()))
        .ok_or_else(|| OpError::new("permission_denied", "A Dots wake snapshot is required"))?;
    let mut pulled = request.clone();
    pulled
        .args
        .insert("transport".into(), json!("web_model_pull"));
    let result = store::dots_update(home, id, |c| {
        let Some(route) = binding(c, &group, &actor) else {
            return Err(denied("dots_binding_changed"));
        };
        if route["revision"] != captured["revision"]
            || route["session_key"].as_str().is_none_or(str::is_empty)
            || route["session_key"] != captured["session_key"]
            || c["dots"]["epoch"] != captured["dots_epoch"]
            || c["dots"]["wake"]["event_id"] != *captured_wake
        {
            return Err(denied("dots_task_expired"));
        }
        let dots = &c["dots"];
        let runtime = super::super::runtime_state::actor_state(home, &group.group_id, &actor.id)
            .map_err(|_| denied("dots_runtime_state_unavailable"))?;
        // Canonical ownership wins even if a crash left the private execution
        // projection absent or only part of the source admission committed.
        if runtime["active_turn_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
            || dots["execution"]["state"] == "active"
            || (!captured_wake.is_null()
                && (dots["execution"]["event_id"] == *captured_wake
                    || dots["last_execution"]["event_id"] == *captured_wake))
        {
            return Ok(Ok(json!({"status":"turn_in_progress","turn":null})));
        }
        let Some(event_id) = captured_wake.as_str() else {
            return Ok(Ok(json!({"status":"idle","turn":null})));
        };
        if !active(&group, &actor)
            || !matches!(
                dots["wake"]["state"].as_str(),
                Some("pending" | "sending" | "delivered")
            )
            || !subscribed(&dots["subscription"], chrono::Utc::now().timestamp_millis())
        {
            return Ok(Ok(json!({"status":"idle","turn":null})));
        }
        let result = super::super::runtime_state::wait_next_turn(home, &pulled);
        match result {
            Err(error) => Ok(Err(error)),
            Ok(result) => {
                let mut result = Value::Object(result);
                if result["status"] == "work_available" {
                    c["dots"]["execution"] = json!({"event_id":event_id,"epoch":c["dots"]["epoch"],
                        "turn_id":result["turn"]["turn_id"],"event_ids":result["turn"]["event_ids"],
                        "state":"active","claimed_at":cccc_contracts::utc_now()});
                    c["dots"]["wake"]["state"] = json!("claimed");
                    // This value is returned only after dots_update commits the
                    // private projection. It never enters the wake or ledger.
                    result["actor_token"] = json!(store::dots_wake_token(c, &route, event_id)?);
                } else if result["status"] == "idle" {
                    c["dots"]["wake"]["state"] = json!("completed");
                }
                Ok(Ok(result))
            }
        }
    })
    .map_err(state_error)??;
    object(result)
}

pub(in crate::ops) fn record_completed(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    completion: &super::super::runtime_completion::Completion,
    user_resolution: bool,
) -> Result<(), OpError> {
    let actor = group
        .actors
        .iter()
        .find(|a| a.id == actor_id)
        .ok_or_else(|| OpError::new("actor_not_found", "Actor not found"))?;
    let Some(c) = connector(home, group, actor)? else {
        if user_resolution {
            return Ok(());
        }
        return Err(OpError::new(
            "dots_binding_unavailable",
            "The Dots binding is unavailable",
        ));
    };
    let id = c["connector_id"].as_str().ok_or_else(|| {
        OpError::new(
            "dots_binding_unavailable",
            "The Dots binding is unavailable",
        )
    })?;
    store::dots_update(home, id, |c| {
        let dots = &mut c["dots"];
        if dots["last_execution"]["turn_id"] == completion.turn_id
            && dots["last_execution"]["event_ids"] == json!(completion.event_ids)
            && dots["last_execution"]["status"] == completion.status
        {
            return Ok(());
        }
        let execution = &dots["execution"];
        if execution["turn_id"] != completion.turn_id
            || execution["event_ids"] != json!(completion.event_ids)
        {
            return Err(denied("dots_completion_conflict"));
        }
        let mut last = execution.clone();
        last["state"] = json!("completed");
        last["status"] = json!(completion.status);
        last["completed_at"] = json!(cccc_contracts::utc_now());
        // Explicit user resolution uses the current epoch; old remote authority
        // remains retired even when the receipt resolves an interrupted turn.
        last["epoch"] = dots["epoch"].clone();
        dots["last_execution"] = last;
        dots["execution"] = Value::Null;
        dots["wake"]["state"] = json!("completed");
        Ok(())
    })
    .map_err(state_error)
}

pub(super) fn control(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let (group, actor) = group_actor(home, request)?;
    if required_arg(request, "action")? != "resolve" {
        return Err(OpError::new(
            "invalid_action",
            "Only explicit unfinished-turn resolution is supported",
        ));
    }
    let runtime = super::super::runtime_state::actor_state(home, &group.group_id, &actor.id)?;
    let turn_id = required_arg(request, "turn_id")?;
    let event_ids = request
        .args
        .get("event_ids")
        .and_then(Value::as_array)
        .filter(|ids| !ids.is_empty() && ids.len() <= 20 && ids.iter().all(Value::is_string))
        .ok_or_else(|| {
            OpError::new(
                "invalid_event_ids",
                "The exact unfinished event_ids are required",
            )
        })?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if runtime["active_turn_id"] != turn_id || runtime["active_event_ids"] != json!(event_ids) {
        return Err(OpError::new(
            "stale_turn",
            "This is not the current unfinished Dots turn",
        ));
    }
    let committed = super::super::runtime_completion::committed(
        home,
        &group.group_id,
        &actor.id,
        &turn_id,
        &event_ids,
    )?;
    // This resolves CCCC authority, not the cloud process. No work is replayed.
    if let Some(c) = connector(home, &group, &actor)? {
        invalidate(home, &group, &actor.id)?;
        let id = c["connector_id"].as_str().ok_or_else(|| {
            OpError::new(
                "dots_binding_unavailable",
                "The Dots binding is unavailable",
            )
        })?;
        // A crash between the existing turn commit and the private projection may
        // leave no execution projection. The exact durable turn remains authority
        // for this explicit user resolution; it is never returned to cloud work.
        store::dots_update(home, id, |c| {
        if c["dots"]["execution"].is_null() {
            c["dots"]["execution"] = json!({"event_id":c["dots"]["wake"]["event_id"],
                "epoch":c["dots"]["epoch"],"turn_id":turn_id,"event_ids":event_ids,"state":"interrupted"});
        }
        Ok(())
        }).map_err(state_error)?;
    }
    let completion = committed.unwrap_or_else(|| super::super::runtime_completion::Completion {
        turn_id,
        event_ids,
        status: "cancelled".into(),
        delivery_id: format!(
            "runtime:{}",
            runtime["active_turn_id"].as_str().unwrap_or_default()
        ),
    });
    // The turn projection precedes individual accepted receipts. A crash in
    // that interval must not let explicit resolution requeue its old inputs.
    for event_id in &completion.event_ids {
        if let Some((state, transport)) = super::super::runtime_delivery::latest_state(
            home,
            &group.group_id,
            &actor.id,
            event_id,
        )? && state == "claimed"
        {
            super::super::runtime_delivery::append_state(
                home,
                &group.group_id,
                &actor.id,
                &actor.created_at,
                event_id,
                &transport,
                super::super::runtime_delivery::DeliveryOutcome::Ambiguous(
                    "Dots unfinished turn was explicitly resolved by the user; its inputs must not be replayed",
                ),
            )?;
        }
    }
    let receipt =
        super::super::runtime_completion::append(home, &group.group_id, &actor.id, &completion)?;
    super::super::runtime_state::finish_completion(
        home,
        &group,
        &actor.id,
        &completion,
        receipt,
        request,
    )
}
