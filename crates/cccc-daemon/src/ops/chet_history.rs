//! Private durable facts for the global assistant. These operations record
//! evidence; they do not start a Runtime or replay an input.
use super::operation::{Operation, Policy::ResourceOwned};
use crate::dispatch::{OpError, OpResult, object};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::chet_history::{
    self, Outcome, Receipt, Registration, StartFailure, Store, TextRequest, Transcript,
    VoiceDelegation,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "chet_history_list" => Operation::new(ResourceOwned, list),
        "chet_history_find" => Operation::new(ResourceOwned, find),
        "chet_history_register" => Operation::new(ResourceOwned, register),
        "chet_history_voice_delegation" => Operation::new(ResourceOwned, voice_delegation),
        "chet_history_receipt" => Operation::new(ResourceOwned, receipt),
        "chet_history_result" => Operation::new(ResourceOwned, result),
        "chet_history_not_started" => Operation::new(ResourceOwned, not_started),
        "chet_history_transcript" => Operation::new(ResourceOwned, transcript),
        _ => return None,
    })
}

fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.get("by").and_then(Value::as_str) != Some("user")
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

fn value<T: DeserializeOwned>(request: &DaemonRequest, key: &str) -> Result<T, OpError> {
    let value = request
        .args
        .get(key)
        .ok_or_else(|| OpError::invalid(format!("{key} is required")))?;
    serde_json::from_value(value.clone()).map_err(OpError::invalid)
}

fn map_error(error: chet_history::Error) -> OpError {
    let code = match &error {
        chet_history::Error::Invalid(_) => "invalid_args",
        chet_history::Error::IdConflict => "chet_history_id_conflict",
        chet_history::Error::OutcomeConflict => "chet_history_outcome_conflict",
        chet_history::Error::ReceiptConflict => "chet_history_receipt_conflict",
        chet_history::Error::UnknownInput => "chet_history_unknown_input",
        chet_history::Error::Corrupt => "chet_history_corrupt",
        chet_history::Error::Rejected(_) => "chet_history_unavailable",
        chet_history::Error::Unknown(_) => "chet_history_unknown",
    };
    OpError::new(code, error.to_string())
}

fn registration(value: Registration) -> &'static str {
    match value {
        Registration::Inserted => "inserted",
        Registration::Existing => "existing",
    }
}

fn list(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let limit = match request.args.get("limit") {
        None | Some(Value::Null) => 50,
        Some(_) => value(request, "limit")?,
    };
    let before = match request.args.get("before") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            value::<String>(request, "before")?
                .parse::<u64>()
                .map_err(OpError::invalid)?,
        ),
    };
    let after = match request.args.get("after") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            value::<String>(request, "after")?
                .parse::<u64>()
                .map_err(OpError::invalid)?,
        ),
    };
    if before.is_some() && after.is_some() {
        return Err(OpError::invalid("before and after cannot be combined"));
    }
    if let Some(after) = after {
        let page = Store::new(home.root())
            .after(after, limit)
            .map_err(map_error)?;
        return object(
            json!({"records":page.records,"after":page.next.to_string(),"has_more":page.has_more}),
        );
    }
    let page = Store::new(home.root())
        .page(before, limit)
        .map_err(map_error)?;
    object(
        json!({"records":page.records,"before":page.before.map(|n| n.to_string()),"after":page.head.to_string()}),
    )
}

fn find(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let records = Store::new(home.root()).find_input(id).map_err(map_error)?;
    object(json!({"records":records}))
}

fn register(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input: TextRequest = value(request, "request")?;
    let status = Store::new(home.root())
        .register_input(id, input)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn result(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input_ids: Vec<Uuid> = value(request, "input_ids")?;
    let outcome: Outcome = value(request, "outcome")?;
    let status = Store::new(home.root())
        .record_result(id, input_ids, outcome)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn voice_delegation(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let delegation: VoiceDelegation = value(request, "delegation")?;
    let status = Store::new(home.root())
        .record_voice_delegation(id, delegation)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn receipt(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input_id: Uuid = value(request, "input_id")?;
    let receipt: Receipt = value(request, "receipt")?;
    let status = Store::new(home.root())
        .record_receipt(id, input_id, receipt)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn not_started(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let input_id: Uuid = value(request, "input_id")?;
    let reason: StartFailure = value(request, "reason")?;
    let status = Store::new(home.root())
        .record_not_started(id, input_id, reason)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

fn transcript(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let id: Uuid = value(request, "id")?;
    let update: Transcript = value(request, "transcript")?;
    let status = Store::new(home.root())
        .record_transcript(id, update)
        .map_err(map_error)?;
    object(json!({"registration":registration(status)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
        crate::dispatch::dispatch(
            home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("fixture object").clone(),
            },
        )
    }

    #[test]
    fn history_rejects_bound_and_non_user_callers() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home layout");
        for args in [
            json!({}),
            json!({"by":"actor"}),
            json!({"by":"user","_cccc_native_binding":{}}),
            json!({"by":"user","_cccc_web_binding":{}}),
        ] {
            let response = call(&home, "chet_history_list", args);
            assert_eq!(response.error.expect("denial").code, "permission_denied");
        }
    }

    #[test]
    fn ipc_registration_is_idempotent_and_history_is_read_only() {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home layout");
        home.initialize().expect("initialize");
        let empty = call(&home, "chet_history_list", json!({"by":"user"}));
        assert!(empty.ok);
        assert!(!temp.path().join("state/chet").exists());
        let id = Uuid::new_v4();
        let args = json!({"by":"user","id":id,"request":{"text":"question","context":null}});
        let first = call(&home, "chet_history_register", args.clone());
        let retry = call(&home, "chet_history_register", args);
        assert_eq!(first.result["registration"], "inserted");
        assert_eq!(retry.result["registration"], "existing");
        let history = call(&home, "chet_history_list", json!({"by":"user"}));
        assert_eq!(
            history.result["records"].as_array().expect("records").len(),
            1
        );
    }
}
