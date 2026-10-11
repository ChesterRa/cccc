//! Pi RPC recovery uses the existing Actor receipt and provider-owned JSONL.
use super::*;
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read};

pub(crate) struct Resume {
    pub(crate) id: String,
    pub(crate) file: Option<PathBuf>,
    pub(crate) attempted: bool,
}

pub(crate) fn identity(cwd: &Path, command: &[String], env: &BTreeMap<String, String>) -> Value {
    let fingerprint = cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Pi,
        runtime_mode: cccc_contracts::RuntimeMode::Default,
        command: command.to_vec(),
        environment: env.clone(),
    }
    .identity_fingerprint();
    json!({"workspace_path":workspace_path(cwd),"launch_fingerprint":fingerprint})
}

pub(crate) fn receipt_path(home: &HomeLayout, group: &str, actor: &str) -> io::Result<PathBuf> {
    path(home, group, actor)
}

pub(crate) fn prepare(
    path: &Path,
    identity: &Value,
    sessions: &Path,
) -> io::Result<Option<Resume>> {
    if !resume_enabled() || !path.exists() {
        return Ok(None);
    }
    let value: Value = cccc_core::fs::read_json(path)
        .map_err(|_| io::Error::other("Pi session receipt is unreadable; use New Session"))?;
    if value["v"] != MANAGED_RECORD_VERSION
        || value["runtime"] != "pi"
        || value["transport"] != "pi_rpc"
        || &value["identity"] != identity
    {
        return Ok(None);
    }
    let id = value["provider_session_id"]
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .ok_or_else(|| {
            io::Error::other("Pi session receipt has an invalid identity; use New Session")
        })?;
    let attempted = value["attempted"]
        .as_bool()
        .ok_or_else(|| io::Error::other("Pi attempt receipt is invalid; use New Session"))?;
    let file = value["session_file"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("Pi session receipt is incomplete; use New Session"))?;
    validate_path(&file, sessions)?;
    if file.exists() {
        validate_file(&file, id)?;
    } else if attempted {
        return Err(io::Error::other(
            "The attempted Pi session is missing; use New Session",
        ));
    }
    Ok(Some(Resume {
        id: id.into(),
        file: file.exists().then_some(file),
        attempted,
    }))
}

pub(crate) fn locate(sessions: &Path, id: &str) -> io::Result<PathBuf> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(io::Error::other(
            "Pi resume identity is invalid; use New Session",
        ));
    }
    let suffix = format!("_{id}.jsonl");
    let mut found = None;
    for item in std::fs::read_dir(sessions)? {
        let path = item?.path();
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
        {
            if found.is_some() {
                return Err(io::Error::other(
                    "Pi resume identity is ambiguous; use New Session",
                ));
            }
            validate_file(&path, id)?;
            found = Some(path);
        }
    }
    found.ok_or_else(|| io::Error::other("The attempted Pi session is missing; use New Session"))
}

pub(crate) fn validate_path(file: &Path, sessions: &Path) -> io::Result<()> {
    if !file.is_absolute()
        || file.parent() != Some(sessions)
        || file.extension().is_none_or(|ext| ext != "jsonl")
    {
        return Err(io::Error::other(
            "Pi returned a session path outside its owned history",
        ));
    }
    Ok(())
}

fn validate_file(file: &Path, id: &str) -> io::Result<()> {
    let file = std::fs::File::open(file)?;
    let mut bytes = Vec::new();
    let mut reader = std::io::BufReader::new(file).take(64 * 1024);
    // Only the bounded provider header is needed; history is neither rewritten nor scanned.
    reader.read_until(b'\n', &mut bytes)?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("Pi session header is invalid; use New Session"))?;
    if value["type"] != "session" || value["id"] != id {
        return Err(io::Error::other(
            "Pi session header changed identity; use New Session",
        ));
    }
    Ok(())
}

pub(crate) fn record(
    path: &Path,
    identity: &Value,
    id: &str,
    file: &Path,
    attempted: bool,
) -> io::Result<()> {
    cccc_core::fs::write_json(
        path,
        &json!({"v":MANAGED_RECORD_VERSION,"kind":"runtime_session","runtime":"pi","transport":"pi_rpc","identity":identity,"workspace_path":identity["workspace_path"],"provider_session_id":id,"provider_thread_id":id,"session_file":file,"attempted":attempted,"status":"usable","resume_eligible":true,"updated_at":utc_now()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_recovery_identity_fences_inherited_provider_homes() {
        const PROBE_ROOT: &str = "CCCC_TEST_PI_IDENTITY_ROOT";
        if let Some(root) = std::env::var_os(PROBE_ROOT).map(PathBuf::from) {
            let environment: BTreeMap<String, String> =
                cccc_core::fs::read_json(&root.join("environment.json"))
                    .expect("isolated Actor environment");
            let command = vec!["pi".into()];
            let current = identity(&root.join("workspace"), &command, &environment);
            let voice = cccc_core::codex_voice_settings::ResolvedAgentRuntime {
                runtime: cccc_contracts::ActorRuntime::Pi,
                runtime_mode: cccc_contracts::RuntimeMode::Default,
                command,
                environment,
            }
            .identity_fingerprint();
            let resumed = prepare(&root.join("receipt.json"), &current, &root.join("sessions"))
                .expect("prepare under inherited provider environment")
                .map(|session| session.id);
            cccc_core::fs::write_json(
                &root.join("result.json"),
                &json!({"identity":current,"voice_fingerprint":voice,"resumed_session_id":resumed}),
            )
            .expect("isolated identity result");
            return;
        }

        let temp = tempfile::tempdir().expect("isolated provider roots");
        let executable = std::env::current_exe().expect("test executable");
        let probe = |root: &Path,
                     inherited: &BTreeMap<String, String>,
                     explicit: &BTreeMap<String, String>| {
            std::fs::create_dir_all(root.join("workspace")).expect("isolated workspace");
            std::fs::create_dir_all(root.join("sessions")).expect("isolated history");
            cccc_core::fs::write_json(&root.join("environment.json"), explicit)
                .expect("isolated launch environment");
            let output = std::process::Command::new(&executable)
                .args([
                    "--exact",
                    "ops::runtime_session::pi::tests::pi_recovery_identity_fences_inherited_provider_homes",
                    "--nocapture",
                ])
                .env(PROBE_ROOT, root)
                .env("CCCC_RUNTIME_RESUME", "1")
                .env_remove("PI_CODING_AGENT_DIR")
                .env_remove("HOME")
                .env_remove("USERPROFILE")
                .envs(inherited)
                .output()
                .expect("isolated inherited-environment probe");
            assert!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            cccc_core::fs::read_json::<Value>(&root.join("result.json"))
                .expect("read isolated identity result")
        };
        for selector in ["PI_CODING_AGENT_DIR", "HOME", "USERPROFILE"] {
            let mut inherited = BTreeMap::from([
                (
                    "HOME".into(),
                    temp.path().join("home-a").to_string_lossy().into_owned(),
                ),
                (
                    "USERPROFILE".into(),
                    temp.path().join("profile-a").to_string_lossy().into_owned(),
                ),
            ]);
            inherited.insert(
                selector.into(),
                temp.path()
                    .join("provider-a")
                    .to_string_lossy()
                    .into_owned(),
            );
            let mut changed = inherited.clone();
            changed.insert(
                selector.into(),
                temp.path()
                    .join("provider-b")
                    .to_string_lossy()
                    .into_owned(),
            );
            for explicit_override in [false, true] {
                let root = temp.path().join(format!("{selector}-{explicit_override}"));
                let explicit = if explicit_override {
                    BTreeMap::from([(selector.into(), inherited[selector].clone())])
                } else {
                    BTreeMap::new()
                };
                let before = probe(&root, &inherited, &explicit);
                let id = uuid::Uuid::new_v4().to_string();
                let file = root.join("sessions").join("session.jsonl");
                std::fs::write(&file, format!("{}\n", json!({"type":"session","id":id})))
                    .expect("provider-owned session header");
                let receipt = root.join("receipt.json");
                record(&receipt, &before["identity"], &id, &file, true)
                    .expect("persist valid Actor receipt");
                let receipt_bytes = std::fs::read(&receipt).expect("original receipt");
                let history_bytes = std::fs::read(&file).expect("original history");
                assert_eq!(
                    probe(&root, &inherited, &explicit)["resumed_session_id"],
                    json!(id),
                    "matching inherited identity must retain its session"
                );
                let after = probe(&root, &changed, &explicit);
                if explicit_override {
                    assert_eq!(
                        after["resumed_session_id"],
                        json!(id),
                        "explicit {selector} must override the inherited change"
                    );
                } else {
                    assert!(
                        after["resumed_session_id"].is_null(),
                        "a changed inherited {selector} must not resume another provider identity"
                    );
                }
                assert_eq!(
                    before["identity"]["launch_fingerprint"],
                    before["voice_fingerprint"]
                );
                assert_eq!(
                    std::fs::read(&receipt).expect("preserved receipt"),
                    receipt_bytes
                );
                assert_eq!(
                    std::fs::read(&file).expect("preserved history"),
                    history_bytes
                );
            }
        }
    }

    #[test]
    fn pi_recovery_never_replaces_attempted_missing_or_changed_history() {
        let tmp = tempfile::tempdir().expect("tmp");
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir(&sessions).expect("create isolated Pi session history");
        let path = tmp.path().join("receipt.json");
        let file = sessions.join("session.jsonl");
        let id = uuid::Uuid::new_v4().to_string();
        let identity = json!({"workspace_path":"fixture"});
        record(&path, &identity, &id, &file, false).expect("record never-attempted Pi receipt");
        let empty = prepare(&path, &identity, &sessions)
            .expect("prepare never-attempted Pi receipt")
            .expect("never-attempted Pi receipt retains its identity");
        assert_eq!(empty.id, id);
        assert!(!empty.attempted);
        assert!(empty.file.is_none());
        record(&path, &identity, &id, &file, true).expect("record attempted Pi receipt");
        assert!(prepare(&path, &identity, &sessions).is_err());
        std::fs::write(
            &file,
            format!(
                "{}\n",
                json!({"type":"session","id":uuid::Uuid::new_v4().to_string()})
            ),
        )
        .expect("write mismatched session header fixture");
        let original = std::fs::read(&file).expect("read original mismatched history");
        assert!(prepare(&path, &identity, &sessions).is_err());
        assert_eq!(
            std::fs::read(&file).expect("read unchanged rejected history"),
            original
        );
        std::fs::write(&file, format!("{}\n", json!({"type":"session","id":id})))
            .expect("write matching session header fixture");
        assert_eq!(
            prepare(&path, &identity, &sessions)
                .expect("prepare matching persisted Pi history")
                .expect("matching Pi receipt is resumable")
                .file,
            Some(file)
        );
    }
}
