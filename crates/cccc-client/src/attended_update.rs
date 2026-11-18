//! Typed outcome of the final preservation check and shutdown request. A lost
//! response never becomes a refusal that lets the host lift its startup fence.

use crate::{ClientError, DaemonClient};
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::update_capabilities::{KeepDecision, PRESERVATION_REFUSED, UpdateCandidate};
use serde_json::json;

#[derive(Debug)]
pub enum PreservationStopReply {
    Requested { request_id: String },
    Refused(KeepDecision),
}

impl DaemonClient {
    pub async fn request_preserving_shutdown(
        &self,
        expected_pid: u32,
        request_id: &str,
        transaction: &str,
        candidate: &UpdateCandidate,
    ) -> Result<PreservationStopReply, ClientError> {
        let request = DaemonRequest {
            v: 1,
            op: "shutdown_preserving_agents".into(),
            args: json!({
                "expected_pid": expected_pid, "request_id": request_id,
                "update": {"transaction": transaction, "candidate": candidate}
            })
            .as_object()
            .expect("request object")
            .clone(),
        };
        let response = self.call(&request).await?;
        parse(response, request_id)
    }
}

fn parse(response: DaemonResponse, request_id: &str) -> Result<PreservationStopReply, ClientError> {
    if response.ok
        && response.result.get("request_id").and_then(|v| v.as_str()) == Some(request_id)
        && response.result.get("mode").and_then(|v| v.as_str()) == Some("preserve_agents")
        && response
            .result
            .get("shutting_down")
            .and_then(|v| v.as_bool())
            == Some(true)
    {
        return Ok(PreservationStopReply::Requested {
            request_id: request_id.into(),
        });
    }
    if !response.ok
        && let Some(error) = response.error
        && error.code == PRESERVATION_REFUSED
        && error.details.get("request_id").and_then(|v| v.as_str()) == Some(request_id)
        && error
            .details
            .get("shutdown_requested")
            .and_then(|v| v.as_bool())
            == Some(false)
        && let Some(value) = error.details.get("decision")
        && let Ok(decision) = serde_json::from_value::<KeepDecision>(value.clone())
        && !decision.allowed
        && !decision.refusals.is_empty()
    {
        return Ok(PreservationStopReply::Refused(decision));
    }
    Err(ClientError::OutcomeUnknown {
        op: "shutdown_preserving_agents".into(),
        message: "the daemon did not confirm this stop or a refusal before shutdown".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::update_capabilities::KeepRefusal;

    #[test]
    fn only_a_matching_explicit_refusal_allows_abandoning_the_update() {
        let decision = KeepDecision {
            allowed: false,
            refusals: vec![KeepRefusal {
                code: "mcp_not_observed".into(),
                group_id: None,
                actor_id: None,
                owner_id: None,
                display_name: None,
                message: "not observed".into(),
            }],
        };
        let mut refusal = DaemonResponse::failure(PRESERVATION_REFUSED, "cannot preserve");
        refusal.error.as_mut().expect("error").details = json!({
            "request_id":"this-stop", "shutdown_requested":false, "decision":decision
        })
        .as_object()
        .expect("details")
        .clone();
        assert!(matches!(
            parse(refusal.clone(), "this-stop"),
            Ok(PreservationStopReply::Refused(_))
        ));
        assert!(matches!(
            parse(refusal.clone(), "another-stop"),
            Err(ClientError::OutcomeUnknown { .. })
        ));
        refusal
            .error
            .as_mut()
            .expect("error")
            .details
            .insert("shutdown_requested".into(), json!(true));
        assert!(matches!(
            parse(refusal, "this-stop"),
            Err(ClientError::OutcomeUnknown { .. })
        ));
        assert!(matches!(
            parse(
                DaemonResponse::failure("shutdown_in_progress", "may already be stopping"),
                "this-stop"
            ),
            Err(ClientError::OutcomeUnknown { .. })
        ));
    }

    #[test]
    fn a_shutdown_acknowledgment_must_match_both_request_and_mode() {
        let ack = DaemonResponse::success(
            json!({
                "request_id":"this-stop", "mode":"preserve_agents", "shutting_down":true
            })
            .as_object()
            .expect("result")
            .clone(),
        );
        assert!(matches!(
            parse(ack.clone(), "this-stop"),
            Ok(PreservationStopReply::Requested { .. })
        ));
        assert!(matches!(
            parse(ack, "another-stop"),
            Err(ClientError::OutcomeUnknown { .. })
        ));
        let stop = DaemonResponse::success(
            json!({
                "request_id":"this-stop", "mode":"stop", "shutting_down":true
            })
            .as_object()
            .expect("result")
            .clone(),
        );
        assert!(matches!(
            parse(stop, "this-stop"),
            Err(ClientError::OutcomeUnknown { .. })
        ));
    }
}
