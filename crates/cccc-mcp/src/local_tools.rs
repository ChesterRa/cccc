use cccc_client::DaemonClient;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ToolCallError;
use crate::router::{daemon, tool_result};

pub async fn call(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    args: Map<String, Value>,
) -> Result<Value, ToolCallError> {
    let root = scope(client, &args).await?;
    let payload = match name {
        "cccc_repo" | "cccc_repo_edit" => {
            let operation = action(&args).to_owned();
            let tool = name.to_owned();
            tokio::task::spawn_blocking(move || {
                crate::repo::call_tool(&root, &tool, &operation, &args)
            })
            .await
            .map_err(|error| format!("repository task failed: {error}"))??
        }
        "cccc_shell" => shell(&root, &args).await?,
        "cccc_git" => git(&root, &args).await?,
        "cccc_exec_command" => crate::local_sessions::start(home, &root, &args).await?,
        "cccc_write_stdin" => crate::local_sessions::write(home, &args).await?,
        "cccc_code_exec" => crate::code_mode::start(home, client, &root, &args).await?,
        "cccc_code_wait" => crate::code_mode::wait(home, client, &args).await?,
        "cccc_apply_patch" => apply_patch(&root, &args).await?,
        "cccc_file" => return file(home, client, &root, &args).await,
        _ => return Err(format!("unsupported local tool: {name}").into()),
    };
    Ok(if matches!(name, "cccc_code_exec" | "cccc_code_wait") {
        crate::code_mode::tool_result(payload)
    } else {
        tool_result(payload)
    })
}

async fn scope(client: &DaemonClient, args: &Map<String, Value>) -> Result<PathBuf, ToolCallError> {
    let group_id = args
        .get("group_id")
        .cloned()
        .ok_or("group_id is required")?;
    let mut request = Map::new();
    request.insert("group_id".into(), group_id);
    let result = daemon(client, "group_show", request).await?;
    let group: GroupDoc = serde_json::from_value(result.get("group").cloned().unwrap_or_default())
        .map_err(|error| error.to_string())?;
    group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
        .map(|scope| PathBuf::from(&scope.url))
        .filter(|path| path.is_dir())
        .ok_or_else(|| "group has no active local scope".into())
}

async fn one_shot(root: &Path, cmd: Vec<String>, seconds: u64) -> Result<Value, String> {
    let (program, arguments) = cmd.split_first().ok_or("command is required")?;
    let mut command = std::process::Command::new(program);
    command.args(arguments).current_dir(root);
    capture(&mut command, seconds, 2_000_000).await
}

async fn shell(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let cmd = command(args)?;
    let (program, arguments) = cmd.split_first().ok_or("command is required")?;
    let mut process = std::process::Command::new(program);
    process
        .args(arguments)
        .current_dir(command_cwd(root, args)?)
        .envs(command_env(args)?);
    capture(&mut process, timeout(args), output_limit(args)).await
}

async fn capture(
    command: &mut std::process::Command,
    seconds: u64,
    limit: usize,
) -> Result<Value, String> {
    let output = cccc_runtime::capture_command(
        command,
        None,
        std::time::Duration::from_secs(seconds),
        limit,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "exit_code":output.status.code(),
        "stdout":String::from_utf8_lossy(&output.stdout),
        "stderr":String::from_utf8_lossy(&output.stderr),
        "stdout_truncated":output.stdout_truncated,
        "stderr_truncated":output.stderr_truncated,
    }))
}

async fn git(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let action = action(args);
    let mut command = vec!["git".into()];
    match action {
        "status" => command.extend(["status".into(), "--short".into()]),
        "diff" => {
            command.push("diff".into());
            if args.get("staged").and_then(Value::as_bool).unwrap_or(false) {
                command.push("--staged".into());
            }
            append_git_paths(root, args, &mut command)?;
        }
        "log" => {
            let count = args
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 100);
            command.extend([
                "log".into(),
                format!("-{count}"),
                "--oneline".into(),
                "--decorate".into(),
            ]);
        }
        "add" => {
            command.push("add".into());
            if args
                .get("all_changes")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                command.push("-A".into());
            } else {
                append_git_paths(root, args, &mut command)?;
                if command.len() == 2 {
                    return Err("path, paths, or all_changes=true is required for git add".into());
                }
            }
        }
        "commit" => {
            let message = args
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or("message is required for git commit")?;
            command.extend(["commit".into(), "-m".into(), message.into()]);
        }
        _ => return Err("git action must be status, diff, log, add, or commit".into()),
    }
    one_shot(root, command, timeout(args)).await
}

fn append_git_paths(
    root: &Path,
    args: &Map<String, Value>,
    command: &mut Vec<String>,
) -> Result<(), String> {
    let mut paths = args
        .get("paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(path) = args
        .get("path")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        paths.push(path.into());
    }
    if !paths.is_empty() {
        command.push("--".into());
    }
    for path in paths {
        crate::repo::resolve(root, &path, true)?;
        command.push(path);
    }
    Ok(())
}

pub(super) async fn apply_patch(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let patch = args
        .get("patch")
        .or_else(|| args.get("input"))
        .and_then(Value::as_str)
        .ok_or("patch is required")?;
    if patch.trim_start().starts_with("*** Begin Patch") {
        let changed = crate::local_patch::apply(root, patch)?;
        return Ok(json!({"applied":true,"files":changed}));
    }
    let mut command = std::process::Command::new("git");
    command.args(["apply", "-"]).current_dir(root);
    let output = cccc_runtime::capture_command(
        &mut command,
        Some(patch.as_bytes()),
        std::time::Duration::from_secs(timeout(args)),
        2_000_000,
    )
    .await
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(json!({"applied":true}))
}

async fn file(
    home: &HomeLayout,
    client: &DaemonClient,
    root: &Path,
    args: &Map<String, Value>,
) -> Result<Value, ToolCallError> {
    let action = action(args);
    if !matches!(action, "read" | "info" | "blob_path" | "send") {
        return Err(format!("unsupported file action: {action}").into());
    }
    let raw = first_non_blank(args, &["path", "rel_path"]).ok_or("path is required")?;
    if args.contains_key("dst_instance_id") {
        if action != "send" {
            return Err(
                "Connect does not expose remote file tools; use the received local attachment path"
                    .into(),
            );
        }
        let mut request = args.clone();
        for key in ["action", "path", "rel_path", "mode"] {
            request.remove(key);
        }
        request.insert(
            "message_mode".into(),
            Value::String(file_message_mode(args)?),
        );
        request.insert("paths".into(), json!([raw]));
        crate::argument_normalization::normalize_message_author(&mut request);
        crate::argument_normalization::normalize_recipients(&mut request);
        crate::mapping::connect_destination(&mut request)?;
        let result = daemon(client, "connect_send_files", request).await?;
        return Ok(tool_result(
            json!({"accepted":true,"queued":result.get("queued"),"result":result}),
        ));
    }
    let path = if raw.starts_with("state/blobs/") {
        let group_id = args
            .get("group_id")
            .and_then(Value::as_str)
            .ok_or("group_id is required")?;
        cccc_core::blobs::resolve(home, group_id, raw).map_err(|error| error.to_string())?
    } else {
        crate::repo::resolve(root, raw, false)?
    };
    if action == "send" {
        let message_mode = file_message_mode(args)?;
        if raw.starts_with("state/blobs/") {
            return Err("send expects a file under the active project scope".into());
        }
        let mut request = args.clone();
        request.remove("action");
        request.remove("path");
        request.remove("rel_path");
        request.remove("mode");
        request.insert("message_mode".into(), Value::String(message_mode));
        crate::argument_normalization::normalize_message_author(&mut request);
        if request
            .get("dst_group_id")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Err("local cross-group file sends are not supported; use dst_instance_id with dst_group_id for CCCC Connect".into());
        }
        request.insert("paths".into(), json!([path.to_string_lossy().into_owned()]));
        let result = daemon(client, "send_files", request).await?;
        let attachment = result["event"]["data"]["attachments"]
            .as_array()
            .and_then(|attachments| attachments.first())
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(tool_result(
            json!({"sent":true,"attachment":attachment,"result":result}),
        ));
    }
    if action == "blob_path" || action == "info" {
        return Ok(tool_result(
            json!({"path":path,"bytes":path.metadata().map(|meta|meta.len()).unwrap_or(0)}),
        ));
    }
    let args = args.clone();
    tokio::task::spawn_blocking(move || crate::file_read::read(&path, &args))
        .await
        .map_err(|error| format!("file read task failed: {error}"))?
        .map_err(Into::into)
}

pub(super) fn command(args: &Map<String, Value>) -> Result<Vec<String>, String> {
    if let Some(values) = args.get("command").and_then(Value::as_array) {
        return Ok(values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect());
    }
    let raw = first_non_blank(args, &["cmd", "command"]).ok_or("cmd is required")?;
    shell_words::split(raw).map_err(|error| error.to_string())
}

pub(super) fn command_cwd(root: &Path, args: &Map<String, Value>) -> Result<PathBuf, String> {
    if ["cwd", "workdir"]
        .iter()
        .any(|key| args.get(*key).is_some_and(|value| !value.is_string()))
    {
        return Err("cwd and workdir must be relative directory strings".into());
    }
    let cwd = first_non_blank(args, &["cwd", "workdir"]).unwrap_or(".");
    let path = crate::repo::resolve(root, cwd, false)?;
    if !path.is_dir() {
        return Err("cwd must be a directory inside the active scope".into());
    }
    Ok(path)
}

pub(super) fn command_env(args: &Map<String, Value>) -> Result<BTreeMap<String, String>, String> {
    let Some(env) = args.get("env") else {
        return Ok(BTreeMap::new());
    };
    let env = env
        .as_object()
        .ok_or("env must be an object of string values")?;
    env.iter()
        .map(|(key, value)| {
            let value = value.as_str().ok_or("env values must be strings")?;
            if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
                return Err("env contains an invalid variable name or value".into());
            }
            Ok((key.clone(), value.to_owned()))
        })
        .collect()
}

fn output_limit(args: &Map<String, Value>) -> usize {
    args.get("max_output_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(200_000)
        .clamp(1, 1_000_000) as usize
}

fn first_non_blank<'a>(args: &'a Map<String, Value>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|name| {
        args.get(*name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    })
}

fn action(args: &Map<String, Value>) -> &str {
    args.get("action").and_then(Value::as_str).unwrap_or("read")
}
fn file_message_mode(args: &Map<String, Value>) -> Result<String, String> {
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("mail")
        .trim();
    if matches!(mode, "mail" | "send" | "request_reply") {
        Ok(mode.to_owned())
    } else {
        Err("mode must be mail, send, or request_reply".into())
    }
}
fn timeout(args: &Map<String, Value>) -> u64 {
    args.get("timeout_s")
        .or_else(|| args.get("timeout_seconds"))
        .and_then(Value::as_u64)
        .unwrap_or(60)
        .clamp(1, 600)
}

#[cfg(test)]
mod tests {
    use super::{command, file_message_mode, timeout};
    use crate::local_patch::apply as apply_codex_patch;
    use serde_json::json;

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_honors_cwd_environment_and_output_limit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("subdir");
        std::fs::create_dir(&cwd).expect("fixture operation");
        let args = json!({"command":["sh","-c","printf '%s\\n%s' \"$PWD\" \"$CCCC_TOOL_CONTRACT_VALUE\""],"cwd":"subdir","env":{"CCCC_TOOL_CONTRACT_VALUE":"fixture-value"}});
        let result = super::shell(temp.path(), args.as_object().expect("arguments"))
            .await
            .expect("fixture operation");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(
            result["stdout"],
            format!(
                "{}\nfixture-value",
                cwd.canonicalize()
                    .expect("canonical fixture directory")
                    .display()
            )
        );
        let args = json!({"command":["sh","-c","printf 0123456789; printf failure >&2; exit 7"],"max_output_bytes":4});
        let result = super::shell(temp.path(), args.as_object().expect("arguments"))
            .await
            .expect("fixture operation");
        assert_eq!(result["exit_code"], 7);
        assert_eq!(result["stdout"], "0123");
        assert_eq!(result["stderr"], "fail");
        assert_eq!(result["stdout_truncated"], true);
        assert_eq!(result["stderr_truncated"], true);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_interprets_operators_only_when_explicitly_requested() {
        let temp = tempfile::tempdir().expect("tempdir");
        let args = json!({"command":"printf '%s ' 'a|b' '&&' '$HOME'"});
        let result = super::shell(temp.path(), args.as_object().expect("arguments"))
            .await
            .expect("fixture operation");
        assert_eq!(result["stdout"], "a|b && $HOME ");
        let args = json!({"command":"sh -c 'printf first && printf second | cat > result.txt'"});
        let result = super::shell(temp.path(), args.as_object().expect("arguments"))
            .await
            .expect("fixture operation");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["stdout"], "first");
        assert_eq!(
            std::fs::read_to_string(temp.path().join("result.txt")).expect("fixture operation"),
            "second"
        );
    }

    #[test]
    fn command_options_keep_workspace_boundaries_and_schema_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("subdir");
        std::fs::create_dir(&cwd).expect("fixture operation");
        std::fs::write(temp.path().join("file"), "data").expect("fixture operation");
        for path in ["../", "file", "missing"] {
            assert!(
                super::command_cwd(
                    temp.path(),
                    json!({"cwd":path}).as_object().expect("arguments")
                )
                .is_err()
            );
        }
        assert!(
            super::command_cwd(
                temp.path(),
                json!({"cwd":temp.path()}).as_object().expect("arguments")
            )
            .is_err()
        );
        assert!(
            super::command_cwd(
                temp.path(),
                json!({"cwd":123}).as_object().expect("arguments")
            )
            .is_err()
        );
        assert_eq!(
            super::command_cwd(
                temp.path(),
                json!({"workdir":"subdir"}).as_object().expect("arguments")
            )
            .expect("fixture operation"),
            cwd.canonicalize().expect("canonical fixture directory")
        );
        let defaults = serde_json::Map::new();
        assert_eq!(super::timeout(&defaults), 60);
        assert_eq!(super::output_limit(&defaults), 200_000);
        assert_eq!(
            super::output_limit(
                json!({"max_output_bytes":999999999})
                    .as_object()
                    .expect("arguments")
            ),
            1_000_000
        );
        assert!(
            super::command_env(
                json!({"env":{"PRIVATE_VALUE":123}})
                    .as_object()
                    .expect("arguments")
            )
            .is_err()
        );
        assert!(
            !super::command_env(
                json!({"env":{"BAD=NAME":"sensitive"}})
                    .as_object()
                    .expect("arguments")
            )
            .expect_err("invalid environment")
            .contains("sensitive")
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_cwd_rejects_symlink_escape() {
        let temp = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        std::os::unix::fs::symlink(outside.path(), temp.path().join("escape"))
            .expect("fixture operation");
        assert!(
            super::command_cwd(
                temp.path(),
                json!({"cwd":"escape"}).as_object().expect("arguments")
            )
            .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_shell_cannot_continue_writing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let error = super::one_shot(
            temp.path(),
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "sleep 1.3; printf late > marker".into(),
            ],
            1,
        )
        .await
        .expect_err("timeout");
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(error.contains("timed out"));
        assert!(
            !temp.path().join("marker").exists(),
            "timed-out tool continued mutating files"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancelled_shell_cannot_continue_writing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pending = super::one_shot(
            temp.path(),
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "sleep 0.3; printf late > marker".into(),
            ],
            10,
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), pending)
                .await
                .is_err()
        );
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        assert!(
            !temp.path().join("marker").exists(),
            "cancelled tool continued mutating files"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_reports_truncated_output_and_preserves_failure_status() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output = super::one_shot(
            temp.path(),
            vec![
                "/bin/sh".into(),
                "-c".into(),
                "head -c 2000001 /dev/zero; printf error >&2; exit 7".into(),
            ],
            3,
        )
        .await
        .expect("captured failure");
        assert_eq!(output["exit_code"], 7);
        assert_eq!(output["stdout"].as_str().expect("stdout").len(), 2_000_000);
        assert_eq!(output["stdout_truncated"], true);
        assert_eq!(output["stderr"], "error");
        assert_eq!(output["stderr_truncated"], false);
    }

    #[tokio::test]
    async fn unified_patch_uses_the_finite_command_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("file.txt"), "before\n").expect("fixture");
        let args = json!({"patch":"diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-before\n+after\n"}).as_object().cloned().expect("args");
        let result = super::apply_patch(temp.path(), &args)
            .await
            .expect("apply unified diff");
        assert_eq!(result["applied"], true);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("file.txt")).expect("file"),
            "after\n"
        );
        assert!(
            super::apply_patch(temp.path(), &args).await.is_err(),
            "rejected patch must remain an error"
        );
    }

    #[test]
    fn shell_timeout_uses_the_schema_field_and_keeps_the_legacy_alias() {
        let current = json!({"timeout_s":600,"timeout_seconds":15})
            .as_object()
            .cloned()
            .expect("arguments");
        assert_eq!(timeout(&current), 600);
        let legacy = json!({"timeout_seconds":45})
            .as_object()
            .cloned()
            .expect("legacy arguments");
        assert_eq!(timeout(&legacy), 45);
    }

    #[test]
    fn shell_command_skips_an_empty_primary_alias() {
        let args = json!({"cmd":" ","command":"printf fallback"})
            .as_object()
            .cloned()
            .expect("arguments");

        assert_eq!(command(&args).expect("command"), ["printf", "fallback"]);
    }

    #[test]
    fn file_send_maps_the_tool_mode_to_the_canonical_message_mode() {
        let omitted = json!({}).as_object().cloned().expect("omitted arguments");
        assert_eq!(file_message_mode(&omitted).expect("default mode"), "mail");
        let explicit = json!({"mode":"request_reply"})
            .as_object()
            .cloned()
            .expect("explicit arguments");
        assert_eq!(
            file_message_mode(&explicit).expect("explicit mode"),
            "request_reply"
        );
        let invalid = json!({"mode":"attention"})
            .as_object()
            .cloned()
            .expect("invalid arguments");
        assert_eq!(
            file_message_mode(&invalid).expect_err("invalid mode"),
            "mode must be mail, send, or request_reply"
        );
    }

    #[test]
    fn applies_codex_add_update_and_delete_sections() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("edit.txt"), "alpha\nbeta\ngamma\n").expect("edit");
        std::fs::write(temp.path().join("delete.txt"), "gone\n").expect("delete");
        let patch = "*** Begin Patch\n*** Update File: edit.txt\n@@\n alpha\n-beta\n+changed\n gamma\n*** Add File: added.txt\n+new\n+file\n*** Delete File: delete.txt\n*** End Patch";
        let files = apply_codex_patch(temp.path(), patch).expect("patch");
        assert_eq!(files, ["edit.txt", "added.txt", "delete.txt"]);
        assert_eq!(
            std::fs::read_to_string(temp.path().join("edit.txt")).expect("edit"),
            "alpha\nchanged\ngamma\n"
        );
        assert_eq!(
            std::fs::read_to_string(temp.path().join("added.txt")).expect("added"),
            "new\nfile\n"
        );
        assert!(!temp.path().join("delete.txt").exists());
    }
}
