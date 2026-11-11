//! Pi RPC recovery uses the existing Actor receipt and provider-owned JSONL.
use super::*;
use cccc_core::codex_voice_settings::LaunchParts;
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read};

pub(crate) struct Resume {
    pub(crate) id: String,
    pub(crate) file: Option<PathBuf>,
    pub(crate) attempted: bool,
}

/// The identity a receipt is matched by, and the parts of its fingerprint
/// taken from the same reading of the provider environment.
pub(crate) fn launch(
    cwd: &Path,
    command: &[String],
    env: &BTreeMap<String, String>,
) -> io::Result<(Value, Option<LaunchParts>)> {
    let (fingerprint, parts) = cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: cccc_contracts::ActorRuntime::Pi,
        runtime_mode: cccc_contracts::RuntimeMode::Default,
        command: command.to_vec(),
        environment: env.clone(),
    }
    .launch_identity_at(cwd)?;
    Ok((
        json!({"workspace_path":workspace_path(cwd),"launch_fingerprint":fingerprint}),
        parts,
    ))
}

pub(crate) fn receipt_path(home: &HomeLayout, group: &str, actor: &str) -> io::Result<PathBuf> {
    path(home, group, actor)
}

/// The saved conversation this launch resumes. A launch whose `PATH` alone
/// changed still resumes it: Pi starts as a new process with the new `PATH`.
pub(crate) fn prepare(
    path: &Path,
    identity: &Value,
    parts: Option<&LaunchParts>,
    sessions: &Path,
) -> io::Result<Option<Resume>> {
    if !resume_enabled() || !path.exists() {
        return Ok(None);
    }
    let value: Value = cccc_core::fs::read_json(path)
        .map_err(|_| io::Error::other("Pi session receipt is unreadable; use New Session"))?;
    let resumes = match parts {
        Some(parts) => replaced::identity_resumes(&value, identity, parts),
        None => &value["identity"] == identity,
    };
    if value["v"] != MANAGED_RECORD_VERSION
        || value["runtime"] != "pi"
        || value["transport"] != "pi_rpc"
        || !resumes
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

/// `parts` goes beside `identity`, never inside it: `prepare` matches the
/// identity as a whole.
pub(crate) fn record(
    path: &Path,
    identity: &Value,
    parts: Option<&LaunchParts>,
    id: &str,
    file: &Path,
    attempted: bool,
) -> io::Result<()> {
    replaced::write_recorded(
        path,
        json!({"v":MANAGED_RECORD_VERSION,"kind":"runtime_session","runtime":"pi","transport":"pi_rpc","identity":identity,"launch_parts":parts,"workspace_path":identity["workspace_path"],"provider_session_id":id,"provider_thread_id":id,"session_file":file,"attempted":attempted,"status":"usable","resume_eligible":true,"updated_at":utc_now()}),
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
            let (current, parts) = launch(&root.join("workspace"), &command, &environment)
                .expect("Pi launch identity");
            let voice = cccc_core::codex_voice_settings::ResolvedAgentRuntime {
                runtime: cccc_contracts::ActorRuntime::Pi,
                runtime_mode: cccc_contracts::RuntimeMode::Default,
                command,
                environment,
            }
            .identity_fingerprint();
            let resumed = prepare(
                &root.join("receipt.json"),
                &current,
                parts.as_ref(),
                &root.join("sessions"),
            )
            .expect("prepare under inherited provider environment")
            .map(|session| session.id);
            cccc_core::fs::write_json(
                &root.join("result.json"),
                &json!({"identity":current,"parts":parts,"voice_fingerprint":voice,"resumed_session_id":resumed}),
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
                record(&receipt, &before["identity"], None, &id, &file, true)
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
                    // No launch value holds the inherited selector: of the parts,
                    // only the rest can show that it changed.
                    assert!(before["parts"]["rest"].is_string());
                    assert_ne!(before["parts"]["rest"], after["parts"]["rest"]);
                    for part in ["path", "cccc_cli"] {
                        assert_eq!(before["parts"][part], after["parts"][part]);
                    }
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
    fn a_receipt_saved_before_fingerprints_were_split_still_resumes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sessions = temp.path().join("sessions");
        std::fs::create_dir(&sessions).expect("provider history");
        let id = uuid::Uuid::new_v4().to_string();
        let file = sessions.join("session.jsonl");
        std::fs::write(&file, format!("{}\n", json!({"type":"session","id":id})))
            .expect("provider-owned session header");
        let path = temp.path().join("receipt.json");
        // Encoded outside this code, from the format in use before the split.
        const SAVED: &str = "f255f339b404ec154e0f739af6014190388d4439a7c101e9a614f43f48c4c0dc";
        let identity =
            json!({"workspace_path":workspace_path(temp.path()),"launch_fingerprint":SAVED});
        // Written as it was then, not through `record`.
        cccc_core::fs::write_json(
            &path,
            &json!({"v":2,"kind":"runtime_session","runtime":"pi","transport":"pi_rpc","identity":identity,"workspace_path":identity["workspace_path"],"provider_session_id":id,"provider_thread_id":id,"session_file":file,"attempted":true,"status":"usable","resume_eligible":true,"updated_at":"2026-01-01T00:00:00Z"}),
        )
        .expect("saved receipt");
        // Every provider selector is set, so the process environment is not read.
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
        let (current, parts) =
            launch(temp.path(), &["pi".to_owned()], &environment).expect("Pi launch identity");
        let resumes = || {
            prepare(&path, &current, parts.as_ref(), &sessions)
                .expect("prepare")
                .map(|session| session.id)
        };
        assert_eq!(resumes().as_ref(), Some(&id));

        // The parts go beside the identity, which keeps its fields and its version.
        assert!(parts.is_some());
        record(&path, &current, parts.as_ref(), &id, &file, true).expect("record");
        let stored: Value = cccc_core::fs::read_json(&path).expect("stored receipt");
        assert_eq!(stored["v"], 2);
        assert_eq!(stored["identity"], identity);
        assert_eq!(stored["launch_parts"], json!(parts));
        assert_eq!(stored["launch_parts"]["v"], 1);
        assert_eq!(resumes().as_ref(), Some(&id));
    }

    #[test]
    fn a_launch_whose_path_alone_changed_resumes_the_saved_session() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sessions = temp.path().join("sessions");
        std::fs::create_dir(&sessions).expect("provider history");
        let id = uuid::Uuid::new_v4().to_string();
        let file = sessions.join("session.jsonl");
        std::fs::write(&file, format!("{}\n", json!({"type":"session","id":id})))
            .expect("provider-owned session header");
        let path = temp.path().join("receipt.json");
        let command = ["pi".to_owned()];
        // Every provider selector is set, so the process environment is not read.
        let environment = |changes: &[(&str, &str)]| {
            let mut environment: BTreeMap<String, String> = [
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
            for (key, value) in changes {
                environment.insert((*key).to_owned(), (*value).to_owned());
            }
            environment
        };
        let (saved, parts) =
            launch(temp.path(), &command, &environment(&[])).expect("Pi launch identity");
        record(&path, &saved, parts.as_ref(), &id, &file, true).expect("record");
        let resumes = |cwd: &Path, changes: &[(&str, &str)]| {
            let (current, parts) =
                launch(cwd, &command, &environment(changes)).expect("Pi launch identity");
            prepare(&path, &current, parts.as_ref(), &sessions)
                .expect("prepare")
                .map(|session| session.id)
        };
        let new_path = [("PATH", "/other/bin:/usr/bin")];
        assert_eq!(resumes(temp.path(), &[]).as_ref(), Some(&id));
        assert_eq!(resumes(temp.path(), &new_path).as_ref(), Some(&id));
        for changes in [
            &[("CCCC_CLI", "/moved/cccc")][..],
            &[("PI_CODING_AGENT_DIR", "/other/pi")],
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
        assert_eq!(resumes(temp.path(), &[]).as_ref(), Some(&id));
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
        record(&path, &identity, None, &id, &file, false)
            .expect("record never-attempted Pi receipt");
        let empty = prepare(&path, &identity, None, &sessions)
            .expect("prepare never-attempted Pi receipt")
            .expect("never-attempted Pi receipt retains its identity");
        assert_eq!(empty.id, id);
        assert!(!empty.attempted);
        assert!(empty.file.is_none());
        record(&path, &identity, None, &id, &file, true).expect("record attempted Pi receipt");
        assert!(prepare(&path, &identity, None, &sessions).is_err());
        std::fs::write(
            &file,
            format!(
                "{}\n",
                json!({"type":"session","id":uuid::Uuid::new_v4().to_string()})
            ),
        )
        .expect("write mismatched session header fixture");
        let original = std::fs::read(&file).expect("read original mismatched history");
        assert!(prepare(&path, &identity, None, &sessions).is_err());
        assert_eq!(
            std::fs::read(&file).expect("read unchanged rejected history"),
            original
        );
        std::fs::write(&file, format!("{}\n", json!({"type":"session","id":id})))
            .expect("write matching session header fixture");
        assert_eq!(
            prepare(&path, &identity, None, &sessions)
                .expect("prepare matching persisted Pi history")
                .expect("matching Pi receipt is resumable")
                .file,
            Some(file)
        );
    }
}
