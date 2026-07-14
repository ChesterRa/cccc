use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::ledger_archive;
use serde_json::json;

use crate::dispatch::{OpResult, object, required_arg, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "ledger_snapshot" => snapshot(home, request),
        "ledger_compact" => compact(home, request),
        _ => return None,
    })
}

fn snapshot(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let reason = string_arg(request, "reason").unwrap_or_else(|| "manual".into());
    let snapshot =
        ledger_archive::snapshot(home, &group_id, &reason).map_err(crate::dispatch::OpError::io)?;
    object(json!({"snapshot": snapshot}))
}

fn compact(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let reason = string_arg(request, "reason").unwrap_or_else(|| "manual".into());
    let segment =
        ledger_archive::compact(home, &group_id, &reason).map_err(crate::dispatch::OpError::io)?;
    object(json!({"compacted": segment.is_some(), "segment": segment}))
}
