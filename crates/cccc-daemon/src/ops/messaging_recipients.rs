use cccc_contracts::Event;
use cccc_core::{GroupDoc, actors, inbox};
use serde_json::{Map, Value, json};

use crate::dispatch::OpError;

pub(super) fn normalize_chat_data(
    group: &GroupDoc,
    by: &str,
    data: &mut Map<String, Value>,
) -> Result<(), OpError> {
    let text = data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let has_files = data
        .get("attachments")
        .and_then(Value::as_array)
        .is_some_and(|files| !files.is_empty());
    if text.is_empty() && !has_files {
        return Err(OpError::new(
            "invalid_args",
            "text or attachments is required",
        ));
    }

    let raw = data
        .get("to")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut recipients = actors::resolve_recipients(group, &raw).map_err(OpError::invalid)?;
    if recipients.is_empty() && raw.is_empty() {
        recipients.push(default_local_recipient(group, by).into());
    }
    reject_sender_only_audience(group, by, &recipients)?;
    data.insert("to".into(), json!(recipients));
    normalize_peer_insight(group, by, data)?;
    data.entry("format")
        .or_insert_with(|| Value::String("plain".into()));
    data.entry("priority")
        .or_insert_with(|| Value::String("normal".into()));
    data.entry("reply_required").or_insert(Value::Bool(false));
    super::message_metadata::add_sender_snapshot(group, by, data);
    Ok(())
}

pub(super) fn apply_cross_group_recipient(
    group: &GroupDoc,
    data: &mut Map<String, Value>,
) -> Result<(), OpError> {
    if !data.contains_key("to") {
        data.insert(
            "to".into(),
            json!([cccc_core::actors::CROSS_GROUP_FOREMAN_RECIPIENT]),
        );
    }
    let Some(items) = data.get("to").and_then(Value::as_array) else {
        return Err(OpError::new(
            "invalid_recipient",
            "cross-group to must be a non-empty string array",
        ));
    };
    if items.is_empty()
        || items
            .iter()
            .any(|item| item.as_str().is_none_or(|value| value.trim().is_empty()))
    {
        return Err(OpError::new(
            "invalid_recipient",
            "cross-group to must be a non-empty string array",
        ));
    }
    let requested = items.len() == 1
        && items[0].as_str() == Some(cccc_core::actors::CROSS_GROUP_FOREMAN_RECIPIENT);
    if !requested {
        return Ok(());
    }
    let actor =
        cccc_core::actors::unique_available_foreman(group).map_err(|error| match error {
            cccc_core::actors::UniqueForemanError::NotFound => {
                OpError::new("foreman_not_found", "target group has no available foreman")
            }
            cccc_core::actors::UniqueForemanError::NotUnique => OpError::new(
                "foreman_not_unique",
                "target group has more than one available foreman",
            ),
        })?;
    data.insert("to".into(), json!([actor.id]));
    Ok(())
}

fn default_local_recipient(group: &GroupDoc, by: &str) -> &'static str {
    if actors::visible(group).any(|actor| actor.id == by) {
        "user"
    } else {
        default_recipient(group)
    }
}

fn reject_sender_only_audience(
    group: &GroupDoc,
    by: &str,
    recipients: &[String],
) -> Result<(), OpError> {
    let local_sender = by == "user" || actors::visible(group).any(|actor| actor.id == by);
    if !local_sender {
        return Ok(());
    }
    if !recipients
        .iter()
        .any(|recipient| !matches!(recipient.as_str(), "user" | "@user"))
    {
        return Ok(());
    }

    let mut event = Event::new("chat.message", &group.group_id);
    event.data.insert("to".into(), json!(recipients));
    if actors::visible(group)
        .filter(|actor| actor.id != by)
        .any(|actor| inbox::is_for_actor(group, &event, &actor.id))
    {
        return Ok(());
    }

    let mut error = OpError::new(
        "no_enabled_recipients",
        "No recipients remain after excluding the sender; reply to the original sender or another recipient.",
    );
    error.details.insert("to".into(), json!(recipients));
    Err(error)
}

pub(super) fn normalize_remote_chat_data(data: &mut Map<String, Value>) -> Result<(), OpError> {
    let required = data
        .remove("require_peer_insight")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    normalize_insight(data)?;
    let peer_facing = data
        .get("to")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|recipient| !matches!(recipient.trim(), "" | "user" | "@user"));
    require_peer_insight(required, peer_facing, data)
}

fn normalize_peer_insight(
    group: &GroupDoc,
    by: &str,
    data: &mut Map<String, Value>,
) -> Result<(), OpError> {
    let required = data
        .remove("require_peer_insight")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    normalize_insight(data)?;
    require_peer_insight(required, peer_facing(group, by, data), data)
}

fn normalize_insight(data: &mut Map<String, Value>) -> Result<(), OpError> {
    let insight = cccc_core::peer_insight::normalize(data.get("insight"))
        .map_err(|message| OpError::new("invalid_insight", message))?;
    match insight {
        Some(insight) => {
            data.insert("insight".into(), Value::String(insight));
        }
        None => {
            data.remove("insight");
        }
    }
    Ok(())
}

fn require_peer_insight(
    required: bool,
    peer_facing: bool,
    data: &Map<String, Value>,
) -> Result<(), OpError> {
    if required && peer_facing && !data.contains_key("insight") {
        let mut error = OpError::new(
            "peer_insight_required",
            "Not sent: this peer-facing message is missing `insight`.",
        );
        error
            .details
            .insert("delivery_state".into(), Value::String("not_sent".into()));
        error
            .details
            .insert("new_side_effects".into(), Value::Bool(false));
        error.details.insert(
            "recommended_action".into(),
            Value::String(cccc_core::peer_insight::PEER_INSIGHT_REQUIRED_ACTION.clone()),
        );
        return Err(error);
    }
    Ok(())
}

fn peer_facing(group: &GroupDoc, by: &str, data: &Map<String, Value>) -> bool {
    let recipients = data
        .get("to")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    recipients.into_iter().any(|recipient| match recipient {
        "user" | "@user" => false,
        "@all" | "@peers" => group
            .actors
            .iter()
            .any(|actor| actor.internal_kind.is_none() && actor.id != by),
        "@foreman" => cccc_core::actors::visible(group)
            .next()
            .is_some_and(|actor| actor.id != by),
        actor_id => group
            .actors
            .iter()
            .any(|actor| actor.internal_kind.is_none() && actor.id == actor_id && actor.id != by),
    })
}

pub(super) fn default_recipient(group: &GroupDoc) -> &'static str {
    let configured = group
        .extra
        .get("settings")
        .and_then(Value::as_object)
        .and_then(|settings| settings.get("default_send_to"))
        .or_else(|| {
            group
                .extra
                .get("messaging")
                .and_then(Value::as_object)
                .and_then(|settings| settings.get("default_send_to"))
        })
        .and_then(Value::as_str);
    if configured == Some("broadcast") {
        "@all"
    } else {
        "@foreman"
    }
}
