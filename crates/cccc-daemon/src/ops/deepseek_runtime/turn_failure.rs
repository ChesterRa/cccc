use super::RuntimeEntry;
use cccc_runtime::deepseek_acp;
use cccc_runtime::deepseek_supervisor::{DeepSeekSupervisor, SupervisorError};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const CANCEL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn fail_sent_request(
    holder: &RuntimeEntry,
    supervisor: &mut DeepSeekSupervisor,
    session_id: &str,
    request_id: u64,
    terminal_seen: bool,
) -> bool {
    if !terminal_seen && cancel_and_confirm(supervisor, session_id, request_id).is_err() {
        holder.running.store(false, Ordering::Release);
        let _ = supervisor.stop();
    }
    false
}

fn cancel_and_confirm(
    supervisor: &mut DeepSeekSupervisor,
    session_id: &str,
    request_id: u64,
) -> Result<(), ()> {
    supervisor.cancel().map_err(|_| ())?;
    let deadline = Instant::now() + CANCEL_CONFIRM_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(());
        }
        let frame = match supervisor.next_frame(remaining.min(Duration::from_millis(200))) {
            Ok(frame) => frame,
            Err(SupervisorError::Timeout) => continue,
            Err(_) => return Err(()),
        };
        if frame.get("method") == Some(&Value::String("session/request_permission".into())) {
            let params = frame.get("params").and_then(Value::as_object).ok_or(())?;
            let permission_id =
                deepseek_acp::permission_request_id(&frame, session_id).map_err(|_| ())?;
            supervisor
                .respond_permission(
                    permission_id,
                    params.get("options").unwrap_or(&Value::Null),
                    true,
                )
                .map_err(|_| ())?;
            continue;
        }
        if frame.get("method") == Some(&Value::String("session/update".into())) {
            deepseek_acp::validate_session_update(&frame, session_id).map_err(|_| ())?;
            continue;
        }
        if frame.get("id") == Some(&json!(request_id)) {
            return Ok(());
        }
    }
}
