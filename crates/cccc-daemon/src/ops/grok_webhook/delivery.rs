use super::*;
const MAX_ATTEMPTS: u64 = 3;

pub(super) fn next(home: &HomeLayout, r: &DaemonRequest) -> OpResult {
    user(r)?;
    let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let now = chrono::Utc::now().timestamp_millis();
    for c in store::load(home)
        .map_err(state_error)?
        .into_iter()
        .filter(|c| c["provider"] == "grok_web" && c["revoked"] != true)
    {
        for b in c["bindings"]
            .as_object()
            .into_iter()
            .flat_map(|b| b.values())
            .filter(|b| b["transport"] == "grok_webhook")
        {
            let (Some(gid), Some(aid), Some(id)) = (
                b["group_id"].as_str(),
                b["actor_id"].as_str(),
                c["connector_id"].as_str(),
            ) else {
                continue;
            };
            let Ok(g) = groups.load(gid) else {
                continue;
            };
            let Some(a) = g
                .actors
                .iter()
                .find(|a| a.id == aid && a.runtime == ActorRuntime::GrokWebModel)
            else {
                continue;
            };
            let s = private::state(&c, gid, aid);
            if matches!(
                s["wake"]["state"].as_str(),
                Some("delivered" | "failed" | "claimed")
            ) {
                continue;
            }
            if !active(&g, a)
                || store::binding_for_actor(
                    &c,
                    gid,
                    aid,
                    &cccc_core::actors::generation_identity(a),
                )
                .is_none()
                || super::super::runtime_state::actor_state(home, gid, aid)?["active_turn_id"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            {
                continue;
            }
            // Settle legacy unowned claims before considering a native wake.
            // This also completes bookkeeping interrupted after configuration.
            super::super::runtime_delivery::settle_replaced_transport_claims(
                home,
                &g,
                aid,
                "web_model_pull",
            )?;
            if super::super::runtime_delivery::pending_sources(home, &g, a, 1)?.is_empty() {
                continue;
            }
            let delivery = private::change(home, id, gid, aid, |c, s| {
                if !s["url"].is_string() || !s["key"].is_string() {
                    return Ok(None);
                }
                let state = s["wake"]["state"].as_str().unwrap_or("");
                if s["execution"]["state"] == "active"
                    || matches!(state, "delivered" | "failed" | "claimed")
                {
                    return Ok(None);
                }
                if state == "sending"
                    && s["wake"]["sent_at_ms"]
                        .as_i64()
                        .is_some_and(|t| now.saturating_sub(t) < 30_000)
                {
                    return Ok(None);
                }
                if state == "pending" && s["wake"]["retry_at_ms"].as_i64().is_some_and(|t| t > now)
                {
                    return Ok(None);
                }
                if !matches!(state, "pending" | "sending") {
                    s["wake"] = json!({
                        "event_id":format!("grok_evt_{}", uuid::Uuid::new_v4().simple()),
                        "state":"pending", "attempt":0
                    });
                }
                let attempt = s["wake"]["attempt"].as_u64().unwrap_or(0) + 1;
                if attempt > MAX_ATTEMPTS {
                    s["wake"]["state"] = json!("failed");
                    s["wake"]["error_code"] = json!("grok_delivery_unconfirmed");
                    return Ok(None);
                }
                s["wake"]["state"] = json!("sending");
                s["wake"]["attempt"] = json!(attempt);
                s["wake"]["sent_at_ms"] = json!(now);
                let event = s["wake"]["event_id"].as_str().unwrap_or("");
                Ok(Some(json!({
                    "connector_id":id, "group_id":gid, "actor_id":aid, "event_id":event,
                    "epoch":s["epoch"], "attempt":attempt, "url":s["url"], "key":s["key"],
                    "body": { "event":"cccc.grok.ready", "event_id":event,
                        "actor_token":private::token(c, b, s, "claim", event, "")? }
                })))
            })
            .map_err(state_error)?;
            if let Some(delivery) = delivery {
                return object(json!({"delivery":delivery}));
            }
        }
    }
    object(json!({"delivery":null}))
}

pub(super) fn record(home: &HomeLayout, r: &DaemonRequest) -> OpResult {
    user(r)?;
    let id = required_arg(r, "connector_id")?;
    let (g, a) = group_actor(home, r)?;
    let event = required_arg(r, "event_id")?;
    let epoch = required_arg(r, "epoch")?;
    let attempt = r
        .args
        .get("attempt")
        .and_then(Value::as_u64)
        .filter(|v| (1..=MAX_ATTEMPTS).contains(v))
        .ok_or_else(|| OpError::new("invalid_args", "Invalid attempt"))?;
    let http = r
        .args
        .get("http_status")
        .and_then(Value::as_u64)
        .filter(|v| (100..=599).contains(v));
    let recorded = private::change(home, &id, &g.group_id, &a.id, |_, s| {
        if s["epoch"] != epoch
            || s["wake"]["event_id"] != event
            || s["wake"]["attempt"] != attempt
            || s["wake"]["state"] != "sending"
        {
            return Ok(false);
        }
        let received = http == Some(200);
        // A duplicate wake can only claim once. Retry transient delivery failures
        // with that same event, never submit business text or switch to a browser.
        let retry = !received
            && (http.is_none() || http == Some(429) || http.is_some_and(|s| s >= 500))
            && attempt < MAX_ATTEMPTS;
        s["wake"]["state"] = json!(if received {
            "delivered"
        } else if retry {
            "pending"
        } else {
            "failed"
        });
        s["wake"]["http_status"] = json!(http);
        s["wake"]["error_code"] = if received {
            Value::Null
        } else {
            json!("grok_webhook_rejected")
        };
        s["wake"]["retry_at_ms"] =
            json!(chrono::Utc::now().timestamp_millis() + 2000 * (1_i64 << (attempt - 1)));
        Ok(true)
    })
    .map_err(state_error)?;
    object(json!({"recorded":recorded}))
}
