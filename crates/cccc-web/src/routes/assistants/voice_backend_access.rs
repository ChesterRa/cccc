use serde_json::{Value, json};

use crate::api::ApiError;

pub(super) fn require_local_asr(assistant: &Value) -> Result<(), ApiError> {
    let backend = assistant["config"]["recognition_backend"]
        .as_str()
        .unwrap_or("browser_asr");
    if backend != "assistant_service_local_asr" {
        return Err(ApiError::bad_code(
            "assistant_voice_backend_mismatch",
            "voice transcription requires recognition_backend=assistant_service_local_asr",
            json!({"recognition_backend": backend}),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::require_local_asr;

    #[test]
    fn local_asr_remains_available_when_secretary_is_disabled() {
        let assistant = json!({
            "enabled": false,
            "config": {"recognition_backend": "assistant_service_local_asr"}
        });

        assert!(require_local_asr(&assistant).is_ok());
    }

    #[test]
    fn rejects_non_local_asr_backend() {
        let assistant = json!({
            "enabled": false,
            "config": {"recognition_backend": "browser_asr"}
        });

        let error = require_local_asr(&assistant).expect_err("browser backend must be rejected");
        assert!(
            error
                .to_string()
                .contains("assistant_voice_backend_mismatch")
        );
    }
}
