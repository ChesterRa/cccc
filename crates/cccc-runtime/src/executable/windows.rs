use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) fn executable_candidates(
    path: &Path,
    windows: bool,
    pathext: Option<&str>,
) -> Vec<PathBuf> {
    if !windows || path.extension().is_some() {
        return vec![path.to_path_buf()];
    }
    extensions(pathext)
        .into_iter()
        .map(|extension| path.with_extension(extension.trim_start_matches('.')))
        .collect()
}

pub(super) fn is_supported_extension(extension: &str, pathext: Option<&str>) -> bool {
    extensions(pathext)
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
}

fn extensions(pathext: Option<&str>) -> Vec<String> {
    let mut values = pathext
        .unwrap_or(".COM;.EXE;.BAT;.CMD")
        .split(';')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with('.') {
                value.to_ascii_lowercase()
            } else {
                format!(".{value}").to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>();
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.to_ascii_uppercase()));
    values
}

pub(super) fn wrap_resolved_command(
    command: &[String],
    resolved: &Path,
    windows: bool,
    comspec: Option<&str>,
) -> Vec<String> {
    let program = resolved.to_string_lossy().into_owned();
    let extension = resolved
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if windows && matches!(extension.to_ascii_lowercase().as_str(), "cmd" | "bat") {
        let mut prepared = vec![
            comspec.unwrap_or("cmd.exe").to_owned(),
            "/D".into(),
            "/S".into(),
            "/C".into(),
            "call".into(),
            escape_batch_argument(&program),
        ];
        prepared.extend(
            command
                .iter()
                .skip(1)
                .map(|argument| escape_batch_argument(argument)),
        );
        return prepared;
    }
    let mut prepared = vec![program];
    prepared.extend(command.iter().skip(1).cloned());
    prepared
}

fn escape_batch_argument(argument: &str) -> String {
    if argument.is_empty() {
        return String::new();
    }
    // CommandBuilder quotes whitespace and embedded quotes when it builds the
    // CreateProcessW command line. Only protect cmd.exe metacharacters that
    // would otherwise be active in an unquoted argument.
    let quoted_by_command_builder = argument
        .chars()
        .any(|character| character.is_whitespace() || character == '"');
    let mut escaped = String::new();
    for character in argument.chars() {
        match character {
            '%' => escaped.push_str("%%"),
            '^' | '&' | '|' | '<' | '>' | '(' | ')' if !quoted_by_command_builder => {
                escaped.push('^');
                escaped.push(character);
            }
            value => escaped.push(value),
        }
    }
    escaped
}
