use super::*;
#[path = "voice_document_move.rs"]
mod file_move;

pub(super) fn delete(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let path = document_path(request)?;
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(&group_id).map_err(OpError::not_found)?;
    require_voice_status_permission(
        &group,
        &string_arg(request, "by").unwrap_or_else(|| "user".into()),
    )?;
    let lease = voice_recording_lease::current(home)
        .map_err(|error| OpError::new(error.code, error.message))?;
    if lease["group_id"] == group_id {
        return Err(OpError::new(
            "voice_recording_active",
            "Stop recording before deleting a voice document",
        ));
    }
    let (source, _) = document_storage_path(home, &group, &path)?;
    let trash = home
        .root()
        .join("voice-secretary")
        .join(&group_id)
        .join("trash")
        .join(format!("{}.md", Uuid::new_v4()));
    let mut moved = false;
    let mut previous = None;
    let result = voice_document_state::update(home, &group_id, |state| {
        previous = Some(state.clone());
        let document = array(state, "documents")
            .iter_mut()
            .find(|item| item["document_path"] == path && !voice_document_state::is_deleted(item))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "document not found"))?;
        std::fs::create_dir_all(trash.parent().expect("trash parent"))?;
        match file_move::move_file(&source, &trash) {
            Ok(()) => moved = true,
            Err(error) if error.kind() == io::ErrorKind::NotFound && !source.try_exists()? => {}
            Err(error) => return Err(error),
        }
        document["status"] = json!("deleted");
        document["updated_at"] = json!(utc_now());
        document["trash_path"] = Value::Null;
        Ok(document.clone())
    });
    let document = match result {
        Ok(document) => document,
        Err(error) => {
            if moved {
                rollback(
                    home,
                    &group_id,
                    &source,
                    &trash,
                    previous.expect("snapshot before move"),
                )?;
            }
            return Err(OpError::io(error));
        }
    };
    let response = match document_result(home, request, &group_id, document, "deleted") {
        Ok(response) => response,
        Err(error) => {
            rollback(
                home,
                &group_id,
                &source,
                &trash,
                previous.expect("snapshot before commit"),
            )?;
            return Err(error);
        }
    };
    // No fallible transaction steps remain: only now discard the recovery copy.
    cleanup_copy(&trash);
    Ok(response)
}

fn rollback(
    home: &HomeLayout,
    group_id: &str,
    source: &std::path::Path,
    trash: &std::path::Path,
    previous: Map<String, Value>,
) -> Result<(), OpError> {
    let result = (|| -> io::Result<()> {
        if trash.try_exists()? {
            file_move::copy_file(trash, source)?;
        }
        voice_document_state::update(home, group_id, |state| {
            *state = previous;
            Ok(())
        })
    })();
    result.map_err(|error| {
        OpError::new(
            "io_error",
            format!(
                "delete rollback failed: {error}; recovery copy retained at {}",
                trash.display()
            ),
        )
    })?;
    cleanup_copy(trash);
    Ok(())
}

fn cleanup_copy(trash: &std::path::Path) {
    if let Err(error) = std::fs::remove_file(trash) {
        if error.kind() != io::ErrorKind::NotFound {
            tracing::warn!(%error, path = %trash.display(), "could not remove voice document transaction backup");
        }
    }
}
