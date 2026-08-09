use super::*;

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
    let kind = string_arg(request, "kind").unwrap_or_else(|| "context_sync".into());
    let payload = match request.args.get("payload") {
        None => Map::new(),
        Some(Value::Object(payload)) => payload.clone(),
        Some(_) => return Err(OpError::new("invalid_args", "payload must be an object")),
    };
    let idempotency = string_arg(request, "idempotency_key").unwrap_or_default();
    if !idempotency.is_empty() {
        let existing = load(home, &group_id)?;
        if let Some(job) = array(&existing, "jobs")
            .iter()
            .find(|item| item["idempotency_key"] == idempotency)
        {
            return object(
                json!({"group_id":group_id,"job_id":job["job_id"],"accepted":true,"completed":true,"deduped":true,"job":job,"queue_summary":summary(&existing),"provider_mode":"active","degraded":false}),
            );
        }
    }
    let input = text_ingest_input(&kind, &payload)?;
    let remote_space_id = binding_id(&load(home, &group_id)?, &lane)?;
    let remote_source = notebooklm::add_text(home, &remote_space_id, &input.title, &input.content)?;
    let (job, deduped) = update(home, &group_id, |value| {
        let root = root(value);
        if !idempotency.is_empty()
            && let Some(item) = array_mut(root, "jobs")
                .iter()
                .find(|item| item["idempotency_key"] == idempotency)
        {
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
            "payload_digest":"",
            "payload_bytes":0,
            "idempotency_key":idempotency,
            "state":"succeeded",
            "attempt":1,
            "max_attempts":3,
            "next_run_at":null,
            "created_at":now,
            "updated_at":now,
            "last_error":{"code":"","message":""}
        });
        array_mut(root, "jobs").push(job.clone());
        array_mut(root,"sources").push(json!({"source_id":remote_source.id,"provider":provider,"lane":lane,"title":remote_source.title,"kind":remote_source.kind,"status":remote_source.status,"payload":payload,"created_at":utc_now()}));
        Ok((job, false))
    })?;
    object(
        json!({"group_id":group_id,"job_id":job["job_id"],"accepted":true,"completed":true,"deduped":deduped,"job":job,"queue_summary":summary(&load(home,&group_id)?),"source_id":remote_source.id,"ingest_result":{"provider":"notebooklm","remote_space_id":remote_space_id,"accepted":true,"kind":kind,"source_mode":input.source_type,"source_type":input.source_type,"source_id":remote_source.id,"title":remote_source.title},"provider_mode":"active","degraded":false}),
    )
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
    if !matches!(current, "failed" | "canceled") {
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
    let remote_space_id = job["remote_space_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or(binding_id(&value, lane_name)?);
    require_notebooklm(stored_provider)?;
    if lane_name != "work" {
        return Err(OpError::new(
            "invalid_args",
            "memory sync jobs must be retried through group_space_sync",
        ));
    }
    let input = text_ingest_input(kind, &payload)?;
    let remote_source = notebooklm::add_text(home, &remote_space_id, &input.title, &input.content)?;
    let job = update(home, group_id, |value| {
        let root = root(value);
        let item = array_mut(root, "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        item["state"] = json!("succeeded");
        item["attempt"] = json!(item["attempt"].as_u64().unwrap_or(0) + 1);
        item["updated_at"] = json!(utc_now());
        let completed = item.clone();
        array_mut(root, "sources").push(json!({
            "source_id":remote_source.id,"provider":stored_provider,"lane":lane_name,
            "title":remote_source.title,"kind":remote_source.kind,"status":remote_source.status,
            "payload":payload,"created_at":utc_now()
        }));
        Ok(completed)
    })?;
    object(json!({
        "group_id":group_id,"provider":stored_provider,"job":job,
        "source_id":remote_source.id,
        "queue_summary":summary(&load(home,group_id)?)
    }))
}

#[derive(Debug)]
struct TextIngestInput {
    title: String,
    content: String,
    source_type: &'static str,
}

fn text_ingest_input(kind: &str, payload: &Map<String, Value>) -> Result<TextIngestInput, OpError> {
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
        return Ok(TextIngestInput {
            title,
            content,
            source_type: "pasted_text",
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
    if requested != "pasted_text" {
        return Err(OpError::new(
            "capability_unavailable",
            format!(
                "the native Rust NotebookLM provider does not yet support resource source_type={requested}; supported source_types: pasted_text"
            ),
        ));
    }
    let content = text_content(payload).ok_or_else(|| {
        OpError::new(
            "invalid_args",
            "content is required for resource_ingest source_type=pasted_text",
        )
    })?;
    Ok(TextIngestInput {
        title,
        content,
        source_type: "pasted_text",
    })
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
        "google_doc" => "google_docs",
        "google_slide" => "google_slides",
        "google_sheet" => "google_spreadsheet",
        other => other,
    }
}

fn infer_source_type(payload: &Map<String, Value>) -> &str {
    if payload
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return "web_page";
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

    #[test]
    fn native_resource_ingest_accepts_only_honest_text_capability() {
        let text = json!({"source_type":"text","content":"hello"});
        let input = text_ingest_input("resource_ingest", text.as_object().expect("text payload"))
            .expect("pasted text");
        assert_eq!(input.source_type, "pasted_text");
        assert_eq!(input.content, "hello");

        let url = json!({"source_type":"web_page","url":"https://example.test"});
        let error = text_ingest_input("resource_ingest", url.as_object().expect("url payload"))
            .expect_err("URL ingest must not masquerade as pasted text");
        assert_eq!(error.code, "capability_unavailable");
    }

    #[test]
    fn context_sync_preserves_structured_payload_fallback() {
        let payload = json!({"decision":"ship"});
        let input = text_ingest_input(
            "context_sync",
            payload.as_object().expect("context payload"),
        )
        .expect("context sync");
        assert!(input.content.contains("\"decision\": \"ship\""));
    }
}
