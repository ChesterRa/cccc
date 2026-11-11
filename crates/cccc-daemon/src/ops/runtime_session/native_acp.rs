//! An empty official ACP session may not exist after restart. Persist the first
//! attempted prompt before writing to the provider, even if admission is lost.
use super::*;
use cccc_contracts::ActorRuntime;
use cccc_core::codex_voice_settings::LaunchParts;
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

/// The identity a receipt is matched by, and the parts of its fingerprint.
pub(crate) fn launch(
    cwd: &Path,
    command: &[String],
    env: &BTreeMap<String, String>,
) -> (Value, LaunchParts) {
    // Hash launch settings, including provider storage/auth selectors, without
    // persisting private environment values or depending on model names.
    let encoded = serde_json::to_vec(&(command, env)).expect("ACP identity serializes");
    (
        json!({"workspace_path":workspace_path(cwd),"launch_fingerprint":format!("{:x}",Sha256::digest(encoded))}),
        LaunchParts::split(env, |env| (command, env)),
    )
}

/// The saved session this launch loads. A launch whose `PATH` alone changed
/// still loads it: the provider starts as a new process with the new `PATH`.
pub(crate) fn prepare(
    path: &Path,
    identity: &Value,
    parts: &LaunchParts,
) -> std::io::Result<Option<String>> {
    if !resume_enabled() || !path.exists() {
        return Ok(None);
    }
    let value: Value = cccc_core::fs::read_json(path)?;
    if value["v"] != 1 || !replaced::identity_resumes(&value, identity, parts) {
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

/// `parts` goes beside `identity`, never inside it: `prepare` matches the
/// identity as a whole.
pub(crate) fn record(
    path: &Path,
    identity: &Value,
    parts: &LaunchParts,
    id: &str,
    resumed: bool,
) -> std::io::Result<()> {
    replaced::write_recorded(
        path,
        json!({"v":1,"identity":identity,"launch_parts":parts,"provider_session_id":id,"attempted":resumed,"updated_at":utc_now()}),
    )
}

pub(crate) fn retire_all(home: &HomeLayout, group: &str, actor: &str) -> std::io::Result<()> {
    for runtime in [
        ActorRuntime::Copilot,
        ActorRuntime::Devin,
        ActorRuntime::Cursor,
        ActorRuntime::Kimi,
        ActorRuntime::Auggie,
        ActorRuntime::Kiro,
        ActorRuntime::Hermes,
        ActorRuntime::Droid,
    ] {
        match std::fs::remove_file(receipt_path(home, group, actor, runtime)?) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_receipt_saved_before_fingerprints_were_split_still_resumes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("receipt.json");
        // Encoded outside this code, from the format in use before the split.
        const SAVED: &str = "fb75935a406fec78ab5eeb9f2aced8b387ecf5b9e2f0668e59093bc9628600eb";
        let identity =
            json!({"workspace_path":workspace_path(temp.path()),"launch_fingerprint":SAVED});
        // Written as it was then, not through `record`.
        cccc_core::fs::write_json(
            &path,
            &json!({"v":1,"identity":identity,"provider_session_id":"acp-saved","attempted":true,"updated_at":"2026-01-01T00:00:00Z"}),
        )
        .expect("saved receipt");
        let environment = [
            ("CCCC_CLI", "/probe/cccc"),
            ("HOME", "/probe/home"),
            ("PATH", "/probe/bin:/usr/bin"),
            ("PI_CODING_AGENT_DIR", "/probe/pi"),
            ("PROBE_SETTING", "same"),
            ("USERPROFILE", "/probe/profile"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        let (current, parts) = launch(
            temp.path(),
            &["agent".to_owned(), "--acp".to_owned()],
            &environment,
        );
        let resumes = || prepare(&path, &current, &parts).expect("prepare");
        assert_eq!(resumes().as_deref(), Some("acp-saved"));

        // The parts go beside the identity, which keeps its fields and its version.
        record(&path, &current, &parts, "acp-saved", true).expect("record");
        let stored: Value = cccc_core::fs::read_json(&path).expect("stored receipt");
        assert_eq!(stored["v"], 1);
        assert_eq!(stored["identity"], identity);
        assert_eq!(stored["launch_parts"], json!(parts));
        assert_eq!(stored["launch_parts"]["v"], 1);
        assert_eq!(resumes().as_deref(), Some("acp-saved"));
    }

    #[test]
    fn a_launch_whose_path_alone_changed_loads_the_saved_session() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("receipt.json");
        let command = ["agent".to_owned(), "--acp".to_owned()];
        let environment = |changes: &[(&str, &str)]| {
            let mut environment: BTreeMap<String, String> = [
                ("CCCC_CLI", "/probe/cccc"),
                ("HOME", "/probe/home"),
                ("PATH", "/probe/bin:/usr/bin"),
                ("PROBE_SETTING", "same"),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
            for (key, value) in changes {
                environment.insert((*key).to_owned(), (*value).to_owned());
            }
            environment
        };
        let (saved, parts) = launch(temp.path(), &command, &environment(&[]));
        record(&path, &saved, &parts, "acp-saved", true).expect("record");
        let resumes = |cwd: &Path, changes: &[(&str, &str)]| {
            let (current, parts) = launch(cwd, &command, &environment(changes));
            prepare(&path, &current, &parts).expect("prepare")
        };
        let new_path = [("PATH", "/other/bin:/usr/bin")];
        assert_eq!(resumes(temp.path(), &[]).as_deref(), Some("acp-saved"));
        assert_eq!(
            resumes(temp.path(), &new_path).as_deref(),
            Some("acp-saved")
        );
        for changes in [
            &[("CCCC_CLI", "/moved/cccc")][..],
            &[("PROBE_SETTING", "changed")],
            &[
                ("PATH", "/other/bin:/usr/bin"),
                ("PROBE_SETTING", "changed"),
            ],
        ] {
            assert_eq!(resumes(temp.path(), changes), None, "{changes:?}");
        }
        let elsewhere = tempfile::tempdir().expect("another workspace");
        assert_eq!(resumes(elsewhere.path(), &new_path), None);

        // A saved identity without its fingerprint is incomplete, whatever its split says.
        let complete: Value = cccc_core::fs::read_json(&path).expect("stored receipt");
        for fingerprint in [None, Some(json!("")), Some(json!(7))] {
            let mut incomplete = complete.clone();
            let identity = incomplete["identity"]
                .as_object_mut()
                .expect("identity object");
            match fingerprint {
                Some(value) => identity.insert("launch_fingerprint".into(), value),
                None => identity.remove("launch_fingerprint"),
            };
            cccc_core::fs::write_json(&path, &incomplete).expect("incomplete receipt");
            assert_eq!(resumes(temp.path(), &new_path), None, "{incomplete}");
        }

        // Without a split the saved receipt cannot tell which part changed.
        let mut unsplit = complete;
        unsplit
            .as_object_mut()
            .expect("receipt object")
            .remove("launch_parts");
        cccc_core::fs::write_json(&path, &unsplit).expect("unsplit receipt");
        assert_eq!(resumes(temp.path(), &new_path), None);
        assert_eq!(resumes(temp.path(), &[]).as_deref(), Some("acp-saved"));
    }
}
