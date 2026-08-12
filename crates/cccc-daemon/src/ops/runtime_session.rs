use cccc_contracts::utc_now;
use cccc_core::{GroupStore, HomeLayout};
use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod codex_hook;
mod grok;
pub use grok::{prepare as prepare_grok_command, prepare_fresh as prepare_fresh_grok_command};

const DEFAULT_CAPTURE_SECONDS: f64 = 8.0;
const STATUS_FALLBACK_GRACE: Duration = Duration::from_secs(2);
const CODEX_HOOK_CAPTURE_SOURCE: &str = "codex_session_start_hook";
const CODEX_STATUS_CAPTURE_SOURCE: &str = "codex_status_command";
const NO_RESUME_VALUES: [&str; 4] = ["0", "false", "no", "off"];
const CODEX_SUBCOMMANDS: [&str; 11] = [
    "app-server",
    "completion",
    "debug",
    "exec",
    "help",
    "login",
    "logout",
    "mcp",
    "proto",
    "resume",
    "sandbox",
];

pub struct PreparedCommand {
    pub command: Vec<String>,
    pub resumed_session_id: Option<String>,
}

pub fn prepare_codex_command(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
) -> PreparedCommand {
    let fresh = || PreparedCommand {
        command: base_command.to_vec(),
        resumed_session_id: None,
    };
    if !resume_enabled() || !supports_codex_resume(base_command) {
        return fresh();
    }
    let Ok(mut document) = read(home, group_id, actor_id) else {
        return fresh();
    };
    if string(&document, "runtime") != "codex"
        || string(&document, "status") != "usable"
        || !document
            .get("resume_eligible")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || string(&document, "workspace_path") != workspace_path(cwd)
        || string(&document, "model") != model_from_command(base_command)
    {
        return fresh();
    }

    let provider_session_id = string(&document, "provider_session_id");
    let provider_thread_id = string(&document, "provider_thread_id");
    if provider_session_id.is_empty() && !provider_thread_id.is_empty() {
        if let Err(error) = mark_resume_failed(
            home,
            group_id,
            actor_id,
            "saved app-server thread cannot be resumed by Codex CLI; starting a fresh session",
        ) {
            tracing::warn!(%error, %group_id, %actor_id, "failed to invalidate app-server resume metadata");
        }
        return fresh();
    }
    if !valid_session_id(&provider_session_id)
        || string(&document, "command_fingerprint") != command_fingerprint(base_command)
    {
        return fresh();
    }
    let session_id = provider_session_id;

    let now = utc_now();
    document.insert("last_resume_attempt_at".into(), json!(now));
    document.insert("updated_at".into(), json!(utc_now()));
    if let Err(error) = write(home, group_id, actor_id, &document) {
        tracing::warn!(%error, %group_id, %actor_id, "failed to persist resume attempt");
        return fresh();
    }

    let mut command = base_command.to_vec();
    command.extend(["resume".into(), session_id.clone()]);
    PreparedCommand {
        command,
        resumed_session_id: Some(session_id),
    }
}

pub fn prepare_codex_app_thread(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &[String],
    model: &str,
) -> std::io::Result<Option<String>> {
    if !resume_enabled() {
        return Ok(None);
    }
    let Ok(mut document) = read(home, group_id, actor_id) else {
        return Ok(None);
    };
    if string(&document, "runtime") != "codex"
        || string(&document, "status") != "usable"
        || !document
            .get("resume_eligible")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || string(&document, "workspace_path") != workspace_path(cwd)
        || string(&document, "command_fingerprint") != app_thread_command_fingerprint(command)
        || string(&document, "model") != model.trim()
        || !string(&document, "provider_session_id").is_empty()
    {
        return Ok(None);
    }
    let thread_id = string(&document, "provider_thread_id");
    if thread_id.is_empty() {
        return Ok(None);
    }
    let now = utc_now();
    document.insert("last_resume_attempt_at".into(), json!(now));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)?;
    Ok(Some(thread_id))
}

pub fn record_codex_app_thread(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &[String],
    thread_id: &str,
    resumed: bool,
) -> std::io::Result<()> {
    if !resume_enabled() {
        return Ok(());
    }
    if thread_id.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty Codex app-server thread id",
        ));
    }
    let now = utc_now();
    let document = Map::from_iter([
        ("v".into(), json!(1)),
        ("kind".into(), json!("runtime_session")),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("codex")),
        ("runner".into(), json!("headless")),
        ("workspace_path".into(), json!(workspace_path(cwd))),
        (
            "command_fingerprint".into(),
            json!(app_thread_command_fingerprint(command)),
        ),
        ("model".into(), json!(model_from_command(command))),
        ("provider_session_id".into(), json!("")),
        ("provider_thread_id".into(), json!(thread_id.trim())),
        ("resume_command_hint".into(), json!("")),
        (
            "captured_from".into(),
            json!(if resumed {
                "app_server_thread_resume"
            } else {
                "app_server_thread_start"
            }),
        ),
        ("status".into(), json!("usable")),
        ("resume_eligible".into(), json!(true)),
        ("last_seen_at".into(), json!(now)),
        ("last_resume_attempt_at".into(), json!("")),
        ("last_resume_error".into(), json!("")),
        ("failure_count".into(), json!(0)),
        ("updated_at".into(), json!(utc_now())),
    ]);
    write(home, group_id, actor_id, &document)
}

pub fn prepare_claude_headless_session(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &[String],
) -> std::io::Result<Option<(String, bool)>> {
    if !resume_enabled() || !supports_claude_managed_session(command) {
        return Ok(None);
    }
    if let Ok(mut document) = read(home, group_id, actor_id)
        && string(&document, "runtime") == "claude"
        && string(&document, "status") == "usable"
        && document
            .get("resume_eligible")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        && string(&document, "workspace_path") == workspace_path(cwd)
        && string(&document, "command_fingerprint") == command_fingerprint(command)
        && string(&document, "model") == model_from_command(command)
        && string(&document, "provider_thread_id").is_empty()
    {
        let session_id = string(&document, "provider_session_id");
        if !session_id.is_empty() {
            document.insert("last_resume_attempt_at".into(), json!(utc_now()));
            document.insert("updated_at".into(), json!(utc_now()));
            write(home, group_id, actor_id, &document)?;
            return Ok(Some((session_id, true)));
        }
    }
    Ok(Some((uuid::Uuid::new_v4().to_string(), false)))
}

pub fn record_claude_headless_session(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    command: &[String],
    session_id: &str,
    resumed: bool,
) -> std::io::Result<()> {
    if !resume_enabled() {
        return Ok(());
    }
    if session_id.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "empty Claude headless session id",
        ));
    }
    let now = utc_now();
    let document = Map::from_iter([
        ("v".into(), json!(1)),
        ("kind".into(), json!("runtime_session")),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("claude")),
        ("runner".into(), json!("headless")),
        ("workspace_path".into(), json!(workspace_path(cwd))),
        (
            "command_fingerprint".into(),
            json!(command_fingerprint(command)),
        ),
        ("model".into(), json!(model_from_command(command))),
        ("provider_session_id".into(), json!(session_id.trim())),
        ("provider_thread_id".into(), json!("")),
        ("resume_command_hint".into(), json!("")),
        (
            "captured_from".into(),
            json!(if resumed {
                "claude_resume_command"
            } else {
                "claude_generated_session_id"
            }),
        ),
        ("status".into(), json!("usable")),
        ("resume_eligible".into(), json!(true)),
        ("last_seen_at".into(), json!(now)),
        ("last_resume_attempt_at".into(), json!("")),
        ("last_resume_error".into(), json!("")),
        ("failure_count".into(), json!(0)),
        ("updated_at".into(), json!(utc_now())),
    ]);
    write(home, group_id, actor_id, &document)
}

pub fn schedule_codex_session_capture(
    home: HomeLayout,
    group_id: String,
    actor_id: String,
    cwd: PathBuf,
    base_command: Vec<String>,
    expected_started_at: String,
) {
    let timeout = capture_seconds();
    if timeout <= 0.0 || !resume_enabled() || !supports_codex_resume(&base_command) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name(format!("cccc-codex-session:{group_id}:{actor_id}"))
        .spawn(move || {
            capture_codex_session(
                &home,
                &group_id,
                &actor_id,
                &cwd,
                &base_command,
                &expected_started_at,
                Duration::from_secs_f64(timeout),
            );
        });
}

pub fn resume_failure(group_id: &str, actor_id: &str) -> Option<String> {
    let history = cccc_runtime::retained_history(group_id, actor_id).ok()?;
    resume_failure_marker(&history.data).map(str::to_owned)
}

pub(super) fn resume_failure_marker(text: &str) -> Option<&'static str> {
    let plain = strip_ansi(text).to_ascii_lowercase();
    [
        "no conversation found",
        "no saved session found",
        "conversation not found",
        "session not found",
        "thread not found",
        "could not resume",
        "failed to resume",
        "invalid session",
        "invalid thread",
    ]
    .iter()
    .find(|marker| plain.contains(**marker))
    .copied()
}

pub fn mark_resume_failed(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    error: &str,
) -> std::io::Result<()> {
    let Ok(mut document) = read(home, group_id, actor_id) else {
        return Ok(());
    };
    let failures = document
        .get("failure_count")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(1);
    document.insert("status".into(), json!("resume_failed"));
    document.insert("resume_eligible".into(), json!(false));
    document.insert("failure_count".into(), json!(failures));
    document.insert("last_resume_error".into(), json!(truncate(error, 1000)));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group_id, actor_id, &document)
}

pub fn remove(home: &HomeLayout, group_id: &str, actor_id: &str) -> std::io::Result<()> {
    let path = path(home, group_id, actor_id)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn snapshot(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> std::io::Result<Option<Map<String, Value>>> {
    let session_path = path(home, group_id, actor_id)?;
    if !session_path.exists() {
        return Ok(None);
    }
    read(home, group_id, actor_id).map(Some)
}

pub fn restore_snapshot(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    snapshot: Option<&Map<String, Value>>,
) -> std::io::Result<()> {
    if let Some(document) = snapshot {
        write(home, group_id, actor_id, document)
    } else {
        remove(home, group_id, actor_id)
    }
}

pub fn actor_fields(home: &HomeLayout, group_id: &str, actor_id: &str) -> Map<String, Value> {
    let document = read(home, group_id, actor_id).unwrap_or_default();
    Map::from_iter([
        (
            "runtime_session_status".into(),
            nullable_string(&document, "status"),
        ),
        (
            "runtime_session_resume_eligible".into(),
            document
                .get("resume_eligible")
                .cloned()
                .filter(Value::is_boolean)
                .unwrap_or(Value::Null),
        ),
        (
            "runtime_session_last_resume_error".into(),
            nullable_string(&document, "last_resume_error"),
        ),
    ])
}

fn capture_codex_session(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
    expected_started_at: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    let mut submitted = false;
    let mut first_output_at = None;
    while Instant::now() <= deadline {
        let Ok(status) = cccc_runtime::status(group_id, actor_id) else {
            return;
        };
        if !status.running || status.started_at != expected_started_at {
            return;
        }
        let hook_pending = match codex_hook::observe(home, group_id, actor_id, status.pid) {
            codex_hook::Observation::Ready(session_id) => {
                if cccc_runtime::status(group_id, actor_id).is_ok_and(|current| {
                    current.running && current.started_at == expected_started_at
                }) {
                    let _ = record_codex_session(
                        home,
                        group_id,
                        actor_id,
                        cwd,
                        base_command,
                        &session_id,
                        CODEX_HOOK_CAPTURE_SOURCE,
                    );
                }
                return;
            }
            codex_hook::Observation::Pending => true,
            codex_hook::Observation::Unavailable => false,
        };
        let history = cccc_runtime::history(group_id, actor_id, None, 64_000)
            .map(|page| page.data)
            .unwrap_or_default();
        if let Some(session_id) = parse_codex_session_id(&history) {
            if cccc_runtime::status(group_id, actor_id)
                .is_ok_and(|current| current.running && current.started_at == expected_started_at)
            {
                let _ = record_codex_session(
                    home,
                    group_id,
                    actor_id,
                    cwd,
                    base_command,
                    &session_id,
                    CODEX_STATUS_CAPTURE_SOURCE,
                );
            }
            return;
        }
        if !history.is_empty() && first_output_at.is_none() {
            first_output_at = Some(Instant::now());
        }
        let ready =
            first_output_at.is_some_and(|started| started.elapsed() >= Duration::from_millis(300));
        let fallback_grace = STATUS_FALLBACK_GRACE.min(timeout / 2);
        let fallback_due = deadline.saturating_duration_since(Instant::now())
            <= if hook_pending {
                fallback_grace
            } else {
                timeout / 2
            };
        if !submitted && ((!hook_pending && ready) || fallback_due) {
            submitted = true;
            let payload =
                if cccc_runtime::bracketed_paste_enabled(group_id, actor_id).unwrap_or(false) {
                    b"\x1b[200~/status\x1b[201~".as_slice()
                } else {
                    b"/status".as_slice()
                };
            let _ = cccc_runtime::submit(group_id, actor_id, payload, b"\r", Duration::ZERO);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn record_codex_session(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    cwd: &Path,
    base_command: &[String],
    session_id: &str,
    captured_from: &str,
) -> std::io::Result<()> {
    let now = utc_now();
    let document = Map::from_iter([
        ("v".into(), json!(1)),
        ("kind".into(), json!("runtime_session")),
        ("group_id".into(), json!(group_id)),
        ("actor_id".into(), json!(actor_id)),
        ("runtime".into(), json!("codex")),
        ("runner".into(), json!("pty")),
        ("workspace_path".into(), json!(workspace_path(cwd))),
        (
            "command_fingerprint".into(),
            json!(command_fingerprint(base_command)),
        ),
        ("model".into(), json!(model_from_command(base_command))),
        ("provider_session_id".into(), json!(session_id)),
        ("provider_thread_id".into(), json!("")),
        (
            "resume_command_hint".into(),
            json!(format!("codex resume {session_id}")),
        ),
        ("captured_from".into(), json!(captured_from)),
        ("status".into(), json!("usable")),
        ("resume_eligible".into(), json!(true)),
        ("last_seen_at".into(), json!(now)),
        ("last_resume_attempt_at".into(), json!("")),
        ("last_resume_error".into(), json!("")),
        ("failure_count".into(), json!(0)),
        ("updated_at".into(), json!(utc_now())),
    ]);
    write(home, group_id, actor_id, &document)
}

fn read(home: &HomeLayout, group_id: &str, actor_id: &str) -> std::io::Result<Map<String, Value>> {
    let value: Value = cccc_core::fs::read_json(&path(home, group_id, actor_id)?)?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| std::io::Error::other("runtime session document is not an object"))
}

fn write(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    document: &Map<String, Value>,
) -> std::io::Result<()> {
    cccc_core::fs::write_json(&path(home, group_id, actor_id)?, document)
}

fn path(home: &HomeLayout, group_id: &str, actor_id: &str) -> std::io::Result<PathBuf> {
    let safe_actor_id = actor_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || (!actor_id.is_empty()
            && !actor_id.contains(['/', '\\'])
            && actor_id != "."
            && actor_id != "..");
    if !safe_actor_id {
        return Err(std::io::Error::other("invalid actor id"));
    }
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("runtime_sessions")
        .join(format!("{actor_id}.json")))
}

fn command_fingerprint(command: &[String]) -> String {
    let raw = serde_json::to_vec(&json!({"argv":command})).unwrap_or_default();
    format!("{:x}", Sha256::digest(raw))
}

fn app_thread_command_fingerprint(command: &[String]) -> String {
    let stable = stable_app_thread_command(command);
    let raw = serde_json::to_vec(&json!({"argv":stable})).unwrap_or_default();
    format!("{:x}", Sha256::digest(raw))
}

fn stable_app_thread_command(command: &[String]) -> Vec<String> {
    if command.len() < 2
        || Path::new(&command[0])
            .file_name()
            .and_then(|value| value.to_str())
            != Some("codex")
        || command[1] != "app-server"
    {
        return command.to_vec();
    }
    let mut stable = Vec::with_capacity(command.len());
    let mut skip_next = false;
    for item in command {
        if skip_next {
            skip_next = false;
        } else if item == "--listen" {
            stable.push(item.clone());
            skip_next = true;
        } else if item.starts_with("--listen=") {
            stable.push("--listen".into());
        } else {
            stable.push(item.clone());
        }
    }
    stable
}

fn model_from_command(command: &[String]) -> String {
    for (index, item) in command.iter().enumerate() {
        if matches!(item.as_str(), "-m" | "--model") {
            return command.get(index + 1).cloned().unwrap_or_default();
        }
        if let Some(model) = item.strip_prefix("--model=") {
            return model.trim().to_owned();
        }
    }
    String::new()
}

fn supports_codex_resume(command: &[String]) -> bool {
    command
        .first()
        .and_then(|program| Path::new(program).file_name())
        .and_then(|program| program.to_str())
        == Some(if cfg!(windows) { "codex.exe" } else { "codex" })
        && !command
            .iter()
            .skip(1)
            .any(|item| CODEX_SUBCOMMANDS.contains(&item.as_str()))
}

fn supports_claude_managed_session(command: &[String]) -> bool {
    command
        .first()
        .and_then(|program| Path::new(program).file_stem())
        .and_then(|program| program.to_str())
        == Some("claude")
        && !command.iter().skip(1).any(|item| {
            matches!(
                item.as_str(),
                "--resume" | "-r" | "--continue" | "-c" | "--session-id"
            ) || item.starts_with("--resume=")
                || item.starts_with("--session-id=")
        })
}

fn parse_codex_session_id(text: &str) -> Option<String> {
    let plain = strip_ansi(text);
    let tail = plain.split("Session:").last()?;
    tail.split_whitespace()
        .find(|candidate| valid_session_id(candidate))
        .map(str::to_owned)
}

fn strip_ansi(text: &str) -> String {
    Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]")
        .map(|pattern| pattern.replace_all(text, "").into_owned())
        .unwrap_or_else(|_| text.to_owned())
}

fn valid_session_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

fn workspace_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn resume_enabled() -> bool {
    std::env::var("CCCC_RUNTIME_RESUME")
        .ok()
        .map(|value| !NO_RESUME_VALUES.contains(&value.trim().to_ascii_lowercase().as_str()))
        .unwrap_or(true)
}

fn capture_seconds() -> f64 {
    std::env::var("CCCC_CODEX_PTY_STATUS_CAPTURE_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_CAPTURE_SECONDS)
        .max(0.0)
}

fn string(document: &Map<String, Value>, key: &str) -> String {
    document
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn nullable_string(document: &Map<String, Value>, key: &str) -> Value {
    let value = string(document, key);
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value)
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, HomeLayout, String, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("runtime session", "")
            .expect("group");
        let cwd = temp.path().join("repo");
        std::fs::create_dir(&cwd).expect("cwd");
        (temp, home, group.group_id, cwd)
    }

    fn command() -> Vec<String> {
        vec![
            "codex".into(),
            "-c".into(),
            "shell_environment_policy.inherit=all".into(),
            "--search".into(),
        ]
    }

    fn app_thread_command() -> Vec<String> {
        vec![
            "codex".into(),
            "app-server".into(),
            "--listen".into(),
            "stdio://".into(),
        ]
    }

    fn claude_headless_command() -> Vec<String> {
        vec![
            "claude".into(),
            "-p".into(),
            "--input-format".into(),
            "stream-json".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--include-partial-messages".into(),
            "--include-hook-events".into(),
            "--verbose".into(),
            "--dangerously-skip-permissions".into(),
        ]
    }

    #[test]
    fn records_python_compatible_codex_app_thread_metadata() {
        let (_temp, home, group_id, cwd) = fixture();
        let command = app_thread_command();

        record_codex_app_thread(&home, &group_id, "peer1", &cwd, &command, "thread-1", false)
            .expect("record app thread");

        let stored = read(&home, &group_id, "peer1").expect("stored metadata");
        assert_eq!(stored["runner"], "headless");
        assert_eq!(stored["provider_session_id"], "");
        assert_eq!(stored["provider_thread_id"], "thread-1");
        assert_eq!(stored["captured_from"], "app_server_thread_start");
        assert_eq!(stored["status"], "usable");
        assert_eq!(stored["resume_eligible"], true);
        assert_eq!(
            stored["command_fingerprint"],
            "e21da22b1aea2a44604536594c24efbfc4eabe61a03b833c6cb64b09f13ecad4"
        );
    }

    #[test]
    fn prepares_python_codex_app_thread_metadata_when_contract_matches() {
        let (_temp, home, group_id, cwd) = fixture();
        let command = app_thread_command();
        let document = Map::from_iter([
            ("runtime".into(), json!("codex")),
            ("status".into(), json!("usable")),
            ("resume_eligible".into(), json!(true)),
            ("workspace_path".into(), json!(workspace_path(&cwd))),
            ("model".into(), json!("gpt-test")),
            (
                "command_fingerprint".into(),
                json!(app_thread_command_fingerprint(&command)),
            ),
            ("provider_session_id".into(), json!("")),
            ("provider_thread_id".into(), json!("thread-1")),
        ]);
        write(&home, &group_id, "peer1", &document).expect("write");

        let prepared =
            prepare_codex_app_thread(&home, &group_id, "peer1", &cwd, &command, "gpt-test")
                .expect("prepare app thread");

        assert_eq!(prepared.as_deref(), Some("thread-1"));
        let stored = read(&home, &group_id, "peer1").expect("stored metadata");
        assert!(
            stored["last_resume_attempt_at"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
    }

    #[test]
    fn app_thread_fingerprint_normalizes_only_the_listen_target() {
        let stdio = app_thread_command();
        let websocket = vec![
            "codex".into(),
            "app-server".into(),
            "--listen=ws://127.0.0.1:12345".into(),
        ];
        assert_eq!(
            app_thread_command_fingerprint(&stdio),
            app_thread_command_fingerprint(&websocket)
        );
    }

    #[test]
    fn prepares_and_records_python_compatible_claude_headless_session() {
        let (_temp, home, group_id, cwd) = fixture();
        let command = claude_headless_command();
        let session_id = "42e9ef0c-3b75-43a0-9056-eef13dd1061d";
        let document = Map::from_iter([
            ("runtime".into(), json!("claude")),
            ("status".into(), json!("usable")),
            ("resume_eligible".into(), json!(true)),
            ("workspace_path".into(), json!(workspace_path(&cwd))),
            ("model".into(), json!("")),
            (
                "command_fingerprint".into(),
                json!(command_fingerprint(&command)),
            ),
            ("provider_session_id".into(), json!(session_id)),
            ("provider_thread_id".into(), json!("")),
        ]);
        write(&home, &group_id, "peer1", &document).expect("write Python metadata");

        let prepared = prepare_claude_headless_session(&home, &group_id, "peer1", &cwd, &command)
            .expect("prepare Claude session")
            .expect("managed Claude session");

        assert_eq!(prepared, (session_id.to_owned(), true));
        assert!(
            read(&home, &group_id, "peer1").expect("resume metadata")["last_resume_attempt_at"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );

        record_claude_headless_session(&home, &group_id, "peer1", &cwd, &command, session_id, true)
            .expect("record resumed session");
        let stored = read(&home, &group_id, "peer1").expect("stored metadata");
        assert_eq!(stored["runtime"], "claude");
        assert_eq!(stored["runner"], "headless");
        assert_eq!(stored["provider_session_id"], session_id);
        assert_eq!(stored["provider_thread_id"], "");
        assert_eq!(stored["captured_from"], "claude_resume_command");
        assert_eq!(stored["status"], "usable");
        assert_eq!(stored["resume_eligible"], true);
        assert_eq!(
            stored["command_fingerprint"],
            "d02815ad3f7e0e0b16b4e3fa3463a38131f137b415dbbd7af1a412b1efea2829"
        );
    }

    #[test]
    fn creates_fresh_claude_session_without_persisting_before_launch() {
        let (_temp, home, group_id, cwd) = fixture();
        let command = claude_headless_command();

        let prepared = prepare_claude_headless_session(&home, &group_id, "peer1", &cwd, &command)
            .expect("prepare Claude session")
            .expect("managed Claude session");

        assert!(valid_session_id(&prepared.0));
        assert!(!prepared.1);
        assert!(read(&home, &group_id, "peer1").is_err());
    }

    #[test]
    fn leaves_user_owned_and_indirect_claude_commands_unmanaged() {
        let (_temp, home, group_id, cwd) = fixture();
        for command in [
            vec!["claude".into(), "--resume".into(), "user-session".into()],
            vec!["claude".into(), "--session-id=user-session".into()],
            vec!["sh".into(), "-c".into(), "exec claude".into()],
        ] {
            assert!(
                prepare_claude_headless_session(&home, &group_id, "peer1", &cwd, &command,)
                    .expect("prepare Claude session")
                    .is_none()
            );
        }
        assert!(read(&home, &group_id, "peer1").is_err());
    }

    #[test]
    fn resumes_python_pty_metadata_when_launch_contract_matches() {
        let (_temp, home, group_id, cwd) = fixture();
        let session_id = "019eece8-8c6d-7811-a700-26593825ae2d";
        let base = command();
        let document = Map::from_iter([
            ("runtime".into(), json!("codex")),
            ("status".into(), json!("usable")),
            ("resume_eligible".into(), json!(true)),
            ("workspace_path".into(), json!(workspace_path(&cwd))),
            ("model".into(), json!("")),
            (
                "command_fingerprint".into(),
                json!(command_fingerprint(&base)),
            ),
            ("provider_session_id".into(), json!(session_id)),
            ("provider_thread_id".into(), json!("")),
        ]);
        write(&home, &group_id, "peer1", &document).expect("write");

        let prepared = prepare_codex_command(&home, &group_id, "peer1", &cwd, &base);

        assert_eq!(prepared.resumed_session_id.as_deref(), Some(session_id));
        assert_eq!(
            prepared.command,
            [base, vec!["resume".into(), session_id.into()]].concat()
        );
    }

    #[test]
    fn starts_fresh_for_python_app_server_thread_metadata() {
        let (_temp, home, group_id, cwd) = fixture();
        let thread_id = "019f4055-4231-77b0-b559-8de345f57f5e";
        let base = command();
        let document = Map::from_iter([
            ("runtime".into(), json!("codex")),
            ("status".into(), json!("usable")),
            ("resume_eligible".into(), json!(true)),
            ("workspace_path".into(), json!(workspace_path(&cwd))),
            ("model".into(), json!("")),
            (
                "command_fingerprint".into(),
                json!("app-server-fingerprint"),
            ),
            ("provider_session_id".into(), json!("")),
            ("provider_thread_id".into(), json!(thread_id)),
        ]);
        write(&home, &group_id, "foreman", &document).expect("write");

        let prepared = prepare_codex_command(&home, &group_id, "foreman", &cwd, &base);

        assert!(prepared.resumed_session_id.is_none());
        assert_eq!(prepared.command, base);
        let stored = read(&home, &group_id, "foreman").expect("stored metadata");
        assert_eq!(stored["status"], "resume_failed");
        assert_eq!(stored["resume_eligible"], false);
        assert!(
            stored["last_resume_error"]
                .as_str()
                .is_some_and(|error| error.contains("app-server thread"))
        );
    }

    #[test]
    fn rejects_metadata_from_another_workspace() {
        let (_temp, home, group_id, cwd) = fixture();
        let base = command();
        let document = Map::from_iter([
            ("runtime".into(), json!("codex")),
            ("status".into(), json!("usable")),
            ("resume_eligible".into(), json!(true)),
            ("workspace_path".into(), json!("/different/workspace")),
            ("model".into(), json!("")),
            (
                "command_fingerprint".into(),
                json!(command_fingerprint(&base)),
            ),
            (
                "provider_session_id".into(),
                json!("019eece8-8c6d-7811-a700-26593825ae2d"),
            ),
        ]);
        write(&home, &group_id, "peer1", &document).expect("write");

        let prepared = prepare_codex_command(&home, &group_id, "peer1", &cwd, &base);

        assert!(prepared.resumed_session_id.is_none());
        assert_eq!(prepared.command, base);
    }

    #[test]
    fn parses_codex_status_with_terminal_escapes() {
        let text = "\x1b[2mSession:\x1b[0m  019eece8-8c6d-7811-a700-26593825ae2d\r\n";
        assert_eq!(
            parse_codex_session_id(text).as_deref(),
            Some("019eece8-8c6d-7811-a700-26593825ae2d")
        );
    }

    #[test]
    fn hook_capture_replaces_failed_metadata_with_a_resumable_session() {
        let (_temp, home, group_id, cwd) = fixture();
        let base = command();
        let old_session_id = "019eece8-8c6d-7811-a700-26593825ae2d";
        record_codex_session(
            &home,
            &group_id,
            "peer1",
            &cwd,
            &base,
            old_session_id,
            CODEX_STATUS_CAPTURE_SOURCE,
        )
        .expect("record old session");
        mark_resume_failed(
            &home,
            &group_id,
            "peer1",
            "provider resume process exited early",
        )
        .expect("mark old session failed");
        let current_session_id = "019fea2e-ea50-7b43-9fc7-efd55e70a585";

        record_codex_session(
            &home,
            &group_id,
            "peer1",
            &cwd,
            &base,
            current_session_id,
            CODEX_HOOK_CAPTURE_SOURCE,
        )
        .expect("record current hook session");

        let stored = read(&home, &group_id, "peer1").expect("stored metadata");
        assert_eq!(stored["provider_session_id"], current_session_id);
        assert_eq!(stored["captured_from"], CODEX_HOOK_CAPTURE_SOURCE);
        assert_eq!(stored["status"], "usable");
        assert_eq!(stored["resume_eligible"], true);
        assert_eq!(stored["failure_count"], 0);
        assert_eq!(stored["last_resume_error"], "");

        let prepared = prepare_codex_command(&home, &group_id, "peer1", &cwd, &base);
        assert_eq!(
            prepared.resumed_session_id.as_deref(),
            Some(current_session_id)
        );
    }

    #[test]
    fn detects_codex_no_saved_session_resume_failure() {
        assert_eq!(
            resume_failure_marker(
                "ERROR: No saved session found with ID 019eece8-8c6d-7811-a700-26593825ae2d"
            ),
            Some("no saved session found")
        );
    }

    #[test]
    fn grok_first_start_creates_managed_session_and_restart_resumes_it() {
        let (_temp, home, group_id, cwd) = fixture();
        let base = vec!["grok".into(), "--always-approve".into()];

        let first = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
        assert_eq!(first.command[0], "grok");
        assert_eq!(first.command[1], "--session-id");
        let session_id = first.command[2].clone();
        assert!(valid_session_id(&session_id));
        assert_eq!(first.command[3], "--always-approve");
        let stored = read(&home, &group_id, "peer1").expect("managed session");
        assert_eq!(stored["runtime"], "grok");
        assert_eq!(stored["provider_session_id"], session_id);
        assert_eq!(stored["captured_from"], "grok_generated_session_id");

        let resumed = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
        assert_eq!(
            resumed.command,
            vec![
                "grok".to_owned(),
                "--resume".to_owned(),
                session_id.clone(),
                "--always-approve".to_owned()
            ]
        );
        assert_eq!(
            resumed.resumed_session_id.as_deref(),
            Some(session_id.as_str())
        );
    }

    #[test]
    fn grok_does_not_resume_session_recorded_for_another_model() {
        let (_temp, home, group_id, cwd) = fixture();
        let base = vec!["grok".into(), "--model".into(), "grok-fast".into()];
        let first = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
        let old_session = first.command[2].clone();
        let mut stored = read(&home, &group_id, "peer1").expect("managed session");
        stored.insert("model".into(), json!("grok-careful"));
        write(&home, &group_id, "peer1", &stored).expect("write mismatched model");

        let next = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
        assert_eq!(next.command[1], "--session-id");
        assert_ne!(next.command[2], old_session);
        assert_eq!(
            read(&home, &group_id, "peer1").expect("replacement")["model"],
            "grok-fast"
        );
    }

    #[test]
    fn grok_explicit_session_controls_and_subcommands_are_not_rewritten() {
        let (_temp, home, group_id, cwd) = fixture();
        for base in [
            vec![
                "grok".into(),
                "--session-id".into(),
                uuid::Uuid::new_v4().to_string(),
            ],
            vec!["grok".into(), "sessions".into(), "list".into()],
            vec!["grok".into(), "-rprevious".into()],
        ] {
            let prepared = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
            assert_eq!(prepared.command, base);
            assert!(prepared.resumed_session_id.is_none());
        }
        assert!(read(&home, &group_id, "peer1").is_err());
    }

    #[test]
    fn grok_fresh_fallback_replaces_failed_session() {
        let (_temp, home, group_id, cwd) = fixture();
        let base = vec!["grok".into(), "--always-approve".into()];
        let first = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
        let old_session = first.command[2].clone();
        mark_resume_failed(&home, &group_id, "peer1", "session not found")
            .expect("mark resume failed");

        let fresh = prepare_fresh_grok_command(&home, &group_id, "peer1", &cwd, &base);
        assert_eq!(fresh.command[1], "--session-id");
        assert_ne!(fresh.command[2], old_session);
        let stored = read(&home, &group_id, "peer1").expect("replacement session");
        assert_eq!(stored["status"], "usable");
        assert_eq!(stored["provider_session_id"], fresh.command[2]);
    }

    #[test]
    fn grok_new_session_after_metadata_removal_gets_a_new_uuid() {
        let (_temp, home, group_id, cwd) = fixture();
        let base = vec!["grok".into(), "--always-approve".into()];
        let first = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
        remove(&home, &group_id, "peer1").expect("remove session");
        let next = prepare_grok_command(&home, &group_id, "peer1", &cwd, &base);
        assert_eq!(next.command[1], "--session-id");
        assert_ne!(first.command[2], next.command[2]);
    }
}
