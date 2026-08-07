use cccc_contracts::DaemonRequest;
use serde_json::{Value, json};

use crate::dispatch::{OpError, OpResult, object, required_arg, string_arg};

use super::common::{message_tokens, normalize_messages, serialize_messages, total_tokens};

pub(super) fn context_check(request: &DaemonRequest) -> OpResult {
    let _ = required_arg(request, "group_id")?;
    let raw = request
        .args
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "messages must be an array"))?;
    let messages = normalize_messages(raw);
    let window = token_arg(request, "context_window_tokens", 128_000, 1_024)?;
    let reserve = token_arg(request, "reserve_tokens", 36_000, 0)?;
    let keep = token_arg(request, "keep_recent_tokens", 20_000, 256)?;
    let token_count = total_tokens(&messages);
    let threshold = window.saturating_sub(reserve).max(1);
    if token_count < threshold {
        return object(json!({
            "needs_compaction":false,
            "token_count":token_count,
            "threshold":threshold,
            "messages_to_summarize":[],
            "turn_prefix_messages":[],
            "left_messages":messages,
            "is_split_turn":false,
            "cut_index":0,
        }));
    }

    let mut accumulated = 0;
    let mut cut_index = 0;
    for index in (0..messages.len()).rev() {
        accumulated += message_tokens(&messages[index]);
        if accumulated >= keep {
            cut_index = index;
            break;
        }
    }
    if cut_index == 0 {
        return object(json!({
            "needs_compaction":true,
            "token_count":token_count,
            "threshold":threshold,
            "messages_to_summarize":[],
            "turn_prefix_messages":[],
            "left_messages":messages,
            "is_split_turn":false,
            "cut_index":0,
        }));
    }
    if messages[cut_index]["role"] == "user" {
        return object(json!({
            "needs_compaction":true,
            "token_count":token_count,
            "threshold":threshold,
            "messages_to_summarize":messages[..cut_index],
            "turn_prefix_messages":[],
            "left_messages":messages[cut_index..],
            "is_split_turn":false,
            "cut_index":cut_index,
        }));
    }
    let turn_start = (0..=cut_index)
        .rev()
        .find(|index| messages[*index]["role"] == "user");
    let Some(turn_start) = turn_start else {
        return object(json!({
            "needs_compaction":true,
            "token_count":token_count,
            "threshold":threshold,
            "messages_to_summarize":messages[..cut_index],
            "turn_prefix_messages":[],
            "left_messages":messages[cut_index..],
            "is_split_turn":false,
            "cut_index":cut_index,
        }));
    };
    object(json!({
        "needs_compaction":true,
        "token_count":token_count,
        "threshold":threshold,
        "messages_to_summarize":messages[..turn_start],
        "turn_prefix_messages":messages[turn_start..cut_index],
        "left_messages":messages[cut_index..],
        "is_split_turn":true,
        "cut_index":cut_index,
    }))
}

pub(super) fn compact(request: &DaemonRequest) -> OpResult {
    let _ = required_arg(request, "group_id")?;
    let history = request
        .args
        .get("messages_to_summarize")
        .and_then(Value::as_array)
        .ok_or_else(|| OpError::new("invalid_args", "messages_to_summarize must be an array"))?;
    let prefix = match request.args.get("turn_prefix_messages") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(prefix)) => prefix.as_slice(),
        Some(_) => {
            return Err(OpError::new(
                "invalid_args",
                "turn_prefix_messages must be an array when provided",
            ));
        }
    };
    let payload = compact_payload(
        history,
        prefix,
        &string_arg(request, "previous_summary").unwrap_or_default(),
        &string_arg(request, "language").unwrap_or_else(|| "en".into()),
        request
            .args
            .get("return_prompt")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );
    object(payload)
}

fn token_arg(
    request: &DaemonRequest,
    field: &str,
    default: u64,
    minimum: u64,
) -> Result<usize, OpError> {
    let value = match request.args.get(field) {
        None => default,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| OpError::new("invalid_args", format!("{field} must be integer")))?,
    };
    if !(minimum..=2_000_000).contains(&value) {
        return Err(OpError::new(
            "invalid_args",
            format!("{field} must be in [{minimum}, 2000000]"),
        ));
    }
    Ok(value as usize)
}

pub(super) fn compact_payload(
    history: &[Value],
    prefix: &[Value],
    previous_summary: &str,
    language: &str,
    return_prompt: bool,
) -> Value {
    let history = serialize_messages(&normalize_messages(history));
    let prefix = serialize_messages(&normalize_messages(prefix));
    let system = format!(
        "Summarize high-signal durable facts only. Remove chit-chat and duplicates. Output language: {}.",
        if language.trim().is_empty() {
            "en"
        } else {
            language.trim()
        }
    );
    if return_prompt {
        let mut prompt = serde_json::Map::new();
        prompt.insert("system".into(), Value::String(system));
        if !history.is_empty() {
            prompt.insert("history_user".into(), Value::String(history));
        }
        if !prefix.is_empty() {
            prompt.insert("turn_prefix_user".into(), Value::String(prefix));
        }
        if !previous_summary.is_empty() {
            prompt.insert(
                "previous_summary".into(),
                Value::String(previous_summary.to_owned()),
            );
        }
        return json!({"prompt":prompt});
    }

    let mut lines = Vec::new();
    if !previous_summary.is_empty() {
        lines.push(format!("Previous summary: {}", previous_summary.trim()));
    }
    if !history.is_empty() {
        lines.push("History summary:".into());
        lines.extend(
            history
                .lines()
                .take(12)
                .filter(|line| !line.trim().is_empty())
                .map(|line| format!("- {}", truncate(line, 400))),
        );
    }
    if !prefix.is_empty() {
        lines.push("Turn context:".into());
        lines.extend(
            prefix
                .lines()
                .take(6)
                .filter(|line| !line.trim().is_empty())
                .map(|line| format!("- {}", truncate(line, 400))),
        );
    }
    json!({"summary":lines.join("\n").trim()})
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::context_check;
    use cccc_contracts::DaemonRequest;
    use serde_json::json;

    #[test]
    fn split_turn_preserves_user_prefix_for_compaction() {
        let request = DaemonRequest {
            v: 1,
            op: "memory_reme_context_check".into(),
            args: json!({
                "group_id":"g_test",
                "messages":[
                    {"role":"user","content":"old ".repeat(200)},
                    {"role":"assistant","content":"answer ".repeat(200)},
                    {"role":"user","name":"alice","content":"new question ".repeat(120)},
                    {"role":"assistant","content":"partial ".repeat(80)},
                    {"role":"tool","content":"tool output ".repeat(80)}
                ],
                "context_window_tokens":1200,
                "reserve_tokens":100,
                "keep_recent_tokens":300
            })
            .as_object()
            .cloned()
            .expect("args"),
        };
        let result = context_check(&request).expect("context check");
        assert_eq!(result["is_split_turn"], true);
        assert_eq!(result["turn_prefix_messages"][0]["role"], "user");
        assert_eq!(result["turn_prefix_messages"][0]["name"], "alice");
    }
}
