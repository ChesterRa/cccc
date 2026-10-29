//! Native capability checks and exact-turn input through Codex's stdio proxy.
//! Connecting and inspecting never start, resume or configure a native thread.
use crate::{OwnedProcessTree, command_output::OwnedChild};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, de::DeserializeOwned, de::IgnoredAny};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub struct NativeCodexSessionInfo {
    pub can_receive: bool,
    /// Optional creation origin, never receive or identity authority.
    pub origin: Option<String>,
}

/// Runtime admission, not proof that a model processed the input. A native hook
/// may still reject an accepted input. Unknown admission must never be replayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeCodexSteerOutcome {
    Accepted,
    NotSubmitted,
    Ambiguous,
}

/// Try the CLI's normal busy-turn input behavior. Only proven non-admission
/// permits the caller to use its ordinary queue transport. No native settings,
/// queue contents, approvals or user answers are changed by this operation.
pub async fn native_codex_try_steer(
    program: &Path,
    codex_home: &Path,
    cwd: &Path,
    thread_id: &str,
    text: &str,
    cancelled: &AtomicBool,
) -> NativeCodexSteerOutcome {
    if cancelled.load(Ordering::Acquire) {
        return NativeCodexSteerOutcome::NotSubmitted;
    }
    let prepared = tokio::time::timeout(Duration::from_secs(2), async {
        let mut proxy = NativeProxy::connect(program, codex_home, cwd).await?;
        let turn = prepare_steer(&mut proxy.socket, thread_id).await?;
        io::Result::Ok((proxy, turn))
    })
    .await;
    let Ok(Ok((mut proxy, Some(turn)))) = prepared else {
        // Everything above is read-only, including an inconclusive lookup.
        return NativeCodexSteerOutcome::NotSubmitted;
    };
    submit_steer(&mut proxy.socket, thread_id, &turn, text, cancelled).await
}

async fn prepare_steer<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    thread_id: &str,
) -> io::Result<Option<String>> {
    let metadata: Option<ThreadAdmission> = request(
        socket,
        2,
        "thread/read",
        json!({"threadId":thread_id,"includeTurns":false}),
    )
    .await?;
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let thread = metadata.thread;
    if thread.can_accept_direct_input != Some(true)
        || thread.status.kind != "active"
        || !thread
            .status
            .active_flags
            .is_some_and(|flags| flags.is_empty())
    {
        return Ok(None);
    }
    // Minimal turn metadata supplies the correlation ID without fetching any
    // transcript. A stale ID is rejected by the native server, never guessed.
    let page: Option<Page<TurnMetadata>> = request(
        socket,
        3,
        "thread/turns/list",
        json!({"threadId":thread_id,"limit":1,"sortDirection":"desc","itemsView":"notLoaded"}),
    )
    .await?;
    let Some(page) = page else { return Ok(None) };
    page.cursor()?;
    Ok(match page.data.as_slice() {
        [turn] if turn.status == "inProgress" && !turn.id.is_empty() => Some(turn.id.clone()),
        _ => None,
    })
}

async fn submit_steer<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut WebSocketStream<S>,
    thread_id: &str,
    turn_id: &str,
    text: &str,
    cancelled: &AtomicBool,
) -> NativeCodexSteerOutcome {
    use NativeCodexSteerOutcome::{Accepted, Ambiguous, NotSubmitted};
    if cancelled.load(Ordering::Acquire) {
        return NotSubmitted;
    }
    // Match the ordinary queue handoff budget. An acknowledged steer is cheap,
    // but false timeouts create permanent ambiguous receipts under server load.
    let submit = tokio::time::timeout(
        Duration::from_secs(10),
        request::<_, SteerReceipt>(
            socket,
            4,
            "turn/steer",
            json!({"threadId":thread_id,"expectedTurnId":turn_id,"input":[{"type":"text","text":text}]}),
        ),
    );
    tokio::pin!(submit);
    loop {
        tokio::select! {
            biased;
            result = &mut submit => return match result {
                Ok(Ok(Some(receipt))) if receipt.turn_id == turn_id => Accepted,
                // JSON-RPC invalid request / unknown method is a definitive
                // rejection. This includes an ended, changed or compact turn.
                Ok(Ok(None)) => NotSubmitted,
                _ => Ambiguous,
            },
            () = tokio::time::sleep(Duration::from_millis(50)) => {
                if cancelled.load(Ordering::Acquire) {
                    return Ambiguous;
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct ThreadAdmission {
    thread: AdmissionMetadata,
}

#[derive(Deserialize)]
struct AdmissionMetadata {
    #[serde(rename = "canAcceptDirectInput")]
    can_accept_direct_input: Option<bool>,
    status: AdmissionStatus,
}

#[derive(Deserialize)]
struct AdmissionStatus {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "activeFlags")]
    active_flags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct TurnMetadata {
    id: String,
    status: String,
}

#[derive(Deserialize)]
struct SteerReceipt {
    #[serde(rename = "turnId")]
    turn_id: String,
}

type ProxySocket =
    WebSocketStream<tokio::io::Join<tokio::process::ChildStdout, tokio::process::ChildStdin>>;

struct NativeProxy {
    socket: ProxySocket,
    _owned: OwnedChild,
}

impl NativeProxy {
    async fn connect(program: &Path, codex_home: &Path, cwd: &Path) -> io::Result<Self> {
        let mut command = Command::new(program);
        command
            .args(["app-server", "proxy"])
            .env("CODEX_HOME", codex_home)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (child, tree) = OwnedProcessTree::spawn(&mut command)?;
        let mut owned = OwnedChild { child, tree };
        let input = tokio::process::ChildStdin::from_std(
            owned
                .child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("Missing native proxy input"))?,
        )?;
        let output = tokio::process::ChildStdout::from_std(
            owned
                .child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("Missing native proxy output"))?,
        )?;
        // The public proxy relays WebSocket bytes, not NDJSON.
        let (mut socket, _) =
            tokio_tungstenite::client_async("ws://localhost", tokio::io::join(output, input))
                .await
                .map_err(io::Error::other)?;
        let initialized: Option<IgnoredAny> = request(
            &mut socket,
            1,
            "initialize",
            json!({"clientInfo":{"name":"cccc-native-session","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),
        )
        .await?;
        if initialized.is_none() {
            return Err(io::Error::other("Native capability initialization failed"));
        }
        socket
            .send(Message::Text("{\"method\":\"initialized\"}".into()))
            .await
            .map_err(io::Error::other)?;
        Ok(Self {
            socket,
            _owned: owned,
        })
    }
}

/// A readiness snapshot, not a promise of queue admission or model consumption.
/// `Ok(false)` is negative capability evidence; I/O, timeout, internal errors and
/// malformed replies leave capability unknown and must not retire native identity.
pub async fn native_codex_can_receive(
    program: &Path,
    codex_home: &Path,
    cwd: &Path,
    thread_id: &str,
) -> io::Result<bool> {
    Ok(inspect(program, codex_home, cwd, thread_id, false)
        .await?
        .can_receive)
}

/// Capture an optional display hint during lifecycle registration only.
pub async fn native_codex_session_info(
    program: &Path,
    codex_home: &Path,
    cwd: &Path,
    thread_id: &str,
) -> io::Result<NativeCodexSessionInfo> {
    inspect(program, codex_home, cwd, thread_id, true).await
}

async fn inspect(
    program: &Path,
    codex_home: &Path,
    cwd: &Path,
    thread_id: &str,
    include_origin: bool,
) -> io::Result<NativeCodexSessionInfo> {
    let check = async {
        let mut proxy = NativeProxy::connect(program, codex_home, cwd).await?;

        let mut id = 1;
        let mut params = json!({});
        let mut cursors = HashSet::new();
        loop {
            id += 1;
            let page: Option<Page<String>> =
                request(&mut proxy.socket, id, "thread/loaded/list", params).await?;
            let Some(page) = page else {
                return Err(io::Error::other("Native loaded-thread lookup failed"));
            };
            let cursor = page.cursor()?;
            if page.data.iter().any(|id| id == thread_id) {
                break;
            }
            let Some(cursor) = cursor else {
                return Ok((false, proxy, id));
            };
            if !cursors.insert(cursor.to_owned()) {
                return Err(invalid_response());
            }
            params = json!({"cursor":cursor});
        }

        // Queue qualification is independent of loaded-thread membership.
        // Public queue/list can include native UserInput even at limit=1. Skip
        // every item during deserialization: never retain or expose its payload.
        let queue: Option<Page<IgnoredAny>> = request(
            &mut proxy.socket,
            id + 1,
            "thread/queue/list",
            json!({"threadId":thread_id,"limit":1}),
        )
        .await?;
        let can_receive = match queue {
            Some(page) => {
                page.cursor()?;
                true
            }
            None => false,
        };
        Ok((can_receive, proxy, id + 1))
    };
    let (can_receive, mut proxy, id) = tokio::time::timeout(Duration::from_secs(2), check)
        .await
        .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "Native queue capability check timed out",
        )
    })??;
    // A missing/changed/slow optional API must not discard qualification.
    // Deserialize only source; includeTurns=false never requests the transcript.
    let origin = if include_origin {
        tokio::time::timeout(
            Duration::from_millis(250),
            request::<_, ThreadOrigin>(
                &mut proxy.socket,
                id + 1,
                "thread/read",
                json!({"threadId":thread_id,"includeTurns":false}),
            ),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .and_then(|reply| match reply.thread.source.as_str() {
            Some("cli") => Some("cli".into()),
            Some("vscode") => Some("vscode".into()),
            _ => None,
        })
    } else {
        None
    };
    Ok(NativeCodexSessionInfo {
        can_receive,
        origin,
    })
}

#[derive(Deserialize)]
struct ThreadOrigin {
    thread: OriginMetadata,
}

#[derive(Deserialize)]
struct OriginMetadata {
    #[serde(default)]
    source: Value,
}

#[derive(Deserialize)]
struct Page<T> {
    data: Vec<T>,
    #[serde(rename = "nextCursor")]
    next_cursor: Value,
}

impl<T> Page<T> {
    fn cursor(&self) -> io::Result<Option<&str>> {
        match &self.next_cursor {
            Value::Null => Ok(None),
            Value::String(cursor) if !cursor.is_empty() => Ok(Some(cursor)),
            _ => Err(invalid_response()),
        }
    }
}

#[derive(Deserialize)]
struct Header {
    id: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
}

#[derive(Deserialize)]
struct Response<T> {
    result: T,
}

async fn request<S: AsyncRead + AsyncWrite + Unpin, T: DeserializeOwned>(
    socket: &mut WebSocketStream<S>,
    id: u64,
    method: &str,
    params: Value,
) -> io::Result<Option<T>> {
    socket
        .send(Message::Text(
            serde_json::to_string(&json!({"id":id,"method":method,"params":params}))?.into(),
        ))
        .await
        .map_err(io::Error::other)?;
    loop {
        let message = socket
            .next()
            .await
            .transpose()
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "Native proxy closed"))?;
        let Message::Text(line) = message else {
            continue;
        };
        let header: Header = serde_json::from_str(&line).map_err(|_| invalid_response())?;
        if header.id != Some(json!(id)) {
            continue;
        }
        if let Some(error) = header.error {
            // Unsupported methods and rejected qualification are definitive;
            // an unavailable/internal lookup is not evidence of disconnection.
            return match error.code {
                -32600 | -32601 => Ok(None),
                _ => Err(io::Error::other("Native capability query failed")),
            };
        }
        let response: Response<T> = serde_json::from_str(&line).map_err(|_| invalid_response())?;
        return Ok(Some(response.result));
    }
}

fn invalid_response() -> io::Error {
    // Deserialization errors can quote provider data. Do not forward them.
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid native capability response",
    )
}

#[cfg(test)]
mod tests;
