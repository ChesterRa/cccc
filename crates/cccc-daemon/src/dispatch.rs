use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::io;

use crate::ops;
use crate::ops::operation::{Operation, Policy};

pub type OpResult = Result<Map<String, Value>, OpError>;

pub fn dispatch(home: &HomeLayout, request: &DaemonRequest) -> DaemonResponse {
    response(dispatch_result(home, request))
}

pub(crate) fn response(result: OpResult) -> DaemonResponse {
    match result {
        Ok(result) => DaemonResponse::success(result),
        Err(error) => {
            let mut response = DaemonResponse::failure(error.code, error.message);
            if let Some(body) = response.error.as_mut() {
                body.details = error.details;
            }
            response
        }
    }
}

fn dispatch_result(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    resolve_operation(request)
        .ok_or_else(|| OpError::new("unknown_op", format!("unknown operation: {}", request.op)))?
        .execute(home, request)
}

pub(crate) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "ping" => Operation::new(Policy::Read, ping),
        "version" => Operation::new(Policy::Read, |_home, _request| {
            object(
                json!({"version": env!("CARGO_PKG_VERSION"), "implementation": "rust", "compatibility": cccc_contracts::RUST_DAEMON_COMPATIBILITY}),
            )
        }),
        "home_get" => Operation::new(Policy::Read, |home, _request| {
            object(json!({"home": home.root(), "environment": "CCCC_HOME"}))
        }),
        "shutdown" | "shutdown_preserving_agents" => Operation::new(Policy::GlobalWrite, shutdown),
        "browser_resources" => Operation::new(
            Policy::Read,
            crate::session_owner::browser_operations::handle,
        ),
        "browser_resource_ensure" | "browser_resource_pages_set" | "browser_resource_close" => {
            Operation::new(
                Policy::GlobalWrite,
                crate::session_owner::browser_operations::handle,
            )
        }
        _ => return ops::resolve_operation(request),
    })
}

fn ping(home: &HomeLayout, _request: &DaemonRequest) -> OpResult {
    let mut result = object(json!({
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "build": cccc_core::build_info::current(),
        "executable": std::env::current_exe().ok(),
        "ts": cccc_contracts::utc_now(),
        "ipc_v": 1,
        "capabilities": {
            "events_stream": true,
            "remote_access": true,
            "presentation_browser_attach": false,
            "presentation_browser_vnc_attach": false,
            "space_provider_auth_browser_attach": false,
            "space_provider_auth_browser_vnc_attach": false,
            "web_model_browser_attach": false,
            "web_model_browser_vnc_attach": false,
            "term_attachment_status": true,
            "term_attach_snapshot_v1": true,
            "assistant_state": true,
            "assistant_voice_recording_lease": true,
            "assistant_voice_model_install": false,
        },
        "implementation": "rust",
        "compatibility": cccc_contracts::RUST_DAEMON_COMPATIBILITY,
    }))?;
    if let Some(agents) = crate::session_owner::startup_retained(home) {
        result.insert("retained_agents".into(), json!(agents));
    }
    Ok(result)
}

fn shutdown(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if let Some(value) = request.args.get("expected_pid") {
        let expected_pid = value.as_u64().filter(|pid| *pid > 0).ok_or_else(|| {
            OpError::new("invalid_args", "expected_pid must be a positive integer")
        })?;
        let current_pid = u64::from(std::process::id());
        if expected_pid != current_pid {
            return Err(OpError::new(
                "daemon_owner_mismatch",
                format!(
                    "shutdown expected daemon pid {expected_pid}, but connected daemon pid is {current_pid}"
                ),
            ));
        }
    }
    let mode = if request.op == "shutdown_preserving_agents" {
        if !crate::session_owner::enabled(home) {
            return Err(OpError::new(
                "unsupported_platform",
                "this daemon cannot preserve independent agent sessions",
            ));
        }
        crate::session_owner::ShutdownMode::PreserveAgents
    } else {
        crate::session_owner::ShutdownMode::Stop
    };
    let request_id = match request.args.get("request_id") {
        Some(Value::String(value)) => {
            uuid::Uuid::parse_str(value)
                .map_err(|_| OpError::new("invalid_args", "request_id must be a UUID"))?;
            value.clone()
        }
        None => uuid::Uuid::new_v4().to_string(),
        _ => return Err(OpError::new("invalid_args", "request_id must be a UUID")),
    };
    if let Some(update) = request.args.get("update") {
        if mode != crate::session_owner::ShutdownMode::PreserveAgents {
            return Err(OpError::new(
                "invalid_args",
                "update preservation requires shutdown_preserving_agents",
            ));
        }
        return shutdown_update(home, update, request_id);
    }
    let selected = crate::session_owner::select_shutdown(home, mode, request_id)
        .map_err(|error| OpError::new("shutdown_in_progress", error.to_string()))?;
    object(json!({"shutting_down": true,"request_id":selected.request_id,"mode":selected.mode}))
}

fn shutdown_update(home: &HomeLayout, value: &Value, request_id: String) -> OpResult {
    use cccc_core::update_capabilities::{
        KeepDecision, KeepRefusal, PRESERVATION_REFUSED, UpdateCandidate,
    };
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Update {
        transaction: String,
        candidate: UpdateCandidate,
    }
    let update: Update = serde_json::from_value(value.clone()).map_err(OpError::invalid)?;
    if crate::session_owner::selected_shutdown(home)
        .map_err(OpError::io)?
        .is_some()
    {
        return Err(OpError::new(
            "shutdown_in_progress",
            "a daemon shutdown has already been selected",
        ));
    }
    let intent = cccc_core::attended_update::active(home)
        .map_err(OpError::io)?
        .ok_or_else(|| OpError::new("update_resume_mismatch", "there is no active update"))?;
    if intent.transaction_id != update.transaction
        || intent.target != update.candidate.build
        || cccc_core::local_sessions::NativeHost::capture(std::process::id())
            .map_err(OpError::io)?
            != intent.holder
    {
        return Err(OpError::new(
            "update_resume_mismatch",
            "this daemon is not the host of the active update",
        ));
    }
    let refusal = |decision: KeepDecision| {
        let mut error = OpError::new(
            PRESERVATION_REFUSED,
            "keeping the members running was refused before requesting shutdown",
        );
        error.details =
            json!({"request_id":request_id,"shutdown_requested":false,"decision":decision})
                .as_object()
                .expect("refusal details")
                .clone();
        error
    };
    let reason = |code: &str, message: &str| KeepDecision {
        allowed: false,
        refusals: vec![KeepRefusal {
            code: code.into(),
            group_id: None,
            actor_id: None,
            owner_id: None,
            display_name: None,
            message: message.into(),
        }],
    };
    let permit = crate::runtime_start_gate::try_permit(home)
        .map_err(|message| OpError::new("shutdown_in_progress", message))?
        .ok_or_else(|| {
            refusal(reason(
                "member_state_unconfirmed",
                "A member is starting; wait for it to settle and try again.",
            ))
        })?;
    let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
        OpError::new(
            "update_preflight_unavailable",
            "the daemon observation runtime is unavailable",
        )
    })?;
    let decision = runtime.block_on(crate::session_owner::preservation_check(
        home,
        &update.candidate.capabilities,
    ));
    if !decision.allowed {
        return Err(refusal(decision));
    }
    if let cccc_core::attended_update::Source::Local {
        program,
        file_identity,
    } = &intent.source
        && cccc_core::update_image::FileIdentity::read(program)
            .ok()
            .as_ref()
            != Some(file_identity)
    {
        return Err(refusal(reason(
            "local_changed",
            "The local program changed after it was checked; check again before restarting.",
        )));
    }
    if cccc_core::attended_update::active(home)
        .map_err(OpError::io)?
        .as_ref()
        != Some(&intent)
    {
        return Err(OpError::new(
            "update_resume_mismatch",
            "the update intent changed during its final check",
        ));
    }
    // The dispatch caller still holds GlobalWrite. Keep this exact StartPermit
    // through selection and turn it into the closed gate before releasing it.
    let selected = crate::session_owner::select_shutdown(
        home,
        crate::session_owner::ShutdownMode::PreserveAgents,
        request_id,
    )
    .map_err(|error| OpError::new("shutdown_in_progress", error.to_string()))?;
    permit.prevent();
    object(json!({"shutting_down":true,"request_id":selected.request_id,"mode":selected.mode}))
}

pub fn required_arg(request: &DaemonRequest, name: &str) -> Result<String, OpError> {
    string_arg(request, name)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| OpError::new("invalid_args", format!("{name} is required")))
}

pub fn string_arg(request: &DaemonRequest, name: &str) -> Option<String> {
    request
        .args
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub fn first_non_blank_arg(request: &DaemonRequest, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        string_arg(request, name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

pub fn bool_arg(request: &DaemonRequest, name: &str, default: bool) -> bool {
    request
        .args
        .get(name)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

pub fn object<T: Serialize>(value: T) -> OpResult {
    value_map(serde_json::to_value(value).map_err(OpError::invalid)?)
}

pub fn value_map(value: Value) -> OpResult {
    match value {
        Value::Object(result) => Ok(result),
        _ => Err(OpError::new("internal_error", "result is not an object")),
    }
}

pub fn store(home: &HomeLayout) -> Result<GroupStore, OpError> {
    GroupStore::new(home.clone()).map_err(OpError::io)
}

#[derive(Debug)]
pub struct OpError {
    pub code: String,
    pub message: String,
    pub details: Map<String, Value>,
}

impl OpError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Map::new(),
        }
    }
    pub fn io(error: io::Error) -> Self {
        Self::new("io_error", error.to_string())
    }
    pub fn not_found(error: io::Error) -> Self {
        Self::new("not_found", error.to_string())
    }
    pub fn invalid(error: impl std::fmt::Display) -> Self {
        Self::new("invalid_args", error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, first_non_blank_arg};
    use cccc_contracts::DaemonRequest;
    use cccc_core::HomeLayout;
    use serde_json::json;

    #[cfg(target_os = "linux")]
    fn update_fixture() -> (tempfile::TempDir, HomeLayout, String) {
        let temp = tempfile::tempdir().expect("isolated update home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        crate::session_owner::initialize(&home).expect("empty owner controller");
        crate::session_owner::prepare_shutdown(&home, Vec::new()).expect("shutdown state");
        crate::runtime_start_gate::allow(&home).expect("allow starts");
        let id = uuid::Uuid::new_v4().simple().to_string();
        std::fs::create_dir_all(home.root().join("update").join(&id)).expect("transaction");
        let intent = cccc_core::attended_update::Intent::new(
            &home,
            &id,
            cccc_core::attended_update::BuildIdentity {
                version: "0.4.59".into(),
                source_id: "1".repeat(64),
            },
            cccc_core::attended_update::Source::Release {
                install_dir: temp.path().to_path_buf(),
            },
        )
        .expect("intent");
        cccc_core::attended_update::arm(&home, &intent).expect("arm");
        (temp, home, id)
    }

    #[cfg(target_os = "linux")]
    fn update_request(id: &str, keep: bool) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: "shutdown_preserving_agents".into(),
            args: json!({
                "expected_pid":std::process::id(), "request_id":uuid::Uuid::new_v4().to_string(),
                "update":{"transaction":id,"candidate":{
                    "build":{"version":"0.4.59","source_id":"1".repeat(64)},
                    "capabilities":{
                        "preserve_agents":keep,
                        "mcp_daemon_contracts":cccc_contracts::MCP_DAEMON_ACCEPTED_CONTRACTS
                    }
                }}
            })
            .as_object()
            .expect("request")
            .clone(),
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_definitive_preflight_refusal_does_not_select_shutdown_or_close_the_start_gate() {
        let (_temp, home, id) = update_fixture();
        let request = update_request(&id, false);
        let checking = home.clone();
        let result = tokio::task::spawn_blocking(move || dispatch(&checking, &request))
            .await
            .expect("dispatch");
        assert!(!result.ok);
        let error = result.error.expect("refusal");
        assert_eq!(
            error.code,
            cccc_core::update_capabilities::PRESERVATION_REFUSED
        );
        assert_eq!(error.details["shutdown_requested"], false);
        assert!(
            crate::session_owner::selected_shutdown(&home)
                .expect("state")
                .is_none()
        );
        assert!(crate::runtime_start_gate::permit(&home).is_ok());
        assert!(
            cccc_core::attended_update::active(&home)
                .expect("intent")
                .is_some()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_final_check_cannot_use_another_transaction_or_builds_declaration() {
        for wrong_build in [false, true] {
            let (_temp, home, id) = update_fixture();
            let mut request = update_request(&id, true);
            if wrong_build {
                request.args["update"]["candidate"]["build"]["source_id"] = json!("2".repeat(64));
            } else {
                request.args["update"]["transaction"] =
                    json!(uuid::Uuid::new_v4().simple().to_string());
            }
            let result = dispatch(&home, &request);
            assert_eq!(
                result.error.expect("mismatched update").code,
                "update_resume_mismatch"
            );
            assert!(
                crate::session_owner::selected_shutdown(&home)
                    .expect("state")
                    .is_none()
            );
            assert!(crate::runtime_start_gate::permit(&home).is_ok());
            assert!(
                cccc_core::attended_update::active(&home)
                    .expect("intent")
                    .is_some()
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_successful_update_check_selects_preservation_and_closes_the_same_start_gate() {
        let (_temp, home, id) = update_fixture();
        let request = update_request(&id, true);
        let request_id = request.args["request_id"].as_str().expect("id").to_owned();
        let checking = home.clone();
        let result = tokio::task::spawn_blocking(move || dispatch(&checking, &request))
            .await
            .expect("dispatch");
        assert!(result.ok, "{:?}", result.error);
        assert_eq!(result.result["request_id"], request_id);
        let selected = crate::session_owner::selected_shutdown(&home)
            .expect("state")
            .expect("selected");
        assert_eq!(
            selected.mode,
            crate::session_owner::ShutdownMode::PreserveAgents
        );
        assert!(crate::runtime_start_gate::permit(&home).is_err());
        let again = update_request(&id, false);
        let checking = home.clone();
        let result = tokio::task::spawn_blocking(move || dispatch(&checking, &again))
            .await
            .expect("dispatch");
        assert_eq!(
            result.error.expect("already stopping").code,
            "shutdown_in_progress"
        );
    }

    #[test]
    fn documented_operations_resolve_or_explicitly_advertise_unavailability() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "ping".into(),
            args: Default::default(),
        };
        let ping = dispatch(&home, &request);
        let spec = include_str!("../../../docs/standards/CCCC_DAEMON_IPC_V1.md");
        for line in spec.lines().filter(|line| line.starts_with("#### ")) {
            for op in line.split('`').skip(1).step_by(2).filter(|op| {
                !op.is_empty()
                    && op
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            }) {
                // These change the connection protocol and are handled before
                // ordinary dispatch in server_connection (with stream regressions).
                if matches!(
                    op,
                    "term_attach" | "events_stream" | "voice_secretary_terminal_attach"
                ) {
                    continue;
                }
                let request = DaemonRequest {
                    v: 1,
                    op: op.into(),
                    args: Default::default(),
                };
                let available = super::resolve_operation(&request).is_some();
                if ping.result["capabilities"]
                    .get(op)
                    .and_then(serde_json::Value::as_bool)
                    == Some(false)
                {
                    assert!(
                        !available,
                        "{op} has a handler but advertises unavailability"
                    );
                } else {
                    assert!(
                        available,
                        "documented operation {op} has no executable route"
                    );
                }
            }
        }
    }

    #[test]
    fn string_aliases_skip_empty_primary_values() {
        let request = DaemonRequest {
            v: 1,
            op: "test".into(),
            args: json!({"primary":"  ","legacy":" value "})
                .as_object()
                .cloned()
                .expect("args"),
        };

        assert_eq!(
            first_non_blank_arg(&request, &["primary", "legacy"]).as_deref(),
            Some("value")
        );
    }

    #[test]
    fn shutdown_rejects_a_different_daemon_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: json!({"expected_pid":u64::from(std::process::id()) + 1})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let response = dispatch(&home, &request);
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("owner mismatch").code,
            "daemon_owner_mismatch"
        );
    }

    #[test]
    fn shutdown_accepts_its_own_daemon_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: json!({"expected_pid":std::process::id()})
                .as_object()
                .cloned()
                .expect("args"),
        };

        assert!(dispatch(&home, &request).ok);
    }

    #[test]
    fn shutdown_rejects_an_invalid_expected_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let request = DaemonRequest {
            v: 1,
            op: "shutdown".into(),
            args: json!({"expected_pid":0})
                .as_object()
                .cloned()
                .expect("args"),
        };

        let response = dispatch(&home, &request);
        assert!(!response.ok);
        assert_eq!(response.error.expect("invalid pid").code, "invalid_args");
    }
}
