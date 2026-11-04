//! Installing a voice model. An install downloads each file into a resume directory, so an
//! interrupted download continues where it stopped, verifies it, and unpacks it into a staging
//! directory. It renames staging to a new version directory, and then switches the install
//! state to that version in one atomic write. A failure at any point leaves the installed version
//! current. Blocking work runs on Tokio's blocking pool, and every piece of it holds the model's
//! install lock until it ends, even when the install that started it is cancelled.

use super::payload::{self, PAYLOAD_PREFIX, StateRead, unpinned, version_root};
use super::{
    Artifact, INSTALL_LOCK, INSTALL_STATE, Model, VoiceError, catalog, files_complete,
    first_non_blank, install_lock_path, model_dir, model_manifest_sha, model_status,
    state_unusable, total_size, validate_relative, validate_relative_path,
};
use axum::body::Bytes;
use bzip2::read::BzDecoder;
use cccc_contracts::utc_now;
use cccc_core::HomeLayout;
use flate2::read::GzDecoder;
use fs2::FileExt;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, HeaderMap, RANGE};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tar::Archive;
use tokio::sync::mpsc;
use uuid::Uuid;

const STAGING_PREFIX: &str = ".install-staging-";
/// The least time between two progress writes while a file downloads.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);
/// A download fails once no data has arrived for this long. One that keeps receiving may take
/// as long as it needs: a model package can exceed 1 GB on a slow connection.
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Partly downloaded files, kept between attempts and between installs, so a dropped
/// connection does not start a large model over. Each part is named by the SHA-256 the catalog
/// expects, so it is only ever continued toward that content; the final hash check decides.
const RESUME_DIR: &str = ".download-resume";
/// A part that no install has continued for this long is dropped.
const RESUME_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// A download gives up after this many requests in a row that receive nothing...
const IDLE_ATTEMPTS: u32 = 3;
/// ...or after this many failed requests in all, such as dropped connections.
const MAX_FAILURES: u32 = 20;
/// The pause before asking again grows by this much with each request that brought nothing.
const RETRY_PAUSE: Duration = if cfg!(test) {
    Duration::from_millis(10)
} else {
    Duration::from_millis(500)
};

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

/// Builds the download client. It limits the wait for each read, not the whole download.
fn download_client(stall_timeout: Duration) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(stall_timeout)
        .build()
}

/// Downloads every artifact, verifying each, and unpacks the archives into staging. A file
/// downloads into the model's resume directory first, so what arrived before a dropped
/// connection, a failed install or a stopped one is continued instead of fetched again.
async fn fetch(job: &Arc<Job>) -> Result<(), VoiceError> {
    let client = download_client(DOWNLOAD_STALL_TIMEOUT)
        .map_err(|error| VoiceError::new("voice_model_download_failed", error.to_string()))?;
    let mut downloaded = 0_u64;
    let mut last_progress = Instant::now();
    for (index, artifact) in job.model.artifacts.iter().enumerate() {
        validate_relative(&artifact.path)?;
        let part = resume_part(&job.model_root, artifact);
        let from = {
            let (part, size) = (part.clone(), artifact.size_bytes);
            blocking(job, move |_| kept_part(&part, size)).await?
        };
        let offset = from.bytes;
        let (sender, receiver) = mpsc::channel::<Chunk<Bytes>>(8);
        let writer = blocking(job, {
            let part = part.clone();
            move |_| write_artifact(&part, from, receiver)
        });
        let streamed = download(job, &client, artifact, sender, offset, |received| {
            (last_progress.elapsed() >= PROGRESS_INTERVAL).then(|| {
                last_progress = Instant::now();
                progress_state(&job.model, &job.kept, downloaded + received, Some(index))
            })
        })
        .await;
        let written = writer.await;
        streamed?;
        let written = written?;
        if let Err(error) = verify(artifact, &written) {
            // A part that does not add up is not continued again.
            blocking(job, move |_| {
                remove_logged(&part);
                Ok(())
            })
            .await?;
            return Err(error);
        }
        downloaded += written.bytes;
        let state = progress_state(&job.model, &job.kept, downloaded, Some(index));
        let target = job.staging.join(&artifact.path);
        let archive = artifact.archive.clone();
        blocking(job, move |job| {
            write_state(&job.model_root, &state)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&part, &target)?;
            extract_artifact(&target, &archive, &job.staging, &job.model.required_files)
        })
        .await?;
        last_progress = Instant::now();
    }
    Ok(())
}

/// Where an artifact downloads before it moves into staging. The part is named by the hash the
/// catalog expects, which the catalog checks is 64 hex digits, so a part is only ever continued
/// toward that content.
fn resume_part(model_root: &Path, artifact: &Artifact) -> PathBuf {
    model_root.join(RESUME_DIR).join(part_name(artifact))
}

fn part_name(artifact: &Artifact) -> String {
    format!("{}.part", artifact.sha256.to_ascii_lowercase())
}

/// Checks a downloaded file against the catalog.
fn verify(artifact: &Artifact, written: &Written) -> Result<(), VoiceError> {
    if !written.sha256.eq_ignore_ascii_case(&artifact.sha256) {
        return Err(VoiceError::new(
            "voice_model_hash_mismatch",
            format!("downloaded artifact hash mismatch: {}", artifact.path),
        )
        .detail("expected_sha256", artifact.sha256.clone())
        .detail("actual_sha256", written.sha256.clone()));
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
    Ok(())
}

/// Streams one artifact to its writer from `offset`, the bytes the writer already holds. After a
/// connection that drops or stalls, or a partial response that covers only part of the rest, it
/// asks again from where it got to. A partial response that brought data is not a failure. The
/// download gives up after `IDLE_ATTEMPTS` requests in a row that bring nothing, or after
/// `MAX_FAILURES` failed requests.
async fn download(
    job: &Arc<Job>,
    client: &reqwest::Client,
    artifact: &Artifact,
    sender: mpsc::Sender<Chunk<Bytes>>,
    mut offset: u64,
    mut progress: impl FnMut(u64) -> Option<Value> + Send,
) -> Result<(), VoiceError> {
    let (mut failures, mut idle) = (0, 0);
    loop {
        let before = offset;
        let error =
            match stream_once(job, client, artifact, &sender, &mut offset, &mut progress).await {
                Ok(Streamed::Complete) => {
                    let _ = sender.send(Chunk::Finish).await;
                    return Ok(());
                }
                // The writer stopped; its own result says why.
                Ok(Streamed::WriterStopped) => return Ok(()),
                Ok(Streamed::Partial) if offset > before => {
                    idle = 0;
                    continue;
                }
                Ok(Streamed::Partial) => VoiceError::new(
                    "voice_model_download_failed",
                    "the server kept ending the download before the end of the file",
                )
                .detail("url", artifact.url.clone()),
                Err(Attempt::Fatal(error)) => return Err(error),
                Err(Attempt::Retry(error)) => error,
            };
        failures += 1;
        idle = if offset > before { 0 } else { idle + 1 };
        if idle >= IDLE_ATTEMPTS || failures >= MAX_FAILURES {
            return Err(error);
        }
        // Straight back after a request that brought data; a growing pause after one that
        // brought none.
        tokio::time::sleep(RETRY_PAUSE * idle).await;
    }
}

/// How one request for an artifact ended.
enum Streamed {
    /// Everything the server had arrived; the writer's hash check decides whether it is right.
    Complete,
    /// The response ended cleanly before the end of the file, as a partial response may.
    Partial,
    /// The writer stopped, and its own result says why.
    WriterStopped,
}

/// Why a request for an artifact failed: worth asking again, or not.
enum Attempt {
    Retry(VoiceError),
    Fatal(VoiceError),
}

/// Asks for an artifact from `offset` and streams what arrives to the writer, moving `offset`
/// on as it goes. Only a response that continues exactly at `offset` is added to what the
/// writer holds, and anything else starts the file over. A file that still changed between two
/// requests, at the same size, fails the final hash check rather than being installed.
async fn stream_once(
    job: &Arc<Job>,
    client: &reqwest::Client,
    artifact: &Artifact,
    sender: &mpsc::Sender<Chunk<Bytes>>,
    offset: &mut u64,
    progress: &mut (impl FnMut(u64) -> Option<Value> + Send),
) -> Result<Streamed, Attempt> {
    if *offset > 0 && artifact.size_bytes == Some(*offset) {
        // An earlier attempt kept the whole file.
        return Ok(Streamed::Complete);
    }
    let mut request = client.get(&artifact.url);
    if *offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = request.send().await.map_err(|error| {
        // A bad address or a redirect loop does not get better by asking again.
        if error.is_builder() || error.is_redirect() {
            Attempt::Fatal(download_error(error, artifact))
        } else {
            Attempt::Retry(download_error(error, artifact))
        }
    })?;
    let status = response.status();
    // The size of the whole file, as the response states it.
    let stated = match status {
        StatusCode::PARTIAL_CONTENT => match content_range(response.headers()) {
            Some(range) if range.first == *offset => range.size,
            _ => return start_over(sender, offset, artifact).await,
        },
        StatusCode::RANGE_NOT_SATISFIABLE => return start_over(sender, offset, artifact).await,
        status if status.is_success() => response.content_length(),
        status => {
            let error = match response.error_for_status_ref() {
                Err(error) => download_error(error, artifact),
                Ok(_) => VoiceError::new(
                    "voice_model_download_failed",
                    format!("unexpected response: {status}"),
                )
                .detail("url", artifact.url.clone()),
            };
            let busy = matches!(
                status,
                StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS
            );
            return Err(if busy || status.is_server_error() {
                Attempt::Retry(error)
            } else {
                Attempt::Fatal(error)
            });
        }
    };
    // Another file than the catalog lists: say so before writing anything, rather than after
    // the whole download as a failed check.
    if let Some((expected, stated)) = artifact
        .size_bytes
        .zip(stated)
        .filter(|(expected, stated)| expected != stated)
    {
        return Err(Attempt::Fatal(
            VoiceError::new(
                "voice_model_download_failed",
                format!("the server's file is {stated} bytes, but the catalog expects {expected}"),
            )
            .detail("url", artifact.url.clone()),
        ));
    }
    if status != StatusCode::PARTIAL_CONTENT && *offset > 0 {
        // The whole file from its start.
        if sender.send(Chunk::Restart).await.is_err() {
            return Ok(Streamed::WriterStopped);
        }
        *offset = 0;
    }
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| Attempt::Retry(download_error(error, artifact)))?
    {
        *offset += chunk.len() as u64;
        if sender.send(Chunk::Data(chunk)).await.is_err() {
            return Ok(Streamed::WriterStopped);
        }
        if let Some(state) = progress(*offset) {
            blocking(job, move |job| write_state(&job.model_root, &state))
                .await
                .map_err(Attempt::Fatal)?;
        }
    }
    // A partial response may cover only part of the rest. Without a known size, a clean end
    // counts as the end of the file, and the hash check decides.
    let size = artifact.size_bytes.or(stated);
    Ok(if size.is_some_and(|size| *offset < size) {
        Streamed::Partial
    } else {
        Streamed::Complete
    })
}

/// Drops what the writer holds after a response that does not continue it, so the next request
/// asks for the whole file.
async fn start_over(
    sender: &mpsc::Sender<Chunk<Bytes>>,
    offset: &mut u64,
    artifact: &Artifact,
) -> Result<Streamed, Attempt> {
    if sender.send(Chunk::Restart).await.is_err() {
        return Ok(Streamed::WriterStopped);
    }
    *offset = 0;
    Err(Attempt::Retry(
        VoiceError::new(
            "voice_model_download_failed",
            "the server could not continue the download where it stopped",
        )
        .detail("url", artifact.url.clone()),
    ))
}

/// What a partial response holds, from `Content-Range: bytes <first>-<last>/<size>`.
struct ContentRange {
    first: u64,
    /// The size of the whole file; `*` when the server does not know it.
    size: Option<u64>,
}

fn content_range(headers: &HeaderMap) -> Option<ContentRange> {
    let value = headers.get(CONTENT_RANGE)?.to_str().ok()?;
    let (range, size) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = range.split_once('-')?;
    let (first, last): (u64, u64) = (first.trim().parse().ok()?, last.trim().parse().ok()?);
    let size = match size.trim() {
        "*" => None,
        size => Some(size.parse::<u64>().ok()?),
    };
    // A range that runs backwards or past the stated size is nothing to build on.
    (first <= last && size.is_none_or(|size| last < size)).then_some(ContentRange { first, size })
}

fn download_error(error: reqwest::Error, artifact: &Artifact) -> VoiceError {
    let message = if error.is_timeout() && !error.is_connect() {
        format!(
            "the download received no data for {} seconds; check the connection and try again",
            DOWNLOAD_STALL_TIMEOUT.as_secs()
        )
    } else {
        error.to_string()
    };
    VoiceError::new("voice_model_download_failed", message).detail("url", artifact.url.clone())
}

/// A downloaded piece of a file; the instruction to drop what was written and start the file
/// over; or the end of the file.
enum Chunk<B> {
    Data(B),
    Restart,
    Finish,
}

/// Where a download continues: the bytes earlier attempts kept, and their hash so far.
#[derive(Default)]
struct Resume {
    bytes: u64,
    hasher: Sha256,
}

/// Reads what earlier attempts kept of a download. A part longer than the file it should
/// become cannot be its start, and is removed.
fn kept_part(part: &Path, size: Option<u64>) -> Result<Resume, VoiceError> {
    let file = match fs::File::open(part) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Resume::default()),
        Err(error) => return Err(error.into()),
    };
    let len = file.metadata()?.len();
    if size.is_some_and(|size| len > size) {
        drop(file);
        fs::remove_file(part)?;
        return Ok(Resume::default());
    }
    let mut hasher = Sha256::new();
    let bytes = io::copy(&mut file.take(len), &mut hasher)?;
    Ok(Resume { bytes, hasher })
}

struct Written {
    sha256: String,
    bytes: u64,
}

/// Writes one download to `part` from where `from` left it, hashing on the way. The file is
/// synced and reported only after `Finish`. A channel that closes before it means the download
/// stopped: what arrived stays for the next attempt, unless nothing did. A part that could not
/// be written is removed.
fn write_artifact<B: AsRef<[u8]>>(
    part: &Path,
    from: Resume,
    mut chunks: mpsc::Receiver<Chunk<B>>,
) -> Result<Written, VoiceError> {
    match write_chunks(part, from, &mut chunks) {
        Ok(Some(written)) => Ok(written),
        Ok(None) => Err(VoiceError::new(
            "voice_model_download_failed",
            "the download stopped before the file was complete",
        )),
        Err(error) => {
            let _ = fs::remove_file(part);
            Err(error)
        }
    }
}

fn write_chunks<B: AsRef<[u8]>>(
    part: &Path,
    from: Resume,
    chunks: &mut mpsc::Receiver<Chunk<B>>,
) -> Result<Option<Written>, VoiceError> {
    if let Some(parent) = part.parent() {
        fs::create_dir_all(parent)?;
    }
    let Resume {
        mut bytes,
        mut hasher,
    } = from;
    let mut file = if bytes == 0 {
        fs::File::create(part)?
    } else {
        let mut file = OpenOptions::new().write(true).open(part)?;
        file.set_len(bytes)?;
        file.seek(SeekFrom::Start(bytes))?;
        file
    };
    while let Some(chunk) = chunks.blocking_recv() {
        match chunk {
            Chunk::Data(data) => {
                let data = data.as_ref();
                hasher.update(data);
                file.write_all(data)?;
                bytes += data.len() as u64;
            }
            Chunk::Restart => {
                file.set_len(0)?;
                file.seek(SeekFrom::Start(0))?;
                (bytes, hasher) = (0, Sha256::new());
            }
            Chunk::Finish => {
                file.sync_all()?;
                return Ok(Some(Written {
                    sha256: format!("{:x}", hasher.finalize()),
                    bytes,
                }));
            }
        }
    }
    drop(file);
    if bytes == 0 {
        let _ = fs::remove_file(part);
    }
    Ok(None)
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
    // What the download kept stays for the next install; an empty directory does not.
    let _ = fs::remove_dir(job.model_root.join(RESUME_DIR));
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

/// Removes what no install or reader needs. Leftover staging always goes, and so do kept
/// downloads no install will continue (see `prune_resume`). When the state names a complete
/// current version, so does every other version no reader has pinned, including the legacy
/// layout's files and whatever old installs left. When the current version cannot be
/// confirmed, every version stays, as any of them may be the only good copy.
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
        } else if name == RESUME_DIR {
            prune_resume(&path, model);
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

/// Drops kept downloads that no install will continue: parts the catalog no longer names, and
/// parts nothing has added to for `RESUME_MAX_AGE`. The directory goes once it is empty.
fn prune_resume(dir: &Path, model: &Model) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let named = model
                .artifacts
                .iter()
                .any(|artifact| name.to_str() == Some(part_name(artifact).as_str()));
            let old = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > RESUME_MAX_AGE);
            if !named || old {
                remove_logged(&entry.path());
            }
        }
    }
    // Removes only an empty directory.
    let _ = fs::remove_dir(dir);
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
