//! An empty official ACP session may not exist after restart. Persist the first
//! attempted prompt before writing to the provider, even if admission is lost.
use super::*;
use cccc_contracts::ActorRuntime;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct PromptReceipt {
    path: PathBuf,
    id_field: &'static str,
    attempted_field: &'static str,
    id: String,
    attempted: AtomicBool,
}

impl PromptReceipt {
    pub(crate) fn actor(path: PathBuf, id: String, resumed: bool) -> Self {
        Self {
            path,
            id_field: "provider_session_id",
            attempted_field: "attempted",
            id,
            attempted: AtomicBool::new(resumed),
        }
    }
    pub(crate) fn voice(home: &HomeLayout, id: String, resumed: bool) -> Self {
        Self {
            path: home.daemon_dir().join("codex_voice_analyst.json"),
            id_field: "thread_id",
            attempted_field: "materialized",
            id,
            attempted: AtomicBool::new(resumed),
        }
    }
    pub(crate) fn resumable(&self) -> bool {
        self.attempted.load(Ordering::Acquire)
    }
    pub(crate) fn mark_attempted(&self) -> std::io::Result<()> {
        if self.resumable() {
            return Ok(());
        }
        let mut value: Value = cccc_core::fs::read_json(&self.path)?;
        if value[self.id_field].as_str() != Some(self.id.as_str()) {
            return Err(std::io::Error::other(
                "ACP prompt receipt belongs to a different session",
            ));
        }
        value[self.attempted_field] = json!(true);
        value["updated_at"] = json!(utc_now());
        cccc_core::fs::write_json(&self.path, &value)?;
        self.attempted.store(true, Ordering::Release);
        Ok(())
    }
}

pub(crate) fn receipt_path(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    runtime: ActorRuntime,
) -> std::io::Result<PathBuf> {
    let base = path(home, group, actor)?;
    Ok(base
        .parent()
        .expect("runtime session directory")
        .join(format!(
            "{}-acp",
            super::super::codex_voice_analyst::native_acp_name(runtime)
        ))
        .join(base.file_name().expect("receipt name")))
}

pub(crate) fn identity(cwd: &Path, command: &[String], env: &BTreeMap<String, String>) -> Value {
    // Hash launch settings, including provider storage/auth selectors, without
    // persisting private environment values or depending on model names.
    let encoded = serde_json::to_vec(&(command, env)).expect("ACP identity serializes");
    json!({"workspace_path":workspace_path(cwd),"launch_fingerprint":format!("{:x}",Sha256::digest(encoded))})
}

pub(crate) fn prepare(path: &Path, identity: &Value) -> std::io::Result<Option<String>> {
    if !resume_enabled() || !path.exists() {
        return Ok(None);
    }
    let value: Value = cccc_core::fs::read_json(path)?;
    if value["v"] != 1 || &value["identity"] != identity {
        return Ok(None);
    }
    let attempted = value["attempted"]
        .as_bool()
        .ok_or_else(|| std::io::Error::other("Invalid ACP attempt receipt; use New Session"))?;
    let id = value["provider_session_id"]
        .as_str()
        .filter(|id| super::super::codex_voice_analyst::valid_native_acp_id(id))
        .ok_or_else(|| std::io::Error::other("Invalid ACP session receipt; use New Session"))?;
    Ok(attempted.then(|| id.to_owned()))
}

pub(crate) fn record(
    path: &Path,
    identity: &Value,
    id: &str,
    resumed: bool,
) -> std::io::Result<()> {
    cccc_core::fs::write_json(
        path,
        &json!({"v":1,"identity":identity,"provider_session_id":id,"attempted":resumed,"updated_at":utc_now()}),
    )
}

pub(crate) fn retire_all(home: &HomeLayout, group: &str, actor: &str) -> std::io::Result<()> {
    for runtime in [
        ActorRuntime::Copilot,
        ActorRuntime::Devin,
        ActorRuntime::Cursor,
    ] {
        match std::fs::remove_file(receipt_path(home, group, actor, runtime)?) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
