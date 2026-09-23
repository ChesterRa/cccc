use super::*;

pub(super) fn resolve(request: &DaemonRequest) -> Option<Operation> {
    match request.op.as_str() {
        "assistant_voice_document_library" => Some(Operation::new(Read, read)),
        "assistant_voice_document_library_update" => Some(Operation::new(Write, mutate)),
        _ => None,
    }
}

fn projection(state: &Value) -> Value {
    json!({"folders":state["folders"].as_array().cloned().unwrap_or_default(),
        "documents":items(state,"documents").iter().filter(|d| !voice_document_state::is_deleted(d)).collect::<Vec<_>>()})
}

fn read(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    object(projection(
        &voice_document_state::load(home, &group_id).map_err(OpError::io)?,
    ))
}

fn mutate(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&group_id)
        .map_err(OpError::not_found)?;
    require_voice_status_permission(
        &group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )?;
    let action = required_arg(request, "action")?;
    let folder_id = string_arg(request, "folder_id").unwrap_or_default();
    let name = string_arg(request, "name")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let path = string_arg(request, "document_path").unwrap_or_default();
    let result = voice_document_state::update(home, &group_id, |state| {
        let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
        match action.as_str() {
            "create_folder" | "rename_folder" => {
                if name.is_empty() || name.chars().count() > 80 {
                    return Err(invalid("Folder name must contain 1 to 80 characters"));
                }
                let folders = array(state, "folders");
                if folders
                    .iter()
                    .any(|f| f["name"] == name && f["folder_id"] != folder_id)
                {
                    return Err(invalid("Folder name already exists"));
                }
                if action == "create_folder" {
                    folders.push(json!({"folder_id":Uuid::new_v4().to_string(),"name":name}));
                } else {
                    folders
                        .iter_mut()
                        .find(|f| f["folder_id"] == folder_id)
                        .ok_or_else(|| invalid("Folder not found"))?["name"] = json!(name);
                }
            }
            "remove_folder" => {
                let folders = array(state, "folders");
                if !folders.iter().any(|f| f["folder_id"] == folder_id) {
                    return Err(invalid("Folder not found"));
                }
                folders.retain(|f| f["folder_id"] != folder_id);
                for document in array(state, "documents") {
                    if document["folder_id"] == folder_id {
                        document["folder_id"] = json!("");
                    }
                }
            }
            "rename" => {
                if name.is_empty() || name.chars().count() > 80 {
                    return Err(invalid("Document title must contain 1 to 80 characters"));
                }
                let document = array(state, "documents")
                    .iter_mut()
                    .find(|d| d["document_path"] == path && !voice_document_state::is_deleted(d))
                    .ok_or_else(|| invalid("Document not found"))?;
                document["title"] = json!(name);
                document["updated_at"] = json!(utc_now());
            }
            "move" | "restore" => {
                if action == "move"
                    && !folder_id.is_empty()
                    && !array(state, "folders")
                        .iter()
                        .any(|f| f["folder_id"] == folder_id)
                {
                    return Err(invalid("Folder not found"));
                }
                let document = array(state, "documents")
                    .iter_mut()
                    .find(|d| d["document_path"] == path && !voice_document_state::is_deleted(d))
                    .ok_or_else(|| invalid("Document not found"))?;
                if action == "restore" {
                    if document["status"] != "archived" {
                        return Err(invalid("Document is not archived"));
                    }
                    document["status"] = json!("active");
                } else {
                    document["folder_id"] = json!(folder_id);
                }
                document["updated_at"] = json!(utc_now());
            }
            _ => return Err(invalid("Unknown library action")),
        }
        Ok(Value::Object(state.clone()))
    })
    .map_err(OpError::io)?;
    object(projection(&result))
}
