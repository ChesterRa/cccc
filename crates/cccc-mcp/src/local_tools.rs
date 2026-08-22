use base64::Engine;
use cccc_client::DaemonClient;
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

use crate::router::{daemon, tool_result};

pub async fn call(
    home: &HomeLayout,
    client: &DaemonClient,
    name: &str,
    args: Map<String, Value>,
) -> Result<Value, String> {
    let root = scope(client, &args).await?;
    let payload = match name {
        "cccc_repo" | "cccc_repo_edit" => crate::repo::call(&root, action(&args), &args)?,
        "cccc_shell" => one_shot(&root, command(&args)?, timeout(&args)).await?,
        "cccc_git" => git(&root, &args).await?,
        "cccc_exec_command" => crate::local_sessions::start(&root, &args)?,
        "cccc_write_stdin" => crate::local_sessions::write(&args)?,
        "cccc_code_exec" => crate::code_mode::start(home, client, &root, &args).await?,
        "cccc_code_wait" => crate::code_mode::wait(home, client, &args).await?,
        "cccc_apply_patch" => apply_patch(&root, &args).await?,
        "cccc_file" => file(home, client, &root, &args).await?,
        _ => return Err(format!("unsupported local tool: {name}")),
    };
    Ok(tool_result(payload))
}

async fn scope(client: &DaemonClient, args: &Map<String, Value>) -> Result<PathBuf, String> {
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
    let mut command = tokio::process::Command::new(program);
    command.args(arguments).current_dir(root);
    let output = tokio::time::timeout(std::time::Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| format!("command timed out after {seconds}s"))?
        .map_err(|error| error.to_string())?;
    Ok(
        json!({"exit_code":output.status.code(),"stdout":bounded(&output.stdout),"stderr":bounded(&output.stderr)}),
    )
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

async fn apply_patch(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let patch = args
        .get("patch")
        .and_then(Value::as_str)
        .ok_or("patch is required")?;
    if patch.trim_start().starts_with("*** Begin Patch") {
        let changed = apply_codex_patch(root, patch)?;
        return Ok(json!({"applied":true,"files":changed}));
    }
    let mut child = tokio::process::Command::new("git")
        .args(["apply", "-"])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    use tokio::io::AsyncWriteExt;
    child
        .stdin
        .as_mut()
        .ok_or("git apply stdin unavailable")?
        .write_all(patch.as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(json!({"applied":true}))
}

enum PatchChange {
    Write(PathBuf, Vec<u8>),
    Delete(PathBuf),
}

fn apply_codex_patch(root: &Path, patch: &str) -> Result<Vec<String>, String> {
    let lines = patch.lines().collect::<Vec<_>>();
    if lines.first().copied() != Some("*** Begin Patch")
        || lines.last().copied() != Some("*** End Patch")
    {
        return Err(
            "Codex patch must start with *** Begin Patch and end with *** End Patch".into(),
        );
    }
    let mut index = 1;
    let mut changes = Vec::new();
    let mut names = Vec::new();
    while index + 1 < lines.len() {
        let header = lines[index];
        let (kind, raw_path) = if let Some(path) = header.strip_prefix("*** Add File: ") {
            ("add", path)
        } else if let Some(path) = header.strip_prefix("*** Update File: ") {
            ("update", path)
        } else if let Some(path) = header.strip_prefix("*** Delete File: ") {
            ("delete", path)
        } else {
            return Err(format!("invalid Codex patch section: {header}"));
        };
        if raw_path.trim().is_empty() {
            return Err("patch path is required".into());
        }
        index += 1;
        let start = index;
        while index + 1 < lines.len() && !lines[index].starts_with("*** ") {
            index += 1;
        }
        let body = &lines[start..index];
        let path = crate::repo::resolve(root, raw_path, kind == "add")?;
        match kind {
            "add" => {
                if path.exists() {
                    return Err(format!("file already exists: {raw_path}"));
                }
                let mut content = body
                    .iter()
                    .map(|line| {
                        line.strip_prefix('+')
                            .ok_or_else(|| "added file lines must start with +".to_owned())
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\n");
                if !body.is_empty() {
                    content.push('\n');
                }
                changes.push(PatchChange::Write(path, content.into_bytes()));
            }
            "delete" => changes.push(PatchChange::Delete(path)),
            "update" => {
                let current = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
                let updated = apply_hunks(current, body)?;
                changes.push(PatchChange::Write(path, updated.into_bytes()));
            }
            _ => unreachable!(),
        }
        names.push(raw_path.to_owned());
    }
    for change in changes {
        match change {
            PatchChange::Write(path, data) => {
                cccc_core::fs::atomic_write(&path, &data).map_err(|error| error.to_string())?
            }
            PatchChange::Delete(path) => {
                std::fs::remove_file(path).map_err(|error| error.to_string())?
            }
        }
    }
    Ok(names)
}

fn apply_hunks(mut current: String, body: &[&str]) -> Result<String, String> {
    let mut index = 0;
    while index < body.len() {
        if !body[index].starts_with("@@") {
            return Err("update patch requires @@ hunk headers".into());
        }
        index += 1;
        let start = index;
        while index < body.len() && !body[index].starts_with("@@") {
            index += 1;
        }
        let mut old = Vec::new();
        let mut new = Vec::new();
        for line in &body[start..index] {
            if line.starts_with("\\ No newline") {
                continue;
            }
            let (marker, content) = line.split_at(line.len().min(1));
            match marker {
                " " => {
                    old.push(content);
                    new.push(content);
                }
                "-" => old.push(content),
                "+" => new.push(content),
                _ => return Err("hunk lines must start with space, +, or -".into()),
            }
        }
        let old = old.join("\n");
        let new = new.join("\n");
        if old.is_empty() {
            return Err("update hunk needs context or removed lines".into());
        }
        if current.matches(&old).count() != 1 {
            return Err("patch hunk context must match exactly once".into());
        }
        current = current.replacen(&old, &new, 1);
    }
    Ok(current)
}

async fn file(
    home: &HomeLayout,
    client: &DaemonClient,
    root: &Path,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let action = action(args);
    let raw = first_non_blank(args, &["path", "rel_path"]).ok_or("path is required")?;
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
        let group_id = args
            .get("group_id")
            .and_then(Value::as_str)
            .ok_or("group_id is required")?;
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
            let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
            let blob = cccc_core::blobs::store(home, group_id, &bytes)
                .map_err(|error| error.to_string())?;
            let title = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment");
            request.insert(
                "attachments".into(),
                json!([{
                    "kind":"file",
                    "path":blob.path,
                    "title":title,
                    "mime_type":mime_guess::from_path(&path).first_or_octet_stream().to_string(),
                    "bytes":blob.bytes,
                    "sha256":blob.sha256,
                    "content_base64":base64::engine::general_purpose::STANDARD.encode(&bytes)
                }]),
            );
            let result = crate::remote_messages::try_send(home, client, request)
                .await
                .ok_or(
                    "attachments are only supported for trusted remote Group Bridge messages",
                )??;
            return Ok(json!({"sent":true,"attachment":blob,"result":result}));
        }
        request.insert("paths".into(), json!([path.to_string_lossy().into_owned()]));
        let result = daemon(client, "send_files", request).await?;
        let attachment = result["event"]["data"]["attachments"]
            .as_array()
            .and_then(|attachments| attachments.first())
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(json!({"sent":true,"attachment":attachment,"result":result}));
    }
    if action == "blob_path" || action == "info" {
        return Ok(json!({"path":path,"bytes":path.metadata().map(|meta|meta.len()).unwrap_or(0)}));
    }
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    Ok(json!({"path":path,"content":String::from_utf8_lossy(&bytes),"bytes":bytes.len()}))
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
        .unwrap_or(30)
        .clamp(1, 600)
}
fn bounded(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(2_000_000)]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::{apply_codex_patch, command, file_message_mode, timeout};
    use serde_json::json;

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
