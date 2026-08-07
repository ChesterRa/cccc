use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::memory::MemoryStore;
use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::dispatch::{OpError, OpResult, object, required_arg};

#[derive(Debug)]
struct SearchBlock {
    path: PathBuf,
    start_line: usize,
    end_line: usize,
    text: String,
    metadata: Map<String, Value>,
}

pub(super) fn reme_search(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let started = Instant::now();
    let group_id = required_arg(request, "group_id")?;
    let query = required_arg(request, "query")?;
    let limit = bounded_u64(request.args.get("max_results"), 5, 1, 50, "max_results")? as usize;
    let min_score = bounded_f64(request.args.get("min_score"), 0.1, 0.0, 1.0, "min_score")?;
    if let Some(value) = request.args.get("vector_weight") {
        let _ = bounded_f64(Some(value), 0.7, 0.0, 1.0, "vector_weight")?;
    }
    let candidate_multiplier = request
        .args
        .get("candidate_multiplier")
        .map(|value| bounded_f64(Some(value), 3.0, 1.0, 20.0, "candidate_multiplier"))
        .transpose()?
        .unwrap_or(3.0);
    let memory_source = request
        .args
        .get("sources")
        .and_then(Value::as_array)
        .map(|sources| {
            let normalized = sources
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .collect::<Vec<_>>();
            normalized
                .iter()
                .any(|source| source.eq_ignore_ascii_case("memory"))
                || !normalized
                    .iter()
                    .any(|source| source.eq_ignore_ascii_case("sessions"))
        })
        .unwrap_or(true);
    let hits = if memory_source {
        search_hits(
            home,
            &group_id,
            &query,
            (limit as f64 * candidate_multiplier).ceil() as usize,
            min_score,
        )?
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    object(json!({
        "count":hits.len(),
        "hits":hits,
        "took_ms":started.elapsed().as_millis(),
    }))
}

pub(super) fn search_hits(
    home: &HomeLayout,
    group_id: &str,
    query: &str,
    limit: usize,
    min_score: f64,
) -> Result<Vec<Value>, OpError> {
    let layout = MemoryStore::new(home.clone())
        .layout(group_id, None)
        .map_err(OpError::io)?;
    let terms = query_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let mut files = vec![layout.memory_file];
    files.extend(
        fs::read_dir(layout.daily_dir)
            .map_err(OpError::io)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "md")),
    );
    let mut ranked = Vec::new();
    for path in files {
        for block in blocks(&path) {
            let score = score(&block.text, &terms);
            if score < min_score {
                continue;
            }
            ranked.push((
                score,
                json!({
                    "path":block.path,
                    "start_line":block.start_line,
                    "end_line":block.end_line,
                    "score":score,
                    "snippet":truncate(block.text.trim(), 800),
                    "source":"memory",
                    "raw_metric":Value::Null,
                    "metadata":block.metadata,
                }),
            ));
        }
    }
    ranked.sort_by(|left, right| right.0.total_cmp(&left.0));
    ranked.truncate(limit.min(1_000));
    Ok(ranked.into_iter().map(|(_, hit)| hit).collect())
}

pub(super) fn reme_get(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let group_id = required_arg(request, "group_id")?;
    let relative = required_arg(request, "path")?;
    let offset = bounded_u64(request.args.get("offset"), 1, 1, 1_000_000, "offset")? as usize;
    let limit = bounded_u64(request.args.get("limit"), 200, 1, 5_000, "limit")? as usize;
    let layout = MemoryStore::new(home.clone())
        .layout(&group_id, None)
        .map_err(OpError::io)?;
    let requested = PathBuf::from(&relative);
    let candidate = if requested.is_absolute() {
        requested
    } else {
        layout.root.join(requested)
    };
    let root = layout.root.canonicalize().map_err(OpError::io)?;
    let path = candidate.canonicalize().map_err(OpError::io)?;
    if !path.starts_with(&root) || path.extension().is_none_or(|extension| extension != "md") {
        return Err(OpError::new(
            "invalid_args",
            "memory path escapes the memory root",
        ));
    }
    let text = fs::read_to_string(&path).map_err(OpError::io)?;
    let lines = text.lines().collect::<Vec<_>>();
    let content = lines
        .iter()
        .skip(offset - 1)
        .take(limit)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    object(json!({
        "path":path,
        "offset":offset,
        "limit":limit,
        "total_lines":lines.len(),
        "content":content,
    }))
}

fn blocks(path: &Path) -> Vec<SearchBlock> {
    let content = fs::read_to_string(path).unwrap_or_default();
    let lines = content.lines().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].starts_with("## mem_") {
            let start = index;
            let mut end = index + 1;
            while end < lines.len() && !lines[end].starts_with("## mem_") {
                end += 1;
            }
            let metadata = lines[start..end]
                .iter()
                .find_map(|line| parse_metadata(line))
                .unwrap_or_default();
            output.push(SearchBlock {
                path: path.to_path_buf(),
                start_line: start + 1,
                end_line: end,
                text: lines[start..end].join("\n"),
                metadata,
            });
            index = end;
            continue;
        }
        let line = lines[index].trim();
        if !line.is_empty() && !line.starts_with('#') {
            output.push(SearchBlock {
                path: path.to_path_buf(),
                start_line: index + 1,
                end_line: index + 1,
                text: line.to_owned(),
                metadata: Map::new(),
            });
        }
        index += 1;
    }
    output
}

fn parse_metadata(line: &str) -> Option<Map<String, Value>> {
    let raw = line
        .trim()
        .strip_prefix("<!-- cccc.memory.meta ")?
        .strip_suffix(" -->")?;
    serde_json::from_str::<Value>(raw)
        .ok()?
        .as_object()
        .cloned()
}

#[derive(Debug)]
struct QueryTerm {
    value: String,
    ascii_word: bool,
}

fn query_terms(query: &str) -> Vec<QueryTerm> {
    let mut terms = Vec::new();
    let mut ascii = String::new();
    for character in query.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            ascii.push(character);
            continue;
        }
        if !ascii.is_empty() {
            terms.push(QueryTerm {
                value: std::mem::take(&mut ascii),
                ascii_word: true,
            });
        }
        if character.is_alphanumeric() {
            terms.push(QueryTerm {
                value: character.to_string(),
                ascii_word: false,
            });
        }
    }
    if !ascii.is_empty() {
        terms.push(QueryTerm {
            value: ascii,
            ascii_word: true,
        });
    }
    terms
}

fn score(text: &str, terms: &[QueryTerm]) -> f64 {
    let lower = text.to_lowercase();
    let words = lower
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let matched = terms
        .iter()
        .filter(|term| {
            if term.ascii_word {
                words.contains(&term.value.as_str())
            } else {
                lower.contains(&term.value)
            }
        })
        .count();
    matched as f64 / terms.len().max(1) as f64
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn bounded_u64(
    value: Option<&Value>,
    default: u64,
    minimum: u64,
    maximum: u64,
    field: &str,
) -> Result<u64, OpError> {
    let value = value.map_or(Ok(default), |value| {
        value
            .as_u64()
            .ok_or_else(|| OpError::new("invalid_args", format!("{field} must be integer")))
    })?;
    if !(minimum..=maximum).contains(&value) {
        return Err(OpError::new(
            "invalid_args",
            format!("{field} must be in [{minimum}, {maximum}]"),
        ));
    }
    Ok(value)
}

fn bounded_f64(
    value: Option<&Value>,
    default: f64,
    minimum: f64,
    maximum: f64,
    field: &str,
) -> Result<f64, OpError> {
    let value = value.map_or(Ok(default), |value| {
        value
            .as_f64()
            .ok_or_else(|| OpError::new("invalid_args", format!("{field} must be float")))
    })?;
    if !(minimum..=maximum).contains(&value) {
        return Err(OpError::new(
            "invalid_args",
            format!("{field} must be in [{minimum}, {maximum}]"),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{query_terms, score};

    #[test]
    fn latin_terms_use_boundaries_and_cjk_terms_match_individual_characters() {
        assert_eq!(score("Capillary notes", &query_terms("api")), 0.0);
        assert_eq!(score("Public API contract", &query_terms("api")), 1.0);
        assert!(score("回答应保持简洁，并使用中文", &query_terms("中文简洁回复")) > 0.5);
    }
}
