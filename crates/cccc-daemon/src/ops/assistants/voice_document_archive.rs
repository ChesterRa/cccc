use super::*;

pub(super) fn archive(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = document_path(request)?;
    let document = voice_document_state::update(home, &group_id, |state| {
        let document = {
            let item = array(state, "documents")
                .iter_mut()
                .find(|item| {
                    item["document_path"] == path && !voice_document_state::is_deleted(item)
                })
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
            item["status"] = json!("archived");
            item["updated_at"] = json!(utc_now());
            item.clone()
        };
        let archived_id = document["document_id"].as_str().unwrap_or_default();
        let was_active =
            state["active_document_id"] == archived_id || state["active_document_path"] == path;
        if was_active {
            let next =
                voice_document_state::latest_active(array(state, "documents"), Some(archived_id))
                    .cloned();
            voice_document_state::set_active(state, next.as_ref());
        }
        Ok(document)
    })
    .map_err(OpError::io)?;
    document_result(home, request, &group_id, document, "archived")
}
