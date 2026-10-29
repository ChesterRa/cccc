//! The interface's daemon reads, and how a failed request reads on screen.

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonError, DaemonRequest};
use serde_json::{Map, Value};

/// One read-only request; an error is the text to show for it.
pub(super) async fn read(
    client: &DaemonClient,
    op: &str,
    args: Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        })
        .await
        .map_err(|error| format!("daemon unavailable ({error})"))?;
    if response.ok {
        Ok(response.result)
    } else {
        Err(error_text(response.error))
    }
}

pub(super) fn error_text(error: Option<DaemonError>) -> String {
    error.map_or_else(
        || "unknown daemon error".into(),
        |error| format!("{}: {}", error.code, error.message),
    )
}
