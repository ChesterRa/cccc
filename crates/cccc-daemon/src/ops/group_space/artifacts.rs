use super::*;
use std::path::{Path, PathBuf};

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
        let provider = provider(request);
        let artifacts = if provider == "notebooklm" {
            let remote_space_id = binding_id(&value, &lane)?;
            notebooklm::artifacts(home, &remote_space_id)?
                .into_iter()
                .filter(|item| kind.is_empty() || item.kind == kind)
                .map(|item| serde_json::to_value(item).unwrap_or(Value::Null))
                .collect::<Vec<_>>()
        } else {
            require_local(&provider)?;
            array(&value, "artifacts")
                .iter()
                .filter(|item| kind.is_empty() || item["kind"] == kind)
                .cloned()
                .collect::<Vec<_>>()
        };
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"action":"list","kind":kind,"artifacts":artifacts,"list_result":{"cached":true,"artifacts":artifacts}}),
        );
    }
    let kind = normalize_kind(&required_arg(request, "kind")?)?;
    if action == "download" {
        let artifact_id = required_arg(request, "artifact_id")?;
        let provider = provider(request);
        let output = output_path(home, &group_id, request, &artifact_id, &kind)?;
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(OpError::io)?;
        }
        if provider == "notebooklm" {
            let remote_space_id = binding_id(&value, &lane)?;
            let artifact = notebooklm::artifacts(home, &remote_space_id)?
                .into_iter()
                .find(|item| item.id == artifact_id)
                .ok_or_else(|| OpError::new("not_found", "artifact not found"))?;
            let bytes = notebooklm::download_artifact(home, &artifact)?;
            std::fs::write(&output, bytes).map_err(OpError::io)?;
        } else {
            require_local(&provider)?;
            let artifact = array(&value, "artifacts")
                .iter()
                .find(|item| item["artifact_id"] == artifact_id)
                .ok_or_else(|| OpError::new("not_found", "artifact not found"))?;
            let source = artifact["output_path"]
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| OpError::new("not_found", "artifact output is unavailable"))?;
            if output != source {
                std::fs::copy(source, &output).map_err(OpError::io)?;
            }
        }
        return object(
            json!({"group_id":group_id,"provider":provider,"lane":lane,"action":"download","kind":kind,"output_path":output,"download_result":{"output_path":output}}),
        );
    }
    if action != "generate" {
        return Err(OpError::new(
            "invalid_args",
            "action must be list, generate, or download",
        ));
    }
    let provider = provider(request);
    if provider != "notebooklm" {
        require_local(&provider)?;
    }
    let remote_space_id = binding_id(&value, &lane)?;
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
    let generation = if provider == "notebooklm" {
        notebooklm::generate_artifact(
            home,
            &remote_space_id,
            &kind,
            language,
            instructions,
            source_ids.as_deref(),
        )?
    } else {
        cccc_notebooklm::ArtifactGeneration {
            artifact_id: format!("gsa_{}", short_id()),
            kind: kind.clone(),
            status: "completed".into(),
            raw: json!({}),
        }
    };
    let artifact_id = generation.artifact_id.clone();
    let artifact = json!({
        "artifact_id":artifact_id,"provider":provider,"lane":lane,"remote_space_id":remote_space_id,
        "kind":kind,"status":generation.status,"created_at":utc_now(),"updated_at":utc_now(),
        "generation_backend":if provider=="notebooklm"{"notebooklm_studio"}else{"local"},
        "provider_result":generation.raw
    });
    update(home, &group_id, |value| {
        array_mut(root(value), "artifacts").push(artifact.clone());
        Ok(())
    })?;
    object(json!({
        "group_id":group_id,"provider":provider,"lane":lane,"action":"generate","kind":kind,
        "artifact":artifact,"artifact_id":artifact_id,"status":generation.status,
        "completed":generation.status=="completed",
        "generate_result":{"status":generation.status,"artifact_id":artifact_id}
    }))
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
    Ok(home
        .root()
        .join("groups")
        .join(group_id)
        .join("space/artifacts")
        .join(format!(
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
