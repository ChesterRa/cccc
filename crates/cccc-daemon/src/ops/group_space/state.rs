use super::*;

pub(super) fn load(home: &HomeLayout, group_id: &str) -> Result<Value, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_get(&store, group_id, KEY).map_err(OpError::io)
}
pub(super) fn update<T>(
    home: &HomeLayout,
    group_id: &str,
    change: impl FnOnce(&mut Value) -> io::Result<T>,
) -> Result<T, OpError> {
    let store = GroupStore::new(home.clone()).map_err(OpError::io)?;
    integration_state::group_update(&store, group_id, KEY, change).map_err(OpError::io)
}
pub(super) fn root(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("space initialized");
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
    let source_ids = source_ids
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
    Ok(Some(source_ids))
}
pub(super) fn provider(request: &DaemonRequest) -> String {
    string_arg(request, "provider").unwrap_or_else(|| "notebooklm".into())
}

pub(super) fn require_local(provider: &str) -> Result<(), OpError> {
    (provider == "local")
        .then_some(())
        .ok_or_else(provider_unavailable)
}

pub(super) fn provider_unavailable() -> OpError {
    OpError::new(
        "provider_unavailable",
        "NotebookLM has no Rust remote adapter; use provider=local only for explicit local fallback",
    )
}

pub(super) fn provider_state(provider: &str, ready: bool) -> Value {
    let local = provider == "local";
    json!({"provider":provider,"enabled":ready,"real_enabled":!local,"mode":if local{"local"}else if ready{"active"}else{"degraded"},"write_ready":ready,"readiness_reason":if local{"explicit local fallback"}else if ready{"authenticated Rust adapter"}else{"health check failed"}})
}
pub(super) fn record_provider_health(
    home: &HomeLayout,
    provider: &str,
    healthy: bool,
    checked_at: &str,
    error: Option<&str>,
) -> Result<(), OpError> {
    integration_state::global_update(home, "space_providers", |value| {
        if !value.is_object() {
            *value = json!({});
        }
        let item = value
            .as_object_mut()
            .expect("providers initialized")
            .entry(provider)
            .or_insert_with(|| json!({}));
        if !item.is_object() {
            *item = json!({});
        }
        item["healthy"] = json!(healthy);
        item["last_health_at"] = json!(checked_at);
        item["last_error"] = error.map_or(Value::Null, |message| json!(message));
        Ok(())
    })
    .map_err(OpError::io)
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
