//! Private, user-directed review of the daemon's own process guard.
use super::operation::{Operation, Policy::ResourceOwned};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::json;

use crate::dispatch::{OpError, OpResult, object, required_arg};
use cccc_runtime::process_recovery::{self, RecoveryError};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "owned_process_recovery_list" => Operation::new(ResourceOwned, list),
        "owned_process_recovery_retry" => Operation::new(ResourceOwned, retry),
        "owned_process_recovery_accept" => Operation::new(ResourceOwned, accept),
        _ => return None,
    })
}

fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.get("by").and_then(|value| value.as_str()) != Some("user")
        || request.args.contains_key("_cccc_native_binding")
        || request.args.contains_key("_cccc_native_source_binding")
        || request.args.contains_key("_cccc_web_binding")
        || request.args.contains_key("group_id")
    {
        return Err(OpError::new(
            "permission_denied",
            "user authority is required",
        ));
    }
    Ok(())
}

fn recovery_error(error: RecoveryError) -> OpError {
    match error {
        RecoveryError::Changed => OpError::new(
            "process_recovery_changed",
            "Recovery evidence changed; review the updated list.",
        ),
        RecoveryError::Unavailable => OpError::new(
            "process_recovery_unavailable",
            "Recovery evidence is unavailable; try again or use offline review.",
        ),
    }
}

fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let snapshot =
        process_recovery::list_active(&home.daemon_dir().join("owned-process-groups.json"))
            .map_err(recovery_error)?;
    object(json!({"owner":"daemon","records":snapshot.records,"diagnostics":snapshot.diagnostics}))
}

fn retry(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id = required_arg(request, "id")?;
    let version = required_arg(request, "version")?;
    let ledger = home.daemon_dir().join("owned-process-groups.json");
    let result = process_recovery::retry_active(&ledger, &id, &version).map_err(recovery_error)?;
    let snapshot = process_recovery::list_active(&ledger).map_err(recovery_error)?;
    let record = snapshot.records.into_iter().find(|record| record.id == id);
    object(
        json!({"owner":"daemon","result":result,"record":record,"diagnostics":snapshot.diagnostics}),
    )
}

fn accept(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id = required_arg(request, "id")?;
    let version = required_arg(request, "version")?;
    process_recovery::accept_active(
        &home.daemon_dir().join("owned-process-groups.json"),
        &id,
        &version,
    )
    .map_err(recovery_error)?;
    object(json!({"owner":"daemon","accepted":true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn review_operations_reject_actor_or_bound_requests_before_reading_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        for args in [
            json!({}),
            json!({"by":"actor"}),
            json!({"by":"user","group_id":"g_1"}),
            json!({"by":"user","_cccc_web_binding":{}}),
        ] {
            let request = DaemonRequest {
                v: 1,
                op: "owned_process_recovery_list".into(),
                args: args.as_object().expect("fixture object").clone(),
            };
            assert_eq!(
                list(&home, &request).expect_err("request is rejected").code,
                "permission_denied"
            );
        }
    }
}
