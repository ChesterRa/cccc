//! Host-mediated reads retain the task's original Group/scope and exclude private
//! runtime state. These bounds do not sandbox the provider's native tools.
use super::*;
use cccc_core::{GroupDoc, GroupStore, workspace};
use std::io::Read;
use std::path::{Component, Path};

const MAX_TEXT: usize = 32_000;

pub(super) fn read(home: &HomeLayout, task: &SecretaryTask, request: &DaemonRequest) -> OpResult {
    let resource = required_arg(request, "resource")?;
    let mut group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(&task.target.group_id)
        .map_err(OpError::io)?;
    group.active_scope_key = task.target.scope_key.clone();
    let path = string_arg(request, "path").unwrap_or_default();
    if path.len() > 1024 {
        return Err(OpError::new(
            "invalid_args",
            "path is limited to 1,024 bytes",
        ));
    }
    let body = match resource.as_str() {
        "messages" => {
            let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
            let (messages, more) = cccc_core::ledger::tail_filtered(
                &store.ledger_path(&group.group_id).map_err(OpError::io)?,
                300,
                Some("chat.message"),
            )
            .map_err(OpError::io)?;
            let mut remaining = MAX_TEXT;
            let mut items = Vec::new();
            for event in messages
                .iter()
                .rev()
                .filter(|e| e.scope_key.is_empty() || e.scope_key == task.target.scope_key)
                .take(30)
            {
                if remaining == 0 {
                    break;
                }
                let text = event.data.get("text").and_then(Value::as_str).unwrap_or("");
                let text = text.chars().take(remaining.min(4000)).collect::<String>();
                remaining = remaining.saturating_sub(text.chars().count());
                items.push(json!({"id":event.id,"by":event.by,"ts":event.ts,"scope_key":event.scope_key,"text":text}));
            }
            json!({"messages":items,"limit":30,"bounded":true,"older_messages_omitted":more || messages.len()>30})
        }
        "document" => {
            if task.target.document_id.is_empty() || !path.is_empty() {
                return Err(OpError::new(
                    "invalid_args",
                    "document reads the registered reference fixed at acceptance; no path override",
                ));
            }
            let file = super::super::assistants::secretary_document_file(home, task)
                .map_err(OpError::io)?;
            let mut bytes = Vec::new();
            std::fs::File::open(file)
                .map_err(OpError::io)?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(OpError::io)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(OpError::new(
                    "invalid_args",
                    "Reference document exceeds 4 MiB",
                ));
            }
            let text = String::from_utf8(bytes).map_err(OpError::invalid)?;
            json!({"document_id":task.target.document_id,"document_path":task.target.document_path,
                "content":text.chars().take(MAX_TEXT).collect::<String>(),"truncated":text.chars().count()>MAX_TEXT,
                "sha256":digest(text.as_bytes())})
        }
        "files" => {
            let root = workspace::root(&group).map_err(OpError::io)?;
            guard(home, &root, &path).map_err(OpError::invalid)?;
            let listing = workspace::list(&group, &path, workspace::ListOptions::default())
                .map_err(OpError::io)?;
            let count = listing.items.len();
            let entries = listing.items.into_iter().filter(|entry| permitted(Path::new(&entry.path)))
                .take(200).map(|entry| json!({"path":entry.path,"is_dir":entry.is_dir,"size":entry.size,"unavailable":entry.unavailable})).collect::<Vec<_>>();
            json!({"path":path,"entries":entries,"truncated":count>200})
        }
        "file" => {
            let root = workspace::root(&group).map_err(OpError::io)?;
            guard(home, &root, &path).map_err(OpError::invalid)?;
            let mut file = workspace::read_file(&group, &path).map_err(OpError::io)?;
            if file.content.chars().count() > MAX_TEXT {
                file.content = file.content.chars().take(MAX_TEXT).collect();
                file.truncated = true;
            }
            let mut value = serde_json::to_value(file).map_err(OpError::invalid)?;
            value.as_object_mut().expect("file").remove("scope_url");
            value
        }
        "search" => Value::Object(search(
            home,
            &group,
            &required_arg(request, "query")?,
            &path,
        )?),
        _ => {
            return Err(OpError::new(
                "invalid_args",
                "resource must be messages, document, files, file or search",
            ));
        }
    };
    object(
        json!({"group_id":task.target.group_id,"scope_key":task.target.scope_key,
        "read_at":cccc_contracts::utc_now(),"resource":resource,"result":body}),
    )
}

fn permitted(path: &Path) -> bool {
    path.components().all(|part| match part {
        Component::Normal(name) => {
            let name = name.to_string_lossy().to_ascii_lowercase();
            (!name.starts_with('.')
                || matches!(name.as_str(), ".github" | ".gitignore" | ".editorconfig"))
                && !matches!(
                    name.as_str(),
                    "node_modules" | "target" | "dist" | "auth.json" | "credentials.json"
                )
                && !name.ends_with(".pem")
                && !name.ends_with(".key")
        }
        _ => false,
    })
}

fn guard(home: &HomeLayout, root: &Path, relative: &str) -> io::Result<()> {
    if root.starts_with(home.root().canonicalize()?) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Runtime state is outside Secretary read authority",
        ));
    }
    if relative.is_empty() {
        return Ok(());
    }
    let path = workspace::resolve_existing(root, relative)?;
    if path.starts_with(home.root().canonicalize()?) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Runtime state is outside Secretary read authority",
        ));
    }
    if !permitted(Path::new(relative)) || !path.strip_prefix(root).is_ok_and(permitted) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Hidden/private files and generated directories are outside Secretary read authority",
        ));
    }
    Ok(())
}

fn search(home: &HomeLayout, group: &GroupDoc, query: &str, start: &str) -> OpResult {
    if query.trim().is_empty() || query.chars().count() > 200 {
        return Err(OpError::new(
            "invalid_args",
            "query must contain 1–200 characters; search uses literal text",
        ));
    }
    let root = workspace::root(group).map_err(OpError::io)?;
    guard(home, &root, start).map_err(OpError::invalid)?;
    let mut directories = std::collections::VecDeque::from([start.to_owned()]);
    let mut matches = Vec::new();
    let mut visited = 0;
    let mut files = 0;
    let mut output = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while let Some(directory) = directories.pop_front() {
        if visited >= 64
            || files >= 512
            || matches.len() >= 40
            || output >= MAX_TEXT
            || std::time::Instant::now() >= deadline
        {
            break;
        }
        visited += 1;
        let listing = workspace::list(group, &directory, workspace::ListOptions::default())
            .map_err(OpError::io)?;
        for entry in listing.items {
            if entry.is_symlink || entry.unavailable.is_some() || !permitted(Path::new(&entry.path))
            {
                continue;
            }
            if entry.is_dir {
                if directories.len() < 64 {
                    directories.push_back(entry.path);
                }
                continue;
            }
            if files >= 512 || matches.len() >= 40 || output >= MAX_TEXT {
                break;
            }
            files += 1;
            if entry.size.is_some_and(|size| size > 256 * 1024) {
                continue;
            }
            guard(home, &root, &entry.path).map_err(OpError::invalid)?;
            let file = workspace::read_file(group, &entry.path).map_err(OpError::io)?;
            for (index, line) in file.content.lines().enumerate() {
                if matches.len() >= 40 || output >= MAX_TEXT {
                    break;
                }
                if line.contains(query) {
                    let text = line.chars().take(800).collect::<String>();
                    output += text.chars().count();
                    matches.push(json!({"path":entry.path,"line":index+1,"text":text}));
                }
            }
        }
    }
    object(
        json!({"query":query,"matches":matches,"files_checked":files,"directories_checked":visited,
        "bounded":true,"limit_hint":"At most 64 directories, 512 files (256 KiB each), 40 matches. Narrow path/query for omitted material."}),
    )
}
