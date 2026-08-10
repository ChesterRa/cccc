use serde_json::{Map, Value, json};

pub(super) fn is_active(document: &Value) -> bool {
    document["status"]
        .as_str()
        .unwrap_or("active")
        .trim()
        .eq_ignore_ascii_case("active")
}

pub(super) fn is_deleted(document: &Value) -> bool {
    document["status"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .eq_ignore_ascii_case("deleted")
}

pub(super) fn resolved_active<'a>(
    documents: &'a [Value],
    configured_id: &str,
    configured_path: &str,
) -> Option<&'a Value> {
    let configured_id = configured_id.trim();
    let configured_path = configured_path.trim();
    documents
        .iter()
        .find(|document| {
            is_active(document)
                && !configured_id.is_empty()
                && document["document_id"] == configured_id
        })
        .or_else(|| {
            documents.iter().find(|document| {
                is_active(document)
                    && !configured_path.is_empty()
                    && document["document_path"] == configured_path
            })
        })
        .or_else(|| {
            (!configured_id.is_empty() || !configured_path.is_empty())
                .then(|| latest_active(documents, None))
                .flatten()
        })
}

pub(super) fn latest_active<'a>(
    documents: &'a [Value],
    excluded_id: Option<&str>,
) -> Option<&'a Value> {
    documents
        .iter()
        .filter(|document| {
            is_active(document) && excluded_id.is_none_or(|id| document["document_id"] != id)
        })
        .max_by(|left, right| {
            updated_at(left).cmp(updated_at(right)).then_with(|| {
                left["document_id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["document_id"].as_str().unwrap_or_default())
            })
        })
}

pub(super) fn active_path(state: &Value) -> Option<&str> {
    let documents = state["documents"].as_array()?;
    resolved_active(
        documents,
        state["active_document_id"].as_str().unwrap_or_default(),
        state["active_document_path"].as_str().unwrap_or_default(),
    )?["document_path"]
        .as_str()
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

pub(super) fn needs_active_repair(state: &Value) -> bool {
    let configured_id = state["active_document_id"].as_str().unwrap_or_default();
    let configured_path = state["active_document_path"].as_str().unwrap_or_default();
    if configured_id.is_empty() && configured_path.is_empty() {
        return false;
    }
    let resolved = state["documents"]
        .as_array()
        .and_then(|documents| resolved_active(documents, configured_id, configured_path));
    resolved
        .and_then(|document| document["document_id"].as_str())
        .unwrap_or_default()
        != configured_id
        || resolved
            .and_then(|document| document["document_path"].as_str())
            .unwrap_or_default()
            != configured_path
}

pub(super) fn repair_active(state: &mut Map<String, Value>) {
    let configured_id = state
        .get("active_document_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let configured_path = state
        .get("active_document_path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if configured_id.is_empty() && configured_path.is_empty() {
        return;
    }
    let resolved = state
        .get("documents")
        .and_then(Value::as_array)
        .and_then(|documents| resolved_active(documents, configured_id, configured_path))
        .cloned();
    set_active(state, resolved.as_ref());
}

pub(super) fn set_active(state: &mut Map<String, Value>, document: Option<&Value>) {
    state.insert(
        "active_document_id".into(),
        document
            .map(|item| item["document_id"].clone())
            .unwrap_or_else(|| json!("")),
    );
    state.insert(
        "active_document_path".into(),
        document
            .map(|item| item["document_path"].clone())
            .unwrap_or_else(|| json!("")),
    );
}

fn updated_at(document: &Value) -> &str {
    document["updated_at"]
        .as_str()
        .filter(|value| !value.is_empty())
        .or_else(|| document["created_at"].as_str())
        .unwrap_or_default()
}
