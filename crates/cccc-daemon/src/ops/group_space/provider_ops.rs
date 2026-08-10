use super::*;

pub(super) fn provider_auth(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let provider = provider(request);
    let action = string_arg(request, "action").unwrap_or_else(|| "status".into());
    if action == "start"
        && space_credentials::resolve(home, &provider)
            .map_err(OpError::io)?
            .is_none()
    {
        return Err(OpError::new(
            "credential_missing",
            "configure a NotebookLM Playwright storage-state JSON before starting authentication",
        ));
    }
    require_notebooklm(&provider)?;
    let credential = space_credentials::status(home, &provider).map_err(OpError::io)?;
    object(json!({
        "provider":provider,
        "provider_state":provider_runtime_state(home, &provider)?,
        "credential":credential,
        "auth":{"provider":provider,"state":if matches!(action.as_str(),"cancel"|"disconnect"){"canceled"}else if action=="start"{"running"}else{"idle"},"updated_at":utc_now(),"error":null}
    }))
}

pub(super) fn credential_status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let provider = provider(request);
    require_notebooklm(&provider)?;
    object(
        json!({"provider":provider,"credential":space_credentials::status(home,&provider).map_err(OpError::io)?}),
    )
}

pub(super) fn credential_update(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let credential = if request
        .args
        .get("clear")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        space_credentials::clear(home, &provider)
    } else {
        let auth_json = required_arg(request, "auth_json")?;
        space_credentials::update(home, &provider, &auth_json)
    }
    .map_err(OpError::invalid)?;
    if credential["source"].as_str() != Some("env") {
        update_provider(home, &provider, |value| {
            value["enabled"] = json!(false);
            value["mode"] = json!("disabled");
            if let Some(item) = value.as_object_mut() {
                item.remove("healthy");
                item.remove("last_health_at");
                item.remove("last_error");
            }
            Ok(())
        })?;
    }
    object(json!({"provider":provider,"credential":credential}))
}

pub(super) fn provider_health(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    require_user(request)?;
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let credential = space_credentials::status(home, &provider).map_err(OpError::io)?;
    let candidate = string_arg(request, "auth_json").filter(|value| !value.trim().is_empty());
    let checked_at = utc_now();
    let health = candidate
        .as_deref()
        .map_or_else(|| notebooklm::health(home), notebooklm::health_candidate);
    match health {
        Ok(()) => {
            if candidate.is_none() {
                record_provider_health(home, &provider, true, &checked_at, None)?;
            }
            object(
                json!({"provider":provider,"healthy":true,"health":{"checked_at":checked_at},"error":null,"provider_state":provider_runtime_state(home,&provider)?,"credential":credential}),
            )
        }
        Err(error) => {
            if candidate.is_none() {
                record_provider_health(home, &provider, false, &checked_at, Some(&error.message))?;
            }
            object(
                json!({"provider":provider,"healthy":false,"health":{"checked_at":checked_at},"error":{"code":error.code,"message":error.message},"provider_state":provider_runtime_state(home,&provider)?,"credential":credential}),
            )
        }
    }
}

pub(super) fn spaces(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let provider = provider(request);
    let value = load(home, &group_id)?;
    require_notebooklm(&provider)?;
    let spaces = notebooklm::notebooks(home)?.into_iter().map(|notebook| json!({"remote_space_id":notebook.id,"title":notebook.title,"is_owner":notebook.is_owner,"sources_count":notebook.sources_count})).collect::<Vec<_>>();
    object(
        json!({"group_id":group_id,"provider":provider,"provider_state":provider_runtime_state(home,&provider)?,"bindings":value["bindings"],"spaces":spaces}),
    )
}
