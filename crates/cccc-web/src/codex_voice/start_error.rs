use anyhow::Error;
use cccc_core::voice_recording_lease::LeaseError;
use cccc_daemon::experimental_codex_voice::RealtimeCallError;
use serde::Serialize;
use std::io;

use crate::api::ApiError;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartStage {
    Configuration,
    Analyst,
    Recording,
    Realtime,
}

impl std::fmt::Display for StartStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Codex Voice {self:?} startup failed")
    }
}

/// Only known categories and numbers may cross the diagnostic boundary.
/// Never serialize the underlying error, paths, SDP, auth or provider body.
#[derive(Debug, Serialize)]
pub(crate) struct StartDiagnostic {
    code: &'static str,
    stage: StartStage,
    elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    os_error: Option<i32>,
}

impl StartDiagnostic {
    pub(crate) fn from_error(error: &Error, elapsed_ms: u64) -> Self {
        let mut stage = error
            .downcast_ref::<StartStage>()
            .copied()
            .unwrap_or(StartStage::Configuration);
        let io_error = error.downcast_ref::<io::Error>();
        let request = error.downcast_ref::<reqwest::Error>();
        let realtime = error.downcast_ref::<RealtimeCallError>();
        let http_status = match realtime {
            Some(RealtimeCallError::HttpStatus(status)) => Some(*status),
            _ => None,
        };
        let code = if let Some(lease) = error.downcast_ref::<LeaseError>() {
            stage = StartStage::Recording;
            if lease.code == "assistant_voice_recording_busy" {
                "codex_voice_recording_busy"
            } else {
                "codex_voice_recording_failed"
            }
        } else {
            match stage {
                StartStage::Configuration => "codex_voice_setup_failed",
                StartStage::Analyst => {
                    if io_error.is_some_and(|error| error.kind() == io::ErrorKind::TimedOut) {
                        "codex_voice_analyst_start_timeout"
                    } else {
                        "codex_voice_analyst_start_failed"
                    }
                }
                StartStage::Recording => "codex_voice_recording_failed",
                StartStage::Realtime => match realtime {
                    Some(RealtimeCallError::Credentials | RealtimeCallError::HttpStatus(401)) => {
                        "codex_voice_realtime_auth_failed"
                    }
                    Some(RealtimeCallError::HttpStatus(429)) => "codex_voice_realtime_rate_limited",
                    Some(RealtimeCallError::HttpStatus(_)) => "codex_voice_realtime_rejected",
                    None if request.is_some_and(reqwest::Error::is_timeout) => {
                        "codex_voice_realtime_timeout"
                    }
                    None if request.is_some() => "codex_voice_realtime_connection_failed",
                    None => "codex_voice_realtime_failed",
                },
            }
        };
        Self {
            code,
            stage,
            elapsed_ms,
            http_status,
            os_error: io_error.and_then(io::Error::raw_os_error),
        }
    }

    pub(crate) fn details(&self) -> serde_json::Value {
        serde_json::json!(self)
    }

    pub(crate) fn into_api_error(self) -> ApiError {
        let message = match self.code {
            "codex_voice_setup_failed" => {
                "Codex Voice configuration could not be loaded. Check Voice settings."
            }
            "codex_voice_analyst_start_timeout" => {
                "The Voice Analyst timed out while starting. Check its Runtime and try again."
            }
            "codex_voice_analyst_start_failed" => {
                "The Voice Analyst could not start. Check its Runtime Profile."
            }
            "codex_voice_recording_busy" => {
                "Another voice session still holds recording access. Stop it or wait briefly, then try again."
            }
            "codex_voice_recording_failed" => {
                "CCCC could not acquire or keep recording access. Try starting voice again."
            }
            "codex_voice_realtime_auth_failed" => {
                "Realtime Voice could not use the ChatGPT login. Check its login; the Analyst session is retained."
            }
            "codex_voice_realtime_timeout" => {
                "Connecting to Realtime Voice timed out. Try again; the Analyst session is retained."
            }
            "codex_voice_realtime_connection_failed" => {
                "CCCC could not communicate with Realtime Voice. Check the network or proxy; the Analyst session is retained."
            }
            "codex_voice_realtime_rate_limited" => {
                "Realtime Voice returned a usage or rate limit. Check account limits or try later; the Analyst session is retained."
            }
            "codex_voice_realtime_rejected" => {
                "Realtime Voice rejected this connection. Try again later; the Analyst session is retained."
            }
            _ => {
                "Realtime Voice returned an unusable connection response. Try again; the Analyst session is retained."
            }
        };
        ApiError::unavailable(self.code, message).with_details(self.details())
    }
}

#[cfg(test)]
#[path = "start_error_tests.rs"]
mod tests;
