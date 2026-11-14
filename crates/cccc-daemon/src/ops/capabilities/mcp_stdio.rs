use super::mcp_client::{self, Failure, Request, redact_diagnostic};
use crate::ops::local_headless::block_on_managed;
use rmcp::ServiceExt;
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[cfg(test)]
mod tests;

const STDERR_BYTES: usize = 8 * 1024;

#[cfg(test)]
pub(super) const STDIO_FIXTURE: &str = include_str!("mcp_stdio/fixture.py");

#[derive(Default)]
struct StderrTail {
    bytes: VecDeque<u8>,
    truncated: bool,
}

impl StderrTail {
    fn append(&mut self, bytes: &[u8]) {
        self.bytes.extend(bytes);
        while self.bytes.len() > STDERR_BYTES {
            self.bytes.pop_front();
            self.truncated = true;
        }
    }

    fn text(&self) -> String {
        let bytes = self.bytes.iter().copied().collect::<Vec<_>>();
        let text = String::from_utf8_lossy(&bytes);
        // Do not reveal the tail fragment of a value cut by the byte budget.
        if self.truncated {
            text.split_once('\n')
                .map_or_else(String::new, |(_, rest)| rest.to_owned())
        } else {
            text.into_owned()
        }
    }
}

pub(super) fn list_tools(
    command: &[String],
    env: &Map<String, Value>,
    timeout: Duration,
) -> Result<Vec<Value>, Failure> {
    run(command, env, Request::List, timeout).and_then(|value| {
        serde_json::from_value(value)
            .map_err(|error| Failure::transport("tools/list result", error))
    })
}

pub(super) fn call_tool(
    command: &[String],
    env: &Map<String, Value>,
    name: &str,
    arguments: Map<String, Value>,
    timeout: Duration,
) -> Result<Value, Failure> {
    run(
        command,
        env,
        Request::Call {
            name: name.into(),
            arguments,
        },
        timeout,
    )
}

fn run(
    command: &[String],
    env: &Map<String, Value>,
    request: Request,
    timeout: Duration,
) -> Result<Value, Failure> {
    let secrets = diagnostic_secrets(command, env);
    block_on_managed(exchange(command, env, request, timeout)).map_err(|mut error| {
        error.message = redact_diagnostic(&error.message, &secrets);
        error
    })
}

async fn exchange(
    command: &[String],
    env: &Map<String, Value>,
    request: Request,
    timeout: Duration,
) -> Result<Value, Failure> {
    if command.first().is_none_or(|value| value.is_empty()) {
        return Err(Failure::transport("spawn", "command is empty"));
    }
    let child_env = env
        .iter()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
        .collect::<std::collections::BTreeMap<_, _>>();
    // A bare name finds only `.exe` on Windows, while the availability check also accepts
    // shims such as `npx.cmd`. Resolve against the PATH the child gets, as actor launch does.
    let resolved = cccc_runtime::resolve_command_executable(command, &child_env);
    let mut process = tokio::process::Command::new(&resolved[0]);
    process
        .args(&command[1..])
        .envs(&child_env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = process
        .spawn()
        .map_err(|error| Failure::transport("spawn", error))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stdin = child.stdin.take().expect("piped stdin");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let tail = Arc::new(Mutex::new(StderrTail::default()));
    let reader_tail = tail.clone();
    let mut reader = tokio::spawn(async move {
        let mut buffer = [0; 2048];
        while let Ok(count) = stderr.read(&mut buffer).await {
            if count == 0 {
                break;
            }
            reader_tail
                .lock()
                .expect("stderr tail")
                .append(&buffer[..count]);
        }
    });

    let deadline = tokio::time::Instant::now() + timeout;
    // The SDK owns framing and handshake order. Retain Child so initialization
    // timeout also has an explicit kill/wait path, rather than an asynchronous Drop.
    let mut result = match tokio::time::timeout_at(deadline, ().serve((stdout, stdin))).await {
        Ok(Ok(service)) => {
            let result = mcp_client::request(&service, request, deadline).await;
            if !matches!(
                tokio::time::timeout(Duration::from_secs(1), service.cancel()).await,
                Ok(Ok(_))
            ) {
                tracing::warn!("MCP stdio connection cleanup did not complete");
            }
            result
        }
        Ok(Err(error)) => Err(Failure::initialize(error)),
        Err(error) => Err(Failure::transport("initialize timeout", error)),
    };

    let status = match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
        Ok(status) => status.ok(),
        Err(_) => {
            let _ = child.start_kill();
            match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                Ok(status) => status.ok(),
                Err(_) => {
                    tracing::warn!("MCP stdio process cleanup did not complete");
                    None
                }
            }
        }
    };
    // Descendants may retain the pipe after the direct child exits. Diagnostics
    // must not turn a bounded operation into an unbounded wait for stderr EOF.
    if tokio::time::timeout(Duration::from_secs(1), &mut reader)
        .await
        .is_err()
    {
        reader.abort();
    }
    if let Err(error) = &mut result {
        if let Some(status) = status.filter(|status| !status.success()) {
            error
                .message
                .push_str(&format!("; process exited with {status}"));
        }
        let diagnostic = tail.lock().expect("stderr tail").text();
        if !diagnostic.trim().is_empty() {
            error.message.push_str("; stderr: ");
            error.message.push_str(diagnostic.trim());
        }
    }
    // A known result remains valid even when process cleanup reports a problem.
    result
}

fn diagnostic_secrets(command: &[String], env: &Map<String, Value>) -> Vec<String> {
    let mut secrets = env
        .values()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    secrets.extend(std::env::vars_os().filter_map(|(key, value)| {
        let (key, value) = (key.to_str()?, value.to_str()?);
        sensitive_key(key).then(|| value.to_owned())
    }));
    let mut secret_argument = false;
    for argument in command.iter().skip(1) {
        if let Ok(url) = url::Url::parse(argument) {
            if matches!(url.scheme(), "http" | "https") {
                secrets.push(url.username().to_owned());
                secrets.extend(url.password().map(str::to_owned));
                secrets.extend(url.query_pairs().map(|(_, value)| value.into_owned()));
            }
        }
        if let Some((_, value)) = argument.split_once("Bearer ") {
            secrets.push(value.trim().to_owned());
        }
        if secret_argument {
            secrets.push(argument.clone());
        }
        secret_argument = argument.starts_with('-') && sensitive_key(argument);
        if let Some((key, value)) = argument.split_once('=') {
            if sensitive_key(key) {
                secrets.push(value.to_owned());
                secret_argument = false;
            }
        }
    }
    let lines = secrets
        .iter()
        .flat_map(|value| value.lines().map(str::to_owned))
        .collect::<Vec<_>>();
    secrets.extend(lines);
    secrets.retain(|value| !value.is_empty());
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets.dedup();
    secrets
}

fn sensitive_key(key: &str) -> bool {
    let mut normalized = String::with_capacity(key.len());
    let mut previous_lower_or_digit = false;
    for character in key.chars() {
        if character.is_ascii_uppercase() && previous_lower_or_digit {
            normalized.push('_');
        }
        previous_lower_or_digit = character.is_ascii_lowercase() || character.is_ascii_digit();
        normalized.push(character.to_ascii_lowercase());
    }
    let key = normalized;
    key.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|part| {
            matches!(
                part,
                "token"
                    | "tokens"
                    | "secret"
                    | "secrets"
                    | "password"
                    | "passwords"
                    | "passwd"
                    | "apikey"
                    | "credential"
                    | "credentials"
                    | "authorization"
                    | "bearer"
                    | "pat"
            )
        })
        || key
            .rsplit(|character: char| !character.is_ascii_alphanumeric())
            .next()
            == Some("key")
}
