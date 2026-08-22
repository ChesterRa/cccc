use cccc_contracts::{DaemonRequest, Event};
use cccc_core::HomeLayout;
use serde_json::Value;

use crate::dispatch::{OpError, required_arg};

pub(super) fn decorate(
    home: &HomeLayout,
    request: &DaemonRequest,
    events: Vec<Event>,
) -> Result<Vec<Value>, OpError> {
    let include = [
        ("with_read_status", "read_status", "_read_status"),
        (
            "with_obligation_status",
            "obligation_status",
            "_obligation_status",
        ),
    ];
    if !include.iter().any(|(flag, _, _)| bool_arg(request, flag)) {
        return events
            .into_iter()
            .map(|event| serde_json::to_value(event).map_err(OpError::invalid))
            .collect();
    }
    let group_id = required_arg(request, "group_id")?;
    let ids = events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let statuses = super::messaging_status::for_events(home, &group_id, &ids)?;
    events
        .into_iter()
        .map(|event| {
            let status = statuses.get(&event.id).and_then(Value::as_object);
            let mut value = serde_json::to_value(event).map_err(OpError::invalid)?;
            let object = value
                .as_object_mut()
                .ok_or_else(|| OpError::invalid("serialized event must be an object"))?;
            for (flag, source, target) in include {
                if bool_arg(request, flag)
                    && let Some(payload) = status.and_then(|item| item.get(source))
                {
                    object.insert(target.into(), payload.clone());
                }
            }
            Ok(value)
        })
        .collect()
}

fn bool_arg(request: &DaemonRequest, name: &str) -> bool {
    request
        .args
        .get(name)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
