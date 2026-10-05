//! ACP receipts are separate from the native TUI receipt. Switching surfaces never erases history.
use super::*;
use std::collections::BTreeMap;

pub(super) fn receipt_path(
    home: &HomeLayout,
    group: &str,
    actor: &str,
) -> std::io::Result<PathBuf> {
    let base = path(home, group, actor)?;
    Ok(base
        .parent()
        .expect("runtime session directory")
        .join("antigravity-acp")
        .join(base.file_name().expect("receipt name")))
}

fn identity(cwd: &Path, command: &[String], env: &BTreeMap<String, String>) -> Value {
    json!({"workspace_path":workspace_path(cwd),"command_fingerprint":command_fingerprint(command),"provider_home":env.get("GEMINI_HOME"),"version":crate::antigravity_acp_setup::VERSION})
}

pub(crate) fn prepare(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    cwd: &Path,
    command: &[String],
    env: &BTreeMap<String, String>,
) -> std::io::Result<Option<String>> {
    if !resume_enabled() {
        return Ok(None);
    }
    let path = receipt_path(home, group, actor)?;
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = cccc_core::fs::read_json(&path)?;
    if value["v"] != 1 || value["identity"] != identity(cwd, command, env) {
        return Ok(None);
    }
    let id = value["provider_session_id"]
        .as_str()
        .filter(|value| uuid::Uuid::parse_str(value).is_ok())
        .ok_or_else(|| {
            std::io::Error::other(
                "Invalid Antigravity ACP resume receipt; use New Session to retire it",
            )
        })?;
    Ok(Some(id.to_owned()))
}

pub(crate) fn record(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    cwd: &Path,
    command: &[String],
    env: &BTreeMap<String, String>,
    session: (&str, bool),
) -> std::io::Result<()> {
    if !resume_enabled() {
        return Ok(());
    }
    let (id, resumed) = session;
    cccc_core::fs::write_json(
        &receipt_path(home, group, actor)?,
        &json!({"v":1,"identity":identity(cwd,command,env),"provider_session_id":id,"resumed":resumed,"updated_at":utc_now()}),
    )
}

pub(crate) fn remove(home: &HomeLayout, group: &str, actor: &str) -> std::io::Result<()> {
    match std::fs::remove_file(receipt_path(home, group, actor)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
