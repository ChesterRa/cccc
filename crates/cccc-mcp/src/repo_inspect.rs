//! Bounded, scope-aware inspection shared by direct and code-mode MCP calls.
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::RegexBuilder;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const SCAN_ENTRIES: usize = 10_000;
const SEARCH_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn call(root: &Path, action: &str, args: &Map<String, Value>) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let raw = crate::repo::aliased_path(args, &["path", "file_path"])?;
    if action == "read" && raw.is_empty() {
        return Err("path is required".into());
    }
    let path = crate::repo::resolve(&root, raw, false)?;
    match action {
        "read" => read(&path, args),
        "search" => search(&root, &path, args),
        _ => list(&root, &path, action, args),
    }
}

fn number(
    args: &Map<String, Value>,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    match args.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| (min..=max).contains(v))
            .ok_or_else(|| format!("{key} must be an integer between {min} and {max}")),
    }
}

fn boolean(args: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    args.get(key).map_or(Ok(default), |v| {
        v.as_bool()
            .ok_or_else(|| format!("{key} must be a boolean"))
    })
}

fn budget(args: &Map<String, Value>) -> Result<usize, String> {
    number(args, "max_bytes", 200_000, 1, 1_000_000)
}

fn regular_file(path: &Path) -> Result<File, String> {
    if !fs::metadata(path).map_err(|e| e.to_string())?.is_file() {
        return Err("expected a regular file".into());
    }
    File::open(path).map_err(|e| e.to_string())
}

fn read(path: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let max = budget(args)?;
    let start = number(args, "start_line", 1, 1, usize::MAX)?;
    let requested_end = number(args, "end_line", usize::MAX, 1, usize::MAX)?;
    if requested_end < start {
        return Err("end_line must be at least start_line".into());
    }
    let mut file = regular_file(path)?;
    let initial = file.metadata().map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    let mut content = Vec::new();
    let mut line = 1_usize;
    let mut selected_bytes = 0_usize;
    let mut bytes_read = 0_u64;
    let mut last = None;
    // Hash the entire original file, but retain only the bounded selected range.
    // Do not chase an actively growing log indefinitely; detect growth below.
    let mut reader = (&mut file).take(initial.len().saturating_add(1));
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let chunk = &buffer[..n];
        if chunk.contains(&0) {
            return Err("file contains binary data; use a binary-aware command".into());
        }
        bytes_read += n as u64;
        hash.update(chunk);
        for &byte in chunk {
            if (start..=requested_end).contains(&line) {
                selected_bytes += 1;
                if content.len() < max {
                    content.push(byte);
                }
            }
            if byte == b'\n' {
                line += 1;
            }
            last = Some(byte);
        }
    }
    let final_metadata = file.metadata().map_err(|e| e.to_string())?;
    if bytes_read != initial.len()
        || final_metadata.len() != initial.len()
        || final_metadata.modified().ok() != initial.modified().ok()
    {
        return Err("file changed while reading; read again before editing".into());
    }
    let total_lines = line - usize::from(last.is_none() || last == Some(b'\n'));
    if let Err(e) = std::str::from_utf8(&content) {
        if e.error_len().is_none() && selected_bytes > content.len() {
            content.truncate(e.valid_up_to());
        } else {
            return Err("requested content is not UTF-8 text".into());
        }
    }
    if content.is_empty() && selected_bytes != 0 {
        return Err("max_bytes is too small for the first UTF-8 character; increase it".into());
    }
    let truncated = selected_bytes > content.len();
    let newline_count = content.iter().filter(|b| **b == b'\n').count();
    let partial_last_line = truncated && content.last().is_some_and(|b| *b != b'\n');
    let next_line = start.saturating_add(newline_count);
    let end = if content.is_empty() {
        start.saturating_sub(1).min(total_lines)
    } else {
        next_line - usize::from(content.last() == Some(&b'\n'))
    };
    // Retain the existing read text convention: LF-separated lines, no final LF.
    let text = std::str::from_utf8(&content)
        .map_err(|e| e.to_string())?
        .lines()
        .collect::<Vec<_>>()
        .join("\n");
    Ok(
        json!({"path":path,"content":text,"start_line":start,"end_line":end,
        "total_lines":total_lines,"sha256":format!("{:x}",hash.finalize()),"truncated":truncated,
        "next_start_line":if truncated { Some(next_line) } else { None },"partial_last_line":partial_last_line}),
    )
}

fn relative_name(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

struct Filters {
    hidden: bool,
    include: GlobSet,
    exclude: GlobSet,
}

impl Filters {
    fn new(args: &Map<String, Value>) -> Result<Self, String> {
        fn globs(args: &Map<String, Value>, key: &str) -> Result<GlobSet, String> {
            let mut builder = GlobSetBuilder::new();
            if let Some(raw) = args.get(key) {
                for value in raw
                    .as_array()
                    .ok_or_else(|| format!("{key} must be an array"))?
                {
                    let pattern = value
                        .as_str()
                        .filter(|p| !p.is_empty())
                        .ok_or_else(|| format!("{key} must contain nonempty strings"))?;
                    // Portable shell-style glob paths, always relative to the scope.
                    let glob = GlobBuilder::new(pattern)
                        .literal_separator(true)
                        .backslash_escape(true)
                        .build()
                        .map_err(|e| format!("invalid {key}: {e}"))?;
                    builder.add(glob);
                }
            }
            builder.build().map_err(|e| e.to_string())
        }
        Ok(Self {
            hidden: boolean(args, "include_hidden", false)?,
            include: globs(args, "include_globs")?,
            exclude: globs(args, "exclude_globs")?,
        })
    }
    fn excluded(&self, path: &Path) -> bool {
        self.exclude.is_match(path)
    }
    fn included(&self, path: &Path) -> bool {
        self.include.is_empty() || self.include.is_match(path)
    }
}

struct Entry {
    path: PathBuf,
    kind: &'static str,
}
struct Scan {
    entries: Vec<Entry>,
    truncated: bool,
    unreadable: usize,
}

// Never follow discovered symlinks. Explicit paths still use normal scope validation.
// Iterative traversal avoids cycles/deep-stack failures and shares ordering for paging.
fn scan(root: &Path, path: &Path, depth: usize, filters: &Filters) -> Result<Scan, String> {
    let mut result = Scan {
        entries: Vec::new(),
        truncated: false,
        unreadable: 0,
    };
    let mut pending = vec![(path.to_owned(), 0)];
    let mut visited = 0;
    while let Some((directory, level)) = pending.pop() {
        if level > 0
            && (fs::symlink_metadata(&directory).map_or(true, |m| m.file_type().is_symlink())
                || directory
                    .canonicalize()
                    .map_or(true, |p| !p.starts_with(root)))
        {
            result.unreadable += 1;
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(e) if level == 0 => return Err(e.to_string()),
            Err(_) => {
                result.unreadable += 1;
                continue;
            }
        };
        for item in entries {
            if visited >= SCAN_ENTRIES {
                result.truncated = true;
                break;
            }
            visited += 1;
            let Ok(entry) = item else {
                result.unreadable += 1;
                continue;
            };
            let name = entry.file_name();
            if !filters.hidden && name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            if filters.excluded(relative) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                result.unreadable += 1;
                continue;
            };
            let kind = if kind.is_symlink() {
                "symlink"
            } else if kind.is_dir() {
                "dir"
            } else if kind.is_file() {
                "file"
            } else {
                "other"
            };
            if kind == "dir"
                && level + 1 < depth
                && !matches!(name.to_str(), Some(".git" | "target" | "node_modules"))
            {
                pending.push((path.clone(), level + 1));
            }
            if filters.included(relative) {
                result.entries.push(Entry { path, kind });
            }
        }
        if result.truncated {
            break;
        }
    }
    result.entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}

fn list(
    root: &Path,
    path: &Path,
    action: &str,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let filters = Filters::new(args)?;
    let depth = number(args, "depth", 2, 1, 8)?;
    let depth = if action == "list" { 1 } else { depth };
    let max = budget(args)?;
    let limit = number(args, "limit", 200, 1, 500)?;
    let offset = number(args, "offset", 1, 1, usize::MAX)?;
    let scan = scan(root, path, depth, &filters)?;
    let mut entries = Vec::new();
    let mut size = 0;
    let mut more = false;
    for entry in scan.entries.iter().skip(offset - 1) {
        let item = json!({"name":entry.path.file_name().map(|n|n.to_string_lossy()),"path":relative_name(entry.path.strip_prefix(root).map_err(|e|e.to_string())?),"kind":entry.kind});
        let bytes = serde_json::to_vec(&item).map_err(|e| e.to_string())?.len();
        if entries.len() == limit || size + bytes > max {
            if entries.is_empty() {
                return Err("max_bytes is too small for a directory entry; increase it".into());
            }
            more = true;
            break;
        }
        size += bytes;
        entries.push(item);
    }
    let next = if more && !scan.truncated {
        Some(offset.saturating_add(entries.len()))
    } else {
        None
    };
    Ok(
        json!({"path":path,"entries":entries,"offset":offset,"next_offset":next,
        "truncated":more||scan.truncated,"scan_truncated":scan.truncated,"unreadable":scan.unreadable,
        "incomplete":more||scan.truncated||scan.unreadable>0}),
    )
}

fn search(root: &Path, path: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .filter(|q| !q.is_empty())
        .ok_or("query is required")?;
    let pattern = if boolean(args, "regex", false)? {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    let matcher = RegexBuilder::new(&pattern)
        .case_insensitive(!boolean(args, "case_sensitive", false)?)
        .build()
        .map_err(|e| e.to_string())?;
    let filters = Filters::new(args)?;
    let limit = number(args, "limit", 200, 1, 500)?;
    let max = budget(args)?;
    let file_max = number(args, "max_file_bytes", 200_000, 1, 1_000_000)?;
    let context = number(args, "context_lines", 0, 0, 10)?;
    let scan = if path.is_dir() {
        scan(root, path, usize::MAX, &filters)?
    } else {
        Scan {
            entries: vec![Entry {
                path: path.to_owned(),
                kind: "file",
            }],
            truncated: false,
            unreadable: 0,
        }
    };
    let mut hits = Vec::new();
    let mut bytes = 0;
    let mut read_bytes = 0;
    let mut oversized = 0;
    let mut non_text = 0;
    let mut unreadable = scan.unreadable;
    let mut reason = if scan.truncated {
        Some("scan_entries")
    } else {
        None
    };
    for entry in scan.entries {
        if entry.kind != "file" {
            continue;
        }
        let relative = entry.path.strip_prefix(root).map_err(|e| e.to_string())?;
        if filters.excluded(relative) || !filters.included(relative) {
            continue;
        }
        // Revalidate containment before opening a file discovered by traversal.
        let checked = match crate::repo::resolve(root, &relative.to_string_lossy(), false) {
            Ok(p) => p,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let file = match regular_file(&checked) {
            Ok(f) => f,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        if size > file_max as u64 {
            oversized += 1;
            continue;
        }
        if read_bytes + file_max + 1 > SEARCH_BYTES {
            reason = Some("scan_bytes");
            break;
        }
        let mut data = Vec::new();
        if file
            .take(file_max as u64 + 1)
            .read_to_end(&mut data)
            .is_err()
        {
            unreadable += 1;
            continue;
        }
        read_bytes += data.len();
        if data.len() > file_max {
            oversized += 1;
            continue;
        }
        let Ok(text) = std::str::from_utf8(&data) else {
            non_text += 1;
            continue;
        };
        if text.contains('\0') {
            non_text += 1;
            continue;
        }
        let lines = text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if !matcher.is_match(line) {
                continue;
            }
            if hits.len() == limit {
                reason = Some("limit");
                break;
            }
            let mut hit = json!({"path":relative_name(relative),"line":index+1,"text":line});
            if context > 0 {
                hit["before"] = json!(&lines[index.saturating_sub(context)..index]);
                hit["after"] = json!(&lines[index + 1..(index + context + 1).min(lines.len())]);
            }
            let size = serde_json::to_vec(&hit).map_err(|e| e.to_string())?.len();
            if bytes + size > max {
                if hits.is_empty() {
                    return Err(format!(
                        "max_bytes is too small for a hit at {}:{}; increase it, reduce context_lines, or read that line directly",
                        relative.display(),
                        index + 1
                    ));
                }
                reason = Some("max_bytes");
                break;
            }
            bytes += size;
            hits.push(hit);
        }
        if matches!(reason, Some("limit" | "max_bytes")) {
            break;
        }
    }
    Ok(
        json!({"hits":hits,"truncated":reason.is_some(),"truncated_reason":reason,
        "incomplete":reason.is_some()||oversized>0||unreadable>0,
        "skipped_files":{"oversized":oversized,"non_text":non_text,"unreadable":unreadable},
        "scanned_bytes":read_bytes}),
    )
}

#[cfg(test)]
#[path = "repo_inspect_tests.rs"]
mod tests;
