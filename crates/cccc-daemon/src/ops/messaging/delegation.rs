use cccc_contracts::{ActorRole, DaemonRequest};
use cccc_core::{GroupDoc, HomeLayout, actors};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub(super) fn relay(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let source = super::load(home, request)?;
    let destination_id = required_arg(request, "dst_group_id")?;
    if destination_id == source.group_id {
        return Err(OpError::new(
            "invalid_dst_group_id",
            "dst_group_id must be different from group_id",
        ));
    }
    let destination = store(home)?
        .load(&destination_id)
        .map_err(|_| OpError::new("group_not_found", "target group not found"))?;
    let sender =
        relay_sender(&source, string_arg(request, "relay_sender").as_deref()).ok_or_else(|| {
            OpError::new(
                "no_relay_agent",
                "no local agent is available to contact the target group",
            )
        })?;
    let target = target_actor(&destination, string_arg(request, "target_actor").as_deref())?;
    let delegation_id = format!("dlg_{}", &Uuid::new_v4().simple().to_string()[..16]);
    let text = render(
        &delegation_id,
        &source.group_id,
        &destination_id,
        &target,
        &string_arg(request, "text").unwrap_or_default(),
        string_arg(request, "contact_text").as_deref(),
    );
    let forwarded = DaemonRequest {
        v: request.v,
        op: "send_cross_group".into(),
        args: Map::from_iter([
            ("group_id".into(), json!(source.group_id)),
            ("dst_group_id".into(), json!(destination_id)),
            ("by".into(), json!(sender)),
            ("text".into(), json!(text)),
            ("to".into(), json!([target])),
            ("message_mode".into(), json!("send")),
        ]),
    };
    let sent = super::send_cross_group(home, &forwarded)?;
    object(json!({
        "relay":{
            "delegation_id":delegation_id,
            "sender":sender,
            "target_actor_id":target,
            "src_group_id":source.group_id,
            "dst_group_id":destination_id,
            "src_event_id":sent.get("src_event").and_then(|event|event.get("id")).and_then(Value::as_str).unwrap_or_default(),
            "dst_event_id":sent.get("dst_event").and_then(|event|event.get("id")).and_then(Value::as_str).unwrap_or_default(),
        }
    }))
}

fn relay_sender(group: &GroupDoc, preferred: Option<&str>) -> Option<String> {
    preferred
        .filter(|id| *id != "user")
        .and_then(|id| {
            actors::find(group, id)
                .filter(|actor| actor.internal_kind.is_none())
                .map(|actor| actor.id.clone())
        })
        .or_else(|| {
            actors::visible(group)
                .find(|actor| actors::effective_role(group, &actor.id) == Some(ActorRole::Foreman))
                .map(|actor| actor.id.clone())
        })
        .or_else(|| actors::visible(group).next().map(|actor| actor.id.clone()))
}

fn target_actor(group: &GroupDoc, requested: Option<&str>) -> Result<String, OpError> {
    if let Some(requested) = requested
        .map(str::trim)
        .map(|value| value.trim_start_matches('@'))
        .filter(|value| !value.is_empty())
    {
        if requested == "user" {
            return Err(OpError::new(
                "target_agent_unavailable",
                "requested target agent is unavailable",
            ));
        }
        return actors::find(group, requested)
            .ok_or_else(|| {
                OpError::new("target_agent_not_found", "requested target agent not found")
            })
            .and_then(|actor| {
                if actor.internal_kind.is_some() || !actor.enabled {
                    Err(OpError::new(
                        "target_agent_unavailable",
                        "requested target agent is unavailable",
                    ))
                } else {
                    Ok(actor.id.clone())
                }
            });
    }
    actors::visible(group)
        .find(|actor| {
            actor.enabled && actors::effective_role(group, &actor.id) == Some(ActorRole::Foreman)
        })
        .map(|actor| actor.id.clone())
        .ok_or_else(|| {
            OpError::new(
                "no_target_foreman",
                "the target group has no usable foreman",
            )
        })
}

fn render(
    delegation_id: &str,
    source_group_id: &str,
    target_group_id: &str,
    target_actor_id: &str,
    user_message: &str,
    contact_text: Option<&str>,
) -> String {
    let visible = contact_text
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| visible_intent(user_message));
    format!(
        "{visible}\n\n<!-- cccc-delegation-protocol\n\
[cccc-delegation:v1]\n\
delegation_id: {delegation_id}\n\
source_group_id: {source_group_id}\n\
target_group_id: {target_group_id}\n\
target_actor_id: {target_actor_id}\n\
source_contact: send back with cccc_message_send(dst_group_id={source_group_id}, text=..., include delegation_id)\n\
target_contact: reply as the addressed target, then send the substantive response/result back to source_group_id\n\n\
Communication protocol:\n\
You are the addressed target CCCC actor/foreman for a cross-group user request.\n\
Do not treat #tokens in the user message as recipients in your group.\n\
Interpret #group and @actor tokens as source-side routing context, not as words to repeat back.\n\
Respond to the user's intent as the addressed target. Do not merely confirm that the relay was received.\n\
If the user is greeting or asking to contact you, answer naturally as yourself.\n\
If the request needs work, either do the work or ask one concrete clarification.\n\
Talk to the source group by sending cross-group messages back with the same delegation_id; report done/refused/failed when appropriate.\n\n\
Original user message (reference only):\n{}\n\
[/cccc-delegation]\n-->",
        if user_message.trim().is_empty() {
            "(no message text)"
        } else {
            user_message.trim()
        }
    )
}

fn visible_intent(original: &str) -> String {
    let without_routes = strip_route_tokens(original);
    let mut visible = compact_visible(&without_routes);
    for prefix in ["请你", "麻烦你", "帮我", "帮忙", "你去", "去", "跟"] {
        if let Some(rest) = visible.strip_prefix(prefix) {
            visible = compact_visible(rest.trim_start());
            break;
        }
    }
    if visible.is_empty() {
        return "想先跟你打个招呼。".into();
    }
    if !visible.ends_with(['。', '！', '？', '!', '?']) {
        visible.push('。');
    }
    visible
}

fn strip_route_tokens(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut at_boundary = true;
    while let Some(character) = chars.next() {
        if at_boundary && matches!(character, '#' | '@') {
            while chars
                .peek()
                .is_some_and(|next| !next.is_whitespace() && !"，,。！？!?；;：:".contains(*next))
            {
                chars.next();
            }
            at_boundary = false;
            continue;
        }
        at_boundary = character.is_whitespace();
        result.push(character);
    }
    result
}

fn compact_visible(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.trim().chars() {
        if character == ' ' || character == '\t' {
            pending_space = !result.is_empty();
            continue;
        }
        if "，,。！？!?；;：:".contains(character) {
            while result.ends_with(' ') {
                result.pop();
            }
            result.push(character);
            pending_space = false;
            continue;
        }
        if pending_space {
            result.push(' ');
        }
        result.push(character);
        pending_space = false;
    }
    result.trim_matches([' ', '，', ',', '。']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegation_visible_text_matches_python_contract() {
        assert_eq!(
            visible_intent("#目标组 @foreman 请你 帮我看看"),
            "帮我看看。"
        );
        assert_eq!(visible_intent("#目标组 @foreman"), "想先跟你打个招呼。");
        assert_eq!(visible_intent("#目标组 你好！"), "你好！");
    }

    #[test]
    fn delegation_render_keeps_natural_body_and_full_protocol() {
        let rendered = render(
            "dlg_1",
            "source",
            "target",
            "foreman",
            "#target 请处理",
            None,
        );
        assert!(rendered.starts_with("请处理。"));
        assert!(
            rendered.contains("Interpret #group and @actor tokens as source-side routing context")
        );
        assert!(rendered.contains(
            "Talk to the source group by sending cross-group messages back with the same delegation_id"
        ));

        let overridden = render(
            "dlg_1",
            "source",
            "target",
            "foreman",
            "#target 请处理",
            Some("你好"),
        );
        assert!(overridden.starts_with("你好\n\n"));
    }
}
