use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn run(root: &Path, args: &[&str]) -> Result<cccc_runtime::CapturedOutput, OpError> {
    let mut command = Command::new("git");
    command
        .args(["-c", "core.fsmonitor=false"])
        .args(args)
        .current_dir(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(name);
    }
    let output = cccc_runtime::capture_command_blocking(
        &mut command,
        None,
        Duration::from_secs(2),
        1024 * 1024,
    )
    .map_err(|e| OpError::new("backlog_git_unavailable", e.to_string()))?;
    if output.stdout_truncated {
        return Err(OpError::new(
            "backlog_git_unavailable",
            "Git result exceeded the read limit",
        ));
    }
    Ok(output)
}

fn git_root(root: &Path) -> Result<Option<PathBuf>, OpError> {
    let output = run(root, &["rev-parse", "--show-toplevel"])?;
    if !output.status.success() {
        if String::from_utf8_lossy(&output.stderr).contains("not a git repository") {
            return Ok(None);
        }
        return Err(OpError::new(
            "backlog_git_unavailable",
            "Git could not inspect this repository",
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(OpError::invalid)?;
    let path = PathBuf::from(text.trim_end_matches(['\r', '\n']));
    Ok(Some(path.canonicalize().map_err(OpError::io)?))
}

pub(super) fn facts(group: &GroupDoc, path: &str) -> Result<Value, OpError> {
    let root = workspace::root(group).map_err(file_error)?;
    if git_root(&root)?.is_none() {
        return Ok(json!({"repository":false,"tracked":false,"ignored":false}));
    }
    if path.is_empty() || !root.join(path).exists() {
        return Ok(json!({"repository":true,"tracked":false,"ignored":false}));
    }
    let tracked = flag(run(
        &root,
        &[
            "ls-files",
            "--error-unmatch",
            "--",
            &format!(":(literal){path}"),
        ],
    )?)?;
    let ignored = flag(run(&root, &["check-ignore", "--quiet", "--", path])?)?;
    Ok(json!({"repository":true,"tracked":tracked,"ignored":ignored}))
}

pub(super) fn exclude(group: &GroupDoc, path: &str) -> Result<bool, OpError> {
    let root = workspace::root(group).map_err(file_error)?;
    let repo = git_root(&root)?.ok_or_else(|| {
        OpError::new(
            "backlog_git_unavailable",
            "This project is not in a Git repository",
        )
    })?;
    if flag(run(
        &root,
        &[
            "ls-files",
            "--error-unmatch",
            "--",
            &format!(":(literal){path}"),
        ],
    )?)? {
        return Err(OpError::new(
            "backlog_tracked",
            "File is already tracked; excluding it cannot make it local-only",
        ));
    }
    let target = root.join(path);
    let relative = target
        .strip_prefix(&repo)
        .map_err(OpError::invalid)?
        .to_str()
        .ok_or_else(|| OpError::invalid("Git path must be UTF-8"))?
        .replace(std::path::MAIN_SEPARATOR, "/");
    let output = run(&root, &["rev-parse", "--git-path", "info/exclude"])?;
    if !output.status.success() {
        return Err(OpError::new(
            "backlog_git_unavailable",
            "Cannot locate local Git exclusion file",
        ));
    }
    let exclude = PathBuf::from(
        String::from_utf8(output.stdout)
            .map_err(OpError::invalid)?
            .trim_end_matches(['\r', '\n']),
    );
    let exclude = if exclude.is_absolute() {
        exclude
    } else {
        root.join(exclude)
    };
    // Different Groups can select files in the same repository. Serialize the
    // read/append/replace within this daemon so neither loses the other's rule.
    static EXCLUDE_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = EXCLUDE_WRITE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut content = match fs::read_to_string(&exclude) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(OpError::io(error)),
    };
    let rule = literal_rule(&relative);
    if content.lines().any(|line| line == rule) {
        return Ok(false);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&rule);
    content.push('\n');
    cccc_core::fs::atomic_write_preserving_mode(&exclude, content.as_bytes())
        .map_err(|error| partial(OpError::io(error), json!({})))?;
    Ok(true)
}

fn literal_rule(path: &str) -> String {
    let mut result = String::from("/");
    for ch in path.chars() {
        if matches!(ch, '\\' | '*' | '?' | '[' | ']' | '!' | '#' | ' ') {
            result.push('\\');
        }
        result.push(ch);
    }
    result
}

fn flag(output: cccc_runtime::CapturedOutput) -> Result<bool, OpError> {
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(OpError::new(
            "backlog_git_unavailable",
            "Git could not determine tracking or ignore status",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusion_is_literal_and_rooted() {
        assert_eq!(
            literal_rule("docs/a[1]* #!.md"),
            "/docs/a\\[1\\]\\*\\ \\#\\!.md"
        );
    }
}
