use super::*;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(super) fn handle(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let action = string_arg(request, "action").unwrap_or_else(|| "run".into());
    if action == "status" {
        let value = load(home, &group_id)?;
        return object(json!({
            "group_id":group_id,"provider":provider(request),"lane":lane,
            "sync":value.get("sync").and_then(|sync|sync.get(&lane)).cloned()
                .unwrap_or_else(||json!({"status":"never","converged":false}))
        }));
    }
    if action != "run" {
        return Err(OpError::new("invalid_args", "action must be status or run"));
    }
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let value = load(home, &group_id)?;
    let remote_space_id = binding_id(&value, &lane)?;
    let root_path = sync_root(home, &group_id, &lane)?;
    let files = collect_files(&root_path, lane == "memory")?;
    let previous = value
        .get("sync")
        .and_then(|sync| sync.get(&lane))
        .and_then(|sync| sync.get("items"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut next = Map::new();
    let mut added = 0_u64;
    let mut updated = 0_u64;
    let mut unchanged = 0_u64;
    for path in files {
        let file_size = std::fs::metadata(&path).map_err(OpError::io)?.len();
        if file_size > MAX_LOCAL_FILE_SIZE_BYTES {
            return Err(OpError::new(
                "space_source_file_too_large",
                format!(
                    "sync source exceeds the {} byte limit: {}",
                    MAX_LOCAL_FILE_SIZE_BYTES,
                    path.display()
                ),
            ));
        }
        let relative = path
            .strip_prefix(&root_path)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let content = std::fs::read_to_string(&path).map_err(OpError::io)?;
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        let old = previous.get(&relative);
        if old.and_then(|item| item["content_hash"].as_str()) == Some(hash.as_str()) {
            next.insert(relative, old.cloned().unwrap_or(Value::Null));
            unchanged += 1;
            continue;
        }
        let source = notebooklm::add_text(home, &remote_space_id, &relative, &content)?;
        if let Some(source_id) = old.and_then(|item| item["source_id"].as_str()) {
            // Publish the replacement before removing the previous source.
            // A failed upload must never erase the last good remote copy.
            notebooklm::delete_source(home, &remote_space_id, source_id)?;
            updated += 1;
        } else {
            added += 1;
        }
        next.insert(relative, json!({
            "source_id":source.id,"content_hash":hash,"bytes":content.len(),"updated_at":utc_now()
        }));
    }
    let removed_paths = previous
        .keys()
        .filter(|path| !next.contains_key(*path))
        .cloned()
        .collect::<Vec<_>>();
    for path in &removed_paths {
        if let Some(source_id) = previous[path]["source_id"].as_str() {
            notebooklm::delete_source(home, &remote_space_id, source_id)?;
        }
    }
    let result = json!({
        "status":"succeeded","converged":true,"provider":provider,"lane":lane,
        "remote_space_id":remote_space_id,"root":root_path,"items":next,
        "added":added,"updated":updated,"removed":removed_paths.len(),"unchanged":unchanged,
        "last_sync_at":utc_now()
    });
    update(home, &group_id, |value| {
        let root = root(value);
        let sync = root.entry("sync").or_insert_with(|| json!({}));
        if !sync.is_object() {
            *sync = json!({});
        }
        sync.as_object_mut()
            .expect("sync initialized")
            .insert(lane.clone(), result.clone());
        Ok(())
    })?;
    object(
        json!({"group_id":group_id,"provider":provider,"lane":lane,"sync":result,"sync_result":{"ok":true,"converged":true}}),
    )
}

fn sync_root(home: &HomeLayout, group_id: &str, lane: &str) -> Result<PathBuf, OpError> {
    if lane == "memory" {
        return cccc_core::memory::MemoryStore::new(home.clone())
            .layout(group_id, None)
            .map(|layout| layout.daily_dir)
            .map_err(OpError::io);
    }
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(OpError::io)?;
    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
        .ok_or_else(|| OpError::new("scope_required", "work sync requires an active scope"))?;
    let root = Path::new(&scope.url).join("space");
    std::fs::create_dir_all(&root).map_err(OpError::io)?;
    Ok(root)
}

fn collect_files(root: &Path, historical_daily_only: bool) -> Result<Vec<PathBuf>, OpError> {
    let mut output = Vec::new();
    collect(root, root, &mut output)?;
    if historical_daily_only {
        let today = utc_now().get(..10).unwrap_or_default().to_owned();
        output.retain(|path| {
            let historical = path
                .file_name()
                .and_then(|value| value.to_str())
                .and_then(|value| value.get(..10))
                .is_some_and(|date| date < today.as_str());
            historical
                && std::fs::read_to_string(path).is_ok_and(|content| has_daily_content(&content))
        });
    }
    output.sort();
    Ok(output)
}

fn has_daily_content(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .any(|line| !line.is_empty() && !line.starts_with('#'))
}

fn collect(root: &Path, path: &Path, output: &mut Vec<PathBuf>) -> Result<(), OpError> {
    for entry in std::fs::read_dir(path).map_err(OpError::io)? {
        let entry = entry.map_err(OpError::io)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(OpError::io)?;
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let excluded_top_level = entry.path().parent().is_some_and(|parent| parent == root)
            && matches!(
                name.as_ref(),
                "artifacts" | "remote_sources" | "remote-sources"
            );
        if excluded_top_level {
            continue;
        }
        if file_type.is_dir() {
            collect(root, &path, output)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "md" | "txt"))
        {
            output.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::collect_files;
    use tempfile::tempdir;

    #[test]
    fn work_sync_skips_hidden_generated_and_symlinked_content() {
        let temp = tempdir().expect("temp");
        let root = temp.path().join("space");
        std::fs::create_dir_all(root.join("artifacts")).expect("artifacts");
        std::fs::create_dir_all(root.join(".sync")).expect("hidden");
        std::fs::write(root.join("keep.md"), "keep").expect("keep");
        std::fs::write(root.join("artifacts/report.md"), "generated").expect("generated");
        std::fs::write(root.join(".sync/state.txt"), "state").expect("state");
        #[cfg(unix)]
        std::os::unix::fs::symlink(temp.path(), root.join("outside")).expect("symlink");

        let files = collect_files(&root, false).expect("files");
        assert_eq!(files, vec![root.join("keep.md")]);
    }
}
