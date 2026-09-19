use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(super) fn call_tool(
    root: &Path,
    tool: &str,
    action: &str,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let allowed = match tool {
        "cccc_repo" => matches!(action, "info" | "list" | "list_dir" | "read" | "search"),
        "cccc_repo_edit" => matches!(
            action,
            "replace" | "multi_replace" | "write" | "mkdir" | "delete" | "move"
        ),
        _ => false,
    };
    if !allowed {
        return Err(format!("{action} is not supported by {tool}"));
    }
    call(root, action, args)
}

pub fn call(root: &Path, action: &str, args: &Map<String, Value>) -> Result<Value, String> {
    match action {
        "info" => Ok(json!({"root":root,"git":root.join(".git").exists()})),
        "list" | "list_dir" | "read" | "search" => crate::repo_inspect::call(root, action, args),
        "replace" | "multi_replace" | "write" | "mkdir" | "delete" | "move" => {
            edit(root, action, args)
        }
        _ => Err(format!("unsupported repository action: {action}")),
    }
}

pub fn resolve(root: &Path, raw: &str, create: bool) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let relative = Path::new(raw);
    if relative.is_absolute()
        || relative
            .components()
            .any(|item| matches!(item, std::path::Component::ParentDir))
    {
        return Err("path must be relative and remain inside the active scope".into());
    }
    let path = root.join(relative);
    let checked = if path.exists() {
        path.canonicalize().map_err(|error| error.to_string())?
    } else if create {
        let parent = path
            .parent()
            .ok_or_else(|| "path has no parent".to_owned())?;
        let parent = parent.canonicalize().map_err(|error| error.to_string())?;
        parent.join(
            path.file_name()
                .ok_or_else(|| "path has no file name".to_owned())?,
        )
    } else {
        return Err(format!("path not found: {raw}"));
    };
    if !checked.starts_with(&root) {
        return Err("path escapes the active scope".into());
    }
    Ok(checked)
}

fn edit(root: &Path, action: &str, args: &Map<String, Value>) -> Result<Value, String> {
    let raw = aliased_path(args, &["path", "file_path"])?;
    if raw.is_empty() {
        return Err("path is required".into());
    }
    let create = matches!(action, "write" | "mkdir");
    let path = resolve(root, raw, create)?;
    if let Some(expected) = args.get("expected_sha256").and_then(Value::as_str) {
        let actual = format!(
            "{:x}",
            Sha256::digest(std::fs::read(&path).map_err(|error| error.to_string())?)
        );
        if actual != expected {
            return Err("expected_sha256 does not match current file".into());
        }
    }
    match action {
        "write" => cccc_core::fs::atomic_write_preserving_mode(
            &path,
            text_arg(args, "content")?.as_bytes(),
        )
        .map_err(|error| error.to_string())?,
        "mkdir" => {
            let exist_ok = args
                .get("exist_ok")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            match std::fs::create_dir(&path) {
                Ok(()) => {}
                Err(error)
                    if exist_ok
                        && error.kind() == std::io::ErrorKind::AlreadyExists
                        && path.is_dir() => {}
                Err(error) => return Err(error.to_string()),
            }
        }
        "delete" if path.is_dir() => {
            std::fs::remove_dir(&path).map_err(|error| error.to_string())?
        }
        "delete" => std::fs::remove_file(&path).map_err(|error| error.to_string())?,
        "move" => {
            let destination = aliased_path(args, &["dest_path", "to_path", "new_path"])?;
            if destination.is_empty() {
                return Err("dest_path is required".into());
            }
            let target = resolve(root, destination, true)?;
            std::fs::rename(&path, target).map_err(|error| error.to_string())?;
        }
        "replace" | "multi_replace" => {
            let original = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            if let Some(expected) = args.get("expected_sha256").and_then(Value::as_str)
                && format!("{:x}", Sha256::digest(original.as_bytes())) != expected
            {
                return Err("expected_sha256 does not match current file".into());
            }
            let mut text = original.clone();
            if action == "replace" {
                text = replace_text(&text, args)?;
            } else {
                let replacements = args
                    .get("replacements")
                    .and_then(Value::as_array)
                    .filter(|items| !items.is_empty())
                    .ok_or("replacements must be a nonempty array")?;
                for item in replacements {
                    text = replace_text(
                        &text,
                        item.as_object()
                            .ok_or("each replacement must be an object")?,
                    )?;
                }
            }
            if std::fs::read_to_string(&path).map_err(|e| e.to_string())? != original {
                return Err("file changed while preparing replacements".into());
            }
            cccc_core::fs::atomic_write_preserving_mode(&path, text.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        _ => {}
    }
    Ok(json!({"action":action,"path":path,"ok":true}))
}

fn replace_text(text: &str, args: &Map<String, Value>) -> Result<String, String> {
    let old = required(args, "old_text")?;
    let new = text_arg(args, "new_text")?;
    let count = text.matches(old).count();
    if let Some(expected) = args.get("expected_replacements") {
        let expected = expected
            .as_u64()
            .filter(|n| (1..=10000).contains(n))
            .ok_or("expected_replacements must be an integer between 1 and 10000")?;
        if count as u64 != expected {
            return Err(format!("expected {expected} matches, found {count}"));
        }
    }
    let all = match args.get("replace_all") {
        Some(value) => value.as_bool().ok_or("replace_all must be a boolean")?,
        None => false,
    };
    if count == 0 || (!all && count != 1) {
        return Err("old_text must match exactly once unless replace_all is true".into());
    }
    Ok(if all {
        text.replace(old, new)
    } else {
        text.replacen(old, new, 1)
    })
}

fn string<'a>(args: &'a Map<String, Value>, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("")
}
fn text_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be a string"))
}
fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    let value = string(args, key);
    if value.is_empty() {
        Err(format!("{key} is required"))
    } else {
        Ok(value)
    }
}

// Read and edit accept the same public path spelling; disagreement is never guessed.
pub(super) fn aliased_path<'a>(
    args: &'a Map<String, Value>,
    keys: &[&str],
) -> Result<&'a str, String> {
    let mut selected = None;
    for &key in keys {
        if let Some(raw) = args.get(key) {
            let value = raw
                .as_str()
                .ok_or_else(|| format!("{key} must be a string"))?;
            if selected.is_some_and(|previous| previous != value) {
                return Err(format!(
                    "{} must agree when supplied together",
                    keys.join(" and ")
                ));
            }
            selected = Some(value);
        }
    }
    Ok(selected.unwrap_or(""))
}
