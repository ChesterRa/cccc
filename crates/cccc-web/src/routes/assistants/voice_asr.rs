use bzip2::read::BzDecoder;
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use flate2::read::GzDecoder;
use fs2::FileExt;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use tar::Archive;
use uuid::Uuid;

mod engine;
pub use engine::{
    StreamingSession, clean_transcript, diarize_pcm16_file, transcribe_file, transcribe_pcm16_file,
    transcribe_pcm16_ranges,
};

const BUILTIN_MANIFEST: &str = include_str!("../../../resources/voice-models.default.json");
pub const DEFAULT_OFFLINE_MODEL_ID: &str = "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8";
pub const DEFAULT_STREAMING_MODEL_ID: &str =
    "sherpa_onnx_streaming_paraformer_trilingual_zh_cantonese_en";
pub const DEFAULT_DIARIZATION_MODEL_ID: &str = "sherpa_onnx_diarization_pyannote_3dspeaker_zh";
pub const MAX_AUDIO_BYTES: usize = 100 * 1024 * 1024;
const RUNTIME_ID: &str = "sherpa_onnx_streaming";
const INSTALL_STATE: &str = "install-state.json";
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
        "title": "sherpa-onnx native Rust ASR",
        "installed": true,
        "available": true,
        "status": "ready",
        "managed": true,
        "implementation": "rust",
        "removable": false,
        "primary_package": "sherpa-onnx",
        "installed_version": "1.13.4",
        "reason": "sherpa-onnx is linked into the CCCC Rust binary"
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

pub fn begin_install(home: HomeLayout, model_id: String) -> Result<Value, VoiceError> {
    let model = catalog(&home)?.remove(&model_id).ok_or_else(|| {
        VoiceError::new(
            "voice_model_not_found",
            format!("voice model not found: {model_id}"),
        )
    })?;
    let root = model_dir(&home, &model.model_id);
    fs::create_dir_all(&root)?;
    let lock = InstallLock::acquire(install_lock_path(&home, &model.model_id))?;
    let previous = read_state(&root);
    write_state(
        &root,
        &json!({
            "model_id":model.model_id,
            "status":"downloading",
            "downloaded_bytes":0,
            "total_size_bytes":model.artifacts.iter().filter_map(|item|item.size_bytes).sum::<u64>(),
            "progress_percent":0.0,
            "updated_at":utc_now(),
            "installed_at":previous["installed_at"],
            "installed_manifest_sha256":super::super::first_non_blank(&previous,&["installed_manifest_sha256","manifest_sha256"]),
            "error":{},"last_update_error":previous["last_update_error"]
        }),
    )?;
    let initial = model_status(&home, &model);
    tokio::spawn(async move {
        if let Err(error) = install_model(&home, &model, lock).await {
            let root = model_dir(&home, &model.model_id);
            let previous = read_state(&root);
            let preserved_ready = previous["installed_at"]
                .as_str()
                .is_some_and(|value| !value.is_empty());
            let _ = write_state(
                &root,
                &json!({
                    "model_id":model.model_id,
                    "status":if preserved_ready {"ready"} else {"failed"},
                    "installed_at":previous["installed_at"],
                    "installed_manifest_sha256":previous["installed_manifest_sha256"],
                    "updated_at":utc_now(),
                    "error":if preserved_ready{json!({})}else{json!({"code":error.code,"message":error.message,"details":error.details})},
                    "last_update_error":{"code":error.code,"message":error.message,"details":error.details}
                }),
            );
        }
    });
    Ok(initial)
}

pub fn remove_model(home: &HomeLayout, model_id: &str) -> Result<Value, VoiceError> {
    let model = catalog(home)?.remove(model_id).ok_or_else(|| {
        VoiceError::new(
            "voice_model_not_found",
            format!("voice model not found: {model_id}"),
        )
    })?;
    let root = model_dir(home, model_id);
    let lock = InstallLock::acquire(install_lock_path(home, model_id)).map_err(|error| {
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

async fn install_model(
    home: &HomeLayout,
    model: &Model,
    lock: InstallLock,
) -> Result<(), VoiceError> {
    let root = model_dir(home, &model.model_id);
    fs::create_dir_all(&root)?;
    let staging = root.join(format!(".install-staging-{}", Uuid::new_v4().simple()));
    fs::create_dir(&staging)?;
    let result = install_into(home, model, &staging).await.and_then(|_| {
        replace_payloads(&root, &staging)?;
        let manifest_sha = model_manifest_sha(model)?;
        write_state(
            &root,
            &json!({
                "model_id":model.model_id,
                "status":"ready",
                "installed_at":utc_now(),
                "updated_at":utc_now(),
                "manifest_sha256":manifest_sha,
                "installed_manifest_sha256":manifest_sha,
                "downloaded_bytes":model.artifacts.iter().filter_map(|item|item.size_bytes).sum::<u64>(),
                "total_size_bytes":model.artifacts.iter().filter_map(|item|item.size_bytes).sum::<u64>(),
                "progress_percent":100.0,
                "error":{}
            }),
        )
    });
    let _ = fs::remove_dir_all(&staging);
    drop(lock);
    result
}

async fn install_into(home: &HomeLayout, model: &Model, staging: &Path) -> Result<(), VoiceError> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(3600))
        .build()
        .map_err(|error| VoiceError::new("voice_model_download_failed", error.to_string()))?;
    let total = model
        .artifacts
        .iter()
        .filter_map(|item| item.size_bytes)
        .sum::<u64>();
    let mut completed = 0_u64;
    for (index, artifact) in model.artifacts.iter().enumerate() {
        validate_relative(&artifact.path)?;
        let target = staging.join(&artifact.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut response = client
            .get(&artifact.url)
            .send()
            .await
            .map_err(|error| {
                VoiceError::new("voice_model_download_failed", error.to_string())
                    .detail("url", artifact.url.clone())
            })?
            .error_for_status()
            .map_err(|error| {
                VoiceError::new("voice_model_download_failed", error.to_string())
                    .detail("url", artifact.url.clone())
            })?;
        let part = target.with_extension(format!(
            "{}part",
            target.extension().and_then(|v| v.to_str()).unwrap_or("")
        ));
        let mut file = fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut artifact_bytes = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| VoiceError::new("voice_model_download_failed", error.to_string()))?
        {
            hasher.update(&chunk);
            file.write_all(&chunk)?;
            artifact_bytes += chunk.len() as u64;
            write_state(
                &model_dir(home, &model.model_id),
                &json!({
                    "model_id":model.model_id,"status":"downloading","updated_at":utc_now(),
                    "downloaded_bytes":completed+artifact_bytes,"total_size_bytes":total,
                    "progress_percent":if total>0{((completed+artifact_bytes) as f64/total as f64*100.0).min(99.9)}else{0.0},
                    "current_artifact_path":artifact.path,"artifact_index":index+1,"artifact_count":model.artifacts.len(),"error":{}
                }),
            )?;
        }
        file.sync_all()?;
        let actual = format!("{:x}", hasher.finalize());
        if !actual.eq_ignore_ascii_case(&artifact.sha256) {
            return Err(VoiceError::new(
                "voice_model_hash_mismatch",
                format!("downloaded artifact hash mismatch: {}", artifact.path),
            )
            .detail("expected_sha256", artifact.sha256.clone())
            .detail("actual_sha256", actual));
        }
        if artifact
            .size_bytes
            .is_some_and(|expected| expected != artifact_bytes)
        {
            return Err(VoiceError::new(
                "voice_model_size_mismatch",
                format!("downloaded artifact size mismatch: {}", artifact.path),
            ));
        }
        fs::rename(&part, &target)?;
        extract_artifact(&target, &artifact.archive, staging)?;
        completed += artifact_bytes;
    }
    for path in &model.required_files {
        validate_relative(path)?;
        if !staging.join(path).is_file() {
            return Err(VoiceError::new(
                "voice_model_required_file_missing",
                format!("model archive is missing required file: {path}"),
            ));
        }
    }
    Ok(())
}

fn extract_artifact(path: &Path, kind: &str, output: &Path) -> Result<(), VoiceError> {
    match kind {
        "" => Ok(()),
        "tar.bz2" | "tbz2" => unpack_tar(BzDecoder::new(fs::File::open(path)?), output),
        "tar.gz" | "tgz" => unpack_tar(GzDecoder::new(fs::File::open(path)?), output),
        "tar" => unpack_tar(fs::File::open(path)?, output),
        other => Err(VoiceError::new(
            "voice_model_archive_invalid",
            format!("unsupported voice model archive: {other}"),
        )),
    }
}

fn unpack_tar(reader: impl Read, output: &Path) -> Result<(), VoiceError> {
    let mut archive = Archive::new(reader);
    for entry in archive.entries().map_err(VoiceError::from)? {
        let mut entry = entry.map_err(VoiceError::from)?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err(VoiceError::new(
                "voice_model_archive_invalid",
                "archive contains a link or unsupported entry",
            ));
        }
        let path = entry.path().map_err(VoiceError::from)?.into_owned();
        validate_relative_path(&path)?;
        entry.unpack_in(output).map_err(VoiceError::from)?;
    }
    Ok(())
}

fn replace_payloads(root: &Path, staging: &Path) -> Result<(), VoiceError> {
    let backup = root.join(format!(".install-previous-{}", Uuid::new_v4().simple()));
    fs::create_dir(&backup)?;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if path == staging
            || path == backup
            || matches!(name, INSTALL_LOCK | INSTALL_STATE)
            || name.starts_with(".install-")
        {
            continue;
        }
        fs::rename(&path, backup.join(name))?;
    }
    let move_result = (|| -> Result<(), VoiceError> {
        for entry in fs::read_dir(staging)? {
            let path = entry?.path();
            let name = path.file_name().ok_or_else(|| {
                VoiceError::new("voice_model_replace_failed", "invalid staged model path")
            })?;
            fs::rename(&path, root.join(name))?;
        }
        Ok(())
    })();
    if move_result.is_err() {
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if path == staging
                || path == backup
                || matches!(name, INSTALL_LOCK | INSTALL_STATE)
                || name.starts_with(".install-")
            {
                continue;
            }
            if path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
        for entry in fs::read_dir(&backup)? {
            let path = entry?.path();
            if let Some(name) = path.file_name() {
                let _ = fs::rename(&path, root.join(name));
            }
        }
    }
    let _ = fs::remove_dir_all(staging);
    let _ = fs::remove_dir_all(backup);
    move_result
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
    if overlay.is_file() {
        let local: Manifest = serde_json::from_slice(&fs::read(&overlay)?).map_err(|error| {
            VoiceError::new("voice_model_manifest_invalid", error.to_string())
                .detail("path", overlay.to_string_lossy().into_owned())
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
    if model.model_id.is_empty()
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
    let root = model_dir(home, &model.model_id);
    let state = read_state(&root);
    let files_ready = model
        .required_files
        .iter()
        .all(|path| root.join(path).is_file());
    let declared = state["status"].as_str().unwrap_or("not_installed");
    let install_active = installation_active(&install_lock_path(home, &model.model_id));
    let interrupted_ready = declared == "downloading"
        && !install_active
        && files_ready
        && state["installed_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty());
    let status = if interrupted_ready {
        "ready"
    } else if declared == "downloading" && !install_active {
        "failed"
    } else if declared == "ready" && !files_ready {
        "not_installed"
    } else {
        declared
    };
    let error = if declared == "downloading" && !install_active && !interrupted_ready {
        json!({"code":"voice_model_install_interrupted","message":"voice model installation was interrupted","details":{}})
    } else {
        state["error"].clone()
    };
    let manifest_sha = model_manifest_sha(model).unwrap_or_default();
    json!({
        "model_id":model.model_id,"kind":model.kind,"title":if model.title.is_empty(){&model.model_id}else{&model.title},
        "description":model.description,"runtime_id":RUNTIME_ID,"status":status,"available":true,"installed":status=="ready",
        "install_dir":root,"installed_at":state["installed_at"],"updated_at":state["updated_at"],"error":error,
        "last_update_error":state["last_update_error"],"manifest_sha256":manifest_sha,
        "installed_manifest_sha256":super::super::first_non_blank(&state,&["installed_manifest_sha256","manifest_sha256"]),
        "update_available":status=="ready" && super::super::first_non_blank(&state,&["installed_manifest_sha256","manifest_sha256"]).is_some_and(|value|value!=manifest_sha),
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
fn read_state(root: &Path) -> Value {
    fs::read(root.join(INSTALL_STATE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({"status":"not_installed"}))
}
fn write_state(root: &Path, state: &Value) -> Result<(), VoiceError> {
    fs::create_dir_all(root)?;
    let target = root.join(INSTALL_STATE);
    let temp = root.join(format!(".{INSTALL_STATE}.{}.tmp", Uuid::new_v4().simple()));
    {
        let mut file = fs::File::create(&temp)?;
        serde_json::to_writer_pretty(&mut file, state)
            .map_err(|error| VoiceError::new("voice_state_error", error.to_string()))?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(temp, target)?;
    Ok(())
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

struct InstallLock {
    _file: fs::File,
}
impl InstallLock {
    fn acquire(path: PathBuf) -> Result<Self, VoiceError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        file.try_lock_exclusive().map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock {
                VoiceError::new(
                    "voice_model_install_busy",
                    "voice model installation is already running",
                )
            } else {
                VoiceError::from(error)
            }
        })?;
        file.set_len(0)?;
        writeln!(&file, "{}", std::process::id())?;
        Ok(Self { _file: file })
    }
}
impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self._file);
    }
}

fn installation_active(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    if file.try_lock_exclusive().is_err() {
        return true;
    }
    let _ = FileExt::unlock(&file);
    false
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
        write_state(&root,&json!({"model_id":DEFAULT_OFFLINE_MODEL_ID,"status":"ready","installed_at":"2026-01-01T00:00:00Z","manifest_sha256":"legacy-python-state"})).expect("state");
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
        write_state(&root,&json!({"model_id":DEFAULT_OFFLINE_MODEL_ID,"status":"downloading","installed_at":"2026-01-01T00:00:00Z","installed_manifest_sha256":"previous"})).expect("state");
        let status = model_status(&home, &model);
        assert_eq!(status["status"], "ready");
        assert_eq!(status["installed"], true);
    }

    #[test]
    fn operating_system_lock_recovers_stale_marker_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join(INSTALL_LOCK);
        std::fs::write(&path, b"stale pid\n").expect("marker");
        let lock = InstallLock::acquire(path.clone()).expect("recover lock");
        assert!(installation_active(&path));
        drop(lock);
        assert!(path.exists());
        assert!(!installation_active(&path));
    }

    #[test]
    fn remove_model_respects_the_installation_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let lock = InstallLock::acquire(install_lock_path(&home, DEFAULT_OFFLINE_MODEL_ID))
            .expect("install lock");
        let error = remove_model(&home, DEFAULT_OFFLINE_MODEL_ID).expect_err("busy removal");
        assert_eq!(error.code, "voice_model_install_busy");
        drop(lock);
        assert!(remove_model(&home, DEFAULT_OFFLINE_MODEL_ID).is_ok());
    }

    #[test]
    fn unavailable_diarization_model_is_skipped_before_audio_is_queued() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        assert!(!diarization_available(&home, DEFAULT_DIARIZATION_MODEL_ID));
    }
}
