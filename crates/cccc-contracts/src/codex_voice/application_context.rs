use serde::{Deserialize, Serialize};

// Local UTF-8 input size limit, not a provider token budget.
const MAX_INSTRUCTIONS_BYTES: usize = 24 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceCallMode {
    #[default]
    Assistant,
    Persona,
}

impl VoiceCallMode {
    fn is_assistant(&self) -> bool {
        *self == Self::Assistant
    }
}

/// Immutable, caller-supplied instructions for one embedded Voice call.
/// This is conversational context, never an identity or authorization grant.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ApplicationContextWire")]
pub struct VoiceApplicationContext {
    id: String,
    instructions: String,
    #[serde(skip_serializing_if = "VoiceCallMode::is_assistant")]
    mode: VoiceCallMode,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationContextWire {
    id: String,
    instructions: String,
    #[serde(default)]
    mode: VoiceCallMode,
}

impl TryFrom<ApplicationContextWire> for VoiceApplicationContext {
    type Error = &'static str;

    fn try_from(value: ApplicationContextWire) -> Result<Self, Self::Error> {
        Self::new_with_mode(value.id, value.instructions, value.mode)
    }
}

impl VoiceApplicationContext {
    pub fn new(id: String, instructions: String) -> Result<Self, &'static str> {
        Self::new_with_mode(id, instructions, VoiceCallMode::Assistant)
    }

    pub fn new_with_mode(
        id: String,
        instructions: String,
        mode: VoiceCallMode,
    ) -> Result<Self, &'static str> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        {
            return Err("application context id must be 1-128 ASCII identifier bytes");
        }
        if instructions.trim().is_empty()
            || instructions.len() > MAX_INSTRUCTIONS_BYTES
            || instructions
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        {
            return Err("application context instructions must be nonempty text up to 24576 bytes");
        }
        Ok(Self {
            id,
            instructions,
            mode,
        })
    }

    pub fn mode(&self) -> VoiceCallMode {
        self.mode
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
    fn mode_defaults_to_assistant_and_is_part_of_call_identity() {
        let legacy = json!({"id":"training", "instructions":"Act as a customer."});
        let context: VoiceApplicationContext =
            serde_json::from_value(legacy.clone()).expect("valid context");
        assert_eq!(context.mode(), VoiceCallMode::Assistant);
        assert_eq!(
            serde_json::to_value(&context).expect("valid context"),
            legacy
        );
        let mut explicit = legacy;
        explicit["mode"] = json!("assistant");
        assert_eq!(
            serde_json::from_value::<VoiceApplicationContext>(explicit.clone())
                .expect("valid context"),
            context
        );
        explicit["mode"] = json!("persona");
        let persona: VoiceApplicationContext =
            serde_json::from_value(explicit.clone()).expect("valid context");
        assert_ne!(persona, context);
        assert_eq!(
            serde_json::to_value(persona).expect("valid context"),
            explicit
        );
        for mode in [json!(null), json!("other"), json!(true)] {
            explicit["mode"] = mode;
            assert!(serde_json::from_value::<VoiceApplicationContext>(explicit.clone()).is_err());
        }
    }

    #[test]
    fn instructions_limit_counts_utf8_bytes_and_preserves_accepted_text_in_all_modes() {
        for mode in [None, Some("assistant"), Some("persona")] {
            for phrase in [
                "Act as a customer.\n",
                "你是一位顾客。\n",
                "あなたはお客様です。\n",
                "Customer 顾客 お客様 🙂\n",
            ] {
                for size in [24_575, 24_576, 24_577] {
                    let mut instructions = phrase.repeat(size / phrase.len());
                    instructions.push_str(&"x".repeat(size - instructions.len()));
                    let mut wire = json!({"id":"training", "instructions":instructions});
                    if let Some(mode) = mode {
                        wire["mode"] = json!(mode);
                    }
                    let result = serde_json::from_value::<VoiceApplicationContext>(wire);
                    if size <= 24_576 {
                        let context = result.expect("accept up to 24 KiB in every mode");
                        assert_eq!(context.instructions(), instructions);
                        assert_eq!(
                            serde_json::to_value(&context).expect("serialize context")["instructions"],
                            instructions
                        );
                        assert!(context.analyst_input("task").contains(&instructions));
                    } else {
                        let error = result.expect_err("reject one byte beyond the limit");
                        assert!(error.to_string().contains("24576 bytes"));
                        assert!(!error.to_string().contains(phrase.trim()));
                    }
                }
            }
        }
    }

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
            json!({"id":"work", "instructions":"あ".repeat(8193)}),
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
