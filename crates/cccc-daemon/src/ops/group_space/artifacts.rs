use super::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

const KINDS: &[&str] = &[
    "audio",
    "video",
    "report",
    "study_guide",
    "quiz",
    "flashcards",
    "infographic",
    "slide_deck",
    "data_table",
    "mind_map",
];

pub(super) fn handle(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let lane = lane(request)?;
    if lane != "work" {
        return Err(OpError::new(
            "space_lane_unsupported",
            "artifacts require lane=work",
        ));
    }
    let action = string_arg(request, "action").unwrap_or_else(|| "list".into());
    let value = load(home, &group_id)?;
    if action == "list" {
        let kind = string_arg(request, "kind").unwrap_or_default();
        let provider_kind = if kind.is_empty() {
            String::new()
        } else {
            let normalized = normalize_kind(&kind)?;
            if normalized == "study_guide" {
                "report".to_owned()
            } else {
                normalized
            }
        };
        let provider = provider(request);
        require_notebooklm(&provider)?;
        let remote_space_id = binding_id(&value, &lane)?;
        let artifacts = notebooklm::artifacts(home, &remote_space_id)?
            .into_iter()
            .filter(|item| provider_kind.is_empty() || item.kind == provider_kind)
            .map(|item| serde_json::to_value(item).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"action":"list","kind":kind,"artifacts":artifacts,"list_result":{"cached":false,"artifacts":artifacts}}),
        );
    }
    let kind = normalize_kind(&required_arg(request, "kind")?)?;
    if action == "download" {
        let artifact_id = required_arg(request, "artifact_id")?;
        let provider = provider(request);
        let save_to_space = bool_arg(request, "save_to_space", false);
        let output_format = string_arg(request, "output_format").unwrap_or_default();
        if !save_to_space
            && string_arg(request, "output_path").is_none_or(|path| path.trim().is_empty())
        {
            return Err(OpError::new(
                "invalid_args",
                "output_path is required when save_to_space=false",
            ));
        }
        validate_native_download(&kind, &output_format)?;
        let output = output_path(home, &group_id, request, &artifact_id, &kind)?;
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(OpError::io)?;
        }
        require_notebooklm(&provider)?;
        let remote_space_id = binding_id(&value, &lane)?;
        let artifact = notebooklm::artifacts(home, &remote_space_id)?
            .into_iter()
            .find(|item| item.id == artifact_id)
            .ok_or_else(|| OpError::new("not_found", "artifact not found"))?;
        if artifact.kind != provider_kind(&kind) {
            return Err(OpError::new(
                "invalid_args",
                format!(
                    "artifact kind mismatch: requested {kind}, provider returned {}",
                    artifact.kind
                ),
            ));
        }
        let bytes = notebooklm::download_artifact(home, &artifact, Some(&output_format))?;
        std::fs::write(&output, bytes).map_err(OpError::io)?;
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"action":"download","kind":kind,"saved_to_space":save_to_space,"output_path":output,"download_result":{"output_path":output}}),
        );
    }
    if action != "generate" {
        return Err(OpError::new(
            "invalid_args",
            "action must be list, generate, or download",
        ));
    }
    let provider = provider(request);
    require_notebooklm(&provider)?;
    let remote_space_id = binding_id(&value, &lane)?;
    let wait = bool_arg(request, "wait", false);
    let save_to_space = bool_arg(request, "save_to_space", false);
    let output_format = string_arg(request, "output_format").unwrap_or_default();
    if save_to_space {
        validate_native_download(&kind, &output_format)?;
        preflight_output_destination(home, &group_id, request, &kind)?;
    }
    let options = request.args.get("options").and_then(Value::as_object);
    let language = options
        .and_then(|options| options.get("language"))
        .and_then(Value::as_str)
        .unwrap_or("en");
    let instructions = options
        .and_then(|options| options.get("instructions"))
        .and_then(Value::as_str);
    let source_ids = options
        .and_then(|options| options.get("source_ids"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    let generation = notebooklm::generate_artifact(
        home,
        &remote_space_id,
        &kind,
        language,
        instructions,
        source_ids.as_deref(),
    )?;
    let artifact_id = generation.artifact_id.clone();
    let mut status = generation.status.clone();
    let mut completed_artifact = None;
    let mut wait_result = json!({});
    if wait && !terminal_status(&status) {
        let timeout = duration_arg(request, "timeout_seconds", 600.0, 10.0, 3_600.0);
        let initial_interval = duration_arg(request, "initial_interval", 2.0, 0.5, 60.0);
        let max_interval =
            duration_arg(request, "max_interval", 10.0, 1.0, 120.0).max(initial_interval);
        let artifact = notebooklm::wait_artifact(
            home,
            &remote_space_id,
            &artifact_id,
            timeout,
            initial_interval,
            max_interval,
        )?;
        status.clone_from(&artifact.status);
        wait_result = json!({
            "task_id":artifact_id,
            "artifact_id":artifact.id,
            "status":artifact.status
        });
        completed_artifact = Some(artifact);
    }

    if completed_artifact.is_none() && status == "completed" {
        completed_artifact = notebooklm::artifacts(home, &remote_space_id)?
            .into_iter()
            .find(|artifact| artifact.id == artifact_id);
    }

    let mut output = None;
    let mut download_result = json!({});
    if save_to_space && status == "completed" {
        let artifact = completed_artifact.as_ref().ok_or_else(|| {
            OpError::new(
                "space_provider_compat_mismatch",
                "completed artifact was absent from the NotebookLM artifact list",
            )
        })?;
        let target = output_path(home, &group_id, request, &artifact.id, &kind)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(OpError::io)?;
        }
        if artifact.kind != provider_kind(&kind) {
            return Err(OpError::new(
                "space_provider_compat_mismatch",
                format!(
                    "generated artifact kind mismatch: requested {kind}, provider returned {}",
                    artifact.kind
                ),
            ));
        }
        let bytes = notebooklm::download_artifact(home, artifact, Some(&output_format))?;
        std::fs::write(&target, bytes).map_err(OpError::io)?;
        download_result = json!({"output_path":target,"artifact_id":artifact.id});
        output = Some(target);
    }
    let artifact = json!({
        "artifact_id":artifact_id,"provider":provider,"lane":lane,"remote_space_id":remote_space_id,
        "kind":kind,"status":status,"created_at":utc_now(),"updated_at":utc_now(),
        "generation_backend":"notebooklm_studio",
        "provider_result":generation.raw
    });
    update(home, &group_id, |value| {
        array_mut(root(value), "artifacts").push(artifact.clone());
        Ok(())
    })?;
    object(json!({
        "group_id":group_id,"provider":provider,"lane":lane,"action":"generate","kind":kind,
        "artifact":artifact,"artifact_id":artifact_id,"task_id":artifact_id,"status":status,
        "wait":wait,"completed":terminal_status(&status),"accepted":!terminal_status(&status),
        "saved_to_space":output.is_some(),"output_path":output,
        "generate_result":{"status":generation.status,"artifact_id":artifact_id},
        "wait_result":wait_result,"download_result":download_result
    }))
}

fn terminal_status(status: &str) -> bool {
    matches!(status, "completed" | "failed")
}

fn duration_arg(request: &DaemonRequest, name: &str, default: f64, min: f64, max: f64) -> Duration {
    let seconds = request
        .args
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .unwrap_or(default)
        .clamp(min, max);
    Duration::from_secs_f64(seconds)
}

fn normalize_kind(raw: &str) -> Result<String, OpError> {
    let value = match raw {
        "study" | "studyguide" => "study_guide",
        "slides" | "slide" | "deck" | "slidedeck" => "slide_deck",
        "table" | "datatable" => "data_table",
        "mindmap" => "mind_map",
        other => other,
    };
    KINDS
        .contains(&value)
        .then(|| value.to_owned())
        .ok_or_else(|| OpError::new("invalid_args", format!("unsupported artifact kind: {raw}")))
}

fn provider_kind(kind: &str) -> &str {
    if kind == "study_guide" {
        "report"
    } else {
        kind
    }
}

fn validate_native_download(kind: &str, output_format: &str) -> Result<(), OpError> {
    match kind {
        "audio" | "video" | "report" | "study_guide" | "infographic" => Ok(()),
        "slide_deck" if matches!(output_format, "" | "pdf" | "pptx") => Ok(()),
        "slide_deck" => Err(OpError::new(
            "invalid_args",
            "native slide deck downloads support output_format=pdf or pptx",
        )),
        "quiz" | "flashcards" | "mind_map" | "data_table" => Err(OpError::new(
            "capability_unavailable",
            format!(
                "native Rust artifact download is not yet available for kind={kind}; generate with save_to_space=false or use the Python implementation"
            ),
        )),
        _ => Err(OpError::new(
            "invalid_args",
            format!("unsupported artifact kind: {kind}"),
        )),
    }
}

fn preflight_output_destination(
    home: &HomeLayout,
    group_id: &str,
    request: &DaemonRequest,
    kind: &str,
) -> Result<(), OpError> {
    let preview = output_path(home, group_id, request, "pending", kind)?;
    let parent = preview.parent().ok_or_else(|| {
        OpError::new(
            "invalid_args",
            "artifact output path must have a writable parent directory",
        )
    })?;
    std::fs::create_dir_all(parent).map_err(OpError::io)
}

fn output_path(
    home: &HomeLayout,
    group_id: &str,
    request: &DaemonRequest,
    artifact_id: &str,
    kind: &str,
) -> Result<PathBuf, OpError> {
    if let Some(path) = string_arg(request, "output_path").filter(|value| !value.is_empty()) {
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            return Ok(candidate);
        }
        let group = GroupStore::new(home.clone())
            .and_then(|store| store.load(group_id))
            .map_err(OpError::io)?;
        let scope = group
            .scopes
            .iter()
            .find(|scope| scope.scope_key == group.active_scope_key)
            .or_else(|| group.scopes.first())
            .ok_or_else(|| {
                OpError::new(
                    "scope_required",
                    "relative artifact output requires an active scope",
                )
            })?;
        return Ok(Path::new(&scope.url).join(candidate));
    }
    let group = GroupStore::new(home.clone())
        .and_then(|store| store.load(group_id))
        .map_err(OpError::io)?;
    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
        .ok_or_else(|| OpError::new("scope_required", "artifact save requires an active scope"))?;
    Ok(Path::new(&scope.url).join("space/artifacts").join(format!(
        "{kind}-{artifact_id}.{}",
        artifact_extension(kind, string_arg(request, "output_format").as_deref())
    )))
}

fn artifact_extension(kind: &str, output_format: Option<&str>) -> &'static str {
    match (kind, output_format.unwrap_or("")) {
        ("audio" | "video", _) => "mp4",
        ("infographic", _) => "png",
        ("slide_deck", "pptx") => "pptx",
        ("slide_deck", _) => "pdf",
        ("data_table", _) => "csv",
        ("quiz" | "flashcards", _) => "html",
        ("mind_map", _) => "json",
        _ => "md",
    }
}

#[cfg(test)]
mod tests {
    use super::{provider_kind, validate_native_download};

    #[test]
    fn native_download_capabilities_fail_before_provider_side_effects() {
        for kind in ["quiz", "flashcards", "mind_map", "data_table"] {
            let error = validate_native_download(kind, "").expect_err("unsupported download");
            assert_eq!(error.code, "capability_unavailable", "{kind}");
        }
        for kind in ["audio", "video", "report", "study_guide", "infographic"] {
            validate_native_download(kind, "").expect("supported download");
        }
        validate_native_download("slide_deck", "pdf").expect("PDF slides");
        validate_native_download("slide_deck", "pptx").expect("PPTX slides");
        assert_eq!(
            validate_native_download("slide_deck", "html")
                .expect_err("unsupported slide format")
                .code,
            "invalid_args"
        );
        assert_eq!(provider_kind("study_guide"), "report");
    }
}
