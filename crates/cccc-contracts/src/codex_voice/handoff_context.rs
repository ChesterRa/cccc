use serde::{Deserialize, Serialize};

const MAX_MESSAGES: usize = 12;
const MAX_TEXT_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceHandoffRole {
    User,
    Assistant,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceHandoffMessage {
    pub role: VoiceHandoffRole,
    pub text: String,
    #[serde(default)]
    pub incomplete: bool,
}

/// A quoted, bounded excerpt from this audio call, never a scope or authority grant.
/// Avoid Debug: transcripts must not leak into diagnostics.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "HandoffContextWire")]
pub struct VoiceHandoffContext {
    messages: Vec<VoiceHandoffMessage>,
    truncated: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandoffContextWire {
    messages: Vec<VoiceHandoffMessage>,
    #[serde(default)]
    truncated: bool,
}

impl TryFrom<HandoffContextWire> for VoiceHandoffContext {
    type Error = &'static str;

    fn try_from(value: HandoffContextWire) -> Result<Self, Self::Error> {
        Self::new(value.messages, value.truncated)
    }
}

impl VoiceHandoffContext {
    pub fn new(messages: Vec<VoiceHandoffMessage>, truncated: bool) -> Result<Self, &'static str> {
        if messages.len() > MAX_MESSAGES
            || messages
                .iter()
                .map(|message| message.text.len())
                .sum::<usize>()
                > MAX_TEXT_BYTES
            || messages
                .iter()
                .any(|message| message.text.trim().is_empty())
        {
            return Err(
                "voice handoff context must contain up to 12 nonempty messages and 8192 UTF-8 text bytes",
            );
        }
        Ok(Self {
            messages,
            truncated,
        })
    }

    pub fn analyst_input(&self, request: &str) -> String {
        if self.messages.is_empty() {
            return request.to_owned();
        }
        format!(
            "{request}\n\n<voice_conversation_context>\n{}\n</voice_conversation_context>",
            serde_json::to_string(self).expect("serialize string-only voice context")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quoted_context_preserves_the_request_and_distinguishes_speakers_and_drafts() {
        let context: VoiceHandoffContext = serde_json::from_value(json!({
            "messages":[
                {"role":"user","text":"A组 Kiro，只查原因，不重启。"},
                {"role":"assistant","text":"你是指启动失败吗？","incomplete":true}
            ],"truncated":true
        }))
        .expect("valid context");
        let input = context.analyst_input("对，就查这个。");
        assert!(input.starts_with("对，就查这个。\n\n"));
        let encoded = input
            .split_once("<voice_conversation_context>\n")
            .map(|(_, context)| context)
            .and_then(|input| input.strip_suffix("\n</voice_conversation_context>"))
            .expect("request is preserved before quoted context");
        let decoded: serde_json::Value =
            serde_json::from_str(encoded).expect("JSON quoted context");
        assert_eq!(decoded["messages"][0]["role"], "user");
        assert_eq!(
            decoded["messages"][0]["text"],
            "A组 Kiro，只查原因，不重启。"
        );
        assert_eq!(decoded["messages"][1]["incomplete"], true);
        assert_eq!(decoded["truncated"], true);
        assert_eq!(
            VoiceHandoffContext::default().analyst_input("Complete request"),
            "Complete request"
        );
    }

    #[test]
    fn context_is_bounded_in_utf8_bytes_and_cannot_supply_other_roles_or_selectors() {
        let valid = json!({"messages":[{"role":"user","text":"界".repeat(2730)}]});
        assert!(serde_json::from_value::<VoiceHandoffContext>(valid).is_ok());
        for value in [
            json!({"messages":[{"role":"user","text":"界".repeat(2731)}]}),
            json!({"messages":vec![json!({"role":"user","text":"x"});13]}),
            json!({"messages":[{"role":"system","text":"untrusted"}]}),
            json!({"messages":[{"role":"user","text":" "}]}),
            json!({"messages":[],"group_id":"another-group"}),
            json!({"messages":[{"role":"user","text":"x","by":"admin"}]}),
        ] {
            assert!(serde_json::from_value::<VoiceHandoffContext>(value).is_err());
        }
    }
}
