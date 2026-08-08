mod buffering;

use cccc_client::DaemonClient;
use cccc_contracts::ActorRuntime;
use cccc_core::{GroupStore, HomeLayout};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};

use buffering::{
    BoundedLine, EVENT_QUEUE_CAPACITY, MAX_EVENT_BYTES, OutputBuffer, content_text,
    read_bounded_line,
};

const DEFAULT_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;
const MAX_YIELD_TIME_MS: u64 = 60_000;
const MAX_OUTPUT_TOKENS: usize = 50_000;
const MAX_SOURCE_CHARS: usize = 500_000;
const MAX_CELLS: usize = 16;
const CELL_TTL: Duration = Duration::from_secs(30 * 60);
const PROCESS_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const NODE_RUNTIME: &str = include_str!("../../../src/cccc/resources/code_mode_runtime.js");
const METADATA: &str = include_str!("../../../src/cccc/resources/code_mode_metadata.json");

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Owner {
    home: PathBuf,
    group_id: String,
    actor_id: String,
}

struct CodeCell {
    owner: Owner,
    process: Mutex<Child>,
    io: Mutex<CellIo>,
    started_at: Instant,
    last_used_at: StdMutex<Instant>,
}

struct CellIo {
    stdin: ChildStdin,
    events: mpsc::Receiver<Value>,
}

type SharedCell = Arc<CodeCell>;
type CellMap = HashMap<String, SharedCell>;

pub async fn start(
    home: &HomeLayout,
    client: &DaemonClient,
    root: &Path,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    ensure_enabled()?;
    let owner = resolve_owner(home, args)?;
    let raw_source = args
        .get("source")
        .or_else(|| args.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (source, pragma) = parse_exec_pragma(raw_source)?;
    if source.trim().is_empty() {
        return Err("missing_source: source is required".into());
    }
    reject_unsupported_source(source)?;
    let yield_time_ms = integer_arg(
        args.get("yield_time_ms")
            .or_else(|| pragma.get("yield-time_ms")),
        DEFAULT_YIELD_TIME_MS,
        0,
        MAX_YIELD_TIME_MS,
    );
    let max_output_tokens = integer_arg(
        args.get("max_output_tokens")
            .or_else(|| pragma.get("max_output_tokens")),
        DEFAULT_MAX_OUTPUT_TOKENS as u64,
        1,
        MAX_OUTPUT_TOKENS as u64,
    ) as usize;

    prune_cells().await;
    let nested_tools = nested_tools(home, client, &owner).await;
    let (cell_id, cell) = spawn_cell(root, owner, source, nested_tools, yield_time_ms).await?;
    cells().lock().await.insert(cell_id.clone(), cell.clone());
    schedule_expiration(cell_id.clone());
    drain(
        home,
        client,
        &cell_id,
        cell,
        yield_time_ms,
        max_output_tokens,
    )
    .await
}

pub async fn wait(
    home: &HomeLayout,
    client: &DaemonClient,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    ensure_enabled()?;
    let owner = resolve_owner(home, args)?;
    let cell_id = args
        .get("cell_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("missing_cell_id: cell_id is required")?
        .to_owned();
    let cell = {
        let active = cells().lock().await;
        let Some(cell) = active.get(&cell_id).cloned() else {
            return Ok(missing_response(&cell_id));
        };
        if cell.owner != owner {
            return Ok(missing_response(&cell_id));
        }
        *cell
            .last_used_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
        cell
    };
    let max_output_tokens = integer_arg(
        args.get("max_tokens")
            .or_else(|| args.get("max_output_tokens")),
        DEFAULT_MAX_OUTPUT_TOKENS as u64,
        1,
        MAX_OUTPUT_TOKENS as u64,
    ) as usize;
    if args
        .get("terminate")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        cells().lock().await.remove(&cell_id);
        terminate_cell(&cell).await;
        return Ok(format_response(
            "terminated",
            &cell_id,
            OutputBuffer::new(max_output_tokens),
            cell.started_at,
            "",
        ));
    }
    let yield_time_ms = integer_arg(
        args.get("yield_time_ms"),
        DEFAULT_YIELD_TIME_MS,
        0,
        MAX_YIELD_TIME_MS,
    );
    drain(
        home,
        client,
        &cell_id,
        cell,
        yield_time_ms,
        max_output_tokens,
    )
    .await
}

fn ensure_enabled() -> Result<(), String> {
    let disabled = std::env::var("CCCC_WEB_MODEL_CODE_MODE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "0" | "false" | "no" | "off" | "disabled"));
    if disabled {
        Err("code_mode_disabled: cccc_code_exec/code_wait is disabled by CCCC_WEB_MODEL_CODE_MODE=0".into())
    } else {
        Ok(())
    }
}

pub(crate) fn enabled() -> bool {
    ensure_enabled().is_ok()
}

fn resolve_owner(home: &HomeLayout, args: &Map<String, Value>) -> Result<Owner, String> {
    let group_id = args
        .get("group_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(
            "missing_runtime_context: web-model local-power tools require group_id and actor_id",
        )?;
    let actor_id = args
        .get("by")
        .or_else(|| args.get("actor_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(
            "missing_runtime_context: web-model local-power tools require group_id and actor_id",
        )?;
    let store = GroupStore::new(home.clone()).map_err(|error| error.to_string())?;
    let group = store
        .load(group_id)
        .map_err(|_| format!("group_not_found: group not found: {group_id}"))?;
    let actor = group
        .actors
        .iter()
        .find(|actor| actor.id == actor_id)
        .ok_or_else(|| format!("actor_not_found: actor not found: {actor_id}"))?;
    if actor.runtime != ActorRuntime::WebModel {
        return Err(
            "invalid_actor_runtime: local-power MCP tools are only available to web_model actors"
                .into(),
        );
    }
    Ok(Owner {
        home: home.root().to_path_buf(),
        group_id: group_id.to_owned(),
        actor_id: actor_id.to_owned(),
    })
}

fn parse_exec_pragma(source: &str) -> Result<(&str, Map<String, Value>), String> {
    let Some(rest) = source.strip_prefix("// @exec:") else {
        return Ok((source, Map::new()));
    };
    let Some((raw, body)) = rest.split_once('\n') else {
        return Ok((source, Map::new()));
    };
    if raw.trim().is_empty() {
        return Ok((body, Map::new()));
    }
    let parsed: Value = serde_json::from_str(raw.trim())
        .map_err(|error| format!("invalid_pragma: failed to parse @exec pragma: {error}"))?;
    let object = parsed
        .as_object()
        .ok_or("invalid_pragma: @exec pragma must be a JSON object")?;
    let extra = object
        .keys()
        .filter(|key| !matches!(key.as_str(), "yield-time_ms" | "max_output_tokens"))
        .cloned()
        .collect::<Vec<_>>();
    if !extra.is_empty() {
        return Err(format!(
            "invalid_pragma: unsupported @exec pragma keys: {}",
            extra.join(", ")
        ));
    }
    Ok((body, object.clone()))
}

fn reject_unsupported_source(source: &str) -> Result<(), String> {
    if source.chars().count() > MAX_SOURCE_CHARS {
        return Err(format!(
            "source_too_large: source exceeds {MAX_SOURCE_CHARS} characters"
        ));
    }
    static REQUIRE: OnceLock<Regex> = OnceLock::new();
    static IMPORT_CALL: OnceLock<Regex> = OnceLock::new();
    static IMPORT_STMT: OnceLock<Regex> = OnceLock::new();
    let require = REQUIRE.get_or_init(|| {
        Regex::new(r"(^|[^\w$])require\s*\(").expect("static require regex must compile")
    });
    let import_call = IMPORT_CALL.get_or_init(|| {
        Regex::new(r"(^|[^\w$])import\s*\(").expect("static import-call regex must compile")
    });
    let import_stmt = IMPORT_STMT.get_or_init(|| {
        Regex::new(r#"(^|[^\w$])import\s+(['"{*$A-Za-z_])"#)
            .expect("static import-statement regex must compile")
    });
    if require.is_match(source) {
        return Err("unsupported_js: cccc_code_exec does not expose require()".into());
    }
    if import_call.is_match(source) || import_stmt.is_match(source) {
        return Err("unsupported_js: cccc_code_exec does not support import".into());
    }
    Ok(())
}

fn integer_arg(value: Option<&Value>, default: u64, minimum: u64, maximum: u64) -> u64 {
    let parsed = value
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|number| u64::try_from(number).ok()))
                .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
        })
        .unwrap_or(default);
    parsed.clamp(minimum, maximum)
}

async fn nested_tools(home: &HomeLayout, client: &DaemonClient, owner: &Owner) -> Vec<Value> {
    crate::visible_tools_for_actor(home, client, &owner.group_id, &owner.actor_id)
        .await
        .into_iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?;
            if matches!(name, "cccc_code_exec" | "cccc_code_wait") {
                return None;
            }
            let description = tool
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let schema = tool
                .get("inputSchema")
                .and_then(|value| serde_json::to_string(value).ok())
                .unwrap_or_default();
            let full_description = if schema.is_empty() {
                description.to_owned()
            } else {
                format!("{description}\ninputSchema={schema}")
            };
            Some(json!({
                "name": name,
                "global_name": normalize_identifier(name),
                "description": full_description,
            }))
        })
        .collect()
}

fn normalize_identifier(name: &str) -> String {
    let mut value = name
        .chars()
        .enumerate()
        .map(|(index, character)| {
            if character == '_'
                || character == '$'
                || character.is_ascii_alphabetic()
                || (index > 0 && character.is_ascii_digit())
            {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_owned();
    if value.is_empty() || value.starts_with(|character: char| character.is_ascii_digit()) {
        value = format!("tool_{value}");
    }
    value
}

async fn spawn_cell(
    root: &Path,
    owner: Owner,
    source: &str,
    nested_tools: Vec<Value>,
    yield_time_ms: u64,
) -> Result<(String, SharedCell), String> {
    let node = std::env::var("CCCC_CODE_MODE_NODE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "node".into());
    let mut command = Command::new(node);
    command
        .arg("-e")
        .arg(NODE_RUNTIME)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "node_not_found: cccc_code_exec requires Node.js on the CCCC server host".to_owned()
        } else {
            format!("code_mode_start_failed: failed to start Node.js code runtime: {error}")
        }
    })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or("code_mode_start_failed: Node.js stdin is unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("code_mode_start_failed: Node.js stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("code_mode_start_failed: Node.js stderr is unavailable")?;
    let (sender, receiver) = mpsc::channel(EVENT_QUEUE_CAPACITY);
    tokio::spawn(read_stream(stdout, sender.clone(), false));
    tokio::spawn(read_stream(stderr, sender, true));

    let cell_id = NEXT_CELL_ID.fetch_add(1, Ordering::Relaxed).to_string();
    let metadata: Value = serde_json::from_str(METADATA)
        .map_err(|error| format!("invalid embedded code-mode metadata: {error}"))?;
    let stored_values = stored_values(&owner)?;
    send_command(
        &mut stdin,
        &json!({
            "type": "start",
            "cell_id": cell_id,
            "source": source,
            "tools": nested_tools,
            "work_loops": metadata.get("work_loops").cloned().unwrap_or_else(|| json!([])),
            "help_aliases": metadata.get("help_aliases").cloned().unwrap_or_else(|| json!({})),
            "help_compact_notes": metadata.get("help_compact_notes").cloned().unwrap_or_else(|| json!({})),
            "help_curated_tools": metadata.get("help_curated_tools").cloned().unwrap_or_else(|| json!({})),
            "help_curated_loops": metadata.get("help_curated_loops").cloned().unwrap_or_else(|| json!({})),
            "stored_values": stored_values,
            "yield_time_ms": yield_time_ms,
        }),
    )
    .await?;
    let now = Instant::now();
    Ok((
        cell_id,
        Arc::new(CodeCell {
            owner,
            process: Mutex::new(child),
            io: Mutex::new(CellIo {
                stdin,
                events: receiver,
            }),
            started_at: now,
            last_used_at: StdMutex::new(now),
        }),
    ))
}

async fn read_stream<R>(stream: R, sender: mpsc::Sender<Value>, stderr: bool)
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stream);
    loop {
        let event = match read_bounded_line(&mut reader).await {
            Ok(Some(BoundedLine::Data(line))) => {
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if stderr {
                    json!({"type":"stderr","text":line})
                } else {
                    serde_json::from_str(line)
                        .unwrap_or_else(|_| json!({"type":"stderr","text":line}))
                }
            }
            Ok(Some(BoundedLine::TooLong)) => json!({
                "type":"output_truncated",
                "message":format!("code-mode runtime event exceeded {MAX_EVENT_BYTES} bytes"),
            }),
            Ok(None) | Err(_) => break,
        };
        if sender.send(event).await.is_err() {
            return;
        }
    }
    if !stderr {
        let _ = sender.send(json!({"type":"runtime_eof"})).await;
    }
}

async fn send_command(stdin: &mut ChildStdin, payload: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(payload).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    stdin
        .write_all(&bytes)
        .await
        .map_err(|error| format!("cell_closed: {error}"))?;
    stdin
        .flush()
        .await
        .map_err(|error| format!("cell_closed: {error}"))
}

async fn drain(
    home: &HomeLayout,
    client: &DaemonClient,
    cell_id: &str,
    cell: SharedCell,
    yield_time_ms: u64,
    max_output_tokens: usize,
) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_millis(yield_time_ms);
    let mut output = OutputBuffer::new(max_output_tokens);
    *cell
        .last_used_at
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
    let mut cell_io = cell.io.lock().await;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(format_response(
                "running",
                cell_id,
                output,
                cell.started_at,
                "",
            ));
        }
        let event = match tokio::time::timeout(remaining, cell_io.events.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => {
                cells().lock().await.remove(cell_id);
                return Ok(failed_response(
                    cell_id,
                    output,
                    cell.started_at,
                    "exec runtime ended unexpectedly",
                ));
            }
            Err(_) => {
                return Ok(format_response(
                    "running",
                    cell_id,
                    output,
                    cell.started_at,
                    "",
                ));
            }
        };
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "started" => {}
            "content" => {
                if let Some(item) = event.get("item").filter(|item| item.is_object()) {
                    output.push(item.clone());
                }
            }
            "tool_call" => {
                let event_id = event.get("id").and_then(Value::as_str).unwrap_or_default();
                let name = event
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if event_id.is_empty() || name.is_empty() {
                    continue;
                }
                let response =
                    call_nested(home, client, &cell.owner, name, event.get("input")).await;
                let payload = match response {
                    Ok(result) => {
                        json!({"type":"tool_response","id":event_id,"ok":true,"result":result})
                    }
                    Err(error) => {
                        json!({"type":"tool_response","id":event_id,"ok":false,"error":error})
                    }
                };
                send_command(&mut cell_io.stdin, &payload).await?;
            }
            "yield" => {
                update_stored_values(&cell.owner, event.get("stored_values"))?;
                return Ok(format_response(
                    "running",
                    cell_id,
                    output,
                    cell.started_at,
                    "",
                ));
            }
            "result" => {
                cells().lock().await.remove(cell_id);
                update_stored_values(&cell.owner, event.get("stored_values"))?;
                let error_text = event
                    .get("error_text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if error_text.is_empty() {
                    return Ok(format_response(
                        "completed",
                        cell_id,
                        output,
                        cell.started_at,
                        "",
                    ));
                }
                return Ok(failed_response(
                    cell_id,
                    output,
                    cell.started_at,
                    error_text,
                ));
            }
            "stderr" => output.push(json!({
                "type":"text",
                "text":event.get("text").and_then(Value::as_str).unwrap_or_default(),
            })),
            "output_truncated" => output.mark_truncated(),
            "runtime_eof" => {
                cells().lock().await.remove(cell_id);
                return Ok(failed_response(
                    cell_id,
                    output,
                    cell.started_at,
                    "exec runtime ended unexpectedly",
                ));
            }
            _ => {}
        }
    }
}

async fn call_nested(
    home: &HomeLayout,
    client: &DaemonClient,
    owner: &Owner,
    name: &str,
    input: Option<&Value>,
) -> Result<Value, String> {
    if matches!(name, "cccc_code_exec" | "cccc_code_wait") {
        return Err(format!("{name} cannot be invoked from code mode"));
    }
    let mut args = match input {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(object)) => object.clone(),
        Some(_) => return Err(format!("{name} expects a JSON object argument")),
    };
    args.insert("group_id".into(), Value::String(owner.group_id.clone()));
    args.insert("by".into(), Value::String(owner.actor_id.clone()));
    args.entry("actor_id")
        .or_insert_with(|| Value::String(owner.actor_id.clone()));
    let result = Box::pin(crate::router::call(home, client, name, args)).await?;
    Ok(result.get("structuredContent").cloned().unwrap_or(result))
}

fn failed_response(
    cell_id: &str,
    mut output: OutputBuffer,
    started_at: Instant,
    error_text: &str,
) -> Value {
    output.push(json!({"type":"text","text":format!("Script error:\n{error_text}")}));
    format_response("failed", cell_id, output, started_at, error_text)
}

fn format_response(
    status: &str,
    cell_id: &str,
    output: OutputBuffer,
    started_at: Instant,
    error_text: &str,
) -> Value {
    let (trimmed, output_truncated) = output.into_parts();
    let output = trimmed
        .iter()
        .map(content_text)
        .collect::<Vec<_>>()
        .join("\n");
    let running = status == "running";
    let status_text = match status {
        "running" => format!("Script running with cell ID {cell_id}"),
        "terminated" => "Script terminated".into(),
        "failed" => "Script failed".into(),
        _ => "Script completed".into(),
    };
    let recommended_action = if status == "failed" {
        "Read error_text, fix the JS or nested tool call, then rerun a smaller cccc_code_exec cell."
    } else if running {
        "Call cccc_code_wait with this cell_id to collect more output or final status."
    } else if output_truncated {
        "Output was truncated; rerun with narrower commands/line ranges or increase max_output_tokens up to 50000."
    } else {
        ""
    };
    let elapsed = (started_at.elapsed().as_secs_f64() * 1000.0).round() / 1000.0;
    json!({
        "status":status,
        "status_text":status_text,
        "cell_id":cell_id,
        "running":running,
        "wall_time_seconds":elapsed,
        "output":output,
        "items":trimmed,
        "output_truncated":output_truncated,
        "error_text":error_text,
        "recommended_action":recommended_action,
    })
}

fn missing_response(cell_id: &str) -> Value {
    json!({
        "status":"missing",
        "status_text":format!("exec cell {cell_id} not found"),
        "cell_id":cell_id,
        "running":false,
        "output":"",
        "items":[],
        "output_truncated":false,
        "error_text":format!("exec cell {cell_id} not found"),
        "recommended_action":"Check the cell_id from the latest cccc_code_exec result; if the cell expired or belonged to another actor, start a new cccc_code_exec cell.",
    })
}

fn stored_values(owner: &Owner) -> Result<Map<String, Value>, String> {
    STORED_VALUES
        .get_or_init(|| StdMutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "code-mode store lock poisoned".to_owned())
        .map(|values| values.get(owner).cloned().unwrap_or_default())
}

fn update_stored_values(owner: &Owner, value: Option<&Value>) -> Result<(), String> {
    let Some(value) = value.and_then(Value::as_object) else {
        return Ok(());
    };
    STORED_VALUES
        .get_or_init(|| StdMutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "code-mode store lock poisoned".to_owned())?
        .insert(owner.clone(), value.clone());
    Ok(())
}

async fn prune_cells() {
    let snapshot = cells()
        .lock()
        .await
        .iter()
        .map(|(cell_id, cell)| (cell_id.clone(), cell.clone()))
        .collect::<Vec<_>>();
    let mut stale = Vec::new();
    let mut ages = Vec::new();
    for (cell_id, cell) in &snapshot {
        let last_used_at = *cell
            .last_used_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last_used_at.elapsed() > CELL_TTL {
            stale.push(cell_id.clone());
        }
        ages.push((cell_id.clone(), last_used_at));
    }
    if snapshot.len().saturating_sub(stale.len()) >= MAX_CELLS
        && let Some((oldest, _)) = ages
            .into_iter()
            .filter(|(cell_id, _)| !stale.contains(cell_id))
            .min_by_key(|(_, last_used)| *last_used)
    {
        stale.push(oldest);
    }
    if stale.is_empty() {
        return;
    }
    let removed = {
        let mut active = cells().lock().await;
        stale
            .iter()
            .filter_map(|cell_id| active.remove(cell_id))
            .collect::<Vec<_>>()
    };
    terminate_cells(removed).await;
}

fn schedule_expiration(cell_id: String) {
    tokio::spawn(expire_cell_after(cell_id, CELL_TTL));
}

async fn expire_cell_after(cell_id: String, ttl: Duration) {
    let mut remaining = ttl;
    loop {
        tokio::time::sleep(remaining).await;
        let (removed, next_delay) = {
            let mut active = cells().lock().await;
            let Some(cell) = active.get(&cell_id).cloned() else {
                return;
            };
            let elapsed = cell
                .last_used_at
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .elapsed();
            let next_delay = ttl.saturating_sub(elapsed);
            if next_delay.is_zero() {
                (active.remove(&cell_id), Duration::ZERO)
            } else {
                (None, next_delay)
            }
        };
        if let Some(cell) = removed {
            terminate_cell(&cell).await;
            return;
        }
        remaining = next_delay;
    }
}

async fn terminate_cells(removed: Vec<SharedCell>) {
    let mut tasks = tokio::task::JoinSet::new();
    for cell in removed {
        tasks.spawn(async move {
            terminate_cell(&cell).await;
        });
    }
    while tasks.join_next().await.is_some() {}
}

async fn terminate_cell(cell: &SharedCell) {
    let mut child = cell.process.lock().await;
    let _ = child.start_kill();
    let _ = tokio::time::timeout(PROCESS_SHUTDOWN_TIMEOUT, child.wait()).await;
}

pub(crate) async fn shutdown(home: &HomeLayout) {
    let removed = {
        let mut active = cells().lock().await;
        let cell_ids = active
            .iter()
            .filter(|(_, cell)| cell.owner.home == home.root())
            .map(|(cell_id, _)| cell_id.clone())
            .collect::<Vec<_>>();
        cell_ids
            .into_iter()
            .filter_map(|cell_id| active.remove(&cell_id))
            .collect::<Vec<_>>()
    };
    terminate_cells(removed).await;
    if let Some(values) = STORED_VALUES.get() {
        values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|owner, _| owner.home != home.root());
    }
}

fn cells() -> &'static Mutex<CellMap> {
    static CELLS: OnceLock<Mutex<CellMap>> = OnceLock::new();
    CELLS.get_or_init(|| Mutex::new(HashMap::new()))
}

static NEXT_CELL_ID: AtomicU64 = AtomicU64::new(1);
static STORED_VALUES: OnceLock<StdMutex<HashMap<Owner, Map<String, Value>>>> = OnceLock::new();

#[cfg(test)]
mod tests;
