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
    let payload = request
        .args
        .get("payload")
        .cloned()
        .unwrap_or_else(|| json!({}));
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
    let remote_space_id = binding_id(&load(home, &group_id)?, &lane)?;
    let title = ["title", "path"]
        .into_iter()
        .filter_map(|name| payload.get(name).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or(&kind);
    let content = ["content", "text"]
        .into_iter()
        .filter_map(|name| payload.get(name).and_then(Value::as_str))
        .find(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(&payload).unwrap_or_default());
    let remote_source = Some(notebooklm::add_text(
        home,
        &remote_space_id,
        title,
        &content,
    )?);
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
        array_mut(root,"sources").push(json!({"source_id":remote_source.as_ref().map(|source|source.id.clone()).unwrap_or_else(||format!("gss_{}",short_id())),"provider":provider,"lane":lane,"title":remote_source.as_ref().and_then(|source|source.title.clone()).or_else(||payload["title"].as_str().map(str::to_owned)),"kind":remote_source.as_ref().map(|source|source.kind.as_str()).unwrap_or(&kind),"status":remote_source.as_ref().map(|source|source.status.as_str()).unwrap_or("ready"),"payload":payload,"created_at":utc_now()}));
        Ok((job, false))
    })?;
    object(
        json!({"group_id":group_id,"job_id":job["job_id"],"accepted":true,"completed":true,"deduped":deduped,"job":job,"queue_summary":summary(&load(home,&group_id)?),"source_id":remote_source.map(|source|source.id),"provider_mode":"active","degraded":false}),
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
    if action == "list" || action == "refresh" && string_arg(request, "source_id").is_none() {
        let sources = notebooklm::sources(home, &remote_space_id)?;
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","binding":value["bindings"][&lane],"action":"list","sources":sources,"list_result":{"count":sources.len()}}),
        );
    }
    let id = required_arg(request, "source_id")?;
    match action.as_str() {
        "delete" => notebooklm::delete_source(home, &remote_space_id, &id)?,
        "rename" => {
            let title = required_arg(request, "new_title")?;
            notebooklm::rename_source(home, &remote_space_id, &id, &title)?;
        }
        "refresh" => {
            let sources = notebooklm::sources(home, &remote_space_id)?;
            let source = sources
                .into_iter()
                .find(|source| source.id == id)
                .ok_or_else(|| OpError::new("not_found", "source not found"))?;
            return object(
                json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","binding":value["bindings"][&lane],"action":action,"source_id":id,"refresh_result":source}),
            );
        }
        _ => {
            return Err(OpError::new(
                "invalid_args",
                "action must be list, refresh, rename, or delete",
            ));
        }
    }
    object(
        json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","binding":value["bindings"][&lane],"action":action,"source_id":id,"changed":true}),
    )
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
    let title = ["title", "path"]
        .into_iter()
        .filter_map(|name| payload.get(name).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or(kind);
    let content = ["content", "text"]
        .into_iter()
        .filter_map(|name| payload.get(name).and_then(Value::as_str))
        .find(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(&payload).unwrap_or_default());
    let remote_source = Some(notebooklm::add_text(
        home,
        &remote_space_id,
        title,
        &content,
    )?);
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
        if let Some(source) = &remote_source {
            array_mut(root, "sources").push(json!({
                "source_id":source.id,"provider":stored_provider,"lane":lane_name,
                "title":source.title,"kind":source.kind,"status":source.status,
                "payload":payload,"created_at":utc_now()
            }));
        }
        Ok(completed)
    })?;
    object(json!({
        "group_id":group_id,"provider":stored_provider,"job":job,
        "source_id":remote_source.map(|source|source.id),
        "queue_summary":summary(&load(home,group_id)?)
    }))
}
pub(super) fn sync(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    super::sync::handle(home, request)
}
