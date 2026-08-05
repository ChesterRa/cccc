use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::presentation::{self, Publish};
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};
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
        presentation::publish(&store(home)?, &group_id, publish).map_err(OpError::invalid)?;
    let event = messaging::append(
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
            "replaced":replaced
        }))?,
    )?;
    object(json!({
        "group_id":group_id,
        "slot_id":slot_id,
        "card":card,
        "presentation":snapshot,
        "replaced":replaced,
        "event_id":event.id
    }))
}

fn clear(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let requested = string_arg(request, "slot").unwrap_or_default();
    let (cleared_slots, snapshot) =
        presentation::clear(&store(home)?, &group_id, &requested).map_err(OpError::invalid)?;
    let event = messaging::append(
        home,
        &group_id,
        "presentation.clear",
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
        object(json!({"cleared_slots":cleared_slots}))?,
    )?;
    object(json!({
        "group_id":group_id,
        "slot_id": if cleared_slots.len() == 1 { Value::String(cleared_slots[0].clone()) } else { Value::String(String::new()) },
        "cleared_slots":cleared_slots,
        "presentation":snapshot,
        "event_id":event.id
    }))
}
