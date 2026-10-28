use super::operation::{Operation, Policy::Write};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::ledger_archive;
use serde_json::json;

use crate::dispatch::{OpResult, object, required_arg, string_arg};

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    Some(match request.op.as_str() {
        "ledger_snapshot" => Operation::new(Write, snapshot),
        "ledger_compact" => Operation::new(Write, compact),
        _ => return None,
    })
}

fn snapshot(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    // Taking the ledger lock creates the Group's state directory, so a deleted
    // Group must be rejected before maintenance touches the filesystem.
    super::context::load_group(home, &group_id)?;
    let reason = string_arg(request, "reason").unwrap_or_else(|| "manual".into());
    let snapshot =
        ledger_archive::snapshot(home, &group_id, &reason).map_err(crate::dispatch::OpError::io)?;
    object(json!({"snapshot": snapshot}))
}

fn compact(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    super::context::load_group(home, &group_id)?;
    let reason = string_arg(request, "reason").unwrap_or_else(|| "manual".into());
    let segment =
        ledger_archive::compact(home, &group_id, &reason).map_err(crate::dispatch::OpError::io)?;
    object(json!({"compacted": segment.is_some(), "segment": segment}))
}
