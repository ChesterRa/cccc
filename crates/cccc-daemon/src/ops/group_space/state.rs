use super::*;
use cccc_core::fs::{read_json, with_exclusive_lock, write_json};
use std::path::PathBuf;

pub(super) fn load(home: &HomeLayout, group_id: &str) -> Result<Value, OpError> {
    migrate_legacy_group(home, group_id)?;
    let bindings_doc = read_doc(&space_path(home, "bindings.json"))?;
    let mut bindings = bindings_doc
        .get("bindings")
        .and_then(|value| value.get(group_id))
        .and_then(|value| value.get("notebooklm"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    for lane in ["memory", "work"] {
        if bindings.get(lane).is_none() {
            object(&mut bindings)
                .insert(lane.into(), default_binding(group_id, "notebooklm", lane));
        }
    }
    let jobs_doc = read_doc(&space_path(home, "jobs.json"))?;
    let jobs = jobs_doc
        .get("jobs")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, job)| job["group_id"].as_str() == Some(group_id))
        .map(|(job_id, job)| {
            let mut job = job.clone();
            job["job_id"] = json!(job_id);
            job["id"] = json!(job_id);
            if job["payload"].as_object().is_some_and(Map::is_empty)
                && let Some(payload_ref) = job["payload_ref"]
                    .as_str()
                    .filter(|value| !value.is_empty())
            {
                let payload_path = home
                    .root()
                    .join("state/space/job_payloads")
                    .join(payload_ref);
                if payload_path.is_file()
                    && let Ok(payload) = read_json::<Value>(&payload_path)
                    && payload.is_object()
                {
                    job["payload"] = payload;
                }
            }
            job
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "bindings":bindings,
        "sources":[],
        "artifacts":[],
        "jobs":jobs
    }))
}

pub(super) fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, OpError> {
    let mut state = load(home, group_id)?;
    let result = change(&mut state).map_err(OpError::io)?;
    save_bindings(home, group_id, &state)?;
    save_jobs(home, group_id, &state)?;
    Ok(result)
}

fn save_bindings(home: &HomeLayout, group_id: &str, state: &Value) -> Result<(), OpError> {
    let path = space_path(home, "bindings.json");
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut doc = read_doc_io(&path)?;
        doc["v"] = json!(2);
        doc.as_object_mut()
            .expect("bindings document is an object")
            .entry("created_at")
            .or_insert_with(|| json!(utc_now()));
        doc["updated_at"] = json!(utc_now());
        let bindings = object_field(&mut doc, "bindings");
        let group = bindings.entry(group_id).or_insert_with(|| json!({}));
        let mut lanes = state["bindings"].clone();
        for lane in ["memory", "work"] {
            let binding = object(&mut lanes)
                .entry(lane)
                .or_insert_with(|| default_binding(group_id, "notebooklm", lane));
            let binding = object(binding);
            binding.remove("id");
            binding.insert("group_id".into(), json!(group_id));
            binding.insert("provider".into(), json!("notebooklm"));
            binding.insert("lane".into(), json!(lane));
            binding
                .entry("remote_space_id")
                .or_insert_with(|| json!(""));
            binding.entry("bound_by").or_insert_with(|| json!(""));
            binding
                .entry("bound_at")
                .or_insert_with(|| json!(utc_now()));
            let status = if binding["remote_space_id"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
            {
                "unbound"
            } else {
                "bound"
            };
            binding.entry("status").or_insert_with(|| json!(status));
        }
        object(group).insert("notebooklm".into(), lanes);
        write_json(&path, &doc)
    })
    .map_err(OpError::io)
}

fn save_jobs(home: &HomeLayout, group_id: &str, state: &Value) -> Result<(), OpError> {
    let path = space_path(home, "jobs.json");
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut doc = read_doc_io(&path)?;
        doc["v"] = json!(2);
        doc.as_object_mut()
            .expect("jobs document is an object")
            .entry("created_at")
            .or_insert_with(|| json!(utc_now()));
        doc["updated_at"] = json!(utc_now());
        let jobs = object_field(&mut doc, "jobs");
        jobs.retain(|_, job| job["group_id"].as_str() != Some(group_id));
        for job in state["jobs"].as_array().into_iter().flatten() {
            let id = job["job_id"]
                .as_str()
                .or_else(|| job["id"].as_str())
                .unwrap_or_default();
            if !id.is_empty() {
                let mut job = job.clone();
                job.as_object_mut()
                    .expect("space job is an object")
                    .remove("id");
                job["job_id"] = json!(id);
                job["group_id"] = json!(group_id);
                normalize_job(&mut job);
                jobs.insert(id.into(), job);
            }
        }
        write_json(&path, &doc)
    })
    .map_err(OpError::io)
}

pub(super) fn provider_record(home: &HomeLayout, provider: &str) -> Result<Value, OpError> {
    migrate_legacy_providers(home)?;
    Ok(read_doc(&space_path(home, "providers.json"))?
        .get("providers")
        .and_then(|providers| providers.get(provider))
        .cloned()
        .unwrap_or_else(|| json!({})))
}

pub(super) fn update_provider<T>(
    home: &HomeLayout,
    provider: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, OpError> {
    migrate_legacy_providers(home)?;
    let path = space_path(home, "providers.json");
    with_exclusive_lock(&path.with_extension("json.lock"), || {
        let mut doc = read_doc_io(&path)?;
        doc["v"] = json!(1);
        doc.as_object_mut()
            .expect("providers document is an object")
            .entry("created_at")
            .or_insert_with(|| json!(utc_now()));
        doc["updated_at"] = json!(utc_now());
        let providers = object_field(&mut doc, "providers");
        let record = providers
            .entry(provider)
            .or_insert_with(|| json!({"provider":provider}));
        let result = change(record)?;
        normalize_provider(record, provider);
        write_json(&path, &doc)?;
        Ok(result)
    })
    .map_err(OpError::io)
}

fn space_path(home: &HomeLayout, name: &str) -> PathBuf {
    home.root().join("state/space").join(name)
}

fn migrate_legacy_group(home: &HomeLayout, group_id: &str) -> Result<(), OpError> {
    let marker = home
        .root()
        .join("groups")
        .join(group_id)
        .join("state/.rust-space-migrated-v1");
    if marker.exists() {
        return Ok(());
    }
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = store.load(group_id).map_err(OpError::io)?;
    if let Some(legacy) = group
        .extra
        .get("group_space")
        .filter(|value| value.is_object())
    {
        save_bindings(home, group_id, legacy)?;
        save_jobs(home, group_id, legacy)?;
    }
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).map_err(OpError::io)?;
    }
    std::fs::write(marker, b"migrated from group.yaml group_space\n").map_err(OpError::io)
}

fn migrate_legacy_providers(home: &HomeLayout) -> Result<(), OpError> {
    let path = space_path(home, "providers.json");
    let marker = home.root().join("state/space/.rust-providers-migrated-v1");
    if marker.exists() {
        return Ok(());
    }
    let settings = cccc_core::settings::load(home).map_err(OpError::io)?;
    if let Some(legacy) = settings
        .extra
        .get("space_providers")
        .and_then(Value::as_object)
    {
        with_exclusive_lock(&path.with_extension("json.lock"), || {
            let mut doc = read_doc_io(&path)?;
            let providers = object_field(&mut doc, "providers");
            for (provider, record) in legacy {
                providers
                    .entry(provider.clone())
                    .or_insert_with(|| record.clone());
            }
            doc["v"] = json!(1);
            doc["updated_at"] = json!(utc_now());
            write_json(&path, &doc)
        })
        .map_err(OpError::io)?;
    }
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).map_err(OpError::io)?;
    }
    std::fs::write(marker, b"migrated from settings space_providers\n").map_err(OpError::io)
}

fn read_doc(path: &std::path::Path) -> Result<Value, OpError> {
    read_doc_io(path).map_err(OpError::io)
}

fn read_doc_io(path: &std::path::Path) -> io::Result<Value> {
    if path.exists() {
        read_json(path)
    } else {
        Ok(json!({}))
    }
}

fn object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    value.as_object_mut().expect("space object initialized")
}

pub(super) fn root(value: &mut Value) -> &mut Map<String, Value> {
    let root = object(value);
    for key in ["bindings", "sources", "artifacts", "jobs"] {
        root.entry(key).or_insert_with(|| {
            if key == "bindings" {
                json!({})
            } else {
                json!([])
            }
        });
    }
    root
}

fn object_field<'a>(value: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    let root = object(value);
    object(root.entry(key).or_insert_with(|| json!({})))
}

fn default_binding(group_id: &str, provider: &str, lane: &str) -> Value {
    json!({
        "group_id":group_id,
        "provider":provider,
        "lane":lane,
        "remote_space_id":"",
        "bound_by":"",
        "bound_at":utc_now(),
        "status":"unbound",
    })
}

fn normalize_job(job: &mut Value) {
    let root = object(job);
    root.insert("provider".into(), json!("notebooklm"));
    root.entry("lane").or_insert_with(|| json!("work"));
    root.entry("remote_space_id").or_insert_with(|| json!(""));
    root.entry("kind").or_insert_with(|| json!("context_sync"));
    root.entry("payload").or_insert_with(|| json!({}));
    root.entry("payload_ref").or_insert_with(|| json!(""));
    root.entry("result").or_insert_with(|| json!({}));
    root.entry("payload_digest").or_insert_with(|| json!(""));
    root.entry("payload_bytes").or_insert_with(|| json!(0));
    root.entry("idempotency_key").or_insert_with(|| json!(""));
    root.entry("state").or_insert_with(|| json!("pending"));
    root.entry("attempt").or_insert_with(|| json!(0));
    root.entry("max_attempts").or_insert_with(|| json!(3));
    root.entry("next_run_at").or_insert(Value::Null);
    root.entry("created_at").or_insert_with(|| json!(utc_now()));
    root.entry("updated_at").or_insert_with(|| json!(utc_now()));
    root.entry("last_error")
        .or_insert_with(|| json!({"code":"","message":""}));
    root.retain(|key, _| {
        matches!(
            key.as_str(),
            "job_id"
                | "group_id"
                | "provider"
                | "lane"
                | "remote_space_id"
                | "kind"
                | "payload"
                | "payload_ref"
                | "result"
                | "payload_digest"
                | "payload_bytes"
                | "idempotency_key"
                | "state"
                | "attempt"
                | "max_attempts"
                | "next_run_at"
                | "created_at"
                | "updated_at"
                | "last_error"
        )
    });
}

pub(super) fn map_field<'a>(
    root: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Map<String, Value> {
    root.get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("map initialized")
}

pub(super) fn array_mut<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Vec<Value> {
    root.get_mut(key)
        .and_then(Value::as_array_mut)
        .expect("array initialized")
}

pub(super) fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub(super) fn summary(value: &Value) -> Value {
    let jobs = array(value, "jobs");
    json!({"pending":jobs.iter().filter(|item|item["state"]=="pending").count(),"running":jobs.iter().filter(|item|item["state"]=="running").count(),"failed":jobs.iter().filter(|item|item["state"]=="failed").count()})
}

pub(super) fn summary_for(value: &Value, lane: &str) -> Value {
    let jobs = array(value, "jobs")
        .iter()
        .filter(|item| item["lane"].as_str() == Some(lane))
        .collect::<Vec<_>>();
    json!({
        "pending":jobs.iter().filter(|item|item["state"]=="pending").count(),
        "running":jobs.iter().filter(|item|item["state"]=="running").count(),
        "failed":jobs.iter().filter(|item|item["state"]=="failed").count()
    })
}

pub(super) fn binding_id(value: &Value, lane: &str) -> Result<String, OpError> {
    value
        .get("bindings")
        .and_then(|bindings| bindings.get(lane))
        .and_then(|binding| binding.get("remote_space_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            OpError::new(
                "binding_required",
                format!("{lane} lane is not bound to a NotebookLM notebook"),
            )
        })
}

pub(super) fn query_source_ids(request: &DaemonRequest) -> Result<Option<Vec<String>>, OpError> {
    let Some(options) = request.args.get("options") else {
        return Ok(None);
    };
    let options = options
        .as_object()
        .ok_or_else(|| OpError::new("invalid_args", "options must be an object"))?;
    if let Some(key) = options.keys().find(|key| key.as_str() != "source_ids") {
        return Err(OpError::new(
            "invalid_args",
            format!("unsupported NotebookLM query option: {key}"),
        ));
    }
    let Some(source_ids) = options.get("source_ids") else {
        return Ok(None);
    };
    let values = source_ids
        .as_array()
        .ok_or_else(|| OpError::new("invalid_args", "options.source_ids must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    OpError::new(
                        "invalid_args",
                        "options.source_ids must contain non-empty strings",
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(values))
}

pub(super) fn provider(request: &DaemonRequest) -> String {
    string_arg(request, "provider").unwrap_or_else(|| "notebooklm".into())
}

pub(super) fn require_notebooklm(provider: &str) -> Result<(), OpError> {
    (provider == "notebooklm")
        .then_some(())
        .ok_or_else(provider_unavailable)
}

pub(super) fn provider_unavailable() -> OpError {
    OpError::new(
        "provider_unavailable",
        "unsupported provider; Python and Rust support provider=notebooklm",
    )
}

pub(super) fn provider_state(provider: &str, ready: bool) -> Value {
    json!({
        "provider":provider,
        "enabled":ready,
        "real_enabled":ready,
        "mode":if ready{"active"}else{"degraded"},
        "write_ready":ready,
        "readiness_reason":if ready{"ready"}else{"health check failed"}
    })
}

pub(super) fn record_provider_health(
    home: &HomeLayout,
    provider: &str,
    healthy: bool,
    checked_at: &str,
    error: Option<&str>,
) -> Result<(), OpError> {
    update_provider(home, provider, |item| {
        let enabled = item["enabled"].as_bool().unwrap_or(false);
        item["mode"] = json!(if !enabled {
            "disabled"
        } else if healthy {
            "active"
        } else {
            "degraded"
        });
        item["last_health_at"] = json!(checked_at);
        item["last_error"] = error.map_or(Value::Null, |message| json!(message));
        Ok(())
    })
}

fn normalize_provider(record: &mut Value, provider: &str) {
    let root = object(record);
    root.insert("provider".into(), json!(provider));
    root.entry("enabled").or_insert_with(|| json!(false));
    root.entry("real_enabled").or_insert_with(|| json!(false));
    root.entry("mode").or_insert_with(|| json!("disabled"));
    root.entry("last_health_at").or_insert(Value::Null);
    root.entry("last_error").or_insert(Value::Null);
    root.retain(|key, _| {
        matches!(
            key.as_str(),
            "provider" | "enabled" | "real_enabled" | "mode" | "last_health_at" | "last_error"
        )
    });
}

pub(super) fn require_user(request: &DaemonRequest) -> Result<(), OpError> {
    (string_arg(request, "by").as_deref().unwrap_or("user") == "user")
        .then_some(())
        .ok_or_else(|| {
            OpError::new(
                "permission_denied",
                "space provider credentials are user-only",
            )
        })
}

pub(super) fn lane(request: &DaemonRequest) -> Result<String, OpError> {
    let value = required_arg(request, "lane")?;
    matches!(value.as_str(), "work" | "memory")
        .then_some(value)
        .ok_or_else(|| OpError::new("invalid_args", "lane must be work or memory"))
}

pub(super) fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].into()
}
