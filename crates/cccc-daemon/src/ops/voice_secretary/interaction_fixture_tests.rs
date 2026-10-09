use super::structured_fixture_tests::wait_until;
use super::*;
use cccc_client::DaemonClient;
use cccc_contracts::RuntimeMode;
use std::os::unix::fs::PermissionsExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn normalized_interactions_settle_tasks_and_release_the_shared_lane() {
    const CHILD: &str = "CCCC_SECRETARY_INTERACTION_FIXTURE";
    if std::env::var_os(CHILD).is_none() {
        let temp = tempfile::tempdir().expect("launcher");
        let launcher = temp.path().join("cccc");
        std::fs::write(&launcher, "#!/bin/sh\nexit 0\n").expect("launcher");
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).expect("mode");
        let output = std::process::Command::new(std::env::current_exe().expect("test"))
            .args([
                "--exact",
                "ops::voice_secretary::tests::interaction_fixture_tests::normalized_interactions_settle_tasks_and_release_the_shared_lane",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("CCCC_LAUNCHER_PATH", launcher)
            .output()
            .expect("isolated fixture");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    for (runtime, program, marker) in [
        (ActorRuntime::Copilot, "copilot", "INTERACTION_PERMISSION"),
        (ActorRuntime::Cursor, "cursor-agent", "INTERACTION_QUESTION"),
        (ActorRuntime::Cursor, "cursor-agent", "INTERACTION_PLAN"),
        (ActorRuntime::Antigravity, "agy", "INTERACTION_PERMISSION"),
        (ActorRuntime::Copilot, "copilot", "INTERACTION_DENIED"),
    ] {
        let temp = tempfile::Builder::new()
            .prefix("cccc-si-")
            .tempdir_in("/tmp")
            .expect("fixture");
        let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
        home.initialize().expect("init");
        let executable = temp.path().join(program);
        let script = include_str!("structured_fixture.py");
        std::fs::write(&executable, script).expect("provider fixture");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("mode");
        if runtime == ActorRuntime::Antigravity {
            crate::antigravity_acp_setup::install_fixture(&home, script);
        }
        let events = temp.path().join("events");
        let provider = temp.path().join("provider");
        std::fs::create_dir(&events).expect("events");
        std::fs::create_dir(&provider).expect("provider");
        let mut command = if runtime == ActorRuntime::Antigravity {
            vec!["agy".to_owned()]
        } else {
            vec![executable.to_string_lossy().into_owned()]
        };
        if marker == "INTERACTION_DENIED" {
            command.push("--allow-all".into());
        } else if marker == "INTERACTION_PLAN" {
            command.push("--force".into());
        }
        let profiles = ProfileStore::new(home.clone()).expect("profiles");
        let profile = profiles
            .upsert(
                json!({"name":"Offline interactive Secretary","runtime":runtime,
                    "runtime_mode":RuntimeMode::Acp,"command":command})
                .as_object()
                .expect("profile")
                .clone(),
                None,
            )
            .expect("profile");
        profiles
            .update_secrets(
                profile["id"].as_str().expect("id"),
                json!({"HOME":provider,"CODEX_HOME":provider.join("codex"),
                    "XDG_CONFIG_HOME":provider.join("config"),"XDG_DATA_HOME":provider.join("data"),
                    "FIXTURE_HOME":home.root(),"FIXTURE_EVENTS":events,
                    "SECRETARY_FIXTURE_RUNTIME":runtime,"FIXTURE_PRIVATE_KEY":"fixture-approval-secret"})
                .as_object().expect("env"),
                &[], false,
            )
            .expect("isolated environment");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let a = groups.create("A", "").expect("A").group_id;
        let b = groups.create("B", "").expect("B").group_id;
        request(
            &home,
            "voice_secretary_settings_update",
            json!({"settings":{"profile_id":profile["id"]}}),
        )
        .expect("configure");
        let daemon = tokio::spawn(crate::server::run(home.clone()));
        wait_until(|| crate::DaemonPaths::new(home.clone()).address.exists()).await;
        let client = DaemonClient::new(home.clone());
        let jobs = SecretaryTaskStore::new(home.clone());
        let ask = client
            .call(&DaemonRequest {
                v: 1,
                op: "assistant_voice_document_instruction".into(),
                args: json!({"group_id":a,"instruction":marker,"input_append_id":"ask"})
                    .as_object()
                    .expect("args")
                    .clone(),
            })
            .await
            .expect("ask");
        assert!(ask.ok);
        let ask_id = ask.result["secretary_task_id"].as_str().expect("id");
        wait_until(|| {
            jobs.load(ask_id)
                .is_ok_and(|t| t.phase == SecretaryTaskPhase::Running)
        })
        .await;
        let prompt = client
            .call(&DaemonRequest {
                v: 1,
                op: "assistant_voice_input_append".into(),
                args: json!({"group_id":b,"kind":"prompt_refine","request_id":"prompt",
                "input_append_id":"prompt","composer_text":"Improve this proposal"})
                .as_object()
                .expect("args")
                .clone(),
            })
            .await
            .expect("prompt");
        assert!(prompt.ok);
        let prompt_id = prompt.result["secretary_task_id"].as_str().expect("id");
        assert_eq!(
            jobs.load(prompt_id).expect("queued").phase,
            SecretaryTaskPhase::Queued
        );
        std::fs::write(events.join("release-interaction"), "").expect("release interaction");
        let resolved = tokio::time::timeout(
            Duration::from_secs(8),
            wait_until(|| {
                jobs.load(ask_id).is_ok_and(|t| {
                    t.phase == SecretaryTaskPhase::NeedsUser
                        && t.cleanup_confirmed
                        && !t.projected_at.is_empty()
                })
            }),
        )
        .await;
        assert!(
            resolved.is_ok(),
            "{runtime:?}/{marker} still occupies the lane: {:?}",
            jobs.load(ask_id)
        );
        let settled = jobs.load(ask_id).expect("settled");
        let receipt = settled.receipt.as_ref().expect("needs-user receipt");
        assert_eq!(receipt.status, SecretaryTaskPhase::NeedsUser);
        assert!(
            receipt.output["reply_text"]
                .as_str()
                .is_some_and(|text| !text.is_empty())
        );
        let persisted = serde_json::to_string(&settled).expect("task");
        assert!(!persisted.contains("fixture-approval-secret"));
        if marker == "INTERACTION_QUESTION" {
            assert!(
                receipt.output["reply_text"]
                    .as_str()
                    .expect("question")
                    .contains("Which date should I use?")
            );
            assert!(
                receipt.output["reply_text"]
                    .as_str()
                    .expect("redacted question")
                    .contains("[redacted]")
            );
        }
        let evidence: Value =
            cccc_core::fs::read_json(&events.join(ask_id)).expect("process receipt");
        assert!(
            !PathBuf::from(format!("/proc/{}", evidence["pid"])).exists(),
            "task process must retire before freeing capacity"
        );
        if events.join("interaction-response").exists() {
            let response: Value =
                cccc_core::fs::read_json(&events.join("interaction-response")).expect("reply");
            assert_eq!(
                response["allowed"], false,
                "the owner must not grant permission"
            );
        }
        wait_until(|| {
            jobs.load(prompt_id).is_ok_and(|t| {
                t.phase == SecretaryTaskPhase::Done
                    && t.cleanup_confirmed
                    && !t.projected_at.is_empty()
            })
        })
        .await;
        assert_eq!(jobs.load(prompt_id).expect("prompt").target.group_id, b);
        let state = cccc_core::assistant_state::load(&home, &b).expect("projected draft");
        assert!(
            state["prompt_draft"]["draft_text"]
                .as_str()
                .expect("draft")
                .contains("Polished")
        );
        assert!(
            jobs.successor(ask_id)
                .expect("no automatic retry")
                .is_none()
        );
        if marker == "INTERACTION_QUESTION" {
            let retried = client
                .call(&DaemonRequest {
                    v: 1,
                    op: "voice_secretary_task_retry".into(),
                    args: json!({"group_id":a,"task_id":ask_id,"followup":"Use January 10."})
                        .as_object()
                        .expect("args")
                        .clone(),
                })
                .await
                .expect("followup");
            assert!(retried.ok);
            let successor = retried.result["task"]["task_id"]
                .as_str()
                .expect("successor");
            wait_until(|| {
                jobs.load(successor)
                    .is_ok_and(|t| t.phase == SecretaryTaskPhase::Done && t.cleanup_confirmed)
            })
            .await;
            assert_eq!(
                jobs.load(successor).expect("retry target").target,
                settled.target
            );
        }
        client
            .call(&DaemonRequest {
                v: 1,
                op: "shutdown".into(),
                args: serde_json::Map::new(),
            })
            .await
            .expect("shutdown");
        tokio::time::timeout(Duration::from_secs(10), daemon)
            .await
            .expect("bounded shutdown")
            .expect("daemon task")
            .expect("daemon");
    }
}
