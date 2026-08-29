use super::*;
use sha2::{Digest, Sha256};

pub(super) fn ingest(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    if lane != "work" {
        return Err(OpError::new(
            "invalid_args",
            "group_space_ingest is supported only for the work lane",
        ));
    }
    let provider = provider(request);
    require_notebooklm(&provider)?;
    require_write_permission(home, &group_id, request)?;
    let kind = string_arg(request, "kind").unwrap_or_else(|| "context_sync".into());
    let payload = match request.args.get("payload") {
        None => Map::new(),
        Some(Value::Object(payload)) => payload.clone(),
        Some(_) => return Err(OpError::new("invalid_args", "payload must be an object")),
    };
    let input = resolve_ingest_input(home, &group_id, ingest_input(&kind, &payload)?)?;
    let existing = load(home, &group_id)?;
    let remote_space_id = binding_id(&existing, &lane)?;
    let payload_bytes = serde_json::to_vec(&payload).map_err(OpError::invalid)?;
    let payload_digest = format!("sha256:{:x}", Sha256::digest(&payload_bytes));
    let idempotency = normalized_idempotency_key(
        string_arg(request, "idempotency_key").as_deref(),
        &provider,
        &lane,
        &remote_space_id,
        &kind,
        &payload_digest,
    );
    if let Some(job) = array(&existing, "jobs")
        .iter()
        .find(|job| reusable_ingest_job(job, &provider, &lane, &remote_space_id, &idempotency))
    {
        let completed = job_is_completed(job);
        return object(
            json!({"group_id":group_id,"job_id":job["job_id"],"accepted":!completed,"completed":completed,"deduped":true,"job":job,"queue_summary":summary(&existing),"provider_mode":"active","degraded":false}),
        );
    }
    let (job, deduped) = begin_ingest_job(
        home,
        &group_id,
        &provider,
        &lane,
        &remote_space_id,
        &kind,
        &payload,
        &payload_digest,
        payload_bytes.len(),
        &idempotency,
    )?;
    if deduped {
        let existing = load(home, &group_id)?;
        let completed = job_is_completed(&job);
        return object(
            json!({"group_id":group_id,"job_id":job["job_id"],"accepted":!completed,"completed":completed,"deduped":true,"job":job,"queue_summary":summary(&existing),"provider_mode":"active","degraded":false}),
        );
    }

    let job_id = job["job_id"].as_str().unwrap_or_default().to_owned();
    let remote_source = match ingest_remote_source(home, &remote_space_id, &input) {
        Ok(source) => source,
        Err(mut error) => {
            let failed = settle_ingest_failure(home, &group_id, &job_id, &error)?;
            error.details.insert("job_id".into(), json!(job_id));
            error.details.insert("job".into(), failed);
            return Err(error);
        }
    };
    let source_type = input.source_type();
    let ingest_result = json!({"provider":"notebooklm","remote_space_id":remote_space_id,"accepted":true,"kind":kind,"source_mode":source_type,"source_type":source_type,"source_id":remote_source.id,"title":remote_source.title});
    let job = settle_ingest_success(home, &group_id, &job_id, &ingest_result)?;
    object(
        json!({"group_id":group_id,"job_id":job["job_id"],"accepted":false,"completed":true,"deduped":false,"job":job,"queue_summary":summary(&load(home,&group_id)?),"source_id":remote_source.id,"ingest_result":ingest_result,"provider_mode":"active","degraded":false}),
    )
}

#[allow(clippy::too_many_arguments)]
fn begin_ingest_job(
    home: &HomeLayout,
    group_id: &str,
    provider: &str,
    lane: &str,
    remote_space_id: &str,
    kind: &str,
    payload: &Map<String, Value>,
    payload_digest: &str,
    payload_bytes: usize,
    idempotency_key: &str,
) -> Result<(Value, bool), OpError> {
    update(home, group_id, |value| {
        let root = root(value);
        if let Some(item) = array_mut(root, "jobs").iter().find(|item| {
            reusable_ingest_job(item, provider, lane, remote_space_id, idempotency_key)
        }) {
            return Ok((item.clone(), true));
        }
        let now = utc_now();
        let job = json!({
            "job_id":format!("spj_{}",short_id()),
            "group_id":group_id,
            "provider":provider,
            "lane":lane,
            "remote_space_id":remote_space_id,
            "kind":kind,
            "payload":payload,
            "payload_ref":"",
            "result":{},
            "payload_digest":payload_digest,
            "payload_bytes":payload_bytes,
            "idempotency_key":idempotency_key,
            "state":"running",
            "attempt":1,
            "max_attempts":3,
            "next_run_at":null,
            "created_at":now,
            "updated_at":now,
            "last_error":{"code":"","message":""}
        });
        array_mut(root, "jobs").push(job.clone());
        Ok((job, false))
    })
}

fn reusable_ingest_job(
    job: &Value,
    provider: &str,
    lane: &str,
    remote_space_id: &str,
    idempotency_key: &str,
) -> bool {
    job["provider"] == provider
        && job["lane"] == lane
        && job["remote_space_id"] == remote_space_id
        && job["idempotency_key"] == idempotency_key
        && matches!(
            job["state"].as_str().unwrap_or_default(),
            "pending" | "running" | "retrying" | "succeeded"
        )
}

fn normalized_idempotency_key(
    requested: Option<&str>,
    provider: &str,
    lane: &str,
    remote_space_id: &str,
    kind: &str,
    payload_digest: &str,
) -> String {
    let requested = requested.unwrap_or_default().trim();
    if !requested.is_empty() {
        return requested.chars().take(256).collect();
    }
    format!("{provider}:{lane}:{remote_space_id}:{kind}:{payload_digest}")
}

fn job_is_completed(job: &Value) -> bool {
    matches!(
        job["state"].as_str().unwrap_or_default(),
        "succeeded" | "failed" | "canceled" | "cancelled"
    )
}

fn settle_ingest_success(
    home: &HomeLayout,
    group_id: &str,
    job_id: &str,
    ingest_result: &Value,
) -> Result<Value, OpError> {
    update(home, group_id, |value| {
        let item = array_mut(root(value), "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == job_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        item["state"] = json!("succeeded");
        item["result"] = ingest_result.clone();
        item["next_run_at"] = Value::Null;
        item["updated_at"] = json!(utc_now());
        item["last_error"] = json!({"code":"","message":""});
        Ok(item.clone())
    })
}

fn settle_ingest_failure(
    home: &HomeLayout,
    group_id: &str,
    job_id: &str,
    error: &OpError,
) -> Result<Value, OpError> {
    update(home, group_id, |value| {
        let item = array_mut(root(value), "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == job_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        item["state"] = json!(if error.code == "space_provider_outcome_unresolved" {
            "running"
        } else {
            "failed"
        });
        item["result"] = json!({});
        item["next_run_at"] = Value::Null;
        item["updated_at"] = json!(utc_now());
        item["last_error"] = json!({"code":error.code,"message":error.message});
        Ok(item.clone())
    })
}

pub(super) fn query(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let query = required_arg(request, "query")?;
    let lane = lane(request)?;
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let source_ids = query_source_ids(request)?;
    let remote_space_id = binding_id(&load(home, &group_id)?, &lane)?;
    let result = notebooklm::query(home, &remote_space_id, &query, source_ids.as_deref())?;
    let reference_count = result.references.len();
    let referenced_source_ids = result
        .references
        .iter()
        .map(|reference| reference.source_id.clone())
        .collect::<Vec<_>>();
    let matches_requested = source_ids.as_ref().map(|requested| {
        referenced_source_ids
            .iter()
            .all(|id| requested.contains(id))
    });
    object(
        json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","degraded":false,"answer":result.answer,"references":result.references,"reference_count":reference_count,"binding_status":"bound","requested_source_ids":source_ids,"referenced_source_ids":referenced_source_ids,"references_match_requested":matches_requested,"error":null}),
    )
}
pub(super) fn sources(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let provider = provider(request);
    let action = string_arg(request, "action").unwrap_or_else(|| "list".into());
    require_notebooklm(&provider)?;
    let value = load(home, &group_id)?;
    let remote_space_id = binding_id(&value, &lane)?;
    if action == "list" {
        let sources = notebooklm::sources(home, &remote_space_id)?;
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","binding":value["bindings"][&lane],"action":"list","sources":sources,"list_result":{"count":sources.len()}}),
        );
    }
    require_write_permission(home, &group_id, request)?;
    let id = required_arg(request, "source_id")?;
    let change_result = match action.as_str() {
        "delete" => {
            notebooklm::delete_source(home, &remote_space_id, &id)?;
            json!({"deleted":true,"source_id":id})
        }
        "rename" => {
            let title = required_arg(request, "new_title")?;
            notebooklm::rename_source(home, &remote_space_id, &id, &title)?;
            json!({"renamed":true,"source_id":id,"title":title})
        }
        "refresh" => {
            notebooklm::refresh_source(home, &remote_space_id, &id)?;
            json!({"refreshed":true,"source_id":id})
        }
        _ => {
            return Err(OpError::new(
                "invalid_args",
                "action must be list, refresh, rename, or delete",
            ));
        }
    };
    let result_key = format!("{action}_result");
    let mut result = json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","binding":value["bindings"][&lane],"action":action,"source_id":id});
    result[&result_key] = change_result;
    object(result)
}
pub(super) fn artifact(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    super::artifacts::handle(home, request)
}
pub(super) fn jobs(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let action = string_arg(request, "action").unwrap_or_else(|| "list".into());
    let provider = provider(request);
    require_notebooklm(&provider)?;
    if action == "list" {
        let value = load(home, &group_id)?;
        let lane_filter = string_arg(request, "lane").unwrap_or_default();
        let state_filter = string_arg(request, "state").unwrap_or_default();
        let remote_filter = string_arg(request, "remote_space_id").unwrap_or_default();
        let limit = request
            .args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(50)
            .clamp(1, 500) as usize;
        let mut jobs = array(&value, "jobs")
            .iter()
            .filter(|item| item["provider"].as_str() == Some(&provider))
            .filter(|item| lane_filter.is_empty() || item["lane"].as_str() == Some(&lane_filter))
            .filter(|item| state_filter.is_empty() || item["state"].as_str() == Some(&state_filter))
            .filter(|item| {
                remote_filter.is_empty() || item["remote_space_id"].as_str() == Some(&remote_filter)
            })
            .cloned()
            .collect::<Vec<_>>();
        jobs.sort_by(|left, right| {
            right["updated_at"]
                .as_str()
                .cmp(&left["updated_at"].as_str())
        });
        jobs.truncate(limit);
        return object(
            json!({"group_id":group_id,"provider":provider,"jobs":jobs,"queue_summary":summary(&value)}),
        );
    }
    require_write_permission(home, &group_id, request)?;
    let id = required_arg(request, "job_id")?;
    if !matches!(action.as_str(), "retry" | "cancel") {
        return Err(OpError::new(
            "invalid_args",
            "action must be list, retry, or cancel",
        ));
    }
    if action == "retry" {
        return retry_job(home, &group_id, &provider, &id);
    }
    let job = update(home, &group_id, |value| {
        let item = array_mut(root(value), "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        let current = item["state"].as_str().unwrap_or("");
        if action == "cancel" && !matches!(current, "pending" | "running" | "retrying") {
            return Err(io::Error::other(format!(
                "cannot cancel job in state={current}"
            )));
        }
        item["state"] = json!("canceled");
        item["updated_at"] = json!(utc_now());
        Ok(item.clone())
    })?;
    object(json!({"group_id":group_id,"job":job,"queue_summary":summary(&load(home,&group_id)?)}))
}

fn retry_job(home: &HomeLayout, group_id: &str, provider_name: &str, id: &str) -> OpResult {
    let value = load(home, group_id)?;
    let job = array(&value, "jobs")
        .iter()
        .find(|item| item["job_id"] == id)
        .cloned()
        .ok_or_else(|| OpError::new("not_found", "job not found"))?;
    let current = job["state"].as_str().unwrap_or("");
    if !matches!(current, "pending" | "failed" | "canceled" | "cancelled") {
        return Err(OpError::new(
            "invalid_state",
            format!("cannot retry job in state={current}"),
        ));
    }
    let stored_provider = job["provider"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(provider_name);
    if stored_provider != provider_name {
        return Err(OpError::new(
            "provider_mismatch",
            format!("job provider is {stored_provider}, not requested provider {provider_name}"),
        ));
    }
    let payload = job
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let kind = job["kind"].as_str().unwrap_or("context_sync");
    let lane_name = job["lane"].as_str().unwrap_or("work");
    require_notebooklm(stored_provider)?;
    if lane_name != "work" {
        return Err(OpError::new(
            "capability_unavailable",
            "legacy memory-sync jobs are read-only after automatic synchronization retirement",
        ));
    }
    let current_remote_space_id = binding_id(&value, lane_name)?;
    let remote_space_id = match job["remote_space_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        Some(remote_space_id) => remote_space_id.to_owned(),
        None => current_remote_space_id.clone(),
    };
    if remote_space_id != current_remote_space_id {
        let mut error = OpError::new(
            "binding_changed",
            "job target no longer matches the current work binding; submit a new ingest for the current binding",
        );
        error
            .details
            .insert("job_remote_space_id".into(), json!(remote_space_id));
        error.details.insert(
            "current_remote_space_id".into(),
            json!(current_remote_space_id),
        );
        return Err(error);
    }
    let input = resolve_ingest_input(home, group_id, ingest_input(kind, &payload)?)?;
    update(home, group_id, |value| {
        let item = array_mut(root(value), "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        item["state"] = json!("running");
        item["attempt"] = json!(item["attempt"].as_u64().unwrap_or(0) + 1);
        item["result"] = json!({});
        item["next_run_at"] = Value::Null;
        item["updated_at"] = json!(utc_now());
        item["last_error"] = json!({"code":"","message":""});
        Ok(item.clone())
    })?;
    let remote_source = match ingest_remote_source(home, &remote_space_id, &input) {
        Ok(source) => source,
        Err(mut error) => {
            let failed = settle_ingest_failure(home, group_id, id, &error)?;
            error.details.insert("job_id".into(), json!(id));
            error.details.insert("job".into(), failed);
            return Err(error);
        }
    };
    let source_type = input.source_type();
    let ingest_result = json!({
        "provider":stored_provider,"remote_space_id":remote_space_id,"accepted":true,
        "kind":kind,"source_mode":source_type,"source_type":source_type,
        "source_id":remote_source.id,"title":remote_source.title
    });
    let job = settle_ingest_success(home, group_id, id, &ingest_result)?;
    object(json!({
        "group_id":group_id,"provider":stored_provider,"job":job,
        "source_id":remote_source.id,
        "ingest_result":ingest_result,
        "queue_summary":summary(&load(home,group_id)?)
    }))
}

#[derive(Debug)]
enum IngestInput {
    Text {
        title: String,
        content: String,
        source_type: String,
    },
    Url {
        url: String,
        title: Option<String>,
        source_type: String,
    },
    Drive {
        title: String,
        file_id: String,
        mime_type: String,
        source_type: String,
    },
    File {
        path: std::path::PathBuf,
        title: String,
        source_type: String,
    },
}

impl IngestInput {
    fn source_type(&self) -> &str {
        match self {
            Self::Text { source_type, .. }
            | Self::Url { source_type, .. }
            | Self::Drive { source_type, .. }
            | Self::File { source_type, .. } => source_type,
        }
    }
}

fn ingest_input(kind: &str, payload: &Map<String, Value>) -> Result<IngestInput, OpError> {
    let title = ["title", "path"]
        .into_iter()
        .filter_map(|name| payload.get(name).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or(kind)
        .to_owned();

    if kind == "context_sync" {
        let content = text_content(payload)
            .unwrap_or_else(|| serde_json::to_string_pretty(payload).unwrap_or_default());
        return Ok(IngestInput::Text {
            title,
            content,
            source_type: "pasted_text".into(),
        });
    }
    if kind != "resource_ingest" {
        return Err(OpError::new(
            "invalid_args",
            "kind must be context_sync or resource_ingest",
        ));
    }

    let requested = payload
        .get("source_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_source_type)
        .unwrap_or_else(|| infer_source_type(payload));
    match requested {
        "pasted_text" => {
            let content = text_content(payload).ok_or_else(|| {
                OpError::new(
                    "invalid_args",
                    "content is required for resource_ingest source_type=pasted_text",
                )
            })?;
            Ok(IngestInput::Text {
                title,
                content,
                source_type: "pasted_text".into(),
            })
        }
        "web_page" | "youtube" => {
            let url = payload
                .get("url")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    OpError::new(
                        "invalid_args",
                        format!("url is required for resource_ingest source_type={requested}"),
                    )
                })?
                .to_owned();
            let classified = cccc_notebooklm::classify_url_source(&url).map_err(|error| {
                OpError::new(
                    "invalid_args",
                    format!("invalid NotebookLM source URL: {error}"),
                )
            })?;
            if requested == "youtube" && classified != cccc_notebooklm::UrlSourceKind::YouTube {
                return Err(OpError::new(
                    "invalid_args",
                    "source_type=youtube requires a recognizable YouTube video URL",
                ));
            }
            Ok(IngestInput::Url {
                url,
                title: payload
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                source_type: match classified {
                    cccc_notebooklm::UrlSourceKind::WebPage => "web_page",
                    cccc_notebooklm::UrlSourceKind::YouTube => "youtube",
                }
                .into(),
            })
        }
        "google_docs" | "google_slides" | "google_spreadsheet" => {
            let file_id = payload
                .get("file_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    OpError::new(
                        "invalid_args",
                        format!("file_id is required for resource_ingest source_type={requested}"),
                    )
                })?
                .to_owned();
            let expected_mime_type = drive_mime_type(requested);
            if let Some(mime_type) = payload
                .get("mime_type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                && mime_type != expected_mime_type
            {
                return Err(OpError::new(
                    "invalid_args",
                    format!("source_type={requested} requires mime_type={expected_mime_type}"),
                ));
            }
            Ok(IngestInput::Drive {
                title,
                file_id,
                mime_type: expected_mime_type.into(),
                source_type: requested.into(),
            })
        }
        "file" => {
            let path = ["file_path", "path"]
                .into_iter()
                .filter_map(|name| payload.get(name).and_then(Value::as_str))
                .map(str::trim)
                .find(|value| !value.is_empty())
                .ok_or_else(|| {
                    OpError::new(
                        "invalid_args",
                        "file_path is required for resource_ingest source_type=file",
                    )
                })?;
            let file_title = payload
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .or_else(|| Path::new(path).file_name().and_then(|value| value.to_str()))
                .unwrap_or(kind)
                .to_owned();
            Ok(IngestInput::File {
                path: path.into(),
                title: file_title,
                source_type: "file".into(),
            })
        }
        other => Err(OpError::new(
            "invalid_args",
            format!("unsupported resource_ingest source_type: {other}"),
        )),
    }
}

fn ingest_remote_source(
    home: &HomeLayout,
    notebook_id: &str,
    input: &IngestInput,
) -> Result<cccc_notebooklm::Source, OpError> {
    match input {
        IngestInput::Text { title, content, .. } => {
            notebooklm::add_text(home, notebook_id, title, content)
        }
        IngestInput::Url { url, title, .. } => {
            notebooklm::add_url(home, notebook_id, url, title.as_deref())
        }
        IngestInput::Drive {
            title,
            file_id,
            mime_type,
            ..
        } => notebooklm::add_drive(home, notebook_id, file_id, title, mime_type),
        IngestInput::File { path, title, .. } => {
            notebooklm::add_file(home, notebook_id, path, Some(title))
        }
    }
}

fn resolve_ingest_input(
    home: &HomeLayout,
    group_id: &str,
    input: IngestInput,
) -> Result<IngestInput, OpError> {
    let IngestInput::File {
        path,
        title,
        source_type,
    } = input
    else {
        return Ok(input);
    };
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(OpError::io)?;
    let scope = cccc_core::group_scope::resolve_attached_scope(&group, &group.active_scope_key)
        .or_else(|| group.scopes.first())
        .ok_or_else(|| {
            OpError::new(
                "scope_required",
                "an attached project scope is required for local-file ingestion",
            )
        })?;
    let path = resolve_local_file(Path::new(&scope.url), &path)?;
    Ok(IngestInput::File {
        path,
        title,
        source_type,
    })
}

fn resolve_local_file(scope_root: &Path, requested: &Path) -> Result<std::path::PathBuf, OpError> {
    let scope_root = scope_root.canonicalize().map_err(|error| {
        OpError::new(
            "invalid_project_root",
            format!("attached project scope is unavailable: {error}"),
        )
    })?;
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        scope_root.join(requested)
    };
    let candidate = candidate.canonicalize().map_err(|error| {
        OpError::new(
            "invalid_args",
            format!("local source file is unavailable: {error}"),
        )
    })?;
    if !candidate.starts_with(&scope_root) {
        return Err(OpError::new(
            "invalid_args",
            "local source file must be inside the active attached project scope",
        ));
    }
    let metadata = candidate.metadata().map_err(OpError::io)?;
    if !metadata.is_file() {
        return Err(OpError::new(
            "invalid_args",
            "local source path must name a regular file",
        ));
    }
    let extension = candidate
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_ascii_lowercase()))
        .unwrap_or_default();
    if !LOCAL_FILE_EXTENSIONS.contains(&extension.as_str()) {
        return Err(OpError::new(
            "space_source_unsupported_format",
            format!(
                "unsupported local source format: {}",
                if extension.is_empty() {
                    "(no extension)"
                } else {
                    &extension
                }
            ),
        ));
    }
    if metadata.len() > MAX_LOCAL_FILE_SIZE_BYTES {
        return Err(OpError::new(
            "space_source_file_too_large",
            "local source exceeds the 200 MiB NotebookLM limit",
        ));
    }
    Ok(candidate)
}

fn drive_mime_type(source_type: &str) -> &'static str {
    match source_type {
        "google_slides" => "application/vnd.google-apps.presentation",
        "google_spreadsheet" => "application/vnd.google-apps.spreadsheet",
        _ => "application/vnd.google-apps.document",
    }
}

fn text_content(payload: &Map<String, Value>) -> Option<String> {
    ["content", "text"]
        .into_iter()
        .filter_map(|name| payload.get(name).and_then(Value::as_str))
        .find(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn normalize_source_type(value: &str) -> &str {
    match value {
        "text" => "pasted_text",
        "url" => "web_page",
        "local_file" | "path" => "file",
        "google_doc" | "drive_doc" => "google_docs",
        "google_slide" | "drive_slide" => "google_slides",
        "google_sheet" | "google_sheets" | "drive_sheet" => "google_spreadsheet",
        other => other,
    }
}

fn infer_source_type(payload: &Map<String, Value>) -> &str {
    if payload
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return if payload
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|url| {
                cccc_notebooklm::classify_url_source(url)
                    .is_ok_and(|kind| kind == cccc_notebooklm::UrlSourceKind::YouTube)
            }) {
            "youtube"
        } else {
            "web_page"
        };
    }
    if ["file_path", "path"].into_iter().any(|name| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    }) {
        return "file";
    }
    if payload
        .get("file_id")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return "google_docs";
    }
    "pasted_text"
}

pub(super) fn sync(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    super::sync::handle(home, request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_notebooklm::Source;

    #[test]
    fn native_resource_ingest_accepts_current_non_file_sources() {
        let text = json!({"source_type":"text","content":"hello"});
        let input = ingest_input("resource_ingest", text.as_object().expect("text payload"))
            .expect("pasted text");
        assert_eq!(input.source_type(), "pasted_text");
        assert!(matches!(input, IngestInput::Text { content, .. } if content == "hello"));

        let url = json!({"source_type":"web_page","url":"https://example.test"});
        let input = ingest_input("resource_ingest", url.as_object().expect("url payload"))
            .expect("URL input");
        assert!(
            matches!(input, IngestInput::Url { ref source_type, .. } if source_type == "web_page")
        );

        let youtube = json!({"url":"https://www.youtube.com/watch?v=abc123"});
        let input = ingest_input(
            "resource_ingest",
            youtube.as_object().expect("YouTube payload"),
        )
        .expect("YouTube input");
        assert!(
            matches!(input, IngestInput::Url { ref source_type, .. } if source_type == "youtube")
        );

        let explicit_youtube = json!({"source_type":"youtube","url":"https://example.test"});
        let error = ingest_input(
            "resource_ingest",
            explicit_youtube
                .as_object()
                .expect("invalid YouTube payload"),
        )
        .expect_err("explicit YouTube input must not downgrade to a Web page");
        assert_eq!(error.code, "invalid_args");

        let invalid_url = json!({"source_type":"web_page","url":"file:///tmp/secret"});
        let error = ingest_input(
            "resource_ingest",
            invalid_url.as_object().expect("invalid URL payload"),
        )
        .expect_err("unsafe URL must fail before provider mutation");
        assert_eq!(error.code, "invalid_args");

        let drive = json!({"source_type":"google_sheet","file_id":"drive-1","title":"Sheet"});
        let input = ingest_input("resource_ingest", drive.as_object().expect("Drive payload"))
            .expect("Drive input");
        assert!(matches!(
            input,
            IngestInput::Drive { ref source_type, ref mime_type, .. }
                if source_type == "google_spreadsheet"
                    && mime_type == "application/vnd.google-apps.spreadsheet"
        ));

        let file = json!({"source_type":"file","file_path":"notes.md"});
        let input = ingest_input("resource_ingest", file.as_object().expect("file payload"))
            .expect("file input");
        assert!(matches!(
            input,
            IngestInput::File { ref path, ref title, ref source_type }
                if path == Path::new("notes.md") && title == "notes.md" && source_type == "file"
        ));
    }

    #[test]
    fn local_file_resolution_is_confined_to_the_attached_scope() {
        let scope = tempfile::tempdir().expect("scope");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(scope.path().join("notes.md"), "inside").expect("inside file");
        std::fs::write(scope.path().join("book.epub"), "inside").expect("EPUB file");
        std::fs::write(outside.path().join("secret.md"), "outside").expect("outside file");

        let resolved = resolve_local_file(scope.path(), Path::new("notes.md"))
            .expect("relative attached-scope file");
        assert_eq!(
            resolved,
            scope
                .path()
                .join("notes.md")
                .canonicalize()
                .expect("canonical Markdown fixture")
        );
        assert_eq!(
            resolve_local_file(scope.path(), Path::new("book.epub")).expect("EPUB source"),
            scope
                .path()
                .join("book.epub")
                .canonicalize()
                .expect("canonical EPUB fixture")
        );

        let error = resolve_local_file(scope.path(), &outside.path().join("secret.md"))
            .expect_err("absolute file outside scope must be rejected");
        assert_eq!(error.code, "invalid_args");
    }

    #[cfg(unix)]
    #[test]
    fn local_file_resolution_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let scope = tempfile::tempdir().expect("scope");
        let outside = tempfile::tempdir().expect("outside");
        let secret = outside.path().join("secret.md");
        std::fs::write(&secret, "outside").expect("outside file");
        symlink(&secret, scope.path().join("link.md")).expect("symlink");

        let error = resolve_local_file(scope.path(), Path::new("link.md"))
            .expect_err("symlink escape must be rejected");
        assert_eq!(error.code, "invalid_args");
    }

    #[test]
    fn context_sync_preserves_structured_payload_fallback() {
        let payload = json!({"decision":"ship"});
        let input = ingest_input(
            "context_sync",
            payload.as_object().expect("context payload"),
        )
        .expect("context sync");
        assert!(matches!(
            input,
            IngestInput::Text { content, .. }
                if content.contains("\"decision\": \"ship\"")
        ));
    }

    #[test]
    fn native_ingest_jobs_are_durable_before_and_after_provider_settlement() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("group store")
            .create("durable ingest", "")
            .expect("group");
        let payload = json!({"content":"durable"});
        let payload = payload.as_object().expect("payload");
        let digest = "sha256:fixture";
        let key = normalized_idempotency_key(
            None,
            "notebooklm",
            "work",
            "notebook-fixture",
            "context_sync",
            digest,
        );
        let (running, deduped) = begin_ingest_job(
            &home,
            &group.group_id,
            "notebooklm",
            "work",
            "notebook-fixture",
            "context_sync",
            payload,
            digest,
            18,
            &key,
        )
        .expect("begin ingest job");
        assert!(!deduped);
        assert_eq!(running["state"], "running");
        assert_eq!(running["attempt"], 1);

        let unresolved = settle_ingest_failure(
            &home,
            &group.group_id,
            running["job_id"].as_str().expect("job id"),
            &OpError::new(
                "space_provider_outcome_unresolved",
                "provider may have created the source",
            ),
        )
        .expect("record unresolved job");
        assert_eq!(unresolved["state"], "running");
        assert_eq!(
            unresolved["last_error"]["code"],
            "space_provider_outcome_unresolved"
        );

        let (same_job, unresolved_deduped) = begin_ingest_job(
            &home,
            &group.group_id,
            "notebooklm",
            "work",
            "notebook-fixture",
            "context_sync",
            payload,
            digest,
            18,
            &key,
        )
        .expect("dedupe unresolved ingest");
        assert!(unresolved_deduped);
        assert_eq!(same_job["job_id"], running["job_id"]);

        let blocked = retry_job(
            &home,
            &group.group_id,
            "notebooklm",
            running["job_id"].as_str().expect("job id"),
        )
        .expect_err("unresolved work must not be retried directly");
        assert_eq!(blocked.code, "invalid_state");

        let failed = settle_ingest_failure(
            &home,
            &group.group_id,
            running["job_id"].as_str().expect("job id"),
            &OpError::new("space_provider_timeout", "provider timed out"),
        )
        .expect("settle failed job");
        assert_eq!(failed["state"], "failed");
        assert_eq!(failed["last_error"]["code"], "space_provider_timeout");

        let (retry, retry_deduped) = begin_ingest_job(
            &home,
            &group.group_id,
            "notebooklm",
            "work",
            "notebook-fixture",
            "context_sync",
            payload,
            digest,
            18,
            &key,
        )
        .expect("begin replacement after terminal failure");
        assert!(!retry_deduped);
        assert_ne!(retry["job_id"], running["job_id"]);

        let source = Source {
            id: "source-fixture".into(),
            title: Some("Fixture source".into()),
            kind: "text".into(),
            status: "ready".into(),
            url: None,
            drive_document_id: None,
        };
        let result = json!({"source_id":source.id});
        let succeeded = settle_ingest_success(
            &home,
            &group.group_id,
            retry["job_id"].as_str().expect("retry job id"),
            &result,
        )
        .expect("settle successful job");
        assert_eq!(succeeded["state"], "succeeded");
        assert_eq!(succeeded["result"]["source_id"], "source-fixture");
    }

    #[test]
    fn retry_rejects_a_job_for_a_previous_work_binding() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("group store")
            .create("rebound work notebook", "")
            .expect("group");
        update(&home, &group.group_id, |value| {
            value["bindings"]["work"]["remote_space_id"] = json!("notebook-current");
            value["bindings"]["work"]["status"] = json!("bound");
            array_mut(root(value), "jobs").push(json!({
                "job_id":"spj_previous_binding",
                "group_id":group.group_id,
                "provider":"notebooklm",
                "lane":"work",
                "remote_space_id":"notebook-previous",
                "kind":"context_sync",
                "payload":{"content":"must not reach the previous notebook"},
                "state":"failed",
                "attempt":1,
                "last_error":{"code":"space_provider_timeout","message":"timeout"}
            }));
            Ok(())
        })
        .expect("stale job fixture");

        let error = retry_job(&home, &group.group_id, "notebooklm", "spj_previous_binding")
            .expect_err("retry must not write to a previous notebook binding");
        assert_eq!(error.code, "binding_changed");
        assert_eq!(error.details["job_remote_space_id"], "notebook-previous");
        assert_eq!(error.details["current_remote_space_id"], "notebook-current");

        let stored = load(&home, &group.group_id).expect("stored jobs");
        let job = array(&stored, "jobs")
            .iter()
            .find(|job| job["job_id"] == "spj_previous_binding")
            .expect("stale job remains inspectable");
        assert_eq!(job["state"], "failed");
        assert_eq!(job["attempt"], 1);
    }

    #[test]
    fn retired_memory_sync_jobs_cannot_be_retried() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("group store")
            .create("legacy memory sync", "")
            .expect("group");
        update(&home, &group.group_id, |value| {
            array_mut(root(value), "jobs").push(json!({
                "job_id":"spj_legacy_memory",
                "provider":"notebooklm",
                "lane":"memory",
                "remote_space_id":"nb-memory",
                "kind":"context_sync",
                "payload":{"content":"legacy"},
                "state":"failed"
            }));
            Ok(())
        })
        .expect("legacy memory job");

        let error = retry_job(&home, &group.group_id, "notebooklm", "spj_legacy_memory")
            .expect_err("retired automatic sync must stay read-only");
        assert_eq!(error.code, "capability_unavailable");
    }
}
