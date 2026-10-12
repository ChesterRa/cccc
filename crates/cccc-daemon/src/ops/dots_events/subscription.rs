use super::*;

const ROTATION_WINDOW_MS: i64 = 5 * 60 * 1000;

fn authorized<'a>(connector: &'a Value, request: &DaemonRequest) -> io::Result<&'a Value> {
    let token = request
        .args
        .get("subscription_token")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let route = store::dots_subscription_binding(connector, token)
        .ok_or_else(|| denied("dots_subscription_invalid"))?;
    let dots = &connector["dots"];
    if route["group_id"] != request.args.get("group_id").cloned().unwrap_or_default()
        || route["actor_id"] != request.args.get("actor_id").cloned().unwrap_or_default()
        || route["revision"]
            != request
                .args
                .get("expected_revision")
                .cloned()
                .unwrap_or_default()
        || dots["epoch"]
            != request
                .args
                .get("expected_epoch")
                .cloned()
                .unwrap_or_default()
    {
        return Err(denied("dots_subscription_changed"));
    }
    Ok(dots)
}

pub(super) fn subscribe(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    group_actor(home, request)?;
    let id = required_arg(request, "connector_id")?;
    let callback = required_arg(request, "callback_url")?;
    let secret = required_arg(request, "signing_secret")?;
    let subscription_id = required_arg(request, "subscription_id")?;
    // Public URL and signed challenge verification belong to the Web port.
    if !callback.starts_with("https://") || subscription_id.len() > 160 || secret.len() > 1024 {
        return Err(OpError::new(
            "invalid_args",
            "Invalid verified Dots subscription",
        ));
    }
    let now = chrono::Utc::now().timestamp_millis();
    let verified_at = match request.args.get("verified_at_ms") {
        None => now,
        Some(value) => value
            .as_i64()
            .filter(|verified| *verified > 0 && *verified <= now)
            .ok_or_else(|| {
                OpError::new("invalid_args", "Invalid Dots callback verification time")
            })?,
    };
    let expiry = match request.args.get("ttl_ms") {
        None | Some(Value::Null) => None,
        Some(value) => value
            .as_u64()
            .filter(|ttl| *ttl > 0)
            .and_then(|ttl| i64::try_from(ttl).ok())
            .and_then(|ttl| now.checked_add(ttl))
            .map(Some)
            .ok_or_else(|| OpError::new("invalid_args", "ttl_ms must be positive or null"))?,
    };
    store::dots_update(home, &id, |c| {
        authorized(c, request)?;
        let old = c["dots"]["subscription"].clone();
        let same = old["subscription_id"] == subscription_id && old["callback_url"] == callback;
        let rotated = same && old["signing_secret"].as_str().is_some_and(|s| s != secret);
        let mut subscription = json!({"subscription_id":subscription_id,"callback_url":callback,
            "signing_secret":secret,"enabled":true,"expires_at_ms":expiry,"updated_at_ms":now,"verified_at_ms":verified_at});
        if rotated {
            subscription["previous_secret"] = old["signing_secret"].clone();
            subscription["previous_secret_until_ms"] = json!(now + ROTATION_WINDOW_MS);
        } else if same
            && old["previous_secret_until_ms"]
                .as_i64()
                .is_some_and(|end| end > now)
        {
            subscription["previous_secret"] = old["previous_secret"].clone();
            subscription["previous_secret_until_ms"] = old["previous_secret_until_ms"].clone();
        }
        if !same && old.is_object() {
            c["dots"]["epoch"] = json!(uuid::Uuid::new_v4().to_string());
            for field in ["wake", "execution"] {
                if c["dots"][field].is_object() {
                    c["dots"][field]["state"] = json!("interrupted");
                }
            }
        }
        c["dots"]["subscription"] = subscription;
        Ok(())
    })
    .map_err(state_error)?;
    object(json!({"subscription_id":subscription_id,"subscribed":true,"expires_at_ms":expiry}))
}

pub(super) fn unsubscribe(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    user(request)?;
    group_actor(home, request)?;
    let id = required_arg(request, "connector_id")?;
    let callback = required_arg(request, "callback_url")?;
    let removed = store::dots_update(home, &id, |c| {
        authorized(c, request)?;
        if c["dots"]["subscription"]["callback_url"] != callback
            || c["dots"]["subscription"]["enabled"] != true
        {
            return Ok(false);
        }
        if c["dots"]["subscription"].is_object() {
            c["dots"]["subscription"]["enabled"] = json!(false);
        }
        c["dots"]["epoch"] = json!(uuid::Uuid::new_v4().to_string());
        for field in ["wake", "execution"] {
            if c["dots"][field].is_object() {
                c["dots"][field]["state"] = json!("interrupted");
            }
        }
        Ok(true)
    })
    .map_err(state_error)?;
    object(json!({"unsubscribed":true,"removed":removed}))
}
