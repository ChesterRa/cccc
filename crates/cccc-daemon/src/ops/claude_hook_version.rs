use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub(super) const MIN_CLAUDE_VERSION: (u64, u64, u64) = (2, 1, 141);

pub(super) fn supported_version(
    executable: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> bool {
    let command = cccc_runtime::prepare_pty_command(&[executable.into(), "--version".into()], env);
    let Some((program, args)) = command.split_first() else {
        return false;
    };
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .envs(env)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let text = String::from_utf8_lossy(&output.stdout);
            parse_version(&text)
        })
        .is_some_and(|version| version >= MIN_CLAUDE_VERSION)
}

pub(super) fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split_whitespace().find_map(|word| {
        let mut parts = word
            .trim_matches(|character: char| !character.is_ascii_digit() && character != '.')
            .split('.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    })
}
