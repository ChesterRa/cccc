use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::dispatch::OpError;

use super::search::search_hits;

pub(super) fn normalize_messages(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(Value::as_object)
        .map(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let role = if matches!(role.as_str(), "system" | "user" | "assistant" | "tool") {
                role
            } else {
                "assistant".into()
            };
            json!({
                "role":role,
                "name":message.get("name").and_then(Value::as_str).unwrap_or_default().trim(),
                "content":message.get("content").and_then(Value::as_str).unwrap_or_default(),
            })
        })
        .collect()
}

pub(super) fn message_tokens(message: &Value) -> usize {
    message
        .get("content")
        .and_then(Value::as_str)
        .map_or(1, |content| (content.chars().count() / 4).max(1))
}

pub(super) fn total_tokens(messages: &[Value]) -> usize {
    messages.iter().map(message_tokens).sum()
}

pub(super) fn serialize_messages(messages: &[Value]) -> String {
    messages
        .iter()
        .map(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("assistant")
                .to_ascii_uppercase();
            let name = message
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let prefix = if name.is_empty() {
                role
            } else {
                format!("{role}({name})")
            };
            format!(
                "{prefix}: {}",
                message
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            )
            .trim()
            .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

#[derive(Debug)]
pub(super) struct DedupMeta {
    intent: String,
    query: String,
    hits: Vec<Value>,
    precheck_decision: String,
}

impl DedupMeta {
    pub(super) fn precheck_is_silent(&self) -> bool {
        self.precheck_decision == "silent" && !self.hits.is_empty()
    }

    pub(super) fn hits(&self) -> &[Value] {
        &self.hits
    }

    pub(super) fn finalize(&self, status: &str, reason: &str) -> Value {
        let (final_decision, final_reason) = if status == "silent" {
            (
                "silent".to_owned(),
                if reason.trim().is_empty() {
                    "persistence_content_hash".to_owned()
                } else {
                    reason.trim().to_ascii_lowercase()
                },
            )
        } else {
            (self.precheck_decision.clone(), "accepted".into())
        };
        let top_score = self
            .hits
            .first()
            .and_then(|hit| hit.get("score"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        json!({
            "intent":self.intent,
            "query":self.query,
            "candidate_count":self.hits.len(),
            "top_score":top_score,
            "hits":self.hits,
            "precheck_decision":self.precheck_decision,
            "final_decision":final_decision,
            "final_reason":final_reason,
            "decision":final_decision,
        })
    }
}

pub(super) fn dedup_intent(value: Option<&Value>) -> String {
    let intent = value
        .and_then(Value::as_str)
        .unwrap_or("new")
        .trim()
        .to_ascii_lowercase();
    if matches!(intent.as_str(), "new" | "update" | "supersede" | "silent") {
        intent
    } else {
        "new".into()
    }
}

pub(super) fn dedup_precheck(
    home: &HomeLayout,
    group_id: &str,
    query: &str,
    intent: String,
) -> DedupMeta {
    let query = truncate(&query.replace('\n', " "), 260).trim().to_owned();
    let hits = if query.is_empty() {
        Vec::new()
    } else {
        search_hits(home, group_id, &query, 3, 0.92)
            .unwrap_or_default()
            .into_iter()
            .take(3)
            .map(|hit| {
                json!({
                    "path":hit.get("path").cloned().unwrap_or(Value::Null),
                    "start_line":hit.get("start_line").cloned().unwrap_or_else(|| json!(1)),
                    "score":hit.get("score").cloned().unwrap_or_else(|| json!(0.0)),
                })
            })
            .collect()
    };
    let precheck_decision = if intent == "silent" && !hits.is_empty() {
        "silent".into()
    } else {
        intent.clone()
    };
    DedupMeta {
        intent,
        query,
        hits,
        precheck_decision,
    }
}

pub(super) struct EntryInput<'a> {
    pub kind: &'a str,
    pub summary: &'a str,
    pub actor_id: &'a str,
    pub source_refs: Vec<String>,
    pub tags: Vec<String>,
    pub supersedes: Vec<String>,
    pub date: &'a str,
}

pub(super) fn build_entry(
    home: &HomeLayout,
    group_id: &str,
    input: EntryInput<'_>,
) -> Result<Map<String, Value>, OpError> {
    let group = GroupStore::new(home.clone())
        .map_err(OpError::io)?
        .load(group_id)
        .map_err(OpError::io)?;
    let created_at = cccc_contracts::utc_now();
    Ok(json!({
        "entry_id":format!("mem_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        "date":input.date,
        "group_label":group.title,
        "kind":input.kind,
        "summary":input.summary.trim(),
        "source_refs":input.source_refs,
        "tags":input.tags,
        "supersedes":input.supersedes,
        "actor_id":input.actor_id,
        "created_at":created_at,
    })
    .as_object()
    .cloned()
    .unwrap_or_default())
}

#[derive(Debug)]
pub(super) struct WriteOutcome {
    pub path: PathBuf,
    pub status: String,
    pub reason: String,
    pub bytes_written: usize,
    pub line_count: usize,
    pub content_hash: String,
}

pub(super) fn append_entry(
    path: &Path,
    entry: &Map<String, Value>,
    idempotency_key: &str,
) -> io::Result<WriteOutcome> {
    let summary = entry
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let summary_hash = digest(summary);
    let mut metadata = entry.clone();
    metadata.remove("summary");
    metadata.insert("content_hash".into(), Value::String(summary_hash.clone()));
    if !idempotency_key.is_empty() {
        metadata.insert(
            "idempotency_key".into(),
            Value::String(idempotency_key.to_owned()),
        );
    }
    let block = format!(
        "## {} [{}] {}\n<!-- cccc.memory.meta {} -->\n\n{}\n\n",
        metadata
            .get("entry_id")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        metadata
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        metadata
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        serde_json::to_string(&metadata).map_err(io::Error::other)?,
        summary,
    );
    write_locked(path, |existing| {
        if !idempotency_key.is_empty() && existing.contains(idempotency_key) {
            return Ok(silent_outcome(
                path,
                existing,
                "persistence_idempotency_key",
            ));
        }
        if contains_content_hash(&existing, &summary_hash) {
            return Ok(silent_outcome(path, existing, "persistence_content_hash"));
        }
        let prefix = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        let merged = format!("{existing}{prefix}{block}");
        Ok((
            merged.clone(),
            WriteOutcome {
                path: path.to_path_buf(),
                status: "written".into(),
                reason: String::new(),
                bytes_written: block.len(),
                line_count: merged.lines().count(),
                content_hash: digest(&merged),
            },
        ))
    })
}

fn contains_content_hash(existing: &str, expected: &str) -> bool {
    existing.lines().any(|line| {
        let Some(raw) = line
            .trim()
            .strip_prefix("<!-- cccc.memory.meta ")
            .and_then(|line| line.strip_suffix(" -->"))
        else {
            return false;
        };
        serde_json::from_str::<Value>(raw)
            .ok()
            .is_some_and(|metadata| {
                metadata.get("content_hash").and_then(Value::as_str) == Some(expected)
            })
    })
}

pub(super) fn write_raw(path: &Path, content: &str, mode: &str) -> io::Result<WriteOutcome> {
    write_locked(path, |existing| {
        let payload = if mode == "replace" {
            content.to_owned()
        } else {
            let prefix = if existing.is_empty() || existing.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            format!("{existing}{prefix}{content}")
        };
        let bytes_written = if mode == "replace" {
            payload.len()
        } else {
            content.len()
        };
        Ok((
            payload.clone(),
            WriteOutcome {
                path: path.to_path_buf(),
                status: "written".into(),
                reason: String::new(),
                bytes_written,
                line_count: payload.lines().count(),
                content_hash: digest(&payload),
            },
        ))
    })
}

fn write_locked(
    path: &Path,
    operation: impl FnOnce(String) -> io::Result<(String, WriteOutcome)>,
) -> io::Result<WriteOutcome> {
    let lock_path = path.with_extension("md.lock");
    cccc_core::fs::with_exclusive_lock(&lock_path, || {
        let existing = match fs::read_to_string(path) {
            Ok(existing) => existing,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let (merged, outcome) = operation(existing)?;
        cccc_core::fs::atomic_write(path, merged.as_bytes())?;
        Ok(outcome)
    })
}

fn silent_outcome(path: &Path, existing: String, reason: &str) -> (String, WriteOutcome) {
    (
        existing.clone(),
        WriteOutcome {
            path: path.to_path_buf(),
            status: "silent".into(),
            reason: reason.into(),
            bytes_written: 0,
            line_count: existing.lines().count(),
            content_hash: digest(&existing),
        },
    )
}

pub(super) fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub(super) fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(super) fn truncate(value: &str, max_chars: usize) -> String {
    let length = value.chars().count();
    if length <= max_chars {
        return value.trim().to_owned();
    }
    if max_chars <= 3 {
        return value.chars().take(max_chars).collect();
    }
    format!(
        "{}...",
        value
            .chars()
            .take(max_chars - 3)
            .collect::<String>()
            .trim_end()
    )
}

#[cfg(test)]
mod tests {
    use super::{contains_content_hash, digest, write_raw};

    #[test]
    fn recognizes_python_spaced_metadata_for_cross_language_dedup() {
        let content_hash = digest("shared summary");
        let existing = format!(
            "<!-- cccc.memory.meta {{\"entry_id\": \"mem_python\", \"content_hash\": \"{content_hash}\"}} -->"
        );
        assert!(contains_content_hash(&existing, &content_hash));
    }

    #[test]
    fn write_preserves_an_existing_file_when_it_cannot_be_decoded() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("MEMORY.md");
        let original = [0xff, 0xfe, 0xfd];
        std::fs::write(&path, original).expect("invalid UTF-8 fixture");

        assert!(write_raw(&path, "replacement", "append").is_err());
        assert_eq!(std::fs::read(path).expect("preserved memory"), original);
    }
}
