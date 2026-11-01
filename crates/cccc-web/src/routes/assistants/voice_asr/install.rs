//! Installing a voice model. An install downloads, verifies and unpacks into a staging
//! directory, renames it to a new version directory, and then switches the install state to
//! that version in one atomic write. A failure at any point leaves the installed version
//! current. Blocking work runs on Tokio's blocking pool, and every piece of it holds the model's
//! install lock until it ends, even when the install that started it is cancelled.

use super::payload::{self, PAYLOAD_PREFIX, StateRead, unpinned, version_root};
use super::{
    INSTALL_LOCK, INSTALL_STATE, Model, VoiceError, catalog, files_complete, first_non_blank,
    install_lock_path, model_dir, model_manifest_sha, model_status, state_unusable, total_size,
    validate_relative, validate_relative_path,
};
use axum::body::Bytes;
use bzip2::read::BzDecoder;
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use flate2::read::GzDecoder;
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tar::Archive;
use tokio::sync::mpsc;
use uuid::Uuid;

const STAGING_PREFIX: &str = ".install-staging-";
/// The least time between two progress writes while a file downloads.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// A model's install lock: an operating-system file lock held while this value lives, so a
/// crashed process leaves no stale lock behind.
pub(super) struct InstallLock {
    file: fs::File,
}

impl InstallLock {
    /// Takes the lock, retrying briefly: a status poll takes it for an instant to see whether an
    /// install is running, and a click that lands on that instant is not a busy install.
    pub(super) fn acquire(path: &Path) -> Result<Self, VoiceError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let mut retries = 5;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                Err(error) if contended(&error) && retries > 0 => {
                    retries -= 1;
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) if contended(&error) => {
                    return Err(VoiceError::new(
                        "voice_model_install_busy",
                        "voice model installation is already running",
                    ));
                }
                Err(error) => return Err(error.into()),
            }
        }
        file.set_len(0)?;
        writeln!(&file, "{}", std::process::id())?;
        Ok(Self { file })
    }
}

impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Whether a lock attempt failed only because someone else holds the lock.
fn contended(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
        || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
}

/// What a look at a model's install lock shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LockProbe {
    Free,
    Held,
    /// The lock could not be checked, so whether an install runs is not known.
    Unknown,
}

pub(super) fn probe_install_lock(path: &Path) -> LockProbe {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return LockProbe::Free,
        Err(_) => return LockProbe::Unknown,
    };
    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = FileExt::unlock(&file);
            LockProbe::Free
        }
        Err(error) if contended(&error) => LockProbe::Held,
        Err(_) => LockProbe::Unknown,
    }
}

/// What an install carries over from the installed version until it replaces it, so that no
/// progress or failure write loses the version that is still current.
#[derive(Clone)]
struct Kept {
    installed_at: Value,
    installed_manifest_sha256: Value,
    /// The installed version's directory name, or null for the legacy layout.
    payload_dir: Value,
    last_update_error: Value,
    /// The installed version's directory.
    root: PathBuf,
}

impl Kept {
    /// What to keep from a state an install may start over. A state that cannot be read, or that
    /// names no valid directory, may be the only record of the current version, so an install
    /// refuses to replace it.
    fn read(model_root: &Path, state: &StateRead) -> Result<Self, VoiceError> {
        let (state, root) = match state {
            StateRead::Missing => (&Value::Null, model_root.to_path_buf()),
            StateRead::Present(state) => (
                state,
                version_root(model_root, state)
                    .map_err(|reason| state_unusable(model_root, &reason))?,
            ),
            StateRead::Unusable(reason) => return Err(state_unusable(model_root, reason)),
        };
        Ok(Self {
            installed_at: state["installed_at"].clone(),
            installed_manifest_sha256: first_non_blank(
                state,
                &["installed_manifest_sha256", "manifest_sha256"],
            )
            .map_or(Value::Null, Value::from),
            payload_dir: state["payload_dir"].clone(),
            last_update_error: state["last_update_error"].clone(),
            root,
        })
    }

    fn apply(&self, state: &mut Value) {
        state["installed_at"] = self.installed_at.clone();
        state["installed_manifest_sha256"] = self.installed_manifest_sha256.clone();
        state["payload_dir"] = self.payload_dir.clone();
        state["last_update_error"] = self.last_update_error.clone();
    }

    /// Whether the installed version can still serve: it was installed and its directory holds
    /// every required file.
    fn complete(&self, model: &Model) -> bool {
        self.installed_at
            .as_str()
            .is_some_and(|value| !value.is_empty())
            && files_complete(model, &self.root)
    }
}

/// One running install. Blocking work holds the job, and with it the install lock, until the
/// work ends.
struct Job {
    model: Model,
    kept: Kept,
    model_root: PathBuf,
    staging: PathBuf,
    /// The version directory this install creates.
    payload_dir: String,
    _lock: InstallLock,
}

/// Starts installing or updating a model and returns its status once the install has begun.
/// The install goes on in the background even if the caller stops waiting.
pub async fn begin_install(home: HomeLayout, model_id: String) -> Result<Value, VoiceError> {
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let job = Arc::new(start(&home, &model_id)?);
        let status = model_status(&home, &job.model);
        runtime.spawn(install(job));
        Ok(status)
    })
    .await
    .map_err(join_error)?
}

/// Takes the lock, clears what earlier installs left behind, and records that the model is
/// downloading, keeping the installed version's details.
fn start(home: &HomeLayout, model_id: &str) -> Result<Job, VoiceError> {
    let model = catalog(home)?.remove(model_id).ok_or_else(|| {
        VoiceError::new(
            "voice_model_not_found",
            format!("voice model not found: {model_id}"),
        )
    })?;
    let model_root = model_dir(home, &model.model_id);
    let lock = InstallLock::acquire(&install_lock_path(home, &model.model_id))?;
    let state = payload::read_state(&model_root);
    let kept = Kept::read(&model_root, &state)?;
    collect_garbage(&model_root, &model, &state);
    // Twelve hex digits keep model paths short for native readers on Windows, which may be
    // limited to 260 characters; a name already taken is skipped, not relied on to be rare.
    let id = claim_staging(&model_root, || {
        Uuid::new_v4().simple().to_string()[..12].to_owned()
    })?;
    let staging = model_root.join(format!("{STAGING_PREFIX}{id}"));
    if let Err(error) = write_state(&model_root, &progress_state(&model, &kept, 0, None)) {
        remove_logged(&staging);
        return Err(error);
    }
    Ok(Job {
        staging,
        payload_dir: format!("{PAYLOAD_PREFIX}{id}"),
        model,
        kept,
        model_root,
        _lock: lock,
    })
}

/// Creates this install's staging directory, so that failure cleanup removes only what the
/// install made, under an id whose version directory does not exist yet. Under the install lock
/// no one else creates either.
fn claim_staging(
    model_root: &Path,
    mut candidate: impl FnMut() -> String,
) -> Result<String, VoiceError> {
    fs::create_dir_all(model_root)?;
    for _ in 0..8 {
        let id = candidate();
        match fs::symlink_metadata(model_root.join(format!("{PAYLOAD_PREFIX}{id}"))) {
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match fs::create_dir(model_root.join(format!("{STAGING_PREFIX}{id}"))) {
            Ok(()) => return Ok(id),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(VoiceError::new(
        "voice_model_install_failed",
        "could not choose an unused directory for the new version",
    ))
}

async fn install(job: Arc<Job>) {
    let commit = match fetch(&job).await {
        // A commit that did not report back may have stopped anywhere, as a failed read-back.
        Ok(()) => blocking(&job, |job| Ok(commit(job, write_state_file)))
            .await
            .unwrap_or_else(Commit::Unknown),
        Err(error) => Commit::NotSwitched(error),
    };
    let finished = match commit {
        Commit::Switched => {
            blocking(&job, |job| {
                collect_garbage(
                    &job.model_root,
                    &job.model,
                    &payload::read_state(&job.model_root),
                );
                Ok(())
            })
            .await
        }
        Commit::NotSwitched(error) => blocking(&job, move |job| finish_failed(job, &error)).await,
        Commit::Unknown(error) => {
            tracing::warn!(
                model = %job.model.model_id,
                payload_dir = %job.payload_dir,
                code = error.code,
                error = %error.message,
                "could not confirm whether the voice model switched versions; keeping both"
            );
            Ok(())
        }
    };
    if let Err(error) = finished {
        tracing::warn!(
            model = %job.model.model_id,
            code = error.code,
            error = %error.message,
            "could not finish the voice model install"
        );
    }
}

/// Runs blocking work for an install on Tokio's blocking pool. The work holds the job, and with
/// it the install lock, until it ends, even when the install is cancelled meanwhile. It starts
/// at once, not when the result is first awaited.
fn blocking<T, F>(
    job: &Arc<Job>,
    work: F,
) -> impl Future<Output = Result<T, VoiceError>> + use<T, F>
where
    T: Send + 'static,
    F: FnOnce(&Job) -> Result<T, VoiceError> + Send + 'static,
{
    let job = Arc::clone(job);
    let task = tokio::task::spawn_blocking(move || work(&job));
    async move { task.await.map_err(join_error)? }
}

fn join_error(error: tokio::task::JoinError) -> VoiceError {
    VoiceError::new("voice_model_install_failed", error.to_string())
}

/// Downloads every artifact into staging, verifying each, and unpacks the archives there.
async fn fetch(job: &Arc<Job>) -> Result<(), VoiceError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(3600))
        .build()
        .map_err(|error| VoiceError::new("voice_model_download_failed", error.to_string()))?;
    let mut downloaded = 0_u64;
    let mut last_progress = Instant::now();
    for (index, artifact) in job.model.artifacts.iter().enumerate() {
        validate_relative(&artifact.path)?;
        let download_error = |error: reqwest::Error| {
            VoiceError::new("voice_model_download_failed", error.to_string())
                .detail("url", artifact.url.clone())
        };
        let response = client
            .get(&artifact.url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(download_error)?;
        let target = job.staging.join(&artifact.path);
        let part = target.with_extension(format!(
            "{}part",
            target.extension().and_then(|v| v.to_str()).unwrap_or("")
        ));
        let (sender, receiver) = mpsc::channel::<Chunk<Bytes>>(8);
        let writer = blocking(job, {
            let part = part.clone();
            move |_| write_artifact(&part, receiver)
        });
        let streamed: Result<(), VoiceError> = async {
            // Owned here, so the writer sees the channel close however this ends.
            let sender = sender;
            let mut response = response;
            let mut received = 0_u64;
            while let Some(chunk) = response.chunk().await.map_err(download_error)? {
                received += chunk.len() as u64;
                if sender.send(Chunk::Data(chunk)).await.is_err() {
                    // The writer stopped; its own result says why.
                    return Ok(());
                }
                if last_progress.elapsed() >= PROGRESS_INTERVAL {
                    let state =
                        progress_state(&job.model, &job.kept, downloaded + received, Some(index));
                    blocking(job, move |job| write_state(&job.model_root, &state)).await?;
                    last_progress = Instant::now();
                }
            }
            let _ = sender.send(Chunk::Finish).await;
            Ok(())
        }
        .await;
        let written = writer.await;
        streamed?;
        let written = written?;
        if !written.sha256.eq_ignore_ascii_case(&artifact.sha256) {
            return Err(VoiceError::new(
                "voice_model_hash_mismatch",
                format!("downloaded artifact hash mismatch: {}", artifact.path),
            )
            .detail("expected_sha256", artifact.sha256.clone())
            .detail("actual_sha256", written.sha256));
        }
        if artifact
            .size_bytes
            .is_some_and(|expected| expected != written.bytes)
        {
            return Err(VoiceError::new(
                "voice_model_size_mismatch",
                format!("downloaded artifact size mismatch: {}", artifact.path),
            ));
        }
        downloaded += written.bytes;
        let state = progress_state(&job.model, &job.kept, downloaded, Some(index));
        let archive = artifact.archive.clone();
        blocking(job, move |job| {
            write_state(&job.model_root, &state)?;
            fs::rename(&part, &target)?;
            extract_artifact(&target, &archive, &job.staging, &job.model.required_files)
        })
        .await?;
        last_progress = Instant::now();
    }
    Ok(())
}

/// A downloaded piece of a file, or the end of the file.
enum Chunk<B> {
    Data(B),
    Finish,
}

struct Written {
    sha256: String,
    bytes: u64,
}

/// Writes one download to `part`, hashing it on the way. The file is synced and reported only
/// after `Finish`; a channel that closes before it means the download stopped, and the part is
/// removed.
fn write_artifact<B: AsRef<[u8]>>(
    part: &Path,
    mut chunks: mpsc::Receiver<Chunk<B>>,
) -> Result<Written, VoiceError> {
    let written = write_chunks(part, &mut chunks);
    if written.is_err() {
        let _ = fs::remove_file(part);
    }
    written
}

fn write_chunks<B: AsRef<[u8]>>(
    part: &Path,
    chunks: &mut mpsc::Receiver<Chunk<B>>,
) -> Result<Written, VoiceError> {
    if let Some(parent) = part.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = BufWriter::new(fs::File::create(part)?);
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    while let Some(chunk) = chunks.blocking_recv() {
        match chunk {
            Chunk::Data(data) => {
                let data = data.as_ref();
                hasher.update(data);
                file.write_all(data)?;
                bytes += data.len() as u64;
            }
            Chunk::Finish => {
                file.into_inner()
                    .map_err(io::IntoInnerError::into_error)?
                    .sync_all()?;
                return Ok(Written {
                    sha256: format!("{:x}", hasher.finalize()),
                    bytes,
                });
            }
        }
    }
    Err(VoiceError::new(
        "voice_model_download_failed",
        "the download stopped before the file was complete",
    ))
}

/// Unpacks a downloaded archive into staging, then removes the archive unless the model requires
/// that file. A plain file stays where it was downloaded.
fn extract_artifact(
    path: &Path,
    kind: &str,
    output: &Path,
    required_files: &[String],
) -> Result<(), VoiceError> {
    match kind {
        "" => return Ok(()),
        "tar.bz2" | "tbz2" => unpack_tar(BzDecoder::new(fs::File::open(path)?), output)?,
        "tar.gz" | "tgz" => unpack_tar(GzDecoder::new(fs::File::open(path)?), output)?,
        "tar" => unpack_tar(fs::File::open(path)?, output)?,
        other => {
            return Err(VoiceError::new(
                "voice_model_archive_invalid",
                format!("unsupported voice model archive: {other}"),
            ));
        }
    }
    if !names_file(output, required_files, path) {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Whether any of `relative` under `root` names the existing file at `path`. The filesystem
/// decides which spellings name one file, such as `a//b` and `a/./b`, or letter case and trailing
/// dots on Windows. A spelling it cannot resolve for any reason but absence counts as a match.
fn names_file(root: &Path, relative: &[String], path: &Path) -> bool {
    let Ok(file) = fs::canonicalize(path) else {
        return true;
    };
    relative
        .iter()
        .any(|item| match fs::canonicalize(root.join(item)) {
            Ok(resolved) => resolved == file,
            Err(error) => error.kind() != io::ErrorKind::NotFound,
        })
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

fn progress_state(model: &Model, kept: &Kept, downloaded: u64, artifact: Option<usize>) -> Value {
    let total = total_size(model);
    let mut state = json!({
        "model_id":model.model_id,"status":"downloading","updated_at":utc_now(),
        "downloaded_bytes":downloaded,"total_size_bytes":total,
        "progress_percent":if total>0{(downloaded as f64/total as f64*100.0).min(99.9)}else{0.0},
        "artifact_count":model.artifacts.len(),"error":{}
    });
    if let Some(index) = artifact {
        state["current_artifact_path"] = json!(model.artifacts[index].path);
        state["artifact_index"] = json!(index + 1);
    }
    kept.apply(&mut state);
    state
}

enum Commit {
    /// The state names the new version.
    Switched,
    /// The state still names the installed version.
    NotSwitched(VoiceError),
    /// A write failed and the state could not be read back, so which version it names is not
    /// known.
    Unknown(VoiceError),
}

/// Makes the unpacked files the current version: checks and syncs them, renames staging to the
/// new version directory, then writes the state that names it. That write is the switch.
fn commit(job: &Job, write: impl FnOnce(&Path, &Value) -> io::Result<()>) -> Commit {
    let ready = match prepare(job) {
        Ok(ready) => ready,
        Err(error) => return Commit::NotSwitched(error),
    };
    let Err(error) = write(&job.model_root.join(INSTALL_STATE), &ready) else {
        return Commit::Switched;
    };
    // A write can fail after its rename, as when syncing the directory fails. Only a state that
    // still names the kept version shows the switch did not happen; any other is no evidence.
    match payload::read_state(&job.model_root) {
        StateRead::Present(state) if state["payload_dir"] == job.payload_dir.as_str() => {
            Commit::Switched
        }
        StateRead::Present(state) if state["payload_dir"] == job.kept.payload_dir => {
            Commit::NotSwitched(error.into())
        }
        StateRead::Present(_) | StateRead::Missing | StateRead::Unusable(_) => {
            Commit::Unknown(error.into())
        }
    }
}

fn prepare(job: &Job) -> Result<Value, VoiceError> {
    for path in &job.model.required_files {
        validate_relative(path)?;
        if !job.staging.join(path).is_file() {
            return Err(VoiceError::new(
                "voice_model_required_file_missing",
                format!("model archive is missing required file: {path}"),
            ));
        }
    }
    let manifest_sha = model_manifest_sha(&job.model)?;
    sync_tree(&job.staging)?;
    fs::rename(&job.staging, job.model_root.join(&job.payload_dir))?;
    let total = total_size(&job.model);
    let now = utc_now();
    Ok(json!({
        "model_id":job.model.model_id,"status":"ready","payload_dir":job.payload_dir,
        "installed_at":now,"updated_at":now,
        "manifest_sha256":manifest_sha,"installed_manifest_sha256":manifest_sha,
        "downloaded_bytes":total,"total_size_bytes":total,"progress_percent":100.0,"error":{}
    }))
}

/// Flushes unpacked files to disk before they become current. Unix also syncs the directories;
/// Windows cannot open a directory to sync it, and syncs files only where it can open them for
/// writing.
fn sync_tree(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            sync_tree(&entry.path())?;
        } else if kind.is_file() {
            sync_file(&entry.path())?;
        }
    }
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn sync_file(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_file(path: &Path) -> io::Result<()> {
    if let Ok(file) = OpenOptions::new().write(true).open(path) {
        let _ = file.sync_all();
    }
    Ok(())
}

/// Ends an install that did not switch: removes its staging and leaves the installed version
/// current, reported ready if it is complete, with this install's error kept for the user. A
/// version directory it renamed but did not switch to is left to the next cleanup, which removes
/// it once the current version is confirmed; this path never deletes a version directory.
fn finish_failed(job: &Job, error: &VoiceError) -> Result<(), VoiceError> {
    remove_logged(&job.staging);
    let previous_complete = job.kept.complete(&job.model);
    let error = json!({"code":error.code,"message":error.message,"details":error.details});
    let mut state = json!({
        "model_id":job.model.model_id,
        "status":if previous_complete {"ready"} else {"failed"},
        "updated_at":utc_now(),
        "error":if previous_complete {json!({})} else {error.clone()},
    });
    job.kept.apply(&mut state);
    state["last_update_error"] = error;
    write_state(&job.model_root, &state)
}

fn write_state(model_root: &Path, state: &Value) -> Result<(), VoiceError> {
    Ok(write_state_file(&model_root.join(INSTALL_STATE), state)?)
}

fn write_state_file(path: &Path, state: &Value) -> io::Result<()> {
    cccc_core::fs::write_json(path, state)
}

/// Removes what no install or reader needs. Leftover staging always goes. When the state names
/// a complete current version, so does every other version no reader has pinned, including the
/// legacy layout's files and whatever old installs left. When the current version cannot be
/// confirmed, everything but staging stays, as any of it may be the only good copy.
fn collect_garbage(model_root: &Path, model: &Model, state: &StateRead) {
    let entries = match fs::read_dir(model_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            tracing::warn!(path = %model_root.display(), %error, "could not list a voice model directory");
            return;
        }
    };
    let mut versions = Vec::new();
    let mut legacy = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(STAGING_PREFIX) {
            remove_logged(&path);
        } else if name.starts_with(PAYLOAD_PREFIX) {
            versions.push(path);
        } else if name != INSTALL_STATE && name != INSTALL_LOCK {
            legacy.push(path);
        }
    }
    let current = match trusted_current(model_root, model, state) {
        Ok(current) => current,
        Err(reason) => {
            if !versions.is_empty() {
                tracing::info!(
                    model = %model.model_id,
                    %reason,
                    "keeping voice model versions until the current one is confirmed"
                );
            }
            return;
        }
    };
    // The legacy layout's files form one version, pinned by the model directory itself.
    let mut unused: Vec<PathBuf> = versions
        .into_iter()
        .filter(|path| *path != current)
        .collect();
    if !legacy.is_empty() {
        unused.push(model_root.to_path_buf());
    }
    for root in unpinned(unused) {
        if root == model_root {
            legacy.iter().for_each(|path| remove_logged(path));
        } else {
            remove_logged(&root);
        }
    }
}

/// The current version's directory, when the state names one that exists and holds every
/// required file.
fn trusted_current(model_root: &Path, model: &Model, state: &StateRead) -> Result<PathBuf, String> {
    let state = match state {
        StateRead::Present(state) => state,
        StateRead::Missing => return Err("the install state is missing".into()),
        StateRead::Unusable(reason) => return Err(reason.clone()),
    };
    if state.get("payload_dir").is_none_or(Value::is_null) {
        return Err("the install state uses the legacy layout".into());
    }
    if model.required_files.is_empty() {
        return Err("the model lists no files that would confirm a version".into());
    }
    let root = version_root(model_root, state)?;
    if !root.is_dir() || !files_complete(model, &root) {
        return Err(format!("{} is missing or incomplete", root.display()));
    }
    Ok(root)
}

fn remove_logged(path: &Path) {
    let removed = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    };
    if let Err(error) = removed {
        tracing::warn!(
            path = %path.display(),
            %error,
            "could not remove an unused voice model file; the next cleanup retries"
        );
    }
}

/// Clears what interrupted installs and replaced versions left behind, off the async workers.
/// A model whose install lock is taken is left to that install.
pub fn spawn_startup_cleanup(home: HomeLayout) {
    tokio::task::spawn_blocking(move || collect_all_garbage(&home));
}

fn collect_all_garbage(home: &HomeLayout) {
    let models = match catalog(home) {
        Ok(models) => models,
        Err(error) => {
            tracing::warn!(code = error.code, error = %error.message, "skipped voice model cleanup");
            return;
        }
    };
    for model in models.values() {
        let model_root = model_dir(home, &model.model_id);
        if !model_root.is_dir() {
            continue;
        }
        match InstallLock::acquire(&install_lock_path(home, &model.model_id)) {
            Ok(_lock) => collect_garbage(&model_root, model, &payload::read_state(&model_root)),
            Err(error) if error.code == "voice_model_install_busy" => {}
            Err(error) => {
                tracing::warn!(
                    model = %model.model_id,
                    code = error.code,
                    error = %error.message,
                    "skipped voice model cleanup"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
