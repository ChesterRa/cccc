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
                json!({"group_id":group_id,"job_id":job["job_id"],"accepted":true,"completed":true,"deduped":true,"job":job,"queue_summary":summary(&existing),"provider_mode":if provider=="local"{"local"}else{"active"},"degraded":provider=="local"}),
            );
        }
    }
    let remote_space_id = binding_id(&load(home, &group_id)?, &lane)?;
    let remote_source = if provider == "notebooklm" {
        let title = payload
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| payload.get("path").and_then(Value::as_str))
            .unwrap_or(&kind);
        let content = payload
            .get("content")
            .or_else(|| payload.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| serde_json::to_string_pretty(&payload).unwrap_or_default());
        Some(notebooklm::add_text(
            home,
            &remote_space_id,
            title,
            &content,
        )?)
    } else {
        require_local(&provider)?;
        None
    };
    let (job, deduped) = update(home, &group_id, |value| {
        let root = root(value);
        if !idempotency.is_empty()
            && let Some(item) = array_mut(root, "jobs")
                .iter()
                .find(|item| item["idempotency_key"] == idempotency)
        {
            return Ok((item.clone(), true));
        }
        let job = json!({"job_id":format!("gsj_{}",short_id()),"group_id":group_id,"provider":provider,"lane":lane,"remote_space_id":remote_space_id,"kind":kind,"payload":payload,"idempotency_key":idempotency,"state":"succeeded","attempt":1,"max_attempts":3,"created_at":utc_now(),"updated_at":utc_now(),"execution_mode":if provider=="local"{"local"}else{"remote"}});
        array_mut(root, "jobs").push(job.clone());
        array_mut(root,"sources").push(json!({"source_id":remote_source.as_ref().map(|source|source.id.clone()).unwrap_or_else(||format!("gss_{}",short_id())),"provider":provider,"lane":lane,"title":remote_source.as_ref().and_then(|source|source.title.clone()).or_else(||payload["title"].as_str().map(str::to_owned)),"kind":remote_source.as_ref().map(|source|source.kind.as_str()).unwrap_or(&kind),"status":remote_source.as_ref().map(|source|source.status.as_str()).unwrap_or("ready"),"payload":payload,"created_at":utc_now()}));
        Ok((job, false))
    })?;
    object(
        json!({"group_id":group_id,"job_id":job["job_id"],"accepted":true,"completed":true,"deduped":deduped,"job":job,"queue_summary":summary(&load(home,&group_id)?),"source_id":remote_source.map(|source|source.id),"provider_mode":if provider=="local"{"local"}else{"active"},"degraded":provider=="local"}),
    )
}
pub(super) fn query(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let query = required_arg(request, "query")?;
    let lane = lane(request)?;
    let provider = provider(request);
    if provider == "notebooklm" {
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
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","degraded":false,"answer":result.answer,"references":result.references,"reference_count":reference_count,"binding_status":"bound","requested_source_ids":source_ids,"referenced_source_ids":referenced_source_ids,"references_match_requested":matches_requested,"error":null}),
        );
    }
    require_local(&provider)?;
    let value = load(home, &group_id)?;
    let terms = query
        .to_ascii_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut refs=array(&value,"sources").iter().filter(|item|item["lane"]==lane).filter_map(|item|{let text=item.to_string();let lower=text.to_ascii_lowercase();terms.iter().any(|term|lower.contains(term)).then(||json!({"source_id":item["source_id"],"title":item["title"],"excerpt":text.chars().take(400).collect::<String>()}))}).take(8).collect::<Vec<_>>();
    if refs.is_empty() {
        let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
        for event in ledger::tail(&store.ledger_path(&group_id).map_err(OpError::io)?, 50)
            .map_err(OpError::io)?
            .into_iter()
            .rev()
        {
            let text = serde_json::to_string(&event).unwrap_or_default();
            let lower = text.to_ascii_lowercase();
            if terms.iter().any(|term| lower.contains(term)) {
                refs.push(json!({"event_id":event.id,"kind":event.kind,"excerpt":text.chars().take(400).collect::<String>()}));
                if refs.len() == 8 {
                    break;
                }
            }
        }
    }
    let answer = refs
        .iter()
        .map(|item| item["excerpt"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    object(
        json!({"group_id":group_id,"provider":provider,"provider_mode":"local","degraded":true,"answer":answer,"references":refs,"error":null}),
    )
}
pub(super) fn sources(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    let provider = provider(request);
    let action = string_arg(request, "action").unwrap_or_else(|| "list".into());
    if provider == "notebooklm" {
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
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"provider_mode":"active","binding":value["bindings"][&lane],"action":action,"source_id":id,"changed":true}),
        );
    }
    require_local(&provider)?;
    if action == "list" {
        let value = load(home, &group_id)?;
        return object(
            json!({"group_id":group_id,"provider":provider,"provider_mode":"local","action":"list","sources":array(&value,"sources").iter().filter(|item|item["lane"]==lane).cloned().collect::<Vec<_>>()}),
        );
    }
    let id = required_arg(request, "source_id")?;
    let changed = update(home, &group_id, |value| {
        let items = array_mut(root(value), "sources");
        if action == "delete" {
            let before = items.len();
            items.retain(|item| item["source_id"] != id);
            return Ok(before != items.len());
        }
        let item = items
            .iter_mut()
            .find(|item| item["source_id"] == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "source not found"))?;
        if action == "rename" {
            item["title"] = json!(string_arg(request, "new_title").unwrap_or_default());
        }
        item["updated_at"] = json!(utc_now());
        Ok(true)
    })?;
    object(json!({"group_id":group_id,"action":action,"source_id":id,"changed":changed}))
}
pub(super) fn artifact(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let _ = home;
    let _ = required_arg(request, "group_id")?;
    Err(OpError::new(
        "provider_unavailable",
        "NotebookLM artifact generation is unavailable; no remote operation was performed",
    ))
}
pub(super) fn jobs(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let action = string_arg(request, "action").unwrap_or_else(|| "list".into());
    let provider = provider(request);
    if provider != "notebooklm" {
        require_local(&provider)?;
    }
    if action == "list" {
        let value = load(home, &group_id)?;
        return object(
            json!({"group_id":group_id,"provider":provider,"jobs":array(&value,"jobs"),"queue_summary":summary(&value)}),
        );
    }
    let id = required_arg(request, "job_id")?;
    let job = update(home, &group_id, |value| {
        let item = array_mut(root(value), "jobs")
            .iter_mut()
            .find(|item| item["job_id"] == id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "job not found"))?;
        item["state"] = json!(if action == "cancel" {
            "canceled"
        } else {
            "succeeded"
        });
        item["updated_at"] = json!(utc_now());
        Ok(item.clone())
    })?;
    object(json!({"group_id":group_id,"job":job,"queue_summary":summary(&load(home,&group_id)?)}))
}
pub(super) fn sync(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let _ = home;
    let _ = required_arg(request, "group_id")?;
    Err(OpError::new(
        "provider_unavailable",
        "NotebookLM sync is unavailable; no remote operation was performed",
    ))
}
