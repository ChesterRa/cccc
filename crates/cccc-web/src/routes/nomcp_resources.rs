use cccc_core::nomcp::Session;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn root(home: &HomeLayout, session: &Session) -> Result<PathBuf, String> {
    if !session.repo_root.trim().is_empty() {
        let root = PathBuf::from(session.repo_root.trim());
        return root
            .is_dir()
            .then_some(root)
            .ok_or_else(|| "session scope is unavailable".into());
    }
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(&session.group_id))
        .map_err(|error| error.to_string())?;
    group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == session.scope_key)
        .map(|scope| PathBuf::from(&scope.url))
        .filter(|path| path.is_dir())
        .ok_or_else(|| "session scope is unavailable".into())
}

pub fn resources(root: &Path, session: &Session) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    walk(root, root, session, &mut files, 5_000)?;
    files.sort();
    Ok(files)
}

pub fn read(
    root: &Path,
    session: &Session,
    raw: &str,
    start: usize,
    end: usize,
) -> Result<Value, ResourceError> {
    let path = checked(root, session, raw)?;
    let bytes = std::fs::read(&path).map_err(|error| ResourceError::bad(error.to_string()))?;
    if bytes.contains(&0) {
        return Err(ResourceError::binary("binary files are not available"));
    }
    let text = String::from_utf8(bytes).map_err(|_| ResourceError::binary("file is not UTF-8"))?;
    let lines: Vec<_> = text.lines().collect();
    let start = start.max(1).min(lines.len().max(1));
    let requested_end = if end == 0 {
        lines.len()
    } else {
        end.min(lines.len())
    };
    let end = requested_end.min(start.saturating_add(499));
    let content = if lines.is_empty() || start > end {
        String::new()
    } else {
        lines[start - 1..end].join("\n")
    };
    Ok(
        json!({"path":raw,"content":content,"start":start,"end":end,"total_lines":lines.len(),"truncated":end<requested_end}),
    )
}

pub fn search(root: &Path, session: &Session, query: &str) -> Result<Value, String> {
    let mut matches = Vec::new();
    for relative in resources(root, session)? {
        let Ok(value) = read(root, session, &relative, 1, 500) else {
            continue;
        };
        if let Some(content) = value.get("content").and_then(Value::as_str) {
            for (index, line) in content.lines().enumerate() {
                if line.contains(query) {
                    matches.push(json!({"path":relative,"line":index+1,"text":line}));
                    if matches.len() >= 200 {
                        return Ok(json!({"matches":matches,"truncated":true}));
                    }
                }
            }
        }
    }
    Ok(json!({"matches":matches,"truncated":false}))
}

fn checked(root: &Path, session: &Session, raw: &str) -> Result<PathBuf, ResourceError> {
    let relative = Path::new(raw);
    if relative.is_absolute()
        || relative
            .components()
            .any(|item| matches!(item, std::path::Component::ParentDir))
    {
        return Err(ResourceError::bad("invalid relative path"));
    }
    if denied(raw) || !allowed(session, raw) {
        return Err(ResourceError::denied("path is not allowed by this session"));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| ResourceError::bad(error.to_string()))?;
    let path = canonical_root
        .join(relative)
        .canonicalize()
        .map_err(|error| ResourceError::bad(error.to_string()))?;
    if !path.starts_with(&canonical_root) {
        return Err(ResourceError::denied("path escapes the session scope"));
    }
    Ok(path)
}

fn allowed(session: &Session, raw: &str) -> bool {
    let allowed = if session.allowed_paths.is_empty() {
        vec!["README.md", "PROJECT.md", "docs", "src", "tests", "web/src"]
    } else {
        session.allowed_paths.iter().map(String::as_str).collect()
    };
    allowed
        .iter()
        .any(|prefix| raw == *prefix || raw.starts_with(&format!("{prefix}/")))
}

fn denied(raw: &str) -> bool {
    raw.split('/').any(|part| {
        part.starts_with('.') || matches!(part, "target" | "node_modules" | "__pycache__")
    })
}

fn walk(
    root: &Path,
    path: &Path,
    session: &Session,
    files: &mut Vec<String>,
    limit: usize,
) -> Result<(), String> {
    for entry in std::fs::read_dir(path)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
    {
        if files.len() >= limit {
            break;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if denied(&relative) || !allowed(session, &relative) && path.is_file() {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, session, files, limit)?;
        } else if path.is_file() && allowed(session, &relative) {
            files.push(relative);
        }
    }
    Ok(())
}

pub struct ResourceError {
    pub status: axum::http::StatusCode,
    pub message: String,
}
impl ResourceError {
    fn bad(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    fn denied(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }
    fn binary(message: impl Into<String>) -> Self {
        Self {
            status: axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            message: message.into(),
        }
    }
}
