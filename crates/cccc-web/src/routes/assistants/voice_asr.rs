use super::super::first_non_blank;
use cccc_core::HomeLayout;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

mod engine;
mod install;
mod payload;
pub use engine::{
    StreamingSession, clean_transcript, diarize_pcm16_file, transcribe_file,
    transcribe_pcm16_ranges, transcribe_pcm16_ranges_partial,
};
use install::{InstallLock, LockProbe};
pub use install::{begin_install, spawn_startup_cleanup};
use payload::ModelSnapshot;

const BUILTIN_MANIFEST: &str = include_str!("../../../resources/voice-models.default.json");
pub const DEFAULT_OFFLINE_MODEL_ID: &str = "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8";
pub const DEFAULT_STREAMING_MODEL_ID: &str =
    "sherpa_onnx_streaming_paraformer_trilingual_zh_cantonese_en";
pub const DEFAULT_DIARIZATION_MODEL_ID: &str = "sherpa_onnx_diarization_pyannote_3dspeaker_zh";
pub const MAX_AUDIO_BYTES: usize = 100 * 1024 * 1024;
const RUNTIME_ID: &str = "sherpa_onnx_streaming";
const INSTALL_STATE: &str = "install-state.json";
/// The lock marker older versions kept in the model directory; locks now live under `.locks`.
const INSTALL_LOCK: &str = ".install.lock";

#[derive(Debug, Clone)]
pub struct VoiceError {
    pub code: &'static str,
    pub message: String,
    pub details: Map<String, Value>,
}

impl VoiceError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Map::new(),
        }
    }

    fn detail(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

impl From<io::Error> for VoiceError {
    fn from(error: io::Error) -> Self {
        Self::new("voice_io_error", error.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    #[serde(default)]
    voice_secretary_asr_models: Vec<Model>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    model_id: String,
    #[serde(default = "default_kind")]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    required_files: Vec<String>,
    #[serde(default)]
    artifacts: Vec<Artifact>,
    #[serde(default)]
    offline: Option<OfflineConfig>,
    #[serde(default)]
    streaming: Option<StreamingConfig>,
    #[serde(default)]
    diarization: Option<DiarizationConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct Artifact {
    path: String,
    url: String,
    sha256: String,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    archive: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OfflineConfig {
    engine: String,
    model: String,
    tokens: String,
    #[serde(default = "default_sample_rate")]
    sample_rate: i32,
    #[serde(default = "default_threads")]
    num_threads: i32,
    #[serde(default = "default_provider")]
    provider: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    use_itn: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct StreamingConfig {
    engine: String,
    encoder: String,
    decoder: String,
    tokens: String,
    #[serde(default)]
    joiner: String,
    #[serde(default = "default_sample_rate")]
    sample_rate: i32,
    #[serde(default = "default_threads")]
    num_threads: i32,
    #[serde(default = "default_provider")]
    provider: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DiarizationConfig {
    engine: String,
    segmentation_model: String,
    embedding_model: String,
    #[serde(default = "default_sample_rate")]
    sample_rate: i32,
    #[serde(default = "default_threads")]
    num_threads: i32,
    #[serde(default = "default_provider")]
    provider: String,
    #[serde(default = "default_num_speakers")]
    num_speakers: i32,
    #[serde(default = "default_cluster_threshold")]
    cluster_threshold: f32,
    #[serde(default = "default_min_duration_on")]
    min_duration_on: f32,
    #[serde(default = "default_min_duration_off")]
    min_duration_off: f32,
}

fn default_kind() -> String {
    "asr".into()
}
const fn default_sample_rate() -> i32 {
    16_000
}
const fn default_threads() -> i32 {
    2
}
fn default_provider() -> String {
    "cpu".into()
}
fn default_language() -> String {
    "auto".into()
}
const fn default_num_speakers() -> i32 {
    -1
}
fn default_cluster_threshold() -> f32 {
    0.5
}
fn default_min_duration_on() -> f32 {
    0.3
}
fn default_min_duration_off() -> f32 {
    0.5
}

pub fn runtime_status() -> Value {
    json!({
        "runtime_id": RUNTIME_ID,
        "title": "sherpa-onnx ASR",
        "installed": true,
        "available": true,
        "status": "ready",
        "installed_version": "1.13.4",
        "reason": "sherpa-onnx is included in the CCCC executable"
    })
}

pub fn streaming_backend_status(home: &HomeLayout, requested: &str) -> Value {
    let catalog = match catalog(home) {
        Ok(value) => value,
        Err(error) => {
            return json!({
                "ready":false,
                "status":"failed",
                "model_id":"",
                "error":{"code":error.code,"message":error.message,"details":error.details}
            });
        }
    };
    let model = catalog
        .get(requested)
        .filter(|model| model.streaming.is_some())
        .or_else(|| catalog.get(DEFAULT_STREAMING_MODEL_ID));
    let Some(model) = model else {
        return json!({
            "ready":false,
            "status":"not_installed",
            "model_id":DEFAULT_STREAMING_MODEL_ID
        });
    };
    let status = model_status(home, model);
    json!({
        "ready":status["streaming_ready"].as_bool().unwrap_or(false),
        "status":status["status"],
        "model_id":model.model_id,
        "error":status["error"]
    })
}

pub fn list_models(home: &HomeLayout) -> Result<Vec<Value>, VoiceError> {
    Ok(catalog(home)?
        .into_values()
        .map(|model| model_status(home, &model))
        .collect())
}

pub fn diarization_available(home: &HomeLayout, model_id: &str) -> bool {
    let requested = if model_id.trim().is_empty() {
        DEFAULT_DIARIZATION_MODEL_ID
    } else {
        model_id
    };
    catalog(home)
        .ok()
        .and_then(|catalog| catalog.get(requested).cloned())
        .is_some_and(|model| {
            model.diarization.is_some() && model_status(home, &model)["status"] == "ready"
        })
}

pub async fn remove_model(home: HomeLayout, model_id: String) -> Result<Value, VoiceError> {
    tokio::task::spawn_blocking(move || remove_model_files(&home, &model_id))
        .await
        .map_err(|error| VoiceError::new("voice_model_remove_failed", error.to_string()))?
}

fn remove_model_files(home: &HomeLayout, model_id: &str) -> Result<Value, VoiceError> {
    let model = catalog(home)?.remove(model_id).ok_or_else(|| {
        VoiceError::new(
            "voice_model_not_found",
            format!("voice model not found: {model_id}"),
        )
    })?;
    let root = model_dir(home, model_id);
    let lock = InstallLock::acquire(&install_lock_path(home, model_id)).map_err(|error| {
        if error.code == "voice_model_install_busy" {
            VoiceError::new(
                "voice_model_install_busy",
                "cannot remove a voice model while installation is running",
            )
        } else {
            error
        }
    })?;
    let _ = fs::remove_file(root.join(INSTALL_LOCK));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    drop(lock);
    Ok(model_status(home, &model))
}

fn catalog(home: &HomeLayout) -> Result<BTreeMap<String, Model>, VoiceError> {
    let builtin: Manifest = serde_json::from_str(BUILTIN_MANIFEST)
        .map_err(|error| VoiceError::new("voice_model_manifest_invalid", error.to_string()))?;
    let mut models = builtin
        .voice_secretary_asr_models
        .into_iter()
        .map(|model| (model.model_id.clone(), model))
        .collect::<BTreeMap<_, _>>();
    let overlay = home.root().join("config/voice-models.json");
    // Only a path that does not exist means there is no overlay. One that cannot be read, such as
    // a dangling link, may define models that cleanup would otherwise judge by the built-in list.
    if !matches!(fs::symlink_metadata(&overlay), Err(error) if error.kind() == io::ErrorKind::NotFound)
    {
        let path = || overlay.to_string_lossy().into_owned();
        let bytes =
            fs::read(&overlay).map_err(|error| VoiceError::from(error).detail("path", path()))?;
        let local: Manifest = serde_json::from_slice(&bytes).map_err(|error| {
            VoiceError::new("voice_model_manifest_invalid", error.to_string())
                .detail("path", path())
        })?;
        for model in local.voice_secretary_asr_models {
            models.insert(model.model_id.clone(), model);
        }
    }
    for model in models.values() {
        validate_model(model)?;
    }
    Ok(models)
}

fn validate_model(model: &Model) -> Result<(), VoiceError> {
    // A leading letter or digit keeps a model directory apart from `.`, `..` and the cache's
    // own entries such as `.locks`. Windows drops trailing dots from a directory name, so
    // `model.` would share the directory of `model` but not its install lock.
    if !model
        .model_id
        .starts_with(|ch: char| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        || model.model_id.ends_with('.')
        || !model.model_id.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.')
        })
    {
        return Err(VoiceError::new(
            "voice_model_manifest_invalid",
            "model_id must be a simple lowercase slug",
        ));
    }
    for path in model
        .required_files
        .iter()
        .chain(model.artifacts.iter().map(|item| &item.path))
    {
        validate_relative(path)?;
    }
    for artifact in &model.artifacts {
        if artifact.sha256.len() != 64 || !artifact.sha256.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(VoiceError::new(
                "voice_model_manifest_invalid",
                format!("invalid sha256 for {}", artifact.path),
            ));
        }
        if !artifact.url.starts_with("https://") && !artifact.url.starts_with("http://") {
            return Err(VoiceError::new(
                "voice_model_manifest_invalid",
                format!("unsupported artifact URL: {}", artifact.url),
            ));
        }
    }
    Ok(())
}

fn model_status(home: &HomeLayout, model: &Model) -> Value {
    status_from(home, model, &payload::snapshot(home, &model.model_id))
}

/// A model's status from one snapshot of its install state, so that status, manifest and
/// directory all describe the same version.
fn status_from(home: &HomeLayout, model: &Model, snapshot: &ModelSnapshot) -> Value {
    let state = &snapshot.state;
    let files_ready = snapshot
        .root
        .as_deref()
        .is_some_and(|root| files_complete(model, root));
    let declared = state["status"].as_str().unwrap_or("not_installed");
    // Only a lock known to be free shows that a recorded download no longer runs.
    let interrupted = declared == "downloading"
        && install::probe_install_lock(&install_lock_path(home, &model.model_id))
            == LockProbe::Free;
    let interrupted_ready = interrupted
        && files_ready
        && state["installed_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty());
    let (status, error) = if let Some(reason) = &snapshot.unusable {
        let error = state_unusable(&model_dir(home, &model.model_id), reason);
        (
            "failed",
            json!({"code":error.code,"message":error.message,"details":error.details}),
        )
    } else if interrupted_ready {
        ("ready", state["error"].clone())
    } else if interrupted {
        (
            "failed",
            json!({"code":"voice_model_install_interrupted","message":"voice model installation was interrupted","details":{}}),
        )
    } else if declared == "ready" && !files_ready {
        ("not_installed", state["error"].clone())
    } else {
        (declared, state["error"].clone())
    };
    let install_dir = snapshot
        .root
        .clone()
        .unwrap_or_else(|| model_dir(home, &model.model_id));
    let manifest_sha = model_manifest_sha(model).unwrap_or_default();
    json!({
        "model_id":model.model_id,"kind":model.kind,"title":if model.title.is_empty(){&model.model_id}else{&model.title},
        "description":model.description,"runtime_id":RUNTIME_ID,"status":status,"available":true,"installed":status=="ready",
        "install_dir":install_dir,"installed_at":state["installed_at"],"updated_at":state["updated_at"],"error":error,
        "last_update_error":state["last_update_error"],"manifest_sha256":manifest_sha,
        "installed_manifest_sha256":first_non_blank(state,&["installed_manifest_sha256","manifest_sha256"]),
        "update_available":status=="ready" && first_non_blank(state,&["installed_manifest_sha256","manifest_sha256"]).is_some_and(|value|value!=manifest_sha),
        "offline_ready":status=="ready"&&model.offline.is_some(),"streaming_ready":status=="ready"&&model.streaming.is_some(),
        "diarization_ready":status=="ready"&&model.diarization.is_some(),"offline":model.offline.as_ref().map(|_|json!({"configured":true})).unwrap_or(json!({})),
        "streaming":model.streaming.as_ref().map(|_|json!({"configured":true})).unwrap_or(json!({})),
        "downloaded_bytes":state["downloaded_bytes"],"total_size_bytes":state["total_size_bytes"],"progress_percent":state["progress_percent"],
        "current_artifact_path":state["current_artifact_path"],"artifact_index":state["artifact_index"],"artifact_count":model.artifacts.len(),
        "artifacts":model.artifacts.iter().map(|item|json!({"path":item.path,"url":item.url,"sha256":item.sha256,"size_bytes":item.size_bytes,"archive":item.archive})).collect::<Vec<_>>()
    })
}

fn model_manifest_sha(model: &Model) -> Result<String, VoiceError> {
    let value = json!({"model_id":model.model_id,"required_files":model.required_files,"artifacts":model.artifacts.iter().map(|item|json!({"path":item.path,"url":item.url,"sha256":item.sha256,"size_bytes":item.size_bytes,"archive":item.archive})).collect::<Vec<_>>()});
    let bytes = serde_json::to_vec(&value)
        .map_err(|error| VoiceError::new("voice_model_manifest_invalid", error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn model_dir(home: &HomeLayout, model_id: &str) -> PathBuf {
    home.root().join("cache/voice-models").join(model_id)
}
fn install_lock_path(home: &HomeLayout, model_id: &str) -> PathBuf {
    home.root()
        .join("cache/voice-models/.locks")
        .join(format!("{model_id}.lock"))
}
/// The state may only be unreadable, with the model intact, so the message puts restoring access
/// before reinstalling, which replaces the model's files.
fn state_unusable(model_root: &Path, reason: &str) -> VoiceError {
    let path = model_root.join(INSTALL_STATE);
    VoiceError::new(
        "voice_model_state_unreadable",
        format!(
            "cannot use the voice model install state at {} ({reason}); if the file is intact, restore read access to it, as reinstalling replaces this model's files",
            path.display()
        ),
    )
    .detail("path", path.to_string_lossy().into_owned())
}
/// Whether a version directory holds every file the model requires.
fn files_complete(model: &Model, root: &Path) -> bool {
    model
        .required_files
        .iter()
        .all(|path| root.join(path).is_file())
}
fn total_size(model: &Model) -> u64 {
    model
        .artifacts
        .iter()
        .filter_map(|item| item.size_bytes)
        .sum()
}
fn validate_relative(value: &str) -> Result<(), VoiceError> {
    validate_relative_path(Path::new(value))
}
fn validate_relative_path(path: &Path) -> Result<(), VoiceError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(VoiceError::new(
            "voice_model_manifest_invalid",
            format!("path must stay inside model directory: {}", path.display()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_model_paths() {
        assert!(validate_relative("models/model.onnx").is_ok());
        assert!(validate_relative("../outside").is_err());
        assert!(validate_relative("/outside").is_err());
    }
    #[test]
    fn decodes_pcm16() {
        let samples = engine::pcm16_samples(&[0, 0, 0xff, 0x7f]);
        assert_eq!(samples.len(), 2);
        assert!(samples[1] > 0.99);
    }

    #[test]
    fn final_pcm16_rejects_incomplete_samples_before_loading_a_model() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let audio = temp.path().join("odd.pcm");
        std::fs::write(&audio, [0]).expect("audio");
        let error =
            engine::transcribe_pcm16_file(&home, DEFAULT_OFFLINE_MODEL_ID, &audio, 16_000, "auto")
                .expect_err("odd PCM16");
        assert_eq!(error.code, "invalid_audio");
    }

    #[test]
    fn recognizes_legacy_model_cache_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let model = catalog(&home)
            .expect("catalog")
            .remove(DEFAULT_OFFLINE_MODEL_ID)
            .expect("model");
        let root = model_dir(&home, DEFAULT_OFFLINE_MODEL_ID);
        for relative in &model.required_files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, b"fixture").expect("file");
        }
        cccc_core::fs::write_json(&root.join(INSTALL_STATE),&json!({"model_id":DEFAULT_OFFLINE_MODEL_ID,"status":"ready","installed_at":"2026-01-01T00:00:00Z","manifest_sha256":"legacy-python-state"})).expect("state");
        let status = model_status(&home, &model);
        assert_eq!(status["status"], "ready");
        assert_eq!(status["installed"], true);
        assert_eq!(status["runtime_id"], "sherpa_onnx_streaming");
    }

    #[test]
    fn interrupted_update_keeps_complete_previous_model_ready() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let model = catalog(&home)
            .expect("catalog")
            .remove(DEFAULT_OFFLINE_MODEL_ID)
            .expect("model");
        let root = model_dir(&home, DEFAULT_OFFLINE_MODEL_ID);
        for relative in &model.required_files {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, b"fixture").expect("file");
        }
        cccc_core::fs::write_json(&root.join(INSTALL_STATE),&json!({"model_id":DEFAULT_OFFLINE_MODEL_ID,"status":"downloading","installed_at":"2026-01-01T00:00:00Z","installed_manifest_sha256":"previous"})).expect("state");
        let status = model_status(&home, &model);
        assert_eq!(status["status"], "ready");
        assert_eq!(status["installed"], true);
    }

    #[test]
    fn operating_system_lock_recovers_stale_marker_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(INSTALL_LOCK);
        std::fs::write(&path, b"stale pid\n").expect("marker");
        let lock = InstallLock::acquire(&path).expect("recover lock");
        assert_eq!(install::probe_install_lock(&path), LockProbe::Held);
        drop(lock);
        assert!(path.exists());
        assert_eq!(install::probe_install_lock(&path), LockProbe::Free);
    }

    #[test]
    fn remove_model_respects_the_installation_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let lock = InstallLock::acquire(&install_lock_path(&home, DEFAULT_OFFLINE_MODEL_ID))
            .expect("install lock");
        let error = remove_model_files(&home, DEFAULT_OFFLINE_MODEL_ID).expect_err("busy removal");
        assert_eq!(error.code, "voice_model_install_busy");
        drop(lock);
        assert!(remove_model_files(&home, DEFAULT_OFFLINE_MODEL_ID).is_ok());
    }

    #[test]
    fn unavailable_diarization_model_is_skipped_before_audio_is_queued() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        assert!(!diarization_available(&home, DEFAULT_DIARIZATION_MODEL_ID));
    }
}
