use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value};

pub(super) fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(home, op, args);
    assert!(
        response.ok,
        "{op} failed: {:?}",
        response.error.as_ref().map(|error| &error.message)
    );
    response
}

pub(super) fn raw_call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    };
    cccc_daemon::handle_request(home, &request)
}
