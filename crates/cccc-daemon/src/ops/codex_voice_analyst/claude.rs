use super::{AnalystEvent, MANAGED_AGENT_DISCONNECTED_METHOD, SessionPurpose};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

mod command;
mod control;
mod transcript;
mod transcript_ack;
mod transcript_buffer;
mod transcript_path;

pub(super) use command::prepare;

pub(super) fn remove_actor_settings(
    home: &cccc_core::HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> io::Result<()> {
    command::remove_settings_owner(home, &format!("actor:{group_id}:{actor_id}"))
}

const EVENT_CAPACITY: usize = 2048;
const COMMAND_CAPACITY: usize = 16;
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
const JOB_READY_TIMEOUT: Duration = Duration::from_secs(10);
const TRANSCRIPT_READY_TIMEOUT: Duration = Duration::from_secs(10);
const PROMPT_ACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);
const PROMPT_SETTLED_CORRELATION_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const LIVENESS_FAILURE_TIMEOUT: Duration = Duration::from_secs(10);
const PARTIAL_TAIL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_LAUNCH_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_STATE_BYTES: u64 = 512 * 1024;
const MAX_TRANSCRIPT_READ_BYTES: u64 = 256 * 1024;
const MAX_TRANSCRIPT_LINE_BYTES: usize = 2 * 1024 * 1024;

pub(super) struct LaunchedClaude {
    pub(super) protocol: ClaudeClient,
    pub(super) session_id: String,
    pub(super) resumed: bool,
    pub(super) tui_command: Vec<String>,
    pub(super) environment: BTreeMap<String, String>,
    pub(super) cleanup_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct Job {
    short: String,
    session_id: String,
    cwd: PathBuf,
    cli_version: command::Version,
}

pub(super) async fn launch(
    prepared: command::PreparedClaude,
    cwd: &Path,
    generation: &str,
    purpose: SessionPurpose,
    requested_session_id: Option<&str>,
) -> io::Result<LaunchedClaude> {
    launch_inner(prepared, cwd, generation, purpose, requested_session_id).await
}

async fn launch_inner(
    prepared: command::PreparedClaude,
    cwd: &Path,
    generation: &str,
    purpose: SessionPurpose,
    requested_session_id: Option<&str>,
) -> io::Result<LaunchedClaude> {
    let expected_version =
        command::require_supported_version(&prepared.executable, cwd, &prepared.launch_environment)
            .await?;
    // Inspect durable job metadata BEFORE respawn can replace it. A receipt
    // alone does not imply that this session ever received its first prompt.
    let skip_existing_transcript = match requested_session_id {
        Some(id) => !known_empty_session(&prepared.config_dir, id)?,
        None => false,
    };

    if let Some(session_id) = requested_session_id {
        validate_session_id(session_id)?;
        if let Some((endpoint, job)) = find_live_job(&prepared.config_dir, session_id).await? {
            validate_worker_version(&job, expected_version)?;
            validate_job_state(&prepared.config_dir, &job, cwd, true)?;
            return connect_launched(
                prepared,
                endpoint,
                job,
                expected_version,
                generation,
                true,
                skip_existing_transcript,
                false,
            )
            .await;
        }
    }

    let name = format!(
        "cccc-{}-{}",
        match purpose {
            SessionPurpose::VoiceAnalyst => "voice-analyst",
            SessionPurpose::Actor => "actor",
        },
        &generation[..generation.len().min(8)]
    );
    let mut arguments = Vec::new();
    if let Some(session_id) = requested_session_id {
        // Agent View resumes the exact durable session only when no launch
        // configuration is repeated. Any additional flag intentionally forks.
        arguments.extend(["--bg".into(), "--resume".into(), session_id.into()]);
    } else {
        arguments.extend(prepared.arguments.iter().cloned());
        arguments.extend(["--name".into(), name, "--bg".into()]);
    }
    let mut command = command::process_command(
        &prepared.executable,
        &arguments,
        &prepared.launch_environment,
    )?;
    command
        .current_dir(cwd)
        .env_clear()
        .envs(&prepared.launch_environment)
        .kill_on_drop(true);
    let output = tokio::time::timeout(LAUNCH_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "Claude Agent View launch timed out",
            )
        })??;
    let output_bytes = output.stdout.len().saturating_add(output.stderr.len());
    if output_bytes > MAX_LAUNCH_OUTPUT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude Agent View launch output exceeded its limit",
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Claude Agent View launch failed: {}",
            nonempty_detail(&stderr, &stdout)
        )));
    }
    let short = parse_short_id(&stdout)
        .or_else(|| parse_short_id(&stderr))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude Agent View launch did not identify its managed session",
            )
        })?;
    let (endpoint, job) = match wait_for_job(&prepared.config_dir, &short).await {
        Ok(value) => value,
        Err(error) => {
            let rollback = rollback_started_job(&prepared.config_dir, &short).await;
            return Err(with_optional_cleanup_error(error, rollback.err()));
        }
    };
    if let Err(error) = validate_worker_version(&job, expected_version) {
        let rollback = kill_and_confirm(&endpoint, &job.short).await;
        return Err(with_optional_cleanup_error(error, rollback.err()));
    }
    if let Some(expected) = requested_session_id
        && job.session_id != expected
    {
        let error = io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude Agent View copied a requested session instead of resuming it exactly",
        );
        let rollback = kill_and_confirm(&endpoint, &job.short).await;
        return Err(with_optional_cleanup_error(error, rollback.err()));
    }
    if let Err(error) = validate_job_state(&prepared.config_dir, &job, cwd, true) {
        let rollback = kill_and_confirm(&endpoint, &job.short).await;
        return Err(with_optional_cleanup_error(error, rollback.err()));
    }
    let resumed = requested_session_id.is_some();
    connect_launched(
        prepared,
        endpoint,
        job,
        expected_version,
        generation,
        resumed,
        skip_existing_transcript,
        true,
    )
    .await
}

fn known_empty_session(config_dir: &Path, session_id: &str) -> io::Result<bool> {
    let jobs = match std::fs::read_dir(config_dir.join("jobs")) {
        Ok(jobs) => jobs,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut found = false;
    for entry in jobs {
        let entry = entry?;
        if !valid_short_id(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let path = entry.path().join("state.json");
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Ok(false),
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_STATE_BYTES
        {
            return Ok(false);
        }
        let Ok(state) = serde_json::from_slice::<Value>(&std::fs::read(path)?) else {
            return Ok(false);
        };
        if state["sessionId"].as_str() != Some(session_id) {
            continue;
        }
        found = true;
        // Positive empty-job evidence, not just a missing transcript. Any
        // prompt, output, consumed bytes or published path requires history.
        // Agent View can initialize its output-token counter to zero; zero
        // usage is not evidence of a conversation. Retain nonzero/unknown usage.
        if state["intent"].as_str() != Some("")
            || state["linkScanOffset"].as_u64() != Some(0)
            || !state["linkScanPath"].is_null()
            || state.get("output") != Some(&Value::Null)
            || !(state["tokens"].is_null() || state["tokens"].as_f64() == Some(0.0))
        {
            return Ok(false);
        }
    }
    if !found {
        return Ok(false);
    }
    // The provider can write input before Agent View publishes linkScanPath
    // (notably while an API call retries). Such a session already has history.
    match std::fs::read_dir(config_dir.join("projects")) {
        Ok(projects) => {
            for project in projects {
                let project = project?;
                if !project.file_type()?.is_dir() {
                    continue;
                }
                match std::fs::symlink_metadata(project.path().join(format!("{session_id}.jsonl")))
                {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    _ => return Ok(false),
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Ok(false),
    }
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
async fn connect_launched(
    prepared: command::PreparedClaude,
    endpoint: control::Endpoint,
    job: Job,
    expected_version: command::Version,
    generation: &str,
    resumed: bool,
    skip_existing_transcript: bool,
    rollback_on_failure: bool,
) -> io::Result<LaunchedClaude> {
    let state_path = prepared
        .config_dir
        .join("jobs")
        .join(&job.short)
        .join("state.json");
    let protocol = match ClaudeClient::new(
        endpoint,
        job.short.clone(),
        job.session_id.clone(),
        expected_version,
        state_path,
        prepared.config_dir.clone(),
        generation.to_owned(),
        skip_existing_transcript,
    )
    .await
    {
        Ok(protocol) => protocol,
        Err(error) if rollback_on_failure => {
            let endpoint = control::Endpoint::resolve(&prepared.config_dir);
            let rollback = match endpoint {
                Ok(endpoint) => kill_and_confirm(&endpoint, &job.short).await,
                Err(error) => Err(error),
            };
            return Err(with_optional_cleanup_error(error, rollback.err()));
        }
        Err(error) => return Err(error),
    };
    Ok(LaunchedClaude {
        protocol,
        session_id: job.session_id,
        resumed,
        tui_command: vec![prepared.executable, "attach".into(), job.short],
        environment: prepared.launch_environment,
        // Agent View persists the original --settings path in its respawn
        // metadata. The owner-scoped file must therefore outlive individual
        // processes so a stopped Actor or Analyst can resume the same session.
        cleanup_paths: Vec::new(),
    })
}

async fn find_live_job(
    config_dir: &Path,
    session_id: &str,
) -> io::Result<Option<(control::Endpoint, Job)>> {
    let endpoint = match control::Endpoint::resolve(config_dir) {
        Ok(endpoint) => endpoint,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let jobs = match control::list(&endpoint).await {
        Ok(jobs) => jobs,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionAborted
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let jobs = jobs
        .iter()
        .filter(|value| value.get("sessionId").and_then(Value::as_str) == Some(session_id))
        .map(parse_job_required)
        .collect::<io::Result<Vec<_>>>()?;
    match jobs.as_slice() {
        [] => Ok(None),
        [job] => {
            endpoint.validate_credentials()?;
            Ok(Some((endpoint, job.clone())))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude Agent View reported duplicate live records for one session",
        )),
    }
}

async fn wait_for_job(config_dir: &Path, short: &str) -> io::Result<(control::Endpoint, Job)> {
    let deadline = tokio::time::Instant::now() + JOB_READY_TIMEOUT;
    let mut last_retryable_error;
    loop {
        match control::Endpoint::resolve(config_dir) {
            Ok(endpoint) => match control::list(&endpoint).await {
                Ok(jobs) => {
                    if let Some(job) = jobs
                        .iter()
                        .find(|value| value.get("short").and_then(Value::as_str) == Some(short))
                    {
                        let job = parse_job_required(job)?;
                        match endpoint.validate_credentials() {
                            Ok(()) => return Ok((endpoint, job)),
                            Err(error) if retryable_control_error(&error) => {
                                last_retryable_error = Some(error);
                            }
                            Err(error) => return Err(error),
                        }
                    } else {
                        last_retryable_error = None;
                    }
                }
                Err(error) if retryable_control_error(&error) => {
                    last_retryable_error = Some(error);
                }
                Err(error) => return Err(error),
            },
            Err(error) if retryable_control_error(&error) => {
                last_retryable_error = Some(error);
            }
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            let suffix = last_retryable_error
                .map(|error| format!(": {error}"))
                .unwrap_or_default();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("Claude Agent View session did not become controllable{suffix}"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn rollback_started_job(config_dir: &Path, short: &str) -> io::Result<()> {
    let endpoint = control::Endpoint::resolve(config_dir)?;
    let jobs = control::list(&endpoint).await?;
    if jobs
        .iter()
        .any(|value| value.get("short").and_then(Value::as_str) == Some(short))
    {
        kill_and_confirm(&endpoint, short).await?;
    }
    Ok(())
}

async fn kill_and_confirm(endpoint: &control::Endpoint, short: &str) -> io::Result<()> {
    let mut kill_error = None;
    match control::list(endpoint).await {
        Ok(jobs)
            if !jobs
                .iter()
                .any(|value| value.get("short").and_then(Value::as_str) == Some(short)) =>
        {
            return Ok(());
        }
        Ok(_) => {}
        Err(error) if retryable_control_error(&error) => {}
        Err(error) => return Err(error),
    }
    if let Err(error) = control::kill(endpoint, short).await {
        kill_error = Some(error);
    }
    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    let mut last_error: Option<io::Error>;
    loop {
        match control::list(endpoint).await {
            Ok(jobs)
                if !jobs
                    .iter()
                    .any(|value| value.get("short").and_then(Value::as_str) == Some(short)) =>
            {
                return Ok(());
            }
            Ok(_) => last_error = None,
            Err(error) if retryable_control_error(&error) => last_error = Some(error),
            Err(error) => return Err(error),
        }
        if tokio::time::Instant::now() >= deadline {
            let observation = last_error
                .map(|error| format!(": {error}"))
                .unwrap_or_default();
            let kill = kill_error
                .map(|error| format!("; kill request also failed: {error}"))
                .unwrap_or_default();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("Claude Agent View task did not stop{observation}{kill}"),
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn retryable_control_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::WouldBlock
    )
}

fn retryable_liveness_error(error: &io::Error) -> bool {
    retryable_control_error(error)
        || matches!(
            error.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::TimedOut | io::ErrorKind::BrokenPipe
        )
}

fn sustained_liveness_failure(
    failure: &mut Option<(tokio::time::Instant, String)>,
    now: tokio::time::Instant,
    error: &io::Error,
) -> Option<String> {
    let failure = failure.get_or_insert_with(|| (now, error.to_string()));
    failure.1 = error.to_string();
    (now.duration_since(failure.0) >= LIVENESS_FAILURE_TIMEOUT).then(|| failure.1.clone())
}

fn observe_live_job(
    jobs: io::Result<Vec<Value>>,
    short: &str,
    session_id: &str,
    expected_version: command::Version,
) -> io::Result<bool> {
    let jobs = jobs?;
    let matching = jobs
        .iter()
        .filter(|value| {
            value.get("short").and_then(Value::as_str) == Some(short)
                && value.get("sessionId").and_then(Value::as_str) == Some(session_id)
        })
        .collect::<Vec<_>>();
    let [value] = matching.as_slice() else {
        return Err(io::Error::new(
            if matching.is_empty() {
                io::ErrorKind::NotFound
            } else {
                io::ErrorKind::InvalidData
            },
            if matching.is_empty() {
                "Claude Agent View temporarily does not report the managed session"
            } else {
                "Claude Agent View reported duplicate managed-session records"
            },
        ));
    };
    let job = parse_job_required(value)?;
    validate_worker_version(&job, expected_version)?;
    Ok(!matches!(
        value.get("tempo").and_then(Value::as_str),
        None | Some("idle" | "blocked")
    ))
}

fn parse_job(value: &Value) -> Option<Job> {
    let short = value.get("short")?.as_str()?.trim();
    let session_id = value.get("sessionId")?.as_str()?.trim();
    let cwd = value.get("cwd")?.as_str()?.trim();
    let cli_version = command::parse_version(value.get("cliVersion")?.as_str()?)?;
    valid_short_id(short)
        .then_some(())
        .and_then(|_| validate_session_id(session_id).ok())?;
    (!cwd.is_empty()).then(|| Job {
        short: short.to_owned(),
        session_id: session_id.to_owned(),
        cwd: PathBuf::from(cwd),
        cli_version,
    })
}

fn parse_job_required(value: &Value) -> io::Result<Job> {
    parse_job(value).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude Agent View returned an invalid managed-session record",
        )
    })
}

fn validate_worker_version(job: &Job, expected: command::Version) -> io::Result<()> {
    if job.cli_version == expected {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "Claude Agent View worker version {}.{}.{} does not match the selected Claude executable {}.{}.{}",
            job.cli_version.0,
            job.cli_version.1,
            job.cli_version.2,
            expected.0,
            expected.1,
            expected.2,
        ),
    ))
}

fn with_cleanup_error(primary: io::Error, cleanup: io::Error) -> io::Error {
    io::Error::new(
        primary.kind(),
        format!("{primary}; cleanup also failed: {cleanup}"),
    )
}

fn with_optional_cleanup_error(primary: io::Error, cleanup: Option<io::Error>) -> io::Error {
    match cleanup {
        Some(cleanup) => with_cleanup_error(primary, cleanup),
        None => primary,
    }
}

fn validate_job_state(
    config_dir: &Path,
    job: &Job,
    expected_cwd: &Path,
    require_quiescent: bool,
) -> io::Result<Value> {
    let state_path = config_dir.join("jobs").join(&job.short).join("state.json");
    let metadata = std::fs::symlink_metadata(&state_path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_STATE_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude Agent View state file failed validation",
        ));
    }
    let state: Value = serde_json::from_slice(&std::fs::read(&state_path)?).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Claude Agent View state is invalid: {error}"),
        )
    })?;
    if state.get("sessionId").and_then(Value::as_str) != Some(job.session_id.as_str())
        || state.get("daemonShort").and_then(Value::as_str) != Some(job.short.as_str())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Claude Agent View state identity does not match the control record",
        ));
    }
    let observed_cwd = job.cwd.canonicalize()?;
    if observed_cwd != expected_cwd.canonicalize()? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Claude Agent View session is bound to a different working directory",
        ));
    }
    if require_quiescent {
        let tasks = state
            .pointer("/inFlight/tasks")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let queued = state
            .pointer("/inFlight/queued")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let tempo = state.get("tempo").and_then(Value::as_str);
        if tasks != 0
            || queued != 0
            || state.get("outcome").is_some_and(|value| !value.is_null())
            || !matches!(tempo, Some("idle" | "blocked"))
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Claude Agent View session has active or unsettled work",
            ));
        }
    }
    Ok(state)
}

enum ClientCommand {
    Prompt {
        delegation_id: String,
        text: String,
        response: oneshot::Sender<io::Result<String>>,
    },
    Cancel {
        turn_id: String,
        response: oneshot::Sender<io::Result<()>>,
    },
    RegisterNativeInput {
        delegation_id: String,
        text: String,
        response: oneshot::Sender<io::Result<()>>,
    },
    ForgetNativeInput {
        delegation_id: String,
        response: oneshot::Sender<()>,
    },
    Close {
        response: oneshot::Sender<io::Result<()>>,
    },
}

struct PendingControl {
    delegation_id: String,
    text: String,
    turn_id: String,
    accepted_at: tokio::time::Instant,
    saw_activity: bool,
    quiescent_since: Option<tokio::time::Instant>,
}

pub(super) struct ClaudeClient {
    commands: mpsc::Sender<ClientCommand>,
    pub(super) events: broadcast::Sender<AnalystEvent>,
    running: Arc<AtomicBool>,
    task: Mutex<Option<JoinHandle<()>>>,
    endpoint: control::Endpoint,
    short: String,
}

impl ClaudeClient {
    #[allow(clippy::too_many_arguments)]
    async fn new(
        endpoint: control::Endpoint,
        short: String,
        session_id: String,
        expected_version: command::Version,
        state_path: PathBuf,
        config_dir: PathBuf,
        generation: String,
        skip_existing_transcript: bool,
    ) -> io::Result<Self> {
        let (commands, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let running = Arc::new(AtomicBool::new(true));
        let mut transcript = TranscriptFollower::new(
            state_path,
            config_dir,
            session_id.clone(),
            skip_existing_transcript,
        );
        transcript.initialize().await?;
        let task = tokio::spawn(run_client(
            receiver,
            endpoint.clone(),
            short.clone(),
            session_id,
            expected_version,
            transcript,
            generation,
            events.clone(),
            Arc::clone(&running),
        ));
        Ok(Self {
            commands,
            events,
            running,
            task: Mutex::new(Some(task)),
            endpoint,
            short,
        })
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<AnalystEvent> {
        self.events.subscribe()
    }

    pub(super) fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub(super) async fn start_prompt(&self, delegation_id: &str, text: &str) -> io::Result<String> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(ClientCommand::Prompt {
                delegation_id: delegation_id.to_owned(),
                text: text.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn register_native_input(
        &self,
        delegation_id: &str,
        text: &str,
    ) -> io::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(ClientCommand::RegisterNativeInput {
                delegation_id: delegation_id.to_owned(),
                text: text.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn forget_native_input(&self, delegation_id: &str) -> io::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(ClientCommand::ForgetNativeInput {
                delegation_id: delegation_id.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())
    }

    pub(super) async fn cancel(&self, turn_id: &str) -> io::Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(ClientCommand::Cancel {
                turn_id: turn_id.to_owned(),
                response: sender,
            })
            .await
            .map_err(|_| closed_error())?;
        receiver.await.map_err(|_| closed_error())?
    }

    pub(super) async fn close(&self) -> io::Result<()> {
        let (sender, receiver) = oneshot::channel();
        if self
            .commands
            .send(ClientCommand::Close { response: sender })
            .await
            .is_ok()
        {
            tokio::time::timeout(STOP_TIMEOUT + Duration::from_secs(1), receiver)
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Claude managed-session stop confirmation timed out",
                    )
                })?
                .map_err(|_| closed_error())??;
        } else {
            kill_and_confirm(&self.endpoint, &self.short).await?;
        }
        let task = self.task.lock().ok().and_then(|mut task| task.take());
        if let Some(mut task) = task
            && tokio::time::timeout(STOP_TIMEOUT, &mut task).await.is_err()
        {
            task.abort();
            let _ = task.await;
        }
        self.running.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for ClaudeClient {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Ok(mut task) = self.task.lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_client(
    mut commands: mpsc::Receiver<ClientCommand>,
    endpoint: control::Endpoint,
    short: String,
    session_id: String,
    expected_version: command::Version,
    mut transcript: TranscriptFollower,
    generation: String,
    events: broadcast::Sender<AnalystEvent>,
    running: Arc<AtomicBool>,
) {
    let mut state =
        transcript::TranscriptState::new(generation.clone(), session_id.clone(), events.clone());
    let mut pending: Option<PendingControl> = None;
    let mut native_inputs = std::collections::VecDeque::new();
    let mut transcript_tick = tokio::time::interval(Duration::from_millis(50));
    transcript_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut liveness_tick = tokio::time::interval(Duration::from_secs(1));
    liveness_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut terminal_error = None;
    let mut expected_close = false;
    let mut liveness_failure: Option<(tokio::time::Instant, String)> = None;

    'client: loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(ClientCommand::Prompt { delegation_id, text, response }) => {
                    if pending.is_some() || state.active_turn_id().is_some() {
                        let _ = response.send(Err(io::Error::new(
                            io::ErrorKind::WouldBlock,
                            "Claude managed session already has an active or pending turn",
                        )));
                        continue;
                    }
                    match control::reply(&endpoint, &short, &text).await {
                        Ok(()) => {
                            let turn_id = format!("claude-{}", uuid::Uuid::new_v4().simple());
                            pending = Some(PendingControl {
                                delegation_id,
                                text,
                                turn_id: turn_id.clone(),
                                accepted_at: tokio::time::Instant::now(),
                                saw_activity: false,
                                quiescent_since: None,
                            });
                            let _ = response.send(Ok(turn_id));
                        }
                        Err(error) => {
                            let kind = error.kind();
                            let detail = error.to_string();
                            let _ = response.send(Err(io::Error::new(kind, detail.clone())));
                            if kind != io::ErrorKind::WouldBlock {
                                terminal_error = Some(detail);
                                break 'client;
                            }
                        }
                    }
                }
                Some(ClientCommand::Cancel { turn_id, response }) => {
                    let pending_matches = pending
                        .as_ref()
                        .is_some_and(|pending| pending.turn_id == turn_id);
                    let result = if state.active_turn_id() != Some(turn_id.as_str())
                        && !pending_matches
                    {
                        Err(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Claude managed session has no matching active turn",
                        ))
                    } else {
                        control::interrupt(&endpoint, &short).await
                    };
                    let _ = response.send(result);
                }
                Some(ClientCommand::RegisterNativeInput { delegation_id, text, response }) => {
                    let _ = response.send(super::native_input::register(
                        &mut native_inputs,
                        delegation_id,
                        text,
                    ));
                }
                Some(ClientCommand::ForgetNativeInput { delegation_id, response }) => {
                    super::native_input::forget(&mut native_inputs, &delegation_id);
                    let _ = response.send(());
                }
                Some(ClientCommand::Close { response }) => {
                    match kill_and_confirm(&endpoint, &short).await {
                        Ok(()) => {
                            expected_close = true;
                            let _ = response.send(Ok(()));
                            break 'client;
                        }
                        Err(error) => {
                            let kind = error.kind();
                            let detail = error.to_string();
                            let _ = response.send(Err(io::Error::new(kind, detail)));
                        }
                    }
                }
                None => {
                    terminal_error = Some("Claude managed session client closed".into());
                    break 'client;
                }
            },
            _ = transcript_tick.tick() => {
                match transcript.poll().await {
                    Ok(records) => {
                        for record in records {
                            let result = {
                                let controlled = pending.as_ref().map(|pending| transcript::PendingPrompt {
                                    delegation_id: &pending.delegation_id,
                                    text: &pending.text,
                                    turn_id: &pending.turn_id,
                                });
                                let native = native_inputs.front().map(|pending| {
                                    transcript::PendingNativeInput {
                                        delegation_id: &pending.delegation_id,
                                        text: &pending.expected_text,
                                    }
                                });
                                state.ingest_with_native(&record, controlled, native)
                            };
                            match result {
                                Ok(transcript::IngestOutcome::Controlled(turn_id)) => {
                                    debug_assert_eq!(
                                        pending.as_ref().map(|pending| pending.turn_id.as_str()),
                                        Some(turn_id.as_str())
                                    );
                                    pending = None;
                                }
                                Ok(transcript::IngestOutcome::Native(delegation_id)) => {
                                    debug_assert_eq!(
                                        native_inputs.front().map(|pending| pending.delegation_id.as_str()),
                                        Some(delegation_id.as_str())
                                    );
                                    native_inputs.pop_front();
                                }
                                Ok(transcript::IngestOutcome::None) => {}
                                Err(error) => {
                                    let detail = error.to_string();
                                    terminal_error = Some(detail);
                                    break 'client;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        terminal_error = Some(error.to_string());
                        break 'client;
                    }
                }
            },
            _ = liveness_tick.tick() => {
                let observation = observe_live_job(
                    control::list(&endpoint).await,
                    &short,
                    &session_id,
                    expected_version,
                );
                match observation {
                    Ok(active) => {
                        liveness_failure = None;
                        if let Some(pending) = pending.as_mut() {
                            let now = tokio::time::Instant::now();
                            if active {
                                pending.saw_activity = true;
                                pending.quiescent_since = None;
                            } else if pending.saw_activity {
                                let settled = pending.quiescent_since.get_or_insert(now);
                                if now.duration_since(*settled)
                                    >= PROMPT_SETTLED_CORRELATION_TIMEOUT
                                {
                                    terminal_error = Some("Claude completed a controlled prompt without publishing its transcript correlation".into());
                                    break 'client;
                                }
                            } else if now.duration_since(pending.accepted_at)
                                >= PROMPT_ACTIVITY_TIMEOUT
                            {
                                terminal_error = Some("Claude accepted a controlled prompt but neither started work nor published its transcript correlation".into());
                                break 'client;
                            }
                        }
                    }
                    Err(error) if retryable_liveness_error(&error) => {
                        let now = tokio::time::Instant::now();
                        if let Some(reason) = sustained_liveness_failure(
                            &mut liveness_failure,
                            now,
                            &error,
                        ) {
                            terminal_error = Some(reason);
                            break 'client;
                        }
                    }
                    Err(error) => {
                        terminal_error = Some(error.to_string());
                        break 'client;
                    }
                }
            }
        }
    }
    running.store(false, Ordering::Release);
    // Receivers may own the client through their session. Always wake them,
    // including a normal close, or their own sender keeps recv() alive forever.
    let _ = events.send(AnalystEvent {
        generation,
        message: json!({
            "method":MANAGED_AGENT_DISCONNECTED_METHOD,
            "params":{"threadId":session_id,"reason":terminal_error,"expected":expected_close}
        }),
        requested_delegation_id: None,
    });
}

async fn transcript_file_identity(file: &tokio::fs::File) -> io::Result<same_file::Handle> {
    // Retain an open handle: file identifiers must not be recycled between polls.
    // same-file uses stable platform APIs, including on our Windows MSRV.
    same_file::Handle::from_file(file.try_clone().await?.into_std().await)
}

struct TranscriptFollower {
    state_path: PathBuf,
    config_dir: PathBuf,
    session_id: String,
    path: Option<PathBuf>,
    identity: Option<same_file::Handle>,
    offset: u64,
    partial: Vec<u8>,
    partial_since: Option<tokio::time::Instant>,
    skip_existing: bool,
    discovered_from_store: bool,
    stale_published_path: Option<PathBuf>,
}

impl TranscriptFollower {
    fn new(
        state_path: PathBuf,
        config_dir: PathBuf,
        session_id: String,
        skip_existing: bool,
    ) -> Self {
        Self {
            state_path,
            config_dir,
            session_id,
            path: None,
            identity: None,
            offset: 0,
            partial: Vec::new(),
            partial_since: None,
            skip_existing,
            discovered_from_store: false,
            stale_published_path: None,
        }
    }

    async fn initialize(&mut self) -> io::Result<()> {
        // A fresh Agent View job does not create linkScanPath or its transcript
        // until the first prompt. Validate an already-published path now, but
        // let poll() discover a fresh transcript after control admission.
        if !self.skip_existing {
            return self.discover_path().await;
        }
        let deadline = tokio::time::Instant::now() + TRANSCRIPT_READY_TIMEOUT;
        loop {
            match self.discover_path().await {
                Ok(()) => {}
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        && tokio::time::Instant::now() < deadline => {}
                Err(error) => return Err(error),
            }
            if self.path.is_some() {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Claude Agent View did not expose its durable transcript",
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    async fn poll(&mut self) -> io::Result<Vec<Value>> {
        self.discover_path().await?;
        let Some(path) = self.path.as_ref() else {
            return Ok(Vec::new());
        };
        let mut file = tokio::fs::File::open(path).await?;
        let metadata = file.metadata().await?;
        if self.identity.as_ref() != Some(&transcript_file_identity(&file).await?) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude transcript file was replaced while the session was active",
            ));
        }
        if metadata.len() < self.offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude transcript was truncated while the session was active",
            ));
        }
        if metadata.len() == self.offset {
            self.require_complete_tail()?;
            return Ok(Vec::new());
        }
        file.seek(std::io::SeekFrom::Start(self.offset)).await?;
        let wanted = metadata
            .len()
            .saturating_sub(self.offset)
            .min(MAX_TRANSCRIPT_READ_BYTES);
        let mut bytes = Vec::with_capacity(wanted as usize);
        file.take(wanted).read_to_end(&mut bytes).await?;
        self.offset = self.offset.saturating_add(bytes.len() as u64);
        self.partial.extend(bytes);
        let records =
            transcript_buffer::take_records(&mut self.partial, MAX_TRANSCRIPT_LINE_BYTES)?;
        self.partial_since = (!self.partial.is_empty()).then(tokio::time::Instant::now);
        Ok(records)
    }

    fn require_complete_tail(&self) -> io::Result<()> {
        if self.partial_since.is_some_and(|started| {
            tokio::time::Instant::now().duration_since(started) >= PARTIAL_TAIL_TIMEOUT
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude transcript ended with an incomplete record",
            ));
        }
        Ok(())
    }

    async fn discover_path(&mut self) -> io::Result<()> {
        let metadata = match tokio::fs::symlink_metadata(&self.state_path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_STATE_BYTES
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude Agent View state file failed validation",
            ));
        }
        let bytes = tokio::fs::read(&self.state_path).await?;
        let Ok(state) = serde_json::from_slice::<Value>(&bytes) else {
            // Agent View may replace this small file while it is being read;
            // retry on the next poll before declaring transcript corruption.
            return Ok(());
        };
        let published = state
            .get("linkScanPath")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let path = if let Some(value) = published {
            PathBuf::from(value)
        } else {
            if self.path.is_some() && !self.discovered_from_store {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Claude Agent View stopped publishing the active transcript",
                ));
            }
            let Some(path) = transcript_path::find(&self.config_dir, &self.session_id)? else {
                return Ok(());
            };
            self.discovered_from_store = true;
            path
        };
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if filename != format!("{}.jsonl", self.session_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude Agent View state referenced a different transcript identity",
            ));
        }
        let path = transcript_path::recover_missing(
            &self.config_dir,
            &self.session_id,
            path,
            self.path.as_deref(),
            &mut self.stale_published_path,
            self.skip_existing,
        )
        .await?;
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Claude transcript must be a regular file",
            ));
        }
        let canonical = tokio::fs::canonicalize(&path).await?;
        let projects = tokio::fs::canonicalize(self.config_dir.join("projects")).await?;
        if !canonical.starts_with(&projects) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Claude transcript escaped the configured session store",
            ));
        }
        let file = tokio::fs::File::open(&canonical).await?;
        let metadata = file.metadata().await?;
        let identity = transcript_file_identity(&file).await?;
        if let Some(current) = self.path.as_ref() {
            if current != &canonical || self.identity.as_ref() != Some(&identity) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Claude Agent View rotated the transcript while the session was active",
                ));
            }
            return Ok(());
        }
        self.offset = if self.skip_existing {
            metadata.len()
        } else {
            0
        };
        self.skip_existing = false;
        self.identity = Some(identity);
        self.path = Some(canonical);
        Ok(())
    }
}

fn parse_short_id(text: &str) -> Option<String> {
    text.split(|character: char| !character.is_ascii_hexdigit())
        .find(|value| valid_short_id(value))
        .map(|value| value.to_ascii_lowercase())
}

fn valid_short_id(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_session_id(value: &str) -> io::Result<()> {
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid Claude session id"))
}

fn nonempty_detail<'a>(preferred: &'a str, fallback: &'a str) -> &'a str {
    if preferred.trim().is_empty() {
        fallback.trim()
    } else {
        preferred.trim()
    }
}

fn closed_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "Claude managed session is closed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_exact_short_ids() {
        assert_eq!(
            parse_short_id("backgrounded · a1B2c3D4 · analyst"),
            Some("a1b2c3d4".into())
        );
        assert_eq!(parse_short_id("abc def 123456789"), None);
    }

    #[test]
    fn validates_job_identity_and_quiescence() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = temp.path().join("config");
        let cwd = temp.path().join("workspace");
        let short = "1234abcd";
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        std::fs::create_dir_all(config.join("jobs").join(short)).expect("jobs");
        std::fs::create_dir(&cwd).expect("cwd");
        cccc_core::fs::write_json(
            &config.join("jobs").join(short).join("state.json"),
            &json!({
                "sessionId":session_id,
                "daemonShort":short,
                "inFlight":{"tasks":0,"queued":0},
                "tempo":"idle",
                "outcome":null,
            }),
        )
        .expect("state");
        let job = Job {
            short: short.into(),
            session_id: session_id.into(),
            cwd: cwd.clone(),
            cli_version: (2, 1, 259),
        };
        validate_job_state(&config, &job, &cwd, true).expect("valid job");

        cccc_core::fs::write_json(
            &config.join("jobs").join(short).join("state.json"),
            &json!({
                "sessionId":session_id,
                "daemonShort":short,
                "inFlight":{"tasks":1,"queued":0},
                "tempo":"working",
            }),
        )
        .expect("busy state");
        assert_eq!(
            validate_job_state(&config, &job, &cwd, true)
                .expect_err("busy job")
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn control_startup_retries_only_transient_endpoint_errors() {
        assert!(retryable_control_error(&io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "starting"
        )));
        assert!(!retryable_control_error(&io::Error::new(
            io::ErrorKind::PermissionDenied,
            "wrong owner"
        )));
        assert!(!retryable_control_error(&io::Error::new(
            io::ErrorKind::InvalidData,
            "protocol drift"
        )));
    }

    #[test]
    fn managed_job_requires_worker_version_and_exact_identity() {
        let value = json!({
            "short":"1234abcd",
            "sessionId":"52b41c61-e23c-4b7c-8b60-809c347451b5",
            "cwd":"/tmp/workspace",
            "cliVersion":"2.1.260",
        });
        let job = parse_job_required(&value).expect("versioned job");
        validate_worker_version(&job, (2, 1, 260)).expect("matching worker");
        assert_eq!(
            validate_worker_version(&job, (2, 1, 261))
                .expect_err("version drift")
                .kind(),
            io::ErrorKind::Unsupported
        );

        let mut missing_version = value;
        missing_version
            .as_object_mut()
            .expect("job object")
            .remove("cliVersion");
        assert_eq!(
            parse_job_required(&missing_version)
                .expect_err("unversioned worker")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn liveness_absence_is_retryable_but_protocol_drift_is_not() {
        let short = "1234abcd";
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        let missing = observe_live_job(Ok(Vec::new()), short, session_id, (2, 1, 260))
            .expect_err("missing job");
        assert!(retryable_liveness_error(&missing));

        let malformed = observe_live_job(
            Ok(vec![json!({
                "short":short,
                "sessionId":session_id,
                "cwd":"/tmp/workspace",
            })]),
            short,
            session_id,
            (2, 1, 260),
        )
        .expect_err("missing version");
        assert_eq!(malformed.kind(), io::ErrorKind::InvalidData);
        assert!(!retryable_liveness_error(&malformed));

        assert!(
            observe_live_job(
                Ok(vec![json!({
                    "short":short,
                    "sessionId":session_id,
                    "cwd":"/tmp/workspace",
                    "cliVersion":"2.1.260",
                    "tempo":"active",
                })]),
                short,
                session_id,
                (2, 1, 260),
            )
            .expect("active observation")
        );
        assert!(
            !observe_live_job(
                Ok(vec![json!({
                    "short":short,
                    "sessionId":session_id,
                    "cwd":"/tmp/workspace",
                    "cliVersion":"2.1.260",
                    "tempo":"idle",
                })]),
                short,
                session_id,
                (2, 1, 260),
            )
            .expect("idle observation")
        );

        let now = tokio::time::Instant::now();
        let mut failure = None;
        assert!(sustained_liveness_failure(&mut failure, now, &missing).is_none());
        assert!(
            sustained_liveness_failure(
                &mut failure,
                now + LIVENESS_FAILURE_TIMEOUT - Duration::from_millis(1),
                &missing,
            )
            .is_none(),
            "one or more transient observations inside the grace period must keep the live TUI attached"
        );
        assert!(
            sustained_liveness_failure(&mut failure, now + LIVENESS_FAILURE_TIMEOUT, &missing,)
                .is_some(),
            "a sustained unverifiable session must eventually fail closed"
        );
    }

    #[tokio::test]
    async fn fresh_follower_discovers_transcript_after_first_prompt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("config");
        let projects_dir = config_dir.join("projects/workspace");
        let job_dir = config_dir.join("jobs/1234abcd");
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        std::fs::create_dir_all(&projects_dir).expect("projects");
        std::fs::create_dir_all(&job_dir).expect("job");
        let state_path = job_dir.join("state.json");
        cccc_core::fs::write_json(
            &state_path,
            &json!({"sessionId":session_id,"detail":"(idle — send a prompt to start)"}),
        )
        .expect("idle state");

        let mut follower =
            TranscriptFollower::new(state_path.clone(), config_dir, session_id.into(), false);
        follower.initialize().await.expect("fresh idle follower");
        assert!(follower.poll().await.expect("idle poll").is_empty());

        let transcript_path = projects_dir.join(format!("{session_id}.jsonl"));
        let first = json!({
            "type":"user",
            "sessionId":session_id,
            "promptId":"first-prompt",
            "message":{"content":"first work"},
        });
        std::fs::write(&transcript_path, format!("{first}\n")).expect("first transcript");
        cccc_core::fs::write_json(&state_path, &json!({"linkScanPath":transcript_path}))
            .expect("materialized state");

        assert_eq!(follower.poll().await.expect("first records"), [first]);
    }

    #[tokio::test]
    async fn resumed_follower_fences_history_before_accepting_new_work() {
        use std::io::Write as _;

        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("config");
        let projects_dir = config_dir.join("projects/workspace");
        let job_dir = config_dir.join("jobs/1234abcd");
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        std::fs::create_dir_all(&projects_dir).expect("projects");
        std::fs::create_dir_all(&job_dir).expect("job");
        let transcript_path = projects_dir.join(format!("{session_id}.jsonl"));
        std::fs::write(
            &transcript_path,
            format!(
                "{}\n",
                json!({
                    "type":"user",
                    "sessionId":session_id,
                    "promptId":"old-prompt",
                    "message":{"content":"old work"},
                })
            ),
        )
        .expect("old transcript");
        let state_path = job_dir.join("state.json");
        cccc_core::fs::write_json(&state_path, &json!({"linkScanPath":transcript_path}))
            .expect("state");

        let mut follower = TranscriptFollower::new(state_path, config_dir, session_id.into(), true);
        follower.initialize().await.expect("history fence");
        let next = json!({
            "type":"user",
            "sessionId":session_id,
            "promptId":"new-prompt",
            "message":{"content":"new work"},
        });
        let mut transcript = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript_path)
            .expect("open transcript");
        writeln!(transcript, "{next}").expect("append new record");

        assert_eq!(follower.poll().await.expect("new records"), [next]);
    }

    #[test]
    fn only_positive_empty_job_evidence_allows_transcript_free_resume() {
        let temp = tempfile::tempdir().expect("tempdir");
        let job = temp.path().join("jobs/1234abcd");
        std::fs::create_dir_all(&job).expect("job");
        let id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        let empty = json!({"sessionId":id,"intent":"","linkScanOffset":0,"linkScanPath":null,"output":null,"tokens":null});
        cccc_core::fs::write_json(&job.join("state.json"), &empty).expect("empty state");
        assert!(known_empty_session(temp.path(), id).expect("empty"));
        assert!(!known_empty_session(temp.path(), "other").expect("unknown"));
        let mut counted_empty = empty.clone();
        counted_empty["tokens"] = json!(0);
        cccc_core::fs::write_json(&job.join("state.json"), &counted_empty)
            .expect("initialized empty counter");
        assert!(known_empty_session(temp.path(), id).expect("zero usage is still empty"));
        for (field, value) in [
            ("intent", json!("first prompt")),
            ("linkScanOffset", json!(42)),
            ("linkScanPath", json!("missing-history.jsonl")),
            ("output", json!("answer")),
            ("tokens", json!(1)),
            ("tokens", json!("0")),
            ("tokens", json!(-1)),
        ] {
            let mut state = empty.clone();
            state[field] = value;
            cccc_core::fs::write_json(&job.join("state.json"), &state).expect("used state");
            assert!(
                !known_empty_session(temp.path(), id).expect("history required"),
                "{field}"
            );
        }
        cccc_core::fs::write_json(&job.join("state.json"), &empty).expect("stale empty state");
        let project = temp.path().join("projects/workspace");
        std::fs::create_dir_all(&project).expect("project");
        std::fs::write(project.join(format!("{id}.jsonl")), b"{}\n").expect("unpublished history");
        assert!(
            !known_empty_session(temp.path(), id).expect("unpublished history must be retained")
        );
    }

    #[tokio::test]
    async fn follower_detects_same_path_replacement_with_equal_length() {
        let temp = tempfile::tempdir().expect("tempdir");
        let project = temp.path().join("projects/workspace");
        std::fs::create_dir_all(&project).expect("project");
        let id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        let path = project.join(format!("{id}.jsonl"));
        let state = temp.path().join("state.json");
        std::fs::write(&path, b"{}\n").expect("transcript");
        cccc_core::fs::write_json(&state, &json!({"linkScanPath":path})).expect("state");
        let mut follower = TranscriptFollower::new(state, temp.path().into(), id.into(), true);
        follower.initialize().await.expect("initialize");
        std::fs::rename(&path, project.join("old.jsonl")).expect("retain old file");
        std::fs::write(&path, b"{}\n").expect("replacement");
        assert_eq!(
            follower
                .poll()
                .await
                .expect_err("replacement must fail")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn follower_rejects_transcript_rotation_and_stale_partial_tail() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("config");
        let first_dir = config_dir.join("projects/first");
        let second_dir = config_dir.join("projects/second");
        let job_dir = config_dir.join("jobs/1234abcd");
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        std::fs::create_dir_all(&first_dir).expect("first project");
        std::fs::create_dir_all(&second_dir).expect("second project");
        std::fs::create_dir_all(&job_dir).expect("job");
        let first = first_dir.join(format!("{session_id}.jsonl"));
        let second = second_dir.join(format!("{session_id}.jsonl"));
        std::fs::write(&first, b"{").expect("partial transcript");
        std::fs::write(&second, b"").expect("replacement transcript");
        let state_path = job_dir.join("state.json");
        cccc_core::fs::write_json(&state_path, &json!({"linkScanPath":first}))
            .expect("initial state");

        let mut follower =
            TranscriptFollower::new(state_path.clone(), config_dir, session_id.into(), false);
        follower.initialize().await.expect("initialize");
        assert!(follower.poll().await.expect("read partial").is_empty());
        follower.partial_since = Some(tokio::time::Instant::now() - PARTIAL_TAIL_TIMEOUT);
        assert_eq!(
            follower
                .poll()
                .await
                .expect_err("stale partial tail")
                .kind(),
            io::ErrorKind::InvalidData
        );

        follower.partial.clear();
        follower.partial_since = None;
        cccc_core::fs::write_json(&state_path, &json!({"linkScanPath":second}))
            .expect("rotated state");
        assert_eq!(
            follower
                .poll()
                .await
                .expect_err("rotated transcript")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agent_view_launch_control_transcript_and_stop_share_one_session() {
        exercise_managed_session(false, true, false).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn transcript_failure_stops_the_owned_background_job() {
        exercise_managed_session(true, false, false).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn actor_transcript_failure_stops_job_and_releases_reader() {
        exercise_managed_session(true, true, false).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn resume_ack_keeps_managed_control_and_delivery_alive() {
        exercise_managed_session(false, true, true).await;
    }

    #[cfg(unix)]
    async fn exercise_managed_session(fail_transcript: bool, actor_reader: bool, resume_ack: bool) {
        use sha2::{Digest, Sha256};
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        struct ControlDirectory(PathBuf);

        impl Drop for ControlDirectory {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("config");
        let workspace = temp.path().join("workspace");
        let executable = temp.path().join("claude");
        let short = "abcdef12";
        let session_id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
        std::fs::create_dir_all(config_dir.join("daemon")).expect("daemon config");
        std::fs::create_dir_all(config_dir.join("jobs").join(short)).expect("job state");
        std::fs::create_dir_all(config_dir.join("projects/workspace")).expect("projects");
        std::fs::create_dir(&workspace).expect("workspace");
        std::fs::write(
            &executable,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '2.1.259 (Claude Code)\\n'\nelse\n  printf 'started · abcdef12\\n'\nfi\n",
        )
        .expect("fake Claude");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
            .expect("executable permissions");
        let control_key = config_dir.join("daemon/control.key");
        std::fs::write(&control_key, "0123456789abcdef0123456789abcdef\n").expect("control key");
        std::fs::set_permissions(&control_key, std::fs::Permissions::from_mode(0o600))
            .expect("control key permissions");

        let config_dir = config_dir.canonicalize().expect("canonical config");
        let digest = format!(
            "{:x}",
            Sha256::digest(config_dir.to_string_lossy().as_bytes())
        );
        let control_dir = Path::new("/tmp")
            .join(format!(
                "cc-daemon-{}",
                config_dir.metadata().expect("metadata").uid()
            ))
            .join(&digest[..8]);
        std::fs::create_dir_all(&control_dir).expect("control directory");
        std::fs::set_permissions(&control_dir, std::fs::Permissions::from_mode(0o700))
            .expect("control directory permissions");
        let _control_directory = ControlDirectory(control_dir.clone());
        let listener = tokio::net::UnixListener::bind(control_dir.join("control.sock"))
            .expect("control socket");

        let transcript_path = config_dir
            .join("projects/workspace")
            .join(format!("{session_id}.jsonl"));
        let state_path = config_dir.join("jobs").join(short).join("state.json");
        cccc_core::fs::write_json(
            &state_path,
            &json!({
                "sessionId":session_id,
                "daemonShort":short,
                "cwd":workspace,
                "inFlight":{"tasks":0,"queued":0},
                "tempo":"idle",
                "outcome":null,
                "detail":"(idle — send a prompt to start)",
            }),
        )
        .expect("state");

        let transcript_release = Arc::new(tokio::sync::Notify::new());
        let server_transcript_release = Arc::clone(&transcript_release);
        let server_transcript = transcript_path.clone();
        let server_state = state_path.clone();
        let server_workspace = workspace.clone();
        let reject_control = Arc::new(AtomicBool::new(false));
        let server_reject_control = Arc::clone(&reject_control);
        let server = tokio::spawn(async move {
            let mut stopped = false;
            loop {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                stream.read_line(&mut line).await.expect("request");
                let request: Value = serde_json::from_str(&line).expect("request JSON");
                let operation = request["op"].as_str().expect("operation");
                let response = match operation {
                    "list" if server_reject_control.load(Ordering::Acquire) => {
                        json!({"ok":false,"op":"list","error":"test control failure"})
                    }
                    "list" => {
                        if stopped {
                            json!({"ok":true,"op":"list","jobs":[]})
                        } else {
                            json!({
                                "ok":true,
                                "op":"list",
                                "jobs":[{
                                    "short":short,
                                    "sessionId":session_id,
                                    "cwd":server_workspace,
                                    "cliVersion":"2.1.259",
                                }],
                            })
                        }
                    }
                    "reply" => {
                        let prompt = request["text"].as_str().expect("prompt").to_owned();
                        let transcript_path = server_transcript.clone();
                        let state_path = server_state.clone();
                        let workspace = server_workspace.clone();
                        let transcript_release = Arc::clone(&server_transcript_release);
                        tokio::spawn(async move {
                            transcript_release.notified().await;
                            let mut records = Vec::new();
                            if resume_ack {
                                records.extend([
                                    json!({"type":"user","sessionId":session_id,"isMeta":true,
                                        "message":{"content":"Continue from where you left off."}}),
                                    json!({"type":"assistant","sessionId":session_id,"isApiErrorMessage":false,
                                        "message":{"model":"<synthetic>","content":[{"type":"text","text":"No response requested."}]}}),
                                ]);
                            }
                            records.extend([
                                json!({
                                    "type":"user",
                                    "sessionId":session_id,
                                    "promptId":"prompt-controlled",
                                    "message":{"content":prompt},
                                }),
                                json!({
                                    "type":"assistant",
                                    "sessionId":session_id,
                                    "message":{"content":[{"type":"text","text":"managed answer"}]},
                                }),
                                json!({
                                    "type":"system",
                                    "sessionId":session_id,
                                    "subtype":"turn_duration",
                                }),
                            ]);
                            tokio::fs::write(&transcript_path, [])
                                .await
                                .expect("create transcript");
                            cccc_core::fs::write_json(
                                &state_path,
                                &json!({
                                    "sessionId":session_id,
                                    "daemonShort":short,
                                    "cwd":workspace,
                                    "linkScanPath":transcript_path,
                                    "inFlight":{"tasks":1,"queued":0},
                                    "tempo":"working",
                                    "outcome":null,
                                }),
                            )
                            .expect("materialize transcript state");
                            let mut transcript = tokio::fs::OpenOptions::new()
                                .append(true)
                                .open(&transcript_path)
                                .await
                                .expect("open transcript");
                            for record in records {
                                transcript
                                    .write_all(format!("{record}\n").as_bytes())
                                    .await
                                    .expect("append transcript");
                            }
                            transcript.flush().await.expect("flush transcript");
                        });
                        json!({"ok":true,"op":"reply"})
                    }
                    "kill" => {
                        stopped = true;
                        json!({"ok":true,"op":"kill"})
                    }
                    other => panic!("unexpected control operation: {other}"),
                };
                let stream = stream.get_mut();
                stream
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .expect("response");
                stream.flush().await.expect("response flush");
                if stopped && operation == "list" {
                    break;
                }
            }
        });

        let launched = launch(
            command::PreparedClaude {
                executable: executable.to_string_lossy().into_owned(),
                arguments: Vec::new(),
                launch_environment: BTreeMap::new(),
                config_dir,
                settings_path: temp.path().join("managed-settings.json"),
            },
            &workspace,
            "generation-12345678",
            SessionPurpose::Actor,
            None,
        )
        .await
        .expect("launch Agent View");
        assert_eq!(launched.session_id, session_id);
        assert!(!launched.resumed);
        assert_eq!(
            launched.tui_command,
            [
                executable.to_string_lossy().into_owned(),
                "attach".into(),
                short.into(),
            ]
        );
        let mut events = launched.protocol.subscribe();
        let turn_id = launched
            .protocol
            .start_prompt("delegation-1", "inspect this")
            .await
            .expect("controlled prompt");
        assert!(turn_id.starts_with("claude-"));
        assert!(
            events.try_recv().is_err(),
            "control acceptance must not wait for transcript publication"
        );
        transcript_release.notify_one();

        let mut final_text = None;
        let mut completed = false;
        while !completed {
            let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .expect("event timeout")
                .expect("event");
            if event.message["method"] == "item/completed"
                && event.message["params"]["item"]["type"] == "agentMessage"
            {
                final_text = event.message["params"]["item"]["text"]
                    .as_str()
                    .map(str::to_owned);
            }
            completed = event.message["method"] == "turn/completed";
        }
        assert_eq!(final_text.as_deref(), Some("managed answer"));
        let session = Arc::new(super::super::AnalystSession {
            binding: super::super::WorkspaceBinding { root: workspace },
            generation: "generation-12345678".into(),
            runtime: cccc_contracts::ActorRuntime::Claude,
            endpoint: String::new(),
            thread_id: launched.session_id,
            remote_tui_prefix: Vec::new(),
            environment: launched.environment,
            protocol: super::super::ManagedProtocol::Claude(launched.protocol),
            process: None,
            auxiliary_processes: Vec::new(),
            native_tui_command: Some(launched.tui_command),
            cleanup_paths: launched.cleanup_paths,
            thread_resumed: launched.resumed,
            delegations: tokio::sync::Mutex::new(Default::default()),
        });
        let _lifecycle = if fail_transcript && !actor_reader {
            let lifecycle = crate::ops::codex_voice_lifecycle::AnalystLifecycle::start(session);
            let mut transcript = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&transcript_path)
                .await
                .expect("transcript");
            transcript
                .write_all(b"not-json\n")
                .await
                .expect("corrupt transcript");
            transcript.flush().await.expect("flush corruption");
            Some(lifecycle)
        } else {
            crate::ops::local_headless::verify_claude_reader_release(
                session,
                fail_transcript.then_some(transcript_path.as_path()),
                &reject_control,
            )
            .await;
            let ended = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let event = events.recv().await.expect("close event");
                    if event.message["method"] == MANAGED_AGENT_DISCONNECTED_METHOD {
                        break event;
                    }
                }
            })
            .await
            .expect("normal close must wake event readers");
            assert_eq!(ended.message["params"]["expected"], !fail_transcript);
            None
        };
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("server timeout")
            .expect("server");
    }
}

#[cfg(test)]
#[path = "claude/transcript_buffer_tests.rs"]
mod transcript_buffer_tests;
