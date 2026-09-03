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
        return vec![
            comspec.unwrap_or("cmd.exe").to_owned(),
            "/D".into(),
            "/S".into(),
            "/C".into(),
            batch_command_line(&program, &command[1..]),
        ];
    }
    let mut prepared = vec![program];
    prepared.extend(command.iter().skip(1).cloned());
    prepared
}

fn batch_command_line(program: &str, arguments: &[String]) -> String {
    let mut command = format!("\"\"{program}\"");
    for argument in arguments {
        command.push(' ');
        command.push_str(&escape_batch_argument(argument));
    }
    command.push('"');
    command
}

fn escape_batch_argument(argument: &str) -> String {
    if argument.is_empty() {
        return "\"\"".into();
    }
    let quote = argument.chars().any(char::is_whitespace);
    let mut escaped = String::new();
    for character in argument.chars() {
        match character {
            '%' => escaped.push_str("%%"),
            '^' | '&' | '|' | '<' | '>' | '(' | ')' if !quote => {
                escaped.push('^');
                escaped.push(character);
            }
            '"' => escaped.push_str("\\\""),
            value => escaped.push(value),
        }
    }
    if quote {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}
