use cccc_core::HomeLayout;
use serde_json::Value;
use std::io;

pub(super) fn ensure_writable(document: &Value) -> io::Result<()> {
    if is_deleted(document) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "voice document is deleted",
        ));
    }
    Ok(())
}

pub(super) fn ensure_path_writable(
    home: &HomeLayout,
    group_id: &str,
    path: &str,
) -> io::Result<()> {
    let state = super::voice_document_state::load(home, group_id)?;
    for document in super::items(&state, "documents") {
        if document["document_path"] == path {
            return ensure_writable(document);
        }
    }
    Ok(())
}

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
