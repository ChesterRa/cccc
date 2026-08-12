use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::presentation::{self, Publish};
use serde_json::json;

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, store, string_arg};
use crate::ops::messaging;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "presentation_get" => get(home, request),
        "presentation_publish" => publish(home, request),
        "presentation_clear" => clear(home, request),
        _ => return None,
    })
}

fn get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let snapshot = presentation::load(&store(home)?, &group_id).map_err(OpError::not_found)?;
    object(json!({"group_id":group_id,"presentation":snapshot}))
}

fn publish(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group_store = store(home)?;
    let previous = presentation::load(&group_store, &group_id).map_err(OpError::not_found)?;
    let publish = Publish {
        slot: string_arg(request, "slot").unwrap_or_default(),
        card_type: string_arg(request, "card_type").unwrap_or_default(),
        title: string_arg(request, "title").unwrap_or_default(),
        summary: string_arg(request, "summary").unwrap_or_default(),
        content: string_arg(request, "content").unwrap_or_default(),
        table: request
            .args
            .get("table")
            .filter(|value| !value.is_null())
            .cloned(),
        path: string_arg(request, "path").unwrap_or_default(),
        url: string_arg(request, "url").unwrap_or_default(),
        blob_rel_path: string_arg(request, "blob_rel_path").unwrap_or_default(),
        by: string_arg(request, "by").unwrap_or_else(|| "user".into()),
        source_label: string_arg(request, "source_label").unwrap_or_default(),
        source_ref: string_arg(request, "source_ref").unwrap_or_default(),
    };
    let (slot_id, card, snapshot, replaced) =
        presentation::publish(&group_store, &group_id, publish).map_err(OpError::invalid)?;
    let event = rollback_on_event_error(
        &group_store,
        &group_id,
        &previous,
        messaging::append(
            home,
            &group_id,
            "presentation.publish",
            &string_arg(request, "by").unwrap_or_else(|| "user".into()),
            object(json!({
                "slot_id":slot_id,
                "card_type":card.card_type,
                "title":card.title,
                "source_label":card.source_label,
                "source_ref":card.source_ref,
                "summary":card.summary
            }))?,
        ),
    )?;
    object(json!({
        "group_id":group_id,
        "slot_id":slot_id,
        "card":card,
        "presentation":snapshot,
        "replaced":replaced,
        "event_id":event.id,
        "event":event
    }))
}

fn clear(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group_store = store(home)?;
    let previous = presentation::load(&group_store, &group_id).map_err(OpError::not_found)?;
    let requested_slot = string_arg(request, "slot").unwrap_or_default();
    let clear_all = bool_arg(request, "all", false) || requested_slot.trim().is_empty();
    let requested = if clear_all { "" } else { &requested_slot };
    let by = string_arg(request, "by").unwrap_or_else(|| "user".into());
    let (cleared_slots, snapshot) =
        presentation::clear(&group_store, &group_id, requested, &by).map_err(OpError::invalid)?;
    let slot_id = if cleared_slots.len() == 1 {
        cleared_slots[0].clone()
    } else {
        String::new()
    };
    let event = rollback_on_event_error(
        &group_store,
        &group_id,
        &previous,
        messaging::append(
            home,
            &group_id,
            "presentation.clear",
            &by,
            object(json!({
                "slot_id":slot_id,
                "cleared_all":clear_all,
                "cleared_slots":cleared_slots
            }))?,
        ),
    )?;
    object(json!({
        "group_id":group_id,
        "slot_id":slot_id,
        "cleared_slots":cleared_slots,
        "presentation":snapshot,
        "event_id":event.id,
        "event":event
    }))
}

fn rollback_on_event_error<T>(
    group_store: &cccc_core::GroupStore,
    group_id: &str,
    previous: &presentation::Snapshot,
    result: Result<T, OpError>,
) -> Result<T, OpError> {
    match result {
        Ok(value) => Ok(value),
        Err(mut error) => {
            if let Err(rollback) = presentation::save(group_store, group_id, previous) {
                error.message = format!("{}; rollback_failed: {rollback}", error.message);
            }
            Err(error)
        }
    }
}
