use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn call(root: &Path, action: &str, args: &Map<String, Value>) -> Result<Value, String> {
    match action {
        "info" => Ok(json!({"root":root,"git":root.join(".git").exists()})),
        "list" | "list_dir" => list(root, args),
        "read" => read(root, args),
        "search" => search(root, args),
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

fn list(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let path = resolve(root, string(args, "path"), false)?;
    let mut entries = std::fs::read_dir(&path)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| {
            let kind = entry.file_type().ok().map_or("unknown", |kind| {
                if kind.is_dir() {
                    "dir"
                } else if kind.is_file() {
                    "file"
                } else {
                    "other"
                }
            });
            json!({"name":entry.file_name().to_string_lossy(),"kind":kind})
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left["name"].as_str().cmp(&right["name"].as_str()));
    Ok(json!({"path":path,"entries":entries}))
}

fn read(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let path = resolve(root, required(args, "path")?, false)?;
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let lines: Vec<_> = text.lines().collect();
    let start = integer(args, "start_line", 1).max(1);
    let end = integer(args, "end_line", lines.len()).min(lines.len());
    let content = if start > end {
        String::new()
    } else {
        lines[start - 1..end].join("\n")
    };
    Ok(
        json!({"path":path,"content":content,"start_line":start,"end_line":end,"total_lines":lines.len()}),
    )
}

fn search(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let query = required(args, "query")?;
    let regex = args.get("regex").and_then(Value::as_bool).unwrap_or(false);
    let matcher = regex
        .then(|| Regex::new(query).map_err(|error| error.to_string()))
        .transpose()?;
    let mut files = Vec::new();
    walk(root, root, &mut files, 10_000)?;
    let mut hits = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            let matched = matcher
                .as_ref()
                .map_or_else(|| line.contains(query), |matcher| matcher.is_match(line));
            if matched {
                hits.push(json!({"path":path.strip_prefix(root).unwrap_or(&path),"line":index+1,"text":line}));
                if hits.len() >= 500 {
                    return Ok(json!({"hits":hits,"truncated":true}));
                }
            }
        }
    }
    Ok(json!({"hits":hits,"truncated":false}))
}

fn edit(root: &Path, action: &str, args: &Map<String, Value>) -> Result<Value, String> {
    let raw = required(args, "path")?;
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
        "write" => cccc_core::fs::atomic_write(&path, required(args, "content")?.as_bytes())
            .map_err(|error| error.to_string())?,
        "mkdir" => std::fs::create_dir(&path).map_err(|error| error.to_string())?,
        "delete" if path.is_dir() => {
            std::fs::remove_dir(&path).map_err(|error| error.to_string())?
        }
        "delete" => std::fs::remove_file(&path).map_err(|error| error.to_string())?,
        "move" => {
            let target = resolve(root, required(args, "new_path")?, true)?;
            std::fs::rename(&path, target).map_err(|error| error.to_string())?;
        }
        "replace" => replace(
            &path,
            required(args, "old_text")?,
            required(args, "new_text")?,
        )?,
        "multi_replace" => {
            for item in args
                .get("replacements")
                .and_then(Value::as_array)
                .ok_or("replacements is required")?
            {
                replace(
                    &path,
                    item.get("old_text")
                        .and_then(Value::as_str)
                        .ok_or("old_text is required")?,
                    item.get("new_text")
                        .and_then(Value::as_str)
                        .ok_or("new_text is required")?,
                )?;
            }
        }
        _ => {}
    }
    Ok(json!({"action":action,"path":path,"ok":true}))
}

fn replace(path: &Path, old: &str, new: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    if old.is_empty() || text.matches(old).count() != 1 {
        return Err("old_text must match exactly once".into());
    }
    cccc_core::fs::atomic_write(path, text.replacen(old, new, 1).as_bytes())
        .map_err(|error| error.to_string())
}

fn walk(root: &Path, path: &Path, files: &mut Vec<PathBuf>, limit: usize) -> Result<(), String> {
    for entry in std::fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
    {
        if files.len() >= limit {
            break;
        }
        let path = entry.path();
        if path.is_dir() {
            if !matches!(
                entry.file_name().to_str(),
                Some(".git" | "target" | "node_modules")
            ) {
                walk(root, &path, files, limit)?;
            }
        } else if path.is_file() && path.starts_with(root) {
            files.push(path);
        }
    }
    Ok(())
}

fn string<'a>(args: &'a Map<String, Value>, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("")
}
fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    let value = string(args, key);
    if value.is_empty() {
        Err(format!("{key} is required"))
    } else {
        Ok(value)
    }
}
fn integer(args: &Map<String, Value>, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}
