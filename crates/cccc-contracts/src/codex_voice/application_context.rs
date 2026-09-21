use serde::{Deserialize, Serialize};

/// Immutable, caller-supplied instructions for one embedded Voice call.
/// This is conversational context, never an identity or authorization grant.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ApplicationContextWire")]
pub struct VoiceApplicationContext {
    id: String,
    instructions: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationContextWire {
    id: String,
    instructions: String,
}

impl TryFrom<ApplicationContextWire> for VoiceApplicationContext {
    type Error = &'static str;

    fn try_from(value: ApplicationContextWire) -> Result<Self, Self::Error> {
        Self::new(value.id, value.instructions)
    }
}

impl VoiceApplicationContext {
    pub fn new(id: String, instructions: String) -> Result<Self, &'static str> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        {
            return Err("application context id must be 1-128 ASCII identifier bytes");
        }
        if instructions.trim().is_empty()
            || instructions.len() > 8192
            || instructions
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        {
            return Err("application context instructions must be nonempty text up to 8192 bytes");
        }
        Ok(Self { id, instructions })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    pub fn analyst_input(&self, request: &str) -> String {
        format!(
            "# Host application context for this call\nContext ID: {}\n{}\n\n# Voice request\n{}",
            self.id, self.instructions, request
        )
    }
}

impl std::fmt::Debug for VoiceApplicationContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VoiceApplicationContext")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn context_is_bounded_and_does_not_leak_through_debug_or_errors() {
        let context = VoiceApplicationContext::new("work:123".into(), "日本語で応答する。".into())
            .expect("valid Japanese context");
        assert_eq!(
            serde_json::from_value::<VoiceApplicationContext>(
                serde_json::to_value(&context).expect("serialize context")
            )
            .expect("deserialize context"),
            context
        );
        assert!(!format!("{context:?}").contains("work:123"));
        for invalid in [
            json!({"id":"../secret", "instructions":"private"}),
            json!({"id":"work", "instructions":" "}),
            json!({"id":"work", "instructions":"secret\u{0}"}),
            json!({"id":"work", "instructions":"あ".repeat(2731)}),
            json!({"id":"work", "instructions":"private", "extra":true}),
        ] {
            let error = serde_json::from_value::<VoiceApplicationContext>(invalid)
                .expect_err("reject invalid context");
            assert!(!error.to_string().contains("private"));
            assert!(!error.to_string().contains("secret"));
        }
        assert!(
            context
                .analyst_input("単価を比較してください")
                .ends_with("# Voice request\n単価を比較してください")
        );
    }
}
