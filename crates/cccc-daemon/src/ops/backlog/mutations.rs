use super::*;
use cccc_core::backlog::{Document, Selector};

const TEMPLATE: &str = "# Backlog\n\n## Open\n\n## Closed\n";

pub(super) fn configure(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let mut group = state::required_group(home, request)?;
    permissions::require_group(&group, &by(request))
        .map_err(|e| OpError::new("permission_denied", e.to_string()))?;
    let path = if request.op == "backlog_exclude" {
        selected_path(&group, request)?
    } else {
        normalized_path(&group, &required_arg(request, "path")?)?
    };
    if request.op == "backlog_exclude" {
        let changed = git::exclude(&group, &path)?;
        let sha = state::read(&group, &path)
            .map(|file| file.sha256)
            .unwrap_or_default();
        if changed {
            event(home, &group, request, "backlog.exclude", &path, &sha, None)
                .map_err(|e| partial(e, json!({"excluded":true})))?;
        }
        return state::snapshot(home, &group).map_err(|e| partial(e, json!({"excluded":true})));
    }
    let mut applied = json!({});
    let sha = if request.op == "backlog_create" {
        let root = workspace::root(&group).map_err(file_error)?;
        let target = workspace::resolve_for_create(&root, &path).map_err(file_error)?;
        if target.symlink_metadata().is_ok() {
            return Err(OpError::new(
                "backlog_file_conflict",
                "File already exists; use the existing file instead",
            ));
        }
        let keep = match request.args.get("keep_local") {
            None => true,
            Some(Value::Bool(value)) => *value,
            _ => return Err(OpError::invalid("keep_local must be boolean")),
        };
        if keep && git::facts(&group, "")?["repository"] == true {
            git::exclude(&group, &path)?;
            applied["excluded"] = json!(true);
        }
        let sha = persist(&group, &path, TEMPLATE, "").map_err(|e| {
            if applied.as_object().is_some_and(|v| !v.is_empty()) {
                partial(e, applied.clone())
            } else {
                e
            }
        })?;
        applied["file"] = json!({"path":path,"sha256":sha});
        sha
    } else {
        state::read(&group, &path)?.sha256
    };
    let key = group.active_scope_key.clone();
    let url = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == key)
        .expect("scope")
        .url
        .clone();
    store(home)?
        .mutate(&group.group_id, |doc| {
            if !doc
                .scopes
                .iter()
                .any(|scope| scope.scope_key == key && scope.url == url)
            {
                return Err(std::io::Error::other("scope_conflict"));
            }
            let settings = doc
                .extra
                .entry("backlogs")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or_else(|| std::io::Error::other("invalid backlog settings"))?;
            settings.insert(key.clone(), json!({"path":path}));
            Ok(())
        })
        .map_err(|e| partial(OpError::io(e), applied.clone()))?;
    group.extra.entry("backlogs").or_insert_with(|| json!({}))[&key] = json!({"path":path});
    applied["configured"] = json!(true);
    let op = if request.op == "backlog_create" {
        "backlog.create"
    } else {
        "backlog.use"
    };
    event(home, &group, request, op, &path, &sha, None).map_err(|e| partial(e, applied.clone()))?;
    state::snapshot(home, &group).map_err(|e| partial(e, applied))
}

pub(super) fn selector(request: &DaemonRequest) -> Result<Selector, OpError> {
    let value = request
        .args
        .get("entry")
        .and_then(Value::as_object)
        .filter(|v| v.len() == 1)
        .ok_or_else(|| OpError::invalid("entry must have exactly id or line"))?;
    if let Some(id) = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| cccc_contracts::backlog::valid_entry_id(id))
    {
        Ok(Selector::Id(id.into()))
    } else if let Some(line) = value
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
    {
        Ok(Selector::Line(line))
    } else {
        Err(OpError::invalid(
            "entry requires a 12hex id or positive heading line",
        ))
    }
}

pub(super) fn edit(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group = state::required_group(home, request)?;
    let path = selected_path(&group, request)?;
    let file = state::read(&group, &path)?;
    let expected = expected(request, request.op == "backlog_add", &file.sha256)?;
    let doc = Document::parse(&file.content);
    let section = || {
        request
            .args
            .get("section")
            .and_then(Value::as_str)
            .ok_or_else(|| OpError::invalid("section is required (empty means the preamble)"))
    };
    let edited = match request.op.as_str() {
        "backlog_add" => doc.add(section()?, text(request, "title")?, text(request, "body")?),
        "backlog_edit" => doc.edit(
            &selector(request)?,
            text(request, "title")?,
            text(request, "body")?,
        ),
        "backlog_move" => doc.move_entry(&selector(request)?, section()?),
        "backlog_close" => doc.close(
            &selector(request)?,
            text(request, "outcome")?,
            &chrono::Local::now().format("%Y-%m-%d").to_string(),
        ),
        _ => unreachable!(),
    }
    .map_err(entry_error)?;
    let sha = persist(&group, &path, &edited.content, &expected)?;
    let applied = file_applied(&path, &sha, Some(&edited.entry_id));
    event(
        home,
        &group,
        request,
        &request.op.replace('_', "."),
        &path,
        &sha,
        Some(&edited.entry_id),
    )
    .map_err(|e| partial(e, applied.clone()))?;
    let mut result = state::snapshot(home, &group).map_err(|e| partial(e, applied))?;
    result.insert("entry_id".into(), json!(edited.entry_id));
    Ok(result)
}

pub(super) fn expected(
    request: &DaemonRequest,
    optional: bool,
    current: &str,
) -> Result<String, OpError> {
    let value = match request.args.get("if_file_sha256") {
        None if optional => return Ok(current.into()),
        Some(Value::String(value))
            if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            value
        }
        _ => return Err(OpError::invalid("if_file_sha256 must be a SHA256 digest")),
    };
    if value != current {
        return Err(conflict(current));
    }
    Ok(value.into())
}

fn conflict(current: &str) -> OpError {
    let mut error = OpError::new(
        "backlog_file_conflict",
        "File changed; reload before writing",
    );
    error.details.insert("sha256".into(), json!(current));
    error
}

pub(super) fn persist(
    group: &GroupDoc,
    path: &str,
    content: &str,
    expected: &str,
) -> Result<String, OpError> {
    if content.len() > workspace::MAX_READ_BYTES as usize {
        return Err(OpError::new(
            "backlog_invalid_file",
            "Backlog exceeds 1 MiB",
        ));
    }
    if content.contains('\0') {
        return Err(OpError::new(
            "backlog_invalid_file",
            "Backlog must remain text without NUL bytes",
        ));
    }
    match workspace::write_file(group, path, content, expected).map_err(|error| {
        if error
            .get_ref()
            .is_some_and(|cause| cause.is::<workspace::OutsideScope>())
        {
            file_error(error)
        } else {
            // An I/O failure can follow rename (for example directory fsync).
            // Without a success receipt, do not tell the caller it wrote nothing.
            partial(OpError::io(error), json!({}))
        }
    })? {
        workspace::WriteOutcome::Written { sha256, .. } => Ok(sha256),
        workspace::WriteOutcome::Conflict { sha256 } => Err(conflict(&sha256)),
    }
}

fn text<'a>(request: &'a DaemonRequest, key: &str) -> Result<&'a str, OpError> {
    request
        .args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| OpError::invalid(format!("{key} must be a string")))
}

pub(super) fn entry_error(error: std::io::Error) -> OpError {
    let message = error.to_string();
    let code = if message.starts_with("ambiguous") {
        "backlog_ambiguous"
    } else {
        "backlog_invalid_entry"
    };
    let mut error = OpError::new(code, &message);
    error.details.insert("reason".into(), json!(message));
    error
}
