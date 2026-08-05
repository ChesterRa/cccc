use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, bool_arg, object, required_arg, string_arg};
use crate::ops::terminal_text;

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "terminal_status" => status(request),
        "terminal_tail" => tail(request),
        "terminal_history" => history(request),
        "terminal_since" => since(request),
        "terminal_write" => write(home, request),
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
    let page = cccc_runtime::retained_history(&group_id, &actor_id).map_err(runtime_error)?;
    let (strip_ansi, compact) = tail_render_options(request);
    let text = render_tail(&page.data, max_chars, strip_ansi, compact);
    object(json!({"text": text, "hint": "", "end_cursor": page.end_cursor}))
}

fn render_tail(text: &str, max_chars: usize, strip_ansi: bool, compact: bool) -> String {
    let rendered = if strip_ansi {
        terminal_text::render(text, compact)
    } else {
        text.to_owned()
    };
    trailing_chars(&rendered, max_chars)
}

fn trailing_chars(text: &str, max_chars: usize) -> String {
    let start = text
        .char_indices()
        .rev()
        .nth(max_chars.saturating_sub(1))
        .map(|(index, _)| index)
        .unwrap_or(0);
    text[start..].to_owned()
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

fn write(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let (group_id, actor_id) = ids(request)?;
    let data = string_arg(request, "data")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OpError::new("invalid_args", "data is required"))?;
    cccc_runtime::write(&group_id, &actor_id, data.as_bytes()).map_err(runtime_error)?;
    super::runtime_hook_input::observe(home, &group_id, &actor_id, data.as_bytes());
    object(json!({"written": data.len()}))
}

#[cfg(test)]
fn is_interrupt_input(data: &str) -> bool {
    data.as_bytes().contains(&0x03) || data == "\u{1b}"
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
    use super::{
        is_interrupt_input, render_tail, tail, tail_render_options, trailing_chars, write,
    };
    use cccc_contracts::{DaemonRequest, RunnerKind};
    use cccc_core::HomeLayout;
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
            let result = write(
                &HomeLayout::from_path(temp.path()).expect("home"),
                &DaemonRequest {
                    v: 1,
                    op: "terminal_write".into(),
                    args,
                },
            );
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
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let args = Map::from_iter([
            ("group_id".into(), Value::String("g1".into())),
            ("actor_id".into(), Value::String("peer1".into())),
            ("data".into(), Value::String(String::new())),
        ]);
        let error = write(
            &home,
            &DaemonRequest {
                v: 1,
                op: "terminal_write".into(),
                args,
            },
        )
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

    #[test]
    fn terminal_tail_truncation_counts_unicode_characters() {
        assert_eq!(trailing_chars("prefix你好世界", 4), "你好世界");
        assert_eq!(trailing_chars("short", 20), "short");
        assert_eq!(trailing_chars("abc", 1), "c");
    }

    #[test]
    fn terminal_tail_renders_before_applying_the_display_limit() {
        let raw = "abcdefgh\u{1b}[6DXY";
        let expected = trailing_chars(&super::terminal_text::render(raw, false), 4);
        let truncated_first = super::terminal_text::render(&trailing_chars(raw, 4), false);

        assert_eq!(render_tail(raw, 4, true, false), expected);
        assert_ne!(expected, truncated_first);
        assert_eq!(render_tail("prefix你好", 2, false, true), "你好");
    }

    #[test]
    fn terminal_tail_uses_the_complete_retained_stream_and_preserves_the_raw_cursor() {
        let temp = tempfile::tempdir().expect("tempdir");
        let group_id = format!("g_terminal_{}", uuid::Uuid::new_v4().simple());
        let actor_id = "tail-peer";
        cccc_runtime::start(LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec![
                "sh".into(),
                "-c".into(),
                "printf '\\033[1;1Habcdefgh\\033[1;1HXY'; sleep 2".into(),
            ],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start runtime");

        let mut retained = cccc_runtime::retained_history(&group_id, actor_id).expect("history");
        for _ in 0..50 {
            if retained.data.contains("abcdefgh") {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            retained = cccc_runtime::retained_history(&group_id, actor_id).expect("history");
        }
        assert!(
            retained.data.contains("abcdefgh"),
            "raw output was not captured"
        );

        let result = tail(&DaemonRequest {
            v: 1,
            op: "terminal_tail".into(),
            args: json!({
                "group_id": group_id,
                "actor_id": actor_id,
                "max_chars": 4,
                "strip_ansi": true,
                "compact": true,
            })
            .as_object()
            .cloned()
            .expect("args"),
        })
        .expect("terminal tail");

        let rendered = super::terminal_text::render(&retained.data, true);
        let expected = trailing_chars(&rendered, 4);
        assert_eq!(
            result.get("text").and_then(Value::as_str),
            Some(expected.as_str())
        );
        assert_eq!(
            result.get("end_cursor").and_then(Value::as_u64),
            Some(retained.end_cursor),
        );
        let since = cccc_runtime::history_since(&group_id, actor_id, retained.end_cursor, 1024)
            .expect("history since tail cursor");
        assert!(since.data.is_empty());
        let _ = cccc_runtime::stop(&group_id, actor_id);
    }

    #[test]
    fn interrupt_input_clears_hook_working_state_without_terminal_output_parsing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        let group_id = format!("g_terminal_{}", uuid::Uuid::new_v4().simple());
        let actor_id = "claude-peer";
        cccc_core::codex_hook_state::begin_launch(
            &home,
            "claude",
            &group_id,
            actor_id,
            "token",
            "HookPending",
        )
        .expect("launch");
        cccc_core::codex_hook_state::record_runtime(
            &home,
            "claude",
            &group_id,
            actor_id,
            "token",
            &json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        )
        .expect("session state");
        cccc_core::codex_hook_state::record_terminal_input(&home, "claude", &group_id, actor_id)
            .expect("working state");
        let runtime = cccc_runtime::start(LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), "sleep 2".into()],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start runtime");
        crate::ops::runtime_hook_session::bind_for_test(
            &home,
            &group_id,
            actor_id,
            "claude",
            "token",
            runtime.pid.expect("pid"),
        );

        let request = DaemonRequest {
            v: 1,
            op: "terminal_write".into(),
            args: json!({"group_id":group_id,"actor_id":actor_id,"data":"\u{3}"})
                .as_object()
                .cloned()
                .expect("args"),
        };
        assert!(write(&home, &request).is_ok(), "write interrupt");

        let state = cccc_core::codex_hook_state::read_runtime(&home, "claude", &group_id, actor_id)
            .expect("hook state");
        assert_eq!(state.status, "idle");
        assert_eq!(state.event, "UserInterrupt");
        assert_eq!(state.turn_id, None);
        assert!(is_interrupt_input("\u{1b}"));
        assert!(is_interrupt_input("\u{3}"));
        assert!(!is_interrupt_input("escape"));
        let _ = cccc_runtime::stop(&group_id, actor_id);
    }

    #[test]
    fn terminal_input_opens_a_new_fail_closed_generation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        home.initialize().expect("initialize home");
        let group_id = format!("g_terminal_{}", uuid::Uuid::new_v4().simple());
        let actor_id = "claude-peer";
        cccc_core::codex_hook_state::begin_launch(
            &home,
            "claude",
            &group_id,
            actor_id,
            "token",
            "HookPending",
        )
        .expect("launch");
        cccc_core::codex_hook_state::record_runtime(
            &home,
            "claude",
            &group_id,
            actor_id,
            "token",
            &json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        )
        .expect("session state");
        let runtime = cccc_runtime::start(LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor_id.into(),
            runner: RunnerKind::Pty,
            command: vec!["sh".into(), "-c".into(), "sleep 2".into()],
            cwd: temp.path().into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect("start runtime");
        crate::ops::runtime_hook_session::bind_for_test(
            &home,
            &group_id,
            actor_id,
            "claude",
            "token",
            runtime.pid.expect("pid"),
        );

        let request = DaemonRequest {
            v: 1,
            op: "terminal_write".into(),
            args: json!({"group_id":group_id,"actor_id":actor_id,"data":"\r"})
                .as_object()
                .cloned()
                .expect("args"),
        };
        assert!(write(&home, &request).is_ok(), "write response");

        let state = cccc_core::codex_hook_state::read_runtime(&home, "claude", &group_id, actor_id)
            .expect("hook state");
        assert_eq!(state.status, "working");
        assert_eq!(state.event, "TerminalInputFailClosed");
        assert_eq!(state.turn_id.as_deref(), Some("local:1"));
        let _ = cccc_runtime::stop(&group_id, actor_id);
    }
}
