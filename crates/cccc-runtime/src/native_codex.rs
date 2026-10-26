//! Read-only native capability check through Codex's documented stdio proxy.
//! The proxy connects to an existing server and never starts a model or thread.
use crate::{OwnedProcessTree, command_output::OwnedChild};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, de::DeserializeOwned, de::IgnoredAny};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub struct NativeCodexSessionInfo {
    pub can_receive: bool,
    /// Optional creation origin, never receive or identity authority.
    pub origin: Option<String>,
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
    let check = async {
        // The public proxy relays raw WebSocket bytes, not NDJSON. Let the
        // existing WebSocket library perform framing and the local handshake.
        let (mut socket, _) =
            tokio_tungstenite::client_async("ws://localhost", tokio::io::join(output, input))
                .await
                .map_err(io::Error::other)?;
        let initialized: Option<IgnoredAny> = request(
            &mut socket,
            1,
            "initialize",
            json!({"clientInfo":{"name":"cccc-native-session-check","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}),
        )
        .await?;
        if initialized.is_none() {
            return Err(io::Error::other("Native capability initialization failed"));
        }
        socket
            .send(Message::Text("{\"method\":\"initialized\"}".into()))
            .await
            .map_err(io::Error::other)?;

        let mut id = 1;
        let mut params = json!({});
        let mut cursors = HashSet::new();
        loop {
            id += 1;
            let page: Option<Page<String>> =
                request(&mut socket, id, "thread/loaded/list", params).await?;
            let Some(page) = page else {
                return Err(io::Error::other("Native loaded-thread lookup failed"));
            };
            let cursor = page.cursor()?;
            if page.data.iter().any(|id| id == thread_id) {
                break;
            }
            let Some(cursor) = cursor else {
                return Ok((false, socket, id));
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
            &mut socket,
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
        Ok((can_receive, socket, id + 1))
    };
    let (can_receive, mut socket, id) = tokio::time::timeout(Duration::from_secs(2), check)
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
                &mut socket,
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
