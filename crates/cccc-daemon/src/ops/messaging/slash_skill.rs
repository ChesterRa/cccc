use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg};

const CONTROL_KIND: &str = "slash_skill_dispatch";

pub(super) struct Dispatch {
    pub request: DaemonRequest,
    command: String,
    capability_id: String,
}

pub(super) fn prepare(home: &HomeLayout, request: &DaemonRequest) -> Result<Dispatch, OpError> {
    let task_text = required_arg(request, "task_text")?.trim().to_owned();
    let command = required_arg(request, "command")?.trim().to_owned();
    let capability_id = required_arg(request, "capability_id")?.trim().to_owned();
    if !capability_id.starts_with("skill:") {
        return Err(OpError::new(
            "invalid_capability_id",
            "slash skill capability_id must start with skill:",
        ));
    }
    cccc_core::capabilities::CapabilityStore::new(home.clone())
        .require(&capability_id)
        .map_err(OpError::not_found)?;

    let mut forwarded = request.clone();
    for key in [
        "task_text",
        "command",
        "capability_id",
        "control_kind",
        "title",
        "hidden",
    ] {
        forwarded.args.remove(key);
    }
    forwarded.args.insert(
        "text".into(),
        Value::String(render_actor_turn(&task_text, &command, &capability_id)),
    );
    forwarded.args.insert(
        "refs".into(),
        json!([{
            "kind":"text",
            "title":CONTROL_KIND,
            "hidden":true,
            "control_kind":CONTROL_KIND,
            "command":command,
            "capability_id":capability_id,
            "task_text":task_text,
        }]),
    );
    forwarded.args.insert("attachments".into(), json!([]));

    Ok(Dispatch {
        request: forwarded,
        command,
        capability_id,
    })
}

pub(super) fn response(dispatch: &Dispatch, sent: &Map<String, Value>) -> OpResult {
    let event = sent
        .get("event")
        .and_then(Value::as_object)
        .ok_or_else(|| OpError::new("internal_error", "slash skill event is missing"))?;
    let event_id = event.get("id").and_then(Value::as_str).unwrap_or_default();
    let to = event
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("to"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut result = json!({
        "hidden":true,
        "delivered":true,
        "event_id":event_id,
        "command":dispatch.command,
        "capability_id":dispatch.capability_id,
        "to":to,
    })
    .as_object()
    .cloned()
    .expect("slash skill response object");
    if sent
        .get("duplicate")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        result.insert("replayed".into(), Value::Bool(true));
    }
    object(result)
}

fn render_actor_turn(task_text: &str, command: &str, capability_id: &str) -> String {
    [
        "[CCCC] INTERNAL CONTROL: CCCC capability skill dispatch".to_owned(),
        concat!(
            "Use the CCCC capability skill selected by the user. This is not a visible chat ",
            "message. Do not inspect the host runtime's local Codex/Claude/Gemini skill list ",
            "for this command."
        )
        .to_owned(),
        format!("skill_command: {command}"),
        format!("capability_id: {capability_id}"),
        concat!(
            "Procedure: run `cccc_help` first to refresh Active Skills (Runtime); if needed, ",
            "check `cccc_capability_state` to confirm active_capsule_skills; then execute the ",
            "user task per that CCCC skill's runtime rules."
        )
        .to_owned(),
        format!("User task:\n{task_text}"),
    ]
    .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::render_actor_turn;

    #[test]
    fn renders_python_compatible_actor_control_turn() {
        let rendered = render_actor_turn(
            "开始执行",
            "/using-superpowers",
            "skill:agent_self_proposed:using-superpowers",
        );
        assert!(rendered.starts_with("[CCCC] INTERNAL CONTROL"));
        assert!(rendered.contains("skill_command: /using-superpowers"));
        assert!(rendered.contains("capability_id: skill:agent_self_proposed:using-superpowers"));
        assert!(rendered.contains("run `cccc_help` first"));
        assert!(rendered.ends_with("User task:\n开始执行"));
    }
}
