//! Exact, line-oriented Codex-style patches. Validate all sections before writes;
//! reject ambiguous unanchored edits rather than guessing or normalizing source.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

struct Change {
    source: PathBuf,
    destination: PathBuf,
    original: Option<Vec<u8>>,
    updated: Option<Vec<u8>>,
    names: Vec<String>,
}

pub(super) fn apply(root: &Path, patch: &str) -> Result<Vec<String>, String> {
    let lines: Vec<_> = patch.trim().lines().collect();
    if lines.first() != Some(&"*** Begin Patch") || lines.last() != Some(&"*** End Patch") {
        return Err(
            "Codex patch must start with *** Begin Patch and end with *** End Patch".into(),
        );
    }
    let mut index = 1;
    let mut changes = Vec::new();
    let mut touched = HashSet::new();
    while index < lines.len() - 1 {
        let header = lines[index];
        index += 1;
        let (kind, raw) = if let Some(path) = header.strip_prefix("*** Add File: ") {
            ("add", path)
        } else if let Some(path) = header.strip_prefix("*** Delete File: ") {
            ("delete", path)
        } else if let Some(path) = header.strip_prefix("*** Update File: ") {
            ("update", path)
        } else {
            return Err(format!("invalid Codex patch section: {header}"));
        };
        let source = resolve(root, raw, kind == "add")?;
        let original = if kind == "add" {
            if source.exists() {
                return Err(format!("file already exists: {raw}"));
            }
            None
        } else {
            Some(std::fs::read(&source).map_err(|e| format!("{raw}: {e}"))?)
        };
        let mut destination = source.clone();
        let mut names = vec![raw.to_owned()];
        if kind == "update" {
            if let Some(path) = lines[index].strip_prefix("*** Move to: ") {
                destination = resolve(root, path, true)?;
                if destination == source || destination.exists() {
                    return Err(format!("move destination already exists: {path}"));
                }
                names.push(path.to_owned());
                index += 1;
            }
        }
        let start = index;
        while index < lines.len() - 1 && !is_file_header(lines[index]) {
            if lines[index] == "*** End Patch" {
                return Err("unexpected End Patch".into());
            }
            index += 1;
        }
        let body = &lines[start..index];
        let updated = match kind {
            "add" => {
                let mut text = String::new();
                for line in body {
                    text.push_str(
                        line.strip_prefix('+')
                            .ok_or("added file lines must start with +")?,
                    );
                    text.push('\n');
                }
                Some(text.into_bytes())
            }
            "delete" => {
                if !body.is_empty() {
                    return Err("Delete File must not contain a hunk".into());
                }
                None
            }
            "update" => {
                let text = std::str::from_utf8(original.as_deref().unwrap_or_default())
                    .map_err(|_| format!("{raw}: patch requires UTF-8 text"))?;
                if body.is_empty() && destination == source {
                    return Err(format!("{raw}: update has no hunks"));
                }
                Some(
                    update(text, body)
                        .map_err(|e| format!("{raw}: {e}"))?
                        .into_bytes(),
                )
            }
            _ => unreachable!(),
        };
        for path in [&source, &destination].into_iter().collect::<HashSet<_>>() {
            if !touched.insert(path.clone()) {
                return Err(
                    "patch touches the same path in multiple sections; combine its hunks".into(),
                );
            }
        }
        changes.push(Change {
            source,
            destination,
            original,
            updated,
            names,
        });
    }
    if changes.is_empty() {
        return Err("patch has no file changes".into());
    }
    // Catch stale input before any file is changed. Individual replacements are
    // atomic; an OS failure during a multi-file commit is reported as partial.
    for change in &changes {
        check_current(change)?;
    }
    let mut applied = Vec::new();
    for change in changes {
        let result = commit(&change);
        if let Err(error) = result {
            return Err(format!(
                "patch failed for {}: {error}; already applied: {applied:?}. Inspect the workspace before retrying",
                change.names.join(" -> ")
            ));
        }
        applied.extend(change.names);
    }
    Ok(applied)
}

fn is_file_header(line: &str) -> bool {
    ["*** Add File: ", "*** Update File: ", "*** Delete File: "]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn resolve(root: &Path, raw: &str, create: bool) -> Result<PathBuf, String> {
    if raw.trim().is_empty() {
        return Err("patch path is required".into());
    }
    if !create {
        return crate::repo::resolve(root, raw, false);
    }
    // New nested files are common. Check the nearest existing ancestor before
    // atomic_write creates parents, including symlinks and '..' containment.
    let relative = Path::new(raw);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("path must be relative and remain inside the active scope".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let path = root.join(relative);
    let mut ancestor = path.as_path();
    while std::fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor.parent().ok_or("path has no existing ancestor")?;
    }
    let checked = ancestor.canonicalize().map_err(|e| e.to_string())?;
    if !checked.starts_with(&root) {
        return Err("path escapes the active scope".into());
    }
    if ancestor != path && !checked.is_dir() {
        return Err("patch parent is not a directory".into());
    }
    Ok(checked.join(path.strip_prefix(ancestor).map_err(|e| e.to_string())?))
}

fn check_current(change: &Change) -> Result<(), String> {
    match &change.original {
        Some(original)
            if std::fs::read(&change.source).map_err(|e| e.to_string())? != *original =>
        {
            return Err("file changed while preparing patch".into());
        }
        None if change.source.exists() => {
            return Err("new file appeared while preparing patch".into());
        }
        _ => {}
    }
    if change.destination != change.source && change.destination.exists() {
        return Err("move destination already exists".into());
    }
    Ok(())
}

fn commit(change: &Change) -> Result<(), String> {
    check_current(change)?;
    if let Some(bytes) = &change.updated {
        cccc_core::fs::atomic_write_preserving_mode(&change.destination, bytes)
            .map_err(|e| e.to_string())?;
        if change.destination != change.source {
            let permissions = std::fs::metadata(&change.source)
                .map_err(|e| e.to_string())?
                .permissions();
            std::fs::set_permissions(&change.destination, permissions)
                .map_err(|e| format!("destination written but permissions failed: {e}"))?;
            std::fs::remove_file(&change.source)
                .map_err(|e| format!("destination written but source removal failed: {e}"))?;
        }
    } else {
        std::fs::remove_file(&change.source).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Clone)]
struct Line<'a> {
    text: &'a str,
    ending: &'a str,
}

fn update<'a>(current: &'a str, body: &[&'a str]) -> Result<String, String> {
    let source: Vec<_> = current
        .split_inclusive('\n')
        .map(|line| {
            if let Some(text) = line.strip_suffix("\r\n") {
                Line {
                    text,
                    ending: "\r\n",
                }
            } else if let Some(text) = line.strip_suffix('\n') {
                Line { text, ending: "\n" }
            } else {
                Line {
                    text: line,
                    ending: "",
                }
            }
        })
        .collect();
    let newline = source
        .iter()
        .find(|line| !line.ending.is_empty())
        .map_or("\n", |line| line.ending);
    let trailing_newline = current.is_empty() || current.ends_with('\n');
    let mut output: Vec<Line<'_>> = Vec::new();
    let mut cursor = 0;
    let mut i = 0;
    while i < body.len() {
        let anchor = if body[i] == "@@" {
            None
        } else if let Some(anchor) = body[i].strip_prefix("@@ ").filter(|s| !s.is_empty()) {
            Some(anchor)
        } else {
            return Err("update hunks must begin with @@ or @@ exact context".into());
        };
        i += 1;
        let mut search_start = cursor;
        if let Some(anchor) = anchor {
            let candidates: Vec<_> = (cursor..source.len())
                .filter(|&n| source[n].text == anchor)
                .collect();
            if candidates.len() != 1 {
                return Err("hunk anchor must match exactly one remaining line".into());
            }
            search_start = candidates[0] + 1;
        }
        let start = i;
        let mut old = Vec::new();
        while i < body.len() && !body[i].starts_with("@@") && body[i] != "*** End of File" {
            match body[i].as_bytes().first() {
                Some(b' ' | b'-') => old.push(&body[i][1..]),
                Some(b'+') => {}
                _ => return Err("hunk lines must start with space, +, or -".into()),
            }
            i += 1;
        }
        if i == start {
            return Err("empty patch hunk".into());
        }
        let end = i;
        let eof = body.get(i) == Some(&"*** End of File");
        if eof {
            i += 1;
        }
        let at = if old.is_empty() {
            source.len()
        } else {
            let candidates: Vec<_> = (search_start..=source.len().saturating_sub(old.len()))
                .filter(|&n| {
                    n + old.len() <= source.len()
                        && (!eof || n + old.len() == source.len())
                        && source[n..n + old.len()]
                            .iter()
                            .map(|l| l.text)
                            .eq(old.iter().copied())
                })
                .collect();
            match candidates.as_slice() {
                [] => {
                    return Err(
                        "patch hunk context was not found after its anchor/previous hunk".into(),
                    );
                }
                [at] => *at,
                [at, ..] if anchor.is_some() => *at,
                _ => {
                    return Err(
                        "patch hunk context is ambiguous; add an exact @@ anchor or more context"
                            .into(),
                    );
                }
            }
        };
        if at < cursor {
            return Err("patch hunks overlap or are out of order".into());
        }
        output.extend_from_slice(&source[cursor..at]);
        let mut from = at;
        for line in &body[start..end] {
            match line.as_bytes()[0] {
                b' ' => {
                    output.push(source[from].clone());
                    from += 1;
                }
                b'-' => from += 1,
                b'+' => output.push(Line {
                    text: &line[1..],
                    ending: newline,
                }),
                _ => unreachable!(),
            }
        }
        cursor = from;
    }
    output.extend_from_slice(&source[cursor..]);
    let last = output.len().saturating_sub(1);
    let mut text = String::new();
    for (index, line) in output.into_iter().enumerate() {
        text.push_str(line.text);
        if index == last && !trailing_newline {
            continue;
        }
        text.push_str(if line.ending.is_empty() {
            newline
        } else {
            line.ending
        });
    }
    Ok(text)
}

#[cfg(test)]
#[path = "local_patch_tests.rs"]
mod tests;
