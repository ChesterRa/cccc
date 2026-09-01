use anyhow::{Result, anyhow, bail};
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

pub(super) fn validate_delegation(delegation: &ProviderDelegation) -> Result<()> {
    if delegation.id.is_empty() || delegation.id.len() > MAX_DELEGATION_ID_BYTES {
        bail!("Codex Voice delegation id is empty or oversized");
    }
    if delegation.text.is_empty() || delegation.text.len() > MAX_DELEGATION_TEXT_BYTES {
        bail!("Codex Voice delegation text is empty or oversized");
    }
    Ok(())
}
