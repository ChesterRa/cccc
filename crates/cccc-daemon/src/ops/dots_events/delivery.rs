use super::*;

const MAX_ATTEMPTS: u64 = 3;
const SEND_LEASE_MS: i64 = 30_000;
const RETRY_DELAY_MS: i64 = 2_000;

pub(super) fn next(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let now = chrono::Utc::now().timestamp_millis();
    for connector in store::load(home).map_err(state_error)? {
        let dots = &connector["dots"];
        let (Some(group_id), Some(actor_id), Some(id)) = (
            dots["group_id"].as_str(),
            dots["actor_id"].as_str(),
            connector["connector_id"].as_str(),
        ) else {
            continue;
        };
        let Ok(group) = groups.load(group_id) else {
            continue;
        };
        let Some(actor) = group
            .actors
            .iter()
            .find(|a| a.id == actor_id && a.runtime == ActorRuntime::ChatgptDots)
        else {
            continue;
        };
        if !active(&group, actor)
            || binding(&connector, &group, actor)
                .is_none_or(|b| b["session_key"].as_str().is_none_or(str::is_empty))
            || !subscribed(&dots["subscription"], now)
        {
            continue;
        }
        let runtime = super::super::runtime_state::actor_state(home, group_id, actor_id)?;
        // Restart never replays the body of an accepted, unfinished turn.
        if runtime["active_turn_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
        {
            continue;
        }
        if super::super::runtime_delivery::pending_sources(home, &group, actor, 1)?.is_empty() {
            continue;
        }
        let result = store::dots_update(home, id, |c| {
            binding(c, &group, actor).ok_or_else(|| denied("dots_binding_changed"))?;
            let dots = &mut c["dots"];
            let state = dots["wake"]["state"].as_str().unwrap_or_default();
            if dots["execution"]["state"] == "active" || state == "delivered" || state == "failed" { return Ok(None); }
            if state == "sending" && dots["wake"]["sent_at_ms"].as_i64()
                .is_some_and(|sent| now.saturating_sub(sent) < SEND_LEASE_MS) { return Ok(None); }
            if state == "pending" && dots["wake"]["retry_at_ms"].as_i64().is_some_and(|retry| retry > now) { return Ok(None); }
            if !matches!(state, "pending" | "sending") {
                dots["wake"] = json!({"event_id":format!("evt_{}", uuid::Uuid::new_v4().simple()),
                    "epoch":dots["epoch"],"state":"pending","attempt":0,"created_at":cccc_contracts::utc_now()});
            }
            let attempts = dots["wake"]["attempt"].as_u64().unwrap_or(0);
            if attempts >= MAX_ATTEMPTS {
                dots["wake"]["state"] = json!("failed");
                dots["wake"]["error_code"] = json!("dots_delivery_unconfirmed");
                return Ok(None);
            }
            let attempt = attempts + 1;
            dots["wake"]["state"] = json!("sending");
            dots["wake"]["attempt"] = json!(attempt);
            dots["wake"]["sent_at_ms"] = json!(now);
            dots["wake"]["error_code"] = Value::Null;
            let event_id = dots["wake"]["event_id"].as_str().ok_or_else(|| denied("dots_event_unavailable"))?.to_owned();
            let dots = &c["dots"];
            let subscription = &dots["subscription"];
            let mut result = json!({"connector_id":id,"event_id":event_id,"epoch":dots["epoch"],"attempt":attempt,
                "subscription_id":subscription["subscription_id"],"callback_url":subscription["callback_url"],
                "signing_secret":subscription["signing_secret"],
                "event":{"eventId":event_id,"name":"cccc.dots.ready","timestamp":dots["wake"]["created_at"],
                    "data":{"event_id":event_id},"cursor":null}});
            if subscription["previous_secret_until_ms"].as_i64().is_some_and(|until| until > now) {
                result["previous_secret"] = subscription["previous_secret"].clone();
            }
            Ok(Some(result))
        }).map_err(state_error)?;
        if let Some(delivery) = result {
            return object(json!({"delivery":delivery}));
        }
    }
    object(json!({"delivery":null}))
}

pub(super) fn record(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    let id = required_arg(request, "connector_id")?;
    let event_id = required_arg(request, "event_id")?;
    let epoch = required_arg(request, "epoch")?;
    let attempt = request
        .args
        .get("attempt")
        .and_then(Value::as_u64)
        .filter(|attempt| *attempt > 0 && *attempt <= MAX_ATTEMPTS)
        .ok_or_else(|| OpError::new("invalid_args", "Invalid Dots delivery attempt"))?;
    let http = request
        .args
        .get("http_status")
        .and_then(Value::as_u64)
        .filter(|status| (100..=599).contains(status));
    let error = string_arg(request, "error_code");
    if http.is_none() && error.is_none() {
        return Err(OpError::new(
            "invalid_args",
            "A callback outcome is required",
        ));
    }
    let now = chrono::Utc::now().timestamp_millis();
    let recorded = store::dots_update(home, &id, |c| {
        let dots = &mut c["dots"];
        if dots["epoch"] != epoch
            || dots["wake"]["event_id"] != event_id
            || dots["wake"]["attempt"] != attempt
            || dots["wake"]["state"] != "sending"
        {
            return Ok(false);
        }
        let received = http.is_some_and(|status| (200..300).contains(&status));
        let wake = &mut dots["wake"];
        wake["http_status"] = json!(http);
        wake["updated_at_ms"] = json!(now);
        let permanent = matches!(http, Some(410 | 413));
        wake["state"] = json!(if received {
            "delivered"
        } else if !permanent && attempt < MAX_ATTEMPTS {
            "pending"
        } else {
            "failed"
        });
        wake["error_code"] = if received {
            Value::Null
        } else {
            json!(if http.is_some() {
                "dots_callback_rejected"
            } else {
                "dots_callback_failed"
            })
        };
        wake["retry_at_ms"] = if wake["state"] == "pending" {
            // Three attempts: wait 2s after the first, 4s after the second.
            json!(now + RETRY_DELAY_MS * (1_i64 << (attempt - 1)))
        } else {
            Value::Null
        };
        if http == Some(410) {
            dots["subscription"]["enabled"] = json!(false);
            dots["wake"]["error_code"] = json!("dots_subscription_gone");
        }
        Ok(true)
    })
    .map_err(state_error)?;
    object(json!({"recorded":recorded}))
}
