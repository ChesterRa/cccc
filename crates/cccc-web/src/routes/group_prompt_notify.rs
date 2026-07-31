use serde_json::{Value, json};

use crate::AppState;
use crate::api::{ApiError, call, object};

pub(super) async fn notify(
    state: &AppState,
    group_id: &str,
    content_changed: bool,
) -> Result<(Vec<String>, Vec<Value>), ApiError> {
    if !content_changed {
        return Ok((Vec::new(), Vec::new()));
    }
    let actors = match call(
        state,
        "actor_list",
        object(json!({
            "group_id":group_id,
            "include_internal":true,
            "by":"user"
        })),
    )
    .await
    {
        Ok(response) => response.0["result"]["actors"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        Err(error) => {
            return Ok((
                Vec::new(),
                vec![json!({"stage":"actor_list","error":error.to_string()})],
            ));
        }
    };
    let mut notified = Vec::new();
    let mut failures = Vec::new();
    for actor_id in actors.iter().filter_map(|actor| {
        actor["running"]
            .as_bool()
            .unwrap_or(false)
            .then(|| actor["id"].as_str().map(str::to_owned))
            .flatten()
    }) {
        let result = call(
            state,
            "system_notify",
            object(json!({
                "group_id":group_id,
                "by":"system",
                "kind":"info",
                "priority":"normal",
                "title":"Help updated",
                "message":"Group help changed. Run `cccc_help` now to refresh your effective protocol reference.",
                "target_actor_id":actor_id,
                "requires_ack":false
            })),
        )
        .await;
        match result {
            Ok(_) => notified.push(actor_id),
            Err(error) => failures.push(json!({"actor_id":actor_id,"error":error.to_string()})),
        }
    }
    notified.sort();
    Ok((notified, failures))
}

pub(super) fn annotate(mut value: Value, notified: Vec<String>, failures: Vec<Value>) -> Value {
    value["notified_actor_ids"] = json!(notified);
    value["notification_failures"] = Value::Array(failures);
    value
}
