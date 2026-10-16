use anyhow::{Result, anyhow, bail};
use cccc_contracts::codex_voice::VoiceProvider;
use serde_json::Value;

const MAX_DELEGATION_ID_BYTES: usize = 256;
const MAX_DELEGATION_TEXT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDelegation {
    pub id: String,
    pub text: String,
}

pub fn parse_provider_delegation(event: &Value) -> Result<Option<ProviderDelegation>> {
    if event.get("type").and_then(Value::as_str) != Some("delegation.created") {
        return Ok(None);
    }
    let item = event
        .get("item")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("Codex Voice delegation has no item"))?;
    if item.get("type").and_then(Value::as_str) != Some("delegation")
        || item.get("target").and_then(Value::as_str) != Some("client")
    {
        return Ok(None);
    }
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    let text = item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("input_text"))
        .map(|part| part.get("text").and_then(Value::as_str).unwrap_or_default())
        .collect::<String>()
        .trim()
        .to_owned();
    let delegation = ProviderDelegation { id, text };
    validate_delegation(&delegation)?;
    Ok(Some(delegation))
}

pub fn parse_provider_delegation_for(
    provider: VoiceProvider,
    event: &Value,
) -> Result<Option<ProviderDelegation>> {
    if provider == VoiceProvider::Codex {
        return parse_provider_delegation(event);
    }
    if event["type"] != "response.function_call_arguments.done" {
        return Ok(None);
    }
    if event["name"] != "cccc_investigate" {
        bail!("unsupported Voice tool");
    }
    let arguments: Value = serde_json::from_str(event["arguments"].as_str().unwrap_or_default())
        .map_err(|_| anyhow!("invalid Voice tool arguments"))?;
    if arguments
        .as_object()
        .is_none_or(|object| object.len() != 1 || !object.contains_key("request"))
    {
        bail!("Voice tool requires only a request");
    }
    let delegation = ProviderDelegation {
        // Grok's function_call_output must retain the provider's exact ID.
        id: event["call_id"].as_str().unwrap_or_default().to_owned(),
        text: arguments["request"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_owned(),
    };
    validate_delegation(&delegation)?;
    Ok(Some(delegation))
}

pub(super) fn validate_delegation(delegation: &ProviderDelegation) -> Result<()> {
    if delegation.id.is_empty() || delegation.id.len() > MAX_DELEGATION_ID_BYTES {
        bail!("Codex Voice delegation id is empty or oversized");
    }
    if delegation.text.is_empty() || delegation.text.len() > MAX_DELEGATION_TEXT_BYTES {
        bail!("Codex Voice delegation text is empty or oversized");
    }
    Ok(())
}

#[cfg(test)]
mod grok_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tools_are_strictly_scoped_to_the_calls_selected_provider() {
        let mut event = json!({"type":"response.function_call_arguments.done", "name":"cccc_investigate", "call_id":"function-1", "arguments":"{\"request\":\"Inspect the selected project\"}"});
        assert!(
            parse_provider_delegation_for(VoiceProvider::Codex, &event)
                .expect("Grok fixture")
                .is_none()
        );
        let parsed = parse_provider_delegation_for(VoiceProvider::Grok, &event)
            .expect("Grok fixture")
            .expect("Grok fixture");
        assert_eq!(parsed.id, "function-1");
        assert_eq!(parsed.text, "Inspect the selected project");
        event["arguments"] = json!("{\"request\":\"Inspect\",\"group_id\":\"another\"}");
        assert!(parse_provider_delegation_for(VoiceProvider::Grok, &event).is_err());
        event["arguments"] = json!("{\"request\":\"Inspect\"}");
        event["name"] = json!("shell");
        assert!(parse_provider_delegation_for(VoiceProvider::Grok, &event).is_err());
        assert!(
            parse_provider_delegation_for(
                VoiceProvider::Grok,
                &json!({"type":"delegation.created"})
            )
            .expect("Grok fixture")
            .is_none()
        );
    }
}
