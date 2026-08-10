use cccc_contracts::DaemonRequest;
use cccc_core::{HomeLayout, group_bridge_legacy};
use serde_json::{Map, Value, json};
use std::io;

use crate::dispatch::OpError;

pub(super) fn bridge_state(home: &HomeLayout) -> Result<Value, OpError> {
    group_bridge_legacy::load(home).map_err(OpError::io)
}

pub(super) fn route(
    state: &Value,
    registration_id: &str,
    group_id: &str,
) -> Result<Value, OpError> {
    ["outbounds", "trusts", "registrations"]
        .into_iter()
        .flat_map(|key| items(state, key))
        .find(|item| {
            (item["registration_id"] == registration_id || item["trust_id"] == registration_id)
                && item["group_id"] == group_id
                && item["status"] == "active"
        })
        .cloned()
        .ok_or_else(|| {
            OpError::new(
                "registration_not_found",
                "active Group Bridge registration not found",
            )
        })
}

pub(super) fn find_delivery(state: &Value, registration_id: &str, key: &str) -> Option<Value> {
    items(state, "deliveries")
        .iter()
        .find(|item| item["registration_id"] == registration_id && item["idempotency_key"] == key)
        .cloned()
}

pub(super) fn store_delivery(home: &HomeLayout, receipt: Value) -> Result<(), OpError> {
    group_bridge_legacy::update(home, |root| {
        let deliveries = root.entry("deliveries").or_insert_with(|| json!([]));
        if !deliveries.is_array() {
            *deliveries = json!([]);
        }
        let deliveries = deliveries
            .as_array_mut()
            .ok_or_else(|| io::Error::other("group bridge deliveries must be an array"))?;
        deliveries.retain(|item| {
            item["registration_id"] != receipt["registration_id"]
                || item["idempotency_key"] != receipt["idempotency_key"]
        });
        deliveries.push(receipt);
        Ok(())
    })
    .map_err(OpError::io)
}

pub(super) fn dispatch_message(
    home: &HomeLayout,
    op: &str,
    args: Map<String, Value>,
) -> Result<Map<String, Value>, OpError> {
    super::super::messaging::handle(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args,
        },
    )
    .ok_or_else(|| OpError::new("internal_error", "messaging operation unavailable"))?
}

pub(super) fn items<'a>(state: &'a Value, key: &str) -> &'a [Value] {
    state
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub(super) fn nonempty(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value[*key].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
