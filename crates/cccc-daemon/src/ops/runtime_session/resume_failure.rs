use super::{read, string, write};
use cccc_contracts::{ActorRuntime, utc_now};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::io;

pub struct ResumeAttempt {
    pub session_id: String,
    pub attempt_id: String,
}

#[derive(Debug)]
pub struct ResumeBlocked(String);

impl std::fmt::Display for ResumeBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} Automatic resume is paused; retry with Start/Restart or choose New Session.",
            self.0
        )
    }
}

impl std::error::Error for ResumeBlocked {}

pub fn is_blocked(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<ResumeBlocked>())
}

/// Called after the provider-specific launch identity has been validated.
pub fn begin(
    document: &mut Map<String, Value>,
    session_id: String,
) -> io::Result<Option<ResumeAttempt>> {
    if string(document, "status") == "resume_failed" {
        return Err(io::Error::other(ResumeBlocked(string(
            document,
            "last_resume_error",
        ))));
    }
    if string(document, "status") != "usable"
        || document.get("resume_eligible").and_then(Value::as_bool) != Some(true)
    {
        return Ok(None);
    }
    let attempt_id = uuid::Uuid::new_v4().simple().to_string();
    document.insert("last_resume_attempt_at".into(), json!(utc_now()));
    document.insert("last_resume_attempt_id".into(), json!(attempt_id));
    document.insert("updated_at".into(), json!(utc_now()));
    Ok(Some(ResumeAttempt {
        session_id,
        attempt_id,
    }))
}

fn supported(document: &Map<String, Value>) -> bool {
    matches!(
        (
            string(document, "runtime").as_str(),
            string(document, "transport").as_str()
        ),
        ("codex", "codex_app_server")
            | ("grok", "grok_acp_leader")
            | ("opencode", "opencode_acp_attach")
            | ("kilo", "kilo_acp_attach")
    )
}

/// Record only a safe host diagnostic; provider errors can contain private launch values.
pub fn failed(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    attempt: &ResumeAttempt,
) -> io::Result<io::Error> {
    let mut document = read(home, group, actor)?;
    let id = if string(&document, "runtime") == "codex" {
        string(&document, "provider_thread_id")
    } else {
        string(&document, "provider_session_id")
    };
    let diagnostic = "The saved Runtime conversation could not be resumed. Its identity and history have been retained.";
    if supported(&document)
        && id == attempt.session_id
        && string(&document, "last_resume_attempt_id") == attempt.attempt_id
    {
        let failures = document
            .get("failure_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        document.insert("status".into(), json!("resume_failed"));
        document.insert("resume_eligible".into(), json!(false));
        document.insert("last_resume_error".into(), json!(diagnostic));
        document.insert("failure_count".into(), json!(failures.saturating_add(1)));
        document.insert("last_resume_attempt_id".into(), json!(""));
        document.insert("updated_at".into(), json!(utc_now()));
        write(home, group, actor, &document)?;
    }
    Ok(io::Error::other(ResumeBlocked(diagnostic.into())))
}

pub fn finish_launch<T>(
    home: &HomeLayout,
    actor: Option<(&str, &str)>,
    attempt: Option<&ResumeAttempt>,
    result: io::Result<T>,
) -> io::Result<T> {
    match (result, actor, attempt) {
        (Err(error), Some((group, actor)), Some(attempt)) => {
            let diagnostic = failed(home, group, actor, attempt).unwrap_or_else(|_| {
                io::Error::other("Saved conversation startup failed and its recovery state could not be recorded. The saved identity has not been replaced.")
            });
            // A failed teardown still owns a process. Preserve that type and
            // ownership while replacing private provider startup details.
            if crate::ops::codex_voice_analyst::PendingManagedStartup::from_error(&error).is_some()
            {
                Err(
                    crate::ops::codex_voice_analyst::PendingManagedStartup::with_startup_diagnostic(
                        error, diagnostic,
                    ),
                )
            } else {
                Err(diagnostic)
            }
        }
        (result, _, _) => result,
    }
}

/// Explicit lifecycle actions may retry the retained conversation; polling and auto-wake may not.
pub fn retry(
    home: &HomeLayout,
    group: &str,
    actor: &str,
    runtime: ActorRuntime,
) -> io::Result<bool> {
    let expected_runtime = match runtime {
        ActorRuntime::Codex => "codex",
        ActorRuntime::Grok => "grok",
        ActorRuntime::Opencode => "opencode",
        ActorRuntime::Kilo => "kilo",
        _ => return Ok(false),
    };
    let mut document = match read(home, group, actor) {
        Ok(document) => document,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if string(&document, "runtime") != expected_runtime
        || !supported(&document)
        || string(&document, "status") != "resume_failed"
    {
        return Ok(false);
    }
    document.insert("status".into(), json!("usable"));
    document.insert("resume_eligible".into(), json!(true));
    document.insert("updated_at".into(), json!(utc_now()));
    write(home, group, actor, &document)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::runtime_session as receipt;
    use cccc_contracts::ActorRuntime;
    use receipt::{grok, opencode};
    use std::{collections::BTreeMap, path::Path};

    fn record(runtime: ActorRuntime, home: &HomeLayout, cwd: &Path, id: &str) {
        let command = cccc_runtime::default_command(runtime);
        match runtime {
            ActorRuntime::Codex => receipt::record_codex_app_thread(
                home,
                "g_resume",
                "peer",
                cwd,
                &command,
                &BTreeMap::new(),
                receipt::CodexAppThread { id, resumed: true },
            ),
            ActorRuntime::Grok => grok::record_managed(
                home,
                "g_resume",
                "peer",
                cwd,
                &command,
                &BTreeMap::new(),
                id,
                true,
            ),
            _ => opencode::record_managed(
                runtime,
                home,
                "g_resume",
                "peer",
                cwd,
                &command,
                &BTreeMap::new(),
                id,
                true,
            ),
        }
        .expect("record conversation");
    }

    fn prepare(
        runtime: ActorRuntime,
        home: &HomeLayout,
        cwd: &Path,
    ) -> io::Result<Option<ResumeAttempt>> {
        let command = cccc_runtime::default_command(runtime);
        match runtime {
            ActorRuntime::Codex => receipt::prepare_codex_app_thread(
                home,
                "g_resume",
                "peer",
                cwd,
                &command,
                &BTreeMap::new(),
                "",
            ),
            ActorRuntime::Grok => {
                grok::prepare_managed(home, "g_resume", "peer", cwd, &command, &BTreeMap::new())
            }
            _ => opencode::prepare_managed(
                runtime,
                home,
                "g_resume",
                "peer",
                cwd,
                &command,
                &BTreeMap::new(),
            ),
        }
    }

    #[test]
    fn failed_saved_conversations_pause_auto_wake_and_retry_the_original_identity() {
        for runtime in [
            ActorRuntime::Codex,
            ActorRuntime::Grok,
            ActorRuntime::Opencode,
            ActorRuntime::Kilo,
        ] {
            let temp = tempfile::tempdir().expect("isolated home");
            let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
            home.initialize().expect("initialize");
            let id = if runtime == ActorRuntime::Grok {
                "01111111-2222-3333-4444-555555555555"
            } else {
                "ses_saved"
            };
            record(runtime, &home, temp.path(), id);
            let attempt = prepare(runtime, &home, temp.path())
                .expect("lookup")
                .expect("attempt");
            let result: io::Result<()> = finish_launch(
                &home,
                Some(("g_resume", "peer")),
                Some(&attempt),
                Err(io::Error::other("synthetic-private-value")),
            );
            let error = result.expect_err("failed resume");
            assert!(is_blocked(&error));
            assert!(!error.to_string().contains("synthetic-private-value"));
            let stored = read(&home, "g_resume", "peer").expect("receipt");
            assert_eq!(stored["status"], "resume_failed");
            assert_eq!(stored["resume_eligible"], false);
            assert_eq!(stored["failure_count"], 1);
            assert!(
                !serde_json::to_string(&stored)
                    .expect("JSON")
                    .contains("synthetic-private-value")
            );
            let blocked = prepare(runtime, &home, temp.path())
                .err()
                .expect("automatic resume paused");
            assert!(is_blocked(&blocked));
            assert!(retry(&home, "g_resume", "peer", runtime).expect("explicit retry"));
            let retried = prepare(runtime, &home, temp.path())
                .expect("retry lookup")
                .expect("same conversation");
            assert_eq!(retried.session_id, id);
            assert_ne!(retried.attempt_id, attempt.attempt_id);
            record(runtime, &home, temp.path(), id);
            let succeeded = read(&home, "g_resume", "peer").expect("successful receipt");
            assert_eq!(succeeded["last_resume_error"], "");
            assert_eq!(succeeded["failure_count"], 0);
        }
    }

    #[test]
    fn explicit_retry_preserves_failed_receipts_for_a_different_selected_runtime() {
        for stored_runtime in [
            ActorRuntime::Codex,
            ActorRuntime::Grok,
            ActorRuntime::Opencode,
            ActorRuntime::Kilo,
        ] {
            let temp = tempfile::tempdir().expect("isolated receipt home");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let store = cccc_core::GroupStore::new(home.clone()).expect("store");
            let mut group = store.create("changed Runtime", "").expect("group");
            let mut actor = cccc_contracts::Actor::new("peer");
            actor.runtime = if stored_runtime == ActorRuntime::Grok {
                ActorRuntime::Codex
            } else {
                ActorRuntime::Grok
            };
            group.actors.push(actor);
            store.save(&group).expect("save selected Runtime");
            let path = receipt::path(&home, &group.group_id, "peer").expect("receipt path");
            let transport = match stored_runtime {
                ActorRuntime::Codex => "codex_app_server",
                ActorRuntime::Grok => "grok_acp_leader",
                ActorRuntime::Opencode => "opencode_acp_attach",
                ActorRuntime::Kilo => "kilo_acp_attach",
                _ => unreachable!("native test runtimes"),
            };
            cccc_core::fs::write_json(
                &path,
                &json!({"runtime":stored_runtime,"transport":transport,
                    "status":"resume_failed","resume_eligible":false}),
            )
            .expect("retained failed receipt");
            let bytes = std::fs::read(&path).expect("original receipt bytes");
            assert!(
                !retry(&home, &group.group_id, "peer", group.actors[0].runtime)
                    .expect("explicit selected Runtime retry"),
                "a selected Runtime must not reopen another Runtime's failed conversation"
            );
            assert_eq!(std::fs::read(&path).expect("retained receipt"), bytes);
        }
    }

    #[test]
    fn unrelated_native_receipt_is_not_read_for_pi_or_acp_retry() {
        for runtime in [ActorRuntime::Pi, ActorRuntime::Kiro] {
            let temp = tempfile::tempdir().expect("isolated receipt home");
            let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
            let store = cccc_core::GroupStore::new(home.clone()).expect("store");
            let mut group = store
                .create("independent controlled receipt", "")
                .expect("group");
            let mut actor = cccc_contracts::Actor::new("peer");
            actor.runtime = runtime;
            if runtime == ActorRuntime::Kiro {
                actor.runtime_mode = cccc_contracts::RuntimeMode::Acp;
            }
            group.actors.push(actor);
            store.save(&group).expect("save selected Runtime");
            let path = receipt::path(&home, &group.group_id, "peer").expect("native receipt path");
            std::fs::create_dir_all(path.parent().expect("receipt directory"))
                .expect("create receipt directory");
            let bytes = b"retained unrelated malformed native receipt";
            std::fs::write(&path, bytes).expect("retained native history metadata");
            assert!(
                !retry(&home, &group.group_id, "peer", runtime)
                    .expect("native receipt is unrelated"),
                "controlled-only retry must leave another receipt family untouched"
            );
            assert_eq!(std::fs::read(&path).expect("retained receipt"), bytes);
        }
    }

    #[test]
    fn late_failure_cannot_pause_a_new_attempt_or_success_for_the_same_conversation() {
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
        home.initialize().expect("initialize");
        let runtime = ActorRuntime::Opencode;
        record(runtime, &home, temp.path(), "ses_saved");
        let old = prepare(runtime, &home, temp.path())
            .expect("lookup")
            .expect("old");
        let current = prepare(runtime, &home, temp.path())
            .expect("lookup")
            .expect("current");
        failed(&home, "g_resume", "peer", &old).expect("late failure");
        let stored = read(&home, "g_resume", "peer").expect("receipt");
        assert_eq!(stored["status"], "usable");
        assert_eq!(stored["last_resume_attempt_id"], current.attempt_id);
        record(runtime, &home, temp.path(), "ses_saved");
        failed(&home, "g_resume", "peer", &current).expect("late failure after success");
        assert_eq!(
            read(&home, "g_resume", "peer").expect("receipt")["status"],
            "usable"
        );
    }
}
