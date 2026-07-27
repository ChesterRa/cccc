use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::terminal_text;

pub fn handle(_home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "terminal_status" => status(request),
        "terminal_tail" => tail(request),
        "terminal_history" => history(request),
        "terminal_since" => since(request),
        "terminal_write" => write(request),
        "terminal_resize" => resize(request),
        "terminal_clear" => clear(request),
        _ => return None,
    })
}

fn ids(request: &DaemonRequest) -> Result<(String, String), OpError> {
    Ok((
        required_arg(request, "group_id")?,
        required_arg(request, "actor_id")?,
    ))
}

fn status(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let status = cccc_runtime::status(&group_id, &actor_id).map_err(runtime_error)?;
    object(json!({"session": status}))
}

fn tail(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let max_chars = integer(request, "max_chars", 8_000).clamp(1, 2_000_000);
    let page =
        cccc_runtime::history(&group_id, &actor_id, None, max_chars).map_err(runtime_error)?;
    let (strip_ansi, compact) = tail_render_options(request);
    let text = if strip_ansi {
        terminal_text::render(&page.data, compact)
    } else {
        page.data
    };
    object(json!({"text": text, "hint": "", "end_cursor": page.end_cursor}))
}

fn history(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let before = request.args.get("before").and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    });
    let limit = integer(request, "limit_bytes", 64_000).clamp(1, 2_000_000);
    let page = cccc_runtime::history(&group_id, &actor_id, before, limit).map_err(runtime_error)?;
    let strip_ansi = bool_arg(request, "strip_ansi", false);
    let text = if strip_ansi {
        terminal_text::render(&page.data, bool_arg(request, "compact", false))
    } else {
        page.data.clone()
    };
    object(json!({
        "text": text,
        "hint": "",
        "start_cursor": page.start_cursor,
        "end_cursor": page.end_cursor,
        "has_more": page.has_more,
        "cursor_expired": page.cursor_expired,
        "history": page,
    }))
}

fn since(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let after = request
        .args
        .get("after")
        .and_then(Value::as_u64)
        .unwrap_or(u64::MAX);
    let limit = integer(request, "limit_bytes", 64_000).clamp(1, 2_000_000);
    let page =
        cccc_runtime::history_since(&group_id, &actor_id, after, limit).map_err(runtime_error)?;
    object(json!({"history": page}))
}

fn write(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let data = string_arg(request, "data")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "data is required"))?;
    cccc_runtime::write(&group_id, &actor_id, data.as_bytes()).map_err(runtime_error)?;
    object(json!({"written": data.len()}))
}

fn resize(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let cols = integer(request, "cols", 120).clamp(1, u16::MAX as usize) as u16;
    let rows = integer(request, "rows", 40).clamp(1, u16::MAX as usize) as u16;
    cccc_runtime::resize(&group_id, &actor_id, cols, rows).map_err(runtime_error)?;
    object(json!({"resized": true, "cols": cols, "rows": rows}))
}

fn clear(request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    cccc_runtime::clear(&group_id, &actor_id).map_err(runtime_error)?;
    object(json!({"cleared": true}))
}

fn integer(request: &DaemonRequest, name: &str, default: usize) -> usize {
    request
        .args
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}

fn tail_render_options(request: &DaemonRequest) -> (bool, bool) {
    (
        bool_arg(request, "strip_ansi", true),
        bool_arg(request, "compact", true),
    )
}

fn runtime_error(error: cccc_runtime::RuntimeError) -> OpError {
    OpError::new("runtime_error", error.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::{tail_render_options, write};
    use cccc_contracts::{DaemonRequest, RunnerKind};
    use cccc_runtime::LaunchSpec;
    use serde_json::{Map, Value, json};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn write_preserves_standalone_carriage_return() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group_id = format!("g_terminal_{}", uuid::Uuid::new_v4().simple());
        let actor_id = "peer1";
        cccc_runtime::start(LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec![
                "sh".into(),
                "-c".into(),
                "IFS= read -r line; printf 'received:<%s>' \"$line\"; sleep 1".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start runtime");

        for data in ["/model", "\r"] {
            let args = json!({"group_id":group_id,"actor_id":actor_id,"data":data})
                .as_object()
                .cloned()
                .expect("args");
            let result = write(&DaemonRequest {
                v: 1,
                op: "terminal_write".into(),
                args,
            });
            assert!(result.is_ok(), "terminal input should be accepted");
        }

        std::thread::sleep(Duration::from_millis(100));
        let output = cccc_runtime::history(&group_id, actor_id, None, 1024)
            .expect("terminal history")
            .data;
        assert!(
            output.contains("received:</model>"),
            "output was {output:?}"
        );
        let _ = cccc_runtime::stop(&group_id, actor_id);
    }

    #[test]
    fn write_rejects_empty_data() {
        let args = Map::from_iter([
            ("group_id".into(), Value::String("g1".into())),
            ("actor_id".into(), Value::String("peer1".into())),
            ("data".into(), Value::String(String::new())),
        ]);
        let error = write(&DaemonRequest {
            v: 1,
            op: "terminal_write".into(),
            args,
        })
        .expect_err("empty input must be rejected");
        assert_eq!(error.code, "invalid_args");
    }

    #[test]
    fn terminal_tail_defaults_to_readable_rendering_for_non_web_callers() {
        let request = DaemonRequest {
            v: 1,
            op: "terminal_tail".into(),
            args: Map::new(),
        };
        assert_eq!(tail_render_options(&request), (true, true));

        let request = DaemonRequest {
            args: Map::from_iter([
                ("strip_ansi".into(), Value::Bool(false)),
                ("compact".into(), Value::Bool(false)),
            ]),
            ..request
        };
        assert_eq!(tail_render_options(&request), (false, false));
    }
}
