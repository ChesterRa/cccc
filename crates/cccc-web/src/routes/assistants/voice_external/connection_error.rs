use super::{AsrError, Provider, connection::transport_error};
use tokio_tungstenite::tungstenite::Error;

pub(super) fn classify(provider: Provider, error: Error) -> AsrError {
    let Error::Http(response) = error else {
        return transport_error();
    };
    let status = response.status().as_u16();
    // Inspect only a bounded known field, and return fixed messages. Never echo
    // upstream bodies or headers, which may contain credentials or private data.
    let resource_denied = provider == Provider::Volcengine
        && status == 403
        && response
            .body()
            .as_ref()
            .filter(|body| body.len() <= 8192)
            .and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok())
            .and_then(|body| {
                body["error"]
                    .as_str()
                    .map(|message| message.ends_with("requested resource not granted"))
            })
            .unwrap_or(false);
    if resource_denied {
        return AsrError::new(
            "external_asr_resource_not_granted",
            "The selected ASR resource is not enabled for this account; select the model version and billing plan enabled in the speech console",
        );
    }
    match status {
        401 => AsrError::new(
            "external_asr_auth_failed",
            "ASR authentication failed; check the credential type and values",
        ),
        403 => AsrError::new(
            "external_asr_access_denied",
            "The ASR provider denied access; check credential permissions and the selected resource",
        ),
        429 => AsrError::new(
            "external_asr_rate_limited",
            "ASR quota or concurrency limit reached; check your speech service quota",
        ),
        _ => transport_error(),
    }
}
